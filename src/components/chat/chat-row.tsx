/**
 * A single 64 px chat-list row (UX-DR3, Story 2.1).
 *
 * Full-width, keyboard-operable `<button>` showing a 3 px per-account hue edge
 * bar, the room avatar, display name, last-message preview, and timestamp.
 * Selecting it (click / Enter / Space) records its `{ accountId, roomId }` via
 * `onSelect`; the selected row is highlighted and marked `aria-current`. Carries
 * a visible focus ring and an accessible label. The hue index comes from Rust
 * (per account) and maps to a CSS `--account-hue-N` variable — no color value is
 * hardcoded here.
 */
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import { accountHueVar } from "@/lib/account-hue";
import { formatRoomTimestamp } from "@/lib/format-time";
import type { InboxRoomVm } from "@/lib/ipc/client";
import type { RoomSelection } from "@/lib/stores/rooms";
import { cn } from "@/lib/utils";

interface ChatRowProps {
  room: InboxRoomVm;
  /** Optional selection callback; receives the row's account + room ids. */
  onSelect?: (selection: RoomSelection) => void;
  /** Whether this row is the currently open conversation. */
  selected?: boolean;
}

/**
 * Derive up-to-two-letter initials from a room display name for the avatar
 * fallback. Falls back to `"#"` for an empty/whitespace name.
 */
function initials(displayName: string): string {
  const words = displayName.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) {
    return "#";
  }
  if (words.length === 1) {
    return words[0].slice(0, 2).toUpperCase();
  }
  return (words[0][0] + words[words.length - 1][0]).toUpperCase();
}

export function ChatRow({ room, onSelect, selected = false }: ChatRowProps) {
  const timestamp = room.timestamp === null ? null : formatRoomTimestamp(room.timestamp) || null;
  // An `mxc://` URI cannot load in the webview (the media scheme handler is a
  // later epic); only a browser-loadable http(s) URL is rendered as an image.
  const httpAvatar = room.avatarUrl && /^https?:\/\//.test(room.avatarUrl) ? room.avatarUrl : null;

  return (
    <button
      type="button"
      onClick={() => onSelect?.({ accountId: room.accountId, roomId: room.roomId })}
      aria-label={`Conversation with ${room.displayName}`}
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
      <Avatar size="lg">
        {httpAvatar !== null && <AvatarImage src={httpAvatar} alt="" />}
        <AvatarFallback>{initials(room.displayName)}</AvatarFallback>
      </Avatar>
      <div className="flex min-w-0 flex-1 flex-col">
        <div className="flex items-baseline justify-between gap-2">
          <span className="truncate font-medium text-sm">{room.displayName}</span>
          {timestamp !== null && (
            <span className="shrink-0 text-muted-foreground text-xs">{timestamp}</span>
          )}
        </div>
        <span className="truncate text-muted-foreground text-sm">{room.lastMessage ?? ""}</span>
      </div>
    </button>
  );
}
