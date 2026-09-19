import { describe, expect, it } from "vitest";
import { markRuns } from "./mark-runs";

describe("markRuns", () => {
  it("slices UTF-16 rather than UTF-8 across ł and an emoji", () => {
    expect(markRuns("ał😀tax end", [[1, 7]])).toEqual([
      { text: "a", marked: false },
      { text: "ł😀tax", marked: true },
      { text: " end", marked: false },
    ]);
  });
  it("sorts and merges overlaps without repeating text", () => {
    expect(
      markRuns("abcdef", [
        [3, 5],
        [1, 4],
        [2, 3],
      ]),
    ).toEqual([
      { text: "a", marked: false },
      { text: "bcde", marked: true },
      { text: "f", marked: false },
    ]);
  });
  it("clips ranges and ignores empty or reversed ranges", () => {
    expect(
      markRuns("abc", [
        [-4, 1],
        [2, 20],
        [2, 2],
        [3, 1],
      ]),
    ).toEqual([
      { text: "a", marked: true },
      { text: "b", marked: false },
      { text: "c", marked: true },
    ]);
  });
  it("leaves unmarked text plain", () => {
    expect(markRuns("ł😀", [])).toEqual([{ text: "ł😀", marked: false }]);
    expect(markRuns("", [])).toEqual([]);
  });
});
