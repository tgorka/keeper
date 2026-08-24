import { act, cleanup, fireEvent, render, screen, within } from "@testing-library/react";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { NOTE_ROW_HEIGHT, NoteList } from "@/components/notes/note-list";
import {
  NOTE_MORE_TAGS_LABEL,
  NOTE_ORDER_UNREADABLE_MARK,
  NOTE_ROW_PIN_LABEL,
  NOTE_ROW_REVEAL_LABEL,
} from "@/components/notes/note-row";
import { WINDOW_ROW_ATTR, WINDOW_VIEWPORT_ATTR } from "@/components/ui/window-list";
import type { NoteRowVm } from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { type ListGeometry, withListGeometry } from "@/test/layout";

/**
 * jsdom reports every element as zero-sized, and a virtualiser fed a zero-height
 * viewport renders only its overscan window. Giving the scroll container a real
 * height is what makes "the list renders these rows" a statement about the list
 * rather than about jsdom's layout engine.
 */
beforeAll(() => {
  Object.defineProperty(HTMLElement.prototype, "getBoundingClientRect", {
    configurable: true,
    value(): DOMRect {
      return {
        width: 320,
        height: 640,
        top: 0,
        left: 0,
        bottom: 640,
        right: 320,
        x: 0,
        y: 0,
        toJSON: () => ({}),
      } as DOMRect;
    },
  });
});

function row(overrides: Partial<NoteRowVm> & { id: string; title: string }): NoteRowVm {
  return {
    path: `${overrides.id}.md`,
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
    // Every note has one, which is the point of 44.5's default; a fixture that
    // omitted it would be a list state the shell cannot produce.
    order: { value: 0, source: "default" },
    ...overrides,
  };
}

const PLAIN = row({ id: "1", title: "Plain" });
const UNREAD = row({
  id: "2",
  title: "Touched",
  unread: true,
  origin: "changed by agent · hesperia",
  headRev: "rev-9",
});
const PINNED = row({ id: "3", title: "Kept", pinned: true });
const CONFLICTED = row({ id: "4", title: "Split", conflict: true });

function renderList(rows: NoteRowVm[], onVerb = vi.fn()) {
  render(
    <NoteList
      rows={rows}
      total={rows.length}
      selectedId={null}
      onSelect={vi.fn()}
      onSelectBeside={vi.fn()}
      onToggleTag={vi.fn()}
      onVerb={onVerb}
      onGrow={vi.fn()}
    />,
  );
  return onVerb;
}

describe("NoteList affordances", () => {
  it("carries unread, pinned and conflict state from the view model", () => {
    renderList([PLAIN, UNREAD, PINNED, CONFLICTED]);

    // State reaches assistive technology through the accessible name, not only
    // through a glyph — colour and shape alone are not carriers.
    expect(screen.getByRole("button", { name: /Note, Touched, unread/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Note, Kept, pinned/ })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: /Note, Split, conflicted/ })).toBeInTheDocument();
    // A plain row claims none of them.
    const plain = screen.getByRole("button", { name: /Note, Plain/ });
    expect(plain).not.toHaveAttribute("data-unread");
    expect(plain).not.toHaveAttribute("data-conflict");
  });

  it("shows provenance instead of the excerpt on an unread row", () => {
    renderList([PLAIN, UNREAD]);

    // For a row you have not read, the useful fact is who touched it — the
    // excerpt answers a question you did not ask (FR-114, AD-63).
    expect(screen.getByText("changed by agent · hesperia")).toBeInTheDocument();
    expect(screen.getAllByText("the body excerpt")).toHaveLength(1);
  });

  it("filters by a row's tag chip instead of opening the note", () => {
    const onSelect = vi.fn();
    const onToggleTag = vi.fn();
    render(
      <NoteList
        rows={[row({ id: "5", title: "Tagged", tags: ["work/clients"] })]}
        total={1}
        selectedId={null}
        onSelect={onSelect}
        onSelectBeside={vi.fn()}
        onToggleTag={onToggleTag}
        onVerb={vi.fn()}
        onGrow={vi.fn()}
      />,
    );

    fireEvent.click(screen.getByRole("button", { name: "Tag work/clients, on this note" }));
    expect(onToggleTag).toHaveBeenCalledWith("work/clients");
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("runs the archive, pin and acknowledge verbs on the row under the cursor", () => {
    const onVerb = renderList([PLAIN, PINNED]);
    const list = screen.getByRole("button", { name: /Note, Plain/ });

    // Arrow down puts the cursor on the first row without opening it, then the
    // bare verb keys act on it.
    fireEvent.keyDown(list, { key: "ArrowDown" });
    fireEvent.keyDown(list, { key: "e" });
    fireEvent.keyDown(list, { key: "p" });
    fireEvent.keyDown(list, { key: "u" });

    expect(onVerb).toHaveBeenNthCalledWith(1, PLAIN, "e");
    expect(onVerb).toHaveBeenNthCalledWith(2, PLAIN, "p");
    expect(onVerb).toHaveBeenNthCalledWith(3, PLAIN, "u");
  });

  /**
   * Story 45.17: `Delete` and `Backspace` both ASK, on the row under the
   * cursor and never on the first row.
   *
   * Two rows, and the cursor moved to the SECOND, because "it dispatched a
   * delete" and "it dispatched a delete for the note you were looking at" are
   * different claims and a one-row list cannot tell them apart. Both spellings
   * are bound: `Delete` on a full keyboard, `Backspace` on the laptops this
   * app is used on.
   */
  it("asks to delete the row under the cursor, on both spellings of the key", () => {
    const onVerb = renderList([PLAIN, PINNED]);
    const list = screen.getByRole("button", { name: /Note, Plain/ });

    fireEvent.keyDown(list, { key: "ArrowDown" });
    fireEvent.keyDown(list, { key: "ArrowDown" });
    fireEvent.keyDown(list, { key: "Delete" });
    fireEvent.keyDown(list, { key: "Backspace" });

    expect(onVerb).toHaveBeenNthCalledWith(1, PINNED, "d");
    expect(onVerb).toHaveBeenNthCalledWith(2, PINNED, "d");
  });

  /**
   * And with no cursor there is no row to ask about, so the key does nothing —
   * rather than falling back to the first row, which is how a list deletes a
   * note nobody was looking at.
   */
  it("does nothing on Delete before the cursor has been placed", () => {
    const onVerb = renderList([PLAIN, PINNED]);

    fireEvent.keyDown(screen.getByRole("button", { name: /Note, Plain/ }), { key: "Delete" });

    expect(onVerb).not.toHaveBeenCalled();
  });

  it("moves the cursor without opening the note; Enter is what opens", () => {
    const onSelect = vi.fn();
    render(
      <NoteList
        rows={[PLAIN, UNREAD]}
        total={2}
        selectedId={null}
        onSelect={onSelect}
        onSelectBeside={vi.fn()}
        onToggleTag={vi.fn()}
        onVerb={vi.fn()}
        onGrow={vi.fn()}
      />,
    );
    const list = screen.getByRole("button", { name: /Note, Plain/ });

    // Walking the list must not stream a body per row — that is the whole
    // reason the roving cursor is separate from the open note.
    fireEvent.keyDown(list, { key: "ArrowDown" });
    fireEvent.keyDown(list, { key: "ArrowDown" });
    expect(onSelect).not.toHaveBeenCalled();

    fireEvent.keyDown(list, { key: "Enter" });
    expect(onSelect).toHaveBeenCalledWith(UNREAD);
  });
});

/**
 * The row's context menu, from the list's side: the wiring, not the menu.
 *
 * `note-row.test.tsx` asserts what the menu offers and that each item acts. What
 * can only be checked here is that the list hands the row the two things the
 * menu needs — the verb dispatcher its own keys already use, and the answer to
 * "does this platform have a file manager" — because a menu built correctly and
 * wired to a row the list never fed would pass every assertion in that file and
 * do nothing in the app.
 */
describe("NoteList — the row's menu is the list's verbs", () => {
  afterEach(() => {
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
  });

  it("dispatches a menu item through the same handler its keys use", async () => {
    capabilitiesStore
      .getState()
      .applySnapshot({ ...DEFAULT_CAPABILITIES, notes: true, revealInFileManager: true });
    const onVerb = renderList([PLAIN, UNREAD]);
    const rowButton = screen.getByRole("button", { name: /Note, Plain/ });

    fireEvent.contextMenu(rowButton);
    fireEvent.click(await screen.findByRole("menuitem", { name: NOTE_ROW_PIN_LABEL }));

    // The same `(row, verb)` pair `p` on the focused row sends — one handler,
    // so the pointer and the keyboard cannot drift apart.
    expect(onVerb).toHaveBeenCalledWith(PLAIN, "p");
    fireEvent.keyDown(rowButton, { key: "ArrowDown" });
    fireEvent.keyDown(rowButton, { key: "p" });
    expect(onVerb).toHaveBeenLastCalledWith(PLAIN, "p");
  });

  it("keeps Reveal out of the menu where the platform has no file manager", async () => {
    capabilitiesStore
      .getState()
      .applySnapshot({ ...DEFAULT_CAPABILITIES, notes: true, revealInFileManager: false });
    renderList([PLAIN]);

    fireEvent.contextMenu(screen.getByRole("button", { name: /Note, Plain/ }));
    // The menu is open — this is the gate, not a menu that failed to appear.
    expect(await screen.findByRole("menuitem", { name: NOTE_ROW_PIN_LABEL })).toBeInTheDocument();
    expect(screen.queryByRole("menuitem", { name: NOTE_ROW_REVEAL_LABEL })).toBeNull();
  });
});

/**
 * Story 44.12 — the notes list's own truncation.
 *
 * A row shows three chips and collapses the rest into `+n`. That is AD-83's
 * second step with the third one missing: a count is not the value, and a row
 * that says `+2` and cannot say which two is the property panel's failure in a
 * smaller box.
 */
describe("NoteList — the tags a row could not fit", () => {
  const MANY = row({
    id: "9",
    title: "Tagged",
    tags: ["work", "clients/acme", "q3", "invoices", "urgent"],
  });

  function renderTagged() {
    const onToggleTag = vi.fn();
    const onSelect = vi.fn();
    render(
      <NoteList
        rows={[MANY]}
        total={1}
        selectedId={null}
        onSelect={onSelect}
        onSelectBeside={vi.fn()}
        onToggleTag={onToggleTag}
        onVerb={vi.fn()}
        onGrow={vi.fn()}
      />,
    );
    return { onToggleTag, onSelect };
  }

  it("names the tags it is hiding rather than only counting them", () => {
    renderTagged();

    // The count is still what is drawn — the row is 64 px and five chips do not
    // fit — but the affordance says what it stands for.
    expect(
      screen.getByRole("button", { name: `${NOTE_MORE_TAGS_LABEL} invoices, urgent` }),
    ).toHaveTextContent("+2");
  });

  it("opens the hidden tags, and each one still filters", () => {
    const { onToggleTag, onSelect } = renderTagged();

    fireEvent.click(
      screen.getByRole("button", { name: `${NOTE_MORE_TAGS_LABEL} invoices, urgent` }),
    );
    // Opening the panel is not opening the note — the trigger sits inside the
    // row's own button, so without the guard this alone would stream a body.
    expect(onSelect).not.toHaveBeenCalled();

    const panel = screen.getByRole("dialog");
    expect(within(panel).getByRole("button", { name: "Tag invoices, on this note" })).toBeVisible();
    fireEvent.click(within(panel).getByRole("button", { name: "Tag urgent, on this note" }));

    expect(onToggleTag).toHaveBeenCalledWith("urgent");
    // A React portal still propagates through the React tree, so a panel click
    // that reached the row would open a note the user was only reading a tag off.
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("says nothing where every tag already fits", () => {
    render(
      <NoteList
        rows={[row({ id: "8", title: "Few", tags: ["work", "q3"] })]}
        total={1}
        selectedId={null}
        onSelect={vi.fn()}
        onSelectBeside={vi.fn()}
        onToggleTag={vi.fn()}
        onVerb={vi.fn()}
        onGrow={vi.fn()}
      />,
    );

    expect(screen.queryByRole("button", { name: new RegExp(NOTE_MORE_TAGS_LABEL) })).toBeNull();
  });
});

/**
 * Story 44.5 — the list's side of it.
 *
 * `note-row.test.tsx` proves one row renders one order. What only the list can
 * get wrong is the two things below: showing an order on SOME rows, which is the
 * half-ordered list the default exists to prevent; and re-deriving the order
 * here, which would make the webview a second opinion about a sequence Rust
 * already decided.
 */
describe("NoteList — the order beside every note", () => {
  /** Rows exactly as `notes_list` hands them over: already sorted by Rust. */
  const SORTED = [
    row({ id: "f", title: "First", order: { value: -1, source: "own" } }),
    row({ id: "d1", title: "Alpha", order: { value: 0, source: "default" } }),
    row({ id: "d2", title: "Beta", order: { value: 0, source: "unreadable" } }),
    row({ id: "l", title: "Last", order: { value: 10, source: "own" } }),
  ];

  function orderCells(): string[] {
    return Array.from(document.querySelectorAll('[data-slot="note-order"]')).map(
      (cell) => cell.textContent ?? "",
    );
  }

  it("shows an order on every row, never on only the placed ones", () => {
    renderList(SORTED);

    // Four rows, four orders. A list where the defaulted notes showed nothing is
    // the half-ordered list of the story's first sentence.
    expect(orderCells()).toHaveLength(SORTED.length);
    expect(orderCells().every((text) => text !== "")).toBe(true);
  });

  it("renders the orders in the sequence Rust sent, without re-sorting them", () => {
    renderList(SORTED);

    // Ascending here only because the shell already sorted; the assertion is that
    // the list echoed its input. A webview that sorted by `order` itself would
    // pass this and then disagree with Rust the moment the space's sort is `name`
    // — which is why the sort lives in exactly one place.
    expect(orderCells()).toEqual(["-1", "0", `0${NOTE_ORDER_UNREADABLE_MARK}`, "10"]);

    const reversed = [...SORTED].reverse();
    cleanup();
    renderList(reversed);
    expect(orderCells()).toEqual([`10`, `0${NOTE_ORDER_UNREADABLE_MARK}`, "0", "-1"]);
  });
});

/**
 * Story 44.10 — a vault, not a screenful.
 *
 * Every assertion here counts MOUNTED rows, because that is the only thing
 * AD-84 is about. `withListGeometry` is not optional scaffolding: jsdom lays
 * nothing out, so without it the scroll offset can never leave zero and a list
 * that mounted all ten thousand rows in a browser would satisfy every one of
 * these on the first window it happened to render.
 *
 * What this cannot prove is named plainly: whether a real note row is really
 * 64 px at the real font. That is a browser fact, and this is not a browser.
 */
describe("NoteList — a vault, not a screenful", () => {
  /** Ten rows fit; nothing exists above row 0 to overscan into. */
  const VISIBLE_ROWS = 10;
  const OVERSCAN = 8;

  const MANY = Array.from({ length: 4000 }, (_, index) =>
    row({ id: `n${index}`, title: `Note ${index}` }),
  );

  let geometry: ListGeometry | null = null;

  afterEach(() => {
    geometry?.undo();
    geometry = null;
  });

  function install(): void {
    geometry = withListGeometry({ viewport: VISIBLE_ROWS * NOTE_ROW_HEIGHT, row: NOTE_ROW_HEIGHT });
  }

  function mountedRows(): number[] {
    return Array.from(document.querySelectorAll(`[${WINDOW_ROW_ATTR}]`)).map((element) =>
      Number(element.getAttribute(WINDOW_ROW_ATTR)),
    );
  }

  function viewport(): HTMLElement {
    const element = document.querySelector(`[${WINDOW_VIEWPORT_ATTR}]`);
    if (!(element instanceof HTMLElement)) {
      throw new Error("the note list has no scroll viewport");
    }
    return element;
  }

  it("mounts a window over four thousand notes, not four thousand rows", () => {
    install();
    renderList(MANY);

    expect(mountedRows()).toHaveLength(VISIBLE_ROWS + OVERSCAN);
    expect(screen.getByRole("button", { name: /Note, Note 0/ })).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /Note, Note 3999/ })).toBeNull();
  });

  it("reaches the last note by scrolling, and still mounts only a window", () => {
    install();
    renderList(MANY);

    act(() =>
      geometry?.scrollTo(
        viewport(),
        MANY.length * NOTE_ROW_HEIGHT - VISIBLE_ROWS * NOTE_ROW_HEIGHT,
      ),
    );

    expect(screen.getByRole("button", { name: /Note, Note 3999/ })).toBeInTheDocument();
    // Everything between the two ends is gone. Note 0 is NOT: it is the tab
    // stop, and the window keeps it mounted deliberately (see the tab-order
    // test below) — one row's worth of cost for a surface Tab can still enter.
    expect(mountedRows()).not.toContain(2000);
    expect(mountedRows()).toContain(0);
    expect(mountedRows().length).toBeLessThanOrEqual(VISIBLE_ROWS + OVERSCAN * 2 + 1);
  });

  it("moves the cursor to a row that was never rendered, and lands focus on it", () => {
    install();
    renderList(MANY);

    // `↑` with no cursor wraps to the LAST note — three thousand nine hundred
    // rows past anything in the DOM. Focus has to land on an element that only
    // exists because the move put it there.
    fireEvent.keyDown(screen.getByRole("button", { name: /Note, Note 0/ }), { key: "ArrowUp" });

    const last = screen.getByRole("button", { name: /Note, Note 3999/ });
    expect(document.activeElement).toBe(last);
    // And one step back is the row before it, not a DOM sibling that happens to
    // be adjacent in the window.
    fireEvent.keyDown(last, { key: "ArrowUp" });
    expect(document.activeElement).toBe(screen.getByRole("button", { name: /Note, Note 3998/ }));
  });

  it("keeps the open note selected across scrolling it out of view and back", () => {
    install();
    render(
      <NoteList
        rows={MANY}
        total={MANY.length}
        selectedId="n2"
        onSelect={vi.fn()}
        onSelectBeside={vi.fn()}
        onToggleTag={vi.fn()}
        onVerb={vi.fn()}
        onGrow={vi.fn()}
      />,
    );
    expect(screen.getByRole("button", { name: /Note, Note 2/ })).toHaveAttribute(
      "aria-current",
      "true",
    );

    act(() => geometry?.scrollTo(viewport(), 100_000));
    act(() => geometry?.scrollTo(viewport(), 0));

    // Still selected, and the list did not decide to go somewhere else on the
    // way back.
    expect(screen.getByRole("button", { name: /Note, Note 2/ })).toHaveAttribute(
      "aria-current",
      "true",
    );
    expect(viewport().scrollTop).toBe(0);
  });

  it("keeps exactly one note in the tab order, wherever the list is scrolled", () => {
    install();
    renderList(MANY);
    const stops = () => document.querySelectorAll('[data-slot="note-row"][tabindex="0"]');

    expect(stops()).toHaveLength(1);

    act(() => geometry?.scrollTo(viewport(), 100_000));

    // The tab stop is row 0, now nowhere near the viewport. Unmounting it would
    // leave the notes list with no tab stop at all and Tab would skip it.
    expect(stops()).toHaveLength(1);
    expect(stops()[0]).toHaveAccessibleName(/Note, Note 0/);
  });
});
