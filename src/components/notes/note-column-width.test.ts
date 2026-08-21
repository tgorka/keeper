import { describe, expect, it } from "vitest";
import { NOTE_COLUMN_CLASS } from "./note-editor";

/**
 * A drift guard, not a layout test.
 *
 * jsdom does not lay flexbox out, so nothing in this suite can catch the open
 * note escaping its column — that was measured in a real browser against the
 * built stylesheet, and the number is in the constant's own doc comment. What
 * this test can do is notice the class going away, which is how the bug got in:
 * `min-h-0` was there from the start and `min-w-0` never was, and no failure
 * anywhere said so.
 *
 * If you are here because this test failed, the column has lost its floor and
 * one nowrap backlink row is again free to decide how wide the note is.
 */
describe("the open note's column keeps its width floor", () => {
  it("carries min-w-0 so a nowrap child cannot widen it", () => {
    expect(NOTE_COLUMN_CLASS.split(" ")).toContain("min-w-0");
  });
});
