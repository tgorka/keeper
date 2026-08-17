import { beforeEach, describe, expect, it, vi } from "vitest";
import type { SessionRowVm } from "@/lib/ipc/client";

const sessionsLogToday = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  sessionsLogToday: (rootId: unknown, sessionId: unknown) => sessionsLogToday(rootId, sessionId),
  // The hook's module graph reaches the detail's constants, which reach the
  // client's own exports through the components they sit beside. A factory that
  // omitted one would make the IMPORT throw rather than a call.
  sessionsDetail: vi.fn(),
  sessionsTree: vi.fn(),
  sessionsRefs: vi.fn(),
  sessionsSpaces: vi.fn(),
  sessionsSpaceFiles: vi.fn(),
  sessionsSpaceDelete: vi.fn(),
  sessionsSpaceSave: vi.fn(),
  sessionsSpacesRestore: vi.fn(),
  sessionsSpacesFoldedGet: vi.fn(),
  sessionsRecordMigrate: vi.fn(),
  sessionsFileNew: vi.fn(),
  sessionsFileNewKind: vi.fn(),
  sessionsFileDelete: vi.fn(),
  sessionsTaskMove: vi.fn(),
  notesSpaceTerms: vi.fn(),
  notesVaults: vi.fn(async () => []),
  notesVaultActive: vi.fn(async () => null),
  notesVaultSetActive: vi.fn(async () => undefined),
  notesTree: vi.fn(),
  listenSessionsChanged: vi.fn(async () => () => {}),
  syncOpenEntry: vi.fn(),
  revealPath: vi.fn(),
  sessionsRoots: vi.fn(),
}));

import { SESSION_RECORD_NAME } from "@/components/sessions/session-detail";
import { logTodayInCurrentSession } from "@/hooks/use-sessions-shortcut";
import { panelsStore } from "@/lib/stores/panels";
import { resetSessionsListStoreForTest, sessionsListStore } from "@/lib/stores/sessions-list";
import { resetSessionsRootsStoreForTest, sessionsRootsStore } from "@/lib/stores/sessions-roots";

const NOW = Date.now();

function row(over: Partial<SessionRowVm> = {}): SessionRowVm {
  return {
    id: "01J5AAAAAAAAAAAAAAAAAAAAAA",
    path: "active/2026-08-10-keeper",
    title: "keeper — rolling work session",
    status: "active",
    archivedYear: null,
    workspaceMs: null,
    recordMs: NOW - 60_000,
    lastLogDate: "2026-08-10",
    lastLogLine: "opened",
    snippet: "",
    tags: [],
    pinned: false,
    unread: false,
    origin: "keeper",
    headRev: "",
    conflict: false,
    lineage: false,
    ...over,
  };
}

beforeEach(() => {
  vi.clearAllMocks();
  resetSessionsRootsStoreForTest();
  resetSessionsListStoreForTest();
  panelsStore.setState(panelsStore.getInitialState(), true);
  sessionsRootsStore.setState({
    roots: [
      {
        id: "tgdrive",
        name: "tgdrive",
        subfolder: "60-sessions",
        root: "/Volumes/merope/tgdrive/60-sessions",
        indexed: true,
        activeCount: 1,
        unreadCount: 0,
      },
    ],
    activeRootId: "tgdrive",
  });
  sessionsListStore.getState().reset("tgdrive", [row()]);
  sessionsLogToday.mockResolvedValue({
    rootId: "tgdrive",
    id: "01J5AAAAAAAAAAAAAAAAAAAAAA",
    path: "active/2026-08-10-keeper",
    title: "keeper — rolling work session",
  });
});

/**
 * Acceptance row 8 of spec 52.1, the half of it that lives here.
 *
 * ⌘⌥L appends today's entry and then opens the record, and the path it composes
 * was a literal `README.md` with no test on it. Story 52.1 replaced that literal
 * with `SESSION_RECORD_NAME`, which holds the same string — so the behaviour is
 * unchanged and the claim that the story fixed something here was not true. What
 * IS worth defending is that one constant decides the target: the board, the
 * detail and this handler must not come to name three different files, and only a
 * test that reads the target can see it if they do.
 *
 * Correctness for a session written before the story comes from its record having
 * been MOVED — `sessions_record_migrate`, pressed on the detail — and not from
 * anything spelled here.
 */
describe("logTodayInCurrentSession", () => {
  it("logs today and opens the record at the one name, under the root's subfolder", async () => {
    await logTodayInCurrentSession();

    expect(sessionsLogToday).toHaveBeenCalledWith("tgdrive", "01J5AAAAAAAAAAAAAAAAAAAAAA");
    const target = panelsStore.getState().panels.find((p) => p.target?.kind === "file")?.target;
    expect(target).toMatchObject({
      kind: "file",
      profileId: "tgdrive",
      relativePath: `60-sessions/active/2026-08-10-keeper/${SESSION_RECORD_NAME}`,
    });
    expect(SESSION_RECORD_NAME).toBe("README.md");
  });

  it("does nothing at all with no active root", async () => {
    resetSessionsRootsStoreForTest();
    await logTodayInCurrentSession();
    expect(sessionsLogToday).not.toHaveBeenCalled();
    expect(panelsStore.getState().panels.find((p) => p.target?.kind === "file")).toBeUndefined();
  });
});
