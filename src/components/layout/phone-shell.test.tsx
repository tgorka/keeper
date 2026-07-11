import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, InboxBatch, InboxRoomVm } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { archiveRoomsStore } from "@/lib/stores/archive-rooms";
import { composerStore } from "@/lib/stores/composer";
import { detailStore } from "@/lib/stores/detail-ui";
import { favoritesRoomsStore } from "@/lib/stores/favorites-rooms";
import { pinsRoomsStore } from "@/lib/stores/pins-rooms";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";

// Mock the typed IPC wrapper so the mounted panes never touch Tauri. The inbox
// subscription captures its `onInbox` handler so a test can stream rows; every
// other subscription resolves a stub id and never emits, and one-shot reads
// resolve benign empties — the stack under test only projects selection state.
const subscribeInbox = vi.fn();
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    subscribeInbox: (...handlers: unknown[]) => subscribeInbox(...handlers),
    unsubscribeInbox: vi.fn(async (): Promise<void> => {}),
    listDrafts: vi.fn(async (): Promise<Array<[string, string]>> => []),
    getFavoritesCollapsed: vi.fn(async (): Promise<boolean> => false),
    setFavoritesCollapsed: vi.fn(async (): Promise<void> => {}),
    subscribeDraftMirror: vi.fn(async (): Promise<number> => 1),
    unsubscribeDraftMirror: vi.fn(async (): Promise<void> => {}),
    subscribeTimeline: vi.fn(async (): Promise<number> => 1),
    unsubscribeTimeline: vi.fn(async (): Promise<void> => {}),
    subscribeTyping: vi.fn(async (): Promise<number> => 1),
    unsubscribeTyping: vi.fn(async (): Promise<void> => {}),
    subscribePaginationStatus: vi.fn(async (): Promise<number> => 1),
    unsubscribePaginationStatus: vi.fn(async (): Promise<void> => {}),
    subscribeOutbox: vi.fn(async (): Promise<number> => 1),
    unsubscribeOutbox: vi.fn(async (): Promise<void> => {}),
    markRoomRead: vi.fn(async (): Promise<void> => {}),
    releaseReceipt: vi.fn(async (): Promise<void> => {}),
    couplingCaveats: vi.fn(async () => []),
    incognitoGet: vi.fn(async () => ({
      effective: false,
      source: "global" as const,
      global: false,
      account: null,
      chat: null,
    })),
    loadDraft: vi.fn(async (): Promise<string | null> => null),
    saveDraft: vi.fn(async (): Promise<void> => {}),
    clearDraft: vi.fn(async (): Promise<void> => {}),
    loadRemoteDraft: vi.fn(async () => null),
    mirrorDraft: vi.fn(async (): Promise<void> => {}),
    clearDraftMirror: vi.fn(async (): Promise<void> => {}),
  };
});

// The conversation pane subscribes to native drag-drop via `getCurrentWebview()`.
// Mock it so the listener registers (and unregisters) without a real Tauri webview.
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({
    onDragDropEvent: vi.fn((_handler?: (e: unknown) => void) => Promise.resolve(() => {})),
  }),
}));

import { PhoneShell } from "@/components/layout/phone-shell";

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

/**
 * Mock matchMedia at a phone-tier width so any `max-width: <bp>` query matches
 * when the simulated viewport is below that breakpoint — the mounted
 * `ChatListPane` reads `useShellLayout().phone` for the composer-focus gate.
 */
const originalMatchMedia = window.matchMedia;
function mockViewportWidth(width: number) {
  window.matchMedia = vi.fn().mockImplementation((query: string) => {
    const match = query.match(/max-width:\s*(\d+)px/);
    const maxWidth = match ? Number(match[1]) : Number.POSITIVE_INFINITY;
    return {
      matches: width <= maxWidth,
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

/** Render the stack and stream a set of inbox rows through the captured emitter. */
async function renderWithRooms(rooms: Array<{ roomId: string; displayName: string }>) {
  const captured: { onInbox: ((b: InboxBatch) => void) | null } = { onInbox: null };
  subscribeInbox.mockImplementation((onInbox: (b: InboxBatch) => void) => {
    captured.onInbox = onInbox;
    return Promise.resolve(1);
  });
  accountsStore.getState().addAccount(account);
  render(<PhoneShell />);
  act(() => {
    captured.onInbox?.({
      ops: [{ op: "reset", rooms: rooms.map((r) => inboxRoom(r.roomId, r.displayName)) }],
      total: rooms.length,
    });
  });
  await waitFor(() => {
    expect(screen.getByText(rooms[0].displayName)).toBeInTheDocument();
  });
}

beforeEach(() => {
  mockViewportWidth(390);
  accountsStore.getState().clear();
  accountsStore.setState({ filterAccountId: null });
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  roomsStore.getState().clearFocus();
  archiveRoomsStore.getState().clear();
  pinsRoomsStore.getState().clear();
  favoritesRoomsStore.getState().clear();
  primaryViewStore.getState().setView("inbox");
  detailStore.setState({ open: false });
  composerStore.setState({ focusNonce: 0 });
  subscribeInbox.mockReset();
  subscribeInbox.mockResolvedValue(1);
});

afterEach(() => {
  window.matchMedia = originalMatchMedia;
  vi.restoreAllMocks();
});

describe("PhoneShell", () => {
  it("renders only the Inbox at level 0", () => {
    render(<PhoneShell />);
    // No account → the chat list sits in its loading state; the Room and Detail
    // levels are unmounted and no back control exists at the stack root.
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
    expect(screen.queryByRole("main")).not.toBeInTheDocument();
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Back" })).not.toBeInTheDocument();
  });

  it("pushes the Room level over the still-mounted Inbox when a room is selected", async () => {
    render(<PhoneShell />);
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    await waitFor(() => {
      expect(screen.getByRole("main")).toBeInTheDocument();
    });
    expect(screen.getByRole("button", { name: "Back" })).toBeInTheDocument();
    // Level 0 stays mounted underneath the opaque Room overlay.
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
  });

  it("pushes the Detail level over the Room when detail opens", async () => {
    render(<PhoneShell />);
    act(() => {
      roomsStore.getState().selectRoom(selection);
      detailStore.getState().openDetail();
    });
    await waitFor(() => {
      expect(screen.getByRole("complementary", { name: "Details" })).toBeInTheDocument();
    });
    // The Room and Inbox levels stay mounted underneath; exactly one back
    // control (the topmost level's) is exposed to assistive tech.
    expect(screen.getByRole("main")).toBeInTheDocument();
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Back" })).toHaveLength(1);
  });

  it("back pops exactly one level: Detail -> Room -> Inbox", async () => {
    render(<PhoneShell />);
    act(() => {
      roomsStore.getState().selectRoom(selection);
      detailStore.getState().openDetail();
    });
    await waitFor(() => {
      expect(screen.getByRole("complementary", { name: "Details" })).toBeInTheDocument();
    });

    // Level 2 -> 1: detail closes, the Room stays open, selection preserved.
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
    expect(screen.getByRole("main")).toBeInTheDocument();
    expect(detailStore.getState().open).toBe(false);
    expect(roomsStore.getState().selected).toEqual(selection);

    // Level 1 -> 0: selection clears, only the Inbox remains.
    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(roomsStore.getState().selected).toBeNull();
    await waitFor(() => {
      expect(screen.queryByRole("main")).not.toBeInTheDocument();
    });
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Back" })).not.toBeInTheDocument();
  });

  it("keeps the Inbox list mounted (same node) across a push and back", async () => {
    await renderWithRooms([
      { roomId: "!a:example.org", displayName: "Alpha" },
      { roomId: "!b:example.org", displayName: "Beta" },
    ]);
    const list = screen.getByRole("list", { name: "Conversations" });

    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    await waitFor(() => {
      expect(screen.getByRole("main")).toBeInTheDocument();
    });
    // The exact same DOM node survives the push — no unmount, so the Inbox
    // scroll offset is preserved.
    expect(screen.getByRole("list", { name: "Conversations" })).toBe(list);

    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    await waitFor(() => {
      expect(screen.queryByRole("main")).not.toBeInTheDocument();
    });
    expect(screen.getByRole("list", { name: "Conversations" })).toBe(list);
  });

  it("deep-links via requestFocus to the Room level with back returning to the Inbox", async () => {
    render(<PhoneShell />);
    act(() => {
      roomsStore.getState().requestFocus({
        accountId: account.accountId,
        roomId: "!a:example.org",
        eventId: "$deep:example.org",
      });
    });
    await waitFor(() => {
      expect(screen.getByRole("main")).toBeInTheDocument();
    });
    expect(roomsStore.getState().selected).toEqual(selection);

    fireEvent.click(screen.getByRole("button", { name: "Back" }));
    expect(roomsStore.getState().selected).toBeNull();
    await waitFor(() => {
      expect(screen.queryByRole("main")).not.toBeInTheDocument();
    });
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
  });

  it("does not bump the composer focusNonce when a chat opens on the phone", async () => {
    await renderWithRooms([{ roomId: "!a:example.org", displayName: "Alpha" }]);
    const container = screen.getByLabelText("Conversations");

    // Row-open via the keyboard path (the one that focuses the composer on
    // desktop): the phone tier must not steal composer focus (UX-DR22).
    fireEvent.keyDown(container, { key: "ArrowDown" });
    fireEvent.keyDown(container, { key: "Enter" });

    expect(roomsStore.getState().selected).toEqual(selection);
    await waitFor(() => {
      expect(screen.getByRole("main")).toBeInTheDocument();
    });
    expect(composerStore.getState().focusNonce).toBe(0);
  });
});
