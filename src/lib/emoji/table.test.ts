/**
 * The generated vocabulary is real (Story 45.11, AD-92).
 *
 * A generated table has one failure mode nothing else in the app has: it can be
 * *present and empty*. A regenerate that hit a 404, a proxy that served an error
 * page, a parse whose anchor moved — each produces a well-formed TypeScript file
 * that compiles, imports, type-checks, and completes nothing. Every assertion
 * here exists to make that state loud.
 */
import { describe, expect, it } from "vitest";
import { EMOJI_TABLE } from "./table";

/**
 * A floor, not a count.
 *
 * The cheat sheet grows every time Unicode ships a release, so pinning the
 * exact number would turn a routine regeneration into a failing test. 1500 is
 * comfortably under today's 1855 and unreachable by any accident that produces
 * a table at all.
 */
const FLOOR = 1500;

describe("the generated emoji table", () => {
  it("is a vocabulary, not an empty file that happens to compile", () => {
    expect(EMOJI_TABLE.length).toBeGreaterThan(FLOOR);
  });

  it("holds the shortcodes people actually reach for", () => {
    // Named individually rather than sampled: `:+1:` is the one whose `+` the
    // parser could silently drop, and the other four are the ones a reviewer
    // would try by hand.
    const table = new Map(EMOJI_TABLE);

    expect(table.get("+1")).toBe("👍");
    expect(table.get("-1")).toBe("👎");
    expect(table.get("tada")).toBe("🎉");
    expect(table.get("rocket")).toBe("🚀");
    expect(table.get("smile")).toBe("😄");
  });

  it("carries multi-codepoint characters whole", () => {
    // A flag is two regional indicators and a family is three joiners; a
    // generator that took only the first codepoint would still produce a
    // plausible-looking table, and every assertion above would still pass.
    const table = new Map(EMOJI_TABLE);

    expect(table.get("jp")).toBe("🇯🇵");
    expect([...(table.get("jp") as string)].length).toBe(2);
  });

  it("has no shortcode twice, so a lookup cannot depend on which one it found", () => {
    const shortcodes = EMOJI_TABLE.map(([shortcode]) => shortcode);

    expect(new Set(shortcodes).size).toBe(shortcodes.length);
  });

  it("is sorted, so a regeneration is a diff and not a reshuffle", () => {
    const shortcodes = EMOJI_TABLE.map(([shortcode]) => shortcode);

    expect(shortcodes).toEqual([...shortcodes].sort());
  });

  it("holds only characters — nothing that needs github.com to render", () => {
    // GitHub's custom emoji (`:octocat:`, `:shipit:`) are PNGs on GitHub's CDN.
    // Inserting one would put a name in a note that no font can draw.
    const table = new Map(EMOJI_TABLE);

    expect(table.has("octocat")).toBe(false);
    expect(table.has("shipit")).toBe(false);
    for (const [shortcode, emoji] of EMOJI_TABLE) {
      expect(emoji, shortcode).not.toBe("");
      expect(emoji, shortcode).not.toMatch(/^[\x20-\x7e]+$/);
    }
  });

  it("spells every shortcode the way the trigger will look for it", () => {
    // The completion's trigger class is `[a-zA-Z0-9_+-]`. A shortcode holding
    // anything else could never be typed, so it would be an entry that exists
    // only to be scrolled past.
    for (const [shortcode] of EMOJI_TABLE) {
      expect(shortcode, shortcode).toMatch(/^[a-z0-9_+-]+$/);
    }
  });
});
