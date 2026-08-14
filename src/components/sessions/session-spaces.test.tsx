import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionSpaceFilesVm, SessionSpaceVm } from "@/lib/ipc/client";

// The section writes through two commands; the editor it opens reaches for two
// more. All four are stubbed at the IPC boundary so the real components — the
// real refusals, the real copy — are what these tests exercise.
vi.mock("@/lib/ipc/client", () => ({
  sessionsSpaceDelete: vi.fn(),
  sessionsSpacesRestore: vi.fn(),
  sessionsSpaceSave: vi.fn(),
  notesSpaceTerms: vi.fn(),
}));

import {
  SESSION_SPACE_BROKEN_SUBTITLE,
  SESSION_SPACE_DELETE,
  SESSION_SPACE_DELETE_CONFIRM,
  SESSION_SPACE_DELETE_FAILED,
  SESSION_SPACE_EDIT,
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
import { notesSpaceTerms, sessionsSpaceDelete, sessionsSpacesRestore } from "@/lib/ipc/client";
import { activePanel, panelsStore, resetPanelsStoreForTest } from "@/lib/stores/panels";

const mockDelete = vi.mocked(sessionsSpaceDelete);
const mockRestore = vi.mocked(sessionsSpacesRestore);
const mockTerms = vi.mocked(notesSpaceTerms);

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

function open(
  spaces: SessionSpaceVm[],
  selections: SessionSpaceFilesVm[] | null,
): { onChanged: ReturnType<typeof vi.fn> } {
  const onChanged = vi.fn();
  render(
    <SessionSpaces rootId="p1" spaces={spaces} selections={selections} onChanged={onChanged} />,
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
   * Opens through the ONE file target the tree and the Files pane use (AD-109),
   * on the `subpath` Rust composed (AD-65) — a second path-join in TypeScript is
   * a second answer to where a file lives.
   */
  it("opens a file on the path Rust composed, not one it joined itself", () => {
    open([space()], [selection("_spaces/tasks.md", ["task-migrate.md"])]);

    fireEvent.click(screen.getByRole("button", { name: /task-migrate/ }));

    expect(opened()).toEqual({
      kind: "file",
      profileId: "p1",
      relativePath: "60-sessions/active/s/task-migrate.md",
    });
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
