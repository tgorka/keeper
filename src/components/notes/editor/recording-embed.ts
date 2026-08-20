/**
 * The one embed widget for a recording note's `![[…]]` (Story 42.4, FR-142,
 * FR-145, AD-65; widened by Story 43.5, FR-150, AD-73).
 *
 * A recording note names its recording only in relative terms, because FR-145
 * keeps absolute paths out of a file the user syncs between machines. The
 * properties panel turns that text into *actions*; this turns it into something
 * the reader can watch, look at, listen to or act on without leaving the note.
 *
 * **One widget, branching at the element — not one module per medium.** The
 * obvious shape for "also show photos and play audio" is a second and a third
 * module beside this one, and it is the wrong shape: three widgets means three
 * parsers, three resolution paths, three degrade behaviours and three places to
 * fix the next bug. There is one question — *what is this file and how should
 * it be shown* — Rust answers it once as a `kind`, and everything below the
 * resolve is a four-way branch on that answer (AD-73).
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
 * **The link is what renders first, and an element only ever replaces it.**
 * `toDOM` returns the ordinary wikilink synchronously and the resolved element
 * takes its place when — and only when — the embed names a file of that
 * session. So a path this session does not have, an unreachable volume and an
 * IPC failure all leave the reader with the same working link they would have
 * had if this module did not exist. A dead player is worse than a plain link in
 * the way that matters: a link states what the note says and does something
 * when clicked, while a player that cannot load states that the recording is
 * broken — and the recording is fine. The note is the durable record; being
 * wrong about it is the one unrecoverable thing a renderer can do.
 *
 * **A file keeper cannot render is still an attachment.** That same rule is why
 * a `kind: "file"` — the manifest, a transcript, a PDF, an extension nobody
 * anticipated — becomes a chip carrying Reveal and Copy path rather than a
 * `<video>` that will never load or a bare line of text. The chip fetches
 * nothing, which is also why `keeper-recording://` refuses to serve its bytes:
 * no element asks for them.
 *
 * **No autoplay, and metadata only.** These are multi-hundred-megabyte screen
 * recordings on what may be a removable or network volume. A media element is
 * given `preload="metadata"` and an image `loading="lazy"`, so opening a note
 * costs a duration and a first frame rather than a download, and playback
 * starts when a person presses play. {@link releaseRecordingMedia} is the other
 * half of that promise: a widget scrolled out of the viewport must give the
 * bytes back.
 */
import { WidgetType } from "@codemirror/view";
import { type RecordingNoteTargetVm, recordingNoteTargets, revealPath } from "@/lib/ipc/client";
import { capabilitiesStore } from "@/lib/stores/capabilities";
import { mediaElementFor } from "./media-element";
import { type RecordingTransport, releaseHost, releaseMediaElement } from "./recording-transport";
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
  /**
   * The clock this session's videos share (Story 43.6).
   *
   * Passed in rather than looked up, because "the same session" only means the
   * same pair within one editor: `live-preview.ts` holds the view and is the
   * only caller that can say which one. Absent — a widget driven directly by a
   * test, or a note rendered outside the decoration layer — every video keeps
   * its own native controls, which is also what a lone video gets.
   */
  transport?: RecordingTransport;
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
 * The Reveal action's label on a file chip.
 *
 * Spelled again rather than imported from `properties-panel.tsx`: that module
 * is React and this one is a CodeMirror widget in the editor's lazy chunk, and
 * pulling React in to share a string would be the more expensive mistake. The
 * wording is the repo's one wording for this affordance — `NOTE_REVEAL_LABEL`,
 * `RECORDINGS_REVEAL_LABEL`, `REVEAL_IN_FINDER_LABEL` — and it must stay so.
 */
export const RECORDING_EMBED_REVEAL_LABEL = "Reveal in Finder";

/** The Copy path action's label on a file chip, same one wording. */
export const RECORDING_EMBED_COPY_PATH_LABEL = "Copy path";

/**
 * The session's target this embed names, or `undefined`.
 *
 * Matched by file NAME, not by comparing relative paths, for the reason the
 * properties panel matches the same way: Story 40.4 renames a session folder
 * after a note is written, so the note's path and the index's legitimately
 * disagree while the file name does not.
 *
 * The folder is excluded and nothing else is. Story 42.4 admitted only
 * `kind: "video"` here, so every other target fell through to the link; Story
 * 43.5 renders all four file kinds, and the kind decides WHICH element rather
 * than WHETHER there is one. A folder is still not an embed: `![[…/2026]]`
 * names a directory, and there is no element for a directory.
 */
export function attachmentTargetFor(
  targets: readonly RecordingNoteTargetVm[] | null,
  embedded: string,
): RecordingNoteTargetVm | undefined {
  if (targets === null) {
    return undefined;
  }
  const name = fileName(embedded);
  return targets.find(
    (target) => target.kind !== "folder" && fileName(target.relativePath) === name,
  );
}

/** The ordinary wikilink: what an embed renders as before it resolves, and what
 *  it stays as when it resolves to nothing this session has. */
function link(target: string, label: string): HTMLElement {
  const anchor = document.createElement("span");
  anchor.className = "cm-lp-wikilink";
  anchor.setAttribute(WIKILINK_ATTR, target);
  // `textContent`, never `innerHTML`: a note body is agent-authorable text.
  anchor.textContent = label;
  return anchor;
}

/** One action on a file chip: a real `<button>`, so it is reachable by keyboard
 *  and announced as a control rather than as decorated text. */
function chipAction(label: string, name: string, run: () => void): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = "cm-lp-recording-chip-action";
  // The name is in the accessible name because a note may embed four of a
  // session's files, and four identical "Copy path" buttons are one control
  // said four times to anyone not looking at the screen.
  button.setAttribute("aria-label", `${label} ${name}`);
  button.textContent = label;
  button.addEventListener("click", run);
  return button;
}

/**
 * The chip for a file keeper cannot render inline.
 *
 * It fetches nothing — no `src`, no request to `keeper-recording://` — because
 * there is no element that could show what it names. What it offers instead is
 * the two actions the properties panel already offers for the same file, at the
 * place in the note where the author put the embed.
 *
 * The visible text is the file NAME and the tooltip is the note's own relative
 * path (FR-145). The absolute path is the argument of an action and never
 * appears on screen, so nothing here can leak a home directory into a
 * screenshot of a note.
 */
function chip(target: RecordingNoteTargetVm): HTMLElement {
  const name = fileName(target.relativePath);
  const node = document.createElement("span");
  node.className = "cm-lp-recording-chip";

  const label = document.createElement("span");
  label.className = "cm-lp-recording-chip-name";
  label.textContent = name;
  label.title = target.relativePath;
  node.append(label);

  // Absent, never disabled, on a platform with no user-visible file manager:
  // the same gate the panel and the recordings browser apply, and for the same
  // reason — an affordance that cannot work is worse than an absent one.
  if (capabilitiesStore.getState().capabilities.revealInFileManager) {
    node.append(
      chipAction(RECORDING_EMBED_REVEAL_LABEL, name, () => {
        // Best effort: the reveal either happens or the file manager said no,
        // and neither is something to interrupt a note with.
        void revealPath(target.absolutePath).catch(() => {});
      }),
    );
  }
  node.append(
    chipAction(RECORDING_EMBED_COPY_PATH_LABEL, name, () => {
      // The absolute path is the useful one — it pastes into a terminal or a
      // Finder "Go to folder" and lands.
      void navigator.clipboard?.writeText(target.absolutePath).catch(() => {});
    }),
  );
  return node;
}

/**
 * The element for one resolved target, and the branch this whole story is.
 *
 * Everything above the branch — the parse, the resolve, the degrade, the
 * teardown — is shared, which is the point: adding a medium is a case here, not
 * a module.
 *
 * The three media kinds get an `onFailedLoad` handler because they fetch, and
 * a fetch that fails must put the link back. The chip fetches nothing and
 * therefore cannot fail.
 */
function elementFor(
  sessionId: string,
  target: RecordingNoteTargetVm,
  onFailedLoad: () => void,
): HTMLElement {
  // Before the URL: the chip requests no bytes, so composing one for it would
  // be a string built to be thrown away.
  //
  // `folder` joins it, which is a change of behaviour: the branch below used to
  // be "video or else audio", so a target that was a directory became an
  // `<audio controls>` named after it. `kind_for_file_name` never returns
  // `folder`, so this was almost certainly unreachable for a recording target —
  // but "almost certainly unreachable" and "silently draws an audio player" are
  // a poor pair, and lifting the drawable kinds into their own type in Story
  // 55.4 is what asked the question.
  if (target.kind === "file" || target.kind === "folder") {
    return chip(target);
  }
  // The three drawable kinds are `media-element.ts`'s, so that an ordinary
  // note's photograph and a recording's photograph are the same `<img>` rather
  // than two that resemble each other (Story 55.4). What stays here is the URL,
  // which is the one thing the two address spaces do not share.
  return mediaElementFor(
    {
      kind: target.kind,
      name: fileName(target.relativePath),
      url: recordingAssetUrl(sessionId, target.relativePath),
    },
    onFailedLoad,
  );
}

/**
 * Resolve `target` against `sessionId` and, if it is one of the session's
 * files, replace `host`'s contents with the element for its kind.
 *
 * Never rejects and never empties the host: a failure is a rendering outcome
 * here, not an exception for someone else to handle, and the link the host
 * already holds is the correct answer to every one of them.
 *
 * Answers **whether it claimed the embed**, because in a recording note there
 * is a second place a target can live and the caller is the only one that can
 * look there. A recording session and a notes vault are different address
 * spaces: `manifest.json` under the session's folder is this module's, and
 * `attachments/people.csv` in the vault beside the note is Story 45.12's panel.
 * `false` means "not one of this session's files, and the link is still what
 * the host holds" — it does NOT mean the file is missing, so a caller may
 * safely go and look elsewhere. The two failures above answer `false` as well
 * and deliberately: the index not answering is not evidence about the vault
 * either, and the honest outcome then is the link the host already has.
 */
export async function renderRecordingEmbedInto(
  host: HTMLElement,
  sessionId: string,
  target: string,
  options: RecordingEmbedOptions = {},
): Promise<boolean> {
  const load = options.load ?? recordingNoteTargets;
  let targets: RecordingNoteTargetVm[] | null = null;
  try {
    targets = await load(sessionId);
  } catch {
    // The index could not answer. That is the same fact as an unknown session
    // to the person reading the note, and it gets the same answer: the link.
    return false;
  }
  if (options.cancelled?.() === true) {
    return false;
  }
  const attachment = attachmentTargetFor(targets, target);
  if (attachment === undefined) {
    return false;
  }

  // Whatever the host is showing now — the link — is what a failed load goes
  // back to, so nothing here can leave a player that will not play. The way
  // that fires is the gap between the resolve and the request: a Story 40.4
  // retitle in between, or a removable volume that went away. Both mean the
  // note's text is still true and the element is not, and UX-DR44's rule is
  // that the surface says the smaller thing, never the alarming one.
  const before = Array.from(host.childNodes);
  const element = elementFor(sessionId, attachment, () => {
    // Leave the pair FIRST. A track whose file went away has left the group,
    // and while the group is staged its element lives in the leader's stage
    // rather than here — `releaseHost` hands it back to this host, which the
    // restore below then replaces with the link. Restoring first would put the
    // link back and then have the dead element handed in underneath it.
    releaseHost(host);
    host.replaceChildren(...before);
  });
  host.replaceChildren(element);

  // After the mount, never before: the transport decides which track leads by
  // where it sits in the note, and an element with no parent sits nowhere.
  if (options.transport !== undefined && element instanceof HTMLVideoElement) {
    options.transport.join(element, host, fileName(attachment.relativePath));
  }
  return true;
}

/**
 * Release the media element this host is responsible for, if there is one.
 *
 * Removing the node is not enough. A `<video>` or `<audio>` with a `src` holds
 * a selected resource — an open range-request pipeline and a decoder — until it
 * is told to let go, and a long editing session that scrolls past a dozen
 * recordings would otherwise accumulate a dozen of them against files that may
 * live on a removable volume the user then cannot eject.
 *
 * An `<img>` and a chip hold nothing a browser will not collect on its own, so
 * a host holding one is left exactly as it is — which is also what makes this
 * safe to call on the degraded link.
 */
export function releaseRecordingMedia(dom: HTMLElement): void {
  // Before the search, not after. A grouped track does not live in its own
  // host — the transport moved it into the shared stage so the pair reads as
  // one player — so looking here first would find nothing and release nothing.
  // `releaseHost` hands the element back to `dom` and, when that drops the
  // group to a single track, gives the survivor its native controls and its
  // own host's contents back before either is torn down.
  releaseHost(dom);
  const player = dom.querySelector("video, audio");
  if (!(player instanceof HTMLMediaElement)) {
    return;
  }
  releaseMediaElement(player);
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
    releaseRecordingMedia(dom);
  }

  /**
   * Keep only the events aimed at a control inside the widget.
   *
   * `true` here means CodeMirror ignores the event entirely — not just its own
   * default handling, but every registered handler, including the renderer's
   * wikilink `mousedown`. So the answer has to depend on what was clicked, and
   * the halves of this widget want opposite things:
   *
   * - **A control keeps its events.** Letting them through would put the caret
   *   on the line, and a revealed line drops its decorations — so pressing play
   *   would destroy the player instead of starting it, and pressing Copy path
   *   would destroy the chip instead of copying. The source stays reachable
   *   from the line above, the line below and the arrow keys, which is the
   *   trade `MermaidWidget` makes the other way round because a diagram has no
   *   controls to lose. Story 43.6 put `input` on that list: the shared
   *   transport's scrub bar and each track's volume are `<input type=range>`,
   *   and a drag that reveals the line un-renders the slider mid-gesture.
   * - **Everything else gives them up** — the degraded link, an `<img>`, the
   *   chip's own name — so each behaves exactly like the wikilink it stands
   *   for: the click follows the target and the caret reveals the source, with
   *   no special case for having been rendered by this widget.
   */
  ignoreEvent(event: Event): boolean {
    return (
      event.target instanceof Element &&
      event.target.closest("video, audio, button, input") !== null
    );
  }
}
