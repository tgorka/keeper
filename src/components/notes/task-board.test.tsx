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
 * A stand-in for the drag data store, because jsdom implements none.
 *
 * This is the half of a drag jsdom CAN be made to see: whether the component
 * wrote the card's identity and declared the effect. Whether WebKit then fires
 * `drop` is not observable here at all — `session-board.test.tsx` was green for
 * two epics over a board whose drop never fired on macOS, which is exactly why
 * these assertions are about the store and not about the gesture.
 */
function store() {
  const written = new Map<string, string>();
  const fake = {
    dropEffect: "none",
    effectAllowed: "uninitialized",
    setData: (format: string, value: string) => {
      written.set(format, value);
    },
    getData: (format: string) => written.get(format) ?? "",
  };
  return fake as unknown as DataTransfer & typeof fake;
}

/** The card element for a title — the `li`, which is what carries the handlers. */
function cardOf(title: string): HTMLElement {
  const li = screen.getByRole("button", { name: title }).closest("li");
  if (li === null) {
    throw new Error(`no card for ${title}`);
  }
  return li;
}

/** The card's column menu — present on every card, revealed rather than drawn. */
function menuOf(title: string): HTMLSelectElement {
  return screen.getByLabelText(`${BOARD_MOVE_LABEL} — ${title}`) as HTMLSelectElement;
}

afterEach(() => {
  vi.clearAllMocks();
});

describe("TaskBoard drag", () => {
  it("writes the dragged card's identity into the drag data store", () => {
    mount();
    const data = store();
    fireEvent.dragStart(cardOf("Draft the plan"), { dataTransfer: data });
    // WebKit fires no `drop` for a drag that carried nothing, so this line is
    // the whole difference between a board that moves cards and one that draws
    // a ghost and forgets it.
    expect(data.getData("text/plain")).toBe("draft-the-plan.md");
  });

  it("declares the drag a move, so the cursor is not a no-drop badge", () => {
    mount();
    const data = store();
    fireEvent.dragStart(cardOf("Draft the plan"), { dataTransfer: data });
    expect(data.effectAllowed).toBe("move");
  });

  it("answers a drag over a card with a move, from the card's own handler", () => {
    // Asserted on a stray card, not a column one: a card inside a column sits in
    // a `ul` that answers for it, so a `dragover` there passes whether or not
    // the card said anything itself. The stray row takes no drops at all, which
    // makes a move cursor over one of its cards attributable to the card.
    mount({ cards: [...cards(), card({ title: "Blocked on review", status: "blocked" })] });
    const data = store();
    fireEvent.dragStart(cardOf("Draft the plan"), { dataTransfer: data });
    fireEvent.dragOver(cardOf("Blocked on review"), { dataTransfer: data });
    expect(data.dropEffect).toBe("move");
  });

  it("answers a drag over a column's empty space with a move", () => {
    mount();
    const data = store();
    fireEvent.dragStart(cardOf("Draft the plan"), { dataTransfer: data });
    fireEvent.dragOver(screen.getByRole("list", { name: "To do" }), { dataTransfer: data });
    expect(data.dropEffect).toBe("move");
  });

  it("starts the drag from the card's title, not only from the grip", async () => {
    // A `button` is a hole in a draggable ancestor: WebKit starts no drag from a
    // mousedown on a form control, so grabbing the obvious place — the title —
    // did nothing. The title is draggable itself and `dragstart` bubbles to the
    // card, which is what makes the whole surface one handle.
    const { onMove } = mount();
    const title = screen.getByRole("button", { name: "Draft the plan" });
    expect(title).toHaveAttribute("draggable", "true");
    const data = store();
    fireEvent.dragStart(title, { dataTransfer: data });
    expect(data.getData("text/plain")).toBe("draft-the-plan.md");
    fireEvent.drop(cardOf("Wire the IPC"), { dataTransfer: data });
    await waitFor(() => expect(onMove).toHaveBeenCalledWith("draft-the-plan.md", "todo", 1));
  });

  it("still opens a card whose title is now a drag handle too", () => {
    const { onOpen } = mount();
    screen.getByRole("button", { name: "Wire the IPC" }).click();
    expect(onOpen).toHaveBeenCalledWith("wire-the-ipc.md");
  });

  it("moves a card dropped on a store-less drag event, which is every jsdom one", async () => {
    // The guard: the React types promise a `DataTransfer` that jsdom's synthetic
    // events do not carry, and a board that threw on `undefined` here would be
    // a board no test in this repo could drive.
    const { onMove } = mount();
    fireEvent.dragStart(cardOf("Draft the plan"));
    fireEvent.drop(cardOf("Wire the IPC"));
    await waitFor(() => expect(onMove).toHaveBeenCalledWith("draft-the-plan.md", "todo", 1));
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
