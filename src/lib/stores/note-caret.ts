/**
 * The caret's line in each note an editor holds open, and the editor's buffer.
 *
 * The notes view's assistant dock tells the proxy which note is in front of the
 * person and where in it (UX-DR130): Rust turns `(vault, note, line, text)` into
 * the note's drive, path and the heading above the line. The editor owns the
 * caret and the buffer, so it publishes both here on every selection or
 * document change; the dock reads the active panel's note out of it. The
 * buffer is CodeMirror's immutable `Text`, so publishing it copies nothing: the
 * dock slices out what Rust reads only when it sends.
 *
 * The line is 1-based in the editor's buffer — the note's body, without the
 * frontmatter, which is never in the buffer. Two panels on one note publish to
 * the same entry: the one whose caret moved last is the one the person is in.
 */
import type { Text } from "@codemirror/state";
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";

/** Where the caret is in one note, and the buffer it is in. */
export interface NoteCaret {
  line: number;
  doc: Text;
}

export interface NoteCaretState {
  /** Caret per note, keyed by {@link caretKey}. */
  carets: ReadonlyMap<string, NoteCaret>;
  /** Record that `noteId`'s caret is on `line` of `doc`. */
  place: (vaultId: string, noteId: string, line: number, doc: Text) => void;
}

function caretKey(vaultId: string, noteId: string): string {
  return `${vaultId}\n${noteId}`;
}

export const noteCaretStore = createStore<NoteCaretState>()((set) => ({
  carets: new Map(),
  place: (vaultId, noteId, line, doc) =>
    set((state) => {
      const key = caretKey(vaultId, noteId);
      const was = state.carets.get(key);
      // A selection change within the line, over the same buffer, is no news.
      if (was !== undefined && was.line === line && was.doc === doc) {
        return state;
      }
      const carets = new Map(state.carets);
      carets.set(key, { line, doc });
      return { carets };
    }),
}));

/** The caret `state` holds for `noteId`, or `null` while no editor has placed one. */
export function caretIn(state: NoteCaretState, vaultId: string, noteId: string): NoteCaret | null {
  return state.carets.get(caretKey(vaultId, noteId)) ?? null;
}

/** The caret of `noteId`, or `null` while no editor has placed one. */
export function useNoteCaret(vaultId: string | null, noteId: string | null): NoteCaret | null {
  return useStore(noteCaretStore, (state) =>
    vaultId === null || noteId === null ? null : caretIn(state, vaultId, noteId),
  );
}

/** The buffer from its start through `caret`'s line: what Rust reads the heading from. */
export function textThroughCaret(caret: NoteCaret): string {
  const line = Math.min(Math.max(caret.line, 1), caret.doc.lines);
  return caret.doc.sliceString(0, caret.doc.line(line).to);
}

/** Test-only reset. */
export function resetNoteCaretForTest(): void {
  noteCaretStore.setState({ carets: new Map() });
}
