/**
 * A GFM table you can read and edit (Story 45.9).
 *
 * Every view test drives a real `EditorView` carrying the product's markdown
 * grammar and the real layer, because the three things this story can get wrong
 * are only visible there: a block decoration that CodeMirror refuses (DW-165), a
 * caret that the realign drags out of the cell it was in, and a document that
 * is briefly not a table between a keystroke and its padding.
 *
 * `withRangeRects` is installed for the same views: CodeMirror's measure pass
 * runs on any animation frame that elapses mid-test and jsdom has no
 * `Range.getClientRects`, so without it a slow run throws outside every `try` a
 * test can write and takes the exit code with it.
 */

import { history, undo } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { syntaxTree } from "@codemirror/language";
import { EditorSelection, EditorState, StateEffect, Transaction } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { within } from "@testing-library/dom";
import { afterEach, beforeEach, describe, expect, it, type Mock, vi } from "vitest";
import type { NoteCsvVm } from "@/lib/ipc/client";
import { withRangeRects } from "@/test/layout";
import { alignedTable, gfmTable } from "./format-commands";
import { livePreview } from "./live-preview";
import {
  csvEmbedTarget,
  csvTargetFor,
  realignedCaret,
  splitTableRow,
  TABLE_ASK_CLASS,
  TABLE_BLOCK_CLASS,
  TABLE_CELL_CLASS,
  TABLE_FROM_CSV_LABEL,
  TABLE_KEEP_LABEL,
  TABLE_NOTICE_CLASS,
  TABLE_RAGGED_CLASS,
  TABLE_REPLACE_LABEL,
  TABLE_SCROLL_CLASS,
  TABLE_TO_CSV_LABEL,
  tableAfter,
  tableCellText,
  tableHits,
  tableLayer,
  tableRefusal,
  tableSource,
} from "./markdown-table";

/** A table the aligner would leave exactly as it is. */
const ALIGNED = ["| a   | b   |", "| --- | --- |", "| c   | d   |"].join("\n");

/** The same table as somebody types it: no padding anywhere. */
const CRAMPED = ["|a|b|", "|-|-|", "|c|d|"].join("\n");

// --- Reading the source ------------------------------------------------------

describe("splitting a row", () => {
  it("keeps an escaped pipe inside the cell it belongs to", () => {
    // The whole reason this is a scanner. Split on every pipe and `a \| b`
    // becomes two cells, the header and the delimiter row stop matching, the
    // block stops being a table, and the realign rewrites the user's cell.
    expect(splitTableRow("| a \\| b | c |")).toEqual(["a \\| b", "c"]);
  });

  it("drops the fence pipes and keeps a genuinely empty cell", () => {
    expect(splitTableRow("| a |  | c |")).toEqual(["a", "", "c"]);
  });

  it("reads a row that has no fence pipes", () => {
    expect(splitTableRow("a | b")).toEqual(["a", "b"]);
  });
});

describe("finding tables in a document", () => {
  it("finds the block, its rows and its alignments", () => {
    const doc = EditorState.create({ doc: `intro\n\n${ALIGNED}\n\nafter\n` }).doc;

    const hits = tableHits(doc);

    expect(hits).toHaveLength(1);
    expect(hits[0].rows).toEqual([
      ["a", "b"],
      ["c", "d"],
    ]);
    expect(hits[0].aligns).toEqual(["none", "none"]);
    expect(doc.sliceString(hits[0].from, hits[0].to)).toBe(ALIGNED);
  });

  it("reads the alignment colons the delimiter row spells", () => {
    const doc = EditorState.create({
      doc: "| a | b | c | d |\n|:--|:-:|--:|---|\n| 1 | 2 | 3 | 4 |\n",
    }).doc;

    expect(tableHits(doc)[0].aligns).toEqual(["left", "center", "right", "none"]);
  });

  it("is not fooled by a sentence containing a pipe", () => {
    const doc = EditorState.create({ doc: "grep a | wc -l\nand then | again\n" }).doc;

    expect(tableHits(doc)).toEqual([]);
  });

  it("leaves a table inside a fenced block alone", () => {
    // The reader asked to see the pipes, and a realign would rewrite the
    // spacing of somebody's example.
    const doc = EditorState.create({ doc: `\`\`\`md\n${CRAMPED}\n\`\`\`\n` }).doc;

    expect(tableHits(doc)).toEqual([]);
  });

  it("does not call a half-typed table a table", () => {
    // A header that has grown a column its delimiter row has not is not a GFM
    // table, and the user is left holding exactly what they typed.
    const doc = EditorState.create({ doc: "| a | b | c |\n| --- | --- |\n| 1 | 2 |\n" }).doc;

    expect(tableHits(doc)).toEqual([]);
  });

  it("needs a row under the header, so a lone pipe line is nothing", () => {
    expect(tableHits(EditorState.create({ doc: "| a | b |\n" }).doc)).toEqual([]);
  });
});

// --- One aligner -------------------------------------------------------------

describe("the one aligner", () => {
  it("is the same function the `/` menu's builder uses", () => {
    // If these two ever diverge, the table the menu inserts and the table the
    // editor maintains are different tables.
    expect(gfmTable({ rows: 2, columns: 2, header: true })).toBe(
      alignedTable([
        ["Column 1", "Column 2"],
        ["", ""],
      ]),
    );
  });

  it("carries the alignment colons through a realign", () => {
    expect(
      alignedTable(
        [
          ["a", "b", "c"],
          ["1", "2", "3"],
        ],
        ["left", "center", "right"],
      ),
    ).toBe(["| a   | b   | c   |", "| :-- | :-: | --: |", "| 1   | 2   | 3   |", ""].join("\n"));
  });

  it("fills a short body row out to the header's width", () => {
    expect(alignedTable([["a", "b"], ["1"]])).toBe(
      ["| a   | b   |", "| --- | --- |", "| 1   |     |", ""].join("\n"),
    );
  });

  it("never drops a cell a long body row has", () => {
    // GFM ignores the excess; deleting it here would delete text the user can
    // see in their own file.
    expect(
      alignedTable([
        ["a", "b"],
        ["1", "2", "3"],
      ]),
    ).toBe(["| a   | b   |", "| --- | --- |", "| 1   | 2   | 3   |", ""].join("\n"));
  });

  it("leaves an escaped pipe byte-identical through a realign", () => {
    const source = ["| x      | y   |", "| ------ | --- |", "| a \\| b | c   |"].join("\n");
    const hit = tableHits(EditorState.create({ doc: `${source}\n` }).doc)[0];

    // Realigning an aligned table is the identity, and the cell's six bytes —
    // backslash included — come back exactly.
    expect(tableSource(hit.indent, hit.rows, hit.aligns)).toBe(source);
    expect(hit.rows[1][0]).toBe("a \\| b");
  });

  it("shows the reader a pipe where the source holds an escape", () => {
    expect(tableCellText("a \\| b")).toBe("a | b");
  });
});

// --- The structural edits, as values -----------------------------------------

describe("the structural edits", () => {
  const hit = () => tableHits(EditorState.create({ doc: `${ALIGNED}\n` }).doc)[0];

  it("adds a column to the header and lets the aligner fill the rest", () => {
    const next = tableAfter(hit(), "add-column");

    expect(next?.rows[0]).toEqual(["a", "b", ""]);
    expect(next?.aligns).toEqual(["none", "none", "none"]);
  });

  it("refuses to remove the last column, naming what to do instead", () => {
    const single = tableHits(EditorState.create({ doc: "| a |\n| - |\n| b |\n" }).doc)[0];

    expect(tableRefusal(single, "remove-column")).toContain("needs one column");
    expect(tableAfter(single, "remove-column")).toBeNull();
  });

  it("refuses to remove the header row", () => {
    const headerOnly = tableHits(EditorState.create({ doc: "| a | b |\n| - | - |\n" }).doc)[0];

    expect(headerOnly.rows).toHaveLength(1);
    expect(tableAfter(headerOnly, "remove-row")).toBeNull();
  });

  it("adds a row of empty cells at the header's width", () => {
    expect(tableAfter(hit(), "add-row")?.rows).toEqual([
      ["a", "b"],
      ["c", "d"],
      ["", ""],
    ]);
  });
});

// --- Where a realign leaves the caret ----------------------------------------

/**
 * The rule the owner's `| a    la |` broke, checked without a view because it
 * is arithmetic: a caret keeps its offset within its cell's TEXT, and can never
 * be left standing in the padding around it.
 */
describe("the caret across a realign", () => {
  it("keeps the caret's offset within the cell's text", () => {
    // After `c`, in a cell that is about to be repadded narrower.
    expect(realignedCaret("| cx    | d     |", "| cx  | d   |", 4)).toBe(4);
    // And in the second cell, whose whole span moves left as the first shrinks.
    expect(realignedCaret("| cx    | d     |", "| cx  | d   |", 10)).toBe(8);
  });

  /**
   * The half of the rule the aligner cannot express by trimming. A caret in a
   * cell's TRAILING padding survives `trim()` untouched, so nothing else pulls
   * it back to the text — and a caret parked in padding is a caret whose next
   * keystroke lands four columns away from the word it is in.
   */
  it("puts a caret standing in a cell's padding back on the end of its text", () => {
    expect(realignedCaret("| c     | d     |", "| c   | d   |", 6)).toBe(3);
    // The far edge of the cell, which is where a press on a wide empty cell
    // lands: still the end of the text, and for an empty cell that is its
    // first column.
    expect(realignedCaret("|       | d     |", "|     | d   |", 7)).toBe(2);
  });

  /**
   * The last cell of a row is a cell even when it is empty, and only the piece
   * BEYOND the closing pipe is fence. Read the other way, an empty final cell
   * is dropped, the caret standing in it belongs to no cell at all, and the
   * correction is skipped for exactly the cell a new table is mostly made of.
   */
  it("finds an empty cell at the end of a row rather than mistaking it for fence", () => {
    expect(realignedCaret("| a |   |", "| a   |     |", 6)).toBe(8);
  });

  it("declines a caret that is not in a cell at all", () => {
    // Before the opening pipe and after the closing one: the fence is not a
    // cell, so there is no offset to preserve and CodeMirror's own mapping is
    // the right answer.
    expect(realignedCaret("| c   | d   |", "| c | d |", 0)).toBeNull();
    expect(realignedCaret("| c   | d   |", "| c | d |", 13)).toBeNull();
  });
});

// --- Through a real editor ----------------------------------------------------

describe("a table in the note editor", () => {
  const views: EditorView[] = [];
  let restoreRects: (() => void) | null = null;

  beforeEach(() => {
    restoreRects = withRangeRects();
  });

  afterEach(() => {
    for (const view of views.splice(0)) {
      view.destroy();
    }
    restoreRects?.();
    restoreRects = null;
  });

  /**
   * A view over the product's grammar and the real layer.
   *
   * The caret is parked at the end of the document. It has to be somewhere, and
   * CodeMirror's default is offset 0 — which for a note that opens with a table
   * is *inside* the table, so the block correctly shows its source and no test
   * about the rendered form could ever see one.
   */
  function open(doc: string, product = false): EditorView {
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc,
        selection: EditorSelection.cursor(doc.length),
        extensions: [
          markdown({ base: markdownLanguage }),
          history(),
          product
            ? livePreview({ vaultId: "vault-1", assetUrl: (rel) => rel, onOpenLink: () => {} })
            : tableLayer(),
        ],
      }),
    });
    views.push(view);
    return view;
  }

  /** The transaction a keystroke makes: the character, and the caret after it. */
  function type(view: EditorView, text: string): void {
    view.dispatch(view.state.replaceSelection(text), { userEvent: "input.type" });
  }

  /**
   * A control by the name it answers to.
   *
   * By ROLE AND ACCESSIBLE NAME since 48.9, not by class and `textContent`.
   * These controls draw a glyph now, so their text content is empty and the
   * old query found nothing — but the thing that broke was the query, not the
   * control: what a user reaches these by is their name, and that is what
   * `getByRole` computes. A class is an implementation detail; a name is the
   * promise. Absent controls are an error, not a skip — `getByRole` throws.
   */
  function control(view: EditorView, label: string): HTMLButtonElement {
    const found = within(view.contentDOM).getByRole("button", { name: label });
    if (!(found instanceof HTMLButtonElement)) {
      throw new Error(`"${label}" is a ${found.tagName}, not a button`);
    }
    return found;
  }

  /**
   * Whether the editor's own GFM parser calls this document a table.
   *
   * The question is put to the parser that decides it in the product rather
   * than to a regex written here, which is 44.9's rule for the same question.
   */
  function parsesAsTable(text: string): boolean {
    const state = EditorState.create({
      doc: text,
      extensions: [markdown({ base: markdownLanguage })],
    });
    let table = false;
    syntaxTree(state).iterate({
      enter: (node) => {
        table = table || node.name === "Table";
      },
    });
    return table;
  }

  it("renders the block as a table instead of the pipes that were typed", () => {
    const view = open(`intro\n\n${ALIGNED}\n\nafter\n`);

    const table = view.contentDOM.querySelector(`.${TABLE_BLOCK_CLASS} table`);
    expect(table).not.toBeNull();
    expect([...(table?.querySelectorAll("th") ?? [])].map((cell) => cell.textContent)).toEqual([
      "a",
      "b",
    ]);
    expect([...(table?.querySelectorAll("td") ?? [])].map((cell) => cell.textContent)).toEqual([
      "c",
      "d",
    ]);
    expect(view.contentDOM.textContent).not.toContain("| a   | b   |");
  });

  /**
   * The owner's report against 0.8.5: notes truncated at the pane's edge with
   * nothing to scroll.
   *
   * `EditorView.lineWrapping` is on, so PROSE wraps. A block widget does not: a
   * table is as wide as its columns need, the content box is pinned to the
   * editor's width, and with nothing between the two the table simply left the
   * pane. What is asserted here is the structure and the declarations that make
   * the scroll possible — jsdom lays nothing out, so the measurement itself
   * (`scrollWidth > clientWidth` on a real 320px pane) is not provable here and
   * was taken in a browser.
   */
  it("puts a scroll box between the block and the table, and the controls outside it", () => {
    const view = open(`intro\n\n${ALIGNED}\n`);

    const block = view.contentDOM.querySelector(`.${TABLE_BLOCK_CLASS}`);
    const scroll = block?.querySelector(`.${TABLE_SCROLL_CLASS}`);
    // The table is INSIDE the scroll box, so it is the thing that moves.
    expect(scroll?.querySelector("table")).not.toBeNull();
    // The controls are its sibling, so scrolling a wide table sideways does not
    // carry the four buttons off the edge with it.
    expect(scroll?.querySelector(".cm-md-table-controls")).toBeNull();
    expect(block?.querySelector(".cm-md-table-controls")).not.toBeNull();
  });

  it("scrolls the box horizontally only when it has to, and never past the pane", () => {
    const view = open(`intro\n\n${ALIGNED}\n`);
    const scroll = view.contentDOM.querySelector(`.${TABLE_SCROLL_CLASS}`);

    if (!(scroll instanceof HTMLElement)) {
      throw new Error("the table rendered without its scroll box");
    }
    const style = getComputedStyle(scroll);
    // `auto` and not `scroll`: a permanent grey strip under every two-column
    // table in the note is a different defect with the same cause.
    expect(style.overflowX).toBe("auto");
    expect(style.maxWidth).toBe("100%");
    // The one that makes the other two work. Without inline-size containment the
    // table's own minimum width propagates up to `.cm-content` — measured at
    // 914px in a 320px pane — and this box ends up as wide as the thing it is
    // supposed to be scrolling.
    expect(style.contain).toBe("inline-size");
    // `max-content`, not `auto`: `EditorView.lineWrapping` puts
    // `overflow-wrap: anywhere` on the content box and a cell inherits it, so
    // `auto` shrank a seven-column table into seven columns of stacked letters
    // and overflowed nothing. That was the report.
    const table = scroll.querySelector("table");
    expect(table === null ? null : getComputedStyle(table).width).toBe("max-content");
  });

  it("is mounted by the note editor's own renderer, not only by this test", () => {
    // 41 correct unit tests say nothing about whether the product composes the
    // layer at all. This is the one assertion that does.
    const view = open(`intro\n\n${ALIGNED}\n`, true);

    expect(view.contentDOM.querySelector(`.${TABLE_BLOCK_CLASS} table`)).not.toBeNull();
  });

  it("shows a pipe in the cell whose source holds an escaped one", () => {
    const view = open(["| x      | y   |", "| ------ | --- |", "| a \\| b | c   |", ""].join("\n"));

    const cells = [...view.contentDOM.querySelectorAll(`td.${TABLE_CELL_CLASS}`)];
    expect(cells[0]?.textContent).toBe("a | b");
    // And the file still holds the escape.
    expect(view.state.doc.toString()).toContain("a \\| b");
  });

  it("puts the pipes back when the caret is in the table", () => {
    const view = open(`intro\n\n${ALIGNED}\n`);
    const inside = view.state.doc.line(3).from + 3;

    view.dispatch({ selection: EditorSelection.cursor(inside) });

    expect(view.contentDOM.querySelector(`.${TABLE_BLOCK_CLASS}`)).toBeNull();
    expect(view.contentDOM.textContent).toContain("| a   | b   |");
  });

  it("leaves a table inside a fenced block as source", () => {
    const view = open(`\`\`\`md\n${CRAMPED}\n\`\`\`\n`);

    expect(view.contentDOM.querySelector(`.${TABLE_BLOCK_CLASS}`)).toBeNull();
    expect(view.state.doc.toString()).toContain("|a|b|");
  });

  it("marks a body row that has more cells than the header", () => {
    const view = open("| a | b |\n| - | - |\n| 1 | 2 | 3 |\n");

    const row = view.contentDOM.querySelector(`tr.${TABLE_RAGGED_CLASS}`);
    expect(row).not.toBeNull();
    // Two cells are drawn because GFM draws two, and the third is still in the
    // file rather than quietly deleted.
    expect(row?.querySelectorAll("td")).toHaveLength(2);
    expect(view.state.doc.toString()).toContain("| 3 ");
  });

  // --- Structure ---------------------------------------------------------

  it("draws every control as a named glyph rather than as a word", () => {
    const view = open(`${ALIGNED}\n`);

    // The row a user sees is four pictures. What they can SAY is still four
    // phrases: `getByRole` computes the accessible name, so this passes only
    // while every control keeps one (Story 48.9, WCAG 2.5.3).
    for (const label of ["Add column", "Remove column", "Add row", "Remove row"]) {
      const button = control(view, label);
      // The word is gone from the surface — that is the change — and is still
      // on the tooltip, which is the only thing a pointer has left to read.
      expect(button.textContent).toBe("");
      expect(button.title).toBe(label);
      // One glyph, and it says nothing the name has not: a picture announced
      // beside its own label is the label read twice.
      const svg = button.querySelector("svg");
      expect(svg).not.toBeNull();
      expect(svg?.getAttribute("aria-hidden")).toBe("true");
      // `createElementNS`, not `createElement`: an HTML element called "svg"
      // renders nothing and would leave a control with no glyph at all.
      expect(svg?.namespaceURI).toBe("http://www.w3.org/2000/svg");
    }
  });

  it("widens every row including the delimiter row when a column is added", () => {
    const view = open(`${ALIGNED}\n`);

    control(view, "Add column").dispatchEvent(new MouseEvent("click"));

    const lines = view.state.doc.toString().split("\n").slice(0, 3);
    expect(lines).toEqual(["| a   | b   |     |", "| --- | --- | --- |", "| c   | d   |     |"]);
    // Every row's pipes at identical offsets — 44.9's alignment promise, kept
    // by the same aligner.
    const pipes = lines.map((line) =>
      [...line].flatMap((char, index) => (char === "|" ? [index] : [])).join(","),
    );
    expect(new Set(pipes).size).toBe(1);
    expect(parsesAsTable(view.state.doc.toString())).toBe(true);
  });

  it("removes the last column from every row", () => {
    const view = open("| a | b | c |\n| - | - | - |\n| 1 | 2 | 3 |\n");

    control(view, "Remove column").dispatchEvent(new MouseEvent("click"));

    expect(view.state.doc.toString().split("\n").slice(0, 3)).toEqual([
      "| a   | b   |",
      "| --- | --- |",
      "| 1   | 2   |",
    ]);
  });

  it("refuses to remove the last column and says why on the control", () => {
    const view = open("| a |\n| - |\n| b |\n");
    const before = view.state.doc.toString();
    const button = control(view, "Remove column");

    expect(button.disabled).toBe(true);
    expect(button.title).toContain("needs one column");
    button.dispatchEvent(new MouseEvent("click"));

    expect(view.state.doc.toString()).toBe(before);
  });

  it("adds and removes a body row", () => {
    const view = open(`${ALIGNED}\n`);

    control(view, "Add row").dispatchEvent(new MouseEvent("click"));
    expect(view.state.doc.toString().split("\n").slice(0, 4)).toEqual([
      "| a   | b   |",
      "| --- | --- |",
      "| c   | d   |",
      "|     |     |",
    ]);

    control(view, "Remove row").dispatchEvent(new MouseEvent("click"));
    expect(view.state.doc.toString()).toBe(`${ALIGNED}\n`);
  });

  it("does not let a control take the caret out of the note it is editing", () => {
    const view = open(`${ALIGNED}\n`);
    const press = new MouseEvent("mousedown", { bubbles: true, cancelable: true });

    control(view, "Add row").dispatchEvent(press);

    expect(press.defaultPrevented).toBe(true);
  });

  // --- Typing ------------------------------------------------------------

  it("keeps the source a parseable table after every single keystroke", () => {
    const view = open(`${ALIGNED}\n`);
    // The caret goes after `c`, in the first body cell.
    const cell = view.state.doc.line(3).from + 3;
    view.dispatch({ selection: EditorSelection.cursor(cell) });

    const seen: string[] = [];
    for (const char of [..."harlequin"]) {
      type(view, char);
      const text = view.state.doc.toString();
      seen.push(text.split("\n")[2]);
      // Asserted mid-edit, not only at the end: sync carries whatever is in the
      // buffer if the app dies between these two keystrokes.
      expect(parsesAsTable(text)).toBe(true);
      expect(tableHits(view.state.doc)).toHaveLength(1);
      const lines = text.split("\n").slice(0, 3);
      const pipes = lines.map((line) =>
        [...line].flatMap((each, index) => (each === "|" ? [index] : [])).join(","),
      );
      expect(new Set(pipes).size).toBe(1);
    }

    expect(seen[0]).toBe("| ch  | d   |");
    // Only the column that grew is repadded. The second column's cells are one
    // character wide and stay at the three-character floor.
    expect(seen[seen.length - 1]).toBe("| charlequin | d   |");
    expect(view.state.doc.toString().split("\n")[0]).toBe("| a          | b   |");
  });

  it("leaves the caret in the cell it was typed into", () => {
    const view = open(`${ALIGNED}\n`);
    const cell = view.state.doc.line(3).from + 3;
    view.dispatch({ selection: EditorSelection.cursor(cell) });

    type(view, "x");

    // The realign deletes a space AFTER the caret on this line and adds none
    // before it, so the caret is still one character past where it was.
    expect(view.state.selection.main.head).toBe(cell + 1);
    const line = view.state.doc.lineAt(view.state.selection.main.head);
    expect(line.text.slice(0, view.state.selection.main.head - line.from)).toBe("| cx");
  });

  /**
   * **The owner's report against 0.8.6, from their screenshot: typing `ala`
   * into a cell produced `| a    la |`.** The first letter landed in one place
   * and the other two somewhere else entirely, which makes a table unusable —
   * you cannot type a word into a cell.
   *
   * The caret starts in the cell's alignment PADDING rather than on its first
   * column, because that is where the gesture leaves it: the rendered block is
   * replaced wholesale, so a press reveals the pipes and the press that follows
   * lands wherever in the source line the pointer was — the middle or the right
   * of a wide empty cell is padding. `End` and the arrow keys reach the same
   * places.
   *
   * The mechanism, measured: the realign correctly pulls the typed `a` to the
   * cell's first column, and the splice that does it starts BEFORE the caret
   * and ends AT it, so CodeMirror maps the caret to the end of the inserted
   * padding — the far end of the cell. `l` and `a` are then typed there, and
   * because they are now interior to the cell's own text no realign takes the
   * spaces between them out again.
   */
  it("types the owner's `ala` into one cell instead of leaving the first letter behind", () => {
    const view = open("| Fruit | Notes |\n| ----- | ----- |\n|       |       |\n");
    const body = view.state.doc.line(3);
    // The right-hand edge of the first cell's five columns of padding.
    view.dispatch({ selection: EditorSelection.cursor(body.from + 7) });

    for (const char of [..."ala"]) {
      type(view, char);
    }

    expect(view.state.doc.toString().split("\n")[2]).toBe("| ala   |       |");
    // And the caret is where the typist's eye is: just past what they typed.
    expect(view.state.selection.main.head).toBe(body.from + 5);
  });

  /**
   * The same defect, stated as the rule that fixes it, at every column of one
   * cell. Wherever in a cell the caret starts, the letter it types is the
   * letter it is left standing after — the padding is not a place text can be.
   */
  it("leaves the caret on the typed letter from anywhere in a cell's padding", () => {
    for (let column = 2; column <= 7; column += 1) {
      const view = open("| Fruit | Notes |\n| ----- | ----- |\n|       |       |\n");
      const body = view.state.doc.line(3);
      view.dispatch({ selection: EditorSelection.cursor(body.from + column) });

      type(view, "a");

      expect(view.state.doc.toString().split("\n")[2]).toBe("| a     |       |");
      expect(view.state.selection.main.head - body.from).toBe(3);
    }
  });

  it("aligns a table the moment the typed delimiter row makes it one", () => {
    // Two cells in the header and one in the delimiter row is not yet a table,
    // so nothing has been rewritten under the typist's hands. The keystroke
    // that completes the delimiter row is the one that makes it one.
    const view = open("|a|b|\n|-|-");

    expect(view.state.doc.toString()).toBe("|a|b|\n|-|-");
    type(view, "|");

    expect(view.state.doc.toString()).toBe("| a   | b   |\n| --- | --- |");
  });

  it("keeps the keystroke and its realign in one undo step", () => {
    const view = open(`${CRAMPED}\n`);
    // Aligning is itself an edit, so start from the state the editor settles in.
    const cell = view.state.doc.line(3).from + 3;
    view.dispatch({ selection: EditorSelection.cursor(cell) });
    const before = view.state.doc.toString();

    type(view, "z");
    expect(view.state.doc.toString()).not.toBe(before);

    undo(view);

    // One step, not two: without `sequential` the padding would be its own
    // transaction and this would leave `z` behind.
    expect(view.state.doc.toString()).toBe(before);
  });

  it("makes one transaction of the keystroke and its realign", () => {
    const view = open(`${ALIGNED}\n`);
    const perChange: number[] = [];
    view.dispatch({
      effects: StateEffect.appendConfig.of(
        EditorView.updateListener.of((update) => {
          if (update.docChanged) {
            perChange.push(update.transactions.length);
          }
        }),
      ),
    });
    view.dispatch({ selection: EditorSelection.cursor(view.state.doc.line(3).from + 3) });

    type(view, "x");

    // One transaction, not two. This is what makes it one undo step, and it is
    // also what stops the note being reported to Rust twice per character.
    expect(perChange).toEqual([1]);
    expect(view.state.doc.toString().split("\n")[2]).toBe("| cx  | d   |");
  });

  it("does not realign a change that arrived from another editor", () => {
    const view = open(`${ALIGNED}\n`);
    const line = view.state.doc.line(3);

    view.dispatch({
      changes: { from: line.from, to: line.to, insert: "|c|dd|" },
      annotations: [Transaction.remote.of(true), Transaction.addToHistory.of(false)],
    });

    // The other side already aligned it its own way; re-padding here would send
    // a change straight back for it to re-pad in turn.
    expect(view.state.doc.toString().split("\n")[2]).toBe("|c|dd|");
  });

  it("leaves a table nothing touched alone", () => {
    const view = open(`${CRAMPED}\n\nafter\n`);
    view.dispatch({ selection: EditorSelection.cursor(view.state.doc.length - 1) });

    type(view, "!");

    // Editing the paragraph below must not reformat the table above it: the
    // realign is scoped to the tables the change actually reached.
    expect(view.state.doc.toString()).toBe(`${CRAMPED}\n\nafter!\n`);
  });
});

// --- A table and a CSV attachment are the same data --------------------------

/**
 * Item 8: the owner asked to convert a markdown table into a CSV attachment and
 * back.
 *
 * The point of the forward direction is the file: the data stops being markdown
 * that only this editor's aligner maintains and becomes something the Files
 * pane, the export and every machine this vault syncs to can read. The point of
 * the reverse is that this is not a one-way door.
 *
 * Rust is injected rather than mocked at the module boundary, which is
 * `csv-table.ts`'s pattern for the same problem: the interesting paths here are
 * the REFUSALS, and a refusal has to be handed to the widget to be tested.
 */
describe("converting between a table and a CSV attachment", () => {
  const views: EditorView[] = [];
  let restoreRects: (() => void) | null = null;

  /** The table Rust says the file is after a write. `relPath` is the only field
   *  this module reads — see the wrapper's own doc for why it must be that and
   *  not the target that was asked for. */
  function written(relPath: string): NoteCsvVm {
    return {
      relPath,
      rev: "r1",
      columns: 2,
      totalRows: 2,
      rows: [],
      notices: [],
      delimiter: ",",
    };
  }

  let toRows: Mock<(vaultId: string, target: string) => Promise<string[][]>>;
  let toCsv: Mock<
    (vaultId: string, target: string, rows: string[][], overwrite: boolean) => Promise<NoteCsvVm>
  >;

  beforeEach(() => {
    restoreRects = withRangeRects();
    toRows = vi.fn();
    toCsv = vi.fn();
  });

  afterEach(() => {
    for (const view of views.splice(0)) {
      view.destroy();
    }
    restoreRects?.();
    restoreRects = null;
  });

  /**
   * A view carrying the real layer and an injected Rust. `null` is the
   * no-vault host, spelled as a value rather than as an omitted argument:
   * passing `undefined` to a defaulted parameter takes the DEFAULT, which is
   * how the first draft of this test asserted the vault case twice.
   */
  function open(doc: string, vaultId: string | null = "vault-1"): EditorView {
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc,
        // Parked at the end of the document, so a table at the top renders.
        selection: EditorSelection.cursor(doc.length),
        extensions: [
          markdown({ base: markdownLanguage }),
          tableLayer({
            vaultId: vaultId ?? undefined,
            convert: {
              toRows: (vault, target) => toRows(vault, target),
              toCsv: (vault, target, rows, overwrite) => toCsv(vault, target, rows, overwrite),
            },
          }),
        ],
      }),
    });
    views.push(view);
    return view;
  }

  function control(view: EditorView, label: string): HTMLButtonElement {
    const found = within(view.contentDOM).getByRole("button", { name: label });
    if (!(found instanceof HTMLButtonElement)) {
      throw new Error(`"${label}" is a ${found.tagName}, not a button`);
    }
    return found;
  }

  /** Let the fired-and-forgotten conversion settle. */
  async function settled(): Promise<void> {
    await Promise.resolve();
    await Promise.resolve();
    await Promise.resolve();
  }

  const TABLE = ["| Fruit | Notes  |", "| ----- | ------ |", "| apple | crisp  |", ""].join("\n");

  it("writes the table to a CSV attachment and leaves the embed in its place", async () => {
    toCsv.mockResolvedValue(written("attachments/fruit-notes.csv"));
    const view = open(TABLE);

    control(view, TABLE_TO_CSV_LABEL).dispatchEvent(new MouseEvent("click"));
    await settled();

    // A BARE name: Rust resolves it into the attachments folder, and composing
    // that path here is the arithmetic AD-65 keeps out of the webview.
    expect(toCsv).toHaveBeenCalledWith(
      "vault-1",
      "fruit-notes.csv",
      [
        ["Fruit", "Notes"],
        ["apple", "crisp"],
      ],
      false,
    );
    // The path Rust ANSWERED with, not the one it was asked for, and Obsidian's
    // embed spelling rather than CommonMark's — the other one renders as text.
    expect(view.state.doc.toString()).toBe("![[attachments/fruit-notes.csv]]\n");
  });

  it("sends the value a reader sees, not the bytes the cell is escaped with", async () => {
    toCsv.mockResolvedValue(written("attachments/a-b.csv"));
    const view = open(["| a \\| b | c |", "| ------ | - |", "| 1      | 2 |", ""].join("\n"));

    control(view, TABLE_TO_CSV_LABEL).dispatchEvent(new MouseEvent("click"));
    await settled();

    // CSV has no pipe escape, and `a \| b` in a note is the one value `a | b`.
    expect(toCsv.mock.calls[0][2][0]).toEqual(["a | b", "c"]);
  });

  it("fills a short row out to the header's width, because that is the table on screen", async () => {
    toCsv.mockResolvedValue(written("attachments/a-b-c.csv"));
    const view = open(["| a | b | c |", "| - | - | - |", "| 1 |", ""].join("\n"));

    control(view, TABLE_TO_CSV_LABEL).dispatchEvent(new MouseEvent("click"));
    await settled();

    expect(toCsv.mock.calls[0][2]).toEqual([
      ["a", "b", "c"],
      ["1", "", ""],
    ]);
  });

  /**
   * **The clobber path (AD-89).** The command refuses an existing file rather
   * than replacing it, and that refusal IS the existence probe: looking first
   * and writing second has a window in it, because this folder is being synced
   * and a file can arrive in the gap.
   */
  it("asks before it replaces a file, naming the file, and writes nothing until told", async () => {
    toCsv.mockRejectedValue({
      code: "notesInvalid",
      message: "attachments/fruit-notes.csv is already there. The table was not written.",
    });
    const view = open(TABLE);

    control(view, TABLE_TO_CSV_LABEL).dispatchEvent(new MouseEvent("click"));
    await settled();

    // Rust's own sentence, which names the file — so the question names what it
    // would destroy without this module composing a path to say it with.
    const question = view.contentDOM.querySelector(`.${TABLE_ASK_CLASS}`);
    expect(question?.textContent).toContain("attachments/fruit-notes.csv is already there");
    // One call, and the table is still the user's table.
    expect(toCsv).toHaveBeenCalledTimes(1);
    expect(view.state.doc.toString()).toBe(TABLE);
  });

  it("keeps the file when that is the answer, and asks again next time", async () => {
    toCsv.mockRejectedValue({ code: "notesInvalid", message: "already there" });
    const view = open(TABLE);
    control(view, TABLE_TO_CSV_LABEL).dispatchEvent(new MouseEvent("click"));
    await settled();

    control(view, TABLE_KEEP_LABEL).dispatchEvent(new MouseEvent("click"));

    // Nothing written, nothing asked twice, and the question is gone rather
    // than left on screen as an unanswered warning.
    expect(toCsv).toHaveBeenCalledTimes(1);
    expect(view.state.doc.toString()).toBe(TABLE);
    expect(view.contentDOM.querySelector(`.${TABLE_ASK_CLASS}`)).toBeNull();
  });

  it("replaces the file only on the second, explicit press", async () => {
    toCsv
      .mockRejectedValueOnce({ code: "notesInvalid", message: "already there" })
      .mockResolvedValueOnce(written("attachments/fruit-notes.csv"));
    const view = open(TABLE);
    control(view, TABLE_TO_CSV_LABEL).dispatchEvent(new MouseEvent("click"));
    await settled();

    control(view, TABLE_REPLACE_LABEL).dispatchEvent(new MouseEvent("click"));
    await settled();

    // The same rows, and now `overwrite` — the flag is never defaulted, because
    // what is at stake is a file of the user's data.
    expect(toCsv.mock.calls[1][3]).toBe(true);
    expect(view.state.doc.toString()).toBe("![[attachments/fruit-notes.csv]]\n");
  });

  it("says why a conversion failed instead of appearing to do nothing", async () => {
    toCsv.mockRejectedValue({ code: "unsupported", message: "that file is too large to table." });
    const view = open(TABLE);

    control(view, TABLE_TO_CSV_LABEL).dispatchEvent(new MouseEvent("click"));
    await settled();

    expect(view.contentDOM.querySelector(`.${TABLE_NOTICE_CLASS}`)?.textContent).toBe(
      "that file is too large to table.",
    );
    // A refusal that is not "the file exists" is not a question: there is
    // nothing here for the user to answer.
    expect(view.contentDOM.querySelector(`.${TABLE_ASK_CLASS}`)).toBeNull();
    expect(view.state.doc.toString()).toBe(TABLE);
  });

  // --- And back ----------------------------------------------------------

  const EMBED = "notes\n\n![[attachments/people.csv]]\n";

  /** The caret on the embed's line, which is where its control appears and
   *  where `live-preview.ts` has already stopped rendering over the source. */
  function caretOnEmbed(view: EditorView): void {
    view.dispatch({ selection: EditorSelection.cursor(view.state.doc.line(3).from) });
  }

  it("reads the attachment back as an aligned GFM table", async () => {
    toRows.mockResolvedValue([
      ["name", "role"],
      ["Ada", "engineer"],
      ["Bo", "cook"],
    ]);
    const view = open(EMBED);
    caretOnEmbed(view);

    control(view, TABLE_FROM_CSV_LABEL).dispatchEvent(new MouseEvent("click"));
    await settled();

    expect(toRows).toHaveBeenCalledWith("vault-1", "attachments/people.csv");
    // 44.9's aligner, through this module's one caller of it — not a second
    // markdown table writer.
    expect(view.state.doc.toString()).toBe(
      [
        "notes",
        "",
        "| name | role     |",
        "| ---- | -------- |",
        "| Ada  | engineer |",
        "| Bo   | cook     |",
        "",
      ].join("\n"),
    );
    // And the file is still on disk: deleting it would be a destructive act
    // from a button that says "convert".
    expect(view.state.doc.toString()).not.toContain("![[");
  });

  it("escapes a field holding a pipe, so the row it writes is still one row", async () => {
    toRows.mockResolvedValue([["a"], ["x|y"]]);
    const view = open(EMBED);
    caretOnEmbed(view);

    control(view, TABLE_FROM_CSV_LABEL).dispatchEvent(new MouseEvent("click"));
    await settled();

    // Unescaped, `x|y` would be two cells, the row would stop matching the
    // header, and the block would not be a table at all.
    expect(view.state.doc.toString()).toContain("| x\\|y |");
    expect(tableHits(view.state.doc)).toHaveLength(1);
  });

  it("refuses a field holding a line break and names it, rather than flattening it", async () => {
    toRows.mockResolvedValue([
      ["note", "who"],
      ["two\nlines", "Ada"],
    ]);
    const view = open(EMBED);
    caretOnEmbed(view);

    control(view, TABLE_FROM_CSV_LABEL).dispatchEvent(new MouseEvent("click"));
    await settled();

    // A markdown cell has no spelling for a line break, and a spreadsheet
    // export is exactly where multi-line fields come from. Naming the field
    // beats rewriting the user's data into a space.
    const notice = view.contentDOM.querySelector(`.${TABLE_NOTICE_CLASS}`);
    expect(notice?.textContent).toContain("Record 2, field 1 holds a line break");
    expect(view.state.doc.toString()).toBe(EMBED);
  });

  /**
   * The doc alone cannot carry this test: reading the line BEFORE the command
   * instead of after it leaves the doc equally unchanged, because the stale
   * offsets are out of range in a note that got shorter and the dispatch throws
   * inside a promise nobody awaits. So what is asserted is the refusal itself,
   * on the control that asked for it — which is still the element that was
   * pressed even though the line it belonged to is gone.
   */
  it("refuses instead of writing when the line moved while the read was in flight", async () => {
    let answer: (rows: string[][]) => void = () => {};
    toRows.mockReturnValue(
      new Promise<string[][]>((resolve) => {
        answer = resolve;
      }),
    );
    const view = open(EMBED);
    caretOnEmbed(view);
    const pressed = control(view, TABLE_FROM_CSV_LABEL);
    const host = pressed.parentElement;
    pressed.dispatchEvent(new MouseEvent("click"));

    // The note is edited while the command is in flight, which is the whole
    // reason the line is re-found after the read and not before it.
    const line = view.state.doc.line(3);
    view.dispatch({ changes: { from: line.from, to: line.to, insert: "gone" } });
    answer([["a"], ["b"]]);
    await settled();

    // Three lines of table over whatever now occupies those offsets is the
    // failure this avoids, and it says so rather than failing silently.
    expect(host?.querySelector(`.${TABLE_NOTICE_CLASS}`)?.textContent).toContain(
      "changed while keeper was working",
    );
    expect(view.state.doc.toString()).toBe("notes\n\ngone\n");
  });

  it("offers no control on a line that holds more than the embed", () => {
    const view = open("notes\n\nsee ![[attachments/people.csv]] for the list\n");
    caretOnEmbed(view);

    // A table owns its lines: an embed in the middle of a sentence has nowhere
    // to put three lines of pipes, so it is not offered rather than offered
    // and refused.
    expect(
      within(view.contentDOM).queryAllByRole("button", { name: TABLE_FROM_CSV_LABEL }),
    ).toHaveLength(0);
  });

  it("offers no control while a predicate block follows the embed", () => {
    const view = open("notes\n\n![[attachments/people.csv]]{ :value_of }\n");
    caretOnEmbed(view);

    // Converting the line would delete a predicate the author wrote, and this
    // control is not the place that decides what happens to it.
    expect(
      within(view.contentDOM).queryAllByRole("button", { name: TABLE_FROM_CSV_LABEL }),
    ).toHaveLength(0);
  });

  it("disables both conversions and says why when there is no vault to write to", () => {
    const view = open(TABLE, null);

    const out = control(view, TABLE_TO_CSV_LABEL);
    expect(out.disabled).toBe(true);
    // A control that vanishes is a feature the user concludes does not exist;
    // one that is dimmed and says why is a fact about this editor.
    expect(out.title).toContain("cannot see a vault");

    const embed = open(EMBED, null);
    caretOnEmbed(embed);
    expect(control(embed, TABLE_FROM_CSV_LABEL).disabled).toBe(true);
  });

  /**
   * 60 correct unit tests say nothing about whether the product composes this
   * at all. These two assertions are the ones that do: without
   * `tableLayer({ vaultId })` at `livePreview`'s callsite both conversions
   * would be permanently disabled in the app while every test above passed.
   */
  it("is reachable from the product's own renderer and not only from this test", () => {
    /** A view over the real renderer, with the caret where the test needs it. */
    function product(doc: string, caret: number): EditorView {
      const parent = document.createElement("div");
      document.body.append(parent);
      const view = new EditorView({
        parent,
        state: EditorState.create({
          doc,
          selection: EditorSelection.cursor(caret),
          extensions: [
            markdown({ base: markdownLanguage }),
            livePreview({ vaultId: "vault-1", assetUrl: (rel) => rel, onOpenLink: () => {} }),
          ],
        }),
      });
      views.push(view);
      return view;
    }

    const table = product(TABLE, TABLE.length);
    expect(control(table, TABLE_TO_CSV_LABEL).disabled).toBe(false);

    // The way back, in the renderer that owns the embed. The caret starts ON
    // the embed's line, which is both where the control belongs and what stops
    // `livePreview` mounting its CSV panel over the same range — the two
    // decorations cannot both replace it, and the reveal rule is what keeps
    // them out of each other's way.
    const embed = product(EMBED, 7);
    expect(embed.state.doc.lineAt(7).number).toBe(3);
    expect(control(embed, TABLE_FROM_CSV_LABEL).disabled).toBe(false);
  });
});

// --- The conversion's pure parts ---------------------------------------------

describe("naming the file a table becomes", () => {
  it("names it after the header, in characters a wikilink can spell", () => {
    expect(csvTargetFor(["Fruit", "Notes"])).toBe("fruit-notes.csv");
    // `#`, `|`, `[`, `]` and `^` are the characters no embed can name a file
    // with, so a header carrying them cannot leave them in the path.
    expect(csvTargetFor(["Cost (#)", "Δ"])).toBe("cost.csv");
  });

  it("falls back rather than naming a file after nothing", () => {
    expect(csvTargetFor(["", ""])).toBe("table.csv");
    expect(csvTargetFor(["///"])).toBe("table.csv");
  });

  it("does not leave a truncated name ending in a separator", () => {
    expect(csvTargetFor(["a".repeat(60)])).toBe(`${"a".repeat(48)}.csv`);
    expect(csvTargetFor([`${"b".repeat(48)} tail`])).toBe(`${"b".repeat(48)}.csv`);
  });
});

describe("reading a CSV embed's line", () => {
  it("takes the target from a line that holds one embed and nothing else", () => {
    expect(csvEmbedTarget("![[attachments/people.csv]]")).toBe("attachments/people.csv");
    expect(csvEmbedTarget("  ![[people.CSV]]  ")).toBe("people.CSV");
  });

  it("declines anything else", () => {
    expect(csvEmbedTarget("see ![[a.csv]] here")).toBeNull();
    // A link, not an embed: nothing was asked to be shown, so nothing is
    // offered to be converted.
    expect(csvEmbedTarget("[[a.csv]]")).toBeNull();
    expect(csvEmbedTarget("![[a.png]]")).toBeNull();
    expect(csvEmbedTarget("![[a.csv]]{ :value_of }")).toBeNull();
  });
});
