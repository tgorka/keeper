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
 * 4. *Stop being markdown.* Convert the table into a CSV attachment, and an
 *    embedded CSV back into a table. A table is markdown only this aligner
 *    maintains; a `.csv` beside the note is a file the Files pane, the export
 *    and every machine the vault syncs to can read — so which of the two a
 *    given table should be is the author's call and not a decision they make
 *    once, at the moment they first type a pipe.
 *
 * **The source is legible markdown after every single keystroke.** Obsidian
 * reads this same file, and if the app is killed mid-edit a half-written table
 * is what sync carries. So the realign is appended to the user's own
 * transaction with `sequential: true` rather than dispatched after it: the
 * document never holds an unaligned intermediate state, the pair is one undo
 * step, and the edit reaches Rust once.
 *
 * **The caret is placed, not mapped.** A realign rewrites the padding around
 * the caret, and mapping a caret through the bytes that were just replaced put
 * the owner's second keystroke four columns from their first — see
 * {@link realignedCaret}, which is the one piece of arithmetic in here a user
 * notices immediately.
 *
 * **There is one aligner and it is 44.9's.** {@link alignedTable} pads the
 * table the `/` menu inserts and the table this module maintains. A second
 * aligner written here would disagree with that one about a cell holding an
 * escaped pipe — `a \| b` is one cell to the writer and two to a naive reader —
 * and the two tables would drift apart in a way nobody notices until a diff is
 * unreadable.
 */
import {
  EditorSelection,
  EditorState,
  type Extension,
  StateField,
  type Text,
  Transaction,
} from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, WidgetType } from "@codemirror/view";
import { type NoteCsvVm, notesCsvFromTable, notesTableFromCsv } from "@/lib/ipc/client";
import { attachmentEmbed } from "@/lib/notes/attach";
import { alignedTable, type TableAlign } from "./format-commands";
import { spliceBetween, type TextSplice } from "./text-splice";

/** The rendered block. The hook a test finds the table by. */
export const TABLE_BLOCK_CLASS = "cm-md-table";

/**
 * The box between the block and the table, which is the only thing in here that
 * can scroll.
 *
 * `EditorView.lineWrapping` pins the content box to the editor's own width, so
 * prose wraps and a BLOCK WIDGET does not: a table of eight columns is as wide
 * as its columns need, and with nothing between it and the pane it simply left
 * the pane — the owner's "notes are truncated with nothing to scroll". The
 * table cannot be the scroll box itself (a `<table>` given `overflow` and a
 * `display` that honours it stops being a table), and the block cannot be it
 * either: {@link TABLE_CONTROLS_LABEL}'s cluster is the block's other child and
 * scrolling the table sideways must not carry the controls off with it.
 *
 * So: one wrapper, the shape `.cm-lp-gallery-grid` already uses for the same
 * problem on the other axis — a fixed box with `auto` overflow, where the
 * content is what moves.
 */
export const TABLE_SCROLL_CLASS = "cm-md-table-scroll";

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

/** The conversion controls, in the words a user reads (item 8). */
export const TABLE_TO_CSV_LABEL = "Convert to a CSV attachment";
export const TABLE_FROM_CSV_LABEL = "Convert to a table";

/** Why a conversion is not on offer here. A disabled control that says nothing
 *  is indistinguishable from a broken one. */
export const TABLE_NO_VAULT =
  "keeper cannot see a vault from this editor, so it has nowhere to write a file.";

/**
 * What a conversion says when what it was pressed on is no longer there: the
 * note was edited while a command was in flight, or while a finger was on the
 * button.
 *
 * Worded for both directions, because both refuse for the same reason — the
 * offsets it holds are now somebody else's text.
 */
export const TABLE_MOVED =
  "This note changed while keeper was working, so nothing was written. Try again.";

/** A refusal, or a fact, read where the reader is already looking. */
export const TABLE_NOTICE_CLASS = "cm-md-table-notice";

/** The question asked before a conversion destroys a file, and its two
 *  answers. Words rather than glyphs: this is the one control in the block
 *  whose outcomes are not symmetrical, and nobody can draw "replace". */
export const TABLE_ASK_CLASS = "cm-md-table-ask";
export const TABLE_REPLACE_CLASS = "cm-md-table-replace";
export const TABLE_REPLACE_LABEL = "Replace the file";
export const TABLE_KEEP_LABEL = "Keep the file";

/** `createElementNS` needs it; `createElement` would make an unrendered HTML
 *  element called "svg". */
const SVG_NS = "http://www.w3.org/2000/svg";

/** A structural edit. Every one of them rewrites the whole block through
 *  {@link alignedTable}, so none can leave the delimiter row behind. */
export type TableOp = "add-column" | "remove-column" | "add-row" | "remove-row";

/**
 * One node of an SVG icon: a tag and its attributes, which is exactly the
 * shape `lucide-react` compiles each of its icons to.
 *
 * Copied as data rather than imported because this control is built with
 * `document.createElement` inside a CodeMirror `WidgetType` — there is no React
 * here to render a `<Paperclip />` into, and `lucide-react` publishes its icon
 * nodes only through the React components. Four glyphs of geometry is a
 * smaller and more honest cost than mounting a React root per table.
 */
type IconNode = readonly (readonly [string, Readonly<Record<string, string>>])[];

/**
 * The controls, in the order they are drawn, in the words a user reads and the
 * glyph they now read instead.
 *
 * The words did not go anywhere: each is the control's `aria-label` and its
 * `title`, so speech input can still say what an eye sees (WCAG 2.5.3) and the
 * suite finds these by accessible name.
 *
 * The four glyphs are two axes crossed with two directions, taken verbatim from
 * lucide (`between-vertical-start`, `fold-horizontal`, `between-horizontal-
 * start`, `fold-vertical`) so they sit beside the format toolbar's lucide marks
 * without looking hand-drawn. Arrows pushing OUT between two blocks insert;
 * arrows folding IN toward a dashed seam remove. Axis is in the glyph as well
 * as in the word, so the pair cannot be told apart only by position.
 */
export const TABLE_CONTROLS: readonly {
  readonly op: TableOp;
  readonly label: string;
  readonly icon: IconNode;
}[] = [
  {
    op: "add-column",
    label: "Add column",
    icon: [
      ["rect", { width: "7", height: "13", x: "3", y: "8", rx: "1" }],
      ["path", { d: "m15 2-3 3-3-3" }],
      ["rect", { width: "7", height: "13", x: "14", y: "8", rx: "1" }],
    ],
  },
  {
    op: "remove-column",
    label: "Remove column",
    icon: [
      ["path", { d: "M2 12h6" }],
      ["path", { d: "M22 12h-6" }],
      ["path", { d: "M12 2v2" }],
      ["path", { d: "M12 8v2" }],
      ["path", { d: "M12 14v2" }],
      ["path", { d: "M12 20v2" }],
      ["path", { d: "m19 9-3 3 3 3" }],
      ["path", { d: "m5 15 3-3-3-3" }],
    ],
  },
  {
    op: "add-row",
    label: "Add row",
    icon: [
      ["rect", { width: "13", height: "7", x: "8", y: "3", rx: "1" }],
      ["path", { d: "m2 9 3 3-3 3" }],
      ["rect", { width: "13", height: "7", x: "8", y: "14", rx: "1" }],
    ],
  },
  {
    op: "remove-row",
    label: "Remove row",
    icon: [
      ["path", { d: "M12 22v-6" }],
      ["path", { d: "M12 8V2" }],
      ["path", { d: "M4 12H2" }],
      ["path", { d: "M10 12H8" }],
      ["path", { d: "M16 12h-2" }],
      ["path", { d: "M22 12h-2" }],
      ["path", { d: "m15 19-3-3-3 3" }],
      ["path", { d: "m15 5-3 3-3-3" }],
    ],
  },
];

/**
 * The two conversion glyphs, verbatim from lucide (`file-spreadsheet`,
 * `table`), for the reason {@link TABLE_CONTROLS} gives.
 *
 * The DESTINATION is what each one draws, not the act: a table becoming a file
 * is a file with a grid in it, and a file becoming a table is a table. Drawing
 * the act instead would need an arrow, and the four structural controls have
 * already spent every arrow direction on something else.
 */
const TO_CSV_ICON: IconNode = [
  [
    "path",
    {
      d: "M6 22a2 2 0 0 1-2-2V4a2 2 0 0 1 2-2h8a2.4 2.4 0 0 1 1.704.706l3.588 3.588A2.4 2.4 0 0 1 20 8v12a2 2 0 0 1-2 2z",
    },
  ],
  ["path", { d: "M14 2v5a1 1 0 0 0 1 1h5" }],
  ["path", { d: "M8 13h2" }],
  ["path", { d: "M14 13h2" }],
  ["path", { d: "M8 17h2" }],
  ["path", { d: "M14 17h2" }],
];

const FROM_CSV_ICON: IconNode = [
  ["path", { d: "M12 3v18" }],
  ["rect", { width: "18", height: "18", x: "3", y: "3", rx: "2" }],
  ["path", { d: "M3 9h18" }],
  ["path", { d: "M3 15h18" }],
];

/**
 * How a conversion reaches Rust.
 *
 * Injected the way {@link CsvTableOptions} injects the CSV panel's two calls,
 * so both directions — and, more to the point, both refusals — are reachable in
 * a test with no Tauri host under it.
 */
export interface TableConversion {
  /** A CSV attachment's records, at the delimiter Rust detected. */
  toRows: (vaultId: string, target: string) => Promise<string[][]>;
  /**
   * Rows to CSV bytes at `target`.
   *
   * `overwrite` is passed explicitly at both call sites, never defaulted: the
   * thing at stake is a file of the user's data, and the refusal this raises
   * when a file is already there IS the existence probe. Looking first and
   * writing second has a window in it — this folder is being synced, so a file
   * can arrive in the gap — and the answer's `relPath` is the path actually
   * written, which is not always the `target` asked for.
   */
  toCsv: (
    vaultId: string,
    target: string,
    rows: string[][],
    overwrite: boolean,
  ) => Promise<NoteCsvVm>;
}

export interface TableLayerOptions {
  /**
   * The open vault, so a table can become a file and a file a table.
   *
   * Optional because a host may mount this layer with no vault at all — and
   * when it does, the two conversion controls are DISABLED and say why, rather
   * than being absent. A control that vanishes is a feature the user concludes
   * does not exist.
   */
  readonly vaultId?: string;
  /** Defaults to the two `notes_*_csv*` commands. */
  readonly convert?: TableConversion;
}

/**
 * The commands, when nothing is injected.
 *
 * Each is CALLED here rather than referenced, so the two bindings are read when
 * a control is pressed and not when this module is imported. A suite that mocks
 * `@/lib/ipc/client` with a factory listing the exports it needs — `live-
 * preview.ts` pulls this module in, so several do — throws on the mere read of
 * an export the factory does not name, and at import time that presents as the
 * whole file failing to load rather than as one failed assertion.
 */
const IPC_CONVERSION: TableConversion = {
  toRows: (vaultId, target) => notesTableFromCsv(vaultId, target),
  toCsv: (vaultId, target, rows, overwrite) => notesCsvFromTable(vaultId, target, rows, overwrite),
};

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
 * A cell as GFM has to spell it: the inverse of {@link tableCellText}.
 *
 * Needed only on the way IN from a CSV, where a field holding `a|b` is one
 * field. Written into a markdown table unescaped it would become two cells, the
 * row would stop matching the header, and the block the user asked for would
 * not be a table at all.
 */
export function tableCellSource(text: string): string {
  return text.replace(/([\\|])/g, "\\$1");
}

/**
 * The table's cells as a CSV should hold them.
 *
 * Unescaped, because CSV has no pipe escape and `a \| b` in a note is the one
 * value `a | b`. Filled out to the header's width, because that is the table
 * the user was LOOKING at when they asked for a file — the aligner draws a
 * short row's missing cells. A row with more cells than the header keeps every
 * one: a conversion is not a place to lose a column.
 */
export function csvRows(hit: TableHit): string[][] {
  const columns = hit.rows[0].length;
  return hit.rows.map((row) =>
    Array.from({ length: Math.max(columns, row.length) }, (_, index) =>
      tableCellText(row[index] ?? ""),
    ),
  );
}

/**
 * The file a table becomes, named after its header.
 *
 * A BARE name and never `attachments/…`: Rust resolves a bare name into the
 * vault's attachments folder and answers with the path it actually wrote, and
 * composing that path here is the arithmetic AD-65 keeps out of the webview.
 *
 * Slugged to `a-z0-9-`, not for looks: a wikilink cannot spell `#`, `|`, `[`,
 * `]` or `^` (`planAttachments` refuses those outright), so a header of
 * "Cost (#)" would name a file no embed can point at. A header that slugs away
 * to nothing leaves nothing to name it after, hence `table`.
 */
export function csvTargetFor(header: readonly string[]): string {
  const slug = header
    .map((cell) => tableCellText(cell))
    .join("-")
    .toLowerCase()
    .replace(/[^a-z0-9]+/g, "-")
    .slice(0, 48)
    .replace(/^-+|-+$/g, "");
  return `${slug === "" ? "table" : slug}.csv`;
}

/**
 * A line that embeds one CSV and holds nothing else, and the target inside it.
 *
 * The WHOLE line, because a table owns its lines: an embed in the middle of a
 * sentence has nowhere to put three lines of pipes, so that line is not offered
 * the conversion rather than offered it and refused. A trailing `{ :predicate }`
 * block fails the match for a sharper reason — converting the line would delete
 * a predicate the author wrote, and this control is not the place that decides
 * what happens to it.
 */
const CSV_EMBED_LINE = /^\s*!\[\[\s*([^[\]|#^]+\.csv)\s*\]\]\s*$/i;

/** The CSV `text` embeds on its own, or null. */
export function csvEmbedTarget(text: string): string | null {
  return CSV_EMBED_LINE.exec(text)?.[1].trim() ?? null;
}

/**
 * The markdown table `rows` become, or the reason there is none.
 *
 * **A field holding a line break is refused, and named.** GFM has no spelling
 * for one inside a cell — the newline would end the row — so the choices are to
 * mangle the value into a space or to say so. A conversion that silently
 * rewrites the user's data is the worse of the two, and a CSV exported from a
 * spreadsheet is exactly where multi-line fields come from.
 */
export function tableFromRows(
  indent: string,
  rows: readonly (readonly string[])[],
): { source: string } | { refusal: string } {
  if (rows.length === 0) {
    return { refusal: "That file has no records, so there is no table to write." };
  }
  for (const [index, row] of rows.entries()) {
    const broken = row.findIndex((field) => /[\r\n]/.test(field));
    if (broken >= 0) {
      return {
        refusal: `Record ${index + 1}, field ${broken + 1} holds a line break, and a markdown table cell cannot. keeper left the file alone.`,
      };
    }
  }
  const escaped = rows.map((row) => row.map((field) => tableCellSource(field)));
  // The one aligner, through the one caller of it in this module. The
  // alignments are `none` because a CSV carries none: inventing `:---:` here
  // would be this module deciding how somebody's numbers are set.
  return {
    source: tableSource(
      indent,
      escaped,
      escaped[0].map(() => "none"),
    ),
  };
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
 * What a rejected command said, and the code it said it with.
 *
 * `IpcError` is the envelope every fallible command rejects with (AD-8), but a
 * widget has to survive a plain thrown value too: an injected conversion in a
 * test, or a webview with no Tauri host under it, rejects with whatever it
 * liked.
 */
function failureOf(error: unknown): { code: string; message: string } {
  if (typeof error === "object" && error !== null) {
    const { code, message } = error as { code?: unknown; message?: unknown };
    if (typeof message === "string" && message !== "") {
      return { code: typeof code === "string" ? code : "", message };
    }
  }
  return { code: "", message: String(error) };
}

/**
 * A sentence in the block, where the reader is already looking.
 *
 * A `<span>` rather than a `<p>`, because this same element is appended inside
 * a block widget AND inside an inline widget sitting at the end of a line of
 * text, where a `<p>` would be a paragraph nested in a line.
 */
function tableNotice(text: string): HTMLElement {
  const notice = document.createElement("span");
  notice.className = TABLE_NOTICE_CLASS;
  notice.setAttribute("role", "alert");
  // `textContent`, never `innerHTML`: the sentence names a path off the disk.
  notice.textContent = text;
  return notice;
}

/** A press on the block's own chrome must not move the caret into the block it
 *  is about to rewrite: 44.9's rule for every control that is not a text
 *  field. */
function keepsCaret(button: HTMLButtonElement): HTMLButtonElement {
  button.addEventListener("mousedown", (event) => {
    event.preventDefault();
  });
  return button;
}

/**
 * The question asked before a conversion destroys a file (AD-89).
 *
 * `reason` is Rust's own sentence, which names the file — so the question names
 * what it would destroy without this module composing a path to say it with.
 * Both answers are explicit: pressing nothing leaves the file alone, and the
 * "keep" button exists so that leaving it alone is also something a keyboard
 * can DO rather than only something it can decline.
 */
function askToReplace(reason: string, replace: () => void): HTMLElement {
  const ask = document.createElement("span");
  ask.className = TABLE_ASK_CLASS;
  const question = document.createElement("span");
  // `alert` on the SENTENCE, and no `role="group"` around the three of them: an
  // alert is announced when it is inserted, which is the only way a reader who
  // cannot see this appear learns that the press asked them something instead
  // of doing it. A group would add a wrapper to read past and would still be
  // silent.
  question.setAttribute("role", "alert");
  question.textContent = reason;
  ask.append(question);

  const yes = keepsCaret(document.createElement("button"));
  yes.type = "button";
  yes.className = TABLE_REPLACE_CLASS;
  yes.textContent = TABLE_REPLACE_LABEL;
  yes.addEventListener("click", () => {
    ask.remove();
    replace();
  });

  const no = keepsCaret(document.createElement("button"));
  no.type = "button";
  no.textContent = TABLE_KEEP_LABEL;
  no.addEventListener("click", () => {
    ask.remove();
  });
  ask.append(yes, no);
  return ask;
}

/**
 * Write this table out as a CSV attachment and leave an embed in its place.
 *
 * The point of the whole conversion is the second half: the data stops being
 * markdown only this editor's aligner maintains and becomes a file the Files
 * pane, the export and every other machine this vault syncs to can read.
 *
 * Re-scanned on the press for {@link applyTableOp}'s reason. Never rejects: a
 * refusal is a sentence in the block, because the alternative is an unhandled
 * rejection and a button that appeared to do nothing.
 */
async function writeTableAsCsv(
  view: EditorView,
  hit: TableHit,
  host: HTMLElement,
  vaultId: string,
  convert: TableConversion,
  overwrite: boolean,
): Promise<void> {
  const hits = tableHits(view.state.doc);
  const current =
    hits.find((each) => each.from === hit.from && each.text === hit.text) ??
    hits.find((each) => each.text === hit.text);
  if (current === undefined) {
    host.append(tableNotice(TABLE_MOVED));
    return;
  }
  const target = csvTargetFor(current.rows[0]);
  let written: NoteCsvVm;
  try {
    written = await convert.toCsv(vaultId, target, csvRows(current), overwrite);
  } catch (error) {
    const failure = failureOf(error);
    // `notesInvalid` from THIS call is "a file is already there". The command's
    // only other refusal of that code is a `.md` target, and the target this
    // module forms always ends in `.csv` — so the question is the right one to
    // ask, and asking is the rule: a conversion never destroys a file the user
    // has not been shown the name of.
    if (!overwrite && failure.code === "notesInvalid") {
      host.append(
        askToReplace(failure.message, () => {
          void writeTableAsCsv(view, hit, host, vaultId, convert, true);
        }),
      );
      return;
    }
    host.append(tableNotice(failure.message));
    return;
  }
  // The answer's `relPath`, never the target that was asked for: a bare name
  // resolves under `attachments/`, so the two differ exactly when a caller is
  // most likely to assume they do not, and an embed naming the unresolved
  // spelling points at a file the reader cannot open. Written through
  // `attachmentEmbed`, the app's one embed writer — the second spelling of an
  // embed is the one `live-preview.ts` renders as flat text.
  view.dispatch({
    changes: {
      from: current.from,
      to: current.to,
      insert: attachmentEmbed(written.relPath),
    },
  });
  view.focus();
}

/**
 * Read this embed's CSV back into an aligned markdown table.
 *
 * **The file is left on disk.** Deleting it would be a destructive act from a
 * button that says "convert", with nothing on screen naming what went (AD-89) —
 * and the file may well be embedded by another note. What the user asked for is
 * the data back in the note, and that is what this does.
 *
 * The line is re-found AFTER the read rather than before it: the note can be
 * edited while a command is in flight, and writing three lines of table over
 * whatever now occupies those offsets is the failure this avoids.
 */
async function readCsvAsTable(
  view: EditorView,
  at: number,
  target: string,
  host: HTMLElement,
  vaultId: string,
  convert: TableConversion,
): Promise<void> {
  let rows: string[][];
  try {
    rows = await convert.toRows(vaultId, target);
  } catch (error) {
    host.append(tableNotice(failureOf(error).message));
    return;
  }
  // Re-found AFTER the read, never before it: the note can be edited while a
  // command is in flight, and the stale line's offsets are both wrong and — in
  // a note that got shorter — out of range, which throws inside a promise
  // nobody is awaiting.
  const line = view.state.doc.lineAt(Math.min(at, view.state.doc.length));
  if (csvEmbedTarget(line.text) !== target) {
    host.append(tableNotice(TABLE_MOVED));
    return;
  }
  const written = tableFromRows(/^ */.exec(line.text)?.[0] ?? "", rows);
  if ("refusal" in written) {
    host.append(tableNotice(written.refusal));
    return;
  }
  view.dispatch({ changes: { from: line.from, to: line.to, insert: written.source } });
  view.focus();
}

/**
 * One glyph control: the button, the name it answers to, and its picture.
 *
 * Shared by the four structural controls and the two conversions so that a
 * sixth control cannot arrive with slightly different lucide attributes — the
 * defect the owner photographed was a control that did not look like the
 * others.
 */
function glyphButton(label: string, icon: IconNode): HTMLButtonElement {
  const button = keepsCaret(document.createElement("button"));
  button.type = "button";
  button.className = TABLE_CONTROL_CLASS;
  // The word leaves the surface and stays everywhere it was load-bearing: the
  // accessible name, and the tooltip a pointer gets.
  button.setAttribute("aria-label", label);
  button.title = label;
  const svg = document.createElementNS(SVG_NS, "svg");
  // lucide's own defaults, which is what makes these read as the same family as
  // the format toolbar's marks rather than as a row of odd drawings.
  svg.setAttribute("viewBox", "0 0 24 24");
  svg.setAttribute("fill", "none");
  svg.setAttribute("stroke", "currentColor");
  svg.setAttribute("stroke-width", "2");
  svg.setAttribute("stroke-linecap", "round");
  svg.setAttribute("stroke-linejoin", "round");
  // The picture says nothing the `aria-label` has not already said.
  svg.setAttribute("aria-hidden", "true");
  for (const [tag, attrs] of icon) {
    const node = document.createElementNS(SVG_NS, tag);
    for (const [name, value] of Object.entries(attrs)) {
      node.setAttribute(name, value);
    }
    svg.append(node);
  }
  button.append(svg);
  return button;
}

/**
 * The rendered table.
 *
 * A block replacement, from a field: several lines become one element, which is
 * the exact shape CodeMirror refuses from a `ViewPlugin` (DW-165).
 */
class TableWidget extends WidgetType {
  constructor(
    private readonly hit: TableHit,
    private readonly options: TableLayerOptions,
  ) {
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
    // The table goes in the scroll box and the scroll box goes in the block, so
    // a table wider than the pane moves under the controls rather than past
    // them. See {@link TABLE_SCROLL_CLASS}.
    const scroll = document.createElement("div");
    scroll.className = TABLE_SCROLL_CLASS;
    scroll.append(table);
    host.append(scroll);

    const controls = document.createElement("div");
    controls.className = "cm-md-table-controls";
    controls.setAttribute("role", "group");
    controls.setAttribute("aria-label", TABLE_CONTROLS_LABEL);
    for (const { op, label, icon } of TABLE_CONTROLS) {
      const button = glyphButton(label, icon);
      const refusal = tableRefusal(this.hit, op);
      if (refusal !== null) {
        button.disabled = true;
        // A refusal is appended to the tooltip rather than replacing it, so a
        // disabled control still says WHICH control it is before it says why it
        // will not act.
        button.title = `${label} — ${refusal}`;
      }
      button.addEventListener("click", () => {
        applyTableOp(view, this.hit, op);
      });
      controls.append(button);
    }
    // The fifth control, and the only one that writes anything outside the
    // note: the table leaves the note and becomes a file the rest of the drive
    // can read (item 8).
    const toCsv = glyphButton(TABLE_TO_CSV_LABEL, TO_CSV_ICON);
    const { vaultId } = this.options;
    if (vaultId === undefined) {
      toCsv.disabled = true;
      toCsv.title = `${TABLE_TO_CSV_LABEL} — ${TABLE_NO_VAULT}`;
    } else {
      toCsv.addEventListener("click", () => {
        void writeTableAsCsv(
          view,
          this.hit,
          host,
          vaultId,
          this.options.convert ?? IPC_CONVERSION,
          false,
        );
      });
    }
    controls.append(toCsv);
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
      event.target instanceof Element &&
      event.target.closest(`.${TABLE_CONTROL_CLASS}, .${TABLE_ASK_CLASS}`) !== null
    );
  }
}

/**
 * The control that brings a CSV attachment back into the note as a table
 * (item 8), offered on the line the caret is on.
 *
 * **Why here and not on the rendered CSV panel.** While the caret is on the
 * embed's line, `live-preview.ts` renders nothing over it — the whole embed
 * loop runs inside its "this line is not revealed" branch — so a revealed line
 * is the one place a control can be put without two decorations replacing the
 * same range. It appears exactly when the source does, which is the same reveal
 * rule the table block itself follows.
 */
class CsvEmbedWidget extends WidgetType {
  constructor(
    private readonly target: string,
    private readonly at: number,
    private readonly options: TableLayerOptions,
  ) {
    super();
  }

  eq(other: CsvEmbedWidget): boolean {
    return other.target === this.target && other.at === this.at;
  }

  toDOM(view: EditorView): HTMLElement {
    const host = document.createElement("span");
    // The class, for the layout the four glyphs use — and no `role="group"`
    // with it: a group of one control is a wrapper a screen reader has to read
    // past to reach the only thing in it. The button's own name is the whole of
    // what there is to announce here.
    host.className = "cm-md-table-controls";
    const button = glyphButton(TABLE_FROM_CSV_LABEL, FROM_CSV_ICON);
    const { vaultId } = this.options;
    if (vaultId === undefined) {
      button.disabled = true;
      button.title = `${TABLE_FROM_CSV_LABEL} — ${TABLE_NO_VAULT}`;
    } else {
      button.addEventListener("click", () => {
        void readCsvAsTable(
          view,
          this.at,
          this.target,
          host,
          vaultId,
          this.options.convert ?? IPC_CONVERSION,
        );
      });
    }
    host.append(button);
    return host;
  }

  /** {@link TableWidget.ignoreEvent}'s reason, for the same chrome. */
  ignoreEvent(event: Event): boolean {
    return (
      event.target instanceof Element &&
      event.target.closest(`.${TABLE_CONTROL_CLASS}, .${TABLE_ASK_CLASS}`) !== null
    );
  }
}

/** The decorations for `hits`, minus any table the selection is inside, plus
 *  the conversion control on a CSV embed the caret is sitting on. */
function tableDecorations(
  hits: readonly TableHit[],
  state: EditorState,
  options: TableLayerOptions,
): DecorationSet {
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
      Decoration.replace({ widget: new TableWidget(hit, options), block: true }).range(
        hit.from,
        hit.to,
      ),
    );
  }
  // The caret's own line only, and the head rather than every range it touches:
  // this is a control offered where the caret is, and a select-all would
  // otherwise hang one off every CSV embed in the note.
  const line = state.doc.lineAt(state.selection.main.head);
  const target = csvEmbedTarget(line.text);
  if (target !== null) {
    // `side: 1` puts it after the embed's own text rather than in front of it,
    // so the line still reads as the thing the author typed.
    decorations.push(
      Decoration.widget({
        widget: new CsvEmbedWidget(target, line.from, options),
        side: 1,
      }).range(line.to),
    );
  }
  return Decoration.set(decorations, true);
}

/**
 * Where each of a row's cells lies in its line: the span between two pipes,
 * padding and all.
 *
 * The same scan as {@link splitTableRow}, deliberately: the cell index this
 * produces is used as the ALIGNER's cell index, so the two have to agree on
 * what a cell is. `a \| b` is one cell in GFM, and a reader that split on every
 * pipe would count a different number of them.
 *
 * (The escape rule cannot currently change the answer {@link realignedCaret}
 * gives — the aligner never rewrites a cell's interior bytes, so a wrong split
 * lands identically in both strings and cancels. It is here because a splitter
 * over a table row that treats `\|` as a separator is wrong about tables, and
 * because the day the aligner does touch a cell's interior the cancellation
 * stops.)
 */
function tableCellSpans(text: string): { from: number; to: number }[] {
  const spans: { from: number; to: number }[] = [];
  let start = 0;
  for (let index = 0; index < text.length; index += 1) {
    if (text[index] === "\\" && index + 1 < text.length) {
      index += 1;
      continue;
    }
    if (text[index] === "|") {
      spans.push({ from: start, to: index });
      start = index + 1;
    }
  }
  spans.push({ from: start, to: text.length });
  const last = spans[spans.length - 1];
  // The fence pipes leave a whitespace-only piece at each end, and only at the
  // ends: an empty piece in the middle is a genuinely empty cell, which
  // {@link splitTableRow} keeps for the same reason.
  if (spans.length > 1 && text.slice(last.from, last.to).trim() === "") {
    spans.pop();
  }
  if (spans.length > 1 && text.slice(spans[0].from, spans[0].to).trim() === "") {
    spans.shift();
  }
  return spans;
}

/**
 * Where the caret at `offset` in `line` belongs once that line reads `aligned`,
 * or null when the caret was not inside a cell at all.
 *
 * **The owner's `| a    la |` (item 4).** Typing `ala` into a cell put the
 * first letter in one place and the other two four columns further along. The
 * realign was right and the mapping was wrong: the splice that pulls a letter
 * typed in a cell's padding back to the cell's first column STARTS before the
 * caret and ENDS at it, so CodeMirror maps the caret to the end of the padding
 * it just wrote — the far side of the cell. The next two letters were typed
 * there, and being interior to the cell's own text by then, no later realign
 * took the spaces between them out again.
 *
 * So the caret is placed rather than mapped, by the one rule that survives
 * repadding: it keeps its offset within the cell's TEXT. Clamped to that text's
 * length, because padding is not a place a character can be — a caret standing
 * in it is put back on the end of the cell's text, which is where the letter it
 * is about to type would land anyway.
 *
 * Stated for one line rather than for the block, and pure, so the rule can be
 * checked without a view: this is the only arithmetic in the realign that a
 * user notices immediately and silently.
 */
export function realignedCaret(line: string, aligned: string, offset: number): number | null {
  const spans = tableCellSpans(line);
  // A position can fall in two spans only if one ends where the next begins,
  // and a pipe always separates them — so the cell a caret is in is unique.
  const index = spans.findIndex((span) => offset >= span.from && offset <= span.to);
  const target = tableCellSpans(aligned)[index];
  if (index < 0 || target === undefined) {
    return null;
  }
  const cell = line.slice(spans[index].from, spans[index].to);
  const text = cell.trim();
  const lead = cell.length - cell.trimStart().length;
  const within = Math.min(Math.max(offset - spans[index].from - lead, 0), text.length);
  const padded = aligned.slice(target.from, target.to);
  // Where the aligner put this cell's text. An EMPTY cell has no text to find
  // and `trimStart` would eat its whole span, answering with the far end of the
  // padding — which is the position this function exists to rescue a caret
  // from. {@link alignedTable} writes one space of gutter after every pipe, so
  // that is where an empty cell's first character will go.
  const start = text === "" ? 1 : padded.length - padded.trimStart().length;
  return target.from + start + within;
}

/** One table's realign. */
interface Realign {
  /** The line-by-line changes that bring the table back into alignment. */
  changes: TextSplice[];
  /**
   * Where the caret belongs afterwards, as an offset within its own aligned
   * line plus that line's start as the document reads NOW. The document
   * position depends on every other line's change as well, so
   * {@link realignTables} finishes the arithmetic once it holds them all.
   *
   * Null when there is no single caret, when it is outside this table, or when
   * the line it stands in is not one this realign rewrites — nothing moved
   * under it, so there is nothing to correct and CodeMirror's own mapping is
   * left to do its job.
   */
  caret: { lineFrom: number; offset: number } | null;
}

/** What one table's realign changes, and where that leaves `cursor`. */
function realignOf(doc: Text, hit: TableHit, cursor: number | null): Realign {
  const aligned = tableSource(hit.indent, hit.rows, hit.aligns);
  if (aligned === hit.text) {
    return { changes: [], caret: null };
  }
  const changes: TextSplice[] = [];
  let caret: Realign["caret"] = null;
  const lines = aligned.split("\n");
  for (let index = 0; index < lines.length; index += 1) {
    const line = doc.line(hit.firstLine + index);
    // Line by line, and minimally within the line, because a whole-block
    // replacement would collapse the caret to the block's edge on every
    // keystroke: the padding of the rows ABOVE the one being typed in changes
    // too, so the single minimal splice over the whole block starts before the
    // caret and swallows it.
    const splice = spliceBetween(line.text, lines[index]);
    if (splice === null) {
      continue;
    }
    changes.push({
      from: line.from + splice.from,
      to: line.from + splice.to,
      insert: splice.insert,
    });
    if (cursor !== null && cursor >= line.from && cursor <= line.to) {
      const offset = realignedCaret(line.text, lines[index], cursor - line.from);
      caret = offset === null ? null : { lineFrom: line.from, offset };
    }
  }
  return { changes, caret };
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
 *
 * The caret is carried explicitly rather than mapped through the padding — see
 * {@link realignedCaret} for the owner's `| a    la |`, which is what mapping
 * it produced.
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
  const { doc, selection } = transaction.state;
  // One caret, or none. A range selection and a multi-cursor edit are left to
  // CodeMirror's mapping: "the offset within the cell's text" is a rule about
  // one point, and inventing an answer for the other end of a selection would
  // be a second rule nobody asked for.
  const cursor = selection.ranges.length === 1 && selection.main.empty ? selection.main.head : null;
  const changes: TextSplice[] = [];
  let placed: Realign["caret"] = null;
  for (const hit of tableHits(doc)) {
    const edited = touched.some((range) => range.from <= hit.to && range.to >= hit.from);
    if (edited) {
      const realign = realignOf(doc, hit, cursor);
      changes.push(...realign.changes);
      placed ??= realign.caret;
    }
  }
  if (changes.length === 0) {
    return transaction;
  }
  if (placed === null) {
    return [transaction, { changes, sequential: true }];
  }
  const caret = placed;
  // Every change ABOVE the caret's line moves that line; the offset within the
  // line already accounts for the line's own. `sequential` is what makes this
  // readable at all: the appended spec's selection is taken in the document its
  // own changes produce, unmapped, and that is the only coordinate space in
  // which the corrected position exists.
  const shift = changes.reduce(
    (total, change) =>
      change.to <= caret.lineFrom
        ? total + change.insert.length - (change.to - change.from)
        : total,
    0,
  );
  return [
    transaction,
    {
      changes,
      sequential: true,
      selection: EditorSelection.cursor(caret.lineFrom + shift + caret.offset),
    },
  ];
});

const tableTheme = EditorView.baseTheme({
  [`.${TABLE_BLOCK_CLASS}`]: {
    // `1em` above and below, not `0.4em`. A table is a block somebody stops at,
    // and at 0.4em it sat as close to the paragraph above it as two lines of
    // that paragraph sat to each other — so the eye read it as part of the
    // sentence rather than as a thing of its own. The cell padding inside is
    // 0.15em, which is why the outside has to do the separating.
    margin: "1em 0",
    // The block never exceeds the pane, whatever is inside it. Belt to the
    // scroll box's braces: a block widget's parent is the wrapped content box,
    // so this is already its width — and it stops being so the day this widget
    // is mounted anywhere else.
    maxWidth: "100%",
  },
  [`.${TABLE_SCROLL_CLASS}`]: {
    // Size containment in the inline axis, and the whole fix turns on it.
    //
    // `.cm-content` is a flex item of `.cm-scroller` sized by its own contents,
    // and `EditorView.lineWrapping` adds `flex-shrink: 1` so wrapped prose lets
    // it fall back to the scroller's width. A `max-content` table inside it has a
    // MINIMUM width of its own full width, so the content box could not shrink
    // past it: measured in Chromium, `.cm-content` grew from 320px to the
    // table's 914px, every line of prose in the note re-laid out to 914px, and
    // both `scrollWidth` and `clientWidth` on this box read 914 — a scroll box as
    // wide as the thing it was supposed to be scrolling.
    //
    // `contain: inline-size` says this box's width comes from its parent and
    // never from its contents. The content box stays at the pane's width, and the
    // overflow lands here, where there is a bar for it. `max-width: 100%` alone
    // cannot do it: a percentage is ignored while intrinsic widths are computed,
    // which is exactly when the damage was done.
    contain: "inline-size",
    maxWidth: "100%",
    // `auto`, so the bar exists when the table's own columns are wider than the
    // pane and is absent when they are not. `scroll` would put a permanent grey
    // strip under every two-column table in the note, which is a different
    // defect with the same cause.
    overflowX: "auto",
  },
  [`.${TABLE_BLOCK_CLASS} table`]: {
    borderCollapse: "collapse",
    // `max-content`, and `auto` — which is what stood here — is what the owner
    // photographed. Measured in Chromium rather than reasoned about, because the
    // reasoning is wrong: `auto` looks like "fit if you can, overflow if you
    // cannot", but `EditorView.lineWrapping` puts `overflow-wrap: anywhere` and
    // `word-break: break-word` on `.cm-content`, and a table cell inherits both.
    // So a cell's minimum width is ONE CHARACTER, "if you can" is always true,
    // and a seven-column table in a 320px pane shrank to seven 14px columns of
    // vertically stacked letters. `scrollWidth` and `clientWidth` were both 320:
    // there was nothing to scroll because nothing had overflowed, which is
    // exactly the report — adapted to the width, into something unreadable.
    //
    // `max-content` is the width the columns actually want. Wider than the pane
    // is then a real overflow, and the box above is where it goes. No
    // `max-width` on the table itself: capping it would hand the cells back to
    // the character-stacking above, one level down.
    width: "max-content",
  },
  [`.${TABLE_CELL_CLASS}`]: {
    border: "1px solid currentColor",
    borderColor: "color-mix(in srgb, currentColor 25%, transparent)",
    // Room to read a value in. 0.15em vertical was one hairline of space above
    // the text and one below it, which is legible and cramped — a table is
    // scanned down a column, and a row that touches its neighbours is a row the
    // eye has to separate for itself.
    padding: "0.35em 0.6em",
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
  // # Why this cluster is styled here and not in Tailwind (Story 48.9)
  //
  // These four are `document.createElement("button")` inside a CodeMirror
  // `WidgetType`. A widget's DOM is built by hand and handed to CodeMirror —
  // there is no JSX here, so `<Button size="icon-sm" variant="ghost">` is not
  // reachable, and a Tailwind class written on the element would be a class
  // this project's build never sees in a scanned file. Until now the
  // consequence was visible: transparent background, a 1px currentColor border
  // and inherited type, which is what a browser default button looks like
  // sitting next to shadcn ones, and it is what the owner photographed.
  //
  // So the design system is restated here DELIBERATELY, value for value,
  // against the same tokens `button.tsx` spends: 32px square (DESIGN.md's
  // load-bearing control height, which `size="icon-sm"` also draws),
  // `min(var(--radius-md), 10px)` radius, a transparent border that becomes
  // `--ring` on focus, `--muted` on hover, a 2px `--ring` shadow for the focus
  // indicator, and a 16px glyph. Every number is the one `buttonVariants`
  // produces; none is a fresh opinion. If `Button` changes, this has to change
  // with it, and that is the price of a control CodeMirror owns.
  //
  // The focus ring is written out rather than inherited because these are NOT
  // our `Button` — nothing here passes through `buttonVariants`, so the base
  // that serves 58 other call sites cannot reach them.
  ".cm-md-table-controls": {
    display: "flex",
    // 4px, DESIGN.md's spacing unit. Icon-only controls in a row need a seam
    // an eye can find: with the words gone, the gap is the only thing saying
    // these are four controls rather than one strip.
    gap: "4px",
    marginTop: "0.25em",
  },
  [`.${TABLE_CONTROL_CLASS}`]: {
    alignItems: "center",
    background: "transparent",
    border: "1px solid transparent",
    borderRadius: "min(var(--radius-md), 10px)",
    color: "inherit",
    cursor: "pointer",
    display: "inline-flex",
    flexShrink: "0",
    height: "32px",
    justifyContent: "center",
    outline: "none",
    padding: "0",
    transition: "all 150ms",
    width: "32px",
  },
  [`.${TABLE_CONTROL_CLASS} svg`]: {
    height: "16px",
    pointerEvents: "none",
    width: "16px",
  },
  [`.${TABLE_CONTROL_CLASS}:hover:not(:disabled)`]: {
    background: "var(--muted)",
    color: "var(--foreground)",
  },
  [`.${TABLE_CONTROL_CLASS}:focus-visible`]: {
    borderColor: "var(--ring)",
    boxShadow: "0 0 0 2px var(--ring)",
  },
  [`.${TABLE_CONTROL_CLASS}:active:not(:disabled)`]: {
    transform: "translateY(1px)",
  },
  [`.${TABLE_CONTROL_CLASS}:disabled`]: {
    cursor: "not-allowed",
    // `Button`'s own disabled opacity. It dims the whole control — glyph and
    // border together — so it is not the text-colour opacity DESIGN.md bans.
    opacity: "0.5",
    pointerEvents: "none",
  },
  // # The conversion chrome
  //
  // Two elements, both `<span>`: each is appended to a BLOCK widget (the
  // rendered table) and to an INLINE one (the control on a revealed embed
  // line), and a `<p>` inside a line of text is a paragraph nested in a line.
  [`.${TABLE_NOTICE_CLASS}`]: {
    // `--destructive` against the surfaces this text actually lands on, both
    // measured rather than chosen: 5.9:1 on `--background` and 5.5:1 on it in
    // the dark theme, 5.5:1 and 5.0:1 on `--card` inside the question below.
    // DESIGN.md's floor is 4.5:1 and none of the four is near it.
    color: "var(--destructive)",
    display: "block",
    marginTop: "0.25em",
  },
  [`.${TABLE_ASK_CLASS}`]: {
    alignItems: "center",
    background: "var(--card)",
    border: "1px solid var(--border)",
    borderRadius: "min(var(--radius-md), 10px)",
    display: "flex",
    // The question is a sentence naming a path, so it wraps rather than
    // pushing its own answers off the pane.
    flexWrap: "wrap",
    gap: "8px",
    marginTop: "0.25em",
    padding: "0.5em 0.6em",
  },
  [`.${TABLE_ASK_CLASS} button`]: {
    background: "transparent",
    border: "1px solid var(--border)",
    borderRadius: "min(var(--radius-md), 10px)",
    color: "inherit",
    cursor: "pointer",
    // The note's own type: these two answers are read, so they are set in the
    // text the reader is already reading.
    font: "inherit",
    // DESIGN.md's load-bearing control height, the one the four glyphs use.
    minHeight: "32px",
    outline: "none",
    padding: "0 0.6em",
  },
  [`.${TABLE_ASK_CLASS} button:hover`]: {
    background: "var(--muted)",
  },
  [`.${TABLE_ASK_CLASS} button:focus-visible`]: {
    borderColor: "var(--ring)",
    boxShadow: "0 0 0 2px var(--ring)",
  },
  [`.${TABLE_REPLACE_CLASS}`]: {
    // The destructive answer says so in a second channel as well as in its
    // words, and the words are what a colour-blind reader has: "Replace the
    // file" is unambiguous with no colour at all.
    color: "var(--destructive)",
  },
});

/**
 * The table layer: the rendered block, the structure controls, the realign, and
 * the two conversions.
 *
 * Composed into {@link livePreview}'s extension array beside `galleryLayer`, so
 * a note still has one renderer. The scan is doc-driven and the reveal is
 * selection-driven, and they are separated: moving the caret rebuilds the
 * decoration set from the tables already found, and only an edit re-scans.
 *
 * `options` is optional so that a host with no vault still gets a rendered,
 * aligned, editable table — it gets the conversions disabled and labelled,
 * which is a different thing from not getting them.
 */
export function tableLayer(options: TableLayerOptions = {}): Extension {
  return [
    StateField.define<{ hits: TableHit[]; decorations: DecorationSet }>({
      create(state) {
        const hits = tableHits(state.doc);
        return { hits, decorations: tableDecorations(hits, state, options) };
      },
      update(value, transaction) {
        if (!transaction.docChanged && transaction.selection === undefined) {
          return value;
        }
        const hits = transaction.docChanged ? tableHits(transaction.state.doc) : value.hits;
        return { hits, decorations: tableDecorations(hits, transaction.state, options) };
      },
      provide: (field) => EditorView.decorations.from(field, (value) => value.decorations),
    }),
    realignTables,
    tableTheme,
  ];
}
