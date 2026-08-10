/**
 * `![[…]]` over a data file, rendered and editable inside the note
 * (Story 45.12, FR-186, FR-187, UX-DR75).
 *
 * # One widget, not one per format
 *
 * Story 44.16 shipped `CsvTableWidget`: a `.csv` embed became a table. This is
 * that widget with the format taken out of it. Which formats get a panel, what
 * that panel renders, which syntax the source gets and whether the bytes may be
 * written are all rows of 45.2's registry, and **this file holds no extension
 * list**. Adding JSON was not a branch here; it was already a row.
 *
 * What was lifted rather than copied, because the alternative was a second
 * `csv-table.ts`:
 *
 * - the *panel* is 45.4's `RawRenderedView` — the toggle, the rendered halves
 *   and the refusal banner — reached through `TextFileFrame`, which is also
 *   what a Files panel mounts;
 * - the *loading, dirty tracking and saving* are 45.6's `useTextBuffer`, with a
 *   vault-scoped {@link ../../viewers/use-text-file.TextFileSource} instead of a
 *   profile-scoped one;
 * - the *table* is still 44.16's `renderCsvTableInto`, still the only thing that
 *   spells a CSV, mounted by `RawRenderedView` exactly as a Files panel would
 *   mount it.
 *
 * What stayed in `csv-table.ts` is the CSV table itself. What left it is the
 * widget, which was never about CSV.
 *
 * # Why this module imports no React
 *
 * A CodeMirror widget lives in the note editor's lazily imported chunk, which
 * `note-editor.tsx` set up for NFR-27 and which `gallery-block.ts` describes as
 * React-free. A panel is React, so the panel is behind a dynamic `import()` and
 * arrives after the link is already on screen — the same "fired and forgotten"
 * shape the mermaid fence and 44.16's table already use, and the reason
 * `toDOM` never blocks on IPC.
 *
 * That is also why the registry is imported from `@/lib/viewers/registry` and
 * not from the `@/lib/viewers` barrel. The barrel re-exports the component
 * table, which reaches `TextFileViewer`, `RawRenderedView` and CodeMirror's
 * language packs; a static edge from here would put all of that in the editor's
 * chunk to answer a question `registry.ts` answers with a frozen `Map` and no
 * imports at all.
 *
 * # A block decoration is still refused here
 *
 * These decorations come from a `ViewPlugin`, and CodeMirror refuses a block
 * decoration from one (DW-165). 45.10 moved the mermaid fence out to its own
 * `StateField` and did not change that rule, so this is an **inline** replace
 * whose host is styled `display: block` — which costs nothing, because an
 * `![[…]]` embed is one line and an inline replace may not span a line break
 * anyway.
 */
import { WidgetType } from "@codemirror/view";
import { resolveViewer } from "@/lib/viewers/registry";
import type { ViewerEntry } from "@/lib/viewers/types";
import { renderRecordingEmbedInto } from "./recording-embed";

/** The block host CodeMirror replaces the embed with. */
export const EMBED_BLOCK_CLASS = "cm-embed-block";

/** The panel inside it, once React has arrived. Its presence is also the test
 *  {@link FileEmbedWidget.ignoreEvent} uses, so a click inside the panel stays
 *  in the panel while the degraded link behaves like the link it still is. */
export const EMBED_BODY_CLASS = "cm-embed-body";

/**
 * The panel's height, in CSS pixels.
 *
 * Fixed, and px rather than rem so {@link FileEmbedWidget.estimatedHeight} and
 * the style cannot disagree about the same box.
 *
 * Fixed rather than fitted to the content for two reasons that both bite. The
 * reader toggles between Source and the rendered view *inside a note they are
 * scrolled into*: a box that resizes on that click moves every line below it,
 * which is the note scrolling out from under someone who pressed a tab. And
 * 44.10's windowed list — which the structure view uses — measures its viewport
 * to decide how many rows to mount, so an auto-height pane would report the
 * height of its own content and mount all of it.
 */
export const EMBED_HEIGHT_PX = 384;

/** How the widget reaches the panel. Injected so a test can drive the mount
 *  without a Tauri host, exactly as `CsvTableOptions` did for 44.16. */
export interface FileEmbedOptions {
  /** Replace the dynamic import of the React panel. */
  mount?: (
    container: HTMLElement,
    args: { vaultId: string; target: string },
  ) => { unmount: () => void };
  /** Replace the dynamic import of 42.4's recording renderer. Answers whether
   *  the session claimed the target. */
  renderRecording?: (
    host: HTMLElement,
    sessionId: string,
    target: string,
    options: { cancelled: () => boolean },
  ) => Promise<boolean>;
}

/**
 * The registry row for an embed target, or `null` when this target is not one
 * of the files an embed opens as a panel.
 *
 * **This is a decision about the EMBED, not a classification of the file.**
 * `kind: "file"` is passed because that is the only kind the registry consults
 * an extension under, and because `classifier-agreement.test.ts` already pins
 * that no extension in `FILE_FORMATS` can be video, image or audio in Rust. It
 * is a hypothesis, and it decides only that the embed gets to *try* — which is
 * the same thing `RecordingEmbedWidget` decides synchronously before the index
 * confirms what a file really is. The panel then resolves the row again with
 * the kind Rust actually returned, and draws that.
 *
 * The set is derived and not listed: a text-shaped format with a rendered half
 * that is a table or a structure. That is CSV, JSON and JSONL today, and it
 * will be whatever else earns a row.
 *
 * **Markdown is deliberately excluded even though it has a rendered half.**
 * `![[note.md]]` is a transclusion — showing one note inside another — which is
 * a different feature with a different meaning, and mounting a raw editor over
 * a note here would be a second way to write a note, without `notes_save`'s
 * base revision or its conflict copy. Rust refuses that write too, so the two
 * halves agree.
 */
export function embedEntryFor(target: string): ViewerEntry | null {
  const name = target.slice(target.lastIndexOf("/") + 1);
  const entry = resolveViewer({ name, kind: "file" });
  if (entry.viewer !== "text") {
    return null;
  }
  return entry.rendered === "table" || entry.rendered === "structure" ? entry : null;
}

/** The ordinary wikilink: what the embed shows before the panel arrives, and
 *  what it stays as when the vault has no such file. */
function link(target: string): HTMLElement {
  const anchor = document.createElement("a");
  anchor.className = "cm-lp-wikilink";
  anchor.textContent = target;
  return anchor;
}

/**
 * Tell every other embed of the same file that it just changed.
 *
 * # Why a bus and not a re-render
 *
 * Two embeds of one file in one note are two panels with two buffers, and after
 * a cell edit in the first the second is showing bytes that are no longer on
 * disk. Nothing above them can notice: they are React roots mounted by
 * CodeMirror widgets, with no common ancestor to hold the state in, and the
 * note's own document did not change so no CodeMirror update fires either.
 *
 * # Keyed on the RESOLVED path, which is the whole point
 *
 * `![[data.csv]]` and `![[attachments/data.csv]]` are the same file — Rust
 * resolves the bare name into the attachments folder — so a bus keyed on the
 * text between the brackets would let those two drift apart, which is exactly
 * the case a reader is most likely to create by hand. The key is the `relPath`
 * Rust answered with, so the two agree by construction.
 *
 * `from` is the announcer, and it never hears itself: the panel that wrote
 * already knows, and a reload it did not ask for would throw away the buffer it
 * just persisted.
 */
export function announceEmbedWrite(vaultId: string, relPath: string, from: object): void {
  const subscribed = listeners.get(`${vaultId}\u0000${relPath}`);
  if (subscribed === undefined) {
    return;
  }
  // A copy, because a listener may unsubscribe while being notified — a panel
  // whose reload unmounts it is the ordinary case, not the exotic one.
  for (const [token, listener] of [...subscribed]) {
    if (token !== from) {
      listener();
    }
  }
}

/** Hear about writes to one file. Returns the unsubscribe. */
export function onEmbedWrite(
  vaultId: string,
  relPath: string,
  token: object,
  listener: () => void,
): () => void {
  const key = `${vaultId}\u0000${relPath}`;
  const subscribed = listeners.get(key) ?? new Map<object, () => void>();
  subscribed.set(token, listener);
  listeners.set(key, subscribed);
  return () => {
    const still = listeners.get(key);
    if (still === undefined) {
      return;
    }
    still.delete(token);
    if (still.size === 0) {
      // Dropped rather than left empty: a note the reader opens and closes all
      // afternoon would otherwise leave one entry per file it ever showed.
      listeners.delete(key);
    }
  };
}

/** Subscribers per file, keyed `vaultId\u0000relPath` — `\u0000` because it can
 *  occur in neither a vault id nor a path, so two files cannot collide by
 *  concatenation. A `Map` rather than a `Record` because entries are added and
 *  removed as panels mount and unmount, and the empty case is detected with
 *  `.size`. */
const listeners = new Map<string, Map<object, () => void>>();

/**
 * The CodeMirror widget that replaces a data-file embed with its panel.
 *
 * One widget for every format the registry gives a rendered half to. The class
 * knows a vault id, a target and — in a recording note — a session id, and
 * nothing else about files; everything it draws it gets from
 * {@link FileEmbedOptions.mount}'s panel or from the recording renderer.
 *
 * # Two address spaces, and the order between them
 *
 * In a recording note an `![[…]]` target may name **the session's own file** —
 * `manifest.json`, under the recordings destination — or **a file in the
 * vault** beside the note, which is what the attachments panel writes. Those
 * are different roots, and no synchronous test can tell them apart from the
 * text between the brackets.
 *
 * Story 44.16 did not have to notice, because it claimed only `.csv` and a
 * session has no spreadsheets. Generalising over the registry breaks that
 * premise on the first row: `manifest.json` IS a session file. So the session
 * is asked first and the vault second — the session's index answers with a
 * definite yes or no, the vault's answer costs a read, and in a recording note
 * the session's own files are the common case by a wide margin.
 *
 * A `false` from the recording renderer is not "missing", it is "not one of
 * this session's files", which is exactly the licence to look in the vault.
 */
export class FileEmbedWidget extends WidgetType {
  /** Set by {@link destroy}, read by the import that may still be in flight. */
  private disposed = false;

  /** The mounted panel, so it can be taken down with the widget. */
  private panel: { unmount: () => void } | null = null;

  constructor(
    private readonly vaultId: string,
    private readonly target: string,
    /** The note's `session:` frontmatter, or null when it has none. Read at
     *  decoration time by `live-preview.ts`, because the editor outlives the
     *  note in it. */
    private readonly sessionId: string | null = null,
    private readonly options: FileEmbedOptions = {},
  ) {
    super();
  }

  /** Same vault and same target, same panel: CodeMirror may reuse the DOM,
   *  which is what keeps a half-typed cell and an unsaved buffer alive while
   *  the caret moves around the note. */
  eq(other: FileEmbedWidget): boolean {
    return (
      other.vaultId === this.vaultId &&
      other.target === this.target &&
      other.sessionId === this.sessionId
    );
  }

  /** So CodeMirror's height map does not have to discover the panel's size by
   *  measuring it, which it cannot do before the panel exists. */
  get estimatedHeight(): number {
    return EMBED_HEIGHT_PX;
  }

  toDOM(): HTMLElement {
    const host = document.createElement("div");
    host.className = EMBED_BLOCK_CLASS;
    // The link is in the document immediately and the panel takes its place
    // when React and the file have arrived. Blocking `toDOM` on an import and
    // an IPC round trip would stall the editor on every keystroke that rebuilds
    // the decorations.
    host.append(link(this.target));
    void this.open(host);
    return host;
  }

  private async open(host: HTMLElement): Promise<void> {
    if (await this.claimedByTheSession(host)) {
      return;
    }
    // Dynamic on purpose, and a static import cannot work here: this module is
    // reached from `live-preview.ts`, which `note-editor.tsx` imports lazily to
    // keep the editor's chunk free of React and of the viewer stack (NFR-27).
    // A static edge would put `RawRenderedView`, `TextEditorSurface` and every
    // CodeMirror language pack into that chunk for the benefit of a note that
    // may contain no embed at all. See the module header.
    const mount = this.options.mount ?? (await import("./file-embed-host")).mountNoteFileEmbed;
    if (this.disposed) {
      return;
    }
    const body = document.createElement("div");
    body.className = EMBED_BODY_CLASS;
    body.style.height = `${EMBED_HEIGHT_PX}px`;
    host.replaceChildren(body);
    // Synchronous, so the panel is either mounted or `this.disposed` was
    // already true — there is no window in which a widget CodeMirror has torn
    // down leaves a React root attached to a detached node.
    this.panel = mount(body, { vaultId: this.vaultId, target: this.target });
  }

  /**
   * Let the recording renderer have the target first, in a recording note.
   *
   * The renderer is handed its own host carrying `cm-lp-recording`, so a
   * claimed embed has the DOM every other recording embed has and 42.4's
   * styling and tests still find it. Nothing is released on destroy because
   * nothing releasable can be produced here: a target only reaches this widget
   * when the registry called it a text-shaped data format, and
   * `classifier-agreement.test.ts` pins that no such extension is video, image
   * or audio in Rust — so the session's answer for one is always a chip.
   */
  private async claimedByTheSession(host: HTMLElement): Promise<boolean> {
    const sessionId = this.sessionId;
    if (sessionId === null) {
      return false;
    }
    // Statically imported, unlike the panel: `recording-embed.ts` is already in
    // this chunk — `live-preview.ts` and `gallery-block.ts` both import it —
    // and it pulls in no React. Deferring it would only add a tick between the
    // link and the chip for no saved byte.
    const render = this.options.renderRecording ?? renderRecordingEmbedInto;
    const recording = document.createElement("div");
    recording.className = "cm-lp-recording";
    recording.append(link(this.target));
    host.replaceChildren(recording);
    const claimed = await render(recording, sessionId, this.target, {
      cancelled: () => this.disposed,
    });
    if (claimed || this.disposed) {
      return true;
    }
    // Declined: put the plain link back at the top level so the vault half
    // starts from the same host state a note without a session starts from.
    host.replaceChildren(link(this.target));
    return false;
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
   * `true` means CodeMirror ignores the event entirely. Everything inside the
   * panel has to be kept: it contains a real text editor, and letting a
   * keystroke through would type into the note instead of the file — while
   * letting a click through would put the caret on the embed's line, and a
   * revealed line drops its decorations, so clicking the panel would destroy it
   * rather than use it. The same trade `RecordingEmbedWidget` and 44.16's table
   * make for their controls.
   *
   * Everything outside the panel gives its events up, so the degraded link
   * behaves like the wikilink it still is.
   */
  ignoreEvent(event: Event): boolean {
    return (
      event.target instanceof Element &&
      // The chip's Reveal and Copy path buttons, for a target the session
      // claimed, on exactly the rule `RecordingEmbedWidget` applies to them.
      event.target.closest(`.${EMBED_BODY_CLASS}, .cm-lp-recording-chip-action`) !== null
    );
  }
}
