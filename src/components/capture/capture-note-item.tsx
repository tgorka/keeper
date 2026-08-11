/**
 * "Open in a capture window", as one item in the note's actions menu (Story
 * 45.15, FR-191).
 *
 * This is the entry point for the sentence the story exists for: **the small
 * window is a way of looking at a note, not a special kind of note.** Until
 * now a capture window could only ever hold a note capture itself had made,
 * which is what made quick capture feel like a separate product with a separate
 * inbox. One menu item ends that, and it needs no new concept to do it —
 * `openNoteAsCapture` is `notes_capture_open` with a note target, the same
 * command the prewarmed window is raised by.
 *
 * # One item, rendered by somebody else's menu
 *
 * It lives in Story 45.17's `NoteActions`, beside Export and above Delete,
 * rather than in a header button or a context menu of its own. Three stories
 * each inventing a home for one per-note verb is how a surface ends up with the
 * verb in three places and the discoverable one in none.
 *
 * Exactly one `DropdownMenuItem` and no wrapper: a wrapping element breaks
 * Radix's typeahead and its arrow-key roving, so the item would render and stop
 * being reachable by keyboard — which on a menu is most of the way to not
 * existing.
 *
 * # The gate
 *
 * `capabilities.notes`, which is `sync && desktop` computed in Rust (AD-27).
 * That is the same flag `use-notes-shortcut.ts` gates the in-app capture chord
 * on, so this item is present exactly where ⌘⌥K is — one answer to "can this
 * build open a capture window" rather than two. Deliberately **not**
 * `revealInFileManager`, which answers whether a file manager exists and
 * nothing about whether a window can.
 */
import { DropdownMenuItem } from "@/components/ui/dropdown-menu";
import { openNoteAsCapture } from "@/hooks/use-notes-actions";
import { useCapabilitiesStore } from "@/lib/stores/capabilities";

/** The item's label, kept verbatim so a test and a user agree. */
export const CAPTURE_NOTE_LABEL = "Open in a capture window";

export function CaptureNoteItem({ vaultId, noteId }: { vaultId: string; noteId: string }) {
  const notes = useCapabilitiesStore((state) => state.capabilities.notes);
  if (!notes) {
    return null;
  }
  return (
    <DropdownMenuItem
      onSelect={() => {
        // Fire and forget: Rust raises an existing window or creates one, and
        // either way the menu is closing. A rejection here would have nowhere
        // to render — the menu is gone — and the failure it would report is
        // "the window did not open", which the absent window already says.
        void openNoteAsCapture(vaultId, noteId);
      }}
    >
      {CAPTURE_NOTE_LABEL}
    </DropdownMenuItem>
  );
}
