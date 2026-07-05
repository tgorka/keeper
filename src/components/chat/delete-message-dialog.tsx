/**
 * The delete-for-everyone confirmation dialog (Story 3.8, FR-15, UX-DR17).
 *
 * A controlled AlertDialog (the sign-out pattern) that confirms deleting an own
 * message for everyone. On open it resolves the Chat's bridged Network label on
 * demand: a native Matrix Room (`label: null`) states removal happens for everyone
 * in this Chat (redaction is honored by all Matrix clients); a bridged Chat names
 * the Network and states remote removal is best-effort — never a guaranteed remote
 * delete, and never a fabricated Network name. The destructive confirm dispatches
 * the redaction through the single Rust gate; a dispatch failure keeps the dialog
 * open, surfaces an honest error, and re-enables the action for retry. Copy follows
 * UX-DR10: sentence case, no exclamation, Glossary nouns (Network, Chat, Mac)
 * capitalized.
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
import { deleteMessage, type IpcError, roomNetworkLabel } from "@/lib/ipc/client";

const FOCUS_RING = "focus-visible:ring-2 focus-visible:ring-ring focus-visible:outline-none";

interface DeleteMessageDialogProps {
  /** The account whose message is being deleted. */
  accountId: string;
  /** The Room the message lives in. */
  roomId: string;
  /**
   * The opaque render `key` of the message to delete, or `null` when the dialog is
   * closed. A non-null key opens the dialog and triggers the Network-label probe.
   */
  itemKey: string | null;
  /** Close the dialog (clears the delete target in the parent). */
  onClose: () => void;
}

export function DeleteMessageDialog({
  accountId,
  roomId,
  itemKey,
  onClose,
}: DeleteMessageDialogProps) {
  const open = itemKey !== null;
  // The resolved bridged Network name, or `null` for a native Matrix Room. Starts
  // `null` and is filled by the on-open probe; a probe failure leaves it `null`
  // (honest generic bridged fallback is folded into the native framing per the
  // matrix — see the copy below).
  const [networkLabel, setNetworkLabel] = useState<string | null>(null);
  const [deleting, setDeleting] = useState(false);
  const [error, setError] = useState<string | null>(null);
  // Whether the surfaced error can be retried. A dispatch failure is retriable, so
  // the destructive action stays live for another attempt; a vanished target
  // (`TargetNotFound` → `retriable: false`) is terminal — the message is already
  // gone, so we surface an honest "no longer available" and withdraw the action
  // rather than loop a retry that can never succeed (Story 3.8 I/O matrix:
  // "Target vanished → honest 'message no longer available'").
  const [errorRetriable, setErrorRetriable] = useState(true);

  // On open, probe the Network label on demand. A failed probe falls back to the
  // honest native framing rather than blocking the delete. Cancelled if the dialog
  // closes (or the target changes) before it resolves.
  useEffect(() => {
    if (itemKey === null) {
      return;
    }
    let cancelled = false;
    setNetworkLabel(null);
    setError(null);
    setErrorRetriable(true);
    setDeleting(false);
    roomNetworkLabel(accountId, roomId)
      .then((label) => {
        if (!cancelled) {
          setNetworkLabel(label);
        }
      })
      .catch(() => {
        // Probe failure → native framing (no fabricated Network name).
        if (!cancelled) {
          setNetworkLabel(null);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [itemKey, accountId, roomId]);

  async function handleConfirm() {
    if (itemKey === null) {
      return;
    }
    setDeleting(true);
    setError(null);
    setErrorRetriable(true);
    try {
      await deleteMessage(accountId, roomId, itemKey);
      // The redaction Set diff arrives over the timeline subscription; close.
      onClose();
    } catch (raw) {
      // Branch on retriability (Story 3.8 I/O matrix). A dispatch failure keeps the
      // dialog open with an honest error and a live retry; a vanished target is
      // terminal — surface an honest "no longer available" and withdraw the action
      // so the user isn't offered a retry that can never succeed.
      const err = raw as IpcError | null | undefined;
      const retriable = err?.retriable !== false;
      setError(
        retriable
          ? (err?.message ?? "Couldn't delete the message. Try again.")
          : "This message is no longer available.",
      );
      setErrorRetriable(retriable);
      setDeleting(false);
    }
  }

  return (
    <AlertDialog
      open={open}
      onOpenChange={(next) => {
        // Ignore dismiss (Esc / backdrop) while a redaction is in flight, mirroring
        // the disabled Cancel button, so the dialog cannot close out from under a
        // pending dispatch and drop its outcome.
        if (!next && !deleting) {
          onClose();
        }
      }}
    >
      <AlertDialogContent>
        <AlertDialogHeader>
          <AlertDialogTitle>Delete this message for everyone</AlertDialogTitle>
          <AlertDialogDescription>
            {networkLabel === null ? (
              // No bridged Network was identified — this is a native Matrix Room, or a
              // bridge keeper couldn't detect. Stay honest either way: never promise a
              // guaranteed remote delete on a Chat that might be bridged (UX-DR17).
              <>
                Deletes your copy on this Mac and removes it for everyone in this Chat. If this Chat
                is bridged to another network, removal there is best-effort.
              </>
            ) : (
              <>
                Deletes your copy on this Mac and removes it for everyone in this Chat. Removal on{" "}
                {networkLabel} is best-effort.
              </>
            )}
          </AlertDialogDescription>
        </AlertDialogHeader>
        {error !== null && (
          <p role="alert" className="text-destructive text-sm">
            {error}
          </p>
        )}
        <AlertDialogFooter>
          <AlertDialogCancel className={FOCUS_RING} disabled={deleting}>
            Cancel
          </AlertDialogCancel>
          <AlertDialogAction
            variant="destructive"
            className={FOCUS_RING}
            // Disabled while a dispatch is in flight, and withdrawn entirely after a
            // terminal (non-retriable) failure so a vanished target offers no futile
            // retry — only Cancel remains to dismiss.
            disabled={deleting || (error !== null && !errorRetriable)}
            onClick={(event) => {
              // Keep the dialog mounted while the async delete runs.
              event.preventDefault();
              void handleConfirm();
            }}
          >
            {deleting ? "Deleting…" : "Delete for everyone"}
          </AlertDialogAction>
        </AlertDialogFooter>
      </AlertDialogContent>
    </AlertDialog>
  );
}
