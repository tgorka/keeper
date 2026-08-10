/**
 * Story 44.9, the half a command test cannot reach.
 *
 * `editor/format-commands.test.ts` proves the commands produce the right
 * document over an editor the test itself assembles. That is exactly the shape
 * of proof this repo's ledger keeps catching out: 43.9's `/` menu had a correct
 * command table and had never opened for anybody. So this suite mounts the real
 * `NoteEditor` — its own boot effect, its own dynamic imports, its own
 * extension list — finds the toolbar the way a user does, by the label on the
 * button, and clicks it.
 *
 * The document is read back through the app's own channel: CodeMirror's update
 * listener calls `onEdit`, which writes the buffer into the notes store. If the
 * store says the words are bold, the click went all the way through the surface
 * the user actually presses, and the edit is on its way to the file.
 */
import { undo } from "@codemirror/commands";
import { EditorView } from "@codemirror/view";
import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteBodyBatch } from "@/lib/ipc/client";
import { withRangeRects } from "@/test/layout";

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
import { NoteEditor } from "./note-editor";

// jsdom has no `Range.getClientRects`, and CodeMirror's measure pass calls it
// on any animation frame that elapses mid-test.
//
// The stub this file used to carry installed an EMPTY `DOMRectList`, so a
// measure that did run read `rects[0]` as undefined and threw anyway — a
// PERMANENT fault that only ever SHOWED as an occasional red, because whether a
// frame elapses at all depends on how busy the box is. It was never an ordering
// problem: vitest isolates per test file (measured with a two-file probe, and
// `isolate`/`pool` are unset in `vitest.config.ts`, where the default is true),
// so every file starts with a clean `Range.prototype` and the stub's
// `if (!Range.prototype.getClientRects)` guard was always true.
//
// `withRangeRects` hands back a real rect, and its undo is paired because
// `Range.prototype` is shared with every test in the file.
let removeRangeRects: (() => void) | null = null;
beforeAll(() => {
  removeRangeRects = withRangeRects();
});
afterAll(() => {
  removeRangeRects?.();
});

const OPENED = "alpha\nbeta\n";

beforeEach(() => {
  vi.clearAllMocks();
  notesOpen.mockImplementation(async (_vault, _note, onBatch) => {
    onBatch({
      kind: "reset",
      text: OPENED,
      frontmatter: "",
      rev: "r0",
      cursor: 0,
      path: "n.md",
    });
    return "sub-1";
  });
});

afterEach(() => {
  notesEditorStore.setState({ text: "", base: "", subscriptionId: null });
});

/**
 * Mount the pane and hand back the editor the app built.
 *
 * `findFromDOM` is deliberate: the view under test is the one `NoteEditor`
 * constructed with the product's real extension list, not one this file could
 * have configured to suit itself.
 */
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

/** Select the first occurrence of `text`, the way a user dragging over it would
 *  leave the editor. */
function select(view: EditorView, text: string): void {
  const at = view.state.doc.toString().indexOf(text);
  expect(at).toBeGreaterThanOrEqual(0);
  view.dispatch({ selection: { anchor: at, head: at + text.length } });
  view.focus();
}

/** What the selection currently covers — the thing a toolbar must not lose. */
function selected(view: EditorView): string {
  return view.state.sliceDoc(view.state.selection.main.from, view.state.selection.main.to);
}

describe("the formatting toolbar, in the editor the user actually types into", () => {
  it("bolds the selection that was there when the button was pressed", async () => {
    const view = await mounted();
    select(view, "beta");

    fireEvent.click(screen.getByRole("button", { name: "Bold" }));

    // Read back through `onEdit` → the notes store: the buffer that would be
    // written to disk, not a CodeMirror internal.
    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe("alpha\n**beta**\n");
    });
  });

  it("unbolds on the second press, from the toolbar", async () => {
    const view = await mounted();
    select(view, "beta");

    fireEvent.click(screen.getByRole("button", { name: "Bold" }));
    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe("alpha\n**beta**\n");
    });
    fireEvent.click(screen.getByRole("button", { name: "Bold" }));

    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe(OPENED);
    });
  });

  it("does not let a button take focus off the text it is formatting", () => {
    // A real browser moves focus to whatever was moused down on. That would
    // blur the editor before the click handler ran — the command would still
    // land, and the user's next keystroke would go to a button. Cancelling the
    // mousedown is what stops focus moving at all, and `fireEvent` reports the
    // cancellation as `false`.
    render(<NoteEditor vaultId="v1" noteId="n1" />);

    for (const name of [
      "Bold",
      "Italic",
      "Underline",
      "Strikethrough",
      "Subscript",
      "Superscript",
      "Inline code",
      "Code block",
      "Task list",
      "Heading",
      "Table",
    ]) {
      expect(fireEvent.mouseDown(screen.getByRole("button", { name }))).toBe(false);
    }
  });

  it("leaves the selection on the same words and the caret back in the note", async () => {
    const view = await mounted();
    select(view, "beta");
    // Focus genuinely elsewhere before the click, which is where a toolbar
    // press in a real browser leaves it.
    const elsewhere = document.createElement("button");
    document.body.append(elsewhere);
    elsewhere.focus();
    expect(view.hasFocus).toBe(false);

    fireEvent.click(screen.getByRole("button", { name: "Italic" }));

    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe("alpha\n*beta*\n");
    });
    expect(selected(view)).toBe("beta");
    expect(view.hasFocus).toBe(true);
    elsewhere.remove();
  });

  it("quotes both lines of a selection that spans them", async () => {
    const view = await mounted();
    select(view, "alpha\nbeta");

    fireEvent.click(screen.getByRole("button", { name: "Quote" }));

    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe("> alpha\n> beta\n");
    });
  });

  /**
   * Story 45.10's five, through the surface the user presses.
   *
   * `editor/format-commands.test.ts` proves each command produces the right
   * document over an editor it assembles itself. That is exactly the proof
   * 43.9's `/` menu had while the feature did not exist for anybody, so each
   * new button is also pressed here, on the real `NoteEditor`, and read back
   * through the notes store — the buffer that would be written to the file.
   */
  describe("the marks Story 45.10 added", () => {
    const buttons: readonly { name: string; wrapped: string }[] = [
      { name: "Underline", wrapped: "<u>beta</u>" },
      { name: "Subscript", wrapped: "~beta~" },
      { name: "Superscript", wrapped: "^beta^" },
    ];

    for (const button of buttons) {
      it(`${button.name} wraps the selection, and a second press puts it back`, async () => {
        const view = await mounted();
        select(view, "beta");

        fireEvent.click(screen.getByRole("button", { name: button.name }));
        await waitFor(() => {
          expect(notesEditorStore.getState().text).toBe(`alpha\n${button.wrapped}\n`);
        });

        fireEvent.click(screen.getByRole("button", { name: button.name }));
        await waitFor(() => {
          expect(notesEditorStore.getState().text).toBe(OPENED);
        });
      });
    }

    it("Task list writes a checkbox on both selected lines and takes them off again", async () => {
      const view = await mounted();
      select(view, "alpha\nbeta");

      fireEvent.click(screen.getByRole("button", { name: "Task list" }));
      await waitFor(() => {
        expect(notesEditorStore.getState().text).toBe("- [ ] alpha\n- [ ] beta\n");
      });

      fireEvent.click(screen.getByRole("button", { name: "Task list" }));
      await waitFor(() => {
        expect(notesEditorStore.getState().text).toBe(OPENED);
      });
    });

    it("Code block fences the selection, where Inline code backticks it in place", async () => {
      const view = await mounted();
      select(view, "beta");

      fireEvent.click(screen.getByRole("button", { name: "Code block" }));
      await waitFor(() => {
        expect(notesEditorStore.getState().text).toBe("alpha\n```\nbeta\n```\n");
      });

      fireEvent.click(screen.getByRole("button", { name: "Code block" }));
      await waitFor(() => {
        expect(notesEditorStore.getState().text).toBe(OPENED);
      });

      select(view, "beta");
      fireEvent.click(screen.getByRole("button", { name: "Inline code" }));
      await waitFor(() => {
        expect(notesEditorStore.getState().text).toBe("alpha\n`beta`\n");
      });
    });
  });

  it("numbers both lines of a selection that spans them", async () => {
    const view = await mounted();
    select(view, "alpha\nbeta");

    fireEvent.click(screen.getByRole("button", { name: "Numbered list" }));

    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe("1. alpha\n2. beta\n");
    });
  });

  it("sets a heading level from the level panel", async () => {
    const view = await mounted();
    select(view, "alpha");

    fireEvent.click(screen.getByRole("button", { name: "Heading" }));
    fireEvent.click(await screen.findByRole("button", { name: "Heading 3" }));

    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe("### alpha\nbeta\n");
    });
    // The panel closes on use: a menu that stays open over the text it just
    // changed is a menu covering the answer.
    expect(screen.queryByRole("button", { name: "Heading 3" })).toBeNull();
  });

  it("builds the table the form describes, header included", async () => {
    const view = await mounted();
    // Caret on the empty last line, which is where a table can own its lines.
    view.dispatch({ selection: { anchor: view.state.doc.length } });

    fireEvent.click(screen.getByRole("button", { name: "Table" }));
    fireEvent.change(screen.getByLabelText("Rows"), { target: { value: "3" } });
    fireEvent.change(screen.getByLabelText("Columns"), { target: { value: "2" } });
    fireEvent.click(screen.getByRole("button", { name: "Insert" }));

    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe(
        [
          "alpha",
          "beta",
          "| Column 1 | Column 2 |",
          "| -------- | -------- |",
          "|          |          |",
          "|          |          |",
          "",
        ].join("\n"),
      );
    });
  });

  it("builds a headerless table with an empty header row, because GFM has no other kind", async () => {
    const view = await mounted();
    view.dispatch({ selection: { anchor: view.state.doc.length } });

    fireEvent.click(screen.getByRole("button", { name: "Table" }));
    fireEvent.change(screen.getByLabelText("Rows"), { target: { value: "3" } });
    fireEvent.change(screen.getByLabelText("Columns"), { target: { value: "2" } });
    fireEvent.click(screen.getByLabelText("First row is a header"));
    fireEvent.click(screen.getByRole("button", { name: "Insert" }));

    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe(
        [
          "alpha",
          "beta",
          "|     |     |",
          "| --- | --- |",
          "|     |     |",
          "|     |     |",
          "|     |     |",
          "",
        ].join("\n"),
      );
    });
  });

  it("folds a toolbar action into one undo step", async () => {
    // The reason these are commands rather than a rewrite of the buffer: the
    // history extension is already in the editor, so a formatting press has to
    // be one ⌘Z — and it has to be undoable at all, which an edit annotated
    // `remote` would not be.
    const view = await mounted();
    select(view, "beta");
    fireEvent.click(screen.getByRole("button", { name: "Strikethrough" }));
    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe("alpha\n~~beta~~\n");
    });

    undo(view);

    await waitFor(() => {
      expect(notesEditorStore.getState().text).toBe(OPENED);
    });
  });
});
