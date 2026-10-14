/**
 * `/leave`'s confirmation (Story 100.5): leaving is not undone by a keystroke,
 * so the command only opens this, and only Leave here leaves. A refusal keeps
 * the dialog open with its sentence and writes `keeper.log` (R381); the room
 * leaves the list through the room-list stream like a leave from any client.
 */
import { useState } from "react";
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
import { reportFrontendError } from "@/lib/crash-report";
import { roomLeave } from "@/lib/ipc/client";

export const LEAVE_ROOM_TITLE = "Leave this room";
export const LEAVE_ROOM_BODY =
  "You stop receiving its messages here and on your other devices. To come back, someone in it has to invite you again, unless it is public.";
export const LEAVE_ROOM_CONFIRM = "Leave";
export const LEAVE_ROOM_FAILED =
  "keeper couldn't leave the room: the server refused it or could not be reached. You are still in it.";

export function LeaveRoomDialog({
  accountId,
  roomId,
  open,
  onClose,
  leave,
}: {
  accountId: string;
  roomId: string;
  open: boolean;
  onClose: () => void;
  /** The leave itself; a test passes its own. */
  leave?: (accountId: string, roomId: string) => Promise<void>;
}) {
  const [leaving, setLeaving] = useState(false);
  const [error, setError] = useState<string | null>(null);

  async function confirm() {
    setLeaving(true);
    setError(null);
    try {
      // Looked up at the click, not at render: a room that is never left
      // never reaches for the IPC.
      await (leave ?? roomLeave)(accountId, roomId);
      onClose();
    } catch (raw) {
      reportFrontendError("chat-command:leave", raw, null);
      setError(LEAVE_ROOM_FAILED);
    } finally {
      setLeaving(false);
    }
  }

  return (
    <AlertDialog
      open={open}
      onOpenChange={(next) => {
        if (!next && !leaving) {
          setError(null);
          onClose();
        }
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>{LEAVE_ROOM_TITLE}</AlertDialogTitle>
          <AlertDialogDescription>{LEAVE_ROOM_BODY}</AlertDialogDescription>
        </AlertDialogHeader>
        {error !== null && (
          <p role="alert" className="text-destructive text-sm">
            {error}
          </p>
        )}
        <AlertDialogFooter>
          <AlertDialogCancel disabled={leaving}>Cancel</AlertDialogCancel>
          <AlertDialogAction
            variant="destructive"
            disabled={leaving}
            onClick={(event) => {
              event.preventDefault();
              void confirm();
            }}
          >
            {leaving ? "Leaving…" : LEAVE_ROOM_CONFIRM}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
