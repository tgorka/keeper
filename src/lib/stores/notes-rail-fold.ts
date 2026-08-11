/**
 * Which sections of the notes rail are folded (Story 47.3, FR-198).
 *
 * **What was wrong before this.** Story 45.20 built a fold and Story 46.13 built
 * another, and neither one landed on the surface the report was about. The chat
 * sidebar's groups fold; the Files panel strip folds; the notes rail — the
 * column with Spaces, Tags and Files in it — had exactly one foldable thing in
 * it, the Files tree, and that fold was a local `useState` that forgot itself on
 * every surface switch. The other two sections had no control at all.
 *
 * **Its own cookie, not a key in the sidebar's.** {@link SIDEBAR_GROUPS} is a
 * closed union used as a storage key, and it already contains `spaces` — the
 * chat sidebar's Matrix space list. The notes rail's first section is also
 * called Spaces and is a list of saved queries. Widening that union would have
 * made one bit stand for two unrelated sections on two unrelated surfaces:
 * folding a saved-query list would fold a Matrix space list, and no test that
 * renders one surface can see it. Two cookies make the collision impossible
 * rather than merely unlikely, and they also give the honest behaviour — these
 * are different surfaces and a person may want Spaces shut in one and open in
 * the other. The cost is one more `hydrate…` call site, paid in `NotesPane`.
 *
 * **Files starts folded and the others start open.** Not a preference: the Files
 * tree loads one `notes_tree` call per expanded directory and has been collapsed
 * on arrival since Story 37.9, so an open default would turn every mount of the
 * notes surface into a cold directory scan nobody asked for. Spaces and Tags are
 * already loaded by the time they render, so their honest default is open.
 *
 * **Folding hides the rows, never the header.** Spaces in particular: its header
 * carries "Restore default spaces", the one control that refills a vault whose
 * owner deleted every default (Story 44.3). That control lives beside the
 * disclosure in the header row, outside the region the fold hides, so a folded
 * Spaces is still a Spaces you can restore into. The same rule makes every fold
 * reversible — the only way back is through the header.
 *
 * The parse and the render are pure and take the cookie string, so the round
 * trip is assertable without a document. That is deliberately NOT a test of the
 * restore: the restore is {@link hydrateNotesRailFold}, mounted in `NotesPane`,
 * and it is exercised there — a store-level test can never see that the pane
 * does not call it (DW-172).
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { foldFlagsCookie, persistFold, readFoldFlags } from "@/lib/stores/fold-cookie";

/** The cookie the notes rail's fold lives in. Not the chat sidebar's. */
export const NOTES_RAIL_FOLD_COOKIE = "keeper_notes_rail_fold";

/**
 * The rail's sections, in the order they render and the order they are written.
 *
 * A closed set rather than an open map, for the reason every persisted key set
 * is closed: an id goes into somebody's cookie and comes back out in a later
 * build, so a typo must be droppable and a section that no longer exists must
 * not leave a key nothing can clear.
 */
export const NOTES_RAIL_GROUPS = ["spaces", "tags", "files"] as const;

export type NotesRailGroup = (typeof NOTES_RAIL_GROUPS)[number];

/** Per section: `true` when that section's rows are folded away. */
export type NotesRailFold = Record<NotesRailGroup, boolean>;

/**
 * The rail as a keeper that has never folded anything shows it.
 *
 * Files folded, for the lazy-load reason in this module's header. A fresh
 * function each call rather than a shared constant: the store mutates what it
 * is handed, and a shared default would be the state every reset restored to
 * once something had written through it.
 */
export function notesRailUnfolded(): NotesRailFold {
  return { spaces: false, tags: false, files: true };
}

/**
 * The fold remembered in a `document.cookie` string.
 *
 * Tolerant in one direction only: a malformed entry, an unknown key, or a value
 * that is not `0`/`1` is dropped and leaves that section at its default. A rail
 * that refused to render because a jar held a stale entry would be a far worse
 * outcome than a rail that starts where a fresh keeper starts.
 *
 * Reads only {@link NOTES_RAIL_FOLD_COOKIE}. A jar holding the chat sidebar's
 * cookie and nothing else parses to the defaults, which is the whole point of
 * the two names.
 */
export function readNotesRailFold(cookie: string): NotesRailFold {
  return readFoldFlags(cookie, NOTES_RAIL_FOLD_COOKIE, NOTES_RAIL_GROUPS, notesRailUnfolded());
}

/**
 * The `document.cookie` assignment that records this fold.
 *
 * Every key, not only the folded ones: a cookie write replaces the name's whole
 * value, so omitting an open section would make "open" indistinguishable from
 * "written by an older build" — and here the two already differ, because Files
 * defaults folded.
 */
export function notesRailFoldCookie(fold: NotesRailFold): string {
  return foldFlagsCookie(NOTES_RAIL_FOLD_COOKIE, NOTES_RAIL_GROUPS, fold);
}

export interface NotesRailFoldState {
  /** What is folded right now. */
  groups: NotesRailFold;
  /** Fold or unfold one section. */
  toggleGroup: (group: NotesRailGroup) => void;
}

export const notesRailFoldStore = createStore<NotesRailFoldState>()((set, get) => ({
  groups: notesRailUnfolded(),
  toggleGroup: (group) => {
    const groups = { ...get().groups, [group]: !get().groups[group] };
    persistFold(notesRailFoldCookie(groups));
    set({ groups });
  },
}));

/** Whether {@link hydrateNotesRailFold} has already run in this document. */
let hydrated = false;

/**
 * Restore the remembered fold.
 *
 * Idempotent, so React's double-invoked development effects restore once, and so
 * a second caller cannot overwrite a fold the user has already changed since the
 * first. Mounted in `NotesPane` rather than in `AppShell`, unlike the chat
 * sidebar's: these three sections render nowhere else, and the notes surface is
 * unmounted whenever another primary view is showing, so hydrating at the shell
 * would read a cookie for a rail that may never appear.
 */
export function hydrateNotesRailFold(cookie: string): void {
  if (hydrated) {
    return;
  }
  hydrated = true;
  notesRailFoldStore.setState({ groups: readNotesRailFold(cookie) });
}

/** React selector hook over {@link notesRailFoldStore}. */
export function useNotesRailFold<T>(selector: (state: NotesRailFoldState) => T): T {
  return useStore(notesRailFoldStore, selector);
}

/** Test-only reset: back to the fresh-keeper fold, unhydrated, no cookie written. */
export function resetNotesRailFoldForTest(): void {
  hydrated = false;
  notesRailFoldStore.setState({ groups: notesRailUnfolded() });
}
