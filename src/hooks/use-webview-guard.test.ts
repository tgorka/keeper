import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/webview-reload", () => ({
  reloadWebview: vi.fn(),
}));

import {
  PROBE_FRAME_WINDOW_MS,
  PROBE_REQUIRED_MISSES,
  useWebviewGuard,
  WEBVIEW_GUARD_FLAG_KEY,
} from "@/hooks/use-webview-guard";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { reloadWebview } from "@/lib/webview-reload";

const mockReload = vi.mocked(reloadWebview);

/** All capabilities present = the desktop tier (no probe attached). */
const DESKTOP_CAPABILITIES = {
  trayIcon: true,
  globalHotkey: true,
  launchAtLogin: true,
  inAppUpdater: true,
  nativeMenuBar: true,
  bridgeSidecar: true,
  revealInFileManager: true,
};

function setVisibility(state: "hidden" | "visible"): void {
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    value: state,
  });
  document.dispatchEvent(new Event("visibilitychange"));
}

/** Mark the mirror hydrated on the reduced (iOS) tier: all-false capabilities. */
function hydrateReducedTier(): void {
  capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
}

// Manual rAF control: the stub records callbacks and only `fireFrame()` services
// them — "the webview is blank/frozen" is simply "no frame is ever fired".
let rafCallbacks: Map<number, FrameRequestCallback>;
let nextRafId: number;

/** Service every pending animation frame (a healthy webview). */
function fireFrame(): void {
  const callbacks = Array.from(rafCallbacks.values());
  rafCallbacks.clear();
  for (const callback of callbacks) {
    callback(0);
  }
}

/** Let one full blank-declaration window elapse (all consecutive miss windows). */
function elapseBlankWindow(): void {
  vi.advanceTimersByTime(PROBE_FRAME_WINDOW_MS * PROBE_REQUIRED_MISSES);
}

beforeEach(() => {
  vi.useFakeTimers();
  rafCallbacks = new Map();
  nextRafId = 0;
  vi.stubGlobal("requestAnimationFrame", (callback: FrameRequestCallback): number => {
    nextRafId += 1;
    rafCallbacks.set(nextRafId, callback);
    return nextRafId;
  });
  vi.stubGlobal("cancelAnimationFrame", (id: number): void => {
    rafCallbacks.delete(id);
  });
  mockReload.mockClear();
  sessionStorage.clear();
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    value: "visible",
  });
});

afterEach(() => {
  vi.useRealTimers();
  vi.unstubAllGlobals();
  vi.restoreAllMocks();
  sessionStorage.clear();
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
});

describe("useWebviewGuard", () => {
  it("reloads once (flag recorded first) after multiple consecutive missed frames on resume", () => {
    hydrateReducedTier();
    renderHook(() => useWebviewGuard());
    // Settle the attach-time confirmation probe as healthy.
    fireFrame();

    setVisibility("hidden");
    setVisibility("visible");
    elapseBlankWindow();

    expect(mockReload).toHaveBeenCalledTimes(1);
    // The one-shot attempt was durably recorded BEFORE the reload.
    expect(sessionStorage.getItem(WEBVIEW_GUARD_FLAG_KEY)).not.toBeNull();
  });

  it("multi-frame requirement: a single missed frame on a slow-but-healthy resume never reloads", () => {
    hydrateReducedTier();
    renderHook(() => useWebviewGuard());
    fireFrame();

    setVisibility("hidden");
    setVisibility("visible");
    // One full window elapses with no frame (a busy snapshot-then-diff resume)…
    vi.advanceTimersByTime(PROBE_FRAME_WINDOW_MS);
    expect(mockReload).not.toHaveBeenCalled();
    // …then the webview services a frame: healthy, and stays healthy.
    fireFrame();
    vi.advanceTimersByTime(PROBE_FRAME_WINDOW_MS * PROBE_REQUIRED_MISSES * 2);

    expect(mockReload).not.toHaveBeenCalled();
    expect(sessionStorage.getItem(WEBVIEW_GUARD_FLAG_KEY)).toBeNull();
  });

  it("attach-time cold boot is confirm-only: an unserviced probe never reloads", () => {
    hydrateReducedTier();
    renderHook(() => useWebviewGuard());
    // Cold boot (no prior resume): the document is visible at attach and no frame
    // is serviced for several blank windows — a slow launch saturated by hydration
    // and first render. A view that never resumed from a jettison must not be
    // reloaded (Review R2); the probe only re-arms/clears on a healthy frame.
    elapseBlankWindow();
    elapseBlankWindow();
    expect(mockReload).not.toHaveBeenCalled();
  });

  it("loop guard: a still-blank second resume never reloads again", () => {
    hydrateReducedTier();
    renderHook(() => useWebviewGuard());
    fireFrame();

    setVisibility("hidden");
    setVisibility("visible");
    elapseBlankWindow();
    expect(mockReload).toHaveBeenCalledTimes(1);

    // Still blank on the next resume: the guard must not thrash.
    setVisibility("hidden");
    setVisibility("visible");
    elapseBlankWindow();
    expect(mockReload).toHaveBeenCalledTimes(1);
  });

  it("fails safe: when the attempt flag cannot be durably recorded, it does not reload", () => {
    hydrateReducedTier();
    vi.spyOn(Storage.prototype, "setItem").mockImplementation(() => {
      throw new Error("quota exceeded");
    });
    renderHook(() => useWebviewGuard());
    fireFrame();

    setVisibility("hidden");
    setVisibility("visible");
    elapseBlankWindow();

    // A recoverable blank beats an unguarded reload loop.
    expect(mockReload).not.toHaveBeenCalled();
  });

  it("healthy probe: a serviced frame never reloads and clears the stored flag", () => {
    hydrateReducedTier();
    // A flag left over from the pre-reload document generation.
    sessionStorage.setItem(WEBVIEW_GUARD_FLAG_KEY, "123");
    renderHook(() => useWebviewGuard());

    // The attach-time confirmation transfers (removes) the stored flag…
    expect(sessionStorage.getItem(WEBVIEW_GUARD_FLAG_KEY)).toBeNull();
    // …and a healthy frame re-arms the guard.
    fireFrame();

    setVisibility("hidden");
    setVisibility("visible");
    elapseBlankWindow();
    // Re-armed: a later genuinely-blank resume may recover again.
    expect(mockReload).toHaveBeenCalledTimes(1);
  });

  it("post-reload document that is still blank is blocked by the transferred flag", () => {
    hydrateReducedTier();
    // This document load follows a guard reload (the prior document set the flag).
    sessionStorage.setItem(WEBVIEW_GUARD_FLAG_KEY, "123");
    renderHook(() => useWebviewGuard());
    // Never fire a frame: this generation is blank too.

    elapseBlankWindow();
    setVisibility("hidden");
    setVisibility("visible");
    elapseBlankWindow();

    expect(mockReload).not.toHaveBeenCalled();
    // The stored flag was consumed into per-document memory, so it cannot leak
    // into (and suppress) a later legitimate recovery in a future generation.
    expect(sessionStorage.getItem(WEBVIEW_GUARD_FLAG_KEY)).toBeNull();
  });

  it("desktop tier: attaches no probe and never reloads", () => {
    capabilitiesStore.getState().applySnapshot(DESKTOP_CAPABILITIES);
    renderHook(() => useWebviewGuard());

    expect(rafCallbacks.size).toBe(0);
    setVisibility("hidden");
    setVisibility("visible");
    elapseBlankWindow();

    expect(rafCallbacks.size).toBe(0);
    expect(mockReload).not.toHaveBeenCalled();
  });

  it("pre-hydration: attaches nothing until the tier resolves", () => {
    renderHook(() => useWebviewGuard());
    expect(rafCallbacks.size).toBe(0);

    setVisibility("hidden");
    setVisibility("visible");
    elapseBlankWindow();
    expect(mockReload).not.toHaveBeenCalled();
  });
});
