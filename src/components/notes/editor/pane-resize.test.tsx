/**
 * The note fits the pane it is in, and keeps fitting when the pane changes.
 *
 * A pane here changes width without the window changing size: a column folds,
 * the strip re-divides between two panels, the sidebar collapses. CodeMirror
 * measures its own width when it is created and when the *window* resizes, so
 * none of those reach it, and every line keeps the width the pane used to have
 * while the pane clips it.
 *
 * Measured in the running app before the fix: the window was dragged 158px
 * narrower, the three columns beside the note did not move a pixel — so the
 * whole change landed on this pane — and not one line re-wrapped. After it, the
 * same drag re-flowed the text.
 *
 * jsdom cannot see any of that; it does no layout. What it can see is the
 * wiring, which is what was missing.
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
import { resetNotesEditorStoreForTest } from "@/lib/stores/notes-editor";
import { withRangeRects } from "@/test/layout";
import { NOTE_ACTIONS_TEXT } from "../note-actions";
import { NoteEditor } from "../note-editor";

let restoreRects: (() => void) | null = null;

beforeAll(() => {
  restoreRects = withRangeRects();
});

afterAll(() => {
  restoreRects?.();
  restoreRects = null;
});

/** Every element a `ResizeObserver` was pointed at, and the callback it carries. */
let observed: { target: Element; fire: () => void }[] = [];
let previousObserver: typeof globalThis.ResizeObserver;

beforeEach(() => {
  vi.clearAllMocks();
  observed = [];
  previousObserver = globalThis.ResizeObserver;
  globalThis.ResizeObserver = class implements ResizeObserver {
    constructor(private readonly callback: ResizeObserverCallback) {}
    observe(target: Element): void {
      observed.push({
        target,
        fire: () => this.callback([] as ResizeObserverEntry[], this as ResizeObserver),
      });
    }
    unobserve(): void {}
    disconnect(): void {}
  };
  notesOpen.mockImplementation(async (_vault, _note, onBatch) => {
    onBatch({
      kind: "reset",
      text: "alpha\n",
      frontmatter: "",
      rev: "r0",
      cursor: 0,
      path: "n.md",
    });
    return "sub-1";
  });
});

afterEach(() => {
  globalThis.ResizeObserver = previousObserver;
  resetNotesEditorStoreForTest();
});

describe("the editor measures itself again when its pane changes width", () => {
  it("observes the host the editor is mounted into", async () => {
    render(<NoteEditor vaultId="v1" noteId="n1" />);
    await screen.findByRole("button", { name: new RegExp(`^${NOTE_ACTIONS_TEXT}`) });
    await waitFor(() => {
      expect(document.querySelector(".cm-content")).not.toBeNull();
    });

    const host = document.querySelector('[data-slot="note-editor-host"]');
    expect(host).not.toBeNull();
    expect(observed.map((o) => o.target)).toContain(host);
  });

  it("asks the view to re-measure when the observation arrives", async () => {
    const measure = vi.spyOn(EditorView.prototype, "requestMeasure");
    try {
      render(<NoteEditor vaultId="v1" noteId="n1" />);
      await screen.findByRole("button", { name: new RegExp(`^${NOTE_ACTIONS_TEXT}`) });
      await waitFor(() => {
        expect(document.querySelector(".cm-content")).not.toBeNull();
      });

      const host = document.querySelector('[data-slot="note-editor-host"]');
      const entry = observed.find((o) => o.target === host);
      expect(entry).toBeDefined();

      measure.mockClear();
      entry?.fire();
      expect(measure).toHaveBeenCalled();
    } finally {
      measure.mockRestore();
    }
  });
});
