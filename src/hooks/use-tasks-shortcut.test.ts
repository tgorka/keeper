/**
 * ⌘8 opens the Tasks view — and does nothing at all where no task surface can
 * exist (Epic 57, FR-351, FR-352, AD-137).
 *
 * The chord is half the owner's complaint ("nie widzę w menu croon like job
 * schedules"); the other half is the registry entry that puts `⌘8` on the ⌘K
 * row, the ⌘? cheat sheet and the native menu bar, which
 * `keeper-core/src/palette.rs` owns and asserts on every host.
 */
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { useTasksShortcut } from "@/hooks/use-tasks-shortcut";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { primaryViewStore } from "@/lib/stores/primary-view";

/** The desktop tier with folder sync, which is the tier tasks exist on. */
const WITH_SYNC = { ...DEFAULT_CAPABILITIES, sync: true, notes: true, sessions: true };

function press(
  key: string,
  opts: { meta?: boolean; ctrl?: boolean; alt?: boolean; composing?: boolean } = {},
  target?: HTMLElement,
): KeyboardEvent {
  const event = new KeyboardEvent("keydown", {
    key,
    metaKey: opts.meta ?? false,
    ctrlKey: opts.ctrl ?? false,
    altKey: opts.alt ?? false,
    isComposing: opts.composing ?? false,
    bubbles: true,
    cancelable: true,
  });
  act(() => {
    (target ?? window).dispatchEvent(event);
  });
  return event;
}

beforeEach(() => {
  primaryViewStore.setState({ view: "inbox" });
  capabilitiesStore.getState().applySnapshot(WITH_SYNC);
});

afterEach(() => {
  primaryViewStore.setState({ view: "inbox" });
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
});

describe("useTasksShortcut", () => {
  it("opens the Tasks view on ⌘8 and preventDefaults", () => {
    renderHook(() => useTasksShortcut());
    const event = press("8", { meta: true });
    expect(primaryViewStore.getState().view).toBe("tasks");
    expect(event.defaultPrevented).toBe(true);
  });

  it("opens on Ctrl+8 (non-mac parity with ⌘1–⌘7)", () => {
    renderHook(() => useTasksShortcut());
    const event = press("8", { ctrl: true });
    expect(primaryViewStore.getState().view).toBe("tasks");
    expect(event.defaultPrevented).toBe(true);
  });

  it("ignores a bare 8 with no modifier, so typing an 8 is still typing an 8", () => {
    renderHook(() => useTasksShortcut());
    const event = press("8");
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(event.defaultPrevented).toBe(false);
  });

  it("ignores ⌘8 mid-IME composition", () => {
    // A composing IME delivers the chord as part of a candidate selection.
    // Acting on it would move the user out of the text they are writing and
    // discard the composition.
    renderHook(() => useTasksShortcut());
    const event = press("8", { meta: true, composing: true });
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(event.defaultPrevented).toBe(false);
  });

  it("leaves ⌘⌥8 alone", () => {
    renderHook(() => useTasksShortcut());
    const event = press("8", { meta: true, alt: true });
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(event.defaultPrevented).toBe(false);
  });

  it("does not hijack the chord while the user is typing in a field", () => {
    renderHook(() => useTasksShortcut());
    for (const tag of ["INPUT", "TEXTAREA", "SELECT"]) {
      const field = document.createElement(tag.toLowerCase());
      document.body.append(field);
      const event = press("8", { meta: true }, field);
      expect(primaryViewStore.getState().view).toBe("inbox");
      expect(event.defaultPrevented).toBe(false);
      field.remove();
    }
    // And in a rich-text surface, which is not a tag but a property.
    //
    // `isContentEditable` is defined on the element rather than set through
    // `contentEditable`, because jsdom implements the attribute and not the
    // derived getter — it answers `false` for a div whose `contentEditable` is
    // `"true"`, so a test that only set the attribute would pass with the
    // guard's `isContentEditable` branch deleted.
    const editable = document.createElement("div");
    Object.defineProperty(editable, "isContentEditable", { value: true });
    document.body.append(editable);
    const event = press("8", { meta: true }, editable);
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(event.defaultPrevented).toBe(false);
    editable.remove();
  });

  it("is a no-op with the capability off: no view change and no preventDefault", () => {
    // AD-27's no-dead-buttons rule. Where folder sync cannot run there is no
    // `sync.db` to keep a task record in, so the chord must fall through
    // untouched rather than switch to a pane that can only be empty — and it
    // must not swallow the event either, or ⌘8 stops doing whatever the webview
    // would otherwise do with it.
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    renderHook(() => useTasksShortcut());
    const event = press("8", { meta: true });
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(event.defaultPrevented).toBe(false);
  });
});
