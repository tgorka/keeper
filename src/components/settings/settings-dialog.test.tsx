import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  encryptionPosture: vi.fn(() => Promise.resolve(null)),
  honorRemoteDeletions: vi.fn(() => Promise.resolve(false)),
  setHonorRemoteDeletions: vi.fn(() => Promise.resolve()),
  incognitoGetGlobal: vi.fn(() => Promise.resolve(false)),
  incognitoSetGlobal: vi.fn(() => Promise.resolve()),
  undoSendWindow: vi.fn(() => Promise.resolve(10)),
  setUndoSendWindow: vi.fn(() => Promise.resolve()),
  verificationCancel: vi.fn(() => Promise.resolve()),
}));

import {
  SDK_STORE_ENCRYPTED_STATUS,
  SDK_STORE_UNENCRYPTED_STATUS,
  STORAGE_HONESTY_SENTENCE,
} from "@/components/settings/at-rest-encryption-choice";
import { SettingsDialog } from "@/components/settings/settings-dialog";
import type { AccountVm } from "@/lib/ipc/client";
import {
  encryptionPosture,
  honorRemoteDeletions,
  setHonorRemoteDeletions,
  setUndoSendWindow,
  undoSendWindow,
} from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { encryptionStatusStore } from "@/lib/stores/encryption-status";
import { keyBackupStore } from "@/lib/stores/key-backup";
import { verificationStore } from "@/lib/stores/verification";
import { wizardStore } from "@/lib/stores/wizard";

const mockPosture = vi.mocked(encryptionPosture);
const mockHonorGet = vi.mocked(honorRemoteDeletions);
const mockHonorSet = vi.mocked(setHonorRemoteDeletions);
const mockUndoGet = vi.mocked(undoSendWindow);
const mockUndoSet = vi.mocked(setUndoSendWindow);

function account(id: string): AccountVm {
  return {
    accountId: id,
    userId: `@${id}:example.org`,
    homeserverUrl: "https://matrix.example.org/",
    hueIndex: 0,
    provider: "password",
  };
}

describe("SettingsDialog", () => {
  beforeEach(() => {
    mockPosture.mockClear();
    mockHonorGet.mockClear();
    mockHonorSet.mockClear();
    mockHonorGet.mockResolvedValue(false);
    mockHonorSet.mockResolvedValue(undefined);
    mockUndoGet.mockClear();
    mockUndoSet.mockClear();
    mockUndoGet.mockResolvedValue(10);
    mockUndoSet.mockResolvedValue(undefined);
    accountsStore.getState().clear();
    encryptionStatusStore.getState().reset();
    keyBackupStore.getState().reset();
    verificationStore.setState({ flow: null, modalOpen: false, activeAccountId: null });
    wizardStore.setState({ active: false, dismissed: false, step: "welcome", accountId: null });
  });

  afterEach(() => {
    vi.clearAllMocks();
    accountsStore.getState().clear();
    encryptionStatusStore.getState().reset();
    keyBackupStore.getState().reset();
    verificationStore.setState({ flow: null, modalOpen: false, activeAccountId: null });
  });

  it("shows the honest archive.db/keeper.db + FileVault copy when open", async () => {
    mockPosture.mockResolvedValue(false);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(await screen.findByText(STORAGE_HONESTY_SENTENCE)).toBeInTheDocument();
    // Section heading present.
    expect(screen.getByText("Archive & Storage")).toBeInTheDocument();
  });

  it("reflects the encrypted SDK-store status when posture is on", async () => {
    mockPosture.mockResolvedValue(true);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(await screen.findByText(SDK_STORE_ENCRYPTED_STATUS)).toBeInTheDocument();
    expect(screen.queryByText(SDK_STORE_UNENCRYPTED_STATUS)).not.toBeInTheDocument();
  });

  it("reflects the FileVault-only SDK-store status when posture is off", async () => {
    mockPosture.mockResolvedValue(false);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    await waitFor(() => expect(screen.getByText(SDK_STORE_UNENCRYPTED_STATUS)).toBeInTheDocument());
    expect(screen.queryByText(SDK_STORE_ENCRYPTED_STATUS)).not.toBeInTheDocument();
  });

  it("shows a Verify button only for an unverified account and opens the modal for it", () => {
    mockPosture.mockResolvedValue(null);
    accountsStore.getState().hydrateAll([account("alice"), account("bob")]);
    encryptionStatusStore.getState().setStatus("alice", "unverified");
    encryptionStatusStore.getState().setStatus("bob", "verified");
    render(<SettingsDialog open onOpenChange={() => {}} />);

    const verifyButtons = screen.getAllByRole("button", { name: "Verify" });
    expect(verifyButtons).toHaveLength(1);

    fireEvent.click(verifyButtons[0]);
    expect(verificationStore.getState().modalOpen).toBe(true);
    expect(verificationStore.getState().activeAccountId).toBe("alice");
  });

  it("shows no Verify button when accounts are verified or checking", () => {
    mockPosture.mockResolvedValue(null);
    accountsStore.getState().hydrateAll([account("alice"), account("bob")]);
    encryptionStatusStore.getState().setStatus("alice", "verified");
    // bob left as pending/unknown (no status set).
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(screen.queryByRole("button", { name: "Verify" })).not.toBeInTheDocument();
  });

  it("shows a 'Set up backup' button for a disabled backup and opens enable", () => {
    mockPosture.mockResolvedValue(null);
    accountsStore.getState().hydrateAll([account("alice")]);
    keyBackupStore.getState().setStatus("alice", "disabled");
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(screen.getByText("Not set up")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Set up backup" }));
    expect(keyBackupStore.getState().modalOpen).toBe(true);
    expect(keyBackupStore.getState().mode).toBe("enable");
    expect(keyBackupStore.getState().accountId).toBe("alice");
  });

  it("shows a 'Restore' button for an incomplete backup and opens restore", () => {
    mockPosture.mockResolvedValue(null);
    accountsStore.getState().hydrateAll([account("alice")]);
    keyBackupStore.getState().setStatus("alice", "incomplete");
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(screen.getByText("Needs your recovery key")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Restore" }));
    expect(keyBackupStore.getState().modalOpen).toBe(true);
    expect(keyBackupStore.getState().mode).toBe("restore");
    expect(keyBackupStore.getState().accountId).toBe("alice");
  });

  it("shows 'Backup on' with no button for an enabled backup", () => {
    mockPosture.mockResolvedValue(null);
    accountsStore.getState().hydrateAll([account("alice")]);
    keyBackupStore.getState().setStatus("alice", "enabled");
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(screen.getByText("Backup on")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Set up backup" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Restore" })).not.toBeInTheDocument();
  });

  it("shows 'Checking…' for a pending backup status", () => {
    mockPosture.mockResolvedValue(null);
    accountsStore.getState().hydrateAll([account("alice")]);
    // No backup status set → pending.
    render(<SettingsDialog open onOpenChange={() => {}} />);

    expect(screen.getAllByText("Checking…").length).toBeGreaterThan(0);
  });

  it("renders the honor-remote-deletions toggle and reads its initial state", async () => {
    mockPosture.mockResolvedValue(false);
    mockHonorGet.mockResolvedValue(true);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    const toggle = await screen.findByRole("switch", {
      name: "Honor remote deletions locally",
    });
    await waitFor(() => expect(toggle).toBeChecked());
    expect(mockHonorGet).toHaveBeenCalled();
  });

  it("persists the honor-remote-deletions toggle on change", async () => {
    mockPosture.mockResolvedValue(false);
    mockHonorGet.mockResolvedValue(false);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    const toggle = await screen.findByRole("switch", {
      name: "Honor remote deletions locally",
    });
    await waitFor(() => expect(toggle).not.toBeChecked());
    fireEvent.click(toggle);
    await waitFor(() => expect(mockHonorSet).toHaveBeenCalledWith(true));
    expect(toggle).toBeChecked();
  });

  it("renders the Undo-Send window control and reads its initial value", async () => {
    mockPosture.mockResolvedValue(false);
    mockUndoGet.mockResolvedValue(15);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    const input = await screen.findByLabelText("Undo-Send window (seconds)");
    await waitFor(() => expect(input).toHaveValue(15));
    expect(mockUndoGet).toHaveBeenCalled();
  });

  it("persists a changed Undo-Send window (clamping out-of-range input)", async () => {
    mockPosture.mockResolvedValue(false);
    mockUndoGet.mockResolvedValue(10);
    render(<SettingsDialog open onOpenChange={() => {}} />);

    const input = await screen.findByLabelText("Undo-Send window (seconds)");
    await waitFor(() => expect(input).toHaveValue(10));
    // An in-range value persists verbatim.
    fireEvent.change(input, { target: { value: "5" } });
    await waitFor(() => expect(mockUndoSet).toHaveBeenCalledWith(5));
    // An out-of-range value clamps to 60 before persisting.
    fireEvent.change(input, { target: { value: "99" } });
    await waitFor(() => expect(mockUndoSet).toHaveBeenCalledWith(60));
    expect(input).toHaveValue(60);
  });

  it("'Run setup again' starts the wizard and closes Settings", async () => {
    mockPosture.mockResolvedValue(false);
    const onOpenChange = vi.fn();
    render(<SettingsDialog open onOpenChange={onOpenChange} />);

    const runAgain = await screen.findByRole("button", { name: "Run setup again" });
    fireEvent.click(runAgain);

    expect(wizardStore.getState().active).toBe(true);
    expect(wizardStore.getState().step).toBe("welcome");
    expect(onOpenChange).toHaveBeenCalledWith(false);
  });
});
