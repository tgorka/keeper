/**
 * The redacted-message timeline stub (Story 3.8, FR-15, UX honesty).
 *
 * A message that was deleted for everyone (its own sender's redaction, or anyone
 * else's) renders this explicit, never-blank stub instead of vanishing silently.
 * The redacted event carries no body — only an honest "Message deleted" note — and
 * the SDK turns the live message into this row in place via a `Set` diff (keeper
 * never removes or re-indexes it; local archive retention is Story 5.2). Copy
 * follows UX-DR10: sentence case, no exclamation.
 *
 * This account's own deletion is queued, so the stub shows before the server has
 * it: `sendState` says where it stands. `sending` reads "Deleting…"; `failed` —
 * the server refused it, the message is still there for everyone, and the room's
 * later messages wait behind it — reads "Not deleted" with Retry and Cancel (which
 * brings the message back); `sent` and `null` read "Message deleted".
 *
 * Not a live region while it is settled: a `reset` batch can render many
 * historical redacted rows at once, and each would announce itself. Overrides the
 * `Alert` primitive's default assertive `role="alert"` with the static `note` role,
 * per the `history-boundary.tsx` precedent that only transient states are
 * announced — a refused deletion is one, and is said as a `status`.
 */
import { Alert, AlertDescription } from "@/components/ui/alert";
import { Button } from "@/components/ui/button";
import type { SendState } from "@/lib/ipc/client";

/** The honest, verbatim redaction copy (Story 3.8 fixed string). */
export const REDACTED_STUB_TEXT = "Message deleted";
export const REDACTION_PENDING_TEXT = "Deleting…";
export const REDACTION_FAILED_TEXT = "Not deleted";

interface RedactedStubProps {
  /** The item's render key, handed to Retry and Cancel. */
  itemKey?: string;
  /** Where this account's own deletion stands; `null` for any other. */
  sendState?: SendState | null;
  onRetry?: (key: string) => void;
  onCancel?: (key: string) => void;
}

export function RedactedStub({
  itemKey = "",
  sendState = null,
  onRetry,
  onCancel,
}: RedactedStubProps) {
  if (sendState === "failed") {
    return (
      <Alert role="status" className="my-3">
        <AlertDescription className="flex flex-wrap items-center gap-2">
          <span className="text-destructive">{REDACTION_FAILED_TEXT}</span>
          <Button type="button" variant="destructive" size="xs" onClick={() => onRetry?.(itemKey)}>
            Retry
          </Button>
          <Button type="button" variant="outline" size="xs" onClick={() => onCancel?.(itemKey)}>
            Cancel
          </Button>
        </AlertDescription>
      </Alert>
    );
  }
  return (
    <Alert role="note" className="my-3">
      <AlertDescription>
        {sendState === "sending" ? REDACTION_PENDING_TEXT : REDACTED_STUB_TEXT}
      </AlertDescription>
    </Alert>
  );
}
