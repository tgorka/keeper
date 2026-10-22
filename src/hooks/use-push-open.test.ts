import { renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PushOpenVm } from "@/lib/ipc/client";
import { agentSessionsStore } from "@/lib/stores/agent-sessions";
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

const SESSION = { database: "tgdrive", sessionId: "ana.1.3" };

beforeEach(() => {
  waiting = null;
  fireTapped = undefined;
  primaryViewStore.setState({ view: "inbox" });
  roomsStore.getState().selectRoom(null);
  agentSessionsStore.getState().select(null);
});

afterEach(() => {
  primaryViewStore.setState({ view: "inbox" });
  roomsStore.getState().selectRoom(null);
  agentSessionsStore.getState().select(null);
});

describe("usePushOpen", () => {
  it("opens the agents session of the tap that launched keeper, by its database", async () => {
    waiting = { kind: "session", ...SESSION };
    renderHook(() => usePushOpen());

    await waitFor(() => expect(agentSessionsStore.getState().selected).toEqual(SESSION));
    expect(primaryViewStore.getState().view).toBe("agents");
    expect(roomsStore.getState().selected).toBeNull();
  });

  it("opens the session of a tap while keeper runs, once, and nothing when none waits", async () => {
    renderHook(() => usePushOpen());
    await waitFor(() => expect(fireTapped).toBeTypeOf("function"));
    expect(agentSessionsStore.getState().selected).toBeNull();
    expect(primaryViewStore.getState().view).toBe("inbox");

    waiting = { kind: "session", ...SESSION };
    fireTapped?.();
    await waitFor(() => expect(agentSessionsStore.getState().selected).toEqual(SESSION));

    agentSessionsStore.getState().select(null);
    const before = takes;
    fireTapped?.();
    await waitFor(() => expect(takes).toBe(before + 1));
    expect(agentSessionsStore.getState().selected).toBeNull();
  });
});
