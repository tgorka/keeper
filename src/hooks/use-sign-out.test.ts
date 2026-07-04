import { renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { AccountVm } from "@/lib/ipc/client";

// Mock the typed IPC wrapper so the hook never touches Tauri.
const signOut = vi.fn();
vi.mock("@/lib/ipc/client", () => ({
  signOut: (accountId: string) => signOut(accountId),
}));

import { useSignOut } from "@/hooks/use-sign-out";
import { accountsStore } from "@/lib/stores/accounts";
import { connectionStore } from "@/lib/stores/connection";
import { roomsStore } from "@/lib/stores/rooms";
import { timelineStore } from "@/lib/stores/timeline";

const account: AccountVm = {
  accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
  userId: "@alice:example.org",
  homeserverUrl: "https://matrix.example.org/",
};

beforeEach(() => {
  accountsStore.getState().clear();
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  timelineStore.getState().clear();
  connectionStore.getState().reset();
  signOut.mockReset();
  signOut.mockResolvedValue(undefined);
});

afterEach(() => {
  accountsStore.getState().clear();
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
});

describe("useSignOut", () => {
  it("calls signOut with the current account id then resets all stores", async () => {
    accountsStore.getState().setCurrentAccount(account);
    // Seed some live state so the resets are observable.
    roomsStore.getState().selectRoom("!room:example.org");
    roomsStore.getState().applyBatch({
      ops: [
        {
          op: "reset",
          rooms: [
            {
              roomId: "!room:example.org",
              displayName: "Room",
              lastMessage: null,
              timestamp: null,
              avatarUrl: null,
            },
          ],
        },
      ],
      total: 1,
    });
    timelineStore.getState().applyBatch({
      ops: [{ op: "reset", items: [{ kind: "other", key: "k1" }] }],
    });
    connectionStore.getState().applyBatch({ status: "offline" });

    const { result } = renderHook(() => useSignOut());
    await result.current();

    expect(signOut).toHaveBeenCalledWith(account.accountId);
    expect(roomsStore.getState().selectedRoomId).toBeNull();
    expect(roomsStore.getState().rooms).toEqual([]);
    expect(roomsStore.getState().total).toBeNull();
    expect(timelineStore.getState().items).toEqual([]);
    expect(connectionStore.getState().status).toBe("online");
    expect(accountsStore.getState().currentAccount).toBeNull();
  });

  it("resets stores in order, clearing the account last", async () => {
    accountsStore.getState().setCurrentAccount(account);
    const calls: string[] = [];
    // Spy on each store's reset to record call order relative to signOut.
    signOut.mockImplementation(() => {
      calls.push("signOut");
      return Promise.resolve(undefined);
    });
    const selectSpy = vi.spyOn(roomsStore.getState(), "selectRoom");
    const roomsClearSpy = vi.spyOn(roomsStore.getState(), "clear");
    const timelineClearSpy = vi.spyOn(timelineStore.getState(), "clear");
    const connectionResetSpy = vi.spyOn(connectionStore.getState(), "reset");
    const accountsClearSpy = vi.spyOn(accountsStore.getState(), "clear");
    selectSpy.mockImplementation(() => {
      calls.push("selectRoom");
    });
    roomsClearSpy.mockImplementation(() => {
      calls.push("roomsClear");
    });
    timelineClearSpy.mockImplementation(() => {
      calls.push("timelineClear");
    });
    connectionResetSpy.mockImplementation(() => {
      calls.push("connectionReset");
    });
    accountsClearSpy.mockImplementation(() => {
      calls.push("accountsClear");
    });

    const { result } = renderHook(() => useSignOut());
    await result.current();

    expect(calls).toEqual([
      "signOut",
      "selectRoom",
      "roomsClear",
      "timelineClear",
      "connectionReset",
      "accountsClear",
    ]);

    selectSpy.mockRestore();
    roomsClearSpy.mockRestore();
    timelineClearSpy.mockRestore();
    connectionResetSpy.mockRestore();
    accountsClearSpy.mockRestore();
  });

  it("no-ops when there is no current account", async () => {
    const { result } = renderHook(() => useSignOut());
    await result.current();
    expect(signOut).not.toHaveBeenCalled();
  });
});
