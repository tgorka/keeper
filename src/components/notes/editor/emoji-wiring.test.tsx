/**
 * Story 45.11, the half a module test cannot reach.
 *
 * `emoji-complete.test.ts` proves the source and the commit filter behave, over
 * a stack the test itself assembles. That leaves the failure this epic family
 * keeps finding wide open: Story 43.9's `/` menu was correct in every module
 * and had never opened, and DW-172 records the same shape again — a hook a test
 * mounts itself can never prove `App` mounts it. So this suite renders the real
 * `NoteEditor`, with its own boot effect, its own dynamic imports and its own
 * extension list, and types into the real content DOM.
 *
 * The document is read back through the app's own channel: CodeMirror's update
 * listener calls `onEdit`, which writes the buffer into the notes store. If the
 * store holds 🎉, the character went all the way through the surface a user
 * actually types into.
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
  notesGallery: vi.fn(async () => ({ entries: [] })),
  recordingNoteTargets: vi.fn(async () => null),
  recordingOpenPath: vi.fn(async () => {}),
  revealPath: vi.fn(async () => {}),
}));

import { EditorView } from "@codemirror/view";
import { notesEditorStore } from "@/lib/stores/notes-editor";
import { withRangeRects } from "@/test/layout";
import { NoteEditor } from "../note-editor";

// jsdom has no `Range.getClientRects`, so CodeMirror's measure pass throws on
// any animation frame that elapses during a test — and this file mounts the
// real `NoteEditor`, so frames do elapse. The undo is mandatory and paired
// because `Range.prototype` is shared with every other test; `afterAll` is the
// hook that can carry it, and `afterEach` is not — restoring the prototype
// between tests, while a just-unmounted view still has frames pending, is
// itself what makes the run exit non-zero with every test reported as passing.
let restoreRects: (() => void) | null = null;

beforeAll(() => {
  restoreRects = withRangeRects();
});

afterAll(() => {
  restoreRects?.();
  restoreRects = null;
});

const OPENED = "alpha\n";

beforeEach(() => {
  vi.clearAllMocks();
  notesOpen.mockImplementation(async (_vault, _note, onBatch) => {
    onBatch({
      kind: "reset",
      text: OPENED,
      frontmatter: "",
      rev: "r0",
      // Pinned rather than left null: with no hint the editor puts the caret at
      // the end of whatever the store held when its lazy chunk landed.
      cursor: 0,
      path: "n.md",
    } as NoteBodyBatch);
    return "sub-1";
  });
});

afterEach(() => {
  notesEditorStore.setState({ text: "", base: "", subscriptionId: null });
});

/** The real editor, once its lazy chunk has landed. */
async function mounted(): Promise<EditorView> {
  render(<NoteEditor vaultId="v1" noteId="n1" />);
  await screen.findByText("Properties");
  return await waitFor(() => {
    const node = document.querySelector<HTMLElement>(".cm-content");
    expect(node).not.toBeNull();
    expect(notesEditorStore.getState().text).toBe(OPENED);
    const view = EditorView.findFromDOM(node as HTMLElement);
    expect(view).not.toBeNull();
    return view as EditorView;
  });
}

/** Type at the caret, one transaction per character, the way typing is. */
function type(view: EditorView, text: string): void {
  for (const character of text) {
    const at = view.state.selection.main.head;
    view.dispatch({
      changes: { from: at, insert: character },
      selection: { anchor: at + character.length },
      userEvent: "input.type",
    });
  }
}

describe("emoji, in the editor the user actually types into", () => {
  it("turns a shortcode typed in full into its character", async () => {
    const view = await mounted();

    type(view, ":tada:");

    // Read back through `onEdit` → the notes store: the buffer that would be
    // written to disk, not a CodeMirror internal.
    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe("🎉alpha\n");
    });
  });

  it("offers the menu, beside the tag and slash sources rather than instead of them", async () => {
    const view = await mounted();

    type(view, ":tad");

    await waitFor(() => {
      const menu = document.querySelector(".cm-tooltip-autocomplete");
      expect(menu?.textContent).toContain("tada");
      expect(menu?.textContent).toContain("🎉");
    });
  });

  it("leaves an unknown shortcode as the text it is", async () => {
    const view = await mounted();

    type(view, ":zzzznotanemoji:");

    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe(":zzzznotanemoji:alpha\n");
    });
  });
});
