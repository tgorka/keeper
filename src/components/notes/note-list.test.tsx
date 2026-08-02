import { fireEvent, render, screen } from "@testing-library/react";
import { beforeAll, describe, expect, it, vi } from "vitest";
import { NoteList } from "@/components/notes/note-list";
import type { NoteRowVm } from "@/lib/ipc/client";

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
    tags: [],
    updatedMs: Date.now() - 3_600_000,
    pinned: false,
    archived: false,
    unread: false,
    conflict: false,
    origin: "",
    headRev: "",
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

  it("moves the cursor without opening the note; Enter is what opens", () => {
    const onSelect = vi.fn();
    render(
      <NoteList
        rows={[PLAIN, UNREAD]}
        total={2}
        selectedId={null}
        onSelect={onSelect}
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
