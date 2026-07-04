import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, IpcError, TimelineBatch } from "@/lib/ipc/client";
import { accountStatusStore } from "@/lib/stores/account-status";
import { accountsStore } from "@/lib/stores/accounts";
import { roomsStore } from "@/lib/stores/rooms";
import { timelineStore } from "@/lib/stores/timeline";

// Mock the typed IPC wrapper so the pane never touches Tauri. `subscribeTimeline`
// captures the `onBatch` handler so the test can drive the stream.
const subscribeTimeline = vi.fn();
const unsubscribeTimeline = vi.fn();
const sendText = vi.fn();
const sendReply = vi.fn();
const editMessage = vi.fn();
const retrySend = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  subscribeTimeline: (accountId: string, roomId: string, onBatch: (b: TimelineBatch) => void) =>
    subscribeTimeline(accountId, roomId, onBatch),
  unsubscribeTimeline: (accountId: string, id: number) => unsubscribeTimeline(accountId, id),
  sendText: (accountId: string, roomId: string, body: string) => sendText(accountId, roomId, body),
  sendReply: (accountId: string, roomId: string, inReplyToKey: string, body: string) =>
    sendReply(accountId, roomId, inReplyToKey, body),
  editMessage: (accountId: string, roomId: string, itemKey: string, body: string) =>
    editMessage(accountId, roomId, itemKey, body),
  retrySend: (accountId: string, roomId: string, itemKey: string) =>
    retrySend(accountId, roomId, itemKey),
}));

import { ConversationPane } from "@/components/layout/conversation-pane";
import { composerStore } from "@/lib/stores/composer";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 0,
  provider: "password",
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
      sendState: null,
      isEdited: false,
      reply: null,
      reactions: [],
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
  accountStatusStore.getState().reset();
  subscribeTimeline.mockReset();
  unsubscribeTimeline.mockReset();
  sendText.mockReset();
  sendText.mockResolvedValue(undefined);
  sendReply.mockReset();
  sendReply.mockResolvedValue(undefined);
  editMessage.mockReset();
  editMessage.mockResolvedValue(undefined);
  retrySend.mockReset();
  retrySend.mockResolvedValue(undefined);
  composerStore.getState().clear();
  composerStore.getState().clearSelection();
});

afterEach(() => {
  accountsStore.getState().clear();
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  timelineStore.getState().clear();
  accountStatusStore.getState().reset();
  composerStore.getState().clear();
  composerStore.getState().clearSelection();
});

describe("ConversationPane", () => {
  it("shows the placeholder when no room is selected", () => {
    render(<ConversationPane {...noopProps()} />);
    expect(screen.getByText("Select a conversation to start reading.")).toBeInTheDocument();
    expect(subscribeTimeline).not.toHaveBeenCalled();
  });

  it("subscribes with the account id and room id when a room is selected", async () => {
    subscribeTimeline.mockResolvedValue(1);
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
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
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
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
              sendState: null,
              isEdited: false,
              reply: null,
              reactions: [],
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

  it("renders a streamed UTD item as an honest stub (never blank)", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);

    captured.onBatch?.({
      ops: [
        {
          op: "reset",
          items: [
            {
              kind: "utd",
              key: "u1",
              sender: "@carol:example.org",
              senderDisplayName: "Carol",
              timestamp: 1,
            },
          ],
        },
      ],
    });

    await waitFor(() => {
      expect(
        screen.getByText("Can't decrypt yet — verify this device or restore key backup"),
      ).toBeInTheDocument();
    });
    // The stub carries a working inline Verify affordance.
    expect(screen.getByRole("button", { name: "Verify" })).toBeInTheDocument();
  });

  it("replaces a UTD stub with the decrypted message when keys arrive (Set diff)", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);

    // An undecryptable event first renders as the honest stub.
    captured.onBatch?.({
      ops: [
        {
          op: "reset",
          items: [
            {
              kind: "utd",
              key: "u1",
              sender: "@carol:example.org",
              senderDisplayName: "Carol",
              timestamp: 1,
            },
          ],
        },
      ],
    });
    await waitFor(() => {
      expect(
        screen.getByText("Can't decrypt yet — verify this device or restore key backup"),
      ).toBeInTheDocument();
    });

    // Keys arrive: the SDK retries decryption and re-maps the item in place via a
    // `Set` diff at the same index/key — no extra client code, the stub self-heals.
    captured.onBatch?.({
      ops: [
        {
          op: "set",
          index: 0,
          item: {
            kind: "message",
            key: "u1",
            sender: "@carol:example.org",
            senderDisplayName: "Carol",
            body: "now decrypted",
            timestamp: 1,
            isOwn: false,
            sendState: null,
            isEdited: false,
            reply: null,
            reactions: [],
          },
        },
      ],
    });

    await waitFor(() => {
      expect(screen.getByText("now decrypted")).toBeInTheDocument();
    });
    // The stub is gone — it was transient, replaced by the decrypted bubble.
    expect(
      screen.queryByText("Can't decrypt yet — verify this device or restore key backup"),
    ).not.toBeInTheDocument();
  });

  it("groups consecutive same-sender messages under a single name", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
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
              sendState: null,
              isEdited: false,
              reply: null,
              reactions: [],
            },
            {
              kind: "message",
              key: "k2",
              sender: "@bob:example.org",
              senderDisplayName: "Bob",
              body: "second",
              timestamp: 2,
              isOwn: false,
              sendState: null,
              isEdited: false,
              reply: null,
              reactions: [],
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
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);

    captured.onBatch?.({ ops: [{ op: "reset", items: [{ kind: "other", key: "o1" }] }] });

    await waitFor(() => {
      expect(screen.getByText("No messages yet.")).toBeInTheDocument();
    });
  });

  it("shows an inline error when the subscribe rejects", async () => {
    subscribeTimeline.mockRejectedValue(ipcError("timelineUnavailable"));
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);

    await waitFor(() => {
      expect(screen.getByText(/Couldn't open this conversation/)).toBeInTheDocument();
    });
  });

  it("unsubscribes and clears the store on unmount", async () => {
    subscribeTimeline.mockResolvedValue(7);
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
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
    roomsStore
      .getState()
      .selectRoom({ accountId: account.accountId, roomId: "!room-a:example.org" });
    const { rerender } = render(<ConversationPane {...noopProps()} />);

    await waitFor(() => {
      expect(subscribeTimeline).toHaveBeenCalledWith(
        account.accountId,
        "!room-a:example.org",
        expect.any(Function),
      );
    });

    // Switch rooms: the effect re-runs (selectedRoomId changed).
    roomsStore
      .getState()
      .selectRoom({ accountId: account.accountId, roomId: "!room-b:example.org" });
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
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    const { unmount } = render(<ConversationPane {...noopProps()} />);

    await waitFor(() => {
      expect(subscribeTimeline).toHaveBeenCalled();
    });

    unmount();

    // A late batch (arriving after cleanup) must not mutate the store.
    captured.onBatch?.({ ops: [message("late", "@bob:example.org", "late body")] });
    expect(timelineStore.getState().items).toEqual([]);
  });

  it("shows the composer disabled until the room's timeline is loaded", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);

    // The composer is present (a room is selected) but disabled before load.
    const textarea = screen.getByLabelText("Message");
    expect(textarea).toBeDisabled();

    // Once a batch arrives (loaded), the composer becomes enabled.
    captured.onBatch?.({ ops: [message("k1", "@bob:example.org", "hi")] });
    await waitFor(() => {
      expect(screen.getByLabelText("Message")).not.toBeDisabled();
    });
  });

  it("wires the composer send to sendText with the account and room ids", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);
    captured.onBatch?.({ ops: [message("k1", "@bob:example.org", "hi")] });

    const textarea = await screen.findByLabelText("Message");
    await waitFor(() => expect(textarea).not.toBeDisabled());
    fireEvent.change(textarea, { target: { value: "hello there" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => {
      expect(sendText).toHaveBeenCalledWith(account.accountId, "!room:example.org", "hello there");
    });
  });

  it("wires a failed bubble's Retry to retrySend with the item key", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);

    captured.onBatch?.({
      ops: [
        {
          op: "reset",
          items: [
            {
              kind: "message",
              key: "outgoing-1",
              sender: account.userId,
              senderDisplayName: null,
              body: "did it send?",
              timestamp: 1,
              isOwn: true,
              sendState: "failed",
              isEdited: false,
              reply: null,
              reactions: [],
            },
          ],
        },
      ],
    });

    const retry = await screen.findByRole("button", { name: "Retry" });
    fireEvent.click(retry);

    await waitFor(() => {
      expect(retrySend).toHaveBeenCalledWith(account.accountId, "!room:example.org", "outgoing-1");
    });
  });

  it("passes offline to bubbles so a sending own message reads Queued", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    // Drive the open conversation's account offline (the pane reads its account's
    // status as a pure projection).
    accountStatusStore.getState().setStatus(account.accountId, "offline");
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);

    captured.onBatch?.({
      ops: [
        {
          op: "reset",
          items: [
            {
              kind: "message",
              key: "outgoing-1",
              sender: account.userId,
              senderDisplayName: null,
              body: "queued while offline",
              timestamp: 1,
              isOwn: true,
              sendState: "sending",
              isEdited: false,
              reply: null,
              reactions: [],
            },
          ],
        },
      ],
    });

    await waitFor(() => {
      expect(screen.getByText("Queued — sends when you're back online")).toBeInTheDocument();
    });
    expect(screen.queryByText("Sending…")).not.toBeInTheDocument();
  });

  it("routes a reply through sendReply with the original's key", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);
    captured.onBatch?.({ ops: [message("orig-1", "@bob:example.org", "the original")] });

    const textarea = await screen.findByLabelText("Message");
    await waitFor(() => expect(textarea).not.toBeDisabled());
    // Enter reply mode via the bubble's Reply action.
    fireEvent.click(await screen.findByRole("button", { name: "Reply" }));
    expect(screen.getByText(/Replying to/)).toBeInTheDocument();

    fireEvent.change(textarea, { target: { value: "my reply" } });
    fireEvent.keyDown(textarea, { key: "Enter" });

    await waitFor(() => {
      expect(sendReply).toHaveBeenCalledWith(
        account.accountId,
        "!room:example.org",
        "orig-1",
        "my reply",
      );
    });
    expect(sendText).not.toHaveBeenCalled();
    // Pending clears on success.
    await waitFor(() => expect(screen.queryByText(/Replying to/)).not.toBeInTheDocument());
  });

  it("routes an edit of an own message through editMessage with the item key", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);
    captured.onBatch?.({
      ops: [
        {
          op: "reset",
          items: [
            {
              kind: "message",
              key: "own-1",
              sender: account.userId,
              senderDisplayName: null,
              body: "my message",
              timestamp: 1,
              isOwn: true,
              sendState: null,
              isEdited: false,
              reply: null,
              reactions: [],
            },
          ],
        },
      ],
    });

    const textarea = await screen.findByLabelText<HTMLTextAreaElement>("Message");
    await waitFor(() => expect(textarea).not.toBeDisabled());
    fireEvent.click(await screen.findByRole("button", { name: "Edit" }));
    // Edit prefills the body.
    await waitFor(() => expect(textarea.value).toBe("my message"));

    fireEvent.change(textarea, { target: { value: "my edited message" } });
    fireEvent.click(screen.getByRole("button", { name: "Save edit" }));

    await waitFor(() => {
      expect(editMessage).toHaveBeenCalledWith(
        account.accountId,
        "!room:example.org",
        "own-1",
        "my edited message",
      );
    });
    expect(sendText).not.toHaveBeenCalled();
  });

  it("`↑` in an empty composer opens edit on the last own message", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);
    captured.onBatch?.({
      ops: [
        {
          op: "reset",
          items: [
            {
              kind: "message",
              key: "own-last",
              sender: account.userId,
              senderDisplayName: null,
              body: "last own message",
              timestamp: 1,
              isOwn: true,
              sendState: null,
              isEdited: false,
              reply: null,
              reactions: [],
            },
          ],
        },
      ],
    });

    const textarea = await screen.findByLabelText<HTMLTextAreaElement>("Message");
    await waitFor(() => expect(textarea).not.toBeDisabled());
    fireEvent.keyDown(textarea, { key: "ArrowUp" });

    await waitFor(() => {
      expect(screen.getByText("Editing your message")).toBeInTheDocument();
      expect(textarea.value).toBe("last own message");
    });
  });

  it("`r` on a selected message opens a reply", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);
    captured.onBatch?.({ ops: [message("sel-1", "@bob:example.org", "pick me")] });

    await screen.findByText("pick me");
    // Select the message, then press `r`.
    composerStore.getState().select("sel-1");
    fireEvent.keyDown(screen.getByLabelText("Messages"), { key: "r" });

    await waitFor(() => expect(screen.getByText(/Replying to/)).toBeInTheDocument());
  });

  it("`e` on a selected own message opens an edit; a non-own does not", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);
    captured.onBatch?.({
      ops: [
        {
          op: "reset",
          items: [
            {
              kind: "message",
              key: "other-1",
              sender: "@bob:example.org",
              senderDisplayName: "Bob",
              body: "not mine",
              timestamp: 1,
              isOwn: false,
              sendState: null,
              isEdited: false,
              reply: null,
              reactions: [],
            },
            {
              kind: "message",
              key: "mine-1",
              sender: account.userId,
              senderDisplayName: null,
              body: "mine",
              timestamp: 2,
              isOwn: true,
              sendState: null,
              isEdited: false,
              reply: null,
              reactions: [],
            },
          ],
        },
      ],
    });

    await screen.findByText("mine");
    const list = screen.getByLabelText("Messages");

    // `e` on a non-own selection is a no-op (not editable).
    composerStore.getState().select("other-1");
    fireEvent.keyDown(list, { key: "e" });
    expect(screen.queryByText("Editing your message")).not.toBeInTheDocument();

    // `e` on the own selection opens edit.
    composerStore.getState().select("mine-1");
    fireEvent.keyDown(list, { key: "e" });
    await waitFor(() => expect(screen.getByText("Editing your message")).toBeInTheDocument());
  });
});
