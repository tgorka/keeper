/**
 * "Export…" in a note's Actions menu (Story 45.21, FR-199, UX-DR83).
 *
 * # One item, in the one place a note is a document
 *
 * The note editor's header is that place, and it is the same header in both
 * hosts: the Notes pane mounts `NoteEditor` and so does a note panel. Putting
 * Export here rather than on the panel frame is what stops a note open in a
 * panel from having two Export controls — one of them the panel's, which knows
 * nothing about the editor's buffer and would export the last autosave.
 *
 * This renders exactly one `DropdownMenuItem` and no wrapper, so it is a legal
 * direct child of Story 45.17's `NoteActions` content and needs no positioning
 * of its own.
 */

import { announceExport } from "@/components/export/export-announce";
import { DropdownMenuItem } from "@/components/ui/dropdown-menu";
import { exportTarget } from "@/lib/export/export-target";

/** The item's label. U+2026, one character: an ellipsis says a dialog is
 *  coming, and three periods is a different glyph run to a screen reader. */
export const EXPORT_NOTE_LABEL = "Export…";

export interface ExportNoteItemProps {
  vaultId: string;
  noteId: string;
}

export function ExportNoteItem({ vaultId, noteId }: ExportNoteItemProps) {
  return (
    <DropdownMenuItem
      onSelect={() => {
        // Not awaited: Radix closes the menu on select, and the folder picker
        // stays open for as long as the person browses. Every rejection is
        // handled inside `exportTarget`, so this can produce no unhandled one.
        void exportTarget({ kind: "note", vaultId, noteId }).then(announceExport);
      }}
    >
      {EXPORT_NOTE_LABEL}
    </DropdownMenuItem>
  );
}
