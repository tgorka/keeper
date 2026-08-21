/**
 * The predicate a link carries, read off the document.
 *
 * The markdown parser has never heard of `{reference="…"}`, so it arrives as
 * ordinary text after the link node. These rules have to match the Rust side
 * exactly — the same syntax is parsed twice, once to draw it and once to put it
 * in the graph, and two readings of one syntax is how a note comes to show a
 * relationship that no query can find.
 */
import { describe, expect, it } from "vitest";
import { __predicateAfterForTest as predicateAfter } from "./live-preview";

describe("the predicate written after a link", () => {
  it("reads the reference out of an attribute block", () => {
    expect(predicateAfter('{reference="supports"} and the rest')).toEqual({
      predicate: "supports",
      length: '{reference="supports"}'.length,
    });
  });

  /** Same rule as Rust: a wrong predicate is worse than an absent one. */
  it("refuses an unquoted value", () => {
    expect(predicateAfter("{reference=supports}")).toBeNull();
  });

  /** Same rule as Rust: `[a](b) {draft}` is a sentence with braces in it. */
  it("reads nothing when the text does not start with the brace", () => {
    expect(predicateAfter(' {reference="supports"}')).toBeNull();
  });

  /** Another key is not a predicate; only `reference` has an agreed meaning. */
  it("ignores a block with no reference in it", () => {
    expect(predicateAfter('{strength="weak"}')).toBeNull();
  });

  /** One line, so a stray brace cannot swallow the rest of a note. */
  it("does not cross a newline", () => {
    expect(predicateAfter('{reference=\n"supports"}')).toBeNull();
  });

  it("finds the reference beside another key", () => {
    expect(predicateAfter('{strength="weak" reference="cites"}')?.predicate).toBe("cites");
  });
});
