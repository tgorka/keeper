import { describe, expect, it } from "vitest";
import { splitMarkerLink } from "./follow-link";

describe("a wikilink's anchor", () => {
  it.each([
    ["Kelly sync#The price we agreed", { note: "Kelly sync", name: "The price we agreed" }],
    // This note.
    ["#The price we agreed", { note: "", name: "The price we agreed" }],
    // No anchor: an ordinary link, followed as it always was.
    ["Kelly sync", { note: "Kelly sync", name: null }],
    ["Kelly sync#", { note: "Kelly sync", name: null }],
    // Only the first `#` splits: a marker name cannot carry one.
    ["a#b#c", { note: "a", name: "b#c" }],
  ])("reads %s", (target, expected) => {
    expect(splitMarkerLink(target)).toEqual(expected);
  });
});
