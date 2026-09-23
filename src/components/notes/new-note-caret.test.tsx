/**
 * Story 44.6's third acceptance criterion: **the caret lands in the body**.
 *
 * It is a claim about the surface, not about a value, so it is asserted over
 * the real `NoteEditor` — its own boot effect, its own dynamic imports, its own
 * extension list — opened on a note shaped the way `notes_create` writes one: a
 * frontmatter block, and a body that is empty or is a template's.
 *
 * Three things have to hold together for a person to be able to start typing,
 * and each of them is a separate way the promise breaks:
 *
 *   1. The editor **takes focus**. Without it the note opens and the next
 *      keystroke goes to whatever the user last clicked — the New Note button.
 *   2. The buffer **is the body**. The frontmatter block travels beside it, so
 *      offset zero is the body's first byte and there is no `---` for a caret
 *      to land above. This is the defect the split was made to kill, and a test
 *      that only checked focus would not notice it coming back.
 *   3. The caret sits **at the end of the body**, which for a blank note is
 *      offset zero and for a templated one is after the scaffold rather than in
 *      front of it.
 *
 * The editor is read through `EditorView.findFromDOM`, so what is asserted is
 * the live view's own selection and focus rather than a value this test handed
 * it.
 */
import { act, render, screen, waitFor } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteBodyBatch, NoteWriteVm } from "@/lib/ipc/client";

const notesOpen =
  vi.fn<(v: string, n: string, on: (b: NoteBodyBatch) => void) => Promise<string>>();
const notesSave = vi.fn<(s: string, text: string, rev: string) => Promise<NoteWriteVm>>();

vi.mock("@/lib/ipc/client", () => ({
  notesOpen: (v: string, n: string, on: (b: NoteBodyBatch) => void) => notesOpen(v, n, on),
  notesClose: vi.fn(async () => {}),
  notesSave: (s: string, text: string, rev: string) => notesSave(s, text, rev),
  notesBufferReport: vi.fn(async () => {}),
  notesTagTree: vi.fn(async () => ({ nodes: [] })),
  notesBacklinks: vi.fn(async () => []),
  notesResolveConflict: vi.fn(async () => {}),
  notesMarkRead: vi.fn(async () => {}),
  notesDiff: vi.fn(async () => null),
  notesHistory: vi.fn(async () => []),
  recordingNoteTargets: vi.fn(async () => null),
  recordingOpenPath: vi.fn(async () => {}),
  revealPath: vi.fn(async () => {}),
}));

import { EditorView } from "@codemirror/view";
import { NOTE_AUTOSAVE_IDLE_MS } from "@/hooks/use-notes-body";
import {
  acceptPending,
  applyBodyBatch,
  readNoteDocument,
  resetNotesEditorStoreForTest,
} from "@/lib/stores/notes-editor";
import { withRangeRects } from "@/test/layout";
import { NOTE_ACTIONS_TEXT } from "./note-actions";
import { NoteEditor } from "./note-editor";

// jsdom does no layout, so CodeMirror's measure pass — which runs on ANY
// animation frame that elapses during a test, and this file mounts the real
// `NoteEditor` — would throw outside every `try` a test can write and take the
// run's exit code with it while the summary still printed passes.
//
// The hand-rolled shim this replaced returned an EMPTY rect list, so a measure
// that did run read `rects[0]` as undefined and threw anyway — a permanent
// latent fault that surfaced only when a box was busy enough for a frame to
// elapse mid-test, which is what made it look like flake. Its
// `if (!Range.prototype.getClientRects)` guard read like order-dependence and
// was not: vitest isolates per file, so that condition was always true.
// Measured with a two-file probe rather than derived from the config.
//
// `withRangeRects` always installs and returns rects with numbers in them; its
// undo is mandatory because `Range.prototype` is shared with every other test
// in the file, and `afterAll` is the hook that can carry it — an `afterEach`
// undo restores the prototype while a just-unmounted view still has frames
// pending, which is itself a non-zero exit.
let restoreRects: (() => void) | null = null;

beforeAll(() => {
  restoreRects = withRangeRects();
});

afterAll(() => {
  restoreRects?.();
  restoreRects = null;
});

/**
 * The frontmatter `create_note` writes for a brand-new note, verbatim in shape.
 *
 * It is here to be asserted **absent** from the buffer. A block that reached
 * the editor would be indistinguishable from body text to CodeMirror, and the
 * first character typed at offset zero would push `---` down into the note.
 */
const BLOCK = "---\nid: 01SEEDNOTE\ncreated: 2026-08-09T10:00:00+02:00\n---\n";

/** Open the editor on a note whose body is `body` and whose caret hint is `cursor`. */
function openOn(body: string, cursor: number | null): void {
  notesOpen.mockImplementation(async (_vault, _note, onBatch) => {
    onBatch({
      kind: "reset",
      text: body,
      frontmatter: BLOCK,
      rev: "r0",
      cursor,
      path: "2026-08-09-untitled.md",
    });
    return "sub-1";
  });
}

/**
 * Open the editor with the channel holding its `reset` back, and return the
 * delivery. This is the order the running app usually sees — the lazy editor
 * chunk lands first, the snapshot second — so the caret hint is placed by the
 * editor's reconcile effect rather than by its boot closure.
 */
function openHeldBack(body: string, cursor: number | null): () => void {
  let push: ((batch: NoteBodyBatch) => void) | null = null;
  notesOpen.mockImplementation(async (_vault, _note, onBatch) => {
    push = onBatch;
    return "sub-1";
  });
  return () => {
    act(() => {
      push?.({
        kind: "reset",
        text: body,
        frontmatter: BLOCK,
        rev: "r0",
        cursor,
        path: "2026-08-09-untitled.md",
      });
    });
  };
}

/** The live editor, once its lazy chunk has landed and the reset has been applied. */
async function view(body: string): Promise<EditorView> {
  return await waitFor(() => {
    const host = document.querySelector<HTMLElement>(".cm-editor");
    expect(host).not.toBeNull();
    const found = EditorView.findFromDOM(host as HTMLElement);
    expect(found).not.toBeNull();
    const editor = found as EditorView;
    expect(editor.state.doc.toString()).toBe(body);
    return editor;
  });
}

/** What `notes_save` acknowledges: the body is on disk at the next revision. */
const WRITTEN: NoteWriteVm = {
  rev: "r1",
  path: "2026-08-09-untitled.md",
  frontmatter: BLOCK,
  conflictCopy: null,
};

/**
 * Type `text` at `at` the way a keystroke reaches CodeMirror: an unannotated
 * user transaction, so the editor's own update listener reports it to the
 * store and arms the autosave exactly as a real key would.
 */
function type(editor: EditorView, at: number, text: string): void {
  act(() => {
    editor.dispatch({
      changes: { from: at, insert: text },
      selection: { anchor: at + text.length },
      userEvent: "input.type",
    });
  });
}

/** Let the autosave's idle timer fire, and the write it starts go out. Only the
 *  timeout pair is faked: CodeMirror's frames and React's scheduler stay real. */
async function idleUntilAutosave(): Promise<void> {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(NOTE_AUTOSAVE_IDLE_MS);
  });
}

/** The template body every hint test opens, and its `{{cursor}}` on line 2. */
const SCAFFOLDED = "# Standup\n\n## Agenda\n";
const HINT = 10;

/**
 * Both orders the opening snapshot and the lazy editor chunk can arrive in. They
 * place the caret hint through different code — the boot closure, or the
 * reconcile effect — and each has to spend it.
 */
const ORDERS: ReadonlyArray<readonly [string, boolean]> = [
  ["the snapshot lands before the editor chunk", false],
  ["the editor chunk lands before the snapshot", true],
];

/** Open the templated note in the given order, and return the editor once the
 *  caret sits on the hint. */
async function openTemplated(noteId: string, heldBack: boolean): Promise<EditorView> {
  let deliver = () => {};
  if (heldBack) {
    deliver = openHeldBack(SCAFFOLDED, HINT);
  } else {
    openOn(SCAFFOLDED, HINT);
  }
  render(<NoteEditor vaultId="v1" noteId={noteId} />);
  await screen.findByRole("button", { name: new RegExp(`^${NOTE_ACTIONS_TEXT}`) });
  if (heldBack) {
    await view("");
  }
  deliver();
  const editor = await view(SCAFFOLDED);
  await waitFor(() => {
    expect(editor.state.selection.main.head).toBe(HINT);
  });
  return editor;
}

beforeEach(() => {
  vi.clearAllMocks();
  notesSave.mockImplementation(async () => WRITTEN);
});

afterEach(() => {
  vi.useRealTimers();
  resetNotesEditorStoreForTest();
});

describe("a note that was just created", () => {
  it("opens focused, with the caret in an empty body and no block in the buffer", async () => {
    openOn("", null);
    render(<NoteEditor vaultId="v1" noteId="new-1" />);
    // The Actions trigger is a glyph since 48.9 — waited for by name, not text.
    await screen.findByRole("button", { name: new RegExp(`^${NOTE_ACTIONS_TEXT}`) });
    const editor = await view("");

    // Focus, because the promise of New Note is that the next thing typed is
    // the note. This is the assertion that fails if the boot effect stops
    // calling `focus()`.
    await waitFor(() => {
      expect(editor.hasFocus).toBe(true);
    });
    expect(document.activeElement).toBe(editor.contentDOM);

    // The buffer is the body: nothing of the block reached it, so there is no
    // offset at which a keystroke can disturb frontmatter.
    expect(editor.state.doc.toString()).not.toContain("---");
    expect(editor.state.selection.main.head).toBe(0);
    expect(editor.state.doc.length).toBe(0);
  });

  it("puts the caret after a template's scaffold rather than in front of it", async () => {
    // What a create from a template with no `{{cursor}}` delivers: a body, and
    // no hint. The end of the body is where someone continuing a note wants the
    // caret, and it is still the body.
    const scaffolded = "# Standup\n\n## Agenda\n";
    openOn(scaffolded, null);
    render(<NoteEditor vaultId="v1" noteId="new-2" />);
    await screen.findByRole("button", { name: new RegExp(`^${NOTE_ACTIONS_TEXT}`) });
    const editor = await view(scaffolded);

    await waitFor(() => {
      expect(editor.state.selection.main.head).toBe(scaffolded.length);
    });
    expect(editor.hasFocus).toBe(true);
  });

  it("honours a template's own caret hint, which is an offset into the body", async () => {
    // `{{cursor}}` after the heading. The offset Rust sends is into the body —
    // never into the file — which is why this lands on line 2 and not inside
    // the block.
    const scaffolded = "# Standup\n\n## Agenda\n";
    openOn(scaffolded, 10);
    render(<NoteEditor vaultId="v1" noteId="new-3" />);
    await screen.findByRole("button", { name: new RegExp(`^${NOTE_ACTIONS_TEXT}`) });
    const editor = await view(scaffolded);

    await waitFor(() => {
      expect(editor.state.selection.main.head).toBe(10);
    });
    expect(editor.state.doc.lineAt(10).number).toBe(2);
  });
});

/**
 * The editor owns the text while the user is typing; a save acknowledgement is
 * Rust agreeing with what the editor already shows. So an acknowledgement must
 * reach nothing in CodeMirror — not the caret (which used to be sent back to the
 * template's `{{cursor}}` on every autosave) and not the document (which used to
 * be spliced back to the bytes that were written, dropping whatever was typed
 * while the write was in flight).
 */
describe("a save acknowledgement", () => {
  it.each(
    ORDERS,
  )("leaves the caret where the typing left it, not on the template's hint (%s)", async (_order, heldBack) => {
    const editor = await openTemplated("saved-1", heldBack);

    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    // Somewhere other than the hint: under the agenda, at the end of the body.
    type(editor, SCAFFOLDED.length, "- ship it");
    const typedTo = SCAFFOLDED.length + "- ship it".length;
    await idleUntilAutosave();
    vi.useRealTimers();

    expect(notesSave).toHaveBeenCalledTimes(1);
    await waitFor(() => {
      expect(readNoteDocument("v1", "saved-1").savedAtMs).not.toBeNull();
    });
    await act(async () => {});
    expect(readNoteDocument("v1", "saved-1").dirty).toBe(false);
    expect(editor.state.selection.main.head).toBe(typedTo);
  });

  it("keeps what was typed while the write was in flight, and stays unsaved", async () => {
    const opened = "# Standup\n";
    openOn(opened, null);
    let land: (write: NoteWriteVm) => void = () => {};
    notesSave.mockImplementationOnce(
      () =>
        new Promise<NoteWriteVm>((resolve) => {
          land = resolve;
        }),
    );
    render(<NoteEditor vaultId="v1" noteId="saved-2" />);
    await screen.findByRole("button", { name: new RegExp(`^${NOTE_ACTIONS_TEXT}`) });
    const editor = await view(opened);

    vi.useFakeTimers({ toFake: ["setTimeout", "clearTimeout"] });
    type(editor, opened.length, "first");
    await idleUntilAutosave();
    expect(notesSave).toHaveBeenCalledWith("sub-1", `${opened}first`, "r0");
    // The write is out and has not answered; the user keeps going.
    type(editor, editor.state.doc.length, " second");
    vi.useRealTimers();

    await act(async () => {
      land(WRITTEN);
    });
    await waitFor(() => {
      expect(readNoteDocument("v1", "saved-2").saving).toBe(false);
    });
    await act(async () => {});

    const mine = `${opened}first second`;
    expect(editor.state.doc.toString()).toBe(mine);
    expect(readNoteDocument("v1", "saved-2").text).toBe(mine);
    // Rust holds "first" only, so " second" is still owed a write.
    expect(readNoteDocument("v1", "saved-2").dirty).toBe(true);
  });
});

/**
 * The other half of the same contract: content that did NOT come from this
 * editor still has to reach it. A write from outside keeper lands live on a
 * clean buffer, and a revision the user accepts from the diff bar replaces a
 * dirty one — both through the one reconcile path a save must not trigger.
 */
describe("a revision from outside the editor", () => {
  const opened = "# Standup\n";
  const theirs = "# Standup\n\nadded by an agent\n";

  it.each(
    ORDERS,
  )("lands live in a clean editor, without sending the caret back to the hint (%s)", async (_order, heldBack) => {
    // A templated note, so the opening caret hint is in play: it was placed
    // once, on open, and a later write must not place it again.
    const appended = `${SCAFFOLDED}- added by an agent\n`;
    const editor = await openTemplated("outside-1", heldBack);
    // The user moves to the top without typing, so the buffer stays clean.
    act(() => {
      editor.dispatch({ selection: { anchor: 0 }, userEvent: "select" });
    });

    act(() => {
      applyBodyBatch("v1", "outside-1", {
        kind: "external",
        rev: "r1",
        frontmatter: BLOCK,
        text: appended,
      });
    });

    await waitFor(() => {
      expect(editor.state.doc.toString()).toBe(appended);
    });
    expect(editor.state.selection.main.head).toBe(0);
  });

  it("replaces a dirty editor's text only once the user accepts it", async () => {
    openOn(opened, null);
    render(<NoteEditor vaultId="v1" noteId="outside-2" />);
    await screen.findByRole("button", { name: new RegExp(`^${NOTE_ACTIONS_TEXT}`) });
    const editor = await view(opened);
    type(editor, opened.length, "mine");

    act(() => {
      applyBodyBatch("v1", "outside-2", {
        kind: "external",
        rev: "r1",
        frontmatter: BLOCK,
        text: theirs,
      });
    });
    await act(async () => {});
    expect(editor.state.doc.toString()).toBe(`${opened}mine`);

    act(() => {
      acceptPending("v1", "outside-2");
    });
    await waitFor(() => {
      expect(editor.state.doc.toString()).toBe(theirs);
    });
  });
});
