/**
 * Selected-room view-model lookup (Story 4.6).
 *
 * The conversation header needs the selected room's full {@link InboxRoomVm} (for
 * its avatar + Network badge + display name), but the selection is only a
 * `{ accountId, roomId }` pair in {@link roomsStore}. This hook subscribes the four
 * window stores (Inbox / Pins / Favorites / Archive) — which stream full windows
 * today — plus the selection, and returns the matching row from whichever window it
 * currently lives in, or `null` when the selection is unset or the room is not in
 * any window (a future true-windowing or filter-hidden case). The header then
 * degrades gracefully to the account-initial chip alone.
 */
import { useMemo } from "react";
import type { InboxRoomVm } from "@/lib/ipc/client";
import { useArchiveRoomsStore } from "@/lib/stores/archive-rooms";
import { useFavoritesRoomsStore } from "@/lib/stores/favorites-rooms";
import { usePinsRoomsStore } from "@/lib/stores/pins-rooms";
import { useRoomsStore } from "@/lib/stores/rooms";

/**
 * Resolve the selected conversation's {@link InboxRoomVm} across the four window
 * stores, or `null` when none is selected or the room is not currently in any
 * streamed window. Never re-derives or re-sorts — a pure lookup by the
 * `{ accountId, roomId }` selection.
 */
export function useSelectedRoomVm(): InboxRoomVm | null {
  const selected = useRoomsStore((s) => s.selected);
  const inboxRooms = useRoomsStore((s) => s.rooms);
  const pinsRooms = usePinsRoomsStore((s) => s.rooms);
  const favoritesRooms = useFavoritesRoomsStore((s) => s.rooms);
  const archiveRooms = useArchiveRoomsStore((s) => s.rooms);

  // Window precedence (inbox → pins → favorites → archive, first match wins) is an
  // intentional, harmless tie-break for the rare transient where a room appears in
  // two windows at once — the same VM either way, so the order only decides which
  // identical row we return.
  return useMemo(() => {
    if (selected === null) {
      return null;
    }
    const match = (room: InboxRoomVm): boolean =>
      room.accountId === selected.accountId && room.roomId === selected.roomId;
    return (
      inboxRooms.find(match) ??
      pinsRooms.find(match) ??
      favoritesRooms.find(match) ??
      archiveRooms.find(match) ??
      null
    );
  }, [selected, inboxRooms, pinsRooms, favoritesRooms, archiveRooms]);
}
