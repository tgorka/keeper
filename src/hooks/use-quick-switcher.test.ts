import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { useQuickSwitcher } from "@/hooks/use-quick-switcher";
import type { InboxRoomVm } from "@/lib/ipc/client";
import { accountsStore } from "@/lib/stores/accounts";
import { archiveRoomsStore } from "@/lib/stores/archive-rooms";
import { composerStore } from "@/lib/stores/composer";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";

const ACC = "01ARZ3NDEKTSV4RRFFQ69G5FAV";
const ACC2 = "01BX5ZZKBKACTAV9WEVGEMMVRZ";

function room(roomId: string, accountId = ACC): InboxRoomVm {
  return {
    accountId,
    hueIndex: 0,
    roomId,
    displayName: roomId,
    lastMessage: "",
    timestamp: null,
    avatarUrl: null,
    isUnread: false,
    mentionCount: 0,
    isArchived: false,
    isPinned: false,
    isFavourite: false,
    network: null,
    networkId: null,
    muteState: "none",
  };
}

function press(opts: { ctrl?: boolean; shift?: boolean; meta?: boolean; alt?: boolean } = {}) {
  const event = new KeyboardEvent("keydown", {
    key: "Tab",
    ctrlKey: opts.ctrl ?? false,
    shiftKey: opts.shift ?? false,
    metaKey: opts.meta ?? false,
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

describe("useQuickSwitcher", () => {
  it("cycles to the next chat on ⌃Tab, focuses the composer, and preventDefaults", () => {
    roomsStore.setState({ rooms: [room("!a"), room("!b"), room("!c")] });
    roomsStore.getState().selectRoom({ accountId: ACC, roomId: "!a" });
    renderHook(() => useQuickSwitcher());
    const event = press({ ctrl: true });
    expect(roomsStore.getState().selected).toEqual({ accountId: ACC, roomId: "!b" });
    expect(composerStore.getState().focusNonce).toBe(1);
    expect(event.defaultPrevented).toBe(true);
  });

  it("cycles to the previous chat on ⌃⇧Tab", () => {
    roomsStore.setState({ rooms: [room("!a"), room("!b"), room("!c")] });
    roomsStore.getState().selectRoom({ accountId: ACC, roomId: "!b" });
    renderHook(() => useQuickSwitcher());
    press({ ctrl: true, shift: true });
    expect(roomsStore.getState().selected).toEqual({ accountId: ACC, roomId: "!a" });
  });

  it("wraps forward from the last row to the first", () => {
    roomsStore.setState({ rooms: [room("!a"), room("!b"), room("!c")] });
    roomsStore.getState().selectRoom({ accountId: ACC, roomId: "!c" });
    renderHook(() => useQuickSwitcher());
    press({ ctrl: true });
    expect(roomsStore.getState().selected).toEqual({ accountId: ACC, roomId: "!a" });
  });

  it("wraps backward from the first row to the last", () => {
    roomsStore.setState({ rooms: [room("!a"), room("!b"), room("!c")] });
    roomsStore.getState().selectRoom({ accountId: ACC, roomId: "!a" });
    renderHook(() => useQuickSwitcher());
    press({ ctrl: true, shift: true });
    expect(roomsStore.getState().selected).toEqual({ accountId: ACC, roomId: "!c" });
  });

  it("opens the first row when nothing is selected", () => {
    roomsStore.setState({ rooms: [room("!a"), room("!b")] });
    renderHook(() => useQuickSwitcher());
    press({ ctrl: true });
    expect(roomsStore.getState().selected).toEqual({ accountId: ACC, roomId: "!a" });
  });

  it("cycles the archive window when the archive view is active", () => {
    archiveRoomsStore.setState({ rooms: [room("!x"), room("!y")] });
    primaryViewStore.setState({ view: "archive" });
    roomsStore.getState().selectRoom({ accountId: ACC, roomId: "!x" });
    renderHook(() => useQuickSwitcher());
    press({ ctrl: true });
    expect(roomsStore.getState().selected).toEqual({ accountId: ACC, roomId: "!y" });
  });

  it("honors the account-switcher display filter", () => {
    roomsStore.setState({ rooms: [room("!a", ACC), room("!b", ACC2), room("!c", ACC)] });
    accountsStore.setState({ filterAccountId: ACC });
    roomsStore.getState().selectRoom({ accountId: ACC, roomId: "!a" });
    renderHook(() => useQuickSwitcher());
    press({ ctrl: true });
    // !b (other account) is filtered out, so the next visible row is !c.
    expect(roomsStore.getState().selected).toEqual({ accountId: ACC, roomId: "!c" });
  });

  it("no-ops on the bridges view", () => {
    roomsStore.setState({ rooms: [room("!a"), room("!b")] });
    primaryViewStore.setState({ view: "bridges" });
    renderHook(() => useQuickSwitcher());
    const event = press({ ctrl: true });
    expect(roomsStore.getState().selected).toBeNull();
    expect(event.defaultPrevented).toBe(false);
  });

  it("no-ops on an empty list", () => {
    primaryViewStore.setState({ view: "inbox" });
    renderHook(() => useQuickSwitcher());
    const event = press({ ctrl: true });
    expect(roomsStore.getState().selected).toBeNull();
    expect(event.defaultPrevented).toBe(false);
  });

  it("ignores a bare Tab (native focus traversal)", () => {
    roomsStore.setState({ rooms: [room("!a"), room("!b")] });
    renderHook(() => useQuickSwitcher());
    const event = press({});
    expect(roomsStore.getState().selected).toBeNull();
    expect(event.defaultPrevented).toBe(false);
  });

  it("ignores ⌘Tab (OS app switcher)", () => {
    roomsStore.setState({ rooms: [room("!a"), room("!b")] });
    renderHook(() => useQuickSwitcher());
    const event = press({ ctrl: true, meta: true });
    expect(roomsStore.getState().selected).toBeNull();
    expect(event.defaultPrevented).toBe(false);
  });
});
