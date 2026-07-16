import { act, fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { PaletteActionVm, PaletteChatVm, PaletteResultsVm } from "@/lib/ipc/client";

// Mock the typed IPC client so the palette never touches Tauri. `paletteQuery` is
// the only backend call the component makes; the action commands are stubbed so
// dispatch can be asserted without a live backend.
const paletteQuery = vi.fn();
const archiveRoom = vi.fn().mockResolvedValue(undefined);
const incognitoGet = vi.fn();
const incognitoGetGlobal = vi.fn().mockResolvedValue(false);
const incognitoSetGlobal = vi.fn().mockResolvedValue(undefined);
const incognitoSetChat = vi.fn().mockResolvedValue(undefined);
const noop = vi.fn().mockResolvedValue(undefined);

vi.mock("@/lib/ipc/client", () => ({
  paletteQuery: (query: string, mode: string, openChat: boolean) =>
    paletteQuery(query, mode, openChat),
  archiveRoom: (a: string, r: string) => archiveRoom(a, r),
  unarchiveRoom: (a: string, r: string) => noop(a, r),
  pinRoom: (a: string, r: string) => noop(a, r),
  unpinRoom: (a: string, r: string) => noop(a, r),
  favoriteRoom: (a: string, r: string) => noop(a, r),
  unfavoriteRoom: (a: string, r: string) => noop(a, r),
  markRoomRead: (a: string, r: string) => noop(a, r),
  markRoomUnread: (a: string, r: string) => noop(a, r),
  incognitoGet: (a: string, r: string) => incognitoGet(a, r),
  incognitoGetGlobal: () => incognitoGetGlobal(),
  incognitoSetGlobal: (v: boolean) => incognitoSetGlobal(v),
  incognitoSetChat: (a: string, r: string, v: boolean | null) => incognitoSetChat(a, r, v),
}));

import { CommandPalette } from "@/components/command-palette/command-palette";
import { commandPaletteStore } from "@/lib/stores/command-palette";
import { primaryViewStore } from "@/lib/stores/primary-view";
import { roomsStore } from "@/lib/stores/rooms";

function chat(p: Partial<PaletteChatVm> & Pick<PaletteChatVm, "roomId">): PaletteChatVm {
  return {
    id: `${p.accountId ?? "acc-a"}|${p.roomId}`,
    accountId: p.accountId ?? "acc-a",
    roomId: p.roomId,
    displayName: p.displayName ?? p.roomId,
    hueIndex: p.hueIndex ?? 0,
    network: p.network ?? null,
    isDirect: p.isDirect ?? false,
  };
}

function action(
  p: Partial<PaletteActionVm> & Pick<PaletteActionVm, "id" | "title">,
): PaletteActionVm {
  return {
    id: p.id,
    title: p.title,
    category: p.category ?? "Navigation",
    keywords: p.keywords ?? [],
    shortcut: p.shortcut ?? null,
    requiresOpenChat: p.requiresOpenChat ?? false,
    requiresRecording: p.requiresRecording ?? false,
    toggleGroup: p.toggleGroup ?? null,
  };
}

const EMPTY: PaletteResultsVm = { contacts: [], chats: [], actions: [] };

beforeEach(() => {
  paletteQuery.mockReset();
  paletteQuery.mockResolvedValue(EMPTY);
  archiveRoom.mockClear();
  incognitoGet.mockClear();
  incognitoSetGlobal.mockClear();
  noop.mockClear();
  commandPaletteStore.setState({ isOpen: false });
  roomsStore.setState({ selected: null });
  primaryViewStore.setState({ view: "inbox" });
});

afterEach(() => {
  commandPaletteStore.setState({ isOpen: false });
  vi.clearAllMocks();
});

function open() {
  act(() => {
    commandPaletteStore.getState().open();
  });
}

function typeQuery(text: string) {
  const input = screen.getByRole("combobox");
  fireEvent.change(input, { target: { value: text } });
}

describe("CommandPalette", () => {
  it("is closed by default and opens via the store (⌘K path)", () => {
    render(<CommandPalette />);
    expect(screen.queryByRole("combobox")).not.toBeInTheDocument();
    open();
    expect(screen.getByRole("combobox")).toBeInTheDocument();
  });

  it("queries palette_query in default mode and renders grouped results", async () => {
    paletteQuery.mockResolvedValue({
      contacts: [chat({ roomId: "!alice", displayName: "Alice", isDirect: true })],
      chats: [chat({ roomId: "!alpha", displayName: "Alpha Team", network: "Telegram" })],
      actions: [action({ id: "open-inbox", title: "Open Inbox", shortcut: "⌘1" })],
    });
    render(<CommandPalette />);
    open();
    typeQuery("al");

    await waitFor(() => {
      expect(paletteQuery).toHaveBeenCalledWith("al", "default", false);
    });
    expect(await screen.findByText("Alice")).toBeInTheDocument();
    expect(screen.getByText("Alpha Team")).toBeInTheDocument();
    expect(screen.getByText("Open Inbox")).toBeInTheDocument();
    // Groups render.
    expect(screen.getByText("Contacts")).toBeInTheDocument();
    expect(screen.getByText("Chats")).toBeInTheDocument();
    // Network badge on the bridged chat.
    expect(screen.getByText("Telegram")).toBeInTheDocument();
  });

  it("switches to action mode when the input starts with >", async () => {
    paletteQuery.mockResolvedValue({
      contacts: [],
      chats: [],
      actions: [action({ id: "open-archive", title: "Open Archive" })],
    });
    render(<CommandPalette />);
    open();
    typeQuery(">arch");

    await waitFor(() => {
      expect(paletteQuery).toHaveBeenCalledWith("arch", "action", false);
    });
  });

  it("shows the top actions plus a > hint on a no-match / short query", async () => {
    paletteQuery.mockResolvedValue({
      contacts: [],
      chats: [],
      actions: [action({ id: "open-inbox", title: "Open Inbox" })],
    });
    render(<CommandPalette />);
    open();
    typeQuery("zzqq");

    expect(await screen.findByText("Open Inbox")).toBeInTheDocument();
    // The hint appears in the actions group heading.
    expect(screen.getByText(/type > to filter/i)).toBeInTheDocument();
  });

  it("Enter on an action dispatches its handler and closes", async () => {
    paletteQuery.mockResolvedValue({
      contacts: [],
      chats: [],
      actions: [action({ id: "open-archive", title: "Open Archive" })],
    });
    render(<CommandPalette />);
    open();
    typeQuery(">arch");

    const item = await screen.findByText("Open Archive");
    fireEvent.click(item);

    await waitFor(() => {
      expect(primaryViewStore.getState().view).toBe("archive");
    });
    expect(commandPaletteStore.getState().isOpen).toBe(false);
  });

  it("Enter on an open-chat action invokes the command with the selected chat", async () => {
    roomsStore.setState({ selected: { accountId: "acc-a", roomId: "!room" } });
    paletteQuery.mockResolvedValue({
      contacts: [],
      chats: [],
      actions: [action({ id: "archive-chat", title: "Archive Chat", requiresOpenChat: true })],
    });
    render(<CommandPalette />);
    open();
    typeQuery(">archive");

    const item = await screen.findByText("Archive Chat");
    fireEvent.click(item);

    await waitFor(() => {
      expect(archiveRoom).toHaveBeenCalledWith("acc-a", "!room");
    });
  });

  it("Enter on a chat result selects it and closes", async () => {
    paletteQuery.mockResolvedValue({
      contacts: [],
      chats: [chat({ roomId: "!alpha", displayName: "Alpha Team" })],
      actions: [],
    });
    render(<CommandPalette />);
    open();
    typeQuery("alpha");

    const item = await screen.findByText("Alpha Team");
    fireEvent.click(item);

    expect(roomsStore.getState().selected).toEqual({ accountId: "acc-a", roomId: "!alpha" });
    expect(commandPaletteStore.getState().isOpen).toBe(false);
  });

  it("⌘Enter on a chat peeks it without closing the palette", async () => {
    paletteQuery.mockResolvedValue({
      contacts: [],
      chats: [chat({ roomId: "!alpha", displayName: "Alpha Team" })],
      actions: [],
    });
    render(<CommandPalette />);
    open();
    typeQuery("alpha");
    await screen.findByText("Alpha Team");

    // cmdk highlights the first item on render; ⌘Enter peeks it.
    const input = screen.getByRole("combobox");
    fireEvent.keyDown(input, { key: "Enter", metaKey: true });

    await waitFor(() => {
      expect(roomsStore.getState().selected).toEqual({ accountId: "acc-a", roomId: "!alpha" });
    });
    // Peek keeps the palette open.
    expect(commandPaletteStore.getState().isOpen).toBe(true);
  });
});
