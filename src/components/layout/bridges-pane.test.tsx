import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, BridgeNetworkVm, Provider } from "@/lib/ipc/client";

// Mock the catalog fetch so the pane never touches Tauri; the spy returns a fixed
// two-network catalog (one low-risk, one volatile).
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
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    bridgeCatalog: () => bridgeCatalog(),
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
});

afterEach(() => {
  accountsStore.getState().clear();
});

describe("BridgesPane", () => {
  it("shows an empty state and no cards when there are no accounts", async () => {
    renderPane();
    expect(await screen.findByText("Add an account to set up bridges.")).toBeInTheDocument();
    // No card action buttons render with zero accounts.
    expect(screen.queryByRole("button", { name: /Connect|Set up/ })).not.toBeInTheDocument();
  });

  it("renders a card per account × network", async () => {
    accountsStore.getState().hydrateAll([alice, bob]);
    renderPane();

    // Two accounts × two networks = four cards. Each low-risk card is "Connect",
    // each volatile is "Set up".
    await waitFor(() => {
      expect(screen.getAllByRole("button", { name: "Connect Matrix" })).toHaveLength(2);
    });
    expect(screen.getAllByRole("button", { name: "Set up Instagram" })).toHaveLength(2);

    // Both account sections are present.
    expect(screen.getByText("@alice:example.org")).toBeInTheDocument();
    expect(screen.getByText("@bob:example.org")).toBeInTheDocument();
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
