import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  encryptionPosture: vi.fn(() => Promise.resolve(null)),
}));

import {
  SDK_STORE_ENCRYPTED_STATUS,
  SDK_STORE_UNENCRYPTED_STATUS,
  STORAGE_HONESTY_SENTENCE,
} from "@/components/settings/at-rest-encryption-choice";
import { SettingsDialog } from "@/components/settings/settings-dialog";
import { encryptionPosture } from "@/lib/ipc/client";

const mockPosture = vi.mocked(encryptionPosture);

describe("SettingsDialog", () => {
  beforeEach(() => {
    mockPosture.mockClear();
  });

  afterEach(() => {
    vi.clearAllMocks();
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
});
