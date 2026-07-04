/**
 * A single text-message bubble (FR-8/FR-9, UX-DR5).
 *
 * Incoming messages (`isOwn: false`) use a muted surface aligned left; outgoing
 * (`isOwn: true`) use the primary surface aligned right. Both use a 14 px radius.
 * Consecutive same-sender messages are `grouped`: only the first shows the
 * avatar and sender name, the rest hide them and tuck under the same column.
 * Renders text only — no media, replies, or reactions (later epics).
 */
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { formatMessageTime } from "@/lib/format-time";
import type { TimelineItemVm } from "@/lib/ipc/client";
import { cn } from "@/lib/utils";

/** The `message`-variant of {@link TimelineItemVm} (the only kind this renders). */
export type MessageVm = Extract<TimelineItemVm, { kind: "message" }>;

interface MessageBubbleProps {
  /** The text message to render. */
  item: MessageVm;
  /**
   * Whether this bubble continues a run from the same sender: when `true`, the
   * avatar and sender name are hidden so the run reads as one grouped block.
   */
  grouped: boolean;
}

/**
 * Derive up-to-two-letter initials from a sender label for the avatar fallback.
 * Falls back to `"?"` for an empty/whitespace label.
 */
function initials(label: string): string {
  const words = label.trim().split(/\s+/).filter(Boolean);
  if (words.length === 0) {
    return "?";
  }
  if (words.length === 1) {
    return words[0].slice(0, 2).toUpperCase();
  }
  return (words[0][0] + words[words.length - 1][0]).toUpperCase();
}

export function MessageBubble({ item, grouped }: MessageBubbleProps) {
  const displayName = item.senderDisplayName ?? item.sender;
  const time = formatMessageTime(item.timestamp);
  const isOwn = item.isOwn;

  return (
    <div
      className={cn(
        "flex w-full items-end gap-2",
        isOwn ? "flex-row-reverse" : "flex-row",
        // Tighten the gap between grouped bubbles from the same sender.
        grouped ? "mt-0.5" : "mt-3",
      )}
    >
      {/* Avatar gutter (incoming only): shown on the group's first bubble,
          reserved as empty space on continuations to keep the column aligned. */}
      {!isOwn &&
        (grouped ? (
          <div aria-hidden="true" className="w-8 shrink-0" />
        ) : (
          <Avatar size="default" className="shrink-0">
            <AvatarFallback>{initials(displayName)}</AvatarFallback>
          </Avatar>
        ))}

      <div className={cn("flex min-w-0 flex-col", isOwn ? "items-end" : "items-start")}>
        {!grouped && !isOwn && (
          <span className="mb-0.5 px-1 font-medium text-muted-foreground text-xs">
            {displayName}
          </span>
        )}
        <div
          className={cn(
            "max-w-[75%] rounded-[14px] px-3 py-2 text-sm",
            isOwn ? "bg-primary text-primary-foreground" : "bg-muted text-foreground",
          )}
        >
          <p className="whitespace-pre-wrap break-words">{item.body}</p>
          {time !== "" && (
            <time
              dateTime={new Date(item.timestamp).toISOString()}
              className={cn(
                "mt-1 block text-right text-[10px] leading-none",
                isOwn ? "text-primary-foreground/70" : "text-muted-foreground",
              )}
            >
              {time}
            </time>
          )}
        </div>
      </div>
    </div>
  );
}
