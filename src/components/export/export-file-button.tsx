/**
 * "Export…" on a file panel's header (Story 45.21, FR-199, UX-DR83).
 *
 * # Why the panel header and not the Files row
 *
 * A Files row already opens a panel on one click, and the panel is where the
 * file is a document rather than a listing entry. A second Export on the row
 * would be a second place to compose the target — the defect shape this epic
 * has already shipped twice, where two entry points to one act disagree about
 * what they are acting on.
 *
 * # Only for a file target
 *
 * A note panel's Export lives in the editor's own Actions menu, so the note
 * surface and the note panel have one Export between them and not two. The
 * panel frame renders this only for `kind: "file"`; see `panel-strip.tsx`.
 */

import { Download } from "lucide-react";
import { announceExport } from "@/components/export/export-announce";
import { Button } from "@/components/ui/button";
import { exportTarget } from "@/lib/export/export-target";

/** The button's label. Matches the note menu item exactly: one act, one word,
 *  and a person who learns it on a PDF finds it on a note. */
export const EXPORT_FILE_LABEL = "Export…";

export interface ExportFileButtonProps {
  profileId: string;
  relativePath: string;
}

export function ExportFileButton({ profileId, relativePath }: ExportFileButtonProps) {
  return (
    <Button
      type="button"
      variant="ghost"
      size="icon-sm"
      // The ellipsis survives the loss of the text (Story 48.9). "Export…"
      // promises that a dialog follows, and a glyph cannot make that promise —
      // so the three dots stay in the accessible name and in the tooltip, where
      // a keyboard user and a pointer both still get the warning that this
      // opens something rather than exporting on the spot.
      //
      // `Download` because this app already draws an export that way:
      // `conversation-pane.tsx` exports a chat with the same glyph, and
      // `phone-header.tsx`'s Export menu item carries it too. Nothing in the
      // sync family spends it on transfer direction — those say "Large file
      // download" in words.
      aria-label={EXPORT_FILE_LABEL}
      title={EXPORT_FILE_LABEL}
      className="shrink-0"
      onClick={() => {
        // Every rejection is handled inside `exportTarget`, so this can produce
        // no unhandled one.
        void exportTarget({ kind: "file", profileId, relativePath }).then(announceExport);
      }}
    >
      <Download aria-hidden="true" />
    </Button>
  );
}
