/**
 * Favorites section (FR-21, UX-DR4, UX-DR13, Story 4.4).
 *
 * An always-visible labeled section rendered between the Pins strip and the inbox
 * scroll, one compact 48 px row per favourited room in the Rust-authoritative
 * recency order (the {@link favoritesRoomsStore} mirror). The header shows an
 * uppercase "FAVORITES" label and a collapse/expand chevron; when expanded, the
 * list of rows follows. Clicking a row selects the conversation; a per-row context
 * menu offers "Unfavorite". Favourite state rides the Matrix `m.favourite` notable
 * tag, so unfavouriting is best-effort with NO optimistic overlay — the row's
 * departure is Rust-authoritative filtering (AD-20).
 *
 * The section is `shrink-0` (sticky above the inbox scroll) so it stays reachable
 * in one interaction from any scroll position. It is hidden entirely
 * (`return null`) when there are no favourites (UX-DR4) — no label, no toggle, no
 * rows. Collapse/expand state is persisted via {@link setFavoritesCollapsed}.
 */
import { ChevronDown, ChevronRight } from "lucide-react";
import { RoomAvatar } from "@/components/chat/RoomAvatar";
import {
  ContextMenu,
  ContextMenuContent,
  ContextMenuItem,
  ContextMenuTrigger,
} from "@/components/ui/context-menu";
import { useLongPress } from "@/hooks/use-long-press";
import { useShellLayout } from "@/hooks/use-shell-layout";
import type { InboxRoomVm } from "@/lib/ipc/client";
import { setFavoritesCollapsed, unfavoriteRoom } from "@/lib/ipc/client";
import { favoritesUiStore, useFavoritesUiStore } from "@/lib/stores/favorites-ui";
import type { RoomSelection } from "@/lib/stores/rooms";
import { cn } from "@/lib/utils";

interface FavoritesSectionProps {
  /** The Favorites window, in Rust-authoritative recency order. */
  favorites: InboxRoomVm[];
  /** Select callback; receives the favourited room's account + room ids. */
  onSelect?: (selection: RoomSelection) => void;
  /** The currently open conversation, to mark the active row. */
  selected?: RoomSelection | null;
}

export function FavoritesSection({ favorites, onSelect, selected }: FavoritesSectionProps) {
  const isCollapsed = useFavoritesUiStore((s) => s.isCollapsed);
  const setCollapsed = useFavoritesUiStore((s) => s.setCollapsed);
  // Phone touch idiom (Story 13.6): a long-press opens the same Unfavorite
  // ContextMenu the desktop right-click does; the native callout is suppressed.
  // One shared hook instance serves every row (one press at a time).
  const { phone } = useShellLayout();
  const longPress = useLongPress();

  // Hidden entirely when empty (UX-DR4): no header, no toggle, no rows.
  if (favorites.length === 0) {
    return null;
  }

  const onToggle = () => {
    const next = !isCollapsed;
    setCollapsed(next);
    // Persist the collapse chrome (best-effort — a registry error is swallowed;
    // the in-memory state still toggles).
    void setFavoritesCollapsed(next).catch(() => {});
  };

  return (
    <section aria-label="Favorites" className="shrink-0 border-border border-b">
      <button
        type="button"
        onClick={onToggle}
        aria-expanded={!isCollapsed}
        className="flex w-full items-center gap-1 px-3 py-1.5 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset"
      >
        {isCollapsed ? (
          <ChevronRight aria-hidden="true" className="size-3 shrink-0 text-muted-foreground" />
        ) : (
          <ChevronDown aria-hidden="true" className="size-3 shrink-0 text-muted-foreground" />
        )}
        <span className="font-medium text-muted-foreground text-xs uppercase tracking-wide">
          Favorites
        </span>
      </button>
      {!isCollapsed && (
        <ul aria-label="Favorite conversations" className="flex flex-col pb-1">
          {favorites.map((room) => {
            const isSelected =
              selected?.roomId === room.roomId && selected?.accountId === room.accountId;
            return (
              <li key={`${room.accountId}:${room.roomId}`}>
                <ContextMenu>
                  <ContextMenuTrigger asChild>
                    <button
                      type="button"
                      onClick={() => onSelect?.({ accountId: room.accountId, roomId: room.roomId })}
                      {...longPress}
                      aria-label={`Favorite conversation with ${room.displayName}`}
                      aria-current={isSelected ? "true" : undefined}
                      className={cn(
                        // Compact 48 px row (UX-DR4): avatar + single-line name.
                        "flex h-12 w-full items-center gap-3 px-4 text-left outline-none focus-visible:ring-2 focus-visible:ring-ring focus-visible:ring-inset",
                        isSelected ? "bg-accent" : "hover:bg-accent",
                        // Long-press target (Story 13.6): suppress the native
                        // callout/selection on the phone tier only.
                        phone && "touch-callout-none select-none",
                      )}
                    >
                      <RoomAvatar room={room} size="lg" />
                      <span className="truncate text-sm">{room.displayName}</span>
                    </button>
                  </ContextMenuTrigger>
                  <ContextMenuContent>
                    <ContextMenuItem
                      className={phone ? "min-h-11" : undefined}
                      onSelect={() => {
                        void unfavoriteRoom(room.accountId, room.roomId).catch(() => {});
                      }}
                    >
                      Unfavorite
                    </ContextMenuItem>
                  </ContextMenuContent>
                </ContextMenu>
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}

/**
 * Hydrate the Favorites collapse UI store from the persisted registry setting.
 * Called once on the chat-list pane mount; a read failure leaves the in-memory
 * default (expanded). Exposed here so the section and its hydration live together.
 */
export async function hydrateFavoritesCollapsed(
  getCollapsed: () => Promise<boolean>,
): Promise<void> {
  try {
    const collapsed = await getCollapsed();
    favoritesUiStore.getState().setCollapsed(collapsed);
  } catch {
    // Missing/unreadable setting → keep the expanded default.
  }
}
