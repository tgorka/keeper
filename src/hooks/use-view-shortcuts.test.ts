import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { useViewShortcuts } from "@/hooks/use-view-shortcuts";
import { primaryViewStore } from "@/lib/stores/primary-view";

function press(
  key: string,
  opts: { meta?: boolean; ctrl?: boolean; target?: EventTarget } = {},
): KeyboardEvent {
  const event = new KeyboardEvent("keydown", {
    key,
    metaKey: opts.meta ?? false,
    ctrlKey: opts.ctrl ?? false,
    bubbles: true,
    cancelable: true,
  });
  if (opts.target !== undefined) {
    Object.defineProperty(event, "target", { value: opts.target, configurable: true });
  }
  act(() => {
    window.dispatchEvent(event);
  });
  return event;
}

beforeEach(() => {
  primaryViewStore.setState({ view: "bridges" });
});

afterEach(() => {
  primaryViewStore.setState({ view: "inbox" });
});

describe("useViewShortcuts", () => {
  it("switches to the inbox view on ⌘1 and preventDefaults", () => {
    renderHook(() => useViewShortcuts());
    const event = press("1", { meta: true });
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(event.defaultPrevented).toBe(true);
  });

  it("switches to the archive view on ⌘2 and preventDefaults", () => {
    renderHook(() => useViewShortcuts());
    const event = press("2", { meta: true });
    expect(primaryViewStore.getState().view).toBe("archive");
    expect(event.defaultPrevented).toBe(true);
  });

  it("switches on Ctrl+1 / Ctrl+2 (non-mac parity)", () => {
    renderHook(() => useViewShortcuts());
    press("1", { ctrl: true });
    expect(primaryViewStore.getState().view).toBe("inbox");
    press("2", { ctrl: true });
    expect(primaryViewStore.getState().view).toBe("archive");
  });

  it("ignores a bare 1/2 with no modifier", () => {
    renderHook(() => useViewShortcuts());
    const event = press("1");
    expect(primaryViewStore.getState().view).toBe("bridges");
    expect(event.defaultPrevented).toBe(false);
  });

  it("does not hijack the chord while typing in an input", () => {
    renderHook(() => useViewShortcuts());
    const input = document.createElement("input");
    const event = press("1", { meta: true, target: input });
    expect(primaryViewStore.getState().view).toBe("bridges");
    expect(event.defaultPrevented).toBe(false);
  });

  it("does not hijack the chord while typing in a textarea", () => {
    renderHook(() => useViewShortcuts());
    const textarea = document.createElement("textarea");
    const event = press("2", { meta: true, target: textarea });
    expect(primaryViewStore.getState().view).toBe("bridges");
    expect(event.defaultPrevented).toBe(false);
  });

  it("ignores an unrelated digit", () => {
    renderHook(() => useViewShortcuts());
    const event = press("5", { meta: true });
    expect(primaryViewStore.getState().view).toBe("bridges");
    expect(event.defaultPrevented).toBe(false);
  });
});
