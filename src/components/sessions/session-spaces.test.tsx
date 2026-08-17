import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { useState } from "react";
import { afterEach, beforeEach, describe, expect, it, type Mock, vi } from "vitest";
import type { NoteVaultVm, SessionSpaceFilesVm, SessionSpaceVm } from "@/lib/ipc/client";

// The section writes through three commands, and the editor it opens reaches
// for two more. All of them are stubbed at the IPC boundary so the real
// components — the real refusals, the real copy, the real gates — are what
// these tests exercise. The two note-index commands stay stubbed although the
// section no longer calls either: not-called is the assertion that Story
// 49.2's note arm has not come back (see row 15).
vi.mock("@/lib/ipc/client", () => ({
  sessionsSpaceDelete: vi.fn(),
  sessionsSpacesRestore: vi.fn(),
  sessionsSpaceSave: vi.fn(),
  sessionsFileNewKind: vi.fn(),
  notesSpaceTerms: vi.fn(),
  notesTree: vi.fn(),
  // The mirror's own reads. The section no longer hydrates it — nothing here
  // asks about vaults any more — so these stay stubbed only to keep a store a
  // fixture seeds from reaching the real IPC layer.
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
  SESSION_SPACE_ROWS_LESS,
  SESSION_SPACE_ROWS_MORE,
  SESSION_SPACE_SETTINGS_SUBTITLE,
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
import {
  hydrateSessionSpacesFold,
  readSessionSpacesFold,
  resetSessionSpacesFoldForTest,
  SESSION_SPACES_FOLD_COOKIE,
  setSpaceFolded,
  setSpacesFoldedDefault,
  spaceFoldKey,
} from "@/lib/stores/session-spaces-fold";

const mockDelete = vi.mocked(sessionsSpaceDelete);
const mockRestore = vi.mocked(sessionsSpacesRestore);
const mockTerms = vi.mocked(notesSpaceTerms);
const mockNewKind = vi.mocked(sessionsFileNewKind);
const mockTree = vi.mocked(notesTree);
const mockSetActive = vi.mocked(notesVaultSetActive);
/** The mirror's own read — stubbed so seeding the store cannot trigger IPC. */
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
    // Both `null` by default, which is a space file that says nothing about how
    // it opens or how much it shows — the shape every fixture below had before
    // Story 51.3, so no existing assertion changes meaning.
    folded: p.folded ?? null,
    rows: p.rows ?? null,
  };
}

/**
 * Rust's own refusal sentences, as the payload carries them.
 *
 * Written out here as FIXTURE data and never imported from the component:
 * `shape::KindHasNoHome` composes these once and `shape.rs`'s own tests own
 * their wording. What this file claims is narrower and is the only half that
 * is the webview's — whatever sentence arrives on `noHome` is printed where
 * the button would have been, and the button is gone. Story 50.1 shipped a
 * TypeScript copy of the mapping AND of these sentences; the copy had already
 * forked the wording, and importing a constant back out of the component would
 * be this suite testing that copy against itself.
 */
const NO_TASK_HOME =
  "a folder-shaped session keeps no task file: that contract has no directory to put one in, \
and writing it anywhere else would produce a file no space in this session can list. Migrate the \
session to the flat shape, where every kind is a tag on a file at the root.";

const NO_LOG_FILE =
  "a folder-shaped session's log is a `### ` entry under `## Log` in README.md, not a file — use \
New log, which appends one there.";

function selection(
  spaceId: string,
  names: string[],
  error: string | null = null,
  /**
   * Why this session's contract keeps no home for the space's kind — Rust's
   * answer, per session, which is what replaced the `shape` prop.
   */
  noHome: string | null = null,
  /** Rust's per-space answer that the record this space wanted already exists. */
  openRecord = false,
) {
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
    noHome,
    openRecord,
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

/**
 * The section under a parent that owns the one create-in-flight flag.
 *
 * `writing` is a PROP now, not this component's state: the Files heading offers
 * creates on the same session through the same command and with the same empty
 * title, so the flag that removes the colliding press has to span both and
 * `SessionDetail` holds it. This harness plays that parent, which is what keeps
 * the in-flight case below a claim about the section's behaviour. That the two
 * components actually share ONE flag is `session-detail.test.tsx`'s claim, and
 * no harness can make it here.
 *
 * There is no `shape` prop any more: which kinds this session's contract has no
 * home for arrives per space on `noHome`, composed by Rust.
 */
function Harness({
  spaces,
  selections,
  onChanged,
  onOpenRecord,
}: {
  spaces: SessionSpaceVm[];
  selections: SessionSpaceFilesVm[] | null;
  onChanged: () => void;
  onOpenRecord: () => void;
}) {
  const [writing, setWriting] = useState(false);
  return (
    <SessionSpaces
      rootId="p1"
      sessionId="01J5AAAAAAAAAAAAAAAAAAAAAA"
      spaces={spaces}
      selections={selections}
      writing={writing}
      onWriting={setWriting}
      onChanged={onChanged}
      recordLabel={RECORD_LABEL}
      onOpenRecord={onOpenRecord}
    />
  );
}

/** What the detail calls the session's own record — a fixture, since the real
 *  label is composed from the shape by `session-detail.tsx`. */
const RECORD_LABEL = "Open about.md";

function open(
  spaces: SessionSpaceVm[],
  selections: SessionSpaceFilesVm[] | null,
): { onChanged: Mock; onOpenRecord: Mock; unmount: () => void } {
  const onChanged = vi.fn();
  const onOpenRecord = vi.fn();
  const view = render(
    <Harness
      spaces={spaces}
      selections={selections}
      onChanged={onChanged}
      onOpenRecord={onOpenRecord}
    />,
  );
  return { onChanged, onOpenRecord, unmount: view.unmount };
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
  mockSetActive.mockReset();
  mockSetActive.mockResolvedValue(undefined);
  mockVaultsRead.mockReset();
  mockVaultsRead.mockResolvedValue([]);
  resetNotesVaultsStoreForTest();
  // Where a person pressing a space row actually is, so row 15's assertion
  // that the view did NOT switch is watching something that could move.
  primaryViewStore.getState().setView("sessions");
  resetPanelsStoreForTest();
  // Nothing folded and nothing recorded: the fold is a document-wide store and
  // a cookie, so a test that folded a space would otherwise fold it for the
  // next one (Story 49.3).
  resetSessionSpacesFoldForTest();
});

afterEach(() => {
  vi.clearAllMocks();
  resetSessionSpacesFoldForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: clearing the fold this suite wrote
  document.cookie = `${SESSION_SPACES_FOLD_COOKIE}=; path=/; max-age=0`;
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
   * Matrix row 15 (Story 50.1), which was row 10 (Story 49.2) — and now the
   * ONLY opener case there is.
   *
   * Opens through the ONE file target the tree and the Files pane use (AD-109),
   * on the `subpath` Rust composed (AD-65): a second path-join in TypeScript is
   * a second answer to where a file lives.
   *
   * **The fixture is the impossible one, deliberately.** The vault seeded here
   * CONTAINS the session's zone — exactly the state Story 49.2's deleted note
   * arm keyed on, and exactly the state `SessionsConfig::validate`
   * (`keeper-sync/src/profile/mod.rs:648-654`) refuses to let anyone configure:
   * "one folder cannot be both a vault and a sessions zone". Seeding it anyway
   * is what makes this a regression test rather than a tautology. Put the arm
   * back and this row resolves as a note, switches the primary view and reaches
   * for the note index — all three of which are asserted against.
   */
  it("row 15: opens the file target, and never resolves a row as a note", async () => {
    zoneInsideAVault();
    open([space()], [selection("_spaces/tasks.md", ["task-migrate.md"])]);

    fireEvent.click(screen.getByRole("button", { name: /task-migrate/ }));

    const target = {
      kind: "file",
      profileId: "p1",
      relativePath: "60-sessions/active/s/task-migrate.md",
    };
    // Landed on the press, with nothing awaited: the opener is one store write.
    expect(opened()).toEqual(target);
    // A macrotask, so an arm that had merely become asynchronous would still
    // have run by the time the four assertions below look. The executor form,
    // not `Promise.withResolvers`: the project compiles against `lib: ES2020`,
    // where that constructor method does not exist.
    await new Promise((resolve) => setTimeout(resolve, 0));
    expect(opened()).toEqual(target);
    expect(primaryViewStore.getState().view).toBe("sessions");
    expect(mockTree).not.toHaveBeenCalled();
    expect(mockSetActive).not.toHaveBeenCalled();
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
 * Matrix rows 1–8 (Story 49.2, FR-273) and 12–14 (Story 50.1, FR-277).
 *
 * Rows 1–6 of BOTH matrices are Rust's decision — `creatable_kind` owns whether
 * a query names a creatable kind and `shape::kind_dir` owns where that kind
 * lives, and each has its own tests. What this file can prove is the half that
 * is the webview's: the VM's answer becomes a control or becomes nothing, the
 * control is visible without a pointer, and a kind this session's contract has
 * no home for is explained rather than silently missing.
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
   * Row 12. The folder contract has no tasks file — `shape::kind_dir` refuses
   * `(Folder, Task)` and `sessions_space_files` puts that refusal on the
   * payload — so the control is absent, and the section says WHY, where the
   * button would have been. Absent AND silent is the state the owner reported
   * as "I only see the count".
   *
   * **The shape does not appear in this test, because it no longer reaches the
   * component.** Story 50.1 fed a `shape` prop to a TypeScript copy of
   * `kind_dir`, so this row tested that copy against a fixture of its own
   * input; the day Rust gave the folder contract a tasks home it would have
   * stayed green while the button stayed hidden. What is asserted now is the
   * only thing the webview decides: a sentence on `noHome` suppresses the
   * create and is printed instead of it.
   */
  it("row 12: says a folder-shaped session keeps no tasks file, and offers no create", () => {
    open([space({ newFileKind: "task" })], [selection("_spaces/tasks.md", [], null, NO_TASK_HOME)]);

    expect(screen.queryByRole("button", { name: /^New note in/ })).toBeNull();
    expect(screen.getByText(NO_TASK_HOME)).toBeInTheDocument();
    // The LISTING is true under both contracts, so the section is not hidden —
    // only its write verb is.
    expect(screen.getByRole("button", { name: `${SESSION_SPACE_EDIT} Tasks` })).toBeInTheDocument();
  });

  /**
   * Row 12's other half. A folder-shaped session's log is a `## Log` heading in
   * README.md, so the space carries the sentence that points at the verb which
   * already appends one rather than growing a second log writer.
   *
   * A DIFFERENT sentence from the tasks one, which is the property the whole
   * `no_home` projection buys: `KindHasNoHome` has three variants because
   * "migrate the session" is right for a task and wrong for a log, and a
   * boolean on the wire would have made this surface write the second wording.
   */
  it("row 12: points a folder-shaped Log space at the log verb rather than a file", () => {
    open(
      [space({ id: "_spaces/log.md", name: "Log", query: "tag:log", newFileKind: "log" })],
      [selection("_spaces/log.md", [], null, NO_LOG_FILE)],
    );

    expect(screen.queryByRole("button", { name: /^New note in/ })).toBeNull();
    expect(screen.getByText(NO_LOG_FILE)).toBeInTheDocument();
    expect(screen.queryByText(NO_TASK_HOME)).toBeNull();
  });

  /**
   * And the line belongs to the session's contract, not to the section: where
   * every kind has a home, `no_home` is null on every space, nothing is
   * explained away and both spaces keep their control.
   */
  it("says nothing about homes where every kind has one", () => {
    open(
      [
        space({ newFileKind: "task" }),
        space({ id: "_spaces/log.md", name: "Log", query: "tag:log", newFileKind: "log" }),
      ],
      [selection("_spaces/tasks.md", []), selection("_spaces/log.md", [])],
    );

    expect(screen.queryByText(NO_TASK_HOME)).toBeNull();
    expect(screen.queryByText(NO_LOG_FILE)).toBeNull();
    expect(screen.getAllByRole("button", { name: /^New note in/ })).toHaveLength(2);
  });

  /**
   * One space refused and one offered, side by side in one payload.
   *
   * The two cases above each render a single space, so neither can tell "this
   * space's create is suppressed" from "no create renders at all" — the shape
   * of the bug the section shipped with. `no_home` is per space, and this is
   * the assertion that it is read per space.
   */
  it("suppresses only the space whose kind has no home, and only its button", () => {
    open(
      [
        space({ newFileKind: "task" }),
        space({ id: "_spaces/refs.md", name: "References", query: "tag:ref", newFileKind: "ref" }),
      ],
      [selection("_spaces/tasks.md", [], null, NO_TASK_HOME), selection("_spaces/refs.md", [])],
    );

    const controls = screen.getAllByRole("button", { name: /^New note in/ });
    expect(controls).toHaveLength(1);
    expect(controls[0]).toBe(
      screen.getByRole("button", { name: `${SESSION_SPACE_NEW_NOTE} References` }),
    );
    expect(screen.getByText(NO_TASK_HOME)).toBeInTheDocument();
  });

  /**
   * The answer has not arrived yet, so neither has the verb.
   *
   * While `selections` is null the section knows a space is called Tasks and
   * does not yet know whether this session's contract keeps a home for a task
   * — the same moment {@link SESSION_SPACES_LOADING} exists for. Offering the
   * create then means drawing a button that vanishes a tick later on every
   * folder-shaped session, which is a wrong answer shown confidently.
   */
  it("offers no create until Rust has answered whether the kind has a home", () => {
    open([space({ newFileKind: "task" })], null);

    expect(screen.queryByRole("button", { name: /^New note in/ })).toBeNull();
    expect(screen.getByText(SESSION_SPACES_LOADING)).toBeInTheDocument();
  });

  /**
   * Rows 13 and 14. The one verb a section exists to offer is not hidden behind
   * a pointer — the owner's report was literally "I don't see the button".
   *
   * **One case, where the matrix has two rows.** Rows 13 and 14 differ only in
   * the session's contract, and the contract no longer reaches this component:
   * a folder-shaped session keeps references in `refs/`, so `kind_dir` answers
   * a directory, so `no_home` is null — exactly as it is on a flat one. Feeding
   * a `shape` prop to tell the two apart is what the deleted TypeScript mirror
   * did, and it made the pair a test of the mirror. Row 13's shape-dependent
   * half is the write below, whose subpath carries the `refs/` segment Rust
   * composed; the mapping itself is `shape.rs`'s rows 1–3.
   *
   * jsdom applies no stylesheet, so `toBeVisible` cannot see a Tailwind
   * `opacity-0`; the class is the fact. It is asserted against the two siblings
   * that still carry it, so this cannot pass on a build that simply has no
   * opacity classes left anywhere.
   */
  it("rows 13 and 14: the create is visible without hovering, wherever the kind has a home", () => {
    open(
      [space({ id: "_spaces/refs.md", name: "References", query: "tag:ref", newFileKind: "ref" })],
      [selection("_spaces/refs.md", [])],
    );

    const create = screen.getByRole("button", { name: `${SESSION_SPACE_NEW_NOTE} References` });
    expect(create).toBeEnabled();
    expect(create.className).not.toMatch(/\bopacity-0\b/);
    for (const label of [SESSION_SPACE_EDIT, SESSION_SPACE_DELETE]) {
      expect(screen.getByRole("button", { name: `${label} References` }).className).toMatch(
        /\bopacity-0\b/,
      );
    }
  });

  /**
   * Row 13's other half, and the whole point of Story 50.1: the press goes
   * through on a folder-shaped session, where 49.2 suppressed it. WHERE the
   * file lands is Rust's answer and `shape.rs`'s own tests own it; the subpath
   * comes back with its `refs/` segment already on it, and this file opens that
   * without composing anything.
   */
  it("row 13: writes into a folder-shaped session instead of suppressing the verb", async () => {
    const written = "60-sessions/active/s/refs/2026-08-16-0900-untitled.md";
    mockNewKind.mockResolvedValue(written);
    const { onChanged } = open(
      [space({ id: "_spaces/refs.md", name: "References", query: "tag:ref", newFileKind: "ref" })],
      [selection("_spaces/refs.md", [])],
    );

    fireEvent.click(screen.getByRole("button", { name: `${SESSION_SPACE_NEW_NOTE} References` }));

    await waitFor(() =>
      expect(mockNewKind).toHaveBeenCalledWith("p1", "01J5AAAAAAAAAAAAAAAAAAAAAA", "ref", ""),
    );
    await waitFor(() => expect(onChanged).toHaveBeenCalled());
    await waitFor(() =>
      expect(opened()).toEqual({ kind: "file", profileId: "p1", relativePath: written }),
    );
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
 * Matrix rows 1–4, 8 and 11 (Story 49.3, FR-275, FR-276).
 *
 * The fold's own mechanism — the disclosure, the verb in its name,
 * `aria-expanded`, the hidden body — belongs to `FoldSection` and is covered by
 * `rail-fold.test.tsx`. What is this section's own is the composition: which
 * spaces the setting moves, which spaces it must not, and that the header's
 * controls and sentences stay reachable with the rows shut.
 *
 * Row 5 — the restore across a remount — is at the MOUNT POINT, in
 * `session-detail.test.tsx`, because no test in this file can see that nothing
 * calls `hydrateSessionSpacesFold` (DW-172). The remount case here is the
 * store's half of it: what the cookie carries, given that somebody hydrates.
 */
describe("SessionSpaces folds", () => {
  const TASKS = spaceFoldKey("p1", "_spaces/tasks.md");
  const LOG = spaceFoldKey("p1", "_spaces/log.md");

  /** Tasks and Log, both with something in them, in the zone's own order. */
  function two(): { spaces: SessionSpaceVm[]; selections: SessionSpaceFilesVm[] } {
    return {
      spaces: [space({ name: "Tasks" }), space({ id: "_spaces/log.md", name: "Log" })],
      selections: [
        selection("_spaces/tasks.md", ["task-migrate.md"]),
        selection("_spaces/log.md", ["2026-08-16.md"]),
      ],
    };
  }

  it("row 1: arrives open when the setting is off and nothing is recorded", () => {
    open([space({ name: "Tasks" })], [selection("_spaces/tasks.md", ["task-migrate.md"])]);

    expect(screen.getByRole("list", { name: "Tasks" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Collapse Tasks" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
  });

  /**
   * Row 2. Folded on arrival, and everything the header carries is still there
   * — the count, the fault-lamp column and all three verbs. A fold that took
   * the buttons with it would make "shut" mean "read-only".
   */
  it("row 2: arrives folded when the setting says so, and keeps its whole header", () => {
    setSpacesFoldedDefault(true);

    open(
      [space({ name: "Tasks", newFileKind: "task" })],
      [selection("_spaces/tasks.md", ["task-migrate.md", "task-board.md"])],
    );

    expect(screen.queryByRole("list", { name: "Tasks" })).toBeNull();
    expect(screen.getByRole("button", { name: "Expand Tasks" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    expect(screen.getByText("2")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `${SESSION_SPACE_NEW_NOTE} Tasks` }),
    ).toBeInTheDocument();
    expect(screen.getByRole("button", { name: `${SESSION_SPACE_EDIT} Tasks` })).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `${SESSION_SPACE_DELETE} Tasks` }),
    ).toBeInTheDocument();
  });

  /** Row 3. The press is on the title, and what it records is an answer — not
   *  the absence of one, or the next mount would shut the space again. */
  it("row 3: opens a folded space on a press and records that answer", async () => {
    setSpacesFoldedDefault(true);
    open([space({ name: "Tasks" })], [selection("_spaces/tasks.md", ["task-migrate.md"])]);

    fireEvent.click(screen.getByRole("button", { name: "Expand Tasks" }));

    expect(await screen.findByRole("list", { name: "Tasks" })).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Collapse Tasks" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(readSessionSpacesFold(document.cookie).get(TASKS)).toBe(false);
  });

  /**
   * Rows 4 and 11, which are the same rule read from both ends: the setting
   * moves the spaces nobody has decided about, and only those.
   *
   * Log is opened by hand while the setting is ON, so from then on it must
   * ignore the setting in BOTH directions — including when the setting is
   * turned off and back on, which is the case a store that only remembered
   * "folded" would pass by accident.
   */
  it("rows 4 and 11: the setting moves an untouched space and never a decided one", () => {
    setSpacesFoldedDefault(true);
    const { spaces, selections } = two();
    open(spaces, selections);

    fireEvent.click(screen.getByRole("button", { name: "Expand Log" }));

    expect(screen.getByRole("list", { name: "Log" })).toBeInTheDocument();
    expect(screen.queryByRole("list", { name: "Tasks" })).toBeNull();

    // The switch in Settings, from the store's side.
    act(() => setSpacesFoldedDefault(false));
    expect(screen.getByRole("list", { name: "Tasks" })).toBeInTheDocument();
    expect(screen.getByRole("list", { name: "Log" })).toBeInTheDocument();

    act(() => setSpacesFoldedDefault(true));
    expect(screen.queryByRole("list", { name: "Tasks" })).toBeNull();
    expect(screen.getByRole("list", { name: "Log" })).toBeInTheDocument();
    expect(readSessionSpacesFold(document.cookie).get(LOG)).toBe(false);
    expect(readSessionSpacesFold(document.cookie).has(TASKS)).toBe(false);
  });

  /**
   * Row 4's second half, through the real cookie rather than through the store:
   * a cold start with the setting still ON finds Log as the person left it.
   *
   * The store is wiped between the two renders — a fresh process, nothing
   * remembered — so the only thing carried across is what the browser kept.
   * That the DETAIL calls the hydrate is `session-detail.test.tsx`'s assertion,
   * not this one's.
   */
  it("row 4: an opened space comes back open from the cookie alone", () => {
    setSpacesFoldedDefault(true);
    const { spaces, selections } = two();
    const first = open(spaces, selections);
    fireEvent.click(screen.getByRole("button", { name: "Expand Log" }));
    const written = document.cookie;
    first.unmount();

    resetSessionSpacesFoldForTest();
    hydrateSessionSpacesFold(written, true);
    open(spaces, selections);

    expect(screen.getByRole("button", { name: "Collapse Log" })).toHaveAttribute(
      "aria-expanded",
      "true",
    );
    expect(screen.getByRole("button", { name: "Expand Tasks" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });

  /**
   * Row 8. `actions` is outside the folded region, so a shut space is still a
   * space you can write into — and what the write had to say is readable
   * without opening it again. A create whose refusal landed inside the fold
   * would be a press with no visible result, which is the exact bug a fold
   * introduces and a fold test that only watches the rows never sees.
   */
  it("row 8: creates from a folded space and says what happened, still folded", async () => {
    const said = "60-sessions/active/s/untitled.md: Permission denied (os error 13)";
    mockNewKind.mockRejectedValue({ message: said });
    setSpacesFoldedDefault(true);
    open([space({ name: "Tasks", newFileKind: "task" })], [selection("_spaces/tasks.md", [])]);

    fireEvent.click(screen.getByRole("button", { name: `${SESSION_SPACE_NEW_NOTE} Tasks` }));

    await waitFor(() => expect(mockNewKind).toHaveBeenCalled());
    // Prefixed with the space's own name (story 49.2): this live region sits
    // under the `Spaces` heading, above every section, so an unprefixed
    // sentence could not say which space it was about.
    expect(await screen.findByText(`${SESSION_SPACE_NEW_NOTE} Tasks: ${said}`)).toBeInTheDocument();
    expect(screen.getByRole("button", { name: "Expand Tasks" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });

  it("row 8: restores the defaults with every space folded, and reports it", async () => {
    mockRestore.mockResolvedValue({ names: ["About"] });
    setSpacesFoldedDefault(true);
    open([space({ name: "Tasks" })], [selection("_spaces/tasks.md", [])]);

    fireEvent.click(screen.getByRole("button", { name: SESSION_SPACES_RESTORE }));

    expect(await screen.findByText("Restored About.")).toBeInTheDocument();
    // Still shut. Without this the case tests a verb that lives on the parent
    // header, outside every fold, so deleting the whole fold feature left it
    // green — and reporting is not a reason to reopen what somebody closed.
    expect(screen.getByRole("button", { name: "Expand Tasks" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
  });

  /**
   * Row 8's other half: `notice` is outside the folded region too, so what is
   * WRONG with a space is readable without opening it. A fault whose only
   * explanation folds away with the thing it is about is a lamp nobody can
   * read.
   *
   * `toBeVisible` and not `getByText` alone, which is what makes this case
   * worth having: `getByText` reaches inside a `[hidden]` subtree, so every
   * pre-existing subtitle case would stay green with `notice` moved INTO the
   * fold. Both sentences, because `notice` has two children — the space's own
   * subtitle and the selection's separate error.
   */
  it("row 8: says what is wrong with a folded space where it can still be read", () => {
    setSpacesFoldedDefault(true);
    const selects = "This space's query selects nothing.";
    open(
      [space({ name: "Prompts", error: "Unexpected end of query after `AND`." })],
      [selection("_spaces/tasks.md", [], selects)],
    );

    expect(screen.getByRole("button", { name: "Expand Prompts" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    expect(screen.getByText(SESSION_SPACE_BROKEN_SUBTITLE)).toBeVisible();
    expect(screen.getByText(selects)).toBeVisible();
  });

  /**
   * A space id is its zone-relative path, and a hand-written space file can be
   * called `my tasks.md` — whereupon a pasted id makes `aria-controls` a list
   * of TWO IDREFs, neither of which names an element. The disclosure then
   * controls nothing for assistive technology while working perfectly for a
   * pointer, so nothing else in this suite could see it.
   *
   * The second space is the collision half: `my tasks.md` and `my-tasks.md`
   * differ only in the character a slug would rewrite, and two spaces sharing
   * one region id would point the second disclosure at the first space's rows.
   */
  it("resolves the disclosure's aria-controls when a space filename holds a space", () => {
    const spaced = "_spaces/my tasks.md";
    const hyphen = "_spaces/my-tasks.md";
    open(
      [space({ id: spaced, name: "My tasks" }), space({ id: hyphen, name: "My tasks archive" })],
      [selection(spaced, ["task-migrate.md"]), selection(hyphen, ["task-board.md"])],
    );

    const disclosure = screen.getByRole("button", { name: "Collapse My tasks" });
    const controls = disclosure.getAttribute("aria-controls") ?? "";
    expect(controls).not.toMatch(/\s/);
    const region = document.getElementById(controls);
    // The region it OPENS, not merely a region: the rows this space listed.
    expect(region).not.toBeNull();
    expect(region).toContainElement(screen.getByRole("list", { name: "My tasks" }));

    const sibling = screen.getByRole("button", { name: "Collapse My tasks archive" });
    expect(sibling.getAttribute("aria-controls")).not.toBe(controls);

    fireEvent.click(disclosure);
    expect(region).not.toBeVisible();
    expect(screen.getByRole("list", { name: "My tasks archive" })).toBeVisible();
  });

  /**
   * The glyph's own attributes, which moving it into `FoldSection`'s `icon`
   * prop dropped once (Story 49.3) while the sibling `data-slot="space-dot"`
   * survived. `data-space-icon` is the STORED name, and it is the only thing
   * that tells an icon this build no longer knows from no icon at all: both
   * draw the same fallback glyph, so the rendered DOM is otherwise identical.
   * The notes rail carries the same pair, and the same case for it
   * (`space-list.test.tsx`).
   */
  it("keeps the stored icon name on the space's glyph, known, unknown or absent", () => {
    open(
      [
        space({ name: "Tasks", icon: "anchor" }),
        space({ id: "_spaces/old.md", name: "Old", icon: "no-such-glyph" }),
        space({ id: "_spaces/log.md", name: "Log", icon: null }),
      ],
      [selection("_spaces/tasks.md", []), selection("_spaces/old.md", [])],
    );

    const headers = screen.getAllByRole("button", { name: /^Collapse / });
    const stored = headers.map((header) => [
      header.getAttribute("aria-label"),
      header.querySelector('[data-slot="space-icon"]')?.getAttribute("data-space-icon"),
    ]);
    expect(stored).toEqual([
      ["Collapse Tasks", "anchor"],
      ["Collapse Old", "no-such-glyph"],
      ["Collapse Log", "none"],
    ]);
  });

  /**
   * A space's name is what the person typed, and the header it sits in already
   * holds a count and three controls at ~208px of card.
   *
   * jsdom has no layout, so what is asserted is the arrangement that produces
   * the behaviour: the title takes the spare width and clips, everything beside
   * it refuses to shrink, and the accessible name stays whole however narrow
   * the visible one gets (WCAG 2.5.3).
   */
  it("keeps a long space name inside its own row", () => {
    const name = "Everything I have ever written about the migration";
    open(
      [space({ name, newFileKind: "task" })],
      [selection("_spaces/tasks.md", ["task-migrate.md"])],
    );

    const disclosure = screen.getByRole("button", { name: `Collapse ${name}` });
    expect(disclosure).toHaveClass("min-w-0", "flex-1");
    expect(disclosure.querySelector("span")).toHaveClass("min-w-0", "truncate");
    expect(screen.getByText("1")).toHaveClass("shrink-0");
    expect(screen.getByRole("button", { name: `${SESSION_SPACE_NEW_NOTE} ${name}` })).toHaveClass(
      "shrink-0",
    );
  });
});

/**
 * The space's own answer about how it opens (Story 51.3, FR-289, rows 1–4).
 *
 * The layering itself is `session-spaces-fold.test.ts`'s — this is the half only
 * a render can prove: that the section asks with the space's own `folded` in
 * hand, rather than composing three of the four layers and dropping the file's.
 * Row 2's cookie precedence is asserted here too, because a component that
 * passed `folded` in the wrong argument slot would still pass the store's test.
 */
describe("SessionSpaces per-space fold", () => {
  it("row 1: arrives folded on the space's own say-so with the setting off", () => {
    open(
      [space({ name: "Tasks", folded: true })],
      [selection("_spaces/tasks.md", ["task-migrate.md"])],
    );

    expect(screen.queryByRole("list", { name: "Tasks" })).toBeNull();
    expect(screen.getByRole("button", { name: "Expand Tasks" })).toHaveAttribute(
      "aria-expanded",
      "false",
    );
    // The count is still the header's, so a folded space says how much it holds.
    expect(screen.getByText("1")).toBeInTheDocument();
  });

  it("row 2: the person's own unfold beats the space's `folded: true`", () => {
    setSpaceFolded(spaceFoldKey("p1", "_spaces/tasks.md"), false);

    open(
      [space({ name: "Tasks", folded: true })],
      [selection("_spaces/tasks.md", ["task-migrate.md"])],
    );

    expect(screen.getByRole("list", { name: "Tasks" })).toBeInTheDocument();
  });

  it("row 4: `folded: false` arrives open even with the setting on", () => {
    setSpacesFoldedDefault(true);

    open(
      [space({ name: "Tasks", folded: false })],
      [selection("_spaces/tasks.md", ["task-migrate.md"])],
    );

    expect(screen.getByRole("list", { name: "Tasks" })).toBeInTheDocument();
  });

  /** Row 3, and the reason the setting stays: a space that says nothing is
   *  exactly what a user-global default is for. */
  it("row 3: a space that says nothing follows the setting", () => {
    setSpacesFoldedDefault(true);

    open([space({ name: "Tasks" })], [selection("_spaces/tasks.md", ["task-migrate.md"])]);

    expect(screen.queryByRole("list", { name: "Tasks" })).toBeNull();
  });

  /** Two spaces, two different answers, one render: a component that read the
   *  file's answer once and reused it would fold both. */
  it("gives each space its own answer in the same session", () => {
    open(
      [
        space({ name: "Tasks", folded: true }),
        space({ id: "_spaces/log.md", name: "Log", folded: false }),
      ],
      [
        selection("_spaces/tasks.md", ["task-migrate.md"]),
        selection("_spaces/log.md", ["2026-08-16.md"]),
      ],
    );

    expect(screen.queryByRole("list", { name: "Tasks" })).toBeNull();
    expect(screen.getByRole("list", { name: "Log" })).toBeInTheDocument();
  });
});

/**
 * The row cap (Story 51.3, FR-290, rows 5–7).
 *
 * What every assertion here is really about: `keeper.rows` caps what the section
 * RENDERS and never what the query SELECTED. So the header's count is checked
 * beside the row count in the same test — a cap that had narrowed the selection
 * would pass a rows-only assertion and lie about the total, which is the one
 * failure this key must not have.
 */
describe("SessionSpaces row cap", () => {
  const TEN = Array.from({ length: 10 }, (_, i) => `task-${i}.md`);

  function rows(): HTMLElement[] {
    return screen.getAllByRole("listitem");
  }

  it("row 5: draws the cap, folds the remainder, and still counts the whole selection", () => {
    open([space({ name: "Tasks", rows: 3 })], [selection("_spaces/tasks.md", TEN)]);

    expect(rows()).toHaveLength(3);
    expect(screen.getByText("10")).toBeInTheDocument();
    expect(
      screen.getByRole("button", { name: `${SESSION_SPACE_ROWS_MORE(7)}: Tasks` }),
    ).toBeInTheDocument();
  });

  it("row 6: the remainder comes back on a press, and folds again", () => {
    open([space({ name: "Tasks", rows: 3 })], [selection("_spaces/tasks.md", TEN)]);

    fireEvent.click(screen.getByRole("button", { name: `${SESSION_SPACE_ROWS_MORE(7)}: Tasks` }));

    expect(rows()).toHaveLength(10);
    const less = screen.getByRole("button", { name: `${SESSION_SPACE_ROWS_LESS}: Tasks` });
    fireEvent.click(less);
    expect(rows()).toHaveLength(3);
  });

  /** Row 7's surface half: Rust warned and dropped the value, so the section
   *  behaves exactly as it did before the key existed — every row, no control. */
  it("row 7: an unreadable cap caps nothing and offers no control", () => {
    open(
      [space({ name: "Tasks", rows: null, warnings: ['keeper can\'t read the row limit "0".'] })],
      [selection("_spaces/tasks.md", TEN)],
    );

    expect(rows()).toHaveLength(10);
    expect(screen.queryByRole("button", { name: /Show \d+ more/ })).toBeNull();
    // A warning and not a fault: the space still works, and the subtitle says so.
    expect(screen.getByText(SESSION_SPACE_SETTINGS_SUBTITLE)).toBeInTheDocument();
  });

  /** A cap nothing reaches is a control that would say "Show 0 more". */
  it("offers no control when the selection already fits the cap", () => {
    open([space({ name: "Tasks", rows: 3 })], [selection("_spaces/tasks.md", TEN.slice(0, 3))]);

    expect(rows()).toHaveLength(3);
    expect(screen.queryByRole("button", { name: /Show/ })).toBeNull();
  });

  /** The cap lives inside the fold, so a folded capped space draws neither its
   *  rows nor the control that would reveal more of them. */
  it("hides the control with the rows when the space is folded", () => {
    open([space({ name: "Tasks", rows: 3, folded: true })], [selection("_spaces/tasks.md", TEN)]);

    expect(screen.queryAllByRole("listitem")).toHaveLength(0);
    expect(screen.queryByRole("button", { name: /Show \d+ more/ })).toBeNull();
    expect(screen.getByText("10")).toBeInTheDocument();
  });

  /** Each section folds on its own: one capped space unfolded must not unfold
   *  the next one's remainder. */
  it("keeps one section's remainder out of another's", () => {
    open(
      [space({ name: "Tasks", rows: 3 }), space({ id: "_spaces/log.md", name: "Log", rows: 2 })],
      [selection("_spaces/tasks.md", TEN), selection("_spaces/log.md", TEN)],
    );

    fireEvent.click(screen.getByRole("button", { name: `${SESSION_SPACE_ROWS_MORE(7)}: Tasks` }));

    expect(screen.getByRole("list", { name: "Tasks" }).children).toHaveLength(10);
    expect(screen.getByRole("list", { name: "Log" }).children).toHaveLength(2);
  });
});

/**
 * Opening the record a space wanted to create (Story 51.7, FR-299, row 3).
 *
 * Wired here because the render is this file's; the payload that decides it is
 * `spaces::create_refused(query, shape).record`, and `session-detail.test.tsx`
 * owns the end-to-end wiring. What is asserted here is narrower and is the only
 * half a render can prove: the flag puts a verb in the create's own slot, and
 * a refusal the record cannot answer puts none there.
 */
describe("SessionSpaces open the record", () => {
  /** Rust's sentence for the one-record refusal, written out as FIXTURE data for
   *  {@link NO_TASK_HOME}'s reason — `shape.rs` owns the wording. */
  const ONE_RECORD =
    "a session has one about record — about.md under the flat contract, README.md under the folder one — and keeper edits it rather than making a second.";

  /**
   * Row 3. Where Rust says the record this space wanted already exists, the verb
   * that applies is opening it — in the create's own slot, and never beside a
   * create, because a query that names the record is refused one by definition.
   *
   * The label is NOT composed here: the record is `about.md` under one contract
   * and `README.md` under the other, and the surface that knows which is the
   * detail's header, which reads `shape` off its own payload.
   */
  it("row 3: offers Open the record where the create was refused because it exists", () => {
    const { onOpenRecord } = open(
      [space({ id: "_spaces/about.md", name: "About", newFileKind: null })],
      [selection("_spaces/about.md", [], null, ONE_RECORD, true)],
    );

    const section = screen.getByRole("region", { name: "About" });
    expect(within(section).queryByRole("button", { name: /^New note in/ })).toBeNull();
    fireEvent.click(within(section).getByRole("button", { name: RECORD_LABEL }));

    expect(onOpenRecord).toHaveBeenCalledTimes(1);
  });

  /** And it is absent where the refusal is a different one: a folder-shaped
   *  session keeps no tasks file, and opening the record is not the answer to
   *  that. */
  it("row 3: offers no record verb for a refusal the record cannot answer", () => {
    open(
      [space({ id: "_spaces/tasks.md", name: "Tasks", newFileKind: "task" })],
      [selection("_spaces/tasks.md", [], null, NO_TASK_HOME, false)],
    );

    const section = screen.getByRole("region", { name: "Tasks" });
    expect(within(section).queryByRole("button", { name: RECORD_LABEL })).toBeNull();
  });
});
