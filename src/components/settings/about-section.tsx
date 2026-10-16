import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import { type MouseEvent, useEffect, useRef, useState } from "react";
import { FileControlled } from "@/components/settings/config-source-section";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { useVoiceFacts } from "@/hooks/use-voice-facts";
import { logFailure, sayFailure } from "@/lib/failure";
import {
  type AutoUpdateHold,
  type AutoUpdateVm,
  accountSendLog,
  autoUpdateGet,
  autoUpdateSet,
  debugLogPath,
  debugModeGet,
  debugModeSet,
  type EgressEndpointVm,
  egressList,
  type LogUploadVm,
  logUploadGet,
  logUploadSet,
} from "@/lib/ipc/client";
import { useAccountStore } from "@/lib/stores/account";
import { useCapabilitiesStore, useIsReducedCapabilityPlatform } from "@/lib/stores/capabilities";
import { usePlatformStore } from "@/lib/stores/platform";
import { syncErrorMessage } from "@/lib/stores/sync";
import { isUpdateBusy, type UpdatePhase, updateStore, useUpdateStore } from "@/lib/stores/update";
import { useVoiceStore } from "@/lib/stores/voice";
import { updateErrorMessage } from "@/lib/update-error";

/**
 * The honest disclosure of the live network policy
 * (Story 11.2, NFR-11, UX-DR17; Epics 23 and 71). Sentence case, no exclamation marks
 * (project voice).
 *
 * Note the GitHub asset CDN: the update *check* hits github.com (listed), but the
 * update *download* is redirected to GitHub's release CDN (githubusercontent.com), so
 * the copy discloses it rather than claiming the listed hosts are exhaustive.
 *
 * The sentence names folder-sync profiles alongside accounts because the list now
 * derives from both (Story 23.7). It has to: a sync remote's row shows a bare host
 * with no scheme and no path, and a person reading "computed from your live
 * accounts" beside `github.com` would reasonably conclude the list had invented an
 * entry. Naming the second input is what makes the row legible as a disclosure.
 *
 * The log upload is said here too: it adds no server to the list (it goes to
 * the organisation account's repository, already listed), but it is the one
 * thing sent there that is on unless switched off, so "optional diagnostics
 * are off by default" alone would read as covering it.
 */
const EGRESS_HONESTY_SENTENCE =
  "These are the servers keeper connects to, computed from live accounts, folder-sync profiles, model providers and local observability consent. A folder-sync entry shows only the host, never the repository path or credentials. Optional diagnostics, product statistics and remote configuration are off by default; control them in Diagnostics & studies. With an organisation account, this device's log also goes to that account's settings repository once a day unless you switch it off below. App-update files are delivered by GitHub's release CDN (githubusercontent.com).";

/**
 * The honest copy for the signed-update control (Story 11.2, NFR-12). Explains that
 * updates are cryptographically verified before installing. Sentence case, no
 * exclamation marks.
 */
const UPDATE_HONESTY_SENTENCE =
  "Updates are downloaded from GitHub and verified against keeper's signing key before they install. If verification fails, the update is refused.";

/**
 * The honest debug-mode disclosure (Story 22.5, FR-79): names exactly what
 * lands on disk and where, and which of it leaves the device. Off by default.
 *
 * `logPath` is what Rust answers for this device (`debug_log_path`, Story
 * 65.3, AD-192) — the Mac's `~/Library/Logs/keeper/keeper.log`, the phone's
 * own container path — never a literal typed here, because the one literal
 * this sentence used to carry named the Mac's folder on every platform.
 * `null` is the answer not yet in, and names the file without a folder.
 */
export function debugModeSentence(logPath: string | null): string {
  return `Writes app logs to ${logPath ?? "keeper.log"} and a per-recording events.log beside each session's manifest. The events logs stay on this device; the app log's newest lines go to your organisation account as described below.`;
}

/**
 * What the daily log upload sends, where to, and who can read it — the switch
 * is on by default, so the line under it has to say all of it.
 */
const LOG_UPLOAD_SENTENCE =
  "Once a day, the newest 512 KB of the app log go to logs/ in your directory of the organisation account's settings repository. Before it leaves this device, your home folder is written as ~, and passwords and tokens in web addresses and sign-in headers as ***. What else the log says stays in it — your account and Matrix ids, device and microphone names, folders outside your home folder, error messages — and everyone with access to that repository can read it.";

/** Why "Send log now" is disabled: there is no directory for the log to go to. */
const SEND_LOG_NEEDS_ACCOUNT =
  "Sign in to an organisation account under Settings › Account to send this device's log.";

/**
 * The honesty lines of the desktop tier's "On this Mac" voice disclosure
 * (Epic 63, Story 63.8, AD-175, AD-178). The Mac has a voice port of its own
 * now, and the two things a person would assume from the phone's lines are not
 * true here: there is no `AVAudioSession` on macOS, so keeper cannot lower other
 * audio, and it releases the microphone while it speaks instead. Project voice:
 * sentence case, no exclamation marks, honest consequence-naming.
 *
 * Present on exactly one condition beyond the tier — `voice_availability`'s
 * answer is not `unsupported` — read from the voice store, never a capability
 * flag (AD-179). Mirrored one-to-one into the "On this Mac" list of
 * `docs/egress.md`; `about-section.test.tsx` reads that file from disk and
 * fails when the two lists differ. Edit both together or neither.
 */
export const MACOS_DISCLOSURE_LINES: ReadonlyArray<string> = [
  "Speech is recognised on this Mac itself and never sent to a server; a language whose on-device recogniser is not installed is refused, not uploaded.",
  "The microphone is open only while voice is on — after you press Talk, the hotkey or the tray item, or while it listens for your wake phrase — and the system microphone indicator shows it.",
  "Nothing is recorded or uploaded; what you said reaches only the model endpoint you configured, as text.",
  "keeper does not lower other audio while it listens or speaks, because macOS has no audio session that could; it stops listening while it talks instead.",
];

/**
 * Open the iOS docs page externally via the system opener. Prevents the default
 * in-webview navigation; an opener rejection is said, since the person clicked a
 * link that then did nothing.
 */
function openExternal(event: MouseEvent<HTMLAnchorElement>, url: string) {
  event.preventDefault();
  void openUrl(url).catch(sayFailure("about.openLink", "keeper couldn't open the link."));
}

/**
 * The honest copy for the background-update switch. Names the cadence Rust
 * chose rather than a literal, and then the two things a person needs in order
 * to predict what keeper will do to their session: it *will* restart itself,
 * and it will not do it while they are using it or while a recording runs.
 */
export function autoUpdateSentence(plan: AutoUpdateVm | undefined): string {
  const hours = plan === undefined ? 0 : Math.round(plan.checkIntervalMs / 3_600_000);
  const cadence =
    hours >= 1 ? `about every ${hours} hour${hours === 1 ? "" : "s"}` : "on a cadence";
  return `keeper looks for a new version ${cadence} and installs it in the background. If you have not restarted into it, keeper restarts itself once you have stopped using it — overnight, or after a few hours away — and never while a recording is running.`;
}

/**
 * What the waiting build is waiting for, as Rust last answered it. A "restart
 * to finish" line that stands for a day is otherwise unexplained — and the
 * recording case in particular is a refusal somebody should be able to read,
 * since it is the one that can hold for hours on a machine nobody is touching.
 */
export function selfRestartNote(hold: AutoUpdateHold | null): string {
  if (hold === "recording") {
    return "keeper will not restart while a recording is running. It restarts itself once the recording has finished and you are away from keeper.";
  }
  return "You can restart now, or leave it: keeper restarts itself once you are away from keeper, and never while a recording is running.";
}

/**
 * Settings → About section (Story 11.2, NFR-11, NFR-12, UX-DR17). Renders the live
 * egress list (loaded on open via {@link egressList}, from the same registry
 * session-restore uses) plus a two-step update control: "Check for updates" detects an
 * update via `@tauri-apps/plugin-updater` and, only after an explicit "Download and
 * install" click, downloads → verifies (against the committed pubkey) → installs →
 * relaunches via `@tauri-apps/plugin-process`. Every updater/egress failure surfaces as
 * a rendered state — never a thrown-to-console-only error, never a panic. A mounted-ref
 * guard keeps every async resolution from setting state after unmount.
 */
export function AboutSection({ open }: { open: boolean }) {
  // The in-app updater is a desktop-only capability; hide the whole software-update
  // sub-block wherever the platform lacks it (the phone tier). The egress list stays
  // ungated. `inAppUpdater` never gates the egress list — only the update controls.
  const inAppUpdater = useCapabilitiesStore((s) => s.capabilities.inAppUpdater);
  // Whether this is the capability-reduced (phone) tier.
  const reducedPlatform = useIsReducedCapabilityPlatform();
  // The phone's or tablet's "On this …" list, composed in Rust for the device
  // this is (`keeper_core::disclosure`, story 98.3 #4) and mirrored into its
  // doc there; written into the webview before the bundle runs, `null` on a desktop.
  const disclosure = usePlatformStore((s) => s.disclosure);
  // The desktop tier is the mirror hydrated and not reduced; `reducedPlatform`
  // is false before hydration too, so the Mac block needs the `hydrated` term
  // for the same reason the iPhone one does — never a flash on the wrong tier.
  const hydrated = useCapabilitiesStore((s) => s.hydrated);
  // Whether this Mac has a voice port that answers (Story 63.8, AD-179): the
  // one runtime answer every voice surface reads, never a capability flag.
  // `undefined` is a question not yet asked, and the disclosure is not drawn
  // from it; `unsupported` is every build without a port.
  useVoiceFacts(open);
  const voiceUnavailable = useVoiceStore((s) => s.unavailable);
  const macVoice =
    hydrated &&
    !reducedPlatform &&
    voiceUnavailable !== undefined &&
    voiceUnavailable?.kind !== "unsupported";
  // `undefined` = still loading; `null` = load failed; otherwise the egress list.
  const [endpoints, setEndpoints] = useState<EgressEndpointVm[] | null | undefined>(undefined);
  // The installed app version, read once per open from the bundle metadata
  // (`getVersion()` — the version in tauri.conf.json the running binary was built
  // with). `undefined` = still loading; `null` = read failed (render "unknown"
  // rather than guessing).
  const [appVersion, setAppVersion] = useState<string | null | undefined>(undefined);
  // The update flow's state, shared with the background loop
  // (`use-auto-update`): an update it installed while Settings was closed is
  // still here to be reported, and a check it started is not raced by a click.
  const update = useUpdateStore((s) => s.phase);
  // Why the self-restart is holding, as the background loop last heard it.
  const restartHold = useUpdateStore((s) => s.restartHold);
  // The background-update plan: `undefined` until Rust answers. Carries the
  // effective `enabled` (a layer file may pin it) and the cadence the sentence
  // names.
  const [autoPlan, setAutoPlan] = useState<AutoUpdateVm | undefined>(undefined);
  const autoWriteId = useRef(0);
  // Debug-mode toggle (Story 22.5): `undefined` = still loading.
  const [debugMode, setDebugMode] = useState<boolean | undefined>(undefined);
  const debugWriteId = useRef(0);
  // Where this device's app log is, as Rust answers it (Story 65.3): `null`
  // until it has, or when it could not.
  const [logPath, setLogPath] = useState<string | null>(null);
  // The daily log upload, effective (a layer file may pin it): `undefined`
  // until Rust answers.
  const [logUpload, setLogUpload] = useState<boolean | undefined>(undefined);
  // The last value Rust confirmed — what a failed write goes back to, never
  // the switch's own optimistic value — and the one write in flight.
  const logUploadConfirmed = useRef<boolean | undefined>(undefined);
  const logUploadPending = useRef(false);
  const [logUploadSaving, setLogUploadSaving] = useState(false);
  const [logUploadError, setLogUploadError] = useState<string | null>(null);
  // "Send log now": in flight, Rust's answer, or why the command failed.
  const [sendingLog, setSendingLog] = useState(false);
  const [logSent, setLogSent] = useState<LogUploadVm | null>(null);
  const [sendLogError, setSendLogError] = useState<string | null>(null);
  // The log goes to this person's directory under this device's name: with
  // neither, there is nowhere for it to go.
  const canSendLog = useAccountStore((s) => s.vm.identity !== null && s.vm.device !== null);
  // Guards every async resolution below so it never sets state after unmount.
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const setUpdateSafe = (next: UpdatePhase) => {
    if (mounted.current) {
      updateStore.getState().setPhase(next);
    }
  };

  useEffect(() => {
    if (!open) {
      return;
    }
    // Reset to the loading state on every (re)open so a stale prior list never
    // lingers while the fresh read is in flight. The update flow is reset with
    // it — except where resetting would discard something that is still true:
    // an installed build waiting for a restart, or a check/download in flight.
    setEndpoints(undefined);
    const flow = updateStore.getState();
    if (flow.phase.kind !== "installedNeedsRestart" && !isUpdateBusy(flow.phase)) {
      flow.reset();
    }
    let cancelled = false;
    void getVersion()
      .then((version) => {
        if (!cancelled) {
          setAppVersion(version);
        }
      })
      .catch((raw: unknown) => {
        logFailure("about.version")(raw);
        // No Tauri runtime / read failure: render an honest "unknown", never a guess.
        if (!cancelled) {
          setAppVersion(null);
        }
      });
    void debugModeGet()
      .then((value) => {
        if (!cancelled) {
          setDebugMode(value);
        }
      })
      .catch((raw: unknown) => {
        logFailure("about.debugModeGet")(raw);
        // A read failure renders the honest default (off) rather than a stuck spinner.
        if (!cancelled) {
          setDebugMode(false);
        }
      });
    if (inAppUpdater) {
      // Only where an in-app updater exists: the switch is inside that block,
      // so reading it on a phone would be a dead round-trip per open.
      void autoUpdateGet()
        .then((plan) => {
          if (!cancelled) {
            setAutoPlan(plan);
          }
        })
        // Unanswered: the switch stays disabled rather than claiming an
        // answer. The manual control below is unaffected.
        .catch(logFailure("about.readAutoUpdate"));
    }
    void debugLogPath()
      .then((path) => {
        if (!cancelled) {
          setLogPath(path);
        }
      })
      .catch((raw: unknown) => {
        logFailure("about.logPath")(raw);
        // Unanswered: the sentence names the file and no folder, never the
        // Mac's folder on a phone.
        if (!cancelled) {
          setLogPath(null);
        }
      });
    void logUploadGet()
      .then((enabled) => {
        if (!cancelled) {
          logUploadConfirmed.current = enabled;
          setLogUpload(enabled);
        }
      })
      // Unanswered: the switch stays disabled rather than claiming an answer.
      .catch(logFailure("about.logUploadGet"));
    let egressRead = 0;
    const refreshEgress = () => {
      const request = ++egressRead;
      void egressList()
        .then((list) => {
          if (!cancelled && request === egressRead) setEndpoints(list);
        })
        .catch((raw: unknown) => {
          logFailure("about.egress")(raw);
          if (!cancelled && request === egressRead) setEndpoints(null);
        });
    };
    refreshEgress();
    const egressTimer = setInterval(refreshEgress, 2_000);
    window.addEventListener("keeper-telemetry-consent-changed", refreshEgress);
    return () => {
      cancelled = true;
      clearInterval(egressTimer);
      window.removeEventListener("keeper-telemetry-consent-changed", refreshEgress);
    };
  }, [open, inAppUpdater]);

  // Step 1 — detect only. Never installs or relaunches; a mere check must not restart
  // the app. On an available update we stash it and surface "available" for consent.
  const onCheckForUpdates = () => {
    updateStore.getState().setPhase({ kind: "checking" });
    void check()
      .then((result) => {
        if (result === null) {
          setUpdateSafe({ kind: "upToDate" });
          return;
        }
        updateStore.getState().setPending(result);
        setUpdateSafe({ kind: "available", version: result.version });
      })
      .catch((raw: unknown) => {
        logFailure("update.check")(raw);
        setUpdateSafe({ kind: "error", message: updateErrorMessage(raw) });
      });
  };

  // Step 2 — explicit consent. Download → verify → install, then relaunch. A relaunch
  // failure is reported distinctly (the update is already on disk) rather than as a
  // generic failure that would imply the install itself failed.
  const onDownloadAndInstall = () => {
    const flow = updateStore.getState();
    const target = flow.pending;
    if (target === null) {
      return;
    }
    // Consume the pending update so a rapid double-click can't launch a second
    // concurrent downloadAndInstall() on the same handle. To retry after a
    // failure the user re-checks, which re-detects and re-arms the update.
    flow.setPending(null);
    flow.setPhase({ kind: "downloading", version: target.version });
    void target
      .downloadAndInstall()
      .then(async () => {
        try {
          await relaunch();
          // A real relaunch exits the process, so this never renders. If relaunch
          // resolves without actually restarting, never leave the flow stuck on
          // "downloading" — the update is already on disk, so ask for a restart.
          setUpdateSafe({ kind: "installedNeedsRestart", version: null, atMs: Date.now() });
        } catch (raw) {
          logFailure("update.relaunch")(raw);
          // Install succeeded but the relaunch failed; the update is on disk.
          setUpdateSafe({ kind: "installedNeedsRestart", version: null, atMs: Date.now() });
        }
      })
      .catch((raw: unknown) => {
        logFailure("update.install")(raw);
        setUpdateSafe({ kind: "error", message: updateErrorMessage(raw) });
      });
  };

  // The restart the background path asks for: the new build is already on disk,
  // so this only exchanges the running process for it. A failed relaunch says
  // so and leaves the "restart to finish" sentence standing — the install is
  // not undone by a relaunch that did not happen.
  const onRestart = () => {
    void relaunch().catch((raw: unknown) => {
      logFailure("update.restart")(raw);
      setUpdateSafe({ kind: "error", message: updateErrorMessage(raw) });
    });
  };

  const busy = isUpdateBusy(update);

  // Optimistic toggle with revert-on-failure, and the *effective* plan applied
  // when it differs: a `update.auto` pinned by a layer file makes the switch
  // visibly refuse to move rather than promising a cadence that never runs.
  const onAutoUpdateChange = (next: boolean) => {
    autoWriteId.current += 1;
    const id = autoWriteId.current;
    const prev = autoPlan;
    if (prev !== undefined) {
      setAutoPlan({ ...prev, enabled: next });
    }
    void autoUpdateSet(next)
      .then((effective) => {
        if (id === autoWriteId.current && mounted.current) {
          setAutoPlan(effective);
        }
      })
      .catch((raw: unknown) => {
        logFailure("about.autoUpdateSet")(raw);
        if (id === autoWriteId.current && mounted.current) {
          setAutoPlan(prev);
        }
      });
  };

  // Optimistic toggle with revert-on-failure (the settings-pane pattern): never
  // display a state that was not actually saved.
  const onDebugModeChange = (next: boolean) => {
    debugWriteId.current += 1;
    const id = debugWriteId.current;
    const prev = debugMode ?? false;
    setDebugMode(next);
    void debugModeSet(next).catch((raw: unknown) => {
      logFailure("about.debugModeSet")(raw);
      if (id === debugWriteId.current) {
        setDebugMode(prev);
      }
    });
  };

  // One write at a time: the switch is disabled while one is pending. Shown
  // at once; Rust's answer is the effective value (a `diagnostics.log_upload`
  // pinned by a layer file snaps back to the file's), and a failure returns
  // to the last value Rust confirmed and says why beside the switch.
  const onLogUploadChange = (next: boolean) => {
    if (logUploadPending.current) {
      return;
    }
    logUploadPending.current = true;
    setLogUploadSaving(true);
    setLogUploadError(null);
    setLogUpload(next);
    void logUploadSet(next)
      .then((effective) => {
        logUploadConfirmed.current = effective;
        if (mounted.current) setLogUpload(effective);
      })
      .catch((raw: unknown) => {
        logFailure("about.logUploadSet")(raw);
        if (mounted.current) {
          setLogUpload(logUploadConfirmed.current);
          setLogUploadError(`The switch was not saved: ${syncErrorMessage(raw)}`);
        }
      })
      .finally(() => {
        logUploadPending.current = false;
        if (mounted.current) setLogUploadSaving(false);
      });
  };

  // The answer goes inline under the button, in Rust's words — a refusal
  // included; only a command that failed outright is said here.
  const onSendLog = async () => {
    setSendingLog(true);
    setLogSent(null);
    setSendLogError(null);
    try {
      const answer = await accountSendLog();
      if (mounted.current) setLogSent(answer);
    } catch (raw) {
      logFailure("about.sendLog")(raw);
      if (mounted.current) setSendLogError(syncErrorMessage(raw));
    } finally {
      if (mounted.current) setSendingLog(false);
    }
  };

  return (
    <div className="mt-2 flex flex-col gap-2 border-border border-t pt-3 text-sm">
      <p className="font-medium">About</p>

      <div className="flex items-center justify-between gap-2">
        <span className="text-muted-foreground">Installed version</span>
        <span className="font-mono text-xs">
          {appVersion === undefined ? "…" : (appVersion ?? "unknown")}
        </span>
      </div>

      <div className="flex flex-col gap-1">
        <p className="text-muted-foreground">Network destinations</p>
        {endpoints === undefined ? (
          <p className="text-muted-foreground text-xs" role="status">
            Loading…
          </p>
        ) : endpoints === null ? (
          <p className="text-held text-xs" role="alert">
            Could not load the egress list.
          </p>
        ) : (
          <ul className="flex flex-col gap-1">
            {endpoints.map((endpoint) => (
              <li
                key={`${endpoint.kind}:${endpoint.url}`}
                className="flex items-center justify-between gap-2"
              >
                <span className="truncate font-mono text-xs" title={endpoint.url}>
                  {endpoint.url}
                </span>
                <span className="text-muted-foreground text-xs">{endpoint.label}</span>
              </li>
            ))}
          </ul>
        )}
        <p className="text-muted-foreground text-xs">{EGRESS_HONESTY_SENTENCE}</p>
      </div>

      {inAppUpdater && (
        <div className="mt-1 flex flex-col gap-1.5">
          <div className="flex items-center justify-between gap-2">
            <span>Software updates</span>
            <Button
              type="button"
              variant="outline"
              size="sm"
              disabled={busy}
              onClick={onCheckForUpdates}
            >
              {update.kind === "checking" ? "Checking…" : "Check for updates"}
            </Button>
          </div>
          {update.kind === "upToDate" && (
            <p className="text-muted-foreground text-xs" role="status">
              keeper is up to date.
            </p>
          )}
          {update.kind === "available" && (
            <div className="flex items-center justify-between gap-2">
              <p className="text-muted-foreground text-xs" role="status">
                Update {update.version} available.
              </p>
              <Button type="button" variant="outline" size="sm" onClick={onDownloadAndInstall}>
                Download and install
              </Button>
            </div>
          )}
          {update.kind === "downloading" && (
            <p className="text-muted-foreground text-xs" role="status">
              Downloading and verifying {update.version}…
            </p>
          )}
          {update.kind === "installedNeedsRestart" && (
            <div className="flex items-center justify-between gap-2">
              <p className="text-muted-foreground text-xs" role="status">
                {update.version === null
                  ? "Update installed. Restart keeper to finish."
                  : `Update ${update.version} installed. Restart keeper to finish.`}
              </p>
              <Button type="button" variant="outline" size="sm" onClick={onRestart}>
                Restart now
              </Button>
            </div>
          )}
          {update.kind === "installedNeedsRestart" && autoPlan?.enabled === true && (
            <p className="text-muted-foreground text-xs">{selfRestartNote(restartHold)}</p>
          )}
          {update.kind === "error" && (
            <p className="text-held text-xs" role="alert">
              Update failed: {update.message}
            </p>
          )}
          <p className="text-muted-foreground text-xs">{UPDATE_HONESTY_SENTENCE}</p>
          {autoPlan?.supported === true && (
            <>
              <div className="mt-0.5 flex items-center justify-between gap-2">
                <Label htmlFor="auto-update">Update automatically</Label>
                <Switch
                  id="auto-update"
                  checked={autoPlan?.enabled ?? false}
                  disabled={autoPlan === undefined}
                  onCheckedChange={onAutoUpdateChange}
                />
              </div>
              <p className="text-muted-foreground text-xs">{autoUpdateSentence(autoPlan)}</p>
            </>
          )}
        </div>
      )}

      <div className="mt-1 flex flex-col gap-1.5">
        <div className="flex items-center justify-between gap-2">
          <Label htmlFor="debug-mode">Debug mode</Label>
          <Switch
            id="debug-mode"
            checked={debugMode ?? false}
            disabled={debugMode === undefined}
            onCheckedChange={onDebugModeChange}
          />
        </div>
        <p className="text-muted-foreground text-xs">{debugModeSentence(logPath)}</p>
      </div>

      <div className="mt-1 flex flex-col gap-1.5">
        <div className="flex items-center justify-between gap-2">
          <Label htmlFor="log-upload">Send this device's log to your account daily</Label>
          <div className="flex shrink-0 items-center gap-2">
            <FileControlled settingKey="diagnostics.log_upload" />
            <Switch
              id="log-upload"
              checked={logUpload ?? false}
              disabled={logUpload === undefined || logUploadSaving}
              onCheckedChange={onLogUploadChange}
            />
          </div>
        </div>
        {logUploadError !== null && <p className="text-destructive text-xs">{logUploadError}</p>}
        <p className="text-muted-foreground text-xs">{LOG_UPLOAD_SENTENCE}</p>
        <div className="flex items-center justify-between gap-2">
          <span>Device log</span>
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={!canSendLog || sendingLog}
            onClick={() => void onSendLog()}
          >
            {sendingLog ? "Sending…" : "Send log now"}
          </Button>
        </div>
        {!canSendLog && <p className="text-muted-foreground text-xs">{SEND_LOG_NEEDS_ACCOUNT}</p>}
        {logSent !== null && (
          // Inline under the button, never a toast; Rust worded it. A refusal
          // takes the destructive colour but stays a status, not an alert.
          <p
            role="status"
            className={
              logSent.outcome === "failed"
                ? "text-destructive text-xs"
                : "text-muted-foreground text-xs"
            }
          >
            {logSent.sentence}
          </p>
        )}
        {sendLogError !== null && <p className="text-destructive text-xs">{sendLogError}</p>}
      </div>

      {macVoice && (
        <div className="mt-1 flex flex-col gap-1.5">
          <p className="font-medium">On this Mac</p>
          <ul className="flex flex-col gap-1">
            {MACOS_DISCLOSURE_LINES.map((line) => (
              <li key={line} className="text-muted-foreground text-xs">
                {line}
              </li>
            ))}
          </ul>
        </div>
      )}

      {reducedPlatform && disclosure !== null && (
        <div className="mt-1 flex flex-col gap-1.5">
          <p className="font-medium">{disclosure.heading}</p>
          <ul className="flex flex-col gap-1">
            {disclosure.lines.map((line) => (
              <li key={line} className="text-muted-foreground text-xs">
                {line}
              </li>
            ))}
          </ul>
          <a
            href={disclosure.docsUrl}
            onClick={(event) => openExternal(event, disclosure.docsUrl)}
            className="text-xs underline underline-offset-2"
          >
            {disclosure.docsLabel}
          </a>
        </div>
      )}
    </div>
  );
}
