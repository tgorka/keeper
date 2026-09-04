/**
 * Which bands of the Bots pane are folded (Epic 64, Story 64.1, FR-427,
 * FR-428, AD-184).
 *
 * **Its own cookie, not a key in the column fold's.** `column-fold.ts` keys
 * {@link "@/lib/column-widths".SURFACE_COLUMN_IDS}, and says why: the same ids
 * key `keeper_column_widths`, so a column that could be folded but not resized
 * would be a typo nobody would find. The voice block is not a column — it is a
 * band inside the transcript level, it has no width, and it folds to a LINE
 * rather than to a rail. Putting it under that key set would either widen a
 * type two stores rely on or make the widths store carry a key it cannot use.
 * The notes rail's sections (`notes-rail-fold.ts`) are the precedent for a
 * band inside a column with its own cookie; this follows them.
 *
 * **Folded by default, which is the story.** Epic 63 gave the Mac a voice, and
 * the block that came with it — switch, phrase, language, sentence, note,
 * limits — is 210–260 px of chrome above the transcript on a surface whose
 * one job is the transcript. Folded, the band is one line that says what
 * matters while a conversation is on screen; every control is one click away,
 * and Settings → Bots keeps the whole block unfolded, so the fold removes no
 * path to anything (AD-184). Not "everything starts open", for the reason the
 * notes rail's Files section does not: the honest default is the one the
 * surface is for.
 *
 * The ENCODING is `fold-cookie`'s, shared with every other closed-set fold, so
 * there is one answer to "what does a fold cookie look like".
 *
 * The parse and the writer are pure and take the cookie string, so the round
 * trip is assertable without a document. That is deliberately NOT a test of
 * the restore: the restore is {@link hydrateBotsPaneFold}, mounted in
 * `BotsPane`, and it is exercised there — a store-level test can never see
 * that the pane does not call it (DW-172).
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { foldFlagsCookie, persistFold, readFoldFlags } from "@/lib/stores/fold-cookie";

/** The cookie the Bots pane's band fold lives in. Not the columns', not the rail's. */
export const BOTS_PANE_FOLD_COOKIE = "keeper_bots_pane_fold";

/**
 * The pane's foldable bands, in the order they render and are written.
 *
 * One today. A closed set all the same, for the reason every persisted key
 * set is closed: an id goes into somebody's cookie and comes back out in a
 * later build, so a typo must be droppable and a band that no longer exists
 * must not leave a key nothing can clear.
 */
export const BOTS_PANE_BANDS = ["voice"] as const;

export type BotsPaneBand = (typeof BOTS_PANE_BANDS)[number];

/** Per band: `true` when it is folded to its one line. */
export type BotsPaneFold = Record<BotsPaneBand, boolean>;

/**
 * The pane as a keeper that has never folded anything shows it: the voice
 * block folded, for the module header's reason. A fresh object each call
 * rather than a shared constant: the store mutates what it is handed, and a
 * shared default would be the state every reset restored to once something
 * had written through it.
 */
export function botsPaneUnfolded(): BotsPaneFold {
  return { voice: true };
}

/**
 * The fold remembered in a `document.cookie` string.
 *
 * Tolerant in one direction only: a malformed entry, an unknown key, or a
 * value that is not `0`/`1` is dropped and leaves that band at its default.
 * Reads only {@link BOTS_PANE_FOLD_COOKIE}; a jar holding the column fold and
 * nothing else parses to the defaults.
 */
export function readBotsPaneFold(cookie: string): BotsPaneFold {
  return readFoldFlags(cookie, BOTS_PANE_FOLD_COOKIE, BOTS_PANE_BANDS, botsPaneUnfolded());
}

/**
 * The `document.cookie` assignment that records this fold.
 *
 * Every key, not only the folded ones: a cookie write replaces the name's
 * whole value, and here "unfolded" and "unwritten" already differ, because
 * the default is folded.
 */
export function botsPaneFoldCookie(fold: BotsPaneFold): string {
  return foldFlagsCookie(BOTS_PANE_FOLD_COOKIE, BOTS_PANE_BANDS, fold);
}

export interface BotsPaneFoldState {
  /** What is folded right now. */
  bands: BotsPaneFold;
  /** Fold or unfold one band. */
  toggleBand: (band: BotsPaneBand) => void;
}

export const botsPaneFoldStore = createStore<BotsPaneFoldState>()((set, get) => ({
  bands: botsPaneUnfolded(),
  toggleBand: (band) => {
    const bands = { ...get().bands, [band]: !get().bands[band] };
    persistFold(botsPaneFoldCookie(bands));
    set({ bands });
  },
}));

/** Whether {@link hydrateBotsPaneFold} has already run in this document. */
let hydrated = false;

/**
 * Restore the remembered fold.
 *
 * Idempotent, so React's double-invoked development effects restore once, and
 * a second caller cannot overwrite a fold the user has changed since the
 * first. Mounted in `BotsPane` rather than in `AppShell`, the notes rail's
 * argument: this band renders nowhere else, and the Bots surface is unmounted
 * whenever another primary view is showing, so hydrating at the shell would
 * read a cookie for a band that may never appear.
 */
export function hydrateBotsPaneFold(cookie: string): void {
  if (hydrated) {
    return;
  }
  hydrated = true;
  botsPaneFoldStore.setState({ bands: readBotsPaneFold(cookie) });
}

/** React selector hook over {@link botsPaneFoldStore}. */
export function useBotsPaneFold<T>(selector: (state: BotsPaneFoldState) => T): T {
  return useStore(botsPaneFoldStore, selector);
}

/** Test-only reset: back to the fresh-keeper fold, unhydrated, no cookie written. */
export function resetBotsPaneFoldForTest(): void {
  hydrated = false;
  botsPaneFoldStore.setState({ bands: botsPaneUnfolded() });
}
