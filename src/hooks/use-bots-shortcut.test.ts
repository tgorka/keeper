/**
 * ⌘9 opens the Bots view — and does nothing at all where no bots surface can
 * exist (Epic 61, Story 61.4, FR-378).
 *
 * The `use-tasks-shortcut.test` shape, with one assertion this hook owes and
 * that one does not: the capability it self-gates on is **`bots`, not
 * `sessions`**, and a test that only flipped `sessions` would pass with the gate
 * pointed at the wrong flag. So the flag-off case is asserted twice — once with
 * everything off, and once on a machine with full folder sync where only `bots`
 * is off, which is the state a copy-paste slip would render as working.
 */
import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { useBotsShortcut } from "@/hooks/use-bots-shortcut";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { primaryViewStore } from "@/lib/stores/primary-view";

/** A desktop that can hold a conversation. Deliberately with `sync` OFF, so the
 *  happy path proves the pane does not need folder sync. */
const WITH_BOTS = { ...DEFAULT_CAPABILITIES, bots: true };

/** A desktop with the whole sync family on and only `bots` off — the state a
 *  gate wired to `sessions` would wrongly treat as available. */
const SYNC_WITHOUT_BOTS = {
  ...DEFAULT_CAPABILITIES,
  sync: true,
  notes: true,
  sessions: true,
  bots: false,
};

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
  capabilitiesStore.getState().applySnapshot(WITH_BOTS);
});

afterEach(() => {
  primaryViewStore.setState({ view: "inbox" });
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
});

describe("useBotsShortcut", () => {
  it("opens the Bots view on ⌘9 and preventDefaults", () => {
    renderHook(() => useBotsShortcut());
    const event = press("9", { meta: true });
    expect(primaryViewStore.getState().view).toBe("bots");
    expect(event.defaultPrevented).toBe(true);
  });

  it("opens on Ctrl+9 (non-mac parity with ⌘1–⌘8)", () => {
    renderHook(() => useBotsShortcut());
    const event = press("9", { ctrl: true });
    expect(primaryViewStore.getState().view).toBe("bots");
    expect(event.defaultPrevented).toBe(true);
  });

  it("opens with folder sync off, because a conversation needs no git", () => {
    // `WITH_BOTS` has `sync`, `notes` and `sessions` all false. This is the
    // assertion that makes the new flag worth its existence.
    expect(WITH_BOTS.sync).toBe(false);
    renderHook(() => useBotsShortcut());
    press("9", { meta: true });
    expect(primaryViewStore.getState().view).toBe("bots");
  });

  it("ignores a bare 9 with no modifier, so typing a 9 is still typing a 9", () => {
    renderHook(() => useBotsShortcut());
    const event = press("9");
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(event.defaultPrevented).toBe(false);
  });

  it("ignores ⌘9 mid-IME composition", () => {
    // A composing IME delivers the chord as part of a candidate selection.
    // Acting on it would move the user out of the text they are writing and
    // discard the composition.
    renderHook(() => useBotsShortcut());
    const event = press("9", { meta: true, composing: true });
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(event.defaultPrevented).toBe(false);
  });

  it("leaves ⌘⌥9 alone", () => {
    renderHook(() => useBotsShortcut());
    const event = press("9", { meta: true, alt: true });
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(event.defaultPrevented).toBe(false);
  });

  it("does not hijack the chord while the user is typing in a field", () => {
    renderHook(() => useBotsShortcut());
    for (const tag of ["INPUT", "TEXTAREA", "SELECT"]) {
      const field = document.createElement(tag.toLowerCase());
      document.body.append(field);
      const event = press("9", { meta: true }, field);
      expect(primaryViewStore.getState().view).toBe("inbox");
      expect(event.defaultPrevented).toBe(false);
      field.remove();
    }
    // And in a rich-text surface, which is not a tag but a property.
    //
    // `isContentEditable` is defined on the element rather than set through
    // `contentEditable`, because jsdom implements the attribute and not the
    // derived getter — so a test that only set the attribute would pass with
    // the guard's `isContentEditable` branch deleted.
    const editable = document.createElement("div");
    Object.defineProperty(editable, "isContentEditable", { value: true });
    document.body.append(editable);
    const event = press("9", { meta: true }, editable);
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(event.defaultPrevented).toBe(false);
    editable.remove();
  });

  it("is a no-op with the capability off: no view change and no preventDefault", () => {
    // AD-27's no-dead-buttons rule: the chord must fall through untouched
    // rather than switch to a pane that cannot exist — and it must not swallow
    // the event either, or ⌘9 stops doing whatever the webview would otherwise
    // do with it.
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    renderHook(() => useBotsShortcut());
    const event = press("9", { meta: true });
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(event.defaultPrevented).toBe(false);
  });

  it("is a no-op on a full sync machine whose bots flag is off", () => {
    // The mutation this catches: gate the hook on `capabilities.sessions` — the
    // flag ⌘8 uses — and every other test in this file still passes, because
    // `WITH_BOTS` would be the only fixture that noticed. Here `sessions` is
    // on and `bots` is off, so a hook reading the wrong flag switches the view.
    capabilitiesStore.getState().applySnapshot(SYNC_WITHOUT_BOTS);
    renderHook(() => useBotsShortcut());
    const event = press("9", { meta: true });
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(event.defaultPrevented).toBe(false);
  });
});
