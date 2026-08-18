/**
 * Which of a file pane's two bands are folded away (Story 53.3, FR-316, FR-318).
 *
 * **Two flags, and they are a standing preference rather than a per-file one.**
 * `TextFileFrame` can put two bands above the file itself: the properties form
 * (Story 50.4) and AD-102's standing caveat. Both are worth reading once and
 * neither is worth reading over every file, and the answer a reader gives by
 * folding one is an answer about how they want to read files — not about
 * `README.md`. So the key set is the two BANDS, and a per-file key is
 * deliberately not expressible here: `fold-cookie.ts` refuses an open key map
 * ("a typo must be droppable and a section that no longer exists must not leave
 * a key nothing can clear"), and the surface has no per-file question to ask it.
 *
 * **Why a cookie and not `useState` in the frame.** The frame outlives the file
 * it shows — a panel replaces its target in place (`text-file-frame.tsx`) — and a
 * folded panel unmounts its body entirely (`panel-strip.tsx`), which throws away
 * every hook inside it. A reader who folded the properties away, folded the
 * panel, and unfolded it would have been shown the form again, which is the shape
 * of a preference that is not one.
 *
 * **Its own cookie, its own closed key set**, as `fold-cookie.ts` requires: the
 * notes rail also has a section called Files and the chat sidebar has a group
 * called `spaces`, and a shared namespace is how folding one silently folds
 * another.
 *
 * The restore is {@link hydrateFileFrameFold}, and it is mounted in
 * `TextFileFrame` itself rather than at the shell — the frame is the only surface
 * these two keys belong to, it is unmounted for the whole of a session that opens
 * no file, and it is the one place the call can be forgotten (DW-172). A
 * store-level test cannot see that omission; the frame's own suite can.
 */
import { useStore } from "zustand";
import { createStore } from "zustand/vanilla";
import { foldFlagsCookie, persistFold, readFoldFlags } from "@/lib/stores/fold-cookie";

/** The cookie a file pane's folds live in. Not the notes rail's, not a column's. */
export const FILE_FRAME_FOLD_COOKIE = "keeper_file_frame_fold";

/**
 * The two bands, in the order they render above the file.
 *
 * `properties` is Story 50.4's form; `caveat` is AD-102's standing sentence,
 * whose folded form is Rust's own one-line composition rather than an absence —
 * the fact never leaves the screen, only three of its four sentences do.
 */
export const FILE_FRAME_BANDS = ["properties", "caveat"] as const;

export type FileFrameBand = (typeof FILE_FRAME_BANDS)[number];

/** Per band: `true` when that band is folded. */
export type FileFrameFold = Record<FileFrameBand, boolean>;

/**
 * How a keeper that has never folded anything sees a file.
 *
 * BOTH folded, and neither default is arbitrary. `properties` matches the notes
 * surface, where `showProperties` has defaulted closed since Story 49
 * (`note-editor.tsx`) — two surfaces disagreeing about the same panel is drift
 * the reader pays for in both. `caveat` folded is the one line rather than the
 * four: AD-102 asks that the fact be on screen before the first keystroke, and it
 * is, in a sentence composed for the purpose.
 */
export function fileFrameFolded(): FileFrameFold {
  return { properties: true, caveat: true };
}

/** The fold remembered in a `document.cookie` string. Pure, so the round trip is
 *  assertable without a document. */
export function readFileFrameFold(cookie: string): FileFrameFold {
  return readFoldFlags(cookie, FILE_FRAME_FOLD_COOKIE, FILE_FRAME_BANDS, fileFrameFolded());
}

/** The `document.cookie` assignment that records this fold. */
export function fileFrameFoldCookie(fold: FileFrameFold): string {
  return foldFlagsCookie(FILE_FRAME_FOLD_COOKIE, FILE_FRAME_BANDS, fold);
}

export interface FileFrameFoldState {
  /** What is folded right now. */
  bands: FileFrameFold;
  /** Fold or unfold one band, and remember it. */
  toggleBand: (band: FileFrameBand) => void;
}

export const fileFrameFoldStore = createStore<FileFrameFoldState>()((set, get) => ({
  bands: fileFrameFolded(),
  toggleBand: (band) => {
    const bands = { ...get().bands, [band]: !get().bands[band] };
    persistFold(fileFrameFoldCookie(bands));
    set({ bands });
  },
}));

/** Whether {@link hydrateFileFrameFold} has already run in this document. */
let hydrated = false;

/**
 * Restore the remembered fold.
 *
 * Idempotent, so React's double-invoked development effects restore once and so
 * the second file pane a reader opens cannot undo a fold they changed in the
 * first. Called from `TextFileFrame` — every surface that draws these two bands
 * mounts one, and there is no earlier place that is not a shell hydrating a
 * cookie for a pane most sessions never open.
 */
export function hydrateFileFrameFold(cookie: string): void {
  if (hydrated) {
    return;
  }
  hydrated = true;
  fileFrameFoldStore.setState({ bands: readFileFrameFold(cookie) });
}

/** React selector hook over {@link fileFrameFoldStore}. */
export function useFileFrameFold<T>(selector: (state: FileFrameFoldState) => T): T {
  return useStore(fileFrameFoldStore, selector);
}

/** Test-only reset: back to both folded, unhydrated, no cookie written. */
export function resetFileFrameFoldForTest(): void {
  hydrated = false;
  fileFrameFoldStore.setState({ bands: fileFrameFolded() });
}
