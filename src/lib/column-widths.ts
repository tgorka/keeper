/**
 * Where a resizable column's width lives, and what a width is allowed to be
 * (Story 44.12, FR-167, FR-168, AD-83).
 *
 * **`document.cookie`, not `localStorage` and not an IPC command.** A column
 * width is a lens the viewer chose, not a fact Rust has any use for, so it does
 * not earn a settings row and a binding. `localStorage` is refused across this
 * codebase (`iosSyncDisclosureShownGet` says so out loud), and the one durable
 * store the frontend already keeps a view preference in is the cookie
 * `SidebarProvider` writes `sidebar_state` to. This follows that, so there is
 * one answer to "where does a remembered pane preference live" rather than two.
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

/** Ids are ours, so they are constrained rather than escaped. */
const ID = /^[a-z][a-z0-9-]*$/;

/** Hold a width inside the range a column is allowed to occupy. */
export function clampColumnWidth(px: number): number {
  if (!Number.isFinite(px)) {
    return MIN_COLUMN_WIDTH;
  }
  return Math.round(Math.min(MAX_COLUMN_WIDTH, Math.max(MIN_COLUMN_WIDTH, px)));
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
        widths[id] = clampColumnWidth(px);
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
    widths[id] = clampColumnWidth(px);
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
