import { afterEach, describe, expect, it } from "vitest";
import type { SpaceVm } from "@/lib/ipc/client";
import { spacesStore } from "@/lib/stores/spaces";

function space(spaceId: string, overrides: Partial<SpaceVm> = {}): SpaceVm {
  return {
    accountId: "acctA",
    spaceId,
    name: spaceId,
    avatarUrl: null,
    ...overrides,
  };
}

afterEach(() => {
  spacesStore.getState().clear();
});

describe("spacesStore", () => {
  it("applySnapshot replaces the list wholesale", () => {
    spacesStore.getState().applySnapshot({ spaces: [space("!a"), space("!b")] });
    expect(spacesStore.getState().spaces.map((s) => s.spaceId)).toEqual(["!a", "!b"]);

    // A second snapshot replaces the list (no diff/merge).
    spacesStore.getState().applySnapshot({ spaces: [space("!c")] });
    expect(spacesStore.getState().spaces.map((s) => s.spaceId)).toEqual(["!c"]);
  });

  it("applySnapshot does not touch the active selection", () => {
    spacesStore.getState().setActiveSpace({ accountId: "acctA", spaceId: "!a" });
    spacesStore.getState().applySnapshot({ spaces: [space("!a")] });
    expect(spacesStore.getState().activeSpace).toEqual({ accountId: "acctA", spaceId: "!a" });
  });

  it("setActiveSpace records and clears the selection", () => {
    spacesStore.getState().setActiveSpace({ accountId: "acctB", spaceId: "!x" });
    expect(spacesStore.getState().activeSpace).toEqual({ accountId: "acctB", spaceId: "!x" });

    spacesStore.getState().setActiveSpace(null);
    expect(spacesStore.getState().activeSpace).toBeNull();
  });

  it("clear resets both the list and the selection", () => {
    spacesStore.getState().applySnapshot({ spaces: [space("!a")] });
    spacesStore.getState().setActiveSpace({ accountId: "acctA", spaceId: "!a" });

    spacesStore.getState().clear();
    expect(spacesStore.getState().spaces).toEqual([]);
    expect(spacesStore.getState().activeSpace).toBeNull();
  });
});
