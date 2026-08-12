import { render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionRootVm, SessionRowVm } from "@/lib/ipc/client";

// Mock the typed IPC client so the pane never touches Tauri.
const sessionsRoots = vi.fn();
const sessionsList = vi.fn();
const sessionsRescan = vi.fn();
const revealPath = vi.fn();
const listenSessionsChanged = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  sessionsRoots: () => sessionsRoots(),
  sessionsList: (rootId: unknown) => sessionsList(rootId),
  sessionsRescan: (rootId: unknown) => sessionsRescan(rootId),
  revealPath: (path: unknown) => revealPath(path),
  listenSessionsChanged: (cb: unknown) => listenSessionsChanged(cb),
}));

import {
  SESSION_ROW_RECORD_TESTID,
  SESSION_ROW_STATUS_TESTID,
  SESSION_ROW_WORKSPACE_TESTID,
} from "@/components/sessions/session-row";
import {
  SESSIONS_LIST_LABEL,
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

beforeEach(() => {
  resetSessionsRootsStoreForTest();
  resetSessionsListStoreForTest();
  sessionsRoots.mockResolvedValue([root()]);
  sessionsList.mockResolvedValue([row()]);
  sessionsRescan.mockResolvedValue(undefined);
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
