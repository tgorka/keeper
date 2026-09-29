/**
 * The one embed widget for a recording note's `![[…]]` (Story 42.4, FR-142,
 * FR-145, AD-65; widened by Story 43.5, FR-150, AD-73).
 *
 * A recording note names its recording only in relative terms, because FR-145
 * keeps absolute paths out of a file the user syncs between machines. This
 * turns that text into something the reader can look at or act on without
 * leaving the note.
 *
 * **One widget, branching on Rust's `kind`.** An image is drawn; a video or an
 * audio file is a chip whose primary action is *Play in a player*, which turns
 * the session's embeds into one `keeper-media` block (`media-chip.ts`); any
 * other file — the manifest, a transcript — is a chip carrying Reveal and Copy
 * path. A note no longer mounts a `<video>` per embed: the media block is where
 * a recording plays, with its parts as one timeline and its transcript under it.
 *
 * **The session id is the key, never the path.** Resolution goes through
 * `recording_note_targets`, which answers from Story 42.1's index, so an embed
 * written before a Story 40.4 retitle still finds its file — and the frontend
 * joins nothing (AD-65).
 *
 * **The link is what renders first, and an element only ever replaces it.**
 * `toDOM` returns the ordinary wikilink synchronously, and the chip or image
 * takes its place when — and only when — the embed names a file of that
 * session. An unknown path, an unreachable volume and an IPC failure all leave
 * the reader the working link they would have had without this module.
 */
import { type EditorView, WidgetType } from "@codemirror/view";
import { type RecordingNoteTargetVm, recordingNoteTargets, revealPath } from "@/lib/ipc/client";
import { capabilitiesStore } from "@/lib/stores/capabilities";
import {
  MEDIA_CHIP_ACTION_CLASS,
  MEDIA_CHIP_COPY_PATH_LABEL,
  MEDIA_CHIP_REVEAL_LABEL,
  mediaChip,
} from "./media-chip";
import { fileNameOf, mediaElementFor } from "./media-element";
import { recordingAssetUrl } from "./media-playback";
import { WIKILINK_ATTR } from "./wikilink";

/** How the widget reaches the index. Injected so the degrade paths — which are
 *  the interesting ones — are reachable in a test without a Tauri host. */
export type RecordingTargetLoader = (sessionId: string) => Promise<RecordingNoteTargetVm[] | null>;

export interface RecordingEmbedOptions {
  /** Overridden in tests; production always asks Rust. */
  load?: RecordingTargetLoader;
  /** Whether the host has been torn down since the render began. */
  cancelled?: () => boolean;
  /**
   * Where *Play in a player* writes: the view the embed sits in and the drive
   * the note is in. Absent — a data-file panel asking the session first — a
   * video or audio target is still a chip, without that action.
   */
  play?: { view: EditorView; profileId: string };
}

/**
 * The session's target this embed names, or `undefined`.
 *
 * Matched by file NAME, not by comparing relative paths, for the reason the
 * properties panel matches the same way: Story 40.4 renames a session folder
 * after a note is written, so the note's path and the index's legitimately
 * disagree while the file name does not. A folder is not an embed.
 */
export function attachmentTargetFor(
  targets: readonly RecordingNoteTargetVm[] | null,
  embedded: string,
): RecordingNoteTargetVm | undefined {
  if (targets === null) {
    return undefined;
  }
  const name = fileNameOf(embedded);
  return targets.find(
    (target) => target.kind !== "folder" && fileNameOf(target.relativePath) === name,
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

/** One action on a file chip: a real `<button>`, named with the file. */
function chipAction(label: string, name: string, run: () => void): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = MEDIA_CHIP_ACTION_CLASS;
  button.setAttribute("aria-label", `${label} ${name}`);
  button.textContent = label;
  button.addEventListener("click", run);
  return button;
}

/**
 * The chip for a file keeper does not draw: the manifest, a transcript, a PDF.
 *
 * It fetches nothing. The visible text is the file NAME and the tooltip the
 * note's own relative path (FR-145); the absolute path is only ever an
 * action's argument.
 */
function fileChip(target: RecordingNoteTargetVm): HTMLElement {
  const name = fileNameOf(target.relativePath);
  const node = document.createElement("span");
  node.className = "cm-lp-recording-chip";

  const label = document.createElement("span");
  label.className = "cm-lp-recording-chip-name";
  label.textContent = name;
  label.title = target.relativePath;
  node.append(label);

  // Absent, never disabled, on a platform with no user-visible file manager.
  if (capabilitiesStore.getState().capabilities.revealInFileManager) {
    node.append(
      chipAction(MEDIA_CHIP_REVEAL_LABEL, name, () => {
        void revealPath(target.absolutePath).catch(() => {});
      }),
    );
  }
  node.append(
    chipAction(MEDIA_CHIP_COPY_PATH_LABEL, name, () => {
      void navigator.clipboard?.writeText(target.absolutePath).catch(() => {});
    }),
  );
  return node;
}

/**
 * Resolve `target` against `sessionId` and, if it is one of the session's
 * files, replace `host`'s contents with what its kind is drawn as.
 *
 * Never rejects and never empties the host. Answers **whether it claimed the
 * embed**: `false` means "not one of this session's files, and the link is
 * still what the host holds", so a caller may look elsewhere.
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
    // The index could not answer: the same fact as an unknown session to the
    // person reading the note, and the same answer — the link.
    return false;
  }
  if (options.cancelled?.() === true) {
    return false;
  }
  const attachment = attachmentTargetFor(targets, target);
  if (attachment === undefined) {
    return false;
  }
  if (attachment.kind === "video" || attachment.kind === "audio") {
    host.replaceChildren(
      mediaChip(
        {
          kind: attachment.kind,
          relativePath: attachment.relativePath,
          absolutePath: attachment.absolutePath,
        },
        options.play === undefined ? null : { ...options.play, target },
      ),
    );
    return true;
  }
  if (attachment.kind === "image") {
    const before = Array.from(host.childNodes);
    host.replaceChildren(
      mediaElementFor(
        {
          kind: "image",
          name: fileNameOf(attachment.relativePath),
          url: recordingAssetUrl(sessionId, attachment.relativePath),
        },
        // A failed load puts the link back: a broken image states that the
        // recording is broken, and usually it is not.
        () => host.replaceChildren(...before),
      ),
    );
    return true;
  }
  host.replaceChildren(fileChip(attachment));
  return true;
}

/**
 * The CodeMirror widget that replaces a recording note's `![[…]]` embed.
 *
 * Only ever constructed from the editor's lazy chunk, and only for a note whose
 * frontmatter carries a `session:`.
 */
export class RecordingEmbedWidget extends WidgetType {
  /** Set by {@link destroy}, read by the render that may still be in flight. */
  private disposed = false;

  constructor(
    private readonly sessionId: string,
    private readonly target: string,
    private readonly label: string,
    private readonly profileId: string,
    private readonly options: Omit<RecordingEmbedOptions, "play"> = {},
  ) {
    super();
  }

  eq(other: RecordingEmbedWidget): boolean {
    return (
      other.sessionId === this.sessionId &&
      other.target === this.target &&
      other.label === this.label &&
      other.profileId === this.profileId
    );
  }

  toDOM(view: EditorView): HTMLElement {
    const host = document.createElement("span");
    host.className = "cm-lp-recording";
    host.append(link(this.target, this.label));
    // Fired and forgotten: the link is in the document immediately and the
    // chip takes its place when the index answers.
    void renderRecordingEmbedInto(host, this.sessionId, this.target, {
      ...this.options,
      play: { view, profileId: this.profileId },
      cancelled: () => this.disposed || this.options.cancelled?.() === true,
    });
    return host;
  }

  destroy(): void {
    this.disposed = true;
  }

  /**
   * Keep only the events aimed at a control inside the widget: letting a
   * button's click through would put the caret on the line, and a revealed
   * line drops its decorations — so pressing Copy path would destroy the chip
   * instead of copying. Everything else behaves like the wikilink it stands
   * for.
   */
  ignoreEvent(event: Event): boolean {
    return event.target instanceof Element && event.target.closest("button") !== null;
  }
}
