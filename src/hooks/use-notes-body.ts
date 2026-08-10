/**
 * The open note's body subscription (Story 37.6, AD-58; Story 38.2 for the
 * heartbeat).
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
 * - **The write.** `notes_save` after 1.5 s of typing idle, on blur, on ⌘S and
 *   on close. It carries the revision the buffer opened at, which is what lets
 *   Rust write a conflict copy before overwriting a file that moved on
 *   underneath us (NFR-30).
 * - **The close.** `notes_close`, so a subscription never outlives the surface
 *   that was reading it.
 *
 * There is no save button anywhere in the product (UX-DR35), so `save()` is
 * what all four of those paths call — never a verb the user sees.
 */
import { useCallback, useEffect, useRef } from "react";
import {
  type IpcError,
  notesBufferReport,
  notesClose,
  notesOpen,
  notesSave,
} from "@/lib/ipc/client";
import type { NotePending } from "@/lib/stores/notes-editor";
import {
  applyBodyBatch,
  beginOpenNote,
  beginSave,
  closeNote,
  editBuffer,
  markSaved,
  markSaveFailed,
  notesEditorStore,
  setBodySubscription,
  useNotesEditorStore,
} from "@/lib/stores/notes-editor";

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
 * Write whatever the store holds right now, without touching the store.
 *
 * Used on the close paths only. The interactive `save()` reports its outcome
 * back into the store, which would be wrong here: by the time the write
 * resolves the store has moved on to a different note, and stamping this
 * note's revision onto that one would be a genuine corruption.
 */
function flushBuffer(): void {
  const state = notesEditorStore.getState();
  if (state.subscriptionId === null || !state.dirty) {
    return;
  }
  notesSave(state.subscriptionId, state.text, state.rev).catch(() => {
    // Nothing left to tell: the surface that would have shown it is going away.
  });
}

/**
 * Write the open note's buffer now and adopt the acknowledgement. Resolves
 * `true` when the buffer is on disk — including when there was nothing to
 * write — and `false` when the write was refused.
 *
 * The same write [`useNotesBody`]'s `save()` performs, lifted out of the hook
 * so a surface that has no editor of its own can force one. Quick capture is
 * the caller: dismissing the panel is a force-flush point (AD-62), and it then
 * asks Rust whether the draft was written in — a question answered from the
 * bytes on disk, which would read "untouched" for the last 1.5 s of typing if
 * this were not awaited first.
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
 * Unlike {@link flushBuffer} this reports back into the store, so `dirty`
 * clears and the later unmount flush does not re-send the same text against a
 * revision the first write has already superseded — which Rust would read as
 * somebody else's edit and answer with a conflict copy.
 */
export async function saveOpenNote(): Promise<boolean> {
  const state = notesEditorStore.getState();
  if (state.subscriptionId === null || !state.dirty) {
    // Nothing to write is not a failure: the bytes the caller cares about are
    // already the bytes on disk, which is what it is about to ask Rust.
    return true;
  }
  const written = state.text;
  beginSave();
  try {
    markSaved(written, await notesSave(state.subscriptionId, written, state.rev));
    return true;
  } catch (error: unknown) {
    markSaveFailed(isIpcError(error) ? error.message : String(error));
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
  /**
   * Where Rust asked for the caret on the opening `Reset`, or null.
   *
   * Only ever a template's `{{cursor}}`. The editor consumes it once the document
   * exists; without one the caret goes to the end of the body.
   */
  cursor: number | null;
  /** Adopt an edit and arm the heartbeat and the write. */
  onEdit: (text: string) => void;
  /** Write the buffer now (blur, ⌘S, close). */
  save: () => Promise<void>;
}

export function useNotesBody(vaultId: string | null, noteId: string | null): UseNotesBody {
  const text = useNotesEditorStore((state) => state.text);
  const dirty = useNotesEditorStore((state) => state.dirty);
  const pending = useNotesEditorStore((state) => state.pending);
  const saving = useNotesEditorStore((state) => state.saving);
  const gone = useNotesEditorStore((state) => state.gone);
  const cursor = useNotesEditorStore((state) => state.cursor);
  const reportTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);
  const saveTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined);

  const save = useCallback(async () => {
    clearTimeout(saveTimer.current);
    saveTimer.current = undefined;
    await saveOpenNote();
  }, []);

  const onEdit = useCallback(
    (next: string) => {
      editBuffer(next);
      clearTimeout(reportTimer.current);
      reportTimer.current = setTimeout(() => {
        reportTimer.current = undefined;
        const state = notesEditorStore.getState();
        if (state.subscriptionId === null || !state.dirty) {
          return;
        }
        // Best-effort: a dropped heartbeat costs merge quality on the next
        // external write, never the buffer, so it is not worth surfacing.
        notesBufferReport(state.subscriptionId, state.text, state.rev).catch(() => {});
      }, NOTE_BUFFER_REPORT_IDLE_MS);
      clearTimeout(saveTimer.current);
      saveTimer.current = setTimeout(() => {
        void save();
      }, NOTE_AUTOSAVE_IDLE_MS);
    },
    [save],
  );

  // Declared BEFORE the subscription effect so its cleanup runs first, while
  // the subscription id is still in the store.
  useEffect(
    () => () => {
      clearTimeout(reportTimer.current);
      flushBuffer();
    },
    [],
  );

  useEffect(() => {
    if (vaultId === null || noteId === null) {
      closeNote();
      return;
    }
    beginOpenNote(vaultId, noteId);
    let cancelled = false;
    let opened: string | null = null;
    void notesOpen(vaultId, noteId, applyBodyBatch)
      .then((subscriptionId) => {
        if (cancelled) {
          // The note changed while the open was in flight. Close the orphan
          // rather than leaking a subscription pushing into a dead store.
          void notesClose(subscriptionId);
          return;
        }
        opened = subscriptionId;
        setBodySubscription(subscriptionId);
      })
      .catch((error: unknown) => {
        if (!cancelled) {
          markSaveFailed(isIpcError(error) ? error.message : String(error));
        }
      });
    return () => {
      cancelled = true;
      // Switching notes — or switching vaults, which hands this hook a null
      // note while the buffer is still dirty — must not strand keystrokes in
      // the webview. Flush first, close second.
      flushBuffer();
      if (opened !== null) {
        void notesClose(opened);
      }
      closeNote();
    };
  }, [vaultId, noteId]);

  return { text, dirty, pending, saving, gone, cursor, onEdit, save };
}
