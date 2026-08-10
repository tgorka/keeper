/**
 * Attach a file from anywhere on the drive to the note in the editor
 * (Story 45.13, FR-188, FR-189, UX-DR76).
 *
 * The third of the story's three entry points, and the only one that starts
 * outside keeper entirely. The attachments panel offers a recording note its own
 * files; the Files pane offers what is in a synced folder; this offers the disk.
 *
 * # Why the file picker and not a drag target
 *
 * `notes_attachment_drop` — the command this story replaced — took paths from
 * Tauri's window drag-drop event, and nothing in the app ever wired one up, so
 * for four epics FR-110 was a Rust function with no way to reach it. The OS
 * picker needs no window-level event plumbing, works from the keyboard, and
 * `@tauri-apps/plugin-dialog` is already a dependency of this app used by five
 * other surfaces. Drag-drop is a good second gesture and is deliberately not
 * this story: it would be a fourth entry point before the first three shared a
 * path, which is how the two inserters happened.
 *
 * # What it does with a file that is not in the vault
 *
 * Nothing, itself. It hands absolute paths to `notes_attach_sources` and Rust
 * decides: inside the vault, the note names it where it lies; outside, keeper
 * copies it into `attachments/` and the note names the copy. FR-145 forbids an
 * absolute path in a note and the vault syncs to other machines, so a link out
 * to `~/Desktop` would be a note that shows a picture on exactly one computer.
 * The button says so afterwards rather than before: a person who picks a file
 * off their Desktop has not asked to be warned, they have asked for the file.
 */
import { open as openFilePicker } from "@tauri-apps/plugin-dialog";
import { useCallback, useState } from "react";
import { Button } from "@/components/ui/button";
import { notesAttachSources } from "@/lib/ipc/client";
import { planAttachments } from "@/lib/notes/attach";

/** The control's label, and the picker's own title. */
export const ATTACH_FILE_LABEL = "Attach a file";

export interface AttachFileButtonProps {
  /** The vault the note lives in — where a copy would land. */
  vaultId: string;
  /**
   * The buffer as the editor has it *now*.
   *
   * The buffer and not the last save: a file attached, then attached again
   * before the autosave fires, is a duplicate, and a check against disk would
   * not see the first one.
   */
  body: string;
  /** Put this text where the caret is. The editor owns the caret. */
  onInsert: (text: string) => void;
  /**
   * What happened, in a sentence — or `null` when a gesture produced nothing
   * worth saying. The editor renders it, because a banner under the header is
   * where this file's other after-the-fact sentences already live.
   */
  onOutcome: (sentence: string | null) => void;
}

export function AttachFileButton({ vaultId, body, onInsert, onOutcome }: AttachFileButtonProps) {
  const [busy, setBusy] = useState(false);

  const pick = useCallback(async () => {
    setBusy(true);
    onOutcome(null);
    try {
      const picked = await openFilePicker({ multiple: true, title: ATTACH_FILE_LABEL });
      // Every case named, none left as the remainder of a ternary.
      //
      // `null` is a cancelled dialog: not an outcome, so no sentence. With
      // `multiple: true` the plugin's own types make the other case `string[]`,
      // so a `string` branch here would be unreachable code justified by a
      // shape the declaration excludes — this used to carry one, explained as
      // tolerance for "a platform that ignores `multiple`", which is not a
      // thing the type permits.
      //
      // What IS possible is the plugin breaking its own contract at runtime,
      // and that gets a sentence rather than a shrug: handing a bare string to
      // `notesAttachSources` would send Rust a shape it cannot read, and the
      // person would watch the picker close and nothing happen. Silence is the
      // failure this whole story exists to end, including when the cause is
      // keeper's own plumbing.
      if (picked === null) {
        return;
      }
      if (!Array.isArray(picked)) {
        onOutcome("keeper could not read what the file picker returned, so it attached nothing.");
        return;
      }
      const paths: string[] = picked;
      if (paths.length === 0) {
        return;
      }

      const resolved = await notesAttachSources(vaultId, paths);
      const plan = planAttachments(
        body,
        resolved.map((source) => source.relPath).filter((path): path is string => path !== null),
      );
      if (plan.text !== "") {
        onInsert(plan.text);
      }

      const copied = resolved.filter((source) => source.copied).length;
      const clauses = [
        copied === 0
          ? null
          : `${copied === 1 ? "1 file was" : `${copied} files were`} outside the vault, so keeper copied ${copied === 1 ? "it" : "them"} into attachments/ — a note cannot name a file the vault's other machines do not have.`,
        plan.refusal,
        // Partitioned on "produced no path", not on "carries a refusal" — see
        // `attach-to-note-dialog.tsx` for why. A source that came back with
        // neither must still reach the person; being dropped in silence is the
        // failure this story exists to end.
        ...resolved
          .filter((source) => source.relPath === null)
          .map(
            (source) =>
              source.refusal ?? `keeper did not attach ${source.name} and did not say why.`,
          ),
      ].filter((clause): clause is string => clause !== null);
      onOutcome(clauses.length === 0 ? null : clauses.join(" "));
    } catch (error) {
      onOutcome(
        `keeper could not attach that: ${error instanceof Error ? error.message : String(error)}`,
      );
    } finally {
      setBusy(false);
    }
  }, [vaultId, body, onInsert, onOutcome]);

  return (
    <Button size="sm" variant="ghost" disabled={busy} onClick={() => void pick()}>
      {ATTACH_FILE_LABEL}
    </Button>
  );
}
