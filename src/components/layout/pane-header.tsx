/**
 * One pane's header: three groups, and the rule about which of them may change
 * width (Story 46.13, AD-104, UX-DR77).
 *
 * # The defect this shape exists to refuse
 *
 * A header is a single non-wrapping flex row, so every member of it competes for
 * one width: when one member grows, the width it took came out of whatever else
 * in the row could give. Story 46.4 found that with a title, a save caption,
 * five buttons and a menu trigger all direct children of one row, every save
 * cycle changed the caption's width twice and the whole button cluster reflowed
 * — in the 560px quick-capture window there was no slack for the title to give
 * back. The fix was never a width on the caption. It was the row's structure:
 *
 * 1. **identity** — `min-w-0 flex-1` off a zero basis. The only member allowed
 *    to give ground, and the only one that does: because its basis is zero it
 *    contributes *nothing* to the row's own content width, so its contents can
 *    change on every keystroke without moving anything to its right.
 * 2. **status** — `shrink-0`, and a box whose width is a constant. A slot that
 *    can be squeezed is not a slot. Its width is reserved by *measurement*
 *    rather than guessed: the caller hands over every string the slot can ever
 *    show, they are rendered `invisible` and `aria-hidden` inside it, and the
 *    browser makes the slot as wide as the widest of them in the font, the
 *    locale and the clock format this machine actually has. The one visible
 *    caption is `absolute` and therefore out of flow, so a string nobody could
 *    reserve for — a message composed in Rust, of unbounded length — cannot
 *    widen the box. It is ellipsised on screen, kept whole in the DOM for a
 *    screen reader, and put on `title` for a pointer.
 * 3. **actions** — last, and deliberately **not** `shrink-0`. That asymmetry is
 *    46.4's corrected ruling: if what the actions hold ever outgrows the window,
 *    the row should squeeze them rather than push the last control off the
 *    right-hand edge, which is how 46.5's defect happened. It is safe precisely
 *    because the status group's width is a constant — a constant offset plus a
 *    shrinkable tail cannot move when the status changes.
 *
 * # Why it is a component now and was not before
 *
 * AD-104's rule of two. 46.4 landed the shape concretely in the note editor and
 * refused to extract it, because a shared component built for a consumer that
 * does not exist is a guess about what the second consumer will need. There are
 * now two real ones with two genuinely different status strings — the note
 * editor's `Saved · HH:MM`, whose width is a locale's business, and the Files
 * pane's `Unsaved changes`, whose width is a font's — plus a third header
 * (`PanelFrame`) with the same identity/actions rule and no status at all. So
 * the structural guarantee lives here once, and 46.4's tests assert it against
 * this file.
 *
 * # What a caller supplies, and what it may not
 *
 * Content only. The three wrappers, their classes and their order are this
 * component's, because they *are* the fix — a caller that could pass a class
 * into the status slot could pass `flex-1` into it, and the jump would be back
 * with the shape intact. The outer `<header>`'s own padding and border differ
 * per surface (a panel header is not a note header) and that is the one class
 * hook: {@link PaneHeaderProps.className}.
 *
 * {@link PaneHeaderProps.status} is nullable rather than always present. A
 * header with nothing to report renders two groups, not an empty reserved box:
 * a zero-width slot in a `gap-2` row is 8px of space reserved for nothing, and
 * "there is no status here" is a different claim from "the status is empty".
 *
 * # When group 3 does not fit (Story 48.5)
 *
 * 46.5's ruling — everything into a `⋯` menu and one control beside it — is
 * right at 560px and wrong at 1400px, and this header is mounted at both. A
 * caller that passes a FUNCTION for {@link PaneHeaderProps.actions} is handed
 * the pixels the row can spare for group 3 and decides for itself how many
 * controls that buys; `PriorityActions` is the one that does. The measuring
 * lives here because only the row knows what the two groups before it have
 * taken, and the deciding lives there because only the surface knows which of
 * its verbs matters most.
 */
import { type ReactNode, useLayoutEffect, useRef, useState } from "react";
import { cn } from "@/lib/utils";

/** The group that absorbs every pixel of slack in the row. */
export const PANE_HEADER_IDENTITY_SLOT = "pane-header-identity";

/** The reserved, measured, unsqueezable middle. */
export const PANE_HEADER_STATUS_SLOT = "pane-header-status";

/** The controls, last, and the only group allowed to be squeezed. */
export const PANE_HEADER_ACTIONS_SLOT = "pane-header-actions";

/**
 * The gap between two adjacent members of the row, in pixels.
 *
 * `gap-2` as a number, because the actions group's overflow arithmetic has to
 * spend it and a Tailwind class cannot be read from JavaScript. The class on
 * the row below and this constant are one fact written twice, and they MUST
 * change together.
 */
export const PANE_HEADER_GAP_PX = 8;

/**
 * Pixels the identity group keeps, whatever else in the row wants them.
 *
 * Group 1's basis is zero, so left to the flexbox alone it would surrender
 * every pixel an action control asked for, and the pane would end up saying
 * what can be done to a note without saying which note. This is a reservation
 * and not a breakpoint: nothing asks "is the window narrower than N", it asks
 * "once the title has this much, what is left". Roughly twenty characters at
 * the header's 13px type — a title recognisable, not a sentence readable.
 */
export const PANE_HEADER_IDENTITY_MIN_PX = 160;

/**
 * Pixels group 3 may occupy: the row, less what the groups before it are owed,
 * less the gaps between them.
 *
 * Pure, because it is the half of the measurement with a decision in it.
 * `header` is the row's CONTENT width — its padding already gone, which is
 * what a `ResizeObserver` entry reports — and `status` is the reserved slot's
 * whole box, or null for a row that has no status group and therefore no gap
 * for one either. Never negative: "no room" is a smaller and truer claim than
 * "minus forty pixels of room".
 */
export function paneHeaderActionsBudget({
  header,
  status,
  identityMin = PANE_HEADER_IDENTITY_MIN_PX,
  gap = PANE_HEADER_GAP_PX,
}: {
  header: number;
  status: number | null;
  identityMin?: number;
  gap?: number;
}): number {
  if (!Number.isFinite(header)) {
    return 0;
  }
  const owed = identityMin + gap + (status === null || !Number.isFinite(status) ? 0 : status + gap);
  return Math.max(0, header - owed);
}

export interface PaneHeaderStatus {
  /**
   * Every string this slot can ever show, so the browser can measure them.
   *
   * Produce them from the same function that produces {@link caption} rather
   * than writing them out, so a change to the wording cannot change what is
   * shown without also changing what is reserved. A caption that cannot be
   * enumerated — a message composed in Rust — is deliberately absent from this
   * list: it is taken out of flow instead and ellipsised.
   *
   * Empty is legal and means the slot reserves nothing: correct for a status
   * whose only non-empty state is unbounded.
   */
  readonly sizers: readonly string[];
  /** The one string on screen right now. `""` renders an empty slot, which is
   *  the state a caption that would only be noise should be in. */
  readonly caption: string;
}

export interface PaneHeaderProps {
  /** The name of the thing this pane is showing, and anything that qualifies
   *  it. The only content in the row whose width is free to change. */
  identity: ReactNode;
  /** What the pane wants to say about itself, or `null` when it has nothing to
   *  say — see the module doc for why that is not the same as `caption: ""`. */
  status?: PaneHeaderStatus | null;
  /**
   * The controls. May themselves appear and disappear; that is what the group
   * is for.
   *
   * **A function instead, for a group that manages its own overflow.** It is
   * handed the pixels the row can spare for it (see
   * {@link paneHeaderActionsBudget}) and is then expected not to exceed them,
   * which is why that form also makes the group `shrink-0`. 46.4 left this
   * group squeezable on the ruling that a squeezed cluster beats a control
   * pushed off the edge; a group that drops its own last item needs neither,
   * and a control squeezed until its word is clipped is worse than one fewer
   * control. The node form keeps 46.4's behaviour exactly.
   */
  actions: ReactNode | ((budget: number) => ReactNode);
  /** The header element's own padding and border, which differ per surface. */
  className?: string;
}

export function PaneHeader({
  identity,
  status = null,
  actions,
  className,
}: PaneHeaderProps): React.ReactElement {
  const rowRef = useRef<HTMLElement>(null);
  const statusRef = useRef<HTMLSpanElement>(null);
  const managed = typeof actions === "function";
  const [budget, setBudget] = useState(0);

  // Zero until the row has been observed, and that is the safe direction: a
  // group with no budget renders the 560px shape, so the worst a missing
  // observation can do is leave the header as 46.5 shipped it. The observer is
  // the only source of the number — measuring once on mount as well would be a
  // second path to the same fact, and the callback arrives before the first
  // paint anyway.
  useLayoutEffect(() => {
    const row = rowRef.current;
    if (!managed || row === null || typeof ResizeObserver === "undefined") {
      return;
    }
    const observer = new ResizeObserver((entries) => {
      const entry = entries[entries.length - 1];
      if (entry === undefined) {
        return;
      }
      // The status group is read from the DOM rather than observed: its width
      // is a constant by construction (that is what group 2 IS), so the only
      // moment it can change is one where the row changed too.
      setBudget(
        paneHeaderActionsBudget({
          header: entry.contentRect.width,
          status: statusRef.current?.getBoundingClientRect().width ?? null,
        }),
      );
    });
    observer.observe(row);
    return () => observer.disconnect();
  }, [managed]);

  return (
    <header ref={rowRef} className={cn("flex shrink-0 items-center gap-2", className)}>
      {/* Group 1 — identity. `flex-1` off a zero basis: its width is whatever
          the row has left over, and it contributes nothing to the row's own
          content width. */}
      <div data-slot={PANE_HEADER_IDENTITY_SLOT} className="flex min-w-0 flex-1 items-center gap-2">
        {identity}
      </div>
      {status === null ? null : (
        /* Group 2 — status. One box for every caption: the sizers are invisible
           and are what set its width, and the word itself is out of flow and so
           cannot change it, however long a sentence a failure hands back. */
        <span
          ref={statusRef}
          data-slot={PANE_HEADER_STATUS_SLOT}
          className="relative grid shrink-0 justify-items-end text-[11px] text-muted-foreground tabular-nums"
        >
          {status.sizers.map((sizer) => (
            <span
              key={sizer}
              aria-hidden="true"
              className="invisible col-start-1 row-start-1 whitespace-nowrap"
            >
              {sizer}
            </span>
          ))}
          <span
            className="absolute inset-0 truncate text-right"
            title={status.caption === "" ? undefined : status.caption}
          >
            {status.caption}
          </span>
        </span>
      )}
      {/* Group 3 — actions. Squeezable when it is a node, because 46.4 ruled
          that squeezing beats pushing the last control off the edge; `shrink-0`
          when it manages its own overflow, because such a group never asks for
          more than the budget above and would rather drop a control than clip
          one. */}
      <div
        data-slot={PANE_HEADER_ACTIONS_SLOT}
        className={cn("flex items-center gap-2", managed && "shrink-0")}
      >
        {typeof actions === "function" ? actions(budget) : actions}
      </div>
    </header>
  );
}
