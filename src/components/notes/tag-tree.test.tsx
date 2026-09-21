import { fireEvent, render, screen, within } from "@testing-library/react";
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

    const parent = await screen.findByRole("button", { name: "Include tag client" });
    const item = parent.closest('[role="treeitem"]') as HTMLElement;
    expect(
      within(item).getByText((_, element) => element?.textContent === "6 items"),
    ).toBeVisible();
    fireEvent.click(item.querySelector('button[aria-hidden="true"]') as HTMLElement);
    const leaf = await screen.findByRole("button", { name: "Include tag client/acme" });
    expect(
      within(leaf.closest('[role="treeitem"]') as HTMLElement).getByText(
        (_, element) => element?.textContent === "5 items",
      ),
    ).toBeVisible();
  });

  it("renders nothing at all without a vault, and asks Rust for nothing", () => {
    const { container } = render(<TagTree vaultId={null} />);

    expect(mockTagTree).not.toHaveBeenCalled();
    expect(container).toBeEmptyDOMElement();
  });
});

describe("TagTree tag states", () => {
  it("toggles only on the sign, preserves siblings and removes only with ×", async () => {
    notesFiltersStore.getState().setTagTerm("other", "include");
    render(<TagTree vaultId="vault-1" />);
    fireEvent.click(await screen.findByRole("button", { name: "Include tag client" }));
    fireEvent.click(await screen.findByRole("button", { name: "Exclude tag client" }));
    fireEvent.click(await screen.findByRole("button", { name: "Include tag client" }));
    fireEvent.click(screen.getByText("client"));
    expect(notesFiltersStore.getState().tagTerms).toEqual([
      { tag: "other", term: "include" },
      { tag: "client", term: "include" },
    ]);
    fireEvent.click(screen.getByRole("button", { name: "Clear tag client filter" }));
    expect(notesFiltersStore.getState().tagTerms).toEqual([{ tag: "other", term: "include" }]);
  });

  it("shows an excluded node as excluded without being hovered", async () => {
    notesFiltersStore.getState().setTagTerm("client", "include");
    const { rerender } = render(<TagTree vaultId="vault-1" />);
    const includedClass = (
      await screen.findByRole("button", {
        name: "Exclude tag client",
      })
    ).closest('[data-slot="filter-chip"]')?.className;

    notesFiltersStore.getState().setTagTerm("client", "exclude");
    rerender(<TagTree vaultId="vault-1" />);

    const excluded = await screen.findByRole("button", {
      name: "Include tag client",
    });
    // Not `aria-selected`: an excluded node is emphatically not selected, and a
    // reader arrowing the tree must not be told it is.
    expect(excluded.closest("[role=treeitem]")).toHaveAttribute("aria-selected", "false");
    expect(excluded.querySelector("svg")).not.toBeNull();
    // And it must not look like an included one. A node that reads as selected
    // while it is removing notes is the exact confusion the sign exists against.
    expect(excluded.closest('[data-slot="filter-chip"]')?.className).not.toBe(includedClass);
  });
});
