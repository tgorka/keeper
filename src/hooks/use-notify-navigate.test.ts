import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { NotifyTarget } from "@/lib/ipc/client";
import { bridgeRelinkStore } from "@/lib/stores/bridge-relink";
import { primaryViewStore } from "@/lib/stores/primary-view";

// Capture the registered event handler so the test can fire the navigate event without a
// live Tauri backend. `listen<T>` delivers `{ payload }`, matching the real Tauri shape.
type NavigateHandler = (event: { payload: NotifyTarget }) => void;
let registered: NavigateHandler | undefined;
const unlisten = vi.fn();
let listenImpl: (event: string, handler: NavigateHandler) => Promise<() => void>;

vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, handler: NavigateHandler) => listenImpl(event, handler),
}));

import { useNotifyNavigate } from "@/hooks/use-notify-navigate";

function fire(target: NotifyTarget): void {
  registered?.({ payload: target });
}

beforeEach(() => {
  registered = undefined;
  unlisten.mockClear();
  listenImpl = (_event, handler) => {
    registered = handler;
    return Promise.resolve(unlisten);
  };
  primaryViewStore.setState({ view: "archive" });
  bridgeRelinkStore.setState({ target: null });
});

afterEach(() => {
  primaryViewStore.setState({ view: "inbox" });
  bridgeRelinkStore.setState({ target: null });
});

describe("useNotifyNavigate", () => {
  it("routes a Message target coarsely to the Inbox (no deep landing)", async () => {
    renderHook(() => useNotifyNavigate());
    await waitFor(() => expect(registered).toBeTypeOf("function"));

    fire({
      kind: "message",
      accountId: "acct-1",
      roomId: "!room:example.org",
      eventId: "$ev:example.org",
    });

    expect(primaryViewStore.getState().view).toBe("inbox");
    // Coarse only — no bridge re-link target recorded for a message.
    expect(bridgeRelinkStore.getState().target).toBeNull();
  });

  it("routes a Bridge target to the Bridges view and records the re-link target", async () => {
    renderHook(() => useNotifyNavigate());
    await waitFor(() => expect(registered).toBeTypeOf("function"));

    fire({ kind: "bridge", accountId: "acct-2", networkId: "signal" });

    expect(primaryViewStore.getState().view).toBe("bridges");
    expect(bridgeRelinkStore.getState().target).toEqual({
      accountId: "acct-2",
      networkId: "signal",
    });
  });

  it("does not switch the view for a None target", async () => {
    primaryViewStore.setState({ view: "bridges" });
    renderHook(() => useNotifyNavigate());
    await waitFor(() => expect(registered).toBeTypeOf("function"));

    fire({ kind: "none" });

    // Unchanged — a plain summon+focus lands nowhere new.
    expect(primaryViewStore.getState().view).toBe("bridges");
    expect(bridgeRelinkStore.getState().target).toBeNull();
  });

  it("unlistens on unmount", async () => {
    const { unmount } = renderHook(() => useNotifyNavigate());
    await waitFor(() => expect(registered).toBeTypeOf("function"));
    unmount();
    expect(unlisten).toHaveBeenCalledTimes(1);
  });

  it("is a graceful no-op outside a Tauri host (listen rejects)", async () => {
    listenImpl = () => Promise.reject(new Error("no tauri host"));
    expect(() => renderHook(() => useNotifyNavigate())).not.toThrow();
    await Promise.resolve();
    expect(primaryViewStore.getState().view).toBe("archive");
  });

  it("does not throw when listen throws synchronously", () => {
    listenImpl = () => {
      throw new Error("ipc internals absent");
    };
    expect(() => renderHook(() => useNotifyNavigate())).not.toThrow();
  });
});
