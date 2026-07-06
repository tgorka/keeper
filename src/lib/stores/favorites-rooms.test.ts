import { afterEach, describe, expect, it } from "vitest";
import type { InboxBatch, InboxOp, InboxRoomVm } from "@/lib/ipc/client";
import { favoritesRoomsStore } from "@/lib/stores/favorites-rooms";

function room(id: string, overrides: Partial<InboxRoomVm> = {}): InboxRoomVm {
  return {
    accountId: "acctA",
    hueIndex: 0,
    roomId: id,
    displayName: id,
    lastMessage: null,
    timestamp: null,
    avatarUrl: null,
    isUnread: false,
    mentionCount: 0,
    isArchived: false,
    isPinned: false,
    isFavourite: true,
    network: null,
    networkId: null,
    muteState: "none",
    ...overrides,
  };
}

function batch(ops: InboxOp[], total: number | null = null): InboxBatch {
  return { ops, total };
}

function ids(): string[] {
  return favoritesRoomsStore.getState().rooms.map((r) => r.roomId);
}

afterEach(() => {
  favoritesRoomsStore.getState().clear();
});

describe("favoritesRoomsStore.applyBatch", () => {
  it("reset replaces contents and sets total", () => {
    favoritesRoomsStore
      .getState()
      .applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b")] }], 2));
    expect(ids()).toEqual(["a", "b"]);
    expect(favoritesRoomsStore.getState().total).toBe(2);
  });

  it("does not re-sort — mirrors the Rust recency order verbatim", () => {
    favoritesRoomsStore
      .getState()
      .applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b")] }]));
    favoritesRoomsStore
      .getState()
      .applyBatch(batch([{ op: "reset", rooms: [room("b"), room("a")] }]));
    expect(ids()).toEqual(["b", "a"]);
  });

  it("folds multiple ops in sequence within one batch", () => {
    favoritesRoomsStore.getState().applyBatch(
      batch([
        { op: "reset", rooms: [room("a")] },
        { op: "append", rooms: [room("b"), room("c")] },
        { op: "remove", index: 0 },
      ]),
    );
    expect(ids()).toEqual(["b", "c"]);
  });

  it("keeps the prior total when a batch omits it", () => {
    favoritesRoomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a")] }], 5));
    favoritesRoomsStore.getState().applyBatch(batch([{ op: "append", rooms: [room("b")] }], null));
    expect(favoritesRoomsStore.getState().total).toBe(5);
  });
});

describe("favoritesRoomsStore.clear", () => {
  it("empties the rooms and resets the total", () => {
    favoritesRoomsStore
      .getState()
      .applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b")] }], 2));
    favoritesRoomsStore.getState().clear();
    expect(ids()).toEqual([]);
    expect(favoritesRoomsStore.getState().total).toBeNull();
  });
});
