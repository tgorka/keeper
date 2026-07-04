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

export interface RoomsState {
  /** The ordered room window, exactly as Rust streamed it. */
  rooms: RoomVm[];
  /** Total rooms the server knows about, or `null` when not yet loaded. */
  total: number | null;
  /** Apply one streamed batch (its ops in sequence), updating `total`. */
  applyBatch: (batch: RoomListBatch) => void;
  /** Reset to the empty state (on unsubscribe / account change). */
  clear: () => void;
}

/**
 * Fold a single op onto `rooms`, returning a new array (immutable). Pure: no
 * network, no derivation of truth, and — critically — no sorting.
 */
function applyOp(rooms: RoomVm[], op: RoomListOp): RoomVm[] {
  switch (op.op) {
    case "reset":
      return [...op.rooms];
    case "append":
      return [...rooms, ...op.rooms];
    case "clear":
      return [];
    case "pushFront":
      return [op.room, ...rooms];
    case "pushBack":
      return [...rooms, op.room];
    case "popFront":
      return rooms.slice(1);
    case "popBack":
      return rooms.slice(0, -1);
    case "insert": {
      if (op.index < 0 || op.index > rooms.length) {
        return rooms;
      }
      const next = [...rooms];
      next.splice(op.index, 0, op.room);
      return next;
    }
    case "set": {
      if (op.index < 0 || op.index >= rooms.length) {
        return rooms;
      }
      const next = [...rooms];
      next[op.index] = op.room;
      return next;
    }
    case "remove": {
      if (op.index < 0 || op.index >= rooms.length) {
        return rooms;
      }
      const next = [...rooms];
      next.splice(op.index, 1);
      return next;
    }
    case "truncate":
      return rooms.slice(0, op.length);
    default: {
      // Exhaustiveness guard: a new op variant must be handled here.
      const _never: never = op;
      return _never;
    }
  }
}

/**
 * The vanilla store instance. Created once at module load, shared across the
 * app; the source of truth for room state stays in Rust.
 */
export const roomsStore = createStore<RoomsState>()((set) => ({
  rooms: [],
  total: null,
  applyBatch: (batch) =>
    set((state) => ({
      rooms: batch.ops.reduce(applyOp, state.rooms),
      total: batch.total ?? state.total,
    })),
  clear: () => set({ rooms: [], total: null }),
}));

/**
 * React selector hook over {@link roomsStore}. Pass a selector to subscribe to
 * just the slice a component needs.
 */
export function useRoomsStore<T>(selector: (state: RoomsState) => T): T {
  return useStore(roomsStore, selector);
}
