import {
  act,
  createEvent,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { BOARD_MOVE_LABEL, type BoardCard, TaskBoard } from "@/components/notes/task-board";
import { DRAG_SELECTION_CLASS } from "@/hooks/use-pointer-drag";

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
  const props: React.ComponentProps<typeof TaskBoard> = {
    heading: "Tasks",
    cards: cards(),
    empty: "No tasks yet.",
    onOpen,
    onMove,
    ...over,
  };
  const result = render(<TaskBoard {...props} />);
  /** An external re-read: the same board, a new `cards` array. */
  const reread = (next: BoardCard[]) => result.rerender(<TaskBoard {...props} cards={next} />);
  return { ...result, onOpen, onMove, reread };
}

/**
 * Record the pointer captures taken on one element.
 *
 * An own property rather than `vi.spyOn`: `setPointerCapture` is inherited from
 * `Element.prototype`, where `src/test/setup.ts` stubs it once for the whole
 * suite, so a spy installed there is shared by every element and cannot say
 * *which* one took the capture — which is the whole question in these tests.
 */
function capturesOn(element: HTMLElement) {
  const taken = vi.fn();
  element.setPointerCapture = taken;
  return taken;
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
 *
 * Two things beyond the pre-54.1 fixture, both of which the board now measures:
 *
 * - Each card's rect MOVES with the `translate()` its own inline style carries,
 *   because `getBoundingClientRect` returns the TRANSFORMED border box. A frozen
 *   closure reported pre-follow geometry no matter what the card did, which is
 *   why every slot assertion in this suite stayed green while a card dragged to
 *   the bottom of its own column was written to the top.
 * - The board's own `<section>` gets a rect, because the follow is capped to it.
 *   Left to `src/test/setup.ts`'s viewport shim, the cap would be measured
 *   against 1024x768 rather than against the board.
 */
const COLUMN_W = 200;
const BOARD_H = 300;
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

/** `base`, moved by the `translate()` in `element`'s own inline style. */
function transformed(element: HTMLElement, base: DOMRect): DOMRect {
  const shift = /translate\((-?[\d.]+)px,\s*(-?[\d.]+)px\)/.exec(element.style.transform);
  if (shift === null) {
    return base;
  }
  return rect(base.left + Number(shift[1]), base.top + Number(shift[2]), base.width, base.height);
}

function layout() {
  // Grouped by board rather than flattened, so two mounted boards each measure
  // their own columns from their own left edge.
  const boards = new Map<HTMLElement, HTMLElement[]>();
  for (const box of document.querySelectorAll<HTMLElement>("[data-board-column]")) {
    const root = box.closest<HTMLElement>("section");
    if (root === null) {
      throw new Error("a board column outside any board");
    }
    const columns = boards.get(root);
    if (columns === undefined) {
      boards.set(root, [box]);
    } else {
      columns.push(box);
    }
  }
  for (const [root, columns] of boards) {
    root.getBoundingClientRect = () => rect(0, 0, COLUMN_W * columns.length, BOARD_H);
    let column = 0;
    for (const box of columns) {
      const left = column * COLUMN_W;
      column += 1;
      box.getBoundingClientRect = () => rect(left, 0, COLUMN_W, BOARD_H);
      let at = 0;
      for (const card of box.querySelectorAll<HTMLElement>("[data-card-key]")) {
        const top = CARD_TOP + at * CARD_H;
        at += 1;
        card.getBoundingClientRect = () => transformed(card, rect(left, top, COLUMN_W, CARD_H));
      }
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

/**
 * Where inside a card the press lands, in viewport y.
 *
 * A card of the first slot spans y=40..70, so `nearTop` is 5 px in and
 * `nearBottom` 28 px in. The distinction is load-bearing rather than thorough:
 * the dragged card's own contribution to a midpoint tally that measured it
 * reduced to `height / 2 < grabOffsetY` — a constant for the whole gesture,
 * decided by exactly this.
 */
const PRESS = { nearTop: 45, nearBottom: 68 };

/** Press `on` at `pressY`, move the pointer to (x, y), release it there. */
function dragTo(on: HTMLElement, x: number, y: number, pressY = PRESS.nearTop) {
  fireEvent.pointerDown(on, { pointerId: 1, button: 0, clientX: 0, clientY: pressY });
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

/**
 * Report `(prefers-reduced-motion: reduce)` as matching, before the mount that
 * reads it — {@link "@/hooks/use-reduced-motion"} initialises synchronously, so a
 * preference installed after `render` would arrive a frame late here as well.
 * Same shape as `use-reduced-motion.test.ts`'s own mock.
 */
const originalMatchMedia = window.matchMedia;
function mockReducedMotion() {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: query.includes("prefers-reduced-motion"),
    media: query,
    onchange: null,
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  }));
}

afterEach(() => {
  window.matchMedia = originalMatchMedia;
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

  it("lands a card last when it is dragged to the bottom of its own column", async () => {
    const { onMove } = mount();
    layout();
    // The gesture the hesperia list never named, and the one the transform broke.
    // "to do" holds this card at slot 0 and one below it; released at y=250 the
    // honest answer is the END of the column without it, which is 1.
    //
    // The card is measured where it now IS: pressed 5 px in and released at 250,
    // its transformed box is y=245..275 and its own midpoint 260 is BELOW the
    // pointer — so a tally that counted it counted it as still above everything,
    // reported slot 1, subtracted the vacated slot, and wrote 0. The bottom of
    // the column became the top.
    dragTo(cardOf("Write the board"), X.todo, Y.empty, PRESS.nearTop);
    await waitFor(() => expect(onMove).toHaveBeenCalledWith("write-the-board.md", "todo", 1));
  });

  it("lands it last however deep in the card the press began", async () => {
    // The other half of the same check: grabbed near its bottom edge, the same
    // gesture must write the same slot. Where the press landed inside the card is
    // not information about where the card is going.
    const { onMove } = mount();
    layout();
    dragTo(cardOf("Write the board"), X.todo, Y.empty, PRESS.nearBottom);
    await waitFor(() => expect(onMove).toHaveBeenCalledWith("write-the-board.md", "todo", 1));
  });

  it("lands a card first when it is dragged up to the top of its own column", async () => {
    // The mirror case, which no compensation could have fixed: grabbed on its
    // lower half and dragged UP, the card's transformed midpoint arrives above
    // the pointer, so a tally that measured it counted it at its new position —
    // and the vacated-slot subtraction, which only fires for a downward move, let
    // that stand. Slot 1 instead of 0: one place too far.
    const { onMove } = mount({
      cards: [
        card({ title: "Write the board", status: "todo", order: 1 }),
        card({ title: "Wire the IPC", status: "todo", order: 2 }),
        card({ title: "Ship the board", status: "todo", order: 3 }),
      ],
    });
    layout();
    // Slot 2 spans y=100..130; pressed at 125 and released at 45, above the first
    // card's midpoint of 55.
    dragTo(cardOf("Ship the board"), X.todo, 45, 125);
    await waitFor(() => expect(onMove).toHaveBeenCalledWith("ship-the-board.md", "todo", 0));
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
    // And a click moves nothing on screen either: the card is not lifted, so it
    // carries no transform at all (Story 54.1).
    expect(cardOf("Wire the IPC").style.transform).toBe("");
    fireEvent.pointerUp(title, { pointerId: 1, clientX: 303, clientY: 77 });
    fireEvent.click(title);
    expect(onMove).not.toHaveBeenCalled();
    expect(onOpen).toHaveBeenCalledWith("wire-the-ipc.md");
    expect(cardOf("Wire the IPC").style.transform).toBe("");
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

  it("takes the capture back when a re-read moves the pressed card, and still lands the move", async () => {
    // The board is safe from the pins strip's own defect only by accident today:
    // it paints no reorder preview, so nothing MOVES the pressed node mid-drag.
    // An external `order:` edit does — from Obsidian, an agent or the watcher —
    // and a keyed child moved inside its own list keeps its DOM node, which means
    // `insertBefore` removes it from the parent first and Pointer Events treats
    // that removal as an implicit release of the capture.
    const { onMove, reread } = mount();
    layout();
    const li = cardOf("Write the board");
    const captured = capturesOn(li);
    fireEvent.pointerDown(li, { pointerId: 1, button: 0, clientX: X.todo, clientY: 45 });
    fireEvent.pointerMove(li, { pointerId: 1, clientX: X.done, clientY: Y.header });
    expect(captured).toHaveBeenCalledTimes(1);
    // A sibling's `order:` drops below this card's: same list, new position.
    reread([
      card({ title: "Draft the plan", status: "in-preparation", order: 1 }),
      card({ title: "Write the board", status: "todo", order: 1 }),
      card({ title: "Wire the IPC", status: "todo", order: 0 }),
    ]);
    layout();
    const todo = screen.getByRole("list", { name: "To do" });
    expect(Array.from(todo.children)).toEqual([cardOf("Wire the IPC"), li]);
    expect(li.isConnected).toBe(true);
    // What WebKit does next, and jsdom never did on its own.
    fireEvent.lostPointerCapture(li, { pointerId: 1 });
    expect(captured).toHaveBeenCalledTimes(2);
    // The gesture is still live: the release still lands where it says.
    fireEvent.pointerMove(li, { pointerId: 1, clientX: X.done, clientY: Y.header });
    fireEvent.pointerUp(li, { pointerId: 1, clientX: X.done, clientY: Y.header });
    await waitFor(() => expect(onMove).toHaveBeenCalledWith("write-the-board.md", "done", 0));
  });

  it("ends the gesture and frees the next click when the pressed card unmounts mid-drag", () => {
    // The other cause of the same event, which wants the opposite answer. A keyed
    // child whose parent list changes is not moved but remounted, so a re-read
    // that restatuses the pressed card takes its node away for good — and the
    // click this drag was going to swallow will never be dispatched at a node
    // that no longer exists.
    const { onMove, onOpen, reread } = mount();
    layout();
    const li = cardOf("Draft the plan");
    fireEvent.pointerDown(li, { pointerId: 1, button: 0, clientX: X.prep, clientY: 45 });
    fireEvent.pointerMove(li, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    expect(cued()).toEqual(["todo"]);
    reread([
      card({ title: "Draft the plan", status: "done", order: 1 }),
      card({ title: "Write the board", status: "todo", order: 1 }),
      card({ title: "Wire the IPC", status: "todo", order: 2 }),
    ]);
    expect(li.isConnected).toBe(false);
    fireEvent.lostPointerCapture(li, { pointerId: 1 });
    // Torn down: nothing written, and no cue left claiming a live gesture.
    expect(onMove).not.toHaveBeenCalled();
    expect(cued()).toEqual([]);
    // The next click on the board is its own.
    fireEvent.click(screen.getByRole("button", { name: "Wire the IPC" }));
    expect(onOpen).toHaveBeenCalledWith("wire-the-ipc.md");
  });

  it("keeps a drag whose pointer left the card before the slop: the board hears the move", async () => {
    // 28 px cards and a 6 px slop: a press 3 px from the edge leaves the card
    // before the press has become a drag, and before the crossing there is no
    // capture. The move lands on the column box, which the card sits below rather
    // than above — so with handlers on the card alone nothing hears it and the
    // drag silently never starts.
    const { onMove } = mount();
    layout();
    const li = cardOf("Draft the plan");
    const section = screen.getByRole("region", { name: "Tasks" });
    const onCard = capturesOn(li);
    const onSection = capturesOn(section);
    fireEvent.pointerDown(li, { pointerId: 1, button: 0, clientX: X.prep, clientY: 68 });
    fireEvent.pointerMove(section, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    fireEvent.pointerUp(section, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    await waitFor(() => expect(onMove).toHaveBeenCalledWith("draft-the-plan.md", "todo", 2));
    // The capture belongs to the pressed card, never to the box a stray move
    // happened to land on.
    expect(onCard).toHaveBeenCalledWith(1);
    expect(onSection).not.toHaveBeenCalled();
  });

  it("moves nothing in answer to an HTML5 drag", () => {
    // `draggable` is the attribute the test above reads; this is the behaviour.
    // `onDragStart`, `onDragOver` and `onDrop` render no DOM attribute at all, so
    // the only honest guard against the dead mechanism growing back one handler
    // at a time is to drive it and find that the board does nothing.
    const { onMove, onOpen } = mount();
    layout();
    const li = cardOf("Draft the plan");
    const todo = document.querySelector<HTMLElement>('[data-board-column="todo"]');
    if (todo === null) {
      throw new Error("no to-do column");
    }
    fireEvent.dragStart(li);
    fireEvent.dragOver(todo);
    fireEvent.drop(todo);
    fireEvent.dragEnd(li);
    expect(onMove).not.toHaveBeenCalled();
    expect(onOpen).not.toHaveBeenCalled();
    expect(cued()).toEqual([]);
  });

  it("frees the click of the next press when the drag ended without one", async () => {
    // A finger's drag ends with no synthesised click, so nothing eats the swallow
    // flag the drag set — and the next press is not guaranteed to reach `begin`,
    // the other site that clears it: a press on a card's own menu returns before
    // it. Left leaking, that press's click is eaten and the menu does not open.
    const { onMove } = mount();
    layout();
    const li = cardOf("Draft the plan");
    fireEvent.pointerDown(li, { pointerId: 1, button: 0, clientX: X.prep, clientY: 45 });
    fireEvent.pointerMove(li, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    fireEvent.pointerUp(li, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    await waitFor(() => expect(onMove).toHaveBeenCalledWith("draft-the-plan.md", "todo", 2));
    const menu = menuOf("Wire the IPC");
    fireEvent.pointerDown(menu, { pointerId: 2, button: 0, clientX: X.todo, clientY: 75 });
    // Not cancelled: `dispatchEvent` answers false for a click the board swallowed.
    expect(fireEvent.click(menu)).toBe(true);
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

/**
 * What jsdom can and cannot say about a card that follows the pointer.
 *
 * It has no layout and no compositor, so nothing here observes a card moving. It
 * observes the arithmetic: the inline `transform` string the component computed
 * from coordinates the test handed it, the class that decides whether that string
 * is animated into, and whether the press was cancelled. Whether the transform
 * reaches the screen — and whether the pane it sits in clips it — is owed to a
 * human on hesperia: jsdom implements no overflow clipping at all, so the cap
 * below can only be measured as a number here. See the story's spec.
 */
describe("TaskBoard drag follow", () => {
  it("translates the pressed card by the pointer's delta from where it pressed", () => {
    mount();
    layout();
    const li = cardOf("Draft the plan");
    const other = cardOf("Wire the IPC");
    fireEvent.pointerDown(li, { pointerId: 1, button: 0, clientX: 40, clientY: 45 });
    // Under the slop: a card that jumped on the press would be worse than one
    // that never moved.
    fireEvent.pointerMove(li, { pointerId: 1, clientX: 43, clientY: 47 });
    expect(li.style.transform).toBe("");
    fireEvent.pointerMove(li, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    // 300 − 40 across and 250 − 45 down: from where the press began, not from the
    // last move, or the card would crawl one step behind the pointer.
    expect(li.style.transform).toBe("translate(260px, 205px)");
    fireEvent.pointerMove(li, { pointerId: 1, clientX: X.done, clientY: Y.header });
    expect(li.style.transform).toBe("translate(460px, -35px)");
    // The pressed card and no other: twenty cards must not grow twenty containing
    // blocks because one of them is moving.
    expect(other.style.transform).toBe("");
    fireEvent.pointerUp(li, { pointerId: 1, clientX: X.done, clientY: Y.header });
  });

  it("withholds the settle transition while the card follows, and restores it at the release", () => {
    const { onMove } = mount();
    layout();
    const li = cardOf("Draft the plan");
    expect(li.className).toContain("transition-transform");
    fireEvent.pointerDown(li, { pointerId: 1, button: 0, clientX: 40, clientY: 45 });
    fireEvent.pointerMove(li, { pointerId: 1, clientX: X.nowhere, clientY: Y.empty });
    // Live: no transition, or the card eases 200 ms behind the finger instead of
    // tracking it (`chat-row.tsx:459`).
    expect(li.className).not.toContain("transition-transform");
    expect(li.dataset.dragging).toBe("true");
    // The cursor rides the same attribute the opacity does. A class string is all
    // jsdom has — it computes no cursor.
    expect(li.className).toContain("data-[dragging=true]:cursor-grabbing");
    fireEvent.pointerUp(li, { pointerId: 1, clientX: X.nowhere, clientY: Y.empty });
    // Released over no column, so nothing is written and this is the same node:
    // the transform goes back to zero on the render that ends the drag, and the
    // transition is back on in the same render, which is the settle.
    expect(onMove).not.toHaveBeenCalled();
    expect(li.style.transform).toBe("");
    expect(li.className).toContain("transition-transform");
    expect(li.dataset.dragging).toBeUndefined();
  });

  it("cuts the landing transition under reduced motion, and never the live follow", () => {
    mockReducedMotion();
    mount();
    layout();
    const li = cardOf("Draft the plan");
    expect(li.className).not.toContain("transition-transform");
    fireEvent.pointerDown(li, { pointerId: 1, button: 0, clientX: 40, clientY: 45 });
    fireEvent.pointerMove(li, { pointerId: 1, clientX: X.done, clientY: Y.empty });
    // Direct manipulation is not animation: the card under the pointer still
    // follows it, at 1:1, with the preference on.
    expect(li.style.transform).toBe("translate(460px, 205px)");
    // Off the board, so nothing is written by the release below.
    fireEvent.pointerMove(li, { pointerId: 1, clientX: X.nowhere, clientY: Y.empty });
    fireEvent.pointerUp(li, { pointerId: 1, clientX: X.nowhere, clientY: Y.empty });
    expect(li.className).not.toContain("transition-transform");
  });

  it("caps the follow at the board's own box, so the pane cannot clip the card", () => {
    // The board is 800 x 300 here and this card is the first slot of the leftmost
    // column: x=0..200, y=40..70. Both hosts put the board inside a clipping
    // ancestor — `session-detail.tsx`'s `overflow-y-auto` and every panel's
    // `overflow-hidden` — and a transformed descendant is clipped by an overflow
    // ancestor AND joins its scrollable overflow, so an uncapped follow hid the
    // card behind the pane edge and grew the scrollbar for the length of the drag.
    mount();
    layout();
    const li = cardOf("Draft the plan");
    fireEvent.pointerDown(li, { pointerId: 1, button: 0, clientX: 40, clientY: 45 });
    // 860 px right of the press would put the card's right edge at 1060, 260 past
    // the board's; it stops with that edge on the board's own.
    fireEvent.pointerMove(li, { pointerId: 1, clientX: X.nowhere, clientY: Y.empty });
    expect(li.style.transform).toBe("translate(600px, 205px)");
    // And downwards, where the cost was the scroll range rather than the clip.
    fireEvent.pointerMove(li, { pointerId: 1, clientX: 40, clientY: 600 });
    expect(li.style.transform).toBe("translate(0px, 230px)");
    // Upwards and leftwards past the board's near edges, which is the same rule.
    fireEvent.pointerMove(li, { pointerId: 1, clientX: -400, clientY: 0 });
    expect(li.style.transform).toBe("translate(0px, -40px)");
    fireEvent.pointerUp(li, { pointerId: 1, clientX: -400, clientY: 0 });
  });

  it("holds a landed card still while keeper writes the drop, rather than gliding it back", async () => {
    // The settle animated the card BACKWARDS on every successful drop. `forget()`
    // zeroes the transform and the release commit still paints the card in its
    // SOURCE column, so restoring `transition-transform` in that commit
    // interpolated it from the drop point to the slot it came from — the opposite
    // direction — and only a Tauri round trip later did the re-read relocate it.
    let written: (() => void) | undefined;
    const onMove = vi.fn(
      () =>
        new Promise<void>((resolve) => {
          written = resolve;
        }),
    );
    mount({ onMove });
    layout();
    const li = cardOf("Draft the plan");
    fireEvent.pointerDown(li, { pointerId: 1, button: 0, clientX: X.prep, clientY: 45 });
    fireEvent.pointerMove(li, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    fireEvent.pointerUp(li, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    expect(onMove).toHaveBeenCalledWith("draft-the-plan.md", "todo", 2);
    // The write is in flight: the drag is over, the transform is gone, and there
    // must be nothing to animate it along.
    expect(li.dataset.dragging).toBeUndefined();
    expect(li.style.transform).toBe("");
    expect(li.className).not.toContain("transition-transform");
    // The re-read has landed the card: the settle belongs to it again, so the next
    // gesture that returns it still travels.
    await act(async () => {
      written?.();
    });
    expect(cardOf("Draft the plan").className).toContain("transition-transform");
  });
});

describe("TaskBoard drag selection", () => {
  it("cancels the press's own default, so no selection is anchored", () => {
    mount();
    layout();
    const li = cardOf("Draft the plan");
    const press = createEvent.pointerDown(li, {
      pointerId: 1,
      button: 0,
      clientX: 40,
      clientY: 45,
    });
    fireEvent(li, press);
    // A cancelled `pointerdown` fires no `mousedown`, and `mousedown` is what
    // anchors the selection every later move extends. The `click` that follows is
    // not cancelled by it, which is what the title tests above rely on.
    expect(press.defaultPrevented).toBe(true);
  });

  it("leaves the column menu's press and a secondary press to the platform", () => {
    mount();
    layout();
    // Cancelling the menu's press would cost the `<select>` its focus and its
    // dropdown, and a secondary press belongs to whatever menu it opens.
    const menu = menuOf("Draft the plan");
    const onMenu = createEvent.pointerDown(menu, {
      pointerId: 1,
      button: 0,
      clientX: 40,
      clientY: 45,
    });
    fireEvent(menu, onMenu);
    expect(onMenu.defaultPrevented).toBe(false);
    const li = cardOf("Wire the IPC");
    const secondary = createEvent.pointerDown(li, {
      pointerId: 2,
      button: 2,
      clientX: 40,
      clientY: 45,
    });
    fireEvent(li, secondary);
    expect(secondary.defaultPrevented).toBe(false);
  });

  it("holds the document unselectable from the slop crossing to the release", () => {
    mount();
    layout();
    const li = cardOf("Draft the plan");
    fireEvent.pointerDown(li, { pointerId: 1, button: 0, clientX: 40, clientY: 45 });
    // A click must not make the whole app unselectable, so the arming waits for
    // the crossing — the moment the press stops being a click.
    expect(document.body.classList.contains(DRAG_SELECTION_CLASS)).toBe(false);
    fireEvent.pointerMove(li, { pointerId: 1, clientX: 43, clientY: 47 });
    expect(document.body.classList.contains(DRAG_SELECTION_CLASS)).toBe(false);
    fireEvent.pointerMove(li, { pointerId: 1, clientX: X.nowhere, clientY: Y.empty });
    expect(document.body.classList.contains(DRAG_SELECTION_CLASS)).toBe(true);
    fireEvent.pointerUp(li, { pointerId: 1, clientX: X.nowhere, clientY: Y.empty });
    expect(document.body.classList.contains(DRAG_SELECTION_CLASS)).toBe(false);
  });

  it("gives the document back when the gesture is cancelled", () => {
    mount();
    layout();
    const li = cardOf("Draft the plan");
    fireEvent.pointerDown(li, { pointerId: 1, button: 0, clientX: 40, clientY: 45 });
    fireEvent.pointerMove(li, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    expect(document.body.classList.contains(DRAG_SELECTION_CLASS)).toBe(true);
    fireEvent.pointerCancel(li, { pointerId: 1 });
    expect(document.body.classList.contains(DRAG_SELECTION_CLASS)).toBe(false);
  });

  it("gives the document back when the pressed card unmounts mid-drag", () => {
    // The re-read that takes the node away for good: nothing will deliver a
    // release to it, so the suppression has to come off the branch that hears the
    // capture being lost.
    const { reread } = mount();
    layout();
    const li = cardOf("Draft the plan");
    fireEvent.pointerDown(li, { pointerId: 1, button: 0, clientX: X.prep, clientY: 45 });
    fireEvent.pointerMove(li, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    expect(document.body.classList.contains(DRAG_SELECTION_CLASS)).toBe(true);
    reread([
      card({ title: "Draft the plan", status: "done", order: 1 }),
      card({ title: "Write the board", status: "todo", order: 1 }),
      card({ title: "Wire the IPC", status: "todo", order: 2 }),
    ]);
    expect(li.isConnected).toBe(false);
    fireEvent.lostPointerCapture(li, { pointerId: 1 });
    expect(document.body.classList.contains(DRAG_SELECTION_CLASS)).toBe(false);
  });

  it("gives the document back when the whole board unmounts mid-drag", () => {
    // Nothing is left to hear a release, so the teardown is the last chance: an
    // app that could never select text again would be the worse regression.
    const { unmount } = mount();
    layout();
    const li = cardOf("Draft the plan");
    fireEvent.pointerDown(li, { pointerId: 1, button: 0, clientX: X.prep, clientY: 45 });
    fireEvent.pointerMove(li, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    expect(document.body.classList.contains(DRAG_SELECTION_CLASS)).toBe(true);
    unmount();
    expect(document.body.classList.contains(DRAG_SELECTION_CLASS)).toBe(false);
  });

  it("gives the document back when a second press replaces one whose release never came", () => {
    // `begin` exists to recover from a press whose release was never seen — a
    // pointer that left a small target without ever crossing the slop. It took the
    // capture hold back and left the selection suppression armed, and the replaced
    // press's own `pointerup` is dropped by the pointerId guard: nothing else
    // could ever release it, so the whole app stayed unselectable until the board
    // unmounted.
    mount();
    layout();
    const first = cardOf("Draft the plan");
    fireEvent.pointerDown(first, { pointerId: 1, button: 0, clientX: X.prep, clientY: 45 });
    fireEvent.pointerMove(first, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    expect(document.body.classList.contains(DRAG_SELECTION_CLASS)).toBe(true);
    // A second pointer presses another card. This overwrites the tracked press.
    fireEvent.pointerDown(cardOf("Wire the IPC"), {
      pointerId: 2,
      button: 0,
      clientX: X.todo,
      clientY: 75,
    });
    expect(document.body.classList.contains(DRAG_SELECTION_CLASS)).toBe(false);
    // And the first pointer's late release is still ignored, so this is not a
    // suppression the guard happened to take off a beat later.
    fireEvent.pointerUp(first, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    expect(document.body.classList.contains(DRAG_SELECTION_CLASS)).toBe(false);
  });

  it("keeps the document unselectable while a second surface's drag is still live", () => {
    // One class, two hook instances — the board and the pins strip in the app,
    // two boards here. Whichever gesture ended first used to strip the suppression
    // from under the one still running, and `armedRef` cannot see it: it stops an
    // instance releasing a suppression it never armed, not one that DID arm from
    // releasing another's.
    const shared = {
      cards: cards(),
      empty: "No tasks yet.",
      onOpen: vi.fn(),
      onMove: vi.fn(async () => {}),
    };
    render(
      <>
        <TaskBoard {...shared} heading="First board" />
        <TaskBoard {...shared} heading="Second board" />
      </>,
    );
    layout();
    const cardIn = (heading: string) => {
      const li = within(screen.getByRole("region", { name: heading }))
        .getByRole("button", { name: "Draft the plan" })
        .closest("li");
      if (li === null) {
        throw new Error(`no card in ${heading}`);
      }
      return li;
    };
    const first = cardIn("First board");
    const second = cardIn("Second board");
    // Below every column, so neither release writes anything.
    const off = { clientX: X.prep, clientY: 400 };
    fireEvent.pointerDown(first, { pointerId: 1, button: 0, clientX: X.prep, clientY: 45 });
    fireEvent.pointerMove(first, { pointerId: 1, ...off });
    fireEvent.pointerDown(second, { pointerId: 2, button: 0, clientX: X.prep, clientY: 45 });
    fireEvent.pointerMove(second, { pointerId: 2, ...off });
    expect(document.body.classList.contains(DRAG_SELECTION_CLASS)).toBe(true);
    fireEvent.pointerUp(second, { pointerId: 2, ...off });
    // The second board's gesture is over; the first board's is not.
    expect(document.body.classList.contains(DRAG_SELECTION_CLASS)).toBe(true);
    fireEvent.pointerUp(first, { pointerId: 1, ...off });
    expect(document.body.classList.contains(DRAG_SELECTION_CLASS)).toBe(false);
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
