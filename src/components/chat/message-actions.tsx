/**
 * Per-message hover/focus action bar (Story 3.4/3.5/3.8, FR-10/FR-11/FR-12/FR-15;
 * epic action-bar).
 *
 * Reveals React (a curated-emoji Popover, always), Reply (always), Edit (own text
 * messages only), and Delete (own messages only — delete for everyone) over a
 * bubble. All are labeled, keyboard-focusable controls. The parent wires
 * `onReact`/`onReply`/`onEdit`/`onDelete`; this component holds no IPC or store
 * knowledge — Delete opens the confirmation dialog in the parent.
 */
import { Pencil, Reply, Trash2 } from "lucide-react";
import { ReactionPopover } from "@/components/chat/reaction-popover";
import { Button } from "@/components/ui/button";

interface MessageActionsProps {
  /** The target message's opaque render key. */
  messageKey: string;
  /** Whether Edit is offered (own text message only). */
  canEdit: boolean;
  /** Whether Delete is offered (own message only — delete for everyone). */
  canDelete: boolean;
  /** Add an emoji reaction to this message (via the curated Popover). */
  onReact: (key: string, emoji: string) => void;
  /** Begin a reply to this message. */
  onReply: (key: string) => void;
  /** Begin an edit of this message (offered only when `canEdit`). */
  onEdit: (key: string) => void;
  /** Begin deleting this message for everyone (offered only when `canDelete`). */
  onDelete: (key: string) => void;
}

export function MessageActions({
  messageKey,
  canEdit,
  canDelete,
  onReact,
  onReply,
  onEdit,
  onDelete,
}: MessageActionsProps) {
  return (
    <div className="flex items-center gap-0.5 rounded-md border border-border bg-background p-0.5 shadow-xs">
      <ReactionPopover onPick={(emoji) => onReact(messageKey, emoji)} />
      <Button
        type="button"
        variant="ghost"
        size="icon-xs"
        aria-label="Reply"
        onClick={() => onReply(messageKey)}
      >
        <Reply aria-hidden="true" />
      </Button>
      {canEdit && (
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          aria-label="Edit"
          onClick={() => onEdit(messageKey)}
        >
          <Pencil aria-hidden="true" />
        </Button>
      )}
      {canDelete && (
        <Button
          type="button"
          variant="ghost"
          size="icon-xs"
          aria-label="Delete"
          onClick={() => onDelete(messageKey)}
        >
          <Trash2 aria-hidden="true" />
        </Button>
      )}
    </div>
  );
}
