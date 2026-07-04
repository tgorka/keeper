/**
 * Room-list mirror store (AD-9, AD-20).
 *
 * A vanilla zustand store created at module load *outside* React. It holds only
 * the ordered {@link RoomVm} array streamed from Rust plus the known total — it
 * is a pure mirror of the recency-sorted `VectorDiff` sequence, never a source
 * of truth. `applyBatch` folds each {@link RoomListOp} onto an immutable array
 * by index and **never sorts, re-sorts, or re-orders** (ordering is authoritative
 * from Rust). A `Reset` replaces contents wholesale, which is why re-subscribing
 * (e.g. StrictMode remount) never duplicates rows.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { RoomListBatch, RoomListOp, RoomVm } from "@/lib/ipc/client";
import { applyDiffOp } from "@/lib/stores/vector-diff";

export interface RoomsState {
  /** The ordered room window, exactly as Rust streamed it. */
  rooms: RoomVm[];
  /** Total rooms the server knows about, or `null` when not yet loaded. */
  total: number | null;
  /**
   * The currently selected room id, or `null` when none is open. Ephemeral UI
   * state (not mirrored from Rust) — it only records which room the conversation
   * pane should stream; the timeline itself stays authoritative in Rust.
   */
  selectedRoomId: string | null;
  /** Apply one streamed batch (its ops in sequence), updating `total`. */
  applyBatch: (batch: RoomListBatch) => void;
  /** Select a room to open (or `null` to close). */
  selectRoom: (roomId: string | null) => void;
  /** Reset to the empty state (on unsubscribe / account change). */
  clear: () => void;
}

/**
 * Fold a single op onto `rooms`, returning a new array (immutable). Delegates to
 * the shared, range-guarded {@link applyDiffOp} reducer — pure, and never sorts.
 * `RoomListOp` (its single-item ops carry `room`, list ops carry `rooms`) is
 * assignable to the reducer's canonical `DiffOp` union.
 */
function applyOp(rooms: RoomVm[], op: RoomListOp): RoomVm[] {
  return applyDiffOp(rooms, op);
}

/**
 * The vanilla store instance. Created once at module load, shared across the
 * app; the source of truth for room state stays in Rust.
 */
export const roomsStore = createStore<RoomsState>()((set) => ({
  rooms: [],
  total: null,
  selectedRoomId: null,
  applyBatch: (batch) =>
    set((state) => ({
      rooms: batch.ops.reduce(applyOp, state.rooms),
      total: batch.total ?? state.total,
    })),
  selectRoom: (roomId) => set({ selectedRoomId: roomId }),
  // `selectedRoomId` is deliberately preserved across a room-list `clear()` so
  // refreshing the streamed window (a Reset) does not close the open
  // conversation. Selection is reset explicitly via `selectRoom(null)` (e.g. on
  // sign-out, Story 1.8); the conversation pane independently clears its own
  // rendered timeline when the account goes away.
  clear: () => set({ rooms: [], total: null }),
}));

/**
 * React selector hook over {@link roomsStore}. Pass a selector to subscribe to
 * just the slice a component needs.
 */
export function useRoomsStore<T>(selector: (state: RoomsState) => T): T {
  return useStore(roomsStore, selector);
}
