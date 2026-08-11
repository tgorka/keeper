/**
 * A column of the shell that folds away and can be dragged wider (Story 48.1).
 *
 * # What this is for
 *
 * Until this story the app had exactly one foldable column — the sidebar, Story
 * 45.20 — and exactly one draggable boundary, the Properties key column, Story
 * 44.12. Every other column was a `w-[240px]` or a `w-[320px]` written onto the
 * element by hand, four times, in four files, with no owner. The owner asked for
 * the folds twice and for the resize once; this is the piece all four surfaces
 * now share so that the answer is the same wherever you meet it.
 *
 * # Why a hook returning nodes, and not a wrapper component
 *
 * The four column roots are a `<nav aria-label="Notes">`, a `<div>` carrying the
 * Esc chip-walk, a `<section aria-label="Files">` and a `<div>` carrying the
 * inbox's focus target and its own key handler. A wrapper that owned the root
 * would have to forward a role, a label, a ref, a `tabIndex` and two key
 * handlers, and would still be wrong for the next column. So the root stays the
 * surface's — it spreads {@link SurfaceColumnFrame.rootProps} — and this hands
 * back the two pieces that must be identical everywhere: the fold control, and
 * the seam that goes AFTER the root as its sibling in the flex row.
 *
 * # The fold and the width are two facts, and folding does not spend one
 *
 * A folded column keeps its remembered width. Nothing here writes
 * `keeper_column_widths` on a fold, and nothing reads the fold when computing a
 * width: fold once and unfold, and the column comes back exactly as wide as the
 * user left it. The alternative — clearing the width on fold, or letting the
 * strip's 48px be written back as "the width" — turns a fold into a lost layout,
 * and it is the shape that gets this wrong silently, because the strip is
 * plausible-looking at every step. (Story 48.2 is fixing the same class of bug
 * one layer down, where a lock overwrote a remembered window size with the
 * normalised one.)
 *
 * The seam is UNMOUNTED while folded rather than disabled. There is nothing to
 * size — a 48px strip that reports itself as a resizable column would let a
 * drag write a width the fold is not showing — and the control that undoes the
 * fold is in the strip, one tab stop away, where a seam would otherwise be.
 *
 * # A folded column always has a way back
 *
 * Folded, the column renders its strip and the strip renders the expand button.
 * A fold with no handle is a column the user has deleted by accident, so the
 * control is a real `<button>` in the tab order with an accessible name that
 * says which way it goes — the same shape `SidebarPane` uses, deliberately, so
 * that a person who has met one fold has met all of them.
 */
import { PanelLeftClose, PanelLeftOpen } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import { Button } from "@/components/ui/button";
import { ColumnResizer, useResizableColumn } from "@/components/ui/resizable-columns";
import { SURFACE_COLUMNS, type SurfaceColumnId } from "@/lib/column-widths";
import { columnFoldStore, useColumnFold } from "@/lib/stores/column-fold";
import { cn } from "@/lib/utils";

/**
 * The strip a folded column leaves behind, in px.
 *
 * The sidebar's collapsed rail, which is `w-12`. One folded column should not
 * be a different width from another, and the sidebar got there first.
 */
export const SURFACE_COLUMN_FOLDED_WIDTH = 48;

/** What the control reads while the column is away. Suffixed with the label. */
export const COLUMN_EXPAND_PREFIX = "Expand";

/** What it reads while the column is showing. */
export const COLUMN_COLLAPSE_PREFIX = "Collapse";

/** Everything a surface needs to render one of its columns. */
export interface SurfaceColumnFrame {
  /** Whether the body should be rendered at all. */
  folded: boolean;
  /**
   * Spread onto the column's own root element.
   *
   * The width travels as an inline style rather than a class because it is a
   * remembered number, and Tailwind cannot name a number it will not know until
   * a drag ends. Any `w-[…]` still on the root must go, or it will win.
   */
  rootProps: {
    id: string;
    style: CSSProperties;
    "data-folded": "true" | undefined;
  };
  /**
   * The fold control. The column's FIRST child, above whatever the surface puts
   * in it, so it is the first thing in the tab order and stays in the same place
   * whether the column is folded or not.
   */
  chrome: ReactNode;
  /**
   * The draggable boundary. The column's next SIBLING, not its child — it
   * straddles the edge between this column and the one after it. Null while
   * folded, and null on the phone.
   */
  seam: ReactNode;
}

/**
 * Fold state, width state and the two controls for one surface column.
 *
 * `enabled` is `false` where the arrangement is not a row of columns at all:
 * the phone stack shows one pane at a time, so folding the chat list would hide
 * the whole screen behind a 48px strip and a seam would be a drag target on a
 * touch device with nothing beside it to trade width with. The column still
 * gets its default width there, and nothing is remembered or restored.
 */
export function useSurfaceColumn(
  id: SurfaceColumnId,
  options?: { enabled?: boolean },
): SurfaceColumnFrame {
  const enabled = options?.enabled ?? true;
  const spec = SURFACE_COLUMNS[id];
  // Both hooks run whatever `enabled` says: a hook that is skipped on a
  // viewport change is a hook order that changes under React.
  const folded = useColumnFold((s) => s.columns[id]) && enabled;
  const column = useResizableColumn(id, spec.label);
  // The width the user chose, or the one this column has always had. Never
  // null past this point: a surface column is never "fitted to content" — it
  // holds a list, and a list is as wide as it is given.
  const width = column.width ?? spec.defaultWidth;

  const chrome = enabled ? (
    <div className={cn("flex shrink-0 p-1", folded ? "justify-center" : "justify-end")}>
      <Button
        type="button"
        variant="ghost"
        size="icon"
        aria-label={`${folded ? COLUMN_EXPAND_PREFIX : COLUMN_COLLAPSE_PREFIX} ${spec.label}`}
        // The button sits inside the region it controls, which is how the
        // sidebar's does it: while folded the strip IS the column, and a
        // control parked in a neighbour would belong to the wrong surface.
        aria-expanded={!folded}
        aria-controls={`column-${id}`}
        data-slot="column-fold"
        className="size-7 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-ring"
        onClick={() => columnFoldStore.getState().toggleColumn(id)}
      >
        {folded ? <PanelLeftOpen aria-hidden="true" /> : <PanelLeftClose aria-hidden="true" />}
      </Button>
    </div>
  ) : null;

  return {
    folded,
    rootProps: {
      id: `column-${id}`,
      style: { width: folded ? SURFACE_COLUMN_FOLDED_WIDTH : width },
      "data-folded": folded ? "true" : undefined,
    },
    chrome,
    seam:
      enabled && !folded ? (
        <ColumnResizer
          {...column.resizerProps}
          // The effective width, not the stored one. `resizerProps.width` is
          // null until the first drag, which would make the seam start a drag
          // from a measured fitted width this column never has and report
          // "Fitted to content" to a screen reader about a 320px list.
          width={width}
          className="shrink-0"
        />
      ) : null,
  };
}
