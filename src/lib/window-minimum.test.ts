import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { FOLD_STRIP } from "@/components/layout/fold-strip";
import { PANEL_MIN_WIDTH_CLASS } from "@/components/layout/panel-strip";
import { SIDEBAR_WIDTH_CLASS } from "@/components/layout/sidebar-pane";
import { TASKS_DETAIL_MIN_WIDTH_PX, TASKS_PANE_MIN_WIDTH_PX } from "@/components/layout/tasks-pane";
import { SIDEBAR_COLLAPSE_BREAKPOINT } from "@/hooks/use-shell-layout";
import { columnMinWidth } from "@/lib/column-widths";

/**
 * The main window may not be resizable to a width a surface cannot fit.
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

const windowMinWidth = (): number => {
  const config = JSON.parse(
    readFileSync(resolve(__dirname, "../../src-tauri/crates/keeper/tauri.conf.json"), "utf8"),
  );
  const main = config.app.windows.find(
    (w: { label?: string; minWidth?: number }) =>
      typeof w.minWidth === "number" && w.minWidth > 500,
  );
  return main.minWidth;
};

describe("the window cannot be narrower than the layout's floor", () => {
  it("keeps tauri's minWidth at or above sidebar + rail + list + one panel", () => {
    const floor =
      px(SIDEBAR_WIDTH_CLASS.expanded) +
      columnMinWidth("notes-rail") +
      columnMinWidth("notes-list") +
      px(PANEL_MIN_WIDTH_CLASS);

    expect(windowMinWidth()).toBeGreaterThanOrEqual(floor);
  });

  /**
   * The Tasks surface, which is the widest arrangement in the app: a folding
   * list column, the pane's OWN detail region beside it (Story 59.1), and the
   * panel strip beside both (Story 59.12). Four boxes, where Notes has three.
   *
   * Story 59.13 exists because nothing connected those four numbers. The detail
   * region had no floor at all, so the arithmetic could not fail — it was the
   * region that absorbed every shortfall, measured at 28px in a 1024px window
   * with the add form inside it at 0px.
   *
   * Two assertions rather than one, because which sidebar is on screen is
   * decided by the viewport and not by the user: below
   * `SIDEBAR_COLLAPSE_BREAKPOINT` the drawer is the 48px rail, at or above it
   * the 156px drawer. The window minimum is inside the first band and the
   * breakpoint is the bottom of the second, so between them these two cover
   * every width the window can be.
   */
  it("fits the Tasks surface's four boxes at the narrowest window, rail sidebar", () => {
    const floor = FOLD_STRIP.widthPx + TASKS_PANE_MIN_WIDTH_PX + px(PANEL_MIN_WIDTH_CLASS);

    expect(windowMinWidth()).toBeGreaterThanOrEqual(floor);
    expect(windowMinWidth()).toBeLessThan(SIDEBAR_COLLAPSE_BREAKPOINT);
  });

  it("fits the Tasks surface at the collapse breakpoint, where the drawer is back", () => {
    const floor =
      px(SIDEBAR_WIDTH_CLASS.expanded) + TASKS_PANE_MIN_WIDTH_PX + px(PANEL_MIN_WIDTH_CLASS);

    expect(SIDEBAR_COLLAPSE_BREAKPOINT).toBeGreaterThanOrEqual(floor);
  });

  it("builds the pane's floor from its two columns, so the sum cannot drift", () => {
    // The one number this file adds to three others, checked against the two it
    // is made of: a hand-written 600 here would go stale the first time either
    // the list's floor or the detail's moved.
    expect(TASKS_PANE_MIN_WIDTH_PX).toBe(columnMinWidth("tasks-list") + TASKS_DETAIL_MIN_WIDTH_PX);
  });
});
