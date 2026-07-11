import { fireEvent, render, screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, InboxRoomVm } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { detailStore } from "@/lib/stores/detail-ui";
import { exportStore } from "@/lib/stores/export";
import { incognitoStore } from "@/lib/stores/incognito";
import { roomsStore } from "@/lib/stores/rooms";

// Mock the typed IPC wrapper so the header's reused chat sub-parts (the
// incognito chip's coupling-caveats fetch) never touch Tauri. Everything else
// the header renders reads plain frontend stores.
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    couplingCaveats: vi.fn(async () => []),
    incognitoSetChat: vi.fn(async (): Promise<void> => {}),
    incognitoGet: vi.fn(async () => ({
      effective: false,
      source: "global" as const,
      global: false,
      account: null,
      chat: null,
    })),
  };
});

import { PhoneHeader } from "@/components/layout/phone-header";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 0,
  provider: "password",
};

const selection = { accountId: account.accountId, roomId: "!a:example.org" };

function inboxRoom(roomId: string, displayName: string): InboxRoomVm {
  return {
    accountId: account.accountId,
    hueIndex: 0,
    roomId,
    displayName,
    lastMessage: "",
    timestamp: null,
    avatarUrl: null,
    isUnread: false,
    mentionCount: 0,
    isArchived: false,
    isPinned: false,
    isFavourite: false,
    network: null,
    networkId: null,
    muteState: "none",
  };
}

/** Stream a room into the inbox window and select it. */
function selectRoom(displayName = "Alpha") {
  roomsStore.getState().applyBatch({
    ops: [{ op: "reset", rooms: [inboxRoom(selection.roomId, displayName)] }],
    total: 1,
  });
  roomsStore.getState().selectRoom(selection);
}

/** Open the ⋯ overflow menu (Radix opens on pointer-down, not click, in jsdom). */
async function openOverflowMenu() {
  const trigger = screen.getByRole("button", { name: "More" });
  fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
  fireEvent.pointerUp(trigger, { button: 0 });
  return await screen.findByRole("menu");
}

beforeEach(() => {
  accountsStore.getState().clear();
  accountsStore.setState({ filterAccountId: null });
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  incognitoStore.getState().clear();
  detailStore.setState({ open: false });
  exportStore.setState({
    isOpen: false,
    preset: { scope: "everything", accountId: null, roomId: null },
    job: null,
  });
  accountsStore.getState().addAccount(account);
});

describe("PhoneHeader", () => {
  it("renders the Room-level back chevron titled Inbox and pops on tap", () => {
    selectRoom();
    const onBack = vi.fn();
    render(<PhoneHeader level={1} onBack={onBack} />);

    const back = screen.getByRole("button", { name: "Back to Inbox" });
    expect(back).toHaveTextContent("Inbox");
    fireEvent.click(back);
    expect(onBack).toHaveBeenCalledTimes(1);
  });

  it("carries the room's display name on the Detail-level back chevron", () => {
    selectRoom("Alpha");
    render(<PhoneHeader level={2} onBack={vi.fn()} />);

    const back = screen.getByRole("button", { name: "Back to Alpha" });
    expect(back).toHaveTextContent("Alpha");
    // Level 2 carries no identity block or overflow menu.
    expect(screen.queryByRole("button", { name: "Open details" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "More" })).not.toBeInTheDocument();
  });

  it("degrades the Detail-level back name to a generic Back for an unknown room", () => {
    // Selection points at a room absent from every streamed window.
    roomsStore.getState().selectRoom(selection);
    render(<PhoneHeader level={2} onBack={vi.fn()} />);
    expect(screen.getByRole("button", { name: "Back" })).toHaveTextContent("Back");
  });

  it("pushes Detail when the identity block is tapped (the phone's ⌘I)", () => {
    selectRoom("Alpha");
    render(<PhoneHeader level={1} onBack={vi.fn()} />);

    const identity = screen.getByRole("button", { name: "Open details" });
    // The reused identity block renders the room's display name inside it.
    expect(within(identity).getByText("Alpha")).toBeInTheDocument();
    fireEvent.click(identity);
    expect(detailStore.getState().open).toBe(true);
  });

  it("opens the Export dialog preset to the open chat from the overflow menu", async () => {
    selectRoom();
    render(<PhoneHeader level={1} onBack={vi.fn()} />);

    const menu = await openOverflowMenu();
    // Export is the only entry today; Search/Mute/Mention-only/Archive land
    // with their owning stories.
    expect(within(menu).getAllByRole("menuitem")).toHaveLength(1);
    fireEvent.click(within(menu).getByRole("menuitem", { name: "Export" }));

    expect(exportStore.getState().isOpen).toBe(true);
    expect(exportStore.getState().preset).toEqual({
      scope: "chat",
      accountId: selection.accountId,
      roomId: selection.roomId,
    });
  });

  it("shows the incognito chip only when incognito is effective for the chat", () => {
    selectRoom();
    const { unmount } = render(<PhoneHeader level={1} onBack={vi.fn()} />);
    // No effective incognito: the violet chip is absent.
    expect(
      screen.queryByRole("button", { name: "Incognito — this chat overrides account" }),
    ).not.toBeInTheDocument();
    unmount();

    // Mirror an effective per-chat VM, exactly as the Rust core would resolve it.
    incognitoStore.getState().applyVm(selection.accountId, selection.roomId, {
      effective: true,
      source: "chat",
      global: false,
      account: null,
      chat: true,
    });
    render(<PhoneHeader level={1} onBack={vi.fn()} />);
    expect(
      screen.getByRole("button", { name: "Incognito — this chat overrides account" }),
    ).toBeInTheDocument();
  });
});
