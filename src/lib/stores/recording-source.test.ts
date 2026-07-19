import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  listRecordingSources: vi.fn(),
}));

import type { RecordingSourcesVm } from "@/lib/ipc/client";
import { listRecordingSources } from "@/lib/ipc/client";
import {
  DEFAULT_RECORDING_TARGET,
  isSameTarget,
  isSelectionAvailable,
  recordingSourceStore,
  refreshRecordingSources,
  resetRecordingSourceForTest,
  selectedRecordingTarget,
  selectRecordingTarget,
  startRecordingSourcePolling,
  stopRecordingSourcePolling,
} from "@/lib/stores/recording-source";

const mockList = vi.mocked(listRecordingSources);

const SOURCES: RecordingSourcesVm = {
  displays: [
    { id: 1, width: 3456, height: 2234, isMain: true },
    { id: 2, width: 1920, height: 1080, isMain: false },
  ],
  applications: [
    { bundleId: "com.apple.Safari", name: "Safari", pid: 501, icon: "data:image/png;base64,AA==" },
    { bundleId: "com.example.NoIcon", name: "No Icon", pid: 777, icon: null },
  ],
  microphones: [],
  cameras: [],
};

beforeEach(() => {
  vi.useFakeTimers();
  mockList.mockReset();
  mockList.mockResolvedValue(SOURCES);
});

afterEach(() => {
  resetRecordingSourceForTest();
  vi.useRealTimers();
});

describe("recording-source store", () => {
  it("defaults the selection to the main display", () => {
    expect(selectedRecordingTarget()).toEqual(DEFAULT_RECORDING_TARGET);
    expect(selectedRecordingTarget()).toEqual({ kind: "display", displayId: null });
  });

  it("refresh mirrors the Rust source list and flips the refreshing flag", async () => {
    let resolve: (vm: RecordingSourcesVm) => void = () => {};
    mockList.mockReturnValue(
      new Promise<RecordingSourcesVm>((r) => {
        resolve = r;
      }),
    );
    const promise = refreshRecordingSources();
    expect(recordingSourceStore.getState().refreshing).toBe(true);
    resolve(SOURCES);
    await promise;
    expect(recordingSourceStore.getState().refreshing).toBe(false);
    expect(recordingSourceStore.getState().sources).toEqual(SOURCES);
  });

  it("a failed enumeration keeps the prior list (never blanks the picker)", async () => {
    await refreshRecordingSources();
    expect(recordingSourceStore.getState().sources).toEqual(SOURCES);
    mockList.mockRejectedValueOnce(new Error("sidecar hung"));
    await refreshRecordingSources();
    // The prior snapshot survives; refreshing is cleared.
    expect(recordingSourceStore.getState().sources).toEqual(SOURCES);
    expect(recordingSourceStore.getState().refreshing).toBe(false);
  });

  it("select sets the target with radio semantics (exactly one)", () => {
    selectRecordingTarget({ kind: "application", pid: 501, bundleId: "com.apple.Safari" });
    expect(selectedRecordingTarget()).toEqual({
      kind: "application",
      pid: 501,
      bundleId: "com.apple.Safari",
    });
    selectRecordingTarget({ kind: "display", displayId: 2 });
    expect(selectedRecordingTarget()).toEqual({ kind: "display", displayId: 2 });
  });

  it("polls immediately then on the fixed interval, stopping on demand", async () => {
    startRecordingSourcePolling();
    // Immediate enumeration on start.
    expect(mockList).toHaveBeenCalledTimes(1);
    await vi.advanceTimersByTimeAsync(3000);
    expect(mockList).toHaveBeenCalledTimes(2);
    await vi.advanceTimersByTimeAsync(3000);
    expect(mockList).toHaveBeenCalledTimes(3);
    stopRecordingSourcePolling();
    await vi.advanceTimersByTimeAsync(9000);
    // No further polls after stop.
    expect(mockList).toHaveBeenCalledTimes(3);
  });

  it("marks a vanished application selection unavailable (never silently swaps)", () => {
    selectRecordingTarget({ kind: "application", pid: 999, bundleId: "com.gone.App" });
    // The selection is not present in the polled list.
    expect(isSelectionAvailable(selectedRecordingTarget(), SOURCES)).toBe(false);
    // The store never rewrote the selection to a present source.
    expect(selectedRecordingTarget().kind).toBe("application");
    expect((selectedRecordingTarget() as { pid: number }).pid).toBe(999);
  });

  it("treats a present selection (and a never-polled list) as available", () => {
    expect(isSelectionAvailable({ kind: "display", displayId: null }, null)).toBe(true);
    expect(isSelectionAvailable({ kind: "display", displayId: null }, SOURCES)).toBe(true);
    expect(isSelectionAvailable({ kind: "display", displayId: 2 }, SOURCES)).toBe(true);
    expect(
      isSelectionAvailable(
        { kind: "application", pid: 501, bundleId: "com.apple.Safari" },
        SOURCES,
      ),
    ).toBe(true);
  });

  it("isSameTarget compares by display id / app pid+bundleId across kinds", () => {
    expect(
      isSameTarget({ kind: "display", displayId: null }, { kind: "display", displayId: null }),
    ).toBe(true);
    expect(isSameTarget({ kind: "display", displayId: 2 }, { kind: "display", displayId: 2 })).toBe(
      true,
    );
    expect(isSameTarget({ kind: "display", displayId: 2 }, { kind: "display", displayId: 3 })).toBe(
      false,
    );
    // Same pid + same bundle id → the same app.
    expect(
      isSameTarget(
        { kind: "application", pid: 501, bundleId: "a" },
        { kind: "application", pid: 501, bundleId: "a" },
      ),
    ).toBe(true);
    // Same pid but a DIFFERENT bundle id → a recycled pid, NOT the same app.
    expect(
      isSameTarget(
        { kind: "application", pid: 501, bundleId: "a" },
        { kind: "application", pid: 501, bundleId: "b" },
      ),
    ).toBe(false);
    expect(
      isSameTarget(
        { kind: "display", displayId: 1 },
        { kind: "application", pid: 501, bundleId: "a" },
      ),
    ).toBe(false);
  });

  it("a recycled pid (different bundle id) reads back as unavailable", () => {
    // The selected app's pid is still in the list, but now belongs to a
    // different app (different bundle id) — must NOT read as still-available.
    expect(
      isSelectionAvailable({ kind: "application", pid: 501, bundleId: "com.gone.App" }, SOURCES),
    ).toBe(false);
  });

  it("reset restores the default selection and clears the mirror", async () => {
    await refreshRecordingSources();
    selectRecordingTarget({ kind: "display", displayId: 2 });
    resetRecordingSourceForTest();
    expect(recordingSourceStore.getState().sources).toBeNull();
    expect(selectedRecordingTarget()).toEqual(DEFAULT_RECORDING_TARGET);
    expect(recordingSourceStore.getState().refreshing).toBe(false);
  });
});
