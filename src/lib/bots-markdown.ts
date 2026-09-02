/**
 * The markdown subset an answer is allowed to be (Epic 61, Story 61.5).
 *
 * **Why a parser at all, and why this one.** Before this story a bot answer was
 * the characters the model sent in a `whitespace-pre-wrap` block, which is what
 * the Matrix timeline does with a message body. A model that answers with a
 * fenced code block, a numbered list and a table is then read as three
 * paragraphs of punctuation. The repository already ships `@lezer/markdown`
 * (it is what `@codemirror/lang-markdown` is built on, and the note editor's
 * `live-preview.ts` already walks a Lezer tree), so the answer is parsed with
 * the parser that is already here rather than with a new dependency.
 *
 * **This module produces data, never DOM and never HTML.** It hands back a
 * tree of plain objects; `bot-answer.tsx` turns those into React elements. That
 * split is what makes the streaming property testable without a renderer, and
 * it is what keeps the house rule mechanical rather than aspirational: there is
 * no HTML string anywhere in the pipeline, so there is nothing for
 * `dangerouslySetInnerHTML` to be tempted by. A raw HTML tag the model wrote is
 * a {@link MdBlock} of kind `literal` or a text run — the characters, shown.
 *
 * **Streaming is the whole difficulty.** This is called on every content delta,
 * so the same prefix must parse to the same blocks each time and a half-typed
 * construct must degrade to something readable:
 *
 * - An unterminated fence is a `code` block with `closed: false`. Lezer already
 *   runs the fence to the end of the document, so a code block that closes
 *   itself is the parser's own behaviour, made explicit and asserted.
 * - An unterminated emphasis marker is literal text: Lezer only emits
 *   `StrongEmphasis`/`Emphasis` when the closing run arrives, so `**bold` mid
 *   stream cannot retroactively bold the rest of the answer.
 * - Every block carries a position-derived {@link MdBlock.key} and its own raw
 *   {@link MdBlock.source}. The key is what stops React remounting settled
 *   blocks when a later one grows; the source is what lets the renderer's memo
 *   skip them entirely. Neither is derived from the block's content hash, and
 *   `bot-answer.test.tsx` asserts DOM node identity across deltas so that a
 *   later "tidy-up" cannot quietly reintroduce a remount per token.
 *
 * **Anything outside the subset renders as its own source text.** Never
 * dropped, never half-applied: a construct this module does not model becomes a
 * `literal` block or a text run holding exactly the characters the model sent.
 */
import type { SyntaxNode, Tree } from "@lezer/common";
import { parser as markdownParser, Strikethrough, Table } from "@lezer/markdown";

/**
 * The parser, configured once at module load.
 *
 * Two GFM extensions and no more: tables (cheap here, because the tree already
 * shapes them) and strikethrough. The rest of `GFM` is deliberately out —
 * `Autolink` would turn every bare URL in an answer into a link node, and task
 * lists would render a checkbox nothing can check, which is the dead affordance
 * AD-27 forbids. Without them, both render as the characters the model wrote.
 */
const answerParser = markdownParser.configure([Table, Strikethrough]);

/** A run of inline content inside one block. */
export type MdInline =
  | { kind: "text"; text: string }
  | { kind: "code"; text: string }
  | { kind: "emphasis"; children: MdInline[] }
  | { kind: "strong"; children: MdInline[] }
  | { kind: "strike"; children: MdInline[] }
  /** A link is content plus a URL, and the renderer shows both as text. */
  | { kind: "link"; children: MdInline[]; url: string };

/** One item of a list: a list item is a block container, so lists nest. */
export interface MdListItem {
  key: string;
  blocks: MdBlock[];
}

/** One block of an answer. `key` and `source` exist for the renderer. */
export type MdBlock = (
  | { kind: "paragraph"; children: MdInline[] }
  | { kind: "heading"; level: 1 | 2 | 3 | 4 | 5 | 6; children: MdInline[] }
  | {
      kind: "code";
      /** The fence's info string, or `null` where the model declared none. */
      language: string | null;
      text: string;
      /** `false` while a fence is still open — mid-stream, or truncated. */
      closed: boolean;
    }
  | { kind: "list"; ordered: boolean; start: number; items: MdListItem[] }
  | { kind: "quote"; blocks: MdBlock[] }
  | { kind: "rule" }
  | { kind: "table"; header: MdInline[][]; rows: MdInline[][][] }
  /** Outside the subset: shown as the characters the model sent. */
  | { kind: "literal" }
) & {
  /**
   * Position-derived: the block's index among its siblings, prefixed by its
   * container's key. Never content-derived — see the module note.
   */
  key: string;
  /** The block's own markdown, verbatim. The renderer memoises on it. */
  source: string;
};

/**
 * Node names that are syntax, not content.
 *
 * These are collected across the WHOLE tree rather than skipped child by child,
 * because a mark can sit inside another node's range without being its child: a
 * blockquote's `>` on a continuation line lands in the middle of the paragraph
 * it is quoting (`Paragraph[2,7]` over `"a\n> b"`, `QuoteMark[4,5]`). Slicing
 * text without subtracting them would print the quote and list markers back
 * into the prose.
 */
const MARK_NODES: Record<string, true> = {
  HeaderMark: true,
  EmphasisMark: true,
  StrikethroughMark: true,
  CodeMark: true,
  CodeInfo: true,
  QuoteMark: true,
  ListMark: true,
  LinkMark: true,
  LinkTitle: true,
  URL: true,
  TableDelimiter: true,
};

/** Half-open ranges of syntax to subtract from any text slice. */
type MarkRanges = ReadonlyArray<readonly [number, number]>;

function collectMarks(tree: Tree): MarkRanges {
  const marks: [number, number][] = [];
  tree.iterate({
    enter(node) {
      if (MARK_NODES[node.name] === true) {
        marks.push([node.from, node.to]);
      }
    },
  });
  return marks;
}

/**
 * `source[from, to)` with syntax removed.
 *
 * Marks are in document order and never overlap, so the scan starts at the
 * first mark that can touch this range instead of at the beginning. That is
 * not a micro-optimisation: a 200 kB answer holds thousands of marks and is
 * sliced thousands of times, and the linear form made the whole parse
 * quadratic — 1.9 s per delta, measured, against 30 ms for this one.
 */
function sliceText(source: string, from: number, to: number, marks: MarkRanges): string {
  let low = 0;
  let high = marks.length;
  while (low < high) {
    const mid = (low + high) >> 1;
    // `?? 0` is unreachable — `mid < high <= marks.length` — and is here only
    // because the index signature is `T | undefined` under this tsconfig.
    if ((marks[mid]?.[1] ?? 0) <= from) {
      low = mid + 1;
    } else {
      high = mid;
    }
  }
  let out = "";
  let pos = from;
  for (let index = low; index < marks.length; index += 1) {
    const mark = marks[index];
    if (mark === undefined || mark[0] >= to) {
      break;
    }
    if (mark[0] > pos) {
      out += source.slice(pos, mark[0]);
    }
    pos = Math.max(pos, mark[1]);
  }
  if (pos < to) {
    out += source.slice(pos, to);
  }
  return out;
}

/**
 * Append `text` to `out`, merging with a trailing text run.
 *
 * Merging is not tidiness: a mark can be a SIBLING that sits inside another
 * node's range (a blockquote's `>` on a continuation line is a child of the
 * quote, positioned inside the paragraph it interrupts), so one line of prose
 * arrives here in two pieces with the marker's indentation stranded at the
 * start of the second. The continuation-indent trim therefore runs on the
 * merged text rather than per slice — a newline plus the spaces or tabs that
 * follow it collapse to the newline. Markdown already ignores that
 * indentation; keeping it would print a ragged left edge into prose the
 * renderer shows with `whitespace-pre-wrap`. Indentation INSIDE a code block
 * never reaches here: code text is assembled from `CodeText` nodes.
 */
function pushText(out: MdInline[], text: string): void {
  if (text === "") {
    return;
  }
  const last = out[out.length - 1];
  if (last !== undefined && last.kind === "text") {
    out[out.length - 1] = { kind: "text", text: (last.text + text).replace(/\n[ \t]+/g, "\n") };
    return;
  }
  out.push({ kind: "text", text: text.replace(/\n[ \t]+/g, "\n") });
}

/**
 * The inline content of `node`, as runs.
 *
 * The walk is over direct children with the gaps between them filled in as
 * text, so any node this function does not name — an inline HTML tag, an image,
 * a footnote reference — falls through to its own source characters.
 */
function inlineRuns(node: SyntaxNode, source: string, marks: MarkRanges): MdInline[] {
  const out: MdInline[] = [];
  let pos = node.from;
  for (let child = node.firstChild; child !== null; child = child.nextSibling) {
    if (child.from > pos) {
      pushText(out, sliceText(source, pos, child.from, marks));
    }
    pos = Math.max(pos, child.to);
    if (MARK_NODES[child.name] === true) {
      continue;
    }
    switch (child.name) {
      case "Emphasis":
        out.push({ kind: "emphasis", children: inlineRuns(child, source, marks) });
        break;
      case "StrongEmphasis":
        out.push({ kind: "strong", children: inlineRuns(child, source, marks) });
        break;
      case "Strikethrough":
        out.push({ kind: "strike", children: inlineRuns(child, source, marks) });
        break;
      case "InlineCode":
        out.push({ kind: "code", text: sliceText(source, child.from, child.to, marks) });
        break;
      case "Link": {
        const url = child.getChild("URL");
        out.push({
          kind: "link",
          children: inlineRuns(child, source, marks),
          url: url === null ? "" : source.slice(url.from, url.to),
        });
        break;
      }
      case "Escape":
        // `\*` is one character the model wanted literally, not two.
        pushText(out, source.slice(child.from + 1, child.to));
        break;
      default:
        // Entities included: `&amp;` stays `&amp;`. Decoding an entity is the
        // first half of rendering HTML, and this pipeline does not have a
        // second half.
        pushText(out, sliceText(source, child.from, child.to, marks));
        break;
    }
  }
  if (pos < node.to) {
    pushText(out, sliceText(source, pos, node.to, marks));
  }
  return out;
}

/** The cells of one `TableHeader`/`TableRow`. */
function tableCells(row: SyntaxNode, source: string, marks: MarkRanges): MdInline[][] {
  const cells: MdInline[][] = [];
  for (let cell = row.firstChild; cell !== null; cell = cell.nextSibling) {
    if (cell.name === "TableCell") {
      cells.push(inlineRuns(cell, source, marks));
    }
  }
  return cells;
}

/** A fenced or indented code block's text: every `CodeText` run, joined.
 *  The gaps between runs are the block's indentation and, inside a quote, its
 *  `>` markers — none of which are the model's code. */
function codeText(node: SyntaxNode, source: string): string {
  let text = "";
  for (let child = node.firstChild; child !== null; child = child.nextSibling) {
    if (child.name === "CodeText") {
      text += source.slice(child.from, child.to);
    }
  }
  return text;
}

/** How many `CodeMark` runs a fence has: one while it is still open. */
function fenceIsClosed(node: SyntaxNode): boolean {
  let marks = 0;
  for (let child = node.firstChild; child !== null; child = child.nextSibling) {
    if (child.name === "CodeMark") {
      marks += 1;
    }
  }
  return marks >= 2;
}

const ATX_HEADING = /^ATXHeading([1-6])$/;
const SETEXT_HEADING = /^SetextHeading([12])$/;
/** The number an ordered list starts at, off its first item's mark. */
const ORDERED_START = /^(\d+)/;

function headingLevel(name: string): 1 | 2 | 3 | 4 | 5 | 6 | null {
  const atx = ATX_HEADING.exec(name) ?? SETEXT_HEADING.exec(name);
  if (atx === null) {
    return null;
  }
  return Number(atx[1]) as 1 | 2 | 3 | 4 | 5 | 6;
}

/**
 * The blocks directly inside `container`, keyed by position under `prefix`.
 *
 * Marks are skipped rather than rendered, so a blockquote's `>` and a list
 * item's `-` do not become blocks of their own.
 */
function blocksIn(
  container: SyntaxNode,
  source: string,
  marks: MarkRanges,
  prefix: string,
): MdBlock[] {
  const blocks: MdBlock[] = [];
  for (let child = container.firstChild; child !== null; child = child.nextSibling) {
    if (MARK_NODES[child.name] === true) {
      continue;
    }
    const key = prefix === "" ? String(blocks.length) : `${prefix}-${blocks.length}`;
    blocks.push(toBlock(child, source, marks, key));
  }
  return blocks;
}

/** Drop the whitespace a removed mark left at the ends of a run list. Only
 *  the outer text runs can carry it, so nothing recurses. */
function trimEdges(runs: MdInline[]): MdInline[] {
  const out = [...runs];
  const first = out[0];
  if (first !== undefined && first.kind === "text") {
    out[0] = { kind: "text", text: first.text.replace(/^[ \t]+/, "") };
  }
  const last = out[out.length - 1];
  if (last !== undefined && last.kind === "text") {
    out[out.length - 1] = { kind: "text", text: last.text.replace(/[ \t]+$/, "") };
  }
  return out.filter((run) => run.kind !== "text" || run.text !== "");
}

function toBlock(node: SyntaxNode, source: string, marks: MarkRanges, key: string): MdBlock {
  const common = { key, source: source.slice(node.from, node.to) };
  const level = headingLevel(node.name);
  // A heading's marks are gone by now, which leaves the space that separated
  // `##` from its text (and, for a closed ATX heading, the one before the
  // trailing `##`) at the edges of the runs.
  if (level !== null) {
    return {
      ...common,
      kind: "heading",
      level,
      children: trimEdges(inlineRuns(node, source, marks)),
    };
  }
  switch (node.name) {
    case "Paragraph":
      return { ...common, kind: "paragraph", children: inlineRuns(node, source, marks) };
    case "FencedCode": {
      const info = node.getChild("CodeInfo");
      const language = info === null ? null : source.slice(info.from, info.to).trim();
      return {
        ...common,
        kind: "code",
        language: language === null || language === "" ? null : language,
        text: codeText(node, source),
        closed: fenceIsClosed(node),
      };
    }
    case "CodeBlock":
      // Indented code has no fence to leave open and no info string to read.
      return {
        ...common,
        kind: "code",
        language: null,
        text: codeText(node, source),
        closed: true,
      };
    case "BulletList":
    case "OrderedList": {
      const items: MdListItem[] = [];
      let start = 1;
      for (let item = node.firstChild; item !== null; item = item.nextSibling) {
        if (item.name !== "ListItem") {
          continue;
        }
        const itemKey = `${key}i${items.length}`;
        if (items.length === 0) {
          const mark = item.getChild("ListMark");
          const digits =
            mark === null ? null : ORDERED_START.exec(source.slice(mark.from, mark.to));
          if (digits !== null) {
            start = Number(digits[1]);
          }
        }
        items.push({ key: itemKey, blocks: blocksIn(item, source, marks, itemKey) });
      }
      return { ...common, kind: "list", ordered: node.name === "OrderedList", start, items };
    }
    case "Blockquote":
      return { ...common, kind: "quote", blocks: blocksIn(node, source, marks, key) };
    case "HorizontalRule":
      return { ...common, kind: "rule" };
    case "Table": {
      const header = node.getChild("TableHeader");
      const rows: MdInline[][][] = [];
      for (let row = node.firstChild; row !== null; row = row.nextSibling) {
        if (row.name === "TableRow") {
          rows.push(tableCells(row, source, marks));
        }
      }
      return {
        ...common,
        kind: "table",
        header: header === null ? [] : tableCells(header, source, marks),
        rows,
      };
    }
    default:
      // HTML blocks, link reference definitions, comment blocks, and anything a
      // later Lezer version starts emitting: the characters, shown.
      return { ...common, kind: "literal" };
  }
}

/**
 * Parse one answer — a whole one, or the prefix that has arrived so far.
 *
 * Pure and total: every character of `source` is reachable in the result, and
 * no input makes it throw.
 */
export function parseAnswer(source: string): MdBlock[] {
  if (source === "") {
    return [];
  }
  const tree = answerParser.parse(source);
  return blocksIn(tree.topNode, source, collectMarks(tree), "");
}

/** What a code block with no info string is labelled. Unknown is unknown, and
 *  "Plain text" is what keeper knows: the model declared no language. */
export const BOT_CODE_PLAIN_LABEL = "Plain text";

/** The copy verb on a code block, and what it becomes once the text is on the
 *  clipboard — the recording row's own pair, so the app says one thing. */
export const BOT_CODE_COPY_LABEL = "Copy code";
export const BOT_CODE_COPIED_LABEL = "Copied";
