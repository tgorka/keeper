/**
 * Next/previous-unread jump chord (Story 9.2).
 *
 * Wires `⌥⌘↓` → next unread and `⌥⌘↑` → previous unread (⌥⌃ for non-mac parity),
 * scanning the currently rendered, Rust-recency-ordered window (inbox when
 * `primaryView === "inbox"`, archive when `"archive"`, honoring the account-switcher
 * display filter) for the next/previous {@link effectiveIsUnread} row after/before
 * the current selection, wrapping. The matched row is selected + opened
 * (`selectRoom`) and focus lands in the composer (`composerStore.requestFocus`).
 * No-op when there are no unread rows, or on the bridges/approval views / an empty
 * list. Only moves a cursor over the array Rust already ordered — never sorts or
 * re-derives order in TS (AD-20). Follows the app's ad-hoc `window` keydown pattern;
 * IME-guarded and `preventDefault`s.
 */
import { useEffect } from "react";
import { renderedWindowRooms } from "@/lib/rendered-window";
import { accountsStore } from "@/lib/stores/accounts";
import { archiveRoomsStore } from "@/lib/stores/archive-rooms";
import { composerStore } from "@/lib/stores/composer";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { effectiveIsUnread, roomsStore } from "@/lib/stores/rooms";

export function useUnreadJump(): void {
  useEffect(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.isComposing) {
        return;
      }
      // `⌥⌘↓` / `⌥⌘↑` (⌥⌃ for non-mac parity). Alt must be held together with ⌘ or
      // ⌃; a bare ↑/↓ carries no modifier and falls through to list/timeline nav.
      const mod = event.metaKey || event.ctrlKey;
      if (!event.altKey || !mod || (event.key !== "ArrowDown" && event.key !== "ArrowUp")) {
        return;
      }
      const rooms = renderedWindowRooms(
        primaryViewStore.getState().view,
        roomsStore.getState().rooms,
        archiveRoomsStore.getState().rooms,
        accountsStore.getState().filterAccountId,
      );
      if (rooms === null || rooms.length === 0) {
        return;
      }
      const optimisticUnread = roomsStore.getState().optimisticUnread;
      // No unread rows in the window → no-op: leave the selection untouched and do
      // NOT preventDefault (there is nothing to jump to, so we don't claim the
      // chord), matching the I/O matrix's "selection unchanged" for the empty case.
      if (!rooms.some((room) => effectiveIsUnread(room, optimisticUnread))) {
        return;
      }
      event.preventDefault();
      const selected = roomsStore.getState().selected;
      const currentIndex = selected
        ? rooms.findIndex(
            (room) => room.accountId === selected.accountId && room.roomId === selected.roomId,
          )
        : -1;
      const forward = event.key === "ArrowDown";
      const count = rooms.length;
      // Scan forward/backward from just past the current selection, wrapping, for
      // the next row whose effective-unread is set. Guaranteed to find one (checked
      // above). When nothing is selected, start the scan so the first step lands on
      // index 0 forward / the last index backward.
      const start = currentIndex === -1 ? (forward ? -1 : count) : currentIndex;
      for (let step = 1; step <= count; step += 1) {
        const index = (((start + (forward ? step : -step)) % count) + count) % count;
        const room = rooms[index];
        if (effectiveIsUnread(room, optimisticUnread)) {
          roomsStore.getState().selectRoom({ accountId: room.accountId, roomId: room.roomId });
          composerStore.getState().requestFocus();
          return;
        }
      }
    };
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);
}
