/**
 * The one set of metrics every folded strip in keeper is drawn to, and the one
 * treatment every foldable surface names itself with.
 *
 * # Why this module exists
 *
 * Four different things in this app fold down to a vertical strip of icons —
 * the chat sidebar (`sidebar-pane.tsx`), a surface column
 * (`surface-column.tsx`), a panel (`panel-strip.tsx`) and a foldable section on
 * the sidebar's own rail (`sidebar-group.tsx`) — and until this module every
 * one of them carried its own numbers. They disagreed:
 *
 * | | sidebar | surface column | panel | FoldSection |
 * |---|---|---|---|---|
 * | width   | `w-12` 48px   | `48` inline   | **`w-auto`**    | inherits |
 * | padding | `p-2` 8px     | `p-2` 8px     | `px-1 py-1` 4px | `px-1`+`pb-1` 4px |
 * | gap     | `gap-1` 4px   | `gap-1` 4px   | none            | `gap-0.5` 2px |
 * | item    | `size-9` 36px | `size-9` 36px | `size-8` 32px   | `h-auto` ≈22px |
 * | divider | **none**      | **yes**       | n/a             | n/a |
 *
 * Two of those numbers were literals in two files whose comments claimed to
 * match each other — `SURFACE_COLUMN_FOLDED_WIDTH = 48` said "the sidebar's
 * collapsed rail, which is `w-12`", and `SIDEBAR_WIDTH_CLASS.collapsed` said
 * the reverse. A comment is not a constraint. The rest were never even claimed
 * to agree, which is why the sidebar's own rail changed rhythm HALFWAY DOWN
 * one strip: its nav list is `p-2`/`gap-1` and the SPACES and NETWORKS sections
 * under it were `px-1`/`gap-0.5`, so both the inset and the row spacing shifted
 * at a boundary nobody drew.
 *
 * So the numbers live here, once, and every mechanism spends them. A fifth
 * foldable thing gets them for free; a change to them changes all five.
 *
 * # What is fixed, and what a strip may still decide
 *
 * The 48px width is load-bearing and `DESIGN.md` says so by name ("Load-bearing
 * dimensions that the design may not move … the 48px folded-column strip"). The
 * padding, gap and item size are the sidebar's, because the sidebar's rail
 * shipped first and is the proof the treatment works at this width.
 *
 * A strip decides exactly one thing for itself: whether anything follows the
 * way back. Where something does — the sidebar's views, a surface column's rail
 * controls — {@link FoldStripDivider} separates the two, because "the way out"
 * and "what is still in here" are two groups and not one list. Where nothing
 * does, as on a folded panel, there is nothing to separate.
 *
 * # A folded strip says its name in a tooltip, and the geometry decided that
 *
 * A foldable surface has a display name ({@link FOLD_STRIP.titleClass} draws it
 * at the top while the surface is open). Folded, the four strips stand side by
 * side wearing the same chevron, and the owner's complaint was exactly that:
 * you cannot tell which menu is which.
 *
 * Vertical text was measured against the real font rather than argued about.
 * Instrument Sans Variable at `label-caps` (11px, 0.08em) renders the five
 * names at 27.7px (Files), 32.8px (Menu), 49.5px (Chat list), 49.6px (Note
 * list) and 56.3px (Notes rail). The strip's content box is 48 − 2×8 = **32px**,
 * so horizontally four of the five do not fit at all and the two that would be
 * truncated — "Chat…" and "Note…" — are the two a reader most needs told apart.
 * Turned on its side the text fits the cross axis easily (an 11px line box at
 * 1.4 is 15.4px) but spends 50–56px of the strip's HEIGHT, which would make the
 * name the single tallest object on a strip whose every control is 36px, on the
 * one axis a strip is short of, in a rotated register that appears nowhere else
 * in this codebase.
 *
 * So the name rides the way back: its accessible name and its tooltip are the
 * same words, on the glyph that is already there, in the treatment every other
 * control on the strip already uses. This is not a new idea — `panel-strip.tsx`
 * has done it since Story 45.1 for exactly this reason ("a pointer that hovered
 * a bare chevron would learn only that the strip folds, not which of four files
 * this one is"). It is now what all four do.
 *
 * # The one documented exception: a strip whose head is a pane header
 *
 * A folded panel is a strip standing in a row of unfolded panels, and its head
 * shares their header band — one continuous 40px rule across the strip, which
 * `DESIGN.md` fixes as the `pane-header` height. A 36px control in that band is
 * 44px and breaks the line. So a panel's head control is
 * {@link FOLD_STRIP.headControlPx}, the pane header's 32px, and that number is
 * named HERE rather than typed there: an exception a module states is a
 * decision, an exception a file keeps to itself is the drift this module exists
 * to end.
 */

/** Names a folded strip in the DOM, so a test can measure one. */
export const FOLD_STRIP_SLOT = "fold-strip";

/** Names a foldable surface's visible title. */
export const FOLD_STRIP_TITLE_SLOT = "fold-title";

/** Names the hairline between the way back and what is still on the strip. */
export const FOLD_STRIP_DIVIDER_SLOT = "fold-divider";

export const FOLD_STRIP = {
  /**
   * What a fold leaves behind, in px, and the class that says the same thing.
   *
   * Both forms exist because neither of the two consumers can use the other's:
   * a surface column's width is an inline style (it is a remembered number the
   * rest of the time, and Tailwind cannot name a number a drag has not produced
   * yet), while the sidebar and a panel are plain elements with a class. They
   * are one value in one place, which is the part that was missing.
   */
  widthPx: 48,
  widthClass: "w-12",

  /** The strip's own inset. 8px, the `gutter` `DESIGN.md` names. */
  padPx: 8,
  padClass: "p-2",
  padXClass: "px-2",

  /**
   * The head of a strip whose body is a separate scrolling element: it ends
   * flush, and the body below opens with exactly one {@link FOLD_STRIP.gapPx},
   * so the rhythm does not change at the seam between the two.
   */
  headPadClass: "p-2 pb-0",

  /** That body. The top is the head's job; the other three sides are the inset. */
  bodyPadClass: "p-2 pt-1",

  /** Between two things on a strip. */
  gapPx: 4,
  gapClass: "gap-1",

  /**
   * One item on a strip: a `size="icon"` button, 36px square.
   *
   * Above `DESIGN.md`'s 32px control floor rather than at it, because a strip
   * is the one place a control has no label beside it to widen its target.
   */
  controlSize: "icon",
  controlPx: 36,

  /**
   * The same 36px for an item on the strip that is NOT a `Button` — the Spaces
   * and Networks rows, which are avatars in their own `<button>`.
   *
   * Spelled out rather than composed, because Tailwind reads source text: a
   * `size-${n}` built from {@link FOLD_STRIP.controlPx} is a class that never
   * gets generated. Both rows used to reach 36px by putting `p-1.5` around a
   * 24px avatar, each with a paragraph of arithmetic explaining why — two
   * copies of a sum that stops being 36 the day the avatar changes size.
   */
  controlClass: "size-9",

  /** The pane-header exception. See this module's header for why. */
  headControlSize: "icon-sm",
  headControlPx: 32,

  /**
   * A foldable surface's visible name, while it is open.
   *
   * `font-heading text-title` is `DESIGN.md`'s `pane-header` typography and the
   * shape `files-pane.tsx` was the only surface to draw before this. `min-w-0
   * flex-1 truncate` because the name shares its row with the fold control and
   * a column can be dragged to 180px: the title gives up its pixels to the
   * control rather than pushing the way back off the end of the row.
   */
  titleClass: "min-w-0 flex-1 truncate font-heading text-title",
} as const;

/**
 * The hairline between the way back and whatever is still reachable below it.
 *
 * `aria-hidden`, and 24px rather than the strip's full 32px of content width:
 * it is a grouping mark, not a boundary between regions, and `DESIGN.md`'s rule
 * that a seam has exactly one owner is about the edges of columns.
 *
 * No margin of its own. The strip's `gap` spaces it like everything else on the
 * strip, which is the whole point — a divider carrying its own `my-*` is how
 * the surface column ended up with 13px where the sidebar had 8px.
 */
export function FoldStripDivider() {
  return (
    <div
      aria-hidden="true"
      data-slot={FOLD_STRIP_DIVIDER_SLOT}
      className="h-px w-6 shrink-0 bg-border"
    />
  );
}
