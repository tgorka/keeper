import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { useUnreadJump } from "@/hooks/use-unread-jump";
import type { InboxRoomVm } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { archiveRoomsStore } from "@/lib/stores/archive-rooms";
import { composerStore } from "@/lib/stores/composer";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";

const ACC = "01ARZ3NDEKTSV4RRFFQ69G5FAV";

function room(roomId: string, isUnread = false, accountId = ACC): InboxRoomVm {
  return {
    accountId,
    hueIndex: 0,
    roomId,
    displayName: roomId,
    lastMessage: "",
    timestamp: null,
    avatarUrl: null,
    isUnread,
    mentionCount: 0,
    isArchived: false,
    isPinned: false,
    isFavourite: false,
    network: null,
    networkId: null,
    muteState: "none",
  };
}

function press(
  key: "ArrowDown" | "ArrowUp",
  opts: { meta?: boolean; ctrl?: boolean; alt?: boolean } = {},
) {
  const event = new KeyboardEvent("keydown", {
    key,
    metaKey: opts.meta ?? false,
    ctrlKey: opts.ctrl ?? false,
    altKey: opts.alt ?? false,
    bubbles: true,
    cancelable: true,
  });
  act(() => {
    window.dispatchEvent(event);
  });
  return event;
}

beforeEach(() => {
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  archiveRoomsStore.getState().clear();
  accountsStore.getState().clear();
  accountsStore.setState({ filterAccountId: null });
  primaryViewStore.setState({ view: "inbox" });
  composerStore.setState({ focusNonce: 0 });
});

afterEach(() => {
  roomsStore.getState().clear();
  roomsStore.getState().selectRoom(null);
  archiveRoomsStore.getState().clear();
  accountsStore.getState().clear();
  accountsStore.setState({ filterAccountId: null });
  primaryViewStore.setState({ view: "inbox" });
});

describe("useUnreadJump", () => {
  it("selects + opens the next unread on ⌥⌘↓, focuses composer, preventDefaults", () => {
    // a(read) b(unread) c(read) d(unread), start on a.
    roomsStore.setState({
      rooms: [room("!a"), room("!b", true), room("!c"), room("!d", true)],
    });
    roomsStore.getState().selectRoom({ accountId: ACC, roomId: "!a" });
    renderHook(() => useUnreadJump());
    const event = press("ArrowDown", { meta: true, alt: true });
    expect(roomsStore.getState().selected).toEqual({ accountId: ACC, roomId: "!b" });
    expect(composerStore.getState().focusNonce).toBe(1);
    expect(event.defaultPrevented).toBe(true);
  });

  it("jumps to the previous unread on ⌥⌘↑", () => {
    roomsStore.setState({
      rooms: [room("!a", true), room("!b"), room("!c", true), room("!d")],
    });
    roomsStore.getState().selectRoom({ accountId: ACC, roomId: "!c" });
    renderHook(() => useUnreadJump());
    press("ArrowUp", { meta: true, alt: true });
    expect(roomsStore.getState().selected).toEqual({ accountId: ACC, roomId: "!a" });
  });

  it("wraps forward past the end back to an earlier unread", () => {
    roomsStore.setState({ rooms: [room("!a", true), room("!b"), room("!c")] });
    roomsStore.getState().selectRoom({ accountId: ACC, roomId: "!c" });
    renderHook(() => useUnreadJump());
    press("ArrowDown", { meta: true, alt: true });
    expect(roomsStore.getState().selected).toEqual({ accountId: ACC, roomId: "!a" });
  });

  it("respects the optimistic-unread overlay", () => {
    // No authoritative unread rows, but the overlay marks !b unread.
    roomsStore.setState({ rooms: [room("!a"), room("!b"), room("!c")] });
    roomsStore.getState().setOptimisticUnread(ACC, "!b", true);
    roomsStore.getState().selectRoom({ accountId: ACC, roomId: "!a" });
    renderHook(() => useUnreadJump());
    press("ArrowDown", { meta: true, alt: true });
    expect(roomsStore.getState().selected).toEqual({ accountId: ACC, roomId: "!b" });
  });

  it("works via ⌥⌃ (non-mac parity)", () => {
    roomsStore.setState({ rooms: [room("!a"), room("!b", true)] });
    roomsStore.getState().selectRoom({ accountId: ACC, roomId: "!a" });
    renderHook(() => useUnreadJump());
    press("ArrowDown", { ctrl: true, alt: true });
    expect(roomsStore.getState().selected).toEqual({ accountId: ACC, roomId: "!b" });
  });

  it("no-ops when there are no unread rows (selection unchanged)", () => {
    roomsStore.setState({ rooms: [room("!a"), room("!b")] });
    roomsStore.getState().selectRoom({ accountId: ACC, roomId: "!a" });
    renderHook(() => useUnreadJump());
    const event = press("ArrowUp", { meta: true, alt: true });
    expect(roomsStore.getState().selected).toEqual({ accountId: ACC, roomId: "!a" });
    expect(event.defaultPrevented).toBe(false);
  });

  it("no-ops on the approval view", () => {
    roomsStore.setState({ rooms: [room("!a", true)] });
    primaryViewStore.setState({ view: "approval" });
    renderHook(() => useUnreadJump());
    const event = press("ArrowDown", { meta: true, alt: true });
    expect(roomsStore.getState().selected).toBeNull();
    expect(event.defaultPrevented).toBe(false);
  });

  it("ignores ↑/↓ without the ⌥ modifier (falls through to list/timeline)", () => {
    roomsStore.setState({ rooms: [room("!a"), room("!b", true)] });
    roomsStore.getState().selectRoom({ accountId: ACC, roomId: "!a" });
    renderHook(() => useUnreadJump());
    const event = press("ArrowDown", { meta: true });
    expect(roomsStore.getState().selected).toEqual({ accountId: ACC, roomId: "!a" });
    expect(event.defaultPrevented).toBe(false);
  });
});
