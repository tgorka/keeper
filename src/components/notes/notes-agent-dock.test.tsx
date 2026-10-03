/**
 * The assistant dock beside the notes (UX-DR130), against a mocked IPC.
 *
 * Every claim here is one a person would notice going wrong: the dock opening on
 * something other than the DM, the chat losing its room to the dock, the scope
 * chip drawing the drives that were ASKED for instead of what the host accepted,
 * the proxy being told about a note while the dock is folded, or being left
 * believing the person is still looking at a note after the dock closed.
 */
import { Text } from "@codemirror/state";
import { act, cleanup, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm, AgentRoomHeaderVm, ProxyRoomVm, TimelineBatch } from "@/lib/ipc/client";

const subscribeTimeline = vi.fn();
const agentRoomsList = vi.fn();
const agentScopeSet = vi.fn();
const agentFocus = vi.fn();
const agentConversationNew = vi.fn();
const cancelHeldSend = vi.fn();
/** Every `agentFocus` call's number, in call order. */
let focusSeqs: number[];
vi.mock("@/lib/ipc/client", () => ({
  agentRoomsList: (accountId: string) => agentRoomsList(accountId),
  agentScopeSet: (accountId: string, roomId: string, drives: string[]) =>
    agentScopeSet(accountId, roomId, drives),
  agentFocus: (accountId: string, roomId: string, seq: number, focus: unknown) => {
    focusSeqs.push(seq);
    return agentFocus(accountId, roomId, focus);
  },
  agentConversationNew: (accountId: string, roomId: string, title: string | null) =>
    agentConversationNew(accountId, roomId, title),
  cancelHeldSend: (accountId: string, roomId: string, id: string) =>
    cancelHeldSend(accountId, roomId, id),
  subscribeTimeline: (accountId: string, roomId: string, onBatch: (b: TimelineBatch) => void) =>
    subscribeTimeline(accountId, roomId, onBatch),
  unsubscribeTimeline: vi.fn(async () => {}),
  markRoomRead: vi.fn(async () => {}),
  incognitoGet: vi.fn(async () => {
    throw new Error("not in this test");
  }),
  setTyping: vi.fn(async () => {}),
  paginateBackwards: vi.fn(async () => false),
  subscribeTyping: vi.fn(async () => 1),
  unsubscribeTyping: vi.fn(async () => {}),
  subscribePaginationStatus: vi.fn(async () => 2),
  unsubscribePaginationStatus: vi.fn(async () => {}),
  subscribeOutbox: vi.fn(async () => 3),
  unsubscribeOutbox: vi.fn(async () => {}),
  sendText: vi.fn(async () => {}),
  loadDraft: vi.fn(async () => null),
  saveDraft: vi.fn(async () => {}),
  clearDraft: vi.fn(async () => {}),
  loadRemoteDraft: vi.fn(async () => null),
  mirrorDraft: vi.fn(async () => {}),
  clearDraftMirror: vi.fn(async () => {}),
}));
const onDragDropEvent = vi.fn(() => Promise.resolve(() => {}));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent }),
}));

import { COLUMN_COLLAPSE_PREFIX } from "@/components/layout/surface-column";
import {
  DOCK_FOLDS_RAIL_BELOW_PX,
  FOCUS_HEARTBEAT_MS,
  NEW_CONVERSATION_POLL_MS,
  NotesAgentDock,
  RELIST_FIRST_MS,
} from "@/components/notes/notes-agent-dock";
import { TooltipProvider } from "@/components/ui/tooltip";
import { SURFACE_COLUMNS } from "@/lib/column-widths";
import { accountsStore } from "@/lib/stores/accounts";
import {
  columnFoldStore,
  columnsAtFirstRun,
  resetColumnFoldForTest,
} from "@/lib/stores/column-fold";
import { composerStore } from "@/lib/stores/composer";
import { noteCaretStore, resetNoteCaretForTest } from "@/lib/stores/note-caret";
import { outboxStore } from "@/lib/stores/outbox";
import { panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";
import { roomsStore } from "@/lib/stores/rooms";
import { timelineStore } from "@/lib/stores/timeline";

/** A note body of `count` lines, `line N` each. */
function body(count: number): Text {
  return Text.of(Array.from({ length: count }, (_, i) => `line ${i + 1}`));
}

/** The body's first `line` lines: what the dock sends Rust to name the heading from. */
function through(line: number): string {
  return Array.from({ length: line }, (_, i) => `line ${i + 1}`).join("\n");
}

function placeCaret(vaultId: string, noteId: string, line: number, doc: Text = body(40)): void {
  act(() => noteCaretStore.getState().place(vaultId, noteId, line, doc));
}

const ACCOUNT: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@tgorka:example.org",
  homeserverUrl: "https://matrix.example.org/",
  hueIndex: 0,
  provider: "password",
};
const DM = "!nixi-dm:example.org";
const READING = "!nixi-reading:example.org";
const CHAT = "!marta:example.org";
const ALLOWED = [
  { id: "tgdrive", title: "tgdrive" },
  { id: "neura", title: "Neura" },
  { id: "private", title: "Private notes" },
];

function proxyRoom(roomId: string, name: string, kind: "main" | "conversation"): ProxyRoomVm {
  return { roomId, name, kind, agent: "@nixi:example.org", allowed: ALLOWED };
}

function header(scope: string[]): AgentRoomHeaderVm {
  return {
    status: {
      agent: "@nixi:example.org",
      agentName: "Nixi",
      handle: "nixi@electra",
      host: "electra",
      title: null,
      kind: "main",
      run: "idle",
      waiting: null,
      detail: null,
      unreadable: null,
    },
    scope: scope.map((id) => ALLOWED.find((drive) => drive.id === id) ?? { id, title: id }),
    label: null,
    scopeUnreadable: null,
    caretKey: null,
  };
}

/** The open timelines, by room: the dock's `onBatch` for each. */
let streams: Map<string, (batch: TimelineBatch) => void>;

function push(roomId: string, batch: TimelineBatch): void {
  const onBatch = streams.get(roomId);
  if (onBatch === undefined) throw new Error(`${roomId} is not open`);
  act(() => onBatch(batch));
}

function renderDock() {
  return render(
    <TooltipProvider>
      <NotesAgentDock />
    </TooltipProvider>,
  );
}

function openDock(): void {
  act(() => columnFoldStore.getState().toggleColumn("notes-agent"));
}

function scopeChips(): string[] {
  const list = screen.queryByRole("list", { name: "Drives in scope" });
  return list === null
    ? []
    : within(list)
        .getAllByRole("listitem")
        .map((li) => li.textContent ?? "");
}

beforeEach(() => {
  focusSeqs = [];
  cancelHeldSend.mockReset();
  cancelHeldSend.mockResolvedValue("");
  outboxStore.getState().clear();
  streams = new Map();
  subscribeTimeline.mockReset();
  subscribeTimeline.mockImplementation(
    async (_account: string, roomId: string, onBatch: (b: TimelineBatch) => void) => {
      streams.set(roomId, onBatch);
      return streams.size;
    },
  );
  agentRoomsList.mockReset();
  agentRoomsList.mockResolvedValue([
    proxyRoom(DM, "Nixi", "main"),
    proxyRoom(READING, "Nixi — reading list", "conversation"),
  ]);
  agentScopeSet.mockReset();
  agentScopeSet.mockResolvedValue(undefined);
  agentFocus.mockReset();
  agentFocus.mockResolvedValue(undefined);
  agentConversationNew.mockReset();
  agentConversationNew.mockResolvedValue("$request:example.org");
  accountsStore.setState({ accounts: [ACCOUNT] });
  roomsStore.getState().selectRoom({ accountId: ACCOUNT.accountId, roomId: CHAT });
  timelineStore.getState().clear();
  composerStore.getState().clear();
  composerStore.getState().clearSelection();
  resetColumnFoldForTest();
  // What a keeper that has never been folded shows: the dock on its rail.
  columnFoldStore.setState({ columns: columnsAtFirstRun() });
  resetPanelsStoreForTest();
  resetNoteCaretForTest();
});

afterEach(() => {
  // Unmount first: resetting the stores under a mounted dock is an update nobody asked for.
  cleanup();
  accountsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  timelineStore.getState().clear();
  resetColumnFoldForTest();
  resetPanelsStoreForTest();
});

describe("the assistant dock", () => {
  it("starts folded and asks the proxy nothing until it is opened", async () => {
    panelsStore.getState().setActiveTarget({ kind: "note", vaultId: "v1", noteId: "n1" });
    renderDock();

    expect(
      screen.getByRole("button", { name: `Expand ${SURFACE_COLUMNS["notes-agent"].label}` }),
    ).toBeInTheDocument();
    expect(agentRoomsList).not.toHaveBeenCalled();
    expect(agentFocus).not.toHaveBeenCalled();
    expect(subscribeTimeline).not.toHaveBeenCalled();
  });

  it("opens on the DM, which Rust lists first", async () => {
    renderDock();
    openDock();

    await waitFor(() =>
      expect(subscribeTimeline).toHaveBeenCalledWith(ACCOUNT.accountId, DM, expect.any(Function)),
    );
    expect(screen.getByRole("combobox", { name: "Conversation" })).toHaveTextContent("Nixi");
    expect(subscribeTimeline).not.toHaveBeenCalledWith(
      ACCOUNT.accountId,
      READING,
      expect.any(Function),
    );
  });

  it("switches the docked room and leaves the chat's room, timeline and composer alone", async () => {
    timelineStore
      .getState()
      .applyBatch({ ops: [{ op: "reset", items: [{ kind: "other", key: "chat-1" }] }] });
    composerStore.getState().select("chat-1");
    renderDock();
    openDock();
    await waitFor(() => expect(streams.has(DM)).toBe(true));
    push(DM, { ops: [{ op: "reset", items: [] }], header: header(["tgdrive"]) });

    fireEvent.keyDown(screen.getByRole("combobox", { name: "Conversation" }), { key: "Enter" });
    fireEvent.click(await screen.findByRole("option", { name: "Nixi — reading list" }));

    await waitFor(() => expect(streams.has(READING)).toBe(true));
    push(READING, { ops: [{ op: "reset", items: [{ kind: "other", key: "dock-1" }] }] });

    expect(roomsStore.getState().selected).toEqual({ accountId: ACCOUNT.accountId, roomId: CHAT });
    expect(timelineStore.getState().items).toEqual([{ kind: "other", key: "chat-1" }]);
    expect(timelineStore.getState().header).toBeNull();
    expect(composerStore.getState().selectedKey).toBe("chat-1");
  });

  it("draws the scope the host echoed, not the one it asked for", async () => {
    renderDock();
    openDock();
    await waitFor(() => expect(streams.has(DM)).toBe(true));
    push(DM, { ops: [{ op: "reset", items: [] }], header: header(["tgdrive"]) });
    expect(scopeChips()).toEqual(["tgdrive"]);

    fireEvent.click(screen.getByRole("button", { name: "Choose drives in scope" }));
    const home = await screen.findByRole("checkbox", { name: /tgdrive/ });
    expect(home).toBeChecked();
    expect(home).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox", { name: "Neura" }));
    fireEvent.click(screen.getByRole("button", { name: "Ask for these drives" }));

    await waitFor(() =>
      expect(agentScopeSet).toHaveBeenCalledWith(ACCOUNT.accountId, DM, ["tgdrive", "neura"]),
    );
    // Asked is not accepted: the chip waits for the host.
    expect(scopeChips()).toEqual(["tgdrive"]);

    push(DM, { ops: [], header: header(["tgdrive", "neura"]) });
    expect(scopeChips()).toEqual(["tgdrive", "Neura"]);
  });

  it("starts the editor from the echoed scope, so unticking sends the rest", async () => {
    renderDock();
    openDock();
    await waitFor(() => expect(streams.has(DM)).toBe(true));
    push(DM, {
      ops: [{ op: "reset", items: [] }],
      header: header(["tgdrive", "neura", "private"]),
    });

    fireEvent.click(screen.getByRole("button", { name: "Choose drives in scope" }));
    fireEvent.click(await screen.findByRole("checkbox", { name: "Neura" }));
    fireEvent.click(screen.getByRole("button", { name: "Ask for these drives" }));

    await waitFor(() =>
      expect(agentScopeSet).toHaveBeenCalledWith(ACCOUNT.accountId, DM, ["tgdrive", "private"]),
    );
  });

  it("offers no editor where the proxy's drives are not on this device", async () => {
    agentRoomsList.mockResolvedValue([{ ...proxyRoom(DM, "Nixi", "main"), allowed: null }]);
    renderDock();
    openDock();
    await waitFor(() => expect(streams.has(DM)).toBe(true));
    push(DM, { ops: [{ op: "reset", items: [] }], header: header(["tgdrive"]) });

    expect(scopeChips()).toEqual(["tgdrive"]);
    expect(
      screen.queryByRole("button", { name: "Choose drives in scope" }),
    ).not.toBeInTheDocument();
  });

  it("tells the room which note and line are in front of you, and that there is none when it closes", async () => {
    panelsStore.getState().setActiveTarget({ kind: "note", vaultId: "v1", noteId: "n1" });
    renderDock();
    openDock();

    await waitFor(() =>
      expect(agentFocus).toHaveBeenLastCalledWith(ACCOUNT.accountId, DM, {
        vaultId: "v1",
        noteId: "n1",
        line: 1,
      }),
    );

    placeCaret("v1", "n1", 7);
    expect(agentFocus).toHaveBeenLastCalledWith(ACCOUNT.accountId, DM, {
      vaultId: "v1",
      noteId: "n1",
      line: 7,
      text: through(7),
    });

    // Another note's caret is not this note's.
    placeCaret("v1", "n2", 30);
    expect(agentFocus).toHaveBeenLastCalledWith(ACCOUNT.accountId, DM, {
      vaultId: "v1",
      noteId: "n1",
      line: 7,
      text: through(7),
    });

    act(() =>
      panelsStore.getState().setActiveTarget({ kind: "note", vaultId: "v1", noteId: "n2" }),
    );
    expect(agentFocus).toHaveBeenLastCalledWith(ACCOUNT.accountId, DM, {
      vaultId: "v1",
      noteId: "n2",
      line: 30,
      text: through(30),
    });

    const calls = agentFocus.mock.calls.length;
    fireEvent.click(
      screen.getByRole("button", {
        name: `${COLUMN_COLLAPSE_PREFIX} ${SURFACE_COLUMNS["notes-agent"].label}`,
      }),
    );
    expect(agentFocus.mock.calls.slice(calls)).toEqual([[ACCOUNT.accountId, DM, null]]);
    // F2: every call's number is larger than the one before, so Rust drops a
    // focus that arrives after the close it preceded.
    expect(focusSeqs.every((seq, i) => i === 0 || seq > focusSeqs[i - 1])).toBe(true);

    // Folded, a caret moving is nobody's business.
    placeCaret("v1", "n2", 31);
    expect(agentFocus.mock.calls.length).toBe(calls + 1);
  });

  it("says the note again on a heartbeat while it stays open (D3)", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      panelsStore.getState().setActiveTarget({ kind: "note", vaultId: "v1", noteId: "n1" });
      renderDock();
      openDock();
      await waitFor(() => expect(agentFocus).toHaveBeenCalled());
      const calls = agentFocus.mock.calls.length;
      act(() => {
        vi.advanceTimersByTime(FOCUS_HEARTBEAT_MS);
      });
      expect(agentFocus.mock.calls.slice(calls)).toEqual([
        [ACCOUNT.accountId, DM, { vaultId: "v1", noteId: "n1", line: 1 }],
      ]);
    } finally {
      vi.useRealTimers();
    }
  });

  it("tells the room it leaves that there is no note, and the room it joins which one", async () => {
    panelsStore.getState().setActiveTarget({ kind: "note", vaultId: "v1", noteId: "n1" });
    renderDock();
    openDock();
    await waitFor(() => expect(agentFocus).toHaveBeenCalled());
    const calls = agentFocus.mock.calls.length;

    fireEvent.keyDown(screen.getByRole("combobox", { name: "Conversation" }), { key: "Enter" });
    fireEvent.click(await screen.findByRole("option", { name: "Nixi — reading list" }));

    await waitFor(() =>
      expect(agentFocus.mock.calls.slice(calls)).toEqual([
        [ACCOUNT.accountId, DM, null],
        [ACCOUNT.accountId, READING, { vaultId: "v1", noteId: "n1", line: 1 }],
      ]),
    );
  });

  it("asks the proxy for a new conversation in its DM", async () => {
    renderDock();
    openDock();
    await waitFor(() => expect(streams.has(DM)).toBe(true));

    fireEvent.click(screen.getByRole("button", { name: "New conversation" }));
    fireEvent.change(await screen.findByLabelText("Title (optional)"), {
      target: { value: "  Q3 plans " },
    });
    fireEvent.click(screen.getByRole("button", { name: "Ask Nixi" }));

    await waitFor(() =>
      expect(agentConversationNew).toHaveBeenCalledWith(ACCOUNT.accountId, DM, "Q3 plans"),
    );
    expect(await screen.findByText(/Asked Nixi for a new conversation/)).toBeInTheDocument();
  });

  it("leaves a file dropped on the window to the note, not its composer", async () => {
    onDragDropEvent.mockClear();
    renderDock();
    openDock();
    await waitFor(() => expect(streams.has(DM)).toBe(true));
    push(DM, { ops: [{ op: "reset", items: [] }], header: header(["tgdrive"]) });

    expect(await screen.findByLabelText("Message")).toBeInTheDocument();
    expect(onDragDropEvent).not.toHaveBeenCalled();
  });

  it("says so when the person has no proxy", async () => {
    agentRoomsList.mockResolvedValue([]);
    renderDock();
    openDock();

    expect(await screen.findByText(/You have no assistant yet/)).toBeInTheDocument();
    expect(subscribeTimeline).not.toHaveBeenCalled();
    expect(screen.queryByRole("combobox", { name: "Conversation" })).not.toBeInTheDocument();
  });

  it("looks again while it lists no assistant, and opens the DM once it is listed (D4)", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      agentRoomsList.mockResolvedValueOnce([]);
      renderDock();
      openDock();
      expect(await screen.findByText(/You have no assistant yet/)).toBeInTheDocument();
      await act(async () => {
        vi.advanceTimersByTime(RELIST_FIRST_MS);
      });
      await waitFor(() => expect(streams.has(DM)).toBe(true));
    } finally {
      vi.useRealTimers();
    }
  });

  it("says why it could not list without saying there is no assistant, and looks again", async () => {
    agentRoomsList.mockRejectedValueOnce(new Error("offline"));
    renderDock();
    openDock();

    expect(await screen.findByRole("alert")).toBeInTheDocument();
    expect(screen.queryByText(/You have no assistant yet/)).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: "Look again" }));
    await waitFor(() => expect(streams.has(DM)).toBe(true));
  });

  it("stops drawing a signed-out account's room before the relist answers (U3)", async () => {
    renderDock();
    openDock();
    await waitFor(() => expect(streams.has(DM)).toBe(true));

    agentRoomsList.mockReturnValue(new Promise(() => {}));
    act(() => accountsStore.setState({ accounts: [] }));
    expect(screen.queryByRole("combobox", { name: "Conversation" })).not.toBeInTheDocument();
  });

  it("opens the conversation it asked for once the proxy has made it (D7)", async () => {
    vi.useFakeTimers({ shouldAdvanceTime: true });
    try {
      const made = "!nixi-q3:example.org";
      renderDock();
      openDock();
      await waitFor(() => expect(streams.has(DM)).toBe(true));
      fireEvent.click(screen.getByRole("button", { name: "New conversation" }));
      fireEvent.click(await screen.findByRole("button", { name: "Ask Nixi" }));
      expect(await screen.findByText(/Asked Nixi for a new conversation/)).toBeInTheDocument();

      agentRoomsList.mockResolvedValue([
        proxyRoom(DM, "Nixi", "main"),
        proxyRoom(made, "Q3 plans", "conversation"),
        proxyRoom(READING, "Nixi — reading list", "conversation"),
      ]);
      await act(async () => {
        vi.advanceTimersByTime(NEW_CONVERSATION_POLL_MS);
      });
      await waitFor(() => expect(streams.has(made)).toBe(true));
      expect(screen.getByRole("combobox", { name: "Conversation" })).toHaveTextContent("Q3 plans");
      expect(screen.queryByText(/Asked Nixi for a new conversation/)).not.toBeInTheDocument();
    } finally {
      vi.useRealTimers();
    }
  });

  it("forgets the last attempt's error when the form opens again (D7)", async () => {
    agentConversationNew.mockRejectedValueOnce(new Error("offline"));
    renderDock();
    openDock();
    await waitFor(() => expect(streams.has(DM)).toBe(true));
    const trigger = screen.getByRole("button", { name: "New conversation" });
    fireEvent.click(trigger);
    fireEvent.click(await screen.findByRole("button", { name: "Ask Nixi" }));
    expect(await screen.findByRole("alert")).toBeInTheDocument();

    fireEvent.keyDown(document.activeElement ?? document.body, { key: "Escape" });
    await waitFor(() => expect(screen.queryByRole("button", { name: "Ask Nixi" })).toBeNull());
    fireEvent.click(trigger);
    await screen.findByRole("button", { name: "Ask Nixi" });
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("says it asked until the host answers, and never asks for what is already in scope (D6)", async () => {
    renderDock();
    openDock();
    await waitFor(() => expect(streams.has(DM)).toBe(true));
    push(DM, { ops: [{ op: "reset", items: [] }], header: header(["tgdrive"]) });

    fireEvent.click(screen.getByRole("button", { name: "Choose drives in scope" }));
    const ask = await screen.findByRole("button", { name: "Ask for these drives" });
    expect(ask).toBeDisabled();
    fireEvent.click(screen.getByRole("checkbox", { name: "Neura" }));
    expect(ask).toBeEnabled();
    fireEvent.click(ask);

    expect(await screen.findByText("Asked Nixi; waiting for the answer.")).toBeInTheDocument();
    push(DM, { ops: [], header: header(["tgdrive", "neura"]) });
    expect(screen.queryByText("Asked Nixi; waiting for the answer.")).not.toBeInTheDocument();
  });

  it("does not wait for an answer that came before the ask resolved (D6, seen in Chrome)", async () => {
    agentScopeSet.mockImplementation(async () => {
      push(DM, { ops: [], header: header(["tgdrive", "neura"]) });
    });
    renderDock();
    openDock();
    await waitFor(() => expect(streams.has(DM)).toBe(true));
    push(DM, { ops: [{ op: "reset", items: [] }], header: header(["tgdrive"]) });

    fireEvent.click(screen.getByRole("button", { name: "Choose drives in scope" }));
    fireEvent.click(await screen.findByRole("checkbox", { name: "Neura" }));
    fireEvent.click(screen.getByRole("button", { name: "Ask for these drives" }));
    await waitFor(() => expect(scopeChips()).toEqual(["tgdrive", "Neura"]));
    await act(async () => {});
    expect(screen.queryByText(/waiting for the answer/)).not.toBeInTheDocument();
  });

  it("keeps a drive in scope that this device's allow does not list (D6)", async () => {
    renderDock();
    openDock();
    await waitFor(() => expect(streams.has(DM)).toBe(true));
    push(DM, { ops: [{ op: "reset", items: [] }], header: header(["tgdrive", "work"]) });

    fireEvent.click(screen.getByRole("button", { name: "Choose drives in scope" }));
    expect(await screen.findByRole("checkbox", { name: "work" })).toBeChecked();
    fireEvent.click(screen.getByRole("checkbox", { name: "Neura" }));
    fireEvent.click(screen.getByRole("button", { name: "Ask for these drives" }));
    await waitFor(() =>
      expect(agentScopeSet).toHaveBeenCalledWith(ACCOUNT.accountId, DM, [
        "tgdrive",
        "neura",
        "work",
      ]),
    );
  });

  it("folds the notes rail when it opens in a window too narrow for every column (D5)", () => {
    const width = window.innerWidth;
    Object.defineProperty(window, "innerWidth", {
      configurable: true,
      value: DOCK_FOLDS_RAIL_BELOW_PX - 1,
    });
    try {
      renderDock();
      expect(columnFoldStore.getState().columns["notes-rail"]).toBe(false);
      openDock();
      expect(columnFoldStore.getState().columns["notes-rail"]).toBe(true);
    } finally {
      Object.defineProperty(window, "innerWidth", { configurable: true, value: width });
    }
  });

  it("undoes its own held send into its own composer, and not from the note (U1, U4)", async () => {
    cancelHeldSend.mockResolvedValue("held body");
    render(
      <TooltipProvider>
        <textarea aria-label="Note" />
        <NotesAgentDock />
      </TooltipProvider>,
    );
    openDock();
    await waitFor(() => expect(streams.has(DM)).toBe(true));
    push(DM, { ops: [{ op: "reset", items: [] }], header: header(["tgdrive"]) });
    const now = Date.now();
    act(() =>
      outboxStore.getState().applySnapshot(ACCOUNT.accountId, DM, [
        {
          id: "h1",
          accountId: ACCOUNT.accountId,
          roomId: DM,
          body: "held body",
          heldAtMs: now,
          dispatchAtMs: now + 10_000,
        },
      ]),
    );
    await screen.findByTestId("undo-send-pill");

    // Redo in the note beside the dock is the note's.
    const note = screen.getByLabelText("Note");
    note.focus();
    fireEvent.keyDown(note, { key: "z", metaKey: true, shiftKey: true });
    expect(cancelHeldSend).not.toHaveBeenCalled();

    const message = await screen.findByLabelText("Message");
    message.focus();
    fireEvent.keyDown(message, { key: "z", metaKey: true, shiftKey: true });
    await waitFor(() => expect(cancelHeldSend).toHaveBeenCalledWith(ACCOUNT.accountId, DM, "h1"));
    // Restored into the dock's draft, never the chat's.
    await waitFor(() => expect(message).toHaveValue("held body"));
    expect(composerStore.getState().restoreBody).toBeNull();
  });
});
