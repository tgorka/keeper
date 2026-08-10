/**
 * The one viewer registry (Story 45.2, FR-174, AD-87, AD-91).
 *
 * Every surface imports from here — `@/lib/viewers` — rather than from a file
 * inside it, so the module's internals can move without a rename reaching six
 * call sites. See `registry.ts` for what this keys on and why an extension
 * refines the answer only inside the kind `file`.
 */

export { openWithForProfileEntry } from "./actions";
export {
  type ResolvedViewer,
  resolveViewerComponent,
  VIEWER_COMPONENTS,
  viewerComponentFor,
} from "./components";
export {
  extensionOf,
  FILE_FORMAT_ENTRIES,
  FILE_FORMATS,
  registeredViewerIds,
  resolveViewer,
  UNKNOWN_ENTRY,
} from "./registry";
export type {
  IconName,
  LanguageId,
  RenderedView,
  ViewerComponent,
  ViewerEntry,
  ViewerFile,
  ViewerFormat,
  ViewerId,
  ViewerProps,
  ViewerSubject,
} from "./types";
export {
  UNKNOWN_VIEWER_EXTENSION_LABEL,
  UNKNOWN_VIEWER_EXTENSION_SLOT,
  UNKNOWN_VIEWER_FORMAT_LABEL,
  UNKNOWN_VIEWER_FORMAT_SLOT,
  UNKNOWN_VIEWER_NO_EXTENSION,
  UNKNOWN_VIEWER_OPEN_LABEL,
  UNKNOWN_VIEWER_REVEAL_LABEL,
  UNKNOWN_VIEWER_SIZE_LABEL,
  UNKNOWN_VIEWER_SIZE_SLOT,
  UNKNOWN_VIEWER_SIZE_UNKNOWN,
  UNKNOWN_VIEWER_TESTID,
  UnknownViewer,
  unknownViewerSentence,
} from "./unknown-viewer";
