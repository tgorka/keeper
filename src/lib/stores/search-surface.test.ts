import { beforeEach, describe, expect, it } from "vitest";
import { searchSurfaceStore } from "@/lib/stores/search-surface";

beforeEach(() => {
  searchSurfaceStore.setState({ isOpen: false, scope: "chats", chatLock: null });
});

describe("searchSurfaceStore", () => {
  it("defaults to closed, Chats scope, no lock", () => {
    const state = searchSurfaceStore.getState();
    expect(state.isOpen).toBe(false);
    expect(state.scope).toBe("chats");
    expect(state.chatLock).toBeNull();
  });

  it("open() with no options opens in Chats scope with no lock", () => {
    searchSurfaceStore.getState().open();
    const state = searchSurfaceStore.getState();
    expect(state.isOpen).toBe(true);
    expect(state.scope).toBe("chats");
    expect(state.chatLock).toBeNull();
  });

  it("open() with a scope opens in that scope", () => {
    searchSurfaceStore.getState().open({ scope: "actions" });
    const state = searchSurfaceStore.getState();
    expect(state.isOpen).toBe(true);
    expect(state.scope).toBe("actions");
    expect(state.chatLock).toBeNull();
  });

  it("open() with a chatLock opens locked (in Messages scope)", () => {
    const lock = { accountId: "acc-a", roomId: "!r:example.org" };
    searchSurfaceStore.getState().open({ scope: "messages", chatLock: lock });
    const state = searchSurfaceStore.getState();
    expect(state.isOpen).toBe(true);
    expect(state.scope).toBe("messages");
    expect(state.chatLock).toEqual(lock);
  });

  it("close() clears the lock and closes", () => {
    searchSurfaceStore.getState().open({
      scope: "messages",
      chatLock: { accountId: "acc-a", roomId: "!r:example.org" },
    });
    searchSurfaceStore.getState().close();
    const state = searchSurfaceStore.getState();
    expect(state.isOpen).toBe(false);
    expect(state.chatLock).toBeNull();
  });
});
