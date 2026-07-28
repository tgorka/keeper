/**
 * One-time verified copy job mirror (Epic 33, Story 33.3, AD-C1/AD-C2/AD-C6).
 *
 * A sibling of `sync-detail.ts` rather than a room inside it, because the two
 * mirror different things. Every record in `sync-detail` is keyed by a
 * `profileId`: `refreshSyncDetailAll` fans out over the profile mirror and
 * `retainProfiles` prunes whatever has left it. A copy is a job, never a
 * relationship (AD-C1) — it has no profile, so it would be the one key in that
 * map nothing owns and nothing prunes. The cadences disagree too: folder detail
 * is a background poll measured in seconds, while this is a foreground job the
 * user is watching, polled at {@link COPY_POLL_MS} and only while it runs.
 *
 * Same idiom as its siblings otherwise: a vanilla zustand store created at
 * module load outside React, mirroring what Rust reports and deciding nothing.
 *
 * Three distinctions this store exists to keep straight, because collapsing any
 * of them would make the surface lie:
 *   - {@link CopyState.error} is the IPC going wrong. {@link CopyJobVm.error} is
 *     the job failing to *run*. A file that could not be copied is neither — it
 *     is an entry with outcome `failed` inside a job that finished.
 *   - `entries` is empty until the job is terminal, so a report is only ever
 *     read from a settled job; a partial one would render as a finished one.
 *   - A total the engine has not worked out yet is `0`, which means unknown.
 *     {@link copyJobFraction} answers `null` there rather than dividing.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import {
  type CopyEntryVm,
  type CopyJobState,
  type CopyJobVm,
  copyCancel,
  copyStart,
  copyStatus,
} from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

/**
 * Poll cadence while a job runs. An order of magnitude faster than the folder
 * detail poll on purpose: this is a bar somebody is watching move, not a list
 * that changes when a file does.
 */
export const COPY_POLL_MS = 500;

/** Last-resort message when a rejection carries no readable one. */
export const COPY_UNKNOWN_ERROR = "The copy failed for an unknown reason.";

/**
 * Outcomes worst first (AD-C6). The report is ordered by this and the summary
 * is derived from the same grouping, so the two can never disagree about what
 * happened.
 */
export const COPY_OUTCOME_ORDER: readonly string[] = ["failed", "collision", "copied", "identical"];

/** One outcome's entries, in the order Rust reported them. */
export interface CopyGroup {
  /** The wire outcome, rendered as itself if Rust ever grows another one. */
  outcome: string;
  entries: CopyEntryVm[];
}

export interface CopyState {
  /** The job this session started, or `null` before the first start. */
  id: string | null;
  /**
   * The newest polled snapshot, or `null` while the first read of a fresh job
   * is still outstanding — unknown, never "nothing happened".
   */
  job: CopyJobVm | null;
  /** Whether {@link copyStart} is in flight and no id has landed yet. */
  starting: boolean;
  /**
   * The last command or read failure, cleared when the next job starts. The
   * IPC going wrong, not the job going wrong: a rejected start (a source that
   * does not exist, a destination inside the source) lands here and no job
   * exists at all.
   */
  error: string | null;
  /** Clear the previous job outright and claim a start is in flight. */
  begin: () => void;
  /** Record the id `copy_start` minted; polling can begin against it. */
  started: (id: string) => void;
  /** Record a polled snapshot. */
  applyJob: (job: CopyJobVm) => void;
  /** Record an IPC failure and stop claiming a start is in flight. */
  fail: (message: string) => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const copyJobStore = createStore<CopyState>()((set) => ({
  id: null,
  job: null,
  starting: false,
  error: null,
  // A second copy must never show the first one's report while it runs, so a
  // start blanks the job rather than merging into it.
  begin: () => set({ id: null, job: null, starting: true, error: null }),
  started: (id) => set({ id, starting: false }),
  applyJob: (job) => set({ job, starting: false, error: null }),
  fail: (message) => set({ starting: false, error: message }),
}));

/**
 * Whether a job has settled: its state cannot change again, and its `entries`
 * are the whole report rather than a partial one.
 */
export function isCopyJobTerminal(state: CopyJobState): boolean {
  return state === "done" || state === "failed" || state === "cancelled";
}

/**
 * Whether a job is under way — starting, or started and not yet settled. Also
 * true for a started job whose first snapshot has not landed: a job we know
 * exists but cannot yet describe is running, not finished.
 */
export function isCopyRunning(state: CopyState): boolean {
  if (state.starting) {
    return true;
  }
  return state.id !== null && (state.job === null || !isCopyJobTerminal(state.job.state));
}

/**
 * The fraction to draw in `[0, 1]`, or `null` when the engine has not worked
 * out a byte total yet and a meter would have to invent a position. Clamped,
 * because a total discovered mid-walk can briefly sit below what has already
 * moved and a bar must not overflow its track.
 */
export function copyJobFraction(job: CopyJobVm): number | null {
  if (job.bytesTotal <= 0) {
    return null;
  }
  return Math.min(1, Math.max(0, job.bytesDone / job.bytesTotal));
}

/**
 * Group a settled job's entries by outcome, worst first (AD-C6).
 *
 * An outcome added in Rust after this file was written ranks after every known
 * one and keeps its first-seen position, because a report that quietly dropped
 * a row it could not rank would under-report the copy — the one thing this
 * surface may never do.
 */
export function copyEntryGroups(entries: readonly CopyEntryVm[]): CopyGroup[] {
  const grouped = new Map<string, CopyEntryVm[]>();
  for (const entry of entries) {
    const bucket = grouped.get(entry.outcome);
    if (bucket === undefined) {
      grouped.set(entry.outcome, [entry]);
    } else {
      bucket.push(entry);
    }
  }
  const groups: CopyGroup[] = [];
  for (const outcome of COPY_OUTCOME_ORDER) {
    const rows = grouped.get(outcome);
    if (rows !== undefined) {
      groups.push({ outcome, entries: rows });
      grouped.delete(outcome);
    }
  }
  for (const [outcome, rows] of grouped) {
    groups.push({ outcome, entries: rows });
  }
  return groups;
}

/**
 * Re-read the running job's snapshot. Never throws and never blanks: a failed
 * read keeps the previous snapshot and records the message, because "no answer"
 * must not render as "no progress".
 *
 * A snapshot for a job the user has already replaced is dropped — a slow read
 * landing after a fresh start would otherwise resurrect the old report.
 */
export async function refreshCopyJob(): Promise<void> {
  const { id } = copyJobStore.getState();
  if (id === null) {
    return;
  }
  try {
    const job = await copyStatus(id);
    if (copyJobStore.getState().id === id) {
      copyJobStore.getState().applyJob(job);
    }
  } catch (raw) {
    if (copyJobStore.getState().id === id) {
      copyJobStore.getState().fail(syncErrorMessage(raw, COPY_UNKNOWN_ERROR));
    }
  }
}

/**
 * Start a job and land its first snapshot without waiting out a poll interval.
 *
 * Never throws: a rejected start (a source that does not exist, a destination
 * inside the source) is recorded as {@link CopyState.error} and leaves no job
 * behind, because Rust refuses both before one is registered.
 *
 * A start while one is already under way is ignored rather than queued. The
 * button that calls this is disabled for exactly that window, but a job the
 * store forgot the id of would keep copying in Rust with nothing able to stop
 * it, and a double-fire is far cheaper to refuse here than to survive there.
 */
export async function startCopyJob(
  source: string,
  destination: string,
  replaceExisting: boolean,
): Promise<void> {
  if (isCopyRunning(copyJobStore.getState())) {
    return;
  }
  copyJobStore.getState().begin();
  try {
    copyJobStore.getState().started(await copyStart(source, destination, replaceExisting));
  } catch (raw) {
    copyJobStore.getState().fail(syncErrorMessage(raw, COPY_UNKNOWN_ERROR));
    return;
  }
  await refreshCopyJob();
}

/**
 * Ask the running job to stop, then read it. Idempotent in Rust and safe at any
 * moment: the engine checks the flag between chunks, leaves no temp file
 * behind, and settles `cancelled` carrying everything that had already
 * finished. Never throws.
 */
export async function cancelCopyJob(): Promise<void> {
  const { id } = copyJobStore.getState();
  if (id === null) {
    return;
  }
  try {
    await copyCancel(id);
  } catch (raw) {
    copyJobStore.getState().fail(syncErrorMessage(raw, COPY_UNKNOWN_ERROR));
    return;
  }
  // The flag is usually seen within a chunk, so reading now shows the stop
  // rather than leaving a dead bar up for another interval.
  await refreshCopyJob();
}

/**
 * Poll the running job until it settles or the returned stop function is
 * called.
 *
 * The loop retires itself on a terminal snapshot: the state that ended the job
 * arrived with the entries, so there is nothing left to ask. The returned stop
 * exists for the other ending — a window closed mid-copy, which leaves the job
 * running in Rust and simply stops watching it.
 */
export function startCopyJobPolling(intervalMs: number = COPY_POLL_MS): () => void {
  let stopped = false;
  let timer: ReturnType<typeof setTimeout> | null = null;

  const schedule = () => {
    const { job } = copyJobStore.getState();
    if (stopped || (job !== null && isCopyJobTerminal(job.state))) {
      return;
    }
    timer = setTimeout(() => {
      void refreshCopyJob().then(schedule);
    }, intervalMs);
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
 * React selector hook over {@link copyJobStore}. Pass a selector to subscribe
 * to just the slice a component needs.
 */
export function useCopyJobStore<T>(selector: (state: CopyState) => T): T {
  return useStore(copyJobStore, selector);
}

/** Test-only reset: forget the job this session started. */
export function resetCopyJobStoreForTest(): void {
  copyJobStore.setState({ id: null, job: null, starting: false, error: null });
}
