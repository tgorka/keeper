/**
 * Story 44.11 — the words a count is said in.
 *
 * The three surfaces each test that they pass the RIGHT number. This file tests
 * only what happens to a number once it arrives, which is the part all three
 * share and the part that would otherwise be asserted three times with three
 * slightly different expectations.
 */
import { describe, expect, it } from "vitest";
import { countLabel, ITEMS, NOTES, SESSIONS } from "@/lib/count-label";

describe("countLabel", () => {
  it("says zero as a number rather than as a silence", () => {
    // The AC's own words: an empty set says zero rather than hiding the count.
    // "No notes" would read better in one place and would make "did the count
    // fail to load" unanswerable in all three.
    expect(countLabel(0, NOTES)).toBe("0 notes");
    expect(countLabel(0, SESSIONS)).toBe("0 sessions");
    expect(countLabel(0, ITEMS)).toBe("0 items");
  });

  it("takes the singular only for exactly one", () => {
    expect(countLabel(1, NOTES)).toBe("1 note");
    expect(countLabel(2, NOTES)).toBe("2 notes");
  });

  it("groups a long number", () => {
    expect(countLabel(12_345, NOTES)).toBe(`${(12_345).toLocaleString()} notes`);
  });

  it("names both numbers when a cap declined some of them", () => {
    // A space with `keeper.limit: 20` whose query matches 347. Saying `20` alone
    // is the same defect as counting the rendered window: a number that looks
    // like a total and silently is not.
    expect(countLabel(20, NOTES, { of: 347 })).toBe("20 of 347 notes");
    // The noun agrees with the number it directly follows. A space capped at
    // one over four matches reads "1 of 4 notes", never "1 of 4 note".
    expect(countLabel(1, NOTES, { of: 4 })).toBe("1 of 4 notes");
    expect(countLabel(0, NOTES, { of: 1 })).toBe("0 of 1 note");
  });

  it("leaves the second number out when the cap declined nothing", () => {
    // A cap of 500 over 12 matches is a cap that did not bite, and `12 of 12`
    // reads as a defect.
    expect(countLabel(12, NOTES, { of: 12 })).toBe("12 notes");
    expect(countLabel(12, NOTES, { of: 4 })).toBe("12 notes");
    expect(countLabel(12, NOTES)).toBe("12 notes");
  });

  it("marks a floor with a plus and never lets it pass for a total", () => {
    // The Files tree stops reading at the listing cap. `1,000+ items` declines
    // to be a total in the number itself, rather than in a sentence below it
    // that a reader may never reach.
    expect(countLabel(1000, ITEMS, { atLeast: true })).toBe(`${(1000).toLocaleString()}+ items`);
    // Plural even at one, because "at least one" is not "one".
    expect(countLabel(1, ITEMS, { atLeast: true })).toBe("1+ items");
  });
});
