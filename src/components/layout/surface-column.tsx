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
 * # A folded column always has a way back, and keeps its inside reachable
 *
 * Folded, the column renders its RAIL: the control that undoes the fold, and
 * under it one icon button per thing the fold would otherwise have taken away.
 * It is the sidebar's collapsed rail — same width, same button metric, same
 * name-plus-tooltip shape — because a person who has met one fold has met all
 * of them, and because two 48px rails side by side must not read as an
 * accident.
 *
 * The first cut of this folded to the fold control and nothing else. The owner
 * folded both Notes columns and got two dead strips: the vault switcher, the
 * create, the three rail sections, the search field and the count were not put
 * away, they were gone. **A fold suspends a WIDTH, never a CAPABILITY.**
 *
 * The body still unmounts, and that is the point — the subscriptions, the
 * virtualisers and the editor buffers are exactly the cost folding reclaims. So
 * the rail is CHROME the hook draws from a declaration, never a resurrected
 * body: nothing behind it is mounted, and a control whose only honest behaviour
 * at 48px is "unfold, and put me where I asked to be" is a legitimate rail
 * control. Reachability is the requirement; doing the work inside 48px is not.
 *
 * {@link SurfaceRail} is REQUIRED and typed non-empty, so the next column
 * cannot be added with a strip nobody filled: the mistake is unrepresentable
 * rather than merely discouraged, and {@link useSurfaceColumn} says it out loud
 * for the cast that gets around a type.
 */
import { type LucideIcon, PanelLeftClose, PanelLeftOpen } from "lucide-react";
import type { CSSProperties, ReactNode } from "react";
import {
  FOLD_STRIP,
  FOLD_STRIP_SLOT,
  FOLD_STRIP_TITLE_SLOT,
  FoldStripDivider,
} from "@/components/layout/fold-strip";
import { Button } from "@/components/ui/button";
import { ColumnResizer, useResizableColumn } from "@/components/ui/resizable-columns";
import { Tooltip, TooltipContent, TooltipProvider, TooltipTrigger } from "@/components/ui/tooltip";
import { SURFACE_COLUMNS, type SurfaceColumnId } from "@/lib/column-widths";
import { columnFoldStore, useColumnFold } from "@/lib/stores/column-fold";
import { cn } from "@/lib/utils";

/** What the control reads while the column is away. Suffixed with the title. */
export const COLUMN_EXPAND_PREFIX = "Expand";

/** What it reads while the column is showing. */
export const COLUMN_COLLAPSE_PREFIX = "Collapse";

/** Names the rail in the DOM, so a test can ask what a folded column offers. */
export const COLUMN_RAIL_SLOT = "column-rail";

/** Names one control on it. */
export const COLUMN_RAIL_CONTROL_SLOT = "column-rail-control";

/**
 * One control a surface contributes to its folded rail.
 *
 * Declared rather than handed over as a node, because the treatment is the part
 * that must not be reinvented per surface: the icon button, its size, the
 * accessible name that leads with the visible word (WCAG 2.5.3), the tooltip
 * that says the same words to a pointer, and the corner count. A surface says
 * what the control IS and what it does; the rail says how it looks.
 */
export interface SurfaceRailControl {
  /** React key, and the `data-rail-control` a test names it by. */
  id: string;
  /** Drawn `aria-hidden`: the name carries the meaning, never the glyph. */
  icon: LucideIcon;
  /** The visible word. First in the name, and the tooltip verbatim. */
  label: string;
  /**
   * What else there is to say — a count in words, a state the strip is hiding.
   * Appended to the name and the tooltip, never replacing the label.
   */
  detail?: string | null;
  /**
   * Digits in the corner, `aria-hidden`, clamped at `99+`. The words that say
   * them belong in {@link SurfaceRailControl.detail} — a badge alone reaches a
   * screen reader not at all.
   */
  count?: number | null;
  /** Disabled rather than absent, so the rail does not change shape under you. */
  disabled?: boolean;
  onSelect: () => void;
}

/**
 * A column's rail, typed so it cannot be empty.
 *
 * The dead 48px strip was not a styling mistake, it was a column with nothing
 * declared for it. A non-empty tuple makes that unwritable: build the rail as
 * `[somethingAlwaysThere, ...whateverIsConditional]` and a surface cannot
 * accidentally hand over the empty array.
 */
export type SurfaceRail = readonly [SurfaceRailControl, ...SurfaceRailControl[]];

/**
 * One rail control, in the house treatment.
 *
 * {@link FOLD_STRIP.controlSize} and a tooltip on the right: the sidebar's
 * collapsed rail, which is the proof this works at 48px. No `title` beside the
 * tooltip — the two draw the same words twice, a second box a second later
 * under the first, and the tooltip is the one this app already uses at this
 * width.
 */
function RailControl({ control }: { control: SurfaceRailControl }) {
  const { icon: Icon, label, detail, count, disabled, onSelect } = control;
  const name =
    detail === null || detail === undefined || detail === "" ? label : `${label}, ${detail}`;
  const badge =
    count === null || count === undefined || count <= 0 ? null : count > 99 ? "99+" : String(count);
  return (
    <Tooltip>
      <TooltipTrigger asChild>
        <Button
          type="button"
          variant="ghost"
          size={FOLD_STRIP.controlSize}
          aria-label={name}
          data-slot={COLUMN_RAIL_CONTROL_SLOT}
          data-rail-control={control.id}
          disabled={disabled}
          className="relative shrink-0"
          onClick={onSelect}
        >
          <Icon aria-hidden="true" />
          {badge !== null && (
            <span
              aria-hidden="true"
              className="absolute -top-0.5 -right-0.5 min-w-4 rounded-full bg-secondary px-1 text-meta text-secondary-foreground leading-none tabular-nums"
            >
              {badge}
            </span>
          )}
        </Button>
      </TooltipTrigger>
      <TooltipContent side="right">{name}</TooltipContent>
    </Tooltip>
  );
}

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
    /**
     * The region's name, from exactly one place.
     *
     * Open, it points at the visible title this frame draws, so a reader hears
     * the words that are on screen and the two cannot drift apart. Folded there
     * is no title to point at — the strip's name lives on the way back — so the
     * name is spelled out instead. Either way the surface spreads this and does
     * NOT write an `aria-label` of its own: a region labelled "Files" wrapping a
     * heading reading "Files" is the same word announced twice.
     */
    "aria-labelledby": string | undefined;
    "aria-label": string | undefined;
  };
  /**
   * The fold control, and while folded the rail under it. The column's FIRST
   * child, above whatever the surface puts in it, so it is the first thing in
   * the tab order and stays in the same place whether the column is folded or
   * not. Folded, it is the ONLY child: it is the whole strip.
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
 * Fold state, width state, the rail and the two controls for one surface column.
 *
 * `rail` is what this column still offers once its body is unmounted, and it is
 * required: see {@link SurfaceRail}. Order it as the surface would read it top
 * to bottom.
 *
 * `enabled` is `false` where the arrangement is not a row of columns at all:
 * the phone stack shows one pane at a time, so folding the chat list would hide
 * the whole screen behind a 48px strip and a seam would be a drag target on a
 * touch device with nothing beside it to trade width with. The column still
 * gets its default width there, and nothing is remembered or restored.
 */
export function useSurfaceColumn(
  id: SurfaceColumnId,
  options: { rail: SurfaceRail; enabled?: boolean },
): SurfaceColumnFrame {
  const enabled = options.enabled ?? true;
  const spec = SURFACE_COLUMNS[id];
  // Both hooks run whatever `enabled` says: a hook that is skipped on a
  // viewport change is a hook order that changes under React.
  const folded = useColumnFold((s) => s.columns[id]) && enabled;
  const column = useResizableColumn(id, spec.label);
  // The width the user chose, or the one this column has always had. Never
  // null past this point: a surface column is never "fitted to content" — it
  // holds a list, and a list is as wide as it is given.
  const width = column.width ?? spec.defaultWidth;
  // The type already refuses this. This is the same rule for the caller that
  // got past it — a cast, a `.filter()`, JavaScript — and it fires whether or
  // not the column happens to be folded right now, because a strip nobody
  // filled is a defect the moment the column exists, not the moment the user
  // finds it. Loud beats a dead 48px strip: that one shipped.
  if (enabled && options.rail.length === 0) {
    throw new Error(
      `the ${id} column would fold to an empty strip — a surface column must contribute at least one rail control`,
    );
  }

  // Mid-sentence, so `spec.label` and not `spec.title`: the name is a sentence
  // and the title is the word in it. WCAG 2.5.3 asks that the visible label be
  // IN the accessible name ignoring case, which `label` guarantees by contract.
  const foldName = `${folded ? COLUMN_EXPAND_PREFIX : COLUMN_COLLAPSE_PREFIX} ${spec.label}`;
  const titleId = `column-${id}-title`;
  const foldControl = (
    <Button
      type="button"
      variant="ghost"
      size={FOLD_STRIP.controlSize}
      // Contains the visible title, so the control can be operated by anyone
      // saying the word they can see (WCAG 2.5.3), and leads with the verb,
      // which is the half a folded strip has no other way to state.
      aria-label={foldName}
      // The button sits inside the region it controls, which is how the
      // sidebar's does it: while folded the strip IS the column, and a
      // control parked in a neighbour would belong to the wrong surface.
      aria-expanded={!folded}
      aria-controls={`column-${id}`}
      data-slot="column-fold"
      onClick={() => columnFoldStore.getState().toggleColumn(id)}
    >
      {folded ? <PanelLeftOpen aria-hidden="true" /> : <PanelLeftClose aria-hidden="true" />}
    </Button>
  );

  const chrome = !enabled ? null : folded ? (
    // The strip, and every number in it comes from {@link FOLD_STRIP} rather
    // than from this file, so the fold control of a folded column sits at the
    // same height and the same size as the fold control of the folded sidebar
    // beside it. Two 48px rails that disagreed by four pixels read as an
    // accident, which is half of what the owner saw.
    //
    // `TooltipProvider` here rather than relied upon from an ancestor: the strip
    // is this hook's, and a column that only names its controls inside an app
    // shell is a column that goes silent in every other host.
    <TooltipProvider>
      <div
        data-slot={COLUMN_RAIL_SLOT}
        data-fold-strip={FOLD_STRIP_SLOT}
        data-fold-strip-items="inset"
        className={cn(
          "flex min-h-0 flex-1 flex-col items-center overflow-y-auto",
          FOLD_STRIP.padClass,
          FOLD_STRIP.gapClass,
        )}
      >
        {/* The way back, and the only thing on a 48px strip that says WHICH
            surface this is. The tooltip and the accessible name are the same
            words, because with the title gone the tooltip IS the visible label
            — see this module's neighbour `fold-strip.tsx` for the measurement
            that ruled out putting the name on the strip itself. */}
        <Tooltip>
          <TooltipTrigger asChild>{foldControl}</TooltipTrigger>
          <TooltipContent side="right">{foldName}</TooltipContent>
        </Tooltip>
        {/* Two things, not one list: the way back, and then what is inside. */}
        <FoldStripDivider />
        {options.rail.map((control) => (
          <RailControl key={control.id} control={control} />
        ))}
      </div>
    </TooltipProvider>
  ) : (
    // Open, the column says its name (Story 48.3). Every foldable surface in
    // the shell draws it here, in one treatment, so four columns side by side
    // are told apart by reading rather than by hovering — and the fold control
    // keeps its place at the end of the row, because a strip that gained a
    // title and lost its way back would be the worse defect.
    <div className={cn("flex shrink-0 items-center", FOLD_STRIP.headPadClass, FOLD_STRIP.gapClass)}>
      <h2 id={titleId} data-slot={FOLD_STRIP_TITLE_SLOT} className={FOLD_STRIP.titleClass}>
        {spec.title}
      </h2>
      {foldControl}
    </div>
  );

  return {
    folded,
    rootProps: {
      id: `column-${id}`,
      style: { width: folded ? FOLD_STRIP.widthPx : width },
      "data-folded": folded ? "true" : undefined,
      "aria-labelledby": enabled && !folded ? titleId : undefined,
      "aria-label": enabled && !folded ? undefined : spec.title,
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
