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
 * rendering (raw HTML in a note body stays text — there is no HTML sink to
 * inject into), and any fetch of a remote image URL (a note must not be able to
 * become a tracking pixel).
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
import { CsvTableWidget, isCsvTarget } from "./csv-table";
import { galleryLayer } from "./gallery-block";
import { MermaidWidget } from "./mermaid-widget";
import { RecordingEmbedWidget } from "./recording-embed";
import { transportFor } from "./recording-transport";
import { WIKILINK, WIKILINK_ATTR } from "./wikilink";

/** How long an externally applied change stays highlighted. */
export const EXTERNAL_FLASH_MS = 1_200;

/** Syntax markers hidden on every line except the one being edited. */
const HIDDEN_MARKS: Record<string, true> = {
  EmphasisMark: true,
  StrongMark: true,
  CodeMark: true,
  HeaderMark: true,
  QuoteMark: true,
  LinkMark: true,
  StrikethroughMark: true,
  CodeInfo: true,
  URL: true,
};

/** Inline nodes that keep their text and gain a class. */
const INLINE_CLASSES: Record<string, string> = {
  Emphasis: "cm-lp-em",
  StrongEmphasis: "cm-lp-strong",
  InlineCode: "cm-lp-code",
  Strikethrough: "cm-lp-strike",
  Link: "cm-lp-link",
};

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
  /** Follow a wikilink. Called with the raw target, never a filesystem path. */
  onOpenLink: (target: string) => void;
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

/** The info string of a fenced block (`mermaid` in ` ```mermaid `), or "". */
function fenceInfo(view: EditorView, from: number, to: number): string {
  let info = "";
  syntaxTree(view.state).iterate({
    from,
    to,
    enter: (node) => {
      if (node.name === "CodeInfo") {
        info = view.state.doc.sliceString(node.from, node.to).trim();
        return false;
      }
      return undefined;
    },
  });
  return info;
}

function buildDecorations(view: EditorView, options: LivePreviewOptions): DecorationSet {
  const { doc } = view.state;
  const revealed = revealedLines(view);
  const decorations: Range<Decoration>[] = [];

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

  for (const visible of view.visibleRanges) {
    syntaxTree(view.state).iterate({
      from: visible.from,
      to: visible.to,
      enter: (node) => {
        // A gallery block is decorated by `galleryLayer` below and not here:
        // it replaces several lines with one element, and CodeMirror refuses
        // both a block decoration and a line-break-spanning replace from a
        // `ViewPlugin`. Nothing needs excluding at this point — the field's
        // replacement covers the whole callout, so the quote's line class and
        // the pins' wikilink marks fall inside a range nothing paints.

        const lineClass = LINE_CLASSES[node.name];
        if (lineClass !== undefined) {
          const first = doc.lineAt(node.from).number;
          const last = doc.lineAt(node.to).number;
          for (let line = first; line <= last; line += 1) {
            decorations.push(Decoration.line({ class: lineClass }).range(doc.line(line).from));
          }
        }

        if (node.name === "FencedCode") {
          const blockFrom = doc.lineAt(node.from).from;
          const blockTo = doc.lineAt(node.to).to;
          if (
            fenceInfo(view, node.from, node.to) === "mermaid" &&
            !isRevealed(node.from, node.to)
          ) {
            const source = doc.sliceString(doc.lineAt(node.from).to + 1, doc.lineAt(node.to).from);
            decorations.push(
              Decoration.replace({ widget: new MermaidWidget(source), block: true }).range(
                blockFrom,
                blockTo,
              ),
            );
            return false;
          }
          const first = doc.lineAt(node.from).number;
          const last = doc.lineAt(node.to).number;
          for (let line = first; line <= last; line += 1) {
            decorations.push(Decoration.line({ class: "cm-lp-fence" }).range(doc.line(line).from));
          }
          return undefined;
        }

        if (node.name === "Image" && !isRevealed(node.from, node.to)) {
          const raw = doc.sliceString(node.from, node.to);
          const match = /^!\[([^\]]*)]\(([^)\s]+)\)$/.exec(raw);
          // A remote URL is left as source on purpose: keeper never fetches one,
          // so a note cannot become a tracking pixel (NFR-11's egress claim).
          if (match && !/^[a-z][a-z0-9+.-]*:/i.test(match[2])) {
            decorations.push(
              Decoration.replace({
                widget: new ImageWidget(match[1], match[2], options.assetUrl),
              }).range(node.from, node.to),
            );
            return false;
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

          // `![[….csv]]`: the one embed syntax, rendered as the table Story
          // 44.16 adds. Checked before the recording branch because a session's
          // own files are video, audio and images — never a spreadsheet — so a
          // `.csv` in a recording note is an ordinary attachment like any
          // other, and a table is what it should be either way.
          if (match[0].startsWith("!") && isCsvTarget(target)) {
            // An INLINE replace with a block-styled host, not `block: true`.
            // These decorations come from a `ViewPlugin`, and CodeMirror refuses
            // a block decoration from one — the embed is a single line, so the
            // inline form is available and is the shape `RecordingEmbedWidget`
            // already uses. (The mermaid fence above asks for `block: true` from
            // this same plugin and throws for it; that is Story 37.8's to fix,
            // and the fix is moving this whole set into a `StateField`.)
            decorations.push(
              Decoration.replace({
                widget: new CsvTableWidget(options.vaultId, target),
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

export interface TextSplice {
  /** Start of the replaced span in the old text. */
  from: number;
  /** End of the replaced span in the old text. */
  to: number;
  /** What replaces it. */
  insert: string;
}

/**
 * The single minimal replacement that turns `before` into `after`, or null when
 * they are identical.
 *
 * Minimal matters twice over: CodeMirror maps the caret and the selection
 * through the change, so replacing only what actually moved is what keeps the
 * caret still when an agent appends a section somewhere else in the file; and
 * the same span is what gets the fading highlight, so the user sees where the
 * change landed rather than a whole-document flash.
 */
export function spliceBetween(before: string, after: string): TextSplice | null {
  if (before === after) {
    return null;
  }
  const shortest = Math.min(before.length, after.length);
  let start = 0;
  while (start < shortest && before[start] === after[start]) {
    start += 1;
  }
  let endBefore = before.length;
  let endAfter = after.length;
  while (endBefore > start && endAfter > start && before[endBefore - 1] === after[endAfter - 1]) {
    endBefore -= 1;
    endAfter -= 1;
  }
  return { from: start, to: endBefore, insert: after.slice(start, endAfter) };
}

/** Paint the fading highlight over a range, then let it go. */
export function flashExternal(view: EditorView, from: number, to: number): void {
  view.dispatch({ effects: flashExternalEffect.of({ from, to }) });
  setTimeout(() => {
    view.dispatch({ effects: clearExternalFlashEffect.of(null) });
  }, EXTERNAL_FLASH_MS);
}

const livePreviewTheme = EditorView.baseTheme({
  ".cm-lp-strong": { fontWeight: "600" },
  ".cm-lp-em": { fontStyle: "italic" },
  ".cm-lp-strike": { textDecoration: "line-through" },
  ".cm-lp-code": {
    fontFamily: "var(--font-mono, ui-monospace, monospace)",
    backgroundColor: "var(--muted)",
    borderRadius: "3px",
    padding: "0 3px",
  },
  ".cm-lp-link, .cm-lp-wikilink": { color: "var(--primary)", cursor: "pointer" },
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
  ".cm-lp-image img": { maxWidth: "100%", borderRadius: "4px" },
  // The host stays inline so an unresolved embed sits in its sentence like the
  // link it still is; a rendered video or image goes block, because either one
  // wedged into a line of prose is neither readable nor watchable. Audio does
  // too: a native transport bar is a couple of hundred pixels wide and its own
  // paragraph either way (Story 42.4, widened by Story 43.5).
  ".cm-lp-recording-player, .cm-lp-recording-image": {
    display: "block",
    maxWidth: "100%",
    maxHeight: "60vh",
    borderRadius: "4px",
    backgroundColor: "var(--muted)",
  },
  ".cm-lp-recording-audio": { display: "block", width: "100%", maxWidth: "24rem" },
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
  ".cm-csv-block": { display: "block", overflowX: "auto" },
  ".cm-csv-table": { borderCollapse: "collapse", fontSize: "0.9em", width: "max-content" },
  ".cm-csv-cell": {
    border: "1px solid var(--border)",
    padding: "0.15em 0.4em",
    textAlign: "left",
    // A cell is one line. A value with an embedded newline in it would
    // otherwise make one row as tall as the paragraph it is quoting, and the
    // row's height is what makes a table scannable.
    whiteSpace: "pre",
    maxWidth: "24em",
    overflow: "hidden",
    textOverflow: "ellipsis",
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
          const link = target.closest(`[${WIKILINK_ATTR}]`);
          const name = link?.getAttribute(WIKILINK_ATTR);
          if (name === null || name === undefined) {
            return false;
          }
          event.preventDefault();
          options.onOpenLink(name);
          return true;
        },
      },
    },
  );
  return [plugin, galleryLayer({ list: options.listFolder }), externalFlashField, livePreviewTheme];
}
