/**
 * The open notes' mirrors (Epic 37 Story 37.6, Epic 38 Story 38.2, AD-58;
 * Story 46.12 for the plural).
 *
 * Rust streams a document over `Channel<NoteBodyBatch>` and this store is the
 * webview's picture of it. Three texts matter and the names are worth keeping
 * straight:
 *
 * - **`base`** — the exact body Rust last delivered or last acknowledged as
 *   written. It is the revision the buffer opened at, and it is what a save
 *   carries as `baseRev` so Rust can tell a clean overwrite from a divergence.
 * - **`text`** — the buffer. Equal to `base` while the note is clean.
 * - **`pending`** — an external revision the buffer could not silently absorb.
 *   Non-null is precisely the condition that raises the inline diff bar.
 *
 * All three are the note's **body**. The frontmatter block is a fourth field,
 * `frontmatter`, and it is never part of the buffer: it renders as the typed
 * properties panel (FR-107), the panel is the only thing that rewrites it, and
 * Rust re-joins the two on every save. A caret therefore has no `---` to land in
 * front of, which is the whole reason for the split.
 *
 * The one rule the reducer encodes, and the reason it lives here rather than in
 * a component: **a clean buffer applies an external write live; a dirty buffer
 * raises the bar.** Never a modal, never a lost keystroke, never a silent
 * clobber (UX-DR39). Everything else in the notes editor reads that decision
 * out of this store rather than re-deriving it.
 *
 * # There is no "the open note" any more (Story 46.12)
 *
 * Until 46.12 this module held exactly one document: one buffer, one base, one
 * `notes_open` subscription, in module scope. That is why `NOTE_PANEL_LIMIT`
 * was 1 — two mounted `NoteEditor`s would have taken turns owning the mirror,
 * and the first would have shown the second's text under the first's title
 * while its autosave wrote the second's body into the first's file. Data loss,
 * not a cosmetic bug.
 *
 * So the mirror is now **keyed by note**: `documents` maps
 * {@link documentKey} to one {@link NoteDocument}, and every reducer here names
 * the note it is acting on. That is the whole of the isolation, and it is
 * structural rather than disciplined — there is no ambient "current" document
 * for a caller to forget to check, because there is no reducer that can be
 * called without a vault and a note.
 *
 * Three shapes were available and this is the one that fits:
 *
 * - **A keyed map in one store (this).** One module store, one subscription
 *   root, one reset. A selector that reads `documents[key].text` is unchanged
 *   by an edit to any other note, so `Object.is` keeps the other panels from
 *   re-rendering without an equality function. And, decisively, the reducers
 *   stay reachable from plain functions: `saveNote` and `exportTarget` are not
 *   React and cannot read a context.
 * - **A store factory per note.** Identical semantics, plus a registry to find
 *   a store from outside React, plus a lifecycle to dispose one, plus a
 *   `useStore` whose store identity changes between renders. That is the map
 *   above with extra parts, and the parts are the ones that break.
 * - **A context-provided instance.** Would put the buffer out of reach of every
 *   non-React caller — `@/lib/export/export-target` flushes a note it is not
 *   rendering — so it would need the registry anyway, and would then have two
 *   ways to find one document.
 *
 * # A document is created and dropped explicitly, and nothing else creates one
 *
 * {@link openNoteDocument} is the only function that adds a key and
 * {@link dropNoteDocument} the only one that removes it. **Every other reducer
 * is a no-op on a note with no document**, which is what makes a batch that
 * arrives after a close harmless: a subscription that outlives its surface by a
 * few milliseconds writes into nothing, rather than resurrecting a document no
 * view is mounted over. `@/hooks/use-notes-body` owns the pairing and
 * reference-counts it, because two panels may show one note and that is one
 * document with two views, never two buffers over one file.
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
  /** The arriving body, whole. */
  text: string;
  /** The block that arrived with it, adopted along with the body. */
  frontmatter: string;
}

/**
 * One note's mirror.
 *
 * Deliberately holds no vault id and no note id: it is the value at a
 * {@link documentKey}, and a copy of the key inside the value is a second
 * source for the same fact that can drift from the first.
 */
export interface NoteDocument {
  /** The body subscription id, needed by every write. Null until `notes_open` resolves. */
  subscriptionId: string | null;
  /**
   * How many mounted views hold this document (Story 46.12).
   *
   * Two panels may show one note, and that is one buffer with two views — not
   * two buffers over one file, which would be the singleton's data loss
   * rebuilt one level down. So the document is reference counted: the first
   * view creates it and opens the channel, the last one out flushes, closes
   * and removes it. Zero is not a state a stored document can be in; it is
   * what {@link EMPTY_NOTE_DOCUMENT} reads as, which is how a caller
   * distinguishes "nobody has this open" from "open and empty".
   */
  views: number;
  /**
   * Which incarnation of this note's document this is.
   *
   * Monotonic across the session and never reused. `notes_open` is a round
   * trip, so a note closed and reopened inside one — the panel that is
   * unmounted and remounted, React's double-invoked effects in development —
   * leaves an in-flight subscription belonging to a document that no longer
   * exists. Without this, that subscription id would be adopted by the NEW
   * document, whose own channel is the one actually pushing batches, and the
   * next save would write through a channel Rust is about to be told to close.
   */
  generation: number;
  /** The body Rust last delivered or acknowledged. */
  base: string;
  /** The revision `base` belongs to. */
  rev: string;
  /** The buffer: the body, and never the block. */
  text: string;
  /**
   * The note's frontmatter block, verbatim — fences and trailing newline
   * included — or empty when it has none.
   *
   * Rust's, not the editor's. It arrives beside every body, the properties panel
   * is the only surface that rewrites it, and a save that does not name a new one
   * keeps it byte for byte (FR-121).
   */
  frontmatter: string;
  /** Whether the buffer has diverged from `base` locally. */
  dirty: boolean;
  /** The note's vault-relative path, updated in place by a `renamed` batch. */
  path: string | null;
  /**
   * Where Rust wants the caret once the document exists, as a byte offset into
   * `text`, or null for "wherever the editor would put it" — the end of the body.
   *
   * Set only when the template the note was created from declared a `{{cursor}}`.
   * The editor consumes it once.
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

export interface NotesEditorState {
  /**
   * Every open note's mirror, by {@link documentKey}.
   *
   * A note with no entry is a note nothing is mounted over; readers get
   * {@link EMPTY_NOTE_DOCUMENT} rather than `undefined`, so no surface has to
   * branch on "not open yet" separately from "open and empty".
   */
  readonly documents: Readonly<Record<string, NoteDocument>>;
}

/**
 * The key one note's mirror lives at.
 *
 * `\u0000` for the same reason `nodeKey` in {@link "@/lib/stores/files-tree"}
 * uses it: it is the one byte neither half can contain, so no pair of ids can
 * compose the same key as a different pair.
 */
export function documentKey(vaultId: string, noteId: string): string {
  return `${vaultId}\u0000${noteId}`;
}

/**
 * What a note nobody has open reads as.
 *
 * One frozen value rather than a fresh object per read: selectors compare with
 * `Object.is`, and a new object each render would re-render every view of every
 * note that is not open.
 */
export const EMPTY_NOTE_DOCUMENT: NoteDocument = Object.freeze({
  views: 0,
  generation: 0,
  subscriptionId: null,
  base: "",
  rev: "",
  text: "",
  frontmatter: "",
  dirty: false,
  path: null,
  cursor: null,
  pending: null,
  gone: false,
  saving: false,
  savedAtMs: null,
  conflictCopy: null,
  error: null,
});

export const notesEditorStore = createStore<NotesEditorState>()(() => ({ documents: {} }));

/** One note's mirror out of a state, or {@link EMPTY_NOTE_DOCUMENT}. */
export function noteDocument(
  state: NotesEditorState,
  vaultId: string | null,
  noteId: string | null,
): NoteDocument {
  if (vaultId === null || noteId === null) {
    return EMPTY_NOTE_DOCUMENT;
  }
  return state.documents[documentKey(vaultId, noteId)] ?? EMPTY_NOTE_DOCUMENT;
}

/** One note's mirror right now, for the callers that are not rendering — the
 *  CodeMirror boot closure, the export flush, the autosave timer. */
export function readNoteDocument(vaultId: string | null, noteId: string | null): NoteDocument {
  return noteDocument(notesEditorStore.getState(), vaultId, noteId);
}

/**
 * Apply a change to one note's mirror, and to nothing else.
 *
 * **A note with no document is left alone.** That is the isolation's second
 * half: the first is that a key names one note, and this is that a closed note
 * cannot be reopened by a straggling batch, a resolved save, or a timer that
 * fired after its editor came down.
 */
function mutate(
  vaultId: string,
  noteId: string,
  next: (document: NoteDocument) => Partial<NoteDocument>,
): void {
  const key = documentKey(vaultId, noteId);
  notesEditorStore.setState((state) => {
    const held = state.documents[key];
    if (held === undefined) {
      return state;
    }
    return { documents: { ...state.documents, [key]: { ...held, ...next(held) } } };
  });
}

/** Monotonic, never reused: see {@link NoteDocument.generation}. */
let nextGeneration = 1;

/**
 * Take a view on this note's mirror, creating it if this is the first.
 *
 * Called before `notes_open`, so a surface never renders another note's body
 * under this note's title — which under a keyed store is not a reset but the
 * absence of an entry, and therefore cannot be forgotten.
 *
 * A second view of a note that is already open **joins** the document rather
 * than blanking it: the two panels are two windows onto one buffer, so the
 * second must show what the first is holding, unsaved keystrokes and all.
 * Returns whether this call is what created the document, which is how
 * {@link "@/hooks/use-notes-body"} decides who opens the channel — one
 * subscription per note, however many views there are.
 */
export function openNoteDocument(vaultId: string, noteId: string): boolean {
  const key = documentKey(vaultId, noteId);
  const held = notesEditorStore.getState().documents[key];
  if (held !== undefined) {
    notesEditorStore.setState((state) => ({
      documents: { ...state.documents, [key]: { ...held, views: held.views + 1 } },
    }));
    return false;
  }
  const created: NoteDocument = { ...EMPTY_NOTE_DOCUMENT, views: 1, generation: nextGeneration };
  nextGeneration += 1;
  notesEditorStore.setState((state) => ({
    documents: { ...state.documents, [key]: created },
  }));
  return true;
}

/**
 * Give up one view on this note's mirror.
 *
 * Returns the document that was removed when this was the last view, so the
 * caller can flush its buffer and close its channel from the value rather than
 * from a second read of a store that no longer holds it — and `null` while
 * another view still has it, which is the whole of "closing one of two panels
 * on the same note closes nothing".
 */
export function dropNoteDocument(vaultId: string, noteId: string): NoteDocument | null {
  const key = documentKey(vaultId, noteId);
  const held = notesEditorStore.getState().documents[key];
  if (held === undefined) {
    return null;
  }
  if (held.views > 1) {
    notesEditorStore.setState((state) => ({
      documents: { ...state.documents, [key]: { ...held, views: held.views - 1 } },
    }));
    return null;
  }
  notesEditorStore.setState((state) => {
    const { [key]: _dropped, ...kept } = state.documents;
    return { documents: kept };
  });
  return held;
}

/**
 * Adopt the subscription id `notes_open` resolved with, if it still belongs to
 * the document that asked for it.
 *
 * Returns whether it was adopted. `false` means the channel is an orphan — the
 * document was dropped, or dropped and recreated, while the open was in flight
 * — and the caller owes Rust a `notes_close` for it. That is not a defensive
 * branch: React's double-invoked effects in development do exactly this on
 * every mount, and so does closing a note panel inside one IPC round trip.
 */
export function adoptBodySubscription(
  vaultId: string,
  noteId: string,
  generation: number,
  subscriptionId: string,
): boolean {
  const held = readNoteDocument(vaultId, noteId);
  if (held.generation !== generation) {
    return false;
  }
  mutate(vaultId, noteId, () => ({ subscriptionId }));
  return true;
}

/**
 * Apply one batch from the body channel.
 *
 * `external` is the interesting case and the whole reason this is a reducer: a
 * clean buffer takes the write live (the editor paints a fading highlight over
 * what moved), while a dirty buffer keeps every character the user typed and
 * raises the bar instead.
 */
export function applyBodyBatch(vaultId: string, noteId: string, batch: NoteBodyBatch): void {
  mutate(vaultId, noteId, (document) => {
    switch (batch.kind) {
      case "reset":
        return {
          base: batch.text,
          text: batch.text,
          frontmatter: batch.frontmatter,
          rev: batch.rev,
          // `?? null` and not a bare read: `path` is a NEW required field on an
          // existing variant, so a batch built before Story 45.18 — a fixture,
          // or an older build's channel — type-checks and delivers `undefined`,
          // which is neither a path nor `null` and would pass every
          // `path !== null` gate while composing "undefined/note.md".
          path: batch.path ?? null,
          dirty: false,
          cursor: batch.cursor,
          pending: null,
          gone: false,
          error: null,
        };
      case "external":
        if (document.dirty) {
          return {
            pending: {
              kind: "external",
              rev: batch.rev,
              text: batch.text,
              frontmatter: batch.frontmatter,
            },
          };
        }
        return {
          base: batch.text,
          text: batch.text,
          frontmatter: batch.frontmatter,
          rev: batch.rev,
          pending: null,
        };
      case "diverged":
        return {
          pending: {
            kind: "diverged",
            rev: batch.rev,
            text: batch.theirs,
            frontmatter: batch.frontmatter,
          },
        };
      case "renamed":
        return { path: batch.path };
      case "gone":
        return { gone: true };
    }
  });
}

/** Adopt a keystroke. Dirtiness is derived, never asserted: a buffer typed back
 *  to what Rust holds is clean again, and the diff bar clears with it. */
export function editBuffer(vaultId: string, noteId: string, text: string): void {
  mutate(vaultId, noteId, (document) => ({
    text,
    dirty: text !== document.base,
    pending: text === document.base ? null : document.pending,
  }));
}

/** Take the arrived revision: its body becomes the buffer and the new base, and
 *  its block comes with it — accepting half a document would be a lie. */
export function acceptPending(vaultId: string, noteId: string): void {
  mutate(vaultId, noteId, (document) => {
    if (document.pending === null) {
      return {};
    }
    return {
      base: document.pending.text,
      text: document.pending.text,
      frontmatter: document.pending.frontmatter,
      rev: document.pending.rev,
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
export function keepMine(vaultId: string, noteId: string): void {
  mutate(vaultId, noteId, () => ({ pending: null }));
}

/** A write is in flight. */
export function beginSave(vaultId: string, noteId: string): void {
  mutate(vaultId, noteId, () => ({ saving: true, error: null }));
}

/** A write was acknowledged: `text` is now the body on disk, and `write` names the
 *  block that landed with it — `updated` having been stamped, it is not quite the
 *  block anyone sent. */
export function markSaved(vaultId: string, noteId: string, text: string, write: NoteWriteVm): void {
  mutate(vaultId, noteId, (document) => ({
    base: text,
    frontmatter: write.frontmatter,
    rev: write.rev,
    path: write.path,
    dirty: document.text !== text,
    saving: false,
    savedAtMs: Date.now(),
    conflictCopy: write.conflictCopy,
    error: null,
  }));
}

/** A write failed. The buffer is untouched — the words stay in front of the user. */
export function markSaveFailed(vaultId: string, noteId: string, message: string): void {
  mutate(vaultId, noteId, () => ({ saving: false, error: message }));
}

/** React selector hook over one note's mirror. A note nobody has open selects
 *  out of {@link EMPTY_NOTE_DOCUMENT}, so a view mounted ahead of its channel
 *  renders the same thing an empty document renders. */
export function useNoteDocument<T>(
  vaultId: string | null,
  noteId: string | null,
  selector: (document: NoteDocument) => T,
): T {
  return useStore(notesEditorStore, (state) => selector(noteDocument(state, vaultId, noteId)));
}

/** Test-only reset: forget every open note. */
export function resetNotesEditorStoreForTest(): void {
  notesEditorStore.setState({ documents: {} });
}
