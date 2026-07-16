import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useAppLifecycle } from "@/hooks/use-app-lifecycle";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { lifecycleStore } from "@/lib/stores/lifecycle";

// Mock the IPC wrapper so the hook drives a spy, never a live Tauri backend.
const appLifecycleChanged = vi.fn<(phase: "foreground" | "background") => Promise<void>>(() =>
  Promise.resolve(),
);
vi.mock("@/lib/ipc/client", () => ({
  appLifecycleChanged: (phase: "foreground" | "background") => appLifecycleChanged(phase),
}));

/** All seven capabilities present = the desktop tier (no listener attached). */
const DESKTOP_CAPABILITIES = {
  trayIcon: true,
  globalHotkey: true,
  launchAtLogin: true,
  inAppUpdater: true,
  nativeMenuBar: true,
  bridgeSidecar: true,
  revealInFileManager: true,
  recording: false,
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

beforeEach(() => {
  appLifecycleChanged.mockClear();
  // Default: reset to the safe pre-hydration state (predicate false).
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  setVisibility("visible");
});

afterEach(() => {
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  // The lifecycle store is a module-load singleton; reset it so no phase leaks
  // into an order-dependent sibling suite.
  lifecycleStore.setState({ phase: "foreground" });
});

describe("useAppLifecycle", () => {
  it("dispatches the current phase on mount — 'foreground' when already visible", () => {
    // beforeEach leaves the document visible: attaching on the reduced tier must
    // emit the current state at once, not wait for a transition.
    hydrateReducedTier();
    renderHook(() => useAppLifecycle());

    expect(appLifecycleChanged).toHaveBeenCalledTimes(1);
    expect(appLifecycleChanged).toHaveBeenCalledWith("foreground");
  });

  it("dispatches 'background' on mount when the reduced tier is already hidden", () => {
    // Launched/hydrated straight into the background: the hidden ⇒ paused
    // guarantee must hold even without a later visibilitychange transition.
    hydrateReducedTier();
    setVisibility("hidden");
    renderHook(() => useAppLifecycle());

    expect(appLifecycleChanged).toHaveBeenCalledTimes(1);
    expect(appLifecycleChanged).toHaveBeenCalledWith("background");
  });

  it("dispatches 'background' when the reduced tier goes hidden", () => {
    hydrateReducedTier();
    renderHook(() => useAppLifecycle());
    // Drop the mount-time 'foreground' so the transition is asserted in isolation.
    appLifecycleChanged.mockClear();

    setVisibility("hidden");

    expect(appLifecycleChanged).toHaveBeenCalledTimes(1);
    expect(appLifecycleChanged).toHaveBeenCalledWith("background");
  });

  it("dispatches 'foreground' when the reduced tier returns visible", () => {
    hydrateReducedTier();
    setVisibility("hidden");
    renderHook(() => useAppLifecycle());
    appLifecycleChanged.mockClear();

    setVisibility("visible");

    expect(appLifecycleChanged).toHaveBeenCalledTimes(1);
    expect(appLifecycleChanged).toHaveBeenCalledWith("foreground");
  });

  it("attaches no listener on the desktop tier (Story 10.3 unregressed)", () => {
    capabilitiesStore.getState().applySnapshot(DESKTOP_CAPABILITIES);
    renderHook(() => useAppLifecycle());

    setVisibility("hidden");
    setVisibility("visible");

    // No mount-time dispatch and no transition dispatch off the desktop tier.
    expect(appLifecycleChanged).not.toHaveBeenCalled();
  });

  it("attaches no listener before capabilities hydrate", () => {
    // hydrated === false (from beforeEach): predicate false, nothing attached.
    renderHook(() => useAppLifecycle());

    setVisibility("hidden");

    expect(appLifecycleChanged).not.toHaveBeenCalled();
  });

  it("removes the listener when the predicate flips to desktop", () => {
    hydrateReducedTier();
    const { rerender } = renderHook(() => useAppLifecycle());

    // Flip to the desktop tier: the reduced-tier listener must be torn down.
    capabilitiesStore.getState().applySnapshot(DESKTOP_CAPABILITIES);
    rerender();
    // Ignore the mount-time 'foreground' from the reduced-tier attach.
    appLifecycleChanged.mockClear();

    setVisibility("hidden");

    expect(appLifecycleChanged).not.toHaveBeenCalled();
  });

  it("removes the listener on unmount", () => {
    hydrateReducedTier();
    const { unmount } = renderHook(() => useAppLifecycle());
    unmount();
    appLifecycleChanged.mockClear();

    setVisibility("hidden");

    expect(appLifecycleChanged).not.toHaveBeenCalled();
  });

  it("feeds the lifecycle store alongside the IPC call on the reduced tier", () => {
    // The single listener also writes the frontend lifecycle store (Story 14.5),
    // so the media shed derives from one lifecycle truth.
    hydrateReducedTier();
    renderHook(() => useAppLifecycle());
    // Mount-time dispatch while visible: store reads foreground (shed false).
    expect(lifecycleStore.getState().phase).toBe("foreground");

    setVisibility("hidden");
    expect(lifecycleStore.getState().phase).toBe("background");

    setVisibility("visible");
    expect(lifecycleStore.getState().phase).toBe("foreground");
  });

  it("never touches the lifecycle store on the desktop tier (byte-identical)", () => {
    capabilitiesStore.getState().applySnapshot(DESKTOP_CAPABILITIES);
    renderHook(() => useAppLifecycle());

    setVisibility("hidden");
    setVisibility("visible");

    // No listener attaches on desktop, so the store stays at its default.
    expect(lifecycleStore.getState().phase).toBe("foreground");
  });

  it("swallows an IPC rejection (no toast, no throw)", () => {
    hydrateReducedTier();
    setVisibility("hidden");
    appLifecycleChanged.mockReturnValueOnce(Promise.reject(new Error("no tauri host")));

    // The mount-time dispatch itself hits the rejected call and must not throw.
    expect(() => renderHook(() => useAppLifecycle())).not.toThrow();
    expect(appLifecycleChanged).toHaveBeenCalledWith("background");
  });
});
