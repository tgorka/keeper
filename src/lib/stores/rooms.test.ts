import { afterEach, describe, expect, it } from "vitest";
import type { InboxBatch, InboxOp, InboxRoomVm } from "@/lib/ipc/client";
import { effectiveIsUnread, roomsStore, unreadOverrideKey } from "@/lib/stores/rooms";

function room(
  id: string,
  accountId = "acctA",
  hue = 0,
  overrides: Partial<InboxRoomVm> = {},
): InboxRoomVm {
  return {
    accountId,
    hueIndex: hue,
    roomId: id,
    displayName: id,
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

describe("roomsStore optimistic-unread overlay", () => {
  it("effectiveIsUnread: override wins over the authoritative value", () => {
    const r = room("a", "acctA", 0, { isUnread: false });
    roomsStore.getState().setOptimisticUnread("acctA", "a", true);
    const overlay = roomsStore.getState().optimisticUnread;
    expect(effectiveIsUnread(r, overlay)).toBe(true);
    expect(overlay.get(unreadOverrideKey("acctA", "a"))).toBe(true);
  });

  it("effectiveIsUnread: falls back to the room's isUnread with no override", () => {
    const r = room("a", "acctA", 0, { isUnread: true });
    expect(effectiveIsUnread(r, roomsStore.getState().optimisticUnread)).toBe(true);
  });

  it("setOptimisticUnread keys by accountId|roomId (does not collide across accounts)", () => {
    roomsStore.getState().setOptimisticUnread("acctA", "a", true);
    roomsStore.getState().setOptimisticUnread("acctB", "a", false);
    const overlay = roomsStore.getState().optimisticUnread;
    expect(overlay.get(unreadOverrideKey("acctA", "a"))).toBe(true);
    expect(overlay.get(unreadOverrideKey("acctB", "a"))).toBe(false);
  });

  it("applyBatch drops an override once the streamed VM matches the intended value", () => {
    // Row starts read; user marks it unread optimistically.
    roomsStore
      .getState()
      .applyBatch(batch([{ op: "reset", rooms: [room("a", "acctA", 0, { isUnread: false })] }]));
    roomsStore.getState().setOptimisticUnread("acctA", "a", true);
    expect(roomsStore.getState().optimisticUnread.size).toBe(1);

    // A non-matching stream update keeps the override in place.
    roomsStore
      .getState()
      .applyBatch(
        batch([{ op: "set", index: 0, room: room("a", "acctA", 0, { isUnread: false }) }]),
      );
    expect(roomsStore.getState().optimisticUnread.size).toBe(1);

    // The authoritative VM converges to isUnread=true → override is dropped.
    roomsStore
      .getState()
      .applyBatch(
        batch([{ op: "set", index: 0, room: room("a", "acctA", 0, { isUnread: true }) }]),
      );
    expect(roomsStore.getState().optimisticUnread.size).toBe(0);
  });

  it("applyBatch leaves an override in place while the stream still disagrees", () => {
    roomsStore
      .getState()
      .applyBatch(batch([{ op: "reset", rooms: [room("a", "acctA", 0, { isUnread: true })] }]));
    // User marks read optimistically; stream still reports unread.
    roomsStore.getState().setOptimisticUnread("acctA", "a", false);
    roomsStore
      .getState()
      .applyBatch(
        batch([{ op: "set", index: 0, room: room("a", "acctA", 0, { isUnread: true }) }]),
      );
    expect(roomsStore.getState().optimisticUnread.get(unreadOverrideKey("acctA", "a"))).toBe(false);
    // The row still renders read via the overlay despite the streamed unread.
    expect(
      effectiveIsUnread(roomsStore.getState().rooms[0], roomsStore.getState().optimisticUnread),
    ).toBe(false);
  });

  it("clear() empties the optimistic-unread overlay", () => {
    roomsStore.getState().setOptimisticUnread("acctA", "a", true);
    roomsStore.getState().clear();
    expect(roomsStore.getState().optimisticUnread.size).toBe(0);
  });

  it("clearOptimisticUnread drops a single override (revert on command rejection)", () => {
    roomsStore.getState().setOptimisticUnread("acctA", "a", false);
    roomsStore.getState().setOptimisticUnread("acctB", "b", true);
    roomsStore.getState().clearOptimisticUnread("acctA", "a");
    expect(roomsStore.getState().optimisticUnread.has(unreadOverrideKey("acctA", "a"))).toBe(false);
    // Unrelated overrides are untouched.
    expect(roomsStore.getState().optimisticUnread.get(unreadOverrideKey("acctB", "b"))).toBe(true);
  });

  it("clearOptimisticUnread on an absent key is a no-op (keeps the same map)", () => {
    roomsStore.getState().setOptimisticUnread("acctA", "a", true);
    const before = roomsStore.getState().optimisticUnread;
    roomsStore.getState().clearOptimisticUnread("acctZ", "z");
    // Same instance — no needless update when nothing changed.
    expect(roomsStore.getState().optimisticUnread).toBe(before);
  });

  it("applyBatch drops an override once its room leaves the streamed window", () => {
    roomsStore
      .getState()
      .applyBatch(batch([{ op: "reset", rooms: [room("a", "acctA", 0, { isUnread: true })] }]));
    // User marks read optimistically; stream still reports unread.
    roomsStore.getState().setOptimisticUnread("acctA", "a", false);
    // The room is then removed from the window (archived / left elsewhere).
    roomsStore.getState().applyBatch(batch([{ op: "remove", index: 0 }]));
    // The override can no longer reconcile, so it is dropped rather than leaking.
    expect(roomsStore.getState().optimisticUnread.has(unreadOverrideKey("acctA", "a"))).toBe(false);
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
