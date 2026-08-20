/**
 * `![[photo.png]]` in an ordinary note (Story 55.4).
 *
 * # The gap this closes
 *
 * Until this module, a photograph embedded in a note rendered as a link to a
 * photograph — unless the note happened to be a *recording* note, in which case
 * Story 42.4's widget resolved it through the recordings index and drew it. The
 * capability existed; the address space did not. A recording note can name its
 * own files and an ordinary note cannot name anything but the vault.
 *
 * So this is the vault's half: `notes_embed_paths` resolves the target through
 * the same candidates the viewer opens and the export carries, and the element
 * is `media-element.ts`'s, the same one the recording widget builds. Nothing
 * here classifies a file — Rust returns the `kind` with the path (AD-87).
 *
 * # Order, and why this is asked last
 *
 * Three things can claim an `![[…]]`, and the order is not arbitrary:
 *
 * 1. **A data file** — `.csv`, `.json` — is Story 45.12's editable panel.
 * 2. **A recording note's own file** is Story 42.4's, and it answers whether it
 *    claimed the embed. In a recording note `manifest.json` under the session
 *    folder and `attachments/people.png` beside the note are different files
 *    with different owners, and only the index can tell them apart.
 * 3. **The vault** is what is left, which is this module.
 *
 * A target the vault does not hold is not an error and never rejects: the link
 * the host already holds is the correct answer, and it is the answer to an
 * unreachable volume and a failed load as well. A dead player states that the
 * file is broken; usually the file is fine and the note is the durable record.
 *
 * # No React, no registry barrel
 *
 * Both for `file-embed.ts`'s reasons: this lives in the editor's lazy chunk,
 * which is React-free, and it reaches `@/lib/viewers/registry` rather than the
 * barrel so the component table does not follow it in.
 */
import { WidgetType } from "@codemirror/view";
import { type NoteEmbedPathVm, notesEmbedPaths } from "@/lib/ipc/client";
import { resolveViewer } from "@/lib/viewers/registry";
import { type DrawableKind, fileNameOf, mediaElementFor } from "./media-element";
import { releaseMediaElement } from "./recording-transport";
import { WIKILINK_ATTR } from "./wikilink";

/** How the widget reaches the resolver. Injected so the degrade paths — which
 *  are the interesting ones — are reachable without a Tauri host. */
export type VaultEmbedResolver = (
  vaultId: string,
  targets: string[],
) => Promise<(NoteEmbedPathVm | null)[]>;

export interface VaultEmbedOptions {
  /** Overridden in tests; production always asks Rust. */
  readonly resolve?: VaultEmbedResolver;
  /** Compose the URL the webview reads the file over. The note editor's
   *  `assetUrl`, passed down, so the scheme and the encoding are stated once. */
  readonly assetUrl: (relPath: string) => string;
  /** True once the widget has been destroyed: a render in flight must not
   *  touch a host CodeMirror has thrown away. */
  readonly cancelled?: () => boolean;
}

/**
 * What a resolved file should be drawn as, or `null` for one this module does
 * not draw.
 *
 * Two answers, in the order they are allowed to be asked:
 *
 * - **Rust's `kind`** is the classifier, and the only thing that can tell an
 *   image from a video: those extensions are deliberately absent from the
 *   frontend's registry (`classifier-agreement.test.ts` pins that they are).
 * - **Inside the kind `file`**, and only there, the viewer registry refines by
 *   extension — which is its declared job, and how a `.pdf` is reached without
 *   a second classifier existing anywhere.
 *
 * Everything else — a `.zip`, a `.docx`, a `.csv` that reached here somehow —
 * is `null` and keeps its link.
 */
export function drawableFor(resolved: NoteEmbedPathVm): DrawableKind | null {
  if (resolved.kind === "image" || resolved.kind === "video" || resolved.kind === "audio") {
    return resolved.kind;
  }
  if (resolved.kind !== "file") {
    return null;
  }
  const entry = resolveViewer({ name: fileNameOf(resolved.relPath), kind: "file" });
  return entry.viewer === "document" && entry.format === "pdf" ? "pdf" : null;
}

/**
 * The host's own class, not `cm-lp-recording`.
 *
 * They behave identically and could have shared one, and that is exactly the
 * reason not to: a photograph in an ordinary note is not a recording, and a
 * class named for the wrong thing is how the next reader learns something
 * false. The *element* classes are shared, because those name what the element
 * is rather than where it came from.
 */
export const VAULT_EMBED_CLASS = "cm-lp-embed";

/** The ordinary wikilink the host starts as and degrades back to. */
function link(target: string): HTMLElement {
  const anchor = document.createElement("a");
  anchor.className = "cm-lp-wikilink";
  anchor.setAttribute(WIKILINK_ATTR, target);
  anchor.textContent = target;
  return anchor;
}

/**
 * Resolve `target` in `vaultId` and, if the vault holds a file this module
 * draws, replace `host`'s contents with its element.
 *
 * Never rejects and never empties the host.
 */
export async function renderVaultEmbedInto(
  host: HTMLElement,
  vaultId: string,
  target: string,
  options: VaultEmbedOptions,
): Promise<void> {
  const resolve = options.resolve ?? notesEmbedPaths;
  let resolved: NoteEmbedPathVm | null;
  try {
    [resolved = null] = await resolve(vaultId, [target]);
  } catch {
    // An unreachable volume, a vault that has gone away, a command that is not
    // there. All of them mean "no better answer than the link", and none of
    // them is this decoration's to report — the note is still readable.
    return;
  }
  if (resolved === null || options.cancelled?.() === true) {
    return;
  }
  const kind = drawableFor(resolved);
  if (kind === null) {
    return;
  }
  const element = mediaElementFor(
    { kind, name: fileNameOf(resolved.relPath), url: options.assetUrl(resolved.relPath) },
    () => {
      // Back to the link, in place. The bytes did not arrive, and a broken
      // player is a worse answer than the text the note actually holds.
      host.replaceChildren(link(target));
    },
  );
  host.replaceChildren(element);
}

/**
 * The CodeMirror widget that replaces an ordinary note's `![[…]]`.
 *
 * Constructed only from the editor's lazy chunk, and only for a target no
 * earlier branch claimed.
 */
export class VaultEmbedWidget extends WidgetType {
  /** Set by {@link destroy}, read by a render that may still be in flight. */
  private disposed = false;

  constructor(
    private readonly vaultId: string,
    private readonly target: string,
    private readonly options: VaultEmbedOptions,
  ) {
    super();
  }

  eq(other: VaultEmbedWidget): boolean {
    return other.vaultId === this.vaultId && other.target === this.target;
  }

  toDOM(): HTMLElement {
    const host = document.createElement("span");
    host.className = VAULT_EMBED_CLASS;
    host.append(link(this.target));
    // Fired and forgotten, as every other embed in this editor is: the link is
    // in the document immediately and the element takes its place when Rust
    // answers. Blocking `toDOM` on IPC would stall the editor on every
    // keystroke that rebuilds the decorations.
    void renderVaultEmbedInto(host, this.vaultId, this.target, {
      ...this.options,
      cancelled: () => this.disposed || this.options.cancelled?.() === true,
    });
    return host;
  }

  destroy(dom: HTMLElement): void {
    this.disposed = true;
    const player = dom.querySelector("video, audio");
    if (player instanceof HTMLMediaElement) {
      // A `<video>` with a `src` holds a decoder and a buffer until the element
      // is told to let go; dropping the node is not telling it.
      releaseMediaElement(player);
    }
    dom.replaceChildren();
  }

  /** The same split `RecordingEmbedWidget` documents: a control keeps its
   *  events, because letting them through reveals the line and un-renders the
   *  player mid-gesture; everything else behaves like the wikilink it stands
   *  for. */
  ignoreEvent(event: Event): boolean {
    return event.target instanceof Element && event.target.closest("video, audio, embed") !== null;
  }
}
