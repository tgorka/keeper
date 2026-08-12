/**
 * The sessions board's row mirror and filter state (Phase 7, FR-228, FR-229).
 *
 * One store for both concerns because at zone scale the list IS the filter's
 * output: rows arrive whole from `sessions_list` (no windowing, no op-diff —
 * the AD-114 zone-scale decision) and the filter is applied in the selector.
 * Holds no Rust-owned session state beyond the mirrored rows; a change event
 * re-reads rather than patching.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { SessionRowVm } from "@/lib/ipc/client";

/** Which lifecycle slice the board shows (FR-229's `is:` set, chip form). */
export type SessionsStatusFilter = "all" | "active" | "archived";

/** How long an active session may sit untouched before `stale` (FR-229). */
export const SESSIONS_STALE_DAYS = 14;

export interface SessionsListState {
  /** The mirrored rows for the active root, or `null` before the first read. */
  rows: SessionRowVm[] | null;
  /** Which root the rows belong to — a stale-guard for late reads. */
  rowsRootId: string | null;
  /** Free-text filter over title, path, tags, snippet and log line. */
  text: string;
  status: SessionsStatusFilter;
  /** Show only pinned sessions. */
  pinnedOnly: boolean;
  /** Show only unread sessions. */
  unreadOnly: boolean;
  /** Human-readable read failure, or `null`. */
  error: string | null;
  reset: (rootId: string, rows: SessionRowVm[]) => void;
  clear: () => void;
  setText: (text: string) => void;
  setStatus: (status: SessionsStatusFilter) => void;
  setPinnedOnly: (pinnedOnly: boolean) => void;
  setUnreadOnly: (unreadOnly: boolean) => void;
  setError: (error: string | null) => void;
}

export const sessionsListStore = createStore<SessionsListState>()((set) => ({
  rows: null,
  rowsRootId: null,
  text: "",
  status: "all",
  pinnedOnly: false,
  unreadOnly: false,
  error: null,
  reset: (rootId, rows) => set({ rows, rowsRootId: rootId, error: null }),
  clear: () => set({ rows: null, rowsRootId: null, error: null }),
  setText: (text) => set({ text }),
  setStatus: (status) => set({ status }),
  setPinnedOnly: (pinnedOnly) => set({ pinnedOnly }),
  setUnreadOnly: (unreadOnly) => set({ unreadOnly }),
  setError: (error) => set({ error }),
}));

export function useSessionsListStore<T>(selector: (state: SessionsListState) => T): T {
  return useStore(sessionsListStore, selector);
}

/**
 * Whether an active row is stale: no change on either freshness signal for
 * {@link SESSIONS_STALE_DAYS}. Archived rows are never stale — staleness is a
 * nudge about running work, and archived work is finished by definition.
 */
export function isStale(row: SessionRowVm, nowMs: number): boolean {
  if (row.status !== "active") {
    return false;
  }
  const newest = Math.max(row.workspaceMs ?? 0, row.recordMs ?? 0);
  if (newest === 0) {
    return false;
  }
  return nowMs - newest > SESSIONS_STALE_DAYS * 24 * 60 * 60 * 1000;
}

/**
 * The filter, applied. Pure and exported so tests exercise it without a
 * component: text folds case, matches title, folder path, tags, snippet and
 * the last log line — the "half-remembered fragment" surfaces (FR-229 board
 * half; the full query grammar arrives with the search story).
 */
export function filterRows(
  rows: readonly SessionRowVm[],
  state: Pick<SessionsListState, "text" | "status" | "pinnedOnly" | "unreadOnly">,
): SessionRowVm[] {
  const needle = state.text.trim().toLowerCase();
  return rows.filter((row) => {
    if (state.status !== "all" && row.status !== state.status) {
      return false;
    }
    if (state.pinnedOnly && !row.pinned) {
      return false;
    }
    if (state.unreadOnly && !row.unread) {
      return false;
    }
    if (needle === "") {
      return true;
    }
    const haystack = [row.title, row.path, row.snippet, row.lastLogLine, ...row.tags]
      .join("\n")
      .toLowerCase();
    return haystack.includes(needle);
  });
}

/** Test-only reset. */
export function resetSessionsListStoreForTest(): void {
  sessionsListStore.setState({
    rows: null,
    rowsRootId: null,
    text: "",
    status: "all",
    pinnedOnly: false,
    unreadOnly: false,
    error: null,
  });
}
