import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, IpcError, RoomListBatch } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { roomsStore } from "@/lib/stores/rooms";

// Mock the typed IPC wrapper so the pane never touches Tauri. `subscribeRoomList`
// captures the `onBatch` handler so the test can drive the stream.
const subscribeRoomList = vi.fn();
const unsubscribeRoomList = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  subscribeRoomList: (accountId: string, onBatch: (b: RoomListBatch) => void) =>
    subscribeRoomList(accountId, onBatch),
  unsubscribeRoomList: (accountId: string, id: number) => unsubscribeRoomList(accountId, id),
}));

import { ChatListPane } from "@/components/layout/chat-list-pane";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
};

function ipcError(code: IpcError["code"]): IpcError {
  return { code, message: "ignored", accountId: null, retriable: true };
}

beforeEach(() => {
  accountsStore.getState().clear();
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  subscribeRoomList.mockReset();
  unsubscribeRoomList.mockReset();
});

afterEach(() => {
  accountsStore.getState().clear();
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
});

describe("ChatListPane", () => {
  it("shows the loading skeleton before the first batch arrives", () => {
    subscribeRoomList.mockResolvedValue(1);
    accountsStore.getState().setCurrentAccount(account);
    render(<ChatListPane />);
    // No batch has been delivered yet: neither the list nor the empty state.
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
    expect(screen.queryByText("No conversations yet.")).not.toBeInTheDocument();
  });

  it("shows the empty state after a batch delivers no rooms", async () => {
    const captured: { onBatch: ((b: RoomListBatch) => void) | null } = { onBatch: null };
    subscribeRoomList.mockImplementation((_accountId, onBatch: (b: RoomListBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    accountsStore.getState().setCurrentAccount(account);
    render(<ChatListPane />);

    // An empty Reset batch marks the list as loaded with zero rooms.
    captured.onBatch?.({ ops: [{ op: "reset", rooms: [] }], total: 0 });

    await waitFor(() => {
      expect(screen.getByText("No conversations yet.")).toBeInTheDocument();
    });
    expect(screen.queryByLabelText("Loading conversations")).not.toBeInTheDocument();
  });

  it("subscribes with the current account id and renders streamed rows", async () => {
    const captured: { onBatch: ((b: RoomListBatch) => void) | null } = { onBatch: null };
    subscribeRoomList.mockImplementation((_accountId, onBatch: (b: RoomListBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    accountsStore.getState().setCurrentAccount(account);
    render(<ChatListPane />);

    expect(subscribeRoomList).toHaveBeenCalledWith(account.accountId, expect.any(Function));

    // Drive a Reset snapshot batch through the captured handler.
    captured.onBatch?.({
      ops: [
        {
          op: "reset",
          rooms: [
            {
              roomId: "!a:example.org",
              displayName: "Alpha Room",
              lastMessage: "first",
              timestamp: null,
              avatarUrl: null,
            },
          ],
        },
      ],
      total: 1,
    });

    await waitFor(() => {
      expect(screen.getByText("Alpha Room")).toBeInTheDocument();
    });
    expect(screen.getByText("first")).toBeInTheDocument();
  });

  it("selects a room and highlights it when a row is clicked", async () => {
    const captured: { onBatch: ((b: RoomListBatch) => void) | null } = { onBatch: null };
    subscribeRoomList.mockImplementation((_accountId, onBatch: (b: RoomListBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    accountsStore.getState().setCurrentAccount(account);
    render(<ChatListPane />);

    captured.onBatch?.({
      ops: [
        {
          op: "reset",
          rooms: [
            {
              roomId: "!a:example.org",
              displayName: "Alpha Room",
              lastMessage: null,
              timestamp: null,
              avatarUrl: null,
            },
          ],
        },
      ],
      total: 1,
    });

    await waitFor(() => {
      expect(screen.getByText("Alpha Room")).toBeInTheDocument();
    });

    fireEvent.click(screen.getByRole("button", { name: "Conversation with Alpha Room" }));

    expect(roomsStore.getState().selectedRoomId).toBe("!a:example.org");
    await waitFor(() => {
      expect(screen.getByRole("button", { name: "Conversation with Alpha Room" })).toHaveAttribute(
        "aria-current",
        "true",
      );
    });
  });

  it("unsubscribes and clears the store on unmount", async () => {
    subscribeRoomList.mockResolvedValue(7);
    accountsStore.getState().setCurrentAccount(account);
    const { unmount } = render(<ChatListPane />);

    await waitFor(() => {
      expect(subscribeRoomList).toHaveBeenCalled();
    });

    unmount();

    await waitFor(() => {
      expect(unsubscribeRoomList).toHaveBeenCalledWith(account.accountId, 7);
    });
    expect(roomsStore.getState().rooms).toEqual([]);
  });

  it("shows an inline error when activation fails with syncUnavailable", async () => {
    subscribeRoomList.mockRejectedValue(ipcError("syncUnavailable"));
    accountsStore.getState().setCurrentAccount(account);
    render(<ChatListPane />);

    await waitFor(() => {
      expect(screen.getByText(/Couldn't start syncing/)).toBeInTheDocument();
    });
  });

  it("does not subscribe when there is no account", () => {
    render(<ChatListPane />);
    expect(subscribeRoomList).not.toHaveBeenCalled();
    // With no account and no batch, the pane sits in its loading state.
    expect(screen.getByLabelText("Loading conversations")).toBeInTheDocument();
  });

  it("ignores a batch delivered after effect cleanup", async () => {
    const captured: { onBatch: ((b: RoomListBatch) => void) | null } = { onBatch: null };
    subscribeRoomList.mockImplementation((_accountId, onBatch: (b: RoomListBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    accountsStore.getState().setCurrentAccount(account);
    const { unmount } = render(<ChatListPane />);

    await waitFor(() => {
      expect(subscribeRoomList).toHaveBeenCalled();
    });

    unmount();

    // A late batch (arriving after cleanup) must not mutate the store.
    captured.onBatch?.({
      ops: [
        {
          op: "reset",
          rooms: [
            {
              roomId: "!late:example.org",
              displayName: "Late Room",
              lastMessage: null,
              timestamp: null,
              avatarUrl: null,
            },
          ],
        },
      ],
      total: 1,
    });

    expect(roomsStore.getState().rooms).toEqual([]);
  });
});
