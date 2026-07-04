import { render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, IpcError, TimelineBatch } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { roomsStore } from "@/lib/stores/rooms";
import { timelineStore } from "@/lib/stores/timeline";

// Mock the typed IPC wrapper so the pane never touches Tauri. `subscribeTimeline`
// captures the `onBatch` handler so the test can drive the stream.
const subscribeTimeline = vi.fn();
const unsubscribeTimeline = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  subscribeTimeline: (accountId: string, roomId: string, onBatch: (b: TimelineBatch) => void) =>
    subscribeTimeline(accountId, roomId, onBatch),
  unsubscribeTimeline: (accountId: string, id: number) => unsubscribeTimeline(accountId, id),
}));

import { ConversationPane } from "@/components/layout/conversation-pane";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
};

function ipcError(code: IpcError["code"]): IpcError {
  return { code, message: "ignored", accountId: null, retriable: true };
}

function message(key: string, sender: string, body: string): TimelineBatch["ops"][number] {
  return {
    op: "pushBack",
    item: {
      kind: "message",
      key,
      sender,
      senderDisplayName: null,
      body,
      timestamp: 1,
      isOwn: false,
    },
  };
}

function noopProps() {
  return { detailOpen: false, onToggleDetail: () => {} };
}

beforeEach(() => {
  accountsStore.getState().clear();
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  timelineStore.getState().clear();
  subscribeTimeline.mockReset();
  unsubscribeTimeline.mockReset();
});

afterEach(() => {
  accountsStore.getState().clear();
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  timelineStore.getState().clear();
});

describe("ConversationPane", () => {
  it("shows the placeholder when no room is selected", () => {
    accountsStore.getState().setCurrentAccount(account);
    render(<ConversationPane {...noopProps()} />);
    expect(screen.getByText("Select a conversation to start reading.")).toBeInTheDocument();
    expect(subscribeTimeline).not.toHaveBeenCalled();
  });

  it("subscribes with the account id and room id when a room is selected", async () => {
    subscribeTimeline.mockResolvedValue(1);
    accountsStore.getState().setCurrentAccount(account);
    roomsStore.getState().selectRoom("!room:example.org");
    render(<ConversationPane {...noopProps()} />);

    await waitFor(() => {
      expect(subscribeTimeline).toHaveBeenCalledWith(
        account.accountId,
        "!room:example.org",
        expect.any(Function),
      );
    });
  });

  it("renders streamed text messages as bubbles", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    accountsStore.getState().setCurrentAccount(account);
    roomsStore.getState().selectRoom("!room:example.org");
    render(<ConversationPane {...noopProps()} />);

    captured.onBatch?.({
      ops: [
        {
          op: "reset",
          items: [
            {
              kind: "message",
              key: "k1",
              sender: "@bob:example.org",
              senderDisplayName: "Bob",
              body: "hi from bob",
              timestamp: 1,
              isOwn: false,
            },
            { kind: "other", key: "o1" },
          ],
        },
      ],
    });

    await waitFor(() => {
      expect(screen.getByText("hi from bob")).toBeInTheDocument();
    });
    // The `Other` item is not rendered as a bubble.
    expect(screen.getByLabelText("Messages")).toBeInTheDocument();
  });

  it("groups consecutive same-sender messages under a single name", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    accountsStore.getState().setCurrentAccount(account);
    roomsStore.getState().selectRoom("!room:example.org");
    render(<ConversationPane {...noopProps()} />);

    captured.onBatch?.({
      ops: [
        {
          op: "reset",
          items: [
            {
              kind: "message",
              key: "k1",
              sender: "@bob:example.org",
              senderDisplayName: "Bob",
              body: "first",
              timestamp: 1,
              isOwn: false,
            },
            {
              kind: "message",
              key: "k2",
              sender: "@bob:example.org",
              senderDisplayName: "Bob",
              body: "second",
              timestamp: 2,
              isOwn: false,
            },
          ],
        },
      ],
    });

    await waitFor(() => {
      expect(screen.getByText("first")).toBeInTheDocument();
    });
    // Only the first bubble in the run shows the sender name.
    expect(screen.getAllByText("Bob")).toHaveLength(1);
  });

  it("shows the empty state when the reset has no renderable messages", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    accountsStore.getState().setCurrentAccount(account);
    roomsStore.getState().selectRoom("!room:example.org");
    render(<ConversationPane {...noopProps()} />);

    captured.onBatch?.({ ops: [{ op: "reset", items: [{ kind: "other", key: "o1" }] }] });

    await waitFor(() => {
      expect(screen.getByText("No messages yet.")).toBeInTheDocument();
    });
  });

  it("shows an inline error when the subscribe rejects", async () => {
    subscribeTimeline.mockRejectedValue(ipcError("timelineUnavailable"));
    accountsStore.getState().setCurrentAccount(account);
    roomsStore.getState().selectRoom("!room:example.org");
    render(<ConversationPane {...noopProps()} />);

    await waitFor(() => {
      expect(screen.getByText(/Couldn't open this conversation/)).toBeInTheDocument();
    });
  });

  it("unsubscribes and clears the store on unmount", async () => {
    subscribeTimeline.mockResolvedValue(7);
    accountsStore.getState().setCurrentAccount(account);
    roomsStore.getState().selectRoom("!room:example.org");
    const { unmount } = render(<ConversationPane {...noopProps()} />);

    await waitFor(() => {
      expect(subscribeTimeline).toHaveBeenCalled();
    });

    unmount();

    await waitFor(() => {
      expect(unsubscribeTimeline).toHaveBeenCalledWith(account.accountId, 7);
    });
    expect(timelineStore.getState().items).toEqual([]);
  });

  it("tears down the old subscription and re-subscribes when the room changes", async () => {
    let nextId = 1;
    subscribeTimeline.mockImplementation(() => Promise.resolve(nextId++));
    accountsStore.getState().setCurrentAccount(account);
    roomsStore.getState().selectRoom("!room-a:example.org");
    const { rerender } = render(<ConversationPane {...noopProps()} />);

    await waitFor(() => {
      expect(subscribeTimeline).toHaveBeenCalledWith(
        account.accountId,
        "!room-a:example.org",
        expect.any(Function),
      );
    });

    // Switch rooms: the effect re-runs (selectedRoomId changed).
    roomsStore.getState().selectRoom("!room-b:example.org");
    rerender(<ConversationPane {...noopProps()} />);

    await waitFor(() => {
      expect(unsubscribeTimeline).toHaveBeenCalledWith(account.accountId, 1);
    });
    await waitFor(() => {
      expect(subscribeTimeline).toHaveBeenCalledWith(
        account.accountId,
        "!room-b:example.org",
        expect.any(Function),
      );
    });
  });

  it("ignores a batch delivered after effect cleanup", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    accountsStore.getState().setCurrentAccount(account);
    roomsStore.getState().selectRoom("!room:example.org");
    const { unmount } = render(<ConversationPane {...noopProps()} />);

    await waitFor(() => {
      expect(subscribeTimeline).toHaveBeenCalled();
    });

    unmount();

    // A late batch (arriving after cleanup) must not mutate the store.
    captured.onBatch?.({ ops: [message("late", "@bob:example.org", "late body")] });
    expect(timelineStore.getState().items).toEqual([]);
  });
});
