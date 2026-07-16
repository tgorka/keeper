import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  PaletteActionVm,
  PaletteChatVm,
  PaletteResultsVm,
  SearchHitVm,
} from "@/lib/ipc/client";

// Mock the typed IPC client so the surface never touches Tauri. `paletteQuery`
// feeds Chats/Actions; `searchArchive` feeds the reused Messages SearchPanel.
const paletteQuery = vi.fn();
const searchArchive = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  paletteQuery: (query: string, mode: string, openChat: boolean) =>
    paletteQuery(query, mode, openChat),
  searchArchive: (filter: unknown) => searchArchive(filter),
}));

import { PhoneSearchSurface } from "@/components/layout/phone-search-surface";
import { accountsStore } from "@/lib/stores/accounts";
import { networksStore } from "@/lib/stores/networks";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";
import { searchSurfaceStore } from "@/lib/stores/search-surface";

function chat(p: Partial<PaletteChatVm> & Pick<PaletteChatVm, "roomId">): PaletteChatVm {
  return {
    id: `${p.accountId ?? "acc-a"}|${p.roomId}`,
    accountId: p.accountId ?? "acc-a",
    roomId: p.roomId,
    displayName: p.displayName ?? p.roomId,
    hueIndex: p.hueIndex ?? 0,
    network: p.network ?? null,
    isDirect: p.isDirect ?? false,
  };
}

function action(
  p: Partial<PaletteActionVm> & Pick<PaletteActionVm, "id" | "title">,
): PaletteActionVm {
  return {
    id: p.id,
    title: p.title,
    category: p.category ?? "Navigation",
    keywords: p.keywords ?? [],
    shortcut: p.shortcut ?? null,
    requiresOpenChat: p.requiresOpenChat ?? false,
    requiresRecording: p.requiresRecording ?? false,
    toggleGroup: p.toggleGroup ?? null,
  };
}

function hit(
  p: Partial<SearchHitVm> & Pick<SearchHitVm, "accountId" | "roomId" | "eventId">,
): SearchHitVm {
  return {
    accountId: p.accountId,
    roomId: p.roomId,
    eventId: p.eventId,
    sender: p.sender ?? "@alice:x",
    body: p.body ?? "hello world",
    timestamp: p.timestamp ?? 1_700_000_000_000,
    redacted: p.redacted ?? false,
  };
}

const EMPTY: PaletteResultsVm = { contacts: [], chats: [], actions: [] };

beforeEach(() => {
  paletteQuery.mockReset();
  paletteQuery.mockResolvedValue(EMPTY);
  searchArchive.mockReset();
  searchArchive.mockResolvedValue([]);
  searchSurfaceStore.setState({ isOpen: false, scope: "chats", chatLock: null });
  roomsStore.setState({ rooms: [], selected: null, focusEvent: null });
  accountsStore.setState({ accounts: [] });
  networksStore.setState({ networks: [] });
  primaryViewStore.setState({ view: "inbox" });
});

afterEach(() => {
  searchSurfaceStore.setState({ isOpen: false, scope: "chats", chatLock: null });
  vi.clearAllMocks();
});

function open(options?: Parameters<ReturnType<typeof searchSurfaceStore.getState>["open"]>[0]) {
  act(() => {
    searchSurfaceStore.getState().open(options);
  });
}

function typeQuery(text: string) {
  const input = screen.getByLabelText("Search query");
  fireEvent.change(input, { target: { value: text } });
}

describe("PhoneSearchSurface", () => {
  it("is closed by default and opens per the store in Chats scope", () => {
    render(<PhoneSearchSurface />);
    expect(screen.queryByTestId("phone-search-surface")).not.toBeInTheDocument();
    open();
    expect(screen.getByTestId("phone-search-surface")).toBeInTheDocument();
    // Segmented scopes + no bottom tab bar (the scopes use a scoped tablist, not a
    // bottom nav).
    expect(screen.getByRole("tab", { name: "Chats" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByRole("tab", { name: "Messages" })).toBeInTheDocument();
    expect(screen.getByRole("tab", { name: "Actions" })).toBeInTheDocument();
  });

  it("queries paletteQuery in default mode for Chats and opens a chat on select", async () => {
    paletteQuery.mockResolvedValue({
      contacts: [chat({ roomId: "!alice", displayName: "Alice", isDirect: true })],
      chats: [chat({ roomId: "!alpha", displayName: "Alpha Team", network: "Telegram" })],
      actions: [],
    });
    render(<PhoneSearchSurface />);
    open();
    typeQuery("al");

    await waitFor(() => expect(paletteQuery).toHaveBeenCalledWith("al", "default", false));
    const alpha = await screen.findByText("Alpha Team");
    fireEvent.click(alpha);

    expect(roomsStore.getState().selected).toEqual({ accountId: "acc-a", roomId: "!alpha" });
    expect(searchSurfaceStore.getState().isOpen).toBe(false);
  });

  it("switches to Actions scope on the > prefix and dispatches on select + closes", async () => {
    paletteQuery.mockResolvedValue({
      contacts: [],
      chats: [],
      actions: [action({ id: "open-archive", title: "Open Archive" })],
    });
    render(<PhoneSearchSurface />);
    open();
    typeQuery(">arch");

    await waitFor(() => expect(paletteQuery).toHaveBeenCalledWith("arch", "action", false));
    // The Actions tab reflects the forced scope.
    expect(screen.getByRole("tab", { name: "Actions" })).toHaveAttribute("aria-selected", "true");

    const item = await screen.findByText("Open Archive");
    fireEvent.click(item);
    await waitFor(() => expect(primaryViewStore.getState().view).toBe("archive"));
    expect(searchSurfaceStore.getState().isOpen).toBe(false);
  });

  it("switches scope via the segmented control", async () => {
    render(<PhoneSearchSurface />);
    open();
    fireEvent.click(screen.getByRole("tab", { name: "Actions" }));
    await waitFor(() => expect(paletteQuery).toHaveBeenCalledWith("", "action", false));
  });

  it("a scope tap strips a stray leading > so the tap is not inert (review patch)", async () => {
    render(<PhoneSearchSurface />);
    open();
    // Typing `>foo` forces Actions via the `>`-prefix rule.
    typeQuery(">foo");
    await waitFor(() => expect(paletteQuery).toHaveBeenCalledWith("foo", "action", false));
    expect(screen.getByRole("tab", { name: "Actions" })).toHaveAttribute("aria-selected", "true");

    // Tapping the Chats tab must actually switch scope — the leading `>` is stripped
    // so it no longer overrides the tapped scope (otherwise the tap looks dead).
    fireEvent.click(screen.getByRole("tab", { name: "Chats" }));
    expect(screen.getByLabelText("Search query")).toHaveValue("foo");
    await waitFor(() => expect(paletteQuery).toHaveBeenCalledWith("foo", "default", false));
    expect(screen.getByRole("tab", { name: "Chats" })).toHaveAttribute("aria-selected", "true");
  });

  it("renders the reused Messages SearchPanel and deep-links + closes on activate", async () => {
    roomsStore.setState({
      rooms: [
        {
          accountId: "a1",
          hueIndex: 0,
          roomId: "!r1:x",
          displayName: "Design",
          lastMessage: null,
          timestamp: null,
          avatarUrl: null,
          isUnread: false,
          mentionCount: 0,
          isArchived: false,
          isFavourite: false,
          isPinned: false,
          network: null,
          networkId: null,
          muteState: "none",
        },
      ],
    });
    searchArchive.mockResolvedValue([hit({ accountId: "a1", roomId: "!r1:x", eventId: "$e1" })]);
    render(<PhoneSearchSurface />);
    open({ scope: "messages" });
    // The Messages scope renders the reused SearchPanel (its offline header).
    expect(screen.getByText("Searching your local archive")).toBeInTheDocument();

    fireEvent.change(screen.getByLabelText("Search query"), { target: { value: "hello" } });
    await waitFor(() => expect(searchArchive).toHaveBeenCalled());
    const result = await screen.findByText("Design");
    expect(result).toBeInTheDocument();
    fireEvent.click(screen.getByRole("option"));

    expect(roomsStore.getState().focusEvent).toEqual({
      accountId: "a1",
      roomId: "!r1:x",
      eventId: "$e1",
    });
    expect(searchSurfaceStore.getState().isOpen).toBe(false);
  });

  it("opens with a chatLock in Messages scope showing a locked Chat chip", async () => {
    roomsStore.setState({
      rooms: [
        {
          accountId: "a1",
          hueIndex: 0,
          roomId: "!r1:x",
          displayName: "Design",
          lastMessage: null,
          timestamp: null,
          avatarUrl: null,
          isUnread: false,
          mentionCount: 0,
          isArchived: false,
          isFavourite: false,
          isPinned: false,
          network: null,
          networkId: null,
          muteState: "none",
        },
      ],
    });
    render(<PhoneSearchSurface />);
    open({ scope: "messages", chatLock: { accountId: "a1", roomId: "!r1:x" } });

    expect(screen.getByRole("tab", { name: "Messages" })).toHaveAttribute("aria-selected", "true");
    expect(screen.getByLabelText("Chat filter (locked)")).toHaveTextContent("Design");
  });

  it("closes on the back/close affordance", () => {
    render(<PhoneSearchSurface />);
    open();
    fireEvent.click(screen.getByRole("button", { name: "Close search" }));
    expect(searchSurfaceStore.getState().isOpen).toBe(false);
  });

  it("renders the reduced-motion cut class and no bottom tab bar", () => {
    render(<PhoneSearchSurface />);
    open();
    const surface = screen.getByTestId("phone-search-surface");
    expect(surface.className).toContain("motion-reduce:animate-none");
    // The scope control is a scoped tablist, never a bottom navigation/tab bar.
    expect(screen.queryByRole("navigation")).not.toBeInTheDocument();
  });
});
