import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, InboxBatch, IpcError } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { archiveRoomsStore } from "@/lib/stores/archive-rooms";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";

// Mock the typed IPC wrapper so the pane never touches Tauri. `subscribeInbox`
// captures the `onInbox`/`onArchive` handlers so the test can drive both windows
// of the merged stream (Story 4.2).
const subscribeInbox = vi.fn();
const unsubscribeInbox = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  subscribeInbox: (
    onInbox: (b: InboxBatch) => void,
    onArchive: (b: InboxBatch) => void,
    onPins: (b: InboxBatch) => void,
  ) => subscribeInbox(onInbox, onArchive, onPins),
  unsubscribeInbox: (id: number) => unsubscribeInbox(id),
  // Best-effort mutation wrappers the strip/rows may call; no-ops here.
  reorderPins: vi.fn(async () => {}),
  unpinRoom: vi.fn(async () => {}),
  pinRoom: vi.fn(async () => {}),
  markRoomRead: vi.fn(async () => {}),
  markRoomUnread: vi.fn(async () => {}),
  archiveRoom: vi.fn(async () => {}),
  unarchiveRoom: vi.fn(async () => {}),
}));

import { ChatListPane } from "@/components/layout/chat-list-pane";
import { pinsRoomsStore } from "@/lib/stores/pins-rooms";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 0,
  provider: "password",
};

const bob: AccountVm = {
  accountId: "01BX5ZZKBKACTAV9WEVGEMMVRZ",
  userId: "@bob:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 1,
  provider: "password",
};

function ipcError(code: IpcError["code"]): IpcError {
  return { code, message: "ignored", accountId: null, retriable: true };
}

function inboxRoom(roomId: string, accountId: string, displayName: string, lastMessage: string) {
  return {
    accountId,
    hueIndex: 0,
    roomId,
    displayName,
    lastMessage,
    timestamp: null,
    avatarUrl: null,
    isUnread: false,
    mentionCount: 0,
    isArchived: false,
    isPinned: false,
  };
}

beforeEach(() => {
  accountsStore.getState().clear();
  accountsStore.setState({ filterAccountId: null });
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  archiveRoomsStore.getState().clear();
  pinsRoomsStore.getState().clear();
  primaryViewStore.getState().setView("inbox");
  subscribeInbox.mockReset();
  unsubscribeInbox.mockReset();
});

afterEach(() => {
  accountsStore.getState().clear();
  accountsStore.setState({ filterAccountId: null });
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  archiveRoomsStore.getState().clear();
  pinsRoomsStore.getState().clear();
  primaryViewStore.getState().setView("inbox");
});

describe("ChatListPane", () => {
  it("shows the loading skeleton before the first batch arrives", () => {
    subscribeInbox.mockResolvedValue(1);
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
    expect(screen.queryByText("No conversations yet.")).not.toBeInTheDocument();
  });

  it("shows the empty state after a batch delivers no rooms", async () => {
    const captured: { onBatch: ((b: InboxBatch) => void) | null } = { onBatch: null };
    subscribeInbox.mockImplementation((onBatch: (b: InboxBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);

    captured.onBatch?.({ ops: [{ op: "reset", rooms: [] }], total: 0 });

    await waitFor(() => {
      expect(screen.getByText("No conversations yet.")).toBeInTheDocument();
    });
    expect(screen.queryByLabelText("Loading conversations")).not.toBeInTheDocument();
  });

  it("subscribes to the merged inbox and renders streamed rows from multiple accounts", async () => {
    const captured: { onBatch: ((b: InboxBatch) => void) | null } = { onBatch: null };
    subscribeInbox.mockImplementation((onBatch: (b: InboxBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    accountsStore.getState().hydrateAll([account, bob]);
    render(<ChatListPane />);

    expect(subscribeInbox).toHaveBeenCalledWith(
      expect.any(Function),
      expect.any(Function),
      expect.any(Function),
    );

    captured.onBatch?.({
      ops: [
        {
          op: "reset",
          rooms: [
            inboxRoom("!a:example.org", account.accountId, "Alpha Room", "first"),
            inboxRoom("!b:example.org", "01BX5ZZKBKACTAV9WEVGEMMVRZ", "Beta Room", "second"),
          ],
        },
      ],
      total: 2,
    });

    await waitFor(() => {
      expect(screen.getByText("Alpha Room")).toBeInTheDocument();
    });
    expect(screen.getByText("Beta Room")).toBeInTheDocument();
  });

  it("selects a row by its account + room ids and highlights it", async () => {
    const captured: { onBatch: ((b: InboxBatch) => void) | null } = { onBatch: null };
    subscribeInbox.mockImplementation((onBatch: (b: InboxBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);

    captured.onBatch?.({
      ops: [
        { op: "reset", rooms: [inboxRoom("!a:example.org", account.accountId, "Alpha Room", "")] },
      ],
      total: 1,
    });

    await waitFor(() => {
      expect(screen.getByText("Alpha Room")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Conversation with Alpha Room" }));

    expect(roomsStore.getState().selected).toEqual({
      accountId: account.accountId,
      roomId: "!a:example.org",
    });
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Conversation with Alpha Room" })).toHaveAttribute(
        "aria-current",
        "true",
      );
    });
  });

  it("unsubscribes and clears the store on unmount", async () => {
    subscribeInbox.mockResolvedValue(7);
    accountsStore.getState().addAccount(account);
    const { unmount } = render(<ChatListPane />);

    await waitFor(() => {
      expect(subscribeInbox).toHaveBeenCalled();
    });

    unmount();

    await waitFor(() => {
      expect(unsubscribeInbox).toHaveBeenCalledWith(7);
    });
    expect(roomsStore.getState().rooms).toEqual([]);
  });

  it("shows an inline error when the merged subscribe fails with syncUnavailable", async () => {
    subscribeInbox.mockRejectedValue(ipcError("syncUnavailable"));
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);

    await waitFor(() => {
      expect(screen.getByText(/Couldn't start syncing/)).toBeInTheDocument();
    });
  });

  it("does not subscribe when there are no accounts", () => {
    render(<ChatListPane />);
    expect(subscribeInbox).not.toHaveBeenCalled();
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
  });

  it("ignores a batch delivered after effect cleanup", async () => {
    const captured: { onBatch: ((b: InboxBatch) => void) | null } = { onBatch: null };
    subscribeInbox.mockImplementation((onBatch: (b: InboxBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    accountsStore.getState().addAccount(account);
    const { unmount } = render(<ChatListPane />);

    await waitFor(() => {
      expect(subscribeInbox).toHaveBeenCalled();
    });

    unmount();

    captured.onBatch?.({
      ops: [
        { op: "reset", rooms: [inboxRoom("!late:example.org", account.accountId, "Late", "")] },
      ],
      total: 1,
    });

    expect(roomsStore.getState().rooms).toEqual([]);
  });

  it("applies the account filter as a pure display filter, hiding other accounts' rooms", async () => {
    const captured: { onBatch: ((b: InboxBatch) => void) | null } = { onBatch: null };
    subscribeInbox.mockImplementation((onBatch: (b: InboxBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    accountsStore.getState().hydrateAll([account, bob]);
    render(<ChatListPane />);

    captured.onBatch?.({
      ops: [
        {
          op: "reset",
          rooms: [
            inboxRoom("!a:example.org", account.accountId, "Alpha Room", "first"),
            inboxRoom("!b:example.org", bob.accountId, "Beta Room", "second"),
          ],
        },
      ],
      total: 2,
    });

    await waitFor(() => {
      expect(screen.getByText("Alpha Room")).toBeInTheDocument();
    });
    expect(screen.getByText("Beta Room")).toBeInTheDocument();

    // Filter to account (alice): only its room stays; the merged subscription is
    // untouched (no re-subscribe).
    accountsStore.getState().toggleFilter(account.accountId);
    await waitFor(() => {
      expect(screen.queryByText("Beta Room")).not.toBeInTheDocument();
    });
    expect(screen.getByText("Alpha Room")).toBeInTheDocument();
    expect(subscribeInbox).toHaveBeenCalledTimes(1);

    // Clearing the filter shows every account's rooms again.
    accountsStore.getState().toggleFilter(account.accountId);
    await waitFor(() => {
      expect(screen.getByText("Beta Room")).toBeInTheDocument();
    });
  });

  it("renders the archive window and empty-state text when the primary view is archive", async () => {
    const captured: {
      onInbox: ((b: InboxBatch) => void) | null;
      onArchive: ((b: InboxBatch) => void) | null;
    } = { onInbox: null, onArchive: null };
    subscribeInbox.mockImplementation(
      (onInbox: (b: InboxBatch) => void, onArchive: (b: InboxBatch) => void) => {
        captured.onInbox = onInbox;
        captured.onArchive = onArchive;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);

    // Feed the inbox window one row and the archive window another.
    captured.onInbox?.({
      ops: [
        { op: "reset", rooms: [inboxRoom("!a:example.org", account.accountId, "Inbox Room", "")] },
      ],
      total: 1,
    });
    captured.onArchive?.({
      ops: [
        {
          op: "reset",
          rooms: [inboxRoom("!z:example.org", account.accountId, "Archived Room", "")],
        },
      ],
      total: 1,
    });

    // Inbox view (default) shows the inbox row, not the archive row.
    await waitFor(() => {
      expect(screen.getByText("Inbox Room")).toBeInTheDocument();
    });
    expect(screen.queryByText("Archived Room")).not.toBeInTheDocument();

    // Switching to the archive view renders the archive row instead.
    primaryViewStore.getState().setView("archive");
    await waitFor(() => {
      expect(screen.getByText("Archived Room")).toBeInTheDocument();
    });
    expect(screen.queryByText("Inbox Room")).not.toBeInTheDocument();
  });

  it("shows the archive empty-state text when the archive window is empty", async () => {
    const captured: {
      onInbox: ((b: InboxBatch) => void) | null;
      onArchive: ((b: InboxBatch) => void) | null;
    } = { onInbox: null, onArchive: null };
    subscribeInbox.mockImplementation(
      (onInbox: (b: InboxBatch) => void, onArchive: (b: InboxBatch) => void) => {
        captured.onInbox = onInbox;
        captured.onArchive = onArchive;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    primaryViewStore.getState().setView("archive");
    render(<ChatListPane />);

    captured.onArchive?.({ ops: [{ op: "reset", rooms: [] }], total: 0 });

    await waitFor(() => {
      expect(screen.getByText(/Nothing archived\./)).toBeInTheDocument();
    });
    // The code-font `E` verb (UX-DR13).
    expect(screen.getByText("E")).toBeInTheDocument();
    expect(screen.queryByText("No conversations yet.")).not.toBeInTheDocument();
  });

  it("keeps the archive skeleton (not a premature empty-state) until the archive window itself loads", async () => {
    const captured: {
      onInbox: ((b: InboxBatch) => void) | null;
      onArchive: ((b: InboxBatch) => void) | null;
    } = { onInbox: null, onArchive: null };
    subscribeInbox.mockImplementation(
      (onInbox: (b: InboxBatch) => void, onArchive: (b: InboxBatch) => void) => {
        captured.onInbox = onInbox;
        captured.onArchive = onArchive;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    primaryViewStore.getState().setView("archive");
    render(<ChatListPane />);

    // Only the inbox window has delivered; the archive channel has not emitted
    // yet. The archive view must still show its loading skeleton, never the
    // "Nothing archived." empty-state (per-window loaded gating).
    captured.onInbox?.({
      ops: [
        { op: "reset", rooms: [inboxRoom("!a:example.org", account.accountId, "Inbox Room", "")] },
      ],
      total: 1,
    });

    await waitFor(() => {
      expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
    });
    expect(screen.queryByText(/Nothing archived\./)).not.toBeInTheDocument();

    // Once the archive window delivers (empty), the empty-state replaces the skeleton.
    captured.onArchive?.({ ops: [{ op: "reset", rooms: [] }], total: 0 });
    await waitFor(() => {
      expect(screen.getByText(/Nothing archived\./)).toBeInTheDocument();
    });
  });

  it("feeds the third channel into the pins store and renders the strip in the inbox view", async () => {
    const captured: { onPins: ((b: InboxBatch) => void) | null } = { onPins: null };
    subscribeInbox.mockImplementation(
      (
        _onInbox: (b: InboxBatch) => void,
        _onArchive: (b: InboxBatch) => void,
        onPins: (b: InboxBatch) => void,
      ) => {
        captured.onPins = onPins;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);

    // No pins yet → the strip is hidden entirely.
    expect(screen.queryByLabelText("Pinned conversations")).not.toBeInTheDocument();

    // The pins channel delivers one pinned room.
    captured.onPins?.({
      ops: [
        {
          op: "reset",
          rooms: [
            {
              ...inboxRoom("!p:example.org", account.accountId, "Pinned Room", ""),
              isPinned: true,
            },
          ],
        },
      ],
      total: 1,
    });

    await waitFor(() => {
      expect(screen.getByLabelText("Pinned conversations")).toBeInTheDocument();
    });
    expect(
      screen.getByRole("button", { name: "Pinned conversation with Pinned Room" }),
    ).toBeInTheDocument();
    // The store mirrors the window.
    expect(pinsRoomsStore.getState().rooms).toHaveLength(1);
  });

  it("hides the pins strip in the archive view", async () => {
    const captured: { onPins: ((b: InboxBatch) => void) | null } = { onPins: null };
    subscribeInbox.mockImplementation(
      (
        _onInbox: (b: InboxBatch) => void,
        _onArchive: (b: InboxBatch) => void,
        onPins: (b: InboxBatch) => void,
      ) => {
        captured.onPins = onPins;
        return Promise.resolve(1);
      },
    );
    accountsStore.getState().addAccount(account);
    render(<ChatListPane />);

    captured.onPins?.({
      ops: [
        {
          op: "reset",
          rooms: [
            {
              ...inboxRoom("!p:example.org", account.accountId, "Pinned Room", ""),
              isPinned: true,
            },
          ],
        },
      ],
      total: 1,
    });

    await waitFor(() => {
      expect(screen.getByLabelText("Pinned conversations")).toBeInTheDocument();
    });

    // Switch to the archive view: the strip is hidden (it lives atop the inbox only).
    primaryViewStore.getState().setView("archive");
    await waitFor(() => {
      expect(screen.queryByLabelText("Pinned conversations")).not.toBeInTheDocument();
    });
  });
});
