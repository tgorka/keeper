import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, InboxRoomVm, IpcError, SearchHitVm } from "@/lib/ipc/client";

// Mock the typed IPC client so the panel never touches Tauri.
const searchArchive = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  searchArchive: (filter: unknown) => searchArchive(filter),
}));

import { SearchPanel } from "@/components/search/search-panel";
import { accountsStore } from "@/lib/stores/accounts";
import { networksStore } from "@/lib/stores/networks";
import { roomsStore } from "@/lib/stores/rooms";

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
  roomsStore.setState({ rooms: [], selected: null, focusEvent: null });
  accountsStore.setState({ accounts: [] });
  networksStore.setState({ networks: [] });
});

afterEach(() => {
  vi.clearAllMocks();
});

async function type(text: string) {
  const input = screen.getByLabelText("Search query");
  fireEvent.change(input, { target: { value: text } });
  return input;
}

describe("SearchPanel", () => {
  it("shows the honest header and offline note", () => {
    render(<SearchPanel active scope="global" chatLock={null} onClose={vi.fn()} />);
    expect(screen.getByText("Searching your local archive")).toBeInTheDocument();
    expect(screen.getByText(/works fully offline/i)).toBeInTheDocument();
  });

  it("debounces a query into searchArchive and renders grouped results", async () => {
    roomsStore.setState({
      rooms: [room({ accountId: "a1", roomId: "!r1:x", displayName: "Design", hueIndex: 2 })],
    });
    accountsStore.setState({ accounts: [account("a1", "@me:x", 2)] });
    searchArchive.mockResolvedValue([
      hit({ accountId: "a1", roomId: "!r1:x", eventId: "$e1", body: "hello there" }),
      hit({ accountId: "a1", roomId: "!r1:x", eventId: "$e2", body: "well hello" }),
    ]);
    render(<SearchPanel active scope="global" chatLock={null} onClose={vi.fn()} />);
    await type("hello");

    await waitFor(() => expect(searchArchive).toHaveBeenCalled());
    await waitFor(() => expect(screen.getByText("Design")).toBeInTheDocument());
    expect(screen.getAllByRole("option")).toHaveLength(2);
    const marks = document.querySelectorAll("mark.bg-search-highlight");
    expect(marks.length).toBeGreaterThanOrEqual(2);
  });

  it("deep-links on Enter and closes the surface", async () => {
    const onClose = vi.fn();
    const requestFocus = vi.fn();
    roomsStore.setState({
      rooms: [room({ accountId: "a1", roomId: "!r1:x", displayName: "Design" })],
      requestFocus,
    });
    searchArchive.mockResolvedValue([hit({ accountId: "a1", roomId: "!r1:x", eventId: "$e1" })]);
    render(<SearchPanel active scope="global" chatLock={null} onClose={onClose} />);
    const input = await type("hello");
    await waitFor(() => expect(screen.getByText("Design")).toBeInTheDocument());

    fireEvent.keyDown(input, { key: "Enter" });
    expect(requestFocus).toHaveBeenCalledWith({
      accountId: "a1",
      roomId: "!r1:x",
      eventId: "$e1",
    });
    expect(onClose).toHaveBeenCalledTimes(1);
  });

  it("locks the surface to the given Chat in chat scope", async () => {
    roomsStore.setState({
      rooms: [room({ accountId: "a1", roomId: "!r1:x", displayName: "Design" })],
    });
    searchArchive.mockResolvedValue([]);
    render(
      <SearchPanel
        active
        scope="chat"
        chatLock={{ accountId: "a1", roomId: "!r1:x" }}
        onClose={vi.fn()}
      />,
    );
    await type("hello");

    expect(screen.getByLabelText("Chat filter (locked)")).toHaveTextContent("Design");
    await waitFor(() => expect(searchArchive).toHaveBeenCalled());
    const calls = searchArchive.mock.calls;
    const filter = calls[calls.length - 1]?.[0];
    expect(filter.roomIds).toEqual(["!r1:x"]);
    expect(filter.accountIds).toEqual(["a1"]);
  });

  it("surfaces an honest inline error on an IpcError and keeps the offline note", async () => {
    const err: IpcError = {
      code: "internal",
      message: "archive read failed",
      accountId: null,
      retriable: true,
    };
    searchArchive.mockRejectedValue(err);
    render(<SearchPanel active scope="global" chatLock={null} onClose={vi.fn()} />);
    await type("hello");

    await waitFor(() => expect(screen.getByRole("alert")).toHaveTextContent(/archive read failed/));
    expect(screen.getByText(/works fully offline/i)).toBeInTheDocument();
  });

  it("makes no call while inactive", async () => {
    render(<SearchPanel active={false} scope="global" chatLock={null} onClose={vi.fn()} />);
    await type("hello");
    await new Promise((r) => setTimeout(r, 250));
    expect(searchArchive).not.toHaveBeenCalled();
  });
});
