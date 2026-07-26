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
 * Not a live region: a `reset` batch can render many historical redacted rows at
 * once, and each would announce itself. Overrides the `Alert` primitive's default
 * assertive `role="alert"` with the static `note` role, per the
 * `history-boundary.tsx` precedent that only transient states are announced.
 */
import { Alert, AlertDescription } from "@/components/ui/alert";

/** The honest, verbatim redaction copy (Story 3.8 fixed string). */
export const REDACTED_STUB_TEXT = "Message deleted";

export function RedactedStub() {
  return (
    <Alert role="note" className="my-3">
      <AlertDescription>{REDACTED_STUB_TEXT}</AlertDescription>
    </Alert>
  );
}
