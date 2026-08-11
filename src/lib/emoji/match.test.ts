/**
 * What the emoji menu offers (Story 45.11, FR-185).
 *
 * The rule under test is not this module's — it is Story 44.13's, asked with
 * `_` where a tag path has `/`. So the assertions here are the emoji spelling of
 * `tag-match.test.ts`'s: a query matches at a word boundary or not at all, it
 * ranks a match at the front of a name above one in the middle, and it is total
 * on rubbish.
 */
import { describe, expect, it } from "vitest";
import { EMOJI_MATCH_LIMIT, matchEmoji } from "./match";

/** Just the shortcodes, which is what an assertion about ranking is about. */
function offered(query: string, limit?: number): string[] {
  return matchEmoji(query, limit).map((hit) => hit.shortcode);
}

describe("matchEmoji", () => {
  it("matches a prefix of the first word", () => {
    expect(offered("tad")).toContain("tada");
  });

  it("matches a whole word in the middle of a name", () => {
    // The reason the rule was lifted rather than reused as a plain prefix
    // match: `hands` is a word of `raised_hands`, and a vocabulary of
    // underscore-joined names is unusable if only the first word is reachable.
    expect(offered("hands")).toContain("raised_hands");
  });

  it("refuses a match that starts in the middle of a word", () => {
    // 44.13's rule, and the defect it was written for: `ent` finding `client`
    // is a substring hit nobody reaching for a name meant. Here, `ada` must not
    // find `tada`.
    expect(offered("ada")).not.toContain("tada");
  });

  it("matches across a word boundary the user typed", () => {
    expect(offered("raised_ha")).toContain("raised_hands");
    expect(offered("rais_hands")).not.toContain("raised_hands");
  });

  it("puts the closest answer first", () => {
    // `smile` matches at word 0 with one word; everything longer or deeper
    // follows it. The arrow keys land somewhere predictable or the menu is a
    // lottery.
    expect(offered("smile")[0]).toBe("smile");
    expect(offered("smi")[0]).toBe("smile");
  });

  it("ranks a first-word match above a later-word one of the same length", () => {
    // Deliberately a pair the OTHER two sort terms cannot separate:
    // `woman_artist` and `bald_woman` are both two words, and alphabetically
    // `bald_woman` comes first. Only "where does the match start" puts the one
    // that is *about* women above the one that merely ends in it. `hands` would
    // not have caught this — there, depth alone reproduces the same order.
    const rows = offered("woman", 200);

    expect(rows.indexOf("woman_artist")).toBeGreaterThanOrEqual(0);
    expect(rows.indexOf("bald_woman")).toBeGreaterThanOrEqual(0);
    expect(rows.indexOf("woman_artist")).toBeLessThan(rows.indexOf("bald_woman"));
  });

  it("ranks a short name above a longer one that matches just as early", () => {
    // `smirk` and `smile_cat` both match `smi` at word 0, and alphabetically
    // `smile_cat` wins. The one-word name is the closer answer, and only the
    // length term says so — without it `smi` offers `smile_cat` second and the
    // three most obvious smileys are scattered down the list.
    const rows = offered("smi", 200);

    expect(rows.indexOf("smirk")).toBeLessThan(rows.indexOf("smile_cat"));
  });

  it("orders the same way twice, so a row cannot move under the caret", () => {
    expect(offered("smi")).toEqual(offered("smi"));
  });

  it("folds case, because the trigger accepts it", () => {
    expect(offered("TAD")).toContain("tada");
  });

  it("finds the two shortcodes with punctuation in them", () => {
    expect(offered("+1")).toContain("+1");
    expect(offered("-1")).toContain("-1");
  });

  it("returns the character, not just the name", () => {
    expect(matchEmoji("tada")[0]).toEqual({ shortcode: "tada", emoji: "🎉" });
  });

  it("offers nothing for a shortcode that does not exist", () => {
    expect(matchEmoji("zzzznotanemoji")).toEqual([]);
  });

  it("is capped, so a query that matches everything is not 1855 allocations", () => {
    // A bare `:` explicitly asked for matches the whole table, and CodeMirror
    // renders a hundred rows at most. The cap is above anything a screen shows
    // and far below the table.
    expect(matchEmoji("").length).toBe(EMOJI_MATCH_LIMIT);
    expect(offered("s", 3)).toHaveLength(3);
  });

  it("treats a query with no words in it as no query at all", () => {
    // `___` and `   ` cut into zero segments, so they have narrowed nothing and
    // the whole (capped) vocabulary comes back — the identical behaviour
    // `matchTags` has for `///`. Deciding that a menu should NOT open for that
    // is the completion source's job, not the matcher's, and
    // `emoji-complete.ts` does it at the trigger.
    expect(matchEmoji("___")).toHaveLength(EMOJI_MATCH_LIMIT);
    expect(matchEmoji("   ")).toHaveLength(EMOJI_MATCH_LIMIT);
  });
});
