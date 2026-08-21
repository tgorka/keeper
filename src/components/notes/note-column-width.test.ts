import { describe, expect, it } from "vitest";
import { NOTE_COLUMN_CLASS } from "./note-editor";

/**
 * A drift guard on a defensive class, and it says which.
 *
 * `min-w-0` was added believing it was what stopped the note escaping its pane.
 * It was not: the column's parent is a block box, so `min-width: auto` never
 * applied here and zeroing it changed nothing — measured against the real chain
 * at 604px with the class and 604px without it. What fits the note to its pane
 * is the `ResizeObserver` in `note-editor.tsx`, covered by
 * `editor/pane-resize.test.tsx`.
 *
 * The class stays because the day this column becomes a flex item the floor is
 * already written, and this test stops it being deleted in a tidy-up before
 * then. It is not evidence that anything works.
 */
describe("the open note's column keeps its width floor", () => {
  it("carries min-w-0 so a nowrap child cannot widen it", () => {
    expect(NOTE_COLUMN_CLASS.split(" ")).toContain("min-w-0");
  });
});
