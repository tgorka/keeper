/**
 * The formatting menu's commands (Story 44.9, FR-164, UX-DR60).
 *
 * The owner knows markdown and would rather not have to type it. That is a
 * request for a toolbar, and a toolbar is only as good as the guarantee that
 * pressing a button twice puts the note back — so every action here is a
 * CodeMirror `Command` over the current selection, and every one of them
 * toggles.
 *
 * **Why commands rather than string surgery on the buffer.** A command runs
 * inside a transaction, which is what makes it compose with everything the
 * editor already has: `history()` folds it into one undo step, 43.1's Tab
 * keymap can bind it later without a second implementation, multiple selection
 * ranges are handled by the same `changeByRange` the rest of CodeMirror uses,
 * and the update listener reports the edit to Rust exactly the way a typed
 * character does. Rewriting `doc.toString()` and calling `applyExternal` would
 * lose the undo step, lose the caret, and be annotated `remote` — which is to
 * say the note would change on screen and never reach the file.
 *
 * **Why the syntax tree decides whether a mark is already there.** The obvious
 * implementation looks at the characters either side of the selection, and it
 * is wrong in the case people hit first: with the caret inside `**bold**`,
 * "is the character before me a `*`?" is true for italic as well, so italic
 * would eat one star from each side and silently turn bold into italic. The
 * markdown parser is already in the editor's extension list and it has already
 * decided which run of stars is emphasis and which is strong emphasis, so this
 * module asks it instead of re-deriving a worse answer. That is also what makes
 * nesting work: inside `***both***` the parser reports an `Emphasis` wrapping a
 * `StrongEmphasis`, so bold removes the inner pair and italic the outer one.
 *
 * Nothing here is exported as a component or a class name. The AC is stated in
 * document text, and document text is what these functions produce.
 */
import { syntaxTree } from "@codemirror/language";
import { type ChangeSpec, EditorSelection, type EditorState } from "@codemirror/state";
import type { Command } from "@codemirror/view";
import type { SyntaxNode } from "@lezer/common";

/** What the toolbar asks for. Plain data on purpose: the toolbar is in the main
 *  bundle and every `@codemirror/*` value has to stay inside the editor's lazy
 *  chunk, so the two sides meet over a description of the action and never over
 *  a `Command`. */
export type FormatAction =
  | { kind: "bold" }
  | { kind: "italic" }
  | { kind: "strikethrough" }
  | { kind: "code" }
  | { kind: "bullet" }
  | { kind: "ordered" }
  | { kind: "quote" }
  | { kind: "link" }
  | { kind: "heading"; level: number }
  | ({ kind: "table" } & TableShape);

/** One inline mark, named the way the markdown parser names it. */
interface InlineMark {
  /** The delimiter written when the mark is absent. */
  token: string;
  /** The node the parser builds once the mark is there. */
  node: string;
  /** The node it gives that mark's own delimiters. */
  mark: string;
}

const BOLD: InlineMark = { token: "**", node: "StrongEmphasis", mark: "EmphasisMark" };
const ITALIC: InlineMark = { token: "*", node: "Emphasis", mark: "EmphasisMark" };
const STRIKE: InlineMark = { token: "~~", node: "Strikethrough", mark: "StrikethroughMark" };
const CODE: InlineMark = { token: "`", node: "InlineCode", mark: "CodeMark" };

/** What a fresh link's destination reads before the user types over it. It is
 *  selected after insertion, so this is a prompt rather than a value that could
 *  survive into the note by accident. */
const URL_PLACEHOLDER = "url";

/**
 * The innermost enclosing node of `name` that covers the whole selection.
 *
 * `resolveInner` is asked at the selection's start with a forward bias so that
 * a selection sitting exactly on `**bold**` starts inside the opening
 * delimiter and finds its parent, rather than landing on the paragraph and
 * concluding the mark is absent.
 */
function enclosing(state: EditorState, from: number, to: number, name: string): SyntaxNode | null {
  let node: SyntaxNode | null = syntaxTree(state).resolveInner(from, 1);
  while (node !== null) {
    if (node.name === name && node.from <= from && node.to >= to) {
      return node;
    }
    node = node.parent;
  }
  return null;
}

/**
 * The two changes that strip a mark's own delimiters and nothing else.
 *
 * Only direct children are considered, which is the whole point: the outer
 * `Emphasis` of `***both***` owns the single stars and the nested
 * `StrongEmphasis` owns the double ones, so each toggle removes its own pair
 * and leaves the other mark intact.
 */
function stripMarks(node: SyntaxNode, mark: string): ChangeSpec[] | null {
  let first: SyntaxNode | null = null;
  let last: SyntaxNode | null = null;
  for (let child = node.firstChild; child !== null; child = child.nextSibling) {
    if (child.name !== mark) {
      continue;
    }
    first ??= child;
    last = child;
  }
  if (first === null || last === null || first === last) {
    return null;
  }
  return [
    { from: first.from, to: first.to },
    { from: last.from, to: last.to },
  ];
}

/**
 * Toggle one inline mark over every selection range.
 *
 * The range returned for each case is the *same text* the user had selected,
 * which is what makes a second press a true undo of the first: after wrapping,
 * the selection is the content between the new delimiters, and after stripping
 * it is the content that is left. An empty selection writes the delimiter pair
 * and sits between them, because a person who pressed bold with no selection is
 * about to type the bold words.
 */
function toggleInline(mark: InlineMark): Command {
  return (view) => {
    view.dispatch(
      view.state.changeByRange((range) => {
        const node = enclosing(view.state, range.from, range.to, mark.node);
        const removal = node === null ? null : stripMarks(node, mark.mark);
        if (removal !== null) {
          const changes = view.state.changes(removal);
          return {
            changes,
            range: EditorSelection.range(
              changes.mapPos(range.from, 1),
              changes.mapPos(range.to, -1),
            ),
          };
        }
        if (range.empty) {
          return {
            changes: { from: range.from, insert: mark.token + mark.token },
            range: EditorSelection.cursor(range.from + mark.token.length),
          };
        }
        const changes = view.state.changes([
          { from: range.from, insert: mark.token },
          { from: range.to, insert: mark.token },
        ]);
        return {
          changes,
          range: EditorSelection.range(changes.mapPos(range.from, 1), changes.mapPos(range.to, -1)),
        };
      }),
    );
    view.focus();
    return true;
  };
}

/**
 * Toggle an inline link.
 *
 * Off is the interesting direction. A link's text is the run between its first
 * two `LinkMark`s, so unwrapping deletes everything outside that run and keeps
 * what the reader was actually reading — the destination goes, which is what
 * "remove the link" means. An `Image` is a different node and is left alone: an
 * embed is not a link the user asked to unwrap.
 */
const toggleLink: Command = (view) => {
  view.dispatch(
    view.state.changeByRange((range) => {
      const node = enclosing(view.state, range.from, range.to, "Link");
      const marks: SyntaxNode[] = [];
      for (let child = node?.firstChild ?? null; child !== null; child = child.nextSibling) {
        if (child.name === "LinkMark") {
          marks.push(child);
        }
      }
      if (node !== null && marks.length >= 2) {
        const changes = view.state.changes([
          { from: node.from, to: marks[0].to },
          { from: marks[1].from, to: node.to },
        ]);
        return {
          changes,
          range: EditorSelection.range(
            changes.mapPos(marks[0].to, 1),
            changes.mapPos(marks[1].from, -1),
          ),
        };
      }
      if (range.empty) {
        return {
          changes: { from: range.from, insert: "[]()" },
          range: EditorSelection.cursor(range.from + 1),
        };
      }
      // The destination is what the user has to supply, so it is what ends up
      // selected: typing replaces the placeholder without a second click.
      const tail = `](${URL_PLACEHOLDER})`;
      const changes = view.state.changes([
        { from: range.from, insert: "[" },
        { from: range.to, insert: tail },
      ]);
      const start = changes.mapPos(range.to, -1) + 2;
      return {
        changes,
        range: EditorSelection.range(start, start + URL_PLACEHOLDER.length),
      };
    }),
  );
  view.focus();
  return true;
};

/**
 * A line's leading markers, split so each block action edits only its own.
 *
 * Two groups rather than one because `> - a` is both a quote and a bullet, and
 * a toolbar that turned it into `- - a` because it treated the prefix as a
 * single token would be destroying structure the user can see.
 */
const PREFIX = /^([ \t]*)((?:> )*)((?:#{1,6} |[-*+] |\d+\. )?)/;

interface Prefix {
  /** Where the quote markers start, relative to the line. */
  quoteAt: number;
  quote: string;
  /** Where the list/heading marker starts, relative to the line. */
  markerAt: number;
  marker: string;
}

function prefixOf(text: string): Prefix {
  const match = PREFIX.exec(text);
  const indent = match?.[1] ?? "";
  const quote = match?.[2] ?? "";
  const marker = match?.[3] ?? "";
  return {
    quoteAt: indent.length,
    quote,
    markerAt: indent.length + quote.length,
    marker,
  };
}

interface Line {
  from: number;
  text: string;
}

/** Every line any selection range touches, once each, in document order. */
function touchedLines(state: EditorState): Line[] {
  const lines: Line[] = [];
  const seen = new Set<number>();
  for (const range of state.selection.ranges) {
    let pos = range.from;
    for (;;) {
      const line = state.doc.lineAt(pos);
      if (!seen.has(line.from)) {
        seen.add(line.from);
        lines.push({ from: line.from, text: line.text });
      }
      if (line.to >= range.to) {
        break;
      }
      pos = line.to + 1;
    }
  }
  return lines.sort((a, b) => a.from - b.from);
}

/**
 * Apply a block marker to the lines under the selection.
 *
 * Blank lines are skipped while there is anything else to act on, because a
 * bullet on the empty line between two paragraphs is a bullet the user did not
 * ask for and will have to delete. When *every* touched line is blank the
 * blanks are the target, which is the "press bullet on an empty line and start
 * typing" case.
 *
 * The change is a splice of the marker region alone, never a rewrite of the
 * line. That is what keeps the selection: replacing a whole line collapses any
 * caret inside it to the line's edge, so a partially selected paragraph would
 * lose its selection the moment it became a quote.
 */
function blockCommand(
  region: (prefix: Prefix) => { at: number; length: number; current: string },
  active: (current: string) => boolean,
  write: (index: number) => string,
): Command {
  return (view) => {
    const { state } = view;
    const all = touchedLines(state);
    const written = all.filter((line) => line.text.trim() !== "");
    const lines = written.length === 0 ? all : written;
    if (lines.length === 0) {
      return false;
    }
    const regions = lines.map((line) => ({ line, ...region(prefixOf(line.text)) }));
    const off = regions.every((each) => active(each.current));
    const changes = state.changes(
      regions.map((each, index) => ({
        from: each.line.from + each.at,
        to: each.line.from + each.at + each.length,
        insert: off ? "" : write(index),
      })),
    );
    // `SelectionRange.map` biases inward, which for a marker written at the
    // line's start means the selection slides past it and the first two
    // characters of the block the user just marked drop out of their own
    // selection. Biasing outward instead keeps whole lines whole, so pressing
    // the same button twice acts on the same block. An empty range is the one
    // exception: a caret must stay a caret and land after the marker, not grow
    // to select it.
    view.dispatch({
      changes,
      selection: EditorSelection.create(
        state.selection.ranges.map((range) =>
          range.empty
            ? EditorSelection.cursor(changes.mapPos(range.head, 1))
            : EditorSelection.range(changes.mapPos(range.from, -1), changes.mapPos(range.to, 1)),
        ),
        state.selection.mainIndex,
      ),
    });
    view.focus();
    return true;
  };
}

/** Which of the two prefix groups an action owns. Three block actions share
 *  the marker group and have to agree on it exactly, or heading would delete a
 *  bullet's indent along with its own token. */
const markerRegion = (prefix: Prefix) => ({
  at: prefix.markerAt,
  length: prefix.marker.length,
  current: prefix.marker,
});

const BULLET = /^[-*+] $/;
const ORDERED = /^\d+\. $/;

/** Rows the reader will count and columns they will fill in. */
export interface TableShape {
  /** Every row the table shows, the header row included when there is one. */
  rows: number;
  columns: number;
  /** Whether the first row names the columns. */
  header: boolean;
}

/**
 * The narrowest a column may be.
 *
 * GFM needs at least one dash in the delimiter row; three is what every
 * markdown tool writes, and it is also the point below which an empty cell
 * stops being visible as a cell in the source.
 */
const MIN_COLUMN = 3;

/**
 * An aligned GFM table.
 *
 * Aligned because this vault is read in Obsidian and in `git diff`, and a table
 * whose pipes do not line up in the source is a table nobody edits by hand
 * afterwards — they retype it, or they leave it wrong. Padding costs a few
 * spaces per row and every renderer ignores them.
 *
 * The delimiter row is not optional in GFM: it is what makes the block a table
 * at all, and the row above it is the header whether or not anyone asked for
 * one. So "no header" cannot mean "no header row" — it means an empty one, and
 * `rows` then counts only the rows the user will type into. With a header,
 * `rows` counts the header as the first of them, which is how the question was
 * asked: *is the first row a header?*
 */
export function gfmTable(shape: TableShape): string {
  const columns = Math.max(1, Math.floor(shape.columns));
  const rows = Math.max(1, Math.floor(shape.rows));
  const heading = Array.from({ length: columns }, (_, index) =>
    shape.header ? `Column ${index + 1}` : "",
  );
  const widths = heading.map((cell) => Math.max(MIN_COLUMN, cell.length));
  const row = (cells: readonly string[]) =>
    `| ${cells.map((cell, index) => cell.padEnd(widths[index])).join(" | ")} |`;
  const blank = Array.from({ length: columns }, () => "");
  const lines = [row(heading), `| ${widths.map((width) => "-".repeat(width)).join(" | ")} |`];
  for (let index = shape.header ? 1 : 0; index < rows; index += 1) {
    lines.push(row(blank));
  }
  return `${lines.join("\n")}\n`;
}

/**
 * Write a table where the caret is.
 *
 * A table has to own its lines, so a caret sitting mid-line gets a newline
 * first rather than a table welded to the end of a sentence. The caret lands in
 * the first cell — offset two is past `| ` — because the next thing anyone does
 * with a new table is name its first column.
 */
function insertTable(shape: TableShape): Command {
  return (view) => {
    const range = view.state.selection.main;
    const line = view.state.doc.lineAt(range.from);
    const lead = range.from === line.from ? "" : "\n";
    const insert = `${lead}${gfmTable(shape)}`;
    view.dispatch({
      changes: { from: range.from, to: range.to, insert },
      selection: { anchor: range.from + lead.length + 2 },
      scrollIntoView: true,
    });
    view.focus();
    return true;
  };
}

/** The one place an action description becomes an editor command. */
export function formatCommand(action: FormatAction): Command {
  switch (action.kind) {
    case "bold":
      return toggleInline(BOLD);
    case "italic":
      return toggleInline(ITALIC);
    case "strikethrough":
      return toggleInline(STRIKE);
    case "code":
      return toggleInline(CODE);
    case "link":
      return toggleLink;
    case "quote":
      return blockCommand(
        (prefix) => ({ at: prefix.quoteAt, length: prefix.quote.length, current: prefix.quote }),
        (current) => current !== "",
        () => "> ",
      );
    case "bullet":
      return blockCommand(
        markerRegion,
        (current) => BULLET.test(current),
        () => "- ",
      );
    case "ordered":
      return blockCommand(
        markerRegion,
        (current) => ORDERED.test(current),
        (index) => `${index + 1}. `,
      );
    case "heading": {
      const token = `${"#".repeat(Math.min(6, Math.max(1, action.level)))} `;
      return blockCommand(
        markerRegion,
        (current) => current === token,
        () => token,
      );
    }
    case "table":
      return insertTable(action);
  }
}
