/**
 * Story 43.1, the half that a module test cannot reach.
 *
 * `indent-keymap.test.ts` proves the bindings behave, over a stack the test
 * itself assembles. That leaves the exact failure this story is fixing wide
 * open: the bug was never a bad command, it was a keymap that nobody put in the
 * editor. So this suite mounts the real `NoteEditor` — its own boot effect, its
 * own dynamic imports, its own extension list — and presses Tab at the real
 * content DOM.
 *
 * The document is read back through the app's own channel: CodeMirror's update
 * listener calls `onEdit`, which writes the buffer into the notes store. If the
 * store says the line is indented, the keystroke went all the way through the
 * surface a user actually types into.
 */
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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

import { notesEditorStore } from "@/lib/stores/notes-editor";
import { NoteEditor } from "../note-editor";

// Same shim, same reason, as `recording-embed.test.ts`: jsdom does no layout,
// so CodeMirror's measure pass would throw out of the test on the first frame.
if (!Range.prototype.getClientRects) {
  Range.prototype.getClientRects = () =>
    Object.assign([] as DOMRect[], { item: () => null }) as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
}

const OPENED = "alpha\nbeta\n";

beforeEach(() => {
  vi.clearAllMocks();
  notesOpen.mockImplementation(async (_vault, _note, onBatch) => {
    onBatch({
      kind: "reset",
      text: OPENED,
      frontmatter: "",
      rev: "r0",
      // Pinned rather than left null: with no hint the editor puts the caret
      // at the end of whatever the store held when its lazy chunk landed, and
      // which line that is depends on a race between the chunk and the reset.
      cursor: 0,
      path: "n.md",
    } as NoteBodyBatch);
    return "sub-1";
  });
});

afterEach(() => {
  notesEditorStore.setState({ text: "", base: "", subscriptionId: null });
});

/** The content DOM of the mounted editor, once its lazy chunk has landed. */
async function content(): Promise<HTMLElement> {
  return await waitFor(() => {
    const node = document.querySelector<HTMLElement>(".cm-content");
    expect(node).not.toBeNull();
    expect(notesEditorStore.getState().text).toBe(OPENED);
    return node as HTMLElement;
  });
}

/**
 * Press a key at the content DOM and report whether the editor claimed it.
 *
 * `fireEvent` returns `false` when a handler called `preventDefault`, which is
 * the one thing worth knowing here: an unclaimed Tab is the bug. It also wraps
 * the dispatch in `act`, so React's own updates settle before the assertion.
 */
function press(node: HTMLElement, key: string, options: { shift?: boolean } = {}): boolean {
  const notCancelled = fireEvent.keyDown(node, {
    key,
    code: key,
    keyCode: key === "Tab" ? 9 : 27,
    shiftKey: options.shift === true,
    cancelable: true,
  });
  return !notCancelled;
}

describe("Tab, in the editor the user actually types into", () => {
  it("indents the caret's line rather than escaping to the web view", async () => {
    render(<NoteEditor vaultId="v1" noteId="n1" />);
    await screen.findByText("Properties");
    const node = await content();

    const claimed = press(node, "Tab");

    expect(claimed).toBe(true);
    // Read back through `onEdit` → the notes store: the buffer that would be
    // written to disk, not a CodeMirror internal.
    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe("  alpha\nbeta\n");
    });
    expect(notesEditorStore.getState().text).not.toContain("\t");
  });

  it("keeps the escape hatch: Escape then Tab leaves, and writes nothing", async () => {
    render(<NoteEditor vaultId="v1" noteId="n1" />);
    await screen.findByText("Properties");
    const node = await content();

    press(node, "Escape");

    // Not claimed: the browser is free to move focus, which is the contract
    // CodeMirror's unbound default exists to keep.
    expect(press(node, "Tab")).toBe(false);
    expect(notesEditorStore.getState().text).toBe(OPENED);
  });

  it("outdents on Shift-Tab", async () => {
    render(<NoteEditor vaultId="v1" noteId="n1" />);
    await screen.findByText("Properties");
    const node = await content();

    press(node, "Tab");
    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe("  alpha\nbeta\n");
    });
    press(node, "Tab", { shift: true });

    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe(OPENED);
    });
  });
});
