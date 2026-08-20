import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { PANEL_MIN_WIDTH_CLASS } from "@/components/layout/panel-strip";
import { SIDEBAR_WIDTH_CLASS } from "@/components/layout/sidebar-pane";
import { columnMinWidth } from "@/lib/column-widths";

/**
 * The main window may not be resizable to a width the Notes surface cannot fit.
 *
 * These are two numbers in two languages — a Tauri config and a pile of CSS —
 * and nothing but this test connects them. `minWidth` was 940 from the first
 * shell story, chosen before any of these floors existed; the floors have moved
 * several times since and the window minimum never followed, which is how a
 * 20px band at the bottom of the range ended up clipping the open note.
 *
 * If you lower a floor, this test goes green on its own. If you raise one, it
 * fails and tells you the window minimum has to move with it.
 */
const px = (cls: string): number => {
  const match = /\[(\d+)px\]/.exec(cls);
  if (!match) throw new Error(`no pixel width in ${cls}`);
  return Number(match[1]);
};

describe("the window cannot be narrower than the layout's floor", () => {
  it("keeps tauri's minWidth at or above sidebar + rail + list + one panel", () => {
    const floor =
      px(SIDEBAR_WIDTH_CLASS.expanded) +
      columnMinWidth("notes-rail") +
      columnMinWidth("notes-list") +
      px(PANEL_MIN_WIDTH_CLASS);

    const config = JSON.parse(
      readFileSync(resolve(__dirname, "../../src-tauri/crates/keeper/tauri.conf.json"), "utf8"),
    );
    const main = config.app.windows.find(
      (w: { label?: string; minWidth?: number }) =>
        typeof w.minWidth === "number" && w.minWidth > 500,
    );

    expect(main.minWidth).toBeGreaterThanOrEqual(floor);
  });
});
