import { describe, expect, it } from "vitest";
import {
  COLUMN_WIDTH_COOKIE,
  clampColumnWidth,
  columnMinWidth,
  columnTemplate,
  columnWidthCookie,
  MAX_COLUMN_WIDTH,
  MIN_COLUMN_WIDTH,
  readColumnWidths,
  SURFACE_COLUMN_IDS,
  SURFACE_COLUMNS,
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

/**
 * A surface column's floor is its own (Story 48.1).
 *
 * {@link MIN_COLUMN_WIDTH} was chosen for a property KEY, where 72px still
 * shows an ellipsis and the overflow trigger that reads the rest. A whole
 * column has no overflow trigger, so each one states its own floor and the
 * codec has to hold it everywhere a width can enter — a drag, a write, and a
 * jar written by a build that had a different one.
 */
describe("surface column floors", () => {
  it("holds each column to its own floor and leaves other ids on the shared one", () => {
    expect(columnMinWidth("notes-rail")).toBe(SURFACE_COLUMNS["notes-rail"].minWidth);
    expect(columnMinWidth("chat-list")).toBe(SURFACE_COLUMNS["chat-list"].minWidth);
    // The Properties key column, which is the only other resizable column.
    expect(columnMinWidth("properties-key")).toBe(MIN_COLUMN_WIDTH);
  });

  it("gives every surface column a floor above the shared one and below its default", () => {
    // Not decoration: a floor at or below 72 would mean nobody decided, and a
    // floor above the default would mean the column starts out of range.
    for (const id of SURFACE_COLUMN_IDS) {
      const spec = SURFACE_COLUMNS[id];
      expect(spec.minWidth).toBeGreaterThan(MIN_COLUMN_WIDTH);
      expect(spec.minWidth).toBeLessThan(spec.defaultWidth);
      expect(spec.defaultWidth).toBeLessThanOrEqual(MAX_COLUMN_WIDTH);
    }
  });

  it("clamps a drag to the column's floor, not to the shared one", () => {
    expect(clampColumnWidth(80, columnMinWidth("notes-rail"))).toBe(
      SURFACE_COLUMNS["notes-rail"].minWidth,
    );
    expect(clampColumnWidth(Number.NaN, columnMinWidth("notes-rail"))).toBe(
      SURFACE_COLUMNS["notes-rail"].minWidth,
    );
  });

  it("lifts a width recorded below today's floor on the way back in", () => {
    // The read path and the write path both, because a floor enforced on write
    // alone leaks the moment a build lowers one — or a person edits the jar.
    const jar = `${COLUMN_WIDTH_COOKIE}=notes-rail:80|properties-key:80`;

    expect(readColumnWidths(jar)).toEqual({
      "notes-rail": SURFACE_COLUMNS["notes-rail"].minWidth,
      "properties-key": 80,
    });
    expect(readColumnWidths(columnWidthCookie(jar, "chat-list", 100))["chat-list"]).toBe(
      SURFACE_COLUMNS["chat-list"].minWidth,
    );
  });
});
