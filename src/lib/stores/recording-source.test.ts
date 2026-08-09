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
  setScreenRecordingAccess,
  startRecordingSourcePolling,
  stopRecordingSourcePolling,
} from "@/lib/stores/recording-source";

const mockList = vi.mocked(listRecordingSources);

const SOURCES: RecordingSourcesVm = {
  displays: [
    { id: 1, width: 3456, height: 2234, isMain: true, pixelWidth: 3456, pixelHeight: 2234 },
    { id: 2, width: 1920, height: 1080, isMain: false, pixelWidth: 1920, pixelHeight: 1080 },
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
  // Every enumeration is gated on the Screen Recording grant, so the default
  // state for the tests about enumeration behaviour is "granted". The gate's own
  // tests below set the ungranted states explicitly; `resetRecordingSourceForTest`
  // fails it closed again after each one.
  setScreenRecordingAccess("granted");
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

  it("issues no enumeration at all while Screen Recording is not granted", async () => {
    // The install-day bug: `list_sources` enumerates applications through
    // `SCShareableContent`, which POSTS the macOS permission prompt when the
    // grant is missing. The picker polls it every 3 s and refreshes on focus, so
    // an ungated poll threw a system popup at the user every 3 s forever. Not one
    // read may leave here until the grant is held — from any path.
    setScreenRecordingAccess("notYetRequested");
    startRecordingSourcePolling();
    await vi.advanceTimersByTimeAsync(30_000);
    // Ten poll intervals: ten popups, before.
    expect(mockList).not.toHaveBeenCalled();

    // The focus path funnels through the same gate, not around it.
    await refreshRecordingSources();
    expect(mockList).not.toHaveBeenCalled();
    // And a gated call must not leave the spinner stuck on a read never made.
    expect(recordingSourceStore.getState().refreshing).toBe(false);

    // An explicit denial is just as ungranted — polling it would prompt too.
    setScreenRecordingAccess("denied");
    await vi.advanceTimersByTimeAsync(30_000);
    expect(mockList).not.toHaveBeenCalled();
  });

  it("resumes enumeration the moment the grant lands (no relaunch, no second click)", async () => {
    setScreenRecordingAccess("notYetRequested");
    startRecordingSourcePolling();
    await vi.advanceTimersByTimeAsync(9000);
    expect(mockList).not.toHaveBeenCalled();

    // The user grants and returns; the pre-flight re-probes and reports it. The
    // picker must fill NOW — a user staring at an empty picker until they relaunch
    // is the same bug wearing different clothes — and it must keep polling.
    setScreenRecordingAccess("granted");
    await vi.advanceTimersByTimeAsync(0);
    expect(mockList).toHaveBeenCalledTimes(1);
    expect(recordingSourceStore.getState().sources).toEqual(SOURCES);
    await vi.advanceTimersByTimeAsync(3000);
    expect(mockList).toHaveBeenCalledTimes(2);

    // A revoke re-closes the gate: the very next tick would be the next popup.
    setScreenRecordingAccess("denied");
    await vi.advanceTimersByTimeAsync(9000);
    expect(mockList).toHaveBeenCalledTimes(2);
  });

  it("a grant that arrives after the surface is gone resumes nothing", async () => {
    // Start (or unmount) stops the poll; a late-arriving grant must not revive a
    // poll nobody asked for — that would spawn a `keeper-rec` every 3 s through a
    // live recording.
    setScreenRecordingAccess("notYetRequested");
    startRecordingSourcePolling();
    stopRecordingSourcePolling();
    setScreenRecordingAccess("granted");
    await vi.advanceTimersByTimeAsync(9000);
    expect(mockList).not.toHaveBeenCalled();
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
