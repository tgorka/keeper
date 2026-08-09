import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteSpaceVm } from "@/lib/ipc/client";

// Mock the typed IPC client so the list never touches Tauri. The editor this
// list opens reaches for three more commands, so they are stubbed here too.
vi.mock("@/lib/ipc/client", () => ({
  notesSpaces: vi.fn(),
  notesSpacesRestoreDefaults: vi.fn(),
  notesSpaceTerms: vi.fn(),
  notesSpaceSave: vi.fn(),
  notesTagTree: vi.fn(),
}));

import {
  RESTORE_DEFAULTS,
  RESTORE_FAILED,
  RESTORE_NOTHING_MISSING,
  SpaceList,
} from "@/components/notes/space-list";
import {
  notesSpaceSave,
  notesSpaces,
  notesSpacesRestoreDefaults,
  notesSpaceTerms,
  notesTagTree,
} from "@/lib/ipc/client";
import { notesFiltersStore, resetNotesFiltersStoreForTest } from "@/lib/stores/notes-filters";

const mockSpaces = vi.mocked(notesSpaces);
const mockRestore = vi.mocked(notesSpacesRestoreDefaults);
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
    defaultKey: p.defaultKey ?? null,
    error: p.error ?? null,
  };
}

beforeEach(() => {
  mockSpaces.mockReset();
  mockTerms.mockReset();
  mockRestore.mockReset();
  mockRestore.mockResolvedValue(0);
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
      defaultKey: null,
    });
  });

  /**
   * The marker rides onto the scope, because the pane's "no recording notes
   * yet" sentence follows the space rather than a name or a scope kind. A row
   * that dropped it would leave that sentence unreachable for a renamed
   * Recordings space, which is exactly the bug the marker exists to prevent.
   */
  it("carries a seeded default's key onto the scope, so a renamed default is still itself", async () => {
    mockSpaces.mockResolvedValue([
      space({ id: "s1", name: "Sessions", icon: "video", defaultKey: "recordings" }),
    ]);
    render(<SpaceList vaultId="vault-1" />);

    fireEvent.click(await screen.findByRole("button", { name: "Sessions" }));

    expect(notesFiltersStore.getState().scope).toEqual({
      kind: "space",
      id: "s1",
      name: "Sessions",
      defaultKey: "recordings",
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

describe("SpaceList restore", () => {
  /**
   * The section is the rail now (Story 44.3). It used to render `null` on an
   * empty list, which would leave a vault whose owner deleted every default with
   * no control anywhere that could bring them back.
   */
  it("shows the restore control on a vault with no spaces at all", async () => {
    mockSpaces.mockResolvedValue([]);
    render(<SpaceList vaultId="vault-1" />);

    expect(await screen.findByRole("button", { name: RESTORE_DEFAULTS })).toBeInTheDocument();
  });

  it("re-reads the list after restoring, so the recreated spaces appear", async () => {
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Active work" })]);
    mockRestore.mockResolvedValue(2);
    render(<SpaceList vaultId="vault-1" />);
    await screen.findByRole("button", { name: "Active work" });

    mockSpaces.mockResolvedValue([
      space({ id: "s1", name: "Active work" }),
      space({ id: "s2", name: "Inbox", icon: "inbox", defaultKey: "inbox" }),
      space({ id: "s3", name: "Pinned", icon: "pin", defaultKey: "pinned" }),
    ]);
    fireEvent.click(screen.getByRole("button", { name: RESTORE_DEFAULTS }));

    expect(await screen.findByRole("button", { name: "Inbox" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Pinned" })).toBeInTheDocument();
    expect(await screen.findByText("Restored 2 spaces.")).toBeInTheDocument();
    expect(mockRestore).toHaveBeenCalledWith("vault-1");
  });

  /**
   * The control's promise is that it never touches a space that is there, so
   * the case where it wrote nothing has to say so rather than flash a success.
   */
  it("says nothing was missing rather than claiming it restored something", async () => {
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Inbox", defaultKey: "inbox" })]);
    mockRestore.mockResolvedValue(0);
    render(<SpaceList vaultId="vault-1" />);
    await screen.findByRole("button", { name: "Inbox" });

    fireEvent.click(screen.getByRole("button", { name: RESTORE_DEFAULTS }));

    expect(await screen.findByText(RESTORE_NOTHING_MISSING)).toBeInTheDocument();
    expect(screen.queryByText(/Restored/)).not.toBeInTheDocument();
  });

  it("says so when keeper could not write, and leaves the list as it was", async () => {
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Active work" })]);
    mockRestore.mockRejectedValue(new Error("read-only volume"));
    render(<SpaceList vaultId="vault-1" />);
    await screen.findByRole("button", { name: "Active work" });

    fireEvent.click(screen.getByRole("button", { name: RESTORE_DEFAULTS }));

    expect(await screen.findByText(RESTORE_FAILED)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Active work" })).toBeInTheDocument();
    // A failed restore is not a reason to re-read a list nothing changed in.
    expect(mockSpaces).toHaveBeenCalledTimes(1);
  });

  it("cannot be pressed with no vault open", async () => {
    render(<SpaceList vaultId={null} />);

    const control = await screen.findByRole("button", { name: RESTORE_DEFAULTS });
    expect(control).toBeDisabled();
    fireEvent.click(control);
    expect(mockRestore).not.toHaveBeenCalled();
  });
});
