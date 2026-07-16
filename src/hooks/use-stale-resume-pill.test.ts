import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  STALE_RESUME_PILL_SHOW_DELAY_MS,
  STALE_RESUME_PILL_TIMEOUT_MS,
  useStaleResumePill,
} from "@/hooks/use-stale-resume-pill";
import { accountStatusStore } from "@/lib/stores/account-status";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";

/** All capabilities present = the desktop tier (no listener attached). */
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
  act(() => {
    document.dispatchEvent(new Event("visibilitychange"));
  });
}

/** Mark the mirror hydrated on the reduced (iOS) tier: all-false capabilities. */
function hydrateReducedTier(): void {
  capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
}

/** Open a resume window: the document goes hidden, then comes back visible. */
function resume(): void {
  setVisibility("hidden");
  setVisibility("visible");
}

beforeEach(() => {
  vi.useFakeTimers();
  accountStatusStore.getState().reset();
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
  Object.defineProperty(document, "visibilityState", {
    configurable: true,
    value: "visible",
  });
});

afterEach(() => {
  vi.useRealTimers();
  accountStatusStore.getState().reset();
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
});

describe("useStaleResumePill", () => {
  it("shows after the show-delay on a stale resume and clears when the sync answers", () => {
    hydrateReducedTier();
    const { result } = renderHook(() => useStaleResumePill());
    expect(result.current).toBe(false);

    resume();
    // Not yet — the delay keeps a fresh resume from ever flashing the pill.
    expect(result.current).toBe(false);
    act(() => {
      vi.advanceTimersByTime(STALE_RESUME_PILL_SHOW_DELAY_MS);
    });
    expect(result.current).toBe(true);

    // The resumed sync answers: an account's status CHANGES to online.
    act(() => {
      accountStatusStore.getState().setStatus("a1", "online");
    });
    expect(result.current).toBe(false);
  });

  it("fast resume: a status answer inside the show-delay means the pill never shows", () => {
    hydrateReducedTier();
    const { result } = renderHook(() => useStaleResumePill());

    resume();
    act(() => {
      accountStatusStore.getState().setStatus("a1", "online");
    });
    act(() => {
      vi.advanceTimersByTime(STALE_RESUME_PILL_SHOW_DELAY_MS * 2);
    });
    expect(result.current).toBe(false);
  });

  it("an unrelated account's tick does not clear the pill (multi-account honesty)", () => {
    hydrateReducedTier();
    // A second account that is ALREADY online before the suspension.
    act(() => {
      accountStatusStore.getState().setStatus("a2", "online");
    });
    const { result } = renderHook(() => useStaleResumePill());

    resume();
    act(() => {
      vi.advanceTimersByTime(STALE_RESUME_PILL_SHOW_DELAY_MS);
    });
    expect(result.current).toBe(true);

    // A same-value re-write on the unrelated account: no transition, no progress.
    act(() => {
      accountStatusStore.getState().setStatus("a2", "online");
    });
    expect(result.current).toBe(true);

    // An offline churn is not the resumed sync answering either.
    act(() => {
      accountStatusStore.getState().setStatus("a3", "offline");
    });
    expect(result.current).toBe(true);

    // Only a genuine transition to a connected value settles the pill.
    act(() => {
      accountStatusStore.getState().setStatus("a1", "online");
    });
    expect(result.current).toBe(false);
  });

  it("timeout backstop: a tickless (offline) resume still clears the pill", () => {
    hydrateReducedTier();
    const { result } = renderHook(() => useStaleResumePill());

    resume();
    act(() => {
      vi.advanceTimersByTime(STALE_RESUME_PILL_SHOW_DELAY_MS);
    });
    expect(result.current).toBe(true);

    act(() => {
      vi.advanceTimersByTime(STALE_RESUME_PILL_TIMEOUT_MS);
    });
    expect(result.current).toBe(false);
  });

  it("going hidden ends the window (nothing to indicate off-screen)", () => {
    hydrateReducedTier();
    const { result } = renderHook(() => useStaleResumePill());

    resume();
    act(() => {
      vi.advanceTimersByTime(STALE_RESUME_PILL_SHOW_DELAY_MS);
    });
    expect(result.current).toBe(true);

    setVisibility("hidden");
    expect(result.current).toBe(false);
  });

  it("desktop tier: always false, even across resume-shaped transitions", () => {
    capabilitiesStore.getState().applySnapshot(DESKTOP_CAPABILITIES);
    const { result } = renderHook(() => useStaleResumePill());

    resume();
    act(() => {
      vi.advanceTimersByTime(STALE_RESUME_PILL_SHOW_DELAY_MS * 2);
    });
    expect(result.current).toBe(false);
  });

  it("pre-hydration: false until the tier resolves", () => {
    const { result } = renderHook(() => useStaleResumePill());
    resume();
    act(() => {
      vi.advanceTimersByTime(STALE_RESUME_PILL_SHOW_DELAY_MS * 2);
    });
    expect(result.current).toBe(false);
  });
});
