/**
 * Pins strip (FR-22, UX-DR4, Story 4.3).
 *
 * A horizontal strip of 44 px circular room avatars rendered atop the Inbox view,
 * one per pinned room in the Rust-authoritative order (the {@link pinsRoomsStore}
 * mirror). Clicking an avatar selects the room; a per-avatar context menu offers
 * "Unpin". Native HTML5 drag reorders the avatars — the drop dispatches
 * {@link reorderPins} with the new full order and the authoritative order arrives
 * back over the stream (there is NO optimistic membership/order overlay; only an
 * ephemeral in-component preview during the drag, cleared on drop).
 *
 * The strip overflows horizontally (`overflow-x-auto`, no wrap, no growth) so 9+
 * pins scroll rather than wrapping. It is hidden entirely when there are no pins.
 */
import { useState } from "react";
import { RoomAvatar } from "@/components/chat/RoomAvatar";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import type { InboxRoomVm } from "@/lib/ipc/client";
import { reorderPins, unpinRoom } from "@/lib/ipc/client";
import type { RoomSelection } from "@/lib/stores/rooms";

interface PinsStripProps {
  /** The Pins window, in Rust-authoritative order. */
  pins: InboxRoomVm[];
  /** Select callback; receives the pinned room's account + room ids. */
  onSelect?: (selection: RoomSelection) => void;
  /** The currently open conversation, to mark the active pin. */
  selected?: RoomSelection | null;
  /**
   * Whether drag-to-reorder is enabled. Reorder rewrites the *full* pin set to a
   * contiguous order, so it is only sound when `pins` is the complete set. While an
   * account switcher filter is active `pins` is a filtered subset, so reordering it
   * would submit a partial order and collide with the hidden pins' orders — drag is
   * therefore disabled while filtered (default `true`).
   */
  reorderable?: boolean;
}

export function PinsStrip({ pins, onSelect, selected, reorderable = true }: PinsStripProps) {
  // Ephemeral drag state: the index being dragged. Cleared on drop/end. The
  // authoritative order always arrives via the stream — this only styles the
  // in-flight avatar during the gesture.
  const [dragIndex, setDragIndex] = useState<number | null>(null);

  // Hidden entirely when empty (UX-DR4): no strip, no border, no label.
  if (pins.length === 0) {
    return null;
  }

  const onDrop = (targetIndex: number) => {
    // Ignore no-op drops, and any drop while reorder is disabled (filtered view).
    if (!reorderable || dragIndex === null || dragIndex === targetIndex) {
      setDragIndex(null);
      return;
    }
    // Guard against a stream Reset that shrank or replaced `pins` between drag-start
    // and drop: stale indices would splice the wrong (or an undefined) element and
    // then throw while reading `accountId` of `undefined`.
    if (
      dragIndex < 0 ||
      dragIndex >= pins.length ||
      targetIndex < 0 ||
      targetIndex >= pins.length
    ) {
      setDragIndex(null);
      return;
    }
    // Compute the new full order by moving the dragged pin to the drop position.
    const next = [...pins];
    const [moved] = next.splice(dragIndex, 1);
    if (moved === undefined) {
      setDragIndex(null);
      return;
    }
    next.splice(targetIndex, 0, moved);
    setDragIndex(null);
    // Dispatch the authoritative reorder; the stream reflects it back. Best-effort.
    void reorderPins(
      next.map((room) => ({ accountId: room.accountId, roomId: room.roomId })),
    ).catch(() => {});
  };

  return (
    <div className="shrink-0 border-border border-b">
      <ul
        aria-label="Pinned conversations"
        className="flex flex-nowrap items-center gap-2 overflow-x-auto p-2"
      >
        {pins.map((room, index) => {
          const isSelected =
            selected?.roomId === room.roomId && selected?.accountId === room.accountId;
          return (
            <li key={`${room.accountId}:${room.roomId}`} className="shrink-0">
              <ContextMenu>
                <ContextMenuTrigger asChild>
                  <button
                    type="button"
                    draggable={reorderable}
                    onDragStart={() => reorderable && setDragIndex(index)}
                    onDragOver={(e) => e.preventDefault()}
                    onDrop={() => onDrop(index)}
                    onDragEnd={() => setDragIndex(null)}
                    onClick={() => onSelect?.({ accountId: room.accountId, roomId: room.roomId })}
                    title={room.displayName}
                    aria-label={`Pinned conversation with ${room.displayName}`}
                    aria-current={isSelected ? "true" : undefined}
                    data-dragging={dragIndex === index ? "true" : undefined}
                    className="rounded-full outline-none focus-visible:ring-2 focus-visible:ring-ring data-[dragging=true]:opacity-50"
                  >
                    <RoomAvatar room={room} size="xl" />
                  </button>
                </ContextMenuTrigger>
                <ContextMenuContent>
                  <ContextMenuItem
                    onSelect={() => {
                      void unpinRoom(room.accountId, room.roomId).catch(() => {});
                    }}
                  >
                    Unpin
                  </ContextMenuItem>
                </ContextMenuContent>
              </ContextMenu>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
