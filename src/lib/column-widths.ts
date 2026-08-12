/**
 * Where a resizable column's width lives, and what a width is allowed to be
 * (Story 44.12, FR-167, FR-168, AD-83).
 *
 * **`document.cookie`, not `localStorage` and not an IPC command.** A column
 * width is a lens the viewer chose, not a fact Rust has any use for, so it does
 * not earn a settings row and a binding. `localStorage` is refused across this
 * codebase (`iosSyncDisclosureShownGet` says so out loud), so a cookie is where
 * a remembered pane preference lives. {@link "@/lib/stores/sidebar-fold"}
 * follows the same shape — one cookie per concern, a `keeper_` name, a year —
 * so there is one answer to "where does a remembered pane preference live"
 * rather than two.
 *
 * This paragraph used to say the precedent was "the cookie `SidebarProvider`
 * writes `sidebar_state` to". That was false the day it was written: shadcn's
 * `SidebarProvider` lived in `src/components/ui/sidebar.tsx` and was imported by
 * nothing, so no build of keeper has ever written `sidebar_state`. Story 45.20
 * deleted the dead component and corrected the sentence. If you find
 * `sidebar_state` in a cookie-jar fixture in this repo, that is what it is: a
 * foreign cookie, chosen precisely because keeper does not write it.
 *
 * Everything here is pure and takes the cookie string as an argument. The
 * parsing, the clamping and the grid template are then assertable without a
 * document — but note that a pure test of these is NOT a test of the drag; the
 * drag is exercised through real pointer events in `resizable-columns.test.tsx`,
 * because the arithmetic being right has never been the part that breaks.
 */

/** The cookie every surface's column widths share. One cookie, not one each. */
export const COLUMN_WIDTH_COOKIE = "keeper_column_widths";

/** A year. A pane preference that expires in a week is a pane preference that
 * silently resets on the user who opens keeper on Mondays. */
export const COLUMN_WIDTH_MAX_AGE = 60 * 60 * 24 * 365;

/**
 * The narrowest a sized column may be, in px.
 *
 * Not zero: a column dragged to nothing is a column whose content has vanished
 * with no affordance to bring it back, and the user who did it by accident has
 * no way to know what happened. The floor is wide enough to keep an ellipsis
 * and the overflow trigger on screen, which is the escape hatch.
 */
export const MIN_COLUMN_WIDTH = 72;

/** The widest, in px — past this the other column is the one that has vanished. */
export const MAX_COLUMN_WIDTH = 640;

/** How far one arrow-key press moves a boundary, in px. */
export const COLUMN_KEY_STEP = 8;

/** How far a shifted arrow-key press moves it. Coarse pass, then fine pass. */
export const COLUMN_KEY_STEP_COARSE = 32;

/**
 * A column of a surface, as opposed to a column inside a panel (Story 48.1).
 *
 * The four here are the ones a person sees beside each other in the shell: the
 * notes rail and the note list on Notes, the tree on Files, the chat list on
 * Inbox. Every one of them used to be a hand-written `w-[240px]` or `w-[320px]`
 * on the element itself, which is why until this story none of them could be
 * folded or dragged: a Tailwind literal is not a number anything can remember.
 *
 * One registry rather than a constant per surface, because the fold store keys
 * its cookie on exactly this id set. Two lists would agree until the day a
 * column is added to one of them.
 */
export interface SurfaceColumnSpec {
  /**
   * What the fold and the seam are named after, mid-sentence and lowercase:
   * "Collapse note list", "Resize note list". The label is the column, not the
   * surface — "Resize Notes" would name two different columns on one screen.
   *
   * It must contain {@link SurfaceColumnSpec.title}, case aside: the title is
   * the visible words and this is the spoken name of the control that carries
   * them, and a control whose visible label is not in its accessible name
   * cannot be operated by anyone saying what they see (WCAG 2.5.3).
   */
  label: string;
  /**
   * The column's name as a reader sees it (Story 48.3).
   *
   * {@link SurfaceColumnSpec.label} is written for the middle of a sentence and
   * cannot be shown: "note list" as a heading is a typo, and capitalising it at
   * the call site is a rule that holds until the first surface forgets. So the
   * display form is declared, once, beside the form it must agree with.
   *
   * Distinct across the four, for the same reason the labels are: two columns
   * sit side by side on the Notes surface, and one name over both of them
   * answers nothing.
   */
  title: string;
  /** The width it occupies until somebody drags it, in px. */
  defaultWidth: number;
  /**
   * The narrowest it may be dragged, in px, decided per column rather than
   * shared. {@link MIN_COLUMN_WIDTH} is a floor for a property KEY, where 72px
   * still shows an ellipsis and the overflow trigger that reads the rest. A
   * surface column has no overflow trigger — what is past its edge is simply
   * gone — so its floor is the width of the narrowest row that is still worth
   * reading, per column and stated per column below.
   */
  minWidth: number;
}

/** Every surface column, in no particular order — ids, not positions. */
export const SURFACE_COLUMN_IDS = ["notes-rail", "notes-list", "files-tree", "chat-list"] as const;

export type SurfaceColumnId = (typeof SURFACE_COLUMN_IDS)[number];

export const SURFACE_COLUMNS: Record<SurfaceColumnId, SurfaceColumnSpec> = {
  // 240 is what the rail has been since Story 37.1. The floor is the New note
  // button: a 16px icon, a gap, the words, and the 8px padding either side —
  // under about 180 the label it exists to advertise starts being clipped, and
  // a space row's trailing `+` lands on top of the space's name.
  "notes-rail": { label: "notes rail", title: "Notes rail", defaultWidth: 240, minWidth: 180 },
  // 320 is what the list has been since Story 37.1. The floor holds a row's two
  // lines — a title and a meta line of tag chips — plus the filter bar's search
  // field above them. Narrower and the chips wrap one per line, which makes the
  // list taller rather than narrower and helps nobody.
  "notes-list": { label: "note list", title: "Note list", defaultWidth: 320, minWidth: 240 },
  // The tree used to be `flex-1`, splitting the surface evenly with the panel
  // strip. That was never a decision — it was two panes with the same class —
  // and it gave half the window to a folder list while the document it opened
  // got the other half. 360 because a tree row indents 16px per level and a
  // path four deep still has to show a filename; the floor keeps three levels
  // and a short name, which is the point past which the tree stops being
  // navigable rather than merely tight.
  //
  // "files", not "file tree": this column IS the left half of the Files
  // surface — it hosts that surface's header — and "Files" is what its nav
  // entry, its region name and its tooltip have always called it. Naming the
  // widget instead of the thing is the mistake, and there is only one surface
  // column here, so nothing else on screen competes for the word.
  "files-tree": { label: "files", title: "Files", defaultWidth: 360, minWidth: 220 },
  // 320 is what the inbox has been since the first shell. A chat row is a 40px
  // avatar, a name, a preview line and a timestamp; the floor is where the
  // timestamp would start eating the name.
  "chat-list": { label: "chat list", title: "Chat list", defaultWidth: 320, minWidth: 240 },
};

/**
 * The floor `id` is held to.
 *
 * Consulted on read as well as on write, so a width recorded by a build with a
 * lower floor — or by a hand-edited jar — comes back inside today's range
 * rather than being enforced at one call site and leaked at the next. An id
 * that is not a surface column — the Properties key column is the only other
 * one — keeps the shared {@link MIN_COLUMN_WIDTH}.
 */
export function columnMinWidth(id: string): number {
  // `id` is a plain string because the Properties key column is one too. The
  // cast widens the key type for the lookup and nothing else; the `undefined`
  // in it is the honest result for a key the record does not hold, which is
  // what the fallback below is for.
  const table = SURFACE_COLUMNS as Record<string, SurfaceColumnSpec | undefined>;
  return table[id]?.minWidth ?? MIN_COLUMN_WIDTH;
}

/** Ids are ours, so they are constrained rather than escaped. */
const ID = /^[a-z][a-z0-9-]*$/;

/**
 * Hold a width inside the range a column is allowed to occupy.
 *
 * `min` defaults to the shared floor and is overridden per surface column
 * ({@link columnMinWidth}): a whole column and a property key are both columns
 * and they are not both readable at 72px.
 */
export function clampColumnWidth(px: number, min: number = MIN_COLUMN_WIDTH): number {
  if (!Number.isFinite(px)) {
    return min;
  }
  return Math.round(Math.min(MAX_COLUMN_WIDTH, Math.max(min, px)));
}

/**
 * Every remembered width in a `document.cookie` string.
 *
 * A malformed pair is dropped rather than throwing: the cookie is shared with
 * every other cookie on the origin and with older builds of keeper, and a pane
 * that refuses to render because someone's jar has a stale entry is a worse
 * outcome than a column that starts at its fitted width.
 */
export function readColumnWidths(cookie: string): Record<string, number> {
  const widths: Record<string, number> = {};
  for (const pair of cookie.split(";")) {
    const separator = pair.indexOf("=");
    if (separator === -1 || pair.slice(0, separator).trim() !== COLUMN_WIDTH_COOKIE) {
      continue;
    }
    for (const entry of decodeURIComponent(pair.slice(separator + 1).trim()).split("|")) {
      const colon = entry.indexOf(":");
      if (colon === -1) {
        continue;
      }
      const id = entry.slice(0, colon);
      const px = Number.parseInt(entry.slice(colon + 1), 10);
      if (ID.test(id) && Number.isFinite(px)) {
        widths[id] = clampColumnWidth(px, columnMinWidth(id));
      }
    }
  }
  return widths;
}

/**
 * The `document.cookie` assignment that records `id` at `px` — or forgets it,
 * when `px` is null, so "reset to fit" leaves nothing behind to re-adopt.
 *
 * Takes the current cookie because a cookie write replaces one name's value
 * wholesale: composing the next value from the current one is what keeps the
 * Files pane's width when the Properties panel's is dragged.
 */
export function columnWidthCookie(cookie: string, id: string, px: number | null): string {
  const widths = readColumnWidths(cookie);
  if (px === null) {
    delete widths[id];
  } else {
    widths[id] = clampColumnWidth(px, columnMinWidth(id));
  }
  const value = Object.entries(widths)
    .map(([key, width]) => `${key}:${width}`)
    .join("|");
  return `${COLUMN_WIDTH_COOKIE}=${encodeURIComponent(value)}; path=/; max-age=${COLUMN_WIDTH_MAX_AGE}`;
}

/**
 * The grid the two columns and their boundary occupy.
 *
 * **Fit is the browser's job, and only the browser can do it** (AD-83). An
 * unsized column is `fit-content(50%)`: the layout engine measures the real
 * glyphs in the real font and gives the column exactly that, capped at half the
 * pane so one absurd key cannot eat the values beside it. Measuring text in
 * TypeScript to compute the same number would be a second, worse font metric —
 * and the reason the column is a guess today is that somebody wrote `w-32`.
 *
 * The middle track is zero-wide and holds the drag handle, which straddles the
 * boundary as an overlay of its own. A real track keeps the handle in flow and
 * spanning every row, without a measured absolute position that would have to
 * be recomputed on each layout.
 */
export function columnTemplate(width: number | null): string {
  const first = width === null ? `minmax(${MIN_COLUMN_WIDTH}px, fit-content(50%))` : `${width}px`;
  return `${first} 0px minmax(0, 1fr)`;
}
