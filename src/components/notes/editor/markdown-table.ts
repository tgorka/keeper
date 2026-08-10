/**
 * A GFM table in a note, rendered as a table and kept aligned while you type
 * (Story 45.9, FR-183, UX-DR72).
 *
 * A table was the one block the note editor still showed as the characters you
 * typed. Everything else — a heading, a quote, an image, a gallery, a mermaid
 * fence, an embedded CSV — renders; a table stayed a wall of pipes, which is
 * the exact complaint this epic exists to answer.
 *
 * **Three separate jobs, and they are separate on purpose.**
 *
 * 1. *Render.* A `StateField` replaces the block with a `<table>`, and puts the
 *    source back the moment the selection touches it — the same reveal rule
 *    every other block in {@link livePreview} follows. A field rather than the
 *    renderer's `ViewPlugin` because a table is several lines replaced by one
 *    element, and CodeMirror refuses a block decoration from a plugin (DW-165).
 *    `galleryLayer` is the in-repo precedent for this shape and this is the
 *    same shape.
 *
 * 2. *Edit the structure.* Add and remove a column, add and remove a row, from
 *    controls on the rendered block. These are the operations that cannot be
 *    expressed by typing: adding a column means editing the header, the
 *    delimiter row and every body row in one step, and doing it by hand is
 *    exactly the chore that makes people stop using tables.
 *
 * 3. *Edit the text.* Put the caret in the table, the pipes come back, and you
 *    type markdown — realigned on every keystroke, in the same transaction as
 *    the keystroke. There is no cell editor floating over the source, because
 *    a widget that owns a text caret has to fight CodeMirror's for it, and the
 *    thing the owner actually asked for is that the pipes line up while they
 *    type.
 *
 * **The source is legible markdown after every single keystroke.** Obsidian
 * reads this same file, and if the app is killed mid-edit a half-written table
 * is what sync carries. So the realign is appended to the user's own
 * transaction with `sequential: true` rather than dispatched after it: the
 * document never holds an unaligned intermediate state, the pair is one undo
 * step, and the edit reaches Rust once.
 *
 * **There is one aligner and it is 44.9's.** {@link alignedTable} pads the
 * table the `/` menu inserts and the table this module maintains. A second
 * aligner written here would disagree with that one about a cell holding an
 * escaped pipe — `a \| b` is one cell to the writer and two to a naive reader —
 * and the two tables would drift apart in a way nobody notices until a diff is
 * unreadable.
 */
import {
  type ChangeSpec,
  EditorState,
  type Extension,
  StateField,
  type Text,
  Transaction,
} from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, WidgetType } from "@codemirror/view";
import { alignedTable, type TableAlign } from "./format-commands";
import { spliceBetween } from "./text-splice";

/** The rendered block. The hook a test finds the table by. */
export const TABLE_BLOCK_CLASS = "cm-md-table";

/** One rendered cell, header or body. */
export const TABLE_CELL_CLASS = "cm-md-table-cell";

/** One structure control. Also the selector {@link TableWidget.ignoreEvent}
 *  uses to keep a press away from CodeMirror. */
export const TABLE_CONTROL_CLASS = "cm-md-table-control";

/** A body row whose cell count is not the header's. Marked rather than fixed:
 *  GFM renders it, and silently rewriting somebody's row is not this module's
 *  business until they ask for a column. */
export const TABLE_RAGGED_CLASS = "cm-md-table-ragged";

/** What the control group is announced as. */
export const TABLE_CONTROLS_LABEL = "Table";

/** A structural edit. Every one of them rewrites the whole block through
 *  {@link alignedTable}, so none can leave the delimiter row behind. */
export type TableOp = "add-column" | "remove-column" | "add-row" | "remove-row";

/** The controls, in the order they are drawn, in the words a user reads. */
export const TABLE_CONTROLS: readonly { readonly op: TableOp; readonly label: string }[] = [
  { op: "add-column", label: "Add column" },
  { op: "remove-column", label: "Remove column" },
  { op: "add-row", label: "Add row" },
  { op: "remove-row", label: "Remove row" },
];

/** One GFM table found in the document. */
export interface TableHit {
  /** Offset of the first character of the header line. */
  from: number;
  /** Offset of the last character of the last row's line. */
  to: number;
  /** 1-based number of the header line, so a realign can splice line by line. */
  firstLine: number;
  /** The block's exact source. Two blocks that read the same are the same
   *  widget; this is half of what {@link TableWidget.eq} compares. */
  text: string;
  /** The header line's indent, reused for every line the block is rewritten
   *  as — a block whose lines are indented differently has no aligned form. */
  indent: string;
  /** The header first. The delimiter row is not one of these: it is derived
   *  from {@link aligns} and the column widths, never carried as text. */
  rows: string[][];
  /** One per header cell. */
  aligns: TableAlign[];
}

/**
 * A line that opens or closes a fenced block.
 *
 * Tracked so that a table inside a ` ```md ` fence stays the sample of markdown
 * it is. Rendering it would be wrong twice over — the reader asked to see the
 * pipes, and the realign would rewrite an example somebody chose the spacing of.
 */
const FENCE = /^ {0,3}(?:```|~~~)/;

/** A line that could be part of a table. The leading pipe is required: without
 *  it every prose sentence containing ` | ` is a candidate, and GFM's
 *  pipe-less form is not what keeper, Obsidian or 44.9's builder writes. */
const TABLE_LINE = /^ {0,3}\|/;

/** A delimiter cell, with or without its alignment colons. */
const DELIMITER_CELL = /^:?-+:?$/;

/**
 * One table row's cells, trimmed, with the fence pipes dropped.
 *
 * A backslash escape is copied through with its backslash intact, which is the
 * whole reason this is a scanner and not `text.split("|")`. `a \| b` is one
 * cell in GFM; split on every pipe and it becomes two, the header and the
 * delimiter row stop matching, the block stops being a table, and a realign
 * would have rewritten the user's cell into two cells on the way past.
 */
export function splitTableRow(text: string): string[] {
  const trimmed = text.trim();
  const cells: string[] = [];
  let cell = "";
  for (let index = 0; index < trimmed.length; index += 1) {
    const char = trimmed[index];
    if (char === "\\" && index + 1 < trimmed.length) {
      cell += char + trimmed[index + 1];
      index += 1;
      continue;
    }
    if (char === "|") {
      cells.push(cell);
      cell = "";
      continue;
    }
    cell += char;
  }
  cells.push(cell);
  // A leading pipe leaves an empty piece in front and a trailing one an empty
  // piece behind: those are the fence, not columns. An empty piece anywhere
  // else is a genuinely empty cell and is kept.
  if (cells.length > 1 && cells[0] === "") {
    cells.shift();
  }
  if (cells.length > 1 && cells[cells.length - 1] === "") {
    cells.pop();
  }
  return cells.map((each) => each.trim());
}

/** How one delimiter cell's colons read. */
function alignOf(cell: string): TableAlign {
  const left = cell.startsWith(":");
  const right = cell.endsWith(":");
  if (left && right) {
    return "center";
  }
  if (left) {
    return "left";
  }
  if (right) {
    return "right";
  }
  return "none";
}

/** The column alignments a delimiter row spells, or null when these cells are
 *  not a delimiter row at all. */
export function tableAligns(cells: readonly string[]): TableAlign[] | null {
  const aligns: TableAlign[] = [];
  for (const cell of cells) {
    if (!DELIMITER_CELL.test(cell)) {
      return null;
    }
    aligns.push(alignOf(cell));
  }
  return aligns;
}

/**
 * Every GFM table in the document.
 *
 * Line-driven rather than tree-driven, for `galleryLayer`'s reason: this runs
 * from a `StateField`, a field has no view to ask which ranges are visible, and
 * walking the whole parse tree on every keystroke would cost far more than a
 * regex per line. The two questions a table asks — does this line start with a
 * pipe, is the second line a delimiter row — are ones a line answers alone.
 *
 * The header and the delimiter row must have the same number of cells, because
 * that is GFM's own rule for whether the block is a table. It is also what
 * makes a half-typed table safe: a header that has grown a column its delimiter
 * row has not is not a table yet, so it is not rendered and not realigned, and
 * the user is left holding exactly the characters they typed.
 */
export function tableHits(doc: Text): TableHit[] {
  const hits: TableHit[] = [];
  let fenced = false;
  let number = 1;
  while (number <= doc.lines) {
    const first = doc.line(number);
    if (FENCE.test(first.text)) {
      fenced = !fenced;
      number += 1;
      continue;
    }
    if (fenced || number === doc.lines || !TABLE_LINE.test(first.text)) {
      number += 1;
      continue;
    }
    const delimiter = doc.line(number + 1);
    const aligns = TABLE_LINE.test(delimiter.text)
      ? tableAligns(splitTableRow(delimiter.text))
      : null;
    const header = splitTableRow(first.text);
    if (aligns === null || aligns.length !== header.length) {
      number += 1;
      continue;
    }
    let last = number + 1;
    while (last < doc.lines && TABLE_LINE.test(doc.line(last + 1).text)) {
      last += 1;
    }
    const rows = [header];
    for (let line = number + 2; line <= last; line += 1) {
      rows.push(splitTableRow(doc.line(line).text));
    }
    const to = doc.line(last).to;
    hits.push({
      from: first.from,
      to,
      firstLine: number,
      text: doc.sliceString(first.from, to),
      indent: /^ */.exec(first.text)?.[0] ?? "",
      rows,
      aligns,
    });
    number = last + 1;
  }
  return hits;
}

/**
 * The block's aligned source, without the trailing newline the hit does not
 * own.
 *
 * The indent is the header line's, applied to every line. A table whose lines
 * start at different columns has no aligned form — the pipes cannot line up —
 * and picking the first line is the only choice that does not depend on which
 * line the user happened to edit.
 */
export function tableSource(
  indent: string,
  rows: readonly (readonly string[])[],
  aligns: readonly TableAlign[],
): string {
  const body = alignedTable(rows, aligns).slice(0, -1);
  if (indent === "") {
    return body;
  }
  return body
    .split("\n")
    .map((line) => indent + line)
    .join("\n");
}

/**
 * A cell as the reader sees it: `\|` is a pipe, `\\` a backslash.
 *
 * Only the rendered table unescapes. The source keeps every byte it was written
 * with, which is what makes a realign byte-identical for a cell holding an
 * escaped pipe — the alternative, unescaping into the model and re-escaping on
 * the way out, is a round trip that has to guess which of the user's
 * backslashes were escapes.
 */
export function tableCellText(cell: string): string {
  return cell.replace(/\\(.)/g, "$1");
}

/**
 * Why `op` is refused on this table, or null when it is not.
 *
 * **Removing the last column is refused, and removing the header row with it.**
 * The alternative — deleting the block — is a destructive act that eats text
 * the user can see, from a button labelled "Remove column", with nothing on
 * screen saying the whole table went. This epic's rule is that a destructive
 * act is confirmed and names what it destroys (AD-89), and a table that is
 * genuinely unwanted is three lines of text: selecting them and pressing delete
 * already works, is visible, and undoes as one step. GFM has no table without a
 * header row and a delimiter row, so "remove the last row" stops at the header
 * for the same reason.
 */
export function tableRefusal(hit: TableHit, op: TableOp): string | null {
  if (op === "remove-column" && hit.rows[0].length <= 1) {
    return "A table needs one column. Select the table and delete it as text.";
  }
  if (op === "remove-row" && hit.rows.length <= 1) {
    return "A table needs its header row. Select the table and delete it as text.";
  }
  return null;
}

/** The rows and alignments `op` produces, or null when {@link tableRefusal}
 *  refuses it. Pure: the dispatch is {@link applyTableOp}'s. */
export function tableAfter(
  hit: TableHit,
  op: TableOp,
): { rows: string[][]; aligns: TableAlign[] } | null {
  if (tableRefusal(hit, op) !== null) {
    return null;
  }
  const rows = hit.rows.map((row) => [...row]);
  const aligns = [...hit.aligns];
  const columns = rows[0].length;
  switch (op) {
    case "add-column":
      // Only the header and the alignments grow. Every body row is filled out
      // to the header's width by the aligner, which is the same rule that fills
      // a row somebody typed short — one behaviour, not two.
      rows[0].push("");
      aligns.push("none");
      break;
    case "remove-column":
      for (const row of rows) {
        row.length = Math.min(row.length, columns - 1);
      }
      aligns.length = columns - 1;
      break;
    case "add-row":
      rows.push(Array.from({ length: columns }, () => ""));
      break;
    case "remove-row":
      rows.pop();
      break;
  }
  return { rows, aligns };
}

/** Apply a structural edit and hand the caret back to the note. Returns whether
 *  anything happened, so a refusal is a value rather than a silence. */
export function applyTableOp(view: EditorView, hit: TableHit, op: TableOp): boolean {
  // The table is re-found in the state as it is now. A widget outlives the
  // state it was built from by however long a finger is on a button, and an
  // edit somewhere else in the note moves every offset below it: one scan on a
  // press is the difference between adding a column and writing a table over
  // whatever now occupies those offsets.
  const hits = tableHits(view.state.doc);
  const current =
    hits.find((each) => each.from === hit.from && each.text === hit.text) ??
    hits.find((each) => each.text === hit.text);
  if (current === undefined) {
    return false;
  }
  const next = tableAfter(current, op);
  if (next === null) {
    return false;
  }
  view.dispatch({
    changes: {
      from: current.from,
      to: current.to,
      insert: tableSource(current.indent, next.rows, next.aligns),
    },
  });
  view.focus();
  return true;
}

/**
 * The rendered table.
 *
 * A block replacement, from a field: several lines become one element, which is
 * the exact shape CodeMirror refuses from a `ViewPlugin` (DW-165).
 */
class TableWidget extends WidgetType {
  constructor(private readonly hit: TableHit) {
    super();
  }

  /** Same place and same source, same table. Position is compared as well as
   *  text because two identical tables in one note are two tables, and the
   *  controls on one must not edit the other. */
  eq(other: TableWidget): boolean {
    return other.hit.from === this.hit.from && other.hit.text === this.hit.text;
  }

  toDOM(view: EditorView): HTMLElement {
    const host = document.createElement("div");
    host.className = TABLE_BLOCK_CLASS;
    const table = document.createElement("table");
    const columns = this.hit.rows[0].length;

    const head = document.createElement("thead");
    const headRow = document.createElement("tr");
    for (let index = 0; index < columns; index += 1) {
      const cell = document.createElement("th");
      const align = this.hit.aligns[index] ?? "none";
      cell.className = TABLE_CELL_CLASS;
      cell.textContent = tableCellText(this.hit.rows[0][index] ?? "");
      // `none` leaves the stylesheet's default rather than writing `left`, so a
      // theme can still decide what an unaligned column looks like.
      cell.style.textAlign = align === "none" ? "" : align;
      headRow.append(cell);
    }
    head.append(headRow);
    table.append(head);

    const body = document.createElement("tbody");
    for (const row of this.hit.rows.slice(1)) {
      const bodyRow = document.createElement("tr");
      if (row.length !== columns) {
        bodyRow.classList.add(TABLE_RAGGED_CLASS);
      }
      for (let index = 0; index < columns; index += 1) {
        const cell = document.createElement("td");
        const align = this.hit.aligns[index] ?? "none";
        cell.className = TABLE_CELL_CLASS;
        cell.textContent = tableCellText(row[index] ?? "");
        cell.style.textAlign = align === "none" ? "" : align;
        bodyRow.append(cell);
      }
      body.append(bodyRow);
    }
    table.append(body);
    host.append(table);

    const controls = document.createElement("div");
    controls.className = "cm-md-table-controls";
    controls.setAttribute("role", "group");
    controls.setAttribute("aria-label", TABLE_CONTROLS_LABEL);
    for (const { op, label } of TABLE_CONTROLS) {
      const button = document.createElement("button");
      button.type = "button";
      button.className = TABLE_CONTROL_CLASS;
      button.textContent = label;
      const refusal = tableRefusal(this.hit, op);
      if (refusal !== null) {
        button.disabled = true;
        button.title = refusal;
      }
      // 44.9's rule for every control that is not a text field: the press must
      // not take the caret out of the note it is about to edit.
      button.addEventListener("mousedown", (event) => {
        event.preventDefault();
      });
      button.addEventListener("click", () => {
        applyTableOp(view, this.hit, op);
      });
      controls.append(button);
    }
    host.append(controls);
    return host;
  }

  /**
   * Keep a press on a control away from CodeMirror.
   *
   * `true` means CodeMirror ignores the event entirely. Without it the press
   * would also move the caret into the block, the block would reveal its source
   * in the same frame, and the button would be gone before its own handler ran.
   * The same trade 44.16's CSV embed widget makes for its cells. Everything else — the
   * cells, the table itself — gives its events up, so clicking the table puts
   * the caret in it and shows the pipes, which is how the text is edited.
   */
  ignoreEvent(event: Event): boolean {
    return (
      event.target instanceof Element && event.target.closest(`.${TABLE_CONTROL_CLASS}`) !== null
    );
  }
}

/** The decorations for `hits`, minus any table the selection is inside. */
function tableDecorations(hits: readonly TableHit[], state: EditorState): DecorationSet {
  const decorations = [];
  for (const hit of hits) {
    // The renderer's own reveal rule, applied to a whole block: put the caret
    // anywhere in a table and the pipes come back, because typing in a cell is
    // typing in the source and the source is the only thing that is real.
    const revealed = state.selection.ranges.some(
      (range) => range.from <= hit.to && range.to >= hit.from,
    );
    if (revealed) {
      continue;
    }
    decorations.push(
      Decoration.replace({ widget: new TableWidget(hit), block: true }).range(hit.from, hit.to),
    );
  }
  return Decoration.set(decorations, true);
}

/** The line-by-line changes that bring one table back into alignment. */
function realignChanges(doc: Text, hit: TableHit): ChangeSpec[] {
  const aligned = tableSource(hit.indent, hit.rows, hit.aligns);
  if (aligned === hit.text) {
    return [];
  }
  const changes: ChangeSpec[] = [];
  const lines = aligned.split("\n");
  for (let index = 0; index < lines.length; index += 1) {
    const line = doc.line(hit.firstLine + index);
    // Line by line, and minimally within the line, because a whole-block
    // replacement would collapse the caret to the block's edge on every
    // keystroke: the padding of the rows ABOVE the one being typed in changes
    // too, so the single minimal splice over the whole block starts before the
    // caret and swallows it. Per line, the only change on the caret's own line
    // is the padding after the caret, which the caret survives.
    const splice = spliceBetween(line.text, lines[index]);
    if (splice !== null) {
      changes.push({
        from: line.from + splice.from,
        to: line.from + splice.to,
        insert: splice.insert,
      });
    }
  }
  return changes;
}

/**
 * Realign every table an edit touched, inside the transaction that made the
 * edit.
 *
 * `sequential: true` appends the padding to the user's own transaction instead
 * of dispatching a second one. Three things follow, and all three are the
 * point: the document is never once observable in an unaligned state — which
 * matters because sync carries whatever is in the buffer when the app dies —
 * the keystroke and its realign undo as one step rather than two, and the note
 * is reported to Rust once instead of twice per character.
 *
 * A remote change is left alone. It came from another editor that has already
 * aligned it its own way, and re-padding it here would send a change back that
 * the other side would re-pad in turn. Undo and redo are left alone for the
 * plainer reason: a realign appended to an undo would stop undo restoring what
 * was there.
 */
const realignTables = EditorState.transactionFilter.of((transaction) => {
  if (!transaction.docChanged || transaction.annotation(Transaction.remote) === true) {
    return transaction;
  }
  if (transaction.isUserEvent("undo") || transaction.isUserEvent("redo")) {
    return transaction;
  }
  const touched: { from: number; to: number }[] = [];
  transaction.changes.iterChangedRanges((_fromA, _toA, fromB, toB) => {
    touched.push({ from: fromB, to: toB });
  });
  const { doc } = transaction.state;
  const changes: ChangeSpec[] = [];
  for (const hit of tableHits(doc)) {
    const edited = touched.some((range) => range.from <= hit.to && range.to >= hit.from);
    if (edited) {
      changes.push(...realignChanges(doc, hit));
    }
  }
  if (changes.length === 0) {
    return transaction;
  }
  return [transaction, { changes, sequential: true }];
});

const tableTheme = EditorView.baseTheme({
  [`.${TABLE_BLOCK_CLASS}`]: {
    margin: "0.4em 0",
  },
  [`.${TABLE_BLOCK_CLASS} table`]: {
    borderCollapse: "collapse",
    width: "auto",
  },
  [`.${TABLE_CELL_CLASS}`]: {
    border: "1px solid currentColor",
    borderColor: "color-mix(in srgb, currentColor 25%, transparent)",
    padding: "0.15em 0.5em",
    verticalAlign: "top",
  },
  [`.${TABLE_BLOCK_CLASS} th.${TABLE_CELL_CLASS}`]: {
    fontWeight: "600",
  },
  [`.${TABLE_RAGGED_CLASS} .${TABLE_CELL_CLASS}`]: {
    // The row has a different number of cells from the header, so what is drawn
    // is not everything that is written. Said in the margin rather than fixed.
    borderStyle: "dashed",
  },
  ".cm-md-table-controls": {
    display: "flex",
    gap: "0.25em",
    marginTop: "0.25em",
  },
  [`.${TABLE_CONTROL_CLASS}`]: {
    background: "transparent",
    border: "1px solid currentColor",
    borderColor: "color-mix(in srgb, currentColor 25%, transparent)",
    borderRadius: "0.25em",
    cursor: "pointer",
    font: "inherit",
    fontSize: "0.75em",
    padding: "0.1em 0.4em",
  },
  [`.${TABLE_CONTROL_CLASS}:disabled`]: {
    cursor: "not-allowed",
    opacity: "0.4",
  },
});

/**
 * The table layer: the rendered block, the structure controls and the realign.
 *
 * Composed into {@link livePreview}'s extension array beside `galleryLayer`, so
 * a note still has one renderer. The scan is doc-driven and the reveal is
 * selection-driven, and they are separated: moving the caret rebuilds the
 * decoration set from the tables already found, and only an edit re-scans.
 */
export function tableLayer(): Extension {
  return [
    StateField.define<{ hits: TableHit[]; decorations: DecorationSet }>({
      create(state) {
        const hits = tableHits(state.doc);
        return { hits, decorations: tableDecorations(hits, state) };
      },
      update(value, transaction) {
        if (!transaction.docChanged && transaction.selection === undefined) {
          return value;
        }
        const hits = transaction.docChanged ? tableHits(transaction.state.doc) : value.hits;
        return { hits, decorations: tableDecorations(hits, transaction.state) };
      },
      provide: (field) => EditorView.decorations.from(field, (value) => value.decorations),
    }),
    realignTables,
    tableTheme,
  ];
}
