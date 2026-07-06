import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { useCheatSheetShortcut } from "@/hooks/use-cheat-sheet-shortcut";
import { cheatSheetStore } from "@/lib/stores/cheat-sheet";

function press(
  key: string,
  opts: { meta?: boolean; ctrl?: boolean; composing?: boolean } = {},
): KeyboardEvent {
  const event = new KeyboardEvent("keydown", {
    key,
    metaKey: opts.meta ?? false,
    ctrlKey: opts.ctrl ?? false,
    isComposing: opts.composing ?? false,
    bubbles: true,
    cancelable: true,
  });
  act(() => {
    window.dispatchEvent(event);
  });
  return event;
}

beforeEach(() => {
  cheatSheetStore.setState({ isOpen: false });
});

afterEach(() => {
  cheatSheetStore.setState({ isOpen: false });
});

describe("useCheatSheetShortcut", () => {
  it("opens the cheat sheet on ⌘? and preventDefaults", () => {
    renderHook(() => useCheatSheetShortcut());
    const event = press("?", { meta: true });
    expect(cheatSheetStore.getState().isOpen).toBe(true);
    expect(event.defaultPrevented).toBe(true);
  });

  it("opens on Ctrl+? (non-mac parity)", () => {
    renderHook(() => useCheatSheetShortcut());
    press("?", { ctrl: true });
    expect(cheatSheetStore.getState().isOpen).toBe(true);
  });

  it("toggles closed on a second ⌘?", () => {
    renderHook(() => useCheatSheetShortcut());
    press("?", { meta: true });
    expect(cheatSheetStore.getState().isOpen).toBe(true);
    press("?", { meta: true });
    expect(cheatSheetStore.getState().isOpen).toBe(false);
  });

  it("ignores a bare ? with no modifier", () => {
    renderHook(() => useCheatSheetShortcut());
    const event = press("?");
    expect(cheatSheetStore.getState().isOpen).toBe(false);
    expect(event.defaultPrevented).toBe(false);
  });

  it("ignores ⌘? mid-IME composition", () => {
    renderHook(() => useCheatSheetShortcut());
    const event = press("?", { meta: true, composing: true });
    expect(cheatSheetStore.getState().isOpen).toBe(false);
    expect(event.defaultPrevented).toBe(false);
  });

  it("ignores a different chord (⌘K)", () => {
    renderHook(() => useCheatSheetShortcut());
    const event = press("k", { meta: true });
    expect(cheatSheetStore.getState().isOpen).toBe(false);
    expect(event.defaultPrevented).toBe(false);
  });
});
