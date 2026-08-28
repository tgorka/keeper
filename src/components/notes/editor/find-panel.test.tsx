/**
 * Story 55.2 — the find bar, at two levels.
 *
 * The first suite assembles a view around `createFindPanel` and walks the
 * matrix. The second mounts the real `NoteEditor` and presses `⌘F`, because
 * DW-172's lesson is written into three files in this directory already: a
 * panel a test wires up itself can never prove `note-editor.tsx` wires it up.
 * The stock panel was, after all, correctly wired the whole time.
 */

import {
  closeSearchPanel,
  getSearchQuery,
  openSearchPanel,
  SearchQuery,
  searchKeymap,
  searchPanelOpen,
  setSearchQuery,
} from "@codemirror/search";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteBodyBatch } from "@/lib/ipc/client";
import { withRangeRects } from "@/test/layout";
import { findBar } from "./find-panel";

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

import { readNoteDocument, resetNotesEditorStoreForTest } from "@/lib/stores/notes-editor";
import { NOTE_ACTIONS_TEXT } from "../note-actions";
import { NoteEditor } from "../note-editor";

// See `emoji-wiring.test.tsx`: jsdom has no `Range.getClientRects`, the undo is
// mandatory, and `afterAll` is the only hook that can carry it.
let restoreRects: (() => void) | null = null;
beforeAll(() => {
  restoreRects = withRangeRects();
});
afterAll(() => {
  restoreRects?.();
  restoreRects = null;
});

const DOC = "alpha beta\nbeta gamma\n";

/** A view with the panel this story ships, and nothing else that could answer
 *  for it. */
async function panelOver(doc: string, readOnly = false): Promise<EditorView> {
  const view = new EditorView({
    parent: document.body,
    state: EditorState.create({
      doc,
      extensions: [EditorState.readOnly.of(readOnly), findBar(), keymap.of(searchKeymap)],
    }),
  });
  openSearchPanel(view);
  await screen.findByRole("search", { name: "Find in note" });
  return view;
}

/** Type into the find field the way a user does: the change handler both sets
 *  React state and dispatches the query. */
function findFor(view: EditorView, text: string): HTMLInputElement {
  const field = screen.getByLabelText("Find") as HTMLInputElement;
  fireEvent.change(field, { target: { value: text } });
  expect(getSearchQuery(view.state).search).toBe(text);
  return field;
}

describe("the find bar speaks the app's vocabulary", () => {
  afterEach(() => {
    document.body.innerHTML = "";
  });

  it("renders app controls, not the browser's own form parts", async () => {
    const view = await panelOver(DOC);
    const panel = screen.getByRole("search", { name: "Find in note" });

    // What the stock panel put on screen, and what this one must not.
    expect(panel.querySelectorAll('input[type="checkbox"]')).toHaveLength(0);
    for (const word of ["next", "previous", "all", "replace all"]) {
      expect(screen.queryByRole("button", { name: word })).toBeNull();
    }

    // What it puts there instead: a named field and named icon buttons.
    expect(screen.getByLabelText("Find")).toHaveClass("h-8");
    for (const name of [
      "Match case",
      "Regular expression",
      "Whole word",
      "Previous match",
      "Next match",
      "Select all matches",
      "Close find",
    ]) {
      expect(screen.getByRole("button", { name })).not.toBeNull();
    }

    view.destroy();
  });

  it("hands the caret to the find field, with any old query selected", async () => {
    const view = await panelOver(DOC);
    view.dispatch({ effects: setSearchQuery.of(new SearchQuery({ search: "beta" })) });
    closeSearchPanel(view);
    openSearchPanel(view);

    const field = (await screen.findByLabelText("Find")) as HTMLInputElement;
    await waitFor(() => {
      expect(document.activeElement).toBe(field);
    });
    // Selected, not just focused: reopening over an old query and typing should
    // replace it rather than append to it, which is what the stock panel did.
    expect(field.selectionStart).toBe(0);
    expect(field.selectionEnd).toBe("beta".length);

    view.destroy();
  });

  it("sits above the note rather than under it", async () => {
    const view = await panelOver(DOC);

    // `showPanel` asks the panel where it goes and puts a silent one at the
    // bottom. Every other assertion in this file passed while the bar sat under
    // the document, which is why this one exists.
    const top = view.dom.querySelector(".cm-panels-top");
    expect(top?.contains(screen.getByRole("search", { name: "Find in note" }))).toBe(true);
    expect(view.dom.querySelector(".cm-panels-bottom")).toBeNull();

    view.destroy();
  });

  it("steps forward on Enter and back on Shift-Enter", async () => {
    const view = await panelOver(DOC);
    const field = findFor(view, "beta");

    fireEvent.keyDown(field, { key: "Enter" });
    const first = view.state.selection.main;
    expect(view.state.sliceDoc(first.from, first.to)).toBe("beta");

    fireEvent.keyDown(field, { key: "Enter" });
    const second = view.state.selection.main;
    expect(second.from).toBeGreaterThan(first.from);

    fireEvent.keyDown(field, { key: "Enter", shiftKey: true });
    expect(view.state.selection.main.from).toBe(first.from);

    view.destroy();
  });

  it("flips each mode and puts the flag in the query CodeMirror holds", async () => {
    const view = await panelOver(DOC);
    findFor(view, "beta");

    for (const [name, flag] of [
      ["Match case", "caseSensitive"],
      ["Regular expression", "regexp"],
      ["Whole word", "wholeWord"],
    ] as const) {
      const button = screen.getByRole("button", { name });
      expect(button).toHaveAttribute("aria-pressed", "false");
      fireEvent.click(button);
      await waitFor(() => {
        expect(screen.getByRole("button", { name })).toHaveAttribute("aria-pressed", "true");
      });
      expect(getSearchQuery(view.state)[flag]).toBe(true);
    }

    view.destroy();
  });

  it("keeps replace behind the chevron, then replaces through CodeMirror", async () => {
    const view = await panelOver(DOC);
    expect(screen.queryByLabelText("Replace")).toBeNull();

    fireEvent.click(screen.getByRole("button", { name: "Show replace" }));
    const replace = await screen.findByLabelText("Replace");

    findFor(view, "beta");
    fireEvent.change(replace, { target: { value: "delta" } });
    fireEvent.click(screen.getByRole("button", { name: "Replace all" }));

    expect(view.state.doc.toString()).toBe("alpha delta\ndelta gamma\n");
    view.destroy();
  });

  it("offers nothing to replace with in a read-only editor", async () => {
    const view = await panelOver(DOC, true);

    expect(screen.queryByLabelText("Replace")).toBeNull();
    expect(screen.queryByRole("button", { name: "Show replace" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Replace all" })).toBeNull();
    // The find half is untouched by read-only.
    expect(screen.getByLabelText("Find")).not.toBeNull();

    view.destroy();
  });

  it("shows a query that arrived from somewhere other than these fields", async () => {
    const view = await panelOver(DOC);

    view.dispatch({
      effects: setSearchQuery.of(new SearchQuery({ search: "gamma", caseSensitive: true })),
    });

    await waitFor(() => {
      expect(screen.getByLabelText("Find")).toHaveValue("gamma");
      expect(screen.getByRole("button", { name: "Match case" })).toHaveAttribute(
        "aria-pressed",
        "true",
      );
    });
    view.destroy();
  });

  it("closes on Escape and on the close button", async () => {
    const view = await panelOver(DOC);

    fireEvent.keyDown(screen.getByLabelText("Find"), { key: "Escape" });
    await waitFor(() => {
      expect(searchPanelOpen(view.state)).toBe(false);
    });

    openSearchPanel(view);
    await screen.findByRole("search", { name: "Find in note" });
    fireEvent.click(screen.getByRole("button", { name: "Close find" }));
    await waitFor(() => {
      expect(searchPanelOpen(view.state)).toBe(false);
    });

    closeSearchPanel(view);
    view.destroy();
  });
});

describe("in the editor a note is actually opened in", () => {
  const OPENED = "alpha beta\n";

  beforeEach(() => {
    vi.clearAllMocks();
    notesOpen.mockImplementation(async (_vault, _note, onBatch) => {
      onBatch({ kind: "reset", text: OPENED, frontmatter: "", rev: "r0", cursor: 0, path: "n.md" });
      return "sub-1";
    });
  });

  afterEach(() => {
    resetNotesEditorStoreForTest();
  });

  it("is the panel ⌘F opens", async () => {
    render(<NoteEditor vaultId="v1" noteId="n1" />);
    await screen.findByRole("button", { name: new RegExp(`^${NOTE_ACTIONS_TEXT}`) });
    const content = await waitFor(() => {
      const node = document.querySelector<HTMLElement>(".cm-content");
      expect(node).not.toBeNull();
      expect(readNoteDocument("v1", "n1").text).toBe(OPENED);
      return node as HTMLElement;
    });

    // `Mod-f`, which is the binding `⌘F` fires on macOS. jsdom is not macOS,
    // and CodeMirror resolves `Mod` per platform, so the chord that reaches the
    // same handler here is Ctrl — deliberately not worked around by faking a
    // user agent, which `src/test/no-user-agent-gating.test.ts` exists to stop.
    fireEvent.keyDown(content, { key: "f", ctrlKey: true });

    // The landmark is this story's; the stock panel had no accessible name at
    // all, so finding it by one is the assertion.
    await screen.findByRole("search", { name: "Find in note" });
    expect(screen.getByLabelText("Find")).toHaveClass("h-8");
  });
});
