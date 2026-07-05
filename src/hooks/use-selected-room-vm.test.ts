import { renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { useSelectedRoomVm } from "@/hooks/use-selected-room-vm";
import type { InboxRoomVm } from "@/lib/ipc/client";
import { archiveRoomsStore } from "@/lib/stores/archive-rooms";
import { favoritesRoomsStore } from "@/lib/stores/favorites-rooms";
import { pinsRoomsStore } from "@/lib/stores/pins-rooms";
import { roomsStore } from "@/lib/stores/rooms";

function room(
  roomId: string,
  accountId = "acctA",
  overrides: Partial<InboxRoomVm> = {},
): InboxRoomVm {
  return {
    accountId,
    hueIndex: 0,
    roomId,
    displayName: roomId,
    lastMessage: null,
    timestamp: null,
    avatarUrl: null,
    isUnread: false,
    mentionCount: 0,
    isArchived: false,
    isPinned: false,
    isFavourite: false,
    network: null,
    networkId: null,
    ...overrides,
  };
}

function resetAll() {
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  pinsRoomsStore.getState().clear();
  favoritesRoomsStore.getState().clear();
  archiveRoomsStore.getState().clear();
}

afterEach(resetAll);

describe("useSelectedRoomVm", () => {
  it("returns null when no room is selected", () => {
    const { result } = renderHook(() => useSelectedRoomVm());
    expect(result.current).toBeNull();
  });

  it("finds the selected room in the inbox window", () => {
    roomsStore.getState().applyBatch({
      ops: [{ op: "reset", rooms: [room("!a"), room("!b", "acctA", { network: "Telegram" })] }],
      total: 2,
    });
    roomsStore.getState().selectRoom({ accountId: "acctA", roomId: "!b" });
    const { result } = renderHook(() => useSelectedRoomVm());
    expect(result.current?.roomId).toBe("!b");
    expect(result.current?.network).toBe("Telegram");
  });

  it("finds the selected room across the pins/favorites/archive windows", () => {
    pinsRoomsStore.getState().applyBatch({
      ops: [{ op: "reset", rooms: [room("!pin", "acctA", { isPinned: true })] }],
      total: 1,
    });
    roomsStore.getState().selectRoom({ accountId: "acctA", roomId: "!pin" });
    const { result } = renderHook(() => useSelectedRoomVm());
    expect(result.current?.roomId).toBe("!pin");
  });

  it("returns null when the selection is not in any window (graceful degrade)", () => {
    roomsStore.getState().applyBatch({ ops: [{ op: "reset", rooms: [room("!a")] }], total: 1 });
    roomsStore.getState().selectRoom({ accountId: "acctA", roomId: "!missing" });
    const { result } = renderHook(() => useSelectedRoomVm());
    expect(result.current).toBeNull();
  });

  it("disambiguates by account id, not just room id", () => {
    roomsStore.getState().applyBatch({
      ops: [
        { op: "reset", rooms: [room("!x", "acctA"), room("!x", "acctB", { network: "Signal" })] },
      ],
      total: 2,
    });
    roomsStore.getState().selectRoom({ accountId: "acctB", roomId: "!x" });
    const { result } = renderHook(() => useSelectedRoomVm());
    expect(result.current?.accountId).toBe("acctB");
    expect(result.current?.network).toBe("Signal");
  });
});
