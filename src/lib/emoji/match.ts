/**
 * Which shortcodes match what has been typed (Story 45.11, FR-185).
 *
 * **The rule is not this module's invention.** It is Story 44.13's rule, lifted
 * into `lib/segment-match.ts` so it could be asked with a different separator:
 * a shortcode is a path whose segments are joined with `_` instead of `/`, so
 * `hands` finds `raised_hands` the same way `acme` finds `client/acme`, and
 * `and` finds neither, because no segment of either begins with it. Two
 * matchers in one app is how the editor and a chooser start disagreeing about
 * whether `ent` should offer `client`, and 44.13 exists because that had
 * already happened once.
 *
 * **The vocabulary is cut into segments once, at module load.** 1855 rows
 * re-split on every keystroke is ~3700 avoidable allocations per character
 * typed, all of them producing the identical answer, over a table that cannot
 * change while the app is running.
 *
 * **This module is the whole reason the table is loaded at all**, which is why
 * the completion source reaches it through a dynamic `import()`: a user who
 * never types a colon never pays for 1855 rows of emoji.
 */
import { segmentMatchOffset, segmentsOf } from "@/lib/segment-match";
import { EMOJI_TABLE } from "./table";

/** Shortcode segments are joined with `_`; everything else about the rule is shared. */
const SHORTCODE_SEPARATOR = "_";

/** One row of the menu: the shortcode that was matched and what it inserts. */
export interface EmojiMatch {
  readonly shortcode: string;
  readonly emoji: string;
}

/**
 * How many rows a query may offer.
 *
 * A bare `:` explicitly asked for matches 1855 rows, and CodeMirror renders at
 * most a hundred of them: the rest are `Completion` objects allocated on every
 * keystroke so they can be discarded unseen. The cap is well above what fits on
 * a screen, so no query a person types is truncated in a way they could notice,
 * and the pathological one stops being pathological.
 */
export const EMOJI_MATCH_LIMIT = 50;

/** The table with its segments precomputed, in the generated (sorted) order. */
const VOCABULARY: ReadonlyArray<{
  readonly shortcode: string;
  readonly emoji: string;
  readonly segments: readonly string[];
  readonly depth: number;
}> = EMOJI_TABLE.map(([shortcode, emoji]) => {
  const segments = segmentsOf(shortcode, SHORTCODE_SEPARATOR);
  return { shortcode, emoji, segments, depth: segments.length };
});

/**
 * The shortcodes `query` matches, closest first, capped at `EMOJI_MATCH_LIMIT`.
 *
 * The order is (where the match starts, how many words the shortcode has, then
 * the shortcode itself) — the same three total, stable comparisons `matchTags`
 * makes, for the same reason: two runs over the same vocabulary must never
 * disagree about which row the arrow keys land on. So `smi` offers `smile`
 * before `smiley` before `smiling_face_with_tear`, and `hands` offers the
 * shortcodes whose SECOND word is `hands` after any whose first word is,
 * because a match at the front of the name is the closer answer.
 */
export function matchEmoji(query: string, limit: number = EMOJI_MATCH_LIMIT): EmojiMatch[] {
  const wanted = segmentsOf(query, SHORTCODE_SEPARATOR);
  const hits: { shortcode: string; emoji: string; at: number; depth: number }[] = [];
  for (const entry of VOCABULARY) {
    const at = segmentMatchOffset(wanted, entry.segments);
    if (at >= 0) {
      hits.push({ shortcode: entry.shortcode, emoji: entry.emoji, at, depth: entry.depth });
    }
  }
  hits.sort(
    (a, b) =>
      a.at - b.at ||
      a.depth - b.depth ||
      (a.shortcode < b.shortcode ? -1 : a.shortcode > b.shortcode ? 1 : 0),
  );
  hits.length = Math.min(hits.length, limit);
  return hits.map((hit) => ({ shortcode: hit.shortcode, emoji: hit.emoji }));
}

/** Exact lookup, built once beside the segmented vocabulary. */
const BY_SHORTCODE = new Map(EMOJI_TABLE);

/**
 * What `:shortcode:` stands for, or `undefined` if keeper has never heard of it.
 *
 * This is what makes a shortcode somebody typed in full — `:tada:`, closing
 * colon and all — become the character, and it is the same question that makes
 * `:zzzz:` stay the text it is. Case is folded because the matcher folds it and
 * a menu that offers `TAD → tada` must not then refuse to commit it; every
 * shortcode in the table is already lower case, so the table is never folded.
 */
export function emojiFor(shortcode: string): string | undefined {
  return BY_SHORTCODE.get(shortcode.toLowerCase());
}
