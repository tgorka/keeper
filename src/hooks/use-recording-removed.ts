/**
 * A removed recording leaves every open note, whichever view it is in.
 *
 * Rust never edits the body of a note open in an editor, so when a recording
 * is removed it announces `keeper://recording-removed` and each open note's
 * buffer loses the widgets naming it here: Rust composes the body without
 * them, the buffer takes it as an edit, and the note saves as ⌘S would. At
 * the document level rather than in the editor's media layer, because Note
 * is only one of three views — Source has no layer and Preview no editor to
 * put one in — and the buffer is what every view draws and what is saved.
 *
 * Mounted once, at the app root, like the other shell events: a note open in
 * a panel behind another view is still open.
 */
import { useEffect } from "react";
import { saveNote } from "@/hooks/use-notes-body";
import { listenRecordingRemoved, mediaBlockWithoutSession } from "@/lib/ipc/client";
import { editBuffer, notesEditorStore, readNoteDocument } from "@/lib/stores/notes-editor";

/**
 * Take the recording `sessionId` out of every open note's buffer and save
 * those it changed. A buffer typed in while Rust answered is left alone: its
 * own words win, and a widget naming a gone recording says so when it draws.
 */
export async function forgetRecordingInOpenNotes(sessionId: string): Promise<void> {
  const { documents } = notesEditorStore.getState();
  await Promise.all(
    Object.entries(documents).map(async ([key, document]) => {
      if (document.views === 0 || document.subscriptionId === null) return;
      if (!document.text.includes(sessionId)) return;
      const next = await mediaBlockWithoutSession(document.text, sessionId).catch(() => null);
      // The store keys a document by vault and note, joined by `\u0000`
      // (`documentKey`), which neither id can contain.
      const [vaultId, noteId] = key.split("\u0000");
      if (next === null || vaultId === undefined || noteId === undefined) return;
      if (readNoteDocument(vaultId, noteId).text !== document.text) return;
      editBuffer(vaultId, noteId, next);
      await saveNote(vaultId, noteId);
    }),
  );
}

export function useRecordingRemoved(): void {
  useEffect(() => {
    let unlisten: (() => void) | undefined;
    let cancelled = false;
    try {
      void listenRecordingRemoved((sessionId) => void forgetRecordingInOpenNotes(sessionId))
        .then((fn) => {
          if (cancelled) fn();
          else unlisten = fn;
        })
        .catch(() => {
          // No Tauri host: nothing is ever removed in this environment.
        });
    } catch {
      // `listen` can throw synchronously when the Tauri IPC internals are absent.
    }
    return () => {
      cancelled = true;
      unlisten?.();
    };
  }, []);
}
