/**
 * Folder-sync mirror store (Epic 29, Stories 29.4 + 29.5, FR-77..FR-93).
 *
 * A vanilla zustand store created at module load *outside* React (the
 * `recording-settings.ts` precedent). It mirrors what the Rust sync engine
 * reports — it is NOT the source of truth. Profiles live in the syncd config
 * and every status line is composed in Rust, so {@link SyncStatusVm.line} is
 * rendered verbatim: the tray and the window must never word the same state
 * differently.
 *
 * Statuses are polled rather than streamed on purpose (the tray has to render
 * with no webview subscribed at all), so {@link startSyncStatusPolling} owns
 * the cadence. A failed poll keeps the previous snapshot — transient IPC noise
 * must never flicker a syncing folder to "no status", the same rule
 * `use-recording-session.ts` follows mid-recording.
 *
 * Every command here rejects with `unsupported` on a machine with no usable
 * `git`, which is why the whole surface is gated on `CapabilitiesVm.sync` and
 * hidden rather than shown disabled.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import {
  type SyncOutcomeVm,
  type SyncProfileReq,
  type SyncProfileVm,
  type SyncStatusVm,
  type SyncVerifyVm,
  syncFolderNow,
  syncProfileRemove,
  syncProfileSave,
  syncProfileSetEnabled,
  syncProfiles,
  syncRescan,
  syncStatuses,
  syncVerify,
} from "@/lib/ipc/client";

/** The legal `direction` values (mirror of the Rust `parse_req` match). */
export const SYNC_DIRECTIONS = ["bidirectional", "pushOnly", "pullOnly"] as const;
export type SyncDirection = (typeof SYNC_DIRECTIONS)[number];

/** The legal `lfsMode` values (mirror of the Rust `parse_req` match). */
export const SYNC_LFS_MODES = ["materialize", "pointerOnly", "disabled"] as const;
export type SyncLfsMode = (typeof SYNC_LFS_MODES)[number];

/**
 * The lane every profile created from Settings uses. The other lane
 * (`worktree`) is an agent airlock that Rust only accepts together with
 * `direction = pushOnly`, so it stays a syncd-config affordance rather than a
 * Settings control that can be combined into a rejected profile.
 */
export const SYNC_DEFAULT_LANE = "main";

/** Default branch for a new profile (mirrors `SyncProfile::new`). */
export const SYNC_DEFAULT_BRANCH = "main";

/** Default LFS threshold in bytes — 4 MiB (mirrors `DEFAULT_LFS_THRESHOLD_BYTES`). */
export const SYNC_DEFAULT_LFS_THRESHOLD_BYTES = 4 * 1024 * 1024;

/**
 * The numbers Rust substitutes when a profile pins nothing, or pins something it
 * cannot honour (Story 34.5, AD-34-8).
 *
 * Mirrored rather than fetched because the form has to name the value that will
 * be in force *while the user is still typing* — ticking the removable box
 * changes the answer before anything is saved, and asking Rust per keystroke is
 * absurd. Rust stays the authority: these only produce placeholders and notes,
 * and `SyncProfileVm.effectiveSettleMs` / `effectivePollIntervalMs` carry the
 * real answer for a profile that exists. Keep them in step with
 * `keeper-sync/src/profile.rs`.
 */
export const SYNC_DEFAULT_SETTLE_MS = 5_000;
/** Mirrors `REMOVABLE_SETTLE_MS`: removable and network volumes report late. */
export const SYNC_REMOVABLE_SETTLE_MS = 10_000;
/** Mirrors `DEFAULT_POLL_INTERVAL_MS`. */
export const SYNC_DEFAULT_POLL_INTERVAL_MS = 15_000;
/** Mirrors `MIN_POLL_INTERVAL_MS`: below this a scan runs on every 1 Hz tick. */
export const SYNC_MIN_POLL_INTERVAL_MS = 2_000;

/**
 * The retention window keeper applies when a folder says nothing about it, and
 * the floor below which it refuses (Story 56.12, Story 56.5).
 *
 * Mirrored for the same reason as the four above — the form has to name the
 * number that will be in force while the box is still empty — but with one
 * difference worth knowing before touching it: `0` is not "nothing pinned"
 * here, it is keeper's documented instruction to never release. Rust REFUSES a
 * non-zero window below {@link SYNC_MIN_RELEASE_TTL_MS} rather than flooring
 * it, so the form must not silently round one up into a save.
 */
export const SYNC_DEFAULT_RELEASE_TTL_MS = 24 * 60 * 60 * 1000;
/** Mirrors `MIN_RELEASE_TTL_MS`. A non-zero window under a minute is refused. */
export const SYNC_MIN_RELEASE_TTL_MS = 60_000;
/** Mirrors `RELEASE_TTL_CEILING_MS` — ten years. Refused above, never clamped. */
export const SYNC_RELEASE_TTL_CEILING_MS = 3650 * 24 * 60 * 60 * 1000;

/** Fast poll cadence, used while any profile is doing work. */
export const SYNC_ACTIVE_POLL_MS = 2_000;

/**
 * How much slower the poll runs when nothing is active. An open settings pane
 * still has to notice a sync the watcher starts on its own, but re-reading
 * every idle profile twice a second buys nothing.
 */
export const SYNC_IDLE_POLL_FACTOR = 5;

/** Last-resort message when a rejection carries no readable one. */
export const SYNC_UNKNOWN_ERROR = "Sync failed for an unknown reason.";

/**
 * What a write aimed at a folder that is no longer configured says (Story 46.10).
 *
 * Only reachable through {@link setSyncProfileRecordingsSubfolder}, which has to
 * re-read the profile it is about to re-send: a folder removed from Settings
 * between the read that showed it and the write is a real race, and it is not the
 * same thing as a refused value. Worded as a fact about the folder rather than as
 * a failure, because nothing went wrong — the folder is simply gone.
 */
export const SYNC_PROFILE_GONE = "That synced folder is no longer configured.";

/**
 * The label the recordings subfolder wears on EVERY surface that shows it.
 *
 * Two surfaces show it: the folder form in Settings → Sync, which composes a
 * whole profile, and the Recording pane's Destination card, which edits this one
 * field on a profile that already exists (Story 46.10). They are deliberately
 * different controls — the card carries a consequence sentence the add flow has
 * nothing to warn about — but they are one setting, and a person who found it in
 * one place has to recognise it in the other. Kept here rather than in either
 * component so neither imports the other for a string.
 */
export const SYNC_RECORDINGS_SUBFOLDER_LABEL = "Recordings subfolder";

export interface SyncState {
  /**
   * The mirrored profiles, or `null` before the first successful load. `null`
   * means "unknown", never "none": controls render disabled against it rather
   * than claiming an empty folder list that was never read.
   */
  profiles: SyncProfileVm[] | null;
  /** The latest status per `profileId` — the polled snapshot the rows render. */
  statuses: Record<string, SyncStatusVm>;
  /** Whether a first load has landed; the boolean twin of `profiles !== null`. */
  hydrated: boolean;
  /**
   * The last *read* failure's message, cleared by the next successful read.
   * Action failures are not recorded here: they reject to their caller, which
   * owns where that message belongs (inline on the form, inline on the row).
   */
  error: string | null;
  /** Replace the mirrored profile list and mark the mirror hydrated. */
  setProfiles: (profiles: SyncProfileVm[]) => void;
  /** Merge fresh statuses in by `profileId`, dropping any orphaned entry. */
  mergeStatuses: (statuses: SyncStatusVm[]) => void;
  /** Record (or clear) the last read failure. */
  setError: (error: string | null) => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const syncStore = createStore<SyncState>()((set) => ({
  profiles: null,
  statuses: {},
  hydrated: false,
  error: null,
  setProfiles: (profiles) => set({ profiles, hydrated: true }),
  mergeStatuses: (statuses) =>
    set((state) => {
      // Merge rather than replace: a partial read must not blank the rows it
      // did not mention.
      const next: Record<string, SyncStatusVm> = { ...state.statuses };
      for (const status of statuses) {
        next[status.profileId] = status;
      }
      // …but a status may never outlive its profile, or a removed folder would
      // leave a ghost line behind.
      if (state.profiles !== null) {
        const known = new Set(state.profiles.map((profile) => profile.id));
        for (const id of Object.keys(next)) {
          if (!known.has(id)) {
            delete next[id];
          }
        }
      }
      return { statuses: next };
    }),
  setError: (error) => set({ error }),
}));

/** In-flight hydration, deduped so concurrent surfaces trigger one read. */
let hydration: Promise<void> | null = null;

/**
 * The human message carried by an {@link IpcError} rejection, or the honest
 * fallback. Shared so the form, the rows and the read path all surface the
 * Rust-authored sentence rather than each inventing their own wording.
 *
 * `fallback` is what a rejection with no readable message reads as. It exists
 * for the neighbouring one-time-copy store, whose failures are not sync
 * failures and must not be reported as one: a second extractor beside this one
 * would be a second place for the envelope shape to be got wrong.
 */
export function syncErrorMessage(raw: unknown, fallback: string = SYNC_UNKNOWN_ERROR): string {
  if (typeof raw === "object" && raw !== null && "message" in raw) {
    const { message } = raw as { message: unknown };
    if (typeof message === "string" && message.trim() !== "") {
      return message;
    }
  }
  return fallback;
}

/**
 * The states in which nothing can be moving, whatever any other field says.
 *
 * Every one of them is the engine reporting that this folder has stopped:
 * refused credentials, an unreachable remote, a detached volume, a human
 * pausing it. None can coexist with a running leg, because `Engine::publish`
 * writes `state = Syncing` for any active phase — so a live pass re-asserts a
 * live state on its very next frame and this list costs it nothing.
 */
const SYNC_STOPPED_STATES: Record<string, true> = {
  offline: true,
  mediaAbsent: true,
  paused: true,
  needsAttention: true,
};

/**
 * Whether this profile is doing work, or has work queued — the fast-poll gate
 * and the condition under which a row shows its progress meter.
 *
 * The stopped states are checked first and are not overridable. This used to be
 * `state === "syncing" || phase !== "idle" || pending > 0`, and the middle
 * clause is what turned an engine bug into a visible lie: `phase` is copied
 * verbatim off the last streamed event by `Engine::publish`, so asking it
 * whether a folder is busy asks the *stream's* last word while claiming to
 * consult the poll. A push refused with a 401 left `phase = "pushing"` behind a
 * `needsAttention` state, and the card went on drawing a half-full bar and a
 * frozen "4.1 MB/s" over a folder that had permanently stopped publishing —
 * directly above the error saying so (Stories 34.8 and 34.10).
 *
 * The engine is where that was fixed, since the tray and `keeper-syncd status`
 * read the same snapshot and a guard here would not have reached them. This
 * stands beside it because the ordering `syncLiveFraction` and `syncLiveRate`
 * already document in `sync-detail.ts` — the poll decides *whether*, the stream
 * only refines *how far* — is true only if "whether" is answered by the poll's
 * own verdict on the folder rather than by a field the stream wrote. A surface
 * whose entire job is honesty should not be one forgotten assignment away from
 * lying.
 */
export function isSyncStatusActive(status: SyncStatusVm): boolean {
  if (SYNC_STOPPED_STATES[status.state] === true) {
    return false;
  }
  return status.state === "syncing" || status.phase !== "idle" || status.pending > 0;
}

/**
 * Can this phase have a transfer rate at all?
 *
 * Mirrors `SyncPhase::carries_rate` in `keeper-sync`: only the three legs that
 * move bytes over a link can. A scan, a commit and a verify move none, so a
 * rate for them is not "not measured yet" — it is a figure that cannot exist.
 *
 * The distinction is what a surface needs to tell "quiet for a tick" from
 * "never applicable". A transfer that has gone quiet keeps its room reserved so
 * the row does not jump; a phase that cannot carry a rate is not given room,
 * because holding a slot open forever prints a separator with nothing after it
 * — which is what the Scanning line did the moment the walk started reporting.
 *
 * Named rather than inlined because the membership list is the contract, not
 * the comparison: it is the TypeScript half of a Rust `match` that is total by
 * construction there, and a phase added on one side has to be answered on this
 * one. Three strings in a `||` at a call site say nothing about which side owns
 * the list.
 */
export function syncPhaseCarriesRate(phase: string): boolean {
  return phase === "fetching" || phase === "uploadingLfs" || phase === "downloadingLfs";
}

/**
 * The progress fraction in `[0, 1]`, or `null` when no total is known and the
 * meter must render indeterminate.
 *
 * Bytes win over files when a byte total is known (they move smoothly); a zero
 * or absent total is "unknown", never a division. Values are clamped, because
 * a total discovered mid-transfer can briefly be smaller than what has already
 * moved and a meter must not overflow its track.
 */
export function syncProgressFraction(status: SyncStatusVm): number | null {
  if (status.bytesTotal !== null && status.bytesTotal > 0) {
    return Math.min(1, Math.max(0, status.bytesDone / status.bytesTotal));
  }
  if (status.filesTotal !== null && status.filesTotal > 0) {
    return Math.min(1, Math.max(0, status.filesDone / status.filesTotal));
  }
  return null;
}

/** Read both halves of the mirror in one round trip. */
async function loadSnapshot(): Promise<void> {
  const [profiles, statuses] = await Promise.all([syncProfiles(), syncStatuses()]);
  const state = syncStore.getState();
  state.setProfiles(profiles);
  state.mergeStatuses(statuses);
  state.setError(null);
}

/**
 * Lazily hydrate the mirror (once per app lifetime; concurrent callers share
 * one read). Best-effort: a read failure leaves the mirror unhydrated (the
 * surface stays disabled, never faking an empty list), records the message,
 * and nulls the shared promise so the next call retries.
 */
export async function ensureSyncHydrated(): Promise<void> {
  if (syncStore.getState().hydrated) {
    return;
  }
  hydration ??= loadSnapshot().catch((raw: unknown) => {
    syncStore.getState().setError(syncErrorMessage(raw));
    // Allow a later retry rather than caching the failure forever.
    hydration = null;
  });
  await hydration;
}

/**
 * Re-read the profile list. Never throws and never blanks: a failed read keeps
 * the previous list and records the message.
 */
export async function refreshSyncProfiles(): Promise<void> {
  try {
    const profiles = await syncProfiles();
    const state = syncStore.getState();
    state.setProfiles(profiles);
    state.setError(null);
  } catch (raw) {
    syncStore.getState().setError(syncErrorMessage(raw));
  }
}

/**
 * Re-read every status and merge by `profileId`. Never throws and never
 * blanks: a failed poll keeps the previous snapshot, so transient IPC noise
 * cannot flicker a transferring folder back to nothing.
 */
export async function refreshSyncStatuses(): Promise<void> {
  try {
    const statuses = await syncStatuses();
    const state = syncStore.getState();
    state.mergeStatuses(statuses);
    state.setError(null);
  } catch (raw) {
    syncStore.getState().setError(syncErrorMessage(raw));
  }
}

/** Re-read both halves after a write. Never throws. */
async function refreshSyncSnapshot(): Promise<void> {
  await refreshSyncProfiles();
  await refreshSyncStatuses();
}

/**
 * Create or update a profile, then re-read the mirror.
 *
 * Rejects with the Rust {@link IpcError} so the caller can surface the
 * validation message and keep the half-typed form intact.
 */
export async function saveSyncProfile(req: SyncProfileReq): Promise<SyncProfileVm> {
  const saved = await syncProfileSave(req);
  await refreshSyncSnapshot();
  return saved;
}

/**
 * Move ONE profile's recordings subfolder and leave everything else exactly as
 * stored (Story 46.10).
 *
 * Not a second writer: `sync_profile_save` is the only command that writes a
 * profile, and this is a named use of it. What it exists for is that the command
 * takes a WHOLE profile — `parse_req` assigns `name`, `localPath`, `remoteUrl`,
 * `branch`, `direction`, `lane`, `subpaths`, `excludes`, `removable` and
 * `lfsMode` unconditionally from the request — so "change the subfolder" has to
 * be expressed as "re-send this profile with one field different". Doing that
 * from a component would put a faithful `SyncProfileVm` → `SyncProfileReq`
 * re-expression next to a recording card, where the next field added to a
 * profile would be silently dropped by it; here it sits beside the writer, with
 * the mapping visible in one screen.
 *
 * The profile is re-read immediately before the save rather than taken from the
 * mirror, because a stale mirror would not merely fail — it would write its own
 * stale `remoteUrl` and `excludes` back over whatever changed since. That
 * narrows the window to one round trip, which is exactly the window the Sync
 * form already has, and no narrower window is available: `sync_profile_save`
 * takes no revision.
 *
 * Every `null` below is the AD-34-9 "not expressed" that leaves the stored value
 * alone, and every one of them is `null` in the VM precisely when the profile
 * pins nothing — so passing the VM's own value through is the faithful thing
 * rather than the lazy one.
 *
 * Rejects with the Rust {@link IpcError} — an unknown id, or
 * `RecordingsConfig::validate`'s own sentence for a subfolder it refuses — so
 * the caller prints the reason beside the field.
 */
export async function setSyncProfileRecordingsSubfolder(
  id: string,
  subfolder: string,
): Promise<SyncProfileVm> {
  const stored = (await syncProfiles()).find((profile) => profile.id === id);
  if (stored === undefined) {
    throw new Error(SYNC_PROFILE_GONE);
  }
  const folderOwned = new Set(stored.folderOwned);
  return await saveSyncProfile({
    id: stored.id,
    name: stored.name,
    localPath: stored.localPath,
    remoteUrl: stored.remoteUrl,
    branch: stored.branch,
    direction: stored.direction,
    lane: stored.lane,
    subpaths: stored.subpaths,
    excludes: stored.excludes,
    removable: stored.removable,
    lfsMode: stored.lfsMode,
    // Every key with an `Option` slot that a folder config file can own is sent
    // as the omission when it is owned, on the folder form's rule — the two
    // writers must not disagree about what "express" means. Sending an owned
    // value back is not harmless: `profile::as_stored` compares it against the
    // TABLE row, which for an owned key differs by definition, so it would log a
    // shadowed-change warning for a field this call never touched.
    lfsThresholdBytes: folderOwned.has("lfsThresholdBytes") ? null : stored.lfsThresholdBytes,
    // The three virtualization knobs (Story 56.12), which this call is not
    // about and must therefore carry faithfully. The VM reports them as they
    // are IN FORCE — `db::list_profiles` layers the folder's own config on
    // before the view model is built — so the omission is what an owned key
    // gets here too.
    virtualPatterns: folderOwned.has("virtualPatterns") ? null : stored.virtualPatterns,
    virtualOverBytes: folderOwned.has("virtualOverBytes") ? null : stored.virtualOverBytes,
    releaseTtlMs: folderOwned.has("releaseTtlMs") ? null : stored.releaseTtlMs,
    settleMs: stored.settleMs,
    pollIntervalMs: stored.pollIntervalMs,
    tags: stored.tags,
    authorOverride: stored.authorOverride,
    commitSubjectTemplate: folderOwned.has("commitSubjectTemplate")
      ? null
      : stored.commitSubjectTemplate,
    // The flags ride along unchanged, on the same ownership rule.
    // `notesSubfolder` is `null` for a folder that is not a vault, which is
    // again the omission rather than a clear.
    notes: folderOwned.has("notes") ? null : stored.notes,
    notesSubfolder: folderOwned.has("notes") ? null : stored.notesSubfolder,
    recordings: folderOwned.has("recordings") ? null : stored.recordings,
    // The one field this call is about. Sent verbatim: Rust trims whitespace and
    // otherwise refuses rather than corrects, and correcting it here would make
    // a save succeed against a folder nobody named.
    recordingsSubfolder: subfolder,
    sessions: folderOwned.has("sessions") ? null : stored.sessions,
    sessionsSubfolder: folderOwned.has("sessions") ? null : stored.sessionsSubfolder,
  });
}

/**
 * Forget a profile, then re-read the mirror. Configuration only — the folder,
 * its contents and its git repository are left on disk. Rust also deletes the
 * profile's stored access token, which is keeper's own configuration and never
 * lived in the folder (AD-34-14).
 */
export async function removeSyncProfile(id: string): Promise<void> {
  await syncProfileRemove(id);
  await refreshSyncSnapshot();
}

/**
 * Pause or resume a profile. The returned status is merged immediately so the
 * row reflects the write without waiting for the next poll; the refresh that
 * follows picks up whatever else moved meanwhile (including `profile.enabled`,
 * which drives the Pause/Resume label).
 */
export async function setSyncProfileEnabled(id: string, enabled: boolean): Promise<SyncStatusVm> {
  const status = await syncProfileSetEnabled(id, enabled);
  syncStore.getState().mergeStatuses([status]);
  await refreshSyncSnapshot();
  return status;
}

/**
 * Sync one folder now, ignoring its schedule, then re-read the statuses.
 *
 * The outcome is the caller's report and every caller renders it. It is not
 * optional decoration: a pass that finds nothing to do completes in
 * milliseconds and leaves the status untouched, so the refresh below can
 * change nothing at all and the click would otherwise look like it did
 * nothing. The engine already knows what happened; this is the only place that
 * answer reaches the screen (AD-34-12).
 */
export async function syncProfileNow(id: string): Promise<SyncOutcomeVm> {
  const outcome = await syncFolderNow(id);
  await refreshSyncStatuses();
  return outcome;
}

/**
 * Re-read a profile's tracked files, then re-read the statuses.
 *
 * Resolves the whole report, not just the faults: `checked` and `virtualPaths`
 * are what let the caller say how much of the folder was looked at and how much
 * of it is away on purpose. An empty `problems` means everything checked out.
 */
export async function verifySyncProfile(id: string): Promise<SyncVerifyVm> {
  const report = await syncVerify(id);
  await refreshSyncStatuses();
  return report;
}

/**
 * Make a profile forget its remembered tree and look again.
 *
 * The statuses are re-read straight after, but the visible answer arrives on the
 * next walk rather than here — the engine queues the work, it does not do it
 * inline, and pretending otherwise would leave the caller reporting a result it
 * does not have.
 */
export async function rescanSyncProfile(id: string): Promise<void> {
  await syncRescan(id);
  await refreshSyncStatuses();
}

/**
 * Poll the statuses until the returned stop function is called.
 *
 * Runs at `intervalMs` while any profile is active and
 * {@link SYNC_IDLE_POLL_FACTOR}× slower when none is, re-deciding after every
 * tick. The first tick lands one interval in — callers hydrate first, so an
 * immediate re-read would only duplicate that. A failed tick keeps the
 * previous snapshot and does not stop the loop.
 */
export function startSyncStatusPolling(intervalMs: number = SYNC_ACTIVE_POLL_MS): () => void {
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const schedule = () => {
    if (stopped) {
      return;
    }
    const active = Object.values(syncStore.getState().statuses).some(isSyncStatusActive);
    timer = setTimeout(tick, active ? intervalMs : intervalMs * SYNC_IDLE_POLL_FACTOR);
  };

  const tick = () => {
    void refreshSyncStatuses().then(schedule);
  };

  schedule();

  return () => {
    stopped = true;
    if (timer !== null) {
      clearTimeout(timer);
      timer = null;
    }
  };
}

/**
 * React selector hook over {@link syncStore}. Pass a selector to subscribe to
 * just the slice a component needs.
 */
export function useSyncStore<T>(selector: (state: SyncState) => T): T {
  return useStore(syncStore, selector);
}

/** Test-only reset: clear the mirror and forget any in-flight hydration. */
export function resetSyncStoreForTest(): void {
  hydration = null;
  syncStore.setState({ profiles: null, statuses: {}, hydrated: false, error: null });
}
