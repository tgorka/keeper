/**
 * Key-backup modal — enable + restore (Story 3.3, FR-14, NFR-14, AD-1).
 *
 * An Element-X-style two-mode modal driven by the {@link keyBackupStore}. It never
 * touches key material beyond the deliberate boundary exception — the base58
 * recovery key that must reach the human to be saved (shown once in `mono`, held
 * only while the modal is open, cleared on close). All crypto lives in Rust.
 *
 * **Enable** mode: on open it calls `backupEnable`, shows the returned recovery
 * key once with Copy + "Save to Keychain" + an explicit "I've saved it"
 * acknowledgment that gates Done, and warns it won't be shown again. A
 * `backupExists` race switches the modal to restore.
 *
 * **Restore** mode: a `font-mono` textarea prefilled from `backupSavedRecoveryKey`,
 * a Restore action, and a *named* inline error per {@link IpcErrorCode}
 * (`backupMalformedKey` vs `backupIncorrectKey` distinct from a generic
 * `backupFailed`), with distinct working / restored / failed states.
 *
 * Fully keyboard-operable (NFR-14): all controls are focusable and the shadcn
 * `Dialog` (Radix) handles focus trapping and `Esc` (which closes and clears the
 * recovery key via the store's `close()`).
 */
import { useEffect, useRef, useState } from "react";
import { useStore } from "zustand";
import { Button } from "@/components/ui/button";
import { Checkbox } from "@/components/ui/checkbox";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { InputGroup, InputGroupTextarea } from "@/components/ui/input-group";
import { Label } from "@/components/ui/label";
import type { IpcError, IpcErrorCode } from "@/lib/ipc/client";
import {
  backupEnable,
  backupRestore,
  backupSavedRecoveryKey,
  backupSaveRecoveryKey,
} from "@/lib/ipc/client";
import { keyBackupStore, useKeyBackupModalOpen } from "@/lib/stores/key-backup";

/** Narrow an unknown rejection to its named {@link IpcErrorCode}, defaulting to
 * the generic backup failure code. */
function errorCodeOf(raw: unknown): IpcErrorCode {
  if (typeof raw === "object" && raw !== null && "code" in raw) {
    return (raw as IpcError).code;
  }
  return "backupFailed";
}

export function KeyBackupDialog() {
  const open = useKeyBackupModalOpen();
  const mode = useStore(keyBackupStore, (s) => s.mode);

  return (
    <Dialog
      open={open}
      onOpenChange={(next) => {
        if (!next) {
          keyBackupStore.getState().close();
        }
      }}
    >
      <DialogContent>
        <DialogHeader>
          <DialogTitle>
            {mode === "restore" ? "Restore key backup" : "Set up key backup"}
          </DialogTitle>
          <DialogDescription>
            {mode === "restore"
              ? "Enter your recovery key to unlock encrypted history on this device."
              : "Back up your encryption keys so you can restore encrypted history on new devices."}
          </DialogDescription>
        </DialogHeader>
        {mode === "restore" ? <RestoreBody /> : <EnableBody />}
      </DialogContent>
    </Dialog>
  );
}

/** The enable path: request the recovery key once, show it, offer Copy + Keychain
 * save, and gate Done behind an explicit "I've saved it" acknowledgment. */
function EnableBody() {
  const accountId = useStore(keyBackupStore, (s) => s.accountId);
  const recoveryKey = useStore(keyBackupStore, (s) => s.recoveryKey);
  const phase = useStore(keyBackupStore, (s) => s.phase);
  const error = useStore(keyBackupStore, (s) => s.error);

  const [acknowledged, setAcknowledged] = useState(false);
  const [copied, setCopied] = useState(false);
  const [saved, setSaved] = useState(false);
  const [saveError, setSaveError] = useState(false);
  // Guard so enable is requested at most once per opened account, even across
  // StrictMode double-invokes (keying on `phase` would re-fire the request when
  // this effect re-runs on the phase transition to `working`).
  const requestedRef = useRef<string | null>(null);

  // Kick off enable once when the modal opens for an account. A `backupExists`
  // race switches the modal to restore (offer restore instead of a generic
  // error); any other failure surfaces the honest failed state.
  useEffect(() => {
    if (accountId === null) {
      requestedRef.current = null;
      return;
    }
    if (requestedRef.current === accountId) {
      return;
    }
    requestedRef.current = accountId;
    const targetAccount = accountId;
    keyBackupStore.getState().setPhase("working");
    void backupEnable(targetAccount)
      .then((key) => {
        if (keyBackupStore.getState().accountId !== targetAccount) {
          return;
        }
        keyBackupStore.getState().setRecoveryKey(key);
      })
      .catch((raw) => {
        if (keyBackupStore.getState().accountId !== targetAccount) {
          return;
        }
        const code = errorCodeOf(raw);
        if (code === "backupExists") {
          keyBackupStore.getState().switchToRestore();
        } else {
          keyBackupStore.getState().setError(code);
        }
      });
  }, [accountId]);

  if (phase === "working") {
    return <p className="text-sm text-muted-foreground">Creating your key backup…</p>;
  }

  if (phase === "failed") {
    return (
      <p className="text-sm text-held">
        {error === "backupFailed"
          ? "Couldn't set up key backup. Please try again."
          : "Couldn't set up key backup."}
      </p>
    );
  }

  if (phase === "shown" && recoveryKey) {
    return (
      <div className="flex flex-col gap-4 text-sm">
        <p className="text-held">
          Save this recovery key now. It is shown only once and cannot be recovered.
        </p>
        <output
          className="block rounded-md border border-border bg-muted/40 p-3 font-mono text-xs break-all"
          aria-label="Recovery key"
        >
          {recoveryKey}
        </output>
        <div className="flex flex-wrap gap-2">
          <Button
            type="button"
            variant="outline"
            size="sm"
            onClick={() => {
              void navigator.clipboard
                ?.writeText(recoveryKey)
                .then(() => setCopied(true))
                .catch(() => {});
            }}
          >
            {copied ? "Copied" : "Copy"}
          </Button>
          <Button
            type="button"
            variant="outline"
            size="sm"
            disabled={saved || accountId === null}
            onClick={() => {
              if (accountId === null) {
                return;
              }
              setSaveError(false);
              void backupSaveRecoveryKey(accountId, recoveryKey)
                .then(() => setSaved(true))
                .catch(() => setSaveError(true));
            }}
          >
            {saved ? "Saved to Keychain" : "Save to Keychain"}
          </Button>
        </div>
        {saveError ? (
          <p className="text-xs text-held">
            Couldn't save to Keychain. Copy the key and store it somewhere safe.
          </p>
        ) : null}
        <Label className="flex items-center gap-2 text-xs">
          <Checkbox
            checked={acknowledged}
            onCheckedChange={(next) => setAcknowledged(next === true)}
          />
          I've saved my recovery key
        </Label>
        <DialogFooter>
          <Button
            type="button"
            size="sm"
            disabled={!acknowledged}
            onClick={() => keyBackupStore.getState().close()}
          >
            Done
          </Button>
        </DialogFooter>
      </div>
    );
  }

  return <p className="text-sm text-muted-foreground">Creating your key backup…</p>;
}

/** The restore path: a mono textarea prefilled from the Keychain, a Restore
 * action, and a named inline error per {@link IpcErrorCode}. */
function RestoreBody() {
  const accountId = useStore(keyBackupStore, (s) => s.accountId);
  const phase = useStore(keyBackupStore, (s) => s.phase);
  const error = useStore(keyBackupStore, (s) => s.error);

  const [value, setValue] = useState("");

  // Prefill from a saved key in the Keychain, once per opened account.
  useEffect(() => {
    if (accountId === null) {
      return;
    }
    const targetAccount = accountId;
    let cancelled = false;
    void backupSavedRecoveryKey(targetAccount)
      .then((saved) => {
        // Only prefill if the field is still empty: a slow Keychain read must
        // never clobber a key the user already pasted/typed in the meantime.
        if (!cancelled && saved) {
          setValue((current) => (current.length === 0 ? saved : current));
        }
      })
      .catch(() => {
        // No prefill on failure — the user can paste manually.
      });
    return () => {
      cancelled = true;
    };
  }, [accountId]);

  if (phase === "restored") {
    return (
      <p className="text-sm text-emerald-600 dark:text-emerald-400">
        Key backup restored. Encrypted history is unlocking now.
      </p>
    );
  }

  const restore = () => {
    if (accountId === null || value.trim().length === 0) {
      return;
    }
    const targetAccount = accountId;
    keyBackupStore.getState().setPhase("working");
    void backupRestore(targetAccount, value.trim())
      .then(() => {
        if (keyBackupStore.getState().accountId === targetAccount) {
          keyBackupStore.getState().setPhase("restored");
        }
      })
      .catch((raw) => {
        if (keyBackupStore.getState().accountId === targetAccount) {
          keyBackupStore.getState().setError(errorCodeOf(raw));
        }
      });
  };

  return (
    <div className="flex flex-col gap-4 text-sm">
      <Label htmlFor="recovery-key-input" className="text-xs">
        Recovery key
      </Label>
      <InputGroup>
        <InputGroupTextarea
          id="recovery-key-input"
          className="font-mono text-xs"
          rows={3}
          placeholder="Paste your recovery key"
          value={value}
          disabled={phase === "working"}
          aria-invalid={phase === "failed"}
          onChange={(e) => {
            setValue(e.target.value);
            // Clear a stale named error (and its `aria-invalid`) as the user
            // edits toward a corrected key — it should not linger while typing.
            if (keyBackupStore.getState().phase === "failed") {
              keyBackupStore.getState().setPhase("idle");
            }
          }}
        />
      </InputGroup>
      {phase === "failed" ? <RestoreError code={error} /> : null}
      <DialogFooter>
        <Button
          type="button"
          size="sm"
          disabled={phase === "working" || value.trim().length === 0}
          onClick={restore}
        >
          {phase === "working" ? "Restoring…" : "Restore"}
        </Button>
      </DialogFooter>
    </div>
  );
}

/** The named inline error copy for a failed restore, distinct per code (FR-14):
 * a malformed key, a well-formed-but-wrong key, and a generic failure each read
 * differently — never a single generic message. */
function RestoreError({ code }: { code: IpcErrorCode | null }) {
  const message =
    code === "backupMalformedKey"
      ? "That doesn't look like a recovery key."
      : code === "backupIncorrectKey"
        ? "Recovery key didn't match this account."
        : "Couldn't restore from key backup. Please try again.";
  return (
    <p className="text-xs text-held" role="alert">
      {message}
    </p>
  );
}
