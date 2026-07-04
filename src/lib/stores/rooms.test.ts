import { afterEach, describe, expect, it } from "vitest";
import type { RoomListBatch, RoomListOp, RoomVm } from "@/lib/ipc/client";
import { roomsStore } from "@/lib/stores/rooms";

function room(id: string): RoomVm {
  return {
    roomId: id,
    displayName: id,
    lastMessage: null,
    timestamp: null,
    avatarUrl: null,
  };
}

function batch(ops: RoomListOp[], total: number | null = null): RoomListBatch {
  return { ops, total };
}

function ids(): string[] {
  return roomsStore.getState().rooms.map((r) => r.roomId);
}

afterEach(() => {
  roomsStore.getState().clear();
});

describe("roomsStore.applyBatch", () => {
  it("reset replaces contents and sets total", () => {
    roomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b")] }], 5));
    expect(ids()).toEqual(["a", "b"]);
    expect(roomsStore.getState().total).toBe(5);
  });

  it("reset replaces without duplicating on re-subscribe", () => {
    roomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b")] }]));
    // A second Reset (e.g. StrictMode remount) must replace, not append.
    roomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b")] }]));
    expect(ids()).toEqual(["a", "b"]);
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

  it("pushFront prepends and pushBack appends", () => {
    roomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("b")] }]));
    roomsStore.getState().applyBatch(batch([{ op: "pushFront", room: room("a") }]));
    roomsStore.getState().applyBatch(batch([{ op: "pushBack", room: room("c") }]));
    expect(ids()).toEqual(["a", "b", "c"]);
  });

  it("popFront and popBack remove ends", () => {
    roomsStore
      .getState()
      .applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b"), room("c")] }]));
    roomsStore.getState().applyBatch(batch([{ op: "popFront" }]));
    roomsStore.getState().applyBatch(batch([{ op: "popBack" }]));
    expect(ids()).toEqual(["b"]);
  });

  it("insert splices at index", () => {
    roomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a"), room("c")] }]));
    roomsStore.getState().applyBatch(batch([{ op: "insert", index: 1, room: room("b") }]));
    expect(ids()).toEqual(["a", "b", "c"]);
  });

  it("set replaces at index in place (recency move to top)", () => {
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

  it("truncate shortens the list", () => {
    roomsStore
      .getState()
      .applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b"), room("c")] }]));
    roomsStore.getState().applyBatch(batch([{ op: "truncate", length: 1 }]));
    expect(ids()).toEqual(["a"]);
  });

  it("applies multiple ops in a single batch in sequence", () => {
    roomsStore.getState().applyBatch(
      batch([
        { op: "reset", rooms: [room("a"), room("b")] },
        { op: "pushFront", room: room("c") },
        { op: "remove", index: 2 },
      ]),
    );
    expect(ids()).toEqual(["c", "a"]);
  });

  it("does not sort — preserves the exact streamed order", () => {
    roomsStore
      .getState()
      .applyBatch(batch([{ op: "reset", rooms: [room("z"), room("a"), room("m")] }]));
    expect(ids()).toEqual(["z", "a", "m"]);
  });

  it("ignores set at an out-of-range index", () => {
    roomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b")] }]));
    roomsStore.getState().applyBatch(batch([{ op: "set", index: 5, room: room("z") }]));
    roomsStore.getState().applyBatch(batch([{ op: "set", index: -1, room: room("z") }]));
    expect(ids()).toEqual(["a", "b"]);
  });

  it("ignores remove at an out-of-range index", () => {
    roomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b")] }]));
    roomsStore.getState().applyBatch(batch([{ op: "remove", index: 5 }]));
    roomsStore.getState().applyBatch(batch([{ op: "remove", index: -1 }]));
    expect(ids()).toEqual(["a", "b"]);
  });

  it("ignores insert at an out-of-range index", () => {
    roomsStore.getState().applyBatch(batch([{ op: "reset", rooms: [room("a"), room("b")] }]));
    roomsStore.getState().applyBatch(batch([{ op: "insert", index: 5, room: room("z") }]));
    roomsStore.getState().applyBatch(batch([{ op: "insert", index: -1, room: room("z") }]));
    expect(ids()).toEqual(["a", "b"]);
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
  it("records the selected room id and clears it with null", () => {
    expect(roomsStore.getState().selectedRoomId).toBeNull();
    roomsStore.getState().selectRoom("!a:example.org");
    expect(roomsStore.getState().selectedRoomId).toBe("!a:example.org");
    roomsStore.getState().selectRoom(null);
    expect(roomsStore.getState().selectedRoomId).toBeNull();
  });
});
