/**
 * Agents-window mirror store.
 *
 * A vanilla zustand store created at module load *outside* React. It holds only
 * the recency-ordered {@link InboxRoomVm} array streamed from the Rust
 * `keeper-core::inbox` merge's **Agents** window — every agent session room, and
 * no other room (control rooms are in no window at all) — plus its total. A pure
 * mirror: `applyBatch` folds each {@link InboxOp} by index and never re-orders.
 *
 * Like the archive store it carries no optimistic overlay and no selection;
 * selection stays single-source in `roomsStore` for every window.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { InboxBatch, InboxRoomVm } from "@/lib/ipc/client";
import { applyDiffOp } from "@/lib/stores/vector-diff";

export interface AgentRoomsState {
  /** The Agents window, exactly as Rust streamed it (recency order). */
  rooms: InboxRoomVm[];
  /** Number of rooms in the streamed Agents window, or `null` before the first batch. */
  total: number | null;
  /**
   * Rust's sentence for why some rows are not live (kept from the last run
   * because an account could not start, or the kept list could not be read
   * or kept — story 102.5, R501), or `null`. Every Agents batch says it again.
   */
  note: string | null;
  /** Apply one streamed batch (its ops in sequence), updating `total` and `note`. */
  applyBatch: (batch: InboxBatch) => void;
  /** Reset to the empty state (on unsubscribe / full sign-out). */
  clear: () => void;
}

export const agentRoomsStore = createStore<AgentRoomsState>()((set) => ({
  rooms: [],
  total: null,
  note: null,
  applyBatch: (batch) =>
    set((state) => ({
      rooms: batch.ops.reduce<InboxRoomVm[]>(applyDiffOp, state.rooms),
      total: batch.total ?? state.total,
      note: batch.note ?? null,
    })),
  clear: () => set({ rooms: [], total: null, note: null }),
}));

/** React selector hook over {@link agentRoomsStore}. */
export function useAgentRoomsStore<T>(selector: (state: AgentRoomsState) => T): T {
  return useStore(agentRoomsStore, selector);
}
