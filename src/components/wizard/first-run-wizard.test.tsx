import { fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { BridgeCatalogState } from "@/hooks/use-bridge-catalog";
import type { BridgeDiscoveryState } from "@/hooks/use-bridge-discovery";
import type { AccountVm, BridgeDiscoveryVm, BridgeNetworkVm } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { wizardStore } from "@/lib/stores/wizard";

// The wizard composes `LoginScreen` in addMode. Stub it with a controllable
// double so the tests can drive the success (addAccount + onDone) vs cancel
// (onDone only) branches without a real login round-trip. This mirrors the real
// contract: in addMode `onDone` fires on BOTH success and cancel, and success
// calls `addAccount` synchronously BEFORE `onDone`.
const addSuccessAccount = vi.hoisted(
  () =>
    ({
      accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
      userId: "@alice:example.org",
      homeserverUrl: "https://matrix.example.org/",
      hueIndex: 0,
      provider: "password",
    }) satisfies AccountVm,
);
vi.mock("@/components/auth/login-screen", () => ({
  LoginScreen: ({ onDone }: { addMode?: boolean; onDone?: () => void }) => (
    <div>
      <button
        type="button"
        onClick={() => {
          accountsStore.getState().addAccount(addSuccessAccount);
          onDone?.();
        }}
      >
        stub-add-success
      </button>
      <button type="button" onClick={() => onDone?.()}>
        stub-add-cancel
      </button>
    </div>
  ),
}));

// Mock the discovery + catalog hooks so the discovery step renders BridgeCards
// from fixture data (never hitting Rust). Each test sets the return values.
const mockCatalog = vi.hoisted(() => vi.fn<() => BridgeCatalogState>());
const mockDiscovery = vi.hoisted(() => vi.fn<(accountId: string) => BridgeDiscoveryState>());
vi.mock("@/hooks/use-bridge-catalog", () => ({
  useBridgeCatalog: () => mockCatalog(),
}));
vi.mock("@/hooks/use-bridge-discovery", () => ({
  useBridgeDiscovery: (accountId: string) => mockDiscovery(accountId),
}));

// BridgeCard (rendered by the discovery step) opens a login Sheet that calls the
// streaming IPC client on proceed; stub the client so the card never touches a
// real Tauri channel.
vi.mock("@/lib/ipc/client", () => ({
  startBridgeLogin: vi.fn(() => new Promise<number>(() => {})),
  submitBridgeLogin: vi.fn(() => Promise.resolve()),
  cancelBridgeLogin: vi.fn(() => Promise.resolve()),
  bridgeBotRoom: vi.fn(() => Promise.resolve("!bot:example.org")),
}));

import { FirstRunWizard } from "@/components/wizard/first-run-wizard";

const matrixNetwork: BridgeNetworkVm = {
  networkId: "whatsapp",
  name: "WhatsApp",
  glyph: "WA",
  tier: "low",
  tierLabel: "Low risk",
  badgeStyle: "secondary",
  requiresAck: false,
  ackCopy: null,
};

const catalogReady: BridgeCatalogState = {
  catalog: [matrixNetwork],
  loading: false,
  error: null,
};

function discoveryReady(networks: BridgeDiscoveryVm["networks"]): BridgeDiscoveryState {
  return {
    discovery: { homeserver: "example.org", networks },
    loading: false,
    error: null,
    retriable: false,
    retry: vi.fn(),
  };
}

function discoveryError(): BridgeDiscoveryState {
  return {
    discovery: null,
    loading: false,
    error: "temporary failure",
    retriable: true,
    retry: vi.fn(),
  };
}

describe("FirstRunWizard", () => {
  beforeEach(() => {
    accountsStore.getState().clear();
    // Start the wizard fresh at the welcome step for each test.
    wizardStore.getState().start();
    mockCatalog.mockReset();
    mockDiscovery.mockReset();
    mockCatalog.mockReturnValue(catalogReady);
    mockDiscovery.mockReturnValue(
      discoveryReady([{ networkId: "whatsapp", status: "configured" }]),
    );
  });

  afterEach(() => {
    accountsStore.getState().clear();
    wizardStore.setState({ active: false, dismissed: false, step: "welcome", accountId: null });
  });

  it("Welcome → Add account: Get started advances to the add-account step", () => {
    render(<FirstRunWizard />);
    expect(screen.getByText("Welcome to keeper")).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Get started" }));
    expect(wizardStore.getState().step).toBe("addAccount");
    // The composed login screen (stubbed) is now shown.
    expect(screen.getByRole("button", { name: "stub-add-success" })).toBeInTheDocument();
  });

  it("Add-account success (count grows) → advances to discovery with the newest account", () => {
    render(<FirstRunWizard />);
    fireEvent.click(screen.getByRole("button", { name: "Get started" }));
    fireEvent.click(screen.getByRole("button", { name: "stub-add-success" }));

    expect(wizardStore.getState().step).toBe("discovery");
    expect(wizardStore.getState().accountId).toBe(addSuccessAccount.accountId);
  });

  it("Add-account cancel (count unchanged) → returns to Welcome, does not advance", () => {
    render(<FirstRunWizard />);
    fireEvent.click(screen.getByRole("button", { name: "Get started" }));
    fireEvent.click(screen.getByRole("button", { name: "stub-add-cancel" }));

    expect(wizardStore.getState().step).toBe("welcome");
    expect(wizardStore.getState().accountId).toBeNull();
    expect(screen.getByText("Welcome to keeper")).toBeInTheDocument();
  });

  it("discovery renders a BridgeCard per discovered network from mocked catalog + discovery", () => {
    accountsStore.getState().addAccount(addSuccessAccount);
    wizardStore.getState().setAccountId(addSuccessAccount.accountId);
    wizardStore.getState().goTo("discovery");
    render(<FirstRunWizard />);

    expect(mockDiscovery).toHaveBeenCalledWith(addSuccessAccount.accountId);
    // The BridgeCard for the discovered, catalog-joined network is present.
    expect(screen.getByText("WhatsApp")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Connect WhatsApp" })).toBeInTheDocument();
  });

  it("discovery shows the 'No bridges found' empty state with a companion-stack link", () => {
    accountsStore.getState().addAccount(addSuccessAccount);
    wizardStore.getState().setAccountId(addSuccessAccount.accountId);
    wizardStore.getState().goTo("discovery");
    mockDiscovery.mockReturnValue(discoveryReady([]));
    render(<FirstRunWizard />);

    expect(screen.getByText(/No bridges found on example.org/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Set up a companion stack" })).toBeInTheDocument();
  });

  it("discovery surfaces a retriable discovery error with a Retry", () => {
    accountsStore.getState().addAccount(addSuccessAccount);
    wizardStore.getState().setAccountId(addSuccessAccount.accountId);
    wizardStore.getState().goTo("discovery");
    mockDiscovery.mockReturnValue(discoveryError());
    render(<FirstRunWizard />);

    expect(screen.getByText(/Could not discover bridges/)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Retry" })).toBeInTheDocument();
  });

  it("Skip on a step opens the confirm and confirming calls finish()", async () => {
    render(<FirstRunWizard />);
    fireEvent.click(screen.getByRole("button", { name: "Skip setup" }));

    const dialog = await screen.findByRole("alertdialog");
    expect(within(dialog).getByText("Skip setup?")).toBeInTheDocument();
    fireEvent.click(within(dialog).getByRole("button", { name: "Skip setup" }));

    expect(wizardStore.getState().active).toBe(false);
    // Zero accounts ⇒ dismissed so App lands in the empty inbox, not login.
    expect(wizardStore.getState().dismissed).toBe(true);
  });

  it("Esc opens the confirm (does not exit immediately); confirming calls finish()", async () => {
    render(<FirstRunWizard />);
    fireEvent.keyDown(window, { key: "Escape" });

    // Still active after the first Esc — the confirm is shown, not an exit.
    expect(wizardStore.getState().active).toBe(true);
    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Skip setup" }));
    expect(wizardStore.getState().active).toBe(false);
  });

  it("re-entry with an existing account starts directly at discovery for that account", () => {
    // Settings "Run setup again" path: with an account already present, start()
    // must land on discovery (not welcome) so the user reaches bridge setup
    // without a redundant sign-in.
    accountsStore.getState().clear();
    accountsStore.getState().addAccount(addSuccessAccount);
    wizardStore.getState().start();

    expect(wizardStore.getState().step).toBe("discovery");
    expect(wizardStore.getState().accountId).toBe(addSuccessAccount.accountId);
  });

  it("Esc stands down while a nested overlay is open (does not pop the skip-confirm)", () => {
    render(<FirstRunWizard />);
    // Simulate an open nested Radix overlay (e.g. the bridge-login Sheet the
    // discovery step's BridgeCard drives), which renders with role="dialog".
    const nested = document.createElement("div");
    nested.setAttribute("role", "dialog");
    document.body.appendChild(nested);
    try {
      fireEvent.keyDown(window, { key: "Escape" });
      // The wizard's own skip-confirm must NOT have opened — Escape belongs to
      // the nested overlay.
      expect(screen.queryByText("Skip setup?")).not.toBeInTheDocument();
      expect(wizardStore.getState().active).toBe(true);
    } finally {
      nested.remove();
    }
  });

  it("Add-account step keeps Skip reachable and shows the honest no-homeserver fork", () => {
    render(<FirstRunWizard />);
    fireEvent.click(screen.getByRole("button", { name: "Get started" }));

    // Skip lives in the persistent wizard chrome so LoginScreen's full-viewport
    // layout can never push it below the fold.
    expect(screen.getByRole("button", { name: "Skip setup" })).toBeInTheDocument();
    // The no-homeserver fork banner (with the real companion-stack docs link).
    expect(screen.getByText(/No homeserver yet\?/)).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "set up a companion stack" })).toBeInTheDocument();
  });

  it("Done: Enter keeper calls finish()", () => {
    accountsStore.getState().addAccount(addSuccessAccount);
    wizardStore.getState().goTo("done");
    render(<FirstRunWizard />);

    fireEvent.click(screen.getByRole("button", { name: "Enter keeper" }));
    expect(wizardStore.getState().active).toBe(false);
    // With an account added, finish() does not dismiss into an empty inbox.
    expect(wizardStore.getState().dismissed).toBe(false);
  });
});
