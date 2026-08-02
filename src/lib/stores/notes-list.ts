/**
 * Note-list mirror store (Epic 37, Story 37.2, AD-8, AD-58).
 *
 * A stream mirror in the {@link bridgeHealthStore} mould, with one difference:
 * notes changes arrive as index-based ops rather than as a wholesale snapshot,
 * because a 500-file agent run must not repaint 10 000 rows. `applyBatch` folds
 * a {@link NoteChangeBatch} onto a plain array by index and **never sorts,
 * re-sorts, filters or re-indexes** — conflicts above pins above the active sort
 * is Rust's ordering (`notes_list`), and re-deriving it here would be a second
 * place for the two to disagree.
 *
 * `total` is the size of the whole filtered set, not of the window. The list is
 * windowed (AD-58), so it is the only way the scrollbar can be honest about a
 * vault it has not shipped.
 *
 * The store holds no note state Rust owns: no read marks, no pin state, no
 * ordering, no filtering. A row is a {@link NoteRowVm} exactly as Rust composed
 * it, and the one thing kept beside the rows — `selectedId` — is a cursor over
 * them, deliberately kept by IDENTITY rather than index so a re-ordering stream
 * moves the row and not the cursor.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import type { NoteChangeBatch, NoteListVm, NoteRowVm } from "@/lib/ipc/client";

/**
 * How many rows one window asks for. Big enough that scrolling a normal vault
 * never touches the growth path, small enough that a 10 000-note vault's first
 * paint stays inside NFR-28's 100 ms budget — the payload is bounded by this
 * number and not by the vault (Story 37.2).
 */
export const NOTES_PAGE_SIZE = 200;

export interface NotesListState {
  /**
   * The rows of the current window, in Rust's order. Empty before the first
   * read — {@link NotesListState.loaded} is what separates "no notes" from "not
   * read yet", because the two want opposite copy on screen.
   */
  rows: NoteRowVm[];
  /** How many notes the active filter matches in total, across every window. */
  total: number;
  /** Where this window starts in the filtered set. */
  offset: number;
  /** Whether a first list read has landed. */
  loaded: boolean;
  /**
   * The note the editor has open, qualified by the vault it belongs to, or
   * `null`.
   *
   * By IDENTITY and not by index, so a streamed re-order or a filter change
   * moves the row and never the cursor (UX-DR41). By VAULT as well, because
   * switching vaults must not *close* a note — the editor stops showing it while
   * another vault is on screen and shows it again on the way back, which is the
   * difference between a filter and a navigation.
   */
  selected: { readonly vaultId: string; readonly noteId: string } | null;
  /**
   * How many rows the window currently asks Rust for. It GROWS rather than
   * paging, and the whole window is re-read at the new size: appending pages
   * would leave the streamed ops' indices meaning one thing to Rust and another
   * here, and a predicate sweep over a 10 000-note index is sub-millisecond, so
   * the simpler shape costs nothing worth having.
   */
  limit: number;
  /** Replace the window from a {@link NoteListVm} read. */
  reset: (vm: NoteListVm) => void;
  /** Fold one streamed change batch onto the window. */
  applyBatch: (batch: NoteChangeBatch) => void;
  /** Move the cursor. */
  select: (vaultId: string, noteId: string) => void;
  /** Drop the cursor entirely (the note it named is gone). */
  clearSelection: () => void;
  /** Ask for one more page's worth of rows on the next read. */
  growWindow: () => void;
  /** Clear the mirror (subscription teardown, vault switch). */
  clear: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const notesListStore = createStore<NotesListState>()((set) => ({
  rows: [],
  total: 0,
  offset: 0,
  loaded: false,
  selected: null,
  limit: NOTES_PAGE_SIZE,
  reset: (vm) => set({ rows: vm.rows, total: vm.total, offset: vm.offset, loaded: true }),
  applyBatch: (batch) =>
    set((state) => {
      let rows = state.rows;
      let total = state.total;
      for (const op of batch.ops) {
        switch (op.op) {
          case "reset": {
            rows = op.rows;
            total = op.total;
            break;
          }
          case "upsert": {
            // Guarded against the range, like every other diff reducer in this
            // app: a batch that raced a window change must drop the op, never
            // punch a hole in the array or append a row at a stale index.
            if (op.index < 0 || op.index > rows.length) {
              break;
            }
            const next = rows.slice();
            const existing = next.findIndex((row) => row.id === op.row.id);
            if (existing >= 0) {
              next.splice(existing, 1);
            } else {
              // A row that was not in the window is one more row in the set.
              total += 1;
            }
            next.splice(Math.min(op.index, next.length), 0, op.row);
            rows = next;
            break;
          }
          case "remove": {
            const at = rows.findIndex((row) => row.id === op.id);
            if (at < 0) {
              break;
            }
            rows = rows.slice(0, at).concat(rows.slice(at + 1));
            total = Math.max(0, total - 1);
            break;
          }
        }
      }
      // The cursor is NOT cleared when its row leaves the window. The note stays
      // open in the editor and the row is simply no longer listed (UX-DR41); a
      // cursor that reset itself would move the user's place on every agent
      // write.
      return { rows, total, loaded: true };
    }),
  select: (vaultId, noteId) => set({ selected: { vaultId, noteId } }),
  clearSelection: () => set({ selected: null }),
  growWindow: () => set((state) => ({ limit: state.limit + NOTES_PAGE_SIZE })),
  clear: () => set({ rows: [], total: 0, offset: 0, loaded: false, limit: NOTES_PAGE_SIZE }),
}));

/**
 * React selector hook over {@link notesListStore}. Pass a selector to subscribe
 * to just the slice a component needs.
 */
export function useNotesListStore<T>(selector: (state: NotesListState) => T): T {
  return useStore(notesListStore, selector);
}

/** Test-only reset: empty the window and drop the cursor. */
export function resetNotesListStoreForTest(): void {
  notesListStore.setState({
    rows: [],
    total: 0,
    offset: 0,
    loaded: false,
    selected: null,
    limit: NOTES_PAGE_SIZE,
  });
}
