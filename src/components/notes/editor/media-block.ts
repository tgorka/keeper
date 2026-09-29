/**
 * The ` ```keeper-media ` block: a recording, a transcript or a list of media
 * files played inside a note, with the transcript's lines under the player.
 *
 * ```md
 * ```keeper-media
 * session = "01J8…-01J8…"
 * ```
 * ```
 *
 * **A fence, not a callout.** keeper's other note widgets are callouts because
 * their content is prose or links that stay useful in Obsidian. This block's
 * content is configuration — a session, parts with offsets and track roles,
 * named markers with times — and a fenced block with a name is the extension
 * point Obsidian itself gives plugins for exactly that (`docs/notes.md`).
 *
 * **Only Rust reads the body.** The text between the fences is handed to
 * `media_block_resolve` verbatim with the note's drive; TypeScript never reads
 * a key, splits a line or joins a path (AD-65). A marker edit comes back from
 * Rust as the block's new body and is spliced over the block's range, found
 * again at the moment of the action — the gallery's rule — so an edit computed
 * against a stale position cannot land in the paragraph above.
 *
 * **A clip's words ride with the block.** A `> [!transcript]` callout on the
 * line directly after the closing fence is the words a *Copy clip* carried for
 * readers without keeper; it is part of the block's range, hidden and revealed
 * with the fence, so a keeper reader never sees the words twice. A callout
 * after a blank line is the person's own and is drawn as any callout.
 *
 * **A block decoration, therefore a `StateField`** — `mermaidLayer`'s shape,
 * for DW-165's reason. Fences are found in the parse tree by `FencedCode` and
 * its `CodeInfo`, so a tilde fence, an indented one and one inside a list item
 * are all found.
 *
 * **This module imports no React** (NFR-27): the panel arrives through a
 * dynamic `import()` after the fence's own text is already on screen.
 */
import { syntaxTree } from "@codemirror/language";
import { type EditorState, type Extension, StateField } from "@codemirror/state";
import { Decoration, type DecorationSet, EditorView, WidgetType } from "@codemirror/view";

/** The fence's info word. Prefixed, so no Obsidian plugin can claim it. */
export const MEDIA_BLOCK_INFO = "keeper-media";

/** The block host CodeMirror replaces the fence with. */
export const MEDIA_BLOCK_CLASS = "cm-media-block";

/** The panel inside it once React has arrived; events inside stay there. */
export const MEDIA_BLOCK_BODY_CLASS = "cm-media-block-body";

/**
 * What CodeMirror assumes the block is tall, and what the panel holds itself
 * to: a player, a row of chips and a fixed box of about twelve lines. A block
 * whose height followed its transcript would stretch the note and shift the
 * height map on every resolve.
 */
export const MEDIA_BLOCK_ESTIMATED_HEIGHT_PX = 720;

/** What the block says with no drive to resolve against (UX-DR124). */
export const MEDIA_BLOCK_NO_DRIVE = "keeper draws media only in notes inside a synced folder.";

/** What the panel is handed. */
export interface MediaBlockMountArgs {
  /** The drive the note is in (a vault's id is its profile's). */
  profileId: string;
  /** The fence's body, verbatim. */
  source: string;
  /** Whether a marker edit can be written here (AD-27): absent controls, not
   *  refusing ones, in a read-only preview. */
  editable: boolean;
  /** Replace this block's body with `next`. Resolves false when the block is
   *  no longer where it was, or no longer holds `source`. */
  replaceSource: (next: string) => boolean;
  /** The link target of the note holding the block, for *Copy link*. */
  noteLink: string | null;
  /** Register the panel's seek, so a `[[note#marker]]` can move it. */
  register: (handle: MediaBlockHandle) => () => void;
  /** Report that this block started playing, and pause any other that is. */
  claimPlayback: (pause: () => void) => () => void;
}

/** What the layer can ask of a mounted panel. */
export interface MediaBlockHandle {
  /** Move the player to `seconds`, paused. */
  seekTo: (seconds: number) => void;
}

export interface MediaBlockOptions {
  /** The drive the note is in, or empty when there is none. */
  profileId?: string;
  /** The note's own link target, read when a panel mounts. */
  noteLink?: () => string | null;
  /** Replace the dynamic import of the React panel. */
  mount?: (container: HTMLElement, args: MediaBlockMountArgs) => MountedMediaBlock;
}

/** One `keeper-media` fence: the range it owns and the body between. */
export interface MediaFence {
  /** Start of the opening fence line. */
  from: number;
  /** End of the closing fence line, or of an adjacent words callout. */
  to: number;
  /** The body's range, for a splice. `bodyFrom === bodyTo` for an empty body. */
  bodyFrom: number;
  bodyTo: number;
  source: string;
}

/** A `> [!transcript]` callout head, case-insensitive as Obsidian's callouts are. */
const WORDS_HEAD = /^\s{0,3}>[ \t]?\[!transcript\][-+]?(?:[ \t]|$)/i;

/** A line that belongs to a blockquote. Lazy continuation is not admitted: the
 *  words end where their `>` markers end. */
const QUOTED = /^\s{0,3}>/;

/**
 * Every `keeper-media` fence in the document, in document order.
 *
 * An unterminated fence — one still being typed — is not a block: until its
 * closing fence exists the parse tree runs it to the end of the note, and
 * drawing a player over everything below would take the note away.
 */
export function mediaFences(state: EditorState): MediaFence[] {
  const fences: MediaFence[] = [];
  const doc = state.doc;
  syntaxTree(state).iterate({
    enter: (node) => {
      if (node.name !== "FencedCode") {
        return undefined;
      }
      let info = "";
      let marks = 0;
      for (let child = node.node.firstChild; child !== null; child = child.nextSibling) {
        if (child.name === "CodeInfo") {
          info = doc.sliceString(child.from, child.to).trim();
        } else if (child.name === "CodeMark") {
          marks += 1;
        }
      }
      if (info.split(/\s+/, 1)[0] !== MEDIA_BLOCK_INFO || marks < 2) {
        return false;
      }
      const open = doc.lineAt(node.from);
      const close = doc.lineAt(node.to);
      const bodyFrom = Math.min(open.to + 1, close.from);
      const bodyTo = Math.max(bodyFrom, close.from - 1);
      let to = close.to;
      if (close.number < doc.lines && WORDS_HEAD.test(doc.line(close.number + 1).text)) {
        let last = close.number + 1;
        while (last < doc.lines && QUOTED.test(doc.line(last + 1).text)) {
          last += 1;
        }
        to = doc.line(last).to;
      }
      fences.push({
        from: open.from,
        to,
        bodyFrom,
        bodyTo,
        source: doc.sliceString(bodyFrom, bodyTo),
      });
      return false;
    },
  });
  return fences;
}

/**
 * Put a widget — a block Rust composed, a gallery head — at `at` (the caret
 * when absent), starting a line of its own: a fence or a callout that began
 * mid-sentence would be neither. A composed block ends in a newline, so what
 * followed the insertion point moves below it. The user's own edit —
 * undoable, and reported to the note's save.
 */
export function insertOnOwnLine(view: EditorView, text: string, at?: number): void {
  const pos = Math.min(at ?? view.state.selection.main.head, view.state.doc.length);
  const line = view.state.doc.lineAt(pos);
  const insert = line.text.slice(0, pos - line.from).trim() === "" ? text : `\n${text}`;
  view.dispatch({
    changes: { from: pos, insert },
    selection: { anchor: pos + insert.length },
    userEvent: "input.media-block",
    scrollIntoView: true,
  });
  view.focus();
}

/** Panels mounted in a view, by their host, so a marker link can reach one. */
const handles = new WeakMap<EditorView, Map<HTMLElement, MediaBlockHandle>>();

/** A seek asked of a block that had not mounted yet, taken when it does. */
const pendingSeeks = new WeakMap<EditorView, { from: number; seconds: number }>();

/** The block playing in each view, so starting another pauses it. */
const playing = new WeakMap<EditorView, { pause: () => void }>();

/**
 * Bring the `ordinal`-th block (0-based, document order) into view and move
 * its player to `seconds`, paused. Answers false when there is no such block.
 *
 * The panel may not exist yet — CodeMirror draws only near the viewport, and a
 * panel arrives after a dynamic import — so the seek is left for it and taken
 * when it registers.
 */
export function seekMediaBlock(view: EditorView, ordinal: number, seconds: number): boolean {
  const fence = mediaFences(view.state)[ordinal];
  if (fence === undefined) {
    return false;
  }
  view.dispatch({ effects: EditorView.scrollIntoView(fence.from, { y: "start", yMargin: 24 }) });
  for (const [host, handle] of handles.get(view) ?? []) {
    if (host.isConnected && safePos(view, host) === fence.from) {
      handle.seekTo(seconds);
      return true;
    }
  }
  pendingSeeks.set(view, { from: fence.from, seconds });
  return true;
}

/** Where a widget's DOM sits in the document, or null once it is detached. */
function safePos(view: EditorView, host: HTMLElement): number | null {
  try {
    return view.posAtDOM(host);
  } catch {
    return null;
  }
}

/** The fence whose range holds `pos`, in the view's current document. */
function fenceAt(view: EditorView, pos: number): MediaFence | undefined {
  return mediaFences(view.state).find((fence) => fence.from <= pos && pos <= fence.to);
}

/** What a mounted panel is, so a changed body can be handed to it. */
export interface MountedMediaBlock {
  unmount: () => void;
  update: (args: MediaBlockMountArgs) => void;
}

/** The panel mounted in each block host, whichever widget instance drew it. */
const panels = new WeakMap<HTMLElement, MountedMediaBlock>();

/** Hosts CodeMirror has torn down, so an import still in flight mounts nothing. */
const dropped = new WeakSet<HTMLElement>();

export class MediaBlockWidget extends WidgetType {
  constructor(
    /** The fence's body; what {@link eq} compares, so a keystroke elsewhere in
     *  the note neither re-resolves the block nor stops its player. */
    private readonly source: string,
    /** The fence as written, shown until the panel arrives. */
    private readonly text: string,
    private readonly options: MediaBlockOptions,
  ) {
    super();
  }

  eq(other: MediaBlockWidget): boolean {
    return other.source === this.source && other.options.profileId === this.options.profileId;
  }

  get estimatedHeight(): number {
    return MEDIA_BLOCK_ESTIMATED_HEIGHT_PX;
  }

  toDOM(view: EditorView): HTMLElement {
    const host = document.createElement("div");
    host.className = MEDIA_BLOCK_CLASS;
    // The fence's own text is the resolving state (UX-DR124): what Obsidian
    // shows, and never an empty box.
    const pre = document.createElement("pre");
    pre.className = "cm-media-block-source";
    pre.textContent = this.text;
    host.append(pre);
    void this.open(view, host);
    return host;
  }

  /**
   * A marker edit changes the body, and a changed body would otherwise be a
   * new widget: a new panel, a new player, playback stopped under the hand
   * that pressed *Mark this moment*. The mounted panel takes the new body
   * instead and resolves it in place.
   */
  updateDOM(dom: HTMLElement, view: EditorView): boolean {
    const panel = panels.get(dom);
    if (panel === undefined || this.profileId() === "") {
      return false;
    }
    panel.update(this.args(view, dom));
    return true;
  }

  private profileId(): string {
    return this.options.profileId ?? "";
  }

  private args(view: EditorView, host: HTMLElement): MediaBlockMountArgs {
    const source = this.source;
    return {
      profileId: this.profileId(),
      source,
      editable: !view.state.readOnly,
      noteLink: this.options.noteLink?.() ?? null,
      replaceSource: (next) => {
        const pos = safePos(view, host);
        const fence = pos === null ? undefined : fenceAt(view, pos);
        if (fence === undefined || fence.source !== source) {
          return false;
        }
        view.dispatch({
          changes: { from: fence.bodyFrom, to: fence.bodyTo, insert: next },
          userEvent: "input.media-block",
        });
        return true;
      },
      register: (handle) => {
        let byHost = handles.get(view);
        if (byHost === undefined) {
          byHost = new Map();
          handles.set(view, byHost);
        }
        byHost.set(host, handle);
        const pending = pendingSeeks.get(view);
        if (pending !== undefined && safePos(view, host) === pending.from) {
          pendingSeeks.delete(view);
          handle.seekTo(pending.seconds);
        }
        return () => {
          handles.get(view)?.delete(host);
        };
      },
      claimPlayback: (pause) => {
        const current = playing.get(view);
        if (current !== undefined && current.pause !== pause) {
          current.pause();
        }
        const claim = { pause };
        playing.set(view, claim);
        return () => {
          if (playing.get(view) === claim) {
            playing.delete(view);
          }
        };
      },
    };
  }

  private async open(view: EditorView, host: HTMLElement): Promise<void> {
    if (this.profileId() === "") {
      const note = document.createElement("p");
      note.className = "cm-media-block-note";
      note.textContent = MEDIA_BLOCK_NO_DRIVE;
      host.prepend(note);
      return;
    }
    let mount = this.options.mount;
    if (mount === undefined) {
      try {
        // Dynamic, and a static import cannot work here: this module is in the
        // editor's React-free chunk (NFR-27), and the host is React.
        mount = (await import("./media-block-host")).mountMediaBlock;
      } catch {
        // The fence's text stays on screen: the block cannot draw, and says
        // what it is.
        return;
      }
    }
    if (dropped.has(host)) {
      return;
    }
    const body = document.createElement("div");
    body.className = MEDIA_BLOCK_BODY_CLASS;
    host.replaceChildren(body);
    panels.set(host, mount(body, this.args(view, host)));
  }

  destroy(dom: HTMLElement): void {
    dropped.add(dom);
    const panel = panels.get(dom);
    panels.delete(dom);
    if (panel === undefined) {
      return;
    }
    // A microtask: this runs while CodeMirror updates its DOM, possibly inside
    // a React commit, and unmounting a root mid-render is refused. Unmounting
    // the panel unmounts its player, whose elements release their media even
    // when playing (AD-359).
    queueMicrotask(() => {
      panel.unmount();
    });
  }

  /** Events inside the panel stay in the panel: letting a click through would
   *  put the caret in the fence, and a revealed fence drops the player. The
   *  fence text shown before the panel arrives gives its events up, so a click
   *  there reveals the source like any fence. */
  ignoreEvent(event: Event): boolean {
    return (
      event.target instanceof Element && event.target.closest(`.${MEDIA_BLOCK_BODY_CLASS}`) !== null
    );
  }
}

/**
 * The media layer. Scan and reveal are separated as `mermaidLayer` separates
 * them: moving the caret rebuilds the decorations from the fences already
 * found, and only an edit — or the parser reaching further — re-scans.
 */
export function mediaBlockLayer(options: MediaBlockOptions = {}): Extension {
  const paint = (fences: readonly MediaFence[], state: EditorState): DecorationSet =>
    Decoration.set(
      fences
        .filter(
          (fence) =>
            !state.selection.ranges.some(
              (range) => range.from <= fence.to && range.to >= fence.from,
            ),
        )
        .map((fence) =>
          Decoration.replace({
            widget: new MediaBlockWidget(
              fence.source,
              state.doc.sliceString(fence.from, fence.to),
              options,
            ),
            block: true,
          }).range(fence.from, fence.to),
        ),
      true,
    );
  return StateField.define<{ fences: MediaFence[]; decorations: DecorationSet }>({
    create(state) {
      const fences = mediaFences(state);
      return { fences, decorations: paint(fences, state) };
    },
    update(value, transaction) {
      const rescan =
        transaction.docChanged ||
        syntaxTree(transaction.startState) !== syntaxTree(transaction.state);
      if (!rescan && transaction.selection === undefined) {
        return value;
      }
      const fences = rescan ? mediaFences(transaction.state) : value.fences;
      return { fences, decorations: paint(fences, transaction.state) };
    },
    provide: (field) => EditorView.decorations.from(field, (value) => value.decorations),
  });
}
