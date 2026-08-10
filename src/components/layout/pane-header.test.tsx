/**
 * The extracted header, and the guarantees it inherited (Story 46.13, AD-104).
 *
 * **What this file can and cannot prove is exactly what `note-editor.test.tsx`
 * said in 46.4, and the limit has not moved.** The defect these classes exist to
 * refuse is a layout shift, jsdom performs no layout, and `src/test/setup.ts`'s
 * rect shim answers a viewport only for zero-sized elements. A test asserting
 * "the last control did not move by N pixels" would be asserting the shim.
 *
 * So these tests assert the structural property that CAUSES the shift, against
 * the component that now owns it: whether the status element is a width-variable
 * participant in the same flex row as the controls. 46.4's six tests still assert
 * it through the note editor, which is the point of the extraction — the
 * consumer's own suite keeps proving the consumer's own header. This file proves
 * the three claims once, in isolation, for whoever consumes it third:
 *
 * 1. the row is groups, never controls-beside-a-caption;
 * 2. identity is the only grower and the status slot cannot be squeezed;
 * 3. an unbounded caption is out of flow, so it cannot widen the box.
 *
 * Plus the one thing 46.4 had no reason to consider: a header with no status at
 * all renders two groups rather than an empty reserved box, because a zero-width
 * slot in a `gap-2` row is 8px of space held for nothing.
 */
import { render, screen } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import {
  PANE_HEADER_ACTIONS_SLOT,
  PANE_HEADER_IDENTITY_SLOT,
  PANE_HEADER_STATUS_SLOT,
  PaneHeader,
} from "@/components/layout/pane-header";

/** The two captions this file measures a slot against. Two DIFFERENT widths, so
 *  a box that tracked its content would be caught changing. */
const SHORT = "Saving…";
const LONG = "Saved · 23:41";

function headerRow(): HTMLElement {
  const found = document.querySelector("header");
  if (found === null) {
    throw new Error("PaneHeader drew no header element");
  }
  return found;
}

function group(slot: string): HTMLElement {
  const found = headerRow().querySelector<HTMLElement>(`:scope > [data-slot="${slot}"]`);
  if (found === null) {
    throw new Error(`the header drew no ${slot} group`);
  }
  return found;
}

/** A group's box, order-insensitively: Tailwind class ORDER is the formatter's
 *  business, and what is claimed is that the SET does not change. */
function box(element: Element): string {
  return Array.from(element.classList).sort().join(" ");
}

/** The invisible strings that decide how wide the status slot is. */
function reserved(): string[] {
  return Array.from(group(PANE_HEADER_STATUS_SLOT).querySelectorAll(":scope > [aria-hidden]")).map(
    (sizer) => sizer.textContent ?? "",
  );
}

/** The one element on screen carrying the caption. */
function shownElement(): Element {
  const shown = group(PANE_HEADER_STATUS_SLOT).querySelector(":scope > :not([aria-hidden='true'])");
  if (shown === null) {
    throw new Error("the status slot rendered no caption element");
  }
  return shown;
}

describe("the row is three groups, not a caption among controls", () => {
  it("puts no control in the same shrink context as the status", () => {
    render(
      <PaneHeader
        identity={<span>a-file.md</span>}
        status={{ sizers: [SHORT, LONG], caption: SHORT }}
        actions={
          <>
            <button type="button">Save</button>
            <button type="button">Close</button>
          </>
        }
      />,
    );

    const row = headerRow();
    // The property that failed before 46.4: with a caption, a title and five
    // buttons all direct children of one non-wrapping flex row, the width the
    // caption gained on a save came out of the buttons beside it.
    expect(row.children).toHaveLength(3);
    expect(Array.from(row.children).filter((child) => child.tagName === "BUTTON")).toHaveLength(0);
    expect(Array.from(row.children).map((child) => child.getAttribute("data-slot"))).toEqual([
      PANE_HEADER_IDENTITY_SLOT,
      PANE_HEADER_STATUS_SLOT,
      PANE_HEADER_ACTIONS_SLOT,
    ]);
    // And the controls really are inside the last group rather than merely
    // absent from the row — otherwise the assertion above would hold for a
    // header that rendered no actions at all.
    expect(group(PANE_HEADER_ACTIONS_SLOT).querySelectorAll("button")).toHaveLength(2);
  });

  it("gives the slack to identity and to nothing else", () => {
    render(
      <PaneHeader
        identity={<span>a-file.md</span>}
        status={{ sizers: [SHORT], caption: SHORT }}
        actions={<button type="button">Save</button>}
      />,
    );

    const row = headerRow();
    expect(Array.from(row.children).filter((child) => child.classList.contains("flex-1"))).toEqual([
      group(PANE_HEADER_IDENTITY_SLOT),
    ]);
    expect(group(PANE_HEADER_IDENTITY_SLOT)).toHaveClass("min-w-0");
    // A slot that can be squeezed is not a slot.
    expect(group(PANE_HEADER_STATUS_SLOT)).toHaveClass("shrink-0");
    // And the actions are deliberately NOT `shrink-0` — 46.4's corrected ruling.
    // If they outgrow the window the row must squeeze them rather than push the
    // last one off the right-hand edge, which is how 46.5's defect happened.
    expect(group(PANE_HEADER_ACTIONS_SLOT)).not.toHaveClass("shrink-0");
  });

  it("renders two groups, not an empty box, for a header with nothing to report", () => {
    render(
      <PaneHeader identity={<span>a-file.md</span>} actions={<button type="button">X</button>} />,
    );

    const row = headerRow();
    expect(row.children).toHaveLength(2);
    expect(row.querySelector(`[data-slot="${PANE_HEADER_STATUS_SLOT}"]`)).toBeNull();
    // The rest of the rule still holds without a status: identity grows, the
    // actions sit where the row's edge puts them.
    expect(Array.from(row.children).filter((child) => child.classList.contains("flex-1"))).toEqual([
      group(PANE_HEADER_IDENTITY_SLOT),
    ]);
  });
});

describe("the status is a box before it is a word", () => {
  it("keeps the same box, and the same reservation, as the caption changes", () => {
    const { rerender } = render(
      <PaneHeader
        identity={<span>a-file.md</span>}
        status={{ sizers: [SHORT, LONG], caption: "" }}
        actions={<button type="button">Save</button>}
      />,
    );
    const emptyBox = box(group(PANE_HEADER_STATUS_SLOT));
    const emptyReservation = reserved();

    rerender(
      <PaneHeader
        identity={<span>a-file.md</span>}
        status={{ sizers: [SHORT, LONG], caption: LONG }}
        actions={<button type="button">Save</button>}
      />,
    );

    expect(shownElement().textContent).toBe(LONG);
    expect(box(group(PANE_HEADER_STATUS_SLOT))).toBe(emptyBox);
    expect(reserved()).toEqual(emptyReservation);
  });

  it("keeps the same box while the group beside it changes width", () => {
    // Identity's contents change constantly — a title derived from a buffer moves
    // on a keystroke. That movement must not reach the status box, and the box
    // must not have been sized off it.
    const { rerender } = render(
      <PaneHeader
        identity={<span>a.md</span>}
        status={{ sizers: [SHORT, LONG], caption: SHORT }}
        actions={<button type="button">Save</button>}
      />,
    );
    const before = box(group(PANE_HEADER_STATUS_SLOT));

    rerender(
      <PaneHeader
        identity={<span>{"a rather long heading ".repeat(12)}</span>}
        status={{ sizers: [SHORT, LONG], caption: SHORT }}
        actions={<button type="button">Save</button>}
      />,
    );

    expect(box(group(PANE_HEADER_STATUS_SLOT))).toBe(before);
  });

  it("reserves exactly the strings it was given, invisibly and unspeakably", () => {
    render(
      <PaneHeader
        identity={<span>a.md</span>}
        status={{ sizers: [SHORT, LONG], caption: SHORT }}
        actions={<button type="button">Save</button>}
      />,
    );

    // Rendered, not described: these are the strings the browser measures. They
    // are `invisible` rather than `hidden` because a `display: none` element has
    // no width to contribute, which is the whole job.
    expect(reserved()).toEqual([SHORT, LONG]);
    for (const sizer of Array.from(
      group(PANE_HEADER_STATUS_SLOT).querySelectorAll(":scope > [aria-hidden]"),
    )) {
      expect(sizer).toHaveClass("invisible");
    }
    // And they are said once, not twice: a reader must not hear the caption
    // three times because the slot reserved for three.
    expect(screen.getAllByText(SHORT)).toHaveLength(2);
  });

  it("cannot be widened by a caption nobody could reserve for", () => {
    const REFUSED =
      "the vault is read-only and the write was refused: /Volumes/profile-1/notes/inbox/meeting.md";
    const { rerender } = render(
      <PaneHeader
        identity={<span>a.md</span>}
        status={{ sizers: [SHORT, LONG], caption: SHORT }}
        actions={<button type="button">Save</button>}
      />,
    );
    const before = box(group(PANE_HEADER_STATUS_SLOT));
    const reservationBefore = reserved();

    rerender(
      <PaneHeader
        identity={<span>a.md</span>}
        status={{ sizers: [SHORT, LONG], caption: REFUSED }}
        actions={<button type="button">Save</button>}
      />,
    );

    // A message composed in Rust is unbounded, so it is the one caption that
    // cannot be reserved for. It is taken out of flow instead — and the box is
    // what everything to its right is standing on.
    expect(shownElement()).toHaveClass("absolute");
    expect(box(group(PANE_HEADER_STATUS_SLOT))).toBe(before);
    expect(reserved()).toEqual(reservationBefore);
    // Ellipsised on screen is not thrown away: the whole sentence stays in the
    // DOM for a screen reader and on `title` for a pointer.
    expect(shownElement()).toHaveClass("truncate");
    expect(shownElement()).toHaveAttribute("title", REFUSED);
  });

  it("hangs no title on an empty caption", () => {
    render(
      <PaneHeader
        identity={<span>a.md</span>}
        status={{ sizers: [SHORT], caption: "" }}
        actions={<button type="button">Save</button>}
      />,
    );

    // An empty tooltip is a tooltip that flickers over a blank box on every
    // hover, which is noise for a state that is deliberately quiet.
    expect(shownElement()).not.toHaveAttribute("title");
  });
});
