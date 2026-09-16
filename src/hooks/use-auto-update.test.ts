import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  autoUpdateGet: vi.fn(),
  autoUpdateRestartCheck: vi.fn(),
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
import { autoUpdateGet, autoUpdateRestartCheck } from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { updateStore } from "@/lib/stores/update";

const mockPlan = vi.mocked(autoUpdateGet);
const mockCheck = vi.mocked(check);
const mockRelaunch = vi.mocked(relaunch);
const mockRestartCheck = vi.mocked(autoUpdateRestartCheck);

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
  restartCheckIntervalMs: 500,
};

beforeEach(() => {
  vi.useFakeTimers();
  mockPlan.mockReset();
  mockCheck.mockReset();
  mockRelaunch.mockClear();
  mockRestartCheck.mockReset();
  mockRestartCheck.mockResolvedValue({ restart: false, hold: "inUse" });
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
    expect(updateStore.getState().phase).toMatchObject({
      kind: "installedNeedsRestart",
      version: "0.9.0",
    });
    // Installing is not restarting: while somebody is using keeper (the hold
    // this file's default answers) the session survives untouched.
    expect(mockRelaunch).not.toHaveBeenCalled();

    // And with a build already waiting on disk, nothing is checked or
    // downloaded again — the only question left is whether it may restart.
    await advance(PLAN.checkIntervalMs * 3);
    expect(mockCheck).toHaveBeenCalledTimes(1);
    expect(mockRestartCheck).toHaveBeenCalled();
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

  it("restarts into the waiting build once Rust says the moment is right", async () => {
    const downloadAndInstall = vi.fn(() => Promise.resolve());
    mockPlan.mockResolvedValue(PLAN);
    mockCheck.mockResolvedValue({ version: "0.9.0", downloadAndInstall } as never);

    renderHook(() => useAutoUpdate());
    await advance(PLAN.firstCheckDelayMs);
    expect(mockRelaunch).not.toHaveBeenCalled();

    // The hold answered so far keeps the session; the moment it clears, keeper
    // gets itself onto the new build without anybody clicking.
    mockRestartCheck.mockResolvedValue({ restart: true, hold: null });
    await advance(PLAN.restartCheckIntervalMs);
    expect(mockRelaunch).toHaveBeenCalledTimes(1);
  });

  it("asks with the install age and the time since the last interaction", async () => {
    const downloadAndInstall = vi.fn(() => Promise.resolve());
    mockPlan.mockResolvedValue(PLAN);
    mockCheck.mockResolvedValue({ version: "0.9.0", downloadAndInstall } as never);

    renderHook(() => useAutoUpdate());
    await advance(PLAN.firstCheckDelayMs);
    await advance(PLAN.restartCheckIntervalMs);
    const lastCall = mockRestartCheck.mock.calls[mockRestartCheck.mock.calls.length - 1];
    const [installedFor, idle] = lastCall ?? [];
    // Both are measured, not invented: the install just happened, and the
    // session has been idle since mount (mounting counts as interaction, so
    // this is the elapsed test time rather than "forever").
    expect(installedFor).toBeGreaterThanOrEqual(PLAN.restartCheckIntervalMs);
    expect(idle).toBeGreaterThanOrEqual(PLAN.firstCheckDelayMs);

    // A keypress is somebody working on it, and resets the idleness keeper
    // reports — the difference between "away" and "reading".
    await act(async () => {
      window.dispatchEvent(new KeyboardEvent("keydown", { key: "a" }));
      await vi.advanceTimersByTimeAsync(PLAN.restartCheckIntervalMs);
    });
    const typedCall = mockRestartCheck.mock.calls[mockRestartCheck.mock.calls.length - 1];
    const [, idleAfterTyping] = typedCall ?? [];
    expect(idleAfterTyping).toBeLessThan(idle ?? 0);
  });

  it("stops asking to restart while the switch is off", async () => {
    const downloadAndInstall = vi.fn(() => Promise.resolve());
    mockPlan.mockResolvedValue(PLAN);
    mockCheck.mockResolvedValue({ version: "0.9.0", downloadAndInstall } as never);
    mockRestartCheck.mockResolvedValue({ restart: true, hold: null });

    renderHook(() => useAutoUpdate());
    await advance(PLAN.firstCheckDelayMs);
    // Turned off between the install and the restart question: the build stays
    // on disk and the restart becomes the person's again.
    mockPlan.mockResolvedValue({ ...PLAN, enabled: false });
    await advance(PLAN.restartCheckIntervalMs * 4);
    expect(mockRestartCheck).not.toHaveBeenCalled();
    expect(mockRelaunch).not.toHaveBeenCalled();
  });

  it("keeps asking after a relaunch that did not happen", async () => {
    const downloadAndInstall = vi.fn(() => Promise.resolve());
    mockPlan.mockResolvedValue(PLAN);
    mockCheck.mockResolvedValue({ version: "0.9.0", downloadAndInstall } as never);
    mockRestartCheck.mockResolvedValue({ restart: true, hold: null });
    mockRelaunch.mockRejectedValue(new Error("refused"));

    renderHook(() => useAutoUpdate());
    await advance(PLAN.firstCheckDelayMs);
    await advance(PLAN.restartCheckIntervalMs * 3);
    // A refused relaunch leaves the build installed and waiting, so the loop
    // must not give up on it after one attempt.
    expect(mockRelaunch.mock.calls.length).toBeGreaterThan(1);
    expect(updateStore.getState().phase.kind).toBe("installedNeedsRestart");
  });
});
