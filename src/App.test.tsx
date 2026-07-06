import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm } from "@/lib/ipc/client";

// Mount `App` runs `useSessionRestore`; override just `sessionRestore` with a
// never-resolving stub so the boot hook never mutates the store (each test drives
// the gate directly). Every other wrapper (e.g. the shell's connection-status
// subscribe) keeps its real implementation.
const mockEncryptionPosture = vi.hoisted(() => vi.fn());

vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  const pending = () => new Promise(() => {});
  return {
    ...actual,
    // Never-resolving stubs so the boot hook and shell subscribes never mutate a
    // store or reject (each test drives the gate directly).
    sessionRestore: pending,
    subscribeInbox: pending,
    unsubscribeInbox: () => Promise.resolve(),
    subscribeConnectionStatus: pending,
    unsubscribeConnectionStatus: () => Promise.resolve(),
    // Drives the first-run at-rest-encryption gate (Story 2.6); each test sets
    // the resolved value. Defaults to "chosen off" so unrelated tests see login.
    encryptionPosture: mockEncryptionPosture,
  };
});

import { CHOICE_TITLE } from "@/components/settings/at-rest-encryption-choice";
import { accountsStore } from "@/lib/stores/accounts";
import { wizardStore } from "@/lib/stores/wizard";
import App from "./App";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 0,
  provider: "password",
};

describe("App", () => {
  beforeEach(() => {
    accountsStore.getState().clear();
    accountsStore.setState({ hydrated: false });
    wizardStore.setState({ active: false, dismissed: false, step: "welcome", accountId: null });
    // Default: posture chosen (off) so the login screen shows past the gate.
    mockEncryptionPosture.mockReset();
    mockEncryptionPosture.mockResolvedValue(false);
  });

  afterEach(() => {
    accountsStore.getState().clear();
    accountsStore.setState({ hydrated: false });
    wizardStore.setState({ active: false, dismissed: false, step: "welcome", accountId: null });
  });

  it("renders a splash while the boot restore is in flight (not hydrated)", () => {
    render(<App />);
    expect(screen.getByRole("status", { name: "Loading keeper" })).toBeInTheDocument();
    // Neither the login screen nor the shell shows behind the splash.
    expect(screen.queryByRole("button", { name: "Sign in" })).not.toBeInTheDocument();
    expect(screen.queryByRole("main")).not.toBeInTheDocument();
  });

  it("auto-starts the first-run wizard (not the bare login screen) when hydrated, unauthenticated, and the posture is chosen", async () => {
    // First run (zero accounts, posture resolved) now opens the wizard full-frame
    // in place of the bare login screen (Story 6.8). The login screen still lives
    // *inside* the wizard's Add-Account step, but the frame is the wizard.
    mockEncryptionPosture.mockResolvedValue(false);
    accountsStore.getState().markHydrated();
    render(<App />);
    expect(await screen.findByRole("region", { name: "First-run setup" })).toBeInTheDocument();
    expect(screen.queryByText(CHOICE_TITLE)).not.toBeInTheDocument();
    expect(screen.queryByRole("main")).not.toBeInTheDocument();
  });

  it("renders the first-run encryption choice when the posture is unchosen (null)", async () => {
    mockEncryptionPosture.mockResolvedValue(null);
    accountsStore.getState().markHydrated();
    render(<App />);
    expect(await screen.findByText(CHOICE_TITLE)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Sign in" })).not.toBeInTheDocument();
  });

  it("renders the app shell landmarks once hydrated with an account set", () => {
    accountsStore.getState().addAccount(account);
    accountsStore.getState().markHydrated();
    render(<App />);
    expect(screen.getByRole("navigation", { name: "Views" })).toBeInTheDocument();
    expect(screen.getByRole("main")).toBeInTheDocument();
    // The room-list subscribe has not delivered a batch yet, so the chat list
    // sits in its loading state.
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
  });

  // --- First-run wizard (Story 6.8) ---------------------------------------

  it("renders the wizard full-frame when it is active (takes precedence over the login gate)", () => {
    wizardStore.getState().start();
    accountsStore.getState().markHydrated();
    render(<App />);
    expect(screen.getByRole("region", { name: "First-run setup" })).toBeInTheDocument();
    // The bare login screen is NOT shown behind the wizard.
    expect(screen.queryByRole("button", { name: "Sign in" })).not.toBeInTheDocument();
  });

  it("auto-starts the wizard once on first run (hydrated, zero accounts, posture resolved)", async () => {
    mockEncryptionPosture.mockResolvedValue(false);
    accountsStore.getState().markHydrated();
    render(<App />);
    // Posture resolves async → the boot effect starts the wizard.
    await waitFor(() => expect(wizardStore.getState().active).toBe(true));
    expect(await screen.findByRole("region", { name: "First-run setup" })).toBeInTheDocument();
  });

  it("does NOT auto-start the wizard while the posture is still loading (undefined)", async () => {
    // A never-resolving posture keeps it undefined; the boot effect must not fire.
    mockEncryptionPosture.mockReturnValue(new Promise(() => {}));
    accountsStore.getState().markHydrated();
    render(<App />);
    // Give the effects a tick; the wizard stays inactive and the splash holds.
    await Promise.resolve();
    expect(wizardStore.getState().active).toBe(false);
  });

  it("renders the empty-inbox shell (not the login screen) when the wizard is dismissed with zero accounts", async () => {
    mockEncryptionPosture.mockResolvedValue(false);
    accountsStore.getState().markHydrated();
    render(<App />);
    // First run auto-starts the wizard; the boot decision is now locked out.
    await waitFor(() => expect(wizardStore.getState().active).toBe(true));
    // Skipping with zero accounts finishes it as dismissed → App lands in the shell,
    // NOT the bare login screen.
    wizardStore.getState().finish();
    expect(wizardStore.getState().dismissed).toBe(true);
    expect(await screen.findByRole("main")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Sign in" })).not.toBeInTheDocument();
  });

  it("still renders the login screen after a sign-out of the last account (wizard does NOT auto-start)", async () => {
    mockEncryptionPosture.mockResolvedValue(false);
    // Boot WITH an account so the one-shot boot decision locks out (not first-run),
    // then sign that last account out — the wizard must not auto-start, and App
    // falls back to the bare login screen (not the dismissed empty-inbox shell).
    accountsStore.getState().addAccount(account);
    accountsStore.getState().markHydrated();
    const { rerender } = render(<App />);
    // Let the boot posture resolve and lock the first-run decision.
    await waitFor(() => expect(screen.getByRole("main")).toBeInTheDocument());

    accountsStore.getState().removeAccount(account.accountId);
    rerender(<App />);

    expect(await screen.findByRole("button", { name: "Sign in" })).toBeInTheDocument();
    expect(screen.queryByRole("main")).not.toBeInTheDocument();
    expect(wizardStore.getState().active).toBe(false);
  });
});
