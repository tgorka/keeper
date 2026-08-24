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
 * The predicates a link carries, drawn as chips in place of their braces.
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
  constructor(private readonly predicates: readonly string[]) {
    super();
  }

  eq(other: PredicateWidget): boolean {
    return (
      other.predicates.length === this.predicates.length &&
      other.predicates.every((predicate, at) => predicate === this.predicates[at])
    );
  }

  toDOM(): HTMLElement {
    const chips = document.createElement("span");
    chips.className = "cm-lp-predicates";
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
 * A predicate with no prefix — the legacy `reference` value — gets one text
 * node and no spans, so its rendering is untouched by all of this.
 */
function predicateChip(predicate: string): HTMLElement {
  const chip = document.createElement("span");
  chip.className = "cm-lp-predicate";
  // Named for a screen reader, which gets the link and then this and would
  // otherwise hear a bare word with no relationship to what precedes it.
  chip.setAttribute("aria-label", `link kind: ${predicate}`);
  const colon = predicate.indexOf(":");
  if (colon === -1) {
    chip.textContent = predicate;
    return chip;
  }
  const prefix = document.createElement("span");
  prefix.className = "cm-lp-predicate-prefix";
  prefix.textContent = predicate.slice(0, colon + 1);
  const local = document.createElement("span");
  local.className = "cm-lp-predicate-local";
  local.textContent = predicate.slice(colon + 1);
  chip.append(prefix, local);
  return chip;
}

/**
 * A CURIE, which is what makes a token a predicate rather than a word.
 *
 * `prefix:local`, both halves `[A-Za-z][A-Za-z0-9_-]*`. The same shape the Rust
 * side tests, and it has to stay the same: one syntax read twice, once to draw
 * it and once to put it in the graph, and two readings of one syntax is how a
 * note comes to show a relationship that no query can find.
 */
const CURIE = /^[A-Za-z][A-Za-z0-9_-]*:[A-Za-z][A-Za-z0-9_-]*$/;

/** The one attribute key that has ever meant "predicate" in this codebase. */
const REFERENCE_KEY = "reference";

/**
 * One attribute block after a link: `{schema:creator, rel="cites"}`.
 *
 * Sticky rather than anchored so a run of ADJACENT blocks can be scanned
 * without slicing the document window once per block. One line: a stray brace
 * must not be able to swallow the rest of a note.
 */
const LINK_ATTRS = /\{([^}\n]*)\}/y;

/**
 * One token inside a block: a `key="value"` pair, or anything not a separator.
 *
 * The pair alternative comes first so a value containing a comma or a space
 * stays one token. Commas and whitespace are the separators, interchangeably —
 * `{a:b, c:d}` and `{a:b c:d}` are the same two predicates.
 */
const ATTR_TOKEN = /[A-Za-z][A-Za-z0-9_-]*\s*=\s*"[^"\n]*"|[^\s,]+/g;

/** `key="value"`, as a whole token. */
const ATTR_PAIR = /^([A-Za-z][A-Za-z0-9_-]*)\s*=\s*"([^"\n]*)"$/;

/** One block of a run, and what its tokens turned out to be. */
interface AttrBlock {
  /** Offsets of `{`…`}`, relative to the start of the run. */
  from: number;
  to: number;
  /** The chips this block draws: its predicates, minus any an earlier block in
   *  the run already drew. Empty when every one of them was a repeat. */
  chips: string[];
  /** Whether the block wrote a predicate at all, repeat or not. A block that
   *  wrote none is not a predicate block and keeps its source. */
  writesPredicate: boolean;
  /** A token that is neither a CURIE nor `key="value"`. Nothing in the block is
   *  decorated when one is present — see the call site. */
  junk: boolean;
}

/**
 * Every attribute block written straight after a link, in order.
 *
 * The markdown parser has never heard of these, so they arrive as ordinary text
 * after the link node and this reads them off the document directly. Same rules
 * as the Rust side, and they have to stay the same: no space before the brace,
 * a quoted value for a pair, one line, and adjacent blocks merged in order with
 * exact duplicates dropped.
 */
function predicatesAfter(text: string): { blocks: AttrBlock[]; length: number } | null {
  const blocks: AttrBlock[] = [];
  const seen = new Set<string>();
  let at = 0;
  for (;;) {
    LINK_ATTRS.lastIndex = at;
    const block = LINK_ATTRS.exec(text);
    if (block === null) {
      break;
    }
    const chips: string[] = [];
    let writesPredicate = false;
    let junk = false;
    for (const token of (block[1] ?? "").matchAll(ATTR_TOKEN)) {
      const pair = ATTR_PAIR.exec(token[0]);
      if (pair === null && !CURIE.test(token[0])) {
        // A token keeper cannot read. Guessing at one is how a graph comes to
        // hold a relationship nobody wrote, so it is recorded and the whole
        // block keeps its source — see the call site.
        junk = true;
        continue;
      }
      // A CURIE is its own predicate; `reference="cites"` names one in the
      // spelling keeper shipped first. Any other pair — `rel="cites"`, which
      // the vault's own toolkit writes — is an attribute and not a predicate.
      const predicate = pair === null ? token[0] : (pair[2] ?? "");
      if (pair !== null && (pair[1] !== REFERENCE_KEY || predicate === "")) {
        continue;
      }
      writesPredicate = true;
      if (!seen.has(predicate)) {
        seen.add(predicate);
        chips.push(predicate);
      }
    }
    blocks.push({ from: at, to: at + block[0].length, chips, writesPredicate, junk });
    at += block[0].length;
  }
  return blocks.length === 0 ? null : { blocks, length: at };
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
          // The attribute blocks written straight after the link. The parser
          // has never heard of them, so they are plain text sitting after this
          // node and have to be read off the document.
          //
          // 200 characters is the window, which is generous for a run of
          // predicates and short enough that a note full of links does not
          // become a note full of string copies.
          const trailing = predicatesAfter(doc.sliceString(node.to, node.to + 200));
          if (trailing !== null && !isRevealed(node.to, node.to + trailing.length)) {
            for (const block of trailing.blocks) {
              // Two blocks stay exactly as the author typed them, and both
              // rules are about not hiding text nobody can see hidden:
              //
              //   - one with a token keeper cannot read, because replacing it
              //     with a chip would show the tokens keeper DID understand and
              //     silently swallow the one the author needs to fix;
              //   - one that writes no predicate at all — `{rel="cites"}`,
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
              decorations.push(
                Decoration.replace({
                  widget: new PredicateWidget(block.chips),
                }).range(node.to + block.from, node.to + block.to),
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
  ".cm-lp-fence": {
    fontFamily: "var(--font-mono, ui-monospace, monospace)",
    backgroundColor: "var(--muted)",
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
  // A CURIE's two halves. The vocabulary is quieter than the term it qualifies,
  // by weight and not by colour: see `predicateChip` for the two ratios that
  // decided that. `500` rather than the `600` the headings use — a chip that
  // out-weighed the prose around it would stop being a label.
  ".cm-lp-predicate-local": { fontWeight: "500" },
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
  // Story 45.12's embed host. `block`, because an inline replace is all a
  // `ViewPlugin` may supply (DW-165) and a panel wedged into a line of prose is
  // neither readable nor usable. The height is set on the body element by the
  // widget, from the same constant its `estimatedHeight` reports.
  ".cm-embed-block": { display: "block" },
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
    tableLayer(),
    mermaidLayer(),
    externalFlashField,
    livePreviewTheme,
  ];
}

/** The predicate reader, for the test that pins it against the Rust rules. */
export const __predicatesAfterForTest = predicatesAfter;
