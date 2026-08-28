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
 * it, and that is all this store holds.
 *
 * **It no longer keeps a cursor.** Which note is open used to live here as
 * `selected`, and Story 45.1 moved it to {@link "@/lib/stores/panels"}: a note
 * on screen is a panel showing a `note` target, exactly as a file on screen is a
 * panel showing a `file` target. Leaving a second copy of that fact beside the
 * panel list is how the two would come to disagree about what is open — so this
 * one was deleted rather than kept in step.
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
  /**
   * How many notes the active lens SELECTS, across every window (Story 44.11).
   *
   * Always Rust's number, never derived here: `reset` takes it and every batch
   * carries a fresh one. The list is windowed (AD-58, Story 44.10), so this is
   * the only honest thing to show a reader and the only honest thing to page
   * against — `rows.length` is a screenful.
   */
  total: number;
  /**
   * How many the lens MATCHED before a space's `keeper.limit` declined any.
   * Equal to `total` unless a cap bit.
   */
  matched: number;
  /** Where this window starts in the filtered set. */
  offset: number;
  /** Whether a first list read has landed. */
  loaded: boolean;
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
  /** Ask for one more page's worth of rows on the next read. */
  growWindow: () => void;
  /** Clear the mirror (subscription teardown, vault switch). */
  clear: () => void;
}

/** The vanilla store instance, created once at module load and shared app-wide. */
export const notesListStore = createStore<NotesListState>()((set) => ({
  rows: [],
  total: 0,
  matched: 0,
  offset: 0,
  loaded: false,
  limit: NOTES_PAGE_SIZE,
  reset: (vm) =>
    set({
      rows: vm.rows,
      total: vm.total,
      matched: vm.matched,
      offset: vm.offset,
      loaded: true,
    }),
  applyBatch: (batch) =>
    set((state) => {
      let rows = state.rows;
      for (const op of batch.ops) {
        switch (op.op) {
          case "reset": {
            rows = op.rows;
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
            break;
          }
        }
      }
      // The counts come off the envelope rather than being carried forward by
      // one per op (Story 44.11). The arithmetic version was right only while
      // every change to the matched set also changed the window: a note that
      // starts matching the filter three thousand rows below the page produces
      // no op, so it moved no count — and once the list is windowed there is no
      // scroll that would have corrected it. Rust recounts the whole set for
      // every batch it sends, so this is a copy, not a derivation.
      //
      // The open note is NOT closed when its row leaves the window. Which note
      // is open is the panel list's business now (Story 45.1), not this
      // mirror's, and the row simply stops being listed (UX-DR41) — a list that
      // closed the editor would move the user's place on every agent write.
      return { rows, total: batch.total, matched: batch.matched, loaded: true };
    }),
  growWindow: () => set((state) => ({ limit: state.limit + NOTES_PAGE_SIZE })),
  clear: () =>
    set({
      rows: [],
      total: 0,
      matched: 0,
      offset: 0,
      loaded: false,
      limit: NOTES_PAGE_SIZE,
    }),
}));

/**
 * React selector hook over {@link notesListStore}. Pass a selector to subscribe
 * to just the slice a component needs.
 */
export function useNotesListStore<T>(selector: (state: NotesListState) => T): T {
  return useStore(notesListStore, selector);
}

/** Test-only reset: empty the window. */
export function resetNotesListStoreForTest(): void {
  notesListStore.setState({
    rows: [],
    total: 0,
    matched: 0,
    offset: 0,
    loaded: false,
    limit: NOTES_PAGE_SIZE,
  });
}
