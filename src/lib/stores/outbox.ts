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
import { useEffect } from "react";
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { logFailure } from "@/lib/failure";
import type { HeldRestoreVm, HeldSendVm } from "@/lib/ipc/client";
import { cancelHeldSend } from "@/lib/ipc/client";
import { attachmentId } from "@/lib/stores/attachments";
import { replyPreviewOf } from "@/lib/stores/composer";
import type { ConversationStores } from "@/lib/stores/conversation-stores";

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
  /** A conversation began showing `(accountId, roomId)`'s held sends. */
  hold: (accountId: string, roomId: string) => void;
  /**
   * It stopped: the room's rows are dropped once no conversation shows them — the
   * chat and the notes dock may both show one room, and one closing must not wipe
   * the other's Undo.
   */
  release: (accountId: string, roomId: string) => void;
  /** Reset to the empty state. */
  clear: () => void;
}

/** Conversations showing each room's held sends: not state anyone renders. */
const holders = new Map<string, number>();

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
  hold: (accountId, roomId) => {
    const key = roomKey(accountId, roomId);
    holders.set(key, (holders.get(key) ?? 0) + 1);
  },
  release: (accountId, roomId) =>
    set((state) => {
      const key = roomKey(accountId, roomId);
      const left = (holders.get(key) ?? 1) - 1;
      if (left > 0) {
        holders.set(key, left);
        return state;
      }
      holders.delete(key);
      if (!state.rooms.has(key)) {
        return state;
      }
      const next = new Map(state.rooms);
      next.delete(key);
      return { rooms: next };
    }),
  clear: () => {
    holders.clear();
    waiting.clear();
    set({ rooms: new Map<string, readonly HeldSendVm[]>() });
  },
}));

/** The stores an Undo puts a held send back into: one open conversation's. */
type RestoreStores = Pick<ConversationStores, "composer" | "timeline" | "attachments">;

/**
 * Undo restores waiting for their room to be shown again (Story 101.3, R447),
 * keyed by `` `${accountId} ${roomId}` ``: a cancel that resolves after the
 * person left the room it was pressed in.
 */
const waiting = new Map<string, HeldRestoreVm[]>();
/** The room each open conversation shows, by its stores: not state anyone renders. */
const showing = new Map<RestoreStores, string>();
/** Each shown conversation's take of the restores waiting for its room. */
const drains = new Set<() => void>();

/** How the reply banner names an original the timeline never showed (R447). */
export const REPLY_UNSHOWN_SENDER = "an earlier message";

/** Put `restored` back into `stores`, the conversation showing the room it was held in. */
function putBack(
  accountId: string,
  roomId: string,
  restored: HeldRestoreVm,
  stores: RestoreStores,
): void {
  const composer = stores.composer.getState();
  if (restored.line.length > 0) {
    composer.restore(accountId, roomId, restored.line);
  }
  const reply = restored.reply;
  if (reply !== null) {
    // Named by its event, so the draft is still a reply even when the original is
    // outside the loaded timeline (R447); the key is only this context's identity.
    composer.startReply({
      targetKey: reply.key ?? `event:${reply.eventId}`,
      eventId: reply.eventId,
      sender: reply.senderDisplayName ?? (reply.sender || REPLY_UNSHOWN_SENDER),
      bodyPreview: replyPreviewOf(reply.body),
    });
  }
  if (restored.attachment !== null) {
    stores.attachments.getState().add({
      id: attachmentId(),
      kind: "path",
      path: restored.attachment.path,
      filename: restored.attachment.filename,
    });
  }
}

/**
 * Undo a held send (Story 8.3/8.4): the single shared effect behind BOTH the
 * undo-send pill's Undo and the timeline Delete affordance on a held bubble. Cancels
 * the durable `outbox` row via {@link cancelHeldSend} (zero network activity) and
 * puts back what it held — the line as typed into the composer, a held reply's
 * reply context (named by its event, loaded or not) and a held attachment's staged
 * file into the tray (Story 101.3, R432) — into the room it was held in, and only
 * there (R447): into `stores`, the conversation the undo was pressed in, while it
 * still shows that room; otherwise it waits for the next conversation that shows
 * that room ({@link useHeldRestores}). A cancel resolving after a room switch never
 * lands in the room now open, so the next Send there carries nothing of it.
 *
 * `cancelHeldSend` is idempotent in Rust — an already-dispatched (window-elapsed) row
 * comes back empty, so nothing is restored. Errors are logged and swallowed (mirrors
 * the pill): a failed cancel never crashes the affordance and never restores stale
 * text. Keeping this in one place stops the pill-undo and delete-undo from drifting
 * apart.
 */
export async function undoHeldSend(
  accountId: string,
  roomId: string,
  id: string,
  stores: RestoreStores,
): Promise<void> {
  const restored = await cancelHeldSend(accountId, roomId, id).catch((raw: unknown) => {
    logFailure("outbox.cancelHeldSend")(raw);
    return null;
  });
  if (
    restored === null ||
    (restored.line.length === 0 && restored.reply === null && restored.attachment === null)
  ) {
    return;
  }
  const key = roomKey(accountId, roomId);
  if (showing.get(stores) === key) {
    putBack(accountId, roomId, restored, stores);
    return;
  }
  waiting.set(key, [...(waiting.get(key) ?? []), restored]);
  for (const drain of [...drains]) {
    drain();
  }
}

/**
 * A conversation shows `(accountId, roomId)` in `stores` (Story 101.3, R447): it
 * receives the Undo restores held for that room — those that resolve while it shows
 * it, and those that resolved while no conversation did. Call it after the effect
 * that clears the conversation's composer and tray on a room switch, so what it
 * puts back is not cleared away.
 */
export function useHeldRestores(
  accountId: string | null,
  roomId: string | null,
  stores: RestoreStores,
): void {
  useEffect(() => {
    if (accountId === null || roomId === null) {
      return;
    }
    const key = roomKey(accountId, roomId);
    showing.set(stores, key);
    const drain = () => {
      const restores = waiting.get(key);
      if (restores === undefined) {
        return;
      }
      waiting.delete(key);
      for (const restored of restores) {
        putBack(accountId, roomId, restored, stores);
      }
    };
    drain();
    drains.add(drain);
    return () => {
      drains.delete(drain);
      if (showing.get(stores) === key) {
        showing.delete(stores);
      }
    };
  }, [accountId, roomId, stores]);
}

/**
 * React selector hook: the held sends for `(accountId, roomId)`, oldest-first, or a
 * stable empty array when the Chat has none. Subscribes to just that one key's rows so
 * an unrelated room's held-send change never re-renders this Chat.
 */
export function useHeldSends(accountId: string, roomId: string): readonly HeldSendVm[] {
  return useStore(outboxStore, (state) => state.rooms.get(roomKey(accountId, roomId)) ?? EMPTY);
}
