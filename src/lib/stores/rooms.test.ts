import { afterEach, describe, expect, it } from "vitest";
import type { InboxBatch, InboxOp, InboxRoomVm } from "@/lib/ipc/client";
import { roomsStore } from "@/lib/stores/rooms";

function room(id: string, accountId = "acctA", hue = 0): InboxRoomVm {
  return {
    accountId,
    hueIndex: hue,
    roomId: id,
    displayName: id,
    lastMessage: null,
    timestamp: null,
    avatarUrl: null,
  };
}

function batch(ops: InboxOp[], total: number | null = null): InboxBatch {
  return { ops, total };
}

function ids(): string[] {
  return roomsStore.getState().rooms.map((r) => r.roomId);
}

afterEach(() => {
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
});

describe("roomsStore.applyBatch", () => {
  it("reset replaces contents and sets total", () => {
    roomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b")] }], 5));
    expect(ids()).toEqual(["a", "b"]);
    expect(roomsStore.getState().total).toBe(5);
  });

  it("reset replaces without duplicating on re-subscribe", () => {
    roomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b")] }]));
    roomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b")] }]));
    expect(ids()).toEqual(["a", "b"]);
  });

  it("carries account id and hue on merged rows", () => {
    roomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a", "acctB", 3)] }]));
    const r = roomsStore.getState().rooms[0];
    expect(r.accountId).toBe("acctB");
    expect(r.hueIndex).toBe(3);
  });

  it("append adds to the end in order", () => {
    roomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a")] }]));
    roomsStore.getState().applyBatch(batch([{ op: "append", rooms: [room("b"), room("c")] }]));
    expect(ids()).toEqual(["a", "b", "c"]);
  });

  it("clear empties the list", () => {
    roomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a")] }]));
    roomsStore.getState().applyBatch(batch([{ op: "clear" }]));
    expect(ids()).toEqual([]);
  });

  it("insert splices at index", () => {
    roomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a"), room("c")] }]));
    roomsStore.getState().applyBatch(batch([{ op: "insert", index: 1, room: room("b") }]));
    expect(ids()).toEqual(["a", "b", "c"]);
  });

  it("set replaces at index in place", () => {
    roomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b")] }]));
    roomsStore.getState().applyBatch(batch([{ op: "set", index: 0, room: room("z") }]));
    expect(ids()).toEqual(["z", "b"]);
  });

  it("remove splices out an index", () => {
    roomsStore
      .getState()
      .applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b"), room("c")] }]));
    roomsStore.getState().applyBatch(batch([{ op: "remove", index: 1 }]));
    expect(ids()).toEqual(["a", "c"]);
  });

  it("does not sort — preserves the exact streamed (recency) order from Rust", () => {
    roomsStore
      .getState()
      .applyBatch(batch([{ op: "reset", rooms: [room("z"), room("a"), room("m")] }]));
    expect(ids()).toEqual(["z", "a", "m"]);
  });

  it("preserves total when a later batch reports total: null", () => {
    roomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a")] }], 5));
    expect(roomsStore.getState().total).toBe(5);
    roomsStore.getState().applyBatch(batch([{ op: "append", rooms: [room("b")] }], null));
    expect(roomsStore.getState().total).toBe(5);
  });

  it("clear() resets rooms and total", () => {
    roomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a")] }], 3));
    roomsStore.getState().clear();
    expect(roomsStore.getState().rooms).toEqual([]);
    expect(roomsStore.getState().total).toBeNull();
  });
});

describe("roomsStore.selectRoom", () => {
  it("records the { accountId, roomId } selection and clears it with null", () => {
    expect(roomsStore.getState().selected).toBeNull();
    roomsStore.getState().selectRoom({ accountId: "acctA", roomId: "!a:example.org" });
    expect(roomsStore.getState().selected).toEqual({
      accountId: "acctA",
      roomId: "!a:example.org",
    });
    roomsStore.getState().selectRoom(null);
    expect(roomsStore.getState().selected).toBeNull();
  });
});
