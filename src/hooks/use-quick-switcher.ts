/**
 * Quick-switcher chord (Story 9.2).
 *
 * Wires `⌃Tab` → next chat and `⌃⇧Tab` → previous chat, cycling the open
 * conversation over the currently rendered, Rust-recency-ordered window (the inbox
 * window when `primaryView === "inbox"`, the archive window when `"archive"`),
 * honoring the active account-switcher display filter. It only moves a cursor over
 * the array Rust already ordered — never sorts, re-sorts, or re-derives order in TS
 * (AD-20). Selecting a chat opens it (`selectRoom`) and drops focus into the
 * composer (`composerStore.requestFocus`). No-op on the bridges/approval views or an
 * empty list. Follows the app's ad-hoc `window` keydown shortcut pattern; `⌃` is the
 * intended modifier (not `meta`), so there is no non-mac parity branch. IME-guarded
 * and `preventDefault`s so the webview never acts on the chord.
 */
import { useEffect } from "react";
import { renderedWindowRooms } from "@/lib/rendered-window";
import { accountsStore } from "@/lib/stores/accounts";
import { archiveRoomsStore } from "@/lib/stores/archive-rooms";
import { composerStore } from "@/lib/stores/composer";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";

export function useQuickSwitcher(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.isComposing) {
        return;
      }
      // The quick-switcher rides `⌃Tab` / `⌃⇧Tab` only — a bare Tab must stay
      // native focus traversal, and `⌘Tab` is the OS app switcher.
      if (event.key !== "Tab" || !event.ctrlKey || event.metaKey || event.altKey) {
        return;
      }
      const rooms = renderedWindowRooms(
        primaryViewStore.getState().view,
        roomsStore.getState().rooms,
        archiveRoomsStore.getState().rooms,
        accountsStore.getState().filterAccountId,
      );
      // No-op on the bridges/approval views (which replace the cluster) or an empty
      // list — nothing to cycle.
      if (rooms === null || rooms.length === 0) {
        return;
      }
      event.preventDefault();
      const selected = roomsStore.getState().selected;
      const currentIndex = selected
        ? rooms.findIndex(
            (room) => room.accountId === selected.accountId && room.roomId === selected.roomId,
          )
        : -1;
      // Cycle forward on Tab, backward on ⇧Tab; wrap. When nothing (or a room
      // outside the window) is currently selected, Tab opens the first row and
      // ⇧Tab opens the last.
      const delta = event.shiftKey ? -1 : 1;
      const base = currentIndex === -1 ? (event.shiftKey ? 0 : -1) : currentIndex;
      const nextIndex = (base + delta + rooms.length) % rooms.length;
      const next = rooms[nextIndex];
      roomsStore.getState().selectRoom({ accountId: next.accountId, roomId: next.roomId });
      composerStore.getState().requestFocus();
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
