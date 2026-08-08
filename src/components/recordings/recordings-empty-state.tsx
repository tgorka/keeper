/**
 * The recordings browser's empty states (Story 42.3, Epic 42).
 *
 * Two states, and telling them apart is the whole job. "Nothing recorded yet"
 * and "nothing matches this filter" render the same empty list and mean
 * opposite things: the first is an invitation to record, the second is an
 * invitation to widen. Wording them the same would leave someone staring at an
 * archive they have never written to wondering which chip to remove, or at an
 * over-filtered list wondering where every session they have ever recorded
 * went.
 *
 * Each state carries exactly one action, so the surface never dead-ends —
 * modelled on {@link NotesEmptyState}, which answers the same question for the
 * note list.
 */
import { Button } from "@/components/ui/button";

/** Which of the two states the list is in. */
export type RecordingsEmptyKind = "no-recordings" | "no-matches";

/** The exact copy. Kept here so neither sentence can be reworded in isolation. */
const COPY: Record<RecordingsEmptyKind, { message: string; action: string }> = {
  "no-recordings": {
    message: "Nothing recorded yet. Record a session and it lands here.",
    action: "Go to Recording",
  },
  "no-matches": {
    message: "No recordings match this filter.",
    action: "Clear filters",
  },
};

/**
 * Render one empty state.
 *
 * The filter row and its chips stay on screen above this — they are not
 * cleared to make room for it — because the fastest way out of "nothing
 * matches" is removing the one chip that went too far, and that is only
 * possible if the chips are still there.
 */
export function RecordingsEmptyState({
  kind,
  onAction,
}: {
  kind: RecordingsEmptyKind;
  onAction: () => void;
}) {
  const { message, action } = COPY[kind];
  return (
    <div className="flex flex-1 flex-col items-center justify-center gap-3 p-6">
      <p className="max-w-[36ch] text-center text-muted-foreground text-sm">{message}</p>
      <Button type="button" variant="outline" size="sm" onClick={onAction}>
        {action}
      </Button>
    </div>
  );
}
