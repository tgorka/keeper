/**
 * Message composer (FR-9, UX-DR5).
 *
 * A controlled {@link Textarea} that autogrows to eight lines then scrolls, with
 * a send {@link Button}. Enter sends; ⇧Enter inserts a newline; a whitespace-only
 * body never dispatches. The draft lives in local `useState` (no IPC round-trip
 * on keystroke, so input stays under one frame) and is cleared on a successful
 * send. This component owns no IPC knowledge — the parent wires `onSend`.
 */
import { type KeyboardEvent, useState } from "react";
import { Button } from "@/components/ui/button";
import { Textarea } from "@/components/ui/textarea";
import { cn } from "@/lib/utils";

interface ComposerProps {
  /**
   * Dispatch the trimmed body. Resolves on success (the draft then clears);
   * rejects if the send could not be enqueued (the draft is kept so the user can
   * retry). The parent wires this to the IPC `sendText`.
   */
  onSend: (body: string) => Promise<void>;
  /** When `true`, the composer is inert (no room loaded). */
  disabled?: boolean;
}

export function Composer({ onSend, disabled = false }: ComposerProps) {
  const [draft, setDraft] = useState("");
  const [sending, setSending] = useState(false);
  const [error, setError] = useState(false);

  const canSend = draft.trim().length > 0 && !disabled && !sending;

  async function send() {
    const body = draft.trim();
    if (body.length === 0 || disabled || sending) {
      // Whitespace-only / disabled / in-flight: never dispatch.
      return;
    }
    setSending(true);
    setError(false);
    try {
      await onSend(body);
      // Clear only on success so a failed enqueue keeps the user's text.
      setDraft("");
    } catch {
      // Enqueue-time failure produces no timeline echo to fall back on, so
      // surface an honest inline error (AD-21) and keep the draft so the user
      // can resend. Async delivery failures instead show as the message's
      // Failed send-state caption.
      setError(true);
    } finally {
      setSending(false);
    }
  }

  function onKeyDown(e: KeyboardEvent<HTMLTextAreaElement>) {
    // Enter sends; ⇧Enter (or any modifier) inserts a newline.
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      void send();
    }
  }

  return (
    <div className="flex flex-col gap-1">
      <div className="flex items-end gap-2">
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
          rows={1}
          // Autogrow via `field-sizing-content` (from the shadcn base) capped at
          // eight lines, then scroll.
          className={cn("max-h-[calc(8*1.5rem+1rem)] min-h-9 resize-none")}
        />
        <Button
          type="button"
          onClick={() => void send()}
          disabled={!canSend}
          aria-label="Send message"
        >
          Send
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
