import { useEffect, useState } from "react";
import {
  SDK_STORE_ENCRYPTED_STATUS,
  SDK_STORE_STATUS_LOADING,
  SDK_STORE_UNENCRYPTED_STATUS,
  STORAGE_HONESTY_SENTENCE,
} from "@/components/settings/at-rest-encryption-choice";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { encryptionPosture } from "@/lib/ipc/client";

interface SettingsDialogProps {
  /** Whether the dialog is open (controlled by the caller). */
  open: boolean;
  /** Called to open/close the dialog. */
  onOpenChange: (open: boolean) => void;
}

/**
 * Settings dialog with a read-only Archive & Storage section (Story 2.6, AD-22,
 * UX-DR17). States plainly that `keeper.db`/`archive.db` are not
 * passphrase-encrypted in this version and rely on FileVault, and reflects
 * whether the per-account Matrix SDK stores are passphrase-encrypted (loaded from
 * `encryptionPosture()` on open). No toggle — the posture is a first-run choice
 * only and is never re-prompted here.
 */
export function SettingsDialog({ open, onOpenChange }: SettingsDialogProps) {
  // `undefined` = still loading; otherwise the resolved posture (`null` unchosen,
  // or `true`/`false`). Keeping "loading" distinct from a resolved value stops the
  // status line from momentarily claiming "not encrypted" before the real posture
  // arrives, on both first open and reopen.
  const [posture, setPosture] = useState<boolean | null | undefined>(undefined);

  useEffect(() => {
    if (!open) {
      return;
    }
    // Reset to the loading state on every (re)open so a stale prior value never
    // flashes while the fresh read is in flight.
    setPosture(undefined);
    let cancelled = false;
    void encryptionPosture()
      .then((value) => {
        if (!cancelled) {
          setPosture(value);
        }
      })
      .catch(() => {
        // On a read failure, fall back to the honest FileVault-only status.
        if (!cancelled) {
          setPosture(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  // While loading, show a neutral checking line — never a definitive (possibly
  // wrong) "not encrypted" claim. `true` ⇒ encrypted; `false`/`null` ⇒ FileVault.
  const sdkStatus =
    posture === undefined
      ? SDK_STORE_STATUS_LOADING
      : posture === true
        ? SDK_STORE_ENCRYPTED_STATUS
        : SDK_STORE_UNENCRYPTED_STATUS;

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent>
        <DialogHeader>
          <DialogTitle>Settings</DialogTitle>
          <DialogDescription>Archive &amp; Storage</DialogDescription>
        </DialogHeader>
        <div className="flex flex-col gap-3 text-sm">
          <p>{sdkStatus}</p>
          <p className="text-muted-foreground">{STORAGE_HONESTY_SENTENCE}</p>
        </div>
      </DialogContent>
    </Dialog>
  );
}
