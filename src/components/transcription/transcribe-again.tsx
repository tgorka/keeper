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

export const TRANSCRIBE_AGAIN_LABEL = "Transcribe again";
export const TRANSCRIBE_AGAIN_TITLE = "Replace the transcript?";
export const TRANSCRIBE_AGAIN_BODY =
  "Your corrections in it will be lost. The voices bank keeps everything you confirmed.";

/**
 * The one question "Transcribe again" asks, wherever it is offered. Only the
 * confirming button calls `onConfirm`, which starts the job with `replace`;
 * the old transcript stays on disk until the new one is written, so a failed
 * or cancelled job loses nothing.
 */
export function TranscribeAgainDialog({
  open,
  onClose,
  onConfirm,
}: {
  open: boolean;
  onClose: () => void;
  onConfirm: () => void;
}) {
  return (
    <AlertDialog
      open={open}
      onOpenChange={(next) => {
        if (!next) onClose();
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{TRANSCRIBE_AGAIN_TITLE}</AlertDialogTitle>
          <AlertDialogDescription>{TRANSCRIBE_AGAIN_BODY}</AlertDialogDescription>
        </AlertDialogHeader>
        <AlertDialogFooter>
          <AlertDialogCancel>Cancel</AlertDialogCancel>
          <AlertDialogAction
            onClick={() => {
              onConfirm();
              onClose();
            }}
          >
            {TRANSCRIBE_AGAIN_LABEL}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
