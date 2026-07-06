/**
 * Native-menu action listener (Story 9.3, epic 9 spine).
 *
 * The native macOS menu bar is built in Rust from the same action registry the
 * palette consumes; a menu click emits `keeper://menu-action` with the clicked
 * item's canonical id. This hook listens for that event and routes it through the
 * SAME frontend dispatch the palette uses ({@link dispatchPaletteAction}) — there is
 * no second dispatch table.
 *
 * Two resolutions happen here, mirroring the palette's `runAction` and the chat-list
 * verb logic exactly (never re-derived independently):
 *   1. Open-chat context is `roomsStore.selected` (the open conversation, or `null`).
 *      When `null`, a `requires_open_chat` handler already no-ops.
 *   2. A collapsed toggle item emits its canonical (positive) id (e.g.
 *      `archive-chat`); we flip it to the opposite direction (`unarchive-chat`) when
 *      the open room's current flag says it is already in that state — the same
 *      direction the chat row would pick. When no room is open we dispatch the
 *      canonical id unchanged (it no-ops via the null context).
 */

import { listen } from "@tauri-apps/api/event";
import { useEffect } from "react";
import { dispatchPaletteAction } from "@/components/command-palette/actions";
import { effectiveIsUnread, roomsStore } from "@/lib/stores/rooms";

/** Canonical (positive) toggle id → its opposite (negative) direction. */
const TOGGLE_OPPOSITE: Record<string, string> = {
  "archive-chat": "unarchive-chat",
  "pin-chat": "unpin-chat",
  "favorite-chat": "unfavorite-chat",
  "mark-read": "mark-unread",
};

/**
 * Resolve the id to dispatch for a native-menu click: a collapsed toggle item's
 * canonical id is flipped to its opposite when the open room is already in that
 * positive state, exactly as the chat-list verb picks direction. Non-toggle ids and
 * the no-open-room case pass through unchanged. Pure over the current store snapshot.
 */
export function resolveMenuActionId(id: string): string {
  const opposite = TOGGLE_OPPOSITE[id];
  if (opposite === undefined) {
    return id;
  }
  const { selected, rooms, optimisticUnread } = roomsStore.getState();
  if (selected === null) {
    // No open room — dispatch the canonical id; its handler no-ops on null context.
    return id;
  }
  const room = rooms.find(
    (candidate) =>
      candidate.accountId === selected.accountId && candidate.roomId === selected.roomId,
  );
  if (room === undefined) {
    // Open room not in the rendered window (can't read its flag) — leave canonical.
    return id;
  }
  // Flip to the negative direction when the positive state already holds, mirroring
  // the chat-list verbs: `archive-chat`→`unarchive-chat` when archived, etc.
  switch (id) {
    case "archive-chat":
      return room.isArchived ? opposite : id;
    case "pin-chat":
      return room.isPinned ? opposite : id;
    case "favorite-chat":
      return room.isFavourite ? opposite : id;
    case "mark-read":
      // `mark-read` clears unread; when already read, flip to `mark-unread`.
      return effectiveIsUnread(room, optimisticUnread) ? id : opposite;
    default:
      return id;
  }
}

export function useMenuActions(): void {
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    // Registering the native-menu listener is best-effort: outside a Tauri webview
    // (e.g. jsdom in tests, or a future non-desktop port) `listen` is unavailable.
    // A failure just means the menu bridge is inert — it must never crash the shell.
    try {
      void listen<string>("keeper://menu-action", (event) => {
        const id = resolveMenuActionId(event.payload);
        void dispatchPaletteAction(id, roomsStore.getState().selected);
      })
        .then((fn) => {
          // Cleaned up before the listener resolved? Unlisten immediately.
          if (cancelled) {
            fn();
          } else {
            unlisten = fn;
          }
        })
        .catch(() => {
          // No Tauri host — the menu bridge is inert in this environment.
        });
    } catch {
      // `listen` can throw synchronously when the Tauri IPC internals are absent.
    }
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}
