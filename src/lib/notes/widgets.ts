/**
 * The widgets a person can insert into a note, and what each writes.
 *
 * One list for the two doors — the toolbar's *Insert widget* menu and the `/`
 * menu — so neither can offer a widget the other does not, or write it
 * differently. Only widgets with a creation path are here: the media player
 * (a picker, then a block Rust composes) and the gallery (its callout head,
 * the caret left where the folder goes). The board, log and refs callouts are
 * typed by hand and have no inserter to share.
 *
 * Plain data, no React and no CodeMirror, so the toolbar in the main bundle and
 * the completion source in the editor's lazy chunk can both import it.
 */

/** What the gallery's inserter writes: its head, ready for a folder. */
export const GALLERY_HEAD = "> [!gallery] ";

export type NoteWidgetChoice = "media" | "gallery";

export interface NoteWidgetEntry {
  id: NoteWidgetChoice;
  /** The menu row. The media player's ends in `…`: it asks what to play. */
  label: string;
  /** The one-line explanation beside it. */
  detail: string;
}

/** The media player first: it is the widget this menu was asked for. */
export const NOTE_WIDGETS: readonly NoteWidgetEntry[] = [
  {
    id: "media",
    label: "Media player…",
    detail: "a recording, a video, an audio file or a transcript",
  },
  { id: "gallery", label: "Gallery", detail: "a folder of photos and videos" },
];

/** The toolbar control's name. */
export const INSERT_WIDGET_LABEL = "Insert widget";
