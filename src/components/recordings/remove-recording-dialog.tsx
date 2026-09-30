/**
 * The one confirmation for removing a recording — from a note's widget and
 * from a Recordings row alike, because a removal is one act.
 *
 * Mounting it asks Rust what the removal would delete: the session folder,
 * its size and files, the drive it syncs through and the notes whose widget
 * goes with it. Nothing is removed by asking; the one destructive button is
 * absent until that answer is on screen, and the dialog stays open through the
 * removal so a refusal is said where it was asked.
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
import { type CountNoun, countLabel } from "@/lib/count-label";
import {
  type RecordingNoteFailureVm,
  type RecordingRemovalPreviewVm,
  type RecordingRemovedVm,
  recordingRemove,
  recordingRemovePreview,
} from "@/lib/ipc/client";
import { formatSize } from "@/lib/recording-format";
import { syncErrorMessage } from "@/lib/stores/sync";

/** The item that opens this dialog, in the widget's ⋯ and a Recordings row's. */
export const REMOVE_RECORDING_LABEL = "Remove recording…";
export const REMOVE_RECORDING_TITLE = "Remove this recording?";
export const REMOVE_RECORDING_CONFIRM = "Remove recording";
export const REMOVE_RECORDING_CANCEL = "Cancel";
export const REMOVE_RECORDING_READING = "Reading what this would remove…";
export const REMOVE_RECORDING_NO_PLAN =
  "keeper couldn't work out what removing this recording would delete, so it hasn't offered to.";
export const REMOVE_RECORDING_FAILED =
  "keeper couldn't remove the recording. Nothing has been deleted.";
export const REMOVE_RECORDING_DONE_TITLE = "The recording is removed";
export const REMOVE_RECORDING_STILL_NAMED =
  "These notes could not be changed and still name it. Remove its widget there by hand:";
export const REMOVE_RECORDING_CLOSE = "Close";

const FILES: CountNoun = { one: "file", many: "files" };

/**
 * Where the removal reaches, and whether anything still holds the recording
 * after it. Only a recording that reached the drive's remote is in its
 * history; one that is committed here, or not even that, is deleted for good.
 */
function reachSentence(drive: string | null, durability: string): string {
  if (drive === null) {
    return "It is deleted from this Mac, the only place it is.";
  }
  if (durability === "pushed" || durability === "verified") {
    return `It is deleted from ${drive} on every device. The drive's history still has it.`;
  }
  if (durability === "committed") {
    return "This recording has not left this Mac yet — removing it deletes the only copy.";
  }
  return `This recording has not reached ${drive}'s history yet — removing it deletes the only copy.`;
}

export function RemoveRecordingDialog({
  sessionId,
  onClose,
  onRemoved,
}: {
  /** The recording to remove. Mounting the dialog is what asks what that deletes. */
  sessionId: string;
  /** Declined, or finished. The host unmounts this. */
  onClose: () => void;
  /** The recording is gone: the host takes its widget, or its row, away. */
  onRemoved: (removed: RecordingRemovedVm) => void;
}) {
  const [plan, setPlan] = useState<RecordingRemovalPreviewVm | null>(null);
  const [failure, setFailure] = useState<string | null>(null);
  const [removing, setRemoving] = useState(false);
  /** Set once the recording is gone and some notes could not be changed. */
  const [stillNamed, setStillNamed] = useState<RecordingNoteFailureVm[] | null>(null);

  useEffect(() => {
    let live = true;
    void recordingRemovePreview(sessionId).then(
      (composed) => {
        if (live) setPlan(composed);
      },
      (cause: unknown) => {
        if (live) setFailure(syncErrorMessage(cause, REMOVE_RECORDING_NO_PLAN));
      },
    );
    return () => {
      live = false;
    };
  }, [sessionId]);

  const confirm = (): void => {
    setRemoving(true);
    setFailure(null);
    void recordingRemove(sessionId).then(
      (removed) => {
        onRemoved(removed);
        // A note that still names the recording is said here, where the
        // removal was asked for, before the dialog lets go.
        if (removed.notesFailed.length > 0) setStillNamed(removed.notesFailed);
        else onClose();
      },
      (cause: unknown) => {
        setRemoving(false);
        setFailure(syncErrorMessage(cause, REMOVE_RECORDING_FAILED));
      },
    );
  };

  if (stillNamed !== null) {
    return (
      <AlertDialog
        open
        onOpenChange={(open) => {
          if (!open) onClose();
        }}
      >
        <AlertDialogContent>
          <AlertDialogHeader>
            <AlertDialogTitle>{REMOVE_RECORDING_DONE_TITLE}</AlertDialogTitle>
            <AlertDialogDescription>{REMOVE_RECORDING_STILL_NAMED}</AlertDialogDescription>
          </AlertDialogHeader>
          <ul aria-label="Notes still naming this recording" className="list-disc pl-5 text-sm">
            {stillNamed.map((note) => (
              <li key={`${note.vaultId}/${note.path}`} title={note.path}>
                {note.title} <span className="text-muted-foreground">— {note.error}</span>
              </li>
            ))}
          </ul>
          <AlertDialogFooter>
            <AlertDialogCancel>{REMOVE_RECORDING_CLOSE}</AlertDialogCancel>
          </AlertDialogFooter>
        </AlertDialogContent>
      </AlertDialog>
    );
  }

  return (
    <AlertDialog
      open
      onOpenChange={(open) => {
        if (!open) onClose();
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>
            {plan === null && failure === null ? REMOVE_RECORDING_READING : REMOVE_RECORDING_TITLE}
          </AlertDialogTitle>
          <AlertDialogDescription>
            {plan === null
              ? ""
              : `${plan.folder} — ${formatSize(plan.bytes)}, ${countLabel(plan.files, FILES)}. ${reachSentence(plan.drive, plan.durability)}`}
          </AlertDialogDescription>
        </AlertDialogHeader>
        {plan !== null && plan.notes.length > 0 && (
          <div className="flex flex-col gap-1 text-sm">
            <p>Its widget goes from:</p>
            <ul aria-label="Notes naming this recording" className="list-disc pl-5">
              {plan.notes.map((note) => (
                <li key={`${note.vaultId}/${note.path}`} className="truncate" title={note.path}>
                  {note.title}
                </li>
              ))}
            </ul>
          </div>
        )}
        {failure !== null && (
          <p role="alert" className="text-destructive text-sm">
            {failure}
          </p>
        )}
        <AlertDialogFooter>
          <AlertDialogCancel>{REMOVE_RECORDING_CANCEL}</AlertDialogCancel>
          {plan !== null && (
            <AlertDialogAction
              variant="destructive"
              disabled={removing}
              // Kept open through the command: a refusal must be said here.
              onClick={(event) => {
                event.preventDefault();
                confirm();
              }}
            >
              {REMOVE_RECORDING_CONFIRM}
            </AlertDialogAction>
          )}
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
