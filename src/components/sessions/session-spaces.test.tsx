import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type {
  NoteFolderVm,
  NoteRowVm,
  NoteVaultVm,
  SessionSpaceFilesVm,
  SessionSpaceVm,
} from "@/lib/ipc/client";

// The section writes through three commands now; the editor it opens reaches
// for two more, and the row's opener reaches the note index through Story
// 45.18's bridge. All of them are stubbed at the IPC boundary so the real
// components — the real refusals, the real copy, the real resolution rule — are
// what these tests exercise.
vi.mock("@/lib/ipc/client", () => ({
  sessionsSpaceDelete: vi.fn(),
  sessionsSpacesRestore: vi.fn(),
  sessionsSpaceSave: vi.fn(),
  sessionsFileNewKind: vi.fn(),
  notesSpaceTerms: vi.fn(),
  notesTree: vi.fn(),
  // The vault mirror hydrates itself from these two when this section is the
  // first surface a window opens; a test that wants vaults seeds the store
  // directly, which marks it hydrated and skips them.
  notesVaults: vi.fn(async () => []),
  notesVaultActive: vi.fn(async () => null),
  notesVaultSetActive: vi.fn(async () => undefined),
}));

import {
  SESSION_SPACE_BROKEN_SUBTITLE,
  SESSION_SPACE_DELETE,
  SESSION_SPACE_DELETE_CONFIRM,
  SESSION_SPACE_DELETE_FAILED,
  SESSION_SPACE_EDIT,
  SESSION_SPACE_NEW_NOTE,
  SESSION_SPACE_NEW_NOTE_FAILED,
  SESSION_SPACE_SETTINGS_SUBTITLE,
  SESSION_SPACE_VAULTS_UNKNOWN,
  SESSION_SPACES_EMPTY,
  SESSION_SPACES_LOADING,
  SESSION_SPACES_NEW,
  SESSION_SPACES_NO_FILES,
  SESSION_SPACES_RESTORE,
  SESSION_SPACES_RESTORE_FAILED,
  SESSION_SPACES_RESTORE_NOTHING,
  SessionSpaces,
} from "@/components/sessions/session-spaces";
import {
  notesSpaceTerms,
  notesTree,
  notesVaultSetActive,
  notesVaults,
  sessionsFileNewKind,
  sessionsSpaceDelete,
  sessionsSpacesRestore,
} from "@/lib/ipc/client";
import { notesVaultsStore, resetNotesVaultsStoreForTest } from "@/lib/stores/notes-vaults";
import { activePanel, panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";
import { primaryViewStore } from "@/lib/stores/primary-view";

const mockDelete = vi.mocked(sessionsSpaceDelete);
const mockRestore = vi.mocked(sessionsSpacesRestore);
const mockTerms = vi.mocked(notesSpaceTerms);
const mockNewKind = vi.mocked(sessionsFileNewKind);
const mockTree = vi.mocked(notesTree);
const mockSetActive = vi.mocked(notesVaultSetActive);
/** The mirror's own read — held so a case can keep the vault list UNKNOWN. */
const mockVaultsRead = vi.mocked(notesVaults);

function space(p: Partial<SessionSpaceVm> = {}): SessionSpaceVm {
  return {
    id: p.id ?? "_spaces/tasks.md",
    name: p.name ?? "Tasks",
    query: p.query ?? "tag:task",
    sort: p.sort ?? "order asc",
    sortEffective: p.sortEffective ?? "order asc",
    icon: p.icon ?? null,
    defaultKey: p.defaultKey ?? null,
    order: p.order ?? 2,
    warnings: p.warnings ?? [],
    error: p.error ?? null,
    // Rust's answer, never derived here: the query above is a fixture detail
    // and this is the contract. `creatable_kind`'s own tests own rows 1–6.
    newFileKind: p.newFileKind ?? null,
  };
}

function selection(spaceId: string, names: string[], error: string | null = null) {
  return {
    spaceId,
    files: names.map((relPath) => ({
      id: `path:${relPath}`,
      relPath,
      subpath: `60-sessions/active/s/${relPath}`,
      title: relPath.replace(/\.md$/, ""),
      tags: ["task"],
      mtimeMs: Date.now() - 60_000,
      unstableIdentity: true,
    })),
    error,
  } satisfies SessionSpaceFilesVm;
}

/** What the strip is showing — the panel a click on a row would have filled. */
function opened() {
  return activePanel(panelsStore.getState()).target;
}

/**
 * One configured vault, as the mirror holds it.
 *
 * **Two vaults on two profiles, always** — the text viewer's rule, and for its
 * reason: a one-vault fixture cannot tell a per-profile filter from an
 * unconditional match, so `p2`'s vault would answer for `p1`'s file and every
 * assertion here would still pass.
 */
function vault(id: string, profileId: string, subfolder: string): NoteVaultVm {
  return {
    id,
    profileId,
    name: id,
    subfolder,
    root: `/Volumes/${profileId}/${subfolder}`,
    indexed: true,
    noteCount: 2,
    unreadCount: 0,
    cadence: { commitIdleMs: 1000, pushIntervalMs: 5000, pushOnBlur: true },
  } as NoteVaultVm;
}

/** The zone this session lives in IS a notes vault, and it is not the active one. */
function zoneInsideAVault(): void {
  notesVaultsStore
    .getState()
    .setVaults([vault("vault-1", "p1", "60-sessions"), vault("vault-2", "p2", "60-sessions")]);
  notesVaultsStore.getState().setActiveVaultId("vault-2");
}

/** Vaults are configured and none of them holds this session's zone. */
function zoneOutsideEveryVault(): void {
  notesVaultsStore
    .getState()
    .setVaults([vault("vault-1", "p2", "60-sessions"), vault("vault-2", "p1", "notes")]);
}

/** A folder listing with more than one note in it, so a match on the wrong row —
 *  or one that keeps only the first — has something to fail against. */
function folderWith(...notes: Array<{ id: string; path: string }>): NoteFolderVm {
  return {
    relDir: "active/s",
    dirs: [],
    notes: notes.map(
      ({ id, path }) =>
        ({
          id,
          path,
          title: path,
          snippet: "",
          tags: [],
          updatedMs: 0,
          pinned: false,
          archived: false,
          unread: false,
          conflict: false,
        }) as unknown as NoteRowVm,
    ),
  };
}

function open(
  spaces: SessionSpaceVm[],
  selections: SessionSpaceFilesVm[] | null,
  // Flat unless a case says otherwise: the create verb is a flat-contract verb
  // (`sessions_file_new_kind` writes into the session root, which a
  // folder-shaped session's pool excludes), so every case about the control
  // needs the shape that can carry it.
  shape = "flat",
): { onChanged: ReturnType<typeof vi.fn> } {
  const onChanged = vi.fn();
  render(
    <SessionSpaces
      rootId="p1"
      sessionId="01J5AAAAAAAAAAAAAAAAAAAAAA"
      shape={shape}
      spaces={spaces}
      selections={selections}
      onChanged={onChanged}
    />,
  );
  return { onChanged };
}

beforeEach(() => {
  mockDelete.mockReset();
  mockDelete.mockResolvedValue(undefined);
  mockRestore.mockReset();
  mockRestore.mockResolvedValue({ names: [] });
  mockTerms.mockReset();
  mockTerms.mockResolvedValue({
    kind: "chips",
    tags: [{ tag: "task", term: "include" }],
    flags: [],
    origin: null,
    text: null,
    fields: [],
  });
  mockNewKind.mockReset();
  mockNewKind.mockResolvedValue("60-sessions/active/s/untitled.md");
  mockTree.mockReset();
  mockTree.mockResolvedValue(folderWith());
  mockSetActive.mockReset();
  mockSetActive.mockResolvedValue(undefined);
  mockVaultsRead.mockReset();
  mockVaultsRead.mockResolvedValue([]);
  resetNotesVaultsStoreForTest();
  // Where a person pressing a space row actually is, so a test that expects
  // the notes view is watching a switch rather than a value that was already
  // right.
  primaryViewStore.getState().setView("sessions");
  resetPanelsStoreForTest();
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("SessionSpaces listing", () => {
  it("lists what a space selected, under the space's own name", () => {
    open(
      [space({ name: "Tasks" })],
      [selection("_spaces/tasks.md", ["task-migrate.md", "task-board.md"])],
    );

    const list = screen.getByRole("list", { name: "Tasks" });
    expect(list).toBeInTheDocument();
    expect(screen.getByText("task-migrate")).toBeInTheDocument();
    expect(screen.getByText("task-board")).toBeInTheDocument();
  });

  /**
   * Matrix row 10, and the half of the old single case that did not change.
   *
   * Opens through the ONE file target the tree and the Files pane use (AD-109),
   * on the `subpath` Rust composed (AD-65) — a second path-join in TypeScript is
   * a second answer to where a file lives.
   *
   * The zone is outside every configured vault here, which Story 49.2 does not
   * treat as a failure: `notePathForFile` answers `null` and the file viewer is
   * the only correct surface, so the section says nothing about it. Vaults are
   * seeded rather than absent so this is "none of them holds this zone" — the
   * configuration a person actually has — and not "keeper has not looked yet".
   */
  it("opens a file on the path Rust composed, not one it joined itself", () => {
    zoneOutsideEveryVault();
    open([space()], [selection("_spaces/tasks.md", ["task-migrate.md"])]);

    fireEvent.click(screen.getByRole("button", { name: /task-migrate/ }));

    expect(opened()).toEqual({
      kind: "file",
      profileId: "p1",
      relativePath: "60-sessions/active/s/task-migrate.md",
    });
    expect(screen.queryByRole("status")).not.toBeInTheDocument();
  });

  /**
   * "Reading…" and "nothing yet" are different answers and must not be the same
   * pixels: the definitions and the selections are two reads, so there is a real
   * moment where keeper knows a space is called Tasks and not yet what is in it.
   */
  it("says it is still reading rather than showing an empty section it has not read", () => {
    open([space()], null);

    expect(screen.getByText(SESSION_SPACES_LOADING)).toBeInTheDocument();
    expect(screen.queryByText(SESSION_SPACES_NO_FILES)).not.toBeInTheDocument();
  });

  it("says a space selected nothing once it has actually looked", () => {
    open([space()], [selection("_spaces/tasks.md", [])]);

    expect(screen.getByText(SESSION_SPACES_NO_FILES)).toBeInTheDocument();
    expect(screen.queryByText(SESSION_SPACES_LOADING)).not.toBeInTheDocument();
  });

  it("offers the defaults when the zone has no spaces at all", () => {
    open([], []);

    expect(screen.getByText(SESSION_SPACES_EMPTY)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: SESSION_SPACES_RESTORE })).toBeInTheDocument();
  });
});

describe("SessionSpaces failure states", () => {
  /**
   * Story 44.4's split, kept exactly: a query that will not parse is an `error`
   * and the space selects NOTHING — never everything. A saved view that silently
   * widened to the whole session is how a bulk action becomes a data-loss story.
   */
  it("says a broken space is broken and lists nothing under it", () => {
    open(
      [space({ name: "Prompts", error: "Unexpected end of query after `AND`." })],
      [selection("_spaces/tasks.md", [], "Unexpected end of query after `AND`.")],
    );

    expect(screen.getByText(SESSION_SPACE_BROKEN_SUBTITLE)).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Prompts" })).not.toBeInTheDocument();
  });

  /**
   * The quieter half: a `sort` keeper could not read still selects what it
   * selects. Sending someone to fix a query that is fine is worse than saying
   * nothing, so this must NOT borrow the broken sentence.
   */
  it("says a misread setting is a setting, and still lists the files", () => {
    const said = "Couldn't read sort `sideways asc`; using modified desc.";
    open(
      [space({ name: "References", sort: "sideways asc", warnings: [said] })],
      [selection("_spaces/tasks.md", ["ref-inputs.md"])],
    );

    const subtitle = screen.getByText(SESSION_SPACE_SETTINGS_SUBTITLE);
    expect(subtitle).toHaveAttribute("title", said);
    expect(screen.queryByText(SESSION_SPACE_BROKEN_SUBTITLE)).not.toBeInTheDocument();
    expect(screen.getByRole("list", { name: "References" })).toBeInTheDocument();
  });

  /**
   * A space can be both, and the two are not the same news. The parse failure
   * wins the one line the row has, because it is the one that changes what the
   * section is showing.
   */
  it("leads with the parse failure when a space is both broken and misread", () => {
    open(
      [space({ error: "unknown search key `nope`", warnings: ["Couldn't read sort `x`."] })],
      [selection("_spaces/tasks.md", [])],
    );

    expect(screen.getByText(SESSION_SPACE_BROKEN_SUBTITLE)).toBeInTheDocument();
    expect(screen.queryByText(SESSION_SPACE_SETTINGS_SUBTITLE)).not.toBeInTheDocument();
  });

  it("says nothing at all about a space it read entirely", () => {
    open([space()], [selection("_spaces/tasks.md", ["task-migrate.md"])]);

    expect(screen.queryByText(SESSION_SPACE_BROKEN_SUBTITLE)).not.toBeInTheDocument();
    expect(screen.queryByText(SESSION_SPACE_SETTINGS_SUBTITLE)).not.toBeInTheDocument();
  });
});

describe("SessionSpaces restore", () => {
  /**
   * Names, not a count. "Restored About, Prompts" tells the operator whether
   * keeper agreed with them about what was missing; "2" does not.
   */
  it("names what it restored", async () => {
    mockRestore.mockResolvedValue({ names: ["About", "Prompts"] });
    const { onChanged } = open([space()], [selection("_spaces/tasks.md", [])]);

    fireEvent.click(screen.getByRole("button", { name: SESSION_SPACES_RESTORE }));

    expect(await screen.findByText("Restored About, Prompts.")).toBeInTheDocument();
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  it("says nothing was missing rather than claiming it restored an empty list", async () => {
    mockRestore.mockResolvedValue({ names: [] });
    open([space()], [selection("_spaces/tasks.md", [])]);

    fireEvent.click(screen.getByRole("button", { name: SESSION_SPACES_RESTORE }));

    expect(await screen.findByText(SESSION_SPACES_RESTORE_NOTHING)).toBeInTheDocument();
  });

  /**
   * Rust's own sentence, not keeper-the-frontend's summary of it: that sentence
   * names the file it could not write, which is the difference between a bug
   * report and a `chmod`.
   */
  it("repeats what Rust said about a refused restore", async () => {
    const said = "60-sessions/_spaces/about.md: Permission denied (os error 13)";
    mockRestore.mockRejectedValue({ message: said });
    const { onChanged } = open([space()], [selection("_spaces/tasks.md", [])]);

    fireEvent.click(screen.getByRole("button", { name: SESSION_SPACES_RESTORE }));

    expect(await screen.findByText(said)).toBeInTheDocument();
    // Nothing was written, so nothing is re-read: a bumped counter here would
    // redraw the section as if the press had landed.
    expect(onChanged).not.toHaveBeenCalled();
  });

  it("still says the restore failed when the rejection carries no sentence", async () => {
    mockRestore.mockRejectedValue({});
    open([space()], [selection("_spaces/tasks.md", [])]);

    fireEvent.click(screen.getByRole("button", { name: SESSION_SPACES_RESTORE }));

    expect(await screen.findByText(SESSION_SPACES_RESTORE_FAILED)).toBeInTheDocument();
  });
});

describe("SessionSpaces delete", () => {
  it("asks before deleting, and deletes nothing on cancel", async () => {
    open([space({ name: "Tasks" })], [selection("_spaces/tasks.md", [])]);

    fireEvent.click(screen.getByRole("button", { name: `${SESSION_SPACE_DELETE} Tasks` }));
    fireEvent.click(await screen.findByRole("button", { name: "Cancel" }));

    expect(mockDelete).not.toHaveBeenCalled();
  });

  it("deletes the space it was asked about, by its own id", async () => {
    const { onChanged } = open(
      [space({ id: "_spaces/log.md", name: "Log" }), space({ name: "Tasks" })],
      [selection("_spaces/log.md", []), selection("_spaces/tasks.md", [])],
    );

    fireEvent.click(screen.getByRole("button", { name: `${SESSION_SPACE_DELETE} Log` }));
    fireEvent.click(await screen.findByRole("button", { name: SESSION_SPACE_DELETE_CONFIRM }));

    await waitFor(() => expect(mockDelete).toHaveBeenCalledWith("p1", "_spaces/log.md"));
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
  });

  /**
   * The row is still there afterwards, and says why. A section that removed the
   * row optimistically would show the space gone until the next read put it
   * back — a lie with a delay on it.
   */
  it("says the delete failed rather than letting the row look gone", async () => {
    mockDelete.mockRejectedValue({});
    open([space({ name: "Tasks" })], [selection("_spaces/tasks.md", [])]);

    fireEvent.click(screen.getByRole("button", { name: `${SESSION_SPACE_DELETE} Tasks` }));
    fireEvent.click(await screen.findByRole("button", { name: SESSION_SPACE_DELETE_CONFIRM }));

    expect(await screen.findByText(SESSION_SPACE_DELETE_FAILED)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: `${SESSION_SPACE_EDIT} Tasks` })).toBeInTheDocument();
  });
});

describe("SessionSpaces editing", () => {
  it("opens the editor on the space whose pencil was pressed", async () => {
    open(
      [space({ id: "_spaces/log.md", name: "Log" }), space({ name: "Tasks" })],
      [selection("_spaces/log.md", []), selection("_spaces/tasks.md", [])],
    );

    fireEvent.click(screen.getByRole("button", { name: `${SESSION_SPACE_EDIT} Log` }));

    expect(await screen.findByLabelText("Name")).toHaveValue("Log");
  });

  it("opens an empty editor for a new space", async () => {
    open([space()], [selection("_spaces/tasks.md", [])]);

    fireEvent.click(screen.getByRole("button", { name: SESSION_SPACES_NEW }));

    expect(await screen.findByLabelText("Name")).toHaveValue("");
  });

  /**
   * The tags the editor offers come from what the spaces already selected — the
   * pool was walked once to answer `sessions_space_files`, and asking for it
   * again to fill a dropdown would be a second walk for a combobox.
   */
  it("offers the session's own tags to the editor without a second read", async () => {
    open([space()], [selection("_spaces/tasks.md", ["task-migrate.md"])]);

    fireEvent.click(screen.getByRole("button", { name: SESSION_SPACES_NEW }));

    const field = await screen.findByLabelText("Add a tag");
    fireEvent.focus(field);
    fireEvent.change(field, { target: { value: "ta" } });

    expect(await screen.findByRole("option", { name: /task/ })).toBeInTheDocument();
  });
});

/**
 * Matrix rows 1–8 (Story 49.2, FR-273).
 *
 * Rows 1–6 are Rust's decision — `creatable_kind` owns whether a query names a
 * creatable kind, and its own tests own those cases. What this file can prove
 * is the half that is the webview's: the VM's answer becomes a control or
 * becomes nothing, and TypeScript never reads the query to second-guess it.
 */
describe("SessionSpaces new note", () => {
  it.each([
    ["1", "tag:task", "Tasks", "task"],
    ["4", "tag:ref", "References", "ref"],
  ])("row %s: offers a create in a space Rust gave a kind, named after the space", (_row, query, name, kind) => {
    open([space({ query, name, newFileKind: kind })], [selection("_spaces/tasks.md", [])]);

    expect(screen.getByRole("button", { name: `${SESSION_SPACE_NEW_NOTE} ${name}` })).toBeEnabled();
  });

  /** The four shapes Rust refuses, and the fault sentence row 5 carries. */
  const noKind: Array<[string, string, string, string | null]> = [
    ["2", "tag:about", "About", null],
    ["3", "tag:log AND date:today", "Today's log", null],
    ["5", "tag:(", "Broken", "This space's query can't be read: unclosed ("],
    ["6", "tag:project/alpha", "Alpha", null],
  ];

  /**
   * Absent, never disabled — the `showNoteInFiles` precedent. A control that
   * exists only to refuse teaches nobody what the space is for, and the four
   * reasons above are indistinguishable from the webview's side: they all
   * arrive as `newFileKind: null`, which is the point of deriving it in Rust.
   *
   * A creatable space is rendered BESIDE the refused one, so what this proves
   * is "absent here" and not "absent everywhere": read as it first shipped,
   * every row of this matrix stayed green against a build with no create
   * control at all.
   */
  it.each(
    noKind,
  )("row %s: offers nothing in a space Rust gave no kind", (_row, query, name, error) => {
    open(
      [
        space({ id: `_spaces/${name}.md`, query, name, newFileKind: null, error }),
        space({ newFileKind: "task" }),
      ],
      [selection(`_spaces/${name}.md`, []), selection("_spaces/tasks.md", [])],
    );

    expect(screen.queryByRole("button", { name: `${SESSION_SPACE_NEW_NOTE} ${name}` })).toBeNull();
    // Not hidden behind `disabled` either: a query for every create control on
    // the surface, which must find exactly the one the control space owns.
    const controls = screen.getAllByRole("button", { name: /^New note in/ });
    expect(controls).toHaveLength(1);
    expect(controls[0]).toBe(
      screen.getByRole("button", { name: `${SESSION_SPACE_NEW_NOTE} Tasks` }),
    );
  });

  /**
   * Row 5, both clauses in one case.
   *
   * The matrix row says `None` AND "the fault sentence is unchanged", and those
   * halves were carried by two unrelated tests — the create case asserted the
   * absence and the pre-existing broken-query case asserted the sentence, so
   * neither could see a build that answered one and dropped the other.
   */
  it("row 5: a space keeper could not read says so and offers no create", () => {
    open(
      [space({ name: "Broken", query: "tag:(", newFileKind: null, error: "unclosed (" })],
      [selection("_spaces/tasks.md", [], "unclosed (")],
    );

    expect(screen.getByText(SESSION_SPACE_BROKEN_SUBTITLE)).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: /^New note in/ })).toBeNull();
  });

  /**
   * The gate its sibling verb already has (`session-file-actions.tsx` refuses
   * `New prompt` on the same shape).
   *
   * `sessions_file_new_kind` writes `YYYY-MM-DD-HHMM-<slug>.md` into the
   * session ROOT, and `sessions_root.rs::read_ref_sources` builds a
   * folder-shaped session's pool from `README.md` plus `refs/` and `prompts/`
   * only. So the file a space created there is in no space's candidate set and
   * in no Unfiled list: the press writes, navigates, re-reads, and the space it
   * was pressed in is exactly as empty as before, with nothing said.
   */
  it("offers no create on a folder-shaped session, whose pool could never list it", () => {
    open([space({ newFileKind: "task" })], [selection("_spaces/tasks.md", [])], "folder");

    expect(screen.queryByRole("button", { name: /^New note in/ })).toBeNull();
    // The LISTING is still true under the folder contract, so the section is
    // not hidden — only its write verb is.
    expect(screen.getByRole("button", { name: `${SESSION_SPACE_EDIT} Tasks` })).toBeInTheDocument();
  });

  /**
   * One create in flight for the whole section, not one per space.
   *
   * `sessions_file_new_kind` stamps the name from the clock to the minute and
   * from an empty title, so two creates started before either write lands both
   * read the same `taken_in`, both get `YYYY-MM-DD-HHMM-untitled.md` out of
   * `files::new_stamped`, and `files::compile_new`'s plain `WriteFile` lets the
   * second overwrite the first — the `tag: task` file becomes a `tag: log` one.
   * Per-space flags are what made that reachable from the UI.
   */
  it("keeps one create in flight across the section, so two cannot collide on a name", async () => {
    // The executor form, not `Promise.withResolvers`: the project compiles
    // against `lib: ES2020`, where that constructor method does not exist.
    let landFirst!: (subpath: string) => void;
    mockNewKind
      .mockImplementationOnce(
        () =>
          new Promise<string>((resolve) => {
            landFirst = resolve;
          }),
      )
      .mockResolvedValue("60-sessions/active/s/untitled-2.md");
    open(
      [
        space({ newFileKind: "task" }),
        space({ id: "_spaces/log.md", name: "Log", query: "tag:log", newFileKind: "log" }),
      ],
      [selection("_spaces/tasks.md", []), selection("_spaces/log.md", [])],
    );

    fireEvent.click(screen.getByRole("button", { name: `${SESSION_SPACE_NEW_NOTE} Tasks` }));

    const other = screen.getByRole("button", { name: `${SESSION_SPACE_NEW_NOTE} Log` });
    expect(other).toBeDisabled();
    fireEvent.click(other);
    expect(mockNewKind).toHaveBeenCalledTimes(1);

    landFirst("60-sessions/active/s/untitled.md");
    await waitFor(() => expect(other).toBeEnabled());
  });

  /**
   * Row 7. The kind is Rust's word passed straight back, the title is empty —
   * `sessions_file_new_kind` names that file `untitled` and the person types the
   * real title into the note they are now looking at, exactly as New log and New
   * prompt already behave. `onChanged` because the definitions and the
   * selections are two payloads: only the re-read puts the new row in the space.
   */
  it("row 7: writes the kind into this session, re-reads before opening, and opens what it wrote", async () => {
    mockNewKind.mockResolvedValue("60-sessions/active/s/untitled.md");
    const { onChanged } = open(
      [space({ newFileKind: "task" })],
      [selection("_spaces/tasks.md", [])],
    );
    const before = opened();
    // What the strip was showing at the moment the re-read ran. `newNote`'s
    // comment calls that order load-bearing — only the re-read puts the new row
    // in the space — and asserting the two facts apart lets the two lines be
    // swapped with the suite still green.
    let atReRead: unknown;
    onChanged.mockImplementation(() => {
      atReRead = opened();
    });

    fireEvent.click(screen.getByRole("button", { name: `${SESSION_SPACE_NEW_NOTE} Tasks` }));

    await waitFor(() =>
      expect(mockNewKind).toHaveBeenCalledWith("p1", "01J5AAAAAAAAAAAAAAAAAAAAAA", "task", ""),
    );
    expect(mockNewKind).toHaveBeenCalledTimes(1);
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    expect(atReRead).toEqual(before);
    // The subpath Rust answered with, opened as-is and never re-joined here.
    // No vault is configured in this case, so the file viewer is the honest
    // surface; row 12 is this same press in a vault-backed zone.
    await waitFor(() =>
      expect(opened()).toEqual({
        kind: "file",
        profileId: "p1",
        relativePath: "60-sessions/active/s/untitled.md",
      }),
    );
  });

  /**
   * Row 8. Rust's own sentence, in the section's live region — it names the file
   * keeper could not write, which is the difference between a bug report and a
   * `chmod`. And nothing moves: navigating after a refused write would show the
   * person a panel for a file that does not exist.
   *
   * Prefixed with the control's own name, because that live region sits under
   * the `Spaces` heading above every section: with two creatable spaces on
   * screen, an unprefixed sentence cannot say which of them failed.
   */
  it("row 8: says why a refused create failed, names its space, and goes nowhere", async () => {
    const said = "60-sessions/active/s/untitled.md: Permission denied (os error 13)";
    mockNewKind.mockRejectedValue({ message: said });
    const { onChanged } = open(
      [space({ newFileKind: "task" })],
      [selection("_spaces/tasks.md", [])],
    );
    const before = opened();

    fireEvent.click(screen.getByRole("button", { name: `${SESSION_SPACE_NEW_NOTE} Tasks` }));

    expect(await screen.findByText(`${SESSION_SPACE_NEW_NOTE} Tasks: ${said}`)).toBeInTheDocument();
    expect(opened()).toEqual(before);
    expect(onChanged).not.toHaveBeenCalled();
  });

  it("still says the create failed when the rejection carries no sentence", async () => {
    mockNewKind.mockRejectedValue({});
    open([space({ newFileKind: "task" })], [selection("_spaces/tasks.md", [])]);

    fireEvent.click(screen.getByRole("button", { name: `${SESSION_SPACE_NEW_NOTE} Tasks` }));

    expect(
      await screen.findByText(`${SESSION_SPACE_NEW_NOTE} Tasks: ${SESSION_SPACE_NEW_NOTE_FAILED}`),
    ).toBeInTheDocument();
  });
});

/**
 * Matrix rows 9, 11 and 12 (Story 49.2, FR-274) — the other half of row 10's
 * old single case.
 *
 * One opener serves the row click and the create, so a file made in a
 * vault-backed space opens in the same surface a row in that space opens in.
 * Two openers is how those two answers drift apart.
 */
describe("SessionSpaces opens a row as a note", () => {
  it("row 9: opens the note behind a row when the zone lives inside a vault", async () => {
    zoneInsideAVault();
    mockTree.mockResolvedValue(
      folderWith(
        { id: "note-other", path: "active/s/task-board.md" },
        { id: "note-1", path: "active/s/task-migrate.md" },
      ),
    );
    open([space()], [selection("_spaces/tasks.md", ["task-migrate.md"])]);

    fireEvent.click(screen.getByRole("button", { name: /task-migrate/ }));

    await waitFor(() =>
      expect(opened()).toEqual({ kind: "note", vaultId: "vault-1", noteId: "note-1" }),
    );
    expect(primaryViewStore.getState().view).toBe("notes");
    // The file's own directory INSIDE the vault, not the profile-relative one:
    // listing `60-sessions/active/s` would name a folder the vault does not have.
    expect(mockTree).toHaveBeenCalledWith("vault-1", "active/s");
    // And the vault was made active first, or the notes pane shows nothing: it
    // only renders the open note while that note's vault is the active one.
    expect(mockSetActive).toHaveBeenCalledWith("vault-1");
  });

  /**
   * Row 11. The index has not caught up — Story 45.18 already worded this, and
   * the section prints that sentence rather than inventing a second one. The
   * file still opens: withholding bytes keeper can read because the nicer
   * surface was unavailable would be keeper punishing the reader for a race.
   */
  it("row 11: says a vault file has no note yet, and still opens the file", async () => {
    zoneInsideAVault();
    mockTree.mockResolvedValue(folderWith({ id: "note-other", path: "active/s/task-board.md" }));
    open([space()], [selection("_spaces/tasks.md", ["task-migrate.md"])]);

    fireEvent.click(screen.getByRole("button", { name: /task-migrate/ }));

    expect(
      await screen.findByText(/no note indexed at active\/s\/task-migrate\.md/),
    ).toBeInTheDocument();
    expect(opened()).toEqual({
      kind: "file",
      profileId: "p1",
      relativePath: "60-sessions/active/s/task-migrate.md",
    });
    expect(primaryViewStore.getState().view).toBe("sessions");
  });

  it("row 12: opens a file it just created as a note, through that same opener", async () => {
    zoneInsideAVault();
    mockNewKind.mockResolvedValue("60-sessions/active/s/untitled.md");
    mockTree.mockResolvedValue(
      folderWith(
        { id: "note-other", path: "active/s/task-board.md" },
        { id: "note-new", path: "active/s/untitled.md" },
      ),
    );
    open([space({ newFileKind: "task" })], [selection("_spaces/tasks.md", [])]);

    fireEvent.click(screen.getByRole("button", { name: `${SESSION_SPACE_NEW_NOTE} Tasks` }));

    await waitFor(() =>
      expect(opened()).toEqual({ kind: "note", vaultId: "vault-1", noteId: "note-new" }),
    );
    expect(primaryViewStore.getState().view).toBe("notes");
  });

  /**
   * The gesture, which row 9 cannot see: it asserts the ACTIVE panel's target,
   * and that is the newly focused one either way.
   *
   * AD-90 gives a single click on a list row the replace gesture
   * (`notes-pane.tsx:289-296`), and the file arm has always used it. Left as
   * `openPanel`, one click on a row would grow the strip inside a vault and
   * replace outside it — the same click meaning two things, decided by
   * configuration the person pressing cannot see.
   */
  it("opens a row's note in the panel that was showing the row, not beside it", async () => {
    zoneInsideAVault();
    mockTree.mockResolvedValue(
      folderWith(
        { id: "note-other", path: "active/s/task-board.md" },
        { id: "note-1", path: "active/s/task-migrate.md" },
      ),
    );
    open([space()], [selection("_spaces/tasks.md", ["task-migrate.md"])]);
    const strip = panelsStore.getState().panels.length;

    fireEvent.click(screen.getByRole("button", { name: /task-migrate/ }));

    await waitFor(() =>
      expect(opened()).toEqual({ kind: "note", vaultId: "vault-1", noteId: "note-1" }),
    );
    expect(panelsStore.getState().panels).toHaveLength(strip);
  });

  /**
   * Two rows, two resolutions, landing out of order.
   *
   * `notes_tree` lists the file's OWN vault directory, so a row in a deep,
   * crowded subfolder is genuinely slower than one at the vault root — a first
   * click really can finish after a second. Without the press guard the loser
   * takes the panel, the active vault, the primary view and the notice with it,
   * and the person ends up on a file they did not click last.
   */
  it("ignores a row resolution that a later press has superseded", async () => {
    zoneInsideAVault();
    // The executor form, not `Promise.withResolvers`: `lib: ES2020`.
    let landSlow!: (folder: NoteFolderVm) => void;
    mockTree
      .mockImplementationOnce(
        () =>
          new Promise<NoteFolderVm>((resolve) => {
            landSlow = resolve;
          }),
      )
      .mockResolvedValue(folderWith({ id: "note-board", path: "active/s/task-board.md" }));
    open([space()], [selection("_spaces/tasks.md", ["task-migrate.md", "task-board.md"])]);

    fireEvent.click(screen.getByRole("button", { name: /task-migrate/ }));
    fireEvent.click(screen.getByRole("button", { name: /task-board/ }));

    await waitFor(() =>
      expect(opened()).toEqual({ kind: "note", vaultId: "vault-1", noteId: "note-board" }),
    );

    landSlow(folderWith({ id: "note-migrate", path: "active/s/task-migrate.md" }));
    // A macrotask, so every microtask the stale chain still owes runs before
    // the assertion — awaiting a resolved promise would only drain the queue as
    // it stands, and this chain queues two more `.then`s.
    await new Promise((resolve) => setTimeout(resolve, 0));

    expect(opened()).toEqual({ kind: "note", vaultId: "vault-1", noteId: "note-board" });
  });
});

/**
 * `vaults: null` is "keeper has not looked yet", never "you have no vault"
 * (`notes-vaults.ts` keeps the two apart on purpose). Every other case in this
 * file seeds the mirror, which marks it hydrated — so these two are the only
 * ones that can see the window in which it is not.
 */
describe("SessionSpaces opens a row before the vault list has landed", () => {
  it("waits for the vault list rather than reading `not looked yet` as `no vault`", async () => {
    // The executor form, not `Promise.withResolvers`: `lib: ES2020`.
    let landVaults!: (vaults: NoteVaultVm[]) => void;
    mockVaultsRead.mockReturnValue(
      new Promise<NoteVaultVm[]>((resolve) => {
        landVaults = resolve;
      }),
    );
    mockTree.mockResolvedValue(
      folderWith(
        { id: "note-other", path: "active/s/task-board.md" },
        { id: "note-1", path: "active/s/task-migrate.md" },
      ),
    );
    open([space()], [selection("_spaces/tasks.md", ["task-migrate.md"])]);
    const before = opened();

    fireEvent.click(screen.getByRole("button", { name: /task-migrate/ }));

    // Nothing decided yet: the press is waiting on the read, not guessing.
    expect(opened()).toEqual(before);

    landVaults([vault("vault-1", "p1", "60-sessions"), vault("vault-2", "p2", "60-sessions")]);

    await waitFor(() =>
      expect(opened()).toEqual({ kind: "note", vaultId: "vault-1", noteId: "note-1" }),
    );
  });

  /**
   * The permanent case. `ensureNotesVaultsHydrated` is best-effort and leaves
   * the mirror unhydrated on a rejected read, and this section calls it once on
   * mount — so after one transient `notes_vaults` failure every row in a
   * vault-backed zone would open as a file, silently, for the life of the
   * mount. Silence belongs to the configuration, not to the failure.
   */
  it("says the vault list could not be read instead of opening the file in silence", async () => {
    mockVaultsRead.mockRejectedValue({ message: "notes_vaults: broken pipe" });
    open([space()], [selection("_spaces/tasks.md", ["task-migrate.md"])]);

    fireEvent.click(screen.getByRole("button", { name: /task-migrate/ }));

    expect(await screen.findByText(SESSION_SPACE_VAULTS_UNKNOWN)).toBeInTheDocument();
    // And the bytes are still shown: withholding a file keeper can read because
    // the nicer surface could not be resolved would punish the reader twice.
    expect(opened()).toEqual({
      kind: "file",
      profileId: "p1",
      relativePath: "60-sessions/active/s/task-migrate.md",
    });
  });
});
