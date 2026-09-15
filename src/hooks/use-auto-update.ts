/**
 * Background app updates: check on a cadence, install quietly, never relaunch.
 *
 * The in-app updater shipped as two clicks — *Check for updates*, then
 * *Download and install* — which is honest and is also why installs lag: the
 * control lives in Settings → About, and nobody opens Settings → About. This
 * hook is the other half, the one every app of this shape has: keeper looks for
 * its own update on a cadence, downloads and verifies it in the background, and
 * the person finds the new build already installed the next time they start the
 * app.
 *
 * **Three things it deliberately does not do.**
 *
 * It does not relaunch. `downloadAndInstall()` replaces the bundle on disk; the
 * running process keeps the old code until somebody restarts it. Restarting for
 * them would discard an unsent message, a half-written note or a recording in
 * progress, so the flow says "restart to finish" and waits — the manual path's
 * relaunch stays, because there a person asked for it.
 *
 * It does not decide the cadence. Every number comes from
 * {@link autoUpdateGet} — `keeper_core::update` owns the first delay, the
 * interval and the failure backoff (AD-55: the webview is a call site). The
 * plan is re-read before every cycle, so flipping the switch in About takes
 * effect within one interval without a relaunch, and a `update.auto` pinned by
 * a layer file is obeyed here too.
 *
 * It does not exist where the updater does not. `inAppUpdater` is false on the
 * phone tier (app-store channels have no in-app updater), and the loop never
 * starts there — the plugin is not even linked.
 *
 * A plan that cannot be read at all leaves background updates off for this
 * launch rather than retrying a wedged settings table on a clock nobody chose;
 * the manual button in About still works, which is the honest fallback.
 */
import { check, type Update } from "@tauri-apps/plugin-updater";
import { useEffect } from "react";
import { type AutoUpdateVm, autoUpdateGet } from "@/lib/ipc/client";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";
import { isUpdateBusy, updateStore } from "@/lib/stores/update";
import { updateErrorMessage } from "@/lib/update-error";

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
    // The last plan Rust answered with. Kept so a cycle whose re-read fails
    // still has a cadence to back off on, rather than inventing one here.
    let plan: AutoUpdateVm | null = null;

    const later = (delayMs: number) => {
      if (stopped) {
        return;
      }
      timer = setTimeout(() => {
        void cycle();
      }, delayMs);
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
      setPhase({ kind: "installedNeedsRestart", version: found.version });
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
        // running process is still reading from. Nothing more until a restart.
        return;
      }
      if (isUpdateBusy(phase)) {
        // Somebody is mid-check or mid-download in About. Never run a second
        // downloadAndInstall() concurrently with theirs.
        later(plan.retryDelayMs);
        return;
      }
      const ok = await attempt();
      if (plan !== null) {
        later(ok ? plan.checkIntervalMs : plan.retryDelayMs);
      }
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
    };
  }, [inAppUpdater]);
}
