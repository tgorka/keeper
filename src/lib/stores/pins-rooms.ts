/**
 * Pins-window mirror store (AD-9, AD-20, Story 4.3).
 *
 * A vanilla zustand store created at module load *outside* React. It holds only
 * the {@link InboxRoomVm} array streamed from the Rust `keeper-core::inbox`
 * merge's **Pins** partition (pinned rooms, `sort_order` ascending), plus its
 * total — a pure mirror of that window, never a source of truth. `applyBatch`
 * folds each {@link InboxOp} onto an immutable array by index and **never sorts,
 * re-sorts, or re-orders** (pin membership and order are authoritative from Rust;
 * a `Reset` replaces contents wholesale).
 *
 * Like {@link archiveRoomsStore} this store carries **no** optimistic overlay and
 * **no** selection (selection stays single-source in `roomsStore`). A drag reorder
 * shows only an ephemeral in-component preview during the gesture; the
 * authoritative order arrives here via the stream after `reorderPins`.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { InboxBatch, InboxOp, InboxRoomVm } from "@/lib/ipc/client";
import { applyDiffOp } from "@/lib/stores/vector-diff";

export interface PinsRoomsState {
  /** The Pins window, exactly as Rust streamed it (`sort_order` ascending). */
  rooms: InboxRoomVm[];
  /** Number of rooms in the streamed Pins window (its partition length), or `null`. */
  total: number | null;
  /** Apply one streamed batch (its ops in sequence), updating `total`. */
  applyBatch: (batch: InboxBatch) => void;
  /** Reset to the empty state (on unsubscribe / full sign-out). */
  clear: () => void;
}

/**
 * Fold a single op onto `rooms`, returning a new array (immutable). Delegates to
 * the shared, range-guarded {@link applyDiffOp} reducer — pure, and never sorts.
 */
function applyOp(rooms: InboxRoomVm[], op: InboxOp): InboxRoomVm[] {
  return applyDiffOp(rooms, op);
}

/**
 * The vanilla store instance. Created once at module load, shared across the
 * app; the source of truth for pin state stays in Rust.
 */
export const pinsRoomsStore = createStore<PinsRoomsState>()((set) => ({
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
 * React selector hook over {@link pinsRoomsStore}. Pass a selector to subscribe to
 * just the slice a component needs.
 */
export function usePinsRoomsStore<T>(selector: (state: PinsRoomsState) => T): T {
  return useStore(pinsRoomsStore, selector);
}
