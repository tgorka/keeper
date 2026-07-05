import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { useSearchShortcuts } from "@/hooks/use-search-shortcuts";
import { roomsStore } from "@/lib/stores/rooms";
import { searchStore } from "@/lib/stores/search";

function press(key: string, opts: { meta?: boolean; shift?: boolean } = {}) {
  const event = new KeyboardEvent("keydown", {
    key,
    metaKey: opts.meta ?? false,
    shiftKey: opts.shift ?? false,
    bubbles: true,
    cancelable: true,
  });
  act(() => {
    window.dispatchEvent(event);
  });
  return event;
}

beforeEach(() => {
  searchStore.setState({ isOpen: false, scope: "global" });
  roomsStore.setState({ selected: null });
});

afterEach(() => {
  searchStore.setState({ isOpen: false, scope: "global" });
  roomsStore.setState({ selected: null });
});

describe("useSearchShortcuts", () => {
  it("opens global search on ⌘⇧F and preventDefaults", () => {
    renderHook(() => useSearchShortcuts());
    const event = press("F", { meta: true, shift: true });
    expect(searchStore.getState().isOpen).toBe(true);
    expect(searchStore.getState().scope).toBe("global");
    expect(event.defaultPrevented).toBe(true);
  });

  it("opens in-chat search on ⌘F when a Chat is open and preventDefaults", () => {
    roomsStore.setState({ selected: { accountId: "a1", roomId: "!r:x" } });
    renderHook(() => useSearchShortcuts());
    const event = press("f", { meta: true });
    expect(searchStore.getState().isOpen).toBe(true);
    expect(searchStore.getState().scope).toBe("chat");
    expect(event.defaultPrevented).toBe(true);
  });

  it("is a no-op on ⌘F with no Chat open, but still preventDefaults native find", () => {
    renderHook(() => useSearchShortcuts());
    const event = press("f", { meta: true });
    expect(searchStore.getState().isOpen).toBe(false);
    // ⌘F is the webview's native find — always suppressed.
    expect(event.defaultPrevented).toBe(true);
  });

  it("ignores a bare F with no modifier", () => {
    renderHook(() => useSearchShortcuts());
    const event = press("f");
    expect(searchStore.getState().isOpen).toBe(false);
    expect(event.defaultPrevented).toBe(false);
  });
});
