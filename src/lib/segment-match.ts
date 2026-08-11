/**
 * "Do these segments line up?" — one matching rule, stated over segments rather
 * than over slashes (lifted out of Story 44.13 by Story 45.11).
 *
 * **Why this file exists.** 44.13 settled how a typed query matches a
 * hierarchical name and put the answer in `components/tags/tag-match.ts`: the
 * query's segments must line up against a consecutive run of the candidate's
 * segments that BEGINS at a segment boundary, every query segment but the last
 * matching its own outright and the last matching as a prefix. That rule is
 * not about tags. An emoji shortcode is the same shape with `_` where a tag
 * path has `/` — `raised_hands` is `raised` then `hands` exactly as
 * `client/acme` is `client` then `acme` — and Story 45.11 needs it over 1855
 * shortcodes. Restating the rule for a second separator is how `ent` starts
 * finding `client` again on one surface and not the other, so the rule lives
 * here and every chooser asks it.
 *
 * **Candidates arrive pre-split, deliberately.** `matchTags` re-splits every
 * tag on every keystroke, which is free across a vault's few dozen tags and is
 * ~1855 lowercasings plus ~1855 array allocations per keystroke across the
 * emoji table. A vocabulary that cannot change while the app is running is cut
 * into segments once, at load; only the query is cut per keystroke.
 *
 * **Case is folded here, and that is a search rule, not a naming rule.**
 * Nothing in this file decides what a tag or a shortcode IS — it decides only
 * what to OFFER, and every string a caller gets back came out of its own
 * vocabulary or out of the user's keystrokes.
 */

/**
 * `text` cut into its case-folded segments, with empty segments dropped.
 *
 * Dropping empties is what makes a half-typed hierarchy behave: `client/` is
 * one segment, so it still matches `client` and everything beneath it rather
 * than matching nothing while the user's finger is still on the slash.
 */
export function segmentsOf(text: string, separator: string): string[] {
  const out: string[] = [];
  for (const segment of text.toLowerCase().split(separator)) {
    const trimmed = segment.trim();
    if (trimmed !== "") {
      out.push(trimmed);
    }
  }
  return out;
}

/**
 * The index of the candidate segment where `query` starts matching, or `-1`.
 *
 * The index is kept rather than thrown away because it is the ranking signal
 * that matters: a candidate the query matches from its root is a closer answer
 * than one it matches three levels down, and sorting by it puts `acme` above
 * `client/acme/renewal` for the query `acme` without a scoring heuristic
 * anybody has to tune.
 *
 * An empty query matches everything at offset 0 — it has narrowed nothing, and
 * saying "no match" for a query nobody has typed yet would empty a list that
 * exists to be browsed.
 */
export function segmentMatchOffset(query: readonly string[], candidate: readonly string[]): number {
  if (query.length === 0) {
    return 0;
  }
  const last = query.length - 1;
  for (let start = 0; start + query.length <= candidate.length; start += 1) {
    let hit = true;
    for (let i = 0; i < query.length; i += 1) {
      const q = query[i] as string;
      const c = candidate[start + i] as string;
      if (i === last ? !c.startsWith(q) : c !== q) {
        hit = false;
        break;
      }
    }
    if (hit) {
      return start;
    }
  }
  return -1;
}
