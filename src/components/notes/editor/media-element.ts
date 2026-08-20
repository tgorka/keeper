/**
 * One file, one element — whichever note it is embedded in (Story 55.4).
 *
 * # What this is, and where it came from
 *
 * Story 43.5 wrote the rule this module is named after: there is one question —
 * *what is this file and how should it be shown* — Rust answers it once as a
 * `kind`, and everything below the resolve is a branch on that answer (AD-73).
 * It wrote that rule inside `recording-embed.ts`, where the branch was, and the
 * only thing tying it there was one line: the URL, composed from a session id.
 *
 * So the branch moved out and the URL became a parameter. A recording note
 * hands it `keeper-recording://…`, an ordinary note hands it `keeper-note://…`,
 * and both get the same `<img>` — which is the point. Two modules that each
 * build an image element are two places for `loading="lazy"` to be true in one
 * of them.
 *
 * # What is NOT here
 *
 * Resolution. This module is handed a kind and a URL and never asks what a file
 * is: a recording note resolves through the recordings index, an ordinary note
 * through the vault's embed candidates, and those are different address spaces
 * that must not learn about each other (AD-65).
 *
 * The `file` kind is also not here. A recording note answers it with a chip of
 * actions — reveal, open — that only a session has, and an ordinary note
 * answers it with the link it already had. Neither is an element built from a
 * URL, so neither belongs to a function whose whole input is a URL.
 */
import { primeFirstFrame } from "./recording-transport";

/** The kinds this module can draw. Everything else is its caller's problem. */
export type DrawableKind = "image" | "video" | "audio" | "pdf";

export interface DrawableFile {
  readonly kind: DrawableKind;
  /** The file's own name, with no path in it: the alt text and the accessible
   *  name, because an embedded file has no caption anywhere else. */
  readonly name: string;
  /** Where the webview reads it from. Composed by the caller, because the
   *  scheme is the one thing the two callers do not share. */
  readonly url: string;
}

/**
 * The element for a file, with its failure handler already attached.
 *
 * `onFailedLoad` fires when the bytes do not arrive — a moved file, an
 * unmounted volume, a codec the engine will not open. Every caller answers it
 * the same way, by putting the link back: a dead player states that the file is
 * broken, and usually the file is fine.
 */
export function mediaElementFor(file: DrawableFile, onFailedLoad: () => void): HTMLElement {
  if (file.kind === "image") {
    const image = document.createElement("img");
    image.className = "cm-lp-recording-image";
    // The file name, because an embedded image has no caption anywhere else and
    // an empty `alt` would tell a screen reader it is decorative.
    image.alt = file.name;
    // Off-screen embeds in a long note cost nothing until they are scrolled to.
    image.loading = "lazy";
    image.decoding = "async";
    image.addEventListener("error", onFailedLoad);
    image.src = file.url;
    return image;
  }

  if (file.kind === "pdf") {
    // `<embed>`, not `<iframe>`: `document-viewer.tsx` states the reason for the
    // Files pane and it holds here — an iframe of a custom-scheme URL is a
    // different navigation the engine treats differently. The renderer is the
    // webview's own; keeper ships no PDF stack.
    const view = document.createElement("embed");
    view.className = "cm-lp-embed-pdf";
    view.setAttribute("type", "application/pdf");
    view.setAttribute("aria-label", file.name);
    // `<embed>` fires no `error` event for a URL that does not resolve, so this
    // is the one kind whose failure the caller must have ruled out before
    // calling — which it has: the path came back from a resolver that stats.
    view.setAttribute("src", file.url);
    return view;
  }

  const player = document.createElement(file.kind === "video" ? "video" : "audio");
  // The video class is the one Story 42.4 shipped and its rule — block, capped
  // height — is right for an audio bar too.
  player.className = file.kind === "video" ? "cm-lp-recording-player" : "cm-lp-recording-audio";
  player.controls = true;
  // Metadata only: a duration and a first frame, not half a gigabyte. The
  // "and a first frame" half is not free and is not the platform's default —
  // see {@link primeFirstFrame}, registered below.
  player.preload = "metadata";
  // The file name, so a screen reader hears which track this is.
  player.setAttribute("aria-label", file.name);
  player.addEventListener("error", onFailedLoad);
  if (player instanceof HTMLVideoElement) {
    // Audio has no frame to show, so it is asked for nothing it cannot use.
    primeFirstFrame(player);
  }
  // Assigned last, because assigning `src` is what starts the load and every
  // handler above must already be registered when it does.
  player.src = file.url;
  return player;
}

/** The last `/`-separated component of a relative path. */
export function fileNameOf(relativePath: string): string {
  const segments = relativePath.split("/").filter((segment) => segment !== "");
  return segments[segments.length - 1] ?? relativePath;
}
