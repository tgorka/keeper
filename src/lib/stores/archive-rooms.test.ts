import { afterEach, describe, expect, it } from "vitest";
import type { InboxBatch, InboxOp, InboxRoomVm } from "@/lib/ipc/client";
import { archiveRoomsStore } from "@/lib/stores/archive-rooms";

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
    isArchived: true,
    isPinned: false,
    isFavourite: false,
    network: null,
    ...overrides,
  };
}

function batch(ops: InboxOp[], total: number | null = null): InboxBatch {
  return { ops, total };
}

function ids(): string[] {
  return archiveRoomsStore.getState().rooms.map((r) => r.roomId);
}

afterEach(() => {
  archiveRoomsStore.getState().clear();
});

describe("archiveRoomsStore.applyBatch", () => {
  it("reset replaces contents and sets total", () => {
    archiveRoomsStore
      .getState()
      .applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b")] }], 2));
    expect(ids()).toEqual(["a", "b"]);
    expect(archiveRoomsStore.getState().total).toBe(2);
  });

  it("set replaces the room at an index in place", () => {
    archiveRoomsStore
      .getState()
      .applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b")] }]));
    archiveRoomsStore.getState().applyBatch(batch([{ op: "set", index: 1, room: room("b2") }]));
    expect(ids()).toEqual(["a", "b2"]);
  });

  it("remove drops the room at an index", () => {
    archiveRoomsStore
      .getState()
      .applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b"), room("c")] }]));
    archiveRoomsStore.getState().applyBatch(batch([{ op: "remove", index: 1 }]));
    expect(ids()).toEqual(["a", "c"]);
  });

  it("folds multiple ops in sequence within one batch", () => {
    archiveRoomsStore.getState().applyBatch(
      batch([
        { op: "reset", rooms: [room("a")] },
        { op: "append", rooms: [room("b"), room("c")] },
        { op: "remove", index: 0 },
      ]),
    );
    expect(ids()).toEqual(["b", "c"]);
  });

  it("keeps the prior total when a batch omits it", () => {
    archiveRoomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a")] }], 5));
    archiveRoomsStore.getState().applyBatch(batch([{ op: "append", rooms: [room("b")] }], null));
    expect(archiveRoomsStore.getState().total).toBe(5);
  });
});

describe("archiveRoomsStore.clear", () => {
  it("empties the rooms and resets the total", () => {
    archiveRoomsStore
      .getState()
      .applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b")] }], 2));
    archiveRoomsStore.getState().clear();
    expect(ids()).toEqual([]);
    expect(archiveRoomsStore.getState().total).toBeNull();
  });
});
