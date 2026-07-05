/**
 * Shared room avatar (Story 4.3).
 *
 * One source of truth for rendering a room's avatar: a browser-loadable image
 * when present, an initials fallback otherwise. Used at `size="lg"` in the 64 px
 * chat row and at `size="xl"` (44 px) in the Pins strip so both surfaces stay
 * pixel-consistent. The overlaid Network badge is intentionally NOT rendered yet —
 * it arrives with resolved Network identity in Story 4.6 (FR-24 attribution); a
 * meaningless placeholder dot would visually regress every existing chat row.
 */
import { Avatar, AvatarFallback, AvatarImage } from "@/components/ui/avatar";
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
    </Avatar>
  );
}
