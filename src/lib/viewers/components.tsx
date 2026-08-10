/**
 * Which component renders which viewer (Story 45.2, AD-87).
 *
 * **The registry is the only thing that knows this.** A surface calls
 * {@link viewerComponentFor} and mounts what it gets back; it does not switch
 * on a kind, a format or an extension. That is what makes "it opens in Files
 * but not in a note" impossible rather than merely unlikely — the two surfaces
 * cannot hold different opinions because neither of them holds one.
 *
 * **A static table, not a `register()` call.** Registration at import time
 * makes what renders depend on which module a bundler happened to evaluate
 * first, and a viewer that fails to render because a side effect did not run
 * is a bug with no stack trace. Adding a viewer is one line in
 * {@link VIEWER_COMPONENTS} and one row in `registry.ts` — the "add a row, not
 * a surface" AD-87 asks for.
 *
 * **An unbound viewer id is visible, not silent.** Three tray listeners
 * shipped in epic 44 declared and never mounted (DW-172), and the thing that
 * let them ship was that nothing said so. A viewer id with no component
 * resolves to the unknown viewer — which names the format it could not draw —
 * AND logs once at `console.info` naming the id. Once, not per render: this is
 * called from a render path, and a line per frame is a line nobody reads.
 */

import { DocumentViewer } from "@/components/viewers/document-viewer";
import { MediaViewer } from "@/components/viewers/media-viewer";
import { TextFileViewer } from "@/components/viewers/text-file-viewer";
import { resolveViewer } from "./registry";
import type { ViewerComponent, ViewerEntry, ViewerFile, ViewerId } from "./types";
import { UnknownViewer } from "./unknown-viewer";

/**
 * The bindings. **Add your viewer here and nowhere else.**
 *
 * `Partial`, deliberately: a wave of this epic lands its viewers one story at
 * a time, and a total `Record` would force a placeholder component for every
 * id that has not arrived — which is how a placeholder ends up shipping. An
 * absent binding is an honest absence and is reported as one.
 */
export const VIEWER_COMPONENTS: Partial<Record<ViewerId, ViewerComponent>> = Object.freeze({
  // Story 45.7. Three ids, ONE component: 43.5's shape, where the medium
  // decides the element rather than the module, so a fourth medium is a case
  // and not a file. The component branches on `entry.viewer`, never on a name.
  video: MediaViewer,
  image: MediaViewer,
  audio: MediaViewer,
  // Story 45.4. Raw and rendered are ONE component under one id (AD-88), so
  // there is no separate "editor" binding beside this one: `TextFileViewer`
  // loads the bytes and mounts 45.6's editor as its raw half.
  text: TextFileViewer,
  // Story 45.8. One id for four formats, for the same reason media is three
  // ids and one component: the format decides the body, not the module.
  // `DocumentViewer` mounts the webview's own PDF renderer for a PDF and Rust's
  // bounded projection for DOCX, PPTX and XLSX, and degrades to the unknown
  // viewer — with the reason Rust worded — for anything it cannot read.
  document: DocumentViewer,
  unknown: UnknownViewer,
});

/** Ids already reported, so a render loop logs once rather than once a frame. */
const reportedUnbound = new Set<ViewerId>();

/** A resolved viewer: the row that chose it, and the component to mount. */
export interface ResolvedViewer {
  /** The row {@link resolveViewer} returned — the format keeper believes this
   *  file is, even when the component that draws it is not bound yet. */
  readonly entry: ViewerEntry;
  /** What to mount. Never `undefined`. */
  readonly Component: ViewerComponent;
}

/**
 * The component for one row, falling back to the unknown viewer and saying so.
 *
 * `components` is a parameter so the fallback can be exercised against an
 * empty table forever, instead of by a test that quietly stops testing
 * anything the day the last viewer is bound.
 */
export function resolveViewerComponent(
  entry: ViewerEntry,
  components: Partial<Record<ViewerId, ViewerComponent>> = VIEWER_COMPONENTS,
): ViewerComponent {
  const bound = components[entry.viewer];
  if (bound !== undefined) {
    return bound;
  }
  if (!reportedUnbound.has(entry.viewer)) {
    reportedUnbound.add(entry.viewer);
    console.info(
      `viewers: no component is bound for viewer "${entry.viewer}" (format "${entry.format}") — falling back to the unknown viewer`,
    );
  }
  return UnknownViewer;
}

/**
 * What to render for a file. **Total**: every input yields a row and a
 * component, and nothing here returns `undefined` or throws.
 *
 * This is the whole public seam. A panel host, a note embed and quick capture
 * all call exactly this, which is what AD-87 means by one registry rather than
 * a viewer per surface.
 */
export function viewerComponentFor(file: ViewerFile): ResolvedViewer {
  const entry = resolveViewer(file);
  return { entry, Component: resolveViewerComponent(entry) };
}
