/**
 * Phone stack header (Story 13.2, UX-DR21): the single 52px bar the `PhoneShell`
 * renders at the Room (level 1) and Detail (level 2) levels.
 *
 * The back button carries the *previous* level's title (level 1 → "Inbox";
 * level 2 → the selected room's display name) with a ≥44pt hit area and a
 * `Back to {title}` accessible name (a generic "Back" when the room's VM is not
 * in any streamed window). At the Room level the bar also renders the shared
 * `ConversationHeaderIdentity` block wrapped as an "Open details" button — the
 * phone's ⌘I replacement, pushing the Detail level via `detailStore` — the
 * shared `ConversationIncognitoChip`, and a ⋯ overflow menu carrying **Search in
 * chat** (Story 13.4: opens the merged full-screen Search surface in Messages
 * scope locked to this Chat via `searchSurfaceStore`) and **Export** (the same
 * `exportStore` action as the desktop header). Mute / Mention-only / Archive land
 * with their owning stories (10.2–13.7 / 4.2–13.6). No chat sub-parts are forked:
 * identity and incognito are the exact components the desktop header renders.
 */
import { ChevronLeft, Download, Ellipsis, Search } from "lucide-react";
import type { Ref } from "react";
import {
  ConversationHeaderIdentity,
  ConversationIncognitoChip,
} from "@/components/layout/conversation-pane";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { useSelectedRoomVm } from "@/hooks/use-selected-room-vm";
import { detailStore } from "@/lib/stores/detail-ui";
import { exportStore } from "@/lib/stores/export";
import { useRoomsStore } from "@/lib/stores/rooms";
import { searchSurfaceStore } from "@/lib/stores/search-surface";

interface PhoneHeaderProps {
  /** The stack level this header sits on: 1 = Room, 2 = Detail. */
  level: 1 | 2;
  /** Pop exactly one level (the chevron's action). */
  onBack: () => void;
  /** Forwarded to the back button so the shell can focus it on push (UX-DR28). */
  backRef?: Ref<HTMLButtonElement>;
}

export function PhoneHeader({ level, onBack, backRef }: PhoneHeaderProps) {
  const selected = useRoomsStore((s) => s.selected);
  const room = useSelectedRoomVm();
  const accountId = selected?.accountId ?? null;
  const roomId = selected?.roomId ?? null;
  const networkId = room?.networkId ?? null;

  // The back button carries the previous level's title. At the Detail level the
  // previous level is the Room; an unknown room VM degrades to a generic "Back".
  const backTitle = level === 1 ? "Inbox" : (room?.displayName ?? null);
  const backLabel = backTitle === null ? "Back" : `Back to ${backTitle}`;

  return (
    <header className="flex h-[var(--phone-header)] shrink-0 items-center gap-1 border-border border-b pr-1">
      <button
        ref={backRef}
        type="button"
        aria-label={backLabel}
        onClick={onBack}
        className="flex h-11 min-w-11 shrink-0 items-center gap-0.5 pr-2 pl-1 text-foreground outline-none focus-visible:ring-2 focus-visible:ring-ring"
      >
        <ChevronLeft className="size-5" aria-hidden="true" />
        <span className="max-w-44 truncate text-sm">{backTitle ?? "Back"}</span>
      </button>
      {level === 1 && (
        <>
          <button
            type="button"
            aria-label="Open details"
            onClick={() => detailStore.getState().openDetail()}
            className="flex h-11 min-w-0 flex-1 items-center justify-start text-left outline-none focus-visible:ring-2 focus-visible:ring-ring"
          >
            <ConversationHeaderIdentity accountId={accountId} />
          </button>
          <ConversationIncognitoChip
            // Key by roomId so a room switch remounts the chip (same guard as the
            // desktop header): it can never leave a Popover bound to the previous chat.
            key={roomId ?? ""}
            accountId={accountId}
            roomId={roomId}
            networkId={networkId}
          />
          <DropdownMenu>
            <DropdownMenuTrigger asChild>
              <Button
                type="button"
                variant="ghost"
                size="icon"
                aria-label="More"
                className="size-11 shrink-0 focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none"
              >
                <Ellipsis aria-hidden="true" />
              </Button>
            </DropdownMenuTrigger>
            <DropdownMenuContent align="end">
              {accountId !== null && roomId !== null && (
                <>
                  <DropdownMenuItem
                    onSelect={() =>
                      searchSurfaceStore.getState().open({
                        scope: "messages",
                        chatLock: { accountId, roomId },
                      })
                    }
                  >
                    <Search aria-hidden="true" />
                    Search in chat
                  </DropdownMenuItem>
                  <DropdownMenuItem
                    onSelect={() =>
                      exportStore.getState().open({
                        scope: "chat",
                        accountId,
                        roomId,
                      })
                    }
                  >
                    <Download aria-hidden="true" />
                    Export
                  </DropdownMenuItem>
                </>
              )}
            </DropdownMenuContent>
          </DropdownMenu>
        </>
      )}
    </header>
  );
}
