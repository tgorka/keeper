import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { BridgeCard } from "@/components/bridges/bridge-card";
import { TooltipProvider } from "@/components/ui/tooltip";
import type { BadgeStyle, BridgeNetworkVm, BridgeStatus, RiskTier } from "@/lib/ipc/client";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";

// The login Sheet opened on proceed calls the streaming IPC client on mount; stub
// it so the card tests never touch a real Tauri channel. A never-resolving start
// keeps the Sheet in its initial waiting state. `bridgeBotRoom` is mocked so the
// Manage → Open Bridge Bot chat action resolves a room id without a real Tauri call.
const bridgeBotRoomMock = vi.fn((_accountId: string, _networkId: string) =>
  Promise.resolve("!bot:example.org"),
);
vi.mock("@/lib/ipc/client", () => ({
  startBridgeLogin: vi.fn(() => new Promise<number>(() => {})),
  submitBridgeLogin: vi.fn(() => Promise.resolve()),
  cancelBridgeLogin: vi.fn(() => Promise.resolve()),
  bridgeBotRoom: (...args: [string, string]) => bridgeBotRoomMock(...args),
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
  beforeEach(() => {
    bridgeBotRoomMock.mockClear();
    // Reset the shared navigation stores so each assertion starts from a known state.
    primaryViewStore.getState().setView("bridges");
    roomsStore.getState().selectRoom(null);
  });

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

  it("Manage → Open Bridge Bot chat resolves the room and navigates to it", async () => {
    renderCard(network());
    // Radix DropdownMenu opens on pointer-down (not `click`) in jsdom.
    const trigger = screen.getByRole("button", { name: "Manage Matrix" });
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    fireEvent.pointerUp(trigger, { button: 0 });

    const menu = await screen.findByRole("menu");
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Open Bridge Bot chat" }));

    // The escape hatch resolves the bot DM for this account × network …
    await waitFor(() => {
      expect(bridgeBotRoomMock).toHaveBeenCalledWith(ACCOUNT_ID, "matrix");
    });
    // … then navigates: primary view → Inbox and the resolved room selected.
    await waitFor(() => {
      expect(primaryViewStore.getState().view).toBe("inbox");
      expect(roomsStore.getState().selected).toEqual({
        accountId: ACCOUNT_ID,
        roomId: "!bot:example.org",
      });
    });
  });
});
