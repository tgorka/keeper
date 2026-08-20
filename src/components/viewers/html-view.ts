/**
 * HTML as a page rather than as angle brackets, and its text as something you
 * can change (Story 55.5, AD-88).
 *
 * # Nothing here is rendered from a string
 *
 * This repo's rule is `textContent`, never `innerHTML`, and six modules say so
 * in a comment. Rendering a `.html` file could look like the one place to break
 * it. It is not, and the rule is kept exactly:
 *
 * - `DOMParser.parseFromString` produces an **inert** document — scripts do not
 *   run, images are not fetched, `<link>` pulls nothing. Parsing is not
 *   rendering, and the parser is the browser's own rather than one written here.
 * - The view is then built **node by node** with `createElement` and
 *   `textContent`, through {@link KEPT_ELEMENTS} and {@link KEPT_ATTRIBUTES}.
 *   An allowlist, so an element nobody has thought about yet is dropped rather
 *   than carried through.
 *
 * The result is that no markup string ever reaches a live parser attached to
 * the document, which is the property that rule exists to protect.
 *
 * # Nothing is fetched
 *
 * NFR-11: keeper never fetches a remote URL, and a document a person opens must
 * not be able to report that they opened it. So a remote `src` is not loaded —
 * the address is shown as text instead, which is the same answer
 * `live-preview.ts` already gives a remote image in a note, for the same reason
 * stated there: hiding the destination made a remote embed look like a word
 * somebody typed.
 *
 * # Editing text, and why an edit can be refused
 *
 * The rendered half edits the **text**, and it does it by splicing the source
 * bytes the text came from — not by reserialising the document. A file opened,
 * edited in one paragraph and saved has to differ by that paragraph and nothing
 * else; a round trip through a serialiser rewrites quoting, self-closing tags
 * and whitespace, and this file is one somebody syncs between machines.
 *
 * The mapping from a rendered node to its source range is by **order**: the
 * parser's Nth text node is the scanner's Nth text run. That is true for every
 * document this can be handed, and "true as far as anyone knows" is not a thing
 * to write bytes on. So {@link spliceText} checks that the bytes at the range
 * are still the bytes the node was built from, and refuses when they are not.
 * A mapping bug costs an edit; it must never cost a file.
 */

/** A run of text in the source, and where it came from. */
export interface TextRun {
  /** The raw source text, entities and all. */
  readonly raw: string;
  /** Byte offsets into the source, `[from, to)`. */
  readonly from: number;
  readonly to: number;
}

/**
 * Elements that survive, mapped to what they are drawn as.
 *
 * Everything absent is dropped and its children are kept — so an unknown
 * element loses its box and never its text, and a `<script>` loses both (see
 * {@link DROPPED_WHOLE}).
 */
const KEPT_ELEMENTS: Record<string, string> = {
  a: "a",
  abbr: "abbr",
  b: "b",
  blockquote: "blockquote",
  br: "br",
  code: "code",
  dd: "dd",
  del: "del",
  div: "div",
  dl: "dl",
  dt: "dt",
  em: "em",
  figcaption: "figcaption",
  figure: "figure",
  h1: "h1",
  h2: "h2",
  h3: "h3",
  h4: "h4",
  h5: "h5",
  h6: "h6",
  hr: "hr",
  i: "i",
  img: "img",
  li: "li",
  mark: "mark",
  ol: "ol",
  p: "p",
  pre: "pre",
  s: "s",
  small: "small",
  span: "span",
  strong: "strong",
  sub: "sub",
  sup: "sup",
  table: "table",
  tbody: "tbody",
  td: "td",
  tfoot: "tfoot",
  th: "th",
  thead: "thead",
  tr: "tr",
  u: "u",
  ul: "ul",
};

/**
 * Elements whose CONTENT goes too, rather than being unwrapped.
 *
 * The distinction matters: an unknown `<custom-thing>` holds text a reader
 * wants, while a `<script>` holds a program and a `<style>` holds rules that
 * would be styling this application's own DOM if they were kept.
 */
const DROPPED_WHOLE = new Set(["script", "style", "template", "iframe", "object", "embed"]);

/** Attributes kept, per element. Nothing global: `href` on a `<span>` means
 *  nothing, and a list per element is a list somebody has thought about. */
const KEPT_ATTRIBUTES: Record<string, readonly string[]> = {
  a: ["href", "title"],
  abbr: ["title"],
  img: ["alt", "title"],
  td: ["colspan", "rowspan"],
  th: ["colspan", "rowspan", "scope"],
};

/** A URL this app is willing to put in front of a reader as a link. Never a
 *  request: an anchor is not followed until somebody clicks it, and the app's
 *  own link handling decides what happens then. */
function safeHref(value: string): string | null {
  const trimmed = value.trim();
  if (/^(javascript|data|vbscript):/i.test(trimmed)) {
    return null;
  }
  return trimmed === "" ? null : trimmed;
}

/**
 * Every run of text in `source`, in document order, with its offsets.
 *
 * Deliberately a scanner and not a parser: what is needed is where the text
 * IS, and the browser has already been asked what the document MEANS. It skips
 * exactly what the parser skips — comments, doctypes, and the content of the
 * elements in {@link DROPPED_WHOLE} that hold raw text — so the two agree about
 * how many runs there are.
 */
export function scanTextRuns(source: string): TextRun[] {
  const runs: TextRun[] = [];
  let at = 0;
  while (at < source.length) {
    const open = source.indexOf("<", at);
    if (open === -1) {
      pushRun(runs, source, at, source.length);
      break;
    }
    pushRun(runs, source, at, open);

    if (source.startsWith("<!--", open)) {
      const close = source.indexOf("-->", open + 4);
      at = close === -1 ? source.length : close + 3;
      continue;
    }
    const tagEnd = source.indexOf(">", open);
    if (tagEnd === -1) {
      // An unterminated tag: everything after it is markup nobody can read as
      // text, so there is no run in it.
      break;
    }
    const name = /^<\/?\s*([a-zA-Z][^\s/>]*)/.exec(source.slice(open, tagEnd + 1))?.[1];
    const lowered = name?.toLowerCase();
    if (lowered !== undefined && DROPPED_WHOLE.has(lowered) && source[open + 1] !== "/") {
      // Raw-text content: the parser gives it no text node in the rendered
      // document, so the scanner must not give it a run either, or every run
      // after it would be off by one.
      const closing = source.toLowerCase().indexOf(`</${lowered}`, tagEnd);
      at = closing === -1 ? source.length : closing;
      continue;
    }
    at = tagEnd + 1;
  }
  return runs;
}

function pushRun(runs: TextRun[], source: string, from: number, to: number): void {
  if (to <= from) {
    return;
  }
  runs.push({ raw: source.slice(from, to), from, to });
}

export interface BuiltHtml {
  /** The fragment to mount. Built, never parsed from a string. */
  readonly node: DocumentFragment;
  /** One entry per editable text node, in the order they were created. */
  readonly runs: TextRun[];
}

/**
 * Parse `source` and build a safe view of it.
 *
 * The returned `runs` are aligned with the text nodes carrying
 * {@link TEXT_RUN_ATTR}, by construction: both are appended in the same walk.
 */
export function buildHtmlView(source: string): BuiltHtml {
  const parsed = new DOMParser().parseFromString(source, "text/html");
  const scanned = scanTextRuns(source);
  const fragment = document.createDocumentFragment();
  const runs: TextRun[] = [];
  let nextRun = 0;

  const walk = (from: Node, into: Node): void => {
    for (const child of Array.from(from.childNodes)) {
      if (child.nodeType === Node.TEXT_NODE) {
        const run = scanned[nextRun];
        nextRun += 1;
        const span = document.createElement("span");
        span.className = HTML_TEXT_CLASS;
        // `textContent`, and the text the PARSER produced — entities resolved,
        // which is what the reader is looking at and therefore what they are
        // editing.
        span.textContent = child.textContent ?? "";
        if (run !== undefined) {
          span.setAttribute(TEXT_RUN_ATTR, String(runs.length));
          runs.push(run);
        }
        into.appendChild(span);
        continue;
      }
      if (!(child instanceof Element)) {
        continue;
      }
      const tag = child.tagName.toLowerCase();
      if (DROPPED_WHOLE.has(tag)) {
        continue;
      }
      const kept = KEPT_ELEMENTS[tag];
      if (kept === undefined) {
        // Unwrapped, not dropped: an element nobody has thought about still
        // holds text somebody wrote.
        walk(child, into);
        continue;
      }
      const element = document.createElement(kept);
      for (const name of KEPT_ATTRIBUTES[tag] ?? []) {
        const value = child.getAttribute(name);
        if (value === null) {
          continue;
        }
        if (name === "href") {
          const href = safeHref(value);
          if (href !== null) {
            element.setAttribute("href", href);
          }
          continue;
        }
        element.setAttribute(name, value);
      }
      if (tag === "img") {
        // The address as text, never a request (NFR-11). An `<img>` with no
        // `src` is an empty box that tells the reader nothing, so what goes in
        // is what the file says the picture is.
        const shown = child.getAttribute("alt")?.trim();
        const address = child.getAttribute("src") ?? "";
        element.setAttribute("alt", shown === undefined || shown === "" ? address : shown);
        const caption = document.createElement("span");
        caption.className = HTML_ADDRESS_CLASS;
        caption.textContent = address === "" ? "image" : `image: ${address}`;
        into.appendChild(caption);
        continue;
      }
      into.appendChild(element);
      walk(child, element);
    }
  };

  walk(parsed.body, fragment);
  return { node: fragment, runs };
}

/** Marks a span the reader may type into, and indexes it into `runs`. */
export const TEXT_RUN_ATTR = "data-html-run";
/** The class every editable text span carries. */
export const HTML_TEXT_CLASS = "keeper-html-text";
/** The class a not-fetched address is shown under. */
export const HTML_ADDRESS_CLASS = "keeper-html-address";

/** What a refused splice says, so the caller shows a sentence rather than
 *  inventing one. */
export const SPLICE_REFUSAL =
  "keeper could not apply that edit: the file changed underneath this view. " +
  "Switch to Source, which is always the file's own bytes.";

export type SpliceResult = { ok: true; text: string } | { ok: false; reason: string };

/**
 * Replace one run's bytes in `source`, or refuse.
 *
 * The check is the whole point. The mapping from a rendered node to a source
 * range is by order, and order is a property of two things agreeing — so before
 * anything is written, the bytes at the range must still be the bytes the run
 * was built from. When they are not, the buffer moved under the view, and the
 * honest outcome is a sentence and an untouched file.
 */
export function spliceText(source: string, run: TextRun, replacement: string): SpliceResult {
  if (run.from < 0 || run.to > source.length || run.from > run.to) {
    return { ok: false, reason: SPLICE_REFUSAL };
  }
  if (source.slice(run.from, run.to) !== run.raw) {
    return { ok: false, reason: SPLICE_REFUSAL };
  }
  return { ok: true, text: source.slice(0, run.from) + replacement + source.slice(run.to) };
}
