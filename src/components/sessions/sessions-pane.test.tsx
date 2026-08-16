import { fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionPatternVm, SessionRootVm, SessionRowVm } from "@/lib/ipc/client";

// Mock the typed IPC client so the pane never touches Tauri.
const sessionsRoots = vi.fn();
const sessionsList = vi.fn();
const sessionsRescan = vi.fn();
const sessionsPatterns = vi.fn();
const sessionsCreate = vi.fn();
const revealPath = vi.fn();
const listenSessionsChanged = vi.fn();
const sessionsTemplateInstall = vi.fn();
const sessionsTemplateEntries = vi.fn();
const sessionsTemplateRename = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  sessionsRoots: () => sessionsRoots(),
  sessionsList: (rootId: unknown) => sessionsList(rootId),
  sessionsRescan: (rootId: unknown) => sessionsRescan(rootId),
  sessionsPatterns: (rootId: unknown) => sessionsPatterns(rootId),
  sessionsCreate: (rootId: unknown, title: unknown, patternId: unknown) =>
    sessionsCreate(rootId, title, patternId),
  revealPath: (path: unknown) => revealPath(path),
  listenSessionsChanged: (cb: unknown) => listenSessionsChanged(cb),
  sessionsTemplateInstall: (rootId: unknown, name: unknown) =>
    sessionsTemplateInstall(rootId, name),
  sessionsTemplateEntries: (rootId: unknown, name: unknown) =>
    sessionsTemplateEntries(rootId, name),
  sessionsTemplateRename: (rootId: unknown, name: unknown, newName: unknown) =>
    sessionsTemplateRename(rootId, name, newName),
  sessionsSetPinned: vi.fn(async () => {}),
  sessionsLogToday: vi.fn(async () => {}),
  sessionsArchive: vi.fn(async () => {}),
  sessionsUnarchive: vi.fn(async () => {}),
  sessionsDelete: vi.fn(async () => {}),
}));

import {
  SESSION_ACTIONS_LABEL,
  SESSION_NEW_LIKE_THIS_LABEL,
} from "@/components/sessions/session-actions";
import {
  SESSION_PATTERN_INSTALL_LABEL,
  SESSION_PATTERN_LABEL,
  SESSION_PATTERN_SKIPS_LABEL,
} from "@/components/sessions/session-pattern-picker";
import {
  SESSION_ROW_RECORD_TESTID,
  SESSION_ROW_STATUS_TESTID,
  SESSION_ROW_WORKSPACE_TESTID,
} from "@/components/sessions/session-row";
import {
  SESSION_TEMPLATE_RENAME,
  SESSION_TEMPLATES_EMPTY,
  SESSION_TEMPLATES_HEADING,
  SESSION_TEMPLATES_NEW,
  SESSION_TEMPLATES_NEW_NAME_LABEL,
  SESSION_TEMPLATES_READING,
} from "@/components/sessions/session-templates";
import {
  SESSIONS_LIST_LABEL,
  SESSIONS_NEW_CONFIRM_LABEL,
  SESSIONS_NEW_LABEL,
  SESSIONS_NEW_TITLE_LABEL,
  SESSIONS_NO_MATCH_LABEL,
  SESSIONS_NO_ROOT_TITLE,
  SESSIONS_PANE_TITLE,
  SESSIONS_RESCAN_LABEL,
  SESSIONS_TEMPLATES_LABEL,
  SessionsPane,
} from "@/components/sessions/sessions-pane";
import { resetSessionsListStoreForTest, sessionsListStore } from "@/lib/stores/sessions-list";
import { resetSessionsRootsStoreForTest } from "@/lib/stores/sessions-roots";

const NOW = Date.now();

function root(over: Partial<SessionRootVm> = {}): SessionRootVm {
  return {
    id: "tgdrive",
    name: "tgdrive",
    subfolder: "60-sessions",
    root: "/Volumes/merope/tgdrive/60-sessions",
    indexed: true,
    activeCount: 1,
    unreadCount: 0,
    ...over,
  };
}

function row(over: Partial<SessionRowVm> = {}): SessionRowVm {
  return {
    id: "01J5AAAAAAAAAAAAAAAAAAAAAA",
    path: "active/2026-08-10-keeper",
    title: "keeper — rolling work session",
    status: "active",
    archivedYear: null,
    workspaceMs: NOW - 2 * 60_000,
    recordMs: NOW - 60 * 60_000,
    lastLogDate: "2026-08-11",
    lastLogLine: "shipped 0.6.5",
    snippet: "State as of opening.",
    tags: ["project/keeper"],
    pinned: false,
    unread: false,
    origin: "",
    headRev: "",
    conflict: false,
    lineage: false,
    ...over,
  };
}

function templatePattern(): SessionPatternVm {
  return {
    id: "_template",
    kind: "template",
    label: "Zone template",
    detail: "the zone's own skeleton — copied whole",
    mtimeMs: null,
    copies: [{ relPath: "prompts/00-brief.md", isDir: false }],
    skips: [],
  };
}

/** A `_template/<name>/` (FR-266) — a template, addressed by its path. */
function namedTemplatePattern(name = "interview"): SessionPatternVm {
  return {
    id: `_template/${name}`,
    kind: "template",
    label: name,
    detail: "a named template — copied whole",
    mtimeMs: NOW - 3 * 24 * 60 * 60_000,
    copies: [{ relPath: "questions.md", isDir: false }],
    skips: [],
  };
}

function sessionPattern(): SessionPatternVm {
  return {
    id: "01J5AAAAAAAAAAAAAAAAAAAAAA",
    kind: "session",
    label: "keeper — rolling work session",
    detail: "continues this session",
    mtimeMs: NOW - 60 * 60_000,
    copies: [{ relPath: "prompts/01-scope.md", isDir: false }],
    skips: [
      {
        relPath: "artifacts/report.md",
        reason: "artifacts stay with the session that produced them",
      },
    ],
  };
}

beforeEach(() => {
  resetSessionsRootsStoreForTest();
  resetSessionsListStoreForTest();
  sessionsRoots.mockResolvedValue([root()]);
  sessionsList.mockResolvedValue([row()]);
  sessionsRescan.mockResolvedValue(undefined);
  sessionsPatterns.mockResolvedValue([templatePattern(), sessionPattern()]);
  sessionsCreate.mockResolvedValue({
    rootId: "tgdrive",
    id: "01J5NEWNEWNEWNEWNEWNEWNEWN",
    path: "active/2026-08-12-next",
    title: "next",
  });
  revealPath.mockResolvedValue(undefined);
  listenSessionsChanged.mockResolvedValue(() => {});
  sessionsTemplateInstall.mockResolvedValue("_template/interview");
  sessionsTemplateEntries.mockResolvedValue([
    { subpath: "60-sessions/_template/AGENTS.md", name: "AGENTS.md", mtimeMs: NOW - 60_000 },
  ]);
  sessionsTemplateRename.mockResolvedValue("_template/kick-off");
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("SessionsPane", () => {
  it("reads roots and rows on mount and shows the board", async () => {
    render(<SessionsPane />);
    expect(screen.getByRole("heading", { name: SESSIONS_PANE_TITLE })).toBeInTheDocument();
    const list = await screen.findByRole("list", { name: SESSIONS_LIST_LABEL });
    expect(within(list).getByText("keeper — rolling work session")).toBeInTheDocument();
    // The subtitle is the last log line, dated (UX-DR85).
    expect(within(list).getByText(/shipped 0\.6\.5/)).toBeInTheDocument();
    // The two freshness signals render separately (UX-DR86) — never one dot.
    expect(within(list).getByTestId(SESSION_ROW_WORKSPACE_TESTID)).toBeInTheDocument();
    expect(within(list).getByTestId(SESSION_ROW_RECORD_TESTID)).toBeInTheDocument();
    expect(within(list).getByTestId(SESSION_ROW_STATUS_TESTID)).toBeInTheDocument();
  });

  it("says 'no sessions folder yet' when no root is flagged — capability on, surface honest", async () => {
    sessionsRoots.mockResolvedValue([]);
    render(<SessionsPane />);
    expect(await screen.findByText(SESSIONS_NO_ROOT_TITLE)).toBeInTheDocument();
    // With no root there is nothing to rescan and no filter row to show.
    expect(screen.queryByRole("button", { name: SESSIONS_RESCAN_LABEL })).not.toBeInTheDocument();
  });

  it("distinguishes 'nothing matches this filter' from an empty zone", async () => {
    render(<SessionsPane />);
    await screen.findByRole("list", { name: SESSIONS_LIST_LABEL });
    sessionsListStore.getState().setText("no-such-fragment");
    expect(await screen.findByText(SESSIONS_NO_MATCH_LABEL)).toBeInTheDocument();
  });

  it("rescan asks Rust and trusts the changed event for the answer", async () => {
    render(<SessionsPane />);
    const button = await screen.findByRole("button", { name: SESSIONS_RESCAN_LABEL });
    button.click();
    await waitFor(() => expect(sessionsRescan).toHaveBeenCalledWith("tgdrive"));
    // No local mutation: the rows mirror is untouched until the event lands.
    expect(sessionsList).toHaveBeenCalledTimes(1);
  });

  it("shows the root switcher only when two or more roots exist", async () => {
    render(<SessionsPane />);
    await screen.findByRole("list", { name: SESSIONS_LIST_LABEL });
    expect(screen.queryByRole("button", { name: /^tgdrive/ })).not.toBeInTheDocument();
  });

  it("creating asks for the title and the pattern on ONE row, template chosen", async () => {
    render(<SessionsPane />);
    (await screen.findByRole("button", { name: SESSIONS_NEW_LABEL })).click();

    const title = await screen.findByLabelText(SESSIONS_NEW_TITLE_LABEL);
    const picker = await screen.findByRole("combobox", { name: SESSION_PATTERN_LABEL });
    // The zone's own answer is pre-chosen — the picker is a change, not a step.
    expect(picker).toHaveTextContent("Zone template");

    fireEvent.change(title, { target: { value: "  next thing  " } });
    (await screen.findByRole("button", { name: SESSIONS_NEW_CONFIRM_LABEL })).click();
    await waitFor(() =>
      expect(sessionsCreate).toHaveBeenCalledWith("tgdrive", "next thing", "_template"),
    );
  });

  it("creates from a named template, sending the path it copies out of", async () => {
    sessionsPatterns.mockResolvedValue([
      templatePattern(),
      namedTemplatePattern(),
      sessionPattern(),
    ]);
    render(<SessionsPane />);
    (await screen.findByRole("button", { name: SESSIONS_NEW_LABEL })).click();

    const picker = await screen.findByRole("combobox", { name: SESSION_PATTERN_LABEL });
    fireEvent.keyDown(picker, { key: "Enter" });
    fireEvent.click(await screen.findByRole("option", { name: /interview/ }));

    fireEvent.change(await screen.findByLabelText(SESSIONS_NEW_TITLE_LABEL), {
      target: { value: "candidate screen" },
    });
    (await screen.findByRole("button", { name: SESSIONS_NEW_CONFIRM_LABEL })).click();
    // `_template/interview` reaches Rust intact. Before FR-266 this id fell
    // past the shell's `!= "_template"` test and was looked up as a session,
    // which failed with "no such session: _template/interview".
    await waitFor(() =>
      expect(sessionsCreate).toHaveBeenCalledWith(
        "tgdrive",
        "candidate screen",
        "_template/interview",
      ),
    );
  });

  it("refuses to create without a title, and never calls Rust to find that out", async () => {
    render(<SessionsPane />);
    (await screen.findByRole("button", { name: SESSIONS_NEW_LABEL })).click();
    (await screen.findByRole("button", { name: SESSIONS_NEW_CONFIRM_LABEL })).click();
    expect(sessionsCreate).not.toHaveBeenCalled();
  });

  it("'New like this' opens the SAME create row with that session chosen", async () => {
    render(<SessionsPane />);
    const list = await screen.findByRole("list", { name: SESSIONS_LIST_LABEL });
    // Radix opens on pointer events, not click (the note-actions precedent).
    const trigger = within(list).getByRole("button", { name: SESSION_ACTIONS_LABEL });
    fireEvent.pointerDown(trigger, { button: 0, ctrlKey: false });
    fireEvent.pointerUp(trigger, { button: 0 });
    const menu = await screen.findByRole("menu");
    fireEvent.click(within(menu).getByRole("menuitem", { name: SESSION_NEW_LIKE_THIS_LABEL }));

    // One door: the title is still asked, and the consequence is still shown.
    const picker = await screen.findByRole("combobox", { name: SESSION_PATTERN_LABEL });
    expect(picker).toHaveTextContent("keeper — rolling work session");
    const skips = await screen.findByRole("list", { name: SESSION_PATTERN_SKIPS_LABEL });
    expect(
      within(skips).getByText(/artifacts stay with the session that produced them/),
    ).toBeInTheDocument();

    fireEvent.change(await screen.findByLabelText(SESSIONS_NEW_TITLE_LABEL), {
      target: { value: "round two" },
    });
    (await screen.findByRole("button", { name: SESSIONS_NEW_CONFIRM_LABEL })).click();
    await waitFor(() =>
      expect(sessionsCreate).toHaveBeenCalledWith(
        "tgdrive",
        "round two",
        "01J5AAAAAAAAAAAAAAAAAAAAAA",
      ),
    );
  });

  it("reads patterns only once a surface needs them — a board at rest walks nothing", async () => {
    render(<SessionsPane />);
    await screen.findByRole("list", { name: SESSIONS_LIST_LABEL });
    expect(sessionsPatterns).not.toHaveBeenCalled();
    (await screen.findByRole("button", { name: SESSIONS_NEW_LABEL })).click();
    await waitFor(() => expect(sessionsPatterns).toHaveBeenCalledWith("tgdrive"));
  });

  it("switching roots re-reads rows scoped to the new root", async () => {
    sessionsRoots.mockResolvedValue([root(), root({ id: "neuradrive", name: "neuradrive" })]);
    render(<SessionsPane />);
    const switcher = await screen.findByRole("button", { name: /neuradrive/ });
    sessionsList.mockResolvedValue([row({ id: "01J5DDDDDDDDDDDDDDDDDDDDDD", title: "neura" })]);
    switcher.click();
    await waitFor(() => expect(sessionsList).toHaveBeenLastCalledWith("neuradrive"));
    const list = await screen.findByRole("list", { name: SESSIONS_LIST_LABEL });
    await within(list).findByText("neura");
  });
});

/**
 * The Templates room (FR-269): a chip that reads as a peer of the status chips
 * and switches what the board is showing, rather than a fourth filter value over
 * rows that have no status.
 */
describe("SessionsPane templates room", () => {
  async function enterTemplates() {
    render(<SessionsPane />);
    await screen.findByRole("list", { name: SESSIONS_LIST_LABEL });
    fireEvent.click(screen.getByRole("button", { name: SESSIONS_TEMPLATES_LABEL }));
    return await screen.findByRole("region", { name: SESSION_TEMPLATES_HEADING });
  }

  it("shows the templates and stands the row-only controls down", async () => {
    const room = await enterTemplates();

    // The zone's templates, with what Rust says is inside them.
    expect(await within(room).findByRole("heading", { name: "Zone template" })).toBeInTheDocument();
    expect(await within(room).findByRole("button", { name: /AGENTS\.md/ })).toBeInTheDocument();
    // The session rows are not what is showing.
    expect(screen.queryByRole("list", { name: SESSIONS_LIST_LABEL })).not.toBeInTheDocument();
    // Search, Pinned and Unread filter session rows; here they would be inert
    // chrome that looks like it does something.
    expect(screen.queryByLabelText("Search sessions")).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Pinned" })).not.toBeInTheDocument();
    expect(screen.queryByRole("button", { name: "Unread" })).not.toBeInTheDocument();
    // The chips themselves stay: they are how you get back.
    expect(screen.getByRole("button", { name: "Active" })).toBeInTheDocument();
  });

  it("comes back to the rows when a status chip is pressed", async () => {
    await enterTemplates();

    fireEvent.click(screen.getByRole("button", { name: "Active" }));

    expect(await screen.findByRole("list", { name: SESSIONS_LIST_LABEL })).toBeInTheDocument();
    expect(screen.getByLabelText("Search sessions")).toBeInTheDocument();
    expect(
      screen.queryByRole("region", { name: SESSION_TEMPLATES_HEADING }),
    ).not.toBeInTheDocument();
  });

  it("New template sends the name Rust has always accepted, and re-reads after", async () => {
    await enterTemplates();
    const reads = sessionsPatterns.mock.calls.length;

    fireEvent.change(screen.getByLabelText(SESSION_TEMPLATES_NEW_NAME_LABEL), {
      target: { value: "interview" },
    });
    fireEvent.click(screen.getByRole("button", { name: SESSION_TEMPLATES_NEW }));

    // The name argument, present at last: `sessions_template_install` has taken
    // it since it was written and the one caller dropped it.
    await waitFor(() =>
      expect(sessionsTemplateInstall).toHaveBeenCalledWith("tgdrive", "interview"),
    );
    // A create makes a pattern without making a session, so the nonce is the
    // only signal that re-reads the list — the same one a rename bumps.
    await waitFor(() => expect(sessionsPatterns.mock.calls.length).toBeGreaterThan(reads));
  });

  it("keeps the zone-template install reachable from a zone with no template", async () => {
    sessionsPatterns.mockResolvedValue([sessionPattern()]);
    await enterTemplates();

    expect(await screen.findByText(SESSION_TEMPLATES_EMPTY)).toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: SESSION_PATTERN_INSTALL_LABEL }));

    // No name: the zone's own `_template/`, which is what the create row's offer
    // writes too.
    await waitFor(() => expect(sessionsTemplateInstall).toHaveBeenCalledWith("tgdrive", undefined));
  });

  it("re-reads for the new root and shows nothing of the old one while it waits", async () => {
    sessionsRoots.mockResolvedValue([root(), root({ id: "neuradrive", name: "neuradrive" })]);
    // A DIFFERENT list per root, and the new root's read held open. Mocking one
    // list for both roots made the stale-name case this test is named after
    // impossible to observe: root A's headings ARE root B's headings.
    let arrive: (list: SessionPatternVm[]) => void = () => {};
    sessionsPatterns.mockImplementation((rootId: unknown) =>
      rootId === "neuradrive"
        ? new Promise<SessionPatternVm[]>((resolve) => {
            arrive = resolve;
          })
        : Promise.resolve([templatePattern(), namedTemplatePattern(), sessionPattern()]),
    );
    const room = await enterTemplates();
    expect(await within(room).findByRole("heading", { name: "interview" })).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: /neuradrive/ }));
    await waitFor(() => expect(sessionsPatterns).toHaveBeenLastCalledWith("neuradrive"));

    // Mid-flight, and this is the whole point: the previous zone's HEADING is
    // gone, not merely a file row swapped underneath it. An untagged pattern list
    // left root A's sections standing for the whole round trip — one directory
    // walk per pattern — with LIVE Rename buttons, each addressing root B's id
    // with root A's folder name.
    expect(screen.queryByRole("heading", { name: "interview" })).not.toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: `${SESSION_TEMPLATE_RENAME} interview` }),
    ).not.toBeInTheDocument();
    expect(screen.getByText(SESSION_TEMPLATES_READING)).toBeInTheDocument();
    // And no entries read went out under the new root with the old root's names.
    expect(sessionsTemplateEntries).not.toHaveBeenCalledWith("neuradrive", "interview");

    arrive([namedTemplatePattern("handbook")]);
    expect(await screen.findByRole("heading", { name: "handbook" })).toBeInTheDocument();
    await waitFor(() =>
      expect(sessionsTemplateEntries).toHaveBeenLastCalledWith("neuradrive", "handbook"),
    );
  });

  it("says a refused pattern read out loud instead of waiting on it forever", async () => {
    const said = "no such sessions root: tgdrive";
    sessionsPatterns.mockRejectedValue({ message: said });
    render(<SessionsPane />);
    await screen.findByRole("list", { name: SESSIONS_LIST_LABEL });

    fireEvent.click(screen.getByRole("button", { name: SESSIONS_TEMPLATES_LABEL }));

    expect(await screen.findByRole("alert")).toHaveTextContent(said);
    // Not "Reading templates…" over a read that already stopped, and not the
    // room's empty state either: keeper has no list, and said so.
    expect(screen.queryByText(SESSION_TEMPLATES_READING)).not.toBeInTheDocument();
    expect(
      screen.queryByRole("region", { name: SESSION_TEMPLATES_HEADING }),
    ).not.toBeInTheDocument();

    // And there is a way out of it: leaving the room and coming back re-runs the
    // read, which is what a refusal with no catch left the operator without.
    sessionsPatterns.mockResolvedValue([templatePattern()]);
    fireEvent.click(screen.getByRole("button", { name: SESSIONS_TEMPLATES_LABEL }));
    fireEvent.click(screen.getByRole("button", { name: SESSIONS_TEMPLATES_LABEL }));

    expect(await screen.findByRole("heading", { name: "Zone template" })).toBeInTheDocument();
    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
  });

  it("the Templates chip announces its state and a second press comes back", async () => {
    render(<SessionsPane />);
    await screen.findByRole("list", { name: SESSIONS_LIST_LABEL });
    const chip = () => screen.getByRole("button", { name: SESSIONS_TEMPLATES_LABEL });
    // A peer of Pinned and Unread, which both announce and both invert.
    expect(chip()).toHaveAttribute("aria-pressed", "false");

    fireEvent.click(chip());
    await screen.findByRole("region", { name: SESSION_TEMPLATES_HEADING });
    expect(chip()).toHaveAttribute("aria-pressed", "true");

    // The way back out of the room, from the control that opened it — not only
    // from a status chip, which was a rule stated in a comment and nowhere here.
    fireEvent.click(chip());
    expect(await screen.findByRole("list", { name: SESSIONS_LIST_LABEL })).toBeInTheDocument();
    expect(
      screen.queryByRole("region", { name: SESSION_TEMPLATES_HEADING }),
    ).not.toBeInTheDocument();
    expect(chip()).toHaveAttribute("aria-pressed", "false");
  });
});
