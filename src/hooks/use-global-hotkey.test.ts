import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { chatListFocusStore } from "@/lib/stores/chat-list-focus";
import { primaryViewStore } from "@/lib/stores/primary-view";

// Capture the registered event listener so the test can fire the hotkey event without a
// live Tauri backend. A per-test `listenImpl` lets one case simulate "outside Tauri".
type HotkeyHandler = () => void;
let registered: HotkeyHandler | undefined;
const unlisten = vi.fn();
let listenImpl: (event: string, handler: HotkeyHandler) => Promise<() => void>;

vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, handler: HotkeyHandler) => listenImpl(event, handler),
}));

import { useGlobalHotkey } from "@/hooks/use-global-hotkey";

beforeEach(() => {
  registered = undefined;
  unlisten.mockClear();
  listenImpl = (_event, handler) => {
    registered = handler;
    return Promise.resolve(unlisten);
  };
  primaryViewStore.setState({ view: "archive" });
});

afterEach(() => {
  primaryViewStore.setState({ view: "inbox" });
});

describe("useGlobalHotkey", () => {
  it("switches to Inbox and requests chat-list focus when the event fires", async () => {
    const before = chatListFocusStore.getState().focusNonce;
    renderHook(() => useGlobalHotkey());
    await waitFor(() => expect(registered).toBeTypeOf("function"));

    registered?.();

    expect(primaryViewStore.getState().view).toBe("inbox");
    expect(chatListFocusStore.getState().focusNonce).toBe(before + 1);
  });

  it("unlistens on unmount", async () => {
    const { unmount } = renderHook(() => useGlobalHotkey());
    await waitFor(() => expect(registered).toBeTypeOf("function"));
    unmount();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("is a graceful no-op outside a Tauri host (listen rejects)", async () => {
    listenImpl = () => Promise.reject(new Error("no tauri host"));
    const before = chatListFocusStore.getState().focusNonce;
    // Must not throw; the hotkey bridge is simply inert.
    expect(() => renderHook(() => useGlobalHotkey())).not.toThrow();
    // Nothing dispatched.
    await Promise.resolve();
    expect(chatListFocusStore.getState().focusNonce).toBe(before);
  });

  it("does not throw when listen throws synchronously", () => {
    listenImpl = () => {
      throw new Error("ipc internals absent");
    };
    expect(() => renderHook(() => useGlobalHotkey())).not.toThrow();
  });
});
