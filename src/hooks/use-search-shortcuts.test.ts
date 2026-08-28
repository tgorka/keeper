import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { useSearchShortcuts } from "@/hooks/use-search-shortcuts";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";
import { searchStore } from "@/lib/stores/search";

function press(
  key: string,
  opts: { meta?: boolean; shift?: boolean; from?: EventTarget } = {},
): KeyboardEvent {
  const event = new KeyboardEvent("keydown", {
    key,
    metaKey: opts.meta ?? false,
    shiftKey: opts.shift ?? false,
    bubbles: true,
    cancelable: true,
  });
  act(() => {
    (opts.from ?? window).dispatchEvent(event);
  });
  return event;
}

/**
 * Press the chord inside an element that claims it first — CodeMirror's own
 * `searchKeymap`, which runs on the focused editor DOM and calls
 * `preventDefault` there. Dispatching from an element (rather than straight at
 * `window`) is the part that makes this faithful: the hook's listener is on
 * `window` and so only ever sees an event that has already bubbled past
 * whatever had focus.
 */
function pressInsideAHandledElement(key: string, opts: { meta?: boolean; shift?: boolean } = {}) {
  const host = document.createElement("div");
  document.body.append(host);
  host.addEventListener("keydown", (event) => event.preventDefault());
  try {
    return press(key, { ...opts, from: host });
  } finally {
    host.remove();
  }
}

beforeEach(() => {
  searchStore.setState({ isOpen: false, scope: "global", source: "messages" });
  roomsStore.setState({ selected: null });
  primaryViewStore.setState({ view: "inbox" });
});

afterEach(() => {
  searchStore.setState({ isOpen: false, scope: "global", source: "messages" });
  roomsStore.setState({ selected: null });
  primaryViewStore.setState({ view: "inbox" });
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

  // FR-267: the global surface opens on the source you were already looking at.
  it.each([
    { view: "notes", source: "notes" },
    { view: "sessions", source: "sessions" },
    { view: "inbox", source: "messages" },
    { view: "files", source: "messages" },
  ] as const)("⌘⇧F in the $view view opens the $source source", ({ view, source }) => {
    primaryViewStore.setState({ view });
    renderHook(() => useSearchShortcuts());
    press("f", { meta: true, shift: true });
    expect(searchStore.getState().source).toBe(source);
  });

  // FR-267: an open document binds ⌘F to its own find and calls preventDefault
  // first; a window listener is always last to see the event, so it stands down.
  // The document-level listener here stands in for CodeMirror's `searchKeymap`,
  // which acts on the focused element and so runs before anything on `window`.
  it("stands down when something closer to the keystroke already handled it", () => {
    roomsStore.setState({ selected: { accountId: "a1", roomId: "!r:x" } });
    renderHook(() => useSearchShortcuts());
    pressInsideAHandledElement("f", { meta: true });
    expect(searchStore.getState().isOpen).toBe(false);
  });

  it("stands down on ⌘⇧F too when the event was already handled", () => {
    renderHook(() => useSearchShortcuts());
    pressInsideAHandledElement("f", { meta: true, shift: true });
    expect(searchStore.getState().isOpen).toBe(false);
  });

  it("forces the messages source for an in-chat surface", () => {
    roomsStore.setState({ selected: { accountId: "a1", roomId: "!r:x" } });
    primaryViewStore.setState({ view: "notes" });
    renderHook(() => useSearchShortcuts());
    press("f", { meta: true });
    expect(searchStore.getState().scope).toBe("chat");
    expect(searchStore.getState().source).toBe("messages");
  });
});
