/**
 * A column boundary you can drag, whose width outlives the window (Story 44.12,
 * FR-167, FR-168, AD-83).
 *
 * Hand-rolled, and it is thirty lines of pointer arithmetic plus a CSS variable
 * — a dependency for this would be a dependency for `clientX - startX`.
 *
 * The order AD-83 asks for is the order this implements. **Fit** is delegated to
 * the layout engine through `columnTemplate`'s `fit-content(50%)`: an unsized
 * column is exactly as wide as its own glyphs in the real font, which is a
 * number no TypeScript measurement would get right and no fixed `w-32` was ever
 * going to guess. **Resize** replaces that with a px width the user chose.
 * **Truncate** is then whatever still does not fit, and it is `OverflowValue`'s
 * job, not this file's.
 *
 * The handle is a real `separator` in the tab order, not a hover-revealed strip.
 * A boundary only a mouse can move is a boundary half the users do not have, and
 * the same keys that move it (`←`/`→`, `Home` to go back to fitted) are the ones
 * every other slider in the app answers to.
 */
import { type CSSProperties, type KeyboardEvent, type PointerEvent, useRef, useState } from "react";
import {
  COLUMN_KEY_STEP,
  COLUMN_KEY_STEP_COARSE,
  clampColumnWidth,
  columnMinWidth,
  columnTemplate,
  columnWidthCookie,
  MAX_COLUMN_WIDTH,
  readColumnWidths,
} from "@/lib/column-widths";
import { cn } from "@/lib/utils";

/** What the surface needs to render a two-column grid with a draggable seam. */
export interface ResizableColumn {
  /** The remembered width in px, or null while the column fits its content. */
  width: number | null;
  /**
   * Goes on the grid container, alongside {@link COLUMN_GRID_CLASS}.
   *
   * The template travels as custom properties rather than as
   * `gridTemplateColumns` directly. Two reasons, and the second is the real
   * one: a custom property is one declaration to override from a stylesheet if
   * a surface ever needs to, and jsdom's CSS parser silently DROPS
   * `minmax(72px, fit-content(50%))` — so a test asserting the fitted template
   * would read an empty string and pass whatever the component did. A value
   * the test environment throws away is a value no test can defend.
   */
  gridStyle: (rowCount: number) => CSSProperties;
  /** Goes on the grid container so the seam can measure the left edge. */
  containerRef: (element: HTMLElement | null) => void;
  /** Goes on {@link ColumnResizer}. */
  resizerProps: ColumnResizerProps;
}

export interface ColumnResizerProps {
  /** What the boundary is between, e.g. "Property name". Becomes its name. */
  label: string;
  width: number | null;
  onWidth: (width: number | null) => void;
  /** The grid container's left edge in viewport px, for reading a fitted width. */
  containerLeft: () => number;
  /**
   * The narrowest this column may be, in px — what a screen reader hears as
   * `aria-valuemin`. Per column since Story 48.1: a whole surface column and a
   * property key do not share a floor, and a slider that announces a minimum
   * the drag will not honour is a slider that lies.
   */
  min: number;
  /**
   * Where the seam sits, for a host that is not the two-column grid.
   *
   * Defaults to the grid placement {@link columnTemplate}'s middle track was
   * cut for. A surface column (Story 48.1) is a flex row instead, where the
   * grid properties would be inert noise and the seam only needs to refuse to
   * shrink — the zero-width box and the 8px hit strip straddling it are the
   * same either way.
   */
  className?: string;
}

/**
 * The seam's accessible name, suffixed with what it sizes. Every resizer in the
 * app reads the same so a user who has met one has met all of them.
 */
export const COLUMN_RESIZER_LABEL = "Resize";

/** What a screen reader hears while the column is still sized by its content. */
export const COLUMN_FITTED_VALUE_TEXT = "Fitted to content";

/** The custom property carrying the column template. */
export const COLUMN_TEMPLATE_VAR = "--keeper-columns";

/** The custom property carrying the explicit row template. */
export const COLUMN_ROWS_VAR = "--keeper-column-rows";

/** Goes on the grid container beside {@link ResizableColumn.gridStyle}. */
export const COLUMN_GRID_CLASS =
  "grid [grid-template-columns:var(--keeper-columns)] [grid-template-rows:var(--keeper-column-rows)]";

/**
 * Read `id`'s remembered width and hand back everything a surface needs to
 * render and change it.
 *
 * The cookie is read in a lazy initialiser and never cached in module scope, so
 * a fresh mount reads exactly what a reload would. That is not an accident of
 * implementation — it is what lets a test prove reload-persistence by
 * unmounting and rendering again, which is the only reload jsdom has.
 */
export function useResizableColumn(id: string, label: string): ResizableColumn {
  const [width, setWidth] = useState<number | null>(
    () => readColumnWidths(document.cookie)[id] ?? null,
  );
  const container = useRef<HTMLElement | null>(null);

  const onWidth = (next: number | null): void => {
    // The column's own floor, which for a surface column (Story 48.1) is not
    // the shared 72px. Clamped here as well as in the cookie so the number on
    // screen during a drag is the number that will be remembered after it.
    setWidth(next === null ? null : clampColumnWidth(next, columnMinWidth(id)));
    document.cookie = columnWidthCookie(document.cookie, id, next);
  };

  return {
    width,
    gridStyle: (rowCount) =>
      ({
        [COLUMN_TEMPLATE_VAR]: columnTemplate(width),
        // Explicit rows, because the seam spans `1 / -1` and `-1` means the end
        // of the EXPLICIT grid. With implicit rows it would resolve to line 2
        // and the handle would be one row tall — draggable along the first
        // property and nowhere else.
        [COLUMN_ROWS_VAR]: `repeat(${rowCount}, auto)`,
      }) as CSSProperties,
    containerRef: (element) => {
      container.current = element;
    },
    resizerProps: {
      label,
      width,
      onWidth,
      min: columnMinWidth(id),
      // The grid's own left edge. Subtracted from the seam's to recover the
      // fitted width the layout engine produced — consulted only while the
      // column is still fitted, because once a width has been chosen that
      // number IS the truth and re-measuring would fold in rounding per grab.
      containerLeft: () => container.current?.getBoundingClientRect().left ?? 0,
    },
  };
}

/**
 * The draggable seam between two columns.
 *
 * Occupies a zero-width grid track and paints an eight-pixel hit strip
 * straddling it, so the target is grabbable without the boundary itself moving
 * the layout by eight pixels. Pointer capture is taken on grab: a fast drag
 * leaves the strip within a frame, and without capture the column stops
 * following the cursor exactly when the user is moving fastest.
 */
export function ColumnResizer({
  label,
  width,
  onWidth,
  containerLeft,
  min,
  className,
}: ColumnResizerProps) {
  const drag = useRef<{ pointerId: number; startX: number; startWidth: number } | null>(null);

  const onPointerDown = (event: PointerEvent<HTMLDivElement>): void => {
    if (event.button !== 0) {
      return;
    }
    // The grab origin is the width the user can see: the chosen one when there
    // is one, and the fitted one the layout engine produced when there is not.
    // Starting a drag from anything else makes the column jump on mouse-down.
    const startWidth = width ?? event.currentTarget.getBoundingClientRect().left - containerLeft();
    drag.current = { pointerId: event.pointerId, startX: event.clientX, startWidth };
    event.currentTarget.setPointerCapture(event.pointerId);
    event.preventDefault();
  };

  const onPointerMove = (event: PointerEvent<HTMLDivElement>): void => {
    const active = drag.current;
    if (active === null || active.pointerId !== event.pointerId) {
      return;
    }
    onWidth(active.startWidth + (event.clientX - active.startX));
  };

  const endDrag = (event: PointerEvent<HTMLDivElement>): void => {
    if (drag.current?.pointerId !== event.pointerId) {
      return;
    }
    drag.current = null;
    event.currentTarget.releasePointerCapture(event.pointerId);
  };

  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>): void => {
    const step = event.shiftKey ? COLUMN_KEY_STEP_COARSE : COLUMN_KEY_STEP;
    // A keyboard nudge on a fitted column needs a number to nudge FROM, and the
    // fitted width is the one on screen.
    const current = width ?? event.currentTarget.getBoundingClientRect().left - containerLeft();
    if (event.key === "ArrowLeft") {
      event.preventDefault();
      onWidth(current - step);
    } else if (event.key === "ArrowRight") {
      event.preventDefault();
      onWidth(current + step);
    } else if (event.key === "Home") {
      // Back to fitted, which is also the only way to forget a width — a column
      // dragged somewhere regrettable must have a door out that is not the
      // cookie jar.
      event.preventDefault();
      onWidth(null);
    }
  };

  return (
    <div
      role="separator"
      aria-orientation="vertical"
      aria-label={`${COLUMN_RESIZER_LABEL} ${label}`}
      aria-valuemin={min}
      aria-valuemax={MAX_COLUMN_WIDTH}
      aria-valuenow={width ?? undefined}
      aria-valuetext={width === null ? COLUMN_FITTED_VALUE_TEXT : undefined}
      tabIndex={0}
      data-slot="column-resizer"
      className={cn("relative select-none", className ?? "col-start-2 [grid-row:1/-1]")}
      onPointerDown={onPointerDown}
      onPointerMove={onPointerMove}
      onPointerUp={endDrag}
      onPointerCancel={endDrag}
      onKeyDown={onKeyDown}
      onDoubleClick={() => onWidth(null)}
    >
      <span
        aria-hidden="true"
        className={cn(
          "-inset-x-1 absolute inset-y-0 cursor-col-resize",
          "before:absolute before:inset-y-0 before:left-1 before:w-px before:bg-border",
          "hover:before:bg-ring focus-visible:before:bg-ring",
        )}
      />
    </div>
  );
}
