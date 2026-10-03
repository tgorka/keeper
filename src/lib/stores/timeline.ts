/**
 * Timeline mirror store (AD-9, AD-20).
 *
 * A vanilla zustand store created *outside* React. It holds only the ordered
 * {@link TimelineItemVm} array streamed from Rust for one open room — a pure
 * mirror of the SDK `Timeline`'s snapshot-then-diff sequence, never a source of
 * truth. `applyBatch` folds each {@link TimelineOp} onto an immutable array by
 * index via the shared {@link applyDiffOp} reducer and **never sorts, re-sorts,
 * or re-orders**. A `Reset` replaces contents wholesale, which is why
 * re-subscribing (StrictMode remount, room re-open) never duplicates items.
 *
 * The chat's open room is {@link timelineStore}; a second open timeline (the
 * notes view's dock) makes its own with {@link createTimelineStore}, so it
 * never disturbs the chat's.
 *
 * In an agent room the stream also carries the room's header beside the ops
 * ({@link AgentRoomHeaderVm}). Rust sends it on the first batch and again only
 * when it changed, so a batch without one leaves the last header standing.
 */
import { createStore, type StoreApi } from "zustand/vanilla";
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

/** A timeline mirror for one open room. */
export function createTimelineStore(): StoreApi<TimelineState> {
  return createStore<TimelineState>()((set) => ({
    items: [],
    header: null,
    applyBatch: (batch) =>
      set((state) => ({
        items: batch.ops.reduce(applyOp, state.items),
        header: batch.header ?? state.header,
      })),
    clear: () => set({ items: [], header: null }),
  }));
}

/** The chat's open room. The source of truth for timeline state stays in Rust. */
export const timelineStore = createTimelineStore();
