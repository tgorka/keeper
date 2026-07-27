/**
 * Per-profile sync detail mirror (Epic 32, Story 32.5, AD-S2/AD-S3/AD-S5/AD-S6).
 *
 * The second half of the sync mirror: `sync.ts` holds what each profile *is*
 * (its configuration and its Rust-composed status line); this holds what it has
 * *done*, what it is *waiting on*, and what is *wrong* — the three lists the
 * Sync view renders. Same idiom as its sibling: a vanilla zustand store created
 * at module load outside React, mirroring Rust and never deciding anything.
 *
 * Three rules this store exists to honor:
 *   - `null` means unknown, `[]` means empty. An unknown profile *rejects* these
 *     reads rather than resolving empty, so a failed read must never be
 *     rendered as "nothing pending" — it keeps the previous list and records
 *     the message.
 *   - Activity, pending and problems are polled on a modest cadence
 *     ({@link SYNC_DETAIL_POLL_MS}); the in-flight counters come from the
 *     progress *stream*, which is the only sub-second source. The two are kept
 *     apart because the polled status is what the tray reads with no webview
 *     subscribed at all.
 *   - A stream event never outranks the poll on the question "is this profile
 *     working": {@link syncLiveFraction} reads the answer from the polled
 *     status and spends the stream only on the number.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import {
  type SyncActivityVm,
  type SyncPendingVm,
  type SyncProblemsVm,
  type SyncProgressVm,
  type SyncStatusVm,
  syncActivity,
  syncPending,
  syncProblems,
  syncRetryParked,
  syncSubscribeProgress,
  syncUnsubscribeProgress,
} from "@/lib/ipc/client";
import {
  isSyncStatusActive,
  syncErrorMessage,
  syncProgressFraction,
  syncStore,
} from "@/lib/stores/sync";

/**
 * How many activity rows the view asks for. The engine keeps far more per
 * profile, but a card is a recent-history surface, not a log viewer.
 */
export const SYNC_ACTIVITY_LIMIT = 20;

/**
 * Detail poll cadence. Deliberately slower than the status poll: the status
 * line is what moves second to second, while these three lists change only
 * when a file does.
 */
export const SYNC_DETAIL_POLL_MS = 5_000;

/** One profile's three lists, plus the last read failure for any of them. */
export interface SyncDetailEntry {
  /** Newest-first activity as Rust ordered it, or `null` before a read lands. */
  activity: SyncActivityVm[] | null;
  /** What is waiting to sync, or `null` before a read lands. */
  pending: SyncPendingVm[] | null;
  /** Everything wrong with the profile, or `null` before a read lands. */
  problems: SyncProblemsVm | null;
  /**
   * The last read failure's message, cleared by the next clean read. Held per
   * profile so one folder's unreachable engine cannot blank another's lists.
   */
  error: string | null;
}

/** The unknown-everything entry a profile starts from. */
const EMPTY_ENTRY: SyncDetailEntry = Object.freeze({
  activity: null,
  pending: null,
  problems: null,
  error: null,
});

export interface SyncDetailState {
  /** The mirrored detail per `profileId`. A missing key is "never read". */
  detail: Record<string, SyncDetailEntry>;
  /**
   * The newest streamed progress event per `profileId` — the only sub-second
   * source of in-flight counters, and the only place `current` (the path in
   * flight) exists at all.
   */
  progress: Record<string, SyncProgressVm>;
  /** Merge whichever legs a read resolved into one profile's entry. */
  mergeDetail: (id: string, entry: Partial<SyncDetailEntry>) => void;
  /** Record the newest streamed progress event for its profile. */
  applyProgress: (event: SyncProgressVm) => void;
  /** Drop detail and progress for every profile no longer in the mirror. */
  retainProfiles: (ids: readonly string[]) => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const syncDetailStore = createStore<SyncDetailState>()((set) => ({
  detail: {},
  progress: {},
  mergeDetail: (id, entry) =>
    set((state) => ({
      detail: { ...state.detail, [id]: { ...(state.detail[id] ?? EMPTY_ENTRY), ...entry } },
    })),
  applyProgress: (event) =>
    set((state) => ({ progress: { ...state.progress, [event.profileId]: event } })),
  retainProfiles: (ids) =>
    set((state) => {
      // A removed folder must not leave its activity behind, the same rule
      // `mergeStatuses` follows for the status line.
      const known = new Set(ids);
      const detail: Record<string, SyncDetailEntry> = {};
      for (const [id, entry] of Object.entries(state.detail)) {
        if (known.has(id)) {
          detail[id] = entry;
        }
      }
      const progress: Record<string, SyncProgressVm> = {};
      for (const [id, event] of Object.entries(state.progress)) {
        if (known.has(id)) {
          progress[id] = event;
        }
      }
      return { detail, progress };
    }),
}));

/**
 * The fraction to draw for a profile in `[0, 1]`, or `null` when there is
 * nothing honest to draw.
 *
 * The polled status decides *whether* a profile is working; the stream only
 * refines *how far*. That ordering matters: a progress event is the last one
 * the engine sent, so an idle profile whose final event never arrived would
 * otherwise keep a filled bar forever.
 *
 * The streamed fraction wins when present because it is composed from
 * denominators the poll has not caught up with — and it is clamped, because a
 * byte total legitimately grows mid-transfer as concurrent large-file objects
 * are announced.
 */
export function syncLiveFraction(
  status: SyncStatusVm | undefined,
  progress: SyncProgressVm | undefined,
): number | null {
  if (status === undefined || !isSyncStatusActive(status)) {
    return null;
  }
  if (progress !== undefined && progress.fraction !== null) {
    return Math.min(1, Math.max(0, progress.fraction));
  }
  return syncProgressFraction(status);
}

/**
 * Re-read one profile's three lists. Never throws and never blanks: a leg that
 * fails keeps its previous value and records the message, because an unknown
 * profile rejects rather than resolving empty and "no rows" must never be
 * inferred from "no answer".
 */
export async function refreshSyncDetail(id: string): Promise<void> {
  const [activity, pending, problems] = await Promise.allSettled([
    syncActivity(id, SYNC_ACTIVITY_LIMIT),
    syncPending(id),
    syncProblems(id),
  ]);
  const entry: Partial<SyncDetailEntry> = {};
  if (activity.status === "fulfilled") {
    entry.activity = activity.value;
  }
  if (pending.status === "fulfilled") {
    entry.pending = pending.value;
  }
  if (problems.status === "fulfilled") {
    entry.problems = problems.value;
  }
  const failure = [activity, pending, problems].find((leg) => leg.status === "rejected");
  entry.error = failure === undefined ? null : syncErrorMessage(failure.reason);
  syncDetailStore.getState().mergeDetail(id, entry);
}

/**
 * Re-read every mirrored profile's detail, and forget any profile that has
 * left the mirror. Never throws.
 */
export async function refreshSyncDetailAll(): Promise<void> {
  const ids = (syncStore.getState().profiles ?? []).map((profile) => profile.id);
  syncDetailStore.getState().retainProfiles(ids);
  await Promise.all(ids.map(refreshSyncDetail));
}

/**
 * Poll every mirrored profile's detail until the returned stop function is
 * called.
 *
 * Unlike the status poll this one runs at a single modest cadence: the lists
 * are file-granular, so re-reading them twice a second buys nothing even
 * mid-transfer. The first tick lands one interval in — callers do the initial
 * read themselves, after hydration, when there are profiles to read.
 */
export function startSyncDetailPolling(intervalMs: number = SYNC_DETAIL_POLL_MS): () => void {
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const schedule = () => {
    if (!stopped) {
      timer = setTimeout(tick, intervalMs);
    }
  };

  const tick = () => {
    void refreshSyncDetailAll().then(schedule);
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
 * Mirror the sync progress stream until the returned stop function is called.
 *
 * The same race-safe shape as the bridge-health subscription: the sink is gated
 * so a late event after teardown never mutates the store, a teardown that beats
 * the subscribe id unsubscribes as soon as it lands, and a failed subscribe is
 * swallowed — progress detail is a refinement, and the polled status keeps the
 * view honest without it.
 */
export function startSyncProgressStream(): () => void {
  let stopped = false;
  let subscriptionId: number | null = null;

  syncSubscribeProgress((event) => {
    if (!stopped) {
      syncDetailStore.getState().applyProgress(event);
    }
  })
    .then((id) => {
      if (stopped) {
        void syncUnsubscribeProgress(id).catch(() => {});
        return;
      }
      subscriptionId = id;
    })
    .catch(() => {
      // No stream: the polled status still carries state, counters and the line.
    });

  return () => {
    stopped = true;
    if (subscriptionId !== null) {
      void syncUnsubscribeProgress(subscriptionId).catch(() => {});
      subscriptionId = null;
    }
  };
}

/**
 * Return one parked unit of work to the pending queue, then re-read the
 * profile's detail so the row leaves the Problems list on its own.
 *
 * Rejects with the Rust {@link IpcError} so the caller can surface the message
 * beside the unit it belongs to.
 */
export async function retrySyncParked(id: string, unitId: number): Promise<void> {
  await syncRetryParked(id, unitId);
  await refreshSyncDetail(id);
}

/**
 * React selector hook over {@link syncDetailStore}. Pass a selector to
 * subscribe to just the slice a component needs.
 */
export function useSyncDetailStore<T>(selector: (state: SyncDetailState) => T): T {
  return useStore(syncDetailStore, selector);
}

/** Test-only reset: forget every mirrored list and streamed counter. */
export function resetSyncDetailStoreForTest(): void {
  syncDetailStore.setState({ detail: {}, progress: {} });
}
