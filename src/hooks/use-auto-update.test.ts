import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  autoUpdateGet: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-updater", () => ({
  check: vi.fn(),
}));
vi.mock("@tauri-apps/plugin-process", () => ({
  relaunch: vi.fn(() => Promise.resolve()),
}));

import { relaunch } from "@tauri-apps/plugin-process";
import { check } from "@tauri-apps/plugin-updater";
import { useAutoUpdate } from "@/hooks/use-auto-update";
import { autoUpdateGet } from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { updateStore } from "@/lib/stores/update";

const mockPlan = vi.mocked(autoUpdateGet);
const mockCheck = vi.mocked(check);
const mockRelaunch = vi.mocked(relaunch);

/** The desktop tier: the only one with an in-app updater. */
const DESKTOP_CAPABILITIES = {
  ...DEFAULT_CAPABILITIES,
  trayIcon: true,
  globalHotkey: true,
  launchAtLogin: true,
  inAppUpdater: true,
  nativeMenuBar: true,
  bridgeSidecar: true,
  revealInFileManager: true,
};

/** The plan Rust serves, with a cadence short enough to step in a test. */
const PLAN = {
  supported: true,
  enabled: true,
  firstCheckDelayMs: 1_000,
  checkIntervalMs: 10_000,
  retryDelayMs: 4_000,
};

beforeEach(() => {
  vi.useFakeTimers();
  mockPlan.mockReset();
  mockCheck.mockReset();
  mockRelaunch.mockClear();
  updateStore.getState().reset();
  capabilitiesStore.setState({ capabilities: DESKTOP_CAPABILITIES, hydrated: true });
});

afterEach(() => {
  vi.useRealTimers();
  updateStore.getState().reset();
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
});

/**
 * Advance the fake clock and let every promise chain it wakes settle. The loop
 * is timers *and* awaits, so `advanceTimersByTime` alone would step the clock
 * past a cycle whose `await` had not resolved yet.
 */
async function advance(ms: number): Promise<void> {
  await act(async () => {
    await vi.advanceTimersByTimeAsync(ms);
  });
}

describe("useAutoUpdate", () => {
  it("installs a found update in the background and never relaunches", async () => {
    const downloadAndInstall = vi.fn(() => Promise.resolve());
    mockPlan.mockResolvedValue(PLAN);
    mockCheck.mockResolvedValue({ version: "0.9.0", downloadAndInstall } as never);

    renderHook(() => useAutoUpdate());
    // Nothing happens during the first-check delay: a cold launch is not the
    // moment to fetch a release manifest.
    await advance(PLAN.firstCheckDelayMs - 1);
    expect(mockCheck).not.toHaveBeenCalled();

    await advance(1);
    expect(downloadAndInstall).toHaveBeenCalledTimes(1);
    expect(updateStore.getState().phase).toEqual({
      kind: "installedNeedsRestart",
      version: "0.9.0",
    });
    // The whole point of the background path: the person's session survives it.
    expect(mockRelaunch).not.toHaveBeenCalled();

    // And with a build already waiting on disk, the loop stops rather than
    // re-downloading it or installing over a bundle this process is reading.
    await advance(PLAN.checkIntervalMs * 3);
    expect(mockCheck).toHaveBeenCalledTimes(1);
  });

  it("checks nothing while the switch is off, and starts when it is turned on", async () => {
    mockPlan.mockResolvedValue({ ...PLAN, enabled: false });
    mockCheck.mockResolvedValue(null);

    renderHook(() => useAutoUpdate());
    await advance(PLAN.checkIntervalMs);
    expect(mockCheck).not.toHaveBeenCalled();

    // The plan is re-read every cycle, so flipping the switch in About takes
    // effect without a relaunch.
    mockPlan.mockResolvedValue(PLAN);
    await advance(PLAN.checkIntervalMs);
    expect(mockCheck).toHaveBeenCalledTimes(1);
    expect(updateStore.getState().phase).toEqual({ kind: "upToDate" });
  });

  it("does not exist where the platform has no in-app updater", async () => {
    mockPlan.mockResolvedValue(PLAN);
    capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: true });

    renderHook(() => useAutoUpdate());
    await advance(PLAN.checkIntervalMs * 2);
    expect(mockPlan).not.toHaveBeenCalled();
    expect(mockCheck).not.toHaveBeenCalled();
  });

  it("never checks on a platform whose install would exit the app", async () => {
    // Windows' updater hands off to an installer that closes keeper. Rust
    // reports `supported: false` there, and a stored `enabled` must not arm it.
    mockPlan.mockResolvedValue({ ...PLAN, supported: false });

    renderHook(() => useAutoUpdate());
    await advance(PLAN.checkIntervalMs * 3);
    expect(mockCheck).not.toHaveBeenCalled();
  });

  it("backs off on the retry delay after a failed check, and recovers", async () => {
    mockPlan.mockResolvedValue(PLAN);
    mockCheck.mockRejectedValueOnce(new Error("offline")).mockResolvedValue(null);

    renderHook(() => useAutoUpdate());
    await advance(PLAN.firstCheckDelayMs);
    expect(updateStore.getState().phase).toEqual({ kind: "error", message: "offline" });

    // Sooner than the interval (a laptop that was offline for a minute must not
    // wait six hours) and not sooner than the retry delay.
    await advance(PLAN.retryDelayMs - 1);
    expect(mockCheck).toHaveBeenCalledTimes(1);
    await advance(1);
    expect(updateStore.getState().phase).toEqual({ kind: "upToDate" });
  });

  it("never starts a second download over a manual one already in flight", async () => {
    mockPlan.mockResolvedValue(PLAN);
    mockCheck.mockResolvedValue(null);
    // What the About section's second click leaves behind while it downloads.
    updateStore.getState().setPhase({ kind: "downloading", version: "0.9.0" });

    renderHook(() => useAutoUpdate());
    await advance(PLAN.firstCheckDelayMs);
    expect(mockCheck).not.toHaveBeenCalled();
    expect(updateStore.getState().phase).toEqual({ kind: "downloading", version: "0.9.0" });

    // It comes back after the retry delay, once that download has resolved.
    updateStore.getState().setPhase({ kind: "idle" });
    await advance(PLAN.retryDelayMs);
    expect(mockCheck).toHaveBeenCalledTimes(1);
  });
});
