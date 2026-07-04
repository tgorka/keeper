import { afterEach, describe, expect, it } from "vitest";
import type { TimelineBatch, TimelineItemVm, TimelineOp } from "@/lib/ipc/client";
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
