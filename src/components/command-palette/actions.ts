/**
 * Command-palette action dispatch map (Story 9.1, epic 9 spine).
 *
 * The action *catalog* (id, title, category, keywords, shortcut, requiresOpenChat)
 * is authored once in Rust (`keeper_core::palette::palette_actions`) and consumed by
 * the palette, cheat sheet, and native menu bar. *Execution* stays in the frontend:
 * this map resolves each action id to a handler that switches a primary view, opens
 * a feature dialog's store, or `invoke`s an existing Tauri command with the open
 * chat's `(accountId, roomId)` context. There is no business logic here — the
 * handlers only route to surfaces already built in epics 1–8.
 *
 * A handler returns `void` or a `Promise<void>`; open-chat handlers receive the
 * currently selected chat context (never `null` — the palette only offers open-chat
 * actions when a chat is open). Ranking/filtering is never done here; the Rust
 * `palette_query` is authoritative per query.
 */
import {
  archiveRoom,
  chatNotifyModeSet,
  favoriteRoom,
  incognitoGet,
  incognitoGetGlobal,
  incognitoSetChat,
  incognitoSetGlobal,
  markRoomRead,
  markRoomUnread,
  pinRoom,
  unarchiveRoom,
  unfavoriteRoom,
  unpinRoom,
} from "@/lib/ipc/client";
import { addAccountStore } from "@/lib/stores/add-account";
import { exportStore } from "@/lib/stores/export";
import { newChatStore } from "@/lib/stores/new-chat";
import { primaryViewStore } from "@/lib/stores/primary-view";
import type { RoomSelection } from "@/lib/stores/rooms";
import { searchStore } from "@/lib/stores/search";

/** The open-chat context an open-chat action operates on. */
export type PaletteActionContext = RoomSelection;

/**
 * A single action handler. Global actions ignore `context`; open-chat actions read
 * the open chat's `(accountId, roomId)` from it. Returning a promise lets the
 * palette await async `invoke`s before closing.
 */
export type PaletteActionHandler = (context: PaletteActionContext | null) => void | Promise<void>;

/**
 * The action-id → handler map. Every id here matches an id in the Rust registry
 * (`palette_actions`); a missing handler is a programmer error surfaced at dispatch.
 */
export const paletteActionHandlers: Record<string, PaletteActionHandler> = {
  // --- Navigation (view switches) ---
  "open-inbox": () => primaryViewStore.getState().setView("inbox"),
  "open-archive": () => primaryViewStore.getState().setView("archive"),
  "open-approval": () => primaryViewStore.getState().setView("approval"),
  "open-bridges": () => primaryViewStore.getState().setView("bridges"),

  // --- Global actions (dialogs / commands) ---
  "new-chat": () => newChatStore.getState().open(),
  "open-search": () => searchStore.getState().open("global"),
  "start-export": () =>
    exportStore.getState().open({ scope: "everything", accountId: null, roomId: null }),
  "add-account": () => addAccountStore.getState().openAddAccount(),
  "toggle-incognito-global": async () => {
    const current = await incognitoGetGlobal();
    await incognitoSetGlobal(!current);
  },

  // --- Open-chat actions (operate on the current conversation) ---
  "archive-chat": (ctx) => (ctx ? archiveRoom(ctx.accountId, ctx.roomId) : undefined),
  "unarchive-chat": (ctx) => (ctx ? unarchiveRoom(ctx.accountId, ctx.roomId) : undefined),
  "pin-chat": (ctx) => (ctx ? pinRoom(ctx.accountId, ctx.roomId) : undefined),
  "unpin-chat": (ctx) => (ctx ? unpinRoom(ctx.accountId, ctx.roomId) : undefined),
  "favorite-chat": (ctx) => (ctx ? favoriteRoom(ctx.accountId, ctx.roomId) : undefined),
  "unfavorite-chat": (ctx) => (ctx ? unfavoriteRoom(ctx.accountId, ctx.roomId) : undefined),
  "mark-read": (ctx) => (ctx ? markRoomRead(ctx.accountId, ctx.roomId) : undefined),
  "mark-unread": (ctx) => (ctx ? markRoomUnread(ctx.accountId, ctx.roomId) : undefined),
  "toggle-incognito-chat": async (ctx) => {
    if (ctx === null) {
      return;
    }
    // Read the resolved effective state and set the per-chat override to its
    // inverse — a real toggle, not a blind set. Precedence stays Rust-owned.
    const vm = await incognitoGet(ctx.accountId, ctx.roomId);
    await incognitoSetChat(ctx.accountId, ctx.roomId, !vm.effective);
  },
  // Per-Chat notification mode (Story 10.2): set a synced Matrix push rule. `unmute`
  // resolves to `all` (clears any per-Chat rule). Rust owns persistence + the row glyph.
  "mute-chat": (ctx) => (ctx ? chatNotifyModeSet(ctx.accountId, ctx.roomId, "mute") : undefined),
  "mention-only-chat": (ctx) =>
    ctx ? chatNotifyModeSet(ctx.accountId, ctx.roomId, "mention_only") : undefined,
  "unmute-chat": (ctx) => (ctx ? chatNotifyModeSet(ctx.accountId, ctx.roomId, "all") : undefined),
  "export-chat": (ctx) =>
    ctx
      ? exportStore.getState().open({ scope: "chat", accountId: ctx.accountId, roomId: ctx.roomId })
      : undefined,
};

/**
 * Dispatch an action by id with the open-chat context, routing through the map.
 * Resolves once the handler settles (so the palette can close after an async
 * `invoke`). An unknown id logs and no-ops — the palette never crashes on a stale id.
 */
export async function dispatchPaletteAction(
  id: string,
  context: PaletteActionContext | null,
): Promise<void> {
  const handler = paletteActionHandlers[id];
  if (handler === undefined) {
    // A stale/unknown id (registry drift). Fail soft — never crash the palette.
    console.warn(`command-palette: no handler for action "${id}"`);
    return;
  }
  await handler(context);
}
