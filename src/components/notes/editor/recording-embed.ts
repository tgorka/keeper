/**
 * The embedded video player for a recording note's `![[…]]` embed
 * (Story 42.4, FR-142, FR-145, AD-65).
 *
 * A recording note names its recording only in relative terms, because FR-145
 * keeps absolute paths out of a file the user syncs between machines. The
 * properties panel turns that text into *actions*; this turns it into something
 * the reader can watch without leaving the note.
 *
 * **`![[…]]`, Obsidian's own embed form.** The vault is read by Obsidian
 * unchanged, and `![[file.mov]]` is exactly what Obsidian means by "embed this
 * file" — it renders a native `<video>` for a path it can resolve and a plain
 * "not found" placeholder for one it cannot, so the same bytes are valid,
 * legible markdown in both apps. It also costs no new grammar: {@link WIKILINK}
 * already matched the embed form for the ordinary link rendering, and the `!`
 * is the whole of the difference.
 *
 * **The session id is the key, never the path.** Resolution goes through
 * `recording_note_targets`, which answers from Story 42.1's index, so an embed
 * written before a Story 40.4 retitle still finds its file — and the frontend
 * joins nothing (AD-65). A note with no `session:` is not a recording note and
 * never reaches this module at all.
 *
 * **The link is what renders first, and a player only ever replaces it.**
 * `toDOM` returns the ordinary wikilink synchronously and the resolved player
 * takes its place when — and only when — the embed names a `kind: "video"`
 * target of that session. So a `manifest.json` embed, a path this session does
 * not have, an unreachable volume and an IPC failure all leave the reader with
 * the same working link they would have had if this module did not exist. A
 * dead player is worse than a plain link in the way that matters: a link states
 * what the note says and does something when clicked, while a player that
 * cannot load states that the recording is broken — and the recording is fine.
 * The note is the durable record; being wrong about it is the one unrecoverable
 * thing a renderer can do.
 *
 * **No autoplay, and metadata only.** These are multi-hundred-megabyte screen
 * recordings on what may be a removable or network volume. The element is given
 * `preload="metadata"` so opening a note costs a duration and a first frame
 * rather than a download, and playback starts when a person presses play.
 * {@link releaseRecordingVideo} is the other half of that promise: a widget
 * scrolled out of the viewport must give the bytes back.
 */
import { WidgetType } from "@codemirror/view";
import { type RecordingNoteTargetVm, recordingNoteTargets } from "@/lib/ipc/client";
import { WIKILINK_ATTR } from "./wikilink";

/**
 * The scheme `keeper-recording://` is served on (`recording_protocol.rs`).
 *
 * A third scheme beside `keeper-note://` because that one is contained to a
 * vault, and `RecordingsConfig::validate` guarantees a recordings root never
 * overlaps one — a recording file is provably out of its reach, and widening it
 * would mean deleting the check that makes it safe (AD-59).
 */
export const RECORDING_ASSET_SCHEME = "keeper-recording";

/** How the widget reaches the index. Injected so the degrade paths — which are
 *  the interesting ones — are reachable in a test without a Tauri host. */
export type RecordingTargetLoader = (sessionId: string) => Promise<RecordingNoteTargetVm[] | null>;

export interface RecordingEmbedOptions {
  /** Overridden in tests; production always asks Rust. */
  load?: RecordingTargetLoader;
  /** Whether the host has been torn down since the render began. */
  cancelled?: () => boolean;
}

/**
 * The URL the webview plays a session's file over.
 *
 * Both halves arrive from Rust — the session id from the note's frontmatter and
 * the path from a target `recording_note_targets` composed — and neither is
 * joined onto anything here, which is the whole of AD-65's rule for this
 * surface. The shape mirrors the `keeper-note://` URL the note editor builds for
 * a vault asset: scheme, an id as the host, then percent-encoded path segments
 * so a `/` stays a separator and a space does not end the path.
 */
export function recordingAssetUrl(sessionId: string, relativePath: string): string {
  const path = relativePath.split("/").map(encodeURIComponent).join("/");
  return `${RECORDING_ASSET_SCHEME}://${encodeURIComponent(sessionId)}/${path}`;
}

/** The last `/`-separated component of a relative path. */
function fileName(relativePath: string): string {
  const segments = relativePath.split("/").filter((segment) => segment !== "");
  return segments[segments.length - 1] ?? relativePath;
}

/**
 * The session's video target this embed names, or `undefined`.
 *
 * Matched by file NAME, not by comparing relative paths, for the reason the
 * properties panel matches the same way: Story 40.4 renames a session folder
 * after a note is written, so the note's path and the index's legitimately
 * disagree while the file name does not. `kind` is the index's answer to "is
 * this something a player means anything for", and it is the only thing that
 * may promote a link to a player — `manifest.json` is a target too.
 */
export function videoTargetFor(
  targets: readonly RecordingNoteTargetVm[] | null,
  embedded: string,
): RecordingNoteTargetVm | undefined {
  if (targets === null) {
    return undefined;
  }
  const name = fileName(embedded);
  return targets.find(
    (target) => target.kind === "video" && fileName(target.relativePath) === name,
  );
}

/** The ordinary wikilink: what an embed renders as before it resolves, and what
 *  it stays as when it resolves to nothing playable. */
function link(target: string, label: string): HTMLElement {
  const anchor = document.createElement("span");
  anchor.className = "cm-lp-wikilink";
  anchor.setAttribute(WIKILINK_ATTR, target);
  // `textContent`, never `innerHTML`: a note body is agent-authorable text.
  anchor.textContent = label;
  return anchor;
}

/**
 * Resolve `target` against `sessionId` and, if it is one of the session's
 * videos, replace `host`'s contents with a player.
 *
 * Never rejects and never empties the host: a failure is a rendering outcome
 * here, not an exception for someone else to handle, and the link the host
 * already holds is the correct answer to every one of them.
 */
export async function renderRecordingEmbedInto(
  host: HTMLElement,
  sessionId: string,
  target: string,
  options: RecordingEmbedOptions = {},
): Promise<void> {
  const load = options.load ?? recordingNoteTargets;
  let targets: RecordingNoteTargetVm[] | null = null;
  try {
    targets = await load(sessionId);
  } catch {
    // The index could not answer. That is the same fact as an unknown session
    // to the person reading the note, and it gets the same answer: the link.
    return;
  }
  if (options.cancelled?.() === true) {
    return;
  }
  const video = videoTargetFor(targets, target);
  if (video === undefined) {
    return;
  }

  // Whatever the host is showing now — the link — is what a failed load goes
  // back to, so nothing here can leave a player that will not play.
  const before = Array.from(host.childNodes);
  const player = document.createElement("video");
  player.className = "cm-lp-recording-player";
  player.controls = true;
  // Metadata only: a duration and a poster frame, not half a gigabyte.
  player.preload = "metadata";
  // The file name, so a screen reader hears which of a session's tracks this is.
  player.setAttribute("aria-label", fileName(video.relativePath));
  // Registered before `src`, because assigning it is what starts the load. The
  // path was one of this session's files a moment ago, so the way this fires is
  // the gap between then and the request: a Story 40.4 retitle in between, or a
  // removable volume that went away. Both mean the note's text is still true
  // and the player is not — and UX-DR44's rule is that the surface says the
  // smaller thing, never the alarming one.
  player.addEventListener("error", () => {
    host.replaceChildren(...before);
  });
  player.src = recordingAssetUrl(sessionId, video.relativePath);
  host.replaceChildren(player);
}

/**
 * Release the media element inside `dom`, if there is one.
 *
 * Removing the node is not enough. A `<video>` with a `src` holds a selected
 * resource — an open range-request pipeline and a decoder — until it is told to
 * let go, and a long editing session that scrolls past a dozen recordings would
 * otherwise accumulate a dozen of them against files that may live on a
 * removable volume the user then cannot eject.
 */
export function releaseRecordingVideo(dom: HTMLElement): void {
  const player = dom.querySelector("video");
  if (player === null) {
    return;
  }
  player.pause();
  player.removeAttribute("src");
  // `load()` is what actually aborts the selected resource; clearing `src`
  // alone only changes what the NEXT load would fetch.
  player.load();
  dom.replaceChildren();
}

/**
 * The CodeMirror widget that replaces a recording note's `![[…]]` embed.
 *
 * Only ever constructed from the editor's lazy chunk, and only for a note whose
 * frontmatter carries a `session:` — the predicate that makes a note a
 * recording note, decided by the renderer before this is reached.
 */
export class RecordingEmbedWidget extends WidgetType {
  /** Set by {@link destroy}, read by the render that may still be in flight. */
  private disposed = false;

  constructor(
    private readonly sessionId: string,
    private readonly target: string,
    private readonly label: string,
    private readonly options: RecordingEmbedOptions = {},
  ) {
    super();
  }

  /** Same session and same path, same player: CodeMirror may reuse the DOM,
   *  which is what keeps a playing video playing while the caret moves. */
  eq(other: RecordingEmbedWidget): boolean {
    return (
      other.sessionId === this.sessionId &&
      other.target === this.target &&
      other.label === this.label
    );
  }

  toDOM(): HTMLElement {
    const host = document.createElement("span");
    host.className = "cm-lp-recording";
    host.append(link(this.target, this.label));
    // Fired and forgotten, exactly as the mermaid fence is: the link is in the
    // document immediately and the player, if there is one, takes its place
    // when the index answers. Blocking `toDOM` on an IPC round trip would stall
    // the editor on every keystroke that rebuilds the decorations.
    void renderRecordingEmbedInto(host, this.sessionId, this.target, {
      ...this.options,
      cancelled: () => this.disposed || this.options.cancelled?.() === true,
    });
    return host;
  }

  destroy(dom: HTMLElement): void {
    this.disposed = true;
    releaseRecordingVideo(dom);
  }

  /**
   * Keep only the events aimed at the player itself.
   *
   * `true` here means CodeMirror ignores the event entirely — not just its own
   * default handling, but every registered handler, including the renderer's
   * wikilink `mousedown`. So the answer has to depend on what was clicked, and
   * the two halves of this widget want opposite things:
   *
   * - **The player keeps its events.** Letting them through would put the caret
   *   on the line, and a revealed line drops its decorations — so pressing play
   *   would destroy the player instead of starting it. The source stays
   *   reachable from the line above, the line below and the arrow keys, which
   *   is the trade `MermaidWidget` makes the other way round because a diagram
   *   has no controls to lose.
   * - **The degraded link gives them up**, so it behaves exactly like the
   *   wikilink it is: the click follows the target and the caret reveals the
   *   source, with no special case for having been rendered by this widget.
   */
  ignoreEvent(event: Event): boolean {
    return event.target instanceof Element && event.target.closest("video") !== null;
  }
}
