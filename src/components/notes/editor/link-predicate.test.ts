/**
 * The predicates a link carries, read off the document.
 *
 * The markdown parser has never heard of `{schema:creator}`, of `{:depends_on}`
 * or of the older `{reference="cites"}`, so they all arrive as ordinary text
 * after the link node. The syntax is kramdown's IAL with Semantic Markdown V0's
 * property-attribute rule laid over it, and these rules have to match the two
 * Rust layers exactly — the same syntax is parsed twice, once to draw it and
 * once to put it in the graph, and two readings of one syntax is how a note
 * comes to show a relationship that no query can find.
 *
 * Which of the two Rust layers: `links.rs` RECORDS what was typed and
 * `index.rs` DECIDES what is an edge, and this file mirrors the decisions —
 * a chip is a promise that the thing carries meaning, and the links panel reads
 * from the same projection. Where they differ is called out below.
 *
 * This file pins the READER. Which blocks the renderer then decorates is pinned
 * against a real `EditorView` in `live-preview-marks.test.ts`, because that is a
 * fact about what a reader sees and not about what a function returns.
 */
import { describe, expect, it } from "vitest";
import { __predicatesAfterForTest as predicatesAfter } from "./live-preview";

/** The predicate names each block draws, block by block, which is the list the
 *  graph gets and the order the panel shows. */
function chips(text: string): string[][] {
  return (predicatesAfter(text)?.blocks ?? []).map((block) => block.chips.map((chip) => chip.name));
}

/** Names paired with their literal objects, for the `{:key="value"}` form. */
function statements(text: string): [string, string | null][][] {
  return (predicatesAfter(text)?.blocks ?? []).map((block) =>
    block.chips.map((chip) => [chip.name, chip.object]),
  );
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

  /** Exact duplicates are dropped, across the whole run and not just within a
   *  block: the chip list IS the list the graph gets, and the graph holds one. */
  it("drops an exact duplicate, in either block", () => {
    expect(chips("{schema:creator, schema:creator}")).toEqual([["schema:creator"]]);
    expect(chips("{schema:creator}{schema:creator}")).toEqual([["schema:creator"], []]);
  });

  it("keeps two predicates that differ only in their prefix", () => {
    expect(chips("{schema:creator, foaf:creator}")).toEqual([["schema:creator", "foaf:creator"]]);
  });

  /**
   * The empty prefix is the document's DEFAULT VOCABULARY, and the colon is
   * stripped. Three spellings, one predicate: what makes that load-bearing is
   * that a vault mixes them freely, and a reader that kept the colon would put
   * `:depends_on` and `depends_on` in the graph as two unrelated edges.
   *
   * What the empty prefix resolves TO is deliberately not here: it comes from
   * the note's own `prefixes:` under the empty key, else from that drive's
   * `.okf/registry/predicates.md`, and only where RDF is emitted. keeper
   * displays the name.
   */
  describe("the default vocabulary", () => {
    it("strips the empty prefix's colon", () => {
      expect(chips("{:depends_on}")).toEqual([["depends_on"]]);
    });

    /** Semantic Markdown V0: an attribute with no `.`, no `#` and no `=` is a
     *  property name. This is the case the first parser missed entirely. */
    it("reads a bare word as a property name", () => {
      expect(chips("{depends_on}")).toEqual([["depends_on"]]);
    });

    it("reads the two spellings as one predicate", () => {
      expect(chips("{:depends_on, depends_on}")).toEqual([["depends_on"]]);
    });

    it("keeps a prefixed predicate distinct from a default-vocabulary one", () => {
      expect(chips("{creator, schema:creator}")).toEqual([["creator", "schema:creator"]]);
    });
  });

  /**
   * `{:type="Metric"}` — a colon-marked key with a literal object, which is the
   * owner's most common annotation. The colon is what announces the key as a
   * predicate: recognising it needs no vocabulary knowledge at all, which is why
   * `links.rs` records these as predicates rather than leaving them to the
   * projection.
   */
  describe("a colon-marked key with a literal object", () => {
    it("reads the key as the predicate and the value as its object", () => {
      expect(statements('{:type="Metric"}')).toEqual([[["type", "Metric"]]]);
    });

    it("keeps a prefix on the key", () => {
      expect(statements('{dc:title="Revenue"}')).toEqual([[["dc:title", "Revenue"]]]);
    });

    it("reads the owner's whole fence annotation", () => {
      expect(statements('{ :type="Metric" :owned_by="https://company.internal" }')).toEqual([
        [
          ["type", "Metric"],
          ["owned_by", "https://company.internal"],
        ],
      ]);
    });

    /** A quoted value is one token whatever is inside it, or `rel="see also"`
     *  would have become two attributes the day a predicate joined it. */
    it("keeps a value holding a comma and a space in one token", () => {
      expect(statements('{:note="one, two three"}')).toEqual([[["note", "one, two three"]]]);
    });

    it("accepts single quotes, as the Rust tokeniser does", () => {
      expect(statements("{:type='Metric'}")).toEqual([[["type", "Metric"]]]);
    });

    /** A bare predicate has no object, and that is a different thing from an
     *  object nobody can see: the renderer draws the two differently. */
    it("gives a bare predicate no object", () => {
      expect(statements("{:depends_on}")).toEqual([[["depends_on", null]]]);
    });

    /**
     * Two values for one predicate is two answers to one question, and only one
     * of them can reach the graph. Neither is guessed at — the block keeps its
     * source so the author can see both and pick.
     */
    it("refuses one predicate handed two different objects", () => {
      const block = predicatesAfter('{:type="Metric", :type="Dimension"}')?.blocks[0];

      expect(block?.chips.map((chip) => chip.name)).toEqual(["type"]);
      expect(block?.junk).toBe(true);
    });

    it("drops a repeat of the identical statement without complaint", () => {
      const block = predicatesAfter('{:type="Metric", :type="Metric"}')?.blocks[0];

      expect(statements('{:type="Metric", :type="Metric"}')).toEqual([[["type", "Metric"]]]);
      expect(block?.junk).toBe(false);
    });

    /** An empty value is not a pair on the Rust side either (`read_pair`
     *  refuses it), so it can only be junk here. Rendering `{:type=""}` as a
     *  bare `type` chip would hide the very thing the author has to fix. */
    it("refuses an empty value", () => {
      const block = predicatesAfter('{:type=""}')?.blocks[0];

      expect(block?.writesPredicate).toBe(false);
      expect(block?.junk).toBe(true);
    });
  });

  describe("the spelling keeper shipped first", () => {
    /** A vault written before this story renders exactly as it did: the legacy
     *  `reference` value folds into the same list. */
    it("reads the reference out of an attribute block", () => {
      expect(chips('{reference="supports"}')).toEqual([["supports"]]);
    });

    it("finds the reference beside another key", () => {
      expect(chips('{strength="weak" reference="cites"}')).toEqual([["cites"]]);
    });

    /**
     * `rel` is an edge too, and this is the assertion that changed with the
     * projection. `IndexProjection` folds bare-key `rel`/`reference` into
     * predicate names, so the graph holds `cites` for `{rel="cites"}` and the
     * links panel shows it. The editor was written when it was the only reader
     * of this syntax and there was no projection to agree with; a chip that
     * stayed away now would have the two halves of keeper disagreeing about
     * which tokens are edges, on one screen, about one link.
     */
    it("reads rel as the edge the index makes of it", () => {
      expect(chips('{rel="cites"}')).toEqual([["cites"]]);
      expect(chips('{schema:creator, rel="cites"}')).toEqual([["schema:creator", "cites"]]);
    });

    /**
     * A legacy name is APPENDED after every modern token of the run, which is
     * not written order and is deliberate: `link_predicate_map` produces that
     * order because `RawLink` keeps tokens and pairs in two vectors and the
     * legacy pair is a compatibility shim. The chips follow it because a chip
     * and a panel row about one link must not disagree about order — the user
     * reads both, often at once. True interleaving is reachable only by giving
     * `links.rs` one ordered list holding tokens and pairs together, and then
     * all three surfaces move at once.
     */
    it("appends the legacy name after the modern tokens, however it was written", () => {
      expect(chips('{reference="supports", schema:creator}')).toEqual([
        ["schema:creator", "supports"],
      ]);
    });

    /** Across the run, for the same reason the projection folds across it:
     *  `{...}{...}` after one link is one link carrying two blocks. The chips
     *  land on the last block that is drawn, which is where the run's chips
     *  end on screen. */
    it("appends across adjacent blocks, not per block", () => {
      expect(chips('{rel="cites"}{schema:creator}')).toEqual([[], ["schema:creator", "cites"]]);
    });

    /** "Last drawn", not "last": a block that draws nothing keeps its source, so
     *  putting the deferred chip there would lose it off the end of the run. */
    it("puts the deferred name on the last block that is drawn at all", () => {
      expect(chips('{rel="cites"}{.highlight}')).toEqual([["cites"], []]);
    });

    /** First-wins dedupe meets the modern tokens first whichever way round the
     *  block was typed, so a `rel=` naming a predicate a token already wrote is
     *  the one that drops. Written the other way it would be order-dependent,
     *  and a note would change meaning when somebody reordered a block. */
    it("lets a modern token win over a legacy pair naming the same predicate", () => {
      expect(chips('{depends_on, rel="depends_on"}')).toEqual([["depends_on"]]);
      expect(chips('{rel="depends_on", depends_on}')).toEqual([["depends_on"]]);
    });

    /**
     * The same precedence ACROSS a run, which is where it becomes visible: the
     * chip is drawn by whichever block holds the read that won, so getting this
     * backwards moves the chip to the other end of the run and sends the wrong
     * block back to source.
     */
    it("deduplicates the modern token first even when it was written second", () => {
      expect(chips('{rel="type"}{:type="Metric"}')).toEqual([[], ["type"]]);
    });

    /**
     * A legacy pair carries no object, so a name a `{:name="value"}` already
     * gave one to is a second answer to one question. The block goes back to
     * source: a chip reading `type=Metric` would show what keeper understood
     * and swallow the `rel=` the author has to decide about.
     */
    it("sends the block to source when a legacy pair renames a predicate that has an object", () => {
      const block = predicatesAfter('{:type="Metric", rel="type"}')?.blocks[0];

      expect(block?.chips.map((chip) => chip.name)).toEqual(["type"]);
      expect(block?.junk).toBe(true);
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

    /**
     * A value that is not a predicate name stays the plain attribute it has
     * always been — presentational, not junk — because the projection applies
     * the same name rule and indexes nothing for it. Two-word `rel` values are
     * what vaults are full of, and a note full of them must not turn into a
     * note full of revealed source.
     */
    it("leaves a rel value that is not a predicate name as an attribute", () => {
      const block = predicatesAfter('{rel="see also"}')?.blocks[0];

      expect(block?.chips).toEqual([]);
      expect(block?.writesPredicate).toBe(false);
      expect(block?.junk).toBe(false);
    });
  });

  describe("what is not a predicate", () => {
    /**
     * kramdown spells a class `.name` and an id `#name`, and marks a
     * block-level IAL with a lone leading `:` — `{: .highlight}`. None of the
     * three is a property name, and Semantic Markdown V0 says so in as many
     * words. Reading `.metric` as a predicate would put a CSS class in the
     * graph, which is an edge nobody wrote.
     */
    it.each([
      "{.highlight}",
      "{#revenue}",
      "{: .highlight}",
      "{:}",
    ])("draws nothing at all for %s", (text) => {
      const block = predicatesAfter(text)?.blocks[0];

      expect(block?.chips).toEqual([]);
      expect(block?.writesPredicate).toBe(false);
      expect(block?.junk).toBe(false);
    });

    /** A presentational token beside a predicate loses nothing: the class stays
     *  in the source the chip replaces, exactly as `strength="weak"` does. */
    it("reads the predicate out of a block that also carries a class", () => {
      expect(chips("{.highlight :depends_on #x}")).toEqual([["depends_on"]]);
    });

    /** Presentation, not a predicate: nothing in the syntax says `width` is an
     *  edge, and the projection is the only thing entitled to decide that. */
    it("writes no predicate for a block of presentational pairs alone", () => {
      const block = predicatesAfter('{class="wide", width="40"}')?.blocks[0];

      expect(block?.chips).toEqual([]);
      expect(block?.writesPredicate).toBe(false);
      expect(block?.junk).toBe(false);
    });

    /** Junk is now something narrower than prose, because a bare word IS a
     *  property name. It is a token with no reading at all — and the block is
     *  marked so the renderer can leave the author's own text where they typed
     *  it. */
    it("marks a block holding an unreadable token as junk", () => {
      const block = predicatesAfter("{oops!}")?.blocks[0];

      expect(block?.chips).toEqual([]);
      expect(block?.junk).toBe(true);
    });

    it("marks junk beside a good CURIE, without losing the CURIE", () => {
      const block = predicatesAfter("{schema:creator oops!}")?.blocks[0];

      expect(block?.chips.map((chip) => chip.name)).toEqual(["schema:creator"]);
      expect(block?.junk).toBe(true);
    });

    it.each([
      "{schema:}",
      "{9schema:creator}",
      "{schema::creator}",
      "{schema:creator:extra}",
      "{schema.org:creator}",
      "{:9creator}",
      "{9creator}",
    ])("refuses %s, which is not the predicate shape", (text) => {
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
