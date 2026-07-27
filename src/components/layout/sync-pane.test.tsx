import { act, fireEvent, render, screen, waitFor, within } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  // The shared profile/status mirror.
  syncProfiles: vi.fn(),
  syncStatuses: vi.fn(),
  syncProfileSave: vi.fn(),
  syncProfileRemove: vi.fn(),
  syncProfileSetEnabled: vi.fn(),
  syncFolderNow: vi.fn(),
  syncVerify: vi.fn(),
  // The three detail reads plus the parked-unit retry (Story 32.4).
  syncActivity: vi.fn(),
  syncPending: vi.fn(),
  syncProblems: vi.fn(),
  syncRetryParked: vi.fn(),
  // The progress stream, the only source of in-flight counters.
  syncSubscribeProgress: vi.fn(),
  syncUnsubscribeProgress: vi.fn(),
  // Reached only through the Settings section this pane borrows its labels from.
  syncSetCredential: vi.fn(),
  syncClearCredential: vi.fn(),
}));

// The Settings section (whose action labels this pane reuses) opens the native
// directory picker; mock it so mounting never reaches Tauri.
vi.mock("@tauri-apps/plugin-dialog", () => ({
  open: vi.fn(() => Promise.resolve(null)),
}));

import {
  formatSyncWaited,
  SYNC_ACTIVITY_EMPTY_SENTENCE,
  SYNC_ACTIVITY_TITLE,
  SYNC_CONFLICT_SENTENCE,
  SYNC_CONFLICT_TITLE,
  SYNC_PANE_EMPTY_SENTENCE,
  SYNC_PARKED_NO_ERROR_SENTENCE,
  SYNC_PARKED_TITLE,
  SYNC_PENDING_EMPTY_SENTENCE,
  SYNC_PENDING_TITLE,
  SYNC_PROBLEMS_TITLE,
  SYNC_RETRY_LABEL,
  SYNC_SETTLING_NOTE,
  SYNC_SETTLING_SENTENCE,
  SyncPane,
  syncParkedSummary,
  syncPendingReason,
} from "@/components/layout/sync-pane";
import {
  SYNC_NOW_LABEL,
  SYNC_PAUSE_LABEL,
  SYNC_PROGRESS_LABEL,
  SYNC_RESUME_LABEL,
} from "@/components/settings/sync-section";
import type {
  SyncActivityVm,
  SyncPendingVm,
  SyncProblemsVm,
  SyncProfileVm,
  SyncProgressVm,
  SyncStatusVm,
} from "@/lib/ipc/client";
import {
  syncActivity,
  syncFolderNow,
  syncPending,
  syncProblems,
  syncProfileSetEnabled,
  syncProfiles,
  syncRetryParked,
  syncStatuses,
  syncSubscribeProgress,
  syncUnsubscribeProgress,
} from "@/lib/ipc/client";
import { resetSyncStoreForTest, syncStore } from "@/lib/stores/sync";
import {
  refreshSyncDetail,
  resetSyncDetailStoreForTest,
  syncLiveFraction,
} from "@/lib/stores/sync-detail";

const mockProfiles = vi.mocked(syncProfiles);
const mockStatuses = vi.mocked(syncStatuses);
const mockActivity = vi.mocked(syncActivity);
const mockPending = vi.mocked(syncPending);
const mockProblems = vi.mocked(syncProblems);
const mockRetryParked = vi.mocked(syncRetryParked);
const mockFolderNow = vi.mocked(syncFolderNow);
const mockSetEnabled = vi.mocked(syncProfileSetEnabled);
const mockSubscribe = vi.mocked(syncSubscribeProgress);
const mockUnsubscribe = vi.mocked(syncUnsubscribeProgress);

/** The exact line Rust composes — the pane must render it character for character. */
const RUST_LINE = "tgdrive — 3 waiting to sync";
const RUST_TRANSFER_LINE = "Transferring tgdrive — 42/310 files · 1.2 GB of 4.7 GB";

/**
 * The reference "now" every timestamp fixture is measured back from. Taken
 * once at load rather than faked: the relative figures are minute-granular, so
 * the few milliseconds until a test renders cannot move one.
 */
const NOW = Date.now();

/** The subscribed progress sink, captured from the mocked subscribe call. */
let emitProgress: ((event: SyncProgressVm) => void) | null = null;

function profileVm(over: Partial<SyncProfileVm> = {}): SyncProfileVm {
  return {
    id: "p1",
    name: "tgdrive",
    localPath: "/Users/alice/Documents/tgdrive",
    remoteUrl: "git@github.com:alice/tgdrive.git",
    branch: "main",
    direction: "bidirectional",
    lane: "main",
    subpaths: [],
    excludes: [],
    removable: false,
    lfsMode: "materialize",
    lfsThresholdBytes: 4 * 1024 * 1024,
    settleMs: 5000,
    tags: [],
    authorOverride: null,
    enabled: true,
    ...over,
  };
}

function statusVm(over: Partial<SyncStatusVm> = {}): SyncStatusVm {
  return {
    profileId: "p1",
    profileName: "tgdrive",
    state: "watching",
    phase: "idle",
    line: RUST_LINE,
    filesDone: 0,
    filesTotal: null,
    bytesDone: 0,
    bytesTotal: null,
    pending: 3,
    warning: null,
    error: null,
    lastSyncMs: null,
    needsAttention: false,
    ...over,
  };
}

function progressVm(over: Partial<SyncProgressVm> = {}): SyncProgressVm {
  return {
    profileId: "p1",
    profileName: "tgdrive",
    phase: "pushing",
    filesDone: 42,
    filesTotal: 310,
    bytesDone: 1_200_000_000,
    bytesTotal: 4_700_000_000,
    current: null,
    fraction: null,
    ...over,
  };
}

function problemsVm(over: Partial<SyncProblemsVm> = {}): SyncProblemsVm {
  return { warning: null, error: null, parked: [], conflicts: [], ...over };
}

/** Mount the pane and wait for the first status snapshot to land. */
async function renderPane() {
  const view = render(<SyncPane />);
  await screen.findByText(RUST_LINE);
  return view;
}

beforeEach(() => {
  resetSyncStoreForTest();
  resetSyncDetailStoreForTest();
  emitProgress = null;
  mockProfiles.mockResolvedValue([profileVm()]);
  mockStatuses.mockResolvedValue([statusVm()]);
  mockActivity.mockResolvedValue([]);
  mockPending.mockResolvedValue([]);
  mockProblems.mockResolvedValue(problemsVm());
  mockSubscribe.mockImplementation((onProgress: (event: SyncProgressVm) => void) => {
    emitProgress = onProgress;
    return Promise.resolve(7);
  });
  mockUnsubscribe.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("SyncPane profile header", () => {
  it("renders the Rust-composed line verbatim beside a state word, path and host", async () => {
    await renderPane();

    // Verbatim: the tray renders this same sentence, so the pane may not reword it.
    expect(screen.getByText(RUST_LINE)).toBeInTheDocument();
    expect(screen.getByText("tgdrive")).toBeInTheDocument();
    expect(screen.getByText("Watching")).toBeInTheDocument();
    expect(screen.getByText("/Users/alice/Documents/tgdrive")).toBeInTheDocument();
    expect(screen.getByText("github.com")).toBeInTheDocument();
  });

  it("offers Sync now and re-reads the lists after the action", async () => {
    mockFolderNow.mockResolvedValue({
      committed: true,
      pushed: true,
      pulled: false,
      filesChanged: 2,
      conflicts: [],
    });
    await renderPane();
    await waitFor(() => expect(mockActivity).toHaveBeenCalled());
    const readsBefore = mockActivity.mock.calls.length;

    fireEvent.click(screen.getByRole("button", { name: SYNC_NOW_LABEL }));

    await waitFor(() => expect(mockFolderNow).toHaveBeenCalledWith("p1"));
    // An action is exactly when the three lists are most likely to have moved,
    // and the poll is deliberately too slow to notice.
    await waitFor(() => expect(mockActivity.mock.calls.length).toBeGreaterThan(readsBefore));
  });

  it("offers Resume instead of Pause for a paused folder", async () => {
    mockProfiles.mockResolvedValue([profileVm({ enabled: false })]);
    mockStatuses.mockResolvedValue([statusVm({ state: "paused" })]);
    mockSetEnabled.mockResolvedValue(statusVm());
    await renderPane();

    expect(screen.getByText("Paused")).toBeInTheDocument();
    expect(screen.queryByRole("button", { name: SYNC_PAUSE_LABEL })).not.toBeInTheDocument();
    fireEvent.click(screen.getByRole("button", { name: SYNC_RESUME_LABEL }));

    await waitFor(() => expect(mockSetEnabled).toHaveBeenCalledWith("p1", true));
  });

  it("draws no bar without a denominator, and the streamed one once there is", async () => {
    mockStatuses.mockResolvedValue([
      statusVm({ state: "syncing", phase: "pushing", line: RUST_TRANSFER_LINE }),
    ]);
    render(<SyncPane />);
    await screen.findByText(RUST_TRANSFER_LINE);

    // No total known anywhere: the Rust line still says what is happening, and
    // a meter that invents a position would be worse than none.
    expect(screen.queryByRole("progressbar")).not.toBeInTheDocument();

    act(() => {
      emitProgress?.(progressVm({ fraction: 0.42, current: "notes/today.md" }));
    });

    const meter = await screen.findByRole("progressbar", {
      name: `${SYNC_PROGRESS_LABEL}: tgdrive`,
    });
    expect(meter).toHaveAttribute("aria-valuenow", "42");
    // The human description stays the one Rust composed.
    expect(meter).toHaveAttribute("aria-valuetext", RUST_TRANSFER_LINE);
    // The path in flight exists only on the stream.
    expect(screen.getByText("notes/today.md")).toBeInTheDocument();
  });

  it("drops a stale streamed fraction once the poll says the folder is settled", async () => {
    await renderPane();
    act(() => {
      emitProgress?.(progressVm({ fraction: 0.42 }));
    });
    // `watching` with pending work is active, so the bar is honest here…
    await screen.findByRole("progressbar");

    act(() => {
      syncStore.getState().mergeStatuses([statusVm({ state: "idle", pending: 0, phase: "idle" })]);
    });

    // …but the last event the engine sent must not leave a filled bar behind.
    await waitFor(() => expect(screen.queryByRole("progressbar")).not.toBeInTheDocument());
  });

  it("unsubscribes from the progress stream on unmount", async () => {
    const view = await renderPane();
    await waitFor(() => expect(mockSubscribe).toHaveBeenCalled());
    // Let the subscription id land before tearing down.
    await act(async () => {});

    view.unmount();

    await waitFor(() => expect(mockUnsubscribe).toHaveBeenCalledWith(7));
  });
});

describe("SyncPane profile list", () => {
  it("claims nothing before the first read, then says the list is empty", async () => {
    // An empty race never settles: the read stays in flight for the whole test.
    mockProfiles.mockReturnValue(Promise.race([]));
    const view = render(<SyncPane />);
    expect(screen.queryByText(SYNC_PANE_EMPTY_SENTENCE)).not.toBeInTheDocument();
    view.unmount();

    resetSyncStoreForTest();
    mockProfiles.mockResolvedValue([]);
    mockStatuses.mockResolvedValue([]);
    render(<SyncPane />);

    expect(await screen.findByText(SYNC_PANE_EMPTY_SENTENCE)).toBeInTheDocument();
  });
});

describe("SyncPane activity", () => {
  const activity: SyncActivityVm[] = [
    { tsMs: NOW - 120_000, kind: "modified", path: "notes/today.md" },
    { tsMs: NOW - 3_600_000, kind: "added", path: "notes/new.md" },
    { tsMs: NOW - 7_200_000, kind: "deleted", path: "notes/old.md" },
    { tsMs: NOW - 10_800_000, kind: "conflict", path: "notes/shared.sync-conflict-01.md" },
  ];

  it("lists what sync did, newest first, with the kind spoken and the time relative", async () => {
    mockActivity.mockResolvedValue(activity);
    await renderPane();

    const list = await screen.findByRole("list", { name: `${SYNC_ACTIVITY_TITLE}: tgdrive` });
    const rows = within(list).getAllByRole("listitem");
    // Rendered in the order Rust returned them, which is newest first.
    expect(rows).toHaveLength(4);
    expect(rows[0]).toHaveTextContent("notes/today.md");
    expect(rows[3]).toHaveTextContent("notes/shared.sync-conflict-01.md");
    // The kind rides an icon on screen and a word to a screen reader.
    expect(rows[0]).toHaveTextContent("Changed");
    expect(rows[1]).toHaveTextContent("Added");
    expect(rows[2]).toHaveTextContent("Deleted");
    expect(rows[3]).toHaveTextContent("Conflict copy");
    // Relative, in whatever the runtime locale calls two minutes.
    expect(rows[0].textContent ?? "").toMatch(/2\s*min/);
  });

  it("asks for a bounded page rather than the whole history", async () => {
    await renderPane();
    await waitFor(() => expect(mockActivity).toHaveBeenCalled());

    expect(mockActivity).toHaveBeenCalledWith("p1", expect.any(Number));
  });

  it("says nothing has synced yet rather than reporting no data", async () => {
    await renderPane();

    expect(await screen.findByText(SYNC_ACTIVITY_EMPTY_SENTENCE)).toBeInTheDocument();
    expect(
      screen.queryByRole("list", { name: `${SYNC_ACTIVITY_TITLE}: tgdrive` }),
    ).not.toBeInTheDocument();
  });

  it("keeps the previous list when a read fails, instead of claiming it is empty", async () => {
    mockActivity.mockResolvedValue(activity);
    await renderPane();
    await screen.findByRole("list", { name: `${SYNC_ACTIVITY_TITLE}: tgdrive` });

    // An unknown profile rejects rather than resolving empty, so a rejection
    // must never be rendered as calm.
    mockActivity.mockRejectedValue({ code: "internal", message: "no such profile" });
    await act(async () => {
      await refreshSyncDetail("p1");
    });

    expect(await screen.findByText("no such profile")).toBeInTheDocument();
    expect(screen.getByText("notes/today.md")).toBeInTheDocument();
    expect(screen.queryByText(SYNC_ACTIVITY_EMPTY_SENTENCE)).not.toBeInTheDocument();
  });
});

describe("SyncPane pending", () => {
  const pending: SyncPendingVm[] = [
    { path: "notes/draft.md", reason: "settling", sinceMs: NOW - 300_000 },
    { path: "notes/scratch.md", reason: "untracked", sinceMs: null },
  ];

  it("lists what is waiting and why", async () => {
    mockPending.mockResolvedValue(pending);
    await renderPane();

    const list = await screen.findByRole("list", { name: `${SYNC_PENDING_TITLE}: tgdrive` });
    const rows = within(list).getAllByRole("listitem");
    expect(rows).toHaveLength(2);
    expect(rows[0]).toHaveTextContent("notes/draft.md");
    expect(rows[1]).toHaveTextContent("notes/scratch.md");
    expect(rows[1]).toHaveTextContent("New file, not synced yet");
  });

  it("explains a settling file as a wait so far, never as a finish time", async () => {
    mockPending.mockResolvedValue(pending);
    await renderPane();

    const list = await screen.findByRole("list", { name: `${SYNC_PENDING_TITLE}: tgdrive` });
    const settling = within(list).getAllByRole("listitem")[0];
    expect(settling).toHaveTextContent(SYNC_SETTLING_SENTENCE);
    // How long it has been waiting — elapsed, not remaining.
    expect(settling).toHaveTextContent("5 min so far");
    // And the reason there is no estimate at all, said once under the list.
    expect(screen.getByText(SYNC_SETTLING_NOTE)).toBeInTheDocument();
    // Nothing in the list promises when it will finish.
    expect(list.textContent ?? "").not.toMatch(/remaining|left|eta|finishe?s|in \d/i);
  });

  it("drops the settling explanation when nothing is settling", async () => {
    mockPending.mockResolvedValue([pending[1]]);
    await renderPane();

    await screen.findByRole("list", { name: `${SYNC_PENDING_TITLE}: tgdrive` });
    expect(screen.queryByText(SYNC_SETTLING_NOTE)).not.toBeInTheDocument();
  });

  it("says nothing is waiting when the list is genuinely empty", async () => {
    await renderPane();

    expect(await screen.findByText(SYNC_PENDING_EMPTY_SENTENCE)).toBeInTheDocument();
  });
});

describe("SyncPane problems", () => {
  it("renders no Problems section at all when nothing is wrong", async () => {
    await renderPane();

    await screen.findByText(SYNC_PENDING_EMPTY_SENTENCE);
    expect(screen.queryByText(SYNC_PROBLEMS_TITLE)).not.toBeInTheDocument();
  });

  it("names a parked unit's error and retries exactly that unit", async () => {
    mockProblems.mockResolvedValue(
      problemsVm({
        parked: [
          { id: 41, kind: "push", attempts: 5, lastError: "remote hung up" },
          { id: 42, kind: "lfsUpload", attempts: 2, lastError: null },
        ],
      }),
    );
    mockRetryParked.mockResolvedValue(undefined);
    await renderPane();

    const list = await screen.findByRole("list", { name: `${SYNC_PARKED_TITLE}: tgdrive` });
    const rows = within(list).getAllByRole("listitem");
    expect(rows[0]).toHaveTextContent("Push · stopped after 5 attempts");
    expect(rows[0]).toHaveTextContent("remote hung up");
    // A unit that failed without a recorded cause says so rather than showing a gap.
    expect(rows[1]).toHaveTextContent("Large file upload · stopped after 2 attempts");
    expect(rows[1]).toHaveTextContent(SYNC_PARKED_NO_ERROR_SENTENCE);

    // Each Retry is named for the unit it retries, so several are tellable apart.
    fireEvent.click(
      screen.getByRole("button", {
        name: `${SYNC_RETRY_LABEL}: Large file upload · stopped after 2 attempts`,
      }),
    );

    await waitFor(() => expect(mockRetryParked).toHaveBeenCalledWith("p1", 42));
    expect(mockRetryParked).toHaveBeenCalledTimes(1);
  });

  it("lists conflict copies and says which version is which", async () => {
    mockProblems.mockResolvedValue(
      problemsVm({ conflicts: ["notes/shared.sync-conflict-20260727-air.md"] }),
    );
    await renderPane();

    const list = await screen.findByRole("list", { name: `${SYNC_CONFLICT_TITLE}: tgdrive` });
    expect(
      within(list).getByText("notes/shared.sync-conflict-20260727-air.md"),
    ).toBeInTheDocument();
    expect(screen.getByText(SYNC_CONFLICT_SENTENCE)).toBeInTheDocument();
  });

  it("shows the live warning and error the engine reported", async () => {
    mockProblems.mockResolvedValue(
      problemsVm({ warning: "Large files are missing.", error: "Authentication failed." }),
    );
    await renderPane();

    expect(await screen.findByText("Authentication failed.")).toBeInTheDocument();
    expect(screen.getByText("Large files are missing.")).toBeInTheDocument();
  });
});

describe("sync pane projections", () => {
  it("reports a wait as elapsed time, coarsely, and never below zero", () => {
    expect(formatSyncWaited(NOW - 30_000, NOW)).toBe("under a minute");
    expect(formatSyncWaited(NOW - 5 * 60_000, NOW)).toBe("5 min");
    expect(formatSyncWaited(NOW - 3 * 3_600_000, NOW)).toBe("3 hr");
    expect(formatSyncWaited(NOW - 86_400_000, NOW)).toBe("1 day");
    expect(formatSyncWaited(NOW - 3 * 86_400_000, NOW)).toBe("3 days");
    // A clock-skewed start must not read as a wait that has not begun.
    expect(formatSyncWaited(NOW + 60_000, NOW)).toBe("under a minute");
  });

  it("words each pending reason, and shows an unknown one as itself", () => {
    expect(syncPendingReason({ path: "a", reason: "modified", sinceMs: null })).toBe(
      "Changed, not synced yet",
    );
    // A settling row with no recorded start still says what it is waiting for.
    expect(syncPendingReason({ path: "a", reason: "settling", sinceMs: null })).toBe(
      SYNC_SETTLING_SENTENCE,
    );
    // A reason Rust grows later is shown, not swallowed.
    expect(syncPendingReason({ path: "a", reason: "quarantined", sinceMs: null })).toBe(
      "quarantined",
    );
  });

  it("counts a single attempt in the singular and an unknown kind as itself", () => {
    expect(syncParkedSummary({ id: 1, kind: "pull", attempts: 1, lastError: null })).toBe(
      "Pull · stopped after 1 attempt",
    );
    expect(syncParkedSummary({ id: 2, kind: "rebase", attempts: 3, lastError: null })).toBe(
      "rebase · stopped after 3 attempts",
    );
  });

  it("lets the poll decide whether a folder is working and the stream how far", () => {
    const idle = statusVm({ state: "idle", pending: 0 });
    const busy = statusVm({ state: "syncing", phase: "pushing", bytesDone: 250, bytesTotal: 1000 });
    // No status at all, or a settled one: nothing to draw, whatever the stream said.
    expect(syncLiveFraction(undefined, progressVm({ fraction: 0.9 }))).toBeNull();
    expect(syncLiveFraction(idle, progressVm({ fraction: 0.9 }))).toBeNull();
    // The stream refines the polled snapshot…
    expect(syncLiveFraction(busy, progressVm({ fraction: 0.9 }))).toBeCloseTo(0.9);
    // …and is clamped, because a byte total grows as more objects are announced.
    expect(syncLiveFraction(busy, progressVm({ fraction: 1.4 }))).toBe(1);
    // With no event yet, the polled counters still answer.
    expect(syncLiveFraction(busy, undefined)).toBeCloseTo(0.25);
  });
});
