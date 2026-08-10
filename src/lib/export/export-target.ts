/**
 * Exporting one panel target to a folder the user picks (Story 45.21, FR-199).
 *
 * # One function, because there is one act
 *
 * A note and a file are exported from two different surfaces — the note
 * editor's Actions menu and a file panel's header — and the two surfaces have
 * nothing in common except this. Putting the picker, the dispatch and the
 * outcome vocabulary here means neither surface can invent its own answer to
 * "what happens when the dialog is cancelled", which is the question a second
 * copy would eventually get wrong: a cancelled dialog that still called the
 * command would create a folder in whatever path the picker returned last.
 *
 * # Cancelling is a status, not an error
 *
 * `open()` resolves `null` when the person presses Cancel. That is the most
 * common outcome of a file dialog and it is not a failure — nothing is called,
 * nothing is written, and nothing is said. A `catch` around a rejected promise
 * cannot express that, which is why the return type is a union and not a
 * `Promise<void>` that throws.
 *
 * # Rust words every sentence
 *
 * The summary and every refusal arrive from `keeper_sync::export` and
 * `keeper_core::vm::ExportReceiptVm`, and are shown verbatim. Nothing here
 * composes a sentence about a path, a count or a failure — see
 * `keeper_sync::export`'s module doc for why the words live in the crate that
 * can be tested on any machine.
 *
 * The three constants below are the exception, and they are narrow on purpose:
 * each is for a case Rust never sees, because in each the command is not
 * called at all. A buffer that would not flush, a target with no export path,
 * and a rejection carrying no message of its own — keeper says those itself
 * because there is nobody else to say them.
 *
 * # Why an unsaved note is a refusal
 *
 * Rust reads the note off the disk, because an export is a copy of a file and a
 * buffer in the webview is not a file. So the export flushes first, through
 * `saveNote` — the same write the autosave performs, not a second writer —
 * and refuses if the buffer still differs afterwards. Exporting a file that is
 * missing the last paragraph somebody typed, under a cheerful "Exported"
 * toast, is the worst outcome available here: it is wrong, and it looks right.
 */

import { open as openFolder } from "@tauri-apps/plugin-dialog";
import { saveNote } from "@/hooks/use-notes-body";
// `PanelTargetVm` from the generated bindings, not from the panel store: the
// store declares the type locally and does not re-export it, and adding an
// export there would make it a second source for a type Rust generates.
// `panel-strip.tsx` takes it from the same place.
import type { PanelTargetVm } from "@/lib/ipc/client";
import { type ExportReceiptVm, notesExport, syncExportEntry } from "@/lib/ipc/client";
import { readNoteDocument } from "@/lib/stores/notes-editor";
import { syncErrorMessage } from "@/lib/stores/sync";

/** The folder chooser's title. Names the act, because the OS dialog's own
 *  chrome says only "Open". */
export const EXPORT_PICKER_TITLE = "Export to…";

/** What a note that will not flush is told. Actionable and not final: the save
 *  is retried by the autosave within a second, so pressing Export again works. */
export const EXPORT_UNSAVED_SENTENCE =
  "keeper could not save your latest edits first, so it did not export a copy that would be missing them. Try again in a moment.";

/** What a target keeper has no export path for is told. Not reachable from a
 *  surface today — no control offers it for a recording — and a sentence
 *  rather than a throw, because the alternative to an honest refusal here is a
 *  component crashing on a case the store's own type allows. */
export const EXPORT_UNSUPPORTED_SENTENCE = "keeper cannot export a recording yet.";

/** The fallback when a rejection carries no sentence of its own. Rust words
 *  every refusal this command can produce, so reaching this means something
 *  threw that was not an `IpcError` — and a finished sentence beats the
 *  stringified object a bare `String(error)` would show. */
export const EXPORT_FAILED_SENTENCE = "keeper could not finish the export.";

/** What happened when somebody pressed Export. */
export type ExportOutcome =
  /** The dialog was dismissed. Nothing was called and nothing was written. */
  | { readonly status: "cancelled" }
  /** Bytes landed. The receipt says what, where, and what did not go. */
  | { readonly status: "exported"; readonly receipt: ExportReceiptVm }
  /** Nothing landed, and this is the sentence to show. */
  | { readonly status: "refused"; readonly reason: string };

/**
 * Flush this note's buffer before its file is read.
 *
 * Story 46.12 deleted the identity check that used to guard this. It existed
 * because `saveOpenNote` and `dirty` both read a module-singleton editor store,
 * so exporting a note that was NOT open would have been refused because some
 * *other* note had unsaved edits. The store is keyed by note now: `saveNote`
 * addresses this note's buffer, resolves `true` when there is nothing to write
 * — which is what a note nobody has open always answers — and no other note's
 * dirtiness can reach this function.
 */
async function flushed(vaultId: string, noteId: string): Promise<boolean> {
  await saveNote(vaultId, noteId);
  return !readNoteDocument(vaultId, noteId).dirty;
}

/**
 * Ask for a folder and export `target` into it.
 *
 * Total: every path returns an outcome, including the two that write nothing.
 * The IPC rejection is caught here rather than at the two call sites, because
 * an export that fails and an export that is refused are one thing to the
 * person who pressed the button, and Rust already worded both.
 */
export async function exportTarget(target: PanelTargetVm): Promise<ExportOutcome> {
  if (target.kind === "recording") {
    return { status: "refused", reason: EXPORT_UNSUPPORTED_SENTENCE };
  }
  if (target.kind === "note" && !(await flushed(target.vaultId, target.noteId))) {
    return { status: "refused", reason: EXPORT_UNSAVED_SENTENCE };
  }

  const destination = await openFolder({
    directory: true,
    multiple: false,
    title: EXPORT_PICKER_TITLE,
  });
  // `null` is Cancel. An array cannot arrive with `multiple: false`, and is
  // refused rather than unwrapped: taking `[0]` would turn a plugin change
  // nobody noticed into an export to a folder the person did not pick.
  if (typeof destination !== "string") {
    return { status: "cancelled" };
  }

  try {
    const receipt =
      target.kind === "note"
        ? await notesExport(target.vaultId, target.noteId, destination)
        : await syncExportEntry(target.profileId, target.relativePath, destination);
    return { status: "exported", receipt };
  } catch (error: unknown) {
    // `syncErrorMessage` rather than a fourth structural guard in this repo:
    // an `IpcError` is an `IpcError` whichever command rejected. Its fallback
    // is a finished sentence, so a rejection that is not one still says
    // something rather than printing `[object Object]`.
    return { status: "refused", reason: syncErrorMessage(error, EXPORT_FAILED_SENTENCE) };
  }
}
