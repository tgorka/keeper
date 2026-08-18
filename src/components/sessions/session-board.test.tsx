import { createEvent, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { SessionTaskVm } from "@/lib/ipc/client";

const sessionsTaskMove = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  sessionsTaskMove: (root: unknown, session: unknown, rel: unknown, s: unknown, i: unknown) =>
    sessionsTaskMove(root, session, rel, s, i),
}));

import {
  SESSION_BOARD_COLUMNS,
  SESSION_BOARD_EMPTY,
  SESSION_BOARD_MOVE_FAILED,
  SESSION_BOARD_MOVE_LABEL,
  SESSION_BOARD_ORDER_DEFAULTED,
  SESSION_BOARD_STRAY_HEADING,
  SessionBoard,
} from "@/components/sessions/session-board";
import { DRAG_SELECTION_CLASS } from "@/hooks/use-pointer-drag";

function task(over: Partial<SessionTaskVm> & Pick<SessionTaskVm, "title">): SessionTaskVm {
  const relPath = over.relPath ?? `${over.title.toLowerCase().replace(/\W+/g, "-")}.md`;
  return {
    id: "01J5AAAAAAAAAAAAAAAAAAAAAA",
    status: "todo",
    order: 1,
    orderIsOwn: true,
    tags: ["task"],
    unstableIdentity: false,
    ...over,
    relPath,
  };
}

/** One card per column, plus a second in "to do" so a position can be dropped into. */
function board(): SessionTaskVm[] {
  return [
    task({ title: "Draft the plan", status: "in-preparation", order: 1 }),
    task({ title: "Write the board", status: "todo", order: 1 }),
    task({ title: "Wire the IPC", status: "todo", order: 2 }),
    task({ title: "Ship the spaces", status: "done", order: 1 }),
    task({ title: "Rethink the widgets", status: "deferred", order: 1 }),
  ];
}

function mount(over: Partial<React.ComponentProps<typeof SessionBoard>> = {}) {
  const onOpen = vi.fn();
  const onChanged = vi.fn();
  const result = render(
    <SessionBoard
      rootId="tgdrive"
      sessionId="active/2026-08-10-keeper"
      tasks={board()}
      onOpen={onOpen}
      onChanged={onChanged}
      {...over}
    />,
  );
  return { ...result, onOpen, onChanged };
}

/**
 * Lay the board out, because jsdom does not — and because the shim in
 * `src/test/setup.ts` answers a zero rect with one full viewport, which would
 * make every point land in the first column.
 *
 * Four column boxes 200 px wide side by side, each column's cards 30 px tall
 * stacked from y=40, so y<40 is the header and y≫40 is the empty space below the
 * last card. Local rather than shared, like `pins-strip.test.tsx`'s
 * `mockPinSlots`; the twin in `notes/task-board.test.tsx` measures the same board
 * from the widget side.
 *
 * A card's rect MOVES with the `translate()` its own inline style carries, and the
 * board's `<section>` gets a rect of its own, because since Story 54.1 the board
 * measures both: `getBoundingClientRect` returns the TRANSFORMED border box, and
 * the follow is capped to the board's box so a pane cannot clip the card.
 */
const COLUMN_W = 200;
const BOARD_H = 300;
const CARD_TOP = 40;
const CARD_H = 30;

/** A DOMRect, spelled once rather than at nine fields a time. */
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
  const columns = Array.from(document.querySelectorAll<HTMLElement>("[data-board-column]"));
  const board = columns[0]?.closest<HTMLElement>("section");
  if (board === null || board === undefined) {
    throw new Error("no board around the columns");
  }
  board.getBoundingClientRect = () => rect(0, 0, COLUMN_W * columns.length, BOARD_H);
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

/** The x that lands in each column, in the order `SESSION_BOARD_COLUMNS` renders. */
const X = { prep: 100, todo: 300, done: 500 };

/** Between the first two cards' midpoints, and far below the last card. */
const Y = { afterFirst: 80, empty: 250 };

/**
 * Press `from`'s card at `pressY`, move the pointer to (x, y) and release it there.
 *
 * The gesture is pointer events, not HTML5 drag: under Tauri on macOS the page's
 * `drop` cannot fire at all (`use-pointer-drag.ts` carries the source lines), and
 * unlike a synthetic `dragstart`/`drop` pair this is the same sequence a real
 * pointer produces — which is why this suite can now claim a move happened.
 *
 * `pressY` says where inside the card the press landed — 45 is 5 px into the first
 * slot, 68 is 28 px in. It used to make no difference to anything; since the card
 * carries a transform, a tally that measured the dragged card turned it into the
 * whole answer.
 */
function drag(from: HTMLElement, x: number, y: number, pressY = 45) {
  fireEvent.pointerDown(from, { pointerId: 1, button: 0, clientX: 0, clientY: pressY });
  fireEvent.pointerMove(from, { pointerId: 1, clientX: x, clientY: y });
  fireEvent.pointerUp(from, { pointerId: 1, clientX: x, clientY: y });
}

/** The card element for a title — the `li`, which is what carries the handlers. */
function cardOf(title: string): HTMLElement {
  const button = screen.getByRole("button", { name: title });
  const card = button.closest("li");
  if (card === null) {
    throw new Error(`no card for ${title}`);
  }
  return card;
}

afterEach(() => {
  vi.clearAllMocks();
});

describe("SessionBoard", () => {
  it("draws the four columns work moves through, in that direction", () => {
    mount();
    const headings = screen.getAllByRole("heading", { level: 4 }).map((h) => h.textContent);
    expect(headings.map((h) => h?.replace(/\d+$/, ""))).toEqual([
      "In preparation",
      "To do",
      "Done",
      "Deferred",
    ]);
    // The four are the closed set Rust parses — a fifth column would be one no
    // session can show.
    expect(SESSION_BOARD_COLUMNS.map((c) => c.status)).toEqual([
      "in-preparation",
      "todo",
      "done",
      "deferred",
    ]);
  });

  it("puts each card in the column its own status names", () => {
    mount();
    const todo = screen.getByRole("list", { name: "To do" });
    expect(within(todo).getByRole("button", { name: "Write the board" })).toBeInTheDocument();
    expect(within(todo).queryByRole("button", { name: "Ship the spaces" })).not.toBeInTheDocument();
  });

  it("orders a column by its files' own order, not by arrival", () => {
    mount({
      tasks: [
        task({ title: "Second", order: 2 }),
        task({ title: "First", order: 1 }),
        task({ title: "Third", order: 3 }),
      ],
    });
    const titles = screen
      .getAllByRole("button", { name: /First|Second|Third/ })
      .map((b) => b.textContent);
    expect(titles).toEqual(["First", "Second", "Third"]);
  });

  it("moves a card to another column at the position the release landed on", async () => {
    const { onChanged } = mount();
    layout();
    drag(cardOf("Draft the plan"), X.todo, Y.afterFirst);
    await waitFor(() => {
      // Released past the first card of "to do" and short of the second — slot
      // 1, and the moved card was never in that column so nothing is subtracted.
      expect(sessionsTaskMove).toHaveBeenCalledWith(
        "tgdrive",
        "active/2026-08-10-keeper",
        "draft-the-plan.md",
        "todo",
        1,
      );
    });
    // The board keeps no order of its own: the re-read is the answer.
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it("counts its own column without the card being dragged inside it", async () => {
    mount();
    layout();
    drag(cardOf("Write the board"), X.todo, Y.afterFirst);
    await waitFor(() => {
      // Slot 0 of "to do", released just past the midpoint of the card below it —
      // and the column Rust resolves against is this one WITHOUT this card, whose
      // only member above the pointer is none: 0.
      expect(sessionsTaskMove).toHaveBeenCalledWith(
        "tgdrive",
        "active/2026-08-10-keeper",
        "write-the-board.md",
        "todo",
        0,
      );
    });
  });

  it("sends a card dragged to the bottom of its own column to the end of it", async () => {
    // The gesture the owner reports, on the surface he reports it from, and the
    // one the follow's transform broke: the dragged card measured itself at the
    // pointer, so its own contribution became `height / 2 < grabOffsetY` — a
    // constant for the whole gesture. Grabbed 5 px in and released at y=250 it
    // counted as above nothing, the tally said slot 1, the vacated-slot
    // subtraction took one off, and the bottom of the column was written as the
    // top. Once from near the card's top edge, once from near its bottom.
    mount();
    layout();
    drag(cardOf("Write the board"), X.todo, Y.empty, 45);
    await waitFor(() => {
      expect(sessionsTaskMove).toHaveBeenCalledWith(
        "tgdrive",
        "active/2026-08-10-keeper",
        "write-the-board.md",
        "todo",
        1,
      );
    });
    sessionsTaskMove.mockClear();
    drag(cardOf("Write the board"), X.todo, Y.empty, 68);
    await waitFor(() => {
      expect(sessionsTaskMove).toHaveBeenCalledWith(
        "tgdrive",
        "active/2026-08-10-keeper",
        "write-the-board.md",
        "todo",
        1,
      );
    });
  });

  it("releases over a column's empty space at the end of it", async () => {
    mount();
    layout();
    // Below every card's midpoint in "Done": the end of that column, and a
    // region the old `<ul>` did not cover at all.
    drag(cardOf("Draft the plan"), X.done, Y.empty);
    await waitFor(() => {
      expect(sessionsTaskMove).toHaveBeenCalledWith(
        "tgdrive",
        "active/2026-08-10-keeper",
        "draft-the-plan.md",
        "done",
        1,
      );
    });
  });

  it("does nothing on a release that never began as a press", () => {
    mount();
    layout();
    fireEvent.pointerUp(cardOf("Wire the IPC"), { pointerId: 1, clientX: X.todo, clientY: 250 });
    expect(sessionsTaskMove).not.toHaveBeenCalled();
  });

  it("addresses the card by its session-relative path, not by its title", () => {
    // What only this suite can say is WHICH identity the gesture carries: the
    // session board moves a card by the path `sessions_task_move` takes, and a
    // task's file need not be named after its heading.
    mount({
      tasks: [
        task({ title: "Draft the plan", relPath: "plans/draft.md", status: "in-preparation" }),
        task({ title: "Write the board", status: "todo", order: 1 }),
      ],
    });
    layout();
    drag(cardOf("Draft the plan"), X.todo, Y.empty);
    expect(sessionsTaskMove).toHaveBeenCalledWith(
      "tgdrive",
      "active/2026-08-10-keeper",
      "plans/draft.md",
      "todo",
      1,
    );
  });

  it("keeps every card's column menu in the session's DOM, revealed rather than drawn", () => {
    // The menu is demoted to hover/focus, and demoted is not deleted: this is
    // the session board's only pointer-free way to move a card, and
    // `session-detail.test.tsx` asserts the board is live through it.
    mount();
    const menu = screen.getByLabelText(`${SESSION_BOARD_MOVE_LABEL} — Draft the plan`);
    expect(menu.className).toContain("opacity-0");
    expect(menu.className).toContain("group-hover:opacity-100");
    expect(menu.className).toContain("focus-within:opacity-100");
    expect(menu).not.toBeDisabled();
    menu.focus();
    expect(menu).toHaveFocus();
  });

  it("moves a card without a pointer, to the end of the column it joins", async () => {
    mount();
    const select = screen.getByLabelText(`${SESSION_BOARD_MOVE_LABEL} — Draft the plan`);
    fireEvent.change(select, { target: { value: "todo" } });
    await waitFor(() => {
      // "to do" holds two cards, so the end of it is index 2.
      expect(sessionsTaskMove).toHaveBeenCalledWith(
        "tgdrive",
        "active/2026-08-10-keeper",
        "draft-the-plan.md",
        "todo",
        2,
      );
    });
  });

  it("opens the card's file rather than treating it as board furniture", () => {
    const { onOpen } = mount();
    screen.getByRole("button", { name: "Wire the IPC" }).click();
    expect(onOpen).toHaveBeenCalledWith("wire-the-ipc.md");
  });

  it("says keeper's own refusal, and returns the card", async () => {
    const { onChanged } = mount();
    layout();
    sessionsTaskMove.mockRejectedValue({ message: "That card is not in this session any more." });
    drag(cardOf("Draft the plan"), X.todo, Y.empty);
    expect(await screen.findByRole("status")).toHaveTextContent(
      "That card is not in this session any more.",
    );
    expect(screen.queryByText(SESSION_BOARD_MOVE_FAILED)).not.toBeInTheDocument();
    expect(onChanged).not.toHaveBeenCalled();
    // The card is where its own `status:` puts it: the board never moved it, and
    // nothing re-read, so the refusal costs the user nothing but the sentence.
    const prep = screen.getByRole("list", { name: "In preparation" });
    expect(within(prep).getByRole("button", { name: "Draft the plan" })).toBeInTheDocument();
  });

  it("falls back to keeper's sentence when the refusal carries none", async () => {
    mount();
    layout();
    sessionsTaskMove.mockRejectedValue({});
    drag(cardOf("Draft the plan"), X.todo, Y.empty);
    expect(await screen.findByRole("status")).toHaveTextContent(SESSION_BOARD_MOVE_FAILED);
  });

  it("shows a card whose status is not a column rather than hiding it", () => {
    mount({ tasks: [...board(), task({ title: "Blocked on review", status: "blocked" })] });
    const stray = screen.getByRole("heading", { name: SESSION_BOARD_STRAY_HEADING })
      .parentElement as HTMLElement;
    expect(within(stray).getByRole("button", { name: "Blocked on review" })).toBeInTheDocument();
    // And its own word is still offered, so switching away is not a one-way door
    // that silently rewrites what the file said.
    const select = within(stray).getByLabelText(`${SESSION_BOARD_MOVE_LABEL} — Blocked on review`);
    expect((select as HTMLSelectElement).value).toBe("blocked");
  });

  it("shows a card with no status at all, which is what an untouched task is", () => {
    mount({ tasks: [task({ title: "Freshly written", status: null })] });
    expect(screen.getByRole("heading", { name: SESSION_BOARD_STRAY_HEADING })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Freshly written" })).toBeInTheDocument();
  });

  it("marks a card whose order keeper had to default", () => {
    mount({ tasks: [task({ title: "No order key", orderIsOwn: false })] });
    expect(screen.getByLabelText(SESSION_BOARD_ORDER_DEFAULTED)).toBeInTheDocument();
    // A card that states its own order says nothing — a warning on every card
    // is a warning on none.
    expect(screen.queryByLabelText(SESSION_BOARD_ORDER_DEFAULTED)).toBeInTheDocument();
  });

  it("says an empty session is empty, and how a task is made", () => {
    mount({ tasks: [] });
    expect(screen.getByText(SESSION_BOARD_EMPTY)).toBeInTheDocument();
    expect(screen.queryByRole("heading", { level: 4 })).not.toBeInTheDocument();
  });

  it("carries the follow and the selection suppression through to the sessions surface", () => {
    // The board's own suite measures the arithmetic; this asserts the surface the
    // owner actually drags on is wired to it, rather than a component nothing
    // mounts having the behaviour. jsdom sees the computed transform string and
    // the cancelled press — never a card moving, and never a WebKit selection.
    mount();
    layout();
    const card = cardOf("Draft the plan");
    const press = createEvent.pointerDown(card, {
      pointerId: 1,
      button: 0,
      clientX: 40,
      clientY: 45,
    });
    fireEvent(card, press);
    expect(press.defaultPrevented).toBe(true);
    fireEvent.pointerMove(card, { pointerId: 1, clientX: X.todo, clientY: Y.empty });
    expect(card.style.transform).toBe("translate(260px, 205px)");
    expect(card.className).not.toContain("transition-transform");
    expect(document.body.classList.contains(DRAG_SELECTION_CLASS)).toBe(true);
    fireEvent.pointerCancel(card, { pointerId: 1 });
    expect(card.style.transform).toBe("");
    expect(card.className).toContain("transition-transform");
    expect(document.body.classList.contains(DRAG_SELECTION_CLASS)).toBe(false);
  });
});
