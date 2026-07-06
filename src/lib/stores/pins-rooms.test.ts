import { afterEach, describe, expect, it } from "vitest";
import type { InboxBatch, InboxOp, InboxRoomVm } from "@/lib/ipc/client";
import { pinsRoomsStore } from "@/lib/stores/pins-rooms";

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
    isPinned: true,
    isFavourite: false,
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
  return pinsRoomsStore.getState().rooms.map((r) => r.roomId);
}

afterEach(() => {
  pinsRoomsStore.getState().clear();
});

describe("pinsRoomsStore.applyBatch", () => {
  it("reset replaces contents and sets total", () => {
    pinsRoomsStore
      .getState()
      .applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b")] }], 2));
    expect(ids()).toEqual(["a", "b"]);
    expect(pinsRoomsStore.getState().total).toBe(2);
  });

  it("does not re-sort — mirrors the Rust order verbatim", () => {
    // A later reset with a different order is applied wholesale (no client sort).
    pinsRoomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b")] }]));
    pinsRoomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("b"), room("a")] }]));
    expect(ids()).toEqual(["b", "a"]);
  });

  it("folds multiple ops in sequence within one batch", () => {
    pinsRoomsStore.getState().applyBatch(
      batch([
        { op: "reset", rooms: [room("a")] },
        { op: "append", rooms: [room("b"), room("c")] },
        { op: "remove", index: 0 },
      ]),
    );
    expect(ids()).toEqual(["b", "c"]);
  });

  it("keeps the prior total when a batch omits it", () => {
    pinsRoomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a")] }], 5));
    pinsRoomsStore.getState().applyBatch(batch([{ op: "append", rooms: [room("b")] }], null));
    expect(pinsRoomsStore.getState().total).toBe(5);
  });
});

describe("pinsRoomsStore.clear", () => {
  it("empties the rooms and resets the total", () => {
    pinsRoomsStore
      .getState()
      .applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b")] }], 2));
    pinsRoomsStore.getState().clear();
    expect(ids()).toEqual([]);
    expect(pinsRoomsStore.getState().total).toBeNull();
  });
});
