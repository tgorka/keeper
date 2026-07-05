import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, IpcError, TimelineBatch, TimelineItemVm } from "@/lib/ipc/client";
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
const sendAttachmentPath = vi.fn();
const sendAttachmentBytes = vi.fn();
const cancelSend = vi.fn();
const deleteMessage = vi.fn();
const roomNetworkLabel = vi.fn();
const markRoomRead = vi.fn();
const setTyping = vi.fn();
const paginateBackwards = vi.fn();
const subscribeTyping = vi.fn();
const unsubscribeTyping = vi.fn();
const subscribePaginationStatus = vi.fn();
const unsubscribePaginationStatus = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  subscribeTimeline: (accountId: string, roomId: string, onBatch: (b: TimelineBatch) => void) =>
    subscribeTimeline(accountId, roomId, onBatch),
  unsubscribeTimeline: (accountId: string, id: number) => unsubscribeTimeline(accountId, id),
  markRoomRead: (accountId: string, roomId: string) => markRoomRead(accountId, roomId),
  setTyping: (accountId: string, roomId: string, typing: boolean) =>
    setTyping(accountId, roomId, typing),
  paginateBackwards: (accountId: string, roomId: string, numEvents: number) =>
    paginateBackwards(accountId, roomId, numEvents),
  subscribeTyping: (accountId: string, roomId: string, onBatch: (b: unknown) => void) =>
    subscribeTyping(accountId, roomId, onBatch),
  unsubscribeTyping: (accountId: string, id: number) => unsubscribeTyping(accountId, id),
  subscribePaginationStatus: (accountId: string, roomId: string, onBatch: (b: unknown) => void) =>
    subscribePaginationStatus(accountId, roomId, onBatch),
  unsubscribePaginationStatus: (accountId: string, id: number) =>
    unsubscribePaginationStatus(accountId, id),
  sendText: (accountId: string, roomId: string, body: string) => sendText(accountId, roomId, body),
  sendReply: (accountId: string, roomId: string, inReplyToKey: string, body: string) =>
    sendReply(accountId, roomId, inReplyToKey, body),
  editMessage: (accountId: string, roomId: string, itemKey: string, body: string) =>
    editMessage(accountId, roomId, itemKey, body),
  retrySend: (accountId: string, roomId: string, itemKey: string) =>
    retrySend(accountId, roomId, itemKey),
  sendAttachmentPath: (accountId: string, roomId: string, path: string, caption?: string) =>
    sendAttachmentPath(accountId, roomId, path, caption),
  sendAttachmentBytes: (
    accountId: string,
    roomId: string,
    bytes: ArrayBuffer,
    filename: string,
    mime: string,
    caption?: string,
  ) => sendAttachmentBytes(accountId, roomId, bytes, filename, mime, caption),
  cancelSend: (accountId: string, roomId: string, itemKey: string) =>
    cancelSend(accountId, roomId, itemKey),
  deleteMessage: (accountId: string, roomId: string, itemKey: string) =>
    deleteMessage(accountId, roomId, itemKey),
  roomNetworkLabel: (accountId: string, roomId: string) => roomNetworkLabel(accountId, roomId),
}));

// The conversation pane subscribes to native drag-drop via `getCurrentWebview()`.
// Mock it so the listener registers (and unregisters) without a real Tauri webview.
const onDragDropEvent = vi.fn((_handler?: (e: unknown) => void) => Promise.resolve(() => {}));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent }),
}));

import { ConversationPane } from "@/components/layout/conversation-pane";
import type { InboxRoomVm } from "@/lib/ipc/client";
import { archiveRoomsStore } from "@/lib/stores/archive-rooms";
import { attachmentsStore } from "@/lib/stores/attachments";
import { composerStore } from "@/lib/stores/composer";
import { favoritesRoomsStore } from "@/lib/stores/favorites-rooms";
import { pinsRoomsStore } from "@/lib/stores/pins-rooms";

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

function messageItem(key: string, sender: string, body: string): TimelineItemVm {
  return {
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
    media: null,
    readers: [],
  };
}

function message(key: string, sender: string, body: string): TimelineBatch["ops"][number] {
  return { op: "pushBack", item: messageItem(key, sender, body) };
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
  sendAttachmentPath.mockReset();
  sendAttachmentPath.mockResolvedValue(undefined);
  sendAttachmentBytes.mockReset();
  sendAttachmentBytes.mockResolvedValue(undefined);
  cancelSend.mockReset();
  cancelSend.mockResolvedValue(undefined);
  deleteMessage.mockReset();
  deleteMessage.mockResolvedValue(undefined);
  roomNetworkLabel.mockReset();
  roomNetworkLabel.mockResolvedValue(null);
  markRoomRead.mockReset();
  markRoomRead.mockResolvedValue(undefined);
  setTyping.mockReset();
  setTyping.mockResolvedValue(undefined);
  paginateBackwards.mockReset();
  paginateBackwards.mockResolvedValue(false);
  subscribeTyping.mockReset();
  subscribeTyping.mockResolvedValue(1);
  unsubscribeTyping.mockReset();
  unsubscribeTyping.mockResolvedValue(undefined);
  subscribePaginationStatus.mockReset();
  subscribePaginationStatus.mockResolvedValue(2);
  unsubscribePaginationStatus.mockReset();
  unsubscribePaginationStatus.mockResolvedValue(undefined);
  onDragDropEvent.mockClear();
  onDragDropEvent.mockImplementation(() => Promise.resolve(() => {}));
  composerStore.getState().clear();
  composerStore.getState().clearSelection();
  attachmentsStore.getState().clear();
});

afterEach(() => {
  accountsStore.getState().clear();
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  timelineStore.getState().clear();
  accountStatusStore.getState().reset();
  composerStore.getState().clear();
  composerStore.getState().clearSelection();
  attachmentsStore.getState().clear();
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
              media: null,
              readers: [],
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
            media: null,
            readers: [],
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
              media: null,
              readers: [],
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
              media: null,
              readers: [],
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

  it("pushes a dropped file path into the composer tray", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    const dropped: { handler: ((e: unknown) => void) | null } = { handler: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    onDragDropEvent.mockImplementation((handler?: (e: unknown) => void) => {
      dropped.handler = handler ?? null;
      return Promise.resolve(() => {});
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);
    captured.onBatch?.({ ops: [message("k1", "@bob:example.org", "hi")] });
    await waitFor(() => expect(dropped.handler).not.toBeNull());

    // Simulate a native drop event with an OS path (no bytes cross here).
    dropped.handler?.({ payload: { type: "drop", paths: ["/home/alice/dropped.png"] } });

    await waitFor(() => {
      expect(screen.getByText("dropped.png")).toBeInTheDocument();
    });
    const pending = attachmentsStore.getState().pending;
    expect(pending).toHaveLength(1);
    expect(pending[0]).toMatchObject({ kind: "path", path: "/home/alice/dropped.png" });
  });

  it("removes each attachment from the tray as it enqueues so a partial failure never re-sends it", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    // First attachment enqueues successfully; the second fails at enqueue time.
    sendAttachmentPath
      .mockReset()
      .mockResolvedValueOnce(undefined)
      .mockRejectedValueOnce(ipcError("sendFailed"));
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);
    captured.onBatch?.({ ops: [message("k1", "@bob:example.org", "hi")] });

    const textarea = await screen.findByLabelText("Message");
    await waitFor(() => expect(textarea).not.toBeDisabled());

    attachmentsStore.getState().addMany([
      { id: "a1", kind: "path", path: "/tmp/one.png", filename: "one.png" },
      { id: "a2", kind: "path", path: "/tmp/two.png", filename: "two.png" },
    ]);
    await waitFor(() => expect(screen.getByText("two.png")).toBeInTheDocument());

    fireEvent.click(screen.getByRole("button", { name: "Send message" }));

    await waitFor(() => expect(sendAttachmentPath).toHaveBeenCalledTimes(2));
    // The first (enqueued) attachment is gone; only the failed one remains, so a
    // retry re-dispatches just it — never a duplicate of the already-enqueued file.
    const pending = attachmentsStore.getState().pending;
    expect(pending).toHaveLength(1);
    expect(pending[0]).toMatchObject({ id: "a2" });
  });

  it("wires an in-flight media echo's Cancel to cancelSend with the item key", async () => {
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
              key: "media-1",
              sender: account.userId,
              senderDisplayName: null,
              body: "",
              timestamp: 1,
              isOwn: true,
              sendState: "sending",
              isEdited: false,
              reply: null,
              reactions: [],
              media: {
                kind: "image",
                url: "keeper-media://media/a/r/media-1/full",
                thumbnailUrl: "keeper-media://media/a/r/media-1/thumb",
                filename: "photo.png",
                mimetype: "image/png",
                size: 2048,
                width: 400,
                height: 300,
                caption: null,
              },
              readers: [],
            },
          ],
        },
      ],
    });

    const cancel = await screen.findByRole("button", { name: "Cancel upload" });
    fireEvent.click(cancel);
    await waitFor(() => {
      expect(cancelSend).toHaveBeenCalledWith(account.accountId, "!room:example.org", "media-1");
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
              media: null,
              readers: [],
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
              media: null,
              readers: [],
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
              media: null,
              readers: [],
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
              media: null,
              readers: [],
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
              media: null,
              readers: [],
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
              media: null,
              readers: [],
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

  it("opens the media preview overlay when an image bubble is clicked", async () => {
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
              key: "media-1",
              sender: "@bob:example.org",
              senderDisplayName: "Bob",
              body: "",
              timestamp: 1,
              isOwn: false,
              sendState: null,
              isEdited: false,
              reply: null,
              reactions: [],
              media: {
                kind: "image",
                url: "keeper-media://media/acct/room/media-1/full",
                thumbnailUrl: "keeper-media://media/acct/room/media-1/thumb",
                filename: "photo.png",
                mimetype: "image/png",
                size: 12345,
                width: 800,
                height: 600,
                caption: null,
              },
              readers: [],
            },
          ],
        },
      ],
    });

    // No overlay before the click.
    expect(screen.queryByRole("dialog")).not.toBeInTheDocument();

    const openButton = await screen.findByRole("button", { name: "Open image photo.png" });
    fireEvent.click(openButton);

    // The Quick-Look overlay opens with the full-res image.
    const dialog = await screen.findByRole("dialog");
    expect(dialog).toBeInTheDocument();
    const previews = screen.getAllByAltText("photo.png") as HTMLImageElement[];
    expect(previews.some((i) => i.getAttribute("src")?.endsWith("/full"))).toBe(true);
  });

  it("renders a streamed redacted item as an honest 'Message deleted' stub (never blank)", async () => {
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
              kind: "redacted",
              key: "r1",
              sender: "@carol:example.org",
              senderDisplayName: "Carol",
              timestamp: 1,
            },
          ],
        },
      ],
    });

    await waitFor(() => {
      expect(screen.getByText("Message deleted")).toBeInTheDocument();
    });
  });

  it("`⌫` on a selected own message opens the delete confirmation; a non-own does not", async () => {
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
              media: null,
              readers: [],
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
              media: null,
              readers: [],
            },
          ],
        },
      ],
    });

    await screen.findByText("mine");
    const list = screen.getByLabelText("Messages");

    // `⌫` on a non-own selection is a no-op (delete is own-only).
    composerStore.getState().select("other-1");
    fireEvent.keyDown(list, { key: "Backspace" });
    expect(
      screen.queryByRole("alertdialog", { name: "Delete this message for everyone" }),
    ).not.toBeInTheDocument();

    // A modifier chord (⌘/Ctrl/Alt+⌫, e.g. delete-word) on the own selection is left
    // alone — it must not open the destructive confirmation.
    composerStore.getState().select("mine-1");
    fireEvent.keyDown(list, { key: "Backspace", metaKey: true });
    expect(
      screen.queryByRole("alertdialog", { name: "Delete this message for everyone" }),
    ).not.toBeInTheDocument();

    // A bare `⌫` on the own (sent) selection opens the confirmation dialog.
    fireEvent.keyDown(list, { key: "Backspace" });
    await waitFor(() =>
      expect(
        screen.getByRole("alertdialog", { name: "Delete this message for everyone" }),
      ).toBeInTheDocument(),
    );
  });
});

/** Configure the scroll container's geometry (jsdom has no layout). */
function setScrollGeometry(
  el: HTMLElement,
  { scrollHeight, clientHeight, scrollTop }: Record<string, number>,
) {
  Object.defineProperty(el, "scrollHeight", { value: scrollHeight, configurable: true });
  Object.defineProperty(el, "clientHeight", { value: clientHeight, configurable: true });
  Object.defineProperty(el, "scrollTop", {
    value: scrollTop,
    writable: true,
    configurable: true,
  });
}

describe("ConversationPane — receipts, typing, pagination (Story 3.9)", () => {
  /** Render an open room, capturing the timeline/typing/pagination onBatch sinks. */
  async function renderOpen() {
    const captured: {
      timeline: ((b: TimelineBatch) => void) | null;
      typing: ((b: { typists: unknown[] }) => void) | null;
      pagination: ((b: { state: string; hitStart: boolean }) => void) | null;
    } = { timeline: null, typing: null, pagination: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.timeline = onBatch;
      return Promise.resolve(1);
    });
    subscribeTyping.mockImplementation((_a, _r, onBatch: (b: { typists: unknown[] }) => void) => {
      captured.typing = onBatch;
      return Promise.resolve(1);
    });
    subscribePaginationStatus.mockImplementation(
      (_a, _r, onBatch: (b: { state: string; hitStart: boolean }) => void) => {
        captured.pagination = onBatch;
        return Promise.resolve(2);
      },
    );
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    const utils = render(<ConversationPane {...noopProps()} />);
    // The subscribe effects run after mount; wait for all three sinks to be armed.
    await waitFor(() => {
      expect(captured.timeline).not.toBeNull();
      expect(captured.typing).not.toBeNull();
      expect(captured.pagination).not.toBeNull();
    });
    return { captured, ...utils };
  }

  it("subscribes to typing + pagination status and marks the room read on view", async () => {
    await renderOpen();
    await waitFor(() => {
      expect(subscribeTyping).toHaveBeenCalledWith(
        account.accountId,
        "!room:example.org",
        expect.any(Function),
      );
      expect(subscribePaginationStatus).toHaveBeenCalledWith(
        account.accountId,
        "!room:example.org",
        expect.any(Function),
      );
      expect(markRoomRead).toHaveBeenCalledWith(account.accountId, "!room:example.org");
    });
  });

  it("renders the typing indicator from the streamed typing set", async () => {
    const { captured } = await renderOpen();
    captured.timeline?.({ ops: [{ op: "reset", items: [] }] });
    await waitFor(() => expect(captured.typing).not.toBeNull());
    captured.typing?.({ typists: [{ userId: "@bob:example.org", displayName: "Bob" }] });
    await waitFor(() => expect(screen.getByText("Bob is typing…")).toBeInTheDocument());
  });

  it("shows the paginating spinner from the pagination status stream", async () => {
    const { captured } = await renderOpen();
    captured.timeline?.({
      ops: [{ op: "reset", items: [messageItem("k1", "@bob:example.org", "hi")] }],
    });
    await waitFor(() => expect(captured.pagination).not.toBeNull());
    captured.pagination?.({ state: "paginating", hitStart: false });
    await waitFor(() =>
      expect(screen.getByText("Older history loads from your homeserver")).toBeInTheDocument(),
    );
  });

  it("states the conversation start when the homeserver has no more history", async () => {
    const { captured } = await renderOpen();
    captured.timeline?.({
      ops: [{ op: "reset", items: [messageItem("k1", "@bob:example.org", "hi")] }],
    });
    await waitFor(() => expect(captured.pagination).not.toBeNull());
    captured.pagination?.({ state: "idle", hitStart: true });
    await waitFor(() =>
      expect(screen.getByText("This is the start of the conversation")).toBeInTheDocument(),
    );
  });

  it("back-paginates when scrolled near the top (online, not at start)", async () => {
    const { captured } = await renderOpen();
    captured.timeline?.({
      ops: [{ op: "reset", items: [messageItem("k1", "@bob:example.org", "hi")] }],
    });
    await waitFor(() => expect(captured.pagination).not.toBeNull());
    captured.pagination?.({ state: "idle", hitStart: false });

    const region = await screen.findByLabelText("Messages");
    const scroll = region.parentElement as HTMLElement;
    setScrollGeometry(scroll, { scrollHeight: 1000, clientHeight: 400, scrollTop: 10 });
    fireEvent.scroll(scroll);

    await waitFor(() =>
      expect(paginateBackwards).toHaveBeenCalledWith(
        account.accountId,
        "!room:example.org",
        expect.any(Number),
      ),
    );
  });

  it("does NOT back-paginate near the top when offline", async () => {
    accountStatusStore.getState().setStatus(account.accountId, "offline");
    const { captured } = await renderOpen();
    captured.timeline?.({
      ops: [{ op: "reset", items: [messageItem("k1", "@bob:example.org", "hi")] }],
    });
    await waitFor(() => expect(captured.pagination).not.toBeNull());
    captured.pagination?.({ state: "idle", hitStart: false });

    const region = await screen.findByLabelText("Messages");
    const scroll = region.parentElement as HTMLElement;
    setScrollGeometry(scroll, { scrollHeight: 1000, clientHeight: 400, scrollTop: 10 });
    fireEvent.scroll(scroll);

    // Offline: the boundary states offline and no pagination is attempted.
    expect(
      screen.getByText("You're offline — older messages will load when you reconnect"),
    ).toBeInTheDocument();
    expect(paginateBackwards).not.toHaveBeenCalled();
  });

  it("does NOT back-paginate once the homeserver start is reached", async () => {
    const { captured } = await renderOpen();
    captured.timeline?.({
      ops: [{ op: "reset", items: [messageItem("k1", "@bob:example.org", "hi")] }],
    });
    await waitFor(() => expect(captured.pagination).not.toBeNull());
    captured.pagination?.({ state: "idle", hitStart: true });

    const region = await screen.findByLabelText("Messages");
    const scroll = region.parentElement as HTMLElement;
    setScrollGeometry(scroll, { scrollHeight: 1000, clientHeight: 400, scrollTop: 10 });
    fireEvent.scroll(scroll);

    expect(paginateBackwards).not.toHaveBeenCalled();
  });

  it("preserves scroll position (compensates by height delta) when older history prepends", async () => {
    const { captured } = await renderOpen();
    captured.timeline?.({
      ops: [{ op: "reset", items: [messageItem("k1", "@bob:example.org", "one")] }],
    });
    const region = await screen.findByLabelText("Messages");
    const scroll = region.parentElement as HTMLElement;

    // The user is scrolled up reading older history (not near the bottom). Model the
    // prepend growing the container: scrollHeight reads 1000 before the batch (the
    // value captured in onBatch) and 1200 after (once the older item is in the DOM),
    // via a getter keyed on the rendered message count.
    let scrollTopValue = 300;
    Object.defineProperty(scroll, "clientHeight", { value: 400, configurable: true });
    Object.defineProperty(scroll, "scrollTop", {
      configurable: true,
      get: () => scrollTopValue,
      set: (v: number) => {
        scrollTopValue = v;
      },
    });
    Object.defineProperty(scroll, "scrollHeight", {
      configurable: true,
      // 1000 with one message rendered; 1200 once the prepended second message is in.
      get: () => (scroll.querySelectorAll("[data-msg-key]").length >= 2 ? 1200 : 1000),
    });

    captured.timeline?.({
      ops: [{ op: "pushFront", item: messageItem("k0", "@bob:example.org", "older") }],
    });

    // The layout effect compensated scrollTop by the +200 height delta (300 → 500),
    // preserving the visual position rather than jumping to the bottom.
    await waitFor(() => expect(scrollTopValue).toBe(500));
  });

  it("does NOT move the view when a new message appends at the bottom while scrolled up", async () => {
    const { captured } = await renderOpen();
    captured.timeline?.({
      ops: [{ op: "reset", items: [messageItem("k1", "@bob:example.org", "one")] }],
    });
    const region = await screen.findByLabelText("Messages");
    const scroll = region.parentElement as HTMLElement;

    // The user is scrolled up reading history (not near the bottom). A peer message
    // arrives at the bottom, growing the container — the view must not jump.
    let scrollTopValue = 300;
    Object.defineProperty(scroll, "clientHeight", { value: 400, configurable: true });
    Object.defineProperty(scroll, "scrollTop", {
      configurable: true,
      get: () => scrollTopValue,
      set: (v: number) => {
        scrollTopValue = v;
      },
    });
    Object.defineProperty(scroll, "scrollHeight", {
      configurable: true,
      get: () => (scroll.querySelectorAll("[data-msg-key]").length >= 2 ? 1200 : 1000),
    });

    captured.timeline?.({
      ops: [{ op: "pushBack", item: messageItem("k2", "@bob:example.org", "newer") }],
    });

    // A bottom-append while scrolled up leaves scrollTop untouched (no yank).
    await new Promise((r) => setTimeout(r, 0));
    expect(scrollTopValue).toBe(300);
  });

  it("shows a sticky retriable error on a failed fetch that a status batch cannot clear, and Retry re-fires", async () => {
    paginateBackwards.mockRejectedValueOnce(new Error("network"));
    const { captured } = await renderOpen();
    captured.timeline?.({
      ops: [{ op: "reset", items: [messageItem("k1", "@bob:example.org", "hi")] }],
    });
    await waitFor(() => expect(captured.pagination).not.toBeNull());
    captured.pagination?.({ state: "idle", hitStart: false });

    const region = await screen.findByLabelText("Messages");
    const scroll = region.parentElement as HTMLElement;
    setScrollGeometry(scroll, { scrollHeight: 1000, clientHeight: 400, scrollTop: 10 });
    fireEvent.scroll(scroll);

    // The failure surfaces the retriable error boundary.
    await waitFor(() =>
      expect(screen.getByText("Couldn't load older messages.")).toBeInTheDocument(),
    );

    // A subsequent status batch must NOT silently clear the error the user needs.
    captured.pagination?.({ state: "idle", hitStart: false });
    expect(screen.getByText("Couldn't load older messages.")).toBeInTheDocument();

    // Retry re-fires the fetch (now succeeding) and clears the error.
    paginateBackwards.mockResolvedValueOnce(false);
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    await waitFor(() => expect(paginateBackwards).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(screen.queryByText("Couldn't load older messages.")).not.toBeInTheDocument(),
    );
  });

  it("shows the offline boundary (not a spinner) when the account goes offline mid-pagination", async () => {
    const { captured } = await renderOpen();
    captured.timeline?.({
      ops: [{ op: "reset", items: [messageItem("k1", "@bob:example.org", "hi")] }],
    });
    await waitFor(() => expect(captured.pagination).not.toBeNull());
    // A pagination is in flight (spinner) ...
    captured.pagination?.({ state: "paginating", hitStart: false });
    await waitFor(() =>
      expect(screen.getByText("Older history loads from your homeserver")).toBeInTheDocument(),
    );
    // ... then the account drops offline: offline honesty overrides the spinner.
    accountStatusStore.getState().setStatus(account.accountId, "offline");
    await waitFor(() =>
      expect(
        screen.getByText("You're offline — older messages will load when you reconnect"),
      ).toBeInTheDocument(),
    );
    expect(screen.queryByText("Older history loads from your homeserver")).not.toBeInTheDocument();
  });
});

function headerRoom(overrides: Partial<InboxRoomVm> = {}): InboxRoomVm {
  return {
    accountId: account.accountId,
    hueIndex: 0,
    roomId: "!room:example.org",
    displayName: "Alice Room",
    lastMessage: null,
    timestamp: null,
    avatarUrl: null,
    isUnread: false,
    mentionCount: 0,
    isArchived: false,
    isPinned: false,
    isFavourite: false,
    network: null,
    ...overrides,
  };
}

describe("ConversationPane — header attribution (Story 4.6)", () => {
  beforeEach(() => {
    pinsRoomsStore.getState().clear();
    favoritesRoomsStore.getState().clear();
    archiveRoomsStore.getState().clear();
    subscribeTimeline.mockResolvedValue(1);
  });

  afterEach(() => {
    pinsRoomsStore.getState().clear();
    favoritesRoomsStore.getState().clear();
    archiveRoomsStore.getState().clear();
  });

  it("shows the room avatar (with Network badge), name, and account chip", () => {
    accountsStore.getState().hydrateAll([account]);
    roomsStore.getState().applyBatch({
      ops: [{ op: "reset", rooms: [headerRoom({ network: "Telegram" })] }],
      total: 1,
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);

    // Room display name in the header.
    expect(screen.getByText("Alice Room")).toBeInTheDocument();
    // The Network badge comes free from the reused RoomAvatar.
    expect(screen.getByLabelText("Telegram network")).toHaveTextContent("T");
    // The account-initial chip (from the owning account's user id) — queried by a
    // stable testid so it never collides with other "A" text in the header.
    expect(screen.getByTestId("account-initial-chip")).toHaveTextContent("A");
  });

  it("degrades to the account chip alone when the room VM is not in any window", () => {
    accountsStore.getState().hydrateAll([account]);
    // No room streamed into any window; only the selection points at it.
    roomsStore
      .getState()
      .selectRoom({ accountId: account.accountId, roomId: "!ghost:example.org" });
    render(<ConversationPane {...noopProps()} />);

    // No room name / no Network badge (VM absent) ...
    expect(screen.queryByText("Alice Room")).not.toBeInTheDocument();
    expect(screen.queryByLabelText(/network$/)).not.toBeInTheDocument();
    // ... but the account-initial chip still renders (never a crash).
    expect(screen.getByTestId("account-initial-chip")).toHaveTextContent("A");
  });
});
