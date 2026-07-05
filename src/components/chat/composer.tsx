/**
 * Message composer (FR-9, UX-DR5; reply/edit context — Story 3.4, FR-10/FR-11).
 *
 * A controlled {@link Textarea} that autogrows to eight lines then scrolls, with
 * a send {@link Button}. Enter sends; ⇧Enter inserts a newline; a whitespace-only
 * body never dispatches. The draft lives in local `useState` (no IPC round-trip
 * on keystroke, so input stays under one frame) and is cleared on a successful
 * send. This component owns no IPC knowledge — the parent wires `onSend` (which
 * routes to reply / edit / text based on `pending`).
 *
 * When `pending` is set, a context banner renders above the textarea (the quoted
 * sender/preview for a reply, "Editing your message" for an edit) with a cancel
 * (×) control. `Esc` cancels the pending context: a reply keeps the typed draft; an
 * edit restores the pre-edit stashed draft (both "cancel without losing composer
 * text"). Entering edit prefills the textarea with the message body (`editPrefill`).
 */
import { open } from "@tauri-apps/plugin-dialog";
import { Paperclip, X } from "lucide-react";
import { type ClipboardEvent, type KeyboardEvent, useEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import {
  attachmentId,
  attachmentsStore,
  type PendingAttachment,
  useAttachmentsStore,
} from "@/lib/stores/attachments";
import type { PendingContext } from "@/lib/stores/composer";
import { cn } from "@/lib/utils";

/** Derive a chip display name for a pending attachment (its filename). */
function chipLabel(attachment: PendingAttachment): string {
  return attachment.filename;
}

/** Format a byte count as a short human-readable size (e.g. `1.2 MB`). */
function formatSize(bytes: number): string {
  if (bytes < 1024) {
    return `${bytes} B`;
  }
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  return `${value.toFixed(value < 10 ? 1 : 0)} ${units[unit]}`;
}

/** The display filename derived from an OS file path (its basename). */
function basename(path: string): string {
  const parts = path.split(/[/\\]/);
  return parts[parts.length - 1] || path;
}

interface ComposerProps {
  /**
   * Dispatch the trimmed body. Resolves on success (the draft then clears);
   * rejects if the send could not be enqueued (the draft is kept so the user can
   * retry). The parent routes this to `sendReply` / `editMessage` / `sendText`
   * based on the current `pending`.
   */
  onSend: (body: string) => Promise<void>;
  /**
   * Dispatch the pending attachments (Story 3.7). `caption` is the trimmed
   * composer text, passed only when exactly one attachment is pending (otherwise
   * `undefined` — a caption maps to a single media event). The parent routes each
   * attachment to `sendAttachmentPath` / `sendAttachmentBytes`. Resolves when all
   * are enqueued (the tray + draft then clear); rejects to keep the tray so the
   * user can retry. Absent → the attach/paste affordances are inert.
   */
  onSendAttachments?: (attachments: PendingAttachment[], caption?: string) => Promise<void>;
  /** When `true`, the composer is inert (no room loaded). */
  disabled?: boolean;
  /** The active reply/edit context, or `null`. Drives the banner + Esc routing. */
  pending?: PendingContext | null;
  /**
   * The message body to prefill the textarea with when entering **edit** mode
   * (`null` outside edit). Applied once per edit target.
   */
  editPrefill?: string | null;
  /**
   * Cancel the pending context (Esc / banner ×). Returns the draft the composer
   * should restore (the stashed pre-edit draft for an edit) or `null` for a reply
   * (whose typed draft is kept). The parent wires this to the composer store's
   * `cancel`.
   */
  onCancelPending?: () => string | null;
  /**
   * `↑` pressed in an empty composer with no pending context (caret at start):
   * the parent opens edit on the last own message (Story 3.4 / epic affordance).
   */
  onEmptyArrowUp?: () => void;
}

export function Composer({
  onSend,
  onSendAttachments,
  disabled = false,
  pending = null,
  editPrefill = null,
  onCancelPending,
  onEmptyArrowUp,
}: ComposerProps) {
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState(false);
  const attachments = useAttachmentsStore((s) => s.pending);
  // The attach/paste affordances are available only when the parent wires the
  // attachment dispatcher and the composer is enabled.
  const attachEnabled = onSendAttachments != null && !disabled;

  // Mirror the live draft in a ref so the prefill effect can stash it without
  // taking `draft` as a dependency (which would re-run the effect every keystroke).
  const draftRef = useRef(draft);
  draftRef.current = draft;
  // The draft that was in the composer just before entering the current edit,
  // restored verbatim on Esc/cancel so an edit "cancels without losing composer
  // text" (Story 3.4, FR-11). Owned here because the draft lives in local state.
  const preEditDraft = useRef("");

  // Prefill the draft with the target's body when entering edit mode (once per
  // edit target). Keyed on the edit target key so re-entering edit on a different
  // message re-prefills, but typing within one edit is not clobbered. The outgoing
  // draft is stashed first so cancel can restore it.
  const editTargetKey = pending?.mode === "edit" ? pending.targetKey : null;
  const prefilledFor = useRef<string | null>(null);
  useEffect(() => {
    if (editTargetKey !== null && prefilledFor.current !== editTargetKey) {
      prefilledFor.current = editTargetKey;
      preEditDraft.current = draftRef.current;
      setDraft(editPrefill ?? "");
      setError(false);
    }
    if (editTargetKey === null) {
      prefilledFor.current = null;
    }
  }, [editTargetKey, editPrefill]);

  const hasAttachments = attachments.length > 0;
  // Send is enabled when there is a trimmed body OR at least one pending
  // attachment (an attachment can be sent with no caption). An edit never carries
  // attachments.
  const canSend =
    (draft.trim().length > 0 || (hasAttachments && pending?.mode !== "edit")) &&
    !disabled &&
    !sending;

  async function send() {
    const body = draft.trim();
    const trayAttachments = attachmentsStore.getState().pending;
    const dispatchAttachments =
      onSendAttachments != null && pending?.mode !== "edit" && trayAttachments.length > 0;
    if ((body.length === 0 && !dispatchAttachments) || disabled || sending) {
      // Whitespace-only with no attachment / disabled / in-flight: never dispatch.
      return;
    }
    setSending(true);
    setError(false);
    try {
      if (dispatchAttachments && onSendAttachments != null) {
        // A caption maps to a single media event, so it rides only when exactly
        // one attachment is pending; with multiple, the text is sent separately.
        const caption = trayAttachments.length === 1 && body.length > 0 ? body : undefined;
        await onSendAttachments(trayAttachments, caption);
        // If the text did not ride as a caption (multiple attachments) but the
        // user typed a body, dispatch it as its own message.
        if (caption === undefined && body.length > 0) {
          await onSend(body);
        }
        // Clear only on success so a failed enqueue keeps the tray + text.
        attachmentsStore.getState().clear();
        setDraft("");
      } else {
        await onSend(body);
        // Clear only on success so a failed enqueue keeps the user's text.
        setDraft("");
      }
    } catch {
      // Enqueue-time failure produces no timeline echo to fall back on, so
      // surface an honest inline error (AD-21) and keep the draft/tray so the
      // user can resend. Async delivery failures instead show as the message's
      // Failed send-state caption.
      setError(true);
    } finally {
      setSending(false);
    }
  }

  /** Open the native file picker and add each chosen path to the tray. */
  async function pickFiles() {
    if (!attachEnabled) {
      return;
    }
    try {
      const selection = await open({ multiple: true });
      if (selection == null) {
        // Dialog cancelled → no-op.
        return;
      }
      const paths = Array.isArray(selection) ? selection : [selection];
      attachmentsStore.getState().addMany(
        paths.map((path) => ({
          id: attachmentId(),
          kind: "path" as const,
          path,
          filename: basename(path),
        })),
      );
    } catch {
      // A dialog failure is non-fatal — the user can retry; nothing to surface.
    }
  }

  /**
   * Intercept a paste that carries an image: add it as a raw-bytes attachment
   * (dispatched later as a raw binary IPC body, never base64). A non-image paste
   * falls through to the default text paste unchanged.
   */
  function onPaste(e: ClipboardEvent<HTMLTextAreaElement>) {
    if (!attachEnabled) {
      return;
    }
    const imageItem = Array.from(e.clipboardData.items).find((it) => it.type.startsWith("image/"));
    if (!imageItem) {
      // Not an image → let the default text paste proceed.
      return;
    }
    const file = imageItem.getAsFile();
    if (!file) {
      return;
    }
    e.preventDefault();
    void file.arrayBuffer().then((bytes) => {
      const ext = file.type.split("/")[1] || "png";
      attachmentsStore.getState().add({
        id: attachmentId(),
        kind: "bytes",
        bytes,
        filename: file.name && file.name !== "" ? file.name : `pasted-image.${ext}`,
        mime: file.type,
        size: file.size,
      });
    });
  }

  /** Remove a pending attachment (a pre-upload cancel). */
  function removeAttachment(id: string) {
    attachmentsStore.getState().remove(id);
  }

  function cancelPending() {
    const wasEdit = pending?.mode === "edit";
    // Clear the pending context in the store (its return value is unused — this
    // component owns the pre-edit draft it restores).
    onCancelPending?.();
    if (wasEdit) {
      // Edit: restore the draft the user had before entering edit.
      setDraft(preEditDraft.current);
    }
    // Reply: leave the typed draft untouched.
  }

  function onKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    if (e.key === "Escape" && pending) {
      // Esc cancels the pending reply/edit without losing composer text.
      e.preventDefault();
      cancelPending();
      return;
    }
    if (
      e.key === "ArrowUp" &&
      !pending &&
      draft.length === 0 &&
      e.currentTarget.selectionStart === 0 &&
      onEmptyArrowUp
    ) {
      // ↑ in an empty composer opens edit on the last own message.
      e.preventDefault();
      onEmptyArrowUp();
      return;
    }
    // Enter sends; ⇧Enter (or any modifier) inserts a newline.
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      void send();
    }
  }

  return (
    <div className="flex flex-col gap-1">
      {pending && (
        <div className="flex items-center gap-2 rounded-md border border-border bg-muted/50 px-3 py-1.5">
          <div className="min-w-0 flex-1">
            {pending.mode === "reply" ? (
              <>
                <span className="block font-medium text-muted-foreground text-xs">
                  Replying to {pending.sender}
                </span>
                <span className="block truncate text-foreground text-xs">
                  {pending.bodyPreview}
                </span>
              </>
            ) : (
              <span className="block font-medium text-muted-foreground text-xs">
                Editing your message
              </span>
            )}
          </div>
          <Button
            type="button"
            variant="ghost"
            size="icon-xs"
            aria-label="Cancel"
            onClick={cancelPending}
          >
            ×
          </Button>
        </div>
      )}
      {/* Pending-attachment tray (Story 3.7): removable chips above the textarea,
          each showing the filename (+ size for pasted bytes). Removing a chip is a
          pre-upload cancel. */}
      {hasAttachments && pending?.mode !== "edit" && (
        <ul aria-label="Pending attachments" className="flex flex-wrap gap-1.5">
          {attachments.map((attachment) => (
            <li
              key={attachment.id}
              className="flex items-center gap-1.5 rounded-md border border-border bg-muted/50 py-1 pr-1 pl-2"
            >
              <span className="max-w-[180px] truncate text-xs" title={chipLabel(attachment)}>
                {chipLabel(attachment)}
              </span>
              {attachment.kind === "bytes" && (
                <span className="text-muted-foreground text-xs">{formatSize(attachment.size)}</span>
              )}
              <Button
                type="button"
                variant="ghost"
                size="icon-xs"
                aria-label={`Remove ${chipLabel(attachment)}`}
                onClick={() => removeAttachment(attachment.id)}
              >
                <X aria-hidden="true" className="size-3" />
              </Button>
            </li>
          ))}
        </ul>
      )}
      <div className="flex items-end gap-2">
        {attachEnabled && pending?.mode !== "edit" && (
          <Button
            type="button"
            variant="ghost"
            size="icon"
            aria-label="Attach file"
            disabled={disabled}
            onClick={() => void pickFiles()}
          >
            <Paperclip aria-hidden="true" />
          </Button>
        )}
        <Textarea
          aria-label="Message"
          placeholder="Write a message…"
          value={draft}
          disabled={disabled}
          onChange={(e) => {
            setDraft(e.target.value);
            if (error) {
              setError(false);
            }
          }}
          onKeyDown={onKeyDown}
          onPaste={onPaste}
          rows={1}
          // Autogrow via `field-sizing-content` (from the shadcn base) capped at
          // eight lines, then scroll.
          className={cn("max-h-[calc(8*1.5rem+1rem)] min-h-9 resize-none")}
        />
        <Button
          type="button"
          onClick={() => void send()}
          disabled={!canSend}
          aria-label={pending?.mode === "edit" ? "Save edit" : "Send message"}
        >
          {pending?.mode === "edit" ? "Save" : "Send"}
        </Button>
      </div>
      {error && (
        <p role="alert" className="text-destructive text-xs">
          Couldn't send. Check your connection and try again.
        </p>
      )}
    </div>
  );
}
