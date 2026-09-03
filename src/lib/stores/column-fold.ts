/**
 * Which surface COLUMNS are folded (Story 48.1, FR-198).
 *
 * **Not the same fact as any fold already here, and that is the whole point.**
 * Story 45.20 folds the app sidebar — one column, and until this story the only
 * one. Story 47.3 folds the SECTIONS inside the notes rail: Spaces, Tags, Files,
 * three groups of rows inside one column. The owner asked twice for the columns
 * ("daj mozliwosc folding innych paneli nie tylko pierwszego", then "wciaz tylko
 * pierwsza kolumna jest mozliwa do foldowania") and both times got something
 * else. This is the columns.
 *
 * **A third cookie, not a third key in somebody else's.** `notes-rail` here and
 * `spaces` in `keeper_notes_rail_fold` are different facts about different
 * things: one says the rail is put away, the other says the Spaces section
 * inside it is. Folding a column while its sections stay as the user left them
 * is the behaviour, and it needs two flags — a shared namespace would make
 * unfolding the column silently unfold the sections too. Story 47.3 refused to
 * share a namespace with Story 45.20's for exactly this reason; this follows it.
 *
 * The key set is {@link SURFACE_COLUMN_IDS}, imported rather than restated: the
 * same four ids key `keeper_column_widths`, and a column that could be folded
 * but not resized (or the reverse) would be a typo nobody would find.
 *
 * The ENCODING is `fold-cookie`'s, shared with both stores above, so there is
 * one answer to "what does a fold cookie look like" rather than three.
 *
 * Everything up to the store is pure and takes the cookie string, so the round
 * trip is assertable without a document — which is deliberately NOT a test of
 * the restore: a restore is a `hydrate…` call at a mount point, and a
 * store-level test can never see that the mount point does not call it (DW-172).
 * `AppShell` makes that call, and `app-shell.test.tsx` is where it is defended.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { SURFACE_COLUMN_IDS, type SurfaceColumnId } from "@/lib/column-widths";
import { foldFlagsCookie, persistFold, readFoldFlags } from "@/lib/stores/fold-cookie";

/** The cookie the surface-column fold lives in. Not the sidebar's, not the rail's. */
export const COLUMN_FOLD_COOKIE = "keeper_column_fold";

/** Per column: `true` when that column is put away behind its strip. */
export type ColumnFold = Record<SurfaceColumnId, boolean>;

/**
 * Every column showing, which is what a keeper that has never been folded does.
 *
 * All six default open, unlike the notes rail's Files section: folding a column
 * hides a browser the surface exists to offer, and there is no cold directory
 * scan to avoid by starting one of them away.
 */
export function columnsUnfolded(): ColumnFold {
  return {
    "notes-rail": false,
    "notes-list": false,
    "files-tree": false,
    "chat-list": false,
    "tasks-list": false,
    "bots-list": false,
  };
}

/**
 * The fold remembered in a `document.cookie` string.
 *
 * Total: an unknown id, a malformed entry or a value that is not `0`/`1` leaves
 * that column open. A jar holding a column this build no longer has must not
 * cost the user a surface.
 */
export function readColumnFold(cookie: string): ColumnFold {
  return readFoldFlags(cookie, COLUMN_FOLD_COOKIE, SURFACE_COLUMN_IDS, columnsUnfolded());
}

/** The `document.cookie` assignment that records this fold. */
export function columnFoldCookie(fold: ColumnFold): string {
  return foldFlagsCookie(COLUMN_FOLD_COOKIE, SURFACE_COLUMN_IDS, fold);
}

export interface ColumnFoldState {
  /** What is folded right now. */
  columns: ColumnFold;
  /** Fold or unfold one column. */
  toggleColumn: (id: SurfaceColumnId) => void;
}

export const columnFoldStore = createStore<ColumnFoldState>()((set, get) => ({
  columns: columnsUnfolded(),
  toggleColumn: (id) => {
    const columns = { ...get().columns, [id]: !get().columns[id] };
    persistFold(columnFoldCookie(columns));
    set({ columns });
  },
}));

/** Whether {@link hydrateColumnFold} has already run in this document. */
let hydrated = false;

/**
 * Restore the remembered fold.
 *
 * Idempotent, so React's double-invoked development effects restore once and a
 * second caller cannot overwrite a fold the user has changed since the first.
 *
 * Mounted in `AppShell` and not in the four surfaces, unlike Story 47.3's rail
 * fold: these columns live on three different primary views, every one of which
 * is unmounted whenever another is showing, so four hydration points would be
 * four chances to forget one — and the one that is forgotten is invisible until
 * somebody switches surfaces twice. One call at the shell covers all four
 * however the app starts, which is the same argument `hydratePanels`,
 * `hydrateSidebarFold` and `hydrateFilesTree` are already there for.
 */
export function hydrateColumnFold(cookie: string): void {
  if (hydrated) {
    return;
  }
  hydrated = true;
  columnFoldStore.setState({ columns: readColumnFold(cookie) });
}

/** React selector hook over {@link columnFoldStore}. */
export function useColumnFold<T>(selector: (state: ColumnFoldState) => T): T {
  return useStore(columnFoldStore, selector);
}

/** Test-only reset: every column showing, unhydrated, no cookie written. */
export function resetColumnFoldForTest(): void {
  hydrated = false;
  columnFoldStore.setState({ columns: columnsUnfolded() });
}
