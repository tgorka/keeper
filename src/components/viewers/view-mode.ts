/**
 * Which of a file's two views the reader last chose, remembered per FORMAT
 * (Story 45.4, FR-177, AD-88, UX-DR67).
 *
 * **Per format, never per file.** A person who prefers to see CSVs as a table
 * prefers it for every CSV, and a person who reads JSON as source reads all of
 * it as source. Keying by path would mean the preference has to be re-taught
 * once per file and would grow without bound in whatever store held it — and
 * the first file of a format a reader opens would always be the wrong view.
 *
 * **`document.cookie`, not `localStorage`.** Same reasoning `column-widths.ts`
 * wrote down and the same store: a chosen view is a lens the reader picked, not
 * a fact Rust has any use for, so it earns no settings row and no binding.
 * `localStorage` is refused across this codebase (`iosSyncDisclosureShownGet`
 * says so out loud), and the durable place the frontend already keeps a pane
 * preference is a cookie. One answer to "where does a remembered view live",
 * not two.
 *
 * Everything here is pure and takes the cookie string as an argument, so the
 * parsing and the defaulting are assertable without a document. What a pure
 * test of these CANNOT prove is that the component reads and writes them at the
 * right moments — that is asserted in `raw-rendered-view.test.tsx` against a
 * real render, because "the preference is stored correctly and consulted
 * never" is the shape of defect this epic exists to stop shipping.
 */

/** The two views every text-shaped format has. `raw` is always editable. */
export type ViewMode = "raw" | "rendered";

/** The cookie every format's remembered view shares. One cookie, not one each
 *  — a jar with a name per format is a jar that hits the per-origin cap. */
export const VIEW_MODE_COOKIE = "keeper_viewer_modes";

/** A year, matching `COLUMN_WIDTH_MAX_AGE`. A view preference that expires in a
 *  week silently resets on the person who opens keeper on Mondays. */
export const VIEW_MODE_MAX_AGE = 60 * 60 * 24 * 365;

/**
 * The view a format opens in when the reader has never said.
 *
 * `rendered` for everything, because the rendered view is the thing and the raw
 * view is the name of the thing — which is the sentence this whole epic is
 * about. Raw is one click away and is what a reader asks for deliberately.
 */
export const DEFAULT_VIEW_MODE: ViewMode = "rendered";

/** Format ids come from the registry's `FILE_FORMATS` table, so they are ours
 *  and are constrained rather than escaped. */
const FORMAT_ID = /^[a-z][a-z0-9+-]*$/;

/** Whether a decoded cookie fragment is one of the two views. */
function isViewMode(value: string): value is ViewMode {
  return value === "raw" || value === "rendered";
}

/**
 * Every remembered view in a `document.cookie` string.
 *
 * A malformed pair is dropped rather than throwing. The jar is shared with
 * every other cookie on the origin and with older builds of keeper, and a
 * viewer that refuses to render because somebody's jar has a stale entry is a
 * far worse outcome than a file that opens in its format's default view.
 */
export function readViewModes(cookie: string): Record<string, ViewMode> {
  const modes: Record<string, ViewMode> = {};
  for (const pair of cookie.split(";")) {
    const separator = pair.indexOf("=");
    if (separator === -1 || pair.slice(0, separator).trim() !== VIEW_MODE_COOKIE) {
      continue;
    }
    for (const entry of decodeURIComponent(pair.slice(separator + 1).trim()).split("|")) {
      const colon = entry.indexOf(":");
      if (colon === -1) {
        continue;
      }
      const format = entry.slice(0, colon);
      const mode = entry.slice(colon + 1);
      if (FORMAT_ID.test(format) && isViewMode(mode)) {
        modes[format] = mode;
      }
    }
  }
  return modes;
}

/**
 * The view `format` opens in: what the reader last chose, or the default.
 *
 * Total on purpose. A format the jar has never heard of, a jar that is empty,
 * a jar written by a build that spelled the value differently — all of them
 * answer `rendered` rather than `undefined`, because every caller of this would
 * otherwise have to repeat the same fallback and one of them would get it
 * wrong.
 */
export function viewModeFor(cookie: string, format: string): ViewMode {
  return readViewModes(cookie)[format] ?? DEFAULT_VIEW_MODE;
}

/**
 * The `document.cookie` assignment that records `format` opening in `mode` — or
 * forgets it, when `mode` is null, so a reset leaves nothing behind to re-adopt.
 *
 * Takes the current cookie because a cookie write replaces one name's value
 * wholesale: composing the next value from the current one is what keeps the
 * CSV preference when the JSON one is changed.
 */
export function viewModeCookie(cookie: string, format: string, mode: ViewMode | null): string {
  const modes = readViewModes(cookie);
  if (mode === null) {
    delete modes[format];
  } else {
    modes[format] = mode;
  }
  const value = Object.entries(modes)
    .map(([key, each]) => `${key}:${each}`)
    .join("|");
  return `${VIEW_MODE_COOKIE}=${encodeURIComponent(value)}; path=/; max-age=${VIEW_MODE_MAX_AGE}`;
}
