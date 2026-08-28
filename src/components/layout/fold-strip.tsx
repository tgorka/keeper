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
 * # The head is a pane-header band, and that is why the panel won
 *
 * Owning the width, the inset, the gap and the item size still left the four
 * strips visibly ragged, because the two numbers that decide where a strip
 * STARTS were not here: the glyph on the way back, and the height of the row it
 * sits in. The owner's second screenshot is exactly those two.
 *
 * | | sidebar | surface column | panel |
 * |---|---|---|---|
 * | glyph, before | `PanelLeftClose`/`Open` | `PanelLeftClose`/`Open` | **`ChevronsRightLeft`/`LeftRight`** |
 * | head height, before | 44px (`p-2 pb-0` + a 36px control) | 44px, the same sum | **40px** (`py-1` + a 32px control) |
 * | rule under it, before | a 24px hairline at y=48 | a 24px hairline at y=48 | **a full-bleed edge at y=40** |
 *
 * Three strips standing in a row, one of them wearing a different glyph 6px
 * higher with its divider 8px off the other two. The fix is NOT to bring the
 * panel up to the other three: a panel's head is one segment of the header rule
 * that runs across every open pane beside it, and `DESIGN.md` fixes that rule's
 * height at 40px (`pane-header`). 44px cannot be made to line up with 40px, so
 * the panel was the only one of the three that was already right and the other
 * two were carrying a sum nothing checked.
 *
 * So {@link FoldStripHead} is the head of every foldable surface, folded or
 * open: `DESIGN.md`'s 40px band, the strip's 8px horizontal inset, a
 * {@link FOLD_STRIP.headControlSize} control in it, and the band's own bottom
 * edge as the rule under it. One consequence worth saying out loud, because it
 * reverses a decision this module used to state: the head control is 32px and
 * the items BELOW the divider are still 36px. They are two groups — that is
 * what the rule between them means — and each is sized by the row it is in. A
 * control in a header band is a header control, which is the size every other
 * control in every other pane header in this app already is; a control on the
 * strip proper has no label beside it to widen its target, so it stays above
 * `DESIGN.md`'s 32px floor rather than at it.
 *
 * The 24px hairline is gone with it. It was the right mark while a head was
 * just the first thing on a strip; now that a head is a band, a band ends in an
 * edge, and one edge at one height across every pane is the whole point.
 *
 * # A folded strip says its name down its own spine
 *
 * A foldable surface has a display name ({@link FOLD_STRIP.titleClass} draws it
 * at the top while the surface is open). Folded, the strips stand side by side
 * wearing the same glyph, and the owner's first complaint was exactly that: you
 * cannot tell which menu is which.
 *
 * This module's first answer was a tooltip, and the reasoning was measured
 * rather than argued: Instrument Sans Variable at `label-caps` (11px, 0.08em)
 * renders the five names at 27.7px (Files), 32.8px (Menu), 49.5px (Chat list),
 * 49.6px (Note list) and 56.3px (Notes rail), against a 32px content box — so
 * horizontally four of the five do not fit at all, and turned on its side the
 * name would spend 50–56px of the strip's HEIGHT, "the single tallest object on
 * a strip whose every control is 36px, on the one axis a strip is short of".
 *
 * The owner has now looked at the tooltip-only strip and asked for the text.
 * The measurement was not wrong; the conclusion was. Height is only scarce at
 * the TOP of a strip, where the controls are. A folded strip is 48px wide and
 * as tall as the window, and below its two-to-five icons it is empty for
 * several hundred pixels. So {@link FoldStripName} is the LAST child of a
 * strip: it takes the leftover and nothing else.
 *
 * That placement is the whole design, and each part of it answers the objection
 * it was built from:
 *
 * - **It cannot push a control.** It is `flex-1` off a zero basis next to a
 *   `shrink` body: free space flows to the name, and the instant the controls
 *   need that space back the name is the thing that gives, down to nothing.
 *   The strip keeps scrolling exactly as it did.
 * - **It cannot run off the strip.** `writing-mode: vertical-rl` makes height
 *   the INLINE axis, so `max-h` is a line-length cap and `truncate` ellipsises
 *   against it — {@link FOLD_STRIP.namePx}, four rail items' worth, past which
 *   a long note title ends in an ellipsis instead of a scroll bar.
 * - **It cannot steal a click.** `pointer-events-none`, so the strip's hit
 *   targets are the controls and only the controls.
 * - **It cannot be read twice.** `aria-hidden`: the strip is already named by
 *   its region's `aria-label` and by the way back's own accessible name, and a
 *   third copy is a third thing to read.
 *
 * The rotation is spelled `vertical-rl` + `rotate-180` rather than
 * `sideways-lr`, which says it in one property: `sideways-lr` is Chromium 130
 * and Safari 26, and this app ships in a WKWebView on macOS 13. Both compose to
 * the same thing — glyph tops to the left, reading from the bottom up, the way
 * a book spine is read.
 */
import { type LucideIcon, PanelLeftClose, PanelLeftOpen } from "lucide-react";
import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/** Names a folded strip in the DOM, so a test can measure one. */
export const FOLD_STRIP_SLOT = "fold-strip";

/** Names a foldable surface's visible title. */
export const FOLD_STRIP_TITLE_SLOT = "fold-title";

/** Names the band at the top of a foldable surface, folded or open. */
export const FOLD_STRIP_HEAD_SLOT = "fold-head";

/** Names the vertical name down a folded strip's spine. */
export const FOLD_STRIP_NAME_SLOT = "fold-name";

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
   * The body under the head: the top is the head's edge, and the body opens one
   * {@link FOLD_STRIP.gapPx} below it so the rhythm does not change at the seam
   * between the head and the scrolling part.
   */
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

  /**
   * The head band: `DESIGN.md`'s `pane-header` height, which is not this
   * module's to choose. See the header for why every strip is now drawn to it
   * rather than only the panel that had to be.
   */
  headPx: 40,
  headHeightClass: "h-10",

  /** The control in that band. 32px, the size every pane header's controls are. */
  headControlSize: "icon-sm",
  headControlPx: 32,

  /**
   * The way back, in both directions, for every foldable surface.
   *
   * One pair, because the owner counted the glyphs: a panel folded under
   * `ChevronsRightLeft` while the three columns beside it folded under
   * `PanelLeftClose`, and nothing but this constant can stop a fifth mechanism
   * picking a third pair. `PanelLeft*` and not the chevrons because it draws
   * the thing that happens — a panel leaving a strip behind — rather than an
   * abstract squeeze.
   */
  foldIcon: PanelLeftClose as LucideIcon,
  unfoldIcon: PanelLeftOpen as LucideIcon,

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

  /**
   * How long that name may get once it is turned on its side, in px of the
   * strip's height, and the class that says the same thing.
   *
   * 160px is four rail items and a gap — enough for every surface title this
   * app has (the longest, "Notes rail", is 56.3px) and enough for a real note
   * title, while still being visibly a label rather than a column of text. A
   * note is titled by its first line (`deriveTitle`), which is user input and
   * therefore unbounded: past this the line ellipsises.
   */
  namePx: 160,
  nameMaxClass: "max-h-40",
} as const;

/**
 * The band at the top of a foldable surface.
 *
 * Folded or open, drawer, column or panel: one height, one inset, one bottom
 * edge, so the way back is at the same y in every strip in the row and the rule
 * under it is one line across the shell. See this module's header for why the
 * folded panel's 40px won over the other two mechanisms' 44px.
 *
 * A caller supplies the arrangement and nothing else — `justify-center` for a
 * strip whose head holds one control, nothing for an open surface whose title
 * is `flex-1` and pushes the control to the end.
 *
 * # Why this is a `div` and not a `header`
 *
 * A `<header>` is a `banner` LANDMARK whenever no `article`/`aside`/`main`/
 * `nav`/`section` scopes it, and the four column roots are not one kind of
 * element: the notes rail and the chat sidebar are `<nav>`, the files tree is
 * `<section>`, and the note list and the chat list are plain `<div>`s carrying
 * their own key handlers. Drafted as a `<header>`, this component was measured
 * in the running app announcing itself as a second and third `banner` — one
 * per div-rooted column — which is a landmark that means "site orientation
 * chrome" and there is only ever one of it. Scoped inside a section the tag
 * carries no role at all, so it was buying nothing anywhere and costing this
 * everywhere. The surfaces are named by their region's own label; the band is
 * a band.
 */
export function FoldStripHead({
  className,
  children,
}: {
  className?: string;
  children: ReactNode;
}) {
  return (
    <div
      data-slot={FOLD_STRIP_HEAD_SLOT}
      data-fold-strip-items="inset"
      className={cn(
        "flex shrink-0 items-center border-border border-b",
        FOLD_STRIP.headHeightClass,
        FOLD_STRIP.padXClass,
        FOLD_STRIP.gapClass,
        className,
      )}
    >
      {children}
    </div>
  );
}

/**
 * A folded strip's name, down its spine, read from the bottom.
 *
 * The LAST child of the strip, after the body: see this module's header for the
 * four properties that make a rotated name at 48px safe rather than the trap it
 * was judged to be. In one line each — it takes only leftover space, it
 * ellipsises at {@link FOLD_STRIP.namePx} instead of running off the strip, it
 * is transparent to the pointer, and it is invisible to a screen reader that
 * has already been told this surface's name twice.
 */
export function FoldStripName({ name }: { name: string }) {
  return (
    <div
      aria-hidden="true"
      data-slot={FOLD_STRIP_NAME_SLOT}
      className={cn(
        "pointer-events-none flex min-h-0 flex-1 select-none items-end justify-center overflow-hidden",
        FOLD_STRIP.padClass,
      )}
    >
      <span
        className={cn(
          "label-caps truncate text-muted-foreground",
          FOLD_STRIP.nameMaxClass,
          "rotate-180 [writing-mode:vertical-rl]",
        )}
      >
        {name}
      </span>
    </div>
  );
}
