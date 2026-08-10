/**
 * The one confirmation for deleting a note or a space (Story 45.17, FR-195,
 * UX-DR78).
 *
 * **One component for three surfaces, because a delete is one act.** It is
 * opened from the note editor's actions menu, from the note list's `Delete`
 * key, and from a space row in the sidebar. Three dialogs would be three
 * chances to forget a sentence, and the one that would get forgotten is the one
 * saying keeper kept a copy — which is the sentence that makes the button
 * pressable at all.
 *
 * **Every word in it is Rust's** (`NoteDeletePlanVm`), for Story 45.3's reason:
 * the sentences are composed by code that knows what the removal does, so the
 * dialog cannot promise something the command will not do. A space is a note
 * with a marker, and whether the confirmation says "this space stays deleted"
 * turns on `keeper.default` in the file — a fact this surface cannot read and
 * must not guess.
 *
 * **Nothing is written by asking.** The plan is a separate command, and
 * declining closes the dialog having called nothing. The delete happens on one
 * press of one button, and the removal is `notes_vault::trash_note` (NFR-30) —
 * never an unlink.
 */
import { useEffect, useState } from "react";
import {
  AlertDialog,
  AlertDialogAction,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { deleteNote } from "@/hooks/use-notes-actions";
import { type NoteDeletePlanVm, notesDeletePlan } from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

/** The confirming button's label, kept verbatim so a test and a user agree. */
export const NOTE_DELETE_CONFIRM = "Delete";

/** The declining button's label. */
export const NOTE_DELETE_CANCEL = "Cancel";

/** What the dialog says while Rust is composing the plan. */
export const NOTE_DELETE_READING = "Reading what this would remove\u2026";

/** What the dialog says when the plan could not be composed. */
export const NOTE_DELETE_NO_PLAN =
  "keeper couldn't work out what deleting this would remove, so it hasn't offered to.";

/** What the dialog says when the delete itself was refused. */
export const NOTE_DELETE_FAILED = "keeper couldn't delete that. Nothing has been removed.";

/** The plan body's test id, so a test reads Rust's sentences and not a paraphrase. */
export const NOTE_DELETE_TESTID = "note-delete-confirm";

export function NoteDeleteDialog({
  vaultId,
  noteId,
  onClose,
  onDeleted,
}: {
  vaultId: string;
  /** The note or space to remove. Mounting the dialog is what asks for a plan. */
  noteId: string;
  /** Declined, or finished. The host unmounts this. */
  onClose: () => void;
  /**
   * The delete landed. The host decides what that means for it — the sidebar
   * re-reads its spaces, the pane drops its cursor — because a dialog that
   * reached into three different stores would be a dialog with three reasons
   * to change.
   */
  onDeleted: () => void;
}) {
  const [plan, setPlan] = useState<NoteDeletePlanVm | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);

  useEffect(() => {
    let live = true;
    void notesDeletePlan(vaultId, noteId)
      .then((composed) => {
        if (live) {
          setPlan(composed);
        }
      })
      .catch((raw: unknown) => {
        // Said out loud and with no Delete button beside it. A confirmation
        // that cannot say what it would remove must not offer to remove it.
        if (live) {
          setFailure(syncErrorMessage(raw, NOTE_DELETE_NO_PLAN));
        }
      });
    return () => {
      live = false;
    };
  }, [vaultId, noteId]);

  const confirm = (): void => {
    setDeleting(true);
    setFailure(null);
    void deleteNote(vaultId, noteId)
      .then(() => {
        onDeleted();
        onClose();
      })
      .catch((raw: unknown) => {
        setDeleting(false);
        setFailure(syncErrorMessage(raw, NOTE_DELETE_FAILED));
      });
  };

  return (
    <AlertDialog
      open
      onOpenChange={(open) => {
        if (!open) {
          onClose();
        }
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          {/* Three states and three headings. "Reading…" while a plan that
              failed is on screen would be keeper still claiming to be working
              on the answer it has just given up on. */}
          <AlertDialogTitle>
            {plan !== null
              ? plan.question
              : failure !== null
                ? NOTE_DELETE_NO_PLAN
                : NOTE_DELETE_READING}
          </AlertDialogTitle>
          <AlertDialogDescription data-testid={NOTE_DELETE_TESTID}>
            {plan === null ? "" : `${plan.consequence} ${plan.recovery}`}
          </AlertDialogDescription>
        </AlertDialogHeader>
        {/* The path, because two notes may carry one title and this is the only
            thing on screen that tells them apart. */}
        {plan !== null && (
          <p className="truncate font-mono text-muted-foreground text-xs">{plan.path}</p>
        )}
        {failure !== null && (
          <p role="alert" className="text-destructive text-sm">
            {failure}
          </p>
        )}
        <AlertDialogFooter>
          <AlertDialogCancel>{NOTE_DELETE_CANCEL}</AlertDialogCancel>
          {/* Absent, not disabled, until there is a plan: a Delete button
              beside "keeper couldn't work out what this would remove" invites
              a press at the one thing keeper has just said it cannot describe. */}
          {plan !== null && (
            <AlertDialogAction
              variant="destructive"
              disabled={deleting}
              // `preventDefault` keeps the dialog mounted through the command.
              // Radix's Action closes on click, and a dialog that has already
              // gone cannot say that the delete was refused — the person would
              // be left with a row still on screen and no sentence explaining
              // why. `composeEventHandlers` honours a prevented default, so
              // this is Radix's own opt-out and not a fight with it.
              onClick={(event) => {
                event.preventDefault();
                confirm();
              }}
            >
              {NOTE_DELETE_CONFIRM}
            </AlertDialogAction>
          )}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
