/**
 * A single 64 px chat-list row (UX-DR3, Story 2.1, Story 4.1).
 *
 * Full-width, keyboard-operable `<button>` showing a 3 px per-account hue edge
 * bar, the room avatar, display name, last-message preview, and timestamp.
 * Selecting it (click / Enter / Space) records its `{ accountId, roomId }` via
 * `onSelect`; the selected row is highlighted and marked `aria-current`. Carries
 * a visible focus ring and an accessible label. The hue index comes from Rust
 * (per account) and maps to a CSS `--account-hue-N` variable — no color value is
 * hardcoded here.
 *
 * Unread state (Story 4.1) is authoritative from Rust: `isUnread` bolds the name
 * and shows a neutral dot; `mentionCount > 0` shows a filled primary mention
 * badge instead. The effective unread is `effectiveIsUnread` — an optimistic
 * overlay lets the row flip within one frame when the user picks a context-menu
 * action, then the streamed VM reconciles it. The right-click context menu offers
 * a single "Mark read" / "Mark unread" item that sets the overlay then round-trips
 * to the server (best-effort — a rejection is swallowed, the stream is truth).
 *
 * A second context-menu item (Story 4.2) archives / unarchives the row via the
 * low-priority tag: "Archive" when `!isArchived`, "Unarchive" otherwise. This is
 * best-effort with NO optimistic overlay — the row's move between the Inbox and
 * Archive windows is Rust-authoritative filtering (AD-20), so it waits on the tag
 * round-trip; a rejection is swallowed.
 *
 * A third context-menu item (Story 4.3) pins / unpins the row: "Pin" when
 * `!isPinned`, "Unpin" otherwise. Pins are keeper-local; the row's move into the
 * Pins strip is likewise Rust-authoritative with NO optimistic overlay.
 */

import { RoomAvatar } from "@/components/chat/RoomAvatar";
import { Badge } from "@/components/ui/badge";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuSeparator,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { accountHueVar } from "@/lib/account-hue";
import { formatRoomTimestamp } from "@/lib/format-time";
import type { InboxRoomVm } from "@/lib/ipc/client";
import {
  archiveRoom,
  markRoomRead,
  markRoomUnread,
  pinRoom,
  unarchiveRoom,
  unpinRoom,
} from "@/lib/ipc/client";
import { effectiveIsUnread, type RoomSelection, useRoomsStore } from "@/lib/stores/rooms";
import { cn } from "@/lib/utils";

interface ChatRowProps {
  room: InboxRoomVm;
  /** Optional selection callback; receives the row's account + room ids. */
  onSelect?: (selection: RoomSelection) => void;
  /** Whether this row is the currently open conversation. */
  selected?: boolean;
}

export function ChatRow({ room, onSelect, selected = false }: ChatRowProps) {
  const timestamp = room.timestamp === null ? null : formatRoomTimestamp(room.timestamp) || null;

  // Unread state is authoritative from Rust; the overlay only lets the row lead
  // the stream by a frame after a context-menu action (Story 4.1).
  const optimisticUnread = useRoomsStore((s) => s.optimisticUnread);
  const setOptimisticUnread = useRoomsStore((s) => s.setOptimisticUnread);
  const clearOptimisticUnread = useRoomsStore((s) => s.clearOptimisticUnread);
  const isUnread = effectiveIsUnread(room, optimisticUnread);
  // The mention badge is gated on the *effective* unread so an optimistic "Mark
  // read" clears the badge in the same frame it un-bolds the name (a read row
  // never carries a mention badge).
  const showMention = isUnread && room.mentionCount > 0;
  const mentionCount = room.mentionCount;

  const onMarkRead = () => {
    // Optimistic within-one-frame flip, then round-trip. On a hard rejection
    // (unknown room / inactive account — dispatch failures are swallowed in the
    // core) drop the override so the row reverts to the authoritative stream
    // rather than stranding a phantom-read overlay the stream never reconciles.
    setOptimisticUnread(room.accountId, room.roomId, false);
    void markRoomRead(room.accountId, room.roomId).catch(() =>
      clearOptimisticUnread(room.accountId, room.roomId),
    );
  };
  const onMarkUnread = () => {
    setOptimisticUnread(room.accountId, room.roomId, true);
    void markRoomUnread(room.accountId, room.roomId).catch(() =>
      clearOptimisticUnread(room.accountId, room.roomId),
    );
  };
  // Archive/unarchive are best-effort with no optimistic overlay (Story 4.2): row
  // membership between the Inbox and Archive windows is Rust-authoritative
  // filtering (AD-20), so the visible move waits on the tag round-trip. A rejection
  // is swallowed — the stream is truth.
  const onArchive = () => {
    void archiveRoom(room.accountId, room.roomId).catch(() => {});
  };
  const onUnarchive = () => {
    void unarchiveRoom(room.accountId, room.roomId).catch(() => {});
  };
  // Pin/unpin are best-effort with no optimistic overlay (Story 4.3): the row's
  // move into the Pins strip is Rust-authoritative (AD-20). A rejection is
  // swallowed — the stream is truth.
  const onPin = () => {
    void pinRoom(room.accountId, room.roomId).catch(() => {});
  };
  const onUnpin = () => {
    void unpinRoom(room.accountId, room.roomId).catch(() => {});
  };

  // Accessible unread cue for the row button's name (the visual dot is
  // aria-hidden and the badge sits outside the button's accessible name), gated
  // on the same effective-unread state the visuals use.
  const unreadLabel = !isUnread
    ? ""
    : showMention
      ? `, ${mentionCount} unread ${mentionCount === 1 ? "mention" : "mentions"}`
      : ", unread";

  return (
    <ContextMenu>
      <ContextMenuTrigger asChild>
        <button
          type="button"
          onClick={() => onSelect?.({ accountId: room.accountId, roomId: room.roomId })}
          aria-label={`Conversation with ${room.displayName}${unreadLabel}`}
          aria-current={selected ? "true" : undefined}
          className={cn(
            "relative flex h-16 w-full shrink-0 items-center gap-3 py-0 pr-3 pl-4 text-left",
            "outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
            selected ? "bg-accent" : "hover:bg-accent",
          )}
        >
          {/* 3 px per-account hue edge bar (UX-DR3). Decorative — the account
              attribution is conveyed by the row's conversation content. */}
          <span
            aria-hidden="true"
            data-testid="account-hue-bar"
            className="absolute inset-y-0 left-0 w-[3px]"
            style={{ backgroundColor: accountHueVar(room.hueIndex) }}
          />
          <RoomAvatar room={room} size="lg" />
          <div className="flex min-w-0 flex-1 flex-col">
            <div className="flex items-baseline justify-between gap-2">
              <span className={cn("truncate text-sm", isUnread ? "font-semibold" : "font-medium")}>
                {room.displayName}
              </span>
              {timestamp !== null && (
                <span className="shrink-0 text-muted-foreground text-xs">{timestamp}</span>
              )}
            </div>
            <div className="flex items-center justify-between gap-2">
              <span className="truncate text-muted-foreground text-sm">
                {room.lastMessage ?? ""}
              </span>
              {/* Unread affordance (UX-DR3): a filled primary mention badge with
                  the count, else a neutral dot for any other unread, else nothing. */}
              {showMention ? (
                <Badge variant="default" data-testid="mention-badge" aria-hidden="true">
                  {mentionCount}
                </Badge>
              ) : isUnread ? (
                <span
                  aria-hidden="true"
                  data-testid="unread-dot"
                  className="size-2 shrink-0 rounded-full bg-muted-foreground"
                />
              ) : null}
            </div>
          </div>
        </button>
      </ContextMenuTrigger>
      <ContextMenuContent>
        {isUnread ? (
          <ContextMenuItem onSelect={onMarkRead}>Mark read</ContextMenuItem>
        ) : (
          <ContextMenuItem onSelect={onMarkUnread}>Mark unread</ContextMenuItem>
        )}
        <ContextMenuSeparator />
        {room.isArchived ? (
          <ContextMenuItem onSelect={onUnarchive}>Unarchive</ContextMenuItem>
        ) : (
          <ContextMenuItem onSelect={onArchive}>Archive</ContextMenuItem>
        )}
        {room.isPinned ? (
          <ContextMenuItem onSelect={onUnpin}>Unpin</ContextMenuItem>
        ) : (
          <ContextMenuItem onSelect={onPin}>Pin</ContextMenuItem>
        )}
      </ContextMenuContent>
    </ContextMenu>
  );
}
