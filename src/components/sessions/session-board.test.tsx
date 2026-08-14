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

/** Drag `from`'s card onto `onto`'s, the way a browser fires the pair. */
function drag(from: HTMLElement, onto: HTMLElement) {
  fireEvent.dragStart(from);
  fireEvent.dragOver(onto);
  fireEvent.drop(onto);
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

  it("moves a card to another column at the position it was dropped on", async () => {
    const { onChanged } = mount();
    drag(cardOf("Draft the plan"), cardOf("Wire the IPC"));
    await waitFor(() => {
      // Dropped onto the second card of "to do" — index 1, and the moved card
      // was never in that column so nothing is subtracted.
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
    drag(cardOf("Write the board"), cardOf("Wire the IPC"));
    await waitFor(() => {
      // Rendered index 1, but the column Rust resolves against has this card
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

  it("drops onto a column's empty space at the end of it", async () => {
    mount();
    const done = screen.getByRole("list", { name: "Done" });
    fireEvent.dragStart(cardOf("Draft the plan"));
    fireEvent.dragOver(done);
    fireEvent.drop(done);
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

  it("does nothing on a drop that never started as a drag", () => {
    mount();
    fireEvent.drop(cardOf("Wire the IPC"));
    expect(sessionsTaskMove).not.toHaveBeenCalled();
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

  it("says keeper's own refusal, and changes nothing", async () => {
    const { onChanged } = mount();
    sessionsTaskMove.mockRejectedValue({ message: "That card is not in this session any more." });
    drag(cardOf("Draft the plan"), cardOf("Wire the IPC"));
    expect(await screen.findByRole("status")).toHaveTextContent(
      "That card is not in this session any more.",
    );
    expect(screen.queryByText(SESSION_BOARD_MOVE_FAILED)).not.toBeInTheDocument();
    expect(onChanged).not.toHaveBeenCalled();
  });

  it("falls back to keeper's sentence when the refusal carries none", async () => {
    mount();
    sessionsTaskMove.mockRejectedValue({});
    drag(cardOf("Draft the plan"), cardOf("Wire the IPC"));
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
