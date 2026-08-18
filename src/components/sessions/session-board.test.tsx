import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
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
 */
const COLUMN_W = 200;
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

/** The x that lands in each column, in the order `SESSION_BOARD_COLUMNS` renders. */
const X = { prep: 100, todo: 300, done: 500 };

/** Between the first two cards' midpoints, and far below the last card. */
const Y = { afterFirst: 80, empty: 250 };

/**
 * Press `from`'s card, move the pointer to (x, y) and release it there.
 *
 * The gesture is pointer events, not HTML5 drag: under Tauri on macOS the page's
 * `drop` cannot fire at all (`use-pointer-drag.ts` carries the source lines), and
 * unlike a synthetic `dragstart`/`drop` pair this is the same sequence a real
 * pointer produces — which is why this suite can now claim a move happened.
 */
function drag(from: HTMLElement, x: number, y: number) {
  fireEvent.pointerDown(from, { pointerId: 1, button: 0, clientX: 0, clientY: 45 });
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

  it("loses the vacated slot when a card is dragged down inside its own column", async () => {
    mount();
    layout();
    drag(cardOf("Write the board"), X.todo, Y.afterFirst);
    await waitFor(() => {
      // Rendered slot 1, but the column Rust resolves against has this card
      // removed — so the honest answer is 0, the classic off-by-one.
      expect(sessionsTaskMove).toHaveBeenCalledWith(
        "tgdrive",
        "active/2026-08-10-keeper",
        "write-the-board.md",
        "todo",
        0,
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
});
