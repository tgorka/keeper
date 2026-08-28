/**
 * Three views of a note query, embedded in any note (FR-264): `> [!board]`,
 * `> [!log]`, `> [!refs]`.
 *
 * The operator's sentence is the whole specification: *"the trello like task
 * view should be the widget inside the md file I could use in the notes as well
 * - not only in the sessions"*, and then *"the log view, references view also
 * can be as a md widget view"*. So a board is not a sessions feature that a note
 * borrows; it is a way of drawing a query, and a session is one place that query
 * is worth drawing.
 *
 * **The syntax is Obsidian's callout, for `gallery-block.ts`'s reason and not a
 * new one.** Obsidian reads the same vault and will never render these, so the
 * block has to be worth reading as plain markdown:
 *
 * ```md
 * > [!board] tag:task path:projects/**
 * ```
 *
 * Obsidian renders that as a titled callout — a labelled quote whose title says
 * what keeper would have drawn. A fenced ` ```keeper-board ` block would degrade
 * to a grey box of keeper's configuration language instead. A callout also costs
 * no new grammar: an unknown callout type is specified to fall back to the
 * default style rather than to an error.
 *
 * **The marker set comes from Rust.** {@link WIDGET_KINDS} is checked against
 * the `ts-rs`-generated {@link WidgetKind} union by a `Record`, so a variant
 * added in `notes/widget.rs` and forgotten here is a type error, and a word
 * invented here that Rust does not know is one too. The pattern is then built
 * from that array. A regex spelling `board|log|refs` independently is the
 * version of this that goes stale silently.
 *
 * **Nothing here composes a query.** The text after the marker is handed to Rust
 * verbatim; Rust decides what an empty argument means and what a non-empty one
 * replaces (AD-65). That is also why a board in a note and a session's own board
 * cannot drift apart in what they select — there is one `effective_query`, and
 * it is not in this language.
 *
 * **A block decoration, therefore a `StateField`.** CodeMirror refuses
 * `block: true` from a `ViewPlugin` (DW-165), and a widget that replaced only
 * part of a line would put a four-column board inside a paragraph. This is the
 * same shape `galleryLayer` uses and it is composed into {@link livePreview}'s
 * extension array beside it, so a note still has one renderer.
 *
 * **This module imports no React** (NFR-27). The editor's chunk is lazily
 * imported and React-free; the panel arrives through a dynamic `import()` after
 * the callout's own text is already on screen, exactly as `file-embed.ts` does
 * it. See that module's header for why a static edge here would be expensive.
 */
import { type EditorState, type Extension, StateField, type Text } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, WidgetType } from "@codemirror/view";
import type { WidgetKind } from "@/lib/ipc/client";

/**
 * Every marker, checked against Rust's own set.
 *
 * The `Record<WidgetKind, true>` is the check: it refuses to compile if a
 * variant is missing, and refuses an excess key that names nothing in Rust. The
 * array is then derived from it, so the pattern below cannot spell a fourth
 * word.
 */
const KIND_SET: Record<WidgetKind, true> = { board: true, log: true, refs: true };

/** The three markers, in Rust's own order. */
export const WIDGET_KINDS = Object.keys(KIND_SET) as WidgetKind[];

/**
 * The first line of a widget block: the quote prefix, the marker, the argument.
 *
 * Case-insensitive because Obsidian's callout matching is — a `[!Board]` typed
 * by a person should not silently be a quotation.
 */
const HEAD = new RegExp(`^\\s{0,3}>[ \\t]?\\[!(${WIDGET_KINDS.join("|")})\\][ \\t]*(.*)$`, "i");

/** A line that belongs to a blockquote. Markdown's lazy continuation would also
 *  admit an unmarked line, and this deliberately does not: a widget block ends
 *  where its `>` markers end, so the paragraph typed underneath stays the
 *  reader's own. The same rule `gallery-block.ts` applies. */
const QUOTED = /^\s{0,3}>/;

/** The block host CodeMirror replaces the callout with. */
export const WIDGET_BLOCK_CLASS = "cm-note-widget";

/** The panel inside it, once React has arrived. Its presence is also the test
 *  {@link NoteWidgetWidget.ignoreEvent} uses, so a drag inside the board stays
 *  in the board while the degraded callout behaves like the quote it still is. */
export const WIDGET_BODY_CLASS = "cm-note-widget-body";

/**
 * What CodeMirror assumes a widget is tall until it has measured one.
 *
 * A hint and not a rule, unlike {@link EMBED_HEIGHT_PX} which is the box itself.
 * The difference is what is inside: a file embed holds a windowed list that
 * measures its own viewport to decide how many rows to mount, so an auto height
 * would make it mount everything — a widget holds a board or a short list that
 * is as tall as its rows, and fixing that would put an inner scrollbar inside a
 * document that already scrolls.
 */
export const WIDGET_ESTIMATED_HEIGHT_PX = 240;

/** What a widget says when nothing here can run a query — a note previewed
 *  outside a vault, or a renderer driven by a test with no loader. Said rather
 *  than left blank: a surface that quietly shows nothing is indistinguishable
 *  from a query that selected nothing. */
export const WIDGET_NO_HOST = "keeper is not drawing widgets here.";

/** What a widget block says, once its head line has been read. */
export interface NoteWidgetBlock {
  kind: WidgetKind;
  /** The callout's own text, verbatim and unparsed. Empty when the callout names
   *  no query, which Rust reads as the kind's default. */
  argument: string;
}

/** Read a widget callout, or decide this blockquote is not one. */
export function parseWidgetBlock(text: string): NoteWidgetBlock | null {
  const first = text.split("\n", 1)[0] ?? "";
  const head = HEAD.exec(first.endsWith("\r") ? first.slice(0, -1) : first);
  if (head === null) {
    return null;
  }
  // Lowercased on the way in, because Rust's `WidgetKind` is a lowercase union
  // and `[!Board]` must name the same widget `[!board]` does.
  return { kind: head[1].toLowerCase() as WidgetKind, argument: head[2].trim() };
}

/** How the widget reaches its panel. Injected so a test can drive the mount
 *  without a Tauri host, exactly as `FileEmbedOptions.mount` is. */
export interface NoteWidgetOptions {
  /** The open vault's id. Empty — a markdown file previewed outside a vault —
   *  means there is no vault to query, and the widget says so rather than
   *  asking Rust about a vault that does not exist. */
  vaultId?: string;
  /** Replace the dynamic import of the React panel. */
  mount?: (
    container: HTMLElement,
    args: { vaultId: string; kind: WidgetKind; argument: string },
  ) => { unmount: () => void };
  /** Whether the host has been torn down since the render began. */
  cancelled?: () => boolean;
}

/** The callout, as plain text: what a widget shows before React arrives and
 *  what it stays as when there is no host. This is close to what Obsidian
 *  shows, which is the point. */
function head(block: NoteWidgetBlock): HTMLElement {
  const bar = document.createElement("div");
  bar.className = "cm-note-widget-head";
  const marker = document.createElement("span");
  marker.className = "cm-note-widget-kind";
  marker.textContent = block.kind;
  bar.append(marker);
  const argument = document.createElement("span");
  argument.className = "cm-note-widget-argument";
  argument.textContent = block.argument;
  bar.append(argument);
  return bar;
}

/**
 * The CodeMirror widget that replaces a widget callout with its panel.
 *
 * One class for all three kinds: which rows are selected and how they are drawn
 * is decided in Rust and in the React panel respectively, and a widget class per
 * kind would be three copies of this mounting dance.
 */
export class NoteWidgetWidget extends WidgetType {
  /** Set by {@link destroy}, read by the import that may still be in flight. */
  private disposed = false;

  /** The mounted panel, so it can be taken down with the widget. */
  private panel: { unmount: () => void } | null = null;

  constructor(
    private readonly block: NoteWidgetBlock,
    /** The block's exact source. Two blocks that read the same are the same
     *  widget; this is what {@link eq} compares, and it is what keeps a board
     *  mounted — with its drag state — while the caret moves around the note. */
    private readonly source: string,
    private readonly options: NoteWidgetOptions = {},
  ) {
    super();
  }

  eq(other: NoteWidgetWidget): boolean {
    return other.source === this.source;
  }

  /** So CodeMirror's height map does not have to discover the panel's size by
   *  measuring it, which it cannot do before the panel exists. */
  get estimatedHeight(): number {
    return WIDGET_ESTIMATED_HEIGHT_PX;
  }

  toDOM(): HTMLElement {
    const host = document.createElement("div");
    host.className = WIDGET_BLOCK_CLASS;
    // The callout's own text is in the document immediately and the panel takes
    // its place when React and the rows have arrived. Blocking `toDOM` on an
    // import and an IPC round trip would stall the editor on every keystroke
    // that rebuilds the decorations.
    host.append(head(this.block));
    void this.open(host);
    return host;
  }

  private async open(host: HTMLElement): Promise<void> {
    const vaultId = this.options.vaultId ?? "";
    if (vaultId === "") {
      const note = document.createElement("span");
      note.className = "cm-note-widget-note";
      note.textContent = WIDGET_NO_HOST;
      host.append(note);
      return;
    }
    // Dynamic on purpose, and a static import cannot work here: this module is
    // reached from `live-preview.ts`, which `note-editor.tsx` imports lazily to
    // keep the editor's chunk free of React (NFR-27). See the module header.
    const mount = this.options.mount ?? (await import("./note-widget-host")).mountNoteWidget;
    if (this.disposed || this.options.cancelled?.() === true) {
      return;
    }
    const body = document.createElement("div");
    body.className = WIDGET_BODY_CLASS;
    host.replaceChildren(body);
    // Synchronous, so the panel is either mounted or `this.disposed` was already
    // true — there is no window in which a widget CodeMirror has torn down
    // leaves a React root attached to a detached node.
    this.panel = mount(body, {
      vaultId,
      kind: this.block.kind,
      argument: this.block.argument,
    });
  }

  destroy(): void {
    this.disposed = true;
    const panel = this.panel;
    this.panel = null;
    if (panel === null) {
      return;
    }
    // A microtask, because this runs while CodeMirror is updating its DOM and
    // that can itself be inside a React commit — the note editor unmounting
    // tears the view down from an effect cleanup. Unmounting a root while React
    // is rendering is refused with a warning and leaves the tree attached.
    queueMicrotask(() => {
      panel.unmount();
    });
  }

  /**
   * Keep the events aimed at the panel.
   *
   * `true` means CodeMirror ignores the event entirely, which is what keeps the
   * caret off the block's lines — and a revealed block drops its decorations, so
   * without this, starting a drag would un-render the board mid-gesture and
   * pressing the column menu would destroy it rather than use it. The same trade
   * `GalleryWidget` and `FileEmbedWidget` make for their controls.
   */
  ignoreEvent(event: Event): boolean {
    return (
      event.target instanceof Element && event.target.closest(`.${WIDGET_BODY_CLASS}`) !== null
    );
  }
}

/** One widget block found in a document. */
interface WidgetHit {
  from: number;
  to: number;
  text: string;
  block: NoteWidgetBlock;
}

/**
 * Every widget block in the document, found by scanning lines.
 *
 * By line rather than through the syntax tree, for `galleryHits`'s reason: this
 * runs in a `StateField`, a field has no view to ask for visible ranges, and the
 * two questions a widget asks — is this line the callout head, does the quote
 * continue — are exactly the two a line answers by itself.
 *
 * A head line inside an existing blockquote is not a head: `> [!board]` on the
 * second line of somebody's quotation is part of that quotation, and starting a
 * block there would swallow the lines above it.
 */
function widgetHits(doc: Text): WidgetHit[] {
  const hits: WidgetHit[] = [];
  let line = 1;
  while (line <= doc.lines) {
    const first = doc.line(line);
    const opens = line === 1 || !QUOTED.test(doc.line(line - 1).text);
    if (opens && HEAD.test(first.text)) {
      let last = line;
      while (last < doc.lines && QUOTED.test(doc.line(last + 1).text)) {
        last += 1;
      }
      const to = doc.line(last).to;
      const text = doc.sliceString(first.from, to);
      const block = parseWidgetBlock(text);
      if (block !== null) {
        hits.push({ from: first.from, to, text, block });
        line = last + 1;
        continue;
      }
    }
    line += 1;
  }
  return hits;
}

/** The decorations for `hits`, minus any block the selection is inside. */
function widgetDecorationSet(
  hits: readonly WidgetHit[],
  state: EditorState,
  options: NoteWidgetOptions,
): DecorationSet {
  const decorations = [];
  for (const hit of hits) {
    // The renderer's own reveal rule, applied to a whole block: put the caret
    // anywhere in a widget and its source comes back, so the marker and the
    // query stay editable as the text they are.
    const revealed = state.selection.ranges.some(
      (range) => range.from <= hit.to && range.to >= hit.from,
    );
    if (revealed) {
      continue;
    }
    decorations.push(
      Decoration.replace({
        widget: new NoteWidgetWidget(hit.block, hit.text, options),
        block: true,
      }).range(hit.from, hit.to),
    );
  }
  return Decoration.set(decorations, true);
}

/**
 * The widget layer, as a `StateField` rather than as part of the renderer's
 * `ViewPlugin` — `galleryLayer`'s shape, for `galleryLayer`'s reason (DW-165).
 *
 * The scan is doc-driven and the reveal is selection-driven, and they are
 * separated: moving the caret rebuilds the decoration set from the blocks
 * already found, and only an edit re-scans the document.
 */
export function noteWidgetLayer(options: NoteWidgetOptions = {}): Extension {
  return StateField.define<{ hits: WidgetHit[]; decorations: DecorationSet }>({
    create(state) {
      const hits = widgetHits(state.doc);
      return { hits, decorations: widgetDecorationSet(hits, state, options) };
    },
    update(value, transaction) {
      if (!transaction.docChanged && transaction.selection === undefined) {
        return value;
      }
      const hits = transaction.docChanged ? widgetHits(transaction.state.doc) : value.hits;
      return { hits, decorations: widgetDecorationSet(hits, transaction.state, options) };
    },
    provide: (field) => EditorView.decorations.from(field, (value) => value.decorations),
  });
}
