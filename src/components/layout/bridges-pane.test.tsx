import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, BridgeDiscoveryVm, BridgeNetworkVm, Provider } from "@/lib/ipc/client";

// Mock the catalog + discovery fetches so the pane never touches Tauri. The catalog
// is the presentation join table; discovery drives which cards appear per account.
const catalog: BridgeNetworkVm[] = [
  {
    networkId: "matrix",
    name: "Matrix",
    glyph: "MX",
    tier: "low",
    tierLabel: "Low risk",
    badgeStyle: "secondary",
    requiresAck: false,
    ackCopy: null,
  },
  {
    networkId: "instagram",
    name: "Instagram",
    glyph: "IG",
    tier: "volatile",
    tierLabel: "Volatile — opt-in",
    badgeStyle: "filledDisconnected",
    requiresAck: true,
    ackCopy: "risk copy",
  },
];

const bridgeCatalog = vi.fn(() => Promise.resolve(catalog));
const bridgeDiscover = vi.fn((_accountId: string) => Promise.resolve(EMPTY_DISCOVERY));

const EMPTY_DISCOVERY: BridgeDiscoveryVm = { homeserver: "example.org", networks: [] };
const TWO_NETWORK_DISCOVERY: BridgeDiscoveryVm = {
  homeserver: "example.org",
  networks: [
    { networkId: "matrix", status: "loggedIn" },
    { networkId: "instagram", status: "notLoggedIn" },
  ],
};

vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    bridgeCatalog: () => bridgeCatalog(),
    bridgeDiscover: (accountId: string) => bridgeDiscover(accountId),
  };
});

import { BridgesPane } from "@/components/layout/bridges-pane";
import { TooltipProvider } from "@/components/ui/tooltip";
import { accountsStore } from "@/lib/stores/accounts";

function account(id: string, userId: string, hue = 0, provider: Provider = "password"): AccountVm {
  return {
    accountId: id,
    userId,
    homeserverUrl: "https://matrix.example.org/",
    hueIndex: hue,
    provider,
  };
}

const alice = account("01ARZ3NDEKTSV4RRFFQ69G5FAV", "@alice:example.org", 0);
const bob = account("01BX5ZZKBKACTAV9WEVGEMMVRZ", "@bob:example.org", 1);

function renderPane() {
  return render(
    <TooltipProvider>
      <BridgesPane />
    </TooltipProvider>,
  );
}

beforeEach(() => {
  accountsStore.getState().clear();
  bridgeCatalog.mockClear();
  bridgeCatalog.mockResolvedValue(catalog);
  bridgeDiscover.mockClear();
  bridgeDiscover.mockResolvedValue(EMPTY_DISCOVERY);
});

afterEach(() => {
  accountsStore.getState().clear();
});

describe("BridgesPane", () => {
  it("shows an empty state and no cards when there are no accounts", async () => {
    renderPane();
    expect(await screen.findByText("Add an account to set up bridges.")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Connect|Set up/ })).not.toBeInTheDocument();
  });

  it("renders a card per discovered network × account, joined to the catalog", async () => {
    bridgeDiscover.mockResolvedValue(TWO_NETWORK_DISCOVERY);
    accountsStore.getState().hydrateAll([alice, bob]);
    renderPane();

    // Two accounts × two discovered networks = four cards. Matrix is "Connect",
    // Instagram (volatile) is "Set up".
    await waitFor(() => {
      expect(screen.getAllByRole("button", { name: "Connect Matrix" })).toHaveLength(2);
    });
    expect(screen.getAllByRole("button", { name: "Set up Instagram" })).toHaveLength(2);

    // Discovery ran once per account.
    expect(bridgeDiscover).toHaveBeenCalledWith(alice.accountId);
    expect(bridgeDiscover).toHaveBeenCalledWith(bob.accountId);

    // Both account sections are present.
    expect(screen.getByText("@alice:example.org")).toBeInTheDocument();
    expect(screen.getByText("@bob:example.org")).toBeInTheDocument();
  });

  it("shows the 'No bridges found' empty state with a docs link when discovery is empty", async () => {
    bridgeDiscover.mockResolvedValue(EMPTY_DISCOVERY);
    accountsStore.getState().hydrateAll([alice]);
    renderPane();

    expect(await screen.findByText(/No bridges found on example\.org\./)).toBeInTheDocument();
    const link = screen.getByRole("link", { name: /companion stack/i });
    expect(link).toHaveAttribute("href", "https://github.com/tgorka/keeper/tree/main/docs");
    expect(link).toHaveAttribute("target", "_blank");
    expect(link).toHaveAttribute("rel", "noreferrer");
    // No cards.
    expect(screen.queryByRole("button", { name: /Connect|Set up/ })).not.toBeInTheDocument();
  });

  it("drops an uncatalogued discovered network (no card)", async () => {
    bridgeDiscover.mockResolvedValue({
      homeserver: "example.org",
      networks: [
        { networkId: "matrix", status: "loggedIn" },
        { networkId: "bogusnet", status: "configured" },
      ],
    });
    accountsStore.getState().hydrateAll([alice]);
    renderPane();

    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Connect Matrix" })).toBeInTheDocument();
    });
    // The uncatalogued network is not carded.
    expect(screen.queryByText("bogusnet")).not.toBeInTheDocument();
  });

  it("shows a retriable per-account error with a Retry that re-runs discovery", async () => {
    bridgeDiscover.mockRejectedValueOnce({
      code: "syncUnavailable",
      message: "homeserver unreachable",
      accountId: null,
      retriable: true,
    });
    accountsStore.getState().hydrateAll([alice]);
    renderPane();

    expect(await screen.findByRole("alert")).toHaveTextContent(/homeserver unreachable/);
    const retry = screen.getByRole("button", { name: "Retry" });
    // The next attempt resolves with a discovered network.
    bridgeDiscover.mockResolvedValue(TWO_NETWORK_DISCOVERY);
    retry.click();
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Connect Matrix" })).toBeInTheDocument();
    });
  });

  it("shows an error state when the catalog fails to load", async () => {
    bridgeCatalog.mockRejectedValueOnce({
      code: "internal",
      message: "bad data",
      accountId: null,
      retriable: false,
    });
    accountsStore.getState().hydrateAll([alice]);
    renderPane();

    expect(await screen.findByRole("alert")).toHaveTextContent(/bad data/);
  });
});
