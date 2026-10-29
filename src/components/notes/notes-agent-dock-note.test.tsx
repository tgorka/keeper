/**
 * The Notes Assistant on the note in front of the person (109.28, R-HQ-86), through the real
 * dock, session view, drives control and composer over a mocked IPC whose fake keeps Rust's state:
 * which drives each chat has ticked, and which of those this person has not been shown the warning
 * for (round 6 ruling: the dock's ticks, never the person's own).
 * The dock opens the chat Rust names for the note's drive; the warning of its tick is the chat's
 * to draw, beside the chip, from that same state; a message sent from it carries the note's caret;
 * it follows the drive rather than the note, deciding which before it asks Rust to open or tick
 * anything (R605-02); and an answer it no longer wants changes nothing and takes nothing back.
 */
import { Text } from "@codemirror/state";
import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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

/**
 * Rust's state, as the fake keeps it: the drives each chat has ticked, and those not seen (ticked
 * by the dock and not yet answered with OK, Untick or the person's own tick). Every view of a chat
 * reads both, as `Hq::session_view` does.
 */
const rust = {
  ticked: new Map<string, Set<string>>(),
  unseen: new Set<string>(),
};
const chatKey = (database: string, sessionId: string) => `${database}/${sessionId}`;

/**
 * Rust's `session_for_note`, made now: the note's drive ticked in `sessionId`, not recorded as
 * seen, unless it was already ticked. What it answers carries no tick: the session shows it.
 */
function onDrive(drive: string, sessionId: string): AgentNoteSessionVm {
  const key = chatKey(drive, sessionId);
  const ticked = rust.ticked.get(key) ?? new Set<string>();
  if (!ticked.has(drive)) {
    rust.unseen.add(`${key}/${drive}`);
    ticked.add(drive);
    rust.ticked.set(key, ticked);
  }
  return {
    target: { agentActor: "assistant", agentName: "Assistant", database: drive, sessionId },
    drive,
  };
}

/** Rust's answer for a drive it did not tick, and why. */
function refused(drive: string, sessionId: string, reason: string): AgentNoteSessionVm {
  return {
    target: { agentActor: "assistant", agentName: "Assistant", database: drive, sessionId },
    drive,
    reason,
  };
}

function view({ database, sessionId }: AgentSessionRef): AgentSessionViewVm {
  const key = chatKey(database, sessionId);
  const ticked = [...(rust.ticked.get(key) ?? [])];
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
    drives: {
      drives: ["tgdrive", "neuradrive"].map((name) => ({ name, ticked: ticked.includes(name) })),
      warning: WARNING,
      pending: false,
      unseen: ticked
        .filter((name) => rust.unseen.has(`${key}/${name}`))
        .map((name) => ({ drive: name, tick: `tick-${key}/${name}` })),
    },
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
const last = () => opened()[opened().length - 1];

/** Draw every open view from Rust's state now, as its stream would. */
function showAll() {
  for (const entry of mock.opened) act(() => entry.show(view(entry.target)));
}

function draw() {
  return render(
    <TooltipProvider>
      <DockBody />
    </TooltipProvider>,
  );
}

/** The ticked chips and the unseen-tick warnings the shown chat draws: they must agree. */
function shown() {
  const chips = screen.queryByRole("list", { name: "Drives ticked" });
  return {
    chips: chips
      ? within(chips)
          .queryAllByRole("listitem")
          .map((item) => item.textContent ?? "")
          .filter((text) => text !== "No drive ticked")
      : [],
    warnings: screen
      .queryAllByRole("note", { name: /^What ticking \S+ allows$/ })
      .map((note) => (note.getAttribute("aria-label") ?? "").split(" ")[2]),
  };
}

/** `agentSessionForNote`'s next answer, held until the test gives it. */
function held(): (vm: () => AgentNoteSessionVm) => Promise<void> {
  let answer: (vm: AgentNoteSessionVm) => void = () => {};
  mock.forNote.mockImplementationOnce(
    () =>
      new Promise<AgentNoteSessionVm>((resolve) => {
        answer = resolve;
      }),
  );
  return async (vm) => {
    await act(async () => answer(vm()));
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  mock.opened = [];
  rust.ticked.clear();
  rust.unseen.clear();
  mock.send.mockReset().mockResolvedValue(undefined);
  mock.forNote.mockReset();
  mock.drive.mockReset().mockImplementation(async (vaultId: string) => DRIVES[vaultId] ?? null);
  // A person's own tick or untick: Rust's one path, which also clears the dock's mark.
  mock.driveSet
    .mockReset()
    .mockImplementation(async (target: AgentSessionRef, drive: string, tick: boolean) => {
      const key = chatKey(target.database, target.sessionId);
      const ticked = rust.ticked.get(key) ?? new Set<string>();
      if (tick) ticked.add(drive);
      else ticked.delete(drive);
      rust.ticked.set(key, ticked);
      rust.unseen.delete(`${key}/${drive}`);
      showAll();
    });
  mock.seen.mockReset().mockImplementation(async (target: AgentSessionRef, drive: string) => {
    rust.unseen.delete(`${chatKey(target.database, target.sessionId)}/${drive}`);
    showAll();
  });
  mock.create.mockReset().mockResolvedValue({ database: "tgdrive", sessionId: "tg.2" });
  resetPanelsStoreForTest();
  resetNoteCaretForTest();
  agentSessionsStore.setState({ selected: null, rows: [], newSession: null });
});

afterEach(() => {
  resetPanelsStoreForTest();
  resetNoteCaretForTest();
});

it("opens the chat on the note's drive, whose own view says what the tick allows, and sends the note", async () => {
  openNote("v1", "inbox");
  noteCaretStore
    .getState()
    .place("v1", "inbox", 3, Text.of(["# Plans", "## Q3", "ship it", "later"]));
  mock.forNote.mockImplementation(async () => onDrive("tgdrive", "tg.1"));
  draw();

  await waitFor(() => expect(mock.forNote).toHaveBeenCalledWith("v1", "inbox", null));
  // Straight to the drive's chat: the agent's main conversation elsewhere never opens here.
  await waitFor(() => expect(opened()).toEqual([{ database: "tgdrive", sessionId: "tg.1" }]));
  showAll();
  expect(shown()).toEqual({ chips: ["tgdrive"], warnings: ["tgdrive"] });
  expect(screen.getByRole("note", { name: "What ticking tgdrive allows" })).toHaveTextContent(
    WARNING,
  );
  // The dock says nothing of its own about a tick.
  expect(screen.queryByRole("note", { name: DRIVE_NOTICE_LABEL })).not.toBeInTheDocument();

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

  // OK: seen, and the drive stays ticked.
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "OK" }));
  });
  expect(mock.seen).toHaveBeenCalledWith(
    { database: "tgdrive", sessionId: "tg.1" },
    "tgdrive",
    "tick-tgdrive/tg.1/tgdrive",
  );
  expect(shown()).toEqual({ chips: ["tgdrive"], warnings: [] });
});

it("unticks from the warning through the person's own untick", async () => {
  openNote("v1", "inbox");
  mock.forNote.mockImplementation(async () => onDrive("tgdrive", "tg.1"));
  draw();
  await waitFor(() => expect(opened()).toEqual([{ database: "tgdrive", sessionId: "tg.1" }]));
  showAll();
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "Untick tgdrive" }));
  });
  expect(mock.driveSet).toHaveBeenCalledWith(
    { database: "tgdrive", sessionId: "tg.1" },
    "tgdrive",
    false,
  );
  expect(shown()).toEqual({ chips: [], warnings: [] });
});

it("says why the note's drive was not ticked", async () => {
  openNote("v1", "inbox");
  mock.forNote.mockResolvedValue(refused("tgdrive", "tg.1", "Assistant does not offer tgdrive."));
  draw();
  const notice = await screen.findByRole("note", { name: DRIVE_NOTICE_LABEL });
  expect(notice).toHaveTextContent("Assistant does not offer tgdrive.");
  expect(notice).not.toHaveTextContent(WARNING);
});

it("follows the note's drive, not the note, and asks for no write on the drive it follows (R605-02)", async () => {
  openNote("v1", "inbox");
  mock.forNote.mockImplementationOnce(async () => onDrive("tgdrive", "tg.1"));
  draw();
  await waitFor(() => expect(opened()).toEqual([{ database: "tgdrive", sessionId: "tg.1" }]));

  // The person picks another chat: it holds, and shows its own drives.
  act(() =>
    agentSessionsStore.setState({
      rows: [row("tgdrive", "tg.1", "Notes"), row("alpha", "main.1", "Plan the release")],
    }),
  );
  fireEvent.keyDown(screen.getByRole("combobox", { name: "Conversation" }), { key: "Enter" });
  fireEvent.click(await screen.findByRole("option", { name: /Plan the release/ }));
  await waitFor(() => expect(last()).toEqual({ database: "alpha", sessionId: "main.1" }));
  showAll();
  expect(shown()).toEqual({ chips: [], warnings: [] });

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

  // A note on another drive: that drive's chat.
  mock.forNote.mockImplementationOnce(async () => onDrive("neuradrive", "nd.1"));
  openNote("v3", "lab");
  await waitFor(() => expect(last()).toEqual({ database: "neuradrive", sessionId: "nd.1" }));
  expect(mock.forNote).toHaveBeenCalledTimes(2);
});

it("leaves an answer for a drive the person moved off unseen and unchosen, never taken back (R605-02)", async () => {
  const answer = held();
  mock.forNote.mockImplementationOnce(async () => onDrive("neuradrive", "nd.1"));
  openNote("v1", "inbox");
  draw();
  await waitFor(() => expect(mock.forNote).toHaveBeenCalledTimes(1));
  openNote("v3", "lab");
  await waitFor(() => expect(mock.drive).toHaveBeenLastCalledWith("v3", "lab"));
  await answer(() => onDrive("tgdrive", "tg.1"));

  await waitFor(() => expect(opened()).toEqual([{ database: "neuradrive", sessionId: "nd.1" }]));
  expect(mock.driveSet).not.toHaveBeenCalled();
  showAll();
  expect(shown()).toEqual({ chips: ["neuradrive"], warnings: ["neuradrive"] });
  // The tgdrive chat keeps its tick, unseen: opened anywhere, it says so.
  expect(
    view({ database: "tgdrive", sessionId: "tg.1" }).drives?.unseen?.map((tick) => tick.drive),
  ).toEqual(["tgdrive"]);
});

it("does nothing with an answer for a note the person left for a file (R605-06)", async () => {
  const answer = held();
  openNote("v1", "inbox");
  draw();
  await waitFor(() => expect(mock.forNote).toHaveBeenCalledTimes(1));
  act(() =>
    panelsStore
      .getState()
      .setActiveTarget({ kind: "file", profileId: "p1", relativePath: "a.txt" }),
  );
  await answer(() => onDrive("tgdrive", "tg.1"));

  await waitFor(() => expect(opened()).toEqual([{ database: "alpha", sessionId: "main.1" }]));
  showAll();
  expect(shown()).toEqual({ chips: [], warnings: [] });
  expect(mock.driveSet).not.toHaveBeenCalled();
  expect(screen.queryByRole("note", { name: DRIVE_NOTICE_LABEL })).not.toBeInTheDocument();
  expect(
    view({ database: "tgdrive", sessionId: "tg.1" }).drives?.unseen?.map((tick) => tick.drive),
  ).toEqual(["tgdrive"]);
});

it("does nothing with an answer while the next note's drive is still being read (R605-06)", async () => {
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
  await answer(() => onDrive("tgdrive", "tg.1"));
  await act(async () => {});
  expect(opened()).toEqual([]);

  mock.forNote.mockImplementationOnce(async () => onDrive("neuradrive", "nd.1"));
  await act(async () => lookup("neuradrive"));
  await waitFor(() => expect(opened()).toEqual([{ database: "neuradrive", sessionId: "nd.1" }]));
  expect(mock.driveSet).not.toHaveBeenCalled();
});

it("leaves an answer that came after the dock closed as an unseen tick (R605-05)", async () => {
  const answer = held();
  openNote("v1", "inbox");
  const first = draw();
  await waitFor(() => expect(mock.forNote).toHaveBeenCalledTimes(1));
  first.unmount();
  await answer(() => onDrive("tgdrive", "tg.1"));
  await act(async () => {});
  expect(mock.driveSet).not.toHaveBeenCalled();
  expect(mock.seen).not.toHaveBeenCalled();

  // Reopened: the same chat, its tick still ticked and still unseen, warning and chip together.
  mock.forNote.mockImplementationOnce(async () => onDrive("tgdrive", "tg.1"));
  draw();
  await waitFor(() => expect(opened()).toEqual([{ database: "tgdrive", sessionId: "tg.1" }]));
  showAll();
  expect(shown()).toEqual({ chips: ["tgdrive"], warnings: ["tgdrive"] });
});

it("keeps the reopened dock's chat its drive when a closed dock's answer comes late (R605-07/-08)", async () => {
  const late = held();
  openNote("v1", "inbox");
  const first = draw();
  await waitFor(() => expect(mock.forNote).toHaveBeenCalledTimes(1));
  first.unmount();
  // The reopened dock's answer comes first and is shown; the person says OK.
  mock.forNote.mockImplementationOnce(async () => onDrive("tgdrive", "tg.1"));
  draw();
  await waitFor(() => expect(opened()).toEqual([{ database: "tgdrive", sessionId: "tg.1" }]));
  showAll();
  await act(async () => {
    fireEvent.click(screen.getByRole("button", { name: "OK" }));
  });
  expect(shown()).toEqual({ chips: ["tgdrive"], warnings: [] });

  // The closed dock's answer comes now, however it was ordered in Rust: nothing happens.
  await late(() => onDrive("tgdrive", "tg.1"));
  await act(async () => {});
  showAll();
  expect(mock.driveSet).not.toHaveBeenCalled();
  expect(shown()).toEqual({ chips: ["tgdrive"], warnings: [] });
  expect(opened()).toEqual([{ database: "tgdrive", sessionId: "tg.1" }]);
});

it("sends the note with the first message of a conversation started in the dock", async () => {
  openNote("v1", "inbox");
  noteCaretStore.getState().place("v1", "inbox", 2, Text.of(["# Plans", "## Q3"]));
  mock.forNote.mockImplementation(async () => onDrive("tgdrive", "tg.1"));
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

it("follows /clean to the new chat, which shows its own drives, and the note goes on", async () => {
  openNote("v1", "inbox");
  noteCaretStore.getState().place("v1", "inbox", 2, Text.of(["# Plans", "## Q3"]));
  mock.forNote.mockImplementation(async () => onDrive("tgdrive", "tg.1"));
  mock.preview.mockImplementation(async (_target: unknown, draft: string) => ({
    draft,
    verdict: { kind: "command", name: "clean", args: null },
    rows: [],
    escapeHint: "",
  }));
  mock.command.mockImplementation(async () => {
    // `/clean` carries the ticks into the new chat; they are the person's carry, not the dock's.
    rust.ticked.set(chatKey("tgdrive", "tg.2"), new Set(["tgdrive"]));
    return {
      session: { database: "tgdrive", sessionId: "tg.2" },
      surface: null,
      message: "",
      pick: null,
    };
  });
  draw();
  await waitFor(() => expect(opened()).toEqual([{ database: "tgdrive", sessionId: "tg.1" }]));
  showAll();
  expect(shown().warnings).toEqual(["tgdrive"]);

  const field = screen.getByRole("textbox", { name: "Message" });
  fireEvent.change(field, { target: { value: "/clean" } });
  fireEvent.keyDown(field, { key: "Enter" });
  await waitFor(() => expect(last()).toEqual({ database: "tgdrive", sessionId: "tg.2" }));
  showAll();
  expect(shown()).toEqual({ chips: ["tgdrive"], warnings: [] });
  // The window's own selection is not the dock's to move (R601-04).
  expect(agentSessionsStore.getState().selected).toBeNull();

  fireEvent.change(screen.getByRole("textbox", { name: "Message" }), {
    target: { value: "and now?" },
  });
  fireEvent.click(screen.getByRole("button", { name: "Send message" }));
  await waitFor(() =>
    expect(mock.send).toHaveBeenCalledWith(
      { database: "tgdrive", sessionId: "tg.2" },
      "and now?",
      [],
      { vaultId: "v1", noteId: "inbox", line: 2, text: "# Plans\n## Q3" },
    ),
  );
  expect(mock.forNote).toHaveBeenCalledTimes(1);
});
