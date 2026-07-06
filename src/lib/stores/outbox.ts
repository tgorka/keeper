/**
 * Held-send (Undo-Send) mirror store (Story 8.3, FR-46).
 *
 * A vanilla zustand store created at module load *outside* React. It mirrors the
 * per-Chat held-send snapshots streamed from Rust ({@link subscribeOutbox}), keyed by
 * `` `${accountId} ${roomId}` ``. The durable `outbox` table in `keeper.db` is the
 * source of truth; this store is a pure mirror — the outbox stream emits a **full
 * snapshot per change** (small, low-churn), so each batch REPLACES the room's rows
 * wholesale (never op-folding).
 *
 * It feeds the amber "Held" bubble at the timeline tail and the floating undo-send
 * pill(s) above the composer, both scoped to the open Chat.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { HeldSendVm } from "@/lib/ipc/client";

/** The composite key for a chat: `` `${accountId} ${roomId}` ``. */
function roomKey(accountId: string, roomId: string): string {
  return `${accountId} ${roomId}`;
}

/** A stable empty list so a room with no held sends returns a referentially-stable
 * value (no render churn). */
const EMPTY: readonly HeldSendVm[] = Object.freeze([]);

export interface OutboxState {
  /**
   * Held sends per chat, keyed by `` `${accountId} ${roomId}` `` → the room's rows
   * (oldest-first, as streamed from Rust). A key is present only while the room has
   * at least one held send.
   */
  rooms: ReadonlyMap<string, readonly HeldSendVm[]>;
  /**
   * Replace `(accountId, roomId)`'s held rows with `rows` (Story 8.3). An empty
   * `rows` clears the room's entry entirely. Fed by the outbox subscription's
   * full-snapshot batches.
   */
  applySnapshot: (accountId: string, roomId: string, rows: readonly HeldSendVm[]) => void;
  /** Reset to the empty state (on unsubscribe / Chat close). */
  clear: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const outboxStore = createStore<OutboxState>()((set) => ({
  rooms: new Map<string, readonly HeldSendVm[]>(),
  applySnapshot: (accountId, roomId, rows) =>
    set((state) => {
      const key = roomKey(accountId, roomId);
      const present = state.rooms.get(key);
      // An empty snapshot removes the room's entry; skip the churn if already absent.
      if (rows.length === 0) {
        if (present === undefined) {
          return state;
        }
        const next = new Map(state.rooms);
        next.delete(key);
        return { rooms: next };
      }
      const next = new Map(state.rooms);
      next.set(key, rows);
      return { rooms: next };
    }),
  clear: () => set({ rooms: new Map<string, readonly HeldSendVm[]>() }),
}));

/**
 * React selector hook: the held sends for `(accountId, roomId)`, oldest-first, or a
 * stable empty array when the Chat has none. Subscribes to just that one key's rows so
 * an unrelated room's held-send change never re-renders this Chat.
 */
export function useHeldSends(accountId: string, roomId: string): readonly HeldSendVm[] {
  return useStore(outboxStore, (state) => state.rooms.get(roomKey(accountId, roomId)) ?? EMPTY);
}
