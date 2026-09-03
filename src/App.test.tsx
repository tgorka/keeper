import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm } from "@/lib/ipc/client";

// Mount `App` runs `useSessionRestore`; override just `sessionRestore` with a
// never-resolving stub so the boot hook never mutates the store (each test drives
// the gate directly). Every other wrapper (e.g. the shell's connection-status
// subscribe) keeps its real implementation.
const mockEncryptionPosture = vi.hoisted(() => vi.fn());
// The tray's open-note bridge (Story 44.6). Mocked here so the App suite can
// assert that mounting the app SUBSCRIBES to it — the hook's own suite renders
// the hook directly and therefore cannot tell a mounted listener from an
// unmounted one, which is precisely how `listenNotesOpenNote` came to be
// declared and called from nowhere for two epics.
const mockListenNotesOpenNote = vi.hoisted(() => vi.fn(async () => () => {}));
// The capability handshake (Story 12.2). Pending by default so every test
// above the no-account path sees the safe default; that path resolves a phone.
// The executor form, not `Promise.withResolvers`: the project compiles
// against `lib: ES2020`, where that constructor method does not exist.
const mockCapabilities = vi.hoisted(() => vi.fn(() => new Promise(() => {})));

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
    listenNotesOpenNote: mockListenNotesOpenNote,
    capabilities: mockCapabilities,
  };
});

import { CHOICE_TITLE } from "@/components/settings/at-rest-encryption-choice";
import { accountsStore } from "@/lib/stores/accounts";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { leadingDrawerStore } from "@/lib/stores/leading-drawer";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { wizardStore } from "@/lib/stores/wizard";
import App, { NO_ACCOUNT_BOTS_LABEL } from "./App";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 0,
  provider: "password",
};

/** A phone: it can talk to a model and nothing else is on. */
const PHONE = { ...DEFAULT_CAPABILITIES, bots: true };

const originalMatchMedia = window.matchMedia;
/** The phone-shell suite's viewport stub: 390px wide, reduced motion. */
function mockPhoneViewport() {
  window.matchMedia = vi.fn().mockImplementation((query: string) => {
    const match = query.match(/max-width:\s*(\d+)px/);
    const maxWidth = match ? Number(match[1]) : Number.POSITIVE_INFINITY;
    const matches = query.includes("prefers-reduced-motion") ? true : 390 <= maxWidth;
    return {
      matches,
      media: query,
      onchange: null,
      addEventListener: vi.fn(),
      removeEventListener: vi.fn(),
      addListener: vi.fn(),
      removeListener: vi.fn(),
      dispatchEvent: vi.fn(),
    };
  });
}

describe("App", () => {
  beforeEach(() => {
    accountsStore.getState().clear();
    accountsStore.setState({ hydrated: false });
    wizardStore.setState({ active: false, dismissed: false, step: "welcome", accountId: null });
    // Default: posture chosen (off) so the login screen shows past the gate.
    mockEncryptionPosture.mockReset();
    mockEncryptionPosture.mockResolvedValue(false);
    mockListenNotesOpenNote.mockClear();
    mockCapabilities.mockReset();
    mockCapabilities.mockImplementation(() => new Promise(() => {}));
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
    primaryViewStore.getState().setView("inbox");
    leadingDrawerStore.getState().close();
  });

  afterEach(() => {
    accountsStore.getState().clear();
    accountsStore.setState({ hydrated: false });
    wizardStore.setState({ active: false, dismissed: false, step: "welcome", accountId: null });
    window.matchMedia = originalMatchMedia;
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
    primaryViewStore.getState().setView("inbox");
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

  /**
   * Story 63.1, AD-180: a phone with no Matrix account can still reach Bots.
   * The login screen is not replaced — Sign in is still the form — but it is
   * no longer a wall: one named control under it opens the shell on Bots,
   * and Add account stays reachable in the drawer after it. The control is
   * absent where the build has no Bots (AD-27), which the default here is.
   */
  it("offers a way into Bots with zero accounts at phone width, and still offers sign-in", async () => {
    mockPhoneViewport();
    mockCapabilities.mockResolvedValue(PHONE);
    // Boot with an account so the first-run decision locks out, then sign it
    // out: the zero-account login screen, the state a signed-out phone sits in.
    accountsStore.getState().addAccount(account);
    accountsStore.getState().markHydrated();
    const { rerender } = render(<App />);
    await waitFor(() => expect(capabilitiesStore.getState().hydrated).toBe(true));
    accountsStore.getState().removeAccount(account.accountId);
    rerender(<App />);

    // Signing in is still the primary path, and the way past it is beside it.
    expect(await screen.findByRole("button", { name: "Sign in" })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: NO_ACCOUNT_BOTS_LABEL }));

    // The shell, on Bots: the phone stack's level 1 with its back bar, and no
    // login form.
    expect(await screen.findByRole("region", { name: "Bots" })).toBeInTheDocument();
    expect(primaryViewStore.getState().view).toBe("bots");
    expect(screen.queryByRole("button", { name: "Sign in" })).not.toBeInTheDocument();
    // Add account is still there, in the drawer under the Inbox.
    act(() => {
      leadingDrawerStore.getState().open();
    });
    expect(await screen.findByRole("button", { name: "Add account" })).toBeInTheDocument();
  });

  it("has no way past the login screen where the build has no Bots", async () => {
    // Absent, not disabled: the safe default (every surface off) is the
    // capability mirror of a build that cannot talk to a model.
    accountsStore.getState().addAccount(account);
    accountsStore.getState().markHydrated();
    const { rerender } = render(<App />);
    await waitFor(() => expect(screen.getByRole("main")).toBeInTheDocument());
    accountsStore.getState().removeAccount(account.accountId);
    rerender(<App />);
    expect(await screen.findByRole("button", { name: "Sign in" })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: NO_ACCOUNT_BOTS_LABEL })).not.toBeInTheDocument();
  });

  /**
   * Story 44.6, FR-160/FR-102. The tray creates a note, raises the window and
   * emits `keeper://notes-open-note`; if nothing in the webview subscribed, the
   * note exists and the user is never shown it. That is not a wrong behaviour
   * with a failing assertion somewhere — it is an ABSENT one, which is why it
   * survived two epics, and why the assertion has to live here rather than in
   * the hook's own suite: `renderHook(() => useNotesOpenNote())` proves the hook
   * works, never that anything mounts it.
   *
   * Asserted at the splash, before any account exists, because the tray is
   * meant to work whatever is on screen (FR-102).
   */
  it("subscribes to the tray's open-note bridge as soon as the app mounts", async () => {
    render(<App />);

    await waitFor(() => expect(mockListenNotesOpenNote).toHaveBeenCalledTimes(1));
  });
});
