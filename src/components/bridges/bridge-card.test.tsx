import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { BridgeCard } from "@/components/bridges/bridge-card";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { BadgeStyle, BridgeNetworkVm, BridgeStatus, RiskTier } from "@/lib/ipc/client";

// The login Sheet opened on proceed calls the streaming IPC client on mount; stub
// it so the card tests never touch a real Tauri channel. A never-resolving start
// keeps the Sheet in its initial waiting state.
vi.mock("@/lib/ipc/client", () => ({
  startBridgeLogin: vi.fn(() => new Promise<number>(() => {})),
  submitBridgeLogin: vi.fn(() => Promise.resolve()),
  cancelBridgeLogin: vi.fn(() => Promise.resolve()),
}));

const ACCOUNT_ID = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

function network(overrides: Partial<BridgeNetworkVm> = {}): BridgeNetworkVm {
  return {
    networkId: "matrix",
    name: "Matrix",
    glyph: "MX",
    tier: "low" satisfies RiskTier,
    tierLabel: "Low risk",
    badgeStyle: "secondary" satisfies BadgeStyle,
    requiresAck: false,
    ackCopy: null,
    ...overrides,
  };
}

const VOLATILE_ACK =
  "Connecting this network may violate its Terms of Service and risks account suspension or a ban. Expect login friction. Continue only if you accept that risk.";

const volatile = network({
  networkId: "instagram",
  name: "Instagram",
  glyph: "IG",
  tier: "volatile",
  tierLabel: "Volatile — opt-in",
  badgeStyle: "filledDisconnected",
  requiresAck: true,
  ackCopy: VOLATILE_ACK,
});

function renderCard(vm: BridgeNetworkVm, status: BridgeStatus = "configured") {
  return render(
    <TooltipProvider>
      <BridgeCard network={vm} accountId={ACCOUNT_ID} status={status} />
    </TooltipProvider>,
  );
}

describe("BridgeCard", () => {
  it("renders the network name, glyph, and its data-driven risk-tier badge", () => {
    renderCard(network());
    expect(screen.getByText("Matrix")).toBeInTheDocument();
    expect(screen.getByText("MX")).toBeInTheDocument();
    expect(screen.getByText("Low risk")).toBeInTheDocument();
  });

  it("renders the discovery status word for each status", () => {
    const { rerender } = renderCard(network(), "loggedIn");
    expect(screen.getByText("Connected")).toBeInTheDocument();

    rerender(
      <TooltipProvider>
        <BridgeCard network={network()} accountId={ACCOUNT_ID} status="notLoggedIn" />
      </TooltipProvider>,
    );
    expect(screen.getByText("Action needed")).toBeInTheDocument();

    rerender(
      <TooltipProvider>
        <BridgeCard network={network()} accountId={ACCOUNT_ID} status="configured" />
      </TooltipProvider>,
    );
    expect(screen.getByText("Not set up")).toBeInTheDocument();
  });

  it("renders the maintenance-heavy badge label from the data", () => {
    renderCard(
      network({
        networkId: "whatsapp",
        name: "WhatsApp",
        glyph: "WA",
        tier: "maintenance",
        tierLabel: "Maintenance-heavy",
        badgeStyle: "outlineDegraded",
      }),
    );
    expect(screen.getByText("Maintenance-heavy")).toBeInTheDocument();
  });

  it("a low-risk Connect proceeds with NO ack dialog and opens the login Sheet", async () => {
    renderCard(network());
    fireEvent.click(screen.getByRole("button", { name: "Connect Matrix" }));
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    // The native login Sheet opens directly (its title names the network).
    expect(await screen.findByRole("dialog", { name: /Connect Matrix/ })).toBeInTheDocument();
  });

  it("a volatile action opens the AlertDialog showing the ack copy and confirm label", async () => {
    renderCard(volatile);
    fireEvent.click(screen.getByRole("button", { name: "Set up Instagram" }));

    const dialog = await screen.findByRole("alertdialog");
    // The tier badge and the backend ack copy are both surfaced in the gate.
    expect(within(dialog).getByText("Volatile — opt-in")).toBeInTheDocument();
    expect(within(dialog).getByText(VOLATILE_ACK)).toBeInTheDocument();
    expect(
      within(dialog).getByRole("button", { name: "I understand the risk — connect" }),
    ).toBeInTheDocument();
  });

  it("confirming the volatile gate closes the ack dialog and opens the login Sheet", async () => {
    renderCard(volatile);
    fireEvent.click(screen.getByRole("button", { name: "Set up Instagram" }));

    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(
      within(dialog).getByRole("button", { name: "I understand the risk — connect" }),
    );

    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    });
    // Proceeding opens the native login Sheet for the connected network.
    expect(await screen.findByRole("dialog", { name: /Connect Instagram/ })).toBeInTheDocument();
  });

  it("cancelling the volatile gate aborts with no side effect (dialog closes)", async () => {
    renderCard(volatile);
    fireEvent.click(screen.getByRole("button", { name: "Set up Instagram" }));

    const dialog = await screen.findByRole("alertdialog");
    fireEvent.click(within(dialog).getByRole("button", { name: "Cancel" }));

    await waitFor(() => {
      expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();
    });
  });
});
