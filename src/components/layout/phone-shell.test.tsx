import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, InboxBatch, InboxRoomVm } from "@/lib/ipc/client";
import { accountStatusStore } from "@/lib/stores/account-status";
import { accountsStore } from "@/lib/stores/accounts";
import { archiveRoomsStore } from "@/lib/stores/archive-rooms";
import { composerStore } from "@/lib/stores/composer";
import { detailStore } from "@/lib/stores/detail-ui";
import { favoritesRoomsStore } from "@/lib/stores/favorites-rooms";
import { leadingDrawerStore } from "@/lib/stores/leading-drawer";
import { pinsRoomsStore } from "@/lib/stores/pins-rooms";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";
import { searchSurfaceStore } from "@/lib/stores/search-surface";

// Mock the typed IPC wrapper so the mounted panes never touch Tauri. The inbox
// subscription captures its `onInbox` handler so a test can stream rows; every
// other subscription resolves a stub id and never emits, and one-shot reads
// resolve benign empties — the stack under test only projects selection state.
const subscribeInbox = vi.fn();
const syncNow = vi.fn(async (): Promise<void> => {});
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
    // The leading drawer mounts SidebarPane → SettingsDialog, which reads the
    // encryption posture on open. Stub it so opening the drawer never hits Tauri.
    encryptionPosture: vi.fn(() => Promise.resolve(false)),
    // The always-mounted PhoneSearchSurface queries these when opened; stub them
    // so a pull-down/magnifier open in the stack tests never reaches Tauri.
    paletteQuery: vi.fn(async () => ({ contacts: [], chats: [], actions: [] })),
    searchArchive: vi.fn(async () => []),
    // Pull-to-refresh (Story 13.6): the sync-loop kick is a spy.
    syncNow: () => syncNow(),
  };
});

// SidebarPane's account footer renders `useSignOut` (imports the IPC client);
// mock the hook so opening the drawer never reaches Tauri.
vi.mock("@/hooks/use-sign-out", () => ({
  useSignOut: () => vi.fn(),
}));

// The stale-resume "Connecting…" pill state (Story 14.4) is driven by a
// visibility/status hook with its own unit suite; here the shell's RENDERING of
// that state is under test, so the hook is a settable stub.
const staleResumePill = { connecting: false };
vi.mock("@/hooks/use-stale-resume-pill", () => ({
  useStaleResumePill: () => staleResumePill.connecting,
}));

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
 * when the simulated viewport is below that breakpoint, and drive the
 * `(prefers-reduced-motion: reduce)` query for `useReducedMotion` (Story 13.2).
 * Tests default to reduced motion so pops unmount synchronously (jsdom never
 * fires `transitionend`); motion-specific tests pass `reducedMotion: false`
 * and end transitions by hand.
 */
const originalMatchMedia = window.matchMedia;
function mockViewportWidth(width: number, { reducedMotion = true } = {}) {
  window.matchMedia = vi.fn().mockImplementation((query: string) => {
    const match = query.match(/max-width:\s*(\d+)px/);
    const maxWidth = match ? Number(match[1]) : Number.POSITIVE_INFINITY;
    const matches = query.includes("prefers-reduced-motion") ? reducedMotion : width <= maxWidth;
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

/** Render the stack and stream a set of inbox rows through the captured emitter. */
async function renderWithRooms(rooms: Array<{ roomId: string; displayName: string }>) {
  const captured: { onInbox: ((b: InboxBatch) => void) | null } = { onInbox: null };
  subscribeInbox.mockImplementation((onInbox: (b: InboxBatch) => void) => {
    captured.onInbox = onInbox;
    return Promise.resolve(1);
  });
  accountsStore.getState().addAccount(account);
  const view = render(<PhoneShell />);
  act(() => {
    captured.onInbox?.({
      ops: [{ op: "reset", rooms: rooms.map((r) => inboxRoom(r.roomId, r.displayName)) }],
      total: rooms.length,
    });
  });
  await waitFor(() => {
    expect(screen.getByText(rooms[0].displayName)).toBeInTheDocument();
  });
  return view;
}

/** The stack-level wrapper for the given level (presence + transform target). */
function stackLevel(level: 0 | 1 | 2): HTMLElement {
  const node = document.querySelector<HTMLElement>(`[data-level="${level}"]`);
  if (node === null) {
    throw new Error(`stack level ${level} is not mounted`);
  }
  return node;
}

/** Mock every rect at the given width so the edge-swipe reads a real drag range. */
function mockRectWidth(width: number) {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    width,
    height: 700,
    top: 0,
    left: 0,
    right: width,
    bottom: 700,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  } as DOMRect);
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
  leadingDrawerStore.getState().close();
  searchSurfaceStore.setState({ isOpen: false, scope: "chats", chatLock: null });
  composerStore.setState({ focusNonce: 0 });
  accountStatusStore.getState().reset();
  subscribeInbox.mockReset();
  subscribeInbox.mockResolvedValue(1);
  syncNow.mockReset();
  syncNow.mockResolvedValue(undefined);
  staleResumePill.connecting = false;
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
    expect(screen.queryByRole("button", { name: /^Back/ })).not.toBeInTheDocument();
  });

  it("pushes the Room level over the still-mounted Inbox when a room is selected", async () => {
    render(<PhoneShell />);
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    await waitFor(() => {
      expect(screen.getByRole("main")).toBeInTheDocument();
    });
    expect(screen.getByRole("button", { name: "Back to Inbox" })).toBeInTheDocument();
    // Level 0 stays mounted underneath the opaque Room overlay.
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
  });

  it("pushes the Detail level over the Room when detail opens", async () => {
    render(<PhoneShell />);
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    // Open Detail in a separate commit: the DW-109 effect closes Detail on any
    // selection change, so a same-batch select+open would (correctly) land on
    // the Room level.
    act(() => {
      detailStore.getState().openDetail();
    });
    await waitFor(() => {
      expect(screen.getByRole("complementary", { name: "Details" })).toBeInTheDocument();
    });
    // The Room and Inbox levels stay mounted underneath. With no streamed room
    // VM the Detail header's back name degrades to a generic "Back".
    expect(screen.getByRole("main")).toBeInTheDocument();
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Back" })).toBeInTheDocument();
  });

  it("back pops exactly one level: Detail -> Room -> Inbox", async () => {
    render(<PhoneShell />);
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    // Open Detail in a separate commit: the DW-109 effect closes Detail on any
    // selection change, so a same-batch select+open would (correctly) land on
    // the Room level.
    act(() => {
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
    fireEvent.click(screen.getByRole("button", { name: "Back to Inbox" }));
    expect(roomsStore.getState().selected).toBeNull();
    await waitFor(() => {
      expect(screen.queryByRole("main")).not.toBeInTheDocument();
    });
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^Back/ })).not.toBeInTheDocument();
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

    fireEvent.click(screen.getByRole("button", { name: "Back to Inbox" }));
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

    fireEvent.click(screen.getByRole("button", { name: "Back to Inbox" }));
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

  it("renders exactly one header bar at the Room level (UX-DR21)", async () => {
    await renderWithRooms([{ roomId: "!a:example.org", displayName: "Alpha" }]);
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    await waitFor(() => {
      expect(screen.getByRole("main")).toBeInTheDocument();
    });

    // One PhoneHeader-owned bar inside the active Room level: back "Inbox" +
    // identity → Detail + ⋯ overflow. (Level 0's own Inbox header stays mounted
    // but inert underneath — a different level's bar, not a second Room bar.)
    const roomLevel = stackLevel(1);
    expect(within(roomLevel).getAllByRole("banner")).toHaveLength(1);
    const header = within(roomLevel).getByRole("banner");
    expect(within(header).getByRole("button", { name: "Back to Inbox" })).toBeInTheDocument();
    expect(within(header).getByRole("button", { name: "Open details" })).toBeInTheDocument();
    expect(within(header).getByRole("button", { name: "More" })).toBeInTheDocument();
    // ConversationPane's own header row is suppressed (showHeader={false}): its
    // desktop-only controls must not exist anywhere in the stack.
    expect(screen.queryByRole("button", { name: "Toggle detail panel" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Export this chat" })).not.toBeInTheDocument();
  });

  it("pushes Detail when the header identity block is tapped", async () => {
    await renderWithRooms([{ roomId: "!a:example.org", displayName: "Alpha" }]);
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    await waitFor(() => {
      expect(screen.getByRole("main")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Open details" }));
    await waitFor(() => {
      expect(screen.getByRole("complementary", { name: "Details" })).toBeInTheDocument();
    });
    // The Detail header's back chevron carries the room's display name.
    expect(screen.getByRole("button", { name: "Back to Alpha" })).toBeInTheDocument();
  });

  it("slides a push in with the level beneath shifted back 25% when motion is allowed", async () => {
    mockViewportWidth(390, { reducedMotion: false });
    render(<PhoneShell />);
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    await waitFor(() => {
      expect(screen.getByRole("main")).toBeInTheDocument();
    });

    const room = stackLevel(1);
    expect(room.className).toContain("transition-transform");
    expect(room.className).toContain("duration-[250ms]");
    expect(room.style.transform).toBe("translateX(0)");
    // The covered Inbox shifts back and is dimmed + inert underneath.
    const inbox = stackLevel(0);
    expect(inbox.style.transform).toBe("translateX(-25%)");
    expect(inbox.className).toContain("brightness-95");
  });

  it("renders pushes as instant cuts (duration-0) under prefers-reduced-motion", async () => {
    render(<PhoneShell />);
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    await waitFor(() => {
      expect(screen.getByRole("main")).toBeInTheDocument();
    });
    expect(stackLevel(1).className).toContain("duration-0");
    expect(stackLevel(1).style.transform).toBe("translateX(0)");
  });

  it("keeps a popped level mounted until its slide-out transition ends", async () => {
    mockViewportWidth(390, { reducedMotion: false });
    render(<PhoneShell />);
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    await waitFor(() => {
      expect(screen.getByRole("main")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Back to Inbox" }));
    expect(roomsStore.getState().selected).toBeNull();
    // Presence: the Room level stays mounted at the trailing edge while the
    // pop transition runs…
    const room = stackLevel(1);
    expect(screen.getByRole("main")).toBeInTheDocument();
    expect(room.style.transform).toBe("translateX(100%)");
    // …and unmounts when its own transform transition completes.
    fireEvent.transitionEnd(room, { propertyName: "transform" });
    expect(screen.queryByRole("main")).not.toBeInTheDocument();
  });

  it("moves focus to the new back button on push and restores the pusher on pop", async () => {
    await renderWithRooms([{ roomId: "!a:example.org", displayName: "Alpha" }]);
    const row = screen.getByRole("button", { name: "Conversation with Alpha" });

    // Push 0 -> 1 from the focused row: focus lands on the Room back button.
    act(() => {
      row.focus();
    });
    fireEvent.click(row);
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Back to Inbox" })).toHaveFocus();
    });

    // Push 1 -> 2 from the identity block: focus lands on the Detail back button.
    const identity = screen.getByRole("button", { name: "Open details" });
    act(() => {
      identity.focus();
    });
    fireEvent.click(identity);
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Back to Alpha" })).toHaveFocus();
    });

    // Pop 2 -> 1 restores the element that pushed Detail…
    fireEvent.click(screen.getByRole("button", { name: "Back to Alpha" }));
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Open details" })).toHaveFocus();
    });

    // …and pop 1 -> 0 restores the Inbox row that pushed the Room.
    fireEvent.click(screen.getByRole("button", { name: "Back to Inbox" }));
    await waitFor(() => {
      expect(row).toHaveFocus();
    });
  });

  it("pops one level per Escape press", async () => {
    await renderWithRooms([{ roomId: "!a:example.org", displayName: "Alpha" }]);
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    // Open Detail in a separate commit: the DW-109 effect closes Detail on any
    // selection change, so a same-batch select+open would (correctly) land on
    // the Room level.
    act(() => {
      detailStore.getState().openDetail();
    });
    await waitFor(() => {
      expect(screen.getByRole("complementary", { name: "Details" })).toBeInTheDocument();
    });

    fireEvent.keyDown(screen.getByRole("complementary", { name: "Details" }), { key: "Escape" });
    expect(detailStore.getState().open).toBe(false);
    expect(roomsStore.getState().selected).toEqual(selection);

    fireEvent.keyDown(screen.getByRole("button", { name: "Back to Inbox" }), { key: "Escape" });
    expect(roomsStore.getState().selected).toBeNull();
  });

  it("marks covered levels inert while a higher level is on top", async () => {
    await renderWithRooms([{ roomId: "!a:example.org", displayName: "Alpha" }]);
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    // Open Detail in a separate commit: the DW-109 effect closes Detail on any
    // selection change, so a same-batch select+open would (correctly) land on
    // the Room level.
    act(() => {
      detailStore.getState().openDetail();
    });
    await waitFor(() => {
      expect(screen.getByRole("complementary", { name: "Details" })).toBeInTheDocument();
    });

    expect(stackLevel(0)).toHaveAttribute("inert");
    expect(stackLevel(1)).toHaveAttribute("inert");
    expect(stackLevel(2)).not.toHaveAttribute("inert");

    fireEvent.click(screen.getByRole("button", { name: "Back to Alpha" }));
    expect(stackLevel(0)).toHaveAttribute("inert");
    expect(stackLevel(1)).not.toHaveAttribute("inert");
  });

  it("commits back when an edge-swipe crosses half the width", async () => {
    mockRectWidth(390);
    render(<PhoneShell />);
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    await waitFor(() => {
      expect(screen.getByRole("main")).toBeInTheDocument();
    });

    const zone = screen.getByTestId("edge-swipe-back");
    fireEvent.pointerDown(zone, { pointerId: 1, clientX: 5 });
    fireEvent.pointerMove(zone, { pointerId: 1, clientX: 250 });
    // Mid-drag the active level tracks the finger and the covered level returns
    // proportionally toward 0.
    expect(stackLevel(1).style.transform).toBe("translateX(245px)");
    expect(stackLevel(0).style.transform).not.toBe("translateX(-25%)");
    fireEvent.pointerUp(zone, { pointerId: 1, clientX: 250 });

    expect(roomsStore.getState().selected).toBeNull();
    await waitFor(() => {
      expect(screen.queryByRole("main")).not.toBeInTheDocument();
    });
  });

  it("snaps back without popping when the edge-swipe releases below the threshold", async () => {
    mockRectWidth(390);
    render(<PhoneShell />);
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    await waitFor(() => {
      expect(screen.getByRole("main")).toBeInTheDocument();
    });

    const zone = screen.getByTestId("edge-swipe-back");
    fireEvent.pointerDown(zone, { pointerId: 1, clientX: 5 });
    fireEvent.pointerMove(zone, { pointerId: 1, clientX: 30 });
    fireEvent.pointerUp(zone, { pointerId: 1, clientX: 30 });

    // Below half width and below the flick minimum: no pop, the level snaps to 0.
    expect(roomsStore.getState().selected).toEqual(selection);
    expect(screen.getByRole("main")).toBeInTheDocument();
    expect(stackLevel(1).style.transform).toBe("translateX(0)");
  });

  it("commits back on a fast flick even below half the width", async () => {
    mockRectWidth(390);
    render(<PhoneShell />);
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    await waitFor(() => {
      expect(screen.getByRole("main")).toBeInTheDocument();
    });

    const zone = screen.getByTestId("edge-swipe-back");
    // ~95px in the few ms between synchronously-fired events: a flick.
    fireEvent.pointerDown(zone, { pointerId: 1, clientX: 5 });
    fireEvent.pointerMove(zone, { pointerId: 1, clientX: 100 });
    fireEvent.pointerUp(zone, { pointerId: 1, clientX: 100 });

    expect(roomsStore.getState().selected).toBeNull();
  });

  it("pops from the Detail level too when the edge-swipe commits", async () => {
    mockRectWidth(390);
    render(<PhoneShell />);
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    // Open Detail in a separate commit: the DW-109 effect closes Detail on any
    // selection change, so a same-batch select+open would (correctly) land on
    // the Room level.
    act(() => {
      detailStore.getState().openDetail();
    });
    await waitFor(() => {
      expect(screen.getByRole("complementary", { name: "Details" })).toBeInTheDocument();
    });

    const zone = screen.getByTestId("edge-swipe-back");
    fireEvent.pointerDown(zone, { pointerId: 1, clientX: 5 });
    fireEvent.pointerMove(zone, { pointerId: 1, clientX: 250 });
    fireEvent.pointerUp(zone, { pointerId: 1, clientX: 250 });

    // Exactly one level pops: Detail closes, the Room survives.
    expect(detailStore.getState().open).toBe(false);
    expect(roomsStore.getState().selected).toEqual(selection);
  });

  it("reserves level 0's leading edge (no back gesture) for the drawer", () => {
    render(<PhoneShell />);
    // The edge-swipe-BACK hit zone exists only on the active overlay at level >= 1;
    // level 0's leading edge carries the drawer-OPEN zone instead.
    expect(screen.queryByTestId("edge-swipe-back")).not.toBeInTheDocument();
    expect(screen.getByTestId("edge-swipe-open")).toBeInTheDocument();
  });

  it("shows exactly one Inbox header with the status cluster and no bottom tab bar at level 0", () => {
    render(<PhoneShell />);
    // Exactly one header bar (the Inbox header) at level 0.
    expect(screen.getAllByRole("banner")).toHaveLength(1);
    const header = screen.getByRole("banner");
    expect(within(header).getByRole("button", { name: "Open navigation" })).toBeInTheDocument();
    expect(within(header).getByRole("button", { name: "Search" })).toBeInTheDocument();
    expect(within(header).getByRole("button", { name: "New chat" })).toBeInTheDocument();
    // No bottom tab bar anywhere.
    expect(screen.queryByRole("tablist")).not.toBeInTheDocument();
    // The chat list renders below the header (its loading state with no account).
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
  });

  it("opens the leading drawer from the Inbox header avatar and renders the reused sidebar", async () => {
    render(<PhoneShell />);
    fireEvent.click(screen.getByRole("button", { name: "Open navigation" }));
    // The reused SidebarPane nav mounts inside the drawer.
    const nav = await screen.findByRole("navigation", { name: "Views" });
    expect(within(nav).getByRole("button", { name: /^Chats/ })).toBeInTheDocument();
    expect(leadingDrawerStore.getState().isOpen).toBe(true);
  });

  it("keeps the drawer-open swipe zone below the header so it never shadows the avatar button", () => {
    render(<PhoneShell />);
    const zone = screen.getByTestId("edge-swipe-open");
    // The zone must start below the header — safe-top + 52px since Story 13.5,
    // never `inset-y-0` — so a tap on the avatar drawer button's leading edge
    // activates the button, not a below-threshold swipe.
    expect(zone.className).toContain("top-[calc(var(--safe-top)+var(--phone-header))]");
    expect(zone.className).not.toContain("inset-y-0");
  });

  it("opens the drawer via a level-0 leading-edge swipe past the threshold", () => {
    mockRectWidth(390);
    render(<PhoneShell />);
    const zone = screen.getByTestId("edge-swipe-open");
    fireEvent.pointerDown(zone, { pointerId: 1, clientX: 5 });
    fireEvent.pointerUp(zone, { pointerId: 1, clientX: 250 });
    expect(leadingDrawerStore.getState().isOpen).toBe(true);
  });

  it("does not open the drawer when the level-0 edge swipe releases below the threshold", () => {
    mockRectWidth(390);
    render(<PhoneShell />);
    const zone = screen.getByTestId("edge-swipe-open");
    fireEvent.pointerDown(zone, { pointerId: 1, clientX: 5 });
    fireEvent.pointerUp(zone, { pointerId: 1, clientX: 30 });
    expect(leadingDrawerStore.getState().isOpen).toBe(false);
  });

  it("keeps the level >= 1 edge-swipe popping (13.2 non-regression) with no drawer-open zone", async () => {
    mockRectWidth(390);
    render(<PhoneShell />);
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    await waitFor(() => {
      expect(screen.getByRole("main")).toBeInTheDocument();
    });
    // At level 1 the leading edge is the back-swipe, not the drawer-open zone.
    expect(screen.queryByTestId("edge-swipe-open")).not.toBeInTheDocument();
    const zone = screen.getByTestId("edge-swipe-back");
    fireEvent.pointerDown(zone, { pointerId: 1, clientX: 5 });
    fireEvent.pointerMove(zone, { pointerId: 1, clientX: 250 });
    fireEvent.pointerUp(zone, { pointerId: 1, clientX: 250 });
    expect(roomsStore.getState().selected).toBeNull();
    expect(leadingDrawerStore.getState().isOpen).toBe(false);
  });

  it("closes the drawer and returns focus to the avatar button when a row/view is selected", async () => {
    render(<PhoneShell />);
    const avatar = screen.getByRole("button", { name: "Open navigation" });
    act(() => {
      avatar.focus();
    });
    fireEvent.click(avatar);
    const nav = await screen.findByRole("navigation", { name: "Views" });

    // Selecting a primary view applies it and closes the drawer.
    fireEvent.click(within(nav).getByRole("button", { name: /^Approvals/ }));
    expect(primaryViewStore.getState().view).toBe("approval");
    await waitFor(() => {
      expect(leadingDrawerStore.getState().isOpen).toBe(false);
    });
    // Focus returns to the avatar drawer button (UX-DR28).
    await waitFor(() => {
      expect(avatar).toHaveFocus();
    });
  });

  it("closes the drawer on Escape and restores focus to the avatar button", async () => {
    render(<PhoneShell />);
    const avatar = screen.getByRole("button", { name: "Open navigation" });
    act(() => {
      avatar.focus();
    });
    fireEvent.click(avatar);
    const dialog = await screen.findByRole("dialog");

    fireEvent.keyDown(dialog, { key: "Escape" });
    await waitFor(() => {
      expect(leadingDrawerStore.getState().isOpen).toBe(false);
    });
    await waitFor(() => {
      expect(avatar).toHaveFocus();
    });
  });

  it("renders the drawer content with the reduced-motion cut class", async () => {
    render(<PhoneShell />);
    fireEvent.click(screen.getByRole("button", { name: "Open navigation" }));
    const content = await screen.findByTestId("leading-drawer-content");
    expect(content.className).toContain("motion-reduce:animate-none");
  });

  it("mounts the merged Search surface (closed) alongside the drawer at level 0", () => {
    render(<PhoneShell />);
    // Always mounted, store-driven, and closed by default (no surface content).
    expect(searchSurfaceStore.getState().isOpen).toBe(false);
    expect(screen.queryByTestId("phone-search-surface")).not.toBeInTheDocument();
  });

  it("opens Search via a level-0 pull-down past the reveal threshold (list at top)", () => {
    mockRectWidth(390);
    render(<PhoneShell />);
    // No account → loading state, no ScrollArea viewport, so the list counts as
    // at-top and the pull arms.
    const zone = screen.getByTestId("pull-down-search");
    fireEvent.pointerDown(zone, { pointerId: 1, clientY: 5 });
    fireEvent.pointerUp(zone, { pointerId: 1, clientY: 120 });
    expect(searchSurfaceStore.getState().isOpen).toBe(true);
  });

  it("does not open Search when the level-0 pull releases below the threshold", () => {
    mockRectWidth(390);
    render(<PhoneShell />);
    const zone = screen.getByTestId("pull-down-search");
    fireEvent.pointerDown(zone, { pointerId: 1, clientY: 5 });
    fireEvent.pointerUp(zone, { pointerId: 1, clientY: 30 });
    expect(searchSurfaceStore.getState().isOpen).toBe(false);
  });

  it("does not open Search when the Inbox list is scrolled away from the top", async () => {
    await renderWithRooms([
      { roomId: "!a:example.org", displayName: "Alpha" },
      { roomId: "!b:example.org", displayName: "Beta" },
    ]);
    // Scroll the list's viewport away from the top so the pull is left to native
    // scrolling (armed === false).
    const viewport = document.querySelector<HTMLElement>('[data-slot="scroll-area-viewport"]');
    if (viewport !== null) {
      Object.defineProperty(viewport, "scrollTop", { configurable: true, value: 200 });
    }
    const zone = screen.getByTestId("pull-down-search");
    fireEvent.pointerDown(zone, { pointerId: 1, clientY: 5 });
    fireEvent.pointerUp(zone, { pointerId: 1, clientY: 300 });
    expect(searchSurfaceStore.getState().isOpen).toBe(false);
  });

  it("does not strand the pull after a scrolled-away press whose release never reaches the zone", async () => {
    await renderWithRooms([{ roomId: "!a:example.org", displayName: "Alpha" }]);
    const viewport = document.querySelector<HTMLElement>('[data-slot="scroll-area-viewport"]');
    // Scrolled away: a press arms nothing and takes no pointer capture, so its
    // release can land off the thin band (no pointerup on the zone here).
    if (viewport !== null) {
      Object.defineProperty(viewport, "scrollTop", { configurable: true, value: 200 });
    }
    const zone = screen.getByTestId("pull-down-search");
    fireEvent.pointerDown(zone, { pointerId: 1, clientY: 5 });
    // Back at the top: a fresh pull past the reveal threshold must still open
    // Search — the earlier orphaned press must not have stranded the tracker.
    if (viewport !== null) {
      Object.defineProperty(viewport, "scrollTop", { configurable: true, value: 0 });
    }
    fireEvent.pointerDown(zone, { pointerId: 2, clientY: 5 });
    fireEvent.pointerUp(zone, { pointerId: 2, clientY: 100 });
    expect(searchSurfaceStore.getState().isOpen).toBe(true);
  });

  it("keeps the 13.2 back-swipe and 13.3 drawer gestures unregressed with the surface mounted", async () => {
    mockRectWidth(390);
    render(<PhoneShell />);
    // Drawer-open swipe still works at level 0.
    const openZone = screen.getByTestId("edge-swipe-open");
    fireEvent.pointerDown(openZone, { pointerId: 1, clientX: 5 });
    fireEvent.pointerUp(openZone, { pointerId: 1, clientX: 250 });
    expect(leadingDrawerStore.getState().isOpen).toBe(true);
    leadingDrawerStore.getState().close();

    // Back-swipe still pops at level >= 1.
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    await waitFor(() => {
      expect(screen.getByRole("main")).toBeInTheDocument();
    });
    const backZone = screen.getByTestId("edge-swipe-back");
    fireEvent.pointerDown(backZone, { pointerId: 2, clientX: 5 });
    fireEvent.pointerMove(backZone, { pointerId: 2, clientX: 250 });
    fireEvent.pointerUp(backZone, { pointerId: 2, clientX: 250 });
    expect(roomsStore.getState().selected).toBeNull();
  });

  it("closes Detail when the selection changes so it lands on the Room level (DW-109)", async () => {
    await renderWithRooms([
      { roomId: "!a:example.org", displayName: "Alpha" },
      { roomId: "!b:example.org", displayName: "Beta" },
    ]);
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    // Open Detail in a separate commit: the DW-109 effect closes Detail on any
    // selection change, so a same-batch select+open would (correctly) land on
    // the Room level.
    act(() => {
      detailStore.getState().openDetail();
    });
    await waitFor(() => {
      expect(screen.getByRole("complementary", { name: "Details" })).toBeInTheDocument();
    });

    // A different room is selected while Detail is open: the stack must land on
    // the Room level, never on Detail.
    act(() => {
      roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!b:example.org" });
    });
    await waitFor(() => {
      expect(detailStore.getState().open).toBe(false);
    });
    expect(screen.queryByRole("complementary")).not.toBeInTheDocument();
    expect(screen.getByRole("main")).toBeInTheDocument();
  });
});

describe("PhoneShell pull-to-refresh (Story 13.6)", () => {
  it("switches the pull affordance from Release-to-search to the refresh spinner across the threshold", () => {
    mockRectWidth(390);
    render(<PhoneShell />);
    const zone = screen.getByTestId("pull-down-search");

    fireEvent.pointerDown(zone, { pointerId: 1, clientY: 5 });
    // Below the reveal band: no indicator at all.
    fireEvent.pointerMove(zone, { pointerId: 1, clientY: 40 });
    expect(screen.queryByTestId("pull-indicator")).not.toBeInTheDocument();
    // In the Search band: the reveal affordance.
    fireEvent.pointerMove(zone, { pointerId: 1, clientY: 100 });
    expect(screen.getByTestId("pull-release-search")).toHaveTextContent("Release to search");
    expect(screen.queryByTestId("pull-refresh-spinner")).not.toBeInTheDocument();
    // Past the refresh threshold: the spinner affordance takes over.
    fireEvent.pointerMove(zone, { pointerId: 1, clientY: 160 });
    expect(screen.getByTestId("pull-refresh-spinner")).toBeInTheDocument();
    expect(screen.queryByTestId("pull-release-search")).not.toBeInTheDocument();
    fireEvent.pointerCancel(zone, { pointerId: 1 });
  });

  it("kicks the sync loop (not Search) when released past the refresh threshold", () => {
    mockRectWidth(390);
    render(<PhoneShell />);
    const zone = screen.getByTestId("pull-down-search");

    fireEvent.pointerDown(zone, { pointerId: 1, clientY: 5 });
    fireEvent.pointerMove(zone, { pointerId: 1, clientY: 160 });
    fireEvent.pointerUp(zone, { pointerId: 1, clientY: 160 });

    expect(syncNow).toHaveBeenCalledTimes(1);
    expect(searchSurfaceStore.getState().isOpen).toBe(false);
    // The spinner persists past the release, until the next status tick.
    expect(screen.getByTestId("pull-refresh-spinner")).toBeInTheDocument();
  });

  it("still opens Search when released inside the [reveal, refresh) band (13.4 preserved)", () => {
    mockRectWidth(390);
    render(<PhoneShell />);
    const zone = screen.getByTestId("pull-down-search");

    fireEvent.pointerDown(zone, { pointerId: 1, clientY: 5 });
    fireEvent.pointerUp(zone, { pointerId: 1, clientY: 105 });
    expect(searchSurfaceStore.getState().isOpen).toBe(true);
    expect(syncNow).not.toHaveBeenCalled();
  });

  it("clears the refresh spinner on the next connection-status tick", async () => {
    mockRectWidth(390);
    accountsStore.getState().addAccount(account);
    render(<PhoneShell />);
    const zone = screen.getByTestId("pull-down-search");

    fireEvent.pointerDown(zone, { pointerId: 1, clientY: 5 });
    fireEvent.pointerMove(zone, { pointerId: 1, clientY: 160 });
    fireEvent.pointerUp(zone, { pointerId: 1, clientY: 160 });
    expect(screen.getByTestId("pull-refresh-spinner")).toBeInTheDocument();

    act(() => {
      accountStatusStore.getState().setStatus(account.accountId, "online");
    });
    await waitFor(() => {
      expect(screen.queryByTestId("pull-refresh-spinner")).not.toBeInTheDocument();
    });
  });

  it("resolves the spinner into the persistent offline pill when every account is offline", () => {
    mockRectWidth(390);
    accountsStore.getState().addAccount(account);
    act(() => {
      accountStatusStore.getState().setStatus(account.accountId, "offline");
    });
    render(<PhoneShell />);
    const zone = screen.getByTestId("pull-down-search");

    fireEvent.pointerDown(zone, { pointerId: 1, clientY: 5 });
    fireEvent.pointerMove(zone, { pointerId: 1, clientY: 160 });
    fireEvent.pointerUp(zone, { pointerId: 1, clientY: 160 });

    // The offline pill, not a spinner — and never an error toast (no toast
    // surface is even wired into this path).
    const pill = screen.getByTestId("pull-offline-pill");
    expect(pill).toHaveTextContent(
      "Offline — showing your local archive. Messages queue until you're back.",
    );
    expect(screen.queryByTestId("pull-refresh-spinner")).not.toBeInTheDocument();
    // The kick is still attempted (best-effort resume — harmless offline).
    expect(syncNow).toHaveBeenCalledTimes(1);
  });

  it("swallows a sync_now IpcError: the spinner clears with no toast", async () => {
    mockRectWidth(390);
    syncNow.mockRejectedValue({ code: "internal", message: "boom", retriable: false });
    render(<PhoneShell />);
    const zone = screen.getByTestId("pull-down-search");

    fireEvent.pointerDown(zone, { pointerId: 1, clientY: 5 });
    fireEvent.pointerMove(zone, { pointerId: 1, clientY: 160 });
    fireEvent.pointerUp(zone, { pointerId: 1, clientY: 160 });

    await waitFor(() => {
      expect(screen.queryByTestId("pull-refresh-spinner")).not.toBeInTheDocument();
    });
    // No error surface: the shell renders no alert/toast for a failed kick.
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("does not refresh from a pull that starts scrolled away from the top", async () => {
    await renderWithRooms([
      { roomId: "!a:example.org", displayName: "Alpha" },
      { roomId: "!b:example.org", displayName: "Beta" },
    ]);
    const viewport = document.querySelector<HTMLElement>('[data-slot="scroll-area-viewport"]');
    if (viewport !== null) {
      Object.defineProperty(viewport, "scrollTop", { configurable: true, value: 200 });
    }
    const zone = screen.getByTestId("pull-down-search");
    fireEvent.pointerDown(zone, { pointerId: 1, clientY: 5 });
    fireEvent.pointerMove(zone, { pointerId: 1, clientY: 200 });
    fireEvent.pointerUp(zone, { pointerId: 1, clientY: 200 });
    expect(syncNow).not.toHaveBeenCalled();
    expect(searchSurfaceStore.getState().isOpen).toBe(false);
  });
});

describe("PhoneShell stale-resume pill (Story 14.4)", () => {
  it("renders the quiet Connecting… pill under the Inbox header while connecting", () => {
    staleResumePill.connecting = true;
    render(<PhoneShell />);

    const pill = screen.getByTestId("stale-resume-pill");
    expect(pill).toHaveTextContent("Connecting…");
    expect(pill).toHaveAttribute("role", "status");
  });

  it("is absent when not connecting", () => {
    render(<PhoneShell />);
    expect(screen.queryByTestId("stale-resume-pill")).not.toBeInTheDocument();
  });

  it("is absent at level ≥ 1 (the pill is an Inbox-header surface)", async () => {
    staleResumePill.connecting = true;
    render(<PhoneShell />);
    act(() => {
      roomsStore.getState().selectRoom(selection);
    });
    await waitFor(() => {
      expect(screen.getByRole("main")).toBeInTheDocument();
    });
    expect(screen.queryByTestId("stale-resume-pill")).not.toBeInTheDocument();
  });

  it("hides only while an actual refresh spinner is in flight, then returns", async () => {
    mockRectWidth(390);
    staleResumePill.connecting = true;
    accountsStore.getState().addAccount(account);
    render(<PhoneShell />);
    expect(screen.getByTestId("stale-resume-pill")).toBeInTheDocument();

    // A released pull past the refresh threshold puts a real spinner in flight —
    // the two indicators would say the same thing, so the pill yields.
    const zone = screen.getByTestId("pull-down-search");
    fireEvent.pointerDown(zone, { pointerId: 1, clientY: 5 });
    fireEvent.pointerMove(zone, { pointerId: 1, clientY: 160 });
    fireEvent.pointerUp(zone, { pointerId: 1, clientY: 160 });
    expect(screen.getByTestId("pull-refresh-spinner")).toBeInTheDocument();
    expect(screen.queryByTestId("stale-resume-pill")).not.toBeInTheDocument();

    // The refresh resolves on the next status tick: the pill may return (the
    // connecting state itself is the pill hook's concern).
    act(() => {
      accountStatusStore.getState().setStatus(account.accountId, "online");
    });
    await waitFor(() => {
      expect(screen.queryByTestId("pull-refresh-spinner")).not.toBeInTheDocument();
    });
    expect(screen.getByTestId("stale-resume-pill")).toBeInTheDocument();
  });

  it("yields to the pull affordance during an active reveal drag, then returns", () => {
    mockRectWidth(390);
    staleResumePill.connecting = true;
    render(<PhoneShell />);

    // Drag into the Search reveal band [reveal, refresh) without releasing: the
    // reveal affordance and the pill share this exact absolute slot, so during an
    // active gesture the pull affordance owns it and the pill yields — no overlap
    // (Review R2). The user is interacting, not passively watching for reconnect.
    const zone = screen.getByTestId("pull-down-search");
    fireEvent.pointerDown(zone, { pointerId: 1, clientY: 5 });
    fireEvent.pointerMove(zone, { pointerId: 1, clientY: 100 });
    expect(screen.getByTestId("pull-release-search")).toBeInTheDocument();
    expect(screen.queryByTestId("stale-resume-pill")).not.toBeInTheDocument();

    // Gesture ends without a refresh: the passive reconnect indicator returns.
    fireEvent.pointerCancel(zone, { pointerId: 1 });
    expect(screen.getByTestId("stale-resume-pill")).toBeInTheDocument();
  });

  it("stays hidden while genuinely offline (Connecting… would be dishonest)", () => {
    staleResumePill.connecting = true;
    accountsStore.getState().addAccount(account);
    render(<PhoneShell />);
    // Pill is up while connectivity is unknown/online…
    expect(screen.getByTestId("stale-resume-pill")).toBeInTheDocument();
    // …but the moment every account is offline, "Connecting…" is untrue — the
    // offline surface owns that state, so the pill yields (Review R2).
    act(() => {
      accountStatusStore.getState().setStatus(account.accountId, "offline");
    });
    expect(screen.queryByTestId("stale-resume-pill")).not.toBeInTheDocument();
  });
});

describe("PhoneShell keyboard inset (Story 13.5)", () => {
  /**
   * A minimal `visualViewport` stand-in (jsdom has none): a real `EventTarget`
   * carrying the mutable `height`/`offsetTop` the keyboard-inset hook reads.
   */
  class MockVisualViewport extends EventTarget {
    height: number;
    offsetTop = 0;

    constructor(height: number) {
      super();
      this.height = height;
    }
  }

  function installVisualViewport(viewport: MockVisualViewport | undefined) {
    Object.defineProperty(window, "visualViewport", {
      value: viewport as unknown as VisualViewport | undefined,
      configurable: true,
      writable: true,
    });
  }

  afterEach(() => {
    installVisualViewport(undefined);
    document.documentElement.style.removeProperty("--kb-inset");
  });

  it("drives --kb-inset from visualViewport on the phone tier", () => {
    Object.defineProperty(window, "innerHeight", {
      value: 700,
      configurable: true,
      writable: true,
    });
    const viewport = new MockVisualViewport(700);
    installVisualViewport(viewport);
    render(<PhoneShell />);

    // Keyboard closed: the visual viewport fills the layout viewport.
    expect(document.documentElement.style.getPropertyValue("--kb-inset")).toBe("0px");

    // Keyboard opens: the composer inset rises to the covered height…
    act(() => {
      viewport.height = 420;
      viewport.dispatchEvent(new Event("resize"));
    });
    expect(document.documentElement.style.getPropertyValue("--kb-inset")).toBe("280px");

    // …and dismissal returns it to 0px with no stranded offset.
    act(() => {
      viewport.height = 700;
      viewport.dispatchEvent(new Event("resize"));
    });
    expect(document.documentElement.style.getPropertyValue("--kb-inset")).toBe("0px");
  });

  it("does not subscribe to visualViewport off the phone tier", () => {
    // ≥768px: the shell's tier gate must leave the keyboard engine off even if
    // the component itself is mounted.
    mockViewportWidth(1024);
    const viewport = new MockVisualViewport(700);
    installVisualViewport(viewport);
    const addSpy = vi.spyOn(viewport, "addEventListener");
    render(<PhoneShell />);

    expect(addSpy).not.toHaveBeenCalled();
    // A keyboard-sized viewport change moves nothing: the var stays at its idle
    // value ("" fresh, or the "0px" a previous unmount's cleanup restored).
    act(() => {
      viewport.height = 420;
      viewport.dispatchEvent(new Event("resize"));
    });
    expect(["", "0px"]).toContain(document.documentElement.style.getPropertyValue("--kb-inset"));
  });
});
