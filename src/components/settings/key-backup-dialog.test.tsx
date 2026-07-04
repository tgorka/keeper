import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { IpcError } from "@/lib/ipc/client";

// Mock every IPC action the modal can call so nothing touches Tauri.
const backupEnable = vi.fn((_a: string) => Promise.resolve("recovery-key-1234"));
const backupRestore = vi.fn((_a: string, _k: string) => Promise.resolve());
const backupSaveRecoveryKey = vi.fn((_a: string, _k: string) => Promise.resolve());
const backupSavedRecoveryKey = vi.fn((_a: string) => Promise.resolve<string | null>(null));
vi.mock("@/lib/ipc/client", () => ({
  backupEnable: (a: string) => backupEnable(a),
  backupRestore: (a: string, k: string) => backupRestore(a, k),
  backupSaveRecoveryKey: (a: string, k: string) => backupSaveRecoveryKey(a, k),
  backupSavedRecoveryKey: (a: string) => backupSavedRecoveryKey(a),
}));

import { KeyBackupDialog } from "@/components/settings/key-backup-dialog";
import { keyBackupStore } from "@/lib/stores/key-backup";

function ipcError(code: IpcError["code"]): IpcError {
  return { code, message: code, accountId: null, retriable: true };
}

beforeEach(() => {
  keyBackupStore.getState().reset();
  backupEnable.mockReset().mockResolvedValue("recovery-key-1234");
  backupRestore.mockReset().mockResolvedValue(undefined);
  backupSaveRecoveryKey.mockReset().mockResolvedValue(undefined);
  backupSavedRecoveryKey.mockReset().mockResolvedValue(null);
  // jsdom lacks a clipboard by default.
  Object.assign(navigator, {
    clipboard: { writeText: vi.fn(() => Promise.resolve()) },
  });
});

afterEach(() => {
  keyBackupStore.getState().reset();
});

describe("KeyBackupDialog — enable", () => {
  it("renders nothing when closed", () => {
    render(<KeyBackupDialog />);
    expect(screen.queryByText("Set up key backup")).not.toBeInTheDocument();
  });

  it("shows the recovery key once and gates Done behind the acknowledgment", async () => {
    keyBackupStore.getState().openEnable("acc-1");
    render(<KeyBackupDialog />);

    expect(backupEnable).toHaveBeenCalledWith("acc-1");
    const key = await screen.findByLabelText("Recovery key");
    expect(key).toHaveTextContent("recovery-key-1234");

    // Done is disabled until the user acknowledges they saved the key.
    const done = screen.getByRole("button", { name: "Done" });
    expect(done).toBeDisabled();

    fireEvent.click(screen.getByRole("checkbox"));
    await waitFor(() => expect(done).toBeEnabled());

    fireEvent.click(done);
    expect(keyBackupStore.getState().modalOpen).toBe(false);
    // The recovery key is cleared on close (never persisted beyond the modal).
    expect(keyBackupStore.getState().recoveryKey).toBeNull();
  });

  it("saves the recovery key to the Keychain on request", async () => {
    keyBackupStore.getState().openEnable("acc-1");
    render(<KeyBackupDialog />);
    await screen.findByLabelText("Recovery key");

    fireEvent.click(screen.getByRole("button", { name: "Save to Keychain" }));
    await waitFor(() =>
      expect(backupSaveRecoveryKey).toHaveBeenCalledWith("acc-1", "recovery-key-1234"),
    );
    expect(await screen.findByRole("button", { name: "Saved to Keychain" })).toBeInTheDocument();
  });

  it("switches to restore when the server already has a backup", async () => {
    backupEnable.mockRejectedValueOnce(ipcError("backupExists"));
    keyBackupStore.getState().openEnable("acc-1");
    render(<KeyBackupDialog />);

    await waitFor(() => expect(keyBackupStore.getState().mode).toBe("restore"));
    expect(await screen.findByText("Restore key backup")).toBeInTheDocument();
  });
});

describe("KeyBackupDialog — restore", () => {
  it("prefills the textarea from a saved recovery key", async () => {
    backupSavedRecoveryKey.mockResolvedValueOnce("saved-key-abcd");
    keyBackupStore.getState().openRestore("acc-1");
    render(<KeyBackupDialog />);

    const textarea = (await screen.findByLabelText("Recovery key")) as HTMLTextAreaElement;
    await waitFor(() => expect(textarea.value).toBe("saved-key-abcd"));
  });

  it("restores with the entered key and shows the restored state", async () => {
    keyBackupStore.getState().openRestore("acc-1");
    render(<KeyBackupDialog />);

    const textarea = screen.getByLabelText("Recovery key");
    fireEvent.change(textarea, { target: { value: "good-key" } });
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));

    await waitFor(() => expect(backupRestore).toHaveBeenCalledWith("acc-1", "good-key"));
    expect(await screen.findByText(/encrypted history is unlocking/i)).toBeInTheDocument();
  });

  it("shows a distinct named error for a malformed key", async () => {
    backupRestore.mockRejectedValueOnce(ipcError("backupMalformedKey"));
    keyBackupStore.getState().openRestore("acc-1");
    render(<KeyBackupDialog />);

    fireEvent.change(screen.getByLabelText("Recovery key"), { target: { value: "bad" } });
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));

    expect(await screen.findByText(/doesn't look like a recovery key/i)).toBeInTheDocument();
    expect(screen.queryByText(/didn't match this account/i)).not.toBeInTheDocument();
  });

  it("shows a distinct named error for a well-formed-but-wrong key", async () => {
    backupRestore.mockRejectedValueOnce(ipcError("backupIncorrectKey"));
    keyBackupStore.getState().openRestore("acc-1");
    render(<KeyBackupDialog />);

    fireEvent.change(screen.getByLabelText("Recovery key"), { target: { value: "wrong" } });
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));

    expect(await screen.findByText(/didn't match this account/i)).toBeInTheDocument();
    expect(screen.queryByText(/doesn't look like a recovery key/i)).not.toBeInTheDocument();
  });

  it("shows a generic error distinct from the named key errors", async () => {
    backupRestore.mockRejectedValueOnce(ipcError("backupFailed"));
    keyBackupStore.getState().openRestore("acc-1");
    render(<KeyBackupDialog />);

    fireEvent.change(screen.getByLabelText("Recovery key"), { target: { value: "key" } });
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));

    expect(await screen.findByText(/couldn't restore from key backup/i)).toBeInTheDocument();
    expect(screen.queryByText(/doesn't look like a recovery key/i)).not.toBeInTheDocument();
    expect(screen.queryByText(/didn't match this account/i)).not.toBeInTheDocument();
  });
});
