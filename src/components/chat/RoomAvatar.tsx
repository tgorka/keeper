/**
 * Shared room avatar (Story 4.3).
 *
 * One source of truth for rendering a room's avatar: a browser-loadable image
 * when present, an initials fallback otherwise. Used at `size="lg"` in the 64 px
 * chat row and at `size="xl"` (44 px) in the Pins strip so both surfaces stay
 * pixel-consistent. When the room is bridged (`network !== null`), a uniform 16 px
 * neutral Network badge (Story 4.6, FR-24) is overlaid bottom-right showing the
 * Network label's first code point — the same avatar (and thus badge) is reused in
 * the conversation header. A native Matrix room (`network === null`) shows no badge.
 * Network identity is never rendered as per-row/pane coloring (UX-DR3).
 */
import { Avatar, AvatarBadge, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
import type { InboxRoomVm } from "@/lib/ipc/client";

/**
 * Derive up-to-two-letter initials from a room display name for the avatar
 * fallback. Falls back to `"#"` for an empty/whitespace name.
 */
export function roomInitials(displayName: string): string {
  const words = displayName.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) {
    return "#";
  }
  if (words.length === 1) {
    return words[0].slice(0, 2).toUpperCase();
  }
  return (words[0][0] + words[words.length - 1][0]).toUpperCase();
}

interface RoomAvatarProps {
  room: InboxRoomVm;
  /** Avatar size: `lg` (40 px, chat row) or `xl` (44 px, Pins strip). */
  size: "lg" | "xl";
}

export function RoomAvatar({ room, size }: RoomAvatarProps) {
  // An `mxc://` URI cannot load in the webview (the media scheme handler is a
  // later epic); only a browser-loadable http(s) URL is rendered as an image.
  const httpAvatar = room.avatarUrl && /^https?:\/\//.test(room.avatarUrl) ? room.avatarUrl : null;
  return (
    <Avatar size={size}>
      {httpAvatar !== null && <AvatarImage src={httpAvatar} alt="" />}
      <AvatarFallback>{roomInitials(room.displayName)}</AvatarFallback>
      {/* Bridged-Network badge (Story 4.6, FR-24): a uniform 16 px neutral chip
          overlaid bottom-right, showing the Network label's first code point (labels
          are ASCII protocol names like Telegram/Signal). Same neutral color for every
          Network (never per-Network coloring, never the primary/mention accent).
          Rendered only when the room is bridged. */}
      {room.network !== null && (
        <AvatarBadge
          // `size-4!` (important) forces the uniform 16 px badge across every avatar
          // size: `AvatarBadge`'s own `group-data-[size=lg|xl]/avatar:size-3(.5)`
          // variants have higher specificity than a plain `size-4` and would otherwise
          // shrink the badge to 12/14 px on the `lg` (row/header) and `xl` (Pins strip)
          // avatars this badge is used on.
          className="size-4! bg-secondary text-[9px] text-secondary-foreground"
          aria-label={`${room.network} network`}
          title={room.network}
        >
          {[...room.network][0]?.toUpperCase() ?? ""}
        </AvatarBadge>
      )}
    </Avatar>
  );
}
