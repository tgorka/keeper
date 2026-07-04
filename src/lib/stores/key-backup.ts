/**
 * Key-backup store (Story 3.3, FR-14, AD-1, AD-8).
 *
 * A vanilla zustand store created at module load *outside* React. It holds two
 * things, neither a source of truth for crypto (which lives entirely in Rust):
 *
 * 1. `statuses` — the Rust-authoritative {@link BackupStatus} per account id,
 *    keyed by opaque account id (mirrors {@link encryptionStatusStore}). An
 *    account absent from the map (`undefined`) means "no status yet". The Settings
 *    backup row is a pure projection of this slice.
 * 2. The single backup modal state (there is at most one open at a time): its
 *    `mode` (`enable` | `restore`), the account it belongs to, the in-flight
 *    `phase`, a *named* `error` code, and — for the enable path — the base58
 *    `recoveryKey` returned once.
 *
 * The `recoveryKey` is the deliberate boundary exception (it must reach the human
 * to be saved), constrained tightly here: it is set only by the enable flow, held
 * only while the modal is open, and CLEARED on `close()` — never persisted beyond
 * the modal's lifecycle, never logged.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { BackupStatus, IpcErrorCode } from "@/lib/ipc/client";

/** The mode the backup modal is in. */
export type BackupMode = "enable" | "restore";

/**
 * The in-flight phase of the modal's action. `idle` is the resting state (the
 * restore textarea, or the pre-enable prompt); `working` is an enable/restore in
 * flight; `shown` (enable only) means the recovery key is displayed once;
 * `restored` (restore only) means history is unlocking; `failed` carries the
 * named `error`.
 */
export type BackupPhase = "idle" | "working" | "shown" | "restored" | "failed";

export interface KeyBackupState {
  /** The current backup status per account id, exactly as Rust streamed it. An
   * account absent from the map has not delivered a status yet. */
  statuses: Record<string, BackupStatus>;
  /** Whether the backup modal is open. */
  modalOpen: boolean;
  /** The mode the open modal is in (`enable` | `restore`), or `null` when closed. */
  mode: BackupMode | null;
  /** The account the open modal belongs to, or `null` when closed. */
  accountId: string | null;
  /** The base58 recovery key returned by enable, shown once. `null` unless an
   * enable succeeded this modal session; CLEARED on `close()`. */
  recoveryKey: string | null;
  /** The in-flight phase of the modal's action. */
  phase: BackupPhase;
  /** The named error code of the last failed action, or `null`. */
  error: IpcErrorCode | null;
  /** Record one account's current status (from a streamed batch). */
  setStatus: (accountId: string, status: BackupStatus) => void;
  /** Drop one account's entry (sign-out / subscription teardown). */
  removeAccount: (accountId: string) => void;
  /** Open the modal in enable mode for an account. */
  openEnable: (accountId: string) => void;
  /** Open the modal in restore mode for an account. */
  openRestore: (accountId: string) => void;
  /** Record the recovery key returned by a successful enable (shown once). */
  setRecoveryKey: (key: string) => void;
  /** Set the modal's in-flight phase. */
  setPhase: (phase: BackupPhase) => void;
  /** Record a named error and move to the `failed` phase. */
  setError: (error: IpcErrorCode) => void;
  /** Switch the open modal to restore mode (e.g. after a `backupExists` race),
   * resetting the transient action state. */
  switchToRestore: () => void;
  /** Close the modal and clear the transient action state — crucially the
   * `recoveryKey` (never persisted beyond the modal's lifecycle). */
  close: () => void;
  /** Clear every tracked account's status (test/reset). */
  reset: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const keyBackupStore = createStore<KeyBackupState>()((set) => ({
  statuses: {},
  modalOpen: false,
  mode: null,
  accountId: null,
  recoveryKey: null,
  phase: "idle",
  error: null,
  setStatus: (accountId, status) =>
    set((state) => ({ statuses: { ...state.statuses, [accountId]: status } })),
  removeAccount: (accountId) =>
    set((state) => {
      if (!(accountId in state.statuses)) {
        return state;
      }
      const { [accountId]: _removed, ...rest } = state.statuses;
      return { statuses: rest };
    }),
  openEnable: (accountId) =>
    set({
      modalOpen: true,
      mode: "enable",
      accountId,
      recoveryKey: null,
      phase: "idle",
      error: null,
    }),
  openRestore: (accountId) =>
    set({
      modalOpen: true,
      mode: "restore",
      accountId,
      recoveryKey: null,
      phase: "idle",
      error: null,
    }),
  setRecoveryKey: (key) => set({ recoveryKey: key, phase: "shown", error: null }),
  setPhase: (phase) => set({ phase, error: null }),
  setError: (error) => set({ error, phase: "failed" }),
  switchToRestore: () => set({ mode: "restore", recoveryKey: null, phase: "idle", error: null }),
  // Clear the recovery key on close: it must never outlive the open modal.
  close: () =>
    set({
      modalOpen: false,
      mode: null,
      accountId: null,
      recoveryKey: null,
      phase: "idle",
      error: null,
    }),
  reset: () =>
    set({
      statuses: {},
      modalOpen: false,
      mode: null,
      accountId: null,
      recoveryKey: null,
      phase: "idle",
      error: null,
    }),
}));

/**
 * The backup status for a single account, or `undefined` when no status has
 * arrived yet. A subscription hook over {@link keyBackupStore}.
 */
export function useKeyBackupStatus(accountId: string): BackupStatus | undefined {
  return useStore(keyBackupStore, (s) => s.statuses[accountId]);
}

/** Subscribe to whether the backup modal is open. */
export function useKeyBackupModalOpen(): boolean {
  return useStore(keyBackupStore, (s) => s.modalOpen);
}
