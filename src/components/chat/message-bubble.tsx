/**
 * A single text-message bubble (FR-8/FR-9, UX-DR5).
 *
 * Incoming messages (`isOwn: false`) use a muted surface aligned left; outgoing
 * (`isOwn: true`) use the primary surface aligned right. Both use a 14 px radius.
 * Consecutive same-sender messages are `grouped`: only the first shows the
 * avatar and sender name, the rest hide them and tuck under the same column.
 * A media message renders a {@link MediaAttachment} above the caption (Story 3.6);
 * a text message renders its body. A reply shows the quoted original inline
 * (clickable → jump to original) and an edited message shows an "Edited" caption
 * (Story 3.4); a hover/focus action bar offers Reply and Edit (own).
 */

import { MediaAttachment } from "@/components/chat/media-attachment";
import { MessageActions } from "@/components/chat/message-actions";
import { Avatar, AvatarFallback } from "@/components/ui/avatar";
import { Button } from "@/components/ui/button";
import { formatMessageTime } from "@/lib/format-time";
import type { ReactionGroupVm, ReplyPreviewVm, TimelineItemVm } from "@/lib/ipc/client";
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
  /**
   * Whether the account is currently offline (UX-DR10). A pure projection of the
   * Rust-streamed connection status: when `true` and this outgoing message is
   * still `sending`, the transient caption reads amber `Queued — sends when
   * you're back online` instead of `Sending…`. `sent`/`failed` are unaffected.
   */
  offline?: boolean;
  /** Begin a reply to this message (Story 3.4). Mounts the action bar's Reply. */
  onReply?: (key: string) => void;
  /**
   * Begin an edit of this message (Story 3.4). The action bar offers Edit only on
   * an own text message.
   */
  onEdit?: (key: string) => void;
  /**
   * Begin deleting this message for everyone (Story 3.8, FR-15). The action bar
   * offers Delete only on an own message; the parent opens the confirmation dialog.
   */
  onDelete?: (key: string) => void;
  /**
   * Jump to (scroll to) the original of a received reply, by the original's opaque
   * render `key`. The reply quote is clickable only when the parent wires this and
   * the quote carries a resolved `inReplyToKey`.
   */
  onJumpTo?: (key: string) => void;
  /**
   * Whether this bubble is the keyboard-selected message (`↑`/`↓`). When `true` a
   * selection ring renders on the bubble.
   */
  selected?: boolean;
  /**
   * Toggle an emoji reaction on this message (Story 3.5, FR-12). Wired to both the
   * action-bar Popover pick and a click on an existing reaction pill. Reactions are
   * stateless on the frontend — this fires the IPC and the diff stream updates the
   * pills. When absent, the action bar's React affordance and the pills are inert.
   */
  onToggleReaction?: (key: string, emoji: string) => void;
  /**
   * Open the Quick-Look preview overlay for a media message, by its opaque render
   * `key` (Story 3.6). Wired to an image/video attachment's click/Enter. When
   * absent, the media renders but is not click-to-open.
   */
  onOpenPreview?: (key: string) => void;
  /**
   * Cancel an in-flight outgoing media echo by its `key` (Story 3.7). Wired to the
   * Cancel affordance overlaid on an own media attachment while it is `sending`.
   * When absent, no Cancel affordance renders.
   */
  onCancelSend?: (key: string) => void;
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

export function MessageBubble({
  item,
  grouped,
  groupTail = true,
  onRetry,
  offline = false,
  onReply,
  onEdit,
  onDelete,
  onJumpTo,
  selected = false,
  onToggleReaction,
  onOpenPreview,
  onCancelSend,
}: MessageBubbleProps) {
  const displayName = item.senderDisplayName ?? item.sender;
  const time = formatMessageTime(item.timestamp);
  const isOwn = item.isOwn;
  const sendState = item.sendState;
  // Only own text messages are editable (Rust also gates on `is_editable()`).
  const canEdit = isOwn;
  // Only own messages that have actually been sent can be deleted for everyone
  // (Story 3.8, FR-15). An in-flight or failed local echo (`sendState !== null`) has
  // no remote event to redact — those use Cancel/Retry (Story 3.7), not Delete.
  const canDelete = isOwn && sendState === null;

  return (
    <div
      data-msg-key={item.key}
      className={cn(
        "group flex w-full items-end gap-2",
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
        <div className={cn("flex items-center gap-1", isOwn ? "flex-row-reverse" : "flex-row")}>
          <div
            className={cn(
              "max-w-[75%] rounded-[14px] px-3 py-2 text-sm",
              isOwn ? "bg-primary text-primary-foreground" : "bg-muted text-foreground",
              selected && "ring-2 ring-ring ring-offset-1 ring-offset-background",
            )}
          >
            {item.reply && <ReplyQuote reply={item.reply} isOwn={isOwn} onJumpTo={onJumpTo} />}
            {item.media && (
              <div className="mb-1">
                <MediaAttachment
                  media={item.media}
                  messageKey={item.key}
                  onOpenPreview={onOpenPreview}
                  // While an own media echo is still sending, overlay an uploading
                  // indicator + Cancel (Story 3.7); derived purely from the existing
                  // send-state (no VM change). `sent`/`failed` have no overlay —
                  // `failed` reuses the SendStateCaption "Failed — Retry" below.
                  uploading={isOwn && sendState === "sending"}
                  onCancel={onCancelSend}
                />
              </div>
            )}
            {/* Text/caption: rendered only when there is a body (a media message
                may carry an empty caption). */}
            {item.body !== "" && <p className="whitespace-pre-wrap break-words">{item.body}</p>}
            <div className="mt-1 flex items-center justify-end gap-1">
              {item.isEdited && (
                <span
                  className={cn(
                    "text-[10px] leading-none",
                    isOwn ? "text-primary-foreground/70" : "text-muted-foreground",
                  )}
                >
                  Edited
                </span>
              )}
              {time !== "" && (
                <time
                  dateTime={new Date(item.timestamp).toISOString()}
                  className={cn(
                    "block text-right text-[10px] leading-none",
                    isOwn ? "text-primary-foreground/70" : "text-muted-foreground",
                  )}
                >
                  {time}
                </time>
              )}
            </div>
          </div>
          {/* Action bar: revealed on hover/focus-within of the bubble row. */}
          {(onReply || onEdit || onDelete || onToggleReaction) && (
            <div className="opacity-0 transition-opacity focus-within:opacity-100 group-hover:opacity-100">
              <MessageActions
                messageKey={item.key}
                canEdit={canEdit}
                canDelete={canDelete}
                onReact={(k, emoji) => onToggleReaction?.(k, emoji)}
                onReply={(k) => onReply?.(k)}
                onEdit={(k) => onEdit?.(k)}
                onDelete={(k) => onDelete?.(k)}
              />
            </div>
          )}
        </div>
        {/* Reaction pill row under the bubble; skipped entirely when empty. */}
        {item.reactions.length > 0 && (
          <ReactionPills
            reactions={item.reactions}
            isOwn={isOwn}
            onToggle={onToggleReaction ? (emoji) => onToggleReaction(item.key, emoji) : undefined}
          />
        )}
        <SendStateCaption
          state={sendState}
          isOwn={isOwn}
          messageKey={item.key}
          groupTail={groupTail}
          onRetry={onRetry}
          offline={offline}
        />
      </div>
    </div>
  );
}

interface ReplyQuoteProps {
  reply: ReplyPreviewVm;
  isOwn: boolean;
  onJumpTo?: (key: string) => void;
}

/**
 * The inline quoted-original preview above a reply's body (Story 3.4, FR-10).
 * Shows the original sender + a one-line body preview. Clickable — jumping to the
 * original — only when a jump handler is wired and the quote carries a resolved
 * `inReplyToKey` (the original is loaded); otherwise it renders as a static block
 * (honest, but not clickable).
 */
function ReplyQuote({ reply, isOwn, onJumpTo }: ReplyQuoteProps) {
  const label = reply.senderDisplayName ?? reply.sender;
  const clickable = onJumpTo != null && reply.inReplyToKey != null;
  const jumpKey = reply.inReplyToKey;

  const content = (
    <>
      <span className="block font-medium text-xs">{label}</span>
      <span className="block truncate text-xs opacity-80">{reply.body}</span>
    </>
  );

  const surface = cn(
    "mb-1 block w-full border-l-2 pl-2 text-left",
    isOwn ? "border-primary-foreground/50" : "border-foreground/30",
  );

  if (clickable && jumpKey != null) {
    return (
      <button
        type="button"
        aria-label="Jump to replied message"
        onClick={() => onJumpTo?.(jumpKey)}
        className={cn(surface, "cursor-pointer hover:opacity-100")}
      >
        {content}
      </button>
    );
  }
  return <div className={surface}>{content}</div>;
}

interface ReactionPillsProps {
  reactions: ReactionGroupVm[];
  isOwn: boolean;
  onToggle?: (emoji: string) => void;
}

/**
 * The click-to-toggle reaction pill row under a bubble (Story 3.5, FR-12). One
 * pill per aggregated emoji group (in the Rust-provided per-key order), showing the
 * emoji and its count. Own-reaction pills are visually highlighted (primary tint).
 * Clicking a pill toggles that reaction via `onToggle` — the diff stream then
 * updates the row (reactions are stateless on the frontend). Rendered only when the
 * group is non-empty (the parent skips it otherwise). The row aligns to the bubble
 * side (right for own messages).
 */
function ReactionPills({ reactions, isOwn, onToggle }: ReactionPillsProps) {
  return (
    <div className={cn("mt-1 flex flex-wrap gap-1", isOwn ? "justify-end" : "justify-start")}>
      {reactions.map((group) => (
        <button
          key={group.emoji}
          type="button"
          disabled={onToggle == null}
          aria-pressed={onToggle != null ? group.isOwn : undefined}
          aria-label={`${group.emoji} ${group.count}${group.isOwn ? ", you reacted" : ""}`}
          onClick={() => onToggle?.(group.emoji)}
          className={cn(
            "flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs leading-none transition-colors",
            onToggle != null && "cursor-pointer hover:bg-accent",
            group.isOwn
              ? "border-primary/40 bg-primary/10 text-foreground"
              : "border-border bg-muted text-muted-foreground",
          )}
        >
          <span aria-hidden="true">{group.emoji}</span>
          <span className="tabular-nums">{group.count}</span>
        </button>
      ))}
    </div>
  );
}

interface SendStateCaptionProps {
  state: MessageVm["sendState"];
  isOwn: boolean;
  messageKey: string;
  groupTail: boolean;
  onRetry?: (key: string) => void;
  offline: boolean;
}

/**
 * The outgoing send-state caption (UX-DR10/UX-DR11): microcopy in sentence case,
 * no error codes, no emoji. `Failed` always renders as a persistent destructive
 * `Failed — Retry` (the Retry never auto-clears); `Sending…`/`Sent` render muted
 * and only under the last bubble of a same-sender group. While the account is
 * `offline`, a still-`sending` *own* message reads the amber `Queued — sends when
 * you're back online` (a pure projection of the connection status + `isOwn`)
 * instead of `Sending…`. A remote message (`sendState: null`) renders nothing.
 */
function SendStateCaption({
  state,
  isOwn,
  messageKey,
  groupTail,
  onRetry,
  offline,
}: SendStateCaptionProps) {
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
    if (offline && isOwn) {
      return (
        <span className="mt-0.5 text-held text-xs">Queued — sends when you're back online</span>
      );
    }
    return <span className="mt-0.5 text-muted-foreground text-xs">Sending…</span>;
  }
  if (state === "sent") {
    return <span className="mt-0.5 text-muted-foreground text-xs">Sent</span>;
  }
  return null;
}
