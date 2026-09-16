/**
 * Background app updates: check on a cadence, install quietly, restart only
 * when nothing is lost.
 *
 * The in-app updater shipped as two clicks — *Check for updates*, then
 * *Download and install* — which is honest and is also why installs lag: the
 * control lives in Settings → About, and nobody opens Settings → About. This
 * hook is the other half, the one every app of this shape has: keeper looks for
 * its own update on a cadence, downloads and verifies it in the background, and
 * then gets itself onto the new build.
 *
 * **The restart is the hard part, and it is not this file's decision.** An
 * install that nobody restarts into is an install nobody got — a machine left
 * running for three weeks would sit on a build it downloaded on day one. So
 * once a build is installed and waiting, this asks Rust once a minute whether
 * now is a moment it may restart, passing the one fact only the webview can
 * measure: how long since somebody last interacted with keeper. Rust weighs
 * that against a live recording (an absolute refusal — a capture cut in half
 * cannot be re-recorded), a grace window so the person who just read "restart
 * to finish" wins the race, the local hour and the length of the quiet spell.
 * See `keeper_core::update::decide_restart`; nothing here decides, and the
 * *only* reason the relaunch is issued from this side is that this side owns
 * the webview it discards.
 *
 * Idleness here is time since the last interaction with **keeper**, not a
 * system-wide idle timer — a webview cannot see the rest of the desktop. The
 * thresholds are picked so that the difference does not matter: a quarter of an
 * hour in the small hours, or four hours at any other time.
 *
 * It does not decide the cadence either. Every number comes from
 * {@link autoUpdateGet} — `keeper_core::update` owns the first delay, the
 * interval, the failure backoff and the restart poll (AD-55: the webview is a
 * call site). The plan is re-read before every cycle, so flipping the switch in
 * About takes effect within one interval without a relaunch, and a
 * `update.auto` pinned by a layer file is obeyed here too.
 *
 * It does not exist where the updater does not. `inAppUpdater` is false on the
 * phone tier (app-store channels have no in-app updater), and the loop never
 * starts there — the plugin is not even linked.
 *
 * A plan that cannot be read at all leaves background updates off for this
 * launch rather than retrying a wedged settings table on a clock nobody chose;
 * the manual button in About still works, which is the honest fallback.
 */
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { useEffect } from "react";
import {
  type AutoUpdateRestartVm,
  type AutoUpdateVm,
  autoUpdateGet,
  autoUpdateRestartCheck,
} from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { isUpdateBusy, updateStore } from "@/lib/stores/update";
import { updateErrorMessage } from "@/lib/update-error";

/**
 * The interactions that count as "somebody is working on it". Deliberately
 * coarse and passive (capture-phase listeners that read nothing): a pointer, a
 * key, a scroll, or the window taking focus. A page that is merely visible is
 * not interaction — a laptop left open on the inbox overnight is idle, which is
 * exactly the machine this feature exists for.
 */
const INTERACTION_EVENTS = ["pointerdown", "keydown", "wheel", "focus"] as const;

/**
 * Run the background update loop for as long as the app is mounted on a
 * platform that has an in-app updater. Mount once, from `App`.
 */
export function useAutoUpdate() {
  const inAppUpdater = useCapabilitiesStore((s) => s.capabilities.inAppUpdater);
  useEffect(() => {
    if (!inAppUpdater) {
      return;
    }
    let stopped = false;
    let timer: ReturnType<typeof setTimeout> | undefined;
    let restartTimer: ReturnType<typeof setTimeout> | undefined;
    // The last plan Rust answered with. Kept so a cycle whose re-read fails
    // still has a cadence to back off on, rather than inventing one here.
    let plan: AutoUpdateVm | null = null;
    // When somebody last touched keeper. Mount counts as interaction: a launch
    // is somebody opening the app, and starting the clock at zero would let a
    // relaunch happen in the first minute of a session.
    let lastInteraction = Date.now();
    const noteInteraction = () => {
      lastInteraction = Date.now();
    };
    for (const event of INTERACTION_EVENTS) {
      window.addEventListener(event, noteInteraction, { capture: true, passive: true });
    }

    const later = (delayMs: number) => {
      if (stopped) {
        return;
      }
      timer = setTimeout(() => {
        void cycle();
      }, delayMs);
    };

    // One pending restart question at a time: the previous timer is always
    // replaced, so an arm from the check loop cannot double up with the
    // restart loop's own re-arm.
    const restartLater = (delayMs: number) => {
      clearTimeout(restartTimer);
      if (stopped) {
        return;
      }
      restartTimer = setTimeout(() => {
        void restartCycle();
      }, delayMs);
    };

    /**
     * Ask whether this is a moment to restart into the waiting build, and do it
     * if Rust says so. Re-reads the plan first: a switch turned off after the
     * install must stop the self-restart too, and leave the restart to a person.
     */
    const restartCycle = async () => {
      if (stopped) {
        return;
      }
      const { phase } = updateStore.getState();
      if (phase.kind !== "installedNeedsRestart") {
        // Nothing is waiting any more (About reset the flow); the check loop
        // will arm this again after the next install.
        return;
      }
      try {
        plan = await autoUpdateGet();
      } catch {
        // Keep the last known plan rather than stopping over one failed read.
      }
      if (stopped || plan === null || !plan.supported || !plan.enabled) {
        return;
      }
      const now = Date.now();
      let verdict: AutoUpdateRestartVm;
      try {
        verdict = await autoUpdateRestartCheck(now - phase.atMs, now - lastInteraction);
      } catch {
        // A wedged answer is not a reason to restart; ask again next minute.
        restartLater(plan.restartCheckIntervalMs);
        return;
      }
      updateStore.getState().setRestartHold(verdict.hold);
      if (verdict.restart) {
        try {
          // A real relaunch exits the process, so nothing after this runs. A
          // refused one changes nothing on disk: the build is still installed
          // and still waiting, so keep asking.
          await relaunch();
        } catch {
          // Reported nowhere on purpose: nobody is watching, by construction.
        }
      }
      if (!stopped && plan !== null) {
        restartLater(plan.restartCheckIntervalMs);
      }
    };

    /**
     * One check, and the install if there is something to install. Resolves
     * `true` when the round-trip succeeded (up to date, or installed), `false`
     * when it failed and the caller should back off.
     */
    const attempt = async (): Promise<boolean> => {
      const { setPhase } = updateStore.getState();
      setPhase({ kind: "checking" });
      let found: Update | null;
      try {
        found = await check();
      } catch (raw: unknown) {
        setPhase({ kind: "error", message: updateErrorMessage(raw) });
        return false;
      }
      if (found === null) {
        setPhase({ kind: "upToDate" });
        return true;
      }
      setPhase({ kind: "downloading", version: found.version });
      try {
        // Downloads, verifies against the committed minisign key, installs.
        // Nothing here relaunches — see the header.
        await found.downloadAndInstall();
      } catch (raw: unknown) {
        setPhase({ kind: "error", message: updateErrorMessage(raw) });
        return false;
      }
      setPhase({ kind: "installedNeedsRestart", version: found.version, atMs: Date.now() });
      return true;
    };

    const cycle = async () => {
      if (stopped) {
        return;
      }
      try {
        plan = await autoUpdateGet();
      } catch {
        // Keep the last known plan; a first-cycle failure is handled by the
        // caller below, which never schedules a cycle without one.
      }
      if (plan === null || stopped) {
        return;
      }
      if (!plan.supported) {
        // This platform's install cannot happen behind somebody's back (Windows
        // exits the app to run its installer), so there is no background path
        // here at all — and no cadence worth waking up on.
        return;
      }
      if (!plan.enabled) {
        // Off — but ask again on the interval, so turning it back on in About
        // starts working without a relaunch. No network happens in this branch.
        later(plan.checkIntervalMs);
        return;
      }
      const { phase } = updateStore.getState();
      if (phase.kind === "installedNeedsRestart") {
        // A build is already waiting on disk. Checking again would at best
        // re-download the same artifact and at worst install over a bundle the
        // running process is still reading from. What is left to do is get onto
        // it, which is the restart loop's question.
        restartLater(plan.restartCheckIntervalMs);
        return;
      }
      if (isUpdateBusy(phase)) {
        // Somebody is mid-check or mid-download in About. Never run a second
        // downloadAndInstall() concurrently with theirs.
        later(plan.retryDelayMs);
        return;
      }
      const ok = await attempt();
      if (plan === null) {
        return;
      }
      if (updateStore.getState().phase.kind === "installedNeedsRestart") {
        // Installed just now: stop checking and start asking whether it may
        // restart, rather than waiting out a whole interval first.
        restartLater(plan.restartCheckIntervalMs);
        return;
      }
      later(ok ? plan.checkIntervalMs : plan.retryDelayMs);
    };

    // The opening read decides whether there is a loop at all, and its delay.
    void autoUpdateGet()
      .then((first) => {
        plan = first;
        if (stopped || !first.supported) {
          return;
        }
        later(first.enabled ? first.firstCheckDelayMs : first.checkIntervalMs);
      })
      .catch(() => {
        // No plan, no background updates this launch. The About button stands.
      });

    return () => {
      stopped = true;
      clearTimeout(timer);
      clearTimeout(restartTimer);
      for (const event of INTERACTION_EVENTS) {
        window.removeEventListener(event, noteInteraction, { capture: true });
      }
    };
  }, [inAppUpdater]);
}
