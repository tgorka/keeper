import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteTagNodeVm, NoteTagTreeVm } from "@/lib/ipc/client";

// Mock the typed IPC client so the tree never touches Tauri.
vi.mock("@/lib/ipc/client", () => ({
  notesTagTree: vi.fn(),
}));

import { TagTree } from "@/components/notes/tag-tree";
import { notesTagTree } from "@/lib/ipc/client";
import { resetNotesFiltersStoreForTest } from "@/lib/stores/notes-filters";

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
