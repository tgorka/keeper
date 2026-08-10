/**
 * How a list says how many it holds (Story 44.11, FR-166).
 *
 * ## The one thing this module exists to prevent
 *
 * Story 44.10 windowed the note list, the recordings archive and the Files
 * tree. Every one of them now renders a screenful of a set that may be ten
 * thousand rows long, which means each of them has two numbers within reach —
 * how many rows are mounted, and how many rows exist — that render identically
 * and differ by two orders of magnitude. A count that means "loaded so far"
 * while looking like a total is worse than no count at all: nothing on screen
 * distinguishes it from the right answer, so nobody checks it.
 *
 * So this function takes a number and a noun and nothing else. It cannot be
 * handed a DOM node, an array of rendered rows or a windowing hook, and every
 * caller has to reach past whatever it just rendered to fetch the count from
 * the backend value that knows the whole set. That is the enforcement: the
 * wrong number is not available here.
 *
 * ## The three shapes, and what each one promises
 *
 * | Call | Reads | Promise |
 * | --- | --- | --- |
 * | `countLabel(0, NOTES)` | `0 notes` | The set is empty. |
 * | `countLabel(347, NOTES)` | `347 notes` | Exactly 347 exist. |
 * | `countLabel(20, NOTES, { of: 347 })` | `20 of 347 notes` | 20 are selectable; 347 matched. |
 * | `countLabel(1000, ITEMS, { atLeast: true })` | `1,000+ items` | At least 1 000; the reader stopped counting. |
 *
 * Zero is a number, not a silence. "No notes" would read better in one place
 * and would make "did this count fail to load" unanswerable everywhere, so the
 * digit is always there.
 *
 * `atLeast` is the one honest way to say a count is not exact, and it exists
 * because one of the three surfaces genuinely cannot be exact: `keeper_sync`'s
 * directory reader stops at `LISTING_CAP` entries, and counting past it would
 * cost a `stat` per dirent on folders with fifty thousand of them. `1,000+`
 * declines to be a total in the label itself rather than in a tooltip nobody
 * opens.
 *
 * Grouped through `toLocaleString`, the way the recordings row already stamps a
 * date: a five-digit vault count set solid is a number people re-read.
 */

/** The noun a surface counts in, singular and plural. */
export interface CountNoun {
  one: string;
  many: string;
}

/** The note list's noun. */
export const NOTES: CountNoun = { one: "note", many: "notes" };

/** The recordings archive's noun. */
export const SESSIONS: CountNoun = { one: "session", many: "sessions" };

/**
 * The Files tree's noun — "item", matching the sentence Rust already composes
 * when a folder runs past the listing cap, so the count and the cap notice
 * below it do not name the same things differently.
 */
export const ITEMS: CountNoun = { one: "item", many: "items" };

export interface CountOptions {
  /**
   * How many the query MATCHED, when a cap declined some of them. Rendered as
   * `20 of 347 notes`.
   *
   * Ignored when it equals or falls below `count`: a cap nobody reached is not
   * worth two numbers, and `347 of 347` reads as a defect.
   */
  of?: number;
  /**
   * Whether `count` is a floor rather than a total — the reader stopped before
   * the end. Renders a `+` and always takes the plural.
   */
  atLeast?: boolean;
}

/**
 * Word a count of things that EXIST. Never pass a rendered-row count.
 */
export function countLabel(
  count: number,
  noun: CountNoun,
  { of, atLeast = false }: CountOptions = {},
): string {
  const shown = `${count.toLocaleString()}${atLeast ? "+" : ""}`;
  // A cap that shrank the count says so in the count. Leaving `of` out here
  // would reintroduce the exact defect the cap was applied to avoid: a number
  // that looks like a total and silently is not.
  const capped = of !== undefined && of > count;
  // The noun agrees with the number it directly follows, which in the two-number
  // form is `of` and not `count`: "1 of 4 notes", never "1 of 4 note". `atLeast`
  // is always plural — "at least one" is not "one".
  const agrees = capped ? of : count;
  const word = atLeast || agrees !== 1 ? noun.many : noun.one;
  if (capped) {
    return `${shown} of ${of.toLocaleString()} ${word}`;
  }
  return `${shown} ${word}`;
}
