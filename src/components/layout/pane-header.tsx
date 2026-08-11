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
 */
import type { ReactNode } from "react";
import { cn } from "@/lib/utils";

/** The group that absorbs every pixel of slack in the row. */
export const PANE_HEADER_IDENTITY_SLOT = "pane-header-identity";

/** The reserved, measured, unsqueezable middle. */
export const PANE_HEADER_STATUS_SLOT = "pane-header-status";

/** The controls, last, and the only group allowed to be squeezed. */
export const PANE_HEADER_ACTIONS_SLOT = "pane-header-actions";

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
  /** The controls. May themselves appear and disappear; that is what the group
   *  is for. */
  actions: ReactNode;
  /** The header element's own padding and border, which differ per surface. */
  className?: string;
}

export function PaneHeader({
  identity,
  status = null,
  actions,
  className,
}: PaneHeaderProps): React.ReactElement {
  return (
    <header className={cn("flex shrink-0 items-center gap-2", className)}>
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
      {/* Group 3 — actions. NOT `shrink-0`: if these outgrow the window the row
          squeezes them rather than pushing the last one off the edge. Safe only
          because the group before it has a constant width. */}
      <div data-slot={PANE_HEADER_ACTIONS_SLOT} className="flex items-center gap-2">
        {actions}
      </div>
    </header>
  );
}
