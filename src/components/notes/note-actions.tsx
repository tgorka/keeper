/**
 * The per-note actions menu (Story 45.17, FR-195, UX-DR78; Story 46.5).
 *
 * **A menu rather than a row of header buttons.** Story 45.17 drew the line at
 * "acts on the note as an object" and left the five view verbs outside; Story
 * 46.5 found the reason that line could not hold. Six controls plus two
 * truncating spans do not fit the 560 px quick-capture window (`notes_window.rs
 * :91`), the row does not wrap, and this control is its last child — so the one
 * verb nobody could afford to lose was the first one off the screen. The owner
 * reported it as "I still see no way to delete notes", which was literally
 * true. Everything that opens a panel or a dialog now lives in here; the
 * header keeps one control beside it. Children render above Delete.
 *
 * **The trigger is a word, not an icon.** It stood among five text buttons as a
 * bare `⋯`, and an icon among words reads as decoration. `bridge-card.tsx:266`
 * is the house's other object-level dropdown sitting in a row of words and it
 * spells its trigger "Manage"; this one spells "Actions". The visible text is a
 * prefix of the accessible name rather than a different word, because a control
 * whose label and whose accessible name disagree cannot be operated by anyone
 * saying what they see (WCAG 2.5.3).
 *
 * Destructive last, and behind a separator: nothing has to reason about
 * position, the item under the cursor when the menu opens is never the one that
 * removes the note, and the confirmation is still the only thing that deletes.
 *
 * The trigger's accessible name carries the note's title, because a screen
 * reader walking a workspace with several note panels open would otherwise hear
 * the same control in each of them.
 */
import { type ReactNode, useState } from "react";
import { NoteDeleteDialog } from "@/components/notes/note-delete-dialog";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

/**
 * The trigger's accessible name, suffixed with the note's title. Worded like
 * `NOTE_PATH_ACTIONS_LABEL` in the properties panel, which is the same
 * affordance over a different object.
 */
export const NOTE_ACTIONS_LABEL = "Actions for";

/**
 * The word ON the trigger. A prefix of `NOTE_ACTIONS_LABEL`, deliberately: the
 * accessible name is the visible label plus the note's title, so speech input
 * and a screen reader and an eye all name the same control.
 */
export const NOTE_ACTIONS_TEXT = "Actions";

/** The delete item's label, kept verbatim so a test and a user agree. */
export const NOTE_DELETE_LABEL = "Delete note";

export function NoteActions({
  vaultId,
  noteId,
  title,
  onDeleted,
  children,
}: {
  vaultId: string;
  noteId: string;
  /** What the note is called, for the trigger's accessible name and nothing else. */
  title: string;
  /**
   * The note was deleted. Optional because the panel model already closes a
   * target whose note went away (Story 45.1) — this is for a host that also has
   * a list to re-read.
   */
  onDeleted?: () => void;
  /**
   * The note's other verbs, rendered above Delete — the header's four panel and
   * navigation items since 46.5, plus whatever other stories contribute. Order
   * is the caller's; only the destructive item's position is this component's.
   */
  children?: ReactNode;
}) {
  const [confirming, setConfirming] = useState(false);

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button size="sm" variant="ghost" aria-label={`${NOTE_ACTIONS_LABEL} ${title}`}>
            {NOTE_ACTIONS_TEXT}
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          {children}
          {/* The destructive verb gets a break above it as well as last place.
              With one item it was the whole menu; with six, position alone is
              not much of a guard against a hand travelling down the list. */}
          <DropdownMenuSeparator />
          <DropdownMenuItem variant="destructive" onSelect={() => setConfirming(true)}>
            {NOTE_DELETE_LABEL}
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      {/* Outside the menu, because Radix unmounts the menu's content on select
          and a dialog mounted inside it would be torn down in the same tick it
          was asked for. */}
      {confirming && (
        <NoteDeleteDialog
          vaultId={vaultId}
          noteId={noteId}
          onClose={() => setConfirming(false)}
          onDeleted={() => onDeleted?.()}
        />
      )}
    </>
  );
}
