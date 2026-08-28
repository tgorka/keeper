import { beforeEach, describe, expect, it } from "vitest";
import type { SessionRowVm } from "@/lib/ipc/client";
import {
  filterRows,
  isStale,
  resetSessionsListStoreForTest,
  SESSIONS_STALE_DAYS,
  sessionsListStore,
} from "@/lib/stores/sessions-list";

const NOW = Date.UTC(2026, 7, 12, 12, 0, 0);
const DAY = 24 * 60 * 60 * 1000;

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
  resetSessionsListStoreForTest();
});

describe("isStale", () => {
  it("marks an active session stale only past the threshold, on the NEWEST signal", () => {
    // Workspace hot, record old: not stale — the agent is iterating.
    expect(isStale(row({ workspaceMs: NOW - DAY, recordMs: NOW - 30 * DAY }), NOW)).toBe(false);
    // Both signals past the threshold: stale.
    expect(
      isStale(
        row({
          workspaceMs: NOW - (SESSIONS_STALE_DAYS + 1) * DAY,
          recordMs: NOW - (SESSIONS_STALE_DAYS + 2) * DAY,
        }),
        NOW,
      ),
    ).toBe(true);
  });

  it("never calls an archived session stale — finished work cannot go stale", () => {
    expect(
      isStale(row({ status: "archived", workspaceMs: null, recordMs: NOW - 400 * DAY }), NOW),
    ).toBe(false);
  });

  it("a session with no signals at all is not stale — absence is not age", () => {
    expect(isStale(row({ workspaceMs: null, recordMs: null }), NOW)).toBe(false);
  });
});

describe("filterRows", () => {
  const rows = [
    row(),
    row({
      id: "01J5BBBBBBBBBBBBBBBBBBBBBB",
      path: "archive/2025/2025-03-01-taxes",
      title: "taxes",
      status: "archived",
      archivedYear: 2025,
      tags: ["records/finance"],
      pinned: true,
      lastLogLine: "",
      snippet: "yearly filing",
    }),
    row({
      id: "01J5CCCCCCCCCCCCCCCCCCCCCC",
      title: "neura pitch",
      unread: true,
      tags: [],
      lastLogLine: "deck v3 drafted",
      snippet: "",
    }),
  ];

  const base = { text: "", status: "all" as const, pinnedOnly: false, unreadOnly: false };

  it("status, pinned and unread chips narrow independently", () => {
    expect(filterRows(rows, { ...base, status: "archived" }).map((r) => r.title)).toEqual([
      "taxes",
    ]);
    expect(filterRows(rows, { ...base, pinnedOnly: true }).map((r) => r.title)).toEqual(["taxes"]);
    expect(filterRows(rows, { ...base, unreadOnly: true }).map((r) => r.title)).toEqual([
      "neura pitch",
    ]);
  });

  it("text sweeps title, path, tags, snippet and the last log line — the half-remembered-fragment surfaces", () => {
    expect(filterRows(rows, { ...base, text: "0.6.5" })).toHaveLength(1);
    expect(filterRows(rows, { ...base, text: "finance" }).map((r) => r.title)).toEqual(["taxes"]);
    expect(filterRows(rows, { ...base, text: "2025-03" }).map((r) => r.title)).toEqual(["taxes"]);
    expect(filterRows(rows, { ...base, text: "FILING" }).map((r) => r.title)).toEqual(["taxes"]);
    expect(filterRows(rows, { ...base, text: "nothing-here" })).toHaveLength(0);
  });
});

describe("sessionsListStore", () => {
  it("reset stamps the owning root so a late read cannot paint the wrong board", () => {
    sessionsListStore.getState().reset("tgdrive", [row()]);
    expect(sessionsListStore.getState().rowsRootId).toBe("tgdrive");
    expect(sessionsListStore.getState().rows).toHaveLength(1);
    sessionsListStore.getState().clear();
    expect(sessionsListStore.getState().rows).toBeNull();
    expect(sessionsListStore.getState().rowsRootId).toBeNull();
  });
});
