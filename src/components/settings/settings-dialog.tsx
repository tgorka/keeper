import type { ReactNode } from "react";
import { useEffect, useRef, useState } from "react";
import {
  SDK_STORE_ENCRYPTED_STATUS,
  SDK_STORE_STATUS_LOADING,
  SDK_STORE_UNENCRYPTED_STATUS,
  STORAGE_HONESTY_SENTENCE,
} from "@/components/settings/at-rest-encryption-choice";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import {
  encryptionPosture,
  honorRemoteDeletions,
  incognitoGetGlobal,
  incognitoSetGlobal,
  setHonorRemoteDeletions,
  setUndoSendWindow,
  undoSendWindow,
} from "@/lib/ipc/client";
import { useAccountsStore } from "@/lib/stores/accounts";
import { useEncryptionStatus } from "@/lib/stores/encryption-status";
import { incognitoStore } from "@/lib/stores/incognito";
import { keyBackupStore, useKeyBackupStatus } from "@/lib/stores/key-backup";
import { verificationStore } from "@/lib/stores/verification";
import { wizardStore } from "@/lib/stores/wizard";

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
          <HonorRemoteDeletionsRow />
        </div>
        <PrivacySection open={open} />
        <EncryptionSection />
        <SetupSection onOpenChange={onOpenChange} />
      </DialogContent>
    </Dialog>
  );
}

/**
 * The plain disclosure for the honor-remote-deletions toggle (Story 5.2, FR-36,
 * UX-DR17). Sentence case, no exclamation marks (project voice).
 */
const HONOR_REMOTE_DELETIONS_SENTENCE =
  "keeper keeps local copies of remotely edited and deleted messages by default. Turning this on hides remotely deleted messages from history retrieval on this Mac; turning it off makes them retrievable again. The local copies are never erased.";

/**
 * The "Honor remote deletions locally" toggle in the Archive & Storage section
 * (Story 5.2, FR-36). Reads its initial state via `honorRemoteDeletions()` and
 * persists changes via `setHonorRemoteDeletions`. On a persist failure the toggle
 * reverts to its prior value (honest — never claims a state that was not saved).
 */
function HonorRemoteDeletionsRow() {
  // `undefined` = still loading; otherwise the resolved boolean.
  const [enabled, setEnabled] = useState<boolean | undefined>(undefined);

  useEffect(() => {
    let cancelled = false;
    void honorRemoteDeletions()
      .then((value) => {
        if (!cancelled) {
          setEnabled(value);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setEnabled(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, []);

  // Monotonic token so a failed persist only reverts when no newer toggle has
  // superseded it — prevents a stale-closure revert clobbering a rapid re-toggle.
  const writeId = useRef(0);

  const onCheckedChange = (next: boolean) => {
    writeId.current += 1;
    const id = writeId.current;
    const prev = enabled ?? false;
    setEnabled(next);
    void setHonorRemoteDeletions(next).catch(() => {
      // Persist failed — revert, but only if this is still the latest toggle.
      if (id === writeId.current) {
        setEnabled(prev);
      }
    });
  };

  return (
    <div className="mt-1 flex flex-col gap-1.5 border-border border-t pt-3">
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor="honor-remote-deletions">Honor remote deletions locally</Label>
        <Switch
          id="honor-remote-deletions"
          checked={enabled ?? false}
          disabled={enabled === undefined}
          onCheckedChange={onCheckedChange}
        />
      </div>
      <p className="text-muted-foreground">{HONOR_REMOTE_DELETIONS_SENTENCE}</p>
    </div>
  );
}

/**
 * The plain disclosure for the global Incognito default (Story 8.1). Sentence case,
 * no exclamation marks, honest consequence-naming (project voice).
 */
const INCOGNITO_GLOBAL_SENTENCE =
  "Reading a chat sends a private read receipt: your read position still syncs across your own devices, but the other person keeps seeing the message as unread. This is the default for every chat; you can override it per account or per chat.";

/** The default Undo-Send window in seconds (mirrors the Rust registry default). */
const UNDO_SEND_WINDOW_DEFAULT = 10;
/** The maximum Undo-Send window in seconds (values clamp to 0..=60). */
const UNDO_SEND_WINDOW_MAX = 60;
/** The honest copy explaining the Undo-Send window (Story 8.3). */
const UNDO_SEND_SENTENCE =
  "Each message you send waits this many seconds before it leaves, so you can undo it. Set to 0 to send immediately.";

/**
 * Privacy section (Story 8.1): the global Incognito default `Switch`, bound to
 * `incognitoSetGlobal`. Reads its initial state via `incognitoGetGlobal()` on open and
 * mirrors the new value into the incognito store so open chats reflect it. On a persist
 * failure the toggle reverts (honest — never claims a state that was not saved).
 */
function PrivacySection({ open }: { open: boolean }) {
  // `undefined` = still loading; otherwise the resolved global default.
  const [enabled, setEnabled] = useState<boolean | undefined>(undefined);

  useEffect(() => {
    if (!open) {
      return;
    }
    setEnabled(undefined);
    let cancelled = false;
    void incognitoGetGlobal()
      .then((value) => {
        if (!cancelled) {
          setEnabled(value);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setEnabled(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  // Monotonic token so a failed persist only reverts when no newer toggle superseded it.
  const writeId = useRef(0);

  const onCheckedChange = (next: boolean) => {
    writeId.current += 1;
    const id = writeId.current;
    const prev = enabled ?? false;
    setEnabled(next);
    // Mirror the new global into the store immediately for the global selector, then
    // bump the policy version once the write lands so any open chat re-reads its fully
    // resolved effective state (the chip/ring reconcile without a room reopen).
    incognitoStore.getState().applyGlobal(next);
    void incognitoSetGlobal(next)
      .then(() => {
        incognitoStore.getState().bumpPolicyVersion();
      })
      .catch(() => {
        if (id === writeId.current) {
          setEnabled(prev);
          incognitoStore.getState().applyGlobal(prev);
          incognitoStore.getState().bumpPolicyVersion();
        }
      });
  };

  // Undo-Send window in seconds (Story 8.3): `undefined` = still loading; otherwise the
  // resolved 0..=60 value (0 disables holding). Load-on-open + optimistic write with
  // revert, mirroring the Incognito toggle above.
  // Named `undoWindow` (not `window`) so it does not shadow the browser global in this
  // component's scope.
  const [undoWindow, setUndoWindow] = useState<number | undefined>(undefined);
  const windowWriteId = useRef(0);

  useEffect(() => {
    if (!open) {
      return;
    }
    setUndoWindow(undefined);
    let cancelled = false;
    void undoSendWindow()
      .then((value) => {
        if (!cancelled) {
          setUndoWindow(value);
        }
      })
      .catch(() => {
        if (!cancelled) {
          setUndoWindow(UNDO_SEND_WINDOW_DEFAULT);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [open]);

  const onWindowChange = (raw: string) => {
    // Parse + clamp to 0..=60 locally so the field never shows an out-of-range value;
    // Rust clamps again defensively. A non-numeric entry is ignored (keeps the prior).
    const parsed = Number.parseInt(raw, 10);
    if (Number.isNaN(parsed)) {
      return;
    }
    const clamped = Math.min(UNDO_SEND_WINDOW_MAX, Math.max(0, parsed));
    windowWriteId.current += 1;
    const id = windowWriteId.current;
    const prev = undoWindow ?? UNDO_SEND_WINDOW_DEFAULT;
    setUndoWindow(clamped);
    void setUndoSendWindow(clamped).catch(() => {
      if (id === windowWriteId.current) {
        setUndoWindow(prev);
      }
    });
  };

  return (
    <div className="mt-2 flex flex-col gap-2 border-border border-t pt-3 text-sm">
      <p className="font-medium">Privacy</p>
      <div className="flex items-center justify-between gap-2">
        <Label htmlFor="incognito-global">Incognito by default</Label>
        <Switch
          id="incognito-global"
          checked={enabled ?? false}
          disabled={enabled === undefined}
          onCheckedChange={onCheckedChange}
        />
      </div>
      <p className="text-muted-foreground">{INCOGNITO_GLOBAL_SENTENCE}</p>
      <div className="mt-1 flex items-center justify-between gap-2">
        <Label htmlFor="undo-send-window">Undo-Send window (seconds)</Label>
        <Input
          id="undo-send-window"
          type="number"
          min={0}
          max={UNDO_SEND_WINDOW_MAX}
          className="w-20"
          value={undoWindow ?? ""}
          disabled={undoWindow === undefined}
          onChange={(e) => onWindowChange(e.target.value)}
        />
      </div>
      <p className="text-muted-foreground">{UNDO_SEND_SENTENCE}</p>
    </div>
  );
}

/** The honest copy explaining what verifying a device unlocks (Story 3.1). */
const ENCRYPTION_HONESTY_SENTENCE =
  "Verifying this device unlocks encrypted history and lets other people trust your messages.";

/**
 * Encryption section (Story 3.1 + 3.2): lists each signed-in account's device
 * state (Verified / Not verified) from the encryption-status store, plus the
 * honest sentence on what verifying unlocks. An account whose device is
 * `unverified` gets an interactive "Verify" button (Story 3.2) that opens the
 * device-verification modal for that account.
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

/**
 * Setup section (Story 6.8): a "Run setup again" entry that re-opens the
 * session-scoped first-run wizard over the shell and closes Settings. The wizard
 * is fully re-runnable; `accountId` defaults to the first account on re-entry.
 */
function SetupSection({ onOpenChange }: { onOpenChange: (open: boolean) => void }) {
  return (
    <div className="mt-2 flex flex-col gap-2 border-border border-t pt-3 text-sm">
      <p className="font-medium">Setup</p>
      <div className="flex items-center justify-between gap-2">
        <p className="text-muted-foreground">Walk through the first-run setup again.</p>
        <Button
          type="button"
          variant="outline"
          size="sm"
          onClick={() => {
            wizardStore.getState().start();
            onOpenChange(false);
          }}
        >
          Run setup again
        </Button>
      </div>
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
    <li className="flex flex-col gap-1">
      <div className="flex items-center justify-between gap-2">
        <span
          className="truncate font-mono text-xs"
          title={typeof children === "string" ? children : undefined}
        >
          {children}
        </span>
        <span className="flex items-center gap-2">
          <span className={tone}>{label}</span>
          {status === "unverified" ? (
            <Button
              type="button"
              variant="outline"
              size="xs"
              onClick={() => verificationStore.getState().openFor(accountId)}
            >
              Verify
            </Button>
          ) : null}
        </span>
      </div>
      <BackupAccountRow accountId={accountId} />
    </li>
  );
}

/** One account's key-backup state line (Story 3.3, FR-14, AC3): four honest
 * states sourced from the Rust core. `disabled` → a "Set up backup" button
 * (enable); `incomplete` → a "Restore" button (the fresh-login "Needs your
 * recovery key" case); `enabled` → "Backup on"; `unknown`/pending → "Checking…"
 * (no false claim before crypto syncs). */
function BackupAccountRow({ accountId }: { accountId: string }) {
  const status = useKeyBackupStatus(accountId);
  const label =
    status === "enabled"
      ? "Backup on"
      : status === "disabled"
        ? "Not set up"
        : status === "incomplete"
          ? "Needs your recovery key"
          : "Checking…";
  // Only `incomplete` (locked history awaiting restore) gets the attention tone.
  const tone = status === "incomplete" ? "text-held text-xs" : "text-muted-foreground text-xs";
  return (
    <div className="flex items-center justify-between gap-2 pl-1">
      <span className="text-muted-foreground text-xs">Key backup</span>
      <span className="flex items-center gap-2">
        <span className={tone}>{label}</span>
        {status === "disabled" ? (
          <Button
            type="button"
            variant="outline"
            size="xs"
            onClick={() => keyBackupStore.getState().openEnable(accountId)}
          >
            Set up backup
          </Button>
        ) : null}
        {status === "incomplete" ? (
          <Button
            type="button"
            variant="outline"
            size="xs"
            onClick={() => keyBackupStore.getState().openRestore(accountId)}
          >
            Restore
          </Button>
        ) : null}
      </span>
    </div>
  );
}
