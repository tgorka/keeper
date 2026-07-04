import { render, screen } from "@testing-library/react";
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
import App from "./App";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 0,
};

describe("App", () => {
  beforeEach(() => {
    accountsStore.getState().clear();
    accountsStore.setState({ hydrated: false });
    // Default: posture chosen (off) so the login screen shows past the gate.
    mockEncryptionPosture.mockReset();
    mockEncryptionPosture.mockResolvedValue(false);
  });

  afterEach(() => {
    accountsStore.getState().clear();
    accountsStore.setState({ hydrated: false });
  });

  it("renders a splash while the boot restore is in flight (not hydrated)", () => {
    render(<App />);
    expect(screen.getByRole("status", { name: "Loading keeper" })).toBeInTheDocument();
    // Neither the login screen nor the shell shows behind the splash.
    expect(screen.queryByRole("button", { name: "Sign in" })).not.toBeInTheDocument();
    expect(screen.queryByRole("main")).not.toBeInTheDocument();
  });

  it("renders the login screen when hydrated, unauthenticated, and the posture is chosen", async () => {
    mockEncryptionPosture.mockResolvedValue(false);
    accountsStore.getState().markHydrated();
    render(<App />);
    expect(await screen.findByRole("button", { name: "Sign in" })).toBeInTheDocument();
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
});
