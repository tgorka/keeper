import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  iosSyncDisclosureShownGet: vi.fn(() => Promise.resolve(false)),
  iosSyncDisclosureShownSet: vi.fn(() => Promise.resolve()),
}));

import {
  NO_BACKGROUND_SYNC_SENTENCE,
  NoBackgroundSyncDisclosure,
} from "@/components/settings/no-background-sync-disclosure";
import type { AccountVm } from "@/lib/ipc/client";
import { iosSyncDisclosureShownGet, iosSyncDisclosureShownSet } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { wizardStore } from "@/lib/stores/wizard";

const mockGet = vi.mocked(iosSyncDisclosureShownGet);
const mockSet = vi.mocked(iosSyncDisclosureShownSet);

/** All seven capabilities present = the desktop tier (the card never renders). */
const DESKTOP_CAPABILITIES = {
  trayIcon: true,
  globalHotkey: true,
  launchAtLogin: true,
  inAppUpdater: true,
  nativeMenuBar: true,
  bridgeSidecar: true,
  revealInFileManager: true,
};

function account(id: string): AccountVm {
  return {
    accountId: id,
    userId: `@${id}:example.org`,
    homeserverUrl: "https://matrix.example.org/",
    hueIndex: 0,
    provider: "password",
  };
}

/** Arrange the happy-path gates: reduced tier + one Account + wizard closed. */
function arrangeReducedTierWithAccount() {
  capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
  accountsStore.getState().hydrateAll([account("alice")]);
}

beforeEach(() => {
  mockGet.mockReset();
  mockGet.mockResolvedValue(false);
  mockSet.mockReset();
  mockSet.mockResolvedValue(undefined);
  accountsStore.getState().clear();
  wizardStore.setState({ active: false, dismissed: false, step: "welcome", accountId: null });
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
});

afterEach(() => {
  vi.clearAllMocks();
  accountsStore.getState().clear();
  wizardStore.setState({ active: false, dismissed: false, step: "welcome", accountId: null });
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
});

describe("NoBackgroundSyncDisclosure", () => {
  it("shows the canonical sentence on the reduced tier with an account, wizard closed, unshown", async () => {
    arrangeReducedTierWithAccount();
    render(<NoBackgroundSyncDisclosure />);

    expect(await screen.findByText(NO_BACKGROUND_SYNC_SENTENCE)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Got it" })).toBeInTheDocument();
    expect(mockGet).toHaveBeenCalled();
  });

  it("acknowledging latches the persisted flag and hides the card", async () => {
    arrangeReducedTierWithAccount();
    render(<NoBackgroundSyncDisclosure />);

    fireEvent.click(await screen.findByRole("button", { name: "Got it" }));

    await waitFor(() => expect(mockSet).toHaveBeenCalled());
    expect(screen.queryByText(NO_BACKGROUND_SYNC_SENTENCE)).not.toBeInTheDocument();
  });

  it("closing the dialog any other way also latches the flag and hides the card", async () => {
    arrangeReducedTierWithAccount();
    render(<NoBackgroundSyncDisclosure />);

    await screen.findByText(NO_BACKGROUND_SYNC_SENTENCE);
    fireEvent.click(screen.getByRole("button", { name: "Close" }));

    await waitFor(() => expect(mockSet).toHaveBeenCalled());
    expect(screen.queryByText(NO_BACKGROUND_SYNC_SENTENCE)).not.toBeInTheDocument();
  });

  it("a failed persist still hides the card for the session (swallowed, no throw)", async () => {
    arrangeReducedTierWithAccount();
    mockSet.mockRejectedValue({
      code: "internal",
      message: "boom",
      accountId: null,
      retriable: false,
    });
    render(<NoBackgroundSyncDisclosure />);

    fireEvent.click(await screen.findByRole("button", { name: "Got it" }));

    await waitFor(() => expect(mockSet).toHaveBeenCalled());
    expect(screen.queryByText(NO_BACKGROUND_SYNC_SENTENCE)).not.toBeInTheDocument();
  });

  it("never shows on desktop and never probes the latch there", async () => {
    capabilitiesStore.getState().applySnapshot(DESKTOP_CAPABILITIES);
    accountsStore.getState().hydrateAll([account("alice")]);
    render(<NoBackgroundSyncDisclosure />);

    // Give any (wrong) async read a chance to land before asserting absence.
    await Promise.resolve();
    expect(screen.queryByText(NO_BACKGROUND_SYNC_SENTENCE)).not.toBeInTheDocument();
    expect(mockGet).not.toHaveBeenCalled();
  });

  it("does not show mid-wizard", async () => {
    arrangeReducedTierWithAccount();
    wizardStore.setState({ active: true, dismissed: false, step: "welcome", accountId: null });
    render(<NoBackgroundSyncDisclosure />);

    await waitFor(() => expect(mockGet).toHaveBeenCalled());
    expect(screen.queryByText(NO_BACKGROUND_SYNC_SENTENCE)).not.toBeInTheDocument();
  });

  it("does not show with no account yet", async () => {
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    render(<NoBackgroundSyncDisclosure />);

    await waitFor(() => expect(mockGet).toHaveBeenCalled());
    expect(screen.queryByText(NO_BACKGROUND_SYNC_SENTENCE)).not.toBeInTheDocument();
  });

  it("does not show when the latch reads already-shown", async () => {
    arrangeReducedTierWithAccount();
    mockGet.mockResolvedValue(true);
    render(<NoBackgroundSyncDisclosure />);

    await waitFor(() => expect(mockGet).toHaveBeenCalled());
    expect(screen.queryByText(NO_BACKGROUND_SYNC_SENTENCE)).not.toBeInTheDocument();
  });

  it("treats a failed latch read as already-shown (never traps)", async () => {
    arrangeReducedTierWithAccount();
    mockGet.mockRejectedValue({
      code: "internal",
      message: "settings unreadable",
      accountId: null,
      retriable: false,
    });
    render(<NoBackgroundSyncDisclosure />);

    await waitFor(() => expect(mockGet).toHaveBeenCalled());
    expect(screen.queryByText(NO_BACKGROUND_SYNC_SENTENCE)).not.toBeInTheDocument();
  });

  it("appears once the wizard finishes on a reduced tier with an account (fresh-install hand-off)", async () => {
    arrangeReducedTierWithAccount();
    wizardStore.setState({ active: true, dismissed: false, step: "done", accountId: "alice" });
    render(<NoBackgroundSyncDisclosure />);

    await waitFor(() => expect(mockGet).toHaveBeenCalled());
    expect(screen.queryByText(NO_BACKGROUND_SYNC_SENTENCE)).not.toBeInTheDocument();

    // The wizard's Done step hands off to the shell → the card fires immediately.
    wizardStore.getState().finish();
    expect(await screen.findByText(NO_BACKGROUND_SYNC_SENTENCE)).toBeInTheDocument();
  });
});
