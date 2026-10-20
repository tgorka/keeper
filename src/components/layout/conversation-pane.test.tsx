import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  AccountVm,
  AgentRunVm,
  ApprovalCardVm,
  HeldRestoreVm,
  IpcError,
  PaginationStatusBatch,
  TimelineBatch,
  TimelineItemVm,
} from "@/lib/ipc/client";
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
const sendReplyToEvent = vi.fn();
const editMessage = vi.fn();
const retrySend = vi.fn();
const sendAttachmentPath = vi.fn();
const sendAttachmentBytes = vi.fn();
const cancelSend = vi.fn();
const deleteMessage = vi.fn();
const roomNetworkLabel = vi.fn();
const markRoomRead = vi.fn();
const releaseReceipt = vi.fn();
const couplingCaveats = vi.fn();
const incognitoSetChat = vi.fn();
const setTyping = vi.fn();
const paginateBackwards = vi.fn();
const loadEarlierMessages = vi.fn();
const subscribeTyping = vi.fn();
const unsubscribeTyping = vi.fn();
const RESTORED_NOTHING: HeldRestoreVm = { line: "", reply: null, attachment: null };
const cancelHeldSend = vi.fn(
  async (_a: string, _r: string, _id: string): Promise<HeldRestoreVm> => RESTORED_NOTHING,
);
const chatCommandPreview = vi.fn();
const roomLeave = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  // The default agent (story 100.3): read by Settings › Agents and the agent
  // room's menu on mount; a read that never answers renders neither.
  agentsDefaultGet: vi.fn(() => new Promise(() => {})),
  agentsDefaultSet: vi.fn(),
  // The questions waiting in an agent DM (story 101.3): none here.
  subscribeAgentAsks: vi.fn(() => Promise.resolve(1)),
  unsubscribeAgentAsks: vi.fn(() => Promise.resolve()),
  frontendErrorReport: vi.fn(() => Promise.resolve()),
  subscribeTimeline: (accountId: string, roomId: string, onBatch: (b: TimelineBatch) => void) =>
    subscribeTimeline(accountId, roomId, onBatch),
  unsubscribeTimeline: (accountId: string, id: number) => unsubscribeTimeline(accountId, id),
  markRoomRead: (accountId: string, roomId: string) => markRoomRead(accountId, roomId),
  releaseReceipt: (accountId: string, roomId: string) => releaseReceipt(accountId, roomId),
  couplingCaveats: () => couplingCaveats(),
  incognitoSetChat: (accountId: string, roomId: string, enabled: boolean | null) =>
    incognitoSetChat(accountId, roomId, enabled),
  setTyping: (accountId: string, roomId: string, typing: boolean) =>
    setTyping(accountId, roomId, typing),
  paginateBackwards: (accountId: string, roomId: string, numEvents: number) =>
    paginateBackwards(accountId, roomId, numEvents),
  loadEarlierMessages: (accountId: string, roomId: string) =>
    loadEarlierMessages(accountId, roomId),
  subscribeTyping: (accountId: string, roomId: string, onBatch: (b: unknown) => void) =>
    subscribeTyping(accountId, roomId, onBatch),
  unsubscribeTyping: (accountId: string, id: number) => unsubscribeTyping(accountId, id),
  sendText: (accountId: string, roomId: string, body: string) => sendText(accountId, roomId, body),
  sendReply: (accountId: string, roomId: string, inReplyToKey: string, body: string) =>
    sendReply(accountId, roomId, inReplyToKey, body),
  sendReplyToEvent: (accountId: string, roomId: string, eventId: string, body: string) =>
    sendReplyToEvent(accountId, roomId, eventId, body),
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
  // Undo-Send held-send subscription (Story 8.3): no held sends; subscribe returns a
  // stub id and never emits, unsubscribe is a no-op.
  subscribeOutbox: vi.fn(async (): Promise<number> => 1),
  unsubscribeOutbox: vi.fn(async (): Promise<void> => {}),
  cancelHeldSend: (accountId: string, roomId: string, id: string) =>
    cancelHeldSend(accountId, roomId, id),
  // Story 100.5's slash path, for the `/leave` lifetime test (R393).
  chatCommandPreview: (accountId: string, roomId: string, draft: string, replying: boolean) =>
    chatCommandPreview(accountId, roomId, draft, replying),
  roomLeave: (accountId: string, roomId: string) => roomLeave(accountId, roomId),
  // Composer persistent drafts (Story 7.1): no stored draft; persist/clear are no-ops.
  loadDraft: vi.fn(async (): Promise<string | null> => null),
  saveDraft: vi.fn(async (): Promise<void> => {}),
  clearDraft: vi.fn(async (): Promise<void> => {}),
  // Cross-device draft mirror (Story 7.2): no remote draft; mirror writes are no-ops.
  loadRemoteDraft: vi.fn(async (): Promise<null> => null),
  mirrorDraft: vi.fn(async (): Promise<void> => {}),
  clearDraftMirror: vi.fn(async (): Promise<void> => {}),
}));

// The conversation pane subscribes to native drag-drop via `getCurrentWebview()`.
// Mock it so the listener registers (and unregisters) without a real Tauri webview.
const onDragDropEvent = vi.fn((_handler?: (e: unknown) => void) => Promise.resolve(() => {}));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent }),
}));

import { COMPOSER_CONTEXT_CHANGED } from "@/components/chat/composer";
import { ConversationPane } from "@/components/layout/conversation-pane";
import { openAndFocus } from "@/lib/default-agent-actions";
import type { CouplingCaveatVm, InboxRoomVm, IncognitoVm } from "@/lib/ipc/client";
import { mirrorDraft, saveDraft } from "@/lib/ipc/client";
import { archiveRoomsStore } from "@/lib/stores/archive-rooms";
import { attachmentsStore } from "@/lib/stores/attachments";
import { composerStore } from "@/lib/stores/composer";
import { favoritesRoomsStore } from "@/lib/stores/favorites-rooms";
import { incognitoStore } from "@/lib/stores/incognito";
import { outboxStore } from "@/lib/stores/outbox";
import { pinsRoomsStore } from "@/lib/stores/pins-rooms";
import { primaryViewStore } from "@/lib/stores/primary-view";

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
    brief: null,
    gate: null,
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
  sendReplyToEvent.mockReset();
  sendReplyToEvent.mockResolvedValue(undefined);
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
  releaseReceipt.mockReset();
  releaseReceipt.mockResolvedValue(undefined);
  couplingCaveats.mockReset();
  couplingCaveats.mockResolvedValue([]);
  incognitoSetChat.mockReset();
  incognitoSetChat.mockResolvedValue(undefined);
  setTyping.mockReset();
  setTyping.mockResolvedValue(undefined);
  paginateBackwards.mockReset();
  paginateBackwards.mockResolvedValue(false);
  loadEarlierMessages.mockReset();
  loadEarlierMessages.mockResolvedValue(undefined);
  subscribeTyping.mockReset();
  subscribeTyping.mockResolvedValue(1);
  unsubscribeTyping.mockReset();
  unsubscribeTyping.mockResolvedValue(undefined);
  onDragDropEvent.mockClear();
  onDragDropEvent.mockImplementation(() => Promise.resolve(() => {}));
  composerStore.getState().clear();
  composerStore.getState().clearSelection();
  composerStore.setState({ restoreBody: null, restoreTarget: null });
  attachmentsStore.getState().clear();
  outboxStore.getState().clear();
  cancelHeldSend.mockReset();
  cancelHeldSend.mockResolvedValue(RESTORED_NOTHING);
  chatCommandPreview.mockReset();
  roomLeave.mockReset();
  roomLeave.mockResolvedValue(undefined);
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
  outboxStore.getState().clear();
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
              brief: null,
              gate: null,
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

  it("draws an agent's approval request where it arrived, and follows the room's state beside the stream", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);
    const card: ApprovalCardVm = {
      id: "01A",
      bindingDigest: "sha256:aa",
      tier: 2,
      tierWord: "T2: it changes something that can be put back",
      summary: "Write `a.md` in tgdrive (3 bytes)",
      tool: "drive_write",
      payload: '{\n  "path": "a.md"\n}',
      attachment: null,
      approvers: [{ user: "@alice:example.org", name: "Alice" }],
      anyone: false,
      chain: [{ user: "@alice:example.org", name: "Alice" }],
      scopes: [{ scope: "once", label: "Approve once", detail: "This action once." }],
      expiresAt: 4_000_000_000_000,
      state: { state: "pending" },
      canDecide: true,
      cannotDecide: null,
      verify: false,
      only: null,
      declassify: null,
      run: null,
      kvm: null,
      gate: null,
    };

    await waitFor(() => expect(captured.onBatch).not.toBeNull());
    act(() => {
      captured.onBatch?.({
        ops: [
          {
            op: "reset",
            items: [
              messageItem("k1", "@nixi:example.org", "before"),
              { kind: "approval", key: "ap", id: "01A" },
              messageItem("k2", "@nixi:example.org", "after"),
            ],
          },
        ],
        approvals: [{ id: "01A", cards: [card], gate: null }],
      });
    });

    const request = await screen.findByRole("article", {
      name: "Approval request: Write a.md in tgdrive (3 bytes)",
    });
    const rows = within(screen.getByRole("list", { name: "Messages" })).getAllByRole("listitem");
    const at = rows.findIndex((row) => row.contains(request));
    expect(rows[at - 1]).toHaveTextContent("before");
    expect(rows[at + 1]).toHaveTextContent("after");
    expect(within(request).getByRole("button", { name: "Approve once" })).toBeInTheDocument();

    // A decision echoed by the room arrives beside the stream, with no ops.
    act(() => {
      captured.onBatch?.({
        ops: [],
        approvals: [
          {
            id: "01A",
            cards: [
              {
                ...card,
                canDecide: false,
                state: {
                  state: "decided",
                  decision: "deny",
                  scope: "once",
                  by: "@alice:example.org",
                  byName: "Alice",
                },
              },
            ],
            gate: null,
          },
        ],
      });
    });
    expect(await within(request).findByText("Denied by Alice.")).toBeInTheDocument();
    expect(within(request).queryByRole("button", { name: "Approve once" })).toBeNull();
  });

  it("draws each approval request in the stream as its own request, whatever order the room lists them in", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);
    const request = (id: string, summary: string): ApprovalCardVm => ({
      id,
      bindingDigest: `sha256:${id}`,
      tier: 2,
      tierWord: "T2: it changes something that can be put back",
      summary,
      tool: "drive_write",
      payload: null,
      attachment: null,
      approvers: [{ user: "@alice:example.org", name: "Alice" }],
      anyone: false,
      chain: [{ user: "@alice:example.org", name: "Alice" }],
      scopes: [{ scope: "once", label: "Approve once", detail: "This action once." }],
      expiresAt: 4_000_000_000_000,
      state: { state: "pending" },
      canDecide: true,
      cannotDecide: null,
      verify: false,
      only: null,
      declassify: null,
      run: null,
      kvm: null,
      gate: null,
    });

    await waitFor(() => expect(captured.onBatch).not.toBeNull());
    act(() => {
      captured.onBatch?.({
        ops: [
          {
            op: "reset",
            items: [
              { kind: "approval", key: "ap1", id: "01A" },
              messageItem("k1", "@nixi:example.org", "between"),
              { kind: "approval", key: "ap2", id: "01B" },
            ],
          },
        ],
        approvals: [
          { id: "01B", cards: [request("01B", "Write b.md in tgdrive")], gate: null },
          { id: "01A", cards: [request("01A", "Write a.md in tgdrive")], gate: null },
        ],
      });
    });

    const first = await screen.findByRole("article", {
      name: "Approval request: Write a.md in tgdrive",
    });
    const second = screen.getByRole("article", { name: "Approval request: Write b.md in tgdrive" });
    // The rows in the stream's order, after the history boundary's own row.
    const rows = within(screen.getByRole("list", { name: "Messages" }))
      .getAllByRole("listitem")
      .filter((row) => within(row).queryByText("Load earlier messages") === null);
    expect(rows.findIndex((row) => row.contains(first))).toBe(0);
    expect(rows[1]).toHaveTextContent("between");
    expect(rows.findIndex((row) => row.contains(second))).toBe(2);
  });

  it("consumes Escape only when there is composer context to clear (phone-shell cascade)", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);
    captured.onBatch?.({
      ops: [{ op: "reset", items: [messageItem("k1", "@bob:example.org", "hi from bob")] }],
    });
    const bubble = await screen.findByText("hi from bob");

    // With a pending reply, Escape clears it AND is marked handled (preventDefault),
    // so the phone-shell Escape handler does not also pop the level.
    composerStore.setState({
      pending: { mode: "reply", targetKey: "k1", sender: "Bob", bodyPreview: "hi from bob" },
    });
    const handledWithPending = fireEvent.keyDown(bubble, { key: "Escape" });
    expect(composerStore.getState().pending).toBeNull();
    expect(handledWithPending).toBe(false); // preventDefault called → shell won't pop

    // With nothing to clear, Escape is left unhandled so it bubbles to the shell,
    // which pops a stack level (UX-DR28).
    const handledWhenEmpty = fireEvent.keyDown(bubble, { key: "Escape" });
    expect(handledWhenEmpty).toBe(true); // not prevented → bubbles for the back-pop
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
            brief: null,
            gate: null,
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
              brief: null,
              gate: null,
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
              brief: null,
              gate: null,
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

    captured.onBatch?.({
      ops: [{ op: "reset", items: [{ kind: "other", key: "o1" }] }],
      pagination: { state: "idle", hitStart: true },
    });

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

  it("asks about leaving only the room /leave was typed in, and a room switch withdraws the question (R393)", async () => {
    const roomA = "!room-a:example.org";
    const roomB = "!room-b:example.org";
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    let nextId = 1;
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(nextId++);
    });
    const leaveVerdict = (draft: string) => ({
      draft,
      verdict:
        draft === "/leave"
          ? { kind: "command", name: "leave", args: null }
          : { kind: "text", text: draft },
      rows: [],
      escapeHint: "",
    });
    // Every answer is held until the test lets it go.
    const asks: Array<() => void> = [];
    chatCommandPreview.mockImplementation(
      (_a: string, _r: string, draft: string) =>
        new Promise((answer) => asks.push(() => answer(leaveVerdict(draft)))),
    );
    const open = async (roomId: string) => {
      roomsStore.getState().selectRoom({ accountId: account.accountId, roomId });
      rerender(<ConversationPane {...noopProps()} />);
      await waitFor(() =>
        expect(subscribeTimeline).toHaveBeenCalledWith(
          account.accountId,
          roomId,
          expect.any(Function),
        ),
      );
      act(() => captured.onBatch?.({ ops: [message(`k-${roomId}`, "@bob:example.org", "hi")] }));
      const field = await screen.findByLabelText("Message");
      await waitFor(() => expect(field).not.toBeDisabled());
      return field;
    };
    const typeLeave = (field: HTMLElement) => {
      fireEvent.change(field, { target: { value: "/leave" } });
      fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    };
    const releaseAll = async () => {
      await act(async () => {
        for (const answer of asks.splice(0)) {
          answer();
        }
      });
    };

    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: roomA });
    const { rerender } = render(<ConversationPane {...noopProps()} />);
    // `/leave` in A, then B is opened before Rust answers: nothing is asked about B.
    typeLeave(await open(roomA));
    await open(roomB);
    await releaseAll();
    expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument();

    // `/leave` in B opens its question; switching to A withdraws it, unanswered.
    typeLeave(await open(roomB));
    await releaseAll();
    expect(await screen.findByRole("alertdialog")).toBeInTheDocument();
    await open(roomA);
    await waitFor(() => expect(screen.queryByRole("alertdialog")).not.toBeInTheDocument());
    expect(roomLeave).not.toHaveBeenCalled();

    // `/leave` in A, confirmed: exactly A is left.
    typeLeave(screen.getByLabelText("Message"));
    await releaseAll();
    fireEvent.click(await screen.findByRole("button", { name: "Leave" }));
    await waitFor(() => expect(roomLeave).toHaveBeenCalledTimes(1));
    expect(roomLeave).toHaveBeenCalledWith(account.accountId, roomA);
  });

  it("sends a slow / line where it was submitted, never into an edit or reply begun meanwhile (R411)", async () => {
    const room = "!room:example.org";
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    // `/plain <text>` is text; every answer is held until the test lets it go.
    const asks: Array<() => void> = [];
    chatCommandPreview.mockImplementation(
      (_a: string, _r: string, draft: string) =>
        new Promise((answer) =>
          asks.push(() =>
            answer({
              draft,
              verdict: { kind: "text", text: draft.replace(/^\/plain /, "") },
              rows: [],
              escapeHint: "",
            }),
          ),
        ),
    );
    const releaseAll = async () => {
      await act(async () => {
        for (const answer of asks.splice(0)) {
          answer();
        }
      });
    };
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: room });
    render(<ConversationPane {...noopProps()} />);
    act(() =>
      captured.onBatch?.({
        ops: [
          message("orig-1", "@bob:example.org", "the original"),
          {
            op: "pushBack",
            item: {
              ...(messageItem("own-1", account.userId, "my message") as Extract<
                TimelineItemVm,
                { kind: "message" }
              >),
              isOwn: true,
            },
          },
        ],
      }),
    );
    const field = await screen.findByLabelText<HTMLTextAreaElement>("Message");
    await waitFor(() => expect(field).not.toBeDisabled());
    const submit = (line: string) => {
      fireEvent.change(field, { target: { value: line } });
      fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    };

    // An edit begun while Rust reads a new message: the line is that new message,
    // and the edit is neither saved nor closed.
    submit("/plain replacement");
    fireEvent.click(await screen.findByRole("button", { name: "Edit" }));
    await waitFor(() => expect(field.value).toBe("my message"));
    await releaseAll();
    await waitFor(() =>
      expect(sendText).toHaveBeenCalledWith(account.accountId, room, "replacement"),
    );
    expect(editMessage).not.toHaveBeenCalled();
    expect(screen.getByText("Editing your message")).toBeInTheDocument();
    expect(field.value).toBe("my message");
    // The text the edit set aside was that line, now sent: leaving the edit
    // brings nothing back.
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(field.value).toBe(""));
    expect(editMessage).not.toHaveBeenCalled();

    // A reply begun meanwhile does not take the line either, and stays open.
    sendText.mockClear();
    submit("/plain second");
    fireEvent.click(screen.getAllByRole("button", { name: "Reply" })[0] as HTMLElement);
    expect(screen.getByText(/Replying to/)).toBeInTheDocument();
    await releaseAll();
    await waitFor(() => expect(sendText).toHaveBeenCalledWith(account.accountId, room, "second"));
    expect(sendReply).not.toHaveBeenCalled();
    expect(screen.getByText(/Replying to/)).toBeInTheDocument();

    // A line submitted as that reply, which is then cancelled: nothing is sent.
    sendText.mockClear();
    submit("/plain third");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await releaseAll();
    expect(await screen.findByRole("alert")).toHaveTextContent(COMPOSER_CONTEXT_CHANGED);
    expect(sendText).not.toHaveBeenCalled();
    expect(sendReply).not.toHaveBeenCalled();
    expect(field.value).toBe("/plain third");
  });

  // R100S3-01: a slow `/` line that fails while an edit borrows the field stays
  // the room's draft — saved and mirrored as the failed line, never as the
  // edit's body — and leaving the edit brings it back.
  it("keeps a failed slow / line as the room's draft, never an edit begun meanwhile", async () => {
    const room = "!room:example.org";
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    const asks: Array<() => void> = [];
    chatCommandPreview.mockImplementation(
      (_a: string, _r: string, draft: string) =>
        new Promise((answer) =>
          asks.push(() =>
            answer({
              draft,
              verdict: { kind: "text", text: draft.replace(/^\/plain /, "") },
              rows: [],
              escapeHint: "",
            }),
          ),
        ),
    );
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: room });
    render(<ConversationPane {...noopProps()} />);
    act(() =>
      captured.onBatch?.({
        ops: [
          {
            op: "pushBack",
            item: {
              ...(messageItem("own-1", account.userId, "my message") as Extract<
                TimelineItemVm,
                { kind: "message" }
              >),
              isOwn: true,
            },
          },
        ],
      }),
    );
    const field = await screen.findByLabelText<HTMLTextAreaElement>("Message");
    await waitFor(() => expect(field).not.toBeDisabled());
    fireEvent.change(field, { target: { value: "/plain replacement" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    fireEvent.click(await screen.findByRole("button", { name: "Edit" }));
    await waitFor(() => expect(field.value).toBe("my message"));

    vi.mocked(saveDraft).mockClear();
    vi.mocked(mirrorDraft).mockClear();
    sendText.mockRejectedValueOnce({ code: "sendFailed", message: "offline", retriable: true });
    await act(async () => {
      for (const answer of asks.splice(0)) {
        answer();
      }
    });
    await waitFor(() =>
      expect(sendText).toHaveBeenCalledWith(account.accountId, room, "replacement"),
    );
    await waitFor(() => expect(mirrorDraft).toHaveBeenCalled());
    expect(vi.mocked(saveDraft).mock.calls.map((call) => call[2])).toEqual(["/plain replacement"]);
    expect(vi.mocked(mirrorDraft).mock.calls.map((call) => call[2])).toEqual([
      "/plain replacement",
    ]);
    // The edit kept its body, and leaving it puts the failed line back.
    expect(field.value).toBe("my message");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => expect(field.value).toBe("/plain replacement"));
    expect(editMessage).not.toHaveBeenCalled();
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
              brief: null,
              gate: null,
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
              brief: null,
              gate: null,
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
              brief: null,
              gate: null,
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
              brief: null,
              gate: null,
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
              brief: null,
              gate: null,
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
              brief: null,
              gate: null,
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
              brief: null,
              gate: null,
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
              brief: null,
              gate: null,
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
              sendState: null,
            },
          ],
        },
      ],
    });

    await waitFor(() => {
      expect(screen.getByText("Message deleted")).toBeInTheDocument();
    });
  });

  it("a refused deletion of my message says 'Not deleted', not 'Message deleted', and Retry and Cancel reach it", async () => {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);

    const stub = (sendState: "sending" | "failed") => ({
      kind: "redacted" as const,
      key: "r1",
      sender: account.userId,
      senderDisplayName: null,
      timestamp: 1,
      sendState,
    });
    captured.onBatch?.({ ops: [{ op: "reset", items: [stub("sending")] }] });
    await waitFor(() => expect(screen.getByText("Deleting…")).toBeInTheDocument());
    expect(screen.queryByText("Message deleted")).toBeNull();

    captured.onBatch?.({ ops: [{ op: "set", index: 0, item: stub("failed") }] });
    await waitFor(() => expect(screen.getByText("Not deleted")).toBeInTheDocument());
    expect(screen.queryByText("Message deleted")).toBeNull();
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(retrySend).toHaveBeenCalledWith(account.accountId, "!room:example.org", "r1");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
    expect(cancelSend).toHaveBeenCalledWith(account.accountId, "!room:example.org", "r1");
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
              brief: null,
              gate: null,
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
              brief: null,
              gate: null,
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

describe("ConversationPane — held-aware Delete (Story 8.4)", () => {
  /** Render an open room with one dispatched own message, capturing nothing extra. */
  async function renderWithOwnMessage() {
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      onBatch({
        ops: [
          {
            op: "reset",
            items: [
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
                brief: null,
                gate: null,
              },
            ],
          },
        ],
      });
      return Promise.resolve(1);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    const utils = render(<ConversationPane {...noopProps()} />);
    await screen.findByText("mine");
    return utils;
  }

  /** Seed one held send for the open room. */
  function seedHeld(id = "held-1", body = "held body") {
    const now = Date.now();
    outboxStore.getState().applySnapshot(account.accountId, "!room:example.org", [
      {
        id,
        accountId: account.accountId,
        roomId: "!room:example.org",
        body,
        heldAtMs: now,
        dispatchAtMs: now + 10_000,
        filename: null,
        reply: false,
      },
    ]);
  }

  it("held-bubble Delete resolves as an undo (cancelHeldSend) and never opens the delete dialog", async () => {
    cancelHeldSend.mockResolvedValue({ ...RESTORED_NOTHING, line: "held body" });
    await renderWithOwnMessage();
    act(() => {
      seedHeld();
    });

    const deleteBtn = await screen.findByTestId("held-delete-button");
    fireEvent.click(deleteBtn);

    await waitFor(() =>
      expect(cancelHeldSend).toHaveBeenCalledWith(account.accountId, "!room:example.org", "held-1"),
    );
    // The held id must NEVER reach the redaction path.
    expect(deleteMessage).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("alertdialog", { name: "Delete this message for everyone" }),
    ).not.toBeInTheDocument();
    // A non-empty returned body is restored to the composer as a Draft.
    await waitFor(() => expect(composerStore.getState().restoreBody).toBe("held body"));
  });

  it("held-bubble Delete is on screen under a coarse pointer (Story 66.1, AD-197)", async () => {
    // Hover-revealed on a mouse; a finger never hovers, so the class list must
    // carry the `pointer-coarse:` fallback the account row's menu wears —
    // otherwise the only way to discard a held send on the phone is `⌫` on a
    // keyboard the phone does not have.
    await renderWithOwnMessage();
    act(() => {
      seedHeld();
    });
    const deleteBtn = await screen.findByTestId("held-delete-button");
    expect(deleteBtn.className).toContain("group-hover:opacity-100");
    expect(deleteBtn.className).toContain("pointer-coarse:opacity-100");
  });

  it("`⌫` on a selected held bubble resolves as an undo and never opens the delete dialog", async () => {
    cancelHeldSend.mockResolvedValue({ ...RESTORED_NOTHING, line: "held body" });
    await renderWithOwnMessage();
    act(() => {
      seedHeld();
    });
    await screen.findByTestId("held-bubble");

    // Select the held bubble (selected key is `held:<id>`) and press ⌫.
    composerStore.getState().select("held:held-1");
    const list = screen.getByLabelText("Messages");
    fireEvent.keyDown(list, { key: "Backspace" });

    await waitFor(() =>
      expect(cancelHeldSend).toHaveBeenCalledWith(account.accountId, "!room:example.org", "held-1"),
    );
    expect(deleteMessage).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("alertdialog", { name: "Delete this message for everyone" }),
    ).not.toBeInTheDocument();
  });

  it("Delete/`⌫` on a dispatched own message still opens the Story 3.8 redaction dialog", async () => {
    await renderWithOwnMessage();
    const list = screen.getByLabelText("Messages");

    composerStore.getState().select("mine-1");
    fireEvent.keyDown(list, { key: "Backspace" });

    await waitFor(() =>
      expect(
        screen.getByRole("alertdialog", { name: "Delete this message for everyone" }),
      ).toBeInTheDocument(),
    );
    // The dispatched path never invokes the held-undo cancel.
    expect(cancelHeldSend).not.toHaveBeenCalled();
  });

  it("keeps opening the redaction dialog for a dispatched own message while a held row is present", async () => {
    // The branch ordering is load-bearing: `tryUndoHeld` must consume only `held:` keys,
    // so a selected dispatched message still redacts even when held rows coexist.
    await renderWithOwnMessage();
    act(() => {
      seedHeld();
    });
    await screen.findByTestId("held-bubble");
    const list = screen.getByLabelText("Messages");

    composerStore.getState().select("mine-1");
    fireEvent.keyDown(list, { key: "Backspace" });

    await waitFor(() =>
      expect(
        screen.getByRole("alertdialog", { name: "Delete this message for everyone" }),
      ).toBeInTheDocument(),
    );
    expect(cancelHeldSend).not.toHaveBeenCalled();
  });

  it("consumes `⌫` on a stale held selection without a redaction or a cancel", async () => {
    // The window elapsed and the row dispatched, but `selectedKey` still points at the
    // vanished `held:<id>`. The key must be consumed (no redaction, no cancel) rather
    // than leaking to the redaction path or the browser's native Backspace.
    await renderWithOwnMessage();
    composerStore.getState().select("held:gone-1");
    const list = screen.getByLabelText("Messages");

    fireEvent.keyDown(list, { key: "Backspace" });

    expect(cancelHeldSend).not.toHaveBeenCalled();
    expect(deleteMessage).not.toHaveBeenCalled();
    expect(
      screen.queryByRole("alertdialog", { name: "Delete this message for everyone" }),
    ).not.toBeInTheDocument();
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
  // The rows' own extent, what decides whether the window scrolls (R513): the
  // list holds `scrollHeight` of them, its history-boundary row nothing.
  const list = el.firstElementChild;
  const boundary = list?.firstElementChild;
  if (list && boundary) {
    Object.defineProperty(list, "getBoundingClientRect", {
      value: () => ({ height: scrollHeight }) as DOMRect,
      configurable: true,
    });
    Object.defineProperty(boundary, "getBoundingClientRect", {
      value: () => ({ height: 0 }) as DOMRect,
      configurable: true,
    });
  }
}

describe("ConversationPane — receipts, typing, pagination (Story 3.9)", () => {
  /** Render an open room, capturing the timeline/typing/pagination onBatch sinks. */
  async function renderOpen() {
    const captured: {
      timeline: ((b: TimelineBatch) => void) | null;
      typing: ((b: { typists: unknown[] }) => void) | null;
      pagination: ((b: PaginationStatusBatch) => void) | null;
    } = { timeline: null, typing: null, pagination: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.timeline = onBatch;
      return Promise.resolve(1);
    });
    subscribeTyping.mockImplementation((_a, _r, onBatch: (b: { typists: unknown[] }) => void) => {
      captured.typing = onBatch;
      return Promise.resolve(1);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    const utils = render(<ConversationPane {...noopProps()} />);
    // The subscribe effects run after mount; wait for the timeline and typing sinks.
    // The pagination status rides on the timeline stream (102.6).
    await waitFor(() => {
      expect(captured.timeline).not.toBeNull();
      expect(captured.typing).not.toBeNull();
    });
    captured.pagination = (b) => captured.timeline?.({ ops: [], pagination: b });
    return { captured, ...utils };
  }

  it("subscribes to typing and marks the room read on view", async () => {
    const { captured } = await renderOpen();
    captured.timeline?.({ ops: [{ op: "reset", items: [] }] });
    await waitFor(() => {
      expect(subscribeTyping).toHaveBeenCalledWith(
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

  // Filling a window that draws too little is Rust's (102.6): the webview
  // asks only when the person does. An empty window whose fill rests short
  // of the start (its budget spent) keeps the person's way on (R509).
  it("leaves filling an empty window to Rust and offers to load earlier messages", async () => {
    const { captured } = await renderOpen();
    captured.timeline?.({
      ops: [{ op: "reset", items: [{ kind: "other", key: "o1" }] }],
      pagination: { state: "idle", hitStart: false },
    });
    const more = await screen.findByRole("button", { name: "Load earlier messages" });
    expect(screen.queryByText("No messages yet.")).not.toBeInTheDocument();
    await new Promise((resolve) => setTimeout(resolve, 50));
    expect(paginateBackwards).not.toHaveBeenCalled();
    expect(loadEarlierMessages).not.toHaveBeenCalled();
    fireEvent.click(more);
    expect(loadEarlierMessages).toHaveBeenCalledWith(account.accountId, "!room:example.org");
  });

  // A phone showing a few short items cannot scroll, so neither a scroll nor
  // (on touch) a wheel ever asks for more (R509): the window offers it as a
  // button, which a scrollable window does not need.
  it("offers older history as a button only while the window cannot scroll", async () => {
    const { captured } = await renderOpen();
    captured.timeline?.({
      ops: [{ op: "reset", items: [messageItem("k1", "@bob:example.org", "hi")] }],
      pagination: { state: "idle", hitStart: false },
    });
    const more = await screen.findByRole("button", { name: "Load earlier messages" });
    fireEvent.click(more);
    expect(loadEarlierMessages).toHaveBeenCalledWith(account.accountId, "!room:example.org");
    expect(paginateBackwards).not.toHaveBeenCalled();

    const scroll = (await screen.findByLabelText("Messages")).parentElement as HTMLElement;
    setScrollGeometry(scroll, { scrollHeight: 1000, clientHeight: 400, scrollTop: 600 });
    captured.timeline?.({
      ops: [{ op: "pushBack", item: messageItem("k2", "@bob:example.org", "more") }],
    });
    // Gone, not merely hidden from assistive technology.
    await waitFor(() => expect(screen.queryByText("Load earlier messages")).toBeNull());
  });

  // Rows just short of the window (R513): were the button's own row counted,
  // it would make the overflow that removes it, and back, at every resize. The
  // button settles, keeps the focus, still asks Rust, keeps the newest message
  // in view, and goes only once the rows themselves cross the window.
  it("settles the load-earlier button for rows just short of the window, across a resize", async () => {
    const observers: { callback: ResizeObserverCallback; targets: Element[] }[] = [];
    vi.stubGlobal(
      "ResizeObserver",
      class {
        targets: Element[] = [];
        constructor(callback: ResizeObserverCallback) {
          observers.push({ callback, targets: this.targets });
        }
        observe(target: Element) {
          this.targets.push(target);
        }
        unobserve() {}
        disconnect() {
          this.targets.length = 0;
        }
      },
    );
    try {
      const { captured } = await renderOpen();
      captured.timeline?.({
        ops: [{ op: "reset", items: [messageItem("k1", "@bob:example.org", "hi")] }],
        pagination: { state: "idle", hitStart: false },
      });
      const list = await screen.findByLabelText("Messages");
      const scroll = list.parentElement as HTMLElement;
      const boundary = list.firstElementChild as HTMLElement;
      // The layout jsdom lacks: 380 px of rows, a 48 px boundary row while it
      // holds the button, and the window's height.
      let viewport = 400;
      const rows = 380;
      const row = () => (within(boundary).queryByRole("button") === null ? 0 : 48);
      let scrollTop = 0;
      Object.defineProperties(scroll, {
        clientHeight: { configurable: true, get: () => viewport },
        scrollHeight: { configurable: true, get: () => Math.max(viewport, rows + row()) },
        scrollTop: {
          configurable: true,
          get: () => scrollTop,
          set: (value: number) => {
            scrollTop = value;
          },
        },
      });
      Object.defineProperty(list, "getBoundingClientRect", {
        configurable: true,
        value: () => ({ height: rows + row() }) as DOMRect,
      });
      Object.defineProperty(boundary, "getBoundingClientRect", {
        configurable: true,
        value: () => ({ height: row() }) as DOMRect,
      });
      // Every size change of the window or its list reaches its observers, as
      // a browser's would.
      const resize = () =>
        act(() => {
          for (const { callback, targets } of observers) {
            if (targets.includes(scroll) || targets.includes(list)) {
              callback([], {} as ResizeObserver);
            }
          }
        });

      resize();
      const more = screen.getByRole("button", { name: "Load earlier messages" });
      more.focus();
      for (let n = 0; n < 4; n += 1) {
        resize();
      }
      // The same button, never taken down and put back; the newest message
      // still in view under it.
      expect(screen.getByRole("button", { name: "Load earlier messages" })).toBe(more);
      expect(more).toHaveFocus();
      expect(scrollTop).toBe(rows + 48);
      fireEvent.click(more);
      expect(loadEarlierMessages).toHaveBeenCalledWith(account.accountId, "!room:example.org");

      // The window shrinks below the rows: it scrolls, and the button goes.
      viewport = 360;
      resize();
      expect(screen.queryByText("Load earlier messages")).toBeNull();
      // It grows past them again: the button is back, the newest message kept.
      viewport = 420;
      scrollTop = 0;
      resize();
      expect(screen.getByRole("button", { name: "Load earlier messages" })).toBeInTheDocument();
      expect(scrollTop).toBe(rows + 48);
    } finally {
      vi.unstubAllGlobals();
    }
  });

  it("offers nothing more at the conversation's start", async () => {
    const { captured } = await renderOpen();
    captured.timeline?.({
      ops: [{ op: "reset", items: [messageItem("k1", "@bob:example.org", "hi")] }],
      pagination: { state: "idle", hitStart: true },
    });
    await screen.findByText("This is the start of the conversation");
    expect(screen.queryByRole("button", { name: "Load earlier messages" })).toBeNull();
  });

  it("pages older history when the wheel turns up over a window that cannot scroll", async () => {
    const { captured } = await renderOpen();
    captured.timeline?.({
      ops: [{ op: "reset", items: [messageItem("k1", "@bob:example.org", "hi")] }],
      pagination: { state: "idle", hitStart: false },
    });
    const region = await screen.findByLabelText("Messages");
    const scroll = region.parentElement as HTMLElement;
    setScrollGeometry(scroll, { scrollHeight: 400, clientHeight: 400, scrollTop: 0 });
    fireEvent.wheel(scroll, { deltaY: 40 });
    expect(paginateBackwards).not.toHaveBeenCalled();
    fireEvent.wheel(scroll, { deltaY: -40 });
    await waitFor(() =>
      expect(paginateBackwards).toHaveBeenCalledWith(
        account.accountId,
        "!room:example.org",
        expect.any(Number),
      ),
    );
  });

  it("says Rust's failed fill with a Retry that pages again", async () => {
    const { captured } = await renderOpen();
    captured.timeline?.({
      ops: [{ op: "reset", items: [{ kind: "other", key: "o1" }] }],
      pagination: { state: "failed", hitStart: false },
    });
    const retry = await screen.findByRole("button", { name: "Retry" });
    expect(screen.getByText("Couldn't load older messages.")).toBeInTheDocument();
    // A later status from the SDK does not clear the failure; only a retry does.
    captured.pagination?.({ state: "idle", hitStart: false });
    expect(screen.getByText("Couldn't load older messages.")).toBeInTheDocument();
    // The Retry goes to Rust's fill, which pages on from its failure (R509).
    fireEvent.click(retry);
    expect(loadEarlierMessages).toHaveBeenCalledWith(account.accountId, "!room:example.org");
    expect(paginateBackwards).not.toHaveBeenCalled();
    expect(screen.queryByText("Couldn't load older messages.")).not.toBeInTheDocument();
  });

  it("says a Retry that could not reach Rust as the failure again", async () => {
    loadEarlierMessages.mockRejectedValueOnce(new Error("no open view"));
    const { captured } = await renderOpen();
    captured.timeline?.({
      ops: [{ op: "reset", items: [messageItem("k1", "@bob:example.org", "hi")] }],
      pagination: { state: "failed", hitStart: false },
    });
    fireEvent.click(await screen.findByRole("button", { name: "Retry" }));
    await screen.findByText("Couldn't load older messages.");
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

    // Retry asks Rust for older history and clears the error.
    fireEvent.click(screen.getByRole("button", { name: "Retry" }));
    expect(loadEarlierMessages).toHaveBeenCalledTimes(1);
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
    networkId: null,
    muteState: "none",
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

const whatsappCaveat: CouplingCaveatVm = {
  networkId: "whatsapp",
  text: "you may also stop seeing others' read receipts",
  appliesTo: "read-receipts",
};

const whatsappCaveatTyping: CouplingCaveatVm = {
  networkId: "whatsapp",
  text: "typing indicators may also be affected on this network",
  appliesTo: "typing",
};

function incognitoVm(overrides: Partial<IncognitoVm> = {}): IncognitoVm {
  return {
    effective: true,
    source: "chat",
    global: false,
    account: null,
    chat: true,
    ...overrides,
  };
}

describe("ConversationPane — Incognito control (Story 8.2)", () => {
  beforeEach(() => {
    pinsRoomsStore.getState().clear();
    favoritesRoomsStore.getState().clear();
    archiveRoomsStore.getState().clear();
    incognitoStore.getState().clear();
    subscribeTimeline.mockResolvedValue(1);
  });

  afterEach(() => {
    pinsRoomsStore.getState().clear();
    favoritesRoomsStore.getState().clear();
    archiveRoomsStore.getState().clear();
    incognitoStore.getState().clear();
  });

  /** Render the pane with an open room whose `networkId` is `network`, mirroring an
   * effective (or not) Incognito VM into the store first. */
  function renderWithRoom(networkId: string | null, vm: IncognitoVm) {
    accountsStore.getState().hydrateAll([account]);
    incognitoStore.getState().applyVm(account.accountId, "!room:example.org", vm);
    roomsStore.getState().applyBatch({
      ops: [{ op: "reset", rooms: [headerRoom({ networkId })] }],
      total: 1,
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    return render(<ConversationPane {...noopProps()} />);
  }

  it("shows the effective-scope chip label (chat > account > global)", async () => {
    couplingCaveats.mockResolvedValue([]);
    renderWithRoom(null, incognitoVm({ source: "chat" }));
    expect(
      await screen.findByRole("button", { name: "Incognito — this chat overrides account" }),
    ).toBeInTheDocument();
  });

  it("renders the account and global scope labels too", () => {
    couplingCaveats.mockResolvedValue([]);
    const { rerender } = renderWithRoom(null, incognitoVm({ source: "account" }));
    expect(screen.getByRole("button", { name: "Incognito — account" })).toBeInTheDocument();

    incognitoStore
      .getState()
      .applyVm(account.accountId, "!room:example.org", incognitoVm({ source: "global" }));
    rerender(<ConversationPane {...noopProps()} />);
    expect(screen.getByRole("button", { name: "Incognito — global" })).toBeInTheDocument();
  });

  it("surfaces the coupling caveat inline for a WhatsApp room", async () => {
    couplingCaveats.mockResolvedValue([whatsappCaveat]);
    renderWithRoom("whatsapp", incognitoVm());
    // Open the popover, then assert the resolved caveat text is shown.
    fireEvent.click(
      screen.getByRole("button", { name: "Incognito — this chat overrides account" }),
    );
    expect(await screen.findByText(whatsappCaveat.text)).toBeInTheDocument();
  });

  it("shows no caveat for a native (null networkId) room", async () => {
    couplingCaveats.mockResolvedValue([whatsappCaveat]);
    renderWithRoom(null, incognitoVm());
    fireEvent.click(
      screen.getByRole("button", { name: "Incognito — this chat overrides account" }),
    );
    // The release action opens, proving the popover rendered — but no caveat text.
    expect(await screen.findByRole("button", { name: "Mark read publicly" })).toBeInTheDocument();
    expect(screen.queryByText(whatsappCaveat.text)).not.toBeInTheDocument();
  });

  it("invokes releaseReceipt exactly once when the user marks read publicly, then closes the popover", async () => {
    couplingCaveats.mockResolvedValue([]);
    renderWithRoom(null, incognitoVm());
    fireEvent.click(
      screen.getByRole("button", { name: "Incognito — this chat overrides account" }),
    );
    const release = await screen.findByRole("button", { name: "Mark read publicly" });
    fireEvent.click(release);
    await waitFor(() =>
      expect(releaseReceipt).toHaveBeenCalledWith(account.accountId, "!room:example.org"),
    );
    // Fired exactly once — the release action closes the popover so a stale re-click
    // can't double-fire.
    expect(releaseReceipt).toHaveBeenCalledTimes(1);
    // The popover closed after the action (the release button is no longer mounted).
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Mark read publicly" })).not.toBeInTheDocument(),
    );
  });

  it("turns Incognito off for the chat and closes the popover (effective → off)", async () => {
    couplingCaveats.mockResolvedValue([]);
    renderWithRoom(null, incognitoVm());
    fireEvent.click(
      screen.getByRole("button", { name: "Incognito — this chat overrides account" }),
    );
    const turnOff = await screen.findByRole("button", {
      name: "Turn Incognito off for this chat",
    });
    fireEvent.click(turnOff);
    await waitFor(() =>
      expect(incognitoSetChat).toHaveBeenCalledWith(account.accountId, "!room:example.org", false),
    );
    // The turn-off handler closes the popover after firing.
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "Turn Incognito off for this chat" }),
      ).not.toBeInTheDocument(),
    );
  });

  it("renders every coupling caveat a network has, not just the first", async () => {
    couplingCaveats.mockResolvedValue([whatsappCaveat, whatsappCaveatTyping]);
    renderWithRoom("whatsapp", incognitoVm());
    fireEvent.click(
      screen.getByRole("button", { name: "Incognito — this chat overrides account" }),
    );
    // Both matched caveats render, each on its own line.
    expect(await screen.findByText(whatsappCaveat.text)).toBeInTheDocument();
    expect(screen.getByText(whatsappCaveatTyping.text)).toBeInTheDocument();
  });

  it("offers an enable affordance (with caveat) when Incognito is off", async () => {
    couplingCaveats.mockResolvedValue([whatsappCaveat]);
    renderWithRoom("whatsapp", incognitoVm({ effective: false, source: "global", chat: null }));
    fireEvent.click(screen.getByRole("button", { name: "Incognito — off for this chat" }));
    const enable = await screen.findByRole("button", {
      name: "Turn Incognito on for this chat",
    });
    expect(enable).toBeInTheDocument();
    // FR-44: the coupling caveat surfaces at toggle time for a coupled network.
    expect(screen.getByText(whatsappCaveat.text)).toBeInTheDocument();
    fireEvent.click(enable);
    await waitFor(() =>
      expect(incognitoSetChat).toHaveBeenCalledWith(account.accountId, "!room:example.org", true),
    );
  });

  // The per-Chat scope is tri-state (the IPC carries `Option<bool>`): clearing an
  // override must write `null` so the chat resumes following account/global — writing
  // `false` would pin it off forever and silently ignore later account changes.
  it("clears an explicit per-chat override by writing null rather than false", async () => {
    couplingCaveats.mockResolvedValue([]);
    renderWithRoom(null, incognitoVm({ effective: true, source: "chat", chat: true }));
    fireEvent.click(
      screen.getByRole("button", { name: "Incognito — this chat overrides account" }),
    );
    fireEvent.click(
      await screen.findByRole("button", { name: "Follow the account/global setting" }),
    );
    await waitFor(() =>
      expect(incognitoSetChat).toHaveBeenCalledWith(account.accountId, "!room:example.org", null),
    );
    expect(incognitoSetChat).not.toHaveBeenCalledWith(
      account.accountId,
      "!room:example.org",
      false,
    );
  });

  it("offers the follow-the-setting item for an explicit off override and hides it while already inheriting", async () => {
    couplingCaveats.mockResolvedValue([]);
    // Explicitly off for this chat while the account says on — the override is what
    // makes the chip read off, so the off branch needs the clear affordance too.
    const { rerender } = renderWithRoom(
      null,
      incognitoVm({ effective: false, source: "chat", account: true, chat: false }),
    );
    fireEvent.click(screen.getByRole("button", { name: "Incognito — off for this chat" }));
    fireEvent.click(
      await screen.findByRole("button", { name: "Follow the account/global setting" }),
    );
    await waitFor(() =>
      expect(incognitoSetChat).toHaveBeenCalledWith(account.accountId, "!room:example.org", null),
    );

    // With no per-Chat override there is nothing to clear, so the item is absent.
    incognitoStore
      .getState()
      .applyVm(
        account.accountId,
        "!room:example.org",
        incognitoVm({ source: "account", account: true, chat: null }),
      );
    rerender(<ConversationPane {...noopProps()} />);
    fireEvent.click(screen.getByRole("button", { name: "Incognito — account" }));
    expect(await screen.findByRole("button", { name: "Mark read publicly" })).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Follow the account/global setting" }),
    ).not.toBeInTheDocument();
  });
});

describe("ConversationPane — safe areas & keyboard-avoiding composer (Story 13.5)", () => {
  /**
   * Mock matchMedia at a phone-tier width (mirrors phone-shell.test.tsx): any
   * `max-width: <bp>` query matches when the simulated viewport is below it, so
   * `useShellLayout().phone` reads true at 390px.
   */
  const originalMatchMedia = window.matchMedia;
  function mockViewportWidth(width: number) {
    window.matchMedia = vi.fn().mockImplementation((query: string) => {
      const match = query.match(/max-width:\s*(\d+)px/);
      const maxWidth = match ? Number(match[1]) : Number.POSITIVE_INFINITY;
      return {
        matches: width <= maxWidth,
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

  afterEach(() => {
    window.matchMedia = originalMatchMedia;
  });

  /** Select a room, render the pane, and stream one message into the timeline. */
  async function renderWithMessage() {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!room:example.org" });
    render(<ConversationPane {...noopProps()} />);
    await waitFor(() => {
      expect(captured.onBatch).not.toBeNull();
    });
    captured.onBatch?.({
      ops: [{ op: "reset", items: [messageItem("k1", "@bob:example.org", "hello")] }],
    });
    await waitFor(() => {
      expect(screen.getByText("hello")).toBeInTheDocument();
    });
  }

  it("phone: the composer footer carries the kb-inset + safe-bottom padding", async () => {
    mockViewportWidth(390);
    await renderWithMessage();
    // jsdom cannot evaluate env()/var(), so assert the class plumbing: the
    // footer floats at calc(var(--kb-inset, 0px) + var(--safe-bottom)).
    const footer = screen.getByTestId("composer-footer");
    expect(footer.className).toContain("pb-[calc(var(--kb-inset,0px)_+_var(--safe-bottom))]");
  });

  it("phone: the timeline scroller contains its overscroll", async () => {
    mockViewportWidth(390);
    await renderWithMessage();
    const scroller = screen.getByLabelText("Messages").parentElement;
    expect(scroller?.className).toContain("overscroll-contain");
  });

  it("desktop (≥768px): the composer footer carries no keyboard/safe-area inset", async () => {
    mockViewportWidth(1024);
    await renderWithMessage();
    const footer = screen.getByTestId("composer-footer");
    // Assert the semantic invariant (no keyboard/safe-area inset leaks onto
    // desktop), not a brittle exact-string match on the merged className.
    expect(footer.className).not.toContain("kb-inset");
    expect(footer.className).not.toContain("safe-bottom");
    expect(footer.className).toContain("border-t");
  });
});

describe("ConversationPane — an agent room", () => {
  const NIXI = "@nixi:example.org";
  const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };

  function agentHeader(
    run: AgentRunVm,
    caretKey: string | null,
  ): NonNullable<TimelineBatch["header"]> {
    return {
      status: {
        agent: NIXI,
        agentName: "Nixi",
        handle: "nixi@electra",
        host: "electra",
        title: "Nixi",
        kind: "main",
        run,
        waiting: null,
        detail: null,
        unreadable: null,
      },
      scope: null,
      label: null,
      scopeUnreadable: null,
      caretKey,
      gate: null,
    };
  }

  function open(props: { showHeader?: boolean } = {}) {
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(1);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: "!nixi:example.org" });
    render(<ConversationPane {...noopProps()} {...props} />);
  }

  function push(batch: TimelineBatch) {
    act(() => captured.onBatch?.(batch));
  }

  /** The message bubble (by its render key) that draws the growing caret, if any. */
  function caretOwners(): string[] {
    return Array.from(document.querySelectorAll('[data-slot="growing-caret"]')).map(
      (caret) => caret.closest("[data-msg-key]")?.getAttribute("data-msg-key") ?? "",
    );
  }

  const items = [
    messageItem("question", "@alice:example.org", "What did I write?"),
    messageItem("old-answer", NIXI, "Earlier answer"),
    messageItem("answer", NIXI, "Synapse"),
  ];

  it("a gate's room offers nothing that writes into it: no composer, reply, edit, reaction or drop (UX-DR147)", async () => {
    const released = vi.fn();
    onDragDropEvent.mockImplementation(() => Promise.resolve(released));
    open();
    push({
      ops: [{ op: "reset", items }],
      header: {
        ...agentHeader("done", null),
        gate: { system: "github", peer: "@github:example.org", peerName: '"GitHub"' },
      },
    });
    expect(await screen.findByTestId("gate-observer")).toBeInTheDocument();
    expect(screen.queryByTestId("composer-footer")).toBeNull();
    expect(screen.queryByPlaceholderText("Write a message…")).toBeNull();
    await screen.findByText("Earlier answer");
    expect(screen.queryByRole("button", { name: "Reply" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Add reaction" })).toBeNull();
    // The `r` key starts no reply on a selected line.
    act(() => composerStore.getState().select("question"));
    fireEvent.keyDown(screen.getByLabelText("Messages"), { key: "r" });
    expect(composerStore.getState().pending).toBeNull();
    // Every drop listener registered before the room was known is released.
    expect(onDragDropEvent).toHaveBeenCalled();
    await waitFor(() => expect(released).toHaveBeenCalledTimes(onDragDropEvent.mock.calls.length));
    // Any other agent room keeps its composer and its line actions.
    push({ ops: [], header: agentHeader("done", null) });
    expect(await screen.findByTestId("composer-footer")).toBeInTheDocument();
    expect(screen.getAllByRole("button", { name: "Reply" }).length).toBeGreaterThan(0);
    fireEvent.keyDown(screen.getByLabelText("Messages"), { key: "r" });
    await waitFor(() => expect(composerStore.getState().pending).not.toBeNull());
  });

  it("draws the caret on the named answer only while the run is running, then removes it", async () => {
    open();
    push({ ops: [{ op: "reset", items }], header: agentHeader("running", "answer") });
    await screen.findByText("Earlier answer");
    expect(caretOwners()).toEqual(["answer"]);
    expect(screen.getByText("(still writing)")).toBeInTheDocument();

    // The answer grows by `set`; the header did not change, so the batch has none.
    push({ ops: [{ op: "set", index: 2, item: messageItem("answer", NIXI, "Synapse took it") }] });
    expect(await screen.findByText(/Synapse took it/)).toBeInTheDocument();
    expect(caretOwners()).toEqual(["answer"]);

    push({ ops: [], header: agentHeader("done", null) });
    await waitFor(() => expect(caretOwners()).toEqual([]));
    expect(screen.queryByText("(still writing)")).not.toBeInTheDocument();
  });

  it("draws no caret once the run has left running, whatever key the header still names", async () => {
    open();
    push({ ops: [{ op: "reset", items }], header: agentHeader("blocked", "answer") });
    await screen.findByText("Earlier answer");
    expect(caretOwners()).toEqual([]);
  });

  it("keeps the header through batches that carry none", async () => {
    open();
    push({ ops: [{ op: "reset", items }], header: agentHeader("running", "answer") });
    push({ ops: [{ op: "pushBack", item: messageItem("more", NIXI, "More") }] });
    await screen.findByText("More");
    const region = screen.getByRole("region", { name: "Agent status" });
    expect(within(region).getByText("nixi@electra")).toBeInTheDocument();
    expect(within(region).getByTestId("agent-run")).toHaveTextContent(/^run: running$/);
  });

  it("shows the header on the phone, where the pane's own header row is hidden", async () => {
    open({ showHeader: false });
    push({ ops: [{ op: "reset", items }], header: agentHeader("idle", null) });
    const region = await screen.findByRole("region", { name: "Agent status" });
    expect(within(region).getByTestId("agent-run")).toHaveTextContent(/^run: idle$/);
    expect(screen.queryByRole("button", { name: "Toggle detail panel" })).not.toBeInTheDocument();
  });

  it("draws no header in a room whose stream carries none", async () => {
    open();
    push({ ops: [{ op: "reset", items }] });
    await screen.findByText("Earlier answer");
    expect(screen.queryByRole("region", { name: "Agent status" })).not.toBeInTheDocument();
    expect(caretOwners()).toEqual([]);
  });

  /** The message bubbles (by render key) that draw the heard mark (UX-DR143). */
  function markOwners(): string[] {
    return Array.from(document.querySelectorAll('[data-slot="heard-mark"]')).map(
      (mark) => mark.closest("[data-msg-key]")?.getAttribute("data-msg-key") ?? "",
    );
  }

  it("draws where the person stopped listening in the answer the mark names, and moves it as Rust does", async () => {
    open();
    push({
      ops: [
        {
          op: "reset",
          items: [
            ...items.slice(0, 2),
            messageItem("answer", NIXI, "Synapse took it. The cut sits at 45 KiB."),
          ],
        },
      ],
      header: agentHeader("done", null),
      heard: [{ key: "answer", splitUtf16: 16 }],
    });
    await screen.findByText("Earlier answer");
    expect(markOwners()).toEqual(["answer"]);
    expect(screen.getByText("The cut sits at 45 KiB.")).toHaveClass("opacity-60");

    // A batch beside the stream that names no answer clears it.
    push({ ops: [], heard: [] });
    await waitFor(() => expect(markOwners()).toEqual([]));
    expect(screen.getByText("Synapse took it. The cut sits at 45 KiB.")).toBeInTheDocument();
  });
});

describe("ConversationPane — an Undo lands in the room it was held in (R447)", () => {
  const roomA = "!room-a:example.org";
  const roomB = "!room-b:example.org";

  function harness() {
    const captured: { onBatch: ((b: TimelineBatch) => void) | null } = { onBatch: null };
    let nextId = 1;
    subscribeTimeline.mockImplementation((_a, _r, onBatch: (b: TimelineBatch) => void) => {
      captured.onBatch = onBatch;
      return Promise.resolve(nextId++);
    });
    roomsStore.getState().selectRoom({ accountId: account.accountId, roomId: roomA });
    const { rerender } = render(<ConversationPane {...noopProps()} />);
    const open = async (roomId: string) => {
      roomsStore.getState().selectRoom({ accountId: account.accountId, roomId });
      rerender(<ConversationPane {...noopProps()} />);
      await waitFor(() =>
        expect(subscribeTimeline).toHaveBeenCalledWith(
          account.accountId,
          roomId,
          expect.any(Function),
        ),
      );
      act(() => captured.onBatch?.({ ops: [message(`k-${roomId}`, "@bob:example.org", "hi")] }));
      const field = await screen.findByLabelText<HTMLTextAreaElement>("Message");
      await waitFor(() => expect(field).not.toBeDisabled());
      return field;
    };
    const hold = (roomId: string, filename: string | null) => {
      const now = Date.now();
      act(() => {
        outboxStore.getState().applySnapshot(account.accountId, roomId, [
          {
            id: "held-1",
            accountId: account.accountId,
            roomId,
            body: "for A",
            heldAtMs: now,
            dispatchAtMs: now + 30_000,
            filename,
            reply: false,
          },
        ]);
      });
    };
    return { open, hold };
  }

  it("keeps an attachment undone in one room out of the room opened before it resolved", async () => {
    let resolve: (restored: HeldRestoreVm) => void = () => {};
    cancelHeldSend.mockImplementation(
      () => new Promise<HeldRestoreVm>((answer) => (resolve = answer)),
    );
    const { open, hold } = harness();
    await open(roomA);
    hold(roomA, "plan.pdf");
    fireEvent.click(await screen.findByTestId("held-delete-button"));
    await waitFor(() =>
      expect(cancelHeldSend).toHaveBeenCalledWith(account.accountId, roomA, "held-1"),
    );

    // B is opened before the cancel resolves.
    const inB = await open(roomB);
    await act(async () => {
      resolve({
        line: "for A",
        reply: null,
        attachment: { path: "/data/outbox-files/held-1/plan.pdf", filename: "plan.pdf" },
      });
    });
    expect(attachmentsStore.getState().pending).toEqual([]);
    expect(inB.value).toBe("");
    fireEvent.change(inB, { target: { value: "hello B" } });
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() => expect(sendText).toHaveBeenCalledWith(account.accountId, roomB, "hello B"));
    expect(sendAttachmentPath).not.toHaveBeenCalled();

    // Back in A, the line and the file are where they were held.
    const inA = await open(roomA);
    await waitFor(() => expect(inA.value).toBe("for A"));
    expect(attachmentsStore.getState().pending).toEqual([
      expect.objectContaining({ path: "/data/outbox-files/held-1/plan.pdf", filename: "plan.pdf" }),
    ]);
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() =>
      expect(sendAttachmentPath).toHaveBeenCalledWith(
        account.accountId,
        roomA,
        "/data/outbox-files/held-1/plan.pdf",
        "for A",
      ),
    );
  });

  it("sends an undone reply to its original by event when the timeline does not show it", async () => {
    cancelHeldSend.mockResolvedValue({
      line: "for A",
      reply: {
        eventId: "$gone:example.org",
        key: null,
        sender: "@tola:example.org",
        senderDisplayName: "Tola Grey",
        body: "Which trip first?",
      },
      attachment: null,
    });
    const { open, hold } = harness();
    const field = await open(roomA);
    hold(roomA, null);
    fireEvent.click(await screen.findByTestId("held-delete-button"));
    expect(await screen.findByText("Replying to Tola Grey")).toBeInTheDocument();
    await waitFor(() => expect(field.value).toBe("for A"));
    fireEvent.click(screen.getByRole("button", { name: "Send message" }));
    await waitFor(() =>
      expect(sendReplyToEvent).toHaveBeenCalledWith(
        account.accountId,
        roomA,
        "$gone:example.org",
        "for A",
      ),
    );
    expect(sendReply).not.toHaveBeenCalled();
    expect(sendText).not.toHaveBeenCalled();
  });
});

describe("ConversationPane — the default agent's composer takes focus once it can (story 102.7, R102L1-01)", () => {
  const originalMatchMedia = window.matchMedia;
  function viewport(width: number) {
    window.matchMedia = vi.fn().mockImplementation((query: string) => {
      const match = query.match(/max-width:\s*(\d+)px/);
      return {
        matches: width <= (match ? Number(match[1]) : Number.POSITIVE_INFINITY),
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
  afterEach(() => {
    window.matchMedia = originalMatchMedia;
    primaryViewStore.getState().setView("inbox");
    composerStore.setState({ focusWhenReady: null });
  });

  const nixi = { accountId: account.accountId, roomId: "!nixi-dm:example.org" };
  const marta = { accountId: account.accountId, roomId: "!marta:example.org" };
  const firstBatch: TimelineBatch = {
    ops: [{ op: "reset", items: [messageItem("k1", "@nixi:example.org", "hello")] }],
  };

  /** The real pane, every room's first batch held until the test delivers it. */
  function heldFirstBatches(): Map<string, (b: TimelineBatch) => void> {
    const held = new Map<string, (b: TimelineBatch) => void>();
    subscribeTimeline.mockImplementation((_a, roomId: string, onBatch) => {
      held.set(roomId, onBatch);
      return Promise.resolve(1);
    });
    primaryViewStore.getState().setView("agents");
    render(<ConversationPane {...noopProps()} />);
    return held;
  }

  /** Every macrotask the launch's old zero-delay focus would have fired in. */
  async function letTimersRun() {
    await act(async () => {
      await new Promise((resolve) => setTimeout(resolve, 10));
    });
  }

  it.each([
    ["desktop", 1280],
    ["phone", 390],
  ])("%s: opened before its first batch, the composer is focused when the batch lands", async (_tier, width) => {
    viewport(width);
    const held = heldFirstBatches();
    act(() => openAndFocus(nixi));
    await waitFor(() => expect(held.has(nixi.roomId)).toBe(true));
    await letTimersRun();
    const field = screen.getByRole("textbox", { name: "Message" });
    expect(field).toBeDisabled();
    expect(field).not.toHaveFocus();

    act(() => held.get(nixi.roomId)?.(firstBatch));
    await waitFor(() => expect(screen.getByRole("textbox", { name: "Message" })).toHaveFocus());
    expect(composerStore.getState().focusWhenReady).toBeNull();
  });

  it("the person opening another room before the batch keeps the focus out of both", async () => {
    viewport(1280);
    const held = heldFirstBatches();
    act(() => openAndFocus(nixi));
    await waitFor(() => expect(held.has(nixi.roomId)).toBe(true));
    act(() => roomsStore.getState().selectRoom(marta));
    await waitFor(() => expect(held.has(marta.roomId)).toBe(true));

    act(() => held.get(marta.roomId)?.(firstBatch));
    const field = await screen.findByRole("textbox", { name: "Message" });
    await waitFor(() => expect(field).toBeEnabled());
    await letTimersRun();
    expect(field).not.toHaveFocus();
    expect(composerStore.getState().focusWhenReady).toBeNull();
  });

  it("the person leaving for another window before the batch drops the request", async () => {
    viewport(390);
    const held = heldFirstBatches();
    act(() => openAndFocus(nixi));
    await waitFor(() => expect(held.has(nixi.roomId)).toBe(true));
    act(() => primaryViewStore.getState().setView("inbox"));

    act(() => held.get(nixi.roomId)?.(firstBatch));
    const field = screen.getByRole("textbox", { name: "Message" });
    await waitFor(() => expect(field).toBeEnabled());
    await letTimersRun();
    expect(field).not.toHaveFocus();
    expect(composerStore.getState().focusWhenReady).toBeNull();
  });
});
