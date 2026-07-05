/**
 * Read-receipt micro-avatar cluster (Story 3.9, receipts, NFR-9).
 *
 * Renders the *other* members whose read receipt sits on a message as a small
 * cluster of initials-based micro-avatars, derived purely from the opaque
 * `readers` user ids the Rust timeline producer carries on the message VM. No
 * avatar images / mxc resolution here (deferred profile work) — the chip shows
 * deterministic initials with a deterministic per-id background hue, so the same
 * reader always renders the same chip. Shows up to three chips then a `+K`
 * overflow. Renders nothing when `readers` is empty.
 */
import { cn } from "@/lib/utils";

/** How many reader chips to show before collapsing the rest into a `+K` badge. */
const MAX_CHIPS = 3;

interface ReadReceiptsProps {
  /** Opaque Matrix user ids of the *other* members who have read this message. */
  readers: string[];
  /** Whether the parent message is the account's own (aligns the cluster right). */
  isOwn?: boolean;
}

/**
 * Derive up-to-two-letter initials from a Matrix user id's localpart (the part
 * between `@` and `:`). Falls back to `?` for an unusable id.
 */
function initialsOf(userId: string): string {
  const localpart = userId.replace(/^@/, "").split(":")[0] ?? "";
  const cleaned = localpart.replace(/[^a-zA-Z0-9]/g, "");
  if (cleaned.length === 0) {
    return "?";
  }
  return cleaned.slice(0, 2).toUpperCase();
}

/**
 * Deterministic hue (0–359) from a user id, so a given reader always gets the same
 * chip color. A small, stable string hash — never a random per-render value.
 */
function hueOf(userId: string): number {
  // Accumulate the hash in full 32-bit width and reduce to a hue once at the end.
  // Reducing modulo 360 on every step (as a prior version did) collapses the state
  // space mid-accumulation, so long ids lose their prefix entropy and colors
  // cluster — different readers then share a hue. `| 0` keeps 32-bit int math.
  let hash = 0;
  for (let i = 0; i < userId.length; i += 1) {
    hash = (hash * 31 + userId.charCodeAt(i)) | 0;
  }
  return Math.abs(hash) % 360;
}

export function ReadReceipts({ readers, isOwn = false }: ReadReceiptsProps) {
  if (readers.length === 0) {
    return null;
  }
  const shown = readers.slice(0, MAX_CHIPS);
  const overflow = readers.length - shown.length;

  return (
    <div
      className={cn("mt-0.5 flex items-center gap-0.5", isOwn ? "justify-end" : "justify-start")}
    >
      {/* Accessible label for the read-receipt cluster (the chips are decorative). */}
      <span className="sr-only">
        Read by {readers.length} {readers.length === 1 ? "person" : "people"}
      </span>
      {shown.map((userId) => (
        <span
          key={userId}
          title={userId}
          aria-hidden="true"
          className="flex size-3.5 items-center justify-center rounded-full font-medium text-[7px] text-white leading-none"
          style={{ backgroundColor: `hsl(${hueOf(userId)} 55% 45%)` }}
        >
          {initialsOf(userId)}
        </span>
      ))}
      {overflow > 0 && (
        <span
          aria-hidden="true"
          className="flex h-3.5 items-center justify-center rounded-full bg-muted px-1 font-medium text-[7px] text-muted-foreground leading-none"
        >
          +{overflow}
        </span>
      )}
    </div>
  );
}
