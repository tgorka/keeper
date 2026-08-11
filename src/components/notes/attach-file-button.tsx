/**
 * Attach a file to the note in the editor, from either of the two places a file
 * can be (Story 45.13, FR-188, FR-189, UX-DR76; Story 46.11).
 *
 * # Two sources, because there are two (Story 46.11)
 *
 * Until 46.11 this was a plain button onto the operating system's file picker,
 * and it was the note editor's only door. The owner asked for the other one:
 * *"attach file — offer attach from a SYNC FOLDER in the dropdown too, not only
 * from outside."* The one in-vault door that existed was in the wrong surface —
 * the Files pane's multiselection into `AttachToNoteDialog`, which chooses the
 * **note** for files already selected — so from inside a note there was no way
 * to reach a file keeper already syncs.
 *
 * They are two menu items and not two controls. AD-104 leaves the note header's
 * action group at exactly two members — this and `NoteActions` — because six
 * controls plus two truncating spans do not fit the 560 px quick-capture window
 * and this row does not wrap; a third one would push `NoteActions` off the right
 * edge again, which is the defect Story 46.5 had just finished repairing. So the
 * one control becomes a dropdown, which is also what the report asked for
 * literally.
 *
 * # The two items do different things to your disk, and say so first
 *
 * This is the one place where a menu item earns a second line. The choice is not
 * "which folder" — it is which of two promises keeper makes:
 *
 * - **From this computer.** Rust decides per file: inside the vault, the note
 *   names it where it lies; outside, keeper **copies** it into `attachments/` and
 *   the note names the copy. FR-145 forbids an absolute path in a note and the
 *   vault syncs to other machines, so a link out to `~/Desktop` would be a note
 *   that shows a picture on exactly one computer.
 * - **From a folder you sync.** The vault already holds the file and the engine
 *   already carries it, so the note points at it where it is and **nothing is
 *   copied**. See {@link AttachFromVaultDialog}, which cannot copy: it never
 *   calls `notes_attach_sources`, which is the only copier in this app.
 *
 * A file that is in no vault cannot be embedded by reference at all —
 * `keeper-note://` will not serve it — so the copy is not a preference and the
 * first door is the only one that can reach such a file. That is why both remain,
 * and why the difference is on screen before the click rather than in the receipt
 * afterwards.
 *
 * # Why the file picker and not a drag target
 *
 * `notes_attachment_drop` — the command Story 45.13 replaced — took paths from
 * Tauri's window drag-drop event, and nothing in the app ever wired one up, so
 * for four epics FR-110 was a Rust function with no way to reach it. The OS
 * picker needs no window-level event plumbing, works from the keyboard, and
 * `@tauri-apps/plugin-dialog` is already a dependency of this app used by five
 * other surfaces. Drag-drop is a good gesture and is deliberately not this story.
 */
import { open as openFilePicker } from "@tauri-apps/plugin-dialog";
import { Paperclip } from "lucide-react";
import { useCallback, useId, useState } from "react";
import { Button } from "@/components/ui/button";
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuTrigger,
} from "@/components/ui/dropdown-menu";
import { notesAttachSources } from "@/lib/ipc/client";
import { planAttachments } from "@/lib/notes/attach";
import { ATTACH_FROM_VAULT_LABEL, AttachFromVaultDialog } from "./attach-from-vault-dialog";

/** The control's label, and the accessible name of the dropdown's trigger. */
export const ATTACH_FILE_LABEL = "Attach a file";

/** The OS picker's item, and the picker window's own title. */
export const ATTACH_FROM_COMPUTER_LABEL = "From this computer";

/**
 * The hint under each item: what this door does to the disk, before it is opened.
 *
 * Not a tooltip and not a receipt. The two doors make two different promises and
 * a person choosing between them has to be able to; a sentence that only appears
 * afterwards answers a question they have already had to guess at.
 */
export const ATTACH_FROM_COMPUTER_HINT = "A file outside this folder is copied into attachments/.";
export const ATTACH_FROM_VAULT_HINT = "Linked where it already is. Nothing is copied.";

export interface AttachFileButtonProps {
  /** The vault the note lives in — where a copy would land, and the folder the
   *  in-vault chooser browses. */
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
   *
   * The in-vault chooser does not use this: it is a dialog covering that banner,
   * so its receipt is in the dialog where the person is looking. One act, one
   * receipt, in the surface that has the eyes.
   */
  onOutcome: (sentence: string | null) => void;
}

export function AttachFileButton({ vaultId, body, onInsert, onOutcome }: AttachFileButtonProps) {
  const [busy, setBusy] = useState(false);
  const [browsing, setBrowsing] = useState(false);
  // One base id per mount, because two of these mount at once whenever two note
  // panels are open and an `id` has to be unique in the document.
  const hintId = useId();

  const pick = useCallback(async () => {
    setBusy(true);
    onOutcome(null);
    try {
      const picked = await openFilePicker({ multiple: true, title: ATTACH_FROM_COMPUTER_LABEL });
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
    <>
      <DropdownMenu>
        <DropdownMenuTrigger asChild>
          {/* Still one `<button>`, so the header's action group is still the
              two controls AD-104 leaves it at — but a paperclip now, not the
              sentence (Story 48.9). `ATTACH_FILE_LABEL` moves from the
              control's text to its `aria-label` and its `title`, unchanged, so
              a screen reader and speech input still hear and say the same words
              (WCAG 2.5.3) and a pointer can still ask what the picture means.
              The paperclip is the one glyph nothing else in this app spends. */}
          <Button
            type="button"
            size="icon-sm"
            variant="ghost"
            aria-label={ATTACH_FILE_LABEL}
            title={ATTACH_FILE_LABEL}
            disabled={busy}
          >
            <Paperclip aria-hidden="true" />
          </Button>
        </DropdownMenuTrigger>
        <DropdownMenuContent align="end">
          {/* The OS picker first: it is what this control has always done, and
              the item a hand travelling down the list reaches first should be
              the one whose behaviour has not changed.

              `aria-label` plus `aria-describedby` rather than letting the two
              lines concatenate into one name. Radix names an item from its text
              content, which would make this control answer to "From this
              computerA file outside this folder is copied into attachments/." —
              unspeakable by anyone using speech input (WCAG 2.5.3), where the
              name has to be the words on the control. The hint is a description
              instead, which is what it is: it reaches a screen reader after the
              name rather than as part of it, and it stays on screen for an eye. */}
          <DropdownMenuItem
            className="flex-col items-start gap-0.5"
            aria-label={ATTACH_FROM_COMPUTER_LABEL}
            aria-describedby={`${hintId}-computer`}
            onSelect={() => void pick()}
          >
            <span>{ATTACH_FROM_COMPUTER_LABEL}</span>
            <span id={`${hintId}-computer`} className="text-muted-foreground text-xs">
              {ATTACH_FROM_COMPUTER_HINT}
            </span>
          </DropdownMenuItem>
          <DropdownMenuItem
            className="flex-col items-start gap-0.5"
            aria-label={ATTACH_FROM_VAULT_LABEL}
            aria-describedby={`${hintId}-vault`}
            onSelect={() => setBrowsing(true)}
          >
            <span>{ATTACH_FROM_VAULT_LABEL}</span>
            <span id={`${hintId}-vault`} className="text-muted-foreground text-xs">
              {ATTACH_FROM_VAULT_HINT}
            </span>
          </DropdownMenuItem>
        </DropdownMenuContent>
      </DropdownMenu>
      {/* Outside the menu, because Radix unmounts the menu's content on select
          and a dialog mounted inside it would be torn down in the same tick it
          was asked for — the same reason `NoteActions` keeps its confirmation
          out here. */}
      {browsing && (
        <AttachFromVaultDialog
          vaultId={vaultId}
          body={body}
          onInsert={onInsert}
          onClose={() => setBrowsing(false)}
        />
      )}
    </>
  );
}
