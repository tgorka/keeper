import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { NoteFilterBar } from "@/components/notes/note-filter-bar";
import type { NoteRailVm, NoteSpaceVm } from "@/lib/ipc/client";
import { ALL_SPACE_ID } from "@/lib/notes/all-spaces";
import { ALL_NOTES_SCOPE } from "@/lib/stores/notes-filters";

// Mock the typed IPC client so the list never touches Tauri. The editor this
// list opens reaches for four more commands, and the delete confirmation for
// two, so they are stubbed here too.
vi.mock("@/lib/ipc/client", () => ({
  notesSpaces: vi.fn(),
  notesSpacesRestoreDefaults: vi.fn(),
  notesSpaceTerms: vi.fn(),
  notesSpaceSave: vi.fn(),
  notesSpaceTouch: vi.fn(),
  notesSpacePark: vi.fn(),
  notesTagTree: vi.fn(),
  notesTemplates: vi.fn(),
  notesDeletePlan: vi.fn(),
  notesDelete: vi.fn(),
  tagsVocabulary: vi.fn(async () => ({ entries: [] })),
  notesIncludePrivateGet: vi.fn(async () => false),
  notesIncludePrivateSet: vi.fn(async () => {}),
}));

import {
  NOTE_DELETE_CANCEL,
  NOTE_DELETE_CONFIRM,
  NOTE_DELETE_TESTID,
} from "@/components/notes/note-delete-dialog";
import {
  DELETE_SPACE,
  RESTORE_DEFAULTS,
  RESTORE_FAILED,
  RESTORE_NOTHING_MISSING,
  SPACE_SETTINGS_SUBTITLE,
  SpaceList,
} from "@/components/notes/space-list";
import {
  notesDelete,
  notesDeletePlan,
  notesSpacePark,
  notesSpaceSave,
  notesSpaces,
  notesSpacesRestoreDefaults,
  notesSpaceTerms,
  notesSpaceTouch,
  notesTagTree,
  notesTemplates,
} from "@/lib/ipc/client";
import { notesFiltersStore, resetNotesFiltersStoreForTest } from "@/lib/stores/notes-filters";

import { notesVaultsStore } from "@/lib/stores/notes-vaults";

function rail(rows: NoteSpaceVm[]): NoteRailVm {
  return {
    rows,
    vaults: [
      ...new Map(
        rows.map((row) => [
          row.vaultId,
          { vaultId: row.vaultId, vaultName: row.vaultName, available: true, reason: "" },
        ]),
      ).values(),
    ],
  };
}

const mockSpaces = {
  mockReset: () => vi.mocked(notesSpaces).mockReset(),
  mockResolvedValue: (rows: NoteSpaceVm[]) => vi.mocked(notesSpaces).mockResolvedValue(rail(rows)),
  mockResolvedValueOnce: (rows: NoteSpaceVm[]) => {
    vi.mocked(notesSpaces).mockResolvedValueOnce(rail(rows));
    return mockSpaces;
  },
};
const mockRestore = vi.mocked(notesSpacesRestoreDefaults);
const mockTerms = vi.mocked(notesSpaceTerms);
const mockSave = vi.mocked(notesSpaceSave);
const mockTagTree = vi.mocked(notesTagTree);
const mockTemplates = vi.mocked(notesTemplates);
const mockDeletePlan = vi.mocked(notesDeletePlan);
const mockDelete = vi.mocked(notesDelete);

function space(p: Partial<NoteSpaceVm> & Pick<NoteSpaceVm, "id" | "name">): NoteSpaceVm {
  return {
    id: p.id,
    name: p.name,
    vaultId: p.vaultId ?? "vault-1",
    vaultName: p.vaultName ?? "Personal",
    updatedMs: p.updatedMs ?? null,
    query: p.query ?? "tag:client/acme",
    sort: p.sort ?? "modified desc",
    sortEffective: p.sortEffective ?? "modified desc",
    limit: p.limit ?? 500,
    icon: p.icon ?? null,
    defaultKey: p.defaultKey ?? null,
    template: p.template ?? null,
    folder: p.folder ?? null,
    warnings: p.warnings ?? [],
    order: p.order ?? 0,
    error: p.error ?? null,
    pinned: p.pinned ?? false,
    ttlHours: p.ttlHours ?? null,
    expiresMs: p.expiresMs ?? null,
    text: p.text ?? null,
    parent: p.parent ?? null,
    leafName: p.leafName ?? p.name,
    depth: p.depth ?? 0,
    descendants: p.descendants ?? 0,
    expiryPhrase: p.expiryPhrase ?? "",
    restore: p.restore ?? {
      tagTerms: {},
      flags: [],
      origin: null,
      text: p.text ?? null,
      sort: p.sort ?? "modified desc",
      opaque: false,
    },
  };
}

beforeEach(() => {
  mockSpaces.mockReset();
  mockTerms.mockReset();
  mockRestore.mockReset();
  mockRestore.mockResolvedValue(0);
  vi.mocked(notesSpaceTouch).mockImplementation(async (_vault, id) => {
    const results = vi.mocked(notesSpaces).mock.results;
    const result = results[results.length - 1];
    if (result?.type !== "return") throw new Error("Space list missing");
    const listing: NoteRailVm = await result.value;
    const found = listing.rows.find((row) => row.id === id);
    if (!found) throw new Error("Space missing");
    return found;
  });
  vi.mocked(notesSpacePark).mockReset();
  mockSave.mockReset();
  mockTagTree.mockReset();
  mockTagTree.mockResolvedValue({ nodes: [] });
  mockTemplates.mockReset();
  mockTemplates.mockResolvedValue([]);
  mockTerms.mockResolvedValue({
    kind: "chips",
    tags: [{ tag: "client/acme", term: "include" }],
    flags: [],
    origin: null,
    text: null,
    fields: [],
  });
  mockDeletePlan.mockReset();
  mockDelete.mockReset();
  mockDelete.mockResolvedValue(undefined);
  resetNotesFiltersStoreForTest();
  notesVaultsStore.setState({ activeVaultId: "vault-1" });
});

afterEach(() => {
  vi.clearAllMocks();
  resetNotesFiltersStoreForTest();
});

async function chooseSpaceAction(name: string, action: string) {
  fireEvent.contextMenu(await screen.findByRole("button", { name }));
  fireEvent.click(await screen.findByRole("menuitem", { name: action }));
}

describe("SpaceList hierarchy and lifetime", () => {
  it("folds virtual groups without scoping, and preserves the fold through refresh", async () => {
    const group = space({ id: "keeper:group:Journal", name: "Journal", descendants: 1, query: "" });
    const bali = space({
      id: "bali",
      name: "Journal/Bali",
      leafName: "Bali",
      parent: group.id,
      depth: 1,
    });
    mockSpaces.mockResolvedValue([group, bali]);
    render(<SpaceList vaultId="vault-1" />);
    const disclosure = await screen.findByRole("button", { name: "Journal" });
    expect(screen.getByRole("button", { name: "Journal/Bali" })).toBeVisible();
    fireEvent.click(disclosure);
    expect(screen.queryByRole("button", { name: "Journal/Bali" })).not.toBeInTheDocument();
    expect(notesFiltersStore.getState().scope).toEqual(ALL_NOTES_SCOPE);
    fireEvent.click(screen.getByRole("button", { name: RESTORE_DEFAULTS }));
    await screen.findByText(RESTORE_NOTHING_MISSING);
    expect(screen.queryByRole("button", { name: "Journal/Bali" })).not.toBeInTheDocument();
    fireEvent.click(disclosure);
    expect(screen.getByRole("button", { name: "Journal/Bali" })).toBeVisible();
  });

  it("creates a new space from a group without selecting the draft", async () => {
    const group = space({ id: "keeper:group:Journal", name: "Journal", descendants: 1, query: "" });
    mockSpaces.mockResolvedValue([group]);
    render(<SpaceList vaultId="vault-1" />);
    await chooseSpaceAction("Journal", "New space…");
    expect(await screen.findByLabelText("Name")).toHaveValue("Journal/Untitled space");
    fireEvent.change(screen.getByLabelText("Search prompt"), { target: { value: "Bali budget" } });
    const saved = space({
      id: "new",
      name: "Journal/Untitled space",
      leafName: "Untitled space",
      parent: group.id,
      depth: 1,
      text: "Bali budget",
    });
    mockSave.mockResolvedValue(saved);
    mockSpaces.mockResolvedValue([group, saved]);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("button", { name: saved.name })).toBeVisible();
    expect(notesFiltersStore.getState().scope).toEqual(ALL_NOTES_SCOPE);
  });

  it("uses the touched VM instead of the stale row when opening", async () => {
    const stale = space({
      id: "temp",
      name: "Trip",
      ttlHours: 48,
      expiryPhrase: "expires today",
      text: "old",
    });
    const touched = space({
      ...stale,
      text: "current",
      expiryPhrase: "expires in 2 d",
      restore: {
        tagTerms: {},
        flags: [],
        origin: null,
        text: "current",
        sort: null,
        opaque: false,
      },
    });
    mockSpaces.mockResolvedValue([stale]);
    vi.mocked(notesSpaceTouch).mockResolvedValue(touched);
    render(<SpaceList vaultId="vault-1" />);
    fireEvent.click(await screen.findByRole("button", { name: "Trip" }));
    expect(await screen.findByText("expires in 2 d")).toBeVisible();
    expect(notesFiltersStore.getState().text).toBe("current");
  });

  it.each([
    "clear",
    "edit-and-undo",
    "scope",
    "chips",
  ] as const)("shows the clicked row immediately but never overwrites a %s during touch", async (change) => {
    const target = space({ id: "work", name: "Work", text: "saved prompt" });
    mockSpaces.mockResolvedValue([target]);
    // The project's ES2022 lib predates Promise.withResolvers.
    let release!: (value: NoteSpaceVm) => void;
    vi.mocked(notesSpaceTouch).mockReturnValueOnce(
      new Promise((resolve) => {
        release = resolve;
      }),
    );
    notesFiltersStore.getState().setText("old prompt");
    render(
      <>
        <SpaceList vaultId="vault-1" />
        <NoteFilterBar onSaveAsSpace={vi.fn()} />
      </>,
    );
    const row = await screen.findByRole("button", { name: "Work" });
    fireEvent.click(row);
    expect(row).toHaveAttribute("aria-current", "true");
    await waitFor(() => expect(notesSpaceTouch).toHaveBeenCalledWith("vault-1", "work"));
    const field = screen.getByRole("combobox", { name: "Search notes" });
    if (change === "clear") fireEvent.click(screen.getByRole("button", { name: "Clear search" }));
    else if (change === "edit-and-undo") {
      fireEvent.change(field, { target: { value: "new prompt" } });
      fireEvent.change(field, { target: { value: "old prompt" } });
    } else if (change === "scope")
      act(() =>
        notesFiltersStore
          .getState()
          .setScope({ kind: "folder", vaultId: "vault-1", path: "journal" }),
      );
    else act(() => notesFiltersStore.getState().setTagTerm("work", "exclude"));
    const expected = notesFiltersStore.getState();
    await act(async () => release(target));
    expect(notesFiltersStore.getState().scope).toEqual(expected.scope);
    expect(notesFiltersStore.getState().tagTerms).toEqual(expected.tagTerms);
    expect(field).toHaveValue(expected.text);
    expect(row).not.toHaveAttribute("aria-current");
  });

  it.each([
    "work",
    ALL_SPACE_ID,
  ])("keeps edits when the already active %s row is clicked again", async (id) => {
    const target = space({ id, name: "Work" });
    mockSpaces.mockResolvedValue([target]);
    notesFiltersStore.getState().enterSpace(target);
    notesFiltersStore.getState().setText("unsaved budget");
    render(<SpaceList vaultId="vault-1" />);
    const row = await screen.findByRole("button", { name: "Work" });
    await act(async () => fireEvent.click(row));
    expect(notesFiltersStore.getState().text).toBe("unsaved budget");
    expect(notesSpacePark).not.toHaveBeenCalled();
  });

  it("keeps a clear made while parking and does not start the obsolete touch", async () => {
    const target = space({ id: "work", name: "Work", text: "saved prompt" });
    mockSpaces.mockResolvedValue([target]);
    let release!: (value: NoteSpaceVm) => void;
    vi.mocked(notesSpacePark).mockReturnValueOnce(
      new Promise((resolve) => {
        release = resolve;
      }),
    );
    notesFiltersStore.getState().setText("unsaved budget");
    render(
      <>
        <SpaceList vaultId="vault-1" />
        <NoteFilterBar onSaveAsSpace={vi.fn()} />
      </>,
    );
    fireEvent.click(await screen.findByRole("button", { name: "Work" }));
    await waitFor(() => expect(notesSpacePark).toHaveBeenCalled());
    fireEvent.click(screen.getByRole("button", { name: "Clear search" }));
    await act(async () => release(target));
    expect(screen.getByRole("combobox", { name: "Search notes" })).toHaveValue("");
    expect(notesFiltersStore.getState().scope).toEqual(ALL_NOTES_SCOPE);
    expect(notesSpaceTouch).not.toHaveBeenCalled();
  });

  it("parks a modified persistent search once before rapid space entries", async () => {
    const first = space({
      id: "first",
      name: "Work",
      query: "tag:work",
      restore: {
        tagTerms: { work: "include" },
        flags: [],
        origin: null,
        text: null,
        sort: null,
        opaque: false,
      },
    });
    const second = space({ id: "second", name: "Travel" });
    mockSpaces.mockResolvedValue([first, second]);
    notesFiltersStore.getState().enterSpace(first);
    notesFiltersStore.getState().removeTag("work");
    notesFiltersStore.getState().setText("unsaved budget");
    render(<SpaceList vaultId="vault-1" />);
    const target = await screen.findByRole("button", { name: "Travel" });
    fireEvent.click(target);
    fireEvent.click(target);
    await waitFor(() => expect(notesFiltersStore.getState().scope).toMatchObject({ id: "second" }));
    expect(notesSpacePark).toHaveBeenCalledTimes(1);
    expect(notesSpacePark).toHaveBeenCalledWith(
      "vault-1",
      "Work",
      expect.objectContaining({
        baseSpaceId: null,
        tagTerms: {},
        text: "unsaved budget",
      }),
    );
    expect(notesFiltersStore.getState().text).toBe("");
  });

  it("does not park an unchanged or temporary search, and refuses a failed park honestly", async () => {
    const first = space({ id: "first", name: "Work" });
    const second = space({ id: "second", name: "Travel", ttlHours: 2 });
    mockSpaces.mockResolvedValue([first, second]);
    notesFiltersStore.getState().enterSpace(first);
    render(<SpaceList vaultId="vault-1" />);
    fireEvent.click(await screen.findByRole("button", { name: "Travel" }));
    await waitFor(() => expect(notesFiltersStore.getState().scope).toMatchObject({ id: "second" }));
    expect(notesSpacePark).not.toHaveBeenCalled();
    act(() => notesFiltersStore.getState().setText("temporary draft"));
    fireEvent.click(screen.getByRole("button", { name: "Work" }));
    await waitFor(() => expect(notesFiltersStore.getState().scope).toMatchObject({ id: "first" }));
    expect(notesSpacePark).not.toHaveBeenCalled();
    act(() => notesFiltersStore.getState().setText("must not lose this"));
    vi.mocked(notesSpacePark).mockRejectedValueOnce(new Error("Vault is read-only"));
    fireEvent.click(screen.getByRole("button", { name: "Travel" }));
    expect(await screen.findByText("Vault is read-only")).toBeVisible();
    expect(notesFiltersStore.getState().scope).toMatchObject({ id: "first" });
    expect(notesFiltersStore.getState().text).toBe("must not lose this");
  });

  it("leaves a sort-only All notes search without attempting an empty park", async () => {
    mockSpaces.mockResolvedValue([space({ id: "next", name: "Work" })]);
    notesFiltersStore.getState().setSort({ key: "name", dir: "asc" });
    render(<SpaceList vaultId="vault-1" />);
    fireEvent.click(await screen.findByRole("button", { name: "Work" }));
    await waitFor(() => expect(notesFiltersStore.getState().scope).toMatchObject({ id: "next" }));
    expect(notesSpacePark).not.toHaveBeenCalled();
  });
});

describe("SpaceList rows", () => {
  it("selects All notes from another scope without offering synthetic file actions", async () => {
    notesFiltersStore.getState().setScope({ kind: "folder", vaultId: "vault-1", path: "projects" });
    mockSpaces.mockResolvedValue([
      space({ id: ALL_SPACE_ID, name: "All notes", query: "" }),
      space({ id: "s1", name: "Work" }),
    ]);
    render(<SpaceList vaultId="vault-1" onNewNote={vi.fn()} />);
    const all = await screen.findByRole("button", { name: "All notes" });
    fireEvent.click(all);
    await waitFor(() => expect(notesFiltersStore.getState().scope).toEqual(ALL_NOTES_SCOPE));
    expect(all).toHaveAttribute("aria-pressed", "true");
    expect(screen.queryByRole("button", { name: "Edit space All notes" })).toBeNull();
    expect(screen.queryByRole("button", { name: "Delete space All notes" })).toBeNull();
    fireEvent.click(all);
    expect(notesFiltersStore.getState().scope).toEqual(ALL_NOTES_SCOPE);
  });

  it("opens the targeted menu without selecting it and replaces hover actions", async () => {
    const onNewNote = vi.fn();
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Work" })]);
    render(<SpaceList vaultId="vault-1" onNewNote={onNewNote} />);
    const select = await screen.findByRole("button", { name: "Work" });
    expect(screen.queryByRole("button", { name: "New note in Work" })).not.toBeInTheDocument();
    fireEvent.contextMenu(select);
    expect(select).toHaveAttribute("data-menu-target");
    expect(select).toHaveAttribute("aria-expanded", "true");
    expect(notesFiltersStore.getState().scope).toEqual(ALL_NOTES_SCOPE);
    expect(screen.getAllByRole("menuitem").map((item) => item.textContent)).toEqual([
      "Open space",
      "Add to current search",
      "Pin space",
      "Edit space…",
      "Duplicate space",
      "New note in this space",
      "Add sub-space",
      "New space…",
      "Delete space…",
    ]);
    fireEvent.click(screen.getByRole("menuitem", { name: "New note in this space" }));
    expect(onNewNote).toHaveBeenCalledOnce();
    await waitFor(() => expect(select).not.toHaveAttribute("data-menu-target"));
    expect(notesFiltersStore.getState().scope).toEqual(ALL_NOTES_SCOPE);
  });

  it("opens from the keyboard and Escape removes the menu target without touching lifetime", async () => {
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Work", ttlHours: 48 })]);
    render(<SpaceList vaultId="vault-1" />);
    const row = await screen.findByRole("button", { name: "Work" });
    act(() => row.focus());
    fireEvent.keyDown(row, { key: "F10", shiftKey: true });
    const menu = await screen.findByRole("menu");
    expect(row).toHaveAttribute("data-menu-target");
    fireEvent.keyDown(menu, { key: "Escape" });
    await waitFor(() => expect(row).not.toHaveAttribute("data-menu-target"));
    expect(row).toHaveFocus();
    expect(notesSpaceTouch).not.toHaveBeenCalled();
    expect(notesFiltersStore.getState().scope).toEqual(ALL_NOTES_SCOPE);
  });

  it("selects a space as a scope without navigating away from the open note", async () => {
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Active work" })]);
    render(<SpaceList vaultId="vault-1" />);

    fireEvent.click(await screen.findByRole("button", { name: "Active work" }));

    await waitFor(() =>
      expect(notesFiltersStore.getState().scope).toEqual({
        kind: "space",
        id: "s1",
        name: "Active work",
        vaultId: "vault-1",
        defaultKey: null,
      }),
    );
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

    await waitFor(() =>
      expect(notesFiltersStore.getState().scope).toEqual({
        kind: "space",
        vaultId: "vault-1",
        id: "s1",
        name: "Sessions",
        defaultKey: "recordings",
      }),
    );
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

  /**
   * The visible half of Story 44.4's fallback.
   *
   * A space's frontmatter is a file a person and an agent both edit, so
   * `sort: bananas` will happen. keeper still lists the space — it selects what
   * it selects — and the ordering it runs is the default. A row that said
   * nothing about that is indistinguishable from keeper ignoring what the user
   * wrote, which is the whole failure this replaces.
   */
  it("says so when it could not read a space's sort, rather than quietly not obeying it", async () => {
    const said =
      'keeper doesn\'t know the sort "bananas", so this space is sorted by modified, newest first.';
    mockSpaces.mockResolvedValue([
      space({ id: "s1", name: "Odd", sort: "bananas", warnings: [said] }),
    ]);
    render(<SpaceList vaultId="vault-1" />);

    const row = await screen.findByRole("button", {
      name: `Odd, ${SPACE_SETTINGS_SUBTITLE}`,
    });
    fireEvent.focus(row);
    expect(await screen.findByRole("tooltip")).toHaveTextContent(said);
    expect(screen.getByText(SPACE_SETTINGS_SUBTITLE)).toBeInTheDocument();
  });

  it("says nothing about the settings of a space it read entirely", async () => {
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Fine" })]);
    render(<SpaceList vaultId="vault-1" />);

    await screen.findByRole("button", { name: "Fine" });
    expect(screen.queryByText(SPACE_SETTINGS_SUBTITLE)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Fine" })).not.toHaveAttribute("title");
  });

  /**
   * A space can be both broken and misread, and the two are not the same news:
   * one means the space selects NOTHING, the other means it still works and is
   * ordered differently from what its file says. Sending someone to fix a query
   * that is fine is worse than saying nothing, so the parse failure wins the one
   * line the row has.
   */
  it("leads with the parse failure when a space is both broken and misread", async () => {
    mockSpaces.mockResolvedValue([
      space({
        id: "s1",
        name: "Both",
        error: "unknown search key `nope`",
        warnings: ['keeper doesn\'t know the sort "bananas"…'],
      }),
    ]);
    render(<SpaceList vaultId="vault-1" />);

    await screen.findByRole("button", { name: /Both, This space's query can't be read/ });
    expect(screen.queryByText(SPACE_SETTINGS_SUBTITLE)).not.toBeInTheDocument();
  });

  /**
   * The rail's order is Rust's (FR-157): `notes_spaces` sorts by each space's
   * `keeper.order` and then newest modification. This asserts the list renders that answer
   * as given — a component that re-sorted by name here would throw the
   * positions away and the whole feature would be invisible.
   */
  it("renders the rail in the order it was handed, not in its own", async () => {
    mockSpaces.mockResolvedValue([
      space({ id: "s1", name: "Zebra", order: -1 }),
      space({ id: "s2", name: "Apple", order: 0 }),
      space({ id: "s3", name: "Mango", order: 3 }),
    ]);
    render(<SpaceList vaultId="vault-1" />);

    await screen.findByRole("button", { name: "Zebra" });
    expect(
      screen
        .getAllByRole("button")
        .map((row) => row.getAttribute("aria-label"))
        // The row's own control is the one whose name is the space; every
        // per-row affordance beside it is filtered out by its verb prefix.
        .filter(
          (label) =>
            label !== null &&
            !label.startsWith("Edit space ") &&
            !label.startsWith(`${DELETE_SPACE} `),
        )
        // The section's own disclosure (Story 47.3) is a header control, not a
        // row, and it is the first button in the section — filtered by name so
        // this test keeps saying what it is about, which is row ORDER.
        .filter((label) => label !== RESTORE_DEFAULTS && label !== "Collapse Spaces"),
    ).toEqual(["Zebra", "Apple", "Mango"]);
  });
});

it("keeps one drive unheaded and renders four named groups without merging identical ids", async () => {
  mockSpaces.mockResolvedValue([space({ id: ALL_SPACE_ID, name: "All notes" })]);
  const { rerender } = render(<SpaceList vaultId="vault-1" />);
  await screen.findByRole("button", { name: "All notes" });
  expect(screen.queryByRole("group", { name: "Personal" })).toBeNull();
  mockSpaces.mockResolvedValue(
    ["Personal", "Work", "Archive", "Shared"].map((name, index) =>
      space({
        id: ALL_SPACE_ID,
        name: "All notes",
        vaultId: `vault-${index + 1}`,
        vaultName: name,
      }),
    ),
  );
  rerender(<SpaceList vaultId="vault-1" vaultIds={["vault-1", "vault-2", "vault-3", "vault-4"]} />);
  for (const name of ["Personal", "Work", "Archive", "Shared"])
    expect(await screen.findByRole("group", { name })).toBeVisible();
  expect(screen.getAllByRole("button", { name: "All notes" })).toHaveLength(4);
});

it("duplicates the targeted drive's definition under a new identity without changing search", async () => {
  const original = space({
    id: "trip",
    name: "Journal/Bali",
    vaultId: "other",
    vaultName: "Work",
    pinned: true,
    ttlHours: 48,
    text: "budget",
  });
  const copy = { ...original, id: "copy", name: "Journal/Bali copy", leafName: "Bali copy" };
  mockSpaces.mockResolvedValue([original]);
  mockSave.mockImplementation(async (_vault, req) => {
    expect(req.id).toBeNull();
    mockSpaces.mockResolvedValue([original, copy]);
    return copy;
  });
  render(<SpaceList vaultId="vault-1" />);
  await chooseSpaceAction("Journal/Bali", "Duplicate space");
  expect(await screen.findByRole("button", { name: "Journal/Bali copy" })).toBeVisible();
  expect(mockSave).toHaveBeenCalledWith(
    "other",
    expect.objectContaining({
      name: "Journal/Bali copy",
      query: original.query,
      sort: original.sort,
      pinned: true,
      ttlHours: 48,
      text: "budget",
    }),
  );
  expect(notesFiltersStore.getState().scope).toEqual(ALL_NOTES_SCOPE);
});

it("opens a temporary sub-space draft with its caret after the parent slash", async () => {
  mockSpaces.mockResolvedValue([space({ id: "trip", name: "Journal/Bali" })]);
  render(<SpaceList vaultId="vault-1" />);
  await chooseSpaceAction("Journal/Bali", "Add sub-space");
  const name = (await screen.findByRole("textbox", { name: "Name" })) as HTMLInputElement;
  expect(name).toHaveValue("Journal/Bali/");
  expect(name.selectionStart).toBe("Journal/Bali/".length);
  expect(screen.getByRole("switch", { name: "Temporary space" })).toBeChecked();
  expect(mockSave).not.toHaveBeenCalled();
});

it("shows Temporary with its clock and pinned rows without a caption", async () => {
  mockSpaces.mockResolvedValue([
    space({ id: "keeper:temporary", name: "Temporary", icon: "clock", descendants: 1 }),
    space({ id: "trip", name: "Trip", parent: "keeper:temporary", pinned: true }),
  ]);
  render(<SpaceList vaultId="vault-1" />);
  const temporary = await screen.findByRole("button", { name: "Temporary" });
  expect(temporary.querySelector('[data-space-icon="clock"]')).toBeInTheDocument();
  expect(screen.queryByText("PINNED")).not.toBeInTheDocument();
  expect(screen.getByRole("button", { name: "Trip" })).toBeVisible();
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
   * still the name nobody recognises.
   *
   * **The fixture was `sparkles`, and Story 45.20 added `sparkles` to the set** —
   * which made this test's own title false while it stayed green. The assertion
   * reads `data-space-icon`, which is the STORED name, and that is identical
   * whether the glyph resolved or fell back: same DOM, different meaning.
   * `no-such-glyph` cannot become a real icon by accident. The durable form —
   * owed, not done — is `spaceIcon` taking the catalogue the way 45.2's
   * `resolveViewerComponent` takes the component table, so the fallback is
   * exercised against an explicitly empty one forever instead of against a name
   * somebody will add later.
   */
  it("draws the fallback glyph for an icon name that is not in the set any more, and keeps the name", async () => {
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Old", icon: "no-such-glyph" })]);
    const { container } = render(<SpaceList vaultId="vault-1" />);

    await screen.findByRole("button", { name: "Old" });
    const glyph = container.querySelector('[data-slot="space-icon"]');
    expect(glyph).toBeInTheDocument();
    expect(glyph).toHaveAttribute("data-space-icon", "no-such-glyph");

    // And the unknown name survives a save that only changed the title.
    await chooseSpaceAction("Old", "Edit space…");
    fireEvent.change(await screen.findByLabelText("Name"), { target: { value: "Older" } });
    mockSave.mockResolvedValue(space({ id: "s1", name: "Older" }));
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    await waitFor(() => expect(mockSave).toHaveBeenCalledTimes(1));
    expect(mockSave.mock.calls[0]?.[1].icon).toBe("no-such-glyph");
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

    await chooseSpaceAction("Archive triage", "Edit space…");

    expect(await screen.findByLabelText("Name")).toHaveValue("Archive triage");
  });

  it("re-reads the list after a save, so the sidebar shows the new name", async () => {
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Active work" })]);
    mockSave.mockResolvedValue(space({ id: "s1", name: "Renamed" }));
    render(<SpaceList vaultId="vault-1" />);

    await chooseSpaceAction("Active work", "Edit space…");
    fireEvent.change(await screen.findByLabelText("Name"), { target: { value: "Renamed" } });
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Renamed" })]);
    fireEvent.click(screen.getByRole("button", { name: "Save" }));

    expect(await screen.findByRole("button", { name: "Renamed" })).toBeInTheDocument();
    expect(notesSpaces).toHaveBeenCalledTimes(2);
  });

  it("leaves the list alone when the editor is cancelled", async () => {
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Active work" })]);
    render(<SpaceList vaultId="vault-1" />);

    await chooseSpaceAction("Active work", "Edit space…");
    await screen.findByLabelText("Name");
    fireEvent.click(screen.getByRole("button", { name: "Cancel" }));

    await waitFor(() => expect(screen.queryByLabelText("Name")).not.toBeInTheDocument());
    expect(mockSave).not.toHaveBeenCalled();
    expect(notesSpaces).toHaveBeenCalledTimes(1);
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

  /**
   * The refusal Rust gives names the file it could not read. Showing the generic
   * sentence instead is how Story 44.3 shipped green and left a field report of
   * "it did nothing" unanswerable: a message that says only "keeper couldn't"
   * sends someone to a bug report, and one that names `.keeper-spaces.json`
   * sends them to the file.
   */
  it("shows the reason keeper gives, naming the file, rather than a generic apology", async () => {
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Active work" })]);
    mockRestore.mockRejectedValue({
      code: "notesInvalid",
      message:
        ".keeper-spaces.json could not be read (permission denied); leaving this vault's spaces alone",
    });
    render(<SpaceList vaultId="vault-1" />);
    await screen.findByRole("button", { name: "Active work" });

    fireEvent.click(screen.getByRole("button", { name: RESTORE_DEFAULTS }));

    expect(await screen.findByText(/\.keeper-spaces\.json could not be read/)).toBeInTheDocument();
    expect(screen.queryByText(RESTORE_FAILED)).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Active work" })).toBeInTheDocument();
    // A failed restore is not a reason to re-read a list nothing changed in.
    expect(notesSpaces).toHaveBeenCalledTimes(1);
  });

  it("falls back to a plain sentence when the rejection carries no message", async () => {
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Active work" })]);
    mockRestore.mockRejectedValue("nope");
    render(<SpaceList vaultId="vault-1" />);
    await screen.findByRole("button", { name: "Active work" });

    fireEvent.click(screen.getByRole("button", { name: RESTORE_DEFAULTS }));

    expect(await screen.findByText(RESTORE_FAILED)).toBeInTheDocument();
  });

  it("cannot be pressed with no vault open", async () => {
    render(<SpaceList vaultId={null} />);

    const control = await screen.findByRole("button", { name: RESTORE_DEFAULTS });
    expect(control).toBeDisabled();
    fireEvent.click(control);
    expect(mockRestore).not.toHaveBeenCalled();
  });
});

describe("SpaceList delete", () => {
  /**
   * The plan a space's confirmation shows, with the fields a test asserts on
   * carrying values a paraphrase would not produce.
   */
  function plan(name: string, seeded: boolean) {
    return {
      path: `spaces/2026-08-09-${name.toLowerCase()}.md`,
      question: `Delete the space "${name}"?`,
      consequence: seeded
        ? "A space is a saved view, and keeper seeded this one."
        : "A space is a saved view.",
      recovery: "keeper moves it into the vault's trash.",
    };
  }

  /**
   * Declining removes nothing — and this asserts the COMMAND was not called
   * rather than that the dialog closed. A dialog that closed while the delete
   * was in flight looks identical on screen and is the opposite outcome.
   */
  it("asks before deleting, and a decline calls no delete", async () => {
    mockSpaces.mockResolvedValue([
      space({ id: "s1", name: "Recordings", defaultKey: "recordings" }),
      space({ id: "s2", name: "Clients" }),
    ]);
    mockDeletePlan.mockResolvedValue(plan("Clients", false));
    render(<SpaceList vaultId="vault-1" />);

    await chooseSpaceAction("Clients", "Delete space…");

    // The plan is asked for by id, and by the id of the row that was pressed:
    // a second space is on the list precisely so "it deleted something" and
    // "it deleted THAT" cannot be the same assertion.
    await waitFor(() => expect(mockDeletePlan).toHaveBeenCalledWith("vault-1", "s2"));
    expect(await screen.findByText(`Delete the space "Clients"?`)).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: NOTE_DELETE_CANCEL }));
    await waitFor(() =>
      expect(screen.queryByText(`Delete the space "Clients"?`)).not.toBeInTheDocument(),
    );
    expect(mockDelete).not.toHaveBeenCalled();
  });

  /**
   * Confirming deletes the space that was pressed, re-reads the rail, and —
   * the part a rendered-text assertion would miss — hands Rust the right ids.
   */
  it("deletes the space it named and re-reads the rail", async () => {
    mockSpaces
      .mockResolvedValueOnce([
        space({ id: "s1", name: "Recordings", defaultKey: "recordings" }),
        space({ id: "s2", name: "Clients" }),
      ])
      .mockResolvedValue([space({ id: "s2", name: "Clients" })]);
    mockDeletePlan.mockResolvedValue(plan("Recordings", true));
    render(<SpaceList vaultId="vault-1" />);

    await chooseSpaceAction("Recordings", "Delete space…");
    fireEvent.click(await screen.findByRole("button", { name: NOTE_DELETE_CONFIRM }));

    await waitFor(() => expect(mockDelete).toHaveBeenCalledWith("vault-1", "s1"));
    // Re-read, so the row goes. Two calls: the mount's and the deletion's.
    await waitFor(() => expect(notesSpaces).toHaveBeenCalledTimes(2));
    await waitFor(() =>
      expect(screen.queryByRole("button", { name: "Recordings" })).not.toBeInTheDocument(),
    );
  });

  /**
   * The confirmation is Rust's, verbatim. A space's whole risk is that a person
   * thinks deleting the saved view deletes the notes it lists, and the sentence
   * that says otherwise is composed where the removal is — so this asserts the
   * words arrive, not that some words arrive.
   */
  it("shows Rust's sentences rather than a paraphrase", async () => {
    mockSpaces.mockResolvedValue([
      space({ id: "s1", name: "Recordings", defaultKey: "recordings" }),
    ]);
    mockDeletePlan.mockResolvedValue(plan("Recordings", true));
    render(<SpaceList vaultId="vault-1" />);

    await chooseSpaceAction("Recordings", "Delete space…");

    const body = await screen.findByTestId(NOTE_DELETE_TESTID);
    expect(body).toHaveTextContent("A space is a saved view, and keeper seeded this one.");
    expect(body).toHaveTextContent("keeper moves it into the vault's trash.");
    expect(screen.getByText("spaces/2026-08-09-recordings.md")).toBeInTheDocument();
  });

  /**
   * A lens pointed at a space that no longer exists selects nothing and cannot
   * say why, so deleting the ACTIVE space returns the scope to all notes —
   * and deleting any other space leaves the scope exactly where it was.
   */
  it("clears the scope only when the deleted space was the active one", async () => {
    mockSpaces.mockResolvedValue([
      space({ id: "s1", name: "Recordings", defaultKey: "recordings" }),
      space({ id: "s2", name: "Clients" }),
    ]);
    mockDeletePlan.mockResolvedValue(plan("Clients", false));
    render(<SpaceList vaultId="vault-1" />);

    fireEvent.click(await screen.findByRole("button", { name: "Recordings" }));
    await waitFor(() =>
      expect(notesFiltersStore.getState().scope).toMatchObject({
        kind: "space",
        id: "s1",
        name: "Recordings",
        defaultKey: "recordings",
      }),
    );
    expect(notesFiltersStore.getState().sort).toEqual({ key: "modified", dir: "desc" });

    // Deleting the OTHER space leaves the lens alone.
    await chooseSpaceAction("Clients", "Delete space…");
    fireEvent.click(await screen.findByRole("button", { name: NOTE_DELETE_CONFIRM }));
    await waitFor(() => expect(mockDelete).toHaveBeenCalledWith("vault-1", "s2"));
    expect(notesFiltersStore.getState().scope).toMatchObject({ kind: "space", id: "s1" });

    // Deleting the active one puts it back to all notes.
    mockDeletePlan.mockResolvedValue(plan("Recordings", true));
    await chooseSpaceAction("Recordings", "Delete space…");
    fireEvent.click(await screen.findByRole("button", { name: NOTE_DELETE_CONFIRM }));
    await waitFor(() => expect(notesFiltersStore.getState().scope).toEqual({ kind: "all" }));
    expect(notesFiltersStore.getState().sort).toBeNull();
    expect(notesFiltersStore.getState().enteredSpace).toBeNull();
  });

  /**
   * The delete was refused. The dialog stays, says so, and the rail is not
   * re-read — a row vanishing beside "keeper couldn't delete that" would be
   * keeper contradicting itself on screen.
   */
  it("keeps the dialog and the row when the delete is refused", async () => {
    mockSpaces.mockResolvedValue([space({ id: "s1", name: "Clients" })]);
    mockDeletePlan.mockResolvedValue(plan("Clients", false));
    mockDelete.mockRejectedValue({ message: "spaces/clients.md is read-only" });
    render(<SpaceList vaultId="vault-1" />);

    await chooseSpaceAction("Clients", "Delete space…");
    fireEvent.click(await screen.findByRole("button", { name: NOTE_DELETE_CONFIRM }));

    expect(await screen.findByRole("alert")).toHaveTextContent("spaces/clients.md is read-only");
    expect(notesSpaces).toHaveBeenCalledTimes(1);
    // The confirmation is still open, so the rail behind it is `aria-hidden`
    // by Radix's modal — `hidden: true` is asking about the DOM rather than
    // about what a screen reader is currently being offered.
    expect(screen.getByText(`Delete the space "Clients"?`)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Clients", hidden: true })).toBeInTheDocument();
  });
});
