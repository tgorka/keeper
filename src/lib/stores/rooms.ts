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

export interface RoomsState {
  /** The merged inbox window, exactly as Rust streamed it (recency order). */
  rooms: InboxRoomVm[];
  /** Total rooms across all accounts the servers know about, or `null`. */
  total: number | null;
  /**
   * The currently selected conversation, or `null` when none is open. Ephemeral
   * UI state (not mirrored from Rust) — it only records which room the
   * conversation pane should stream, on which account.
   */
  selected: RoomSelection | null;
  /** Apply one streamed batch (its ops in sequence), updating `total`. */
  applyBatch: (batch: InboxBatch) => void;
  /** Select a conversation to open (or `null` to close). */
  selectRoom: (selection: RoomSelection | null) => void;
  /** Reset to the empty state (on unsubscribe / full sign-out). */
  clear: () => void;
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
  selected: null,
  applyBatch: (batch) =>
    set((state) => ({
      rooms: batch.ops.reduce(applyOp, state.rooms),
      total: batch.total ?? state.total,
    })),
  selectRoom: (selection) => set({ selected: selection }),
  // `selected` is deliberately preserved across an inbox `clear()` so refreshing
  // the streamed window (a Reset) does not close the open conversation.
  // Selection is reset explicitly via `selectRoom(null)` (e.g. on sign-out).
  clear: () => set({ rooms: [], total: null }),
}));

/**
 * React selector hook over {@link roomsStore}. Pass a selector to subscribe to
 * just the slice a component needs.
 */
export function useRoomsStore<T>(selector: (state: RoomsState) => T): T {
  return useStore(roomsStore, selector);
}
