import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  syncProfiles: vi.fn(),
  syncStatuses: vi.fn(),
  syncProfileSave: vi.fn(),
  syncProfileRemove: vi.fn(),
  syncProfileSetEnabled: vi.fn(),
  syncFolderNow: vi.fn(),
  syncVerify: vi.fn(),
}));

import type { SyncOutcomeVm, SyncProfileVm, SyncStatusVm } from "@/lib/ipc/client";
import {
  syncFolderNow,
  syncProfileRemove,
  syncProfileSave,
  syncProfileSetEnabled,
  syncProfiles,
  syncStatuses,
  syncVerify,
} from "@/lib/ipc/client";
import {
  ensureSyncHydrated,
  isSyncStatusActive,
  removeSyncProfile,
  resetSyncStoreForTest,
  SYNC_UNKNOWN_ERROR,
  saveSyncProfile,
  setSyncProfileEnabled,
  startSyncStatusPolling,
  syncErrorMessage,
  syncProfileNow,
  syncProgressFraction,
  syncStore,
  verifySyncProfile,
} from "@/lib/stores/sync";

const mockProfiles = vi.mocked(syncProfiles);
const mockStatuses = vi.mocked(syncStatuses);
const mockSave = vi.mocked(syncProfileSave);
const mockRemove = vi.mocked(syncProfileRemove);
const mockSetEnabled = vi.mocked(syncProfileSetEnabled);
const mockFolderNow = vi.mocked(syncFolderNow);
const mockVerify = vi.mocked(syncVerify);

function profileVm(over: Partial<SyncProfileVm> = {}): SyncProfileVm {
  return {
    id: "p1",
    name: "tgdrive",
    localPath: "/Users/alice/tgdrive",
    remoteUrl: "git@github.com:alice/tgdrive.git",
    branch: "main",
    direction: "bidirectional",
    lane: "main",
    subpaths: [],
    excludes: [],
    removable: false,
    lfsMode: "materialize",
    lfsThresholdBytes: 4 * 1024 * 1024,
    settleMs: null,
    effectiveSettleMs: 5_000,
    pollIntervalMs: null,
    effectivePollIntervalMs: 15_000,
    tags: [],
    commitSubjectTemplate: "",
    notes: false,
    notesSubfolder: null,
    recordings: false,
    // Resolved by Rust even for a folder that holds no recordings: it is the
    // subfolder flagging it would use (Story 41.7).
    recordingsSubfolder: "recordings",
    sessions: false,
    sessionsSubfolder: "60-sessions",
    authorOverride: null,
    enabled: true,
    ...over,
  };
}

function statusVm(over: Partial<SyncStatusVm> = {}): SyncStatusVm {
  return {
    profileId: "p1",
    profileName: "tgdrive",
    state: "idle",
    phase: "idle",
    queuedFiles: 0,
    queuedBytes: 0,
    line: "tgdrive — up to date",
    filesDone: 0,
    filesTotal: null,
    bytesDone: 0,
    bytesTotal: null,
    pending: 0,
    settling: 0,
    warning: null,
    error: null,
    lastSyncMs: null,
    needsAttention: false,
    ...over,
  };
}

beforeEach(() => {
  resetSyncStoreForTest();
  mockProfiles.mockResolvedValue([profileVm()]);
  mockStatuses.mockResolvedValue([statusVm()]);
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("ensureSyncHydrated", () => {
  it("loads profiles and statuses once, deduping concurrent first callers", async () => {
    await Promise.all([ensureSyncHydrated(), ensureSyncHydrated(), ensureSyncHydrated()]);

    expect(mockProfiles).toHaveBeenCalledTimes(1);
    expect(mockStatuses).toHaveBeenCalledTimes(1);
    const state = syncStore.getState();
    expect(state.hydrated).toBe(true);
    expect(state.profiles).toEqual([profileVm()]);
    expect(state.statuses.p1.line).toBe("tgdrive — up to date");
  });

  it("does not re-read once hydrated", async () => {
    await ensureSyncHydrated();
    await ensureSyncHydrated();

    expect(mockProfiles).toHaveBeenCalledTimes(1);
  });

  it("leaves the mirror unhydrated on failure, records the message, and retries later", async () => {
    mockProfiles.mockRejectedValueOnce({ code: "unsupported", message: "git is not available" });

    await ensureSyncHydrated();

    expect(syncStore.getState().hydrated).toBe(false);
    // `null` (unknown), never `[]` — an empty list would be a fake value.
    expect(syncStore.getState().profiles).toBeNull();
    expect(syncStore.getState().error).toBe("git is not available");

    // The shared promise was released, so the next call actually retries.
    await ensureSyncHydrated();

    expect(mockProfiles).toHaveBeenCalledTimes(2);
    expect(syncStore.getState().hydrated).toBe(true);
    expect(syncStore.getState().error).toBeNull();
  });
});

describe("status merging", () => {
  it("merges by profileId, keeping rows a partial read did not mention", async () => {
    mockProfiles.mockResolvedValue([profileVm(), profileVm({ id: "p2", name: "notes" })]);
    mockStatuses.mockResolvedValue([
      statusVm(),
      statusVm({ profileId: "p2", line: "notes — idle" }),
    ]);
    await ensureSyncHydrated();

    mockStatuses.mockResolvedValue([statusVm({ line: "tgdrive — 3 waiting to sync" })]);
    await syncProfileNow("p1");

    const { statuses } = syncStore.getState();
    expect(statuses.p1.line).toBe("tgdrive — 3 waiting to sync");
    expect(statuses.p2.line).toBe("notes — idle");
  });

  it("drops a status whose profile is gone, so a removed folder leaves no ghost", async () => {
    mockProfiles.mockResolvedValue([profileVm(), profileVm({ id: "p2", name: "notes" })]);
    mockStatuses.mockResolvedValue([
      statusVm(),
      statusVm({ profileId: "p2", line: "notes — idle" }),
    ]);
    await ensureSyncHydrated();
    expect(Object.keys(syncStore.getState().statuses)).toEqual(["p1", "p2"]);

    mockProfiles.mockResolvedValue([profileVm()]);
    mockStatuses.mockResolvedValue([statusVm()]);
    await removeSyncProfile("p2");

    expect(mockRemove).toHaveBeenCalledWith("p2");
    expect(Object.keys(syncStore.getState().statuses)).toEqual(["p1"]);
  });
});

describe("actions", () => {
  it("save resolves the stored profile and re-reads the mirror", async () => {
    await ensureSyncHydrated();
    const saved = profileVm({ id: "p2", name: "notes" });
    mockSave.mockResolvedValue(saved);
    mockProfiles.mockResolvedValue([profileVm(), saved]);

    const result = await saveSyncProfile({
      id: null,
      name: "notes",
      localPath: "/Users/alice/notes",
      remoteUrl: "git@github.com:alice/notes.git",
      branch: "main",
      direction: "bidirectional",
      lane: "main",
      subpaths: [],
      excludes: [],
      removable: false,
      lfsMode: "materialize",
      lfsThresholdBytes: null,
      settleMs: null,
      pollIntervalMs: null,
      tags: [],
      authorOverride: null,
      commitSubjectTemplate: null,
      notes: false,
      notesSubfolder: null,
      recordings: false,
      recordingsSubfolder: null,
      sessions: false,
      sessionsSubfolder: null,
    });

    expect(result).toEqual(saved);
    expect(syncStore.getState().profiles).toHaveLength(2);
  });

  it("save rejects to its caller so the form can keep what was typed", async () => {
    await ensureSyncHydrated();
    mockSave.mockRejectedValue({ code: "internal", message: "local path must be absolute" });

    await expect(
      saveSyncProfile({
        id: null,
        name: "notes",
        localPath: "relative/path",
        remoteUrl: "git@github.com:alice/notes.git",
        branch: "main",
        direction: "bidirectional",
        lane: "main",
        subpaths: [],
        excludes: [],
        removable: false,
        lfsMode: "materialize",
        lfsThresholdBytes: null,
        settleMs: null,
        pollIntervalMs: null,
        tags: [],
        authorOverride: null,
        commitSubjectTemplate: null,
        notes: false,
        notesSubfolder: null,
        recordings: false,
        recordingsSubfolder: null,
        sessions: false,
        sessionsSubfolder: null,
      }),
    ).rejects.toMatchObject({ message: "local path must be absolute" });
    // A rejected write is the caller's to surface; it is not a read failure.
    expect(syncStore.getState().error).toBeNull();
  });

  it("setEnabled merges the returned status immediately and re-reads the profiles", async () => {
    await ensureSyncHydrated();
    const paused = statusVm({ state: "paused", line: "tgdrive — paused" });
    mockSetEnabled.mockResolvedValue(paused);
    mockProfiles.mockResolvedValue([profileVm({ enabled: false })]);
    mockStatuses.mockResolvedValue([paused]);

    const returned = await setSyncProfileEnabled("p1", false);

    expect(mockSetEnabled).toHaveBeenCalledWith("p1", false);
    expect(returned).toEqual(paused);
    expect(syncStore.getState().statuses.p1.line).toBe("tgdrive — paused");
    expect(syncStore.getState().profiles?.[0].enabled).toBe(false);
  });

  it("syncNow resolves the whole outcome and refreshes the statuses", async () => {
    await ensureSyncHydrated();
    // The sentence and the byte figure are the report the UI renders; a store
    // action that dropped either would leave the click with nothing to show,
    // which is the bug (AD-34-12).
    const outcome: SyncOutcomeVm = {
      committed: true,
      pushed: true,
      pulled: false,
      filesChanged: 3,
      conflicts: [],
      stale: [],
      bytes: 2_048,
      line: "Committed and pushed 3 files, moved 2 KB.",
    };
    mockFolderNow.mockResolvedValue(outcome);
    mockStatuses.mockResolvedValue([statusVm({ line: "tgdrive — up to date just now" })]);

    await expect(syncProfileNow("p1")).resolves.toEqual(outcome);
    expect(syncStore.getState().statuses.p1.line).toBe("tgdrive — up to date just now");
  });

  it("verify resolves the problems found and refreshes the statuses", async () => {
    await ensureSyncHydrated();
    mockVerify.mockResolvedValue(["notes/a.md: digest mismatch"]);

    await expect(verifySyncProfile("p1")).resolves.toEqual(["notes/a.md: digest mismatch"]);
    expect(mockVerify).toHaveBeenCalledWith("p1");
    expect(mockStatuses).toHaveBeenCalledTimes(2);
  });
});

describe("startSyncStatusPolling", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("polls at the active cadence while a profile is working", async () => {
    const working = statusVm({ state: "syncing", phase: "pushing" });
    syncStore.getState().mergeStatuses([working]);
    mockStatuses.mockResolvedValue([working]);
    const stop = startSyncStatusPolling(2000);

    await vi.advanceTimersByTimeAsync(2000);
    expect(mockStatuses).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(2000);
    expect(mockStatuses).toHaveBeenCalledTimes(2);

    stop();
  });

  it("drops back to the idle cadence once a tick reports the work done", async () => {
    syncStore.getState().mergeStatuses([statusVm({ state: "syncing", phase: "pushing" })]);
    // The tick resolves an idle status, so the next delay is the slow one.
    const stop = startSyncStatusPolling(2000);

    await vi.advanceTimersByTimeAsync(2000);
    expect(mockStatuses).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(2000);
    expect(mockStatuses).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(8000);
    expect(mockStatuses).toHaveBeenCalledTimes(2);

    stop();
  });

  it("backs off when nothing is active", async () => {
    syncStore.getState().mergeStatuses([statusVm()]);
    const stop = startSyncStatusPolling(2000);

    await vi.advanceTimersByTimeAsync(2000);
    expect(mockStatuses).not.toHaveBeenCalled();
    // 2 s × the idle factor.
    await vi.advanceTimersByTimeAsync(8000);
    expect(mockStatuses).toHaveBeenCalledTimes(1);

    stop();
  });

  it("keeps the previous snapshot when a poll fails, and keeps polling", async () => {
    const working = statusVm({ state: "syncing", phase: "pushing", line: "Transferring tgdrive" });
    syncStore.getState().mergeStatuses([working]);
    mockStatuses.mockRejectedValue({ code: "internal", message: "engine busy" });
    const stop = startSyncStatusPolling(2000);

    await vi.advanceTimersByTimeAsync(2000);

    // Never flickers to empty mid-transfer.
    expect(syncStore.getState().statuses.p1).toEqual(working);
    expect(syncStore.getState().error).toBe("engine busy");

    await vi.advanceTimersByTimeAsync(2000);
    expect(mockStatuses).toHaveBeenCalledTimes(2);

    stop();
  });

  it("stops on the returned stop function", async () => {
    syncStore.getState().mergeStatuses([statusVm({ state: "syncing" })]);
    const stop = startSyncStatusPolling(2000);

    stop();
    await vi.advanceTimersByTimeAsync(20000);

    expect(mockStatuses).not.toHaveBeenCalled();
  });
});

describe("isSyncStatusActive", () => {
  it("is true while syncing, mid-phase, or with work queued", () => {
    expect(isSyncStatusActive(statusVm({ state: "syncing" }))).toBe(true);
    expect(isSyncStatusActive(statusVm({ phase: "fetching" }))).toBe(true);
    expect(isSyncStatusActive(statusVm({ pending: 3 }))).toBe(true);
  });

  it("is false for a settled profile", () => {
    expect(isSyncStatusActive(statusVm())).toBe(false);
    expect(isSyncStatusActive(statusVm({ state: "paused" }))).toBe(false);
  });

  it("is false for a folder the engine says has stopped, whatever the phase says", () => {
    // Stories 34.8 and 34.10. `phase` is copied off the last streamed event, so
    // a pass that ended without resetting it left every one of these drawing a
    // bar and a rate over a folder that had stopped — a 401 under a half-full
    // "pushing" bar being the case that named the defect. The engine no longer
    // leaves the phase behind; this is the second lock on the same door, and it
    // holds even for a phase no engine wrote.
    for (const state of ["needsAttention", "offline", "mediaAbsent", "paused"]) {
      expect(isSyncStatusActive(statusVm({ state, phase: "pushing" }))).toBe(false);
      // Queued work does not resurrect it either: twelve changes waiting behind
      // a rejected token are waiting, not moving.
      expect(isSyncStatusActive(statusVm({ state, pending: 12 }))).toBe(false);
    }
  });
});

describe("syncProgressFraction", () => {
  it("prefers bytes when a byte total is known", () => {
    expect(syncProgressFraction(statusVm({ bytesDone: 25, bytesTotal: 100 }))).toBe(0.25);
  });

  it("falls back to files when no byte total is known", () => {
    expect(syncProgressFraction(statusVm({ filesDone: 3, filesTotal: 4 }))).toBe(0.75);
  });

  it("is null when no total is known at all", () => {
    expect(syncProgressFraction(statusVm({ bytesDone: 900, filesDone: 2 }))).toBeNull();
  });

  it("treats a zero total as unknown rather than dividing by it", () => {
    expect(syncProgressFraction(statusVm({ bytesTotal: 0, filesTotal: 0 }))).toBeNull();
    expect(syncProgressFraction(statusVm({ bytesTotal: 0, filesDone: 1, filesTotal: 2 }))).toBe(
      0.5,
    );
  });

  it("clamps a total that turned out smaller than what already moved", () => {
    expect(syncProgressFraction(statusVm({ bytesDone: 500, bytesTotal: 100 }))).toBe(1);
  });
});

describe("syncErrorMessage", () => {
  it("uses the Rust-authored message", () => {
    expect(syncErrorMessage({ code: "internal", message: "branch must not be empty" })).toBe(
      "branch must not be empty",
    );
  });

  it("falls back honestly for a rejection with nothing readable", () => {
    expect(syncErrorMessage(undefined)).toBe(SYNC_UNKNOWN_ERROR);
    expect(syncErrorMessage({ message: "   " })).toBe(SYNC_UNKNOWN_ERROR);
  });
});
