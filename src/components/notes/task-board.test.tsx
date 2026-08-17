import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BOARD_MOVE_LABEL, type BoardCard, TaskBoard } from "@/components/notes/task-board";

function card(over: Partial<BoardCard> & Pick<BoardCard, "title">): BoardCard {
  return {
    key: `${over.title.toLowerCase().replace(/\W+/g, "-")}.md`,
    status: "todo",
    order: 1,
    orderIsOwn: true,
    tags: ["task"],
    unstableIdentity: false,
    ...over,
  };
}

/** One card per column, plus a second in "to do" so a position can be dropped into. */
function cards(): BoardCard[] {
  return [
    card({ title: "Draft the plan", status: "in-preparation", order: 1 }),
    card({ title: "Write the board", status: "todo", order: 1 }),
    card({ title: "Wire the IPC", status: "todo", order: 2 }),
  ];
}

function mount(over: Partial<React.ComponentProps<typeof TaskBoard>> = {}) {
  const onOpen = vi.fn();
  const onMove = vi.fn(async () => {});
  const result = render(
    <TaskBoard
      heading="Tasks"
      cards={cards()}
      empty="No tasks yet."
      onOpen={onOpen}
      onMove={onMove}
      {...over}
    />,
  );
  return { ...result, onOpen, onMove };
}

/**
 * Lay the board out, because jsdom does not.
 *
 * Four column boxes 200 px wide side by side, each 300 px tall, each column's
 * cards 30 px tall stacked from y=40 — so y<40 is the header and y>40+30n is the
 * empty space below the last card. Every rect in jsdom is zero and
 * `document.elementFromPoint` does not exist at all, so the geometry the board
 * hit-tests against is the one thing a test has to supply. Same shape as
 * `pins-strip.test.tsx`'s `mockPinSlots`.
 */
const COLUMN_W = 200;
const CARD_TOP = 40;
const CARD_H = 30;

/** A DOMRect, spelled once rather than twice at nine fields each. */
function rect(left: number, top: number, width: number, height: number): DOMRect {
  return {
    width,
    height,
    top,
    left,
    right: left + width,
    bottom: top + height,
    x: left,
    y: top,
    toJSON: () => ({}),
  } as DOMRect;
}

function layout() {
  let column = 0;
  for (const box of document.querySelectorAll<HTMLElement>("[data-board-column]")) {
    const left = column * COLUMN_W;
    column += 1;
    box.getBoundingClientRect = () => rect(left, 0, COLUMN_W, 300);
    let at = 0;
    for (const card of box.querySelectorAll<HTMLElement>("[data-card-key]")) {
      const top = CARD_TOP + at * CARD_H;
      card.getBoundingClientRect = () => rect(left, top, COLUMN_W, CARD_H);
      at += 1;
    }
  }
}

/** The x that lands in each column, in the order `BOARD_COLUMNS` renders them. */
const X = { prep: 100, todo: 300, done: 500, nowhere: 900 };

/**
 * Interesting y positions inside a column: above every card's midpoint (the
 * header), between the first and second card's midpoints, and far below the last
 * card (the empty space the dead `<ul>` never covered).
 */
const Y = { header: 10, afterFirst: 80, empty: 250 };

/** Press `on`, move the pointer to (x, y), release it there. */
function dragTo(on: HTMLElement, x: number, y: number) {
  fireEvent.pointerDown(on, { pointerId: 1, button: 0, clientX: 0, clientY: 45 });
  fireEvent.pointerMove(on, { pointerId: 1, clientX: x, clientY: y });
  fireEvent.pointerUp(on, { pointerId: 1, clientX: x, clientY: y });
}

/** The card element for a title — the `li`, which is what carries the handlers. */
function cardOf(title: string): HTMLElement {
  const li = screen.getByRole("button", { name: title }).closest("li");
  if (li === null) {
    throw new Error(`no card for ${title}`);
  }
  return li;
}

/** Which columns are drawing a drop cue, by status. */
function cued(): string[] {
  return Array.from(document.querySelectorAll<HTMLElement>("[data-board-column]"))
    .filter((box) => box.className.includes("border-dashed"))
    .map((box) => box.dataset.boardColumn ?? "");
}

/** The card's column menu — present on every card, revealed rather than drawn. */
function menuOf(title: string): HTMLSelectElement {
  return screen.getByLabelText(`${BOARD_MOVE_LABEL} — ${title}`) as HTMLSelectElement;
}

afterEach(() => {
  vi.clearAllMocks();
});

describe("TaskBoard pointer gesture", () => {
  it("moves the card to the column the release landed in", async () => {
    const { onMove } = mount();
    layout();
    // Released between "to do"'s two cards: one midpoint above the pointer, so
    // the slot is 1 — the position the old per-card `onDrop` had to be dropped
    // exactly onto a card to produce.
    dragTo(cardOf("Draft the plan"), X.todo, Y.afterFirst);
    await waitFor(() => expect(onMove).toHaveBeenCalledWith("draft-the-plan.md", "todo", 1));
  });

  it("accepts a release over a column's empty space below the last card", async () => {
    // The case the dead `<ul>` broke: below `min-h-16` nothing was a target,
    // while the column box drew the highlight as if everything were.
    const { onMove } = mount();
    layout();
    dragTo(cardOf("Draft the plan"), X.todo, Y.empty);
    await waitFor(() => expect(onMove).toHaveBeenCalledWith("draft-the-plan.md", "todo", 2));
  });

  it("accepts a release over the column header, at the top of that column", async () => {
    const { onMove } = mount();
    layout();
    dragTo(cardOf("Draft the plan"), X.todo, Y.header);
    await waitFor(() => expect(onMove).toHaveBeenCalledWith("draft-the-plan.md", "todo", 0));
  });

  it("loses the vacated slot when a card is dragged down inside its own column", async () => {
    const { onMove } = mount();
    layout();
    // Rendered slot 2 (below both midpoints), but the column Rust resolves
    // against has this card removed — the classic off-by-one.
    dragTo(cardOf("Write the board"), X.todo, Y.empty);
    await waitFor(() => expect(onMove).toHaveBeenCalledWith("write-the-board.md", "todo", 1));
  });

  it("is one handle: the press may begin on the card's title", async () => {
    const { onMove, onOpen } = mount();
    layout();
    // The title is a `button`, which under HTML5 was a hole in the drag and had
    // to be marked `draggable` itself. A press bubbles to the card, and the
    // click that follows a moved press is swallowed, so the file does not also
    // open behind the move.
    dragTo(screen.getByRole("button", { name: "Draft the plan" }), X.todo, Y.empty);
    await waitFor(() => expect(onMove).toHaveBeenCalledWith("draft-the-plan.md", "todo", 2));
    fireEvent.click(screen.getByRole("button", { name: "Draft the plan" }));
    expect(onOpen).not.toHaveBeenCalled();
  });

  it("keeps a press that does not move a click: the title still opens the file", () => {
    const { onMove, onOpen } = mount();
    layout();
    const title = screen.getByRole("button", { name: "Wire the IPC" });
    fireEvent.pointerDown(title, { pointerId: 1, button: 0, clientX: 300, clientY: 75 });
    // Under the slop: a hand is not perfectly still, and this must stay a click.
    fireEvent.pointerMove(title, { pointerId: 1, clientX: 303, clientY: 77 });
    fireEvent.pointerUp(title, { pointerId: 1, clientX: 303, clientY: 77 });
    fireEvent.click(title);
    expect(onMove).not.toHaveBeenCalled();
    expect(onOpen).toHaveBeenCalledWith("wire-the-ipc.md");
  });

  it("draws the drop cue only on the column a release would be accepted by", () => {
    mount();
    layout();
    const card = cardOf("Draft the plan");
    // Nothing is cued before the press has travelled: a click draws no cue.
    fireEvent.pointerDown(card, { pointerId: 1, button: 0, clientX: 0, clientY: 45 });
    expect(cued()).toEqual([]);
    fireEvent.pointerMove(card, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    expect(cued()).toEqual(["todo"]);
    fireEvent.pointerMove(card, { pointerId: 1, clientX: X.done, clientY: Y.header });
    expect(cued()).toEqual(["done"]);
    // Off every column: no region would accept the release, so no cue claims it.
    fireEvent.pointerMove(card, { pointerId: 1, clientX: X.nowhere, clientY: Y.empty });
    expect(cued()).toEqual([]);
    fireEvent.pointerUp(card, { pointerId: 1, clientX: X.nowhere, clientY: Y.empty });
  });

  it("moves nothing when the release lands outside every column", () => {
    const { onMove } = mount();
    layout();
    dragTo(cardOf("Draft the plan"), X.nowhere, Y.empty);
    expect(onMove).not.toHaveBeenCalled();
  });

  it("returns the card and says nothing when the gesture is cancelled", () => {
    const { onMove } = mount();
    layout();
    const card = cardOf("Draft the plan");
    fireEvent.pointerDown(card, { pointerId: 1, button: 0, clientX: 0, clientY: 45 });
    fireEvent.pointerMove(card, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    fireEvent.pointerCancel(card, { pointerId: 1 });
    expect(onMove).not.toHaveBeenCalled();
    expect(cued()).toEqual([]);
    // Still where its own `status:` puts it — the board holds no order of its own.
    const prep = screen.getByRole("list", { name: "In preparation" });
    expect(within(prep).getByRole("button", { name: "Draft the plan" })).toBeInTheDocument();
  });

  it("leaves the column menu's own press alone", () => {
    const { onMove } = mount();
    layout();
    const menu = menuOf("Draft the plan");
    fireEvent.pointerDown(menu, { pointerId: 1, button: 0, clientX: 0, clientY: 45 });
    fireEvent.pointerMove(menu, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    fireEvent.pointerUp(menu, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    // A press that opens the menu is not a drag of the card behind it.
    expect(onMove).not.toHaveBeenCalled();
  });

  it("starts no gesture from a secondary button", () => {
    const { onMove } = mount();
    layout();
    const card = cardOf("Draft the plan");
    fireEvent.pointerDown(card, { pointerId: 1, button: 2, clientX: 0, clientY: 45 });
    fireEvent.pointerMove(card, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    fireEvent.pointerUp(card, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    expect(onMove).not.toHaveBeenCalled();
  });

  it("says keeper's refusal and leaves the card where its file says", async () => {
    const onMove = vi.fn(async () => {
      throw { message: "That card is not on this board any more." };
    });
    mount({ onMove });
    layout();
    dragTo(cardOf("Draft the plan"), X.todo, Y.empty);
    expect(await screen.findByRole("status")).toHaveTextContent(
      "That card is not on this board any more.",
    );
    const prep = screen.getByRole("list", { name: "In preparation" });
    expect(within(prep).getByRole("button", { name: "Draft the plan" })).toBeInTheDocument();
  });

  it("carries no HTML5 drag anywhere in the board", () => {
    // The mechanism that cannot work under Tauri on macOS is gone, not parked
    // beside the one that can: a second mechanism for one verb is how the dead
    // one survived two epics.
    const { container } = mount({
      cards: [...cards(), card({ title: "Blocked on review", status: "blocked" })],
    });
    expect(container.querySelectorAll("[draggable]")).toHaveLength(0);
  });
});

describe("TaskBoard column menu", () => {
  it("keeps a menu on every card, at all times", () => {
    mount();
    // Not a hover-mounted control: it is in the DOM for each card whether or not
    // a pointer is anywhere near it, because it is the keyboard path.
    for (const title of ["Draft the plan", "Write the board", "Wire the IPC"]) {
      expect(menuOf(title)).toBeInTheDocument();
    }
  });

  it("reveals the menu on hover and on focus rather than drawing it always", () => {
    mount();
    const menu = menuOf("Draft the plan");
    // `opacity-0` and not `hidden`: opacity leaves the control in the tab order
    // and in the accessibility tree, so a keyboard user tabbing to it both
    // reveals it and hears it. `hidden` would remove the only keyboard path.
    expect(menu.className).toContain("opacity-0");
    expect(menu.className).toContain("group-hover:opacity-100");
    expect(menu.className).toContain("focus-within:opacity-100");
    // Which needs the card to be the hover group.
    expect(cardOf("Draft the plan").className).toContain("group");
  });

  it("leaves the menu reachable by keyboard", () => {
    mount();
    const menu = menuOf("Draft the plan");
    expect(menu).not.toBeDisabled();
    expect(menu).not.toHaveAttribute("tabindex", "-1");
    expect(menu).not.toHaveAttribute("aria-hidden");
    menu.focus();
    expect(menu).toHaveFocus();
  });

  it("moves a card without a pointer, to the end of the column it joins", async () => {
    const { onMove } = mount();
    fireEvent.change(menuOf("Draft the plan"), { target: { value: "todo" } });
    // "to do" holds two cards, so the end of it is index 2 — the same write the
    // drag makes, at the position nobody named.
    await waitFor(() => expect(onMove).toHaveBeenCalledWith("draft-the-plan.md", "todo", 2));
  });

  it("offers a card's own word when it is not one of the four", () => {
    mount({ cards: [card({ title: "Blocked on review", status: "blocked" })] });
    const menu = menuOf("Blocked on review");
    expect(menu.value).toBe("blocked");
    expect(within(menu).getByRole("option", { name: "blocked" })).toBeInTheDocument();
  });
});
