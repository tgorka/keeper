import { describe, expect, it } from "vitest";
import { spliceBetween } from "./live-preview";

describe("spliceBetween", () => {
  it("reports nothing when the texts are identical", () => {
    expect(spliceBetween("same\n", "same\n")).toBeNull();
  });

  it("names only the appended tail when an agent adds a section at the end", () => {
    const before = "# Notes\n\nfirst paragraph\n";
    const after = `${before}\n## Added by the agent\n`;

    const splice = spliceBetween(before, after);

    // The caret sits in the first paragraph; nothing before `before.length`
    // may move, or applying this change would drag the caret with it.
    expect(splice).toEqual({
      from: before.length,
      to: before.length,
      insert: "\n## Added by the agent\n",
    });
  });

  it("names only the changed middle, keeping the shared head and tail", () => {
    const splice = spliceBetween("alpha beta gamma", "alpha DELTA gamma");

    expect(splice).not.toBeNull();
    expect("alpha beta gamma".slice(0, splice?.from)).toBe("alpha ");
    expect("alpha beta gamma".slice(splice?.to)).toBe(" gamma");
    expect(splice?.insert).toBe("DELTA");
  });

  it("expresses a pure deletion as an empty insert", () => {
    const splice = spliceBetween("keep\ndrop\nkeep\n", "keep\nkeep\n");

    expect(splice?.insert).toBe("");
    expect("keep\ndrop\nkeep\n".slice(splice?.from, splice?.to)).toBe("drop\n");
  });
});
