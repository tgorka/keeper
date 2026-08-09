/**
 * "Which tags match what I typed" — stated once, for every tag chooser
 * (Story 44.13, FR-169).
 *
 * **Why this file exists.** Before it, keeper had no answer to that question:
 * it had two delegations of it. The editor's `#` popup
 * (`components/notes/editor/tag-complete.ts`) handed the whole vocabulary to
 * CodeMirror and let CodeMirror's fuzzy matcher decide, and the recording
 * card's field (`tag-vocabulary-input.tsx`) handed the whole vocabulary to a
 * `<datalist>` and let the browser decide. Two libraries, two rules, neither of
 * them keeper's, and neither of them aware that a tag is a PATH. Typing `ent`
 * in the editor offered `client` — a substring hit in the middle of a segment,
 * which is never what someone reaching for a tag meant.
 *
 * **The rule is the tree's own rule: segments, aligned at a boundary.** The
 * vocabulary arrives as full paths (`client`, `client/acme`,
 * `client/acme/renewal`) because that is how `keeper-core`'s tag tree is
 * flattened. A query is split the same way, and it matches when its segments
 * line up against a consecutive run of the tag's segments that BEGINS at a
 * segment boundary: every query segment but the last must equal its segment
 * outright, and the last may be a prefix of its own. So `cl/ac` finds
 * `client/acme`, `acme` finds `client/acme` without knowing about `client`,
 * and `ent` finds nothing, because no tag has a segment that starts with it.
 *
 * **Case is folded here, and that is a search rule, not a tag rule.** What a
 * tag IS — its case, its whitespace, its slashes — is decided exactly once, in
 * `keeper-core/src/notes/tags.rs::normalise`, and nothing here reverses or
 * restates it. Folding case to decide what to OFFER is the same kind of
 * courtesy a `<datalist>` extends; it never rewrites the text the user typed
 * and never invents a canonical form. Every string this module returns came
 * out of the vocabulary or out of the user's own keystrokes.
 */

/**
 * A tag path cut into its segments, case-folded, with empty segments dropped.
 *
 * Dropping empties is what makes a half-typed hierarchy behave: `client/` is
 * one segment, so it still matches `client` and everything beneath it rather
 * than matching nothing while the user's finger is still on the slash.
 */
function segmentsOf(path: string): string[] {
  const out: string[] = [];
  for (const segment of path.toLowerCase().split("/")) {
    const trimmed = segment.trim();
    if (trimmed !== "") {
      out.push(trimmed);
    }
  }
  return out;
}

/**
 * The index of the tag segment where `query` starts matching, or `-1`.
 *
 * The index is kept rather than thrown away because it is the ranking signal
 * that matters: a tag the query matches from its root is a closer answer than
 * one it matches three levels down, and sorting by it puts `acme` above
 * `client/acme/renewal` for the query `acme` without a scoring heuristic
 * anybody has to tune.
 */
export function tagMatchOffset(query: string, tag: string): number {
  const wanted = segmentsOf(query);
  const have = segmentsOf(tag);
  if (wanted.length === 0) {
    return 0;
  }
  const last = wanted.length - 1;
  for (let start = 0; start + wanted.length <= have.length; start += 1) {
    let hit = true;
    for (let i = 0; i < wanted.length; i += 1) {
      const q = wanted[i] as string;
      const t = have[start + i] as string;
      if (i === last ? !t.startsWith(q) : t !== q) {
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

/**
 * The vocabulary narrowed to what `query` matches, closest first.
 *
 * An empty query returns the vocabulary untouched and in its own order, which
 * is the whole point of the control this feeds: the list is there to be browsed
 * before a single key is pressed, and re-sorting it when there is nothing to
 * sort by would shuffle a list the user is reading.
 *
 * The order for a real query is (how deep the match starts, how deep the tag
 * is, then the path) — three total, stable comparisons, so two runs over the
 * same vocabulary can never disagree about which row the arrow keys land on.
 */
export function matchTags(query: string, vocabulary: readonly string[]): string[] {
  if (segmentsOf(query).length === 0) {
    return [...vocabulary];
  }
  const hits: { tag: string; at: number; depth: number }[] = [];
  for (const tag of vocabulary) {
    const at = tagMatchOffset(query, tag);
    if (at >= 0) {
      hits.push({ tag, at, depth: segmentsOf(tag).length });
    }
  }
  hits.sort(
    (a, b) => a.at - b.at || a.depth - b.depth || (a.tag < b.tag ? -1 : a.tag > b.tag ? 1 : 0),
  );
  return hits.map((hit) => hit.tag);
}

/**
 * Whether `query` already names one of `tags`.
 *
 * This is what decides whether "create it" is on offer, so it compares
 * segment-wise rather than by string equality: `/Client/Acme/` and
 * `client/acme` are the same path written twice, and offering to create the
 * second one while the first is right there would be the control lying about
 * its own vocabulary. It is deliberately NOT a re-implementation of
 * `normalise` — a query that only Rust's folding would reconcile (`My Tag`
 * against `my-tag`) is allowed through as a creation, and Rust folds it into
 * the existing tag at the boundary, which is the correct outcome by a
 * different road.
 */
export function namesTag(query: string, tags: readonly string[]): boolean {
  const wanted = segmentsOf(query).join("/");
  if (wanted === "") {
    return false;
  }
  return tags.some((tag) => segmentsOf(tag).join("/") === wanted);
}
