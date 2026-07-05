/**
 * Typing indicator row (Story 3.9, typing, UX-DR10).
 *
 * Renders "<name> is typing…", "<a> and <b> are typing…", or "Several people are
 * typing…" from the Rust-streamed set of *other* members currently typing. Copy is
 * sentence case with no exclamation (UX-DR10). `aria-live="polite"` so a screen
 * reader announces a change without interrupting. Renders nothing when nobody is
 * typing (an empty row), so the layout does not jump when it clears.
 */
import type { TypistVm } from "@/lib/ipc/client";

interface TypingIndicatorProps {
  /** The members currently typing (others than the account's own user). */
  typists: TypistVm[];
}

/** A typist's display label — its resolved display name, else the opaque id. */
function labelOf(typist: TypistVm): string {
  return typist.displayName ?? typist.userId;
}

/** Compose the honest typing copy for the current set of typists (UX-DR10). */
function typingText(typists: TypistVm[]): string {
  if (typists.length === 1) {
    return `${labelOf(typists[0])} is typing…`;
  }
  if (typists.length === 2) {
    return `${labelOf(typists[0])} and ${labelOf(typists[1])} are typing…`;
  }
  return "Several people are typing…";
}

export function TypingIndicator({ typists }: TypingIndicatorProps) {
  // `aria-live` must stay mounted to announce transitions; render an empty,
  // zero-height live region when nobody is typing rather than unmounting.
  return (
    <div
      aria-live="polite"
      className="min-h-4 px-1 text-muted-foreground text-xs leading-4"
      data-testid="typing-indicator"
    >
      {typists.length > 0 ? typingText(typists) : null}
    </div>
  );
}
