/**
 * A note's body subscription (Story 37.6, AD-58; Story 38.2 for the heartbeat;
 * Story 46.12 for there being more than one).
 *
 * A note body is never a command return value: `notes_open` hands Rust a
 * channel, the channel opens with a `reset` snapshot and then pushes whatever
 * happens to the document afterwards. This hook owns that subscription's
 * lifetime and the three messages that go the other way:
 *
 * - **The buffer heartbeat.** `notes_buffer_report` after 400 ms of typing
 *   idle. Rust needs the current buffer to run the three-way merge when someone
 *   else writes the file; a merge against a stale `mine` would raise the diff
 *   bar for hunks the user has already moved past.
 * - **The write.** `notes_save` after 1.5 s of typing idle, on blur and on ⌘S.
 *   It carries the revision the buffer opened at, which is what lets Rust write
 *   a conflict copy before overwriting a file that moved on underneath us
 *   (NFR-30).
 * - **The close.** `notes_close`, so a subscription never outlives the surface
 *   that was reading it. The last view's close is also the note's release: the
 *   words not yet on disk ride it rather than a separate save that could land
 *   after it, and Rust answers whether it removed a new note nobody wrote in
 *   (AD-304) — which then leaves every panel and every step of history. It may
 *   remove one only when no panel still targets the note (`discard`).
 *
 * There is no save button anywhere in the product (UX-DR35), so `save()` is
 * what the first three of those paths call — never a verb the user sees.
 *
 * # Every one of those is now addressed to a note (Story 46.12)
 *
 * The timers, the heartbeat and the write used to read "the open note" out of a
 * module singleton, so two mounted editors would have taken turns owning it.
 * They are keyed on the note instead ({@link "@/lib/stores/notes-editor"}), and
 * the consequences are worth naming because they are the story:
 *
 * - **Blur means "this editor lost focus", not "the editor lost focus".** Each
 *   mounted editor's blur handler saves the note that editor is showing. Two
 *   panels side by side, clicking from one into the other, writes the first and
 *   leaves the second's buffer alone — before this it wrote whichever note the
 *   store happened to be pointing at.
 * - **The autosave and heartbeat timers are per mount and are cleared when the
 *   note under that mount changes**, so a timer armed by note A can no longer
 *   fire after the panel has moved to note B. It would have reported B's
 *   subscription with A's keystrokes.
 * - **The caret hint is per document**, so a template's `{{cursor}}` is
 *   consumed by the editor over the note it belongs to.
 *
 * # One subscription per note, however many views
 *
 * Two panels may hold the same note — the panel model lets a single click
 * retarget one panel onto what another already shows. That is one document with
 * two views, never two buffers over one file, so the document is reference
 * counted in the store: the first view opens the channel, the last one out
 * releases it. A view that arrives while the open is still in flight simply
 * finds the document already there.
 */
import { useCallback, useEffect, useRef } from "react";
import {
  type IpcError,
  type NoteReleaseReq,
  notesBufferReport,
  notesClose,
  notesOpen,
  notesSave,
} from "@/lib/ipc/client";
import type { NotePending } from "@/lib/stores/notes-editor";
import {
  adoptBodySubscription,
  applyBodyBatch,
  beginSave,
  dropNoteDocument,
  editBuffer,
  markSaved,
  markSaveFailed,
  openNoteDocument,
  readNoteDocument,
  useNoteDocument,
} from "@/lib/stores/notes-editor";
import { panelsStore, sameTarget } from "@/lib/stores/panels";

/** How long the buffer may run ahead of Rust while the user is typing. */
export const NOTE_BUFFER_REPORT_IDLE_MS = 400;

/** How long the buffer may stay unwritten while the user is typing. */
export const NOTE_AUTOSAVE_IDLE_MS = 1_500;

/** Structural guard for the {@link IpcError} envelope thrown by the IPC client. */
function isIpcError(value: unknown): value is IpcError {
  if (typeof value !== "object" || value === null) {
    return false;
  }
  const candidate = value as Record<string, unknown>;
  return typeof candidate.code === "string" && typeof candidate.message === "string";
}

/**
 * Whether a refused `notes_open` means the note is not there at all — a panel
 * restored onto a note the startup sweep removed, or one deleted elsewhere.
 *
 * Read from the sentence because the envelope has no code of its own for it:
 * `notes_error` in keeper's `notes_ipc.rs` folds `NotesError::NotFound` (and an
 * unknown vault) into `internal`, and the sentence is `NotesError::NotFound`'s
 * Display in keeper-core's `notes/mod.rs`, `no such note: {id}`.
 */
function isNoteNotFound(error: unknown): boolean {
  return (
    isIpcError(error) && error.code === "internal" && error.message.startsWith("no such note:")
  );
}

/**
 * Whether a released note may be removed when nothing was written in it: only
 * when no panel still targets it. A fold, a switch of primary view, or any
 * other unmount that keeps the panel's target lets the editor go but not the
 * note — the person can still see it, or get it back with one click.
 */
function mayDiscard(vaultId: string, noteId: string): boolean {
  const note = { kind: "note", vaultId, noteId } as const;
  return !panelsStore.getState().panels.some((panel) => sameTarget(panel.target, note));
}

/** Hand one subscription back to Rust, and follow a removal out of every panel. */
function release(vaultId: string, noteId: string, subscriptionId: string, request: NoteReleaseReq) {
  notesClose(subscriptionId, request)
    .then((removed) => {
      if (removed) {
        // Nobody wrote in a new note, so Rust removed it. Back onto it
        // would open nothing, and words typed there would go nowhere.
        panelsStore.getState().closeTarget({ kind: "note", vaultId, noteId });
      }
    })
    .catch(() => {
      // Nothing left to tell: the surface that would have shown it is
      // going away.
    });
}

/**
 * Write one note's buffer now and adopt the acknowledgement. Resolves `true`
 * when the buffer is on disk — including when there was nothing to write, and
 * including when nobody has the note open at all — and `false` when the write
 * was refused.
 *
 * The same write [`useNotesBody`]'s `save()` performs, lifted out of the hook
 * so a surface that has no editor of its own can force one. Two callers:
 * quick capture, where dismissing the panel is a force-flush point (AD-62) and
 * it then asks Rust whether the draft was written in — a question answered from
 * the bytes on disk, which would read "untouched" for the last 1.5 s of typing
 * if this were not awaited first — and export, which reads the file Rust holds
 * rather than the buffer the webview does.
 *
 * **The boolean is the point, and it is why this does not simply throw.** A
 * function that catches its own failure and records it somewhere turns every
 * `await` on it into a no-op assertion: the caller cannot tell a write that
 * landed from one that was refused, and the sequenced step after it runs
 * anyway. Capture's next step is *hide the window*, so without an answer here
 * a refused write would take the panel away with the reason legible only
 * inside the window that just disappeared — the exact swallow UX-DR35 forbids.
 * It reports rather than throwing because the store still has to learn about
 * the failure for the editor's own caption, and one write must not produce two
 * different error channels.
 *
 * Unlike the release on the last close this reports back into the store, so
 * `dirty` clears and that release does not re-send the same text against a
 * revision the first write has already superseded — which Rust would read as
 * somebody else's edit and answer with a conflict copy.
 */
export async function saveNote(vaultId: string, noteId: string): Promise<boolean> {
  const document = readNoteDocument(vaultId, noteId);
  if (document.subscriptionId === null || !document.dirty) {
    // Nothing to write is not a failure: the bytes the caller cares about are
    // already the bytes on disk, which is what it is about to ask Rust. A note
    // nobody has open takes this branch too, and correctly — its file is not
    // being held back by any buffer in this webview.
    return true;
  }
  const written = document.text;
  beginSave(vaultId, noteId);
  try {
    markSaved(
      vaultId,
      noteId,
      written,
      await notesSave(document.subscriptionId, written, document.rev),
    );
    return true;
  } catch (error: unknown) {
    markSaveFailed(vaultId, noteId, isIpcError(error) ? error.message : String(error));
    return false;
  }
}

export interface UseNotesBody {
  /** The buffer: the note's body. */
  text: string;
  /** Whether the buffer has diverged from what Rust holds. */
  dirty: boolean;
  /** An external revision awaiting a decision, or null. */
  pending: NotePending | null;
  /** Whether a write is in flight. */
  saving: boolean;
  /** Whether the note has left the disk under us. */
  gone: boolean;
  /** Adopt an edit and arm the heartbeat and the write. */
  onEdit: (text: string) => void;
  /** Write the buffer now (blur, ⌘S, close). */
  save: () => Promise<void>;
}

export function useNotesBody(vaultId: string | null, noteId: string | null): UseNotesBody {
  const text = useNoteDocument(vaultId, noteId, (document) => document.text);
  const dirty = useNoteDocument(vaultId, noteId, (document) => document.dirty);
  const pending = useNoteDocument(vaultId, noteId, (document) => document.pending);
  const saving = useNoteDocument(vaultId, noteId, (document) => document.saving);
  const gone = useNoteDocument(vaultId, noteId, (document) => document.gone);
  const reportTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const save = useCallback(async () => {
    clearTimeout(saveTimer.current);
    saveTimer.current = undefined;
    if (vaultId === null || noteId === null) {
      return;
    }
    await saveNote(vaultId, noteId);
  }, [vaultId, noteId]);

  const onEdit = useCallback(
    (next: string) => {
      if (vaultId === null || noteId === null) {
        return;
      }
      editBuffer(vaultId, noteId, next);
      clearTimeout(reportTimer.current);
      reportTimer.current = setTimeout(() => {
        reportTimer.current = undefined;
        const document = readNoteDocument(vaultId, noteId);
        if (document.subscriptionId === null || !document.dirty) {
          return;
        }
        // Best-effort: a dropped heartbeat costs merge quality on the next
        // external write, never the buffer, so it is not worth surfacing.
        notesBufferReport(document.subscriptionId, document.text, document.rev).catch(() => {});
      }, NOTE_BUFFER_REPORT_IDLE_MS);
      clearTimeout(saveTimer.current);
      saveTimer.current = setTimeout(() => {
        void save();
      }, NOTE_AUTOSAVE_IDLE_MS);
    },
    [vaultId, noteId, save],
  );

  useEffect(() => {
    if (vaultId === null || noteId === null) {
      return;
    }
    // The store is what decides whether this view is the first: it holds the
    // reference count, so two panels on one note agree without a second
    // registry beside it that a test reset could get out of step with.
    const opening = openNoteDocument(vaultId, noteId);
    const generation = readNoteDocument(vaultId, noteId).generation;
    if (opening) {
      void notesOpen(vaultId, noteId, (batch) => applyBodyBatch(vaultId, noteId, batch))
        .then((subscriptionId) => {
          if (adoptBodySubscription(vaultId, noteId, generation, subscriptionId)) {
            return;
          }
          // This document was dropped — or dropped and reopened — while the
          // open was in flight. Close the orphan rather than leaking a
          // subscription pushing into a document nothing is mounted over.
          // Nothing was edited through it, so it carries no words; but when
          // no view came back for the note it is still the last one out, and
          // a new note left before its open resolved (⌘⌥N twice) must go
          // like any other.
          if (readNoteDocument(vaultId, noteId).views === 0) {
            release(vaultId, noteId, subscriptionId, {
              text: null,
              baseRev: "",
              discard: mayDiscard(vaultId, noteId),
            });
          } else {
            notesClose(subscriptionId, null).catch(() => {});
          }
        })
        .catch((error: unknown) => {
          markSaveFailed(vaultId, noteId, isIpcError(error) ? error.message : String(error));
          if (isNoteNotFound(error)) {
            // A panel onto a note that is not there has nothing to offer:
            // typing into it would write nowhere.
            panelsStore.getState().closeTarget({ kind: "note", vaultId, noteId });
          }
        });
    }
    return () => {
      // Both timers belong to THIS note under THIS mount. A report or a save
      // armed by the note being left must not fire against the note arriving:
      // the heartbeat would hand Rust one note's keystrokes under another's
      // subscription, and the autosave would write them.
      clearTimeout(reportTimer.current);
      reportTimer.current = undefined;
      clearTimeout(saveTimer.current);
      saveTimer.current = undefined;
      const closed = dropNoteDocument(vaultId, noteId);
      if (closed === null || closed.subscriptionId === null) {
        // Another view still holds the document, or the channel never opened.
        return;
      }
      // Switching notes — or switching vaults, which hands this hook a null
      // note while the buffer is still dirty — must not strand keystrokes in
      // the webview. The words ride the close itself: a save sent beside it is
      // a second IPC call, and Rust could run the close first and refuse the
      // save. Unreported on purpose: by the time the release resolves this
      // document is gone, and stamping its revision onto whatever replaced it
      // would be a genuine corruption.
      release(vaultId, noteId, closed.subscriptionId, {
        text: closed.dirty ? closed.text : null,
        baseRev: closed.rev,
        // Read now, not at mount: by the time this cleanup runs the panel has
        // already moved on, folded, or gone.
        discard: mayDiscard(vaultId, noteId),
      });
    };
  }, [vaultId, noteId]);

  return { text, dirty, pending, saving, gone, onEdit, save };
}
