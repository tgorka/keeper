import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NewChatDialog } from "@/components/chat/new-chat-dialog";
import type { AccountVm, BridgeNetworkVm, IpcError, ResolveSupportVm } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { composerStore } from "@/lib/stores/composer";
import { newChatStore } from "@/lib/stores/new-chat";
import { roomsStore } from "@/lib/stores/rooms";

// The dialog resolves through two client wrappers; mock them so tests drive the
// resolve/support outcomes without a real Tauri call.
const bridgeResolveSupport = vi.fn<(networkId: string) => Promise<ResolveSupportVm>>();
const resolveBridgeIdentifier =
  vi.fn<
    (accountId: string, networkId: string, identifier: string) => Promise<{ roomId: string }>
  >();
vi.mock("@/lib/ipc/client", () => ({
  bridgeResolveSupport: (networkId: string) => bridgeResolveSupport(networkId),
  resolveBridgeIdentifier: (accountId: string, networkId: string, identifier: string) =>
    resolveBridgeIdentifier(accountId, networkId, identifier),
}));

// The Network picker reads the bridge catalog; mock the hook to a fixed set.
const CATALOG: BridgeNetworkVm[] = [
  {
    networkId: "whatsapp",
    name: "WhatsApp",
    glyph: "WA",
    tier: "maintenance",
    tierLabel: "Maintenance-heavy",
    badgeStyle: "outlineDegraded",
    requiresAck: false,
    ackCopy: null,
  },
  {
    networkId: "slack",
    name: "Slack",
    glyph: "SL",
    tier: "maintenance",
    tierLabel: "Maintenance-heavy",
    badgeStyle: "outlineDegraded",
    requiresAck: false,
    ackCopy: null,
  },
];
vi.mock("@/hooks/use-bridge-catalog", () => ({
  useBridgeCatalog: () => ({ catalog: CATALOG, loading: false, error: null }),
}));

const ACCOUNT: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://example.org",
  hueIndex: 0,
  provider: "password",
};

function supported(networkId: string): ResolveSupportVm {
  return {
    networkId,
    supported: true,
    identifierHint: "Phone number or username",
    placeholder: "+1 555 123 4567",
  };
}

beforeEach(() => {
  bridgeResolveSupport.mockReset();
  resolveBridgeIdentifier.mockReset();
  bridgeResolveSupport.mockImplementation((networkId) => Promise.resolve(supported(networkId)));
  accountsStore.getState().hydrateAll([ACCOUNT]);
  roomsStore.getState().selectRoom(null);
  composerStore.setState({ focusNonce: 0 });
  newChatStore.setState({ isOpen: true, lastAccountId: null, lastNetworkId: null });
});

afterEach(() => {
  newChatStore.getState().close();
});

describe("NewChatDialog", () => {
  it("resolves an identifier, opens the chat, focuses the composer, and closes", async () => {
    resolveBridgeIdentifier.mockResolvedValue({ roomId: "!portal:example.org" });
    render(<NewChatDialog />);

    const input = await screen.findByLabelText("Identifier");
    fireEvent.change(input, { target: { value: " +15551234567 " } });
    fireEvent.click(screen.getByRole("button", { name: "Start chat" }));

    await waitFor(() => {
      // The trimmed identifier is resolved through the bridge.
      expect(resolveBridgeIdentifier).toHaveBeenCalledWith(
        ACCOUNT.accountId,
        "whatsapp",
        "+15551234567",
      );
    });
    await waitFor(() => {
      // Success opens the resolved room and closes the dialog.
      expect(roomsStore.getState().selected).toEqual({
        accountId: ACCOUNT.accountId,
        roomId: "!portal:example.org",
      });
    });
    expect(composerStore.getState().focusNonce).toBeGreaterThan(0);
    expect(newChatStore.getState().isOpen).toBe(false);
  });

  it("shows inline 'Not found' and retains the input when resolution fails", async () => {
    const err: IpcError = {
      code: "syncUnavailable",
      message: "no such user",
      accountId: null,
      retriable: true,
    };
    resolveBridgeIdentifier.mockRejectedValue(err);
    render(<NewChatDialog />);

    const input = await screen.findByLabelText<HTMLInputElement>("Identifier");
    fireEvent.change(input, { target: { value: "@nobody" } });
    fireEvent.click(screen.getByRole("button", { name: "Start chat" }));

    // Inline not-found copy appears; the dialog stays open and the input is retained.
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "Not found on WhatsApp — check the number or username.",
    );
    expect(newChatStore.getState().isOpen).toBe(true);
    expect(input.value).toBe("@nobody");
    expect(roomsStore.getState().selected).toBeNull();
  });

  it("disables the input and shows an upfront gate for an unsupported network", async () => {
    bridgeResolveSupport.mockImplementation((networkId) =>
      Promise.resolve({
        networkId,
        supported: false,
        identifierHint: "Starting new chats isn't supported on Slack from keeper",
        placeholder: "",
      }),
    );
    // Default the picker to the unsupported network so the gate is shown on open.
    newChatStore.setState({ isOpen: true, lastNetworkId: "slack" });
    render(<NewChatDialog />);

    // The "not supported" copy is shown upfront; no identifier field is rendered and
    // no resolve command is issued.
    expect(await screen.findByText(/isn't supported on Slack/i)).toBeInTheDocument();
    expect(screen.queryByLabelText("Identifier")).not.toBeInTheDocument();
    expect(resolveBridgeIdentifier).not.toHaveBeenCalled();
    // The Start button is disabled for the unsupported network.
    expect(screen.getByRole("button", { name: "Start chat" })).toBeDisabled();
  });

  it("fails closed (no resolve, Start disabled) when the capability read errors", async () => {
    // A capability-read failure must NOT open the gate — an unsupported network could
    // otherwise be resolved through. The dialog shows the manual-escape message and
    // never renders the identifier field or issues a resolve.
    bridgeResolveSupport.mockRejectedValue(new Error("ipc hiccup"));
    render(<NewChatDialog />);

    expect(await screen.findByText(/couldn't check whether this network/i)).toBeInTheDocument();
    expect(screen.queryByLabelText("Identifier")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Start chat" })).toBeDisabled();
    expect(resolveBridgeIdentifier).not.toHaveBeenCalled();
  });

  it("keeps the Start button disabled for an empty / whitespace identifier", async () => {
    render(<NewChatDialog />);
    const input = await screen.findByLabelText("Identifier");

    expect(screen.getByRole("button", { name: "Start chat" })).toBeDisabled();
    fireEvent.change(input, { target: { value: "   " } });
    expect(screen.getByRole("button", { name: "Start chat" })).toBeDisabled();
    expect(resolveBridgeIdentifier).not.toHaveBeenCalled();
  });
});
