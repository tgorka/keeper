import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

vi.mock("@/lib/ipc/client", () => ({
  cancelHeldSend: vi.fn(() => Promise.resolve({ line: "", reply: null, attachment: null })),
}));

import type { HeldRestoreVm, HeldSendVm } from "@/lib/ipc/client";
import { cancelHeldSend } from "@/lib/ipc/client";
import { attachmentsStore } from "@/lib/stores/attachments";
import { composerStore } from "@/lib/stores/composer";
import {
  outboxStore,
  REPLY_UNSHOWN_SENDER,
  undoHeldSend,
  useHeldRestores,
  useHeldSends,
} from "@/lib/stores/outbox";
import { timelineStore } from "@/lib/stores/timeline";

const STORES = { composer: composerStore, timeline: timelineStore, attachments: attachmentsStore };
const restored = (over: Partial<HeldRestoreVm> = {}): HeldRestoreVm => ({
  line: "",
  reply: null,
  attachment: null,
  ...over,
});

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
    filename: null,
    reply: false,
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
  // The conversation the undo is pressed in shows the held send's room.
  let shown: { unmount: () => void } | null = null;
  afterEach(() => {
    shown?.unmount();
    shown = null;
  });
  beforeEach(() => {
    shown = renderHook(() => useHeldRestores("acctA", "!r1", STORES));
    mockCancel.mockClear();
    mockCancel.mockResolvedValue(restored());
    composerStore.getState().clear();
    composerStore.setState({ restoreBody: null, restoreTarget: null, restoreNonce: 0 });
    timelineStore.getState().clear();
    attachmentsStore.getState().clear();
  });

  it("gives a held reply back replying, and a held attachment back to the tray (R432)", async () => {
    timelineStore.setState({
      items: [
        {
          kind: "message",
          key: "k-original",
          sender: "@tola:example.org",
          senderDisplayName: "Tola Grey",
          body: "Which   trip?\nTatry or Mazury",
          timestamp: 1,
          isOwn: false,
          sendState: null,
          isEdited: false,
          reply: null,
          reactions: [],
          media: null,
          readers: [],
        },
      ],
    });
    mockCancel.mockResolvedValue(
      restored({
        line: "the view",
        reply: {
          eventId: "$original:example.org",
          key: "k-original",
          sender: "@tola:example.org",
          senderDisplayName: "Tola Grey",
          body: "Which   trip?\nTatry or Mazury",
        },
        attachment: { path: "/data/outbox-files/h1/photo.png", filename: "photo.png" },
      }),
    );
    await undoHeldSend("acctA", "!r1", "h1", STORES);
    expect(composerStore.getState().restoreBody).toBe("the view");
    expect(composerStore.getState().pending).toEqual({
      mode: "reply",
      targetKey: "k-original",
      eventId: "$original:example.org",
      sender: "Tola Grey",
      bodyPreview: "Which trip? Tatry or Mazury",
    });
    expect(attachmentsStore.getState().pending).toEqual([
      expect.objectContaining({
        kind: "path",
        path: "/data/outbox-files/h1/photo.png",
        filename: "photo.png",
      }),
    ]);
  });

  it("gives a reply back replying to its event when the timeline does not show the original (R447)", async () => {
    mockCancel.mockResolvedValue(
      restored({
        line: "Tatry first",
        reply: {
          eventId: "$unloaded:example.org",
          key: null,
          sender: "",
          senderDisplayName: null,
          body: "",
        },
      }),
    );
    await undoHeldSend("acctA", "!r1", "h1", STORES);
    expect(composerStore.getState().pending).toEqual({
      mode: "reply",
      targetKey: "event:$unloaded:example.org",
      eventId: "$unloaded:example.org",
      sender: REPLY_UNSHOWN_SENDER,
      bodyPreview: "",
    });
  });

  it("cancels the held send and restores a non-empty returned body to the composer", async () => {
    mockCancel.mockResolvedValue(restored({ line: "restored body" }));
    await undoHeldSend("acctA", "!r1", "id1", STORES);
    expect(mockCancel).toHaveBeenCalledWith("acctA", "!r1", "id1");
    expect(composerStore.getState().restoreBody).toBe("restored body");
    // The restore is scoped to the originating chat (never lands in another room).
    expect(composerStore.getState().restoreTarget).toEqual({ accountId: "acctA", roomId: "!r1" });
  });

  it("does not restore when the cancel returns an empty body (already dispatched)", async () => {
    mockCancel.mockResolvedValue(restored());
    await undoHeldSend("acctA", "!r1", "id1", STORES);
    expect(mockCancel).toHaveBeenCalledWith("acctA", "!r1", "id1");
    expect(composerStore.getState().restoreBody).toBeNull();
  });

  it("swallows a cancel rejection and does not restore (mirrors the pill)", async () => {
    mockCancel.mockRejectedValue(new Error("boom"));
    await expect(undoHeldSend("acctA", "!r1", "id1", STORES)).resolves.toBeUndefined();
    expect(composerStore.getState().restoreBody).toBeNull();
  });
});
