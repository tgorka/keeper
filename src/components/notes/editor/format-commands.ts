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
  | { kind: "mark" }
  | { kind: "code" }
  | { kind: "subscript" }
  | { kind: "superscript" }
  | { kind: "underline" }
  | { kind: "bullet" }
  | { kind: "ordered" }
  | { kind: "task" }
  | { kind: "quote" }
  | { kind: "link" }
  | { kind: "codeblock" }
  | { kind: "heading"; level: number }
  | ({ kind: "table" } & TableShape)
  | { kind: "emoji"; text: string };

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
// `markdown-marks.ts` defines these two nodes; without that extension loaded
// this toggle would write `==` that nothing ever parses back off again.
const MARK: InlineMark = { token: "==", node: "Highlight", mark: "HighlightMark" };

/**
 * Subscript and superscript, in the spelling the parser already in this editor
 * understands.
 *
 * `markdownLanguage` — the base `note-editor.tsx` loads — is CommonMark + GFM
 * *plus* the Subscript and Superscript extensions, so `H~2~O` and `x^2^`
 * already arrive as `Subscript` and `Superscript` nodes with their own mark
 * children. Nothing here teaches the editor a new syntax; it names one it was
 * already parsing and nobody had rendered or written.
 *
 * **What Obsidian does with these bytes.** Obsidian is CommonMark + GFM without
 * those two extensions, so `H~2~O` and `x^2^` render there as themselves —
 * literal, legible, unambiguous, and losslessly round-tripped back into keeper.
 * That is the whole test the epic sets: a note must not get *worse* to read
 * outside keeper. `<sub>`/`<sup>` would render correctly in Obsidian and as raw
 * angle brackets here (this editor renders no HTML), which fails the same test
 * from the other side, and would put a second HTML spelling in the vault beside
 * the one underline needs.
 */
const SUBSCRIPT: InlineMark = { token: "~", node: "Subscript", mark: "SubscriptMark" };
const SUPERSCRIPT: InlineMark = { token: "^", node: "Superscript", mark: "SuperscriptMark" };

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
 * Underline's two delimiters.
 *
 * **Every markdown spelling of underline is a compromise; this is the one that
 * does not lie.** `__text__` is *bold* in CommonMark and in Obsidian, and this
 * editor's own parser agrees — it reports `StrongEmphasis` for it, so keeper
 * could not tell an underline from a bold even if it wanted to. `==text==` is
 * *highlight* in Obsidian, a different mark with a different meaning. `_text_`
 * is italic. Each of those makes the same file say two different things
 * depending on who opens it, which is exactly the failure the epic told this
 * story to reject.
 *
 * `<u>` means underline and nothing else, in Obsidian, on GitHub, in Pandoc and
 * in every browser. Obsidian renders it as a real underline; keeper renders it
 * as one too (`live-preview.ts` hides these two tags and marks the run between
 * them, the same treatment `**` gets).
 *
 * **This is not an HTML sink.** `live-preview.ts` recognises the two literal
 * strings below as delimiters and paints a CSS class over the text between
 * them. Nothing is ever parsed as HTML and nothing reaches `innerHTML`, so the
 * module's standing refusal to render HTML in a note body is intact: every
 * other tag a note contains is still inert text.
 */
const UNDERLINE_OPEN = "<u>";
const UNDERLINE_CLOSE = "</u>";

/**
 * The top-level block `pos` sits in, or the whole document when it sits in none.
 *
 * Underline pairing is bounded to one block because an unclosed `<u>` three
 * paragraphs up must not pair with the `</u>` under the caret and delete a tag
 * the user cannot see.
 *
 * **Found by walking the root's children, not by resolving upward from `pos`.**
 * `markdownLanguage` mixes in the HTML parser, so resolving inside an
 * `HTMLTag` descends into a nested tree that has a `Document` node of its own —
 * three characters wide. Climbing until "the parent is a Document" therefore
 * stopped at the opening tag itself, the search range became `<u>`, and the
 * closing tag was never in it: a selection covering `<u>word</u>` got wrapped a
 * second time instead of unwrapped.
 */
function topBlock(state: EditorState, pos: number): { from: number; to: number } {
  for (
    let child = syntaxTree(state).topNode.firstChild;
    child !== null;
    child = child.nextSibling
  ) {
    if (child.from <= pos && child.to >= pos) {
      return { from: child.from, to: child.to };
    }
  }
  return { from: 0, to: state.doc.length };
}

/**
 * The `<u>` … `</u>` pair enclosing `from`..`to`, or null.
 *
 * Found through the syntax tree rather than by scanning characters, for the
 * reason the module header gives about the other marks: the parser has already
 * decided which angle brackets are a tag, so a `<u>` written inside `` `code` ``
 * or inside a fence is not one of these nodes and cannot be paired with.
 * Nesting is handled by the stack — the innermost pair that still matches the
 * selection wins, so a second press undoes the press before it.
 *
 * A pair matches in both of the directions `toggleInline` accepts: the
 * selection sitting *inside* the run (a caret in the middle of it), and the
 * selection *containing* the pair (a user who dragged over the tags as well as
 * the word). The second is not a nicety — without it, selecting exactly what
 * the first press produced and pressing again wraps it a second time, which is
 * the opposite of a toggle.
 */
function underlinePair(
  state: EditorState,
  from: number,
  to: number,
): { open: SyntaxNode; close: SyntaxNode } | null {
  const block = topBlock(state, from);
  const open: SyntaxNode[] = [];
  let found: { open: SyntaxNode; close: SyntaxNode } | null = null;
  syntaxTree(state).iterate({
    from: block.from,
    to: block.to,
    enter: (node) => {
      if (found !== null || node.name !== "HTMLTag") {
        return undefined;
      }
      const text = state.doc.sliceString(node.from, node.to);
      if (text === UNDERLINE_OPEN) {
        open.push(node.node);
      } else if (text === UNDERLINE_CLOSE) {
        const start = open.pop();
        const encloses = start !== undefined && start.to <= from && node.from >= to;
        const enclosed = start !== undefined && start.from >= from && node.to <= to;
        if (start !== undefined && (encloses || enclosed)) {
          found = { open: start, close: node.node };
        }
      }
      return undefined;
    },
  });
  return found;
}

/**
 * Toggle underline, with the same selection contract as {@link toggleInline}:
 * whatever text was selected is still selected afterwards, so pressing the
 * button twice is a true undo of pressing it once.
 */
const toggleUnderline: Command = (view) => {
  view.dispatch(
    view.state.changeByRange((range) => {
      const pair = underlinePair(view.state, range.from, range.to);
      if (pair !== null) {
        const changes = view.state.changes([
          { from: pair.open.from, to: pair.open.to },
          { from: pair.close.from, to: pair.close.to },
        ]);
        return {
          changes,
          range: EditorSelection.range(changes.mapPos(range.from, 1), changes.mapPos(range.to, -1)),
        };
      }
      if (range.empty) {
        return {
          changes: { from: range.from, insert: UNDERLINE_OPEN + UNDERLINE_CLOSE },
          range: EditorSelection.cursor(range.from + UNDERLINE_OPEN.length),
        };
      }
      const changes = view.state.changes([
        { from: range.from, insert: UNDERLINE_OPEN },
        { from: range.to, insert: UNDERLINE_CLOSE },
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

/**
 * A line's leading markers, split so each block action edits only its own.
 *
 * Two groups rather than one because `> - a` is both a quote and a bullet, and
 * a toolbar that turned it into `- - a` because it treated the prefix as a
 * single token would be destroying structure the user can see.
 *
 * **The checkbox belongs to the marker group, not to the content.** A task is
 * `- ` plus `[ ] `, and if the marker stopped at the bullet then pressing the
 * task button on `- [ ] a` would write a second box and produce `- [ ] [ ] a`.
 * Owning both means every block action agrees about where the text starts:
 * pressing bullet on a task turns it back into a plain bullet, pressing heading
 * on one takes the whole marker with it, and neither leaves an orphan `[ ]`.
 */
const PREFIX = /^([ \t]*)((?:> )*)((?:#{1,6} |(?:[-*+]|\d+\.) (?:\[[ xX]\] )?)?)/;

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

/** Which of the two prefix groups an action owns. Four block actions share
 *  the marker group and have to agree on it exactly, or heading would delete a
 *  bullet's indent along with its own token. */
const markerRegion = (prefix: Prefix) => ({
  at: prefix.markerAt,
  length: prefix.marker.length,
  current: prefix.marker,
});

const BULLET = /^[-*+] $/;
const ORDERED = /^\d+\. $/;

/**
 * A task marker, whichever list it hangs off and whichever case the tick is in.
 *
 * Both cases, because `- [X] done` is what several other editors write and a
 * toolbar that did not recognise it would add a second box to a line that
 * already had one. GFM, Obsidian and this editor's parser all accept either.
 */
const TASK = /^(?:[-*+]|\d+\.) \[[ xX]\] $/;

/** The fence a code block is written with. Backticks rather than tildes because
 *  it is what every other tool writes, so a diff of this vault stays uniform. */
const FENCE = "```";

/**
 * Wrap the selection in a fenced code block, or unwrap the fence it is inside.
 *
 * A fence owns whole lines, so this works in lines rather than in characters —
 * which is also the thing that keeps it from being confused with the `code`
 * action beside it. `code` writes one backtick either side of a run *within* a
 * line and produces an `InlineCode` node; this writes three backticks on lines
 * of their own and produces a `FencedCode`. They are different nodes, they get
 * different decorations, and neither can turn into the other by accident.
 *
 * Unwrapping keeps the body and drops the fence lines. A fence the user has
 * opened but not yet closed has only one of them, and dropping "the closing
 * line" it does not have would eat whatever came after it — so the closer is
 * looked for rather than assumed, and its absence means the body runs to the
 * end of the block the parser found.
 */
const toggleCodeBlock: Command = (view) => {
  const { state } = view;
  const range = state.selection.main;
  const fence = enclosing(state, range.from, range.to, "FencedCode");
  const marks: SyntaxNode[] = [];
  for (let child = fence?.firstChild ?? null; child !== null; child = child.nextSibling) {
    if (child.name === "CodeMark") {
      marks.push(child);
    }
  }
  if (fence !== null && marks.length >= 1) {
    const open = state.doc.lineAt(marks[0].from);
    const close = marks.length >= 2 ? state.doc.lineAt(marks[marks.length - 1].from) : null;
    // Sliced rather than spliced around, because an empty fence has no body
    // and its two "delete the line break too" ranges would overlap.
    const bodyFrom = open.to + 1;
    const bodyTo = close === null ? fence.to : close.from - 1;
    const body = bodyTo > bodyFrom ? state.doc.sliceString(bodyFrom, bodyTo) : "";
    view.dispatch({
      changes: { from: open.from, to: close === null ? fence.to : close.to, insert: body },
      selection: EditorSelection.range(open.from, open.from + body.length),
    });
    view.focus();
    return true;
  }
  const first = state.doc.lineAt(range.from);
  const last = state.doc.lineAt(range.to);
  const body = state.doc.sliceString(first.from, last.to);
  const anchor = first.from + FENCE.length + 1;
  view.dispatch({
    changes: { from: first.from, to: last.to, insert: `${FENCE}\n${body}\n${FENCE}` },
    // The body stays selected, and an empty one leaves a caret on the empty
    // middle line — which is where the next keystroke belongs either way.
    selection: EditorSelection.range(anchor, anchor + body.length),
    scrollIntoView: true,
  });
  view.focus();
  return true;
};

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
 * Which way a column's cells are set, as its delimiter row spells it.
 *
 * Carried through {@link alignedTable} so that realigning a table somebody
 * wrote with `:---:` does not quietly re-centre their numbers to the left.
 */
export type TableAlign = "none" | "left" | "center" | "right";

/** One delimiter cell: the dashes, plus the colons that carry the column's
 *  alignment. `width` is never below {@link MIN_COLUMN}, so `:-:` is the
 *  narrowest a centred column can get and no form can run out of dashes. */
function delimiterCell(width: number, align: TableAlign): string {
  switch (align) {
    case "left":
      return `:${"-".repeat(width - 1)}`;
    case "right":
      return `${"-".repeat(width - 1)}:`;
    case "center":
      return `:${"-".repeat(width - 2)}:`;
    default:
      return "-".repeat(width);
  }
}

/**
 * Cells in, aligned GFM source out. **The one aligner in the app.**
 *
 * Story 45.9 realigns a table the user is typing in, and a second aligner
 * written there would disagree with this one about a cell holding an escaped
 * pipe — `a \| b` is one cell six characters wide to the writer of the table
 * and two cells to a second reader of it. Then the table the `/` menu inserts
 * and the table the editor maintains are different tables, and the difference
 * only shows up in somebody's diff. So the padding lives here, once, and
 * {@link gfmTable} is one caller of it.
 *
 * `rows[0]` is the header, and the header decides how many columns the table
 * has: GFM only recognises a table when the delimiter row matches the header
 * cell for cell, so those two are always emitted at the same width. A body row
 * with fewer cells is filled out to that width (GFM inserts the empty cells
 * anyway; writing them makes the source legible). A body row with *more* keeps
 * every one of them — a renderer ignores the excess, and dropping a cell here
 * would delete text the user can see in their file.
 */
export function alignedTable(
  rows: readonly (readonly string[])[],
  aligns: readonly TableAlign[] = [],
): string {
  const columns = Math.max(1, rows[0]?.length ?? 0);
  const widest = rows.reduce((most, row) => Math.max(most, row.length), columns);
  const widths = Array.from({ length: widest }, (_, index) =>
    rows.reduce((width, row) => Math.max(width, (row[index] ?? "").length), MIN_COLUMN),
  );
  const line = (cells: readonly string[], count: number): string => {
    const padded = Array.from({ length: count }, (_, index) =>
      (cells[index] ?? "").padEnd(widths[index]),
    );
    return `| ${padded.join(" | ")} |`;
  };
  const delimiter = Array.from({ length: columns }, (_, index) =>
    delimiterCell(widths[index], aligns[index] ?? "none"),
  );
  const source = [line(rows[0] ?? [], columns), `| ${delimiter.join(" | ")} |`];
  for (const row of rows.slice(1)) {
    source.push(line(row, Math.max(columns, row.length)));
  }
  return `${source.join("\n")}\n`;
}

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
  const blank = Array.from({ length: columns }, () => "");
  const body = Array.from({ length: rows - (shape.header ? 1 : 0) }, () => blank);
  return alignedTable([heading, ...body]);
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

/**
 * Put literal text where the caret is, replacing a selection if there is one.
 *
 * The emoji picker's insertion, and deliberately the *character* rather than
 * the `:shortcode:` — `emoji-complete.ts` resolves a typed shortcode to the
 * character on commit, so a picker that wrote the shortcode back would leave
 * the buffer holding two spellings of one emoji depending on which door it came
 * through.
 */
function insertAtCaret(text: string): Command {
  return (view) => {
    const range = view.state.selection.main;
    view.dispatch({
      changes: { from: range.from, to: range.to, insert: text },
      selection: { anchor: range.from + text.length },
      scrollIntoView: true,
      userEvent: "input.type",
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
    case "mark":
      return toggleInline(MARK);
    case "code":
      return toggleInline(CODE);
    case "subscript":
      return toggleInline(SUBSCRIPT);
    case "superscript":
      return toggleInline(SUPERSCRIPT);
    case "underline":
      return toggleUnderline;
    case "codeblock":
      return toggleCodeBlock;
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
    case "task":
      return blockCommand(
        markerRegion,
        (current) => TASK.test(current),
        () => "- [ ] ",
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
    case "emoji":
      return insertAtCaret(action.text);
  }
}
