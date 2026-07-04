import type { ReactNode } from "react";
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
import { useAccountsStore } from "@/lib/stores/accounts";
import { useEncryptionStatus } from "@/lib/stores/encryption-status";

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
        <EncryptionSection />
      </DialogContent>
    </Dialog>
  );
}

/** The honest copy explaining what verifying a device unlocks (Story 3.1). */
const ENCRYPTION_HONESTY_SENTENCE =
  "Verifying this device unlocks encrypted history and lets other people trust your messages.";

/**
 * Read-only Encryption section (Story 3.1): lists each signed-in account's device
 * state (Verified / Not verified) from the encryption-status store, plus the
 * honest sentence on what verifying unlocks. There is intentionally NO interactive
 * Verify button here — the interactive verify flow lands in Story 3.2; 3.1 only
 * makes the honest state visible.
 */
function EncryptionSection() {
  const accounts = useAccountsStore((s) => s.accounts);

  return (
    <div className="mt-2 flex flex-col gap-2 border-border border-t pt-3 text-sm">
      <p className="font-medium">Encryption</p>
      {accounts.length === 0 ? (
        <p className="text-muted-foreground">No accounts signed in.</p>
      ) : (
        <ul className="flex flex-col gap-1.5">
          {accounts.map((account) => (
            <EncryptionAccountRow key={account.accountId} accountId={account.accountId}>
              {account.userId}
            </EncryptionAccountRow>
          ))}
        </ul>
      )}
      <p className="text-muted-foreground">{ENCRYPTION_HONESTY_SENTENCE}</p>
    </div>
  );
}

/** One account's device-verification state row. Three honest states, never
 * over-claiming: `verified` reads "Verified"; an explicit `unverified` reads
 * "Not verified"; and `unknown`/pending (crypto not yet reported) reads a neutral
 * "Checking…" — the same "no false nag before crypto syncs" rule the banner
 * honors, so a device mid-sync is never labelled a problem. */
function EncryptionAccountRow({ accountId, children }: { accountId: string; children: ReactNode }) {
  const status = useEncryptionStatus(accountId);
  const label =
    status === "verified" ? "Verified" : status === "unverified" ? "Not verified" : "Checking…";
  // Only an explicit `unverified` gets the attention tone; verified and the
  // transient checking state stay muted.
  const tone = status === "unverified" ? "text-held text-xs" : "text-muted-foreground text-xs";
  return (
    <li className="flex items-center justify-between gap-2">
      <span
        className="truncate font-mono text-xs"
        title={typeof children === "string" ? children : undefined}
      >
        {children}
      </span>
      <span className={tone}>{label}</span>
    </li>
  );
}
