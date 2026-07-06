import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { useApprovalShortcut } from "@/hooks/use-approval-shortcut";
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
  primaryViewStore.setState({ view: "inbox" });
});

afterEach(() => {
  primaryViewStore.setState({ view: "inbox" });
});

describe("useApprovalShortcut", () => {
  it("switches to the approval view on ⌘3 and preventDefaults", () => {
    renderHook(() => useApprovalShortcut());
    const event = press("3", { meta: true });
    expect(primaryViewStore.getState().view).toBe("approval");
    expect(event.defaultPrevented).toBe(true);
  });

  it("switches to the approval view on Ctrl+3 (non-mac parity)", () => {
    renderHook(() => useApprovalShortcut());
    press("3", { ctrl: true });
    expect(primaryViewStore.getState().view).toBe("approval");
  });

  it("ignores a bare 3 with no modifier", () => {
    renderHook(() => useApprovalShortcut());
    const event = press("3");
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(event.defaultPrevented).toBe(false);
  });

  it("does not hijack the chord while typing in an input", () => {
    renderHook(() => useApprovalShortcut());
    const input = document.createElement("input");
    const event = press("3", { meta: true, target: input });
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(event.defaultPrevented).toBe(false);
  });

  it("does not hijack the chord while typing in a textarea", () => {
    renderHook(() => useApprovalShortcut());
    const textarea = document.createElement("textarea");
    const event = press("3", { meta: true, target: textarea });
    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(event.defaultPrevented).toBe(false);
  });
});
