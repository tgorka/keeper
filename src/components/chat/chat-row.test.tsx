import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { ChatRow } from "@/components/chat/chat-row";
import type { BridgeHealth, InboxBatch, InboxRoomVm } from "@/lib/ipc/client";
import { bridgeHealthStore } from "@/lib/stores/bridge-health";
import { draftsStore } from "@/lib/stores/drafts";
import { favoritesRoomsStore } from "@/lib/stores/favorites-rooms";
import { roomsStore } from "@/lib/stores/rooms";

// The row round-trips read/unread through the typed IPC client wrappers; mock them
// so tests assert the command without a live Tauri backend.
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    markRoomRead: vi.fn(async () => {}),
    markRoomUnread: vi.fn(async () => {}),
    archiveRoom: vi.fn(async () => {}),
    unarchiveRoom: vi.fn(async () => {}),
    pinRoom: vi.fn(async () => {}),
    unpinRoom: vi.fn(async () => {}),
    favoriteRoom: vi.fn(async () => {}),
    unfavoriteRoom: vi.fn(async () => {}),
    chatNotifyModeSet: vi.fn(async () => {}),
  };
});

import {
  archiveRoom,
  chatNotifyModeSet,
  favoriteRoom,
  markRoomRead,
  markRoomUnread,
  pinRoom,
  unarchiveRoom,
  unfavoriteRoom,
  unpinRoom,
} from "@/lib/ipc/client";

/** Seed the Favorites-window mirror's total so the discovery hint hides. */
function setFavoritesTotal(total: number): void {
  favoritesRoomsStore.getState().applyBatch({ ops: [], total } as InboxBatch);
}

function room(overrides: Partial<InboxRoomVm> = {}): InboxRoomVm {
  return {
    accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
    hueIndex: 0,
    roomId: "!abc:example.org",
    displayName: "Alice Smith",
    lastMessage: "hey there",
    timestamp: Date.now(),
    avatarUrl: null,
    isUnread: false,
    mentionCount: 0,
    isArchived: false,
    isPinned: false,
    isFavourite: false,
    network: null,
    networkId: null,
    muteState: "none",
    ...overrides,
  };
}

beforeEach(() => {
  // Default to a non-empty Favorites window so the one-time discovery hint is
  // hidden unless a test explicitly asserts it (empty favourites).
  setFavoritesTotal(1);
});

afterEach(() => {
  roomsStore.getState().clear();
  favoritesRoomsStore.getState().clear();
  bridgeHealthStore.getState().reset();
  draftsStore.getState().clear();
  vi.clearAllMocks();
});

/** Seed one session's live health into the store for the given account/network. */
function seedHealth(accountId: string, networkId: string, health: BridgeHealth) {
  bridgeHealthStore.getState().applySnapshot({
    sessions: [
      { accountId, networkId, networkName: networkId, health, lastCheckedMs: 1, detail: null },
    ],
  });
}

describe("ChatRow", () => {
  it("renders display name and preview", () => {
    render(<ChatRow room={room()} />);
    expect(screen.getByText("Alice Smith")).toBeInTheDocument();
    expect(screen.getByText("hey there")).toBeInTheDocument();
  });

  it("is a full-width accessible button with a room-labelled name", () => {
    render(<ChatRow room={room()} />);
    const button = screen.getByRole("button", { name: "Conversation with Alice Smith" });
    expect(button).toBeInTheDocument();
    expect(button).toHaveClass("w-full");
  });

  it("renders a 3 px per-account hue edge bar mapped from the hue index", () => {
    const { getByTestId } = render(<ChatRow room={room({ hueIndex: 3 })} />);
    const bar = getByTestId("account-hue-bar");
    expect(bar).toHaveClass("w-[3px]");
    // The bar's color is the CSS variable for the account's hue index — no
    // hardcoded color value.
    expect(bar.style.backgroundColor).toBe("var(--account-hue-3)");
  });

  it("wraps an out-of-range hue index into the 0..8 wheel", () => {
    const { getByTestId } = render(<ChatRow room={room({ hueIndex: 9 })} />);
    expect(getByTestId("account-hue-bar").style.backgroundColor).toBe("var(--account-hue-1)");
  });

  it("shows avatar fallback initials when no avatar url", () => {
    render(<ChatRow room={room({ displayName: "Alice Smith" })} />);
    expect(screen.getByText("AS")).toBeInTheDocument();
  });

  it("renders an empty preview when lastMessage is null", () => {
    render(<ChatRow room={room({ lastMessage: null })} />);
    expect(screen.queryByText("hey there")).not.toBeInTheDocument();
  });

  it("omits the timestamp when it is null", () => {
    const { container } = render(<ChatRow room={room({ timestamp: null })} />);
    expect(container.querySelector(".text-xs")).toBeNull();
  });

  it("renders initials fallback and no img for an mxc:// avatar url", () => {
    const { container } = render(
      <ChatRow room={room({ displayName: "Alice Smith", avatarUrl: "mxc://x/y" })} />,
    );
    expect(screen.getByText("AS")).toBeInTheDocument();
    expect(container.querySelector('img[src="mxc://x/y"]')).toBeNull();
    expect(container.querySelector("img")).toBeNull();
  });

  it("renders an img for an https:// avatar url", async () => {
    // Radix Avatar only mounts the <img> once the image reports "loaded"; jsdom
    // never fires load events, so stub window.Image to dispatch "load" once src
    // is set.
    const RealImage = window.Image;
    class LoadingImage {
      #listeners: Record<string, Array<(e: unknown) => void>> = {};
      referrerPolicy = "";
      crossOrigin: string | null = null;
      complete = false;
      naturalWidth = 0;
      #src = "";
      addEventListener(type: string, cb: (e: unknown) => void): void {
        const list = this.#listeners[type] ?? [];
        list.push(cb);
        this.#listeners[type] = list;
      }
      removeEventListener(): void {}
      get src(): string {
        return this.#src;
      }
      set src(value: string) {
        this.#src = value;
        queueMicrotask(() => {
          this.complete = true;
          this.naturalWidth = 1;
          for (const cb of this.#listeners.load ?? []) {
            cb({ currentTarget: this });
          }
        });
      }
    }
    window.Image = LoadingImage as unknown as typeof Image;
    try {
      const { container } = render(
        <ChatRow room={room({ avatarUrl: "https://cdn.example.org/a.png" })} />,
      );
      await waitFor(() => {
        expect(container.querySelector('img[src="https://cdn.example.org/a.png"]')).not.toBeNull();
      });
    } finally {
      window.Image = RealImage;
    }
  });

  it("calls onSelect with the account and room ids when clicked", () => {
    const onSelect = vi.fn();
    render(
      <ChatRow
        room={room({ accountId: "acctB", roomId: "!xyz:example.org" })}
        onSelect={onSelect}
      />,
    );
    fireEvent.click(screen.getByRole("button"));
    expect(onSelect).toHaveBeenCalledWith({ accountId: "acctB", roomId: "!xyz:example.org" });
  });

  it("marks the selected row with aria-current and a highlight", () => {
    render(<ChatRow room={room()} selected />);
    const button = screen.getByRole("button");
    expect(button).toHaveAttribute("aria-current", "true");
    expect(button).toHaveClass("bg-accent");
  });

  it("does not mark an unselected row with aria-current", () => {
    render(<ChatRow room={room()} selected={false} />);
    expect(screen.getByRole("button")).not.toHaveAttribute("aria-current");
  });

  it("read row: normal-weight name, no dot, no mention badge", () => {
    render(<ChatRow room={room({ isUnread: false, mentionCount: 0 })} />);
    expect(screen.getByText("Alice Smith")).toHaveClass("font-medium");
    expect(screen.getByText("Alice Smith")).not.toHaveClass("font-semibold");
    expect(screen.queryByTestId("unread-dot")).not.toBeInTheDocument();
    expect(screen.queryByTestId("mention-badge")).not.toBeInTheDocument();
  });

  it("unread without mention: bold name + neutral dot, no badge", () => {
    render(<ChatRow room={room({ isUnread: true, mentionCount: 0 })} />);
    expect(screen.getByText("Alice Smith")).toHaveClass("font-semibold");
    const dot = screen.getByTestId("unread-dot");
    expect(dot).toBeInTheDocument();
    expect(dot).toHaveClass("bg-muted-foreground");
    expect(screen.queryByTestId("mention-badge")).not.toBeInTheDocument();
  });

  it("unread mention: bold name + filled primary badge showing the count, no dot", () => {
    render(<ChatRow room={room({ isUnread: true, mentionCount: 3 })} />);
    expect(screen.getByText("Alice Smith")).toHaveClass("font-semibold");
    const badge = screen.getByTestId("mention-badge");
    expect(badge).toHaveTextContent("3");
    expect(badge).toHaveAttribute("data-variant", "default");
    expect(screen.queryByTestId("unread-dot")).not.toBeInTheDocument();
  });

  it("manually-marked unread with zero counts renders as unread (bold + dot)", () => {
    // Rust folds `is_marked_unread` into `isUnread`, so the row treats it as unread.
    render(<ChatRow room={room({ isUnread: true, mentionCount: 0 })} />);
    expect(screen.getByText("Alice Smith")).toHaveClass("font-semibold");
    expect(screen.getByTestId("unread-dot")).toBeInTheDocument();
  });

  it("context menu on an unread row shows Mark read and invokes markRoomRead", async () => {
    render(
      <ChatRow room={room({ accountId: "acctB", roomId: "!xyz:example.org", isUnread: true })} />,
    );
    fireEvent.contextMenu(screen.getByRole("button"));
    const item = await screen.findByText("Mark read");
    expect(screen.queryByText("Mark unread")).not.toBeInTheDocument();
    fireEvent.click(item);
    expect(markRoomRead).toHaveBeenCalledWith("acctB", "!xyz:example.org");
    expect(markRoomUnread).not.toHaveBeenCalled();
    // The overlay flipped the row to read within one frame.
    expect(roomsStore.getState().optimisticUnread.get("acctB|!xyz:example.org")).toBe(false);
  });

  it("context menu on a read row shows Mark unread and invokes markRoomUnread", async () => {
    render(
      <ChatRow room={room({ accountId: "acctB", roomId: "!xyz:example.org", isUnread: false })} />,
    );
    fireEvent.contextMenu(screen.getByRole("button"));
    const item = await screen.findByText("Mark unread");
    expect(screen.queryByText("Mark read")).not.toBeInTheDocument();
    fireEvent.click(item);
    expect(markRoomUnread).toHaveBeenCalledWith("acctB", "!xyz:example.org");
    expect(markRoomRead).not.toHaveBeenCalled();
    expect(roomsStore.getState().optimisticUnread.get("acctB|!xyz:example.org")).toBe(true);
  });

  it("reflects the optimistic overlay: a read row renders unread when overridden", () => {
    roomsStore.getState().setOptimisticUnread("acctB", "!xyz:example.org", true);
    render(
      <ChatRow room={room({ accountId: "acctB", roomId: "!xyz:example.org", isUnread: false })} />,
    );
    expect(screen.getByText("Alice Smith")).toHaveClass("font-semibold");
    expect(screen.getByTestId("unread-dot")).toBeInTheDocument();
  });

  it("optimistic read clears the mention badge in the same frame it un-bolds", () => {
    // A mention row optimistically marked read must drop the badge too, not just
    // un-bold — a read row never carries a mention badge.
    roomsStore.getState().setOptimisticUnread("acctB", "!xyz:example.org", false);
    render(
      <ChatRow
        room={room({
          accountId: "acctB",
          roomId: "!xyz:example.org",
          isUnread: true,
          mentionCount: 3,
        })}
      />,
    );
    expect(screen.getByText("Alice Smith")).not.toHaveClass("font-semibold");
    expect(screen.queryByTestId("mention-badge")).not.toBeInTheDocument();
    expect(screen.queryByTestId("unread-dot")).not.toBeInTheDocument();
  });

  it("carries the unread state in the button's accessible name", () => {
    const { rerender } = render(<ChatRow room={room({ isUnread: true, mentionCount: 0 })} />);
    expect(
      screen.getByRole("button", { name: "Conversation with Alice Smith, unread" }),
    ).toBeInTheDocument();

    rerender(<ChatRow room={room({ isUnread: true, mentionCount: 1 })} />);
    expect(
      screen.getByRole("button", { name: "Conversation with Alice Smith, 1 unread mention" }),
    ).toBeInTheDocument();

    rerender(<ChatRow room={room({ isUnread: true, mentionCount: 3 })} />);
    expect(
      screen.getByRole("button", { name: "Conversation with Alice Smith, 3 unread mentions" }),
    ).toBeInTheDocument();

    rerender(<ChatRow room={room({ isUnread: false })} />);
    expect(
      screen.getByRole("button", { name: "Conversation with Alice Smith" }),
    ).toBeInTheDocument();
  });

  it("context menu on a non-archived row shows Archive and invokes archiveRoom", async () => {
    render(
      <ChatRow
        room={room({ accountId: "acctB", roomId: "!xyz:example.org", isArchived: false })}
      />,
    );
    fireEvent.contextMenu(screen.getByRole("button"));
    const item = await screen.findByText("Archive");
    expect(screen.queryByText("Unarchive")).not.toBeInTheDocument();
    fireEvent.click(item);
    expect(archiveRoom).toHaveBeenCalledWith("acctB", "!xyz:example.org");
    expect(unarchiveRoom).not.toHaveBeenCalled();
  });

  it("context menu on an archived row shows Unarchive and invokes unarchiveRoom", async () => {
    render(
      <ChatRow room={room({ accountId: "acctB", roomId: "!xyz:example.org", isArchived: true })} />,
    );
    fireEvent.contextMenu(screen.getByRole("button"));
    const item = await screen.findByText("Unarchive");
    expect(screen.queryByText("Archive")).not.toBeInTheDocument();
    fireEvent.click(item);
    expect(unarchiveRoom).toHaveBeenCalledWith("acctB", "!xyz:example.org");
    expect(archiveRoom).not.toHaveBeenCalled();
  });

  it("context menu on an unpinned row shows Pin and invokes pinRoom", async () => {
    render(
      <ChatRow room={room({ accountId: "acctB", roomId: "!xyz:example.org", isPinned: false })} />,
    );
    fireEvent.contextMenu(screen.getByRole("button"));
    const item = await screen.findByText("Pin");
    expect(screen.queryByText("Unpin")).not.toBeInTheDocument();
    fireEvent.click(item);
    expect(pinRoom).toHaveBeenCalledWith("acctB", "!xyz:example.org");
    expect(unpinRoom).not.toHaveBeenCalled();
  });

  it("context menu on a pinned row shows Unpin and invokes unpinRoom", async () => {
    render(
      <ChatRow room={room({ accountId: "acctB", roomId: "!xyz:example.org", isPinned: true })} />,
    );
    fireEvent.contextMenu(screen.getByRole("button"));
    const item = await screen.findByText("Unpin");
    expect(screen.queryByText("Pin")).not.toBeInTheDocument();
    fireEvent.click(item);
    expect(unpinRoom).toHaveBeenCalledWith("acctB", "!xyz:example.org");
    expect(pinRoom).not.toHaveBeenCalled();
  });

  it("context menu on a non-favourite row shows Favorite and invokes favoriteRoom", async () => {
    render(
      <ChatRow
        room={room({ accountId: "acctB", roomId: "!xyz:example.org", isFavourite: false })}
      />,
    );
    fireEvent.contextMenu(screen.getByRole("button"));
    const item = await screen.findByText("Favorite");
    expect(screen.queryByText("Unfavorite")).not.toBeInTheDocument();
    fireEvent.click(item);
    expect(favoriteRoom).toHaveBeenCalledWith("acctB", "!xyz:example.org");
    expect(unfavoriteRoom).not.toHaveBeenCalled();
  });

  it("context menu on a favourite row shows Unfavorite and invokes unfavoriteRoom", async () => {
    render(
      <ChatRow
        room={room({ accountId: "acctB", roomId: "!xyz:example.org", isFavourite: true })}
      />,
    );
    fireEvent.contextMenu(screen.getByRole("button"));
    const item = await screen.findByText("Unfavorite");
    expect(screen.queryByText("Favorite")).not.toBeInTheDocument();
    fireEvent.click(item);
    expect(unfavoriteRoom).toHaveBeenCalledWith("acctB", "!xyz:example.org");
    expect(favoriteRoom).not.toHaveBeenCalled();
  });

  it("shows the one-time favourites hint only when the favourites window is empty", async () => {
    // Empty favourites window → the hint shows by the Favorite item (UX-DR13).
    setFavoritesTotal(0);
    const { unmount } = render(<ChatRow room={room({ isFavourite: false })} />);
    fireEvent.contextMenu(screen.getByRole("button"));
    expect(await screen.findByText(/Favorites keeps key chats/)).toBeInTheDocument();
    unmount();

    // Once any favourite exists, the hint disappears.
    setFavoritesTotal(1);
    render(<ChatRow room={room({ isFavourite: false })} />);
    fireEvent.contextMenu(screen.getByRole("button"));
    await screen.findByText("Favorite");
    expect(screen.queryByText(/Favorites keeps key chats/)).not.toBeInTheDocument();
  });

  it("hides the favourites hint before the first batch loads (total unknown, not zero)", async () => {
    // Pre-load the Favorites window `total` is `null`, not `0`. A user who in fact
    // has favourites must not see the discovery hint flash before the window
    // streams in, so a `null` total keeps the hint hidden (only a known-empty `0`
    // shows it).
    favoritesRoomsStore.getState().clear();
    render(<ChatRow room={room({ isFavourite: false })} />);
    fireEvent.contextMenu(screen.getByRole("button"));
    await screen.findByText("Favorite");
    expect(screen.queryByText(/Favorites keeps key chats/)).not.toBeInTheDocument();
  });

  it("reverts the optimistic overlay when the mark command hard-rejects", async () => {
    vi.mocked(markRoomRead).mockRejectedValueOnce(new Error("inactive account"));
    render(
      <ChatRow room={room({ accountId: "acctB", roomId: "!xyz:example.org", isUnread: true })} />,
    );
    fireEvent.contextMenu(screen.getByRole("button"));
    fireEvent.click(await screen.findByText("Mark read"));
    // The override is dropped so the row falls back to the authoritative stream
    // rather than stranding a phantom-read overlay the stream never reconciles.
    await waitFor(() => {
      expect(roomsStore.getState().optimisticUnread.has("acctB|!xyz:example.org")).toBe(false);
    });
  });

  it("renders the Network badge on a bridged row (Story 4.6)", () => {
    render(<ChatRow room={room({ network: "Telegram" })} />);
    expect(screen.getByLabelText("Telegram network")).toHaveTextContent("T");
  });

  it("renders no Network badge on a native row", () => {
    render(<ChatRow room={room({ network: null })} />);
    expect(screen.queryByLabelText(/network$/)).not.toBeInTheDocument();
  });

  // --- Affected-row health dot (Story 6.5) ---------------------------------

  it("shows a health dot when the row's (accountId, networkId) session is unhealthy", () => {
    const accountId = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    seedHealth(accountId, "whatsapp", "disconnected");
    render(<ChatRow room={room({ accountId, network: "WhatsApp", networkId: "whatsapp" })} />);
    const dot = screen.getByTestId("bridge-health-dot");
    expect(dot).toBeInTheDocument();
    expect(dot).toHaveClass("bg-bridge-disconnected");
  });

  it("shows no health dot when the session is healthy", () => {
    const accountId = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    seedHealth(accountId, "whatsapp", "healthy");
    render(<ChatRow room={room({ accountId, network: "WhatsApp", networkId: "whatsapp" })} />);
    expect(screen.queryByTestId("bridge-health-dot")).not.toBeInTheDocument();
  });

  it("shows no health dot when the networkId does not match (only the label matches)", () => {
    const accountId = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    // An unhealthy telegram session must not light up a whatsapp row.
    seedHealth(accountId, "telegram", "disconnected");
    render(<ChatRow room={room({ accountId, network: "WhatsApp", networkId: "whatsapp" })} />);
    expect(screen.queryByTestId("bridge-health-dot")).not.toBeInTheDocument();
  });

  it("shows no health dot on a native row (no networkId)", () => {
    const accountId = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
    seedHealth(accountId, "whatsapp", "disconnected");
    render(<ChatRow room={room({ accountId, network: null, networkId: null })} />);
    expect(screen.queryByTestId("bridge-health-dot")).not.toBeInTheDocument();
  });

  // --- Pending-draft marker (Story 7.1) ------------------------------------

  it("shows an amber pencil + Draft prefix when the chat has a pending draft", () => {
    draftsStore.getState().mark("01ARZ3NDEKTSV4RRFFQ69G5FAV", "!abc:example.org", true);
    render(<ChatRow room={room()} />);
    const marker = screen.getByTestId("draft-marker");
    expect(marker).toBeInTheDocument();
    expect(marker).toHaveTextContent("Draft");
    expect(marker).toHaveClass("text-held");
    // The last-message preview is still rendered alongside the marker.
    expect(screen.getByText("hey there")).toBeInTheDocument();
  });

  it("shows no draft marker when the chat has no pending draft", () => {
    render(<ChatRow room={room()} />);
    expect(screen.queryByTestId("draft-marker")).not.toBeInTheDocument();
  });

  it("scopes the draft marker to the matching (accountId, roomId)", () => {
    // A draft on a different room must not light up this row.
    draftsStore.getState().mark("01ARZ3NDEKTSV4RRFFQ69G5FAV", "!other:example.org", true);
    render(<ChatRow room={room()} />);
    expect(screen.queryByTestId("draft-marker")).not.toBeInTheDocument();
  });

  // ── Mute glyph + Notifications submenu (Story 10.2) ────────────────────────
  it("shows the bell-off glyph when the room is muted", () => {
    render(<ChatRow room={room({ muteState: "muted" })} />);
    expect(screen.getByTestId("mute-glyph")).toBeInTheDocument();
    expect(screen.queryByTestId("mention-only-glyph")).not.toBeInTheDocument();
  });

  it("shows the at-sign glyph when the room is mention-only", () => {
    render(<ChatRow room={room({ muteState: "mention_only" })} />);
    expect(screen.getByTestId("mention-only-glyph")).toBeInTheDocument();
    expect(screen.queryByTestId("mute-glyph")).not.toBeInTheDocument();
  });

  it("shows no mute glyph when the room is not muted", () => {
    render(<ChatRow room={room({ muteState: "none" })} />);
    expect(screen.queryByTestId("mute-glyph")).not.toBeInTheDocument();
    expect(screen.queryByTestId("mention-only-glyph")).not.toBeInTheDocument();
  });

  it("Notifications submenu → Mute sets the mute mode via chatNotifyModeSet", async () => {
    render(<ChatRow room={room({ accountId: "acctZ", roomId: "!z:example.org" })} />);
    fireEvent.contextMenu(screen.getByRole("button"));
    // Open the Notifications submenu, then pick Mute.
    fireEvent.click(await screen.findByText("Notifications"));
    fireEvent.click(await screen.findByText("Mute"));
    expect(chatNotifyModeSet).toHaveBeenCalledWith("acctZ", "!z:example.org", "mute");
  });

  it("Notifications submenu → Mentions only sets mention_only", async () => {
    render(<ChatRow room={room({ accountId: "acctZ", roomId: "!z:example.org" })} />);
    fireEvent.contextMenu(screen.getByRole("button"));
    fireEvent.click(await screen.findByText("Notifications"));
    fireEvent.click(await screen.findByText("Mentions only"));
    expect(chatNotifyModeSet).toHaveBeenCalledWith("acctZ", "!z:example.org", "mention_only");
  });

  it("Notifications submenu → All clears the rule (unmute target)", async () => {
    render(
      <ChatRow room={room({ accountId: "acctZ", roomId: "!z:example.org", muteState: "muted" })} />,
    );
    fireEvent.contextMenu(screen.getByRole("button"));
    fireEvent.click(await screen.findByText("Notifications"));
    fireEvent.click(await screen.findByText("All"));
    expect(chatNotifyModeSet).toHaveBeenCalledWith("acctZ", "!z:example.org", "all");
  });

  // ── Phone touch idioms (Story 13.6) ────────────────────────────────────────
  it("renders no swipe stage off the phone tier (desktop byte-for-byte)", () => {
    render(<ChatRow room={room()} />);
    expect(screen.queryByTestId("chat-row-swipe")).not.toBeInTheDocument();
  });
});

// ── Phone touch idioms (Story 13.6) ──────────────────────────────────────────
describe("ChatRow phone touch idioms", () => {
  const originalMatchMedia = window.matchMedia;

  /** Mock matchMedia at a phone-tier width (mirrors the phone-shell tests). */
  function mockPhoneViewport() {
    window.matchMedia = vi.fn().mockImplementation((query: string) => {
      const match = query.match(/max-width:\s*(\d+)px/);
      const maxWidth = match ? Number(match[1]) : Number.POSITIVE_INFINITY;
      return {
        matches: query.includes("prefers-reduced-motion") ? false : 390 <= maxWidth,
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

  /** Mock every rect at the given width so the swipe reads a real drag range. */
  function mockRowWidth(width: number) {
    vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
      width,
      height: 64,
      top: 0,
      left: 0,
      right: width,
      bottom: 64,
      x: 0,
      y: 0,
      toJSON: () => ({}),
    } as DOMRect);
  }

  /** Dispatch a pointer event with an explicit timeStamp (slow, non-flick drags). */
  function firePointer(
    el: Element,
    type: "pointerdown" | "pointermove" | "pointerup",
    init: { pointerId: number; clientX: number; clientY: number; timeStamp: number },
  ) {
    const event = new MouseEvent(type, {
      bubbles: true,
      cancelable: true,
      clientX: init.clientX,
      clientY: init.clientY,
    });
    Object.defineProperty(event, "pointerId", { value: init.pointerId });
    Object.defineProperty(event, "timeStamp", { value: init.timeStamp });
    fireEvent(el, event);
  }

  /** Slow-settle the trailing actions open (More + Archive tappable). */
  function settleTrailingReveal(wrapper: Element) {
    firePointer(wrapper, "pointerdown", {
      pointerId: 1,
      clientX: 300,
      clientY: 30,
      timeStamp: 1000,
    });
    firePointer(wrapper, "pointermove", {
      pointerId: 1,
      clientX: 200,
      clientY: 30,
      timeStamp: 1200,
    });
    firePointer(wrapper, "pointerup", { pointerId: 1, clientX: 200, clientY: 30, timeStamp: 1400 });
  }

  beforeEach(() => {
    mockPhoneViewport();
  });

  afterEach(() => {
    window.matchMedia = originalMatchMedia;
    vi.restoreAllMocks();
    vi.useRealTimers();
  });

  it("opens the row's identical context menu on a stationary long-press", async () => {
    vi.useFakeTimers();
    render(<ChatRow room={room()} />);
    const wrapper = screen.getByTestId("chat-row-swipe");
    fireEvent.pointerDown(wrapper, { pointerId: 1, clientX: 100, clientY: 30 });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    vi.useRealTimers();
    // The same menu the desktop right-click opens: triage verbs + Notifications.
    expect(await screen.findByText("Mark unread")).toBeInTheDocument();
    expect(screen.getByText("Archive")).toBeInTheDocument();
    expect(screen.getByText("Notifications")).toBeInTheDocument();
  });

  it("does not open the menu when the press moves (scroll intent)", () => {
    vi.useFakeTimers();
    render(<ChatRow room={room()} />);
    const wrapper = screen.getByTestId("chat-row-swipe");
    fireEvent.pointerDown(wrapper, { pointerId: 1, clientX: 100, clientY: 30 });
    fireEvent.pointerMove(wrapper, { pointerId: 1, clientX: 100, clientY: 60 });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    expect(screen.queryByText("Mark unread")).not.toBeInTheDocument();
  });

  it("suppresses the native callout on the swipe/long-press stage", () => {
    render(<ChatRow room={room()} />);
    const wrapper = screen.getByTestId("chat-row-swipe");
    expect(wrapper).toHaveClass("touch-callout-none");
    expect(wrapper).toHaveClass("select-none");
  });

  it("reveals More + Archive mid-drag and floods the Archive label past half-swipe", () => {
    mockRowWidth(320);
    render(<ChatRow room={room()} />);
    const wrapper = screen.getByTestId("chat-row-swipe");

    fireEvent.pointerDown(wrapper, { pointerId: 1, clientX: 300, clientY: 30 });
    fireEvent.pointerMove(wrapper, { pointerId: 1, clientX: 200, clientY: 30 });
    // Below the half-swipe commit: the two revealed buttons, no flooded label.
    expect(
      screen.getByRole("button", { name: "More actions for Alice Smith" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Archive Alice Smith" })).toBeInTheDocument();
    expect(screen.queryByTestId("swipe-commit-label")).not.toBeInTheDocument();

    // Past half the width the label appears (the full-swipe affordance).
    fireEvent.pointerMove(wrapper, { pointerId: 1, clientX: 100, clientY: 30 });
    expect(screen.getByTestId("swipe-commit-label")).toHaveTextContent("Archive");
    fireEvent.pointerCancel(wrapper, { pointerId: 1 });
  });

  it("commits Archive on a full trailing swipe", () => {
    mockRowWidth(320);
    render(<ChatRow room={room({ accountId: "acctB", roomId: "!s:example.org" })} />);
    const wrapper = screen.getByTestId("chat-row-swipe");

    fireEvent.pointerDown(wrapper, { pointerId: 1, clientX: 300, clientY: 30 });
    fireEvent.pointerMove(wrapper, { pointerId: 1, clientX: 100, clientY: 30 });
    fireEvent.pointerUp(wrapper, { pointerId: 1, clientX: 100, clientY: 30 });
    expect(archiveRoom).toHaveBeenCalledWith("acctB", "!s:example.org");
  });

  it("commits Unarchive on the trailing swipe of an archived row", () => {
    mockRowWidth(320);
    render(<ChatRow room={room({ isArchived: true })} />);
    const wrapper = screen.getByTestId("chat-row-swipe");

    fireEvent.pointerDown(wrapper, { pointerId: 1, clientX: 300, clientY: 30 });
    fireEvent.pointerMove(wrapper, { pointerId: 1, clientX: 100, clientY: 30 });
    fireEvent.pointerUp(wrapper, { pointerId: 1, clientX: 100, clientY: 30 });
    expect(unarchiveRoom).toHaveBeenCalled();
    expect(archiveRoom).not.toHaveBeenCalled();
  });

  it("toggles unread on a leading swipe with the verb label past half-swipe", () => {
    mockRowWidth(320);
    render(<ChatRow room={room({ isUnread: false })} />);
    const wrapper = screen.getByTestId("chat-row-swipe");

    fireEvent.pointerDown(wrapper, { pointerId: 1, clientX: 20, clientY: 30 });
    fireEvent.pointerMove(wrapper, { pointerId: 1, clientX: 220, clientY: 30 });
    expect(screen.getByTestId("swipe-leading")).toHaveTextContent("Unread");
    fireEvent.pointerUp(wrapper, { pointerId: 1, clientX: 220, clientY: 30 });
    expect(markRoomUnread).toHaveBeenCalled();
    expect(markRoomRead).not.toHaveBeenCalled();
  });

  it("marks an unread row read on the leading swipe (with the optimistic overlay)", () => {
    mockRowWidth(320);
    render(<ChatRow room={room({ isUnread: true })} />);
    const wrapper = screen.getByTestId("chat-row-swipe");

    fireEvent.pointerDown(wrapper, { pointerId: 1, clientX: 20, clientY: 30 });
    fireEvent.pointerMove(wrapper, { pointerId: 1, clientX: 220, clientY: 30 });
    fireEvent.pointerUp(wrapper, { pointerId: 1, clientX: 220, clientY: 30 });
    expect(markRoomRead).toHaveBeenCalled();
    // The same optimistic within-one-frame flip the menu action uses.
    expect(screen.getByText("Alice Smith")).toHaveClass("font-medium");
  });

  it("snaps back with no action on a small release", () => {
    mockRowWidth(320);
    render(<ChatRow room={room()} />);
    const wrapper = screen.getByTestId("chat-row-swipe");

    fireEvent.pointerDown(wrapper, { pointerId: 1, clientX: 300, clientY: 30 });
    fireEvent.pointerMove(wrapper, { pointerId: 1, clientX: 270, clientY: 30 });
    fireEvent.pointerUp(wrapper, { pointerId: 1, clientX: 270, clientY: 30 });
    expect(archiveRoom).not.toHaveBeenCalled();
    expect(screen.getByTestId("chat-row-swipe-stage").style.transform).toBe("translateX(0px)");
  });

  it("does not open the conversation on the click after a snap-back swipe", () => {
    mockRowWidth(320);
    const onSelect = vi.fn();
    render(<ChatRow room={room()} onSelect={onSelect} />);
    const wrapper = screen.getByTestId("chat-row-swipe");

    // Drag past the intent slop then release back at the origin, then the
    // browser-synthesized click must be swallowed (not tap through to open).
    fireEvent.pointerDown(wrapper, { pointerId: 1, clientX: 300, clientY: 30 });
    fireEvent.pointerMove(wrapper, { pointerId: 1, clientX: 280, clientY: 30 });
    fireEvent.pointerUp(wrapper, { pointerId: 1, clientX: 300, clientY: 30 });
    fireEvent.click(screen.getByRole("button", { name: "Conversation with Alice Smith" }));
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("leaves vertical scrolling alone (|dy| > |dx| bails out)", () => {
    mockRowWidth(320);
    render(<ChatRow room={room()} />);
    const wrapper = screen.getByTestId("chat-row-swipe");

    fireEvent.pointerDown(wrapper, { pointerId: 1, clientX: 200, clientY: 30 });
    fireEvent.pointerMove(wrapper, { pointerId: 1, clientX: 195, clientY: 80 });
    expect(screen.getByTestId("chat-row-swipe-stage").style.transform).toBe("translateX(0px)");
    fireEvent.pointerUp(wrapper, { pointerId: 1, clientX: 195, clientY: 80 });
    expect(archiveRoom).not.toHaveBeenCalled();
    expect(markRoomRead).not.toHaveBeenCalled();
    expect(markRoomUnread).not.toHaveBeenCalled();
  });

  it("opens the row menu (mute ▸ Notifications) from the settled More button", async () => {
    mockRowWidth(320);
    render(<ChatRow room={room()} />);
    const wrapper = screen.getByTestId("chat-row-swipe");

    settleTrailingReveal(wrapper);
    const more = screen.getByRole("button", { name: "More actions for Alice Smith" });
    fireEvent.click(more);
    // The identical ContextMenu opens — mute lives in its Notifications submenu.
    fireEvent.click(await screen.findByText("Notifications"));
    fireEvent.click(await screen.findByText("Mute"));
    expect(chatNotifyModeSet).toHaveBeenCalledWith(
      "01ARZ3NDEKTSV4RRFFQ69G5FAV",
      "!abc:example.org",
      "mute",
    );
  });

  it("archives from the settled Archive button", () => {
    mockRowWidth(320);
    render(<ChatRow room={room()} />);
    const wrapper = screen.getByTestId("chat-row-swipe");

    settleTrailingReveal(wrapper);
    fireEvent.click(screen.getByRole("button", { name: "Archive Alice Smith" }));
    expect(archiveRoom).toHaveBeenCalled();
    // The reveal closes after the action.
    expect(screen.getByTestId("chat-row-swipe-stage").style.transform).toBe("translateX(0px)");
  });

  it("closes a settled reveal on row tap instead of opening the conversation", () => {
    mockRowWidth(320);
    const onSelect = vi.fn();
    render(<ChatRow room={room()} onSelect={onSelect} />);
    const wrapper = screen.getByTestId("chat-row-swipe");

    settleTrailingReveal(wrapper);
    fireEvent.click(screen.getByRole("button", { name: "Conversation with Alice Smith" }));
    expect(onSelect).not.toHaveBeenCalled();
    expect(screen.getByTestId("chat-row-swipe-stage").style.transform).toBe("translateX(0px)");

    // The next tap opens the conversation as usual.
    fireEvent.click(screen.getByRole("button", { name: "Conversation with Alice Smith" }));
    expect(onSelect).toHaveBeenCalledTimes(1);
  });
});
