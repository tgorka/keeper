/**
 * A gallery of a folder, rendered inside a note (Story 44.15, FR-171, AD-84,
 * AD-65, FR-145).
 *
 * A note can already embed one file at a time. What it cannot do is stand over
 * a folder — a shoot, a scan batch, a trip — and show it, and a vault that
 * holds four hundred photographs beside the note about them is the ordinary
 * case rather than the extreme one. This module is that block: its syntax, the
 * pins it carries, and the windowed grid it becomes.
 *
 * **The syntax is Obsidian's own callout, and that decision is the whole of the
 * legibility promise.** Obsidian reads the same vault and will never render
 * this widget, so the block has to be worth reading as plain markdown:
 *
 * ```md
 * > [!gallery] Photos/Trip
 * > [[Photos/Trip/hero.jpg]]
 * > [[Photos/Trip/map.png]]
 * ```
 *
 * Obsidian renders that as a titled callout containing two working links: the
 * folder is named, the pinned items are reachable in one click, and nothing is
 * broken. The obvious alternative — a fenced ` ```keeper-gallery ` block, which
 * is what {@link MermaidWidget}'s neighbour would suggest — degrades to a grey
 * box of source code with `folder:` and `pin:` keys in it. That is legible, and
 * it is strictly worse: the reader gets keeper's configuration language instead
 * of their own photographs, and the links do not work. `[!gallery]` also costs
 * no new grammar. A blockquote is a blockquote, `[!type]` is Obsidian's
 * established callout marker, and an unknown callout type is specified to fall
 * back to the default style rather than to an error.
 *
 * **A pin is a link, not an embed.** `![[hero.jpg]]` would make Obsidian show
 * the pinned images inline, which reads better for three pins and lies about
 * what the note contains: the gallery is over a folder of hundreds and the note
 * holds none of them. `[[hero.jpg]]` says exactly the true thing — *these are
 * the ones this note singles out* — and costs Obsidian no decode.
 *
 * **Pins live in the NOTE and there is nowhere else they could go.** Two notes
 * over one folder pin different things, and a pin written into the folder —
 * a dotfile, a sidecar, an order key — would be one note editing another note's
 * view of the same photographs. It would also be a write, and the vault is the
 * user's (FR-121). So a pin is a line of the block, a toggle is a one-line
 * splice into the note, and every other byte of the block is left alone the way
 * `Frontmatter` leaves a key it did not touch.
 *
 * **No path is composed here.** The folder is the callout's title, handed to
 * Rust verbatim; every item's vault-relative path and every item's
 * `keeper-note://` URL are composed in Rust and echoed back (AD-65). A pin
 * writes the path Rust produced, which is vault-relative and therefore
 * resolvable by Obsidian and never absolute (FR-145).
 *
 * **The window is 44.10's window.** {@link rowOffsets} and {@link windowSlice}
 * are the same arithmetic the notes list, the recordings list and the files
 * tree run; a grid row is a row like any other, so a folder of four hundred
 * mounts a screen of tiles and not four hundred. What this module supplies on
 * top is only the DOM binding, because a CodeMirror widget lives in the
 * editor's React-free lazy chunk and cannot call a hook — and because a grid of
 * uniform tiles needs no measurement, so the hook's ResizeObserver half has
 * nothing to do here.
 */
import { type EditorState, type Extension, StateField, type Text } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, WidgetType } from "@codemirror/view";
import {
  rowOffsets,
  WINDOW_ROW_ATTR,
  WINDOW_VIEWPORT_ATTR,
  windowSlice,
} from "@/components/ui/window-list";
import type { NoteGalleryItemVm, NoteGalleryVm } from "@/lib/ipc/client";
import { releaseRecordingMedia } from "./recording-embed";
import { primeFirstFrame } from "./recording-transport";
import { WIKILINK_ATTR } from "./wikilink";

/**
 * The first line of a gallery block: the quote prefix a spliced line must copy,
 * then the folder.
 *
 * `[!gallery]` is spelled here and nowhere else, and matched case-insensitively
 * because Obsidian's own callout matching is — a `[!Gallery]` typed by a person
 * should not silently be a quotation.
 */
const HEAD = /^(\s{0,3}>[ \t]?)\[!gallery\][ \t]*(.*)$/i;

/** A pin line: an ordinary wikilink, alone on its line inside the quote. */
const PIN = /^[ \t]*\[\[([^\]|]+?)(?:\|[^\]]*)?\]\][ \t]*$/;

/** A line that belongs to a blockquote. Markdown's lazy continuation would also
 *  admit an unmarked line, and this deliberately does not: a gallery block ends
 *  where its `>` markers end, so the paragraph a user types underneath is
 *  theirs and not swallowed into the widget. */
const QUOTED = /^\s{0,3}>/;

/** Pin / unpin, one wording. A tile says which state pressing it produces, the
 *  way every other toggle in the repo does. */
export const GALLERY_PIN_LABEL = "Pin";

/** The same control once the item is pinned. */
export const GALLERY_UNPIN_LABEL = "Unpin";

/** What a gallery says when nothing here can list a folder — a note rendered
 *  outside the editor, or a widget driven by a test with no loader. Said rather
 *  than left blank: a surface that quietly shows nothing is indistinguishable
 *  from an empty folder. */
export const GALLERY_NO_LISTING = "keeper is not listing this folder here.";

/** What a gallery says when the listing call itself failed — the host went
 *  away, IPC rejected. Distinct from the sentences Rust composes about the
 *  FOLDER, which are about a folder that was really looked at. */
export const GALLERY_UNREACHABLE = "this folder could not be listed just now.";

/**
 * A tile's box, including its caption and its controls.
 *
 * Fixed rather than measured, and that is the difference between this binding
 * and the hook's. Every tile in a grid is the same size by construction — the
 * media is letterboxed into a square well — so there is nothing for a
 * measurement to discover, and a measured grid would pay a ResizeObserver per
 * tile to be told the number that is written here.
 */
export const GALLERY_TILE_HEIGHT = 168;

/** A tile's minimum width. The column count is the viewport divided by this, so
 *  a narrow pane shows two columns and a wide one shows eight without either
 *  being configured. */
export const GALLERY_TILE_WIDTH = 160;

/** Space between tiles, folded into each row's box the way the hook folds it. */
const GALLERY_GAP = 8;

/** Rows kept mounted past each edge, so a flick does not expose blank space.
 *  Two rows rather than the list's six: a row here is a whole line of media
 *  elements, and each one off screen is a metadata fetch nobody asked for. */
const GALLERY_OVERSCAN = 2;

/** The grid's height until layout reports its own — about four rows. jsdom
 *  never lays anything out, so a test that arranges no geometry still gets a
 *  window with tiles in it rather than an empty grid. */
const ASSUMED_GRID_HEIGHT = 480;

/** The grid's width until layout reports its own, on the same rule. */
const ASSUMED_GRID_WIDTH = 640;

/** What a gallery block says, once its source has been read. */
export interface GalleryBlock {
  /** The vault-relative folder the callout's title names. `""` when the block
   *  names none, which is a block that can never list anything and says so. */
  folder: string;
  /** The pinned items' vault-relative paths, in the order the note lists them —
   *  which is the order they float to the top in. */
  pins: string[];
}

/** Strip one blockquote marker, leaving the line's own content. */
function unquote(line: string): string {
  const marker = /^\s{0,3}>[ \t]?/.exec(line);
  return marker === null ? line : line.slice(marker[0].length);
}

/** The lines of `text`, each without its carriage return, plus whether the
 *  block used CRLF — so a rewrite puts back exactly what it took out. */
function linesOf(text: string): { lines: string[]; cr: string } {
  const raw = text.split("\n");
  const cr = raw.some((line) => line.endsWith("\r")) ? "\r" : "";
  return { lines: raw.map((line) => (line.endsWith("\r") ? line.slice(0, -1) : line)), cr };
}

/**
 * Read a gallery block, or decide this blockquote is not one.
 *
 * Everything that is not the head line and not a lone wikilink is ignored
 * rather than refused: a person may write a sentence inside the callout, and a
 * gallery that stopped being a gallery because of it would be a syntax people
 * learn to fear. Those lines survive every rewrite below untouched.
 */
export function parseGalleryBlock(text: string): GalleryBlock | null {
  const { lines } = linesOf(text);
  const head = HEAD.exec(lines[0] ?? "");
  if (head === null) {
    return null;
  }
  const pins: string[] = [];
  for (const line of lines.slice(1)) {
    const pin = PIN.exec(unquote(line));
    if (pin !== null) {
      pins.push(pin[1].trim());
    }
  }
  return { folder: head[2].trim().replace(/\/+$/, ""), pins };
}

/**
 * The block text with `relPath` pinned, or unchanged if it already is.
 *
 * The new line goes after the last pin, so the note's pin order is the order
 * they were pinned in and a new pin does not jump the queue. Everything else is
 * byte-identical, including the lines this module does not understand: a pin is
 * a one-line splice and never a re-serialisation of the block.
 */
export function withPin(text: string, relPath: string): string {
  const { lines, cr } = linesOf(text);
  const block = parseGalleryBlock(text);
  if (block === null || block.pins.includes(relPath)) {
    return text;
  }
  const prefix = HEAD.exec(lines[0])?.[1] ?? "> ";
  let at = 0;
  for (let index = 1; index < lines.length; index += 1) {
    if (PIN.test(unquote(lines[index]))) {
      at = index;
    }
  }
  lines.splice(at + 1, 0, `${prefix}[[${relPath}]]`);
  return lines.map((line) => line + cr).join("\n");
}

/** The block text with `relPath` unpinned. Removes the one line that names it
 *  and nothing else; a path that is not pinned leaves the text alone. */
export function withoutPin(text: string, relPath: string): string {
  const { lines, cr } = linesOf(text);
  const at = lines.findIndex((line, index) => {
    const pin = index === 0 ? null : PIN.exec(unquote(line));
    return pin !== null && pin[1].trim() === relPath;
  });
  if (at === -1) {
    return text;
  }
  lines.splice(at, 1);
  return lines.map((line) => line + cr).join("\n");
}

/** Where the gallery block containing `pos` starts and ends, and its source.
 *  `null` when the lines around `pos` are not a gallery — which is what a stale
 *  position looks like, and is why the caller re-reads rather than remembering
 *  the range it was constructed with. */
export function galleryRangeAt(
  doc: Text,
  pos: number,
): { from: number; to: number; text: string } | null {
  const at = doc.lineAt(Math.max(0, Math.min(pos, doc.length)));
  if (!QUOTED.test(at.text)) {
    return null;
  }
  let first = at.number;
  while (first > 1 && QUOTED.test(doc.line(first - 1).text)) {
    first -= 1;
  }
  let last = at.number;
  while (last < doc.lines && QUOTED.test(doc.line(last + 1).text)) {
    last += 1;
  }
  const from = doc.line(first).from;
  const to = doc.line(last).to;
  const text = doc.sliceString(from, to);
  return parseGalleryBlock(text) === null ? null : { from, to, text };
}

/**
 * The items a gallery shows, pinned first, and what it had to leave out.
 *
 * A pin naming something that is not in the folder is counted rather than
 * dropped from the note: the file may be on a volume that is out, or renamed
 * outside keeper, and silently rewriting somebody's note to agree with a
 * listing taken one second ago is the one unrecoverable thing this surface
 * could do.
 */
export function galleryOrder(
  items: readonly NoteGalleryItemVm[],
  pins: readonly string[],
): { shown: NoteGalleryItemVm[]; skipped: number; missingPins: number } {
  // A tile is offered for exactly the kinds Rust composed a URL for, which is
  // the set `keeper-note://` will serve. Testing the URL rather than
  // re-deciding the kind here is what keeps the classifier singular (AD-73):
  // a file with no URL has no element that could load it, so a tile for it
  // would be the dead player Story 42.6 refuses.
  const media = items.filter((item) => item.url !== null);
  const byPath = new Map(media.map((item) => [item.relPath, item]));
  const shown: NoteGalleryItemVm[] = [];
  const taken = new Set<string>();
  let missingPins = 0;
  for (const pin of pins) {
    const item = byPath.get(pin);
    if (item === undefined || taken.has(pin)) {
      missingPins += 1;
      continue;
    }
    taken.add(pin);
    shown.push(item);
  }
  for (const item of media) {
    if (!taken.has(item.relPath)) {
      shown.push(item);
    }
  }
  return { shown, skipped: items.length - media.length, missingPins };
}

/** The sentence under a gallery's folder name: what it is showing, and what it
 *  is not. Only the true clauses appear, so an ordinary folder gets a count and
 *  nothing else to read. */
export function gallerySummary(counts: {
  shown: number;
  skipped: number;
  missingPins: number;
  truncated: boolean;
}): string {
  const parts = [counts.shown === 1 ? "1 item" : `${counts.shown} items`];
  if (counts.skipped > 0) {
    parts.push(
      counts.skipped === 1
        ? "1 file is not media and is not shown"
        : `${counts.skipped} files are not media and are not shown`,
    );
  }
  if (counts.missingPins > 0) {
    parts.push(
      counts.missingPins === 1
        ? "1 pinned item is not in this folder"
        : `${counts.missingPins} pinned items are not in this folder`,
    );
  }
  if (counts.truncated) {
    parts.push("this folder holds more than the listing shows");
  }
  return parts.join(" · ");
}

/** How the widget reaches the vault. Injected so the degrade paths — which are
 *  the interesting ones — are reachable in a test without a Tauri host. */
export type GalleryLoader = (folder: string) => Promise<NoteGalleryVm>;

export interface GalleryOptions {
  /** Overridden in tests; production always asks Rust. Absent, every gallery
   *  stays the head and its pinned links, and says so. */
  list?: GalleryLoader;
  /** Whether the host has been torn down since the render began. */
  cancelled?: () => boolean;
}

/** What a mounted gallery keeps, so a pin toggle can re-order the tiles it
 *  already has instead of listing the folder again. Held off the DOM node so a
 *  destroyed widget takes it with it. */
interface GalleryMount {
  /** The folder the mounted tiles came from. A widget over a DIFFERENT folder
   *  must not adopt them, which is the whole of {@link GalleryWidget.updateDOM}'s
   *  test. */
  folder: string;
  repaint: (pins: readonly string[]) => void;
  release: () => void;
}

const mounts = new WeakMap<HTMLElement, GalleryMount>();

/** The ordinary wikilink a pin degrades to — the same element the renderer
 *  builds for `[[…]]` anywhere else, so clicking it follows the link. */
function pinLink(target: string): HTMLElement {
  const anchor = document.createElement("span");
  anchor.className = "cm-lp-wikilink";
  anchor.setAttribute(WIKILINK_ATTR, target);
  // `textContent`, never `innerHTML`: a note body is agent-authorable text.
  anchor.textContent = target;
  return anchor;
}

/** The element a tile's kind asks for. Mirrors `recording-embed`'s branch, and
 *  for the same reason: the kind decides WHICH element there is, and Rust
 *  decided the kind. */
function mediaElement(item: NoteGalleryItemVm, url: string): HTMLElement {
  if (item.kind === "image") {
    const image = document.createElement("img");
    image.className = "cm-lp-gallery-media";
    // Lazily, because a window of tiles is still more images than a reader
    // looks at while scrolling past.
    image.loading = "lazy";
    image.alt = item.name;
    image.src = url;
    return image;
  }
  if (item.kind === "audio") {
    const audio = document.createElement("audio");
    audio.className = "cm-lp-gallery-media";
    audio.controls = true;
    audio.preload = "metadata";
    audio.src = url;
    return audio;
  }
  const video = document.createElement("video");
  video.className = "cm-lp-gallery-media";
  video.controls = true;
  // Metadata only: these may be multi-hundred-megabyte recordings on a
  // removable volume, and a grid that downloaded them would be a grid nobody
  // can open. `primeFirstFrame` buys the one frame `metadata` does not fetch,
  // so a video tile shows the recording instead of a black rectangle.
  video.preload = "metadata";
  video.src = url;
  primeFirstFrame(video);
  return video;
}

/** One tile: the media, its name, and the control that pins it. */
function tile(item: NoteGalleryItemVm, pinned: boolean, onToggle: () => void): HTMLElement {
  const figure = document.createElement("figure");
  figure.className = "cm-lp-gallery-tile";
  figure.style.width = `${GALLERY_TILE_WIDTH}px`;
  if (pinned) {
    figure.setAttribute("data-gallery-pinned", "true");
  }
  if (item.url !== null) {
    figure.append(mediaElement(item, item.url));
  }

  const caption = document.createElement("figcaption");
  caption.className = "cm-lp-gallery-caption";
  caption.textContent = item.name;
  figure.append(caption);

  const button = document.createElement("button");
  button.type = "button";
  button.className = "cm-lp-gallery-pin";
  button.setAttribute("aria-pressed", pinned ? "true" : "false");
  // The name is in the accessible name because a gallery holds dozens of these,
  // and dozens of identical "Pin" buttons are one control said dozens of times
  // to anyone not looking at the screen.
  button.setAttribute(
    "aria-label",
    `${pinned ? GALLERY_UNPIN_LABEL : GALLERY_PIN_LABEL} ${item.name}`,
  );
  button.textContent = pinned ? GALLERY_UNPIN_LABEL : GALLERY_PIN_LABEL;
  button.addEventListener("click", onToggle);
  figure.append(button);
  return figure;
}

/**
 * Mount the windowed grid into `into`, and answer with how to re-order and how
 * to let go.
 *
 * The rows are mounted and unmounted against a scroll position exactly as the
 * hook's are; the difference is that this one owns the DOM directly and pays
 * one ResizeObserver for the grid rather than one per tile.
 */
function mountGrid(
  into: HTMLElement,
  items: readonly NoteGalleryItemVm[],
  pins: readonly string[],
  onToggle: (relPath: string) => void,
): Omit<GalleryMount, "folder"> {
  const grid = document.createElement("div");
  grid.className = "cm-lp-gallery-grid";
  grid.setAttribute(WINDOW_VIEWPORT_ATTR, "true");
  const canvas = document.createElement("div");
  canvas.className = "cm-lp-gallery-canvas";
  grid.append(canvas);
  into.append(grid);

  const box = GALLERY_TILE_HEIGHT + GALLERY_GAP;
  const mounted = new Map<number, HTMLElement>();
  let shown: NoteGalleryItemVm[] = [];
  let pinned = new Set<string>();

  const releaseRow = (row: HTMLElement): void => {
    for (const figure of row.children) {
      // The same release the note's own embeds get: a `<video>` holding a
      // selected resource keeps an open range-request pipeline against a file
      // that may live on a volume the reader then cannot eject, and removing
      // the node does not hand it back.
      releaseRecordingMedia(figure as HTMLElement);
    }
    row.remove();
  };

  const paint = (): void => {
    const width = grid.clientWidth > 0 ? grid.clientWidth : ASSUMED_GRID_WIDTH;
    const height = grid.clientHeight > 0 ? grid.clientHeight : ASSUMED_GRID_HEIGHT;
    const columns = Math.max(1, Math.floor(width / (GALLERY_TILE_WIDTH + GALLERY_GAP)));
    const rows = Math.ceil(shown.length / columns);
    const offsets = rowOffsets(rows, () => box);
    canvas.style.height = `${offsets[rows]}px`;

    const slice = windowSlice(offsets, rows, grid.scrollTop, height, GALLERY_OVERSCAN);
    const wanted = new Set(slice.indices);
    for (const [index, row] of mounted) {
      if (!wanted.has(index)) {
        releaseRow(row);
        mounted.delete(index);
      }
    }
    for (const index of slice.indices) {
      // A row already on screen is left exactly as it is: rebuilding it would
      // restart every video in it on every scroll tick.
      if (mounted.has(index)) {
        continue;
      }
      const row = document.createElement("div");
      row.className = "cm-lp-gallery-row";
      row.setAttribute(WINDOW_ROW_ATTR, String(index));
      row.style.position = "absolute";
      row.style.top = "0";
      row.style.left = "0";
      row.style.transform = `translateY(${offsets[index]}px)`;
      for (const item of shown.slice(index * columns, index * columns + columns)) {
        row.append(
          tile(item, pinned.has(item.relPath), () => {
            onToggle(item.relPath);
          }),
        );
      }
      canvas.append(row);
      mounted.set(index, row);
    }
  };

  const repaint = (next: readonly string[]): void => {
    const order = galleryOrder(items, next);
    shown = order.shown;
    pinned = new Set(next);
    for (const [index, row] of mounted) {
      releaseRow(row);
      mounted.delete(index);
    }
    paint();
  };

  grid.addEventListener("scroll", paint, { passive: true });
  const resizes = new ResizeObserver(paint);
  resizes.observe(grid);
  repaint(pins);

  return {
    repaint,
    release: () => {
      resizes.disconnect();
      grid.removeEventListener("scroll", paint);
      for (const [index, row] of mounted) {
        releaseRow(row);
        mounted.delete(index);
      }
    },
  };
}

/** The head every gallery shows before anything is listed and after anything
 *  goes wrong: the folder it names, and its pins as working links. This is what
 *  Obsidian shows too, which is the point. */
function head(block: GalleryBlock): HTMLElement {
  const host = document.createElement("div");
  host.className = "cm-lp-gallery";

  const bar = document.createElement("div");
  bar.className = "cm-lp-gallery-head";
  const folder = document.createElement("span");
  folder.className = "cm-lp-gallery-folder";
  folder.textContent = block.folder === "" ? "Gallery" : block.folder;
  bar.append(folder);
  const note = document.createElement("span");
  note.className = "cm-lp-gallery-note";
  bar.append(note);
  host.append(bar);

  const links = document.createElement("div");
  links.className = "cm-lp-gallery-pins";
  for (const pin of block.pins) {
    links.append(pinLink(pin));
  }
  host.append(links);
  return host;
}

/** Say something in the head's note slot, and keep the pinned links visible —
 *  a gallery that cannot list its folder is still a block that names one. */
function say(host: HTMLElement, sentence: string): void {
  const note = host.querySelector(".cm-lp-gallery-note");
  if (note !== null) {
    note.textContent = sentence;
  }
}

/**
 * List `block.folder` and turn `host` into its gallery, or leave it as the
 * links it already shows and say why.
 *
 * Exported for the tests that drive the degrade paths directly: an empty
 * folder, a missing one, an unreadable one and a rejecting host are the four
 * that matter and none of them needs a CodeMirror view.
 */
export async function renderGalleryInto(
  host: HTMLElement,
  block: GalleryBlock,
  onToggle: (relPath: string) => void,
  options: GalleryOptions = {},
): Promise<void> {
  const load = options.list;
  if (load === undefined) {
    say(host, GALLERY_NO_LISTING);
    return;
  }
  if (block.folder === "") {
    say(host, "this gallery names no folder, so there is nothing to list.");
    return;
  }

  let listing: NoteGalleryVm;
  try {
    listing = await load(block.folder);
  } catch {
    // The same rule the recording embed follows: never throw out of a render,
    // and never leave the reader with less than the markdown already said.
    say(host, GALLERY_UNREACHABLE);
    return;
  }
  if (options.cancelled?.() === true) {
    return;
  }
  if (listing.problem !== null) {
    // Rust's own sentence, verbatim. It knows whether the folder was missing,
    // unreadable or outside the vault, and this does not.
    say(host, listing.problem);
    return;
  }

  const order = galleryOrder(listing.items, block.pins);
  say(
    host,
    gallerySummary({
      shown: order.shown.length,
      skipped: order.skipped,
      missingPins: order.missingPins,
      truncated: listing.truncated,
    }),
  );
  if (order.shown.length === 0) {
    return;
  }
  // The pinned links were the stand-in for the tiles; the tiles say the same
  // thing better, and leaving both would show every pin twice.
  host.querySelector(".cm-lp-gallery-pins")?.remove();
  const mount = mountGrid(host, listing.items, block.pins, onToggle);
  mounts.set(host, { ...mount, folder: block.folder });
}

/**
 * The CodeMirror widget that replaces a gallery callout.
 *
 * Constructed only from the renderer, and only for a blockquote whose first
 * line is a gallery callout — the predicate is decided there, the way
 * `recording-embed`'s `session:` predicate is.
 */
export class GalleryWidget extends WidgetType {
  /** Set by {@link destroy}, read by the render that may still be in flight. */
  private disposed = false;

  constructor(
    private readonly block: GalleryBlock,
    /** The block's exact source. Two blocks that read the same are the same
     *  widget; this is what {@link eq} compares. */
    private readonly source: string,
    private readonly options: GalleryOptions = {},
  ) {
    super();
  }

  eq(other: GalleryWidget): boolean {
    return other.source === this.source;
  }

  /**
   * Re-order an already-listed gallery in place when only its pins changed.
   *
   * Without this, pinning one photograph in a folder of four hundred would
   * throw the listing away, ask Rust for it again and drop the reader back to
   * the top of the grid — a re-render that costs an IPC round trip to move one
   * tile. The listing belongs to the mounted DOM rather than to the widget, so
   * a new widget over the same folder can adopt it.
   */
  updateDOM(dom: HTMLElement, _view: EditorView): boolean {
    const mount = mounts.get(dom);
    if (mount === undefined || mount.folder !== this.block.folder) {
      return false;
    }
    mount.repaint(this.block.pins);
    return true;
  }

  toDOM(view: EditorView): HTMLElement {
    const host = head(this.block);
    // Fired and forgotten, exactly as the mermaid fence and the recording embed
    // are: the folder name and the pinned links are in the document
    // immediately, and the grid replaces them when the listing answers.
    // Blocking `toDOM` on an IPC round trip would stall the editor on every
    // keystroke that rebuilds the decorations.
    void renderGalleryInto(
      host,
      this.block,
      (relPath) => {
        this.toggle(view, host, relPath);
      },
      {
        ...this.options,
        cancelled: () => this.disposed || this.options.cancelled?.() === true,
      },
    );
    return host;
  }

  /**
   * Pin or unpin one item by rewriting the note.
   *
   * The block's range is re-read from the document rather than remembered:
   * everything above this block may have been edited since it was constructed,
   * and a splice at a stale offset would land in somebody else's paragraph.
   */
  private toggle(view: EditorView, host: HTMLElement, relPath: string): void {
    const range = galleryRangeAt(view.state.doc, view.posAtDOM(host));
    if (range === null) {
      return;
    }
    const pinned = parseGalleryBlock(range.text)?.pins.includes(relPath) === true;
    const next = pinned ? withoutPin(range.text, relPath) : withPin(range.text, relPath);
    if (next === range.text) {
      return;
    }
    view.dispatch({ changes: { from: range.from, to: range.to, insert: next } });
  }

  destroy(dom: HTMLElement): void {
    this.disposed = true;
    mounts.get(dom)?.release();
    mounts.delete(dom);
  }

  /**
   * Keep the events aimed at a control inside the gallery.
   *
   * `true` means CodeMirror ignores the event entirely, which is what keeps the
   * caret off the block's lines — and a revealed line drops its decorations, so
   * without this, pressing Pin would un-render the gallery instead of pinning,
   * and pressing play on a video tile would destroy the player. The pinned
   * links are deliberately not on the list: they are wikilinks and must behave
   * like the wikilinks they are.
   */
  ignoreEvent(event: Event): boolean {
    return (
      event.target instanceof Element &&
      event.target.closest("video, audio, button, input, .cm-lp-gallery-grid") !== null
    );
  }
}

/** One gallery block found in a document. */
interface GalleryHit {
  from: number;
  to: number;
  text: string;
  block: GalleryBlock;
}

/**
 * Every gallery block in the document, found by scanning lines.
 *
 * By line rather than through the syntax tree, because this runs in a
 * `StateField` and a field has no view to ask for visible ranges — walking the
 * whole parse tree on every keystroke would cost far more than a regex per
 * line, and the two questions a gallery asks (is this line the callout head,
 * does the quote continue) are exactly the two a line answers by itself.
 *
 * A head line inside an existing blockquote is not a head: `> [!gallery]` on
 * the second line of somebody's quotation is part of that quotation, and
 * starting a block there would swallow the lines above it.
 */
function galleryHits(doc: Text): GalleryHit[] {
  const hits: GalleryHit[] = [];
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
      const block = parseGalleryBlock(text);
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
function galleryDecorationSet(
  hits: readonly GalleryHit[],
  state: EditorState,
  options: GalleryOptions,
): DecorationSet {
  const decorations = [];
  for (const hit of hits) {
    // The renderer's own reveal rule, applied to a whole block: put the caret
    // anywhere in a gallery and its source comes back, so the folder and the
    // pins stay editable as the text they are.
    const revealed = state.selection.ranges.some(
      (range) => range.from <= hit.to && range.to >= hit.from,
    );
    if (revealed) {
      continue;
    }
    decorations.push(
      Decoration.replace({
        widget: new GalleryWidget(hit.block, hit.text, options),
        block: true,
      }).range(hit.from, hit.to),
    );
  }
  return Decoration.set(decorations, true);
}

/**
 * The gallery layer, as a `StateField` rather than as part of the renderer's
 * `ViewPlugin`.
 *
 * Not a preference. A gallery replaces several lines with one element, and
 * CodeMirror refuses both halves of that from a plugin — `block: true` is
 * rejected outright, and an inline replace spanning a line break is rejected
 * the moment a block has its first pin. The renderer's plugin therefore cannot
 * host this, and a field is the shape CodeMirror documents for exactly this
 * case. It is composed into {@link livePreview}'s extension array beside the
 * plugin, so a note still has one renderer.
 *
 * The scan is doc-driven and the reveal is selection-driven, and they are
 * separated: moving the caret rebuilds the decoration set from the blocks
 * already found, and only an edit re-scans the document.
 */
export function galleryLayer(options: GalleryOptions = {}): Extension {
  return StateField.define<{ hits: GalleryHit[]; decorations: DecorationSet }>({
    create(state) {
      const hits = galleryHits(state.doc);
      return { hits, decorations: galleryDecorationSet(hits, state, options) };
    },
    update(value, transaction) {
      if (!transaction.docChanged && transaction.selection === undefined) {
        return value;
      }
      const hits = transaction.docChanged ? galleryHits(transaction.state.doc) : value.hits;
      return { hits, decorations: galleryDecorationSet(hits, transaction.state, options) };
    },
    provide: (field) => EditorView.decorations.from(field, (value) => value.decorations),
  });
}
