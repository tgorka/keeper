/**
 * The open note's mirror (Epic 37 Story 37.6, Epic 38 Story 38.2, AD-58).
 *
 * Rust streams a document over `Channel<NoteBodyBatch>` and this store is the
 * webview's picture of it. Three texts matter and the names are worth keeping
 * straight:
 *
 * - **`base`** — the exact text Rust last delivered or last acknowledged as
 *   written. It is the revision the buffer opened at, and it is what a save
 *   carries as `baseRev` so Rust can tell a clean overwrite from a divergence.
 * - **`text`** — the buffer. Equal to `base` while the note is clean.
 * - **`pending`** — an external revision the buffer could not silently absorb.
 *   Non-null is precisely the condition that raises the inline diff bar.
 *
 * The one rule the reducer encodes, and the reason it lives here rather than in
 * a component: **a clean buffer applies an external write live; a dirty buffer
 * raises the bar.** Never a modal, never a lost keystroke, never a silent
 * clobber (UX-DR39). Everything else in the notes editor reads that decision
 * out of this store rather than re-deriving it.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { NoteBodyBatch, NoteWriteVm } from "@/lib/ipc/client";

/** Which kind of arrival raised the diff bar. */
export type NotePendingKind = "external" | "diverged";

export interface NotePending {
  /**
   * `external` — Rust merged the non-overlapping hunks and this is the result.
   * `diverged` — the hunks overlapped, nothing was merged, and this is theirs.
   */
  kind: NotePendingKind;
  /** The revision the arriving text belongs to. */
  rev: string;
  /** The arriving text, whole. */
  text: string;
}

export interface NotesEditorState {
  vaultId: string | null;
  noteId: string | null;
  /** The body subscription id, needed by every write. Null until `notes_open` resolves. */
  subscriptionId: string | null;
  /** The text Rust last delivered or acknowledged. */
  base: string;
  /** The revision `base` belongs to. */
  rev: string;
  /** The buffer. */
  text: string;
  /** Whether the buffer has diverged from `base` locally. */
  dirty: boolean;
  /** The note's vault-relative path, updated in place by a `renamed` batch. */
  path: string | null;
  /**
   * Where Rust wants the caret on the next mount, as a byte offset into `text`.
   *
   * Load-bearing rather than a nicety: a note opens with its frontmatter block at
   * the top, and a caret at offset 0 sits *in front of* that block, so the first
   * thing the user types lands above `---` and splits the document. Rust sends
   * the body offset (and a template's `{{cursor}}` where it declared one); the
   * editor consumes this once and clears it.
   */
  cursor: number | null;
  /** An external revision awaiting the user's decision, or null. */
  pending: NotePending | null;
  /** Set by a `gone` batch: the note is no longer on disk. */
  gone: boolean;
  /** Whether a write is in flight. */
  saving: boolean;
  /** When the last write was acknowledged, for the state word. */
  savedAtMs: number | null;
  /**
   * The conflict copy Rust wrote before overwriting a note whose disk bytes had
   * moved on (NFR-30). Worth surfacing: a file appeared that the user did not
   * create, and silence about it is how a conflict copy becomes litter.
   */
  conflictCopy: string | null;
  /** The last write failure, verbatim. */
  error: string | null;
}

const EMPTY: NotesEditorState = {
  vaultId: null,
  noteId: null,
  subscriptionId: null,
  base: "",
  rev: "",
  text: "",
  dirty: false,
  path: null,
  cursor: null,
  pending: null,
  gone: false,
  saving: false,
  savedAtMs: null,
  conflictCopy: null,
  error: null,
};

export const notesEditorStore = createStore<NotesEditorState>()(() => ({ ...EMPTY }));

/** Point the store at a note. Called before `notes_open`, so the surface never
 *  renders the previous note's body under the new note's title. */
export function beginOpenNote(vaultId: string, noteId: string): void {
  notesEditorStore.setState({ ...EMPTY, vaultId, noteId });
}

/** Adopt the subscription id `notes_open` resolved with. */
export function setBodySubscription(subscriptionId: string | null): void {
  notesEditorStore.setState({ subscriptionId });
}

/** Forget the open note. */
export function closeNote(): void {
  notesEditorStore.setState({ ...EMPTY });
}

/**
 * Apply one batch from the body channel.
 *
 * `external` is the interesting case and the whole reason this is a reducer: a
 * clean buffer takes the write live (the editor paints a fading highlight over
 * what moved), while a dirty buffer keeps every character the user typed and
 * raises the bar instead.
 */
export function applyBodyBatch(batch: NoteBodyBatch): void {
  notesEditorStore.setState((state) => {
    switch (batch.kind) {
      case "reset":
        return {
          base: batch.text,
          text: batch.text,
          rev: batch.rev,
          dirty: false,
          cursor: batch.cursor,
          pending: null,
          gone: false,
          error: null,
        };
      case "external":
        if (state.dirty) {
          return { pending: { kind: "external", rev: batch.rev, text: batch.text } };
        }
        return { base: batch.text, text: batch.text, rev: batch.rev, pending: null };
      case "diverged":
        return { pending: { kind: "diverged", rev: batch.rev, text: batch.theirs } };
      case "renamed":
        return { path: batch.path };
      case "gone":
        return { gone: true };
    }
  });
}

/** Adopt a keystroke. Dirtiness is derived, never asserted: a buffer typed back
 *  to what Rust holds is clean again, and the diff bar clears with it. */
export function editBuffer(text: string): void {
  notesEditorStore.setState((state) => ({
    text,
    dirty: text !== state.base,
    pending: text === state.base ? null : state.pending,
  }));
}

/** Take the arrived revision: it becomes the buffer and the new base. */
export function acceptPending(): void {
  notesEditorStore.setState((state) => {
    if (state.pending === null) {
      return {};
    }
    return {
      base: state.pending.text,
      text: state.pending.text,
      rev: state.pending.rev,
      dirty: false,
      pending: null,
    };
  });
}

/**
 * Keep the buffer and dismiss the bar.
 *
 * `base` and `rev` deliberately stay where they were, so the next save still
 * carries the stale base revision — which is exactly what makes Rust write the
 * disk version out as a conflict copy before overwriting it. Keeping mine costs
 * the other side nothing (NFR-30).
 */
export function keepMine(): void {
  notesEditorStore.setState({ pending: null });
}

/** A write is in flight. */
export function beginSave(): void {
  notesEditorStore.setState({ saving: true, error: null });
}

/** A write was acknowledged: `text` is now what is on disk. */
export function markSaved(text: string, write: NoteWriteVm): void {
  notesEditorStore.setState((state) => ({
    base: text,
    rev: write.rev,
    path: write.path,
    dirty: state.text !== text,
    saving: false,
    savedAtMs: Date.now(),
    conflictCopy: write.conflictCopy,
    error: null,
  }));
}

/** A write failed. The buffer is untouched — the words stay in front of the user. */
export function markSaveFailed(message: string): void {
  notesEditorStore.setState({ saving: false, error: message });
}

/** React selector hook over {@link notesEditorStore}. */
export function useNotesEditorStore<T>(selector: (state: NotesEditorState) => T): T {
  return useStore(notesEditorStore, selector);
}

/** Test-only reset: forget the open note. */
export function resetNotesEditorStoreForTest(): void {
  notesEditorStore.setState({ ...EMPTY });
}
