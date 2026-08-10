/**
 * The per-note actions menu (Story 45.17, FR-195, UX-DR78).
 *
 * **A menu rather than a fifth header button**, and the line between the two is
 * what the control acts on. Attach, Attachments, Properties, History and Show
 * in Files all change what this pane is *showing*; the items in here act on the
 * whole note as an object — delete it, export it, open it in a window of its
 * own. Those are rarer, they are heavier, and one of them is destructive, which
 * is a bad neighbour for a row of one-press toggles.
 *
 * The slot is deliberate. Story 45.21 and Story 45.15 each need exactly one
 * per-note verb, and three stories inventing three homes for them is how a
 * surface ends up with an Export in the header, an Export in a context menu and
 * neither of them the one people find. Children render above Delete: destructive
 * last, so nothing has to reason about position, and so the item under the
 * cursor when the menu opens is never the one that removes the note.
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
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";

/**
 * The trigger's accessible name, suffixed with the note's title. Worded like
 * `NOTE_PATH_ACTIONS_LABEL` in the properties panel, which is the same
 * affordance over a different object.
 */
export const NOTE_ACTIONS_LABEL = "Actions for";

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
  /** Other stories' per-note verbs, rendered above Delete. */
  children?: ReactNode;
}) {
  const [confirming, setConfirming] = useState(false);

  return (
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          <Button size="sm" variant="ghost" aria-label={`${NOTE_ACTIONS_LABEL} ${title}`}>
            <MoreHorizontal aria-hidden="true" className="size-4" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          {children}
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
