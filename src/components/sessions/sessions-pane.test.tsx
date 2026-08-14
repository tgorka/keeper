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
vi.mock("@/lib/ipc/client", () => ({
  sessionsRoots: () => sessionsRoots(),
  sessionsList: (rootId: unknown) => sessionsList(rootId),
  sessionsRescan: (rootId: unknown) => sessionsRescan(rootId),
  sessionsPatterns: (rootId: unknown) => sessionsPatterns(rootId),
  sessionsCreate: (rootId: unknown, title: unknown, patternId: unknown) =>
    sessionsCreate(rootId, title, patternId),
  revealPath: (path: unknown) => revealPath(path),
  listenSessionsChanged: (cb: unknown) => listenSessionsChanged(cb),
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
  SESSION_PATTERN_LABEL,
  SESSION_PATTERN_SKIPS_LABEL,
} from "@/components/sessions/session-pattern-picker";
import {
  SESSION_ROW_RECORD_TESTID,
  SESSION_ROW_STATUS_TESTID,
  SESSION_ROW_WORKSPACE_TESTID,
} from "@/components/sessions/session-row";
import {
  SESSIONS_LIST_LABEL,
  SESSIONS_NEW_CONFIRM_LABEL,
  SESSIONS_NEW_LABEL,
  SESSIONS_NEW_TITLE_LABEL,
  SESSIONS_NO_MATCH_LABEL,
  SESSIONS_NO_ROOT_TITLE,
  SESSIONS_PANE_TITLE,
  SESSIONS_RESCAN_LABEL,
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
function namedTemplatePattern(): SessionPatternVm {
  return {
    id: "_template/interview",
    kind: "template",
    label: "interview",
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

  it("reads patterns only once the create row is open — a board nobody creates on walks nothing", async () => {
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
