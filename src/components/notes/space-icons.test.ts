import { describe, expect, it } from "vitest";
import {
  matchSpaceIcons,
  SPACE_ICON_GROUPS,
  SPACE_ICONS,
  SpaceIconFallback,
  spaceIcon,
} from "@/components/notes/space-icons";

/** Every key the chooser offers, flattened out of the groups. */
function allKeys(groups: readonly { icons: Readonly<Record<string, unknown>> }[]): string[] {
  return groups.flatMap((group) => Object.keys(group.icons));
}

describe("the catalogue", () => {
  it("derives the flat map from the groups, losing nothing and inventing nothing", () => {
    // The map is what `spaceIcon` reads and the groups are what the picker
    // draws. A key in one and not the other is an icon that is either browsable
    // and unstorable or stored and undrawable, and neither shows up until
    // somebody picks that exact glyph.
    const fromGroups = allKeys(SPACE_ICON_GROUPS).sort();
    expect(Object.keys(SPACE_ICONS).sort()).toEqual(fromGroups);
  });

  it("has no key in two groups", () => {
    // A duplicate would silently lose one of the two to `Object.fromEntries`,
    // and the picker would draw the same name twice with different glyphs.
    const keys = allKeys(SPACE_ICON_GROUPS);
    expect(keys.length).toBe(new Set(keys).size);
  });

  it("is much larger than the flat wrap it replaced", () => {
    // 44.4's twenty-four fitted in one wrap. The point of the chooser is that
    // the set no longer has to, and a floor here is what stops a later edit
    // quietly shrinking it back to a size that needed no chooser.
    expect(Object.keys(SPACE_ICONS).length).toBeGreaterThan(120);
  });

  it("spells every key the way frontmatter stores it", () => {
    // Lucide's own kebab-case, because the value in the file is a thing a human
    // hand-editing frontmatter should be able to guess. A stray capital or
    // underscore here is a name nobody types and the search never finds.
    for (const key of Object.keys(SPACE_ICONS)) {
      expect(key, key).toMatch(/^[a-z][a-z0-9-]*$/);
    }
  });

  /**
   * The keys Story 44.4 shipped, written out one by one.
   *
   * Nothing may leave this map. A stored icon name whose entry disappeared
   * draws the fallback on a space somebody deliberately gave a glyph, and the
   * change that did it would be invisible — no test fails, no vault is
   * rewritten, the rail is just quietly wrong. A count would not catch it; the
   * names do.
   */
  it("still offers every glyph Story 44.4 shipped", () => {
    for (const key of [
      "inbox",
      "calendar-days",
      "pin",
      "video",
      "archive",
      "bell",
      "bookmark",
      "briefcase",
      "clock",
      "code",
      "file-text",
      "flag",
      "folder",
      "globe",
      "hash",
      "heart",
      "lightbulb",
      "mic",
      "search",
      "star",
      "tag",
      "target",
      "users",
      "zap",
    ]) {
      expect(SPACE_ICONS[key], key).toBeDefined();
    }
  });

  /**
   * The other half of `every_default_names_an_icon_and_no_two_defaults_share_a_key`
   * in `keeper-core::notes::default_spaces`. Rust names these glyphs as strings
   * and cannot see this file; a default naming an icon the picker has not got
   * renders the unknown-icon fallback on a rail of rows keeper itself wrote.
   */
  it("offers every glyph a seeded default asks for, including the templates one", () => {
    for (const key of ["inbox", "calendar-days", "pin", "video", "layout-template"]) {
      expect(SPACE_ICONS[key], key).toBeDefined();
    }
  });

  it("falls back for an unknown name and for none, without touching the name", () => {
    expect(spaceIcon(null)).toBe(SpaceIconFallback);
    expect(spaceIcon("no-such-glyph")).toBe(SpaceIconFallback);
    expect(spaceIcon("inbox")).toBe(SPACE_ICONS.inbox);
    expect(spaceIcon("inbox")).not.toBe(SpaceIconFallback);
  });
});

describe("matchSpaceIcons", () => {
  it("finds an icon by its own name", () => {
    const hits = allKeys(matchSpaceIcons("template"));
    expect(hits).toContain("layout-template");
    // Narrowed, not merely non-empty: a filter that returned everything would
    // satisfy `toContain` and would not be a search.
    expect(hits.length).toBeLessThan(Object.keys(SPACE_ICONS).length);
  });

  it("ignores case and hyphens in both directions", () => {
    const canonical = allKeys(matchSpaceIcons("calendar-days"));
    expect(canonical).toEqual(["calendar-days"]);
    for (const spelling of ["calendar days", "CALENDAR-DAYS", "  Calendar Days  "]) {
      expect(allKeys(matchSpaceIcons(spelling)), spelling).toEqual(canonical);
    }
  });

  it("finds a glyph by the word a person types when lucide spells it otherwise", () => {
    // `layout-template` is findable by its key too; `house` is not findable by
    // "home", `banknote` is not findable by "money", and those are the searches
    // people actually run. Aliases exist for exactly that gap.
    expect(allKeys(matchSpaceIcons("home"))).toContain("house");
    expect(allKeys(matchSpaceIcons("money"))).toContain("banknote");
    expect(allKeys(matchSpaceIcons("meeting"))).toContain("users");
    expect(allKeys(matchSpaceIcons("scaffold"))).toContain("layout-template");
  });

  it("matches across groups and keeps each group's own label", () => {
    // Two groups in the result, not one: a search that only ever looked in the
    // first group would pass every single-group fixture.
    const groups = matchSpaceIcons("c");
    expect(groups.length).toBeGreaterThan(1);
    for (const group of groups) {
      expect(SPACE_ICON_GROUPS.some((source) => source.label === group.label)).toBe(true);
      expect(Object.keys(group.icons).length).toBeGreaterThan(0);
    }
  });

  it("drops a group with no matches rather than rendering an empty heading", () => {
    const groups = matchSpaceIcons("template");
    expect(groups.map((group) => group.label)).toEqual(["keeper"]);
  });

  it("is every group when the query is blank", () => {
    for (const blank of ["", "   "]) {
      expect(matchSpaceIcons(blank), blank).toBe(SPACE_ICON_GROUPS);
    }
  });

  it("is nothing when nothing matches", () => {
    expect(matchSpaceIcons("qqzzx")).toEqual([]);
  });

  it("never hands back an icon the flat map does not have", () => {
    // The chooser presses what this returns and the save writes that name. A
    // search that could produce a key `spaceIcon` cannot resolve would store a
    // glyph the rail then draws as the fallback.
    for (const key of allKeys(matchSpaceIcons("a"))) {
      expect(SPACE_ICONS[key], key).toBeDefined();
    }
  });
});
