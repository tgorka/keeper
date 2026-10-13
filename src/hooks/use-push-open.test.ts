import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PushOpenVm } from "@/lib/ipc/client";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";

// The shell's two halves without a Tauri backend: what `push_open_take` answers next,
// and the `push://open` handler the hook registers.
let waiting: PushOpenVm | null = null;
let fireTapped: (() => void) | undefined;
let takes = 0;

vi.mock("@tauri-apps/api/core", () => ({
  invoke: (command: string) => {
    if (command !== "push_open_take") {
      return Promise.reject(new Error(`unexpected ${command}`));
    }
    takes += 1;
    const answer = waiting;
    waiting = null;
    return Promise.resolve(answer);
  },
}));
vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, handler: () => void) => {
    if (event === "push://open") {
      fireTapped = handler;
    }
    return Promise.resolve(() => {});
  },
}));

import { usePushOpen } from "@/hooks/use-push-open";

const SESSION = { accountId: "01J9ZK0000000000000000ACCT", roomId: "!session:example.org" };

beforeEach(() => {
  waiting = null;
  fireTapped = undefined;
  primaryViewStore.setState({ view: "inbox" });
  roomsStore.getState().selectRoom(null);
});

afterEach(() => {
  primaryViewStore.setState({ view: "inbox" });
  roomsStore.getState().selectRoom(null);
});

describe("usePushOpen", () => {
  it("opens the session room of the tap that launched keeper, in the Agents window", async () => {
    waiting = SESSION;
    renderHook(() => usePushOpen());

    await waitFor(() => expect(roomsStore.getState().selected).toEqual(SESSION));
    expect(primaryViewStore.getState().view).toBe("agents");
  });

  it("opens the room of a tap while keeper runs, once, and nothing when none waits", async () => {
    renderHook(() => usePushOpen());
    await waitFor(() => expect(fireTapped).toBeTypeOf("function"));
    expect(roomsStore.getState().selected).toBeNull();
    expect(primaryViewStore.getState().view).toBe("inbox");

    waiting = SESSION;
    fireTapped?.();
    await waitFor(() => expect(roomsStore.getState().selected).toEqual(SESSION));

    roomsStore.getState().selectRoom(null);
    const before = takes;
    fireTapped?.();
    await waitFor(() => expect(takes).toBe(before + 1));
    expect(roomsStore.getState().selected).toBeNull();
  });
});
