import { describe, expect, it } from "vitest";
import { applyDiffOp, type DiffOp } from "@/lib/stores/vector-diff";

// Exercise the reducer over string elements using the `item`/`items` payload
// keys. The `room`/`rooms` keys are covered end-to-end by the room-list store
// tests (rooms.test.ts).
function apply(arr: string[], op: DiffOp<string>): string[] {
  return applyDiffOp(arr, op);
}

describe("applyDiffOp", () => {
  it("reset replaces contents", () => {
    expect(apply(["x"], { op: "reset", items: ["a", "b"] })).toEqual(["a", "b"]);
  });

  it("reset replaces without duplicating on a second reset", () => {
    let arr = apply([], { op: "reset", items: ["a", "b"] });
    arr = apply(arr, { op: "reset", items: ["a", "b"] });
    expect(arr).toEqual(["a", "b"]);
  });

  it("append adds to the end in order", () => {
    expect(apply(["a"], { op: "append", items: ["b", "c"] })).toEqual(["a", "b", "c"]);
  });

  it("clear empties the list", () => {
    expect(apply(["a", "b"], { op: "clear" })).toEqual([]);
  });

  it("pushFront prepends and pushBack appends", () => {
    expect(apply(["b"], { op: "pushFront", item: "a" })).toEqual(["a", "b"]);
    expect(apply(["a"], { op: "pushBack", item: "b" })).toEqual(["a", "b"]);
  });

  it("popFront and popBack remove ends", () => {
    expect(apply(["a", "b", "c"], { op: "popFront" })).toEqual(["b", "c"]);
    expect(apply(["a", "b", "c"], { op: "popBack" })).toEqual(["a", "b"]);
  });

  it("insert splices at index", () => {
    expect(apply(["a", "c"], { op: "insert", index: 1, item: "b" })).toEqual(["a", "b", "c"]);
  });

  it("insert at the end (index === length) is allowed", () => {
    expect(apply(["a"], { op: "insert", index: 1, item: "b" })).toEqual(["a", "b"]);
  });

  it("set replaces at index in place", () => {
    expect(apply(["a", "b"], { op: "set", index: 0, item: "z" })).toEqual(["z", "b"]);
  });

  it("remove splices out an index", () => {
    expect(apply(["a", "b", "c"], { op: "remove", index: 1 })).toEqual(["a", "c"]);
  });

  it("truncate shortens the list", () => {
    expect(apply(["a", "b", "c"], { op: "truncate", length: 1 })).toEqual(["a"]);
  });

  it("ignores a negative truncate length rather than dropping from the end", () => {
    expect(apply(["a", "b", "c"], { op: "truncate", length: -1 })).toEqual(["a", "b", "c"]);
  });

  it("truncate to zero empties the list", () => {
    expect(apply(["a", "b"], { op: "truncate", length: 0 })).toEqual([]);
  });

  it("ignores a single-element op with a missing payload", () => {
    expect(apply(["a"], { op: "pushBack" })).toEqual(["a"]);
    expect(apply(["a"], { op: "insert", index: 1 })).toEqual(["a"]);
  });

  it("does not sort — preserves the exact streamed order", () => {
    expect(apply([], { op: "reset", items: ["z", "a", "m"] })).toEqual(["z", "a", "m"]);
  });

  it("ignores insert at an out-of-range index", () => {
    expect(apply(["a", "b"], { op: "insert", index: 5, item: "z" })).toEqual(["a", "b"]);
    expect(apply(["a", "b"], { op: "insert", index: -1, item: "z" })).toEqual(["a", "b"]);
  });

  it("ignores set at an out-of-range index", () => {
    expect(apply(["a", "b"], { op: "set", index: 5, item: "z" })).toEqual(["a", "b"]);
    expect(apply(["a", "b"], { op: "set", index: -1, item: "z" })).toEqual(["a", "b"]);
  });

  it("ignores remove at an out-of-range index", () => {
    expect(apply(["a", "b"], { op: "remove", index: 5 })).toEqual(["a", "b"]);
    expect(apply(["a", "b"], { op: "remove", index: -1 })).toEqual(["a", "b"]);
  });

  it("returns a new array (immutable) rather than mutating the input", () => {
    const input = ["a", "b"];
    const out = apply(input, { op: "pushBack", item: "c" });
    expect(input).toEqual(["a", "b"]);
    expect(out).not.toBe(input);
  });
});
