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
  // Both arrive with the tabs under the note: the panel asks for links in each
  // direction, and the template offer asks whether this note has drifted from
  // the template it was made with. Absent from the mock they reject after the
  // test has torn its environment down, which vitest reports as an unhandled
  // error beside a green run.
  notesForwardlinks: vi.fn(async () => []),
  notesTemplateUpdatePreview: vi.fn(async () => null),
  notesResolveConflict: vi.fn(async () => {}),
  notesMarkRead: vi.fn(async () => {}),
  notesDiff: vi.fn(async () => null),
  notesHistory: vi.fn(async () => []),
  recordingNoteTargets: vi.fn(async () => null),
  recordingOpenPath: vi.fn(async () => {}),
  revealPath: vi.fn(async () => {}),
}));

import { readNoteDocument, resetNotesEditorStoreForTest } from "@/lib/stores/notes-editor";
import { NOTE_ACTIONS_TEXT } from "./note-actions";
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
  resetNotesEditorStoreForTest();
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
  // The header's own always-present control, as a "the editor mounted" barrier.
  // It used to be the word "Properties"; Story 46.5 moved that into this menu,
  // and 48.9 turned the trigger itself into a glyph — so the barrier is the
  // name it answers to rather than text it no longer renders.
  await screen.findByRole("button", { name: new RegExp(`^${NOTE_ACTIONS_TEXT}`) });
  return await waitFor(() => {
    const node = document.querySelector<HTMLElement>(".cm-content");
    expect(node).not.toBeNull();
    expect(readNoteDocument("v1", "n1").text).toBe(OPENED);
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

/** The opening set's size, as a fact this suite is allowed to know: the
 *  assertion is that none of them is blank, and that needs a total. */
const EMOJI_OPENING_COUNT = 48;

describe("the formatting toolbar, in the editor the user actually types into", () => {
  it("bolds the selection that was there when the button was pressed", async () => {
    const view = await mounted();
    select(view, "beta");

    fireEvent.click(screen.getByRole("button", { name: "Bold" }));

    // Read back through `onEdit` → the notes store: the buffer that would be
    // written to disk, not a CodeMirror internal.
    await waitFor(() => {
      expect(readNoteDocument("v1", "n1").text).toBe("alpha\n**beta**\n");
    });
  });

  it("unbolds on the second press, from the toolbar", async () => {
    const view = await mounted();
    select(view, "beta");

    fireEvent.click(screen.getByRole("button", { name: "Bold" }));
    await waitFor(() => {
      expect(readNoteDocument("v1", "n1").text).toBe("alpha\n**beta**\n");
    });
    fireEvent.click(screen.getByRole("button", { name: "Bold" }));

    await waitFor(() => {
      expect(readNoteDocument("v1", "n1").text).toBe(OPENED);
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
      expect(readNoteDocument("v1", "n1").text).toBe("alpha\n*beta*\n");
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
      expect(readNoteDocument("v1", "n1").text).toBe("> alpha\n> beta\n");
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
  describe("the emoji picker (Story 55.3)", () => {
    it("puts the character in the note and closes", async () => {
      const view = await mounted();
      select(view, "beta");

      fireEvent.click(screen.getByRole("button", { name: "Emoji" }));
      const search = await screen.findByLabelText("Search emoji");
      fireEvent.change(search, { target: { value: "tada" } });

      // Named by its shortcode, because the character alone is nothing a
      // screen reader can announce usefully.
      fireEvent.click(await screen.findByRole("button", { name: "tada" }));

      await waitFor(() => {
        // The character, not `:tada:` — one buffer spelling whichever door the
        // emoji came through.
        expect(readNoteDocument("v1", "n1").text).toBe("alpha\n🎉\n");
      });
      expect(screen.queryByLabelText("Search emoji")).toBeNull();
    });

    it("opens on a set somebody would actually reach for, all of it real", async () => {
      await mounted();
      fireEvent.click(screen.getByRole("button", { name: "Emoji" }));
      await screen.findByLabelText("Search emoji");

      // `querySelectorAll` rather than `getAllByRole`, and it is not a
      // shortcut: the picker holds 1855 buttons, and a role query computes an
      // accessible name for every one of them — 1097ms against 5ms, measured,
      // for the identical 1855 elements. Each `getByRole(name:)` below costs
      // the same scan again. That is what put this test over its 15s budget
      // under a loaded suite while it passed in 2.4s alone.
      //
      // Nothing is weakened by the swap. Every choice is a real `<button>`
      // carrying an `aria-label`, so the label IS the accessible name, and
      // the two queries return the same set.
      const picker = screen.getByRole("group", { name: "Emoji picker" });
      const choices = Array.from(picker.querySelectorAll("button"));
      const labels = choices.map((button) => button.getAttribute("aria-label"));

      // The familiar ones LEAD, in their own order. That was the whole point of
      // the curated list and it survives: the picker must not open on `+1`,
      // `100`, `1234`, `8ball`, `a`, `ab` and a run of flags, which is what the
      // table's own order starts with.
      expect(labels.slice(0, 2)).toEqual(["smile", "smiley"]);
      expect(labels).toContain("tada");
      expect(labels).toContain("white_check_mark");

      // ...and the rest FOLLOWS, which it did not before. A person who does not
      // know the shortcode cannot type their way to an emoji, and browsing is
      // the reason to open a picker rather than type `:` — so `8ball` has to be
      // reachable by scrolling even though nobody would put it in a top row.
      expect(labels).toContain("8ball");
      expect(choices.length).toBeGreaterThan(EMOJI_OPENING_COUNT * 10);

      // Every button resolves to a character: the curated list names shortcodes
      // and the table owns the characters, so one that fell out of the table
      // would otherwise render as a blank button.
      expect(choices.filter((button) => button.textContent?.trim() === "")).toHaveLength(0);

      // And nothing is offered twice — the lead is subtracted from the tail.
      expect(new Set(labels).size).toBe(labels.length);
    });

    it("shuts when the next press lands somewhere else", async () => {
      await mounted();
      fireEvent.click(screen.getByRole("button", { name: "Emoji" }));
      await screen.findByLabelText("Search emoji");

      // `pointerdown`, which is what the panel listens for: a press that starts
      // outside means "leave this" whatever it finishes on, and waiting for a
      // click lets whatever is under the pointer act with the panel still over
      // it.
      fireEvent.pointerDown(document.body);

      expect(screen.queryByLabelText("Search emoji")).toBeNull();
    });

    it("stays open when the press is inside the toolbar it belongs to", async () => {
      await mounted();
      const toggle = screen.getByRole("button", { name: "Emoji" });
      fireEvent.click(toggle);
      const search = await screen.findByLabelText("Search emoji");

      // The toolbar's own subtree is excluded, and the toggle is the reason:
      // closing here and reopening in the toggle would leave the button unable
      // to shut its own panel.
      fireEvent.pointerDown(search);
      expect(screen.queryByLabelText("Search emoji")).not.toBeNull();
    });

    it("says so when nothing matches, rather than showing an empty grid", async () => {
      await mounted();

      fireEvent.click(screen.getByRole("button", { name: "Emoji" }));
      fireEvent.change(await screen.findByLabelText("Search emoji"), {
        target: { value: "zzzznotanemoji" },
      });

      expect(await screen.findByText(/No emoji matches/)).not.toBeNull();
    });

    it("opens with a clean query the second time", async () => {
      await mounted();

      fireEvent.click(screen.getByRole("button", { name: "Emoji" }));
      fireEvent.change(await screen.findByLabelText("Search emoji"), { target: { value: "tada" } });
      fireEvent.click(screen.getByRole("button", { name: "Emoji" }));
      fireEvent.click(screen.getByRole("button", { name: "Emoji" }));

      // A stale query is a picker that lies about what it is showing.
      expect(await screen.findByLabelText("Search emoji")).toHaveValue("");
    });
  });

  describe("the marks Story 45.10 added", () => {
    const buttons: readonly { name: string; wrapped: string }[] = [
      { name: "Underline", wrapped: "<u>beta</u>" },
      { name: "Subscript", wrapped: "~beta~" },
      { name: "Superscript", wrapped: "^beta^" },
      // Story 55.3's, in the same table: the toolbar makes one promise and it
      // should be one test for all of them.
      { name: "Highlight", wrapped: "==beta==" },
    ];

    for (const button of buttons) {
      it(`${button.name} wraps the selection, and a second press puts it back`, async () => {
        const view = await mounted();
        select(view, "beta");

        fireEvent.click(screen.getByRole("button", { name: button.name }));
        await waitFor(() => {
          expect(readNoteDocument("v1", "n1").text).toBe(`alpha\n${button.wrapped}\n`);
        });

        fireEvent.click(screen.getByRole("button", { name: button.name }));
        await waitFor(() => {
          expect(readNoteDocument("v1", "n1").text).toBe(OPENED);
        });
      });
    }

    it("Task list writes a checkbox on both selected lines and takes them off again", async () => {
      const view = await mounted();
      select(view, "alpha\nbeta");

      fireEvent.click(screen.getByRole("button", { name: "Task list" }));
      await waitFor(() => {
        expect(readNoteDocument("v1", "n1").text).toBe("- [ ] alpha\n- [ ] beta\n");
      });

      fireEvent.click(screen.getByRole("button", { name: "Task list" }));
      await waitFor(() => {
        expect(readNoteDocument("v1", "n1").text).toBe(OPENED);
      });
    });

    it("Code block fences the selection, where Inline code backticks it in place", async () => {
      const view = await mounted();
      select(view, "beta");

      fireEvent.click(screen.getByRole("button", { name: "Code block" }));
      await waitFor(() => {
        expect(readNoteDocument("v1", "n1").text).toBe("alpha\n```\nbeta\n```\n");
      });

      fireEvent.click(screen.getByRole("button", { name: "Code block" }));
      await waitFor(() => {
        expect(readNoteDocument("v1", "n1").text).toBe(OPENED);
      });

      select(view, "beta");
      fireEvent.click(screen.getByRole("button", { name: "Inline code" }));
      await waitFor(() => {
        expect(readNoteDocument("v1", "n1").text).toBe("alpha\n`beta`\n");
      });
    });
  });

  it("numbers both lines of a selection that spans them", async () => {
    const view = await mounted();
    select(view, "alpha\nbeta");

    fireEvent.click(screen.getByRole("button", { name: "Numbered list" }));

    await waitFor(() => {
      expect(readNoteDocument("v1", "n1").text).toBe("1. alpha\n2. beta\n");
    });
  });

  it("sets a heading level from the level panel", async () => {
    const view = await mounted();
    select(view, "alpha");

    fireEvent.click(screen.getByRole("button", { name: "Heading" }));
    fireEvent.click(await screen.findByRole("button", { name: "Heading 3" }));

    await waitFor(() => {
      expect(readNoteDocument("v1", "n1").text).toBe("### alpha\nbeta\n");
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
      expect(readNoteDocument("v1", "n1").text).toBe(
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
      expect(readNoteDocument("v1", "n1").text).toBe(
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
      expect(readNoteDocument("v1", "n1").text).toBe("alpha\n~~beta~~\n");
    });

    undo(view);

    await waitFor(() => {
      expect(readNoteDocument("v1", "n1").text).toBe(OPENED);
    });
  });
});
