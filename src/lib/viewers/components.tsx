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

/**
 * Which viewers draw the row their HOST would otherwise draw (Story 53.3,
 * FR-317).
 *
 * **Add your viewer here only if it draws a header in every state it can
 * render.** A host that finds its viewer named here gives up its own title row
 * and hands its fold, its close and its Export down through
 * {@link ViewerProps.frame}. A viewer that then drew no row — while it was
 * loading, for a file it could not read, for bytes that are not text — would
 * leave the panel with no title and no way out of it, in exactly the states a
 * reader most needs one.
 *
 * `text` is the only one today, and it earns it: {@link TextFileViewer}'s frame
 * already draws a bar carrying the file's name, the save word and Save, so the
 * panel's own row was that name a second time (the owner's report). Media and
 * document viewers draw no chrome at all, so a host keeps its row for them.
 *
 * A table beside {@link VIEWER_COMPONENTS}, in the same shape and for the same
 * reason, rather than a flag on a registry row: this is a property of the
 * COMPONENT and not of the format — every `.md` in the world resolves to one
 * row, and what draws it is what decides whether there is a header in it.
 */
export const VIEWERS_OWNING_HOST_ROW: Partial<Record<ViewerId, true>> = Object.freeze({
  text: true,
});

/** Ids already reported, so a render loop logs once rather than once a frame. */
const reportedUnbound = new Set<ViewerId>();

/** A resolved viewer: the row that chose it, the component to mount, and whether
 *  that component draws its host's header row. */
export interface ResolvedViewer {
  /** The row {@link resolveViewer} returned — the format keeper believes this
   *  file is, even when the component that draws it is not bound yet. */
  readonly entry: ViewerEntry;
  /** What to mount. Never `undefined`. */
  readonly Component: ViewerComponent;
  /**
   * Whether {@link ResolvedViewer.Component} draws the header row a frame around
   * it would otherwise draw — {@link VIEWERS_OWNING_HOST_ROW}.
   *
   * Answered here so a surface never switches on a viewer id to find out
   * (AD-87). It is `false` for a component that fell back to the unknown viewer,
   * whatever the row said, because the promise belongs to what is actually
   * mounted.
   */
  readonly ownsHostRow: boolean;
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
  const Component = resolveViewerComponent(entry);
  return {
    entry,
    Component,
    // The mounted component's promise, not the row's: an unbound `viewer` falls
    // back to the unknown viewer, which draws no header, and a host that had
    // given up its row on the row's word alone would be left with none.
    ownsHostRow: Component !== UnknownViewer && VIEWERS_OWNING_HOST_ROW[entry.viewer] === true,
  };
}
