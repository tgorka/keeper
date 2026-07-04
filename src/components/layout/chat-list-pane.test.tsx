import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, InboxBatch, IpcError } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { roomsStore } from "@/lib/stores/rooms";

// Mock the typed IPC wrapper so the pane never touches Tauri. `subscribeInbox`
// captures the `onBatch` handler so the test can drive the merged stream.
const subscribeInbox = vi.fn();
const unsubscribeInbox = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  subscribeInbox: (onBatch: (b: InboxBatch) => void) => subscribeInbox(onBatch),
  unsubscribeInbox: (id: number) => unsubscribeInbox(id),
}));

import { ChatListPane } from "@/components/layout/chat-list-pane";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 0,
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
  };
}

beforeEach(() => {
  accountsStore.getState().clear();
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  subscribeInbox.mockReset();
  unsubscribeInbox.mockReset();
});

afterEach(() => {
  accountsStore.getState().clear();
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
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
    accountsStore.getState().hydrateAll([
      account,
      {
        accountId: "01BX5ZZKBKACTAV9WEVGEMMVRZ",
        userId: "@bob:example.org",
        homeserverUrl: "https://matrix.example.org/",
        hueIndex: 1,
      },
    ]);
    render(<ChatListPane />);

    expect(subscribeInbox).toHaveBeenCalledWith(expect.any(Function));

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
});
