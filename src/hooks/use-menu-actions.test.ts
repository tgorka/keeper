import { act, renderHook, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { InboxRoomVm } from "@/lib/ipc/client";
import { roomsStore } from "@/lib/stores/rooms";

// Capture the registered event listener so the test can fire a menu-action event
// without a live Tauri backend, and stub the shared dispatch to assert routing.
type MenuHandler = (event: { payload: string }) => void;
let registered: MenuHandler | undefined;
const unlisten = vi.fn();

vi.mock("@tauri-apps/api/event", () => ({
  listen: (_event: string, handler: MenuHandler) => {
    registered = handler;
    return Promise.resolve(unlisten);
  },
}));

const dispatchPaletteAction = vi.fn().mockResolvedValue(undefined);
vi.mock("@/components/command-palette/actions", () => ({
  dispatchPaletteAction: (id: string, ctx: unknown) => dispatchPaletteAction(id, ctx),
}));

import { resolveMenuActionId, useMenuActions } from "@/hooks/use-menu-actions";

const ACC = "acc-a";

function room(roomId: string, flags: Partial<InboxRoomVm> = {}): InboxRoomVm {
  return {
    accountId: ACC,
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
    ...flags,
  };
}

beforeEach(() => {
  registered = undefined;
  unlisten.mockClear();
  dispatchPaletteAction.mockClear();
  roomsStore.getState().clear();
  roomsStore.setState({ selected: null });
});

afterEach(() => {
  roomsStore.getState().clear();
  roomsStore.setState({ selected: null });
});

describe("resolveMenuActionId", () => {
  it("passes non-toggle ids through unchanged", () => {
    expect(resolveMenuActionId("open-inbox")).toBe("open-inbox");
    expect(resolveMenuActionId("new-chat")).toBe("new-chat");
  });

  it("keeps the canonical id when no chat is open", () => {
    roomsStore.setState({ selected: null });
    expect(resolveMenuActionId("archive-chat")).toBe("archive-chat");
  });

  it("flips archive→unarchive when the open room is archived", () => {
    roomsStore.setState({
      rooms: [room("!r", { isArchived: true })],
      selected: { accountId: ACC, roomId: "!r" },
    });
    expect(resolveMenuActionId("archive-chat")).toBe("unarchive-chat");
  });

  it("keeps archive-chat when the open room is not archived", () => {
    roomsStore.setState({
      rooms: [room("!r", { isArchived: false })],
      selected: { accountId: ACC, roomId: "!r" },
    });
    expect(resolveMenuActionId("archive-chat")).toBe("archive-chat");
  });

  it("flips pin and favorite by their flags", () => {
    roomsStore.setState({
      rooms: [room("!r", { isPinned: true, isFavourite: false })],
      selected: { accountId: ACC, roomId: "!r" },
    });
    expect(resolveMenuActionId("pin-chat")).toBe("unpin-chat");
    expect(resolveMenuActionId("favorite-chat")).toBe("favorite-chat");
  });

  it("resolves mark-read/unread from effective unread state", () => {
    roomsStore.setState({
      rooms: [room("!r", { isUnread: true })],
      selected: { accountId: ACC, roomId: "!r" },
    });
    // Unread → mark-read is the correct positive direction.
    expect(resolveMenuActionId("mark-read")).toBe("mark-read");

    roomsStore.setState({ rooms: [room("!r", { isUnread: false })] });
    // Already read → flip to mark-unread.
    expect(resolveMenuActionId("mark-read")).toBe("mark-unread");
  });
});

describe("useMenuActions", () => {
  it("dispatches the resolved id with the open-chat context on a menu event", async () => {
    roomsStore.setState({
      rooms: [room("!r", { isArchived: true })],
      selected: { accountId: ACC, roomId: "!r" },
    });
    renderHook(() => useMenuActions());
    await waitFor(() => expect(registered).toBeDefined());

    act(() => {
      registered?.({ payload: "archive-chat" });
    });

    expect(dispatchPaletteAction).toHaveBeenCalledWith("unarchive-chat", {
      accountId: ACC,
      roomId: "!r",
    });
  });

  it("dispatches a non-toggle id with a null context when no chat is open", async () => {
    roomsStore.setState({ selected: null });
    renderHook(() => useMenuActions());
    await waitFor(() => expect(registered).toBeDefined());

    act(() => {
      registered?.({ payload: "open-archive" });
    });

    expect(dispatchPaletteAction).toHaveBeenCalledWith("open-archive", null);
  });

  it("unlistens on unmount", async () => {
    const { unmount } = renderHook(() => useMenuActions());
    await waitFor(() => expect(registered).toBeDefined());
    unmount();
    await waitFor(() => expect(unlisten).toHaveBeenCalled());
  });
});
