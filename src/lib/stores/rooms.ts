/**
 * Merged-inbox mirror store (AD-9, AD-20).
 *
 * A vanilla zustand store created at module load *outside* React. It holds only
 * the recency-ordered {@link InboxRoomVm} array streamed from the Rust
 * `keeper-core::inbox` merge, plus the known total — a pure mirror of the merged
 * window, never a source of truth. `applyBatch` folds each {@link InboxOp} onto
 * an immutable array by index and **never sorts, re-sorts, or re-orders**
 * (ordering across accounts is authoritative from Rust). A `Reset` replaces
 * contents wholesale, so re-subscribing (StrictMode remount, account add/remove)
 * never duplicates rows.
 *
 * Selection is `{ accountId, roomId }` because rows now come from different
 * accounts (Story 2.1): the conversation pane binds its timeline/composer to
 * that pair, not a global "current account".
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { InboxBatch, InboxOp, InboxRoomVm } from "@/lib/ipc/client";
import { applyDiffOp } from "@/lib/stores/vector-diff";

/** The open conversation, identified by its owning account and room. */
export interface RoomSelection {
  accountId: string;
  roomId: string;
}

/**
 * A pending deep-link focus target (Story 5.4): the account/room to open and the
 * `eventId` (the search hit's sanctioned deep-link handle) the conversation pane
 * should resolve to a timeline render key, scroll to, and tint. Cleared once the
 * pane has handled it (jumped, or degraded honestly when unreachable).
 */
export interface FocusEvent {
  accountId: string;
  roomId: string;
  eventId: string;
}

/**
 * Key an optimistic-unread override by its owning account and room. Kept as a flat
 * string so a plain `Map` can carry the overlay without nested lookups.
 */
export function unreadOverrideKey(accountId: string, roomId: string): string {
  return `${accountId}|${roomId}`;
}

export interface RoomsState {
  /** The merged inbox window, exactly as Rust streamed it (recency order). */
  rooms: InboxRoomVm[];
  /** Number of rooms in the streamed Inbox window (its partition length), or `null`. */
  total: number | null;
  /**
   * Ephemeral optimistic-unread overlay (Story 4.1). Keyed by
   * {@link unreadOverrideKey}, mapping to the *intended* `isUnread` value the user
   * just chose from the row context menu. This is NOT a source of truth — it
   * mirrors the send local-echo pattern: it lets a row flip within one frame while
   * the authoritative Rust stream catches up, and each entry is dropped in
   * `applyBatch` once the streamed VM for that room matches the intended value.
   */
  optimisticUnread: Map<string, boolean>;
  /**
   * The currently selected conversation, or `null` when none is open. Ephemeral
   * UI state (not mirrored from Rust) — it only records which room the
   * conversation pane should stream, on which account.
   */
  selected: RoomSelection | null;
  /**
   * A pending deep-link focus target (Story 5.4), or `null` when none is pending.
   * Set by {@link requestFocus} (typically from a search result activation) and
   * consumed by the conversation pane, which resolves the `eventId` to a timeline
   * render key, scrolls to it, and applies the search-highlight tint. Ephemeral UI
   * state — never a source of truth.
   */
  focusEvent: FocusEvent | null;
  /** Apply one streamed batch (its ops in sequence), updating `total`. */
  applyBatch: (batch: InboxBatch) => void;
  /**
   * Set an optimistic-unread override for a room (Story 4.1). Records the intended
   * `isUnread` so the row renders it within one frame; `applyBatch` reconciles it
   * away once the authoritative stream converges.
   */
  setOptimisticUnread: (accountId: string, roomId: string, isUnread: boolean) => void;
  /**
   * Drop a room's optimistic-unread override (Story 4.1). Used to revert the
   * overlay when the mark command hard-rejects, so a phantom override the stream
   * will never reconcile cannot persist.
   */
  clearOptimisticUnread: (accountId: string, roomId: string) => void;
  /** Select a conversation to open (or `null` to close). */
  selectRoom: (selection: RoomSelection | null) => void;
  /**
   * Request a deep-link focus (Story 5.4): open the target room (via
   * {@link selectRoom}) and record the pending {@link FocusEvent} so the
   * conversation pane resolves + scrolls to the matched message. Selecting the
   * room here means a search result on a not-yet-open Chat lands correctly.
   */
  requestFocus: (focus: FocusEvent) => void;
  /** Clear the pending deep-link focus once the pane has handled it. */
  clearFocus: () => void;
  /** Reset to the empty state (on unsubscribe / full sign-out). */
  clear: () => void;
}

/**
 * The room's effective unread state: an active optimistic override wins, else the
 * authoritative streamed `isUnread` (Story 4.1). The frontend never re-derives
 * unread from events — this only lets the local echo lead the stream by a frame.
 */
export function effectiveIsUnread(
  room: InboxRoomVm,
  optimisticUnread: Map<string, boolean>,
): boolean {
  const override = optimisticUnread.get(unreadOverrideKey(room.accountId, room.roomId));
  return override ?? room.isUnread;
}

/**
 * Reconcile the optimistic overlay against the freshly-applied window. An override
 * is dropped when the stream has caught up to it — either the room is present with
 * an authoritative `isUnread` equal to the intended value (converged), or the room
 * has left the window entirely (removed/archived elsewhere) and can no longer
 * reconcile. This prevents stale overrides from leaking unboundedly or masking the
 * authoritative state. Iterates the (tiny) override set, not the window. Returns the
 * same map instance when nothing changed so the store can skip a needless update. Pure.
 */
function reconcileOptimisticUnread(
  overrides: Map<string, boolean>,
  rooms: InboxRoomVm[],
): Map<string, boolean> {
  if (overrides.size === 0) {
    return overrides;
  }
  const authoritative = new Map<string, boolean>();
  for (const room of rooms) {
    authoritative.set(unreadOverrideKey(room.accountId, room.roomId), room.isUnread);
  }
  let next: Map<string, boolean> | null = null;
  for (const [key, intended] of overrides) {
    const auth = authoritative.get(key);
    // Drop when the room is gone from the window (auth === undefined) or the
    // stream now agrees with the intended value (converged).
    if (auth === undefined || auth === intended) {
      if (next === null) {
        next = new Map(overrides);
      }
      next.delete(key);
    }
  }
  return next ?? overrides;
}

/**
 * Fold a single op onto `rooms`, returning a new array (immutable). Delegates to
 * the shared, range-guarded {@link applyDiffOp} reducer — pure, and never sorts.
 * `InboxOp` (its single-item ops carry `room`, list ops carry `rooms`) is
 * assignable to the reducer's canonical `DiffOp` union.
 */
function applyOp(rooms: InboxRoomVm[], op: InboxOp): InboxRoomVm[] {
  return applyDiffOp(rooms, op);
}

/**
 * The vanilla store instance. Created once at module load, shared across the
 * app; the source of truth for room state stays in Rust.
 */
export const roomsStore = createStore<RoomsState>()((set) => ({
  rooms: [],
  total: null,
  optimisticUnread: new Map<string, boolean>(),
  selected: null,
  focusEvent: null,
  applyBatch: (batch) =>
    set((state) => {
      const rooms = batch.ops.reduce(applyOp, state.rooms);
      return {
        rooms,
        total: batch.total ?? state.total,
        // Reconcile the overlay against the freshly-applied authoritative window:
        // drop any override the stream has now caught up to (Story 4.1).
        optimisticUnread: reconcileOptimisticUnread(state.optimisticUnread, rooms),
      };
    }),
  setOptimisticUnread: (accountId, roomId, isUnread) =>
    set((state) => {
      const next = new Map(state.optimisticUnread);
      next.set(unreadOverrideKey(accountId, roomId), isUnread);
      return { optimisticUnread: next };
    }),
  clearOptimisticUnread: (accountId, roomId) =>
    set((state) => {
      const key = unreadOverrideKey(accountId, roomId);
      if (!state.optimisticUnread.has(key)) {
        return {};
      }
      const next = new Map(state.optimisticUnread);
      next.delete(key);
      return { optimisticUnread: next };
    }),
  selectRoom: (selection) => set({ selected: selection }),
  requestFocus: (focus) =>
    set({ selected: { accountId: focus.accountId, roomId: focus.roomId }, focusEvent: focus }),
  clearFocus: () => set({ focusEvent: null }),
  // `selected` is deliberately preserved across an inbox `clear()` so refreshing
  // the streamed window (a Reset) does not close the open conversation.
  // Selection is reset explicitly via `selectRoom(null)` (e.g. on sign-out).
  clear: () => set({ rooms: [], total: null, optimisticUnread: new Map<string, boolean>() }),
}));

/**
 * React selector hook over {@link roomsStore}. Pass a selector to subscribe to
 * just the slice a component needs.
 */
export function useRoomsStore<T>(selector: (state: RoomsState) => T): T {
  return useStore(roomsStore, selector);
}
