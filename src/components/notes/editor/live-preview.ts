/**
 * The live-preview decoration layer (Story 37.6, UX-DR40).
 *
 * This file is the renderer. There is no second one, and adding one would be a
 * mistake: two rendering paths over the same markdown are two places for the
 * document you read and the document you edit to disagree. So the editor never
 * leaves edit mode — decorations hide the syntax and style what is left, and
 * the line under the caret drops its decorations so its source is right there.
 *
 * That reveal rule is also why nothing here needs to be atomic. A hidden range
 * is never on the caret's line, because putting the caret on a line un-hides
 * it, so the caret can never be trapped inside something it cannot see.
 *
 * What is deliberately absent: syntax highlighting inside code fences (not this
 * phase, and colour that implies highlighting would be a lie), any HTML
 * rendering beyond the two literal `<u>` tags underline is spelled with (raw
 * HTML in a note body stays text — there is no HTML sink to inject into, and
 * underline adds none: it hides two strings and paints a CSS class), and any
 * fetch of a remote image URL (a note must not be able to become a tracking
 * pixel).
 */
import { syntaxTree } from "@codemirror/language";
import { type Extension, type Range, StateEffect, StateField } from "@codemirror/state";
import {
  Decoration,
  type DecorationSet,
  EditorView,
  ViewPlugin,
  type ViewUpdate,
  WidgetType,
} from "@codemirror/view";
import type { SyntaxNode } from "@lezer/common";
import type { NoteGalleryVm } from "@/lib/ipc/client";
import { embedEntryFor, FileEmbedWidget } from "./file-embed";
import { galleryLayer } from "./gallery-block";
import { tableLayer } from "./markdown-table";
import { mermaidLayer } from "./mermaid-widget";
import { type NoteWidgetOptions, noteWidgetLayer } from "./note-widget";
import { RecordingEmbedWidget } from "./recording-embed";
import { transportFor } from "./recording-transport";
import { VaultEmbedWidget } from "./vault-embed";
import { LINK_ATTR, WIKILINK, WIKILINK_ATTR } from "./wikilink";

/** How long an externally applied change stays highlighted. */
export const EXTERNAL_FLASH_MS = 1_200;

/**
 * Syntax markers hidden on every line except the one being edited.
 *
 * Only the marks whose node makes them unambiguous live here. A link's
 * punctuation does not: the parser reuses `LinkMark`, `URL` and `LinkLabel`
 * across constructs where they mean opposite things, and hiding them by name
 * is what made a bare URL disappear from a note. Those three are decided by
 * their parent, further down.
 */
const HIDDEN_MARKS: Record<string, true> = {
  EmphasisMark: true,
  StrongMark: true,
  CodeMark: true,
  HeaderMark: true,
  QuoteMark: true,
  StrikethroughMark: true,
  SubscriptMark: true,
  SuperscriptMark: true,
  HighlightMark: true,
};

/** Inline nodes that keep their text and gain a class. */
const INLINE_CLASSES: Record<string, string> = {
  Emphasis: "cm-lp-em",
  StrongEmphasis: "cm-lp-strong",
  InlineCode: "cm-lp-code",
  Strikethrough: "cm-lp-strike",
  Subscript: "cm-lp-sub",
  Superscript: "cm-lp-sup",
  Highlight: "cm-lp-mark",
  // The language a fence declares. Visible, unlike the backticks around it:
  // hiding it left every code block opening with a blank grey line and threw
  // away the one piece of information the block carried about itself.
  CodeInfo: "cm-lp-fence-info",
};

/**
 * The nodes whose `LinkMark`, `URL` and `LinkLabel` children are punctuation.
 *
 * `Link` and `Image` own a destination the reader does not want to see.
 * `Autolink` owns two angle brackets around a URL that IS the text. A
 * `LinkReference` definition line owns neither — it is metadata the writer
 * typed and must be able to read back, so nothing in it is hidden.
 */
const LINK_PUNCTUATION: Record<string, true> = { Link: true, Image: true, Autolink: true };

/** Underline's two delimiters, in the spelling `format-commands.ts` writes.
 *  Matched as literal strings against `HTMLTag` nodes — nothing here parses or
 *  renders HTML, so every other tag in a note body stays inert text. */
const UNDERLINE_OPEN = "<u>";
const UNDERLINE_CLOSE = "</u>";

/** Block nodes that colour their whole line. */
const LINE_CLASSES: Record<string, string> = {
  ATXHeading1: "cm-lp-h1",
  ATXHeading2: "cm-lp-h2",
  ATXHeading3: "cm-lp-h3",
  ATXHeading4: "cm-lp-h4",
  ATXHeading5: "cm-lp-h5",
  ATXHeading6: "cm-lp-h6",
  SetextHeading1: "cm-lp-h1",
  SetextHeading2: "cm-lp-h2",
  Blockquote: "cm-lp-quote",
};

export interface LivePreviewOptions {
  /**
   * The open vault's id, so a `![[….csv]]` embed can be read and written.
   *
   * The editor is built per vault, so this is a value rather than a getter —
   * unlike `recordingSession`, which changes with the note in the editor.
   */
  vaultId: string;
  /** Turn a vault-relative asset path into its `keeper-note://` URL (AD-59). */
  assetUrl: (relPath: string) => string;
  /**
   * Follow a wikilink. Called with the raw target, never a filesystem path.
   *
   * Optional, because a host that cannot resolve a note name has none to give
   * — and until Story 45.18 `markdown-preview.ts` supplied `() => {}` for
   * exactly that case, which is a fabricated value standing in for a missing
   * one. Absent, a click falls through rather than being swallowed by a
   * function that does nothing.
   */
  onOpenLink?: (target: string) => void;
  /**
   * Follow an ordinary markdown link's destination (Story 45.18).
   *
   * Optional, and its absence is a real configuration rather than an oversight:
   * a host with no way to hand a URL to the OS must render a link that does
   * NOT look pressable, so the class is withheld and the decoration falls back
   * to a plain `title`. Between 37.6 and this story every host was in that
   * position and none of them said so — `.cm-lp-link` was `cursor: pointer`
   * over text that did nothing at all.
   */
  onOpenUrl?: (url: string) => void;
  /**
   * The open note's `session:` frontmatter, or null when it has none.
   *
   * Read at decoration time rather than captured, because the editor is built
   * once and outlives the note in it. Its presence is the whole test for "this
   * is a recording note" (Story 42.4) — the same predicate the properties panel
   * uses — and it is what turns an `![[…]]` embed into a player instead of a
   * link. Absent, every embed stays the ordinary link it has always been, which
   * is exactly right for somebody else's note that happens to contain one.
   */
  recordingSession?: () => string | null;
  /**
   * List a vault folder for a gallery block (Story 44.15, FR-171).
   *
   * Injected rather than imported for the same reason `assetUrl` is: this
   * module knows nothing about which vault is open, and the editor that built
   * it does. Absent — a note rendered outside the editor, or a test driving the
   * renderer directly — a gallery block stays the folder name and the pinned
   * links it degrades to, and says that keeper is not listing here rather than
   * showing an empty grid.
   */
  listFolder?: (folder: string) => Promise<NoteGalleryVm>;
  /**
   * Mount a `> [!board]` / `> [!log]` / `> [!refs]` panel (FR-264).
   *
   * Injected for the same reason `listFolder` is, and with the same fallback:
   * absent, the widget mounts its own React host through a dynamic import. What
   * a caller overrides it for is a test — a renderer driven without a Tauri
   * host has no vault to query — which is why the default is the real one
   * rather than nothing.
   *
   * Note the vault is NOT a second option here: {@link LivePreviewOptions.vaultId}
   * is already the vault this editor was built for, and a widget asking a
   * different one than an embed in the same note could not be right.
   */
  mountWidget?: NoteWidgetOptions["mount"];
}

/** An embedded image, or — when the file is not there — its alt text and the
 *  path keeper looked for (UX-DR44). Never an empty box. */
class ImageWidget extends WidgetType {
  constructor(
    private readonly alt: string,
    private readonly src: string,
    private readonly resolve: (relPath: string) => string,
  ) {
    super();
  }

  eq(other: ImageWidget): boolean {
    return other.src === this.src && other.alt === this.alt;
  }

  toDOM(): HTMLElement {
    const wrapper = document.createElement("span");
    wrapper.className = "cm-lp-image";
    const image = document.createElement("img");
    image.alt = this.alt;
    image.src = this.resolve(this.src);
    image.addEventListener("error", () => {
      const missing = document.createElement("span");
      missing.className = "cm-lp-image-missing";
      missing.textContent = `${this.alt === "" ? "image" : this.alt} — not found: ${this.src}`;
      wrapper.replaceChildren(missing);
    });
    wrapper.append(image);
    return wrapper;
  }
}

/** The line numbers the selection touches: their source stays visible. */
function revealedLines(view: EditorView): Set<number> {
  const revealed = new Set<number>();
  for (const range of view.state.selection.ranges) {
    const first = view.state.doc.lineAt(range.from).number;
    const last = view.state.doc.lineAt(range.to).number;
    for (let line = first; line <= last; line += 1) {
      revealed.add(line);
    }
  }
  return revealed;
}

/** The three characters a task marker is spelled with. */
const TASK_MARKER = /^\[[ xX]]$/;

/**
 * A task list's checkbox, which is the marker and is clickable.
 *
 * **The widget carries no position.** A widget outlives the decoration set that
 * placed it, so a captured offset goes stale the moment anything above it is
 * typed and the click would tick a different line. The live position is asked
 * of the view at click time, and the three characters found there are checked
 * to still be a marker before anything is written — so the worst a stale click
 * can do is nothing.
 *
 * `mousedown` is cancelled rather than handled: letting it through would move
 * the caret onto the line, the reveal rule would show the line's source, and
 * the checkbox would vanish out from under the click that was toggling it.
 */
/**
 * One statement an attribute block makes, in the spelling the registry uses.
 *
 * `name` never carries the empty prefix's colon: `{:depends_on}`, `{depends_on}`
 * and the legacy `{rel="depends_on"}` are three spellings of ONE predicate, and
 * a chip that showed the colon for the first would read as a fourth.
 */
interface Predicate {
  /** `prefix:local`, or `local` when the block used the default vocabulary. */
  name: string;
  /** The literal object of `{:type="Metric"}`, or null for a bare predicate. */
  object: string | null;
}

/**
 * The predicates a link or a fence carries, drawn as chips in place of braces.
 *
 * `[Belief](belief.md){schema:creator, foaf:knows}` reads as a link followed by
 * six words of punctuation until the braces are replaced by the words they
 * contain. The chips are the author's own vocabulary — keeper neither invents a
 * predicate nor translates one — so they are shown verbatim, in written order.
 *
 * Replaced rather than hidden: hiding the braces would leave a link that says
 * "creator" nowhere, and the whole point of writing one is that a reader can
 * see what kind of link it is without opening the source.
 *
 * ONE widget for N chips, and one for the `{reference="cites"}` spelling keeper
 * shipped first: that spelling is the same concept with an older syntax, it
 * folds into the same list, and a vault written before this story renders
 * exactly as it did — a single chip, no prefix, same class, same aria-label.
 */
class PredicateWidget extends WidgetType {
  /**
   * `onCode` is the fence-line case, and it exists because a chip's surface is
   * `--muted` and so is a code block's. Measured in Chromium on the owner's own
   * document: both resolved to `rgb(236, 234, 226)`, so the pill was there in
   * the DOM and invisible on screen — the chips read as bare text on the fence
   * line while the identical chips one line below read as labels. No DOM test
   * could see it; two colours being equal is only a defect once it is drawn.
   */
  constructor(
    private readonly predicates: readonly Predicate[],
    private readonly onCode = false,
  ) {
    super();
  }

  eq(other: PredicateWidget): boolean {
    return (
      other.predicates.length === this.predicates.length &&
      other.onCode === this.onCode &&
      other.predicates.every(
        (predicate, at) =>
          predicate.name === this.predicates[at]?.name &&
          predicate.object === this.predicates[at]?.object,
      )
    );
  }

  toDOM(): HTMLElement {
    const chips = document.createElement("span");
    chips.className = this.onCode
      ? "cm-lp-predicates cm-lp-predicates-on-code"
      : "cm-lp-predicates";
    for (const predicate of this.predicates) {
      chips.append(predicateChip(predicate));
    }
    return chips;
  }

  ignoreEvent(): boolean {
    return false;
  }
}

/**
 * One chip: `schema:` quiet, `creator` in the weight that carries the meaning.
 *
 * The split is weight and not colour, which took a measurement to settle. The
 * obvious quiet token for the prefix is `--faint`, and on the chip's `--muted`
 * surface it comes out at **3.32:1** in light and **3.69:1** in dark — over the
 * 3:1 floor `--faint` is held to, under the 4.5:1 that anything carrying a fact
 * needs. A CURIE's prefix IS a fact: `schema:creator` and `foaf:creator` are
 * different predicates, and a reader who cannot read the prefix cannot tell
 * which vocabulary the link speaks. So both halves keep `--muted-foreground`
 * (5.01:1 light, 6.82:1 dark on `--muted`) and the local part takes the weight.
 *
 * A literal object — `{:type="Metric"}` — was measured the same way and lands
 * in the same place. `Metric` is data: `:type="Metric"` and `:type="Dimension"`
 * are different statements, so the value cannot be spent at `--faint`'s 3.32:1
 * either, and `--foreground` (13.87:1 light, 14.35:1 dark on `--muted`) would
 * out-shout the prose the chip is only a label on. The quietest token that
 * still clears 4.5:1 is the chip's own `--muted-foreground`, so the value keeps
 * it and what separates data from vocabulary is shape instead: an `=`, which is
 * punctuation with no fact in it and so is the one part `--faint`'s 3.32:1 is
 * the right floor for, and the value in the resting weight and italic against
 * the local part's 500.
 *
 * A predicate with no prefix — `{:depends_on}`, `{depends_on}`, or the legacy
 * `reference` value — gets one text node and no prefix span.
 */
function predicateChip(predicate: Predicate): HTMLElement {
  const chip = document.createElement("span");
  chip.className = "cm-lp-predicate";
  // Named for a screen reader, which gets the link and then this and would
  // otherwise hear a bare word with no relationship to what precedes it.
  chip.setAttribute(
    "aria-label",
    predicate.object === null
      ? `link kind: ${predicate.name}`
      : `link kind: ${predicate.name} is ${predicate.object}`,
  );
  const colon = predicate.name.indexOf(":");
  if (colon === -1) {
    chip.append(document.createTextNode(predicate.name));
  } else {
    const prefix = document.createElement("span");
    prefix.className = "cm-lp-predicate-prefix";
    prefix.textContent = predicate.name.slice(0, colon + 1);
    const local = document.createElement("span");
    local.className = "cm-lp-predicate-local";
    local.textContent = predicate.name.slice(colon + 1);
    chip.append(prefix, local);
  }
  if (predicate.object !== null) {
    const equals = document.createElement("span");
    equals.className = "cm-lp-predicate-equals";
    equals.textContent = "=";
    const object = document.createElement("span");
    object.className = "cm-lp-predicate-object";
    object.textContent = predicate.object;
    chip.append(equals, object);
  }
  return chip;
}

/**
 * One half of a predicate: a letter, then letters, digits, `_` or `-`.
 *
 * The same shape `is_name` tests on the Rust side, and it has to stay the same:
 * one syntax read twice, once to draw it and once to put it in the graph, and
 * two readings of one syntax is how a note comes to show a relationship that no
 * query can find.
 */
const NAME = /^[A-Za-z][A-Za-z0-9_-]*$/;

/**
 * The two attribute keys whose VALUE names a predicate.
 *
 * `reference` is the spelling keeper shipped first and `rel` is HTML's own
 * relation attribute, which is what vaults are actually written with. Both are
 * folded into predicate names by `IndexProjection`, so both have to draw a chip
 * here: the links panel will show `cites` for `{rel="cites"}`, and an editor
 * that drew nothing would have the two halves of keeper disagreeing about which
 * tokens are edges.
 */
const LEGACY_PREDICATE_KEYS = new Set(["rel", "reference"]);

/**
 * The predicate a token spells, or null when it spells none.
 *
 * Three inputs, one output, which is the point: `prefix:local` keeps both
 * halves; `:local` is the document's default vocabulary and the colon is
 * STRIPPED, so it lands on the same string a bare `local` does; `local` alone is
 * Semantic Markdown V0's property name. Exactly one colon, because
 * `schema::creator` and `schema:creator:extra` both leave a half that is not a
 * name, and a token with two readings gets none.
 */
function predicateName(token: string): string | null {
  const colon = token.indexOf(":");
  if (colon === -1) {
    return NAME.test(token) ? token : null;
  }
  const local = token.slice(colon + 1);
  if (!NAME.test(local)) {
    return null;
  }
  const prefix = token.slice(0, colon);
  // The empty prefix resolves to the drive's own base — the note's own
  // `prefixes:` first, else `.okf/registry/predicates.md` — and that resolution
  // belongs where RDF is emitted. keeper displays the name it was given.
  if (prefix === "") {
    return local;
  }
  return NAME.test(prefix) ? `${prefix}:${local}` : null;
}

/** What one token inside a block turned out to be. */
type Token =
  | {
      kind: "predicate";
      predicate: Predicate;
      /** Written as `rel="x"` / `reference="x"` rather than as a token. The
       *  projection folds these in AFTER the modern tokens, and the chips have
       *  to be ordered the same way — see `predicatesAfter`. */
      legacy: boolean;
    }
  /** A `.class`, a `#id`, the lone `:` of kramdown's `{: .foo}` marker, or a
   *  pair like `width="40"`: presentation, and never a predicate. */
  | { kind: "presentational" }
  /** No single obvious reading. A wrong edge in a graph somebody queries is
   *  worse than an absent one, so nothing is repaired — see `AttrBlock.junk`. */
  | { kind: "junk" };

/**
 * One attribute block: `{schema:creator, :type="Metric", .highlight}`.
 *
 * Sticky rather than anchored so a run of ADJACENT blocks can be scanned
 * without slicing the document window once per block. One line: a stray brace
 * must not be able to swallow the rest of a note.
 */
const LINK_ATTRS = /\{([^}\n]*)\}/y;

/**
 * One token inside a block: a `key="value"` pair, or anything not a separator.
 *
 * The pair alternatives come first so a value containing a comma or a space
 * stays one token, and both quote characters are accepted because the Rust
 * tokeniser accepts both. Commas and whitespace are the separators,
 * interchangeably — `{a:b, c:d}` and `{a:b c:d}` are the same two predicates.
 */
const ATTR_TOKEN = /[^\s,="']+\s*=\s*"[^"\n]*"|[^\s,="']+\s*=\s*'[^'\n]*'|[^\s,]+/g;

/** `key="value"`, as a whole token. An empty value is not one, matching the
 *  Rust `read_pair`, so `{:type=""}` is junk on both sides rather than a
 *  predicate whose object silently went missing. */
const ATTR_PAIR = /^([^\s,="']+)\s*=\s*(?:"([^"\n]+)"|'([^'\n]+)')$/;

/**
 * Read one token, per Semantic Markdown V0's property-attribute rule laid over
 * kramdown's IAL.
 */
function readToken(token: string): Token {
  const pair = ATTR_PAIR.exec(token);
  if (pair !== null) {
    const key = pair[1] ?? "";
    const value = pair[2] ?? pair[3] ?? "";
    if (key.includes(":")) {
      // A colon-marked key states a predicate ABOUT the thing and gives it a
      // literal object: `{:type="Metric"}`, the owner's common case.
      const name = predicateName(key);
      return name === null
        ? { kind: "junk" }
        : { kind: "predicate", predicate: { name, object: value }, legacy: false };
    }
    if (!NAME.test(key)) {
      return { kind: "junk" };
    }
    if (LEGACY_PREDICATE_KEYS.has(key)) {
      // Here the VALUE is the predicate's name. `rel="see also"` is not a name,
      // so it stays the plain attribute it has always been rather than becoming
      // a two-word predicate nothing could ever query.
      const name = predicateName(value);
      return name === null
        ? { kind: "presentational" }
        : { kind: "predicate", predicate: { name, object: null }, legacy: true };
    }
    // `class`, `id`, `width`: presentation, and some of them are what the
    // vault's own toolkit writes. They keep the treatment they have always had.
    return { kind: "presentational" };
  }
  // kramdown spells a class `.name`, an id `#name`, and marks a block-level IAL
  // with a lone leading `:` — `{: .highlight}`. None of the three is a property
  // name, and reading `.metric` as one would put a CSS class into the graph.
  if (token === ":" || token.startsWith(".") || token.startsWith("#")) {
    return { kind: "presentational" };
  }
  // Anything still holding an `=` is a pair whose value was unquoted or empty.
  if (token.includes("=")) {
    return { kind: "junk" };
  }
  const name = predicateName(token);
  return name === null
    ? { kind: "junk" }
    : { kind: "predicate", predicate: { name, object: null }, legacy: false };
}

/** One block of a run, and what its tokens turned out to be. */
interface AttrBlock {
  /** Offsets of `{`…`}`, relative to the start of the run. */
  from: number;
  to: number;
  /** The chips this block draws: its predicates, minus any an earlier block in
   *  the run already drew. Empty when every one of them was a repeat. */
  chips: Predicate[];
  /** Whether the block wrote a predicate at all, repeat or not. A block that
   *  wrote none is not a predicate block and keeps its source. */
  writesPredicate: boolean;
  /** Set when nothing in the block may be replaced by a chip: a token with no
   *  single obvious reading, or one predicate handed two different objects.
   *  Both are things only the author can fix, and a chip drawn over either
   *  would show what keeper understood while swallowing the part that is wrong.
   *  See the call site. */
  junk: boolean;
}

/**
 * Every attribute block written straight against `text`'s first character.
 *
 * The markdown parser has never heard of these, so after a link they arrive as
 * ordinary text and this reads them off the document directly. Same rules as
 * the Rust side, and they have to stay the same: no space before the brace, a
 * quoted value for a pair, one line, and adjacent blocks merged in order with
 * duplicate predicates dropped.
 *
 * Order, and where it comes from: modern tokens keep true written order, and a
 * legacy `rel=`/`reference=` name is APPENDED after every one of them across
 * the whole run. That is not this module's choice — it is what
 * `link_predicate_map` produces, because `RawLink` keeps tokens and pairs in
 * two vectors and a legacy pair is a compatibility shim, and the chips have to
 * agree with the panel about both membership AND order or the same link reads
 * two ways on one screen. Interleaving is only reachable by giving `links.rs`
 * one ordered list holding both, at which point all three surfaces move
 * together.
 *
 * The run and not the block, for the same reason the projection folds across
 * the run: `[x](y){:a}{rel="b"}` is one link carrying two blocks.
 */
function predicatesAfter(text: string): { blocks: AttrBlock[]; length: number } | null {
  const blocks: AttrBlock[] = [];
  /** Every predicate read, in scan order, tagged with the block that wrote it
   *  so a repeat can send THAT block back to source. */
  const reads: { block: AttrBlock; predicate: Predicate; legacy: boolean }[] = [];
  let at = 0;
  for (;;) {
    LINK_ATTRS.lastIndex = at;
    const match = LINK_ATTRS.exec(text);
    if (match === null) {
      break;
    }
    const block: AttrBlock = {
      from: at,
      to: at + match[0].length,
      chips: [],
      writesPredicate: false,
      junk: false,
    };
    for (const token of (match[1] ?? "").matchAll(ATTR_TOKEN)) {
      const read = readToken(token[0]);
      if (read.kind === "presentational") {
        continue;
      }
      if (read.kind === "junk") {
        block.junk = true;
        continue;
      }
      block.writesPredicate = true;
      reads.push({ block, predicate: read.predicate, legacy: read.legacy });
    }
    blocks.push(block);
    at += match[0].length;
  }
  if (blocks.length === 0) {
    return null;
  }

  // Keyed by name and not by whole statement, because this list is the list the
  // graph gets and `IndexEntry.link_predicates` holds one entry per name.
  const seen = new Map<string, string | null>();
  const legacy: Predicate[] = [];
  for (const read of reads) {
    if (read.legacy) {
      continue;
    }
    const { name, object } = read.predicate;
    if (seen.has(name)) {
      // The name repeats. An identical statement is the duplicate the graph
      // already collapses; a different object is two answers to one question
      // and only one can reach the graph, so the source stays on screen.
      if (seen.get(name) !== object) {
        read.block.junk = true;
      }
      continue;
    }
    seen.set(name, object);
    read.block.chips.push(read.predicate);
  }
  // A second pass over the same array rather than one over a sorted copy of it.
  // Second because the projection folds legacy pairs in AFTER the modern
  // tokens, so first-wins dedupe has to meet the modern ones first: `{:a,
  // rel="a"}` and `{rel="a", :a}` both leave the token standing and the pair
  // dropped, whichever way round they were typed.
  for (const read of reads) {
    if (!read.legacy) {
      continue;
    }
    const { name } = read.predicate;
    if (seen.has(name)) {
      // A legacy pair never carries an object, so a name already spoken for by
      // a `{:name="value"}` is a second answer and sends the block to source.
      if (seen.get(name) !== null) {
        read.block.junk = true;
      }
      continue;
    }
    seen.set(name, null);
    // Held back rather than pushed onto its own block: it has to be drawn after
    // every modern chip of the run, wherever in the run it was written.
    legacy.push(read.predicate);
  }
  // The last block that is going to be drawn at all is where the run's chips
  // end, so that is where the deferred names go. A junk block keeps its source,
  // which already shows the author the `rel=` they wrote.
  const drawn = blocks.filter((block) => block.writesPredicate && !block.junk);
  drawn[drawn.length - 1]?.chips.push(...legacy);

  return { blocks, length: at };
}

/**
 * The attribute-block run a fence's info string ends with, or null.
 *
 * ```` ```json { :type="Metric" } ```` puts the block after the language, so
 * unlike a link's run this one does not begin at offset zero and has to be
 * found. Found by trying each `{` left to right and keeping the first whose run
 * reaches the end of the info string: that is what makes the block the TAIL of
 * the info string rather than something buried inside a quoted value, so
 * `json { :a="x{y}" }` — whose braces do not nest — reads as no block at all
 * and keeps its source instead of drawing a chip for `y`.
 *
 * Every CommonMark rule about WHICH lines have an info string is the parser's
 * answer and not this function's: a `CodeInfo` node exists only for the
 * outermost opening fence, only when the indent is under four spaces, never for
 * a closing fence, and never when a backtick fence's info string contains a
 * backtick. Writing those rules a second time here is how the two readings
 * drift apart.
 */
function infoStringPredicates(info: string): AttrBlock[] | null {
  // `info.length` and not a trimmed length: the parser hands over an info
  // string with its trailing spaces already stripped, so trimming here would
  // be a second rule doing nothing.
  for (let brace = info.indexOf("{"); brace !== -1; brace = info.indexOf("{", brace + 1)) {
    const run = predicatesAfter(info.slice(brace));
    if (run === null || brace + run.length !== info.length) {
      continue;
    }
    // The gap between the language and the brace goes with the first block, so
    // a replaced block leaves `json` and its chips one chip-margin apart rather
    // than that plus however many spaces the author happened to type.
    let gap = brace;
    while (gap > 0 && /\s/.test(info[gap - 1] ?? "")) {
      gap -= 1;
    }
    return run.blocks.map((block, at) => ({
      ...block,
      from: at === 0 ? gap : brace + block.from,
      to: brace + block.to,
    }));
  }
  return null;
}

/**
 * Where the attribute block belonging to `link` would begin.
 *
 * `**[JWT Auth Service](https://github.com)**{ :depends_on }` is the owner's own
 * spelling: the block goes after the emphasis markers that CLOSE the link, so
 * the run does not start at the link's end and a reader who scanned from there
 * would find `**` and give up.
 *
 * Walked out one emphasis at a time, and only while the closing marker sits
 * flush against what it wraps. In `**[a](b) tail**{ :p }` the block qualifies
 * the emphasis's last word rather than the link, so it stops at the link's end
 * and nothing is drawn — attaching it anyway would put an edge on the wrong
 * subject, which is the one failure this whole story exists to avoid.
 */
function attrRunStart(link: SyntaxNode): number {
  let end = link.to;
  for (let node = link; ; ) {
    const parent = node.parent;
    if (parent === null || (parent.name !== "Emphasis" && parent.name !== "StrongEmphasis")) {
      return end;
    }
    const closing = parent.lastChild;
    if (closing === null || closing.name !== "EmphasisMark" || closing.from !== end) {
      return end;
    }
    end = closing.to;
    node = parent;
  }
}

class TaskWidget extends WidgetType {
  constructor(private readonly checked: boolean) {
    super();
  }

  eq(other: TaskWidget): boolean {
    return other.checked === this.checked;
  }

  toDOM(view: EditorView): HTMLElement {
    const box = document.createElement("input");
    box.type = "checkbox";
    box.className = "cm-lp-task";
    box.checked = this.checked;
    box.setAttribute("aria-label", this.checked ? "Done" : "To do");
    box.addEventListener("mousedown", (event) => event.preventDefault());
    box.addEventListener("click", (event) => {
      event.preventDefault();
      const from = view.posAtDOM(box);
      const current = view.state.doc.sliceString(from, from + 3);
      if (!TASK_MARKER.test(current)) {
        return;
      }
      view.dispatch({
        changes: { from, to: from + 3, insert: current[1] === " " ? "[x]" : "[ ]" },
      });
    });
    return box;
  }

  /** The checkbox handles its own clicks; the editor must not also treat one
   *  as a place to put the caret. */
  ignoreEvent(): boolean {
    return true;
  }
}

function buildDecorations(view: EditorView, options: LivePreviewOptions): DecorationSet {
  const { doc } = view.state;
  const revealed = revealedLines(view);
  const decorations: Range<Decoration>[] = [];
  // A link that can be followed says so with the cursor; one that cannot must
  // not (Story 45.18). Between 37.6 and this story `.cm-lp-link` was
  // `cursor: pointer` in every host and no host had a follower, so the whole
  // affordance was a pointer over dead text.
  const urlClass = options.onOpenUrl === undefined ? "cm-lp-link" : "cm-lp-link cm-lp-followable";

  /** Whether any line the range spans is showing its source. */
  const isRevealed = (from: number, to: number): boolean => {
    const first = doc.lineAt(from).number;
    const last = doc.lineAt(to).number;
    for (let line = first; line <= last; line += 1) {
      if (revealed.has(line)) {
        return true;
      }
    }
    return false;
  };

  // Underline is a pair of literal `HTMLTag` nodes with no node of their own to
  // hang a class on, so the opening tags wait here until their closer arrives.
  const openUnderlines: { from: number; to: number }[] = [];

  for (const visible of view.visibleRanges) {
    syntaxTree(view.state).iterate({
      from: visible.from,
      to: visible.to,
      enter: (node) => {
        // A gallery block is decorated by `galleryLayer` below and not here,
        // and a mermaid fence by `mermaidLayer`: both replace several lines
        // with one element, and CodeMirror refuses both a block decoration and
        // a line-break-spanning replace from a `ViewPlugin`. Nothing needs
        // excluding at this point — a field's replacement covers the whole
        // block, so the line classes and marks underneath it fall inside a
        // range nothing paints. (Supplying one from here anyway is what DW-165
        // was: it threw out of `EditorView` construction.)

        const lineClass = LINE_CLASSES[node.name];
        if (lineClass !== undefined) {
          const first = doc.lineAt(node.from).number;
          const last = doc.lineAt(node.to).number;
          for (let line = first; line <= last; line += 1) {
            decorations.push(Decoration.line({ class: lineClass }).range(doc.line(line).from));
          }
        }

        if (node.name === "FencedCode") {
          const first = doc.lineAt(node.from).number;
          const last = doc.lineAt(node.to).number;
          for (let line = first; line <= last; line += 1) {
            decorations.push(Decoration.line({ class: "cm-lp-fence" }).range(doc.line(line).from));
          }
          return undefined;
        }

        // A fence carries its attributes on the tail of its opening info
        // string: ```` ```json { :type="Metric" } ````. The language stays
        // visible and the block becomes chips, so the fence line says what the
        // block IS rather than only what it is written in — which for the
        // owner's `Metric` blocks is the whole point of the annotation.
        //
        // Anchored on `CodeInfo` and nothing else, because that node exists
        // exactly where CommonMark says an info string does: on the OUTERMOST
        // opening fence only, so a ``` line nested inside a 4-backtick block is
        // content and gets nothing; never on a closing fence; not under a
        // four-space indent; and not at all when a backtick fence's info string
        // contains a backtick. Tilde fences have one, and so does a fence the
        // author never closed.
        if (node.name === "CodeInfo") {
          const blocks = infoStringPredicates(doc.sliceString(node.from, node.to));
          if (blocks !== null && !isRevealed(node.from, node.to)) {
            for (const block of blocks) {
              if (block.junk || !block.writesPredicate) {
                continue;
              }
              decorations.push(
                Decoration.replace({
                  // `onCode`: this line is inside the code block's own surface,
                  // which is the same `--muted` the chip uses.
                  widget: new PredicateWidget(block.chips, true),
                }).range(node.from + block.from, node.from + block.to),
              );
            }
          }
          // No `return`: execution has to reach `INLINE_CLASSES.CodeInfo` at
          // the foot of this callback, which is what keeps the language itself
          // visible and quiet. Returning here is what blanked every fence's
          // language the first time this branch was written.
        }

        if (node.name === "Image") {
          const raw = doc.sliceString(node.from, node.to);
          const match = /^!\[([^\]]*)]\(([^)\s]+)\)$/.exec(raw);
          // A remote URL is left as source, whole and visible: keeper never
          // fetches one, so a note cannot become a tracking pixel (NFR-11's
          // egress claim). Showing the alt text alone — which is what hiding
          // the destination by node name used to do — made a remote embed look
          // like a word somebody typed, with no hint that an image was meant.
          if (match === null || /^[a-z][a-z0-9+.-]*:/i.test(match[2])) {
            return false;
          }
          if (!isRevealed(node.from, node.to)) {
            decorations.push(
              Decoration.replace({
                widget: new ImageWidget(match[1], match[2], options.assetUrl),
              }).range(node.from, node.to),
            );
            return false;
          }
          return undefined;
        }

        // A link keeps its text, gains the link class, and carries its
        // destination twice: in a `title`, because the destination is about to
        // be hidden and hovering has to answer "where does this go?" without
        // moving the caret into the line; and in `LINK_ATTR`, which is what the
        // click handler follows (Story 45.18). Two attributes rather than one
        // because a title is a user-visible string and following a link must
        // not depend on how it is worded.
        if (node.name === "Link" || node.name === "Autolink") {
          let destination: string | null = null;
          for (let child = node.node.firstChild; child !== null; child = child.nextSibling) {
            if (child.name === "URL") {
              destination = doc.sliceString(child.from, child.to);
              break;
            }
          }
          decorations.push(
            Decoration.mark({
              class: urlClass,
              attributes:
                destination === null ? {} : { title: destination, [LINK_ATTR]: destination },
            }).range(node.from, node.to),
          );
          // The attribute blocks written straight after the link — or straight
          // after the emphasis markers that close it, which is where the
          // owner's own notes put them. The parser has never heard of either,
          // so they are plain text sitting past this node and have to be read
          // off the document.
          //
          // 200 characters is the window, which is generous for a run of
          // predicates and short enough that a note full of links does not
          // become a note full of string copies.
          const runFrom = attrRunStart(node.node);
          const trailing = predicatesAfter(doc.sliceString(runFrom, runFrom + 200));
          if (trailing !== null && !isRevealed(node.to, runFrom + trailing.length)) {
            for (const block of trailing.blocks) {
              // Two blocks stay exactly as the author typed them, and both
              // rules are about not hiding text nobody can see hidden:
              //
              //   - one with a token keeper cannot read, because replacing it
              //     with a chip would show the tokens keeper DID understand and
              //     silently swallow the one the author needs to fix;
              //   - one that writes no predicate at all — `{class="wide"}`,
              //     `{strength="weak"}` — which is source, and was source
              //     before this story too.
              //
              // A block whose predicates were all written by an earlier block
              // of the run is neither: it gets an empty widget, so the braces
              // go and the duplicate does not draw a second identical chip.
              // The chips are the list the graph gets, exactly.
              if (block.junk || !block.writesPredicate) {
                continue;
              }
              // The range is the BLOCK and not the emphasis run before it, even
              // for `**[a](b)**{ :p }` where the two are adjacent: the closing
              // `**` is a `EmphasisMark`, `HIDDEN_MARKS` replaces it on the same
              // reveal condition, and claiming those bytes here would put two
              // replacements on one offset for no visible gain. The apparent gap
              // is deliberate.
              decorations.push(
                Decoration.replace({
                  widget: new PredicateWidget(block.chips),
                }).range(runFrom + block.from, runFrom + block.to),
              );
            }
          }
          return undefined;
        }

        // `URL`, `LinkMark` and `LinkLabel` mean different things under
        // different parents, and hiding them by name is the defect this story
        // was sent to fix: `<https://example.com>` and a bare
        // `https://example.com` are both a `URL` node whose text IS the link,
        // so blanket-hiding it deleted them from the rendered note entirely.
        if (node.name === "LinkMark" || node.name === "LinkLabel") {
          if (LINK_PUNCTUATION[node.node.parent?.name ?? ""] === true) {
            if (!isRevealed(node.from, node.to)) {
              decorations.push(Decoration.replace({}).range(node.from, node.to));
            }
          }
          return undefined;
        }

        if (node.name === "URL") {
          const parent = node.node.parent?.name ?? "";
          // A destination — the one URL a reader does not want to see, because
          // the link's text is right there saying where it goes.
          if (parent === "Link" || parent === "Image") {
            if (!isRevealed(node.from, node.to)) {
              decorations.push(Decoration.replace({}).range(node.from, node.to));
            }
            return false;
          }
          // Under an `Autolink` the surrounding node already carries the class;
          // under a `LinkReference` this is a definition line, which is source.
          if (parent !== "Autolink" && parent !== "LinkReference") {
            // A bare GFM autolink: no wrapping node, so it gets the class here.
            decorations.push(
              Decoration.mark({
                class: urlClass,
                attributes: {
                  title: doc.sliceString(node.from, node.to),
                  [LINK_ATTR]: doc.sliceString(node.from, node.to),
                },
              }).range(node.from, node.to),
            );
          }
          return undefined;
        }

        // The checkbox IS the marker, so a rendered task list is the same
        // number of characters wide as its source and nothing reflows when the
        // caret arrives and the source comes back.
        if (node.name === "TaskMarker" && !isRevealed(node.from, node.to)) {
          const marker = doc.sliceString(node.from, node.to);
          decorations.push(
            Decoration.replace({ widget: new TaskWidget(marker[1] !== " ") }).range(
              node.from,
              node.to,
            ),
          );
          return false;
        }

        if (node.name === "HTMLTag") {
          const tag = doc.sliceString(node.from, node.to);
          if (tag === UNDERLINE_OPEN) {
            openUnderlines.push({ from: node.from, to: node.to });
          } else if (tag === UNDERLINE_CLOSE) {
            const open = openUnderlines.pop();
            // An empty `<u></u>` gets its tags hidden and no mark: CodeMirror
            // rejects a mark decoration with nothing between its ends.
            if (open !== undefined && !isRevealed(open.from, node.to)) {
              if (open.to < node.from) {
                decorations.push(
                  Decoration.mark({ class: "cm-lp-underline" }).range(open.to, node.from),
                );
              }
              decorations.push(Decoration.replace({}).range(open.from, open.to));
              decorations.push(Decoration.replace({}).range(node.from, node.to));
            }
          }
          return undefined;
        }

        const inlineClass = INLINE_CLASSES[node.name];
        if (inlineClass !== undefined) {
          decorations.push(Decoration.mark({ class: inlineClass }).range(node.from, node.to));
        }

        if (HIDDEN_MARKS[node.name] === true && !isRevealed(node.from, node.to)) {
          decorations.push(Decoration.replace({}).range(node.from, node.to));
        }
        return undefined;
      },
    });

    // Wikilinks, line by line, because the grammar does not see them.
    let line = doc.lineAt(visible.from);
    while (line.from <= visible.to) {
      if (!revealed.has(line.number)) {
        WIKILINK.lastIndex = 0;
        for (const match of line.text.matchAll(WIKILINK)) {
          const start = line.from + (match.index ?? 0);
          const end = start + match[0].length;
          const target = match[1];
          const label = match[2] ?? target;

          // The attribute block written straight after `]]`, for EVERY wikilink
          // form — an embed, a piped label, a bare `[[note]]`.
          //
          // The owner reported `![[attachments/….csv]]{ :value_of }` rendering
          // as a panel with the braces left on screen as source, and this is
          // the whole of that defect: each of the three branches below reaches
          // `continue` before anything reads the document past `end`, so the
          // run was never looked at. Read HERE rather than inside a branch
          // because a predicate is about the link and not about what the target
          // turned out to be — an embedded CSV, a recording's video and a plain
          // note all carry them.
          //
          // Rust already agreed: `links::extract` calls `read_attrs` at
          // `close + 2` whatever `RawLink::embed` says, `link_predicate_map`
          // never reads that flag, and `RawLink::span` covers the block. So
          // before this the graph held the edge and the note showed the braces,
          // which is the editor and the links panel disagreeing about which
          // tokens are edges — on one screen, about one link.
          //
          // No `isRevealed` call: this whole loop is already inside
          // `!revealed.has(line.number)`, and a block never crosses a newline,
          // so the run is on a line that is known to be hiding its source.
          // 200 characters, the same window the `Link` branch uses: generous for
          // a run of predicates, short enough that a note full of links is not a
          // note full of string copies.
          const runFrom = (match.index ?? 0) + match[0].length;
          const attrs = predicatesAfter(line.text.slice(runFrom, runFrom + 200));
          for (const block of attrs?.blocks ?? []) {
            // Junk and a block that writes no predicate stay as the author typed
            // them, for the two reasons the `Link` branch gives above: a chip
            // over an unreadable token hides the one thing the author has to
            // fix, and `{width="40"}` was source before this and still is.
            if (block.junk || !block.writesPredicate) {
              continue;
            }
            decorations.push(
              Decoration.replace({ widget: new PredicateWidget(block.chips) }).range(
                end + block.from,
                end + block.to,
              ),
            );
          }

          // `![[….csv]]`, `![[….json]]`, `![[….jsonl]]`: the one embed syntax,
          // rendered as the panel Story 45.12 mounts. WHICH targets get one is
          // 45.2's registry's answer and not a list here — `embedEntryFor`
          // returns a row or null, and this branch is the whole of what
          // `live-preview.ts` knows about formats.
          //
          // The note's session goes with it rather than sending the target to
          // the branch below, because in a recording note a data target may be
          // either the session's own `manifest.json` or a vault attachment
          // beside the note, and only an answer from the index can tell those
          // apart. The widget asks the session first and the vault second.
          if (match[0].startsWith("!") && embedEntryFor(target) !== null) {
            // An INLINE replace with a block-styled host, not `block: true`.
            // These decorations come from a `ViewPlugin`, and CodeMirror refuses
            // a block decoration from one (DW-165) — 45.10 moved the mermaid
            // fence out to `mermaidLayer` rather than changing that rule. The
            // embed is a single line, so the inline form is available and is the
            // shape `RecordingEmbedWidget` already uses.
            decorations.push(
              Decoration.replace({
                widget: new FileEmbedWidget(
                  options.vaultId,
                  target,
                  options.recordingSession?.() ?? null,
                ),
              }).range(start, end),
            );
            continue;
          }

          // `![[…]]` in a note that carries a session id: an embed of one of
          // that recording's files. The widget renders this same link until the
          // index confirms the path is a video, so the only thing decided here
          // is that the embed gets to try (Story 42.4).
          if (match[0].startsWith("!")) {
            const sessionId = options.recordingSession?.() ?? null;
            if (sessionId !== null) {
              decorations.push(
                Decoration.replace({
                  widget: new RecordingEmbedWidget(sessionId, target, label, {
                    // Scoped to this view: two editors open on one note are two
                    // readers, and one pressing play must not move the other's
                    // video (Story 43.6).
                    transport: transportFor(view, sessionId),
                  }),
                }).range(start, end),
              );
              continue;
            }

            // Not a data file, and not a recording note: the vault is what is
            // left (Story 55.4). Until this branch a photograph embedded in an
            // ordinary note was a link to a photograph — the widget existed and
            // only a recording note could reach it, because only a recording
            // note had an address space to resolve in.
            //
            // A recording note never reaches here, and deliberately: in one,
            // `manifest.json` under the session folder and `attachments/x.png`
            // beside the note are different files, and `RecordingEmbedWidget`
            // is the only thing that can tell them apart. It already looks in
            // the vault when the session declines.
            decorations.push(
              Decoration.replace({
                widget: new VaultEmbedWidget(options.vaultId, target, {
                  assetUrl: options.assetUrl,
                }),
              }).range(start, end),
            );
            continue;
          }

          const labelStart = start + match[0].indexOf(label, match[0].indexOf("[[") + 2);
          decorations.push(Decoration.replace({}).range(start, labelStart));
          decorations.push(
            Decoration.mark({
              class: "cm-lp-wikilink",
              attributes: { [WIKILINK_ATTR]: target },
            }).range(labelStart, labelStart + label.length),
          );
          decorations.push(Decoration.replace({}).range(labelStart + label.length, end));
        }
      }
      if (line.to >= doc.length) {
        break;
      }
      line = doc.lineAt(line.to + 1);
    }
  }

  return Decoration.set(decorations, true);
}

/** Highlight a range that arrived from outside this editor. */
export const flashExternalEffect = StateEffect.define<{ from: number; to: number }>();

/** Drop every external highlight. */
export const clearExternalFlashEffect = StateEffect.define<null>();

const externalFlashField = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, transaction) {
    let next = value.map(transaction.changes);
    for (const effect of transaction.effects) {
      if (effect.is(clearExternalFlashEffect)) {
        next = Decoration.none;
      }
      if (effect.is(flashExternalEffect)) {
        const ranges: Range<Decoration>[] = [];
        const first = transaction.state.doc.lineAt(effect.value.from).number;
        const last = transaction.state.doc.lineAt(effect.value.to).number;
        for (let line = first; line <= last; line += 1) {
          ranges.push(
            Decoration.line({ class: "cm-lp-external" }).range(
              transaction.state.doc.line(line).from,
            ),
          );
        }
        next = Decoration.set(ranges, true);
      }
    }
    return next;
  },
  provide: (field) => EditorView.decorations.from(field),
});

// Re-exported rather than moved out of sight: `note-editor.tsx` reaches it as
// `preview.spliceBetween` and this module is still the natural place to look
// for it. The implementation lives in `text-splice.ts` so that
// `markdown-table.ts` can import it without importing this module back — see
// that file for the cycle it broke.
export { spliceBetween, type TextSplice } from "./text-splice";

/** Paint the fading highlight over a range, then let it go. */
export function flashExternal(view: EditorView, from: number, to: number): void {
  view.dispatch({ effects: flashExternalEffect.of({ from, to }) });
  setTimeout(() => {
    view.dispatch({ effects: clearExternalFlashEffect.of(null) });
  }, EXTERNAL_FLASH_MS);
}

const livePreviewTheme = EditorView.baseTheme({
  // The floor under every widget this editor draws, and the reason it is one
  // line rather than a rule per widget.
  //
  // `.cm-content` is a flex item of `.cm-scroller`, so its `min-width` is `auto`
  // — the widest thing inside it. `EditorView.lineWrapping` makes prose wrap to
  // that width, which means ONE wide, unbreakable child re-lays the whole
  // document at its width and the pane clips the rest. Every wide block here is
  // supposed to be contained and most of them are; a note in the vault proves
  // they are not all contained, and hunting the last one is a game with no end
  // — the next widget somebody adds starts it again.
  //
  // Measured against a real CodeMirror with `lineWrapping` and one uncontained
  // block, in a 600px pane: `.cm-content` came out **1796px**, which is exactly
  // what the pane was showing. With this line, 600px, and the block keeps its
  // own `overflow-x` so nothing becomes unreachable — it scrolls inside itself
  // instead of dragging the prose out of the pane with it.
  ".cm-content": {
    minWidth: "0",
  },

  ".cm-lp-strong": { fontWeight: "600" },
  ".cm-lp-em": { fontStyle: "italic" },
  ".cm-lp-strike": { textDecoration: "line-through" },
  // The one inline mark that paints a background rather than changing the
  // glyphs, so it is the one that has to answer for contrast in both themes.
  // `--mark` is defined beside the other palette tokens in `index.css`, and is
  // not `--search-highlight`: see the comment there.
  ".cm-lp-mark": {
    backgroundColor: "var(--mark)",
    color: "var(--mark-foreground)",
    borderRadius: "2px",
    padding: "0 1px",
  },
  ".cm-lp-underline": { textDecoration: "underline" },
  // `vertical-align` alone would push the line's height around; the smaller
  // font is what keeps a paragraph containing `H~2~O` the same height as one
  // that does not, which is what stops a list of formulae from jittering.
  ".cm-lp-sub": { verticalAlign: "sub", fontSize: "0.75em" },
  ".cm-lp-sup": { verticalAlign: "super", fontSize: "0.75em" },
  ".cm-lp-code": {
    fontFamily: "var(--font-mono, ui-monospace, monospace)",
    backgroundColor: "var(--muted)",
    borderRadius: "3px",
    padding: "0 3px",
  },
  // Colour says "this is a link"; the cursor says "this one goes somewhere when
  // you press it". They were one rule until Story 45.18, which is why an
  // external URL looked pressable for four stories while nothing followed it.
  // A wikilink always keeps the pointer: every host that mounts this layer can
  // resolve a note name, and the embed widgets' degrade anchors reuse the class.
  ".cm-lp-link, .cm-lp-wikilink": { color: "var(--primary)" },
  ".cm-lp-wikilink, .cm-lp-followable": { cursor: "pointer" },
  ".cm-lp-h1": { fontSize: "1.5em", fontWeight: "600" },
  ".cm-lp-h2": { fontSize: "1.3em", fontWeight: "600" },
  ".cm-lp-h3": { fontSize: "1.15em", fontWeight: "600" },
  ".cm-lp-h4, .cm-lp-h5, .cm-lp-h6": { fontWeight: "600" },
  ".cm-lp-quote": {
    borderLeft: "3px solid var(--border)",
    paddingLeft: "0.75em",
    color: "var(--muted-foreground)",
  },
  // The one line decoration in this file that paints a SURFACE rather than
  // recolouring glyphs, which is why it is the only one that has to answer for
  // its own edges.
  //
  // `Decoration.line` puts this class on `.cm-line`, and CodeMirror's own base
  // theme gives every line `padding: 0 2px 0 6px`. A background paints the
  // padding box, so the grey ran the FULL width of the content box while the
  // text inside it — and every other line's text in the note — occupied the
  // column 6px in from the left and 2px in from the right. Measured in
  // Chromium, 520px pane, content box x=2..518: the fence's grey was
  // **2..518** against prose text at **8..516**, so it overhung the reading
  // column by 6px on the left and 2px on the right and ran flush into the
  // note's border with no gutter. Which is the report — a rendered fence
  // bleeding past the note's edge — and note the asymmetry is the part a reader
  // actually sees: 6 one side, 2 the other, a wonky edge nobody can name.
  //
  // The inset is a MARGIN and not a smaller padding, because padding is where
  // the code's own breathing room comes from and because CodeMirror reads the
  // first line's `paddingLeft`/`paddingRight` to place selection rectangles
  // (`rectanglesForRange`) — a fence with different padding from prose would
  // draw a selection that disagreed with the text it covers. Horizontal only:
  // a vertical margin on `.cm-line` would lie to the height map.
  //
  // Nothing reflows when the caret arrives, either. `.cm-lp-fence` is applied
  // to every line of a `FencedCode` unconditionally — there is no `isRevealed`
  // test on it — so the revealed and rendered states have the same box.
  ".cm-lp-fence": {
    fontFamily: "var(--font-mono, ui-monospace, monospace)",
    backgroundColor: "var(--muted)",
    marginLeft: "6px",
    marginRight: "2px",
  },
  // The language name, on the opening fence line. Quiet, because it labels the
  // block rather than being part of it.
  ".cm-lp-fence-info": {
    color: "var(--muted-foreground)",
    fontSize: "0.8em",
  },
  // The chips a link's predicates are drawn as. Quiet: they are labels on
  // something else, not things in their own right, and a note with many of them
  // should still read as prose rather than as a list of badges.
  ".cm-lp-predicate": {
    marginInlineStart: "0.3em",
    padding: "0 0.35em",
    borderRadius: "0.25em",
    background: "var(--muted)",
    color: "var(--muted-foreground)",
    fontSize: "0.85em",
  },
  // On a fence line the chip is sitting on the code block's surface, which IS
  // `--muted` — so the pill above would be an invisible rectangle and the
  // annotation would read as bare text exactly where a reader is least likely
  // to expect meaning. `--background` is the only other surface in play here,
  // and it measures BETTER than the ordinary case rather than worse:
  // `--muted-foreground` on it is 5.32:1 light and 7.34:1 dark (against 4.96:1
  // and 6.82:1 on `--muted`), and the `=`'s `--faint` is 3.57:1 and 3.97:1,
  // both over the 3:1 an indicator is held to. Measured from the tokens in
  // `src/index.css`, not sampled from a screenshot.
  ".cm-lp-predicates-on-code .cm-lp-predicate": {
    background: "var(--background)",
  },
  // A CURIE's two halves. The vocabulary is quieter than the term it qualifies,
  // by weight and not by colour: see `predicateChip` for the two ratios that
  // decided that. `500` rather than the `600` the headings use — a chip that
  // out-weighed the prose around it would stop being a label.
  ".cm-lp-predicate-local": { fontWeight: "500" },
  // The `=` of `{:type="Metric"}`. The one part of a chip that states no fact,
  // so it is the one part `--faint` is spendable on: 3.32:1 light and 3.69:1
  // dark on `--muted`, over the 3:1 an indicator is held to and under the 4.5:1
  // the value beside it needs. Not padded — `type=Metric` reads as one chip,
  // and space around the sign would make it read as two.
  ".cm-lp-predicate-equals": { color: "var(--faint)" },
  // The literal object. `--muted-foreground` like the rest of the chip, because
  // it is data and every quieter token misses 4.5:1 — see `predicateChip`. What
  // marks it as data rather than vocabulary is the resting weight against the
  // local part's 500, and italic, which is what a quoted literal has looked
  // like in running text since long before any of this. Costs no contrast.
  ".cm-lp-predicate-object": { fontStyle: "italic" },
  // Sized and spaced to occupy the three columns `[ ]` occupied, so ticking a
  // box does not reflow the paragraph and neither does moving the caret onto
  // the line and getting the source back.
  ".cm-lp-task": {
    verticalAlign: "middle",
    margin: "0 0.15em",
    cursor: "pointer",
  },
  ".cm-lp-image img": { maxWidth: "100%", borderRadius: "4px" },
  // The host stays inline so an unresolved embed sits in its sentence like the
  // link it still is; a rendered video or image goes block, because either one
  // wedged into a line of prose is neither readable nor watchable. Audio does
  // too: a native transport bar is a couple of hundred pixels wide and its own
  // paragraph either way (Story 42.4, widened by Story 43.5).
  // Named for the recording embed that first needed them and now shared with an
  // ordinary note's (Story 55.4): they say what the element IS, and an `<img>`
  // is the same `<img>` whichever address space resolved it.
  ".cm-lp-recording-player, .cm-lp-recording-image": {
    display: "block",
    maxWidth: "100%",
    maxHeight: "60vh",
    borderRadius: "4px",
    backgroundColor: "var(--muted)",
  },
  ".cm-lp-recording-audio": { display: "block", width: "100%", maxWidth: "24rem" },
  // A PDF gets a page's worth of height rather than `maxHeight`: an `<embed>`
  // has no intrinsic size to cap, so without one it collapses to nothing.
  ".cm-lp-embed-pdf": {
    display: "block",
    width: "100%",
    height: "60vh",
    borderRadius: "4px",
    backgroundColor: "var(--muted)",
  },
  // The chip stays INLINE, unlike the three that render: it is a reference to a
  // file, closer to the link it replaced than to a player, and a block-level
  // box for `manifest.json` would shout louder than the recording above it.
  ".cm-lp-recording-chip": {
    display: "inline-flex",
    alignItems: "center",
    gap: "0.375em",
    padding: "0.1em 0.5em",
    borderRadius: "4px",
    border: "1px solid var(--border)",
    backgroundColor: "var(--muted)",
    fontSize: "0.9em",
  },
  ".cm-lp-recording-chip-name": {
    fontFamily: "var(--font-mono, ui-monospace, monospace)",
  },
  ".cm-lp-recording-chip-action": {
    color: "var(--primary)",
    cursor: "pointer",
    // The two actions are text, not icons: this module has no icon set that is
    // not React, and a labelled control is legible to everyone anyway.
    textDecoration: "underline",
  },
  // The grouped pair of a session's videos (Stories 43.6, 44.1). The stage is
  // block-level and holds a row of track boxes with the one transport beneath,
  // because the pair is one player and a clock floating beside one of two
  // videos is read as that video's own controls.
  ".cm-lp-recording-stage": {
    display: "flex",
    flexDirection: "column",
    gap: "0.25em",
  },
  // Fit two tracks side by side, and wrap rather than crush them: `12rem` is
  // the width below which a video is no longer worth watching, so a pane too
  // narrow for two stacks them instead of showing two useless slivers.
  ".cm-lp-recording-tracks": {
    display: "flex",
    flexWrap: "wrap",
    alignItems: "flex-start",
    gap: "0.5em",
  },
  // The box is the answer to a mute slider that floated away from the track it
  // governs: a border is a boundary, and a control inside one is unambiguously
  // about what else is inside it.
  ".cm-lp-recording-track": {
    display: "flex",
    flexDirection: "column",
    gap: "0.25em",
    // `min-width: 0` because a flex item's default `min-width: auto` is its
    // content's intrinsic width, and a 1440-wide screen recording would push
    // the pair back out of the pane it was just fitted into.
    flex: "1 1 12rem",
    minWidth: "0",
    padding: "0.25em",
    borderRadius: "4px",
    border: "1px solid var(--border)",
  },
  // Inside a box the video fills the box rather than the pane, and gives up
  // some height, because two of them are on screen at once. The lone video's
  // rule above is untouched.
  ".cm-lp-recording-track .cm-lp-recording-player": {
    width: "100%",
    height: "auto",
    maxHeight: "40vh",
  },
  ".cm-lp-recording-transport": {
    display: "flex",
    flexDirection: "column",
    fontSize: "0.9em",
  },
  // The row that must not break. `nowrap` is the belt; the glyph labels are
  // the braces, and they are the part that actually holds — measured in a real
  // WKWebView, the shipped sentence labels laid this row out over five rows at
  // a 320 px pane whatever the wrapping rule said, because a button wider than
  // its container breaks its own text.
  ".cm-lp-recording-transport-row": {
    display: "flex",
    flexWrap: "nowrap",
    alignItems: "center",
    gap: "0.5em",
    padding: "0.25em 0",
  },
  ".cm-lp-recording-transport-toggle, .cm-lp-recording-transport-skip": {
    color: "var(--primary)",
    cursor: "pointer",
    // A control is a glyph and never a sentence, so it has no business
    // reflowing; `nowrap` says that of the glyph pairs too.
    whiteSpace: "nowrap",
  },
  ".cm-lp-recording-scrub": { flex: "1 1 4rem", minWidth: "3rem" },
  ".cm-lp-recording-time": {
    fontFamily: "var(--font-mono, ui-monospace, monospace)",
    color: "var(--muted-foreground)",
    // Tabular figures would still reflow as the minutes tick over in a
    // proportional fallback, and a readout that shifts the scrub bar sideways
    // once a second is unusable.
    fontVariantNumeric: "tabular-nums",
    whiteSpace: "nowrap",
  },
  // Its own line, below the controls: it is the one real sentence here
  // ("Playback was refused"), and a sentence on the control row is either a
  // wrapped row or a truncated message. Gone entirely while empty, which is
  // almost always, so the bar is one row high in the ordinary case.
  ".cm-lp-recording-transport-status": {
    color: "var(--muted-foreground)",
    fontSize: "0.9em",
  },
  ".cm-lp-recording-transport-status:empty": { display: "none" },
  // Volume and mute stay per track, so the mixer sits inside its own track's
  // box rather than on the shared bar (UX-DR53).
  ".cm-lp-recording-mix": {
    display: "flex",
    alignItems: "center",
    gap: "0.5em",
    fontSize: "0.85em",
  },
  ".cm-lp-recording-mix-mute": { color: "var(--primary)", cursor: "pointer" },
  // One glyph for both states: struck through is the muted one. Two symbols
  // would be two things to learn, and `aria-pressed` already carries the truth
  // for anyone not looking at it.
  '.cm-lp-recording-mix-mute[aria-pressed="true"]': {
    textDecoration: "line-through",
    color: "var(--muted-foreground)",
  },
  ".cm-lp-recording-mix-volume": { flex: "1 1 3rem", minWidth: "3rem", width: "auto" },
  // The gallery block (Story 44.15). The grid is the scroll container the
  // window measures, so its height is fixed here and its content is what
  // scrolls — a grid that grew with its folder would defeat the window it is
  // built on.
  ".cm-lp-gallery": {
    border: "1px solid var(--border)",
    borderRadius: "0.5rem",
    padding: "0.5rem",
    margin: "0.25rem 0",
  },
  ".cm-lp-gallery-head": {
    display: "flex",
    alignItems: "baseline",
    gap: "0.5rem",
    flexWrap: "wrap",
  },
  ".cm-lp-gallery-folder": { fontWeight: "600" },
  ".cm-lp-gallery-note": { color: "var(--muted-foreground)", fontSize: "0.85em" },
  ".cm-lp-gallery-pins": { display: "flex", gap: "0.5rem", flexWrap: "wrap" },
  ".cm-lp-gallery-grid": {
    position: "relative",
    overflowY: "auto",
    maxHeight: "24rem",
    marginTop: "0.5rem",
  },
  ".cm-lp-gallery-canvas": { position: "relative", width: "100%" },
  ".cm-lp-gallery-row": { display: "flex", gap: "0.5rem" },
  ".cm-lp-gallery-tile": {
    display: "flex",
    flexDirection: "column",
    gap: "0.25rem",
    margin: "0",
    overflow: "hidden",
  },
  // The pinned tiles float to the top of the grid; the outline is what says
  // WHY they are there rather than leaving the order looking arbitrary.
  '.cm-lp-gallery-tile[data-gallery-pinned="true"]': {
    outline: "2px solid var(--primary)",
    outlineOffset: "-2px",
    borderRadius: "0.25rem",
  },
  ".cm-lp-gallery-media": { width: "100%", height: "6.5rem", objectFit: "cover" },
  ".cm-lp-gallery-caption": {
    fontSize: "0.75em",
    color: "var(--muted-foreground)",
    overflow: "hidden",
    textOverflow: "ellipsis",
    whiteSpace: "nowrap",
  },
  ".cm-lp-gallery-pin": { fontSize: "0.75em", color: "var(--primary)", cursor: "pointer" },
  ".cm-lp-image-missing, .cm-mermaid-error-message": {
    color: "var(--muted-foreground)",
    fontSize: "0.85em",
  },
  ".cm-mermaid-error-source, .cm-mermaid-block": {
    fontFamily: "var(--font-mono, ui-monospace, monospace)",
    whiteSpace: "pre-wrap",
  },
  // # The note's two block columns (items 5 and 6 of the owner's report)
  //
  // A note has exactly two left edges and they are different on purpose.
  //
  // **The reading column.** Prose, a fence's code, a rendered table: things a
  // reader is READING. CodeMirror's `.cm-line` puts that column 6px in from the
  // content box on the left and 2px in on the right, so in a 520px pane whose
  // content box measures x=2..518 the column is **8..516**.
  //
  // **The pane.** A mounted panel — an embedded CSV, JSON or HTML file — is a
  // viewport onto ANOTHER document, not a paragraph of this one, so the reading
  // column around it is wasted margin. It gets the whole content box, **2..518**.
  //
  // What made both of these wrong was one fact about CodeMirror's DOM: a
  // `block: true` replacement's host is a direct child of `.cm-content`, a
  // SIBLING of the `.cm-line` elements and not inside one, so it inherits none
  // of the line's padding; an INLINE replacement's host is inside a `.cm-line`
  // and inherits all of it. The two rules below put each one where it belongs.
  ".cm-content > :not(.cm-line):not(.cm-widgetBuffer)": {
    // A block widget, indented to the reading column. Measured in Chromium in a
    // 520px pane: `.cm-md-table` was **2..518** against prose text at 8..516 —
    // the grid's first cell border sitting on the note's own border with no
    // breathing room, which is the report.
    //
    // Written as "every content child that is not a line" rather than per
    // widget, because the alternative is a list that the next block widget
    // somebody adds is missing from, and the defect it produces is invisible in
    // every DOM assertion. It covers the markdown table, the mermaid fence, the
    // gallery block and FR-264's widget today.
    //
    // `.cm-widgetBuffer` is excluded because it is not a block of the note:
    // it is the zero-width `<img>` CodeMirror draws beside an uneditable widget
    // to work around browser caret bugs. Indenting it would change nothing
    // visible, which is exactly why it is named — a rule that reads "every
    // block of the note except the lines" should be true rather than
    // accidentally true.
    //
    // The inset is on the WIDGET and never on `.cm-line`: the widget replaces
    // the source lines, and moving the caret in brings them back. Padding the
    // line instead would shift the paragraph sideways at the moment the caret
    // lands on it, which is a reflow a reader reads as the note jumping.
    marginLeft: "6px",
    marginRight: "2px",
  },
  // Story 45.12's embed host. `block`, because an inline replace is all a
  // `ViewPlugin` may supply (DW-165) and a panel wedged into a line of prose is
  // neither readable nor usable. The height is set on the body element by the
  // widget, from the same constant its `estimatedHeight` reports.
  //
  // The negative margins cancel `.cm-line`'s `padding: 0 2px 0 6px` exactly, so
  // the panel spans the content box instead of the reading column: measured in
  // Chromium, **8..516 before, 2..518 after**. BOTH sides, and that is the
  // point rather than a detail — cancelling only the left would leave the panel
  // flush on one edge and 2px short on the other, which is precisely the
  // asymmetry item 3 was reported for, reproduced one element over.
  ".cm-embed-block": {
    display: "block",
    marginLeft: "-6px",
    marginRight: "-2px",
  },
  ".cm-embed-body": {
    border: "1px solid var(--border)",
    borderRadius: "4px",
    // The panel's width comes from the pane and never from the file inside it.
    //
    // Its HEIGHT was already fixed by the widget; its width was not, and a
    // `max-content` CSV table propagated its own minimum straight up through this
    // panel to `.cm-content` — which is a flex item sized by its contents.
    // Measured in Chromium against the seven-column fixture in `dev/mock-shell`:
    // a 320px pane became a 1301px content box, so every line of prose in the
    // note re-laid out to 1301px and the panel's own `overflow-auto` had nothing
    // left to scroll, because it was as wide as the table it contained. Which is
    // the report: truncated at the pane's edge with no way to reach the rest.
    //
    // With the width contained, the panel is the pane's width and
    // `RawRenderedView`'s CSV pane is the scroll box it was always written to be.
    // The same rule and the same reason as `.cm-md-table-scroll`.
    contain: "inline-size",
  },
  // FR-264's widget host. `block` for the same reason the embed's is, and with
  // the same width containment: a board is four columns of cards and a card's
  // title is arbitrary text, so without this a long title would widen
  // `.cm-content` and re-lay-out every line of prose in the note.
  //
  // The height is deliberately NOT fixed, unlike `.cm-embed-body`. A widget
  // holds as many rows as the query selected, and a fixed box would put a
  // second scrollbar inside a document that already scrolls — the widget's
  // `estimatedHeight` is a hint to the height map, not the box.
  ".cm-note-widget": { display: "block" },
  ".cm-note-widget-body": {
    border: "1px solid var(--border)",
    borderRadius: "4px",
    contain: "inline-size",
  },
  // The degraded state: the callout as Obsidian would show it, which is what a
  // widget looks like before its panel arrives and what it stays as where
  // nothing can query. A quote, because that is what the source is.
  ".cm-note-widget-head": {
    borderLeft: "2px solid var(--border)",
    display: "flex",
    gap: "0.5em",
    padding: "0.25em 0.6em",
  },
  ".cm-note-widget-kind": { fontWeight: "600", textTransform: "uppercase" },
  ".cm-note-widget-argument": { color: "var(--muted-foreground)" },
  ".cm-note-widget-note": {
    color: "var(--muted-foreground)",
    display: "block",
    padding: "0 0.6em 0.25em 0.6em",
  },
  // `max-content`, deliberately: a CSV is a grid, and a grid that compressed
  // itself into a narrow pane would wrap every cell and stop being scannable.
  // The panel this is mounted in scrolls (`RawRenderedView`'s CSV pane is
  // `overflow-auto`), so wider than the pane means reachable, not lost.
  ".cm-csv-table": { borderCollapse: "collapse", fontSize: "0.9em", width: "max-content" },
  ".cm-csv-cell": {
    border: "1px solid var(--border)",
    padding: "0.15em 0.4em",
    textAlign: "left",
    // A cell is one line. A value with an embedded newline in it would
    // otherwise make one row as tall as the paragraph it is quoting, and the
    // row's height is what makes a table scannable.
    //
    // What is NOT here any more is the pair that went with it: `max-width: 24em`
    // with `overflow: hidden` and `text-overflow: ellipsis`. Those truncated by
    // construction — a 30-character path or a sentence in a cell was ellipsised
    // at 24em with NO way to read the rest of it, because the clip was on the
    // cell and the only scroll box is the panel outside the table. Ellipsis is
    // an honest affordance only when something can then be pressed or scrolled
    // to see the whole value, and there was nothing. The one-line rule is what
    // that pair was actually protecting, and `white-space: pre` above is the
    // whole of it: the cell is now as wide as its value, the table is as wide as
    // its widest cells, and the panel's own horizontal scroll is what reaches
    // the far edge of both.
    whiteSpace: "pre",
  },
  // A column the row does not have is drawn as absent, not as blank: hatching
  // says "there is nothing here" where an empty box says "this is empty", and
  // those are different files.
  ".cm-csv-missing": {
    border: "1px dashed var(--border)",
    backgroundColor: "color-mix(in oklch, var(--muted-foreground) 8%, transparent)",
  },
  ".cm-csv-ragged .cm-csv-cell": {
    borderColor: "color-mix(in oklch, var(--primary) 45%, var(--border))",
  },
  ".cm-csv-input": { font: "inherit", width: "100%", border: "none", background: "transparent" },
  ".cm-csv-notice": { color: "var(--muted-foreground)", fontSize: "0.85em" },
  ".cm-csv-error": { color: "var(--destructive)", fontSize: "0.85em" },
  // 1.2 s is long enough to notice and short enough not to become furniture.
  ".cm-lp-external": {
    backgroundColor: "color-mix(in oklch, var(--primary) 18%, transparent)",
    transition: `background-color ${EXTERNAL_FLASH_MS}ms ease-out`,
  },
});

/** The whole live-preview extension: decorations, the flash field and the theme. */
export function livePreview(options: LivePreviewOptions): Extension {
  const plugin = ViewPlugin.fromClass(
    class {
      decorations: DecorationSet;

      constructor(view: EditorView) {
        this.decorations = buildDecorations(view, options);
      }

      update(update: ViewUpdate): void {
        // Selection changes matter as much as document changes here: moving the
        // caret is what reveals and re-hides a line's source.
        if (update.docChanged || update.selectionSet || update.viewportChanged) {
          this.decorations = buildDecorations(update.view, options);
        }
      }
    },
    {
      decorations: (value) => value.decorations,
      eventHandlers: {
        mousedown(event: MouseEvent) {
          const target = event.target;
          if (!(target instanceof HTMLElement)) {
            return false;
          }
          const wiki = target.closest(`[${WIKILINK_ATTR}]`)?.getAttribute(WIKILINK_ATTR);
          const openNote = options.onOpenLink;
          if (wiki !== null && wiki !== undefined && openNote !== undefined) {
            event.preventDefault();
            openNote(wiki);
            return true;
          }
          // A host with no URL follower withholds the pressable class as well,
          // so this branch is not reachable by pointing at one — the guard is
          // here because an attribute outlives the re-render that dropped the
          // option, and because a keyboard or synthetic event does not consult
          // a cursor.
          const url = target.closest(`[${LINK_ATTR}]`)?.getAttribute(LINK_ATTR);
          const follow = options.onOpenUrl;
          if (url === null || url === undefined || follow === undefined) {
            return false;
          }
          event.preventDefault();
          follow(url);
          return true;
        },
      },
    },
  );
  return [
    plugin,
    galleryLayer({ list: options.listFolder }),
    noteWidgetLayer({ vaultId: options.vaultId, mount: options.mountWidget }),
    // The vault the table is in, so Story 56's two conversion controls
    // ("to CSV attachment" and back) have somewhere to write. Passed rather
    // than defaulted: without it both controls render permanently disabled,
    // which is a feature present in the code and unreachable in the product.
    tableLayer({ vaultId: options.vaultId }),
    mermaidLayer(),
    externalFlashField,
    livePreviewTheme,
  ];
}

/** The predicate reader, for the test that pins it against the Rust rules. */
export const __predicatesAfterForTest = predicatesAfter;
