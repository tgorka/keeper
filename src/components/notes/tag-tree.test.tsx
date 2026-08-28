import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteTagNodeVm, NoteTagTreeVm } from "@/lib/ipc/client";

// Mock the typed IPC client so the tree never touches Tauri.
vi.mock("@/lib/ipc/client", () => ({
  notesTagTree: vi.fn(),
}));

import { TagTree } from "@/components/notes/tag-tree";
import { notesTagTree } from "@/lib/ipc/client";
import { notesFiltersStore, resetNotesFiltersStoreForTest } from "@/lib/stores/notes-filters";

const mockTagTree = vi.mocked(notesTagTree);

function node(p: Partial<NoteTagNodeVm> & Pick<NoteTagNodeVm, "path" | "count">): NoteTagNodeVm {
  return {
    name: p.name ?? p.path.split("/").pop() ?? p.path,
    path: p.path,
    count: p.count,
    children: p.children ?? [],
  };
}

/**
 * The Story 42.5 counts matrix row: 2 notes and 3 recordings carry
 * `client/acme`, and one more note carries `client/beta`. Rust sums the
 * producers, so `client/acme` is 5 and its parent is 6.
 */
const SUMMED: NoteTagTreeVm = {
  nodes: [
    node({
      path: "client",
      count: 6,
      children: [node({ path: "client/acme", count: 5 }), node({ path: "client/beta", count: 1 })],
    }),
  ],
};

beforeEach(() => {
  mockTagTree.mockReset();
  mockTagTree.mockResolvedValue(SUMMED);
  resetNotesFiltersStoreForTest();
});

afterEach(() => {
  vi.clearAllMocks();
  resetNotesFiltersStoreForTest();
});

describe("TagTree counts", () => {
  it("renders the count the tree reports, which sums notes and recordings (2 notes + 3 recordings under client/acme shows 5)", async () => {
    render(<TagTree vaultId="vault-1" />);

    // The parent first: 6 things under `client`, whichever producer put them there.
    const parent = await screen.findByRole("button", { name: "Tag client, 6 items, filter" });
    expect(parent).toHaveTextContent("6");

    // Expand to reach the leaf the matrix names.
    fireEvent.click(parent.previousElementSibling as HTMLElement);

    const leaf = await screen.findByRole("button", {
      name: "Tag client/acme, 5 items, filter",
    });
    expect(leaf).toHaveTextContent("5");
  });

  it("says 'items', not 'notes', because the number behind a node is no longer one producer", async () => {
    render(<TagTree vaultId="vault-1" />);

    await screen.findByRole("button", { name: "Tag client, 6 items, filter" });
    expect(screen.queryByRole("button", { name: /6 notes/ })).toBeNull();
  });

  it("passes the reported count straight through — nothing here filters a producer out", async () => {
    mockTagTree.mockResolvedValue({ nodes: [node({ path: "q3", count: 3 })] });
    render(<TagTree vaultId="vault-1" />);

    // Three recordings and no notes at all: the tree still shows 3.
    expect(await screen.findByRole("button", { name: "Tag q3, 3 items, filter" })).toBeVisible();
  });

  it("renders nothing at all without a vault, and asks Rust for nothing", () => {
    const { container } = render(<TagTree vaultId={null} />);

    expect(mockTagTree).not.toHaveBeenCalled();
    expect(container).toBeEmptyDOMElement();
  });
});

describe("TagTree tag states", () => {
  it("cycles a node through include, exclude and off on plain presses", async () => {
    render(<TagTree vaultId="vault-1" />);
    const terms = () => notesFiltersStore.getState().tagTerms;

    fireEvent.click(await screen.findByRole("button", { name: "Tag client, 6 items, filter" }));
    expect(terms()).toEqual([{ tag: "client", term: "include" }]);

    // The second press must reach exclude. A plain press clears the rest of the
    // bar first, and reading the state after that clear — the obvious way to
    // write this — would restart the cycle at include forever, leaving exclude
    // reachable only with the shift key.
    fireEvent.click(
      await screen.findByRole("button", {
        name: "Tag client, 6 items: included. Exclude it instead.",
      }),
    );
    expect(terms()).toEqual([{ tag: "client", term: "exclude" }]);

    fireEvent.click(
      await screen.findByRole("button", {
        name: "Tag client, 6 items: excluded. Stop filtering by it.",
      }),
    );
    expect(terms()).toEqual([]);
  });

  it("shows an excluded node as excluded without being hovered", async () => {
    notesFiltersStore.getState().setTagTerm("client", "include");
    const { rerender } = render(<TagTree vaultId="vault-1" />);
    const includedClass = (
      await screen.findByRole("button", {
        name: "Tag client, 6 items: included. Exclude it instead.",
      })
    ).className;

    notesFiltersStore.getState().setTagTerm("client", "exclude");
    rerender(<TagTree vaultId="vault-1" />);

    const excluded = await screen.findByRole("button", {
      name: "Tag client, 6 items: excluded. Stop filtering by it.",
    });
    // Not `aria-selected`: an excluded node is emphatically not selected, and a
    // reader arrowing the tree must not be told it is.
    expect(excluded.closest("[role=treeitem]")).toHaveAttribute("aria-selected", "false");
    expect(excluded.querySelector("svg")).not.toBeNull();
    // And it must not look like an included one. A node that reads as selected
    // while it is removing notes is the exact confusion the sign exists against.
    expect(excluded.className).not.toBe(includedClass);
  });

  it("adds to the intersection on a shift press instead of replacing it", async () => {
    render(<TagTree vaultId="vault-1" />);

    const parent = await screen.findByRole("button", { name: "Tag client, 6 items, filter" });
    fireEvent.click(parent);
    fireEvent.click(parent.previousElementSibling as HTMLElement);
    fireEvent.click(
      await screen.findByRole("button", { name: "Tag client/acme, 5 items, filter" }),
      { shiftKey: true },
    );

    expect(notesFiltersStore.getState().tagTerms).toEqual([
      { tag: "client", term: "include" },
      { tag: "client/acme", term: "include" },
    ]);
  });
});
