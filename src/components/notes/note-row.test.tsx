import { createEvent, fireEvent, render, screen } from "@testing-library/react";
import { describe, expect, it, vi } from "vitest";
import { NOTE_DELETE_LABEL } from "@/components/notes/note-actions";
import {
  formatNoteOrder,
  NOTE_ORDER_UNREADABLE_MARK,
  NOTE_ROW_ARCHIVE_LABEL,
  NOTE_ROW_MARK_READ_LABEL,
  NOTE_ROW_OPEN_BESIDE_LABEL,
  NOTE_ROW_OPEN_HERE_LABEL,
  NOTE_ROW_PIN_LABEL,
  NOTE_ROW_REVEAL_LABEL,
  NOTE_ROW_UNARCHIVE_LABEL,
  NOTE_ROW_UNPIN_LABEL,
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
    predicates: [],
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
      canReveal={true}
      onSelect={vi.fn()}
      onSelectBeside={vi.fn()}
      onToggleTag={vi.fn()}
      onVerb={vi.fn()}
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
      canReveal={true}
      onSelect={vi.fn()}
      onSelectBeside={vi.fn()}
      onToggleTag={vi.fn()}
      onVerb={vi.fn()}
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

/**
 * The row's context menu — the owner's report against 0.8.6: a right-click on a
 * note gave the macOS WebView's own menu for selected text (Look Up, Translate,
 * Search with Google), which is a menu about a text selection and knows nothing
 * about the note under the pointer.
 *
 * Two things are asserted, and the first is the one that is easy to forget: the
 * native menu is suppressed. A `contextmenu` event that is not
 * `preventDefault()`ed is the WebView's, whatever else this row draws — so the
 * app menu appearing is only half the fix, and a regression that stopped
 * cancelling the event would leave both menus fighting over the same click.
 *
 * The second is that the items ACT. A menu of correct words wired to nothing is
 * the defect this whole file exists to catch, so every item is pressed and the
 * verb it dispatches is checked against the one the list's keyboard already
 * sends for the same act — `p`, `e`, `u`, `r`, `d`. One handler for both routes
 * is the property worth pinning: it is what stops a menu item and its keystroke
 * drifting apart.
 */
function renderForMenu(overrides: Partial<NoteRowVm> = {}, canReveal = true) {
  const onSelect = vi.fn();
  const onSelectBeside = vi.fn();
  const onVerb = vi.fn();
  const noteRow = row({ value: 0, source: "default" }, overrides);
  const { container } = render(
    <NoteRow
      row={noteRow}
      selected={false}
      tabIndex={0}
      canReveal={canReveal}
      onSelect={onSelect}
      onSelectBeside={onSelectBeside}
      onToggleTag={vi.fn()}
      onVerb={onVerb}
    />,
  );
  const button = container.querySelector<HTMLElement>('[data-slot="note-row"]');
  if (button === null) {
    throw new Error("the row drew no row");
  }
  return { button, noteRow, onSelect, onSelectBeside, onVerb };
}

/** Right-click the row; answers whether the native menu was cancelled. */
async function openMenu(button: HTMLElement): Promise<boolean> {
  const event = createEvent.contextMenu(button, { clientX: 12, clientY: 8 });
  fireEvent(button, event);
  await screen.findByRole("menu");
  return event.defaultPrevented;
}

describe("NoteRow — right-click gives the note's menu, not the WebView's", () => {
  it("cancels the native menu and opens the app's in its place", async () => {
    const { button } = renderForMenu();

    // Unprevented, the WebView draws Look Up / Translate / Search with Google
    // over the row. This assertion IS the owner's report.
    expect(await openMenu(button)).toBe(true);
    expect(screen.getByRole("menuitem", { name: NOTE_ROW_OPEN_HERE_LABEL })).toBeInTheDocument();
  });

  it("offers the Files tree's verbs, in the Files tree's order", async () => {
    const { button } = renderForMenu({ unread: true, headRev: "abc123" });
    await openMenu(button);

    // The panel pair first — the click and the double click the row already
    // answers to, named at last — then what changes the note, then the
    // destructive one alone at the bottom.
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      NOTE_ROW_OPEN_HERE_LABEL,
      NOTE_ROW_OPEN_BESIDE_LABEL,
      NOTE_ROW_MARK_READ_LABEL,
      NOTE_ROW_PIN_LABEL,
      NOTE_ROW_ARCHIVE_LABEL,
      NOTE_ROW_REVEAL_LABEL,
      NOTE_DELETE_LABEL,
    ]);
  });

  it("opens the note in this panel, and beside it, from the two panel items", async () => {
    const here = renderForMenu();
    await openMenu(here.button);
    fireEvent.click(screen.getByRole("menuitem", { name: NOTE_ROW_OPEN_HERE_LABEL }));
    expect(here.onSelect).toHaveBeenCalledWith(here.noteRow);
    expect(here.onSelectBeside).not.toHaveBeenCalled();

    const beside = renderForMenu();
    await openMenu(beside.button);
    fireEvent.click(screen.getByRole("menuitem", { name: NOTE_ROW_OPEN_BESIDE_LABEL }));
    expect(beside.onSelectBeside).toHaveBeenCalledWith(beside.noteRow);
  });

  it("names pin and archive for the state the row is in, and dispatches the list's verbs", async () => {
    const plain = renderForMenu({ pinned: false, archived: false });
    await openMenu(plain.button);
    expect(screen.queryByRole("menuitem", { name: NOTE_ROW_UNPIN_LABEL })).toBeNull();
    fireEvent.click(screen.getByRole("menuitem", { name: NOTE_ROW_PIN_LABEL }));
    // `p` — the same key the list binds, through the same handler, so the two
    // routes cannot come to mean different things.
    expect(plain.onVerb).toHaveBeenCalledWith(plain.noteRow, "p");

    const flagged = renderForMenu({ pinned: true, archived: true });
    await openMenu(flagged.button);
    expect(screen.queryByRole("menuitem", { name: NOTE_ROW_PIN_LABEL })).toBeNull();
    expect(screen.getByRole("menuitem", { name: NOTE_ROW_UNPIN_LABEL })).toBeInTheDocument();
    fireEvent.click(screen.getByRole("menuitem", { name: NOTE_ROW_UNARCHIVE_LABEL }));
    expect(flagged.onVerb).toHaveBeenCalledWith(flagged.noteRow, "e");
  });

  it("offers Mark read only where there is a revision to acknowledge", async () => {
    const unread = renderForMenu({ unread: true, headRev: "abc123" });
    await openMenu(unread.button);
    fireEvent.click(screen.getByRole("menuitem", { name: NOTE_ROW_MARK_READ_LABEL }));
    expect(unread.onVerb).toHaveBeenCalledWith(unread.noteRow, "u");

    // A read row has nothing to acknowledge and there is no mark-unread twin to
    // call, so the item is absent rather than drawn as a control that returns
    // without doing anything.
    const read = renderForMenu({ unread: false, headRev: "abc123" });
    await openMenu(read.button);
    expect(screen.queryByRole("menuitem", { name: NOTE_ROW_MARK_READ_LABEL })).toBeNull();

    // Nor for a note that has never been committed: no revision, no mark.
    const uncommitted = renderForMenu({ unread: true, headRev: "" });
    await openMenu(uncommitted.button);
    expect(screen.queryByRole("menuitem", { name: NOTE_ROW_MARK_READ_LABEL })).toBeNull();
  });

  it("reveals the note's real path, and offers nothing where there is no file manager", async () => {
    const withFinder = renderForMenu();
    await openMenu(withFinder.button);
    fireEvent.click(screen.getByRole("menuitem", { name: NOTE_ROW_REVEAL_LABEL }));
    expect(withFinder.onVerb).toHaveBeenCalledWith(withFinder.noteRow, "r");

    // Absent, never disabled: the rule the Files tree, the recordings browser
    // and the completion card all follow.
    const without = renderForMenu({}, false);
    await openMenu(without.button);
    expect(screen.queryByRole("menuitem", { name: NOTE_ROW_REVEAL_LABEL })).toBeNull();
  });

  it("asks before deleting, and puts the ask last and alone", async () => {
    const { button, noteRow, onVerb } = renderForMenu();
    await openMenu(button);

    const del = screen.getByRole("menuitem", { name: NOTE_DELETE_LABEL });
    // Marked destructive and last, which is the position `NoteActions` already
    // gives it: the item under the cursor when the menu opens is never the one
    // that removes the note.
    expect(del).toHaveAttribute("data-variant", "destructive");
    const items = screen.getAllByRole("menuitem");
    expect(items[items.length - 1].textContent).toBe(NOTE_DELETE_LABEL);

    fireEvent.click(del);
    // `d` ASKS. The row never deletes; the confirmation the list opens is the
    // only thing that does.
    expect(onVerb).toHaveBeenCalledWith(noteRow, "d");
  });

  it("does not open the note when the menu is what was asked for", async () => {
    const { button, onSelect } = renderForMenu();
    await openMenu(button);

    // A right-click is not a click: the row must not also swap the panel out
    // from under the menu that just opened.
    expect(onSelect).not.toHaveBeenCalled();
  });
});
