import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteSpaceVm } from "@/lib/ipc/client";

// Mock the typed IPC client so the list never touches Tauri. The editor this
// list opens reaches for three more commands, so they are stubbed here too.
vi.mock("@/lib/ipc/client", () => ({
  notesSpaces: vi.fn(),
  notesSpaceTerms: vi.fn(),
  notesSpaceSave: vi.fn(),
  notesTagTree: vi.fn(),
}));

import { SpaceList } from "@/components/notes/space-list";
import { notesSpaceSave, notesSpaces, notesSpaceTerms, notesTagTree } from "@/lib/ipc/client";
import { notesFiltersStore, resetNotesFiltersStoreForTest } from "@/lib/stores/notes-filters";

const mockSpaces = vi.mocked(notesSpaces);
const mockTerms = vi.mocked(notesSpaceTerms);
const mockSave = vi.mocked(notesSpaceSave);
const mockTagTree = vi.mocked(notesTagTree);

function space(p: Partial<NoteSpaceVm> & Pick<NoteSpaceVm, "id" | "name">): NoteSpaceVm {
  return {
    id: p.id,
    name: p.name,
    query: p.query ?? "tag:client/acme",
    sort: p.sort ?? "modified desc",
    limit: p.limit ?? 500,
    icon: p.icon ?? null,
    error: p.error ?? null,
  };
}

beforeEach(() => {
  mockSpaces.mockReset();
  mockTerms.mockReset();
  mockSave.mockReset();
  mockTagTree.mockReset();
  mockTagTree.mockResolvedValue({ nodes: [] });
  mockTerms.mockResolvedValue({
    kind: "chips",
    tags: [{ tag: "client/acme", term: "include" }],
    flags: [],
    origin: null,
    text: null,
  });
  resetNotesFiltersStoreForTest();
});

afterEach(() => {
  vi.clearAllMocks();
  resetNotesFiltersStoreForTest();
});

describe("SpaceList rows", () => {
  it("selects a space as a scope without navigating away from the open note", async () => {
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Active work" })]);
    render(<SpaceList vaultId="vault-1" />);

    fireEvent.click(await screen.findByRole("button", { name: "Active work" }));

    expect(notesFiltersStore.getState().scope).toEqual({
      kind: "space",
      id: "s1",
      name: "Active work",
    });
  });

  it("says a broken space is broken in its accessible name, not only with a dot", async () => {
    mockSpaces.mockResolvedValue([
      space({ id: "s1", name: "Broken", error: "unknown search key `nope`" }),
    ]);
    render(<SpaceList vaultId="vault-1" />);

    expect(
      await screen.findByRole("button", { name: /Broken, This space's query can't be read/ }),
    ).toBeInTheDocument();
  });
});

describe("SpaceList icons", () => {
  it("draws the icon the space stored", async () => {
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Starred", icon: "star" })]);
    const { container } = render(<SpaceList vaultId="vault-1" />);

    await screen.findByRole("button", { name: "Starred" });
    expect(container.querySelector('[data-slot="space-icon"]')).toHaveAttribute(
      "data-space-icon",
      "star",
    );
  });

  /**
   * The decision this pins: an icon set that shrinks must never leave a row with
   * a hole where every sibling has a glyph, and it must never rewrite the stored
   * name to make that true. The row draws the fallback and the value on disk is
   * still `sparkles`.
   */
  it("draws the fallback glyph for an icon name that is not in the set any more, and keeps the name", async () => {
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Old", icon: "sparkles" })]);
    const { container } = render(<SpaceList vaultId="vault-1" />);

    await screen.findByRole("button", { name: "Old" });
    const glyph = container.querySelector('[data-slot="space-icon"]');
    expect(glyph).toBeInTheDocument();
    expect(glyph).toHaveAttribute("data-space-icon", "sparkles");

    // And the unknown name survives a save that only changed the title.
    fireEvent.click(screen.getByRole("button", { name: "Edit space Old" }));
    fireEvent.change(await screen.findByLabelText("Name"), { target: { value: "Older" } });
    mockSave.mockResolvedValue({ vaultId: "vault-1", id: "s1", path: "spaces/x.md", title: "" });
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(mockSave.mock.calls[0]?.[1].icon).toBe("sparkles");
  });

  it("draws a glyph for a space with no icon rather than nothing", async () => {
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Plain", icon: null })]);
    const { container } = render(<SpaceList vaultId="vault-1" />);

    await screen.findByRole("button", { name: "Plain" });
    const glyph = container.querySelector('[data-slot="space-icon"]');
    expect(glyph).toBeInTheDocument();
    expect(glyph).toHaveAttribute("data-space-icon", "none");
  });
});

describe("SpaceList editing", () => {
  it("opens the editor for the row that was pressed", async () => {
    mockSpaces.mockResolvedValue([
      space({ id: "s1", name: "Active work" }),
      space({ id: "s2", name: "Archive triage" }),
    ]);
    render(<SpaceList vaultId="vault-1" />);

    fireEvent.click(await screen.findByRole("button", { name: "Edit space Archive triage" }));

    expect(await screen.findByLabelText("Name")).toHaveValue("Archive triage");
  });

  it("re-reads the list after a save, so the sidebar shows the new name", async () => {
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Active work" })]);
    mockSave.mockResolvedValue({
      vaultId: "vault-1",
      id: "s1",
      path: "spaces/renamed.md",
      title: "Renamed",
    });
    render(<SpaceList vaultId="vault-1" />);

    fireEvent.click(await screen.findByRole("button", { name: "Edit space Active work" }));
    fireEvent.change(await screen.findByLabelText("Name"), { target: { value: "Renamed" } });
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Renamed" })]);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByRole("button", { name: "Renamed" })).toBeInTheDocument();
    expect(mockSpaces).toHaveBeenCalledTimes(2);
  });

  it("leaves the list alone when the editor is cancelled", async () => {
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Active work" })]);
    render(<SpaceList vaultId="vault-1" />);

    fireEvent.click(await screen.findByRole("button", { name: "Edit space Active work" }));
    await screen.findByLabelText("Name");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    await waitFor(() => expect(screen.queryByLabelText("Name")).not.toBeInTheDocument());
    expect(mockSave).not.toHaveBeenCalled();
    expect(mockSpaces).toHaveBeenCalledTimes(1);
  });
});
