import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  cancelHeldSend: vi.fn(() => Promise.resolve("")),
}));

import type { HeldSendVm } from "@/lib/ipc/client";
import { cancelHeldSend } from "@/lib/ipc/client";
import { composerStore } from "@/lib/stores/composer";
import { outboxStore, undoHeldSend, useHeldSends } from "@/lib/stores/outbox";

const mockCancel = vi.mocked(cancelHeldSend);

afterEach(() => {
  outboxStore.getState().clear();
});

function held(id: string, roomId: string, dispatchAtMs: number): HeldSendVm {
  return {
    id,
    accountId: "acctA",
    roomId,
    body: `body-${id}`,
    heldAtMs: dispatchAtMs - 10_000,
    dispatchAtMs,
  };
}

describe("outboxStore", () => {
  it("starts empty", () => {
    expect(outboxStore.getState().rooms.size).toBe(0);
  });

  it("applySnapshot replaces a room's rows wholesale", () => {
    outboxStore.getState().applySnapshot("acctA", "!r1", [held("id1", "!r1", 100)]);
    expect(outboxStore.getState().rooms.get("acctA !r1")?.length).toBe(1);

    // A second snapshot REPLACES (does not merge) — three rows now.
    const rows = [held("id1", "!r1", 100), held("id2", "!r1", 200), held("id3", "!r1", 300)];
    outboxStore.getState().applySnapshot("acctA", "!r1", rows);
    const stored = outboxStore.getState().rooms.get("acctA !r1");
    expect(stored?.map((r) => r.id)).toEqual(["id1", "id2", "id3"]);
  });

  it("an empty snapshot clears the room's entry", () => {
    outboxStore.getState().applySnapshot("acctA", "!r1", [held("id1", "!r1", 100)]);
    expect(outboxStore.getState().rooms.has("acctA !r1")).toBe(true);

    outboxStore.getState().applySnapshot("acctA", "!r1", []);
    expect(outboxStore.getState().rooms.has("acctA !r1")).toBe(false);
  });

  it("an empty snapshot for an absent room does not churn state", () => {
    const before = outboxStore.getState().rooms;
    outboxStore.getState().applySnapshot("acctA", "!r9", []);
    expect(outboxStore.getState().rooms).toBe(before);
  });

  it("keeps rooms independent", () => {
    outboxStore.getState().applySnapshot("acctA", "!r1", [held("id1", "!r1", 100)]);
    outboxStore.getState().applySnapshot("acctA", "!r2", [held("id2", "!r2", 200)]);
    outboxStore.getState().applySnapshot("acctA", "!r1", []);
    // Clearing !r1 leaves !r2 untouched.
    expect(outboxStore.getState().rooms.has("acctA !r1")).toBe(false);
    expect(outboxStore.getState().rooms.get("acctA !r2")?.length).toBe(1);
  });

  it("useHeldSends returns a stable empty array for a room with none", () => {
    const { result } = renderHook(() => useHeldSends("acctA", "!none"));
    expect(result.current).toEqual([]);
    const first = result.current;
    // A snapshot to an unrelated room does not change this room's referential value.
    act(() => {
      outboxStore.getState().applySnapshot("acctA", "!other", [held("id1", "!other", 100)]);
    });
    expect(result.current).toBe(first);
  });

  it("useHeldSends reflects the room's held rows and updates on snapshot", () => {
    const { result } = renderHook(() => useHeldSends("acctA", "!r1"));
    expect(result.current).toEqual([]);
    act(() => {
      outboxStore.getState().applySnapshot("acctA", "!r1", [held("id1", "!r1", 100)]);
    });
    expect(result.current.map((r) => r.id)).toEqual(["id1"]);
    act(() => {
      outboxStore.getState().applySnapshot("acctA", "!r1", []);
    });
    expect(result.current).toEqual([]);
  });

  it("clear resets everything", () => {
    outboxStore.getState().applySnapshot("acctA", "!r1", [held("id1", "!r1", 100)]);
    outboxStore.getState().clear();
    expect(outboxStore.getState().rooms.size).toBe(0);
  });

  it("keeps a room's holds while another conversation still shows them (U2)", () => {
    // The chat and the notes dock both show !r1; the dock closes.
    outboxStore.getState().hold("acctA", "!r1");
    outboxStore.getState().hold("acctA", "!r1");
    outboxStore.getState().applySnapshot("acctA", "!r1", [held("id1", "!r1", 100)]);
    outboxStore.getState().release("acctA", "!r1");
    expect(
      outboxStore
        .getState()
        .rooms.get("acctA !r1")
        ?.map((r) => r.id),
    ).toEqual(["id1"]);
    // The last one closes: the rows go.
    outboxStore.getState().release("acctA", "!r1");
    expect(outboxStore.getState().rooms.has("acctA !r1")).toBe(false);
  });
});

describe("undoHeldSend (shared undo effect, Story 8.4)", () => {
  beforeEach(() => {
    mockCancel.mockClear();
    mockCancel.mockResolvedValue("");
    composerStore.getState().clear();
    composerStore.setState({ restoreBody: null, restoreTarget: null, restoreNonce: 0 });
  });

  it("cancels the held send and restores a non-empty returned body to the composer", async () => {
    mockCancel.mockResolvedValue("restored body");
    await undoHeldSend("acctA", "!r1", "id1", composerStore);
    expect(mockCancel).toHaveBeenCalledWith("acctA", "!r1", "id1");
    expect(composerStore.getState().restoreBody).toBe("restored body");
    // The restore is scoped to the originating chat (never lands in another room).
    expect(composerStore.getState().restoreTarget).toEqual({ accountId: "acctA", roomId: "!r1" });
  });

  it("does not restore when the cancel returns an empty body (already dispatched)", async () => {
    mockCancel.mockResolvedValue("");
    await undoHeldSend("acctA", "!r1", "id1", composerStore);
    expect(mockCancel).toHaveBeenCalledWith("acctA", "!r1", "id1");
    expect(composerStore.getState().restoreBody).toBeNull();
  });

  it("swallows a cancel rejection and does not restore (mirrors the pill)", async () => {
    mockCancel.mockRejectedValue(new Error("boom"));
    await expect(undoHeldSend("acctA", "!r1", "id1", composerStore)).resolves.toBeUndefined();
    expect(composerStore.getState().restoreBody).toBeNull();
  });
});
