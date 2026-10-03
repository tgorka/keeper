/**
 * Timeline mirror store (AD-9, AD-20).
 *
 * A vanilla zustand store created at module load *outside* React. It holds only
 * the ordered {@link TimelineItemVm} array streamed from Rust for the single
 * open room — a pure mirror of the SDK `Timeline`'s snapshot-then-diff sequence,
 * never a source of truth. `applyBatch` folds each {@link TimelineOp} onto an
 * immutable array by index via the shared {@link applyDiffOp} reducer and
 * **never sorts, re-sorts, or re-orders**. A `Reset` replaces contents wholesale,
 * which is why re-subscribing (StrictMode remount, room re-open) never
 * duplicates items.
 *
 * In an agent room the stream also carries the room's header beside the ops
 * ({@link AgentRoomHeaderVm}). Rust sends it on the first batch and again only
 * when it changed, so a batch without one leaves the last header standing.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type {
  AgentRoomHeaderVm,
  TimelineBatch,
  TimelineItemVm,
  TimelineOp,
} from "@/lib/ipc/client";
import { applyDiffOp } from "@/lib/stores/vector-diff";

/**
 * Fold a single op onto `items`, returning a new array (immutable). Delegates to
 * the shared, range-guarded {@link applyDiffOp} reducer — pure, and never sorts.
 * `TimelineOp` (its single-item ops carry `item`, list ops carry `items`) is
 * assignable to the reducer's canonical `DiffOp` union.
 */
function applyOp(items: TimelineItemVm[], op: TimelineOp): TimelineItemVm[] {
  return applyDiffOp(items, op);
}

export interface TimelineState {
  /** The ordered timeline, exactly as Rust streamed it. */
  items: TimelineItemVm[];
  /** The agent room's header, or `null` in every other room. */
  header: AgentRoomHeaderVm | null;
  /** Apply one streamed batch (its ops in sequence). */
  applyBatch: (batch: TimelineBatch) => void;
  /** Reset to the empty state (on room change / unsubscribe). */
  clear: () => void;
}

/**
 * The vanilla store instance. Created once at module load, shared across the
 * app; the source of truth for timeline state stays in Rust.
 */
export const timelineStore = createStore<TimelineState>()((set) => ({
  items: [],
  header: null,
  applyBatch: (batch) =>
    set((state) => ({
      items: batch.ops.reduce(applyOp, state.items),
      header: batch.header ?? state.header,
    })),
  clear: () => set({ items: [], header: null }),
}));

/**
 * React selector hook over {@link timelineStore}. Pass a selector to subscribe
 * to just the slice a component needs.
 */
export function useTimelineStore<T>(selector: (state: TimelineState) => T): T {
  return useStore(timelineStore, selector);
}
