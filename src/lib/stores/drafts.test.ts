import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { draftsStore, useHasDraft } from "@/lib/stores/drafts";

afterEach(() => {
  draftsStore.getState().clear();
});

describe("draftsStore", () => {
  it("starts empty", () => {
    expect(draftsStore.getState().keys.size).toBe(0);
  });

  it("marks and unmarks a chat's presence", () => {
    draftsStore.getState().mark("acctA", "!r1", true);
    expect(draftsStore.getState().keys.has("acctA !r1")).toBe(true);

    draftsStore.getState().mark("acctA", "!r1", false);
    expect(draftsStore.getState().keys.has("acctA !r1")).toBe(false);
  });

  it("mark is idempotent and does not churn state when unchanged", () => {
    draftsStore.getState().mark("acctA", "!r1", true);
    const before = draftsStore.getState().keys;
    // Re-marking present when already present returns the same set reference.
    draftsStore.getState().mark("acctA", "!r1", true);
    expect(draftsStore.getState().keys).toBe(before);
    // Removing an absent key is likewise a no-op.
    draftsStore.getState().mark("acctB", "!r9", false);
    expect(draftsStore.getState().keys).toBe(before);
  });

  it("applyKeys replaces the whole set wholesale (cross-account)", () => {
    draftsStore.getState().mark("acctA", "!stale", true);
    draftsStore.getState().applyKeys([
      ["acctA", "!r1"],
      ["acctB", "!r2"],
    ]);
    const { keys } = draftsStore.getState();
    expect(keys.has("acctA !r1")).toBe(true);
    expect(keys.has("acctB !r2")).toBe(true);
    // The pre-seed marker is gone (wholesale replace).
    expect(keys.has("acctA !stale")).toBe(false);
  });

  it("clear empties the set", () => {
    draftsStore.getState().mark("acctA", "!r1", true);
    draftsStore.getState().clear();
    expect(draftsStore.getState().keys.size).toBe(0);
  });

  it("useHasDraft reflects presence and updates on mark", () => {
    const { result } = renderHook(() => useHasDraft("acctA", "!r1"));
    expect(result.current).toBe(false);

    act(() => {
      draftsStore.getState().mark("acctA", "!r1", true);
    });
    expect(result.current).toBe(true);

    act(() => {
      draftsStore.getState().mark("acctA", "!r1", false);
    });
    expect(result.current).toBe(false);
  });
});
