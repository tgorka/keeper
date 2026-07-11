import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useReducedMotion } from "@/hooks/use-reduced-motion";

/**
 * Mock matchMedia so the `(prefers-reduced-motion: reduce)` query reports the
 * given preference, and return a controller that flips it and notifies every
 * registered `change` listener — exercising the hook's reactive path.
 */
const originalMatchMedia = window.matchMedia;
function mockReducedMotion(initial: boolean) {
  let matches = initial;
  const listeners: Array<() => void> = [];
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    get matches() {
      return query.includes("prefers-reduced-motion") ? matches : false;
    },
    media: query,
    onchange: null,
    addEventListener: (_type: string, listener: () => void) => {
      listeners.push(listener);
    },
    removeEventListener: (_type: string, listener: () => void) => {
      const index = listeners.indexOf(listener);
      if (index !== -1) {
        listeners.splice(index, 1);
      }
    },
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  }));
  return {
    set(next: boolean) {
      matches = next;
      for (const listener of [...listeners]) {
        listener();
      }
    },
  };
}

afterEach(() => {
  window.matchMedia = originalMatchMedia;
  vi.restoreAllMocks();
});

describe("useReducedMotion", () => {
  it("returns false when the preference does not match", () => {
    mockReducedMotion(false);
    const { result } = renderHook(() => useReducedMotion());
    expect(result.current).toBe(false);
  });

  it("returns true synchronously when reduce is already preferred", () => {
    mockReducedMotion(true);
    const { result } = renderHook(() => useReducedMotion());
    expect(result.current).toBe(true);
  });

  it("reacts to a live preference change in both directions", () => {
    const media = mockReducedMotion(false);
    const { result } = renderHook(() => useReducedMotion());
    expect(result.current).toBe(false);

    act(() => {
      media.set(true);
    });
    expect(result.current).toBe(true);

    act(() => {
      media.set(false);
    });
    expect(result.current).toBe(false);
  });

  it("stops listening after unmount", () => {
    const media = mockReducedMotion(false);
    const { result, unmount } = renderHook(() => useReducedMotion());
    unmount();
    // Flipping after unmount must not throw (the listener was removed).
    act(() => {
      media.set(true);
    });
    expect(result.current).toBe(false);
  });
});
