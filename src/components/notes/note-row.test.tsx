import { render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import {
  formatNoteOrder,
  NOTE_ORDER_UNREADABLE_MARK,
  NoteRow,
  noteOrderLabel,
} from "@/components/notes/note-row";
import type { NoteOrder, NoteRowVm } from "@/lib/ipc/client";

/**
 * Story 44.5 — the row's side of "a note has an order you can see".
 *
 * The whole story is that a number the reader cannot account for reads as
 * randomness, so what is asserted here is the account: the number is drawn, it is
 * the note's own value rather than a rounded or renumbered one, and every one of
 * the three provenances — the note said so, the note was silent, the note said
 * something that is not a number — is a different sentence in the accessible
 * name. A row that drew `0` for all three would pass a laxer test and would be
 * the defect.
 */

function row(order: NoteOrder, overrides: Partial<NoteRowVm> = {}): NoteRowVm {
  return {
    id: "n1",
    path: "notes/n1.md",
    title: "A note",
    snippet: "the body excerpt",
    tags: [],
    updatedMs: Date.now() - 3_600_000,
    pinned: false,
    archived: false,
    unread: false,
    conflict: false,
    origin: "",
    headRev: "",
    order,
    ...overrides,
  };
}

function renderRow(order: NoteOrder) {
  const { container } = render(
    <NoteRow
      row={row(order)}
      selected={false}
      tabIndex={0}
      onSelect={vi.fn()}
      onSelectBeside={vi.fn()}
      onToggleTag={vi.fn()}
    />,
  );
  const cell = container.querySelector('[data-slot="note-order"]');
  if (cell === null) {
    throw new Error("the row drew no order at all");
  }
  return cell;
}

describe("NoteRow — the order beside the note", () => {
  it("draws the default for a note that has never had an order, and says it is the default", () => {
    const cell = renderRow({ value: 0, source: "default" });

    expect(cell).toHaveTextContent("0");
    expect(cell).toHaveAttribute("data-order-source", "default");
    // Named, not merely drawn: `aria-label` on the row overrides its contents, so
    // an order that is only painted is an order a screen reader user never gets.
    expect(screen.getByRole("button", { name: /order 0, the default/ })).toBeInTheDocument();
  });

  it("draws the note's own order, including a fraction and a negative, unrounded", () => {
    // The reason the value is not an integer: 1.5 is how a person slots a note
    // between 1 and 2, and a row that rendered `2` would be lying about the file.
    expect(renderRow({ value: 1.5, source: "own" })).toHaveTextContent("1.5");
    expect(renderRow({ value: -1, source: "own" })).toHaveTextContent("-1");
    expect(renderRow({ value: 12, source: "own" })).toHaveTextContent("12");
  });

  it("distinguishes an order the note stated from the default it was given", () => {
    const own = renderRow({ value: 0, source: "own" });

    // Same number, different fact: `order: 0` in the file is a placement, and an
    // absent key is not. Without the distinction a deliberately-zeroed note is
    // indistinguishable from every silent one.
    expect(own).toHaveAttribute("data-order-source", "own");
    expect(screen.getByRole("button", { name: /order 0$/ })).toBeInTheDocument();
  });

  it("falls back visibly when the note's own order is not a number", () => {
    const cell = renderRow({ value: 0, source: "unreadable" });

    // A bare `0` here would be the silent fallback this story forbids: the list
    // would be totally ordered and would quietly disagree with the file.
    expect(cell).toHaveTextContent(`0${NOTE_ORDER_UNREADABLE_MARK}`);
    expect(cell).toHaveAttribute("data-order-source", "unreadable");
    expect(
      screen.getByRole("button", { name: /this note's own order is not a number/ }),
    ).toBeInTheDocument();
  });

  it("carries the mark in text rather than in colour alone", () => {
    // UX-DR43: the destructive tint is not a carrier on a monochrome panel, so
    // the unreadable case has to differ in what it SAYS, not only in how it looks.
    expect(formatNoteOrder({ value: 0, source: "unreadable" })).not.toEqual(
      formatNoteOrder({ value: 0, source: "default" }),
    );
    expect(noteOrderLabel({ value: 0, source: "unreadable" })).not.toEqual(
      noteOrderLabel({ value: 0, source: "default" }),
    );
    expect(noteOrderLabel({ value: 0, source: "default" })).not.toEqual(
      noteOrderLabel({ value: 0, source: "own" }),
    );
  });
});

/**
 * How a row boundary is drawn in this list, and — the load-bearing half — how it
 * is deliberately NOT drawn.
 *
 * The owner reported missing borders and the notes list was the candidate,
 * because it is the one list in keeper with no row rule. It is also the one list
 * with no row ANCHOR: chat rows repeat an avatar and a full-height account bar,
 * recording rows are enclosed cards, tree rows repeat a file icon, and this row's
 * only mark in that lane went `bg-transparent` the moment a note was read. So the
 * fix is the anchor, not a hairline — a rule here would make this the only ruled
 * list in the app, which is heavier than the app rather than more legible.
 *
 * The negative assertion is the point of the file: it is what stops the next
 * person reading "no separators" and adding one.
 */
function renderNoteRow(overrides: Partial<NoteRowVm> = {}): HTMLElement {
  const { container } = render(
    <NoteRow
      row={row({ value: 0, source: "default" }, overrides)}
      selected={false}
      tabIndex={0}
      onSelect={vi.fn()}
      onSelectBeside={vi.fn()}
      onToggleTag={vi.fn()}
    />,
  );
  const found = container.querySelector<HTMLElement>('[data-slot="note-row"]');
  if (found === null) {
    throw new Error("the row drew no row");
  }
  return found;
}

function unreadDot(rowElement: HTMLElement): HTMLElement {
  const found = rowElement.querySelector<HTMLElement>('[data-slot="unread-dot"]');
  if (found === null) {
    throw new Error("the row drew no unread dot");
  }
  return found;
}

describe("NoteRow — where one row stops and the next begins", () => {
  it("keeps a mark in the anchor lane after a note has been read", () => {
    const read = unreadDot(renderNoteRow({ unread: false }));

    // A dot that vanishes leaves a list of read notes — the common case — with
    // nothing repeating down its left edge and no rhythm to read a boundary
    // from. Hollow is still a mark.
    expect(read.className).not.toContain("bg-transparent");
    expect(read).toHaveClass("border");
    expect(read).toHaveClass("rounded-full");
  });

  it("tells read from unread by fill and not by the presence of the dot", () => {
    const unread = unreadDot(renderNoteRow({ unread: true })).className;
    const read = unreadDot(renderNoteRow({ unread: false })).className;

    // DESIGN.md's grammar: filled and hollow, never a bare dot carrying its one
    // state in colour alone.
    expect(unread).not.toEqual(read);
    expect(unread).toContain("bg-primary");
  });

  it("centres its content so the gap above a row equals the gap below it", () => {
    // `items-start` pooled every pixel of a 64px row's slack underneath its
    // text, so the boundary between two rows sat nowhere in particular. This is
    // the chat list's construction, which is the list this row was built to
    // match on density in the first place.
    expect(renderNoteRow()).toHaveClass("items-center");
  });

  it("draws no row rule, on purpose", () => {
    const rowElement = renderNoteRow();

    // Not an omission — a decision, pinned here so it is not quietly reversed.
    // No list in this app rules its rows. The left edge a conflicted row grows
    // is a status mark and not a separator, which is why it is asserted apart.
    expect(rowElement.className).not.toContain("border-b");
    expect(rowElement.className).not.toContain("border-t");
    expect(unreadDot(renderNoteRow({ conflict: true }))).toBeInTheDocument();
    expect(renderNoteRow({ conflict: true })).toHaveClass("border-l-[3px]");
  });
});
