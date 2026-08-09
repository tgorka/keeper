import { act, render, screen } from "@testing-library/react";
import { useCallback, useRef } from "react";
import { afterEach, describe, expect, it } from "vitest";
import {
  useWindowedRows,
  WINDOW_ROW_ATTR,
  WINDOW_VIEWPORT_ATTR,
} from "@/components/ui/window-list";
import { type ListGeometry, withListGeometry } from "@/test/layout";

/**
 * Story 44.10 — the window itself.
 *
 * Every assertion here is about which rows are MOUNTED, which is the only fact
 * AD-84 is about. jsdom lays nothing out, so each test installs
 * `withListGeometry` first: without it the viewport is zero pixels tall, the
 * scroll offset can never leave zero, and a list that renders all ten thousand
 * rows in a browser would satisfy every assertion below.
 */

/** A viewport ten rows tall, in a world where a row is twenty pixels. */
const VIEWPORT_PX = 200;
const ROW_PX = 20;
const OVERSCAN = 2;

/** Ten rows fit, and there is nothing above row 0 to overscan into. Written out
 * rather than computed, because a count the test derives from the same
 * arithmetic the component uses would agree with a broken component. */
const WINDOWED_ROWS = 10 + OVERSCAN;

let geometry: ListGeometry | null = null;

afterEach(() => {
  geometry?.undo();
  geometry = null;
});

function Harness({
  count,
  pinnedIndex,
  rowHeight = ROW_PX,
  revealTo,
}: {
  count: number;
  pinnedIndex?: number;
  rowHeight?: number;
  /** Rendered as a button, so a test can ask for a row from outside. */
  revealTo?: number;
}) {
  const rows = useRef(new Map<number, HTMLButtonElement>());
  const getKey = useCallback((index: number) => `note-${index}`, []);
  const list = useWindowedRows({
    count,
    getKey,
    rowHeight,
    overscan: OVERSCAN,
    pinnedIndex,
    onReveal: (index) => rows.current.get(index)?.focus(),
  });

  return (
    <div>
      {revealTo !== undefined && (
        <button type="button" onClick={() => list.reveal(revealTo)}>
          Go
        </button>
      )}
      <div {...list.viewportProps} data-testid="viewport" style={{ overflowY: "auto" }}>
        <div style={{ position: "relative", height: list.totalSize }} data-testid="spacer">
          {list.rows.map((row) => (
            <div key={row.key} {...list.rowProps(row)}>
              <button
                type="button"
                ref={(element) => {
                  if (element === null) {
                    return;
                  }
                  rows.current.set(row.index, element);
                  return () => {
                    rows.current.delete(row.index);
                  };
                }}
                tabIndex={pinnedIndex === row.index ? 0 : -1}
              >
                Row {row.index}
              </button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function mounted(): number[] {
  return [...document.querySelectorAll(`[${WINDOW_ROW_ATTR}]`)].map((element) =>
    Number(element.getAttribute(WINDOW_ROW_ATTR)),
  );
}

function viewport(): HTMLElement {
  return screen.getByTestId("viewport");
}

describe("useWindowedRows", () => {
  it("mounts a window, not the vault", () => {
    geometry = withListGeometry({ viewport: VIEWPORT_PX, row: ROW_PX });
    render(<Harness count={5000} />);

    expect(mounted()).toHaveLength(WINDOWED_ROWS);
    expect(screen.getByText("Row 0")).toBeInTheDocument();
    expect(screen.queryByText("Row 4999")).not.toBeInTheDocument();
    // The spacer still claims the whole list's height, which is what keeps the
    // scrollbar honest about how much there is.
    expect(screen.getByTestId("spacer")).toHaveStyle({ height: `${5000 * ROW_PX}px` });
  });

  it("marks its viewport so the scroll container is findable", () => {
    geometry = withListGeometry({ viewport: VIEWPORT_PX, row: ROW_PX });
    render(<Harness count={10} />);

    expect(viewport()).toHaveAttribute(WINDOW_VIEWPORT_ATTR);
  });

  it("reaches the last row by scrolling, and still mounts only a window", () => {
    geometry = withListGeometry({ viewport: VIEWPORT_PX, row: ROW_PX });
    render(<Harness count={5000} />);

    act(() => geometry?.scrollTo(viewport(), 5000 * ROW_PX - VIEWPORT_PX));

    expect(screen.getByText("Row 4999")).toBeInTheDocument();
    expect(screen.queryByText("Row 0")).not.toBeInTheDocument();
    expect(mounted().length).toBeLessThanOrEqual(WINDOWED_ROWS);
  });

  it("mounts a row that was never rendered before focusing it", () => {
    geometry = withListGeometry({ viewport: VIEWPORT_PX, row: ROW_PX });
    render(<Harness count={5000} revealTo={3000} />);

    expect(screen.queryByText("Row 3000")).not.toBeInTheDocument();

    act(() => {
      screen.getByRole("button", { name: "Go" }).click();
    });

    // The row exists AND has focus. Mounting it a frame later would satisfy the
    // first half of that sentence and drop focus on the floor.
    const target = screen.getByText("Row 3000");
    expect(target).toBeInTheDocument();
    expect(document.activeElement).toBe(target);
  });

  it("keeps the pinned row mounted however far away it is scrolled", () => {
    geometry = withListGeometry({ viewport: VIEWPORT_PX, row: ROW_PX });
    render(<Harness count={5000} pinnedIndex={0} />);

    act(() => geometry?.scrollTo(viewport(), 5000 * ROW_PX - VIEWPORT_PX));

    // Row 0 is nowhere near the viewport, and it is still the one tab stop.
    // Unmounting it would leave the list with no row in the tab order at all.
    const stops = [...document.querySelectorAll('button[tabindex="0"]')];
    expect(stops).toHaveLength(1);
    expect(stops[0]).toHaveTextContent("Row 0");
    expect(mounted()).toContain(0);
    expect(mounted()).toContain(4999);
  });

  it("returns to exactly the window it started with, and moves nothing else", () => {
    geometry = withListGeometry({ viewport: VIEWPORT_PX, row: ROW_PX });
    render(<Harness count={5000} />);
    const first = mounted();

    act(() => geometry?.scrollTo(viewport(), 4000));
    act(() => geometry?.scrollTo(viewport(), 0));

    expect(mounted()).toEqual(first);
    expect(viewport().scrollTop).toBe(0);
  });

  it("lets a measured row correct the estimate, and still reaches the last row", () => {
    // Every row measures 40 while the list assumed 20 — the case a fixed-height
    // window gets wrong the moment a row wraps to a second line.
    geometry = withListGeometry({ viewport: VIEWPORT_PX, row: 40 });
    render(<Harness count={100} rowHeight={20} />);

    // Half as many rows fit as the estimate implied, and the window shrank to
    // match rather than staying at the estimate's ten.
    expect(mounted()).toHaveLength(5 + OVERSCAN);
    // The total is taller than the estimate claimed, and it is still short of
    // the truth: only rows that have been mounted have been measured. That is
    // the honest cost of measuring, and the loop below is what it costs — a
    // list whose total grows as it is scrolled still reaches its last row.
    const height = () => Number.parseFloat(screen.getByTestId("spacer").style.height);
    expect(height()).toBeGreaterThan(100 * 20);
    expect(height()).toBeLessThan(100 * 40);

    for (let attempt = 0; attempt < 8 && screen.queryByText("Row 99") === null; attempt += 1) {
      act(() => geometry?.scrollTo(viewport(), height() - VIEWPORT_PX));
    }

    // Reached — and the total is STILL an estimate for the rows that were
    // skipped over on the way down. That is not a bug to fix by measuring
    // everything; measuring everything is the thing this hook exists not to do.
    expect(screen.getByText("Row 99")).toBeInTheDocument();
    expect(height()).toBeLessThan(100 * 40);
  });

  it("does nothing when asked to reveal an index the list does not have", () => {
    geometry = withListGeometry({ viewport: VIEWPORT_PX, row: ROW_PX });
    render(<Harness count={5} revealTo={99} />);
    const go = screen.getByRole("button", { name: "Go" });
    go.focus();

    act(() => go.click());

    // No row 99 conjured into the window, and focus left where it was rather
    // than dropped on the floor.
    expect(mounted()).toEqual([0, 1, 2, 3, 4]);
    expect(document.activeElement).toBe(go);
  });
});
