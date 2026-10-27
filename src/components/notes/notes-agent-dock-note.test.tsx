/**
 * The Notes Assistant on the note in front of the person (109.28, R-HQ-86), through the real
 * dock, session view and composer over a mocked IPC: it opens the chat Rust names for the note's
 * drive and says what ticking it allows, a message sent from it carries the note's caret,
 * and it follows the drive rather than the note.
 */
import { Text } from "@codemirror/state";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { DockBody, DRIVE_NOTICE_LABEL } from "@/components/notes/notes-agent-dock";
import { TooltipProvider } from "@/components/ui/tooltip";
import type * as Ipc from "@/lib/ipc/client";
import type { AgentNoteSessionVm, AgentSessionRef, AgentSessionViewVm } from "@/lib/ipc/client";
import { agentSessionsStore } from "@/lib/stores/agent-sessions";
import { noteCaretStore, resetNoteCaretForTest } from "@/lib/stores/note-caret";
import { panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";

const mock = vi.hoisted(() => ({
  opened: [] as { target: AgentSessionRef; show: (vm: AgentSessionViewVm) => void }[],
  forNote: vi.fn(),
  send: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-dialog", () => ({ open: vi.fn() }));
vi.mock("@tauri-apps/api/webview", () => ({
  getCurrentWebview: () => ({ onDragDropEvent: vi.fn(async () => () => {}) }),
}));
vi.mock("@/lib/failure", () => ({ logFailure: () => () => {}, sayFailure: () => () => {} }));
vi.mock("@/lib/ipc/client", async (original) => ({
  ...(await original<typeof Ipc>()),
  agentSessionDefault: vi.fn().mockResolvedValue({
    agentActor: "assistant",
    agentName: "Assistant",
    database: "alpha",
    sessionId: "main.1",
  }),
  agentSessionForNote: mock.forNote,
  agentsSessionsSubscribe: vi.fn(async () => "list"),
  agentsSessionsUnsubscribe: vi.fn().mockResolvedValue(undefined),
  agentSessionOpen: vi.fn(async (target: AgentSessionRef, show) => {
    mock.opened.push({ target, show });
    return "view";
  }),
  agentSessionClose: vi.fn().mockResolvedValue(undefined),
  agentSendLine: mock.send,
  agentMentionPreview: vi.fn(async (_target: unknown, before: string, after: string) => ({
    before,
    after,
    token: null,
    rows: [],
    note: null,
  })),
}));

const WARNING =
  "Once you tick a drive, the agent may read it and send what it reads to the web, in a search " +
  "or a page it fetches, without asking you, even after this chat has read outside content. " +
  "Text it read on the web or in a drive can lead it to do that.";

/** Rust's answer for a note on `drive`: the assistant's chat `sessionId` in that database. */
function onDrive(drive: string, sessionId: string, ticked: boolean): AgentNoteSessionVm {
  return {
    target: { agentActor: "assistant", agentName: "Assistant", database: drive, sessionId },
    drive,
    tickedNow: ticked,
    ...(ticked ? { warning: WARNING } : {}),
  };
}

function view({ database, sessionId }: AgentSessionRef): AgentSessionViewVm {
  return {
    database,
    sessionId,
    title: "Notes",
    displayName: "assistant · Notes",
    handle: "jolly-pigeon-squeaks",
    agentActor: "assistant",
    agentName: "Assistant",
    agentMark: "A",
    archived: false,
    readOnly: false,
    complete: true,
    hasEarlier: false,
    loadingEarlier: false,
    readsTrace: true,
    context: {
      used: null,
      window: null,
      label: "—",
      percent: null,
      meterPercent: null,
      ringBackground: "conic-gradient(currentColor 0deg, var(--muted) 0deg)",
      summary: "Context unknown: no answer has measured it yet.",
      parts: [],
    },
    footer: { model: "Default", effort: "Default", choosesModel: false, choosesEffort: false },
    live: [],
    tools: [],
    prefs: { viewMode: "normal" },
    approvals: [],
    items: [],
    folds: [],
    messages: [],
  };
}

function openNote(vaultId: string, noteId: string) {
  act(() => panelsStore.getState().setActiveTarget({ kind: "note", vaultId, noteId }));
}

const opened = () => mock.opened.map((entry) => entry.target);

/** Draw the newest opened session, so its composer is live. */
function showNewest() {
  const newest = mock.opened[mock.opened.length - 1];
  if (newest) act(() => newest.show(view(newest.target)));
}

function draw() {
  render(
    <TooltipProvider>
      <DockBody />
    </TooltipProvider>,
  );
}

beforeEach(() => {
  vi.clearAllMocks();
  mock.opened = [];
  mock.send.mockReset().mockResolvedValue(undefined);
  mock.forNote.mockReset();
  resetPanelsStoreForTest();
  resetNoteCaretForTest();
  agentSessionsStore.setState({ selected: null, rows: [], newSession: null });
});

afterEach(() => {
  resetPanelsStoreForTest();
  resetNoteCaretForTest();
});

it("opens the chat on the note's drive, says what ticking it allows, and sends the note", async () => {
  openNote("v1", "inbox");
  noteCaretStore
    .getState()
    .place("v1", "inbox", 3, Text.of(["# Plans", "## Q3", "ship it", "later"]));
  mock.forNote.mockResolvedValue(onDrive("tgdrive", "tg.1", true));
  draw();

  await waitFor(() => expect(mock.forNote).toHaveBeenCalledWith("v1", "inbox", null));
  const notice = await screen.findByRole("note", { name: DRIVE_NOTICE_LABEL });
  expect(notice).toHaveTextContent("tgdrive is ticked for Assistant to read.");
  expect(notice).toHaveTextContent(WARNING);
  // Straight to the drive's chat: the agent's main conversation elsewhere never opens here.
  await waitFor(() => expect(opened()).toEqual([{ database: "tgdrive", sessionId: "tg.1" }]));
  showNewest();

  fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
    target: { value: "what am I looking at?" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() =>
    expect(mock.send).toHaveBeenCalledWith(
      { database: "tgdrive", sessionId: "tg.1" },
      "what am I looking at?",
      [],
      { vaultId: "v1", noteId: "inbox", line: 3, text: "# Plans\n## Q3\nship it" },
    ),
  );
});

it("says why the note's drive was not ticked", async () => {
  openNote("v1", "inbox");
  mock.forNote.mockResolvedValue({
    ...onDrive("tgdrive", "tg.1", false),
    reason: "Assistant does not offer tgdrive.",
  });
  draw();
  const notice = await screen.findByRole("note", { name: DRIVE_NOTICE_LABEL });
  expect(notice).toHaveTextContent("Assistant does not offer tgdrive.");
  expect(notice).not.toHaveTextContent(WARNING);
});

it("follows the note's drive, not the note", async () => {
  openNote("v1", "inbox");
  mock.forNote.mockResolvedValueOnce(onDrive("tgdrive", "tg.1", true));
  draw();
  await waitFor(() => expect(opened()).toEqual([{ database: "tgdrive", sessionId: "tg.1" }]));

  // Another note of the same vault: the same drive, nothing asked.
  openNote("v1", "plans");
  await act(async () => {});
  expect(mock.forNote).toHaveBeenCalledTimes(1);

  // Another vault Rust names the same drive: the chat stays, whatever Rust would open.
  mock.forNote.mockResolvedValueOnce(onDrive("tgdrive", "tg.9", false));
  openNote("v2", "journal");
  await waitFor(() => expect(mock.forNote).toHaveBeenLastCalledWith("v2", "journal", null));
  await act(async () => {});
  expect(opened()).toEqual([{ database: "tgdrive", sessionId: "tg.1" }]);

  // A note on another drive: that drive's chat.
  mock.forNote.mockResolvedValueOnce(onDrive("neuradrive", "nd.1", true));
  openNote("v3", "lab");
  await waitFor(() =>
    expect(opened()).toEqual([
      { database: "tgdrive", sessionId: "tg.1" },
      { database: "neuradrive", sessionId: "nd.1" },
    ]),
  );
});
