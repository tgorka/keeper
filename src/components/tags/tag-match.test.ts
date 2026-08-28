import { describe, expect, it } from "vitest";
import { matchTags, namesTag, tagMatchOffset } from "@/components/tags/tag-match";

/**
 * The vocabulary as `keeper-core`'s tag tree flattens it: every ancestor prefix
 * is its own entry, ascending. Matching is written against this shape and not
 * against a list of leaves, because that is what both callers are handed.
 */
const VAULT = [
  "client",
  "client/acme",
  "client/acme/renewal",
  "client/anvil",
  "renewal/entry",
  "standup",
];

describe("matching a tag at its segment boundaries", () => {
  it("browses the whole vocabulary, in its own order, with nothing typed", () => {
    // The half of the control that is a list rather than a field. Re-ordering
    // here would shuffle a list somebody is reading.
    expect(matchTags("", VAULT)).toEqual(VAULT);
  });

  it("finds a tag by a segment that is not its first", () => {
    expect(matchTags("acme", VAULT)).toEqual(["client/acme", "client/acme/renewal"]);
  });

  it("refuses a substring that starts inside a segment", () => {
    // The concrete failure: CodeMirror's fuzzy filter matched `ent` against the
    // middle of `client`. A segment either begins with what was typed or it is
    // not a match — `renewal/entry` qualifies, `client` does not.
    expect(matchTags("ent", VAULT)).toEqual(["renewal/entry"]);
  });

  it("completes a hierarchy segment by segment, the earlier ones exactly", () => {
    // `cl/ac` is two segments: `cl` must be the WHOLE first segment of the
    // match and it is not, so the only thing that can align is a tag whose
    // first matched segment is literally `cl`. Nothing here is.
    expect(matchTags("cl/ac", VAULT)).toEqual([]);
    expect(matchTags("client/ac", VAULT)).toEqual(["client/acme", "client/acme/renewal"]);
    // The sibling is reached by its own segment, not by the one beside it.
    expect(matchTags("client/an", VAULT)).toEqual(["client/anvil"]);
  });

  it("treats a half-typed slash as the boundary it is", () => {
    // Someone reaching for a child has their finger on the slash. Dropping the
    // empty segment is what keeps the list from emptying mid-keystroke.
    expect(matchTags("client/", VAULT)).toEqual([
      "client",
      "client/acme",
      "client/anvil",
      "client/acme/renewal",
    ]);
  });

  it("ranks a root-anchored match above a deeper one, then the shallower tag", () => {
    // `renewal` is the last segment of `client/acme/renewal` and the first of
    // `renewal/entry`. The one it anchors is the closer answer.
    expect(matchTags("renewal", VAULT)).toEqual(["renewal/entry", "client/acme/renewal"]);
  });

  it("folds case to decide what to offer, and returns the vocabulary's spelling", () => {
    // Case is a search courtesy here; what a tag IS stays Rust's decision. The
    // string handed back is the one the vault holds, never the one typed.
    expect(matchTags("CLIENT/Ac", VAULT)).toContain("client/acme");
    expect(matchTags("CLIENT/Ac", VAULT)).not.toContain("CLIENT/Acme");
  });

  it("reports where the match started, which is what the ranking is made of", () => {
    expect(tagMatchOffset("client", "client/acme")).toBe(0);
    expect(tagMatchOffset("acme", "client/acme")).toBe(1);
    expect(tagMatchOffset("acme", "standup")).toBe(-1);
  });

  it("cannot match a query deeper than the tag", () => {
    expect(tagMatchOffset("client/acme/renewal", "client/acme")).toBe(-1);
  });
});

describe("whether what was typed already names a tag", () => {
  it("recognises the same path written differently", () => {
    expect(namesTag("/Client/Acme/", VAULT)).toBe(true);
    expect(namesTag("client/acme", VAULT)).toBe(true);
  });

  it("does not mistake a prefix of a tag for the tag", () => {
    // `client/acm` completes to `client/acme`; it does not NAME it, so a
    // create-allowed chooser must still offer to create it.
    expect(namesTag("client/acm", VAULT)).toBe(false);
  });

  it("names nothing when nothing was typed", () => {
    expect(namesTag("   ", VAULT)).toBe(false);
  });
});
