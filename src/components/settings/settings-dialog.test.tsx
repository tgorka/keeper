import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  encryptionPosture: vi.fn(() => Promise.resolve(null)),
  verificationCancel: vi.fn(() => Promise.resolve()),
}));

import {
  SDK_STORE_ENCRYPTED_STATUS,
  SDK_STORE_UNENCRYPTED_STATUS,
  STORAGE_HONESTY_SENTENCE,
} from "@/components/settings/at-rest-encryption-choice";
import { SettingsDialog } from "@/components/settings/settings-dialog";
import type { AccountVm } from "@/lib/ipc/client";
import { encryptionPosture } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { encryptionStatusStore } from "@/lib/stores/encryption-status";
import { verificationStore } from "@/lib/stores/verification";

const mockPosture = vi.mocked(encryptionPosture);

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
    accountsStore.getState().clear();
    encryptionStatusStore.getState().reset();
    verificationStore.setState({ flow: null, modalOpen: false, activeAccountId: null });
  });

  afterEach(() => {
    vi.clearAllMocks();
    accountsStore.getState().clear();
    encryptionStatusStore.getState().reset();
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
});
