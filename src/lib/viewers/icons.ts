/**
 * One glyph per registry icon name (Story 43.8, Story 45.2, AD-87).
 *
 * **Why this lives beside the registry and not in the pane that draws rows.**
 * It began in the Files pane, which was the only surface with rows; FR-254's
 * session tree is the second, and a table imported from a 1700-line pane would
 * drag that pane's whole module graph — its dialogs, its stores — into every
 * surface that only wanted an icon. The registry is where the answer belongs:
 * a format's glyph is a property of the format, the same on every surface, and
 * keeping it here is what makes "adding a format is a row" (AD-87) true of the
 * glyph as well as of the viewer. A new format arrives with an icon name and
 * every tree renders it without being edited.
 *
 * Keyed on {@link IconName} rather than on `string`, which keeps the property
 * the map it replaced was written for: a name added to the registry's union
 * fails THIS FILE to compile rather than rendering an empty cell.
 *
 * The lucide identifiers are the canonical ones rather than the older aliases
 * `FileVideo` / `FileAudio` / `FileJson` / `FileQuestion` that 43.8 imported.
 * Same glyphs — lucide re-exports the old names — but the alias renders a class
 * that does not match what the import is called (`FileVideo` draws
 * `lucide-file-play`), which is a thing to discover twice: once when reading
 * this table and once when a test asks what a row drew.
 */
import type { LucideIcon } from "lucide-react";
import {
  FileBadge,
  FileBraces,
  FileCode,
  FileHeadphone,
  FileImage,
  FilePlay,
  FileQuestionMark,
  FileSpreadsheet,
  FileText,
  FileType,
  Folder,
  Presentation,
} from "lucide-react";
import type { IconName } from "./types";

/** The glyph for each registry icon name. Total over the union, by type. */
export const VIEWER_ICON: Record<IconName, LucideIcon> = {
  "file-video": FilePlay,
  "file-image": FileImage,
  "file-audio": FileHeadphone,
  "file-text": FileText,
  "file-code": FileCode,
  "file-table": FileSpreadsheet,
  "file-json": FileBraces,
  "file-document": FileType,
  // A sealed page. PDF is where a document goes when it is finished — the
  // signed LOI, the deck that was sent — and the seal is the one thing that
  // separates it from the editable formats around it.
  "file-pdf": FileBadge,
  // Not a `File…` glyph, and that is the point: slides are the one format in
  // this table that is not read as a page, so drawing it as a page beside the
  // others is what made a deck indistinguishable from a PDF at a glance.
  "file-slides": Presentation,
  folder: Folder,
  "file-question": FileQuestionMark,
};
