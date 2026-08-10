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
import { render, screen, waitFor } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteBodyBatch } from "@/lib/ipc/client";

const notesOpen =
  vi.fn<(v: string, n: string, on: (b: NoteBodyBatch) => void) => Promise<string>>();

vi.mock("@/lib/ipc/client", () => ({
  notesOpen: (v: string, n: string, on: (b: NoteBodyBatch) => void) => notesOpen(v, n, on),
  notesClose: vi.fn(async () => {}),
  notesSave: vi.fn(async () => ({ frontmatter: "", rev: "r1", path: "n.md", conflictCopy: null })),
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
import { notesEditorStore } from "@/lib/stores/notes-editor";
import { withRangeRects } from "@/test/layout";
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
    } as NoteBodyBatch);
    return "sub-1";
  });
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

beforeEach(() => {
  vi.clearAllMocks();
});

afterEach(() => {
  notesEditorStore.setState({ text: "", base: "", subscriptionId: null, cursor: null });
});

describe("a note that was just created", () => {
  it("opens focused, with the caret in an empty body and no block in the buffer", async () => {
    openOn("", null);
    render(<NoteEditor vaultId="v1" noteId="new-1" />);
    await screen.findByText("Properties");
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
    await screen.findByText("Properties");
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
    await screen.findByText("Properties");
    const editor = await view(scaffolded);

    await waitFor(() => {
      expect(editor.state.selection.main.head).toBe(10);
    });
    expect(editor.state.doc.lineAt(10).number).toBe(2);
  });
});
