/**
 * Top-of-timeline history-boundary row (Story 3.9, pagination, epic UX honesty).
 *
 * Sits at the top of the conversation list and states — honestly, per state — what
 * is happening with older history:
 * - `paginating`: a spinner + "Older history loads from your homeserver"
 *   (`aria-busy`), while a back-pagination request is in flight.
 * - `offline`: "You're offline — older messages will load when you reconnect", with
 *   NO spinner (it stops, never spins forever).
 * - `atStart`: "This is the start of the conversation" — the homeserver has no more
 *   older history and no further pagination is attempted.
 * - `error`: an honest, retriable failure message with a Retry button.
 * - `idle`: renders nothing (there is more history but nothing is loading yet — the
 *   scroll trigger will paginate as the user nears the top).
 *
 * Copy follows UX-DR10: sentence case, no exclamation marks. Static states are not
 * `aria-live` flooders — only the transient `paginating` row is `aria-busy`.
 */
import { Loader2 } from "lucide-react";
import { Button } from "@/components/ui/button";

/** The honest boundary states, mapped from the Rust pagination status + connectivity. */
export type HistoryBoundaryState = "idle" | "paginating" | "offline" | "atStart" | "error";

interface HistoryBoundaryProps {
  /** Which honest boundary state to render. */
  state: HistoryBoundaryState;
  /** Retry a failed pagination (only wired/rendered in the `error` state). */
  onRetry?: () => void;
}

export function HistoryBoundary({ state, onRetry }: HistoryBoundaryProps) {
  if (state === "idle") {
    return null;
  }

  if (state === "paginating") {
    return (
      <div
        aria-busy="true"
        role="status"
        className="flex items-center justify-center gap-2 py-3 text-muted-foreground text-xs"
      >
        <Loader2 aria-hidden="true" className="size-3.5 animate-spin" />
        <span>Older history loads from your homeserver</span>
      </div>
    );
  }

  if (state === "offline") {
    return (
      <div className="flex items-center justify-center py-3 text-muted-foreground text-xs">
        <span>You're offline — older messages will load when you reconnect</span>
      </div>
    );
  }

  if (state === "atStart") {
    return (
      <div className="flex items-center justify-center py-3 text-muted-foreground text-xs">
        <span>This is the start of the conversation</span>
      </div>
    );
  }

  // state === "error"
  return (
    <div
      role="alert"
      className="flex items-center justify-center gap-2 py-3 text-muted-foreground text-xs"
    >
      <span>Couldn't load older messages.</span>
      {onRetry && (
        <Button type="button" variant="outline" size="xs" onClick={onRetry}>
          Retry
        </Button>
      )}
    </div>
  );
}
