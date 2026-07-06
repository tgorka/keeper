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
 *
 * A fourth context-menu item (Story 4.4) favourites / unfavourites the row:
 * "Favorite" when `!isFavourite`, "Unfavorite" otherwise, via the `m.favourite`
 * notable tag (best-effort, no optimistic overlay). While the user has zero
 * favourites a one-time muted hint (UX-DR13) sits by the Favorite item explaining
 * the section; it disappears once any favourite exists.
 */

import { AtSign, BellOff, Pencil } from "lucide-react";
import { forwardRef } from "react";
import { RoomAvatar } from "@/components/chat/RoomAvatar";
import { Badge } from "@/components/ui/badge";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuLabel,
  ContextMenuRadioGroup,
  ContextMenuRadioItem,
  ContextMenuSeparator,
  ContextMenuSub,
  ContextMenuSubContent,
  ContextMenuSubTrigger,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { accountHueVar } from "@/lib/account-hue";
import { BRIDGE_HEALTH_DOT_CLASS } from "@/lib/bridges";
import { formatRoomTimestamp } from "@/lib/format-time";
import type { ChatNotifyMode, InboxRoomVm } from "@/lib/ipc/client";
import {
  archiveRoom,
  chatNotifyModeSet,
  favoriteRoom,
  markRoomRead,
  markRoomUnread,
  pinRoom,
  unarchiveRoom,
  unfavoriteRoom,
  unpinRoom,
} from "@/lib/ipc/client";
import { useBridgeHealth } from "@/lib/stores/bridge-health";
import { useHasDraft } from "@/lib/stores/drafts";
import { useFavoritesRoomsStore } from "@/lib/stores/favorites-rooms";
import { effectiveIsUnread, type RoomSelection, useRoomsStore } from "@/lib/stores/rooms";
import { cn } from "@/lib/utils";

interface ChatRowProps {
  room: InboxRoomVm;
  /** Optional selection callback; receives the row's account + room ids. */
  onSelect?: (selection: RoomSelection) => void;
  /** Whether this row is the currently open conversation. */
  selected?: boolean;
  /**
   * Roving tabindex driven by the chat-list pane's keyboard navigation
   * (Story 9.2): the keyboard-focused row is `0`, every other row `-1`, so a
   * single Tab lands on the active row and `↑`/`↓`/`j`/`k` move the ring. Omitted
   * on surfaces that don't drive roving focus (the row stays natively focusable).
   */
  tabIndex?: number;
}

/**
 * The row `<button>` forwards its ref so the chat-list pane can imperatively
 * `.focus()` the roving-tabindex row as the keyboard selection moves (Story 9.2).
 */
export const ChatRow = forwardRef<HTMLButtonElement, ChatRowProps>(function ChatRow(
  { room, onSelect, selected = false, tabIndex },
  ref,
) {
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
  // Favorite/unfavorite are best-effort with no optimistic overlay (Story 4.4):
  // favourite state rides the `m.favourite` notable tag, so the row's move into
  // the Favorites section is Rust-authoritative (AD-20). A rejection is swallowed.
  const onFavorite = () => {
    void favoriteRoom(room.accountId, room.roomId).catch(() => {});
  };
  const onUnfavorite = () => {
    void unfavoriteRoom(room.accountId, room.roomId).catch(() => {});
  };
  // Per-Chat notification mode (Story 10.2): a synced Matrix push rule. Best-effort
  // with NO optimistic overlay — the row's mute glyph is Rust-authoritative (resolved
  // at inbox emit from the synced rule + muted-Network set), so it waits on the
  // round-trip; a rejection is swallowed and the stream stays truth.
  const setNotifyMode = (mode: ChatNotifyMode) => {
    void chatNotifyModeSet(room.accountId, room.roomId, mode).catch(() => {});
  };
  // The Notifications radio reflects the durable per-Chat rule. `mention_only` maps to
  // the mention-only radio; both `muted` (a Chat rule OR a muted Network) and `none`
  // otherwise resolve to mute/all — the radio shows "Mute" for `muted`, else "All".
  const notifyRadioValue =
    room.muteState === "mention_only"
      ? "mention_only"
      : room.muteState === "muted"
        ? "mute"
        : "all";
  // One-time discovery hint (UX-DR13): while the user has zero favourites, show a
  // muted helper line by the Favorite item explaining the section. Once any
  // favourite exists (`favouritesTotal > 0`) the hint disappears — no persisted
  // "seen" flag. `total` is the Rust-authoritative Favorites-window length; it is
  // `null` until the first Favorites batch streams in, so the hint stays hidden
  // pre-load (only `0` — a known-empty window — shows it), never flashing for a
  // user who actually has favourites.
  const favoritesTotal = useFavoritesRoomsStore((s) => s.total);
  const showFavoritesHint = !room.isFavourite && favoritesTotal === 0;

  // Affected-row health dot (Story 6.5, UX-DR8): a row is "affected" iff it matches an
  // unhealthy bridge session on BOTH `accountId` AND the room's stable machine
  // `networkId` (the `protocol.id`, never the display label). A native room (no
  // networkId) or a healthy/unmonitored session shows no dot. Rust owns the state.
  const sessionHealth = useBridgeHealth(room.accountId, room.networkId ?? "");
  const affectedHealth =
    sessionHealth !== undefined && sessionHealth.health !== "healthy" ? sessionHealth.health : null;

  // Pending-draft marker (Story 7.1, AD-15): when this chat carries unsent composer
  // text, the preview line leads with an amber (`held`) pencil + "Draft" prefix. Draft
  // presence is Rust-authoritative (the `drafts` table), mirrored in `draftsStore`.
  const hasDraft = useHasDraft(room.accountId, room.roomId);

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
          ref={ref}
          type="button"
          tabIndex={tabIndex}
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
              <span className="flex min-w-0 items-center gap-1.5">
                {/* Affected-row health dot (Story 6.5): shown iff this room's
                    (accountId, networkId) session is unhealthy — a persistent,
                    Rust-authoritative indicator, never re-derived here. */}
                {affectedHealth !== null && (
                  <span
                    aria-hidden="true"
                    data-testid="bridge-health-dot"
                    className={cn(
                      "size-2 shrink-0 rounded-full",
                      BRIDGE_HEALTH_DOT_CLASS[affectedHealth],
                    )}
                  />
                )}
                <span
                  className={cn("truncate text-sm", isUnread ? "font-semibold" : "font-medium")}
                >
                  {room.displayName}
                </span>
                {/* Durable mute glyph (Story 10.2, FR-52): a bell-off for a muted Chat
                    or muted Network, an at-sign for mention-only. Rust-authoritative
                    (`room.muteState`), never re-derived; DND is NOT stamped here. */}
                {room.muteState === "muted" ? (
                  <BellOff
                    aria-label="Muted"
                    data-testid="mute-glyph"
                    className="size-3 shrink-0 text-muted-foreground"
                  />
                ) : room.muteState === "mention_only" ? (
                  <AtSign
                    aria-label="Mentions only"
                    data-testid="mention-only-glyph"
                    className="size-3 shrink-0 text-muted-foreground"
                  />
                ) : null}
              </span>
              {timestamp !== null && (
                <span className="shrink-0 text-muted-foreground text-xs">{timestamp}</span>
              )}
            </div>
            <div className="flex items-center justify-between gap-2">
              <span className="flex min-w-0 items-center gap-1 truncate text-muted-foreground text-sm">
                {hasDraft && (
                  <span
                    data-testid="draft-marker"
                    className="inline-flex shrink-0 items-center gap-1 text-held"
                  >
                    <Pencil aria-hidden="true" className="size-3" />
                    Draft
                  </span>
                )}
                <span className="truncate">{room.lastMessage ?? ""}</span>
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
        {room.isFavourite ? (
          <ContextMenuItem onSelect={onUnfavorite}>Unfavorite</ContextMenuItem>
        ) : (
          <ContextMenuItem onSelect={onFavorite}>Favorite</ContextMenuItem>
        )}
        <ContextMenuSeparator />
        <ContextMenuSub>
          <ContextMenuSubTrigger>Notifications</ContextMenuSubTrigger>
          <ContextMenuSubContent>
            <ContextMenuRadioGroup value={notifyRadioValue}>
              <ContextMenuRadioItem value="all" onSelect={() => setNotifyMode("all")}>
                All
              </ContextMenuRadioItem>
              <ContextMenuRadioItem
                value="mention_only"
                onSelect={() => setNotifyMode("mention_only")}
              >
                Mentions only
              </ContextMenuRadioItem>
              <ContextMenuRadioItem value="mute" onSelect={() => setNotifyMode("mute")}>
                Mute
              </ContextMenuRadioItem>
            </ContextMenuRadioGroup>
          </ContextMenuSubContent>
        </ContextMenuSub>
        {showFavoritesHint && (
          <ContextMenuLabel className="max-w-56 font-normal text-xs">
            Favorites keeps key chats one interaction away in a section above the inbox.
          </ContextMenuLabel>
        )}
      </ContextMenuContent>
    </ContextMenu>
  );
});
