/**
 * Truncating text without cutting a character in half (Story 44.12, FR-168,
 * AD-83).
 *
 * `String.prototype.slice` counts UTF-16 code units, so `slice(0, 400)` on a
 * note whose 400th unit is the first half of a surrogate pair emits a lone
 * surrogate — which renders as `` and, worse, is not the text the file
 * contains. An emoji, a flag, a family, a Devanagari cluster and an accented
 * letter written as base-plus-combining-mark are all more than one code unit,
 * and none of them is exotic in someone's own vault.
 *
 * Prefer CSS to this. `text-overflow: ellipsis` truncates at the glyph the
 * layout engine actually painted, in the font it actually used, and it re-does
 * that on every resize for free. This exists only for the places where a length
 * cap is a real requirement — a preview of a block keeper has decided not to
 * parse, which must not paste an entire file into the panel.
 */

/** The character a truncated value ends in. One code point, never `...`. */
export const ELLIPSIS = "…";

/** The one member of `Intl.Segmenter` this file uses. */
interface GraphemeSegmenter {
  segment(input: string): Iterable<{ segment: string }>;
}

/** The `Intl` object as the runtime actually presents it. */
interface IntlWithSegmenter {
  Segmenter?: new (locales: undefined, options: { granularity: "grapheme" }) => GraphemeSegmenter;
}

/**
 * `Intl.Segmenter` exists in every runtime keeper ships on and does not exist
 * in this project's `lib` — `tsconfig.json` stops at ES2020, and the type
 * arrived in ES2022. Widening `lib` to type one constructor would change how
 * every file in the codebase is checked, so the shape is declared above.
 *
 * Unchecked, and deliberately: this is not external input, it is a standard
 * global whose declaration this project's `lib` predates. The `?` is what makes
 * the fallback path below reachable rather than theoretical.
 */
const RUNTIME_INTL = globalThis.Intl as unknown as IntlWithSegmenter;

/**
 * `text` cut to at most `limit` user-perceived characters, ending in `…` when
 * anything was removed. Returns the input unchanged when it already fits, so a
 * short value never grows an ellipsis it did not earn.
 */
export function truncateGraphemes(text: string, limit: number): string {
  // Cheap reject first: a string shorter than the limit in code units cannot be
  // longer than it in graphemes, and this is the common case by far.
  if (text.length <= limit) {
    return text;
  }
  const clusters: string[] = [];
  // `Intl.Segmenter` is the only correct answer — it knows that ZWJ sequences
  // and combining marks belong to the cluster before them. Where it is missing,
  // iterating code points is still strictly better than code units: it cannot
  // split a surrogate pair, only a cluster, which degrades to a visible glyph
  // rather than a broken one.
  const Segmenter = RUNTIME_INTL.Segmenter;
  const source: Iterable<string | { segment: string }> =
    Segmenter === undefined
      ? text
      : new Segmenter(undefined, { granularity: "grapheme" }).segment(text);
  for (const piece of source) {
    clusters.push(typeof piece === "string" ? piece : piece.segment);
    // One past the limit is all it takes to know truncation is needed; reading
    // the rest of a large note to count clusters we are about to discard is
    // work with no consumer.
    if (clusters.length > limit) {
      return `${clusters.slice(0, limit).join("")}${ELLIPSIS}`;
    }
  }
  return text;
}
