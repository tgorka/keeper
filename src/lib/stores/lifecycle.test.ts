import { renderHook } from "@testing-library/react";
import { act } from "react";
import { afterEach, describe, expect, it } from "vitest";
import { lifecycleStore, useMediaShed } from "@/lib/stores/lifecycle";

// The store is a module-load singleton; reset it to the default after every test
// so no phase leaks into an order-dependent sibling suite.
afterEach(() => {
  lifecycleStore.setState({ phase: "foreground" });
});

describe("lifecycleStore", () => {
  it("defaults to the foreground phase (shed false)", () => {
    expect(lifecycleStore.getState().phase).toBe("foreground");
  });

  it("setPhase toggles between foreground and background", () => {
    lifecycleStore.getState().setPhase("background");
    expect(lifecycleStore.getState().phase).toBe("background");
    lifecycleStore.getState().setPhase("foreground");
    expect(lifecycleStore.getState().phase).toBe("foreground");
  });

  it("useMediaShed is false at the default foreground phase", () => {
    const { result } = renderHook(() => useMediaShed());
    expect(result.current).toBe(false);
  });

  it("useMediaShed is true only while backgrounded and restores on foreground", () => {
    const { result } = renderHook(() => useMediaShed());
    expect(result.current).toBe(false);

    act(() => {
      lifecycleStore.getState().setPhase("background");
    });
    expect(result.current).toBe(true);

    act(() => {
      lifecycleStore.getState().setPhase("foreground");
    });
    expect(result.current).toBe(false);
  });
});
