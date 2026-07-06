import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it } from "vitest";
import { draftsStore, useHasDraft, useRemoteDraft } from "@/lib/stores/drafts";

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

describe("draftsStore remote mirror (Story 7.2)", () => {
  it("starts with an empty remote map", () => {
    expect(draftsStore.getState().remote.size).toBe(0);
  });

  it("applyRemote sets a remote draft body + timestamp for a key", () => {
    draftsStore.getState().applyRemote("acctA", "!r1", "from device B", 100);
    const entry = draftsStore.getState().remote.get("acctA !r1");
    expect(entry).toEqual({ body: "from device B", updatedTs: 100 });
  });

  it("applyRemote with a null/empty body (tombstone) removes the key", () => {
    draftsStore.getState().applyRemote("acctA", "!r1", "text", 1);
    expect(draftsStore.getState().remote.has("acctA !r1")).toBe(true);
    // A null body tombstones the remote draft.
    draftsStore.getState().applyRemote("acctA", "!r1", null, 2);
    expect(draftsStore.getState().remote.has("acctA !r1")).toBe(false);
    // An empty-string body is likewise a tombstone.
    draftsStore.getState().applyRemote("acctA", "!r1", "text", 3);
    draftsStore.getState().applyRemote("acctA", "!r1", "", 4);
    expect(draftsStore.getState().remote.has("acctA !r1")).toBe(false);
  });

  it("applyRemote does not churn state when the body is unchanged (dedupe echo)", () => {
    draftsStore.getState().applyRemote("acctA", "!r1", "same", 1);
    const before = draftsStore.getState().remote;
    // A re-apply carrying only a newer timestamp keeps the same map reference.
    draftsStore.getState().applyRemote("acctA", "!r1", "same", 999);
    expect(draftsStore.getState().remote).toBe(before);
  });

  it("clear empties the remote map too", () => {
    draftsStore.getState().applyRemote("acctA", "!r1", "text", 1);
    draftsStore.getState().clear();
    expect(draftsStore.getState().remote.size).toBe(0);
  });

  it("useRemoteDraft reflects the offered remote draft and updates on applyRemote", () => {
    const { result } = renderHook(() => useRemoteDraft("acctA", "!r1"));
    expect(result.current).toBeUndefined();

    act(() => {
      draftsStore.getState().applyRemote("acctA", "!r1", "remote text", 42);
    });
    expect(result.current).toEqual({ body: "remote text", updatedTs: 42 });

    act(() => {
      draftsStore.getState().applyRemote("acctA", "!r1", null, 43);
    });
    expect(result.current).toBeUndefined();
  });

  it("useRemoteDraft isolates unrelated keys (no cross re-render)", () => {
    const { result } = renderHook(() => useRemoteDraft("acctA", "!r1"));
    act(() => {
      draftsStore.getState().applyRemote("acctB", "!other", "not mine", 1);
    });
    expect(result.current).toBeUndefined();
  });

  it("clearAccount drops only the signed-out account's markers and remote bodies", () => {
    draftsStore.getState().mark("acctA", "!r1", true);
    draftsStore.getState().applyRemote("acctA", "!r1", "A secret draft", 1);
    draftsStore.getState().mark("acctB", "!r2", true);
    draftsStore.getState().applyRemote("acctB", "!r2", "B draft", 2);

    draftsStore.getState().clearAccount("acctA");

    // acctA's marker and mirrored body are gone (no signed-out text lingering).
    expect(draftsStore.getState().keys.has("acctA !r1")).toBe(false);
    expect(draftsStore.getState().remote.has("acctA !r1")).toBe(false);
    // acctB is untouched (cross-account isolation).
    expect(draftsStore.getState().keys.has("acctB !r2")).toBe(true);
    expect(draftsStore.getState().remote.get("acctB !r2")).toEqual({
      body: "B draft",
      updatedTs: 2,
    });
  });

  it("clearAccount is a no-op (same references) when the account has no entries", () => {
    draftsStore.getState().mark("acctB", "!r2", true);
    draftsStore.getState().applyRemote("acctB", "!r2", "B draft", 2);
    const keysBefore = draftsStore.getState().keys;
    const remoteBefore = draftsStore.getState().remote;
    draftsStore.getState().clearAccount("acctA");
    expect(draftsStore.getState().keys).toBe(keysBefore);
    expect(draftsStore.getState().remote).toBe(remoteBefore);
  });
});
