/**
 * A header's action group, sized to the header (Story 48.5, AD-104).
 *
 * # The defect
 *
 * Story 46.5 measured correctly and concluded too widely. Six controls plus two
 * truncating spans do not fit the 560px quick-capture window, the row does not
 * wrap, and the action cluster is its last child — so the verb at the end fell
 * off the edge. The repair was to put every verb in a `⋯` menu and keep one
 * control beside it. That is the right shape at 560px and the wrong shape at
 * 1400px, and the note editor is mounted at both. Three separate field reports
 * against 0.8.1 — "I still see no way to delete notes", "I don't see
 * attachments", and tags-on-a-recording-note reported as missing when it works
 * — are one cause: a menu that holds everything is a menu nobody opens.
 *
 * # The rule
 *
 * Show what fits, menu what does not, in a priority order the caller declares.
 * One mechanism at every width: no media query, no host-conditional render, no
 * "is this the capture window" test — the capture window and the workspace pane
 * mount the same header and differ only in how many pixels they hand it.
 *
 * # Why the policy is a pure function and the measuring is not
 *
 * jsdom performs no layout. Every element reports a zero rect, and
 * `src/test/setup.ts`'s shim answers one viewport for the zero-sized ones and
 * deliberately stops at CodeMirror's edge — so no test in this repository can
 * observe a reflow, and a test that asserted "the fourth control moved into the
 * menu at 900px" would be asserting the shim. So the decision is
 * {@link planPriorityActions}: (available width, item widths, gap) in, a count
 * out, no DOM. Every boundary in the policy is then provable exactly, and the
 * only untestable part left is the plumbing that reads three numbers off the
 * page — which is also the part that has no branches.
 *
 * # Why the widths are measured and not declared
 *
 * A table of per-item widths is a guess about a font, a locale and a user's
 * text-size setting, and it is wrong on the first machine that disagrees with
 * the one it was written on. The first commit renders every candidate as a
 * control, reads the width the browser gave each one, and re-plans before the
 * frame is painted: a layout effect runs after the DOM is mutated and before
 * paint, so the pass is a measurement and not a flicker. Each width is recorded
 * **once**. A group that re-measured what it had just re-laid out is the shape
 * that oscillates — promote an item, the group grows, the measurement changes,
 * demote it, repeat — and a toolbar that flickers at one particular window
 * width is a worse defect than the one being fixed here. Widths are constants:
 * these labels do not change, and the wrappers are `shrink-0` so nothing is
 * measured while squeezed.
 *
 * An item the group has never rendered has no width, so a candidate that
 * appears later (`Show in Files` resolves when the vault list arrives) puts the
 * group back into a measuring pass for one commit and then re-plans. That is
 * why the map is keyed by {@link PriorityAction.id} rather than by index.
 *
 * # What this component does not decide
 *
 * The order, the labels, and what the menu holds. Priority is a product
 * decision and belongs with the surface that has the product; the menu is a
 * render prop taking a predicate, so a caller can interleave items this group
 * has never heard of (an export flow, a capture window) in its own order and
 * keep its own separator rules. The one thing the caller must honour is the
 * predicate: an item the group promoted must not also be put in the menu.
 */
import { type ReactNode, useLayoutEffect, useRef, useState } from "react";
import { Button } from "@/components/ui/button";
import { PANE_HEADER_GAP_PX } from "./pane-header";

/** Marks the group, so a test can find it without knowing the caller. */
export const PRIORITY_ACTIONS_SLOT = "priority-actions";

/** Marks a promoted control with the id it was promoted for. */
export const PRIORITY_ACTION_ATTR = "data-priority-action";

/** One candidate for the row, in the caller's priority order. */
export interface PriorityAction {
  /**
   * Stable identity. What {@link PriorityActionsProps.menu}'s predicate is
   * asked about, and the key the measured width is remembered under — so an
   * item that comes and goes is measured once and not once per appearance.
   */
  readonly id: string;
  /** The word, on the control and in the menu item. Its width IS the measurement. */
  readonly label: string;
  /** What the control and the menu item both do. One handler, so they cannot drift. */
  readonly onSelect: () => void;
}

export interface PriorityPlanInput {
  /** Pixels the whole group may occupy. */
  readonly available: number;
  /** Pixels the group spends before any candidate: the fixed leading controls,
   *  the menu trigger, and the gaps between those. */
  readonly reserved: number;
  /** Each candidate's natural width, in priority order. */
  readonly widths: readonly number[];
  /** Pixels between two adjacent controls. */
  readonly gap: number;
}

/**
 * How many leading candidates are rendered as controls. The rest go in the menu.
 *
 * **A prefix, deliberately.** When the next item does not fit, the walk stops
 * rather than skipping ahead to a narrower one further down the list. A group
 * that packed by width would reorder itself as the window is dragged — the
 * fourth control becoming the second because the third grew a character — and
 * a toolbar whose contents change places is unlearnable. Priority means
 * priority: an item is out here only if everything above it is.
 *
 * **The menu is never in the budget's gift.** `reserved` already contains the
 * trigger, so a group too narrow for anything promotes nothing and still has
 * its menu. There is no width at which a verb becomes unreachable.
 *
 * **Every promoted item costs its own gap**, because there is always something
 * to its right: the menu trigger, at minimum. That also makes the degenerate
 * measurement honest — before anything is measured every width is zero, and a
 * zero-cost item would "fit" in a zero-width group.
 *
 * A width that is not a finite number is a measurement that has not happened;
 * it stops the walk rather than being treated as zero.
 */
export function planPriorityActions({
  available,
  reserved,
  widths,
  gap,
}: PriorityPlanInput): number {
  if (!Number.isFinite(available) || !Number.isFinite(reserved) || !Number.isFinite(gap)) {
    return 0;
  }
  let free = available - reserved;
  let promoted = 0;
  for (const width of widths) {
    if (!Number.isFinite(width)) {
      break;
    }
    const cost = width + gap;
    if (free < cost) {
      break;
    }
    free -= cost;
    promoted += 1;
  }
  return promoted;
}

export interface PriorityActionsProps {
  /**
   * Pixels this group may occupy — `PaneHeader`'s render-prop form supplies it
   * from a `ResizeObserver` on the header. Zero is the honest answer before the
   * first observation, and it renders the 560px shape: leading controls and a
   * menu.
   */
  budget: number;
  /** Controls that are always out here and never in the menu, before the
   *  candidates. Their measured width is part of `reserved`. */
  leading?: ReactNode;
  /** The candidates, most important first. */
  items: readonly PriorityAction[];
  /**
   * The menu, rendered by the caller so it keeps its own order and its own
   * separators. `inMenu(id)` answers for every candidate; an id this group does
   * not know is in the menu, which is the right answer for the items that never
   * promote.
   */
  menu: (inMenu: (id: string) => boolean) => ReactNode;
  /** Pixels between adjacent controls. The arithmetic and the layout read this
   *  same number, so they cannot come to disagree. */
  gap?: number;
}

export function PriorityActions({
  budget,
  leading = null,
  items,
  menu,
  gap = PANE_HEADER_GAP_PX,
}: PriorityActionsProps): React.ReactElement {
  const leadingRef = useRef<HTMLDivElement>(null);
  const menuRef = useRef<HTMLDivElement>(null);
  const controls = useRef<Map<string, HTMLElement>>(new Map()).current;
  const [reserved, setReserved] = useState<number | null>(null);
  const [widths, setWidths] = useState<ReadonlyMap<string, number>>(() => new Map());

  // A candidate with no width yet has never been rendered, and the only way to
  // measure it is to render it: this commit is the measuring pass.
  const measuring = reserved === null || items.some((item) => !widths.has(item.id));
  const promoted = measuring
    ? items.length
    : planPriorityActions({
        available: budget,
        reserved,
        widths: items.map((item) => widths.get(item.id) ?? Number.NaN),
        gap,
      });

  // Every commit, because a candidate can arrive at any of them. Write-once per
  // measurement, which is what makes that safe: the effect can only ever turn
  // unknowns into numbers, so it cannot chase its own re-layout.
  useLayoutEffect(() => {
    const menuBox = menuRef.current;
    if (menuBox === null) {
      return;
    }
    if (reserved === null) {
      const leadingBox = leadingRef.current;
      setReserved(
        menuBox.getBoundingClientRect().width +
          (leadingBox === null ? 0 : leadingBox.getBoundingClientRect().width + gap),
      );
    }
    let taken: Map<string, number> | null = null;
    for (const item of items) {
      if (widths.has(item.id)) {
        continue;
      }
      const control = controls.get(item.id);
      if (control === undefined) {
        continue;
      }
      taken ??= new Map(widths);
      taken.set(item.id, control.getBoundingClientRect().width);
    }
    if (taken !== null) {
      setWidths(taken);
    }
  });

  const inMenu = (id: string): boolean => {
    const at = items.findIndex((item) => item.id === id);
    return at < 0 || at >= promoted;
  };

  return (
    // `gap` is a number here rather than a class because the same number is
    // what the plan spends; a `gap-2` beside a `PANE_HEADER_GAP_PX` is two
    // places to change one fact.
    <div data-slot={PRIORITY_ACTIONS_SLOT} className="flex items-center" style={{ gap }}>
      {leading === null ? null : (
        // `shrink-0` on every wrapper: a measurement taken while the browser is
        // squeezing the thing measured is a measurement of the squeeze.
        <div ref={leadingRef} className="flex shrink-0 items-center">
          {leading}
        </div>
      )}
      {items.slice(0, promoted).map((item) => (
        <Button
          key={item.id}
          ref={(node) => {
            if (node === null) {
              controls.delete(item.id);
            } else {
              controls.set(item.id, node);
            }
          }}
          {...{ [PRIORITY_ACTION_ATTR]: item.id }}
          size="sm"
          variant="ghost"
          className="shrink-0"
          onClick={item.onSelect}
        >
          {item.label}
        </Button>
      ))}
      <div ref={menuRef} className="flex shrink-0 items-center">
        {menu(inMenu)}
      </div>
    </div>
  );
}
