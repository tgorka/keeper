/**
 * An agent's surface requests, executed in the product's own note editor
 * (UX-DR131): each tool's effect on the buffer and the view, a
 * proposal that waits for the person, and an answer that leaves exactly once.
 *
 * The editors are real CodeMirror over the real notes store; only the IPC
 * client is replaced, and every answer is read off `agentSurfaceResult`'s
 * arguments — that call is what the agent is told.
 */
import { undo } from "@codemirror/commands";
import { EditorView } from "@codemirror/view";
import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  NoteBodyBatch,
  PanelTargetVm,
  SurfaceAnswerReq,
  SurfaceRequestVm,
} from "@/lib/ipc/client";
import { withRangeRects } from "@/test/layout";
import { settleNoteEditorBoot } from "@/test/note-editor-boot";

const notesOpen =
  vi.fn<(v: string, n: string, on: (b: NoteBodyBatch) => void) => Promise<string>>();
const agentSurfaceResult =
  vi.fn<(accountId: string, roomId: string, answer: SurfaceAnswerReq) => Promise<void>>();
const agentPresenceView = vi.fn<(view: string) => Promise<void>>();
let deliver: ((request: SurfaceRequestVm) => void) | null = null;

vi.mock("@/lib/ipc/client", () => ({
  notesOpen: (v: string, n: string, on: (b: NoteBodyBatch) => void) => notesOpen(v, n, on),
  notesClose: vi.fn(async () => false),
  notesSave: vi.fn(async () => ({ frontmatter: "", rev: "r1", path: "n2.md", conflictCopy: null })),
  notesBufferReport: vi.fn(async () => {}),
  notesNoteMarks: vi.fn(async () => ({ rev: "r0", ranges: [] })),
  notesTagTree: vi.fn(async () => ({ nodes: [] })),
  notesBacklinks: vi.fn(async () => []),
  notesResolveConflict: vi.fn(async () => {}),
  notesMarkRead: vi.fn(async () => {}),
  notesDiff: vi.fn(async () => null),
  notesHistory: vi.fn(async () => []),
  notesVaults: vi.fn(async () => []),
  notesVaultActive: vi.fn(async () => null),
  notesVaultSetActive: vi.fn(async () => {}),
  notesTemplateUpdatePreview: vi.fn(async () => null),
  recordingNoteTargets: vi.fn(async () => null),
  recordingOpenPath: vi.fn(async () => {}),
  revealPath: vi.fn(async () => {}),
  agentSurfaceSubscribe: async (onRequest: (request: SurfaceRequestVm) => void) => {
    deliver = onRequest;
  },
  agentSurfaceResult: (accountId: string, roomId: string, answer: SurfaceAnswerReq) =>
    agentSurfaceResult(accountId, roomId, answer),
  agentPresenceView: (view: string) => agentPresenceView(view),
}));

import { NoteEditor } from "@/components/notes/note-editor";
import {
  SURFACE_APPLY_LABEL,
  SURFACE_DECLINE_LABEL,
  SURFACE_DISMISS_HIGHLIGHT_LABEL,
  SURFACE_PROPOSAL_LABEL,
} from "@/components/notes/note-surface-strips";
import { capabilitiesStore } from "@/lib/stores/capabilities";
import { readNoteDocument, resetNotesEditorStoreForTest } from "@/lib/stores/notes-editor";
import {
  activePanel,
  panelsStore,
  resetPanelsStoreForTest,
  usePanelsStore,
} from "@/lib/stores/panels";
import { primaryViewStore } from "@/lib/stores/primary-view";
import {
  NO_SUCH_HEADING,
  resetAgentSurfaceForTest,
  SURFACE_EXPIRED_NOTICE,
  SURFACE_STALE_DETAIL,
  startAgentSurface,
} from "./surface";

vi.setConfig({ testTimeout: 20_000 });

const ACCOUNT = "acct";
const ROOM = "!dm:example.org";
const N2: PanelTargetVm = { kind: "note", vaultId: "v1", noteId: "n2" };
// Seven body lines: 1 title, 3 "## Focus", 5 "## Log", 7 "## Carried forward".
const BODY = "# 2026-08-10\n\n## Focus\n\n## Log\n\n## Carried forward";

let removeRangeRects: (() => void) | null = null;
beforeAll(() => {
  removeRangeRects = withRangeRects();
});
afterAll(() => {
  removeRangeRects?.();
});

beforeEach(() => {
  vi.clearAllMocks();
  resetAgentSurfaceForTest();
  resetPanelsStoreForTest();
  resetNotesEditorStoreForTest();
  primaryViewStore.getState().setView("inbox");
  capabilitiesStore.setState({
    capabilities: { ...capabilitiesStore.getState().capabilities, notes: true, sync: true },
  });
  agentSurfaceResult.mockResolvedValue(undefined);
  agentPresenceView.mockResolvedValue(undefined);
  notesOpen.mockImplementation(async (_vault, noteId, onBatch) => {
    onBatch({ kind: "reset", text: BODY, frontmatter: "", rev: "r0", cursor: 0, path: noteId });
    return "sub-n2";
  });
  startAgentSurface();
});

afterEach(settleNoteEditorBoot);
afterEach(() => {
  resetNotesEditorStoreForTest();
});

/** What the notes view mounts: the active panel's note, in the product's editor. */
function ActiveNote() {
  const target = usePanelsStore((state) => activePanel(state).target);
  return target?.kind === "note" ? (
    <NoteEditor vaultId={target.vaultId} noteId={target.noteId} />
  ) : null;
}

function request(
  overrides: Partial<SurfaceRequestVm> & Pick<SurfaceRequestVm, "requestId" | "tool">,
): SurfaceRequestVm {
  return {
    accountId: ACCOUNT,
    roomId: ROOM,
    target: N2,
    heading: null,
    range: null,
    text: null,
    expected: null,
    expiresAtMs: Date.now() + 60_000,
    ...overrides,
  };
}

function send(each: SurfaceRequestVm): void {
  if (deliver === null) {
    throw new Error("the surface never subscribed");
  }
  const handler = deliver;
  act(() => handler(each));
}

function editorView(): EditorView {
  const dom = document.querySelector<HTMLElement>(".cm-editor");
  const view = dom === null ? null : EditorView.findFromDOM(dom);
  if (view === null) {
    throw new Error("no editor");
  }
  return view;
}

/** Mount the note view with n2 already open, its text in the editor. */
async function openN2(): Promise<EditorView> {
  panelsStore.getState().setActiveTarget(N2);
  render(<ActiveNote />);
  return await waitFor(() => {
    const view = editorView();
    expect(view.state.doc.toString()).toBe(BODY);
    return view;
  });
}

/** Every answer sent for one request. */
function answersFor(requestId: string): SurfaceAnswerReq[] {
  return agentSurfaceResult.mock.calls
    .map(([, , answer]) => answer)
    .filter((answer) => answer.requestId === requestId);
}

async function answered(requestId: string): Promise<SurfaceAnswerReq> {
  return await waitFor(() => {
    const answers = answersFor(requestId);
    expect(answers).toHaveLength(1);
    return answers[0] as SurfaceAnswerReq;
  });
}

function linesWith(className: string): string[] {
  return Array.from(document.querySelectorAll(`.cm-line.${className}`)).map((line) =>
    (line.textContent ?? "").replace(/^#+\s*/, "").trim(),
  );
}

describe("open", () => {
  it("brings a note that is not open yet forward and puts the caret at the heading's line", async () => {
    render(<ActiveNote />);
    expect(document.querySelector(".cm-editor")).toBeNull();

    send(request({ requestId: "r-open", tool: "open", heading: "Log", range: { from: 5, to: 6 } }));

    expect(primaryViewStore.getState().view).toBe("notes");
    expect(activePanel(panelsStore.getState()).target).toEqual(N2);
    expect(await answered("r-open")).toEqual({
      requestId: "r-open",
      outcome: "done",
      applied: null,
      detail: null,
    });
    const view = editorView();
    expect(view.state.doc.lineAt(view.state.selection.main.head).number).toBe(5);
    expect(agentSurfaceResult.mock.calls[0]?.slice(0, 2)).toEqual([ACCOUNT, ROOM]);
  });

  it("leaves the caret alone in an editor the person is typing in, and only scrolls", async () => {
    const view = await openN2();
    act(() => {
      view.focus();
      view.dispatch({ selection: { anchor: view.state.doc.line(7).from } });
    });
    expect(view.hasFocus).toBe(true);

    send(
      request({ requestId: "r-typing", tool: "open", heading: "Log", range: { from: 5, to: 6 } }),
    );

    expect((await answered("r-typing")).outcome).toBe("done");
    // The person's next keystroke lands where they were, not at the heading.
    expect(view.state.doc.lineAt(view.state.selection.main.head).number).toBe(7);
    expect(view.state.doc.toString()).toBe(BODY);
  });

  it("opens at the top and says so when Rust found no such heading", async () => {
    const view = await openN2();
    // The note is in front but nobody is typing in it: the caret may move.
    act(() => {
      view.dispatch({ selection: { anchor: view.state.doc.line(7).from } });
      view.contentDOM.blur();
    });
    expect(view.hasFocus).toBe(false);

    send(request({ requestId: "r-top", tool: "open", heading: "Budget" }));

    expect((await answered("r-top")).detail).toBe(NO_SUCH_HEADING);
    expect(view.state.selection.main.head).toBe(0);
  });

  it("opens a file outside every vault in the Files preview", async () => {
    const file: PanelTargetVm = { kind: "file", profileId: "p1", relativePath: "media/talk.txt" };

    send(request({ requestId: "r-file", tool: "open", target: file }));

    expect(primaryViewStore.getState().view).toBe("files");
    expect(activePanel(panelsStore.getState()).target).toEqual(file);
    expect((await answered("r-file")).outcome).toBe("done");
  });
});

describe("highlight, point and scroll", () => {
  it("highlights lines until the person dismisses it", async () => {
    await openN2();

    send(request({ requestId: "r-hl", tool: "highlight", range: { from: 3, to: 3 } }));

    expect((await answered("r-hl")).outcome).toBe("done");
    await waitFor(() => expect(linesWith("cm-agent-highlight")).toEqual(["Focus"]));
    fireEvent.click(screen.getByRole("button", { name: SURFACE_DISMISS_HIGHLIGHT_LABEL }));
    expect(linesWith("cm-agent-highlight")).toEqual([]);
    expect(screen.queryByRole("button", { name: SURFACE_DISMISS_HIGHLIGHT_LABEL })).toBeNull();
  });

  it("highlights lines in a note that was not open yet", async () => {
    render(<ActiveNote />);

    send(request({ requestId: "r-hl0", tool: "highlight", range: { from: 5, to: 5 } }));

    expect((await answered("r-hl0")).outcome).toBe("done");
    await waitFor(() => expect(linesWith("cm-agent-highlight")).toEqual(["Log"]));
  });

  it("a second highlight replaces the first", async () => {
    await openN2();

    send(request({ requestId: "r-hl1", tool: "highlight", range: { from: 3, to: 3 } }));
    send(request({ requestId: "r-hl2", tool: "highlight", range: { from: 5, to: 7 } }));

    await answered("r-hl2");
    await waitFor(() =>
      expect(linesWith("cm-agent-highlight")).toEqual(["Log", "", "Carried forward"]),
    );
  });

  it("points at lines with a pulse that lets go", async () => {
    await openN2();

    send(request({ requestId: "r-pt", tool: "point", range: { from: 5, to: 5 } }));

    expect((await answered("r-pt")).outcome).toBe("done");
    await waitFor(() => expect(linesWith("cm-agent-point")).toEqual(["Log"]));
    await waitFor(() => expect(linesWith("cm-agent-point")).toEqual([]), { timeout: 5_000 });
  });

  it("scrolls without moving the caret or touching the text", async () => {
    const view = await openN2();
    act(() => view.dispatch({ selection: { anchor: 2 } }));

    send(
      request({
        requestId: "r-sc",
        tool: "scroll",
        heading: "Carried forward",
        range: { from: 7, to: 7 },
      }),
    );

    expect((await answered("r-sc")).outcome).toBe("done");
    expect(view.state.selection.main.head).toBe(2);
    expect(view.state.doc.toString()).toBe(BODY);
  });

  it("answers unavailable for lines past the end of the note", async () => {
    await openN2();

    send(request({ requestId: "r-past", tool: "highlight", range: { from: 7, to: 9 } }));

    expect((await answered("r-past")).outcome).toBe("unavailable");
    expect(linesWith("cm-agent-highlight")).toEqual([]);
  });
});

describe("a proposed edit waits for the person", () => {
  const PROPOSAL = {
    tool: "propose_edit" as const,
    range: { from: 3, to: 4 },
    text: "## Focus\n\nShip the surface tools.",
    expected: "## Focus\n",
  };
  const APPLIED =
    "# 2026-08-10\n\n## Focus\n\nShip the surface tools.\n## Log\n\n## Carried forward";

  it("shows the diff and changes no byte until Apply, which is one undoable edit of the person's", async () => {
    const view = await openN2();

    send(request({ requestId: "r-pr", ...PROPOSAL }));

    const strip = await screen.findByRole("region", { name: SURFACE_PROPOSAL_LABEL });
    expect(strip.querySelector("ins")).toHaveTextContent("Ship the surface tools.");
    expect(strip.querySelector("del")).toHaveTextContent("## Focus");
    expect(view.state.doc.toString()).toBe(BODY);
    expect(answersFor("r-pr")).toEqual([]);

    fireEvent.click(screen.getByRole("button", { name: SURFACE_APPLY_LABEL }));

    expect(view.state.doc.toString()).toBe(APPLIED);
    // Through `onEdit`, as typing goes: the buffer the autosave writes.
    expect(readNoteDocument("v1", "n2").text).toBe(APPLIED);
    expect(await answered("r-pr")).toEqual({
      requestId: "r-pr",
      outcome: "done",
      applied: true,
      detail: null,
    });
    expect(screen.queryByRole("region", { name: SURFACE_PROPOSAL_LABEL })).toBeNull();
    act(() => {
      undo(view);
    });
    expect(view.state.doc.toString()).toBe(BODY);
  });

  it("Decline changes nothing and is answered declined", async () => {
    const view = await openN2();

    send(request({ requestId: "r-dec", ...PROPOSAL }));
    fireEvent.click(await screen.findByRole("button", { name: SURFACE_DECLINE_LABEL }));

    expect(await answered("r-dec")).toEqual({
      requestId: "r-dec",
      outcome: "declined",
      applied: false,
      detail: null,
    });
    expect(view.state.doc.toString()).toBe(BODY);
    expect(screen.queryByRole("region", { name: SURFACE_PROPOSAL_LABEL })).toBeNull();
  });

  it("applies to the buffer the person sees, keeping what they typed elsewhere", async () => {
    const view = await openN2();
    act(() => view.dispatch({ changes: { from: view.state.doc.length, insert: "\n- typed" } }));

    send(request({ requestId: "r-dirty", ...PROPOSAL }));
    fireEvent.click(await screen.findByRole("button", { name: SURFACE_APPLY_LABEL }));

    expect(view.state.doc.toString()).toBe(`${APPLIED}\n- typed`);
    expect((await answered("r-dirty")).applied).toBe(true);
  });

  it("is answered unavailable, applying nothing, when the lines changed before Apply", async () => {
    const view = await openN2();
    send(request({ requestId: "r-stale", ...PROPOSAL }));
    await screen.findByRole("region", { name: SURFACE_PROPOSAL_LABEL });

    act(() => view.dispatch({ changes: { from: view.state.doc.line(3).to, insert: " now" } }));
    const typed = view.state.doc.toString();
    fireEvent.click(screen.getByRole("button", { name: SURFACE_APPLY_LABEL }));

    expect(await answered("r-stale")).toEqual({
      requestId: "r-stale",
      outcome: "unavailable",
      applied: null,
      detail: SURFACE_STALE_DETAIL,
    });
    expect(view.state.doc.toString()).toBe(typed);
  });

  it("is answered unavailable, with no strip, when the buffer never held what the agent read", async () => {
    await openN2();

    send(
      request({
        requestId: "r-old",
        ...PROPOSAL,
        expected: "## Something the note no longer says\n",
      }),
    );

    expect((await answered("r-old")).outcome).toBe("unavailable");
    expect(screen.queryByRole("region", { name: SURFACE_PROPOSAL_LABEL })).toBeNull();
  });

  it("is answered expired, and leaves the strip with a word on why, when the agent stops waiting", async () => {
    const view = await openN2();

    send(request({ requestId: "r-late", ...PROPOSAL, expiresAtMs: Date.now() + 400 }));
    await screen.findByRole("region", { name: SURFACE_PROPOSAL_LABEL });

    expect((await answered("r-late")).outcome).toBe("expired");
    await waitFor(() =>
      expect(screen.queryByRole("region", { name: SURFACE_PROPOSAL_LABEL })).toBeNull(),
    );
    // The person may have been reading the diff: where it was says why it went.
    expect(screen.getByRole("status")).toHaveTextContent(SURFACE_EXPIRED_NOTICE);
    expect(view.state.doc.toString()).toBe(BODY);
  });
});

describe("answers", () => {
  it("answers a request already past its expiry expired, and does nothing", async () => {
    send(request({ requestId: "r-exp", tool: "open", expiresAtMs: Date.now() - 1 }));

    expect(await answered("r-exp")).toEqual({
      requestId: "r-exp",
      outcome: "expired",
      applied: null,
      detail: null,
    });
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(activePanel(panelsStore.getState()).target).toBeNull();
  });

  it("answers each request exactly once, whatever arrives after", async () => {
    await openN2();
    const proposal = request({
      requestId: "r-once",
      tool: "propose_edit",
      range: { from: 3, to: 4 },
      text: "x",
      expected: "## Focus\n",
      expiresAtMs: Date.now() + 400,
    });

    send(proposal);
    fireEvent.click(await screen.findByRole("button", { name: SURFACE_DECLINE_LABEL }));
    // A sync replay hands the same request on again, and its expiry passes.
    send(proposal);
    await new Promise((resolve) => setTimeout(resolve, 600));

    expect(answersFor("r-once")).toHaveLength(1);
    expect(answersFor("r-once")[0]?.outcome).toBe("declined");
    expect(screen.queryByRole("region", { name: SURFACE_PROPOSAL_LABEL })).toBeNull();
  });
});

describe("presence", () => {
  it("reports the primary view when it changes, the chat windows as one view", () => {
    expect(agentPresenceView.mock.calls).toEqual([["chats"]]);

    act(() => primaryViewStore.getState().setView("notes"));
    act(() => primaryViewStore.getState().setView("archive"));
    act(() => primaryViewStore.getState().setView("inbox"));

    expect(agentPresenceView.mock.calls).toEqual([["chats"], ["notes"], ["chats"]]);
  });
});
