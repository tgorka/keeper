import { getVersion } from "@tauri-apps/api/app";
import { openUrl } from "@tauri-apps/plugin-opener";
import { relaunch } from "@tauri-apps/plugin-process";
import { check, type Update } from "@tauri-apps/plugin-updater";
import { type MouseEvent, useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { useVoiceFacts } from "@/hooks/use-voice-facts";
import {
  debugLogPath,
  debugModeGet,
  debugModeSet,
  type EgressEndpointVm,
  egressList,
} from "@/lib/ipc/client";
import { useCapabilitiesStore, useIsReducedCapabilityPlatform } from "@/lib/stores/capabilities";
import { useVoiceStore } from "@/lib/stores/voice";

/**
 * The honest disclosure of what the egress list is and the no-telemetry invariant
 * (Story 11.2, NFR-11, UX-DR17; Story 23.7). Sentence case, no exclamation marks
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
 */
const EGRESS_HONESTY_SENTENCE =
  "These are the servers keeper connects to, computed from your live accounts and folder-sync profiles — nothing else. A folder-sync entry shows the remote's host only, never the repository path or any credential. keeper has no telemetry, analytics, or crash reporting. App-update files are delivered by GitHub's release CDN (githubusercontent.com).";

/**
 * The honest copy for the signed-update control (Story 11.2, NFR-12). Explains that
 * updates are cryptographically verified before installing. Sentence case, no
 * exclamation marks.
 */
const UPDATE_HONESTY_SENTENCE =
  "Updates are downloaded from GitHub and verified against keeper's signing key before they install. If verification fails, the update is refused.";

/** The docs page opened from the "On this iPhone" disclosure (Story 13.7). */
const IOS_DOCS_URL = "https://github.com/tgorka/keeper/blob/main/docs/ios.md";

/**
 * The honest debug-mode disclosure (Story 22.5, FR-79): names exactly what
 * lands on disk and where, and that it is local-only. Off by default.
 *
 * `logPath` is what Rust answers for this device (`debug_log_path`, Story
 * 65.3, AD-192) — the Mac's `~/Library/Logs/keeper/keeper.log`, the phone's
 * own container path — never a literal typed here, because the one literal
 * this sentence used to carry named the Mac's folder on every platform.
 * `null` is the answer not yet in, and names the file without a folder.
 */
export function debugModeSentence(logPath: string | null): string {
  return `Writes app logs to ${logPath ?? "keeper.log"} and a per-recording events.log beside each session's manifest. Local files only — nothing is uploaded.`;
}

/**
 * The five honesty lines of the reduced-platform (phone tier) "On this iPhone"
 * disclosure (Story 13.7; the fifth from Epic 62, Story 62.3, FR-400). Project
 * voice: sentence case, no exclamation marks, honest consequence-naming. Each
 * names a desktop-only affordance the phone lacks.
 *
 * Mirrored one-to-one into the Limitations list of `docs/ios.md`, which the
 * disclosure links to; `about-section.test.tsx` reads that file from disk and
 * fails when the two lists differ. Edit both together or neither.
 */
export const IOS_DISCLOSURE_LINES: ReadonlyArray<string> = [
  "keeper syncs and notifies only while it's open; background notifications await a future decision.",
  "No self-hosted bridge runner — manage your own bridges from your Mac.",
  "No global summon hotkey.",
  "Updates arrive by reinstalling keeper; its signature renews every 7 days.",
  "Bots talks to a model but cannot reach the folders you sync — the drive tools live on your Mac.",
  "Listening for the wake phrase starts in keeper, and then keeps working with another app in front and with the screen locked; speech is recognised on this phone and never sent to a server.",
  "Listening stops when you turn it off, when iOS ends the audio session, or when keeper is force-quit; while it listens the microphone indicator stays lit and it uses battery.",
];

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
 * Open the iOS docs page externally via the system opener, best-effort. Prevents
 * the default in-webview navigation and swallows any opener rejection — mirrors the
 * `void openUrl(url).catch(() => {})` pattern in `login-screen.tsx`.
 */
function openExternal(event: MouseEvent<HTMLAnchorElement>, url: string) {
  event.preventDefault();
  void openUrl(url).catch(() => {
    // Best-effort: nothing actionable if the system opener fails.
  });
}

/**
 * The states of the in-app update flow (Story 11.2, NFR-12). Every path — including a
 * failed check, a failed install, or a failed relaunch — resolves to one of these
 * rendered states; an error is never thrown to the console only. Installing and
 * relaunching happen only after an explicit second click (consent), so merely checking
 * never restarts the app out from under an in-progress compose.
 */
type UpdateState =
  | { kind: "idle" }
  | { kind: "checking" }
  | { kind: "upToDate" }
  | { kind: "available"; version: string }
  | { kind: "downloading"; version: string }
  | { kind: "installedNeedsRestart" }
  | { kind: "error"; message: string };

/**
 * Extract a human-readable message from an unknown thrown value (never throws). Falls
 * back to a generic line for a non-string / empty / object-valued `message` so the
 * error surface never renders "[object Object]", "undefined", or a dangling colon.
 */
function errorMessage(raw: unknown): string {
  if (typeof raw === "string" && raw.trim() !== "") {
    return raw;
  }
  if (typeof raw === "object" && raw !== null && "message" in raw) {
    const message = (raw as { message: unknown }).message;
    if (typeof message === "string" && message.trim() !== "") {
      return message;
    }
  }
  return "Something went wrong.";
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
  // Whether this is the capability-reduced (phone) tier — drives the "On this
  // iPhone" disclosure list below.
  const reducedPlatform = useIsReducedCapabilityPlatform();
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
  const [update, setUpdate] = useState<UpdateState>({ kind: "idle" });
  // Debug-mode toggle (Story 22.5): `undefined` = still loading.
  const [debugMode, setDebugMode] = useState<boolean | undefined>(undefined);
  const debugWriteId = useRef(0);
  // Where this device's app log is, as Rust answers it (Story 65.3): `null`
  // until it has, or when it could not.
  const [logPath, setLogPath] = useState<string | null>(null);
  // The detected-but-not-yet-installed update, held between the two clicks. Not state:
  // it is not rendered, only consumed by the install step.
  const pendingUpdate = useRef<Update | null>(null);
  // Guards every async resolution below so it never sets state after unmount.
  const mounted = useRef(true);
  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
    };
  }, []);
  const setUpdateSafe = (next: UpdateState) => {
    if (mounted.current) {
      setUpdate(next);
    }
  };

  useEffect(() => {
    if (!open) {
      return;
    }
    // Reset to the loading state (and a fresh update flow) on every (re)open so a
    // stale prior list never lingers while the fresh read is in flight.
    setEndpoints(undefined);
    setUpdate({ kind: "idle" });
    pendingUpdate.current = null;
    let cancelled = false;
    void getVersion()
      .then((version) => {
        if (!cancelled) {
          setAppVersion(version);
        }
      })
      .catch(() => {
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
      .catch(() => {
        // A read failure renders the honest default (off) rather than a stuck spinner.
        if (!cancelled) {
          setDebugMode(false);
        }
      });
    void debugLogPath()
      .then((path) => {
        if (!cancelled) {
          setLogPath(path);
        }
      })
      .catch(() => {
        // Unanswered: the sentence names the file and no folder, never the
        // Mac's folder on a phone.
        if (!cancelled) {
          setLogPath(null);
        }
      });
    void egressList()
      .then((list) => {
        if (!cancelled) {
          setEndpoints(list);
        }
      })
      .catch(() => {
        // A registry read failure renders an honest error line rather than an empty
        // (and therefore dishonest) list.
        if (!cancelled) {
          setEndpoints(null);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  // Step 1 — detect only. Never installs or relaunches; a mere check must not restart
  // the app. On an available update we stash it and surface "available" for consent.
  const onCheckForUpdates = () => {
    setUpdate({ kind: "checking" });
    void check()
      .then((result) => {
        if (result === null) {
          setUpdateSafe({ kind: "upToDate" });
          return;
        }
        pendingUpdate.current = result;
        setUpdateSafe({ kind: "available", version: result.version });
      })
      .catch((raw: unknown) => {
        setUpdateSafe({ kind: "error", message: errorMessage(raw) });
      });
  };

  // Step 2 — explicit consent. Download → verify → install, then relaunch. A relaunch
  // failure is reported distinctly (the update is already on disk) rather than as a
  // generic failure that would imply the install itself failed.
  const onDownloadAndInstall = () => {
    const target = pendingUpdate.current;
    if (target === null) {
      return;
    }
    // Consume the pending update so a rapid double-click can't launch a second
    // concurrent downloadAndInstall() on the same handle. To retry after a
    // failure the user re-checks, which re-detects and re-arms the update.
    pendingUpdate.current = null;
    setUpdate({ kind: "downloading", version: target.version });
    void target
      .downloadAndInstall()
      .then(async () => {
        try {
          await relaunch();
          // A real relaunch exits the process, so this never renders. If relaunch
          // resolves without actually restarting, never leave the flow stuck on
          // "downloading" — the update is already on disk, so ask for a restart.
          setUpdateSafe({ kind: "installedNeedsRestart" });
        } catch {
          // Install succeeded but the relaunch failed; the update is on disk.
          setUpdateSafe({ kind: "installedNeedsRestart" });
        }
      })
      .catch((raw: unknown) => {
        setUpdateSafe({ kind: "error", message: errorMessage(raw) });
      });
  };

  const busy = update.kind === "checking" || update.kind === "downloading";

  // Optimistic toggle with revert-on-failure (the settings-pane pattern): never
  // display a state that was not actually saved.
  const onDebugModeChange = (next: boolean) => {
    debugWriteId.current += 1;
    const id = debugWriteId.current;
    const prev = debugMode ?? false;
    setDebugMode(next);
    void debugModeSet(next).catch(() => {
      if (id === debugWriteId.current) {
        setDebugMode(prev);
      }
    });
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
            <p className="text-muted-foreground text-xs" role="status">
              Update installed. Restart keeper to finish.
            </p>
          )}
          {update.kind === "error" && (
            <p className="text-held text-xs" role="alert">
              Update failed: {update.message}
            </p>
          )}
          <p className="text-muted-foreground text-xs">{UPDATE_HONESTY_SENTENCE}</p>
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

      {reducedPlatform && (
        <div className="mt-1 flex flex-col gap-1.5">
          <p className="font-medium">On this iPhone</p>
          <ul className="flex flex-col gap-1">
            {IOS_DISCLOSURE_LINES.map((line) => (
              <li key={line} className="text-muted-foreground text-xs">
                {line}
              </li>
            ))}
          </ul>
          <a
            href={IOS_DOCS_URL}
            onClick={(event) => openExternal(event, IOS_DOCS_URL)}
            className="text-xs underline underline-offset-2"
          >
            What's different on iPhone
          </a>
        </div>
      )}
    </div>
  );
}
