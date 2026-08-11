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
 * **The trigger is an icon again, and for the same reason it stopped being one
 * (Story 48.9).** 46.5's ruling was not "words beat icons": it was that a bare
 * `⋯` STANDING AMONG FIVE TEXT BUTTONS reads as decoration. The row it stands
 * in is now four icons and a paperclip, and a word among pictures reads exactly
 * as badly as a picture among words did — so the rule holds and the answer
 * flips. `MoreHorizontal` and not some new glyph: `properties-panel.tsx`'s
 * `NOTE_PATH_ACTIONS_LABEL` is the same affordance over a different object and
 * already draws it, and this trigger's label was worded after that one.
 *
 * {@link NOTE_ACTIONS_TEXT} is therefore no longer rendered as text — it is the
 * `title` a pointer gets, and the prefix of the accessible name, which still
 * carries the note's title after it. A control whose spoken name does not
 * contain the word its tooltip shows cannot be operated by anyone saying what
 * they see (WCAG 2.5.3).
 *
 * Destructive last, and behind a separator: nothing has to reason about
 * position, the item under the cursor when the menu opens is never the one that
 * removes the note, and the confirmation is still the only thing that deletes.
 *
 * The trigger's accessible name carries the note's title, because a screen
 * reader walking a workspace with several note panels open would otherwise hear
 * the same control in each of them.
 */
import { MoreHorizontal } from "lucide-react";
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
 * The word FOR the trigger: its tooltip, and the prefix of
 * `NOTE_ACTIONS_LABEL`. Not rendered as text since 48.9 — the trigger draws
 * `MoreHorizontal` — but still the one word this control answers to, so speech
 * input, a screen reader, a tooltip and a test all name it the same way.
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
          <Button
            type="button"
            size="icon-sm"
            variant="ghost"
            // The name carries the note's title so a workspace with several
            // note panels open does not announce the same control in each of
            // them; the tooltip carries only the word, because the title is
            // already on screen an inch to the left.
            aria-label={`${NOTE_ACTIONS_LABEL} ${title}`}
            title={NOTE_ACTIONS_TEXT}
          >
            <MoreHorizontal aria-hidden="true" />
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
