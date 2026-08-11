import { describe, expect, it } from "vitest";
import {
  COLUMN_WIDTH_COOKIE,
  clampColumnWidth,
  columnTemplate,
  columnWidthCookie,
  MAX_COLUMN_WIDTH,
  MIN_COLUMN_WIDTH,
  readColumnWidths,
} from "@/lib/column-widths";

/**
 * The codec, not the feature.
 *
 * A drag that persists is proved in `properties-panel.test.tsx`, by dragging the
 * panel's real seam and reading the real cookie back after a real remount. What
 * is here is the part that has to survive a jar it does not own: another
 * cookie beside it, a value written by an older build, a width someone edited
 * by hand.
 */
describe("column width persistence", () => {
  it("keeps one surface's width when another's is written", () => {
    const first = columnWidthCookie("", "properties-key", 180);
    // The cookie header a browser hands back carries only `name=value` pairs.
    const jar = `${first.split(";")[0]}; theme=dark`;

    const second = columnWidthCookie(jar, "files-name", 240);

    expect(readColumnWidths(second.split(";")[0])).toEqual({
      "properties-key": 180,
      "files-name": 240,
    });
  });

  it("forgets a width rather than recording a null, so reset means fitted again", () => {
    const jar = columnWidthCookie("", "properties-key", 180).split(";")[0];

    const reset = columnWidthCookie(jar, "properties-key", null);

    expect(readColumnWidths(reset)).toEqual({});
    expect(columnTemplate(readColumnWidths(reset)["properties-key"] ?? null)).toContain(
      "fit-content(50%)",
    );
  });

  it("reads nothing out of a jar that has no widths in it", () => {
    expect(readColumnWidths("")).toEqual({});
    expect(readColumnWidths("sidebar_state=true; theme=dark")).toEqual({});
  });

  it("drops a malformed entry instead of refusing to render the pane", () => {
    // A jar from an older build, a hand-edit, another origin's junk. The pane
    // that throws here is a pane nobody can open again.
    const jar = `${COLUMN_WIDTH_COOKIE}=properties-key:180|garbage|:12|Bad Id:9|files-name:notanumber`;

    expect(readColumnWidths(jar)).toEqual({ "properties-key": 180 });
  });

  it("clamps a stored width that is out of range on the way back in", () => {
    const jar = `${COLUMN_WIDTH_COOKIE}=properties-key:9999|files-name:1`;

    expect(readColumnWidths(jar)).toEqual({
      "properties-key": MAX_COLUMN_WIDTH,
      "files-name": MIN_COLUMN_WIDTH,
    });
  });

  it("never lets a column reach zero, because a vanished column cannot be dragged back", () => {
    expect(clampColumnWidth(-400)).toBe(MIN_COLUMN_WIDTH);
    expect(clampColumnWidth(0)).toBe(MIN_COLUMN_WIDTH);
    expect(clampColumnWidth(Number.NaN)).toBe(MIN_COLUMN_WIDTH);
    expect(clampColumnWidth(MAX_COLUMN_WIDTH + 1)).toBe(MAX_COLUMN_WIDTH);
    expect(clampColumnWidth(180.6)).toBe(181);
  });

  it("asks the layout engine to fit until a width is chosen", () => {
    // AD-83's first step is the browser's job: an unsized column is as wide as
    // its own glyphs, not as wide as somebody's `w-32`.
    expect(columnTemplate(null)).toBe(
      `minmax(${MIN_COLUMN_WIDTH}px, fit-content(50%)) 0px minmax(0, 1fr)`,
    );
    expect(columnTemplate(180)).toBe("180px 0px minmax(0, 1fr)");
  });
});
