/**
 * The note list's empty states (Epic 37, UX-DR35/UX-DR37).
 *
 * Four states, and telling them apart is the whole job. "Nothing here" and
 * "nothing matches" look identical on screen and mean opposite things: the first
 * is an invitation to write, the second is an invitation to widen. Wording them
 * the same would leave someone staring at an empty vault wondering which of
 * their chips to remove, or at an over-filtered list wondering where their notes
 * went.
 *
 * Every state carries exactly one action, so the surface never dead-ends. None
 * of them is a toast: each is true for as long as it is true, and each is
 * dismissed by fixing the thing it describes.
 */
import { Button } from "@/components/ui/button";

/** Which of the four states the list is in. */
export type NotesEmptyKind = "no-vault" | "empty-vault" | "no-matches" | "no-search-matches";

/** The exact copy, kept verbatim from the experience spine. */
const COPY: Record<NotesEmptyKind, { message: string; action: string }> = {
  "no-vault": {
    message: "No notes vault yet. Flag a folder you already sync and it becomes one.",
    action: "Open Settings → Sync",
  },
  "empty-vault": {
    message: "This vault is empty. Write the first note.",
    action: "New Note",
  },
  "no-matches": {
    message: "No notes match these filters.",
    action: "Clear filters",
  },
  "no-search-matches": {
    message: "No matches in this vault.",
    action: "Clear search",
  },
};

/**
 * Render one empty state.
 *
 * The chips stay on screen above this — they are not cleared to make room for
 * it — because the fastest way out of "nothing matches" is removing the one chip
 * that went too far, and that is only possible if the chips are still there.
 */
export function NotesEmptyState({
  kind,
  onAction,
}: {
  kind: NotesEmptyKind;
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
