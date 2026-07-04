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
import { Button } from "@/components/ui/button";
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
  /**
   * Whether this bubble is the last in its same-sender run. Only the group tail
   * shows the transient `Sending…`/`Sent` caption (to avoid per-bubble noise); a
   * `Failed` caption always renders regardless.
   */
  groupTail?: boolean;
  /**
   * Retry a failed outgoing message by its `key`. Wired by the parent to the
   * controlled send path; the `Failed — Retry` button calls it.
   */
  onRetry?: (key: string) => void;
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

export function MessageBubble({ item, grouped, groupTail = true, onRetry }: MessageBubbleProps) {
  const displayName = item.senderDisplayName ?? item.sender;
  const time = formatMessageTime(item.timestamp);
  const isOwn = item.isOwn;
  const sendState = item.sendState;

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
        <SendStateCaption
          state={sendState}
          messageKey={item.key}
          groupTail={groupTail}
          onRetry={onRetry}
        />
      </div>
    </div>
  );
}

interface SendStateCaptionProps {
  state: MessageVm["sendState"];
  messageKey: string;
  groupTail: boolean;
  onRetry?: (key: string) => void;
}

/**
 * The outgoing send-state caption (UX-DR10/UX-DR11): microcopy in sentence case,
 * no error codes, no emoji. `Failed` always renders as a persistent destructive
 * `Failed — Retry` (the Retry never auto-clears); `Sending…`/`Sent` render muted
 * and only under the last bubble of a same-sender group. A remote message
 * (`sendState: null`) renders nothing.
 */
function SendStateCaption({ state, messageKey, groupTail, onRetry }: SendStateCaptionProps) {
  if (state === "failed") {
    return (
      <div className="mt-0.5 flex items-center gap-1">
        <span className="text-destructive text-xs">Failed</span>
        <span aria-hidden="true" className="text-muted-foreground text-xs">
          —
        </span>
        <Button type="button" variant="destructive" size="xs" onClick={() => onRetry?.(messageKey)}>
          Retry
        </Button>
      </div>
    );
  }
  if (!groupTail) {
    return null;
  }
  if (state === "sending") {
    return <span className="mt-0.5 text-muted-foreground text-xs">Sending…</span>;
  }
  if (state === "sent") {
    return <span className="mt-0.5 text-muted-foreground text-xs">Sent</span>;
  }
  return null;
}
