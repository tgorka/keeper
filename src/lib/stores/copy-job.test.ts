import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  copyStart: vi.fn(),
  copyStatus: vi.fn(),
  copyCancel: vi.fn(),
}));

import type { CopyJobVm } from "@/lib/ipc/client";
import { copyCancel, copyStart, copyStatus } from "@/lib/ipc/client";
import {
  COPY_POLL_MS,
  COPY_UNKNOWN_ERROR,
  cancelCopyJob,
  copyEntryGroups,
  copyJobFraction,
  copyJobStore,
  isCopyJobTerminal,
  isCopyRunning,
  refreshCopyJob,
  resetCopyJobStoreForTest,
  startCopyJob,
  startCopyJobPolling,
} from "@/lib/stores/copy-job";

const mockStart = vi.mocked(copyStart);
const mockStatus = vi.mocked(copyStatus);
const mockCancel = vi.mocked(copyCancel);

function jobVm(over: Partial<CopyJobVm> = {}): CopyJobVm {
  return {
    id: "job-1",
    source: "/Users/alice/Pictures",
    destination: "/Volumes/backup",
    state: "copying",
    filesDone: 3,
    filesTotal: 10,
    bytesDone: 250,
    bytesTotal: 1000,
    current: "2019/summer.jpg",
    entries: [],
    error: null,
    ...over,
  };
}

beforeEach(() => {
  resetCopyJobStoreForTest();
  mockStart.mockResolvedValue("job-1");
  mockStatus.mockResolvedValue(jobVm());
  mockCancel.mockResolvedValue(undefined);
});

afterEach(() => {
  vi.clearAllMocks();
});

describe("copy job lifecycle", () => {
  it("starts a job and lands its first snapshot without waiting for a poll", async () => {
    await startCopyJob("/Users/alice/Pictures", "/Volumes/backup", true);

    expect(mockStart).toHaveBeenCalledWith("/Users/alice/Pictures", "/Volumes/backup", true);
    const state = copyJobStore.getState();
    expect(state.id).toBe("job-1");
    expect(state.job).toEqual(jobVm());
    expect(state.error).toBeNull();
  });

  it("records a refused start as an IPC failure with no job behind it", async () => {
    // Rust rejects both bad shapes before registering anything, and names which.
    mockStart.mockRejectedValue({ code: "internal", message: "/nope does not exist" });

    await startCopyJob("/nope", "/Volumes/backup", false);

    const state = copyJobStore.getState();
    expect(state.error).toBe("/nope does not exist");
    expect(state.id).toBeNull();
    expect(state.job).toBeNull();
    expect(mockStatus).not.toHaveBeenCalled();
    // Nothing is running, so nothing will be polled.
    expect(isCopyRunning(state)).toBe(false);
  });

  it("falls back to a copy failure rather than a sync one when a rejection says nothing", async () => {
    mockStart.mockRejectedValue(new Error(""));

    await startCopyJob("/a", "/b", false);

    expect(copyJobStore.getState().error).toBe(COPY_UNKNOWN_ERROR);
  });

  it("keeps the last snapshot when a read fails, rather than blanking it", async () => {
    await startCopyJob("/a", "/b", false);
    mockStatus.mockRejectedValue({ code: "internal", message: "the app is shutting down" });

    const stop = startCopyJobPolling(1);
    await vi.waitFor(() => expect(copyJobStore.getState().error).not.toBeNull());
    stop();

    // "No answer" must never render as "no progress".
    expect(copyJobStore.getState().job).toEqual(jobVm());
    expect(copyJobStore.getState().error).toBe("the app is shutting down");
  });

  it("drops a snapshot for a job the user has already replaced", async () => {
    await startCopyJob("/a", "/b", false);
    // A read of the first job is still in flight…
    let landStale: (job: CopyJobVm) => void = () => {};
    mockStatus.mockReturnValueOnce(
      new Promise<CopyJobVm>((resolve) => {
        landStale = resolve;
      }),
    );
    const stale = refreshCopyJob();
    // …when it settles and the user starts another copy straight away.
    copyJobStore.getState().applyJob(jobVm({ state: "cancelled", current: null }));
    mockStart.mockResolvedValue("job-2");
    mockStatus.mockResolvedValue(jobVm({ id: "job-2", filesDone: 0 }));
    await startCopyJob("/c", "/d", false);

    landStale(jobVm({ filesDone: 9 }));
    await stale;

    // The slow read must not resurrect the job it belonged to.
    expect(copyJobStore.getState().id).toBe("job-2");
    expect(copyJobStore.getState().job?.id).toBe("job-2");
    expect(copyJobStore.getState().job?.filesDone).toBe(0);
  });

  it("cancels the running job and reads the stop straight away", async () => {
    await startCopyJob("/a", "/b", false);
    mockStatus.mockResolvedValue(jobVm({ state: "cancelled", current: null }));

    await cancelCopyJob();

    expect(mockCancel).toHaveBeenCalledWith("job-1");
    expect(copyJobStore.getState().job?.state).toBe("cancelled");
  });

  it("cancels nothing when no job was ever started", async () => {
    await cancelCopyJob();

    expect(mockCancel).not.toHaveBeenCalled();
    expect(copyJobStore.getState().error).toBeNull();
  });

  it("refuses a second start over a running job rather than orphaning the first", async () => {
    await startCopyJob("/a", "/b", false);
    mockStart.mockResolvedValue("job-2");

    await startCopyJob("/c", "/d", false);

    // A job whose id the store forgot would keep copying in Rust with nothing
    // able to stop it.
    expect(mockStart).toHaveBeenCalledTimes(1);
    expect(copyJobStore.getState().id).toBe("job-1");
  });
});

describe("copy job polling", () => {
  beforeEach(() => {
    vi.useFakeTimers();
  });

  afterEach(() => {
    vi.useRealTimers();
  });

  it("polls the running job until it settles, then retires itself", async () => {
    copyJobStore.setState({ id: "job-1", job: jobVm(), starting: false, error: null });
    const stop = startCopyJobPolling();

    await vi.advanceTimersByTimeAsync(COPY_POLL_MS);
    expect(mockStatus).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(COPY_POLL_MS);
    expect(mockStatus).toHaveBeenCalledTimes(2);

    mockStatus.mockResolvedValue(jobVm({ state: "done", current: null }));
    await vi.advanceTimersByTimeAsync(COPY_POLL_MS);
    expect(mockStatus).toHaveBeenCalledTimes(3);

    // A terminal job cannot change, and its report arrived with the state that
    // ended it: there is nothing left to ask.
    await vi.advanceTimersByTimeAsync(COPY_POLL_MS * 10);
    expect(mockStatus).toHaveBeenCalledTimes(3);

    stop();
  });

  it("never starts against a job that has already settled", async () => {
    copyJobStore.setState({
      id: "job-1",
      job: jobVm({ state: "done" }),
      starting: false,
      error: null,
    });

    const stop = startCopyJobPolling();
    await vi.advanceTimersByTimeAsync(COPY_POLL_MS * 4);
    stop();

    expect(mockStatus).not.toHaveBeenCalled();
  });

  it("stops on request, for the window that closed mid-copy", async () => {
    copyJobStore.setState({ id: "job-1", job: jobVm(), starting: false, error: null });

    const stop = startCopyJobPolling();
    stop();
    await vi.advanceTimersByTimeAsync(COPY_POLL_MS * 10);

    expect(mockStatus).not.toHaveBeenCalled();
  });
});

describe("copy job projections", () => {
  it("calls a job running until it settles, including before its first snapshot", () => {
    expect(isCopyRunning({ ...copyJobStore.getState(), starting: true })).toBe(true);
    // Started, but nothing read back yet: running, not finished.
    expect(isCopyRunning({ ...copyJobStore.getState(), id: "job-1", job: null })).toBe(true);
    expect(isCopyRunning({ ...copyJobStore.getState(), id: "job-1", job: jobVm() })).toBe(true);
    expect(
      isCopyRunning({ ...copyJobStore.getState(), id: "job-1", job: jobVm({ state: "failed" }) }),
    ).toBe(false);
    // Nothing started at all.
    expect(isCopyRunning(copyJobStore.getState())).toBe(false);
  });

  it("treats every ending as terminal and neither working state as one", () => {
    expect(isCopyJobTerminal("copying")).toBe(false);
    expect(isCopyJobTerminal("verifying")).toBe(false);
    expect(isCopyJobTerminal("done")).toBe(true);
    expect(isCopyJobTerminal("failed")).toBe(true);
    expect(isCopyJobTerminal("cancelled")).toBe(true);
  });

  it("draws no fraction without a byte total, and clamps the one it has", () => {
    expect(copyJobFraction(jobVm())).toBeCloseTo(0.25);
    // A total the walk has not finished working out is unknown, never a zero
    // denominator.
    expect(copyJobFraction(jobVm({ bytesTotal: 0 }))).toBeNull();
    // A total discovered mid-walk can briefly sit under what has already moved.
    expect(copyJobFraction(jobVm({ bytesDone: 1400, bytesTotal: 1000 }))).toBe(1);
  });

  it("groups entries worst first and keeps every row Rust reported", () => {
    const groups = copyEntryGroups([
      { path: "b", bytes: 1, outcome: "identical", reason: null },
      { path: "a", bytes: 1, outcome: "copied", reason: null },
      { path: "c", bytes: 0, outcome: "failed", reason: "gone" },
      { path: "d", bytes: 1, outcome: "collision", reason: null },
      { path: "e", bytes: 1, outcome: "copied", reason: null },
    ]);

    expect(groups.map((group) => group.outcome)).toEqual([
      "failed",
      "collision",
      "copied",
      "identical",
    ]);
    // Order within a group is the order Rust walked the tree in.
    expect(groups[2].entries.map((entry) => entry.path)).toEqual(["a", "e"]);
    expect(groups.reduce((count, group) => count + group.entries.length, 0)).toBe(5);
  });

  it("has no groups for a job that copied nothing", () => {
    expect(copyEntryGroups([])).toEqual([]);
  });
});
