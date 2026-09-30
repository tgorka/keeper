/**
 * What an `![[clip.mov]]` embed of a video or an audio file is in a note: a
 * compact chip, not a player.
 *
 * The note used to mount a native `<video>` per embed, and a recording note a
 * row of them sharing one hand-built transport. The owner's verdict on that was
 * that it "works badly, and the new widget is better and more intentional" —
 * the `keeper-media` block is where a recording plays, with its parts as one
 * timeline and its transcript under it. So an embed says what the file is and
 * offers the one move that matters, **Play in a player**, which turns the embed
 * into that block.
 *
 * **Rust composes the block, the editor splices it.** The chip sends the note's
 * text, the embed's line and the target as written to `media_block_for_embed`;
 * Rust decides whether the target is one of the note's session's files (then
 * every media embed of that session collapses into ONE block naming the
 * session) or a plain file (a block with one `[[part]]`), and answers with line
 * edits. Nothing here reads a path or composes a block (AD-65). The edits are
 * applied in one transaction, so ⌘Z brings the embeds back.
 *
 * **No bytes are fetched.** The chip names the file and nothing else, so a note
 * full of embeds costs nothing to open.
 */
import type { EditorView } from "@codemirror/view";
import { type LineEditVm, mediaBlockForEmbed, revealPath } from "@/lib/ipc/client";
import { capabilitiesStore } from "@/lib/stores/capabilities";
import { fileNameOf } from "./media-element";

/** The chip's primary action. */
export const PLAY_IN_PLAYER_LABEL = "Play in a player";

/** The repo's one wording for revealing a file (`NOTE_REVEAL_LABEL`, …). */
export const MEDIA_CHIP_REVEAL_LABEL = "Reveal in Finder";

/** The Copy path action's label, same one wording. */
export const MEDIA_CHIP_COPY_PATH_LABEL = "Copy path";

/** The chip's own class; its buttons keep their events (see `ignoreEvent`). */
export const MEDIA_CHIP_CLASS = "cm-lp-media-chip";

/** The class every chip action carries. */
export const MEDIA_CHIP_ACTION_CLASS = "cm-lp-media-chip-action";

export interface MediaChipFile {
  kind: "video" | "audio";
  /** The path as the note or the index knows it: the tooltip (FR-145). */
  relativePath: string;
  /** Only ever an action's argument — never rendered. */
  absolutePath: string;
}

/** Lucide's `film` and `audio-lines`, drawn here because the editor's chunk
 *  carries no React and so no icon components. */
const ICON_PATHS: Record<MediaChipFile["kind"], string> = {
  video:
    '<rect width="18" height="18" x="3" y="3" rx="2"/><path d="M7 3v18"/><path d="M3 7.5h4"/><path d="M3 12h18"/><path d="M3 16.5h4"/><path d="M17 3v18"/><path d="M17 7.5h4"/><path d="M17 16.5h4"/>',
  audio:
    '<path d="M2 10v3"/><path d="M6 6v11"/><path d="M10 3v18"/><path d="M14 8v7"/><path d="M18 5v13"/><path d="M22 10v3"/>',
};

function icon(kind: MediaChipFile["kind"]): SVGSVGElement {
  const svg = document.createElementNS("http://www.w3.org/2000/svg", "svg");
  for (const [name, value] of Object.entries({
    viewBox: "0 0 24 24",
    width: "14",
    height: "14",
    fill: "none",
    stroke: "currentColor",
    "stroke-width": "2",
    "stroke-linecap": "round",
    "stroke-linejoin": "round",
    "aria-hidden": "true",
    class: "cm-lp-media-chip-icon",
  })) {
    svg.setAttribute(name, value);
  }
  // A constant of this module, never note text.
  svg.innerHTML = ICON_PATHS[kind];
  return svg;
}

/** One action: a real `<button>`, named with the file so a note of four chips
 *  is not four identical "Copy path" controls to a screen reader. */
function action(label: string, name: string, run: () => void): HTMLButtonElement {
  const button = document.createElement("button");
  button.type = "button";
  button.className = MEDIA_CHIP_ACTION_CLASS;
  button.setAttribute("aria-label", `${label} ${name}`);
  button.textContent = label;
  button.addEventListener("click", run);
  return button;
}

/**
 * Apply Rust's line edits to the document, in one transaction.
 *
 * `text` replaces lines `firstLine..lastLine` (1-based, inclusive) and carries
 * no terminator of its own; `null` deletes those lines with their newline.
 */
export function applyLineEdits(view: EditorView, edits: readonly LineEditVm[]): void {
  const doc = view.state.doc;
  const changes = edits.map((edit) => {
    const first = doc.line(edit.firstLine);
    const last = doc.line(edit.lastLine);
    if (edit.text !== null) {
      return { from: first.from, to: last.to, insert: edit.text };
    }
    // Deleting the last lines of the document takes the newline BEFORE them,
    // so the note does not keep a dangling empty line.
    if (edit.lastLine < doc.lines) {
      return { from: first.from, to: doc.line(edit.lastLine + 1).from, insert: "" };
    }
    return {
      from: edit.firstLine > 1 ? doc.line(edit.firstLine - 1).to : 0,
      to: last.to,
      insert: "",
    };
  });
  view.dispatch({ changes, userEvent: "input.media-block", scrollIntoView: true });
}

/**
 * Ask Rust for the block the embed at `host` becomes, and splice it.
 *
 * The embed's line is found again from the DOM at the moment of the press, and
 * a document edited while Rust was answering is asked again — the gallery's
 * rule: an edit computed against old text is never applied to new text.
 * Resolves with the refusal sentence, or `null` when the block was written.
 */
export async function playInPlayer(
  view: EditorView,
  host: HTMLElement,
  profileId: string,
  target: string,
): Promise<string | null> {
  for (let attempt = 0; attempt < 2; attempt += 1) {
    const doc = view.state.doc;
    let line: number;
    try {
      line = doc.lineAt(view.posAtDOM(host)).number;
    } catch {
      return null;
    }
    let edits: LineEditVm[];
    try {
      edits = await mediaBlockForEmbed(profileId, doc.toString(), line, target);
    } catch (cause) {
      const message = (cause as { message?: unknown } | null)?.message;
      return typeof message === "string" && message !== ""
        ? message
        : "keeper could not make a player for this file.";
    }
    if (view.state.doc === doc) {
      applyLineEdits(view, edits);
      return null;
    }
  }
  return "The note changed while keeper was answering. Press Play in a player again.";
}

/**
 * The chip for one video or audio embed.
 *
 * `profileId` empty means there is no drive to compose a block against — a
 * markdown file previewed outside any synced folder — and a read-only view
 * cannot take the edit; in both the Play action is absent rather than a
 * button that refuses (AD-27).
 */
export function mediaChip(
  file: MediaChipFile,
  play: { view: EditorView; profileId: string; target: string } | null,
): HTMLElement {
  const name = fileNameOf(file.relativePath);
  const node = document.createElement("span");
  node.className = MEDIA_CHIP_CLASS;
  node.append(icon(file.kind));

  const label = document.createElement("span");
  label.className = "cm-lp-media-chip-name";
  label.textContent = name;
  label.title = file.relativePath;
  node.append(label);

  const status = document.createElement("span");
  status.className = "cm-lp-media-chip-status";
  status.setAttribute("role", "status");

  if (play !== null && play.profileId !== "" && !play.view.state.readOnly) {
    const button = action(PLAY_IN_PLAYER_LABEL, name, () => {
      button.disabled = true;
      status.textContent = "";
      void playInPlayer(play.view, node, play.profileId, play.target).then((refusal) => {
        button.disabled = false;
        status.textContent = refusal ?? "";
      });
    });
    button.classList.add("cm-lp-media-chip-play");
    node.append(button);
  }
  if (capabilitiesStore.getState().capabilities.revealInFileManager) {
    node.append(
      action(MEDIA_CHIP_REVEAL_LABEL, name, () => {
        void revealPath(file.absolutePath).catch(() => {});
      }),
    );
  }
  node.append(
    action(MEDIA_CHIP_COPY_PATH_LABEL, name, () => {
      void navigator.clipboard?.writeText(file.absolutePath).catch(() => {});
    }),
  );
  node.append(status);
  return node;
}
