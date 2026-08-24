/**
 * The predicates a link carries, read off the document.
 *
 * The markdown parser has never heard of `{schema:creator}` or of the older
 * `{reference="cites"}`, so both arrive as ordinary text after the link node.
 * These rules have to match the Rust side exactly — the same syntax is parsed
 * twice, once to draw it and once to put it in the graph, and two readings of
 * one syntax is how a note comes to show a relationship that no query can find.
 *
 * This file pins the READER. Which blocks the renderer then decorates is pinned
 * against a real `EditorView` in `live-preview-marks.test.ts`, because that is a
 * fact about what a reader sees and not about what a function returns.
 */
import { describe, expect, it } from "vitest";
import { __predicatesAfterForTest as predicatesAfter } from "./live-preview";

/** The chips each block draws, block by block, which is what the widget walks. */
function chips(text: string): string[][] {
  return (predicatesAfter(text)?.blocks ?? []).map((block) => block.chips);
}

describe("the predicates written after a link", () => {
  it("reads one CURIE", () => {
    expect(chips("{schema:creator}")).toEqual([["schema:creator"]]);
  });

  it("reads a comma-separated list, in order", () => {
    expect(chips("{schema:creator, foaf:knows}")).toEqual([["schema:creator", "foaf:knows"]]);
  });

  /** Commas are optional. Both spellings are in the syntax contract, and a
   *  reader who has to remember which one is right will write the other. */
  it("reads a space-separated list, in order", () => {
    expect(chips("{schema:creator foaf:knows}")).toEqual([["schema:creator", "foaf:knows"]]);
  });

  it("reads adjacent blocks as a run, keeping each block's own extent", () => {
    const run = predicatesAfter("{dcterms:source}{schema:status} and the rest");

    expect(chips("{dcterms:source}{schema:status}")).toEqual([
      ["dcterms:source"],
      ["schema:status"],
    ]);
    // The extents are what the decoration is placed over, so they are the part
    // that must not drift: each brace pair, and nothing of the prose after.
    expect(run?.blocks.map((block) => [block.from, block.to])).toEqual([
      [0, "{dcterms:source}".length],
      ["{dcterms:source}".length, "{dcterms:source}{schema:status}".length],
    ]);
    expect(run?.length).toBe("{dcterms:source}{schema:status}".length);
  });

  it("stops at the first thing that is not a block", () => {
    expect(predicatesAfter("{schema:creator} {foaf:knows}")?.length).toBe(
      "{schema:creator}".length,
    );
  });

  it("reads a CURIE beside a pair the vault's toolkit writes", () => {
    expect(chips('{schema:creator, rel="cites"}')).toEqual([["schema:creator"]]);
  });

  /** Exact duplicates are dropped, across the whole run and not just within a
   *  block: the chip list IS the list the graph gets, and the graph holds one. */
  it("drops an exact duplicate, in either block", () => {
    expect(chips("{schema:creator, schema:creator}")).toEqual([["schema:creator"]]);
    expect(chips("{schema:creator}{schema:creator}")).toEqual([["schema:creator"], []]);
  });

  it("keeps two predicates that differ only in their prefix", () => {
    expect(chips("{schema:creator, foaf:creator}")).toEqual([["schema:creator", "foaf:creator"]]);
  });

  describe("the spelling keeper shipped first", () => {
    /** A vault written before this story renders exactly as it did: the legacy
     *  `reference` value folds into the same list as its first entry. */
    it("reads the reference out of an attribute block", () => {
      expect(chips('{reference="supports"}')).toEqual([["supports"]]);
    });

    it("finds the reference beside another key", () => {
      expect(chips('{strength="weak" reference="cites"}')).toEqual([["cites"]]);
    });

    it("reads a reference and a CURIE as one list", () => {
      expect(chips('{reference="supports", schema:creator}')).toEqual([
        ["supports", "schema:creator"],
      ]);
    });

    /** Same rule as Rust: a wrong predicate is worse than an absent one. */
    it("refuses an unquoted value", () => {
      const block = predicatesAfter("{reference=supports}")?.blocks[0];

      expect(block?.writesPredicate).toBe(false);
      expect(block?.junk).toBe(true);
    });

    it("refuses an empty value", () => {
      expect(predicatesAfter('{reference=""}')?.blocks[0]?.writesPredicate).toBe(false);
    });
  });

  describe("what is not a predicate", () => {
    /** Another key is not a predicate; `rel="cites"` is the vault's own
     *  attribute and keeps the treatment it has always had, which is source. */
    it("writes no predicate for a block of pairs alone", () => {
      const block = predicatesAfter('{rel="cites", strength="weak"}')?.blocks[0];

      expect(block?.writesPredicate).toBe(false);
      expect(block?.junk).toBe(false);
    });

    /** Words are not CURIEs. Ignoring them is not enough — the block is marked
     *  so the renderer can leave the author's own text where they typed it. */
    it("marks a block of prose as junk", () => {
      const block = predicatesAfter("{not a curie}")?.blocks[0];

      expect(block?.chips).toEqual([]);
      expect(block?.junk).toBe(true);
    });

    it("marks junk beside a good CURIE, without losing the CURIE", () => {
      const block = predicatesAfter("{schema:creator oops!}")?.blocks[0];

      expect(block?.chips).toEqual(["schema:creator"]);
      expect(block?.junk).toBe(true);
    });

    it.each([
      "{schema:}",
      "{:creator}",
      "{9schema:creator}",
      "{schema::creator}",
      "{schema:creator:extra}",
      "{schema.org:creator}",
    ])("refuses %s, which is not the CURIE shape", (text) => {
      expect(predicatesAfter(text)?.blocks[0]?.chips).toEqual([]);
    });
  });

  /** Same rule as Rust: `[a](b) {schema:creator}` is a sentence with braces in
   *  it, not a link with a predicate. */
  it("reads nothing when the text does not start with the brace", () => {
    expect(predicatesAfter(' {reference="supports"}')).toBeNull();
    expect(predicatesAfter(" {schema:creator}")).toBeNull();
  });

  /** One line, so a stray brace cannot swallow the rest of a note. */
  it("does not cross a newline", () => {
    expect(predicatesAfter('{reference=\n"supports"}')).toBeNull();
    expect(predicatesAfter("{schema:creator,\nfoaf:knows}")).toBeNull();
  });

  it("reads nothing at all when there is no block", () => {
    expect(predicatesAfter(" and the rest of the sentence")).toBeNull();
  });
});
