/**
 * The Notes Assistant on the note in front of the person (109.28, R-HQ-86), through the real
 * dock, session view and composer over a mocked IPC: it opens the chat Rust names for the note's
 * drive and says what ticking it allows over that chat, a message sent from it carries the note's
 * caret, a new conversation's first message too, and it follows the drive rather than the note —
 * deciding which before it asks Rust to open or tick anything (R605-02).
 */
import { Text } from "@codemirror/state";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, expect, it, vi } from "vitest";
import { DockBody, DRIVE_NOTICE_LABEL } from "@/components/notes/notes-agent-dock";
import { TooltipProvider } from "@/components/ui/tooltip";
import type * as Ipc from "@/lib/ipc/client";
import type {
  AgentNoteSessionVm,
  AgentSessionRef,
  AgentSessionRowVm,
  AgentSessionViewVm,
} from "@/lib/ipc/client";
import { agentSessionsStore } from "@/lib/stores/agent-sessions";
import { noteCaretStore, resetNoteCaretForTest } from "@/lib/stores/note-caret";
import { panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";

const mock = vi.hoisted(() => ({
  opened: [] as { target: AgentSessionRef; show: (vm: AgentSessionViewVm) => void }[],
  forNote: vi.fn(),
  drive: vi.fn(),
  driveSet: vi.fn(),
  seen: vi.fn(),
  create: vi.fn(),
  preview: vi.fn(),
  command: vi.fn(),
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
  agentNoteDrive: mock.drive,
  agentSessionDriveSet: mock.driveSet,
  agentNoteTickSeen: mock.seen,
  agentSessionNew: mock.create,
  agentCommandPreview: mock.preview,
  agentCommand: mock.command,
  agentsChoices: vi.fn().mockResolvedValue([
    {
      agentActor: "assistant",
      agentName: "Assistant",
      agentMark: "A",
      databases: ["tgdrive"],
      database: "tgdrive",
      ask: false,
    },
  ]),
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
    footer: {
      model: "Default",
      effort: "Default",
      shortModel: "Default",
      effortCode: "D",
      choosesModel: false,
      choosesEffort: false,
    },
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

/** The drive Rust names for each vault's notes. */
const DRIVES: Record<string, string | null> = { v1: "tgdrive", v2: "tgdrive", v3: "neuradrive" };

function row(database: string, sessionId: string, title: string): AgentSessionRowVm {
  return {
    database,
    sessionId,
    title,
    titleWrite: { state: "confirmed" },
    displayName: `assistant · ${title}`,
    handle: "jolly-pigeon-squeaks",
    createdAtMs: 1,
    trust: "native",
    instances: [],
    agentActor: "assistant",
    agentName: "Assistant",
    agentMark: "A",
    archived: false,
    choosesDrives: true,
    choosesModel: false,
    lastActivityMs: 1,
  };
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
  mock.drive.mockReset().mockImplementation(async (vaultId: string) => DRIVES[vaultId] ?? null);
  mock.driveSet.mockReset().mockResolvedValue(undefined);
  mock.seen.mockReset().mockResolvedValue(undefined);
  mock.create.mockReset().mockResolvedValue({ database: "tgdrive", sessionId: "tg.2" });
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
  // Shown, so Rust no longer answers it as news (R605-05).
  await waitFor(() =>
    expect(mock.seen).toHaveBeenCalledWith({ database: "tgdrive", sessionId: "tg.1" }, "tgdrive"),
  );
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

const last = () => opened()[opened().length - 1];

it("follows the note's drive, not the note, and asks for no write on the drive it follows", async () => {
  openNote("v1", "inbox");
  mock.forNote.mockResolvedValueOnce(onDrive("tgdrive", "tg.1", true));
  draw();
  await waitFor(() => expect(opened()).toEqual([{ database: "tgdrive", sessionId: "tg.1" }]));
  expect(await screen.findByRole("note", { name: DRIVE_NOTICE_LABEL })).toBeInTheDocument();

  // The person picks another chat: it holds, and the tick's notice, about tg.1, goes with it.
  act(() =>
    agentSessionsStore.setState({
      rows: [row("tgdrive", "tg.1", "Notes"), row("alpha", "main.1", "Plan the release")],
    }),
  );
  fireEvent.keyDown(screen.getByRole("combobox", { name: "Conversation" }), { key: "Enter" });
  fireEvent.click(await screen.findByRole("option", { name: /Plan the release/ }));
  await waitFor(() => expect(last()).toEqual({ database: "alpha", sessionId: "main.1" }));
  expect(screen.queryByRole("note", { name: DRIVE_NOTICE_LABEL })).not.toBeInTheDocument();

  // Another note of the same vault, a file and back, another vault on the same drive: the drive
  // is read, nothing is opened or ticked, and the pick holds.
  openNote("v1", "plans");
  act(() =>
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "p1", relativePath: "a.txt" }),
  );
  openNote("v1", "inbox");
  openNote("v2", "journal");
  await waitFor(() => expect(mock.drive).toHaveBeenLastCalledWith("v2", "journal"));
  await act(async () => {});
  expect(mock.forNote).toHaveBeenCalledTimes(1);
  expect(last()).toEqual({ database: "alpha", sessionId: "main.1" });
  expect(screen.queryByRole("note", { name: DRIVE_NOTICE_LABEL })).not.toBeInTheDocument();

  // A note on another drive: that drive's chat.
  mock.forNote.mockResolvedValueOnce(onDrive("neuradrive", "nd.1", true));
  openNote("v3", "lab");
  await waitFor(() => expect(last()).toEqual({ database: "neuradrive", sessionId: "nd.1" }));
  expect(mock.forNote).toHaveBeenCalledTimes(2);
});

it("takes back a tick no one sees, when the person moved to another drive before it answered", async () => {
  let answer: (vm: AgentNoteSessionVm) => void = () => {};
  mock.forNote.mockImplementationOnce(
    () =>
      new Promise<AgentNoteSessionVm>((resolve) => {
        answer = resolve;
      }),
  );
  mock.forNote.mockResolvedValueOnce(onDrive("neuradrive", "nd.1", false));
  openNote("v1", "inbox");
  draw();
  await waitFor(() => expect(mock.forNote).toHaveBeenCalledTimes(1));
  openNote("v3", "lab");
  await waitFor(() => expect(mock.drive).toHaveBeenLastCalledWith("v3", "lab"));
  await act(async () => answer(onDrive("tgdrive", "tg.1", true)));

  await waitFor(() =>
    expect(mock.driveSet).toHaveBeenCalledWith(
      { database: "tgdrive", sessionId: "tg.1" },
      "tgdrive",
      false,
    ),
  );
  await waitFor(() => expect(opened()).toEqual([{ database: "neuradrive", sessionId: "nd.1" }]));
  expect(mock.forNote).toHaveBeenLastCalledWith("v3", "lab", null);
  expect(screen.queryByRole("note", { name: DRIVE_NOTICE_LABEL })).not.toBeInTheDocument();
});

it("sends the note with the first message of a conversation started in the dock", async () => {
  openNote("v1", "inbox");
  noteCaretStore.getState().place("v1", "inbox", 2, Text.of(["# Plans", "## Q3"]));
  mock.forNote.mockResolvedValue(onDrive("tgdrive", "tg.1", false));
  draw();
  await waitFor(() => expect(opened()).toEqual([{ database: "tgdrive", sessionId: "tg.1" }]));
  fireEvent.click(screen.getByRole("button", { name: "New conversation" }));
  const field = await screen.findByRole("textbox", { name: "Message" });
  fireEvent.change(field, { target: { value: "what is this note?" } });
  await waitFor(() => expect(screen.getByRole("button", { name: "Send message" })).toBeEnabled());
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() =>
    expect(mock.create).toHaveBeenCalledWith("tgdrive", "assistant", "what is this note?", null, {
      vaultId: "v1",
      noteId: "inbox",
      line: 2,
      text: "# Plans\n## Q3",
    }),
  );
});

/** `agentSessionForNote`'s next answer, held until the test gives it. */
function held(): (vm: AgentNoteSessionVm) => Promise<void> {
  let answer: (vm: AgentNoteSessionVm) => void = () => {};
  mock.forNote.mockImplementationOnce(
    () =>
      new Promise<AgentNoteSessionVm>((resolve) => {
        answer = resolve;
      }),
  );
  return async (vm) => {
    await act(async () => answer(vm));
  };
}

it("drops an answer for a note the person left for a file, and takes its tick back (R605-06)", async () => {
  const answer = held();
  openNote("v1", "inbox");
  draw();
  await waitFor(() => expect(mock.forNote).toHaveBeenCalledTimes(1));
  act(() =>
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "p1", relativePath: "a.txt" }),
  );
  await answer(onDrive("tgdrive", "tg.1", true));

  await waitFor(() =>
    expect(mock.driveSet).toHaveBeenCalledWith(
      { database: "tgdrive", sessionId: "tg.1" },
      "tgdrive",
      false,
    ),
  );
  await waitFor(() => expect(opened()).toEqual([{ database: "alpha", sessionId: "main.1" }]));
  expect(mock.seen).not.toHaveBeenCalled();
  expect(screen.queryByRole("note", { name: DRIVE_NOTICE_LABEL })).not.toBeInTheDocument();
});

it("drops an answer while the next note's drive is still being read (R605-06)", async () => {
  const answer = held();
  openNote("v1", "inbox");
  draw();
  await waitFor(() => expect(mock.forNote).toHaveBeenCalledTimes(1));
  let lookup: (drive: string | null) => void = () => {};
  mock.drive.mockImplementationOnce(
    () =>
      new Promise<string | null>((resolve) => {
        lookup = resolve;
      }),
  );
  openNote("v3", "lab");
  await waitFor(() => expect(mock.drive).toHaveBeenLastCalledWith("v3", "lab"));
  await answer(onDrive("tgdrive", "tg.1", true));
  await waitFor(() => expect(mock.driveSet).toHaveBeenCalledTimes(1));
  expect(opened()).toEqual([]);

  mock.forNote.mockResolvedValueOnce(onDrive("neuradrive", "nd.1", true));
  await act(async () => lookup("neuradrive"));
  await waitFor(() => expect(opened()).toEqual([{ database: "neuradrive", sessionId: "nd.1" }]));
  expect(mock.seen).toHaveBeenCalledWith(
    { database: "neuradrive", sessionId: "nd.1" },
    "neuradrive",
  );
});

it("never reports a tick seen that nobody saw, when the dock closed and the untick failed (R605-05)", async () => {
  const answer = held();
  mock.driveSet.mockRejectedValueOnce({ message: "The replica could not be written." });
  openNote("v1", "inbox");
  const { unmount } = render(
    <TooltipProvider>
      <DockBody />
    </TooltipProvider>,
  );
  await waitFor(() => expect(mock.forNote).toHaveBeenCalledTimes(1));
  unmount();
  await answer(onDrive("tgdrive", "tg.1", true));

  await waitFor(() => expect(mock.driveSet).toHaveBeenCalledTimes(1));
  await act(async () => {});
  // The tick stays news in Rust: the next open says it, warning and all.
  expect(mock.seen).not.toHaveBeenCalled();
  expect(opened()).toEqual([]);
});

it("follows /clean to the new chat: the notice stays with the chat it is about, the note goes on", async () => {
  openNote("v1", "inbox");
  noteCaretStore.getState().place("v1", "inbox", 2, Text.of(["# Plans", "## Q3"]));
  mock.forNote.mockResolvedValue(onDrive("tgdrive", "tg.1", true));
  mock.preview.mockImplementation(async (_target: unknown, draft: string) => ({
    draft,
    verdict: { kind: "command", name: "clean", args: null },
    rows: [],
    escapeHint: "",
  }));
  mock.command.mockResolvedValue({
    session: { database: "tgdrive", sessionId: "tg.2" },
    surface: null,
    message: "",
    pick: null,
  });
  draw();
  await waitFor(() => expect(opened()).toEqual([{ database: "tgdrive", sessionId: "tg.1" }]));
  expect(await screen.findByRole("note", { name: DRIVE_NOTICE_LABEL })).toBeInTheDocument();
  showNewest();

  const field = screen.getByRole("textbox", { name: "Message" });
  fireEvent.change(field, { target: { value: "/clean" } });
  fireEvent.keyDown(field, { key: "Enter" });
  await waitFor(() =>
    expect(opened()).toEqual([
      { database: "tgdrive", sessionId: "tg.1" },
      { database: "tgdrive", sessionId: "tg.2" },
    ]),
  );
  // The warning was said over tg.1, where the tick was made; tg.2 got its ticks by /clean's
  // carry, which is not the dock's to announce again.
  expect(screen.queryByRole("note", { name: DRIVE_NOTICE_LABEL })).not.toBeInTheDocument();
  // The window's own selection is not the dock's to move (R601-04).
  expect(agentSessionsStore.getState().selected).toBeNull();

  showNewest();
  fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
    target: { value: "and now?" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() =>
    expect(mock.send).toHaveBeenCalledWith(
      { database: "tgdrive", sessionId: "tg.2" },
      "and now?",
      [],
      {
        vaultId: "v1",
        noteId: "inbox",
        line: 2,
        text: "# Plans\n## Q3",
      },
    ),
  );
  // The switch asked nothing of the note's drive again.
  expect(mock.forNote).toHaveBeenCalledTimes(1);
});
