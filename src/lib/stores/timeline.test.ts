import { afterEach, describe, expect, it } from "vitest";
import type {
  AgentRoomHeaderVm,
  ApprovalVm,
  TimelineBatch,
  TimelineItemVm,
  TimelineOp,
} from "@/lib/ipc/client";
import { timelineStore } from "@/lib/stores/timeline";

function message(key: string, sender = "@bob:example.org"): TimelineItemVm {
  return {
    kind: "message",
    key,
    sender,
    senderDisplayName: null,
    body: `body ${key}`,
    timestamp: 1,
    isOwn: false,
    sendState: null,
    isEdited: false,
    reply: null,
    reactions: [],
    media: null,
    readers: [],
    brief: null,
  };
}

function other(key: string): TimelineItemVm {
  return { kind: "other", key };
}

function batch(ops: TimelineOp[]): TimelineBatch {
  return { ops };
}

function keys(): string[] {
  return timelineStore.getState().items.map((i) => i.key);
}

afterEach(() => {
  timelineStore.getState().clear();
});

describe("timelineStore.applyBatch", () => {
  it("reset replaces contents", () => {
    timelineStore
      .getState()
      .applyBatch(batch([{ op: "reset", items: [message("a"), other("b")] }]));
    expect(keys()).toEqual(["a", "b"]);
  });

  it("reset replaces without duplicating on re-subscribe", () => {
    timelineStore.getState().applyBatch(batch([{ op: "reset", items: [message("a")] }]));
    // A second Reset (StrictMode remount / room re-open) must replace, not append.
    timelineStore.getState().applyBatch(batch([{ op: "reset", items: [message("a")] }]));
    expect(keys()).toEqual(["a"]);
  });

  it("pushBack appends a new item (live incoming message)", () => {
    timelineStore.getState().applyBatch(batch([{ op: "reset", items: [message("a")] }]));
    timelineStore.getState().applyBatch(batch([{ op: "pushBack", item: message("b") }]));
    expect(keys()).toEqual(["a", "b"]);
  });

  it("insert and set operate by index", () => {
    timelineStore
      .getState()
      .applyBatch(batch([{ op: "reset", items: [message("a"), message("c")] }]));
    timelineStore.getState().applyBatch(batch([{ op: "insert", index: 1, item: message("b") }]));
    expect(keys()).toEqual(["a", "b", "c"]);
    timelineStore.getState().applyBatch(batch([{ op: "set", index: 0, item: message("z") }]));
    expect(keys()).toEqual(["z", "b", "c"]);
  });

  it("remove and truncate shrink the list", () => {
    timelineStore
      .getState()
      .applyBatch(batch([{ op: "reset", items: [message("a"), message("b"), message("c")] }]));
    timelineStore.getState().applyBatch(batch([{ op: "remove", index: 1 }]));
    expect(keys()).toEqual(["a", "c"]);
    timelineStore.getState().applyBatch(batch([{ op: "truncate", length: 1 }]));
    expect(keys()).toEqual(["a"]);
  });

  it("applies multiple ops in a single batch in sequence", () => {
    timelineStore.getState().applyBatch(
      batch([
        { op: "reset", items: [message("a")] },
        { op: "pushBack", item: message("b") },
        { op: "pushFront", item: message("c") },
      ]),
    );
    expect(keys()).toEqual(["c", "a", "b"]);
  });

  it("does not sort — preserves the exact streamed order", () => {
    timelineStore
      .getState()
      .applyBatch(batch([{ op: "reset", items: [message("z"), message("a"), message("m")] }]));
    expect(keys()).toEqual(["z", "a", "m"]);
  });

  it("clear empties the timeline", () => {
    timelineStore.getState().applyBatch(batch([{ op: "reset", items: [message("a")] }]));
    timelineStore.getState().clear();
    expect(timelineStore.getState().items).toEqual([]);
  });
});

describe("timelineStore header", () => {
  const header = (caretKey: string | null): AgentRoomHeaderVm => ({
    status: null,
    scope: null,
    label: null,
    scopeUnreadable: null,
    caretKey,
  });

  it("keeps the last header through batches that carry none, and replaces it when one does", () => {
    timelineStore
      .getState()
      .applyBatch({ ops: [{ op: "reset", items: [message("a")] }], header: header("a") });
    timelineStore.getState().applyBatch(batch([{ op: "pushBack", item: message("b") }]));
    expect(timelineStore.getState().header).toEqual(header("a"));
    // A header arriving alone is a batch with no ops.
    timelineStore.getState().applyBatch({ ops: [], header: header(null) });
    expect(timelineStore.getState().header).toEqual(header(null));
    expect(keys()).toEqual(["a", "b"]);
  });

  it("clear drops the header with the room", () => {
    timelineStore.getState().applyBatch({ ops: [], header: header("a") });
    timelineStore.getState().clear();
    expect(timelineStore.getState().header).toBeNull();
  });
});

describe("timelineStore approvals", () => {
  const approvals = (state: "pending" | "consumed"): ApprovalVm[] => [
    { id: "01A", cards: [{ id: "01A", state: { state } } as ApprovalVm["cards"][number]] },
  ];

  it("keeps the last cards through batches that carry none, and the room's clear drops them", () => {
    timelineStore.getState().applyBatch({
      ops: [{ op: "reset", items: [{ kind: "approval", key: "k", id: "01A" }] }],
      approvals: approvals("pending"),
    });
    timelineStore.getState().applyBatch(batch([{ op: "pushBack", item: message("b") }]));
    expect(timelineStore.getState().approvals).toEqual(approvals("pending"));
    // A change beside the stream arrives as a batch with no ops.
    timelineStore.getState().applyBatch({ ops: [], approvals: approvals("consumed") });
    expect(timelineStore.getState().approvals).toEqual(approvals("consumed"));
    expect(keys()).toEqual(["k", "b"]);
    timelineStore.getState().clear();
    expect(timelineStore.getState().approvals).toEqual([]);
  });
});
