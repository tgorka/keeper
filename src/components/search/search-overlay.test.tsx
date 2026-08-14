import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, InboxRoomVm, IpcError, SearchHitVm } from "@/lib/ipc/client";

// Mock the typed IPC client so the overlay never touches Tauri. The two
// document searches (FR-267) resolve to nothing here: what this file tests is
// the surface and its source switcher, and each document panel has its own.
const searchArchive = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  searchArchive: (filter: unknown) => searchArchive(filter),
  notesSearch: () => Promise.resolve(undefined),
  sessionsSearch: () => Promise.resolve("scan-1"),
  sessionsSearchCancel: () => Promise.resolve(undefined),
}));

import { SearchOverlay } from "@/components/search/search-overlay";
import { accountsStore } from "@/lib/stores/accounts";
import { networksStore } from "@/lib/stores/networks";
import { notesVaultsStore } from "@/lib/stores/notes-vaults";
import { roomsStore } from "@/lib/stores/rooms";
import { searchStore } from "@/lib/stores/search";
import { sessionsRootsStore } from "@/lib/stores/sessions-roots";

function room(p: Pick<InboxRoomVm, "accountId" | "roomId"> & Partial<InboxRoomVm>): InboxRoomVm {
  return {
    accountId: p.accountId,
    hueIndex: p.hueIndex ?? 0,
    roomId: p.roomId,
    displayName: p.displayName ?? p.roomId,
    lastMessage: null,
    timestamp: null,
    avatarUrl: null,
    isUnread: false,
    mentionCount: 0,
    isArchived: false,
    isFavourite: false,
    isPinned: false,
    network: p.network ?? null,
    networkId: p.networkId ?? null,
    muteState: p.muteState ?? "none",
  };
}

function account(accountId: string, userId: string, hueIndex = 0): AccountVm {
  return { accountId, userId, homeserverUrl: "https://x", hueIndex, provider: "password" };
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

beforeEach(() => {
  searchArchive.mockReset();
  searchArchive.mockResolvedValue([]);
  searchStore.setState({ isOpen: true, scope: "global", source: "messages" });
  roomsStore.setState({ rooms: [], selected: null, focusEvent: null });
  accountsStore.setState({ accounts: [] });
  networksStore.setState({ networks: [] });
  notesVaultsStore.setState({ activeVaultId: null });
  sessionsRootsStore.setState({ activeRootId: null });
});

afterEach(() => {
  searchStore.setState({ isOpen: false, scope: "global", source: "messages" });
  vi.clearAllMocks();
});

async function type(text: string) {
  const input = screen.getByLabelText("Search query");
  fireEvent.change(input, { target: { value: text } });
  return input;
}

describe("SearchOverlay", () => {
  it("shows the honest header and offline note", () => {
    render(<SearchOverlay />);
    expect(screen.getByText("Searching your local archive")).toBeInTheDocument();
    expect(screen.getByText(/works fully offline/i)).toBeInTheDocument();
  });

  it("groups results by Chat and tints matched terms", async () => {
    roomsStore.setState({
      rooms: [room({ accountId: "a1", roomId: "!r1:x", displayName: "Design", hueIndex: 2 })],
    });
    accountsStore.setState({ accounts: [account("a1", "@me:x", 2)] });
    searchArchive.mockResolvedValue([
      hit({ accountId: "a1", roomId: "!r1:x", eventId: "$e1", body: "hello there" }),
      hit({ accountId: "a1", roomId: "!r1:x", eventId: "$e2", body: "well hello" }),
    ]);
    render(<SearchOverlay />);
    await type("hello");

    await waitFor(() => expect(screen.getByText("Design")).toBeInTheDocument());
    // Both hits render under the one Chat group.
    expect(screen.getAllByRole("option")).toHaveLength(2);
    // The matched term is wrapped in a <mark> carrying the search-highlight tint.
    const marks = document.querySelectorAll("mark.bg-search-highlight");
    expect(marks.length).toBeGreaterThanOrEqual(2);
  });

  it("shows the no-matches state with removable chips and keeps the offline note", async () => {
    searchArchive.mockResolvedValue([]);
    render(<SearchOverlay />);
    // Set a sender filter chip (a plain field), then run a query with 0 hits.
    fireEvent.change(screen.getByLabelText("Sender"), { target: { value: "@bob:x" } });
    await type("nothing");

    await waitFor(() =>
      expect(screen.getByText("No matches in your archive.")).toBeInTheDocument(),
    );
    // The active chip is present and one-tap removable; the offline note stays.
    expect(screen.getByText(/Sender: @bob:x/)).toBeInTheDocument();
    expect(screen.getByText(/works fully offline/i)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: /Remove Sender: @bob:x/ }));
    expect(screen.queryByText(/Sender: @bob:x/)).not.toBeInTheDocument();
  });

  it("disambiguates the same contact across two accounts with hue dot + userId meta", async () => {
    roomsStore.setState({
      rooms: [
        room({ accountId: "a1", roomId: "!r:x", displayName: "Bob", hueIndex: 1 }),
        room({ accountId: "a2", roomId: "!r:x", displayName: "Bob", hueIndex: 3 }),
      ],
    });
    accountsStore.setState({
      accounts: [account("a1", "@me1:x", 1), account("a2", "@me2:x", 3)],
    });
    searchArchive.mockResolvedValue([
      hit({ accountId: "a1", roomId: "!r:x", eventId: "$a" }),
      hit({ accountId: "a2", roomId: "!r:x", eventId: "$b" }),
    ]);
    render(<SearchOverlay />);
    await type("hello");

    // Two distinct groups, each carrying its account userId in the meta.
    await waitFor(() => expect(screen.getByText("@me1:x")).toBeInTheDocument());
    expect(screen.getByText("@me2:x")).toBeInTheDocument();
  });

  it("deep-links on Enter: requests focus for the active hit and closes", async () => {
    const requestFocus = vi.fn();
    roomsStore.setState({
      rooms: [room({ accountId: "a1", roomId: "!r1:x", displayName: "Design" })],
      requestFocus,
    });
    searchArchive.mockResolvedValue([hit({ accountId: "a1", roomId: "!r1:x", eventId: "$e1" })]);
    render(<SearchOverlay />);
    const input = await type("hello");
    await waitFor(() => expect(screen.getByText("Design")).toBeInTheDocument());

    fireEvent.keyDown(input, { key: "Enter" });
    expect(requestFocus).toHaveBeenCalledWith({
      accountId: "a1",
      roomId: "!r1:x",
      eventId: "$e1",
    });
    expect(searchStore.getState().isOpen).toBe(false);
  });

  it("surfaces an honest inline error on an IpcError and keeps the offline note", async () => {
    const err: IpcError = {
      code: "internal",
      message: "archive read failed",
      accountId: null,
      retriable: true,
    };
    searchArchive.mockRejectedValue(err);
    render(<SearchOverlay />);
    await type("hello");

    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(/archive read failed/));
    expect(screen.getByText(/works fully offline/i)).toBeInTheDocument();
  });

  it("locks the surface to the open Chat in chat scope", async () => {
    roomsStore.setState({
      rooms: [room({ accountId: "a1", roomId: "!r1:x", displayName: "Design" })],
      selected: { accountId: "a1", roomId: "!r1:x" },
    });
    searchStore.setState({ isOpen: true, scope: "chat" });
    searchArchive.mockResolvedValue([]);
    render(<SearchOverlay />);
    await type("hello");

    // The locked Chat chip is shown; the query is scoped to that room/account.
    expect(screen.getByLabelText("Chat filter (locked)")).toHaveTextContent("Design");
    await waitFor(() => expect(searchArchive).toHaveBeenCalled());
    const calls = searchArchive.mock.calls;
    const filter = calls[calls.length - 1]?.[0];
    expect(filter.roomIds).toEqual(["!r1:x"]);
    expect(filter.accountIds).toEqual(["a1"]);
  });

  it("discards a stale (superseded) response so the newest query wins", async () => {
    roomsStore.setState({
      rooms: [room({ accountId: "a1", roomId: "!r1:x", displayName: "Design" })],
    });
    accountsStore.setState({ accounts: [account("a1", "@me:x")] });
    // First (slow) call resolves late with a stale body; second (fast) wins.
    let resolveSlow: (v: SearchHitVm[]) => void = () => {};
    searchArchive
      .mockImplementationOnce(
        () =>
          new Promise<SearchHitVm[]>((resolve) => {
            resolveSlow = resolve;
          }),
      )
      .mockResolvedValueOnce([
        hit({ accountId: "a1", roomId: "!r1:x", eventId: "$fresh", body: "fresh hit" }),
      ]);

    render(<SearchOverlay />);
    await type("hell");
    // Let the debounce fire the first (slow, still-pending) call.
    await waitFor(() => expect(searchArchive).toHaveBeenCalledTimes(1));
    await type("hello");
    // The second (fast) call dispatches and resolves; the fresh result shows.
    await waitFor(() => expect(searchArchive).toHaveBeenCalledTimes(2));
    await waitFor(() => expect(screen.getByText(/fresh hit/)).toBeInTheDocument());
    // Now the stale (superseded) response arrives — it must be discarded.
    resolveSlow([hit({ accountId: "a1", roomId: "!r1:x", eventId: "$stale", body: "stale hit" })]);
    await new Promise((r) => setTimeout(r, 10));
    expect(screen.queryByText(/stale hit/)).not.toBeInTheDocument();
    expect(screen.getByText(/fresh hit/)).toBeInTheDocument();
  });

  it("makes no call for an empty query", async () => {
    render(<SearchOverlay />);
    await type("   ");
    await new Promise((r) => setTimeout(r, 250));
    expect(searchArchive).not.toHaveBeenCalled();
  });

  // FR-267: three sources on one surface, so the operator does not have to know
  // where the sentence they remember was written before they can look for it.
  describe("source switcher", () => {
    it("switches to the notes body and back", () => {
      notesVaultsStore.setState({ activeVaultId: "v1" });
      render(<SearchOverlay />);
      expect(screen.getByText("Searching your local archive")).toBeInTheDocument();

      fireEvent.click(screen.getByRole("tab", { name: "Notes" }));
      expect(searchStore.getState().source).toBe("notes");
      // The messages body is gone — its header is the tell — and the notes
      // field is what took its place.
      expect(screen.queryByText("Searching your local archive")).not.toBeInTheDocument();
      expect(screen.getByPlaceholderText("Search notes")).toBeInTheDocument();

      fireEvent.click(screen.getByRole("tab", { name: "Messages" }));
      expect(screen.getByText("Searching your local archive")).toBeInTheDocument();
    });

    it("opens on the source the store was given", () => {
      sessionsRootsStore.setState({ activeRootId: "r1" });
      searchStore.setState({ isOpen: true, scope: "global", source: "sessions" });
      render(<SearchOverlay />);
      expect(screen.getByPlaceholderText("Search sessions")).toBeInTheDocument();
      expect(screen.getByRole("tab", { name: "Sessions" })).toHaveAttribute(
        "aria-selected",
        "true",
      );
    });

    it("shows no switcher in chat scope — the lock is the point of that surface", () => {
      roomsStore.setState({
        rooms: [room({ accountId: "a1", roomId: "!r1:x", displayName: "Design" })],
        selected: { accountId: "a1", roomId: "!r1:x" },
      });
      searchStore.setState({ isOpen: true, scope: "chat", source: "messages" });
      render(<SearchOverlay />);
      expect(screen.queryByRole("tablist", { name: "Search source" })).not.toBeInTheDocument();
      expect(screen.getByText("Searching your local archive")).toBeInTheDocument();
    });
  });
});
