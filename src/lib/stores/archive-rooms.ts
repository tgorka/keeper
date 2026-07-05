/**
 * Archive-window mirror store (AD-9, AD-20, Story 4.2).
 *
 * A vanilla zustand store created at module load *outside* React. It holds only
 * the recency-ordered {@link InboxRoomVm} array streamed from the Rust
 * `keeper-core::inbox` merge's **Archive** partition (`is_archived && !is_unread`),
 * plus its total — a pure mirror of that window, never a source of truth.
 * `applyBatch` folds each {@link InboxOp} onto an immutable array by index and
 * **never sorts, re-sorts, or re-orders** (the inbox/archive split and ordering are
 * authoritative from Rust; a `Reset` replaces contents wholesale).
 *
 * Unlike {@link roomsStore} this store carries **no** optimistic-unread overlay
 * (archive rows are read by definition — an unread room auto-returns to the inbox
 * window) and **no** selection (selection stays single-source in `roomsStore` for
 * both views).
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { InboxBatch, InboxOp, InboxRoomVm } from "@/lib/ipc/client";
import { applyDiffOp } from "@/lib/stores/vector-diff";

export interface ArchiveRoomsState {
  /** The Archive window, exactly as Rust streamed it (recency order). */
  rooms: InboxRoomVm[];
  /** Number of rooms in the streamed Archive window (its partition length), or `null`. */
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
 * app; the source of truth for archive state stays in Rust.
 */
export const archiveRoomsStore = createStore<ArchiveRoomsState>()((set) => ({
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
 * React selector hook over {@link archiveRoomsStore}. Pass a selector to
 * subscribe to just the slice a component needs.
 */
export function useArchiveRoomsStore<T>(selector: (state: ArchiveRoomsState) => T): T {
  return useStore(archiveRoomsStore, selector);
}
