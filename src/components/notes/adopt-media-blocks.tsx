/**
 * "Use the media player in recording notes…": turn every recording note in a
 * vault whose body still embeds its own session's files one by one into the
 * one `keeper-media` block a new note is written with.
 *
 * Rust does the rewrite (`recording_notes_adopt_media_block`) and decides
 * which notes qualify: a note whose embeds were edited by hand beyond the
 * stub's shape is skipped and named, and anything else in a note stays
 * byte-identical. The dialog asks first with a dry run's count, because this
 * writes into many notes at once, and says what happened after.
 */
import { useEffect, useState } from "react";
import {
  AlertDialog,
  AlertDialogCancel,
  AlertDialogContent,
  AlertDialogDescription,
  AlertDialogFooter,
  AlertDialogHeader,
  AlertDialogTitle,
} from "@/components/ui/alert-dialog";
import { Button } from "@/components/ui/button";
import { type MediaAdoptionVm, recordingNotesAdoptMediaBlock } from "@/lib/ipc/client";
import { syncErrorMessage } from "@/lib/stores/sync";

export const ADOPT_MEDIA_BLOCKS_LABEL = "Use the media player in recording notes…";

function notes(count: number): string {
  return count === 1 ? "1 recording note" : `${count} recording notes`;
}

/** The notes left alone, as a sentence, or null when there are none. */
function skippedSentence(skipped: readonly string[]): string | null {
  if (skipped.length === 0) return null;
  return `Left as they are, because their embeds were edited by hand: ${skipped.join(", ")}.`;
}

export function AdoptMediaBlocksDialog({
  vaultId,
  open,
  onClose,
}: {
  vaultId: string;
  open: boolean;
  onClose: () => void;
}) {
  const [plan, setPlan] = useState<MediaAdoptionVm | null>(null);
  const [done, setDone] = useState<MediaAdoptionVm | null>(null);
  const [problem, setProblem] = useState<string | null>(null);
  const [busy, setBusy] = useState(false);

  useEffect(() => {
    setPlan(null);
    setDone(null);
    setProblem(null);
    if (!open) return;
    let live = true;
    void recordingNotesAdoptMediaBlock(vaultId, true).then(
      (counted) => {
        if (live) setPlan(counted);
      },
      (cause: unknown) => {
        if (live) setProblem(syncErrorMessage(cause));
      },
    );
    return () => {
      live = false;
    };
  }, [vaultId, open]);

  async function adopt() {
    setBusy(true);
    try {
      setDone(await recordingNotesAdoptMediaBlock(vaultId, false));
      setProblem(null);
    } catch (cause) {
      setProblem(syncErrorMessage(cause));
    } finally {
      setBusy(false);
    }
  }

  const shown = done ?? plan;
  const skipped = shown === null ? null : skippedSentence(shown.skipped);
  return (
    <AlertDialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Use the media player in recording notes</AlertDialogTitle>
          <AlertDialogDescription>
            {done !== null
              ? `Done: ${notes(done.changed)} now play in one media player.`
              : plan === null
                ? "Counting the recording notes in this vault…"
                : plan.changed === 0
                  ? "No recording note in this vault still plays its files one by one."
                  : `${notes(plan.changed)} still play their files one by one. keeper replaces those embeds with one media player per note, and changes nothing else in them.`}
          </AlertDialogDescription>
        </AlertDialogHeader>
        {skipped !== null && <p className="text-muted-foreground text-sm">{skipped}</p>}
        {problem !== null && (
          <p role="alert" className="text-destructive text-sm">
            {problem}
          </p>
        )}
        <AlertDialogFooter>
          <AlertDialogCancel>{done === null ? "Cancel" : "Close"}</AlertDialogCancel>
          {done === null && plan !== null && plan.changed > 0 && (
            // A Button, not AlertDialogAction: the action closes the dialog, and
            // the result is said here before it closes.
            <Button disabled={busy} onClick={() => void adopt()}>
              Use the media player
            </Button>
          )}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
