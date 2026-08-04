import { describe, expect, it } from "vitest";
import { alignBlocks, assemble, type ConflictBlock } from "./conflict-resolver";

const MINE = "shared opening\n\nmy paragraph\n\nshared closing";
const THEIRS = "shared opening\n\ntheir paragraph\n\nshared closing";

describe("alignBlocks", () => {
  it("keeps the paragraphs both sides agree on out of the conflict", () => {
    const blocks = alignBlocks(MINE, THEIRS);

    expect(blocks).toEqual<ConflictBlock[]>([
      { kind: "same", text: "shared opening" },
      { kind: "differs", mine: "my paragraph", theirs: "their paragraph" },
      { kind: "same", text: "shared closing" },
    ]);
  });

  it("does not push the whole tail into the diff when one side inserts", () => {
    const blocks = alignBlocks("a\n\nb\n\nc", "a\n\ninserted\n\nb\n\nc");

    expect(blocks).toEqual<ConflictBlock[]>([
      { kind: "same", text: "a" },
      { kind: "differs", mine: "", theirs: "inserted" },
      { kind: "same", text: "b" },
      { kind: "same", text: "c" },
    ]);
  });
});

describe("assemble", () => {
  it("keeps the chosen side of every differing block", () => {
    const blocks = alignBlocks(MINE, THEIRS);

    expect(assemble(blocks, ["theirs"])).toBe(`${THEIRS}\n`);
    expect(assemble(blocks, ["mine"])).toBe(`${MINE}\n`);
  });

  it("keeps both sides, in this machine's order, when asked", () => {
    const blocks = alignBlocks(MINE, THEIRS);

    expect(assemble(blocks, ["both"])).toContain("my paragraph\n\ntheir paragraph");
  });

  it("drops nothing silently: an unanswered block contributes nothing", () => {
    const blocks = alignBlocks(MINE, THEIRS);

    // Which is exactly why `Finish` stays disabled until every block is
    // answered — this output must never be reachable through the UI.
    expect(assemble(blocks, [null])).toBe("shared opening\n\nshared closing\n");
  });
});
