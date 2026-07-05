import { fireEvent, render, screen, waitFor } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { ChatRow } from "@/components/chat/chat-row";
import type { InboxRoomVm } from "@/lib/ipc/client";
import { roomsStore } from "@/lib/stores/rooms";

// The row round-trips read/unread through the typed IPC client wrappers; mock them
// so tests assert the command without a live Tauri backend.
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    markRoomRead: vi.fn(async () => {}),
    markRoomUnread: vi.fn(async () => {}),
    archiveRoom: vi.fn(async () => {}),
    unarchiveRoom: vi.fn(async () => {}),
    pinRoom: vi.fn(async () => {}),
    unpinRoom: vi.fn(async () => {}),
  };
});

import {
  archiveRoom,
  markRoomRead,
  markRoomUnread,
  pinRoom,
  unarchiveRoom,
  unpinRoom,
} from "@/lib/ipc/client";

function room(overrides: Partial<InboxRoomVm> = {}): InboxRoomVm {
  return {
    accountId: "01ARZ3NDEKTSV4RRFFQ69G5FAV",
    hueIndex: 0,
    roomId: "!abc:example.org",
    displayName: "Alice Smith",
    lastMessage: "hey there",
    timestamp: Date.now(),
    avatarUrl: null,
    isUnread: false,
    mentionCount: 0,
    isArchived: false,
    isPinned: false,
    ...overrides,
  };
}

afterEach(() => {
  roomsStore.getState().clear();
  vi.clearAllMocks();
});

describe("ChatRow", () => {
  it("renders display name and preview", () => {
    render(<ChatRow room={room()} />);
    expect(screen.getByText("Alice Smith")).toBeInTheDocument();
    expect(screen.getByText("hey there")).toBeInTheDocument();
  });

  it("is a full-width accessible button with a room-labelled name", () => {
    render(<ChatRow room={room()} />);
    const button = screen.getByRole("button", { name: "Conversation with Alice Smith" });
    expect(button).toBeInTheDocument();
    expect(button).toHaveClass("w-full");
  });

  it("renders a 3 px per-account hue edge bar mapped from the hue index", () => {
    const { getByTestId } = render(<ChatRow room={room({ hueIndex: 3 })} />);
    const bar = getByTestId("account-hue-bar");
    expect(bar).toHaveClass("w-[3px]");
    // The bar's color is the CSS variable for the account's hue index — no
    // hardcoded color value.
    expect(bar.style.backgroundColor).toBe("var(--account-hue-3)");
  });

  it("wraps an out-of-range hue index into the 0..8 wheel", () => {
    const { getByTestId } = render(<ChatRow room={room({ hueIndex: 9 })} />);
    expect(getByTestId("account-hue-bar").style.backgroundColor).toBe("var(--account-hue-1)");
  });

  it("shows avatar fallback initials when no avatar url", () => {
    render(<ChatRow room={room({ displayName: "Alice Smith" })} />);
    expect(screen.getByText("AS")).toBeInTheDocument();
  });

  it("renders an empty preview when lastMessage is null", () => {
    render(<ChatRow room={room({ lastMessage: null })} />);
    expect(screen.queryByText("hey there")).not.toBeInTheDocument();
  });

  it("omits the timestamp when it is null", () => {
    const { container } = render(<ChatRow room={room({ timestamp: null })} />);
    expect(container.querySelector(".text-xs")).toBeNull();
  });

  it("renders initials fallback and no img for an mxc:// avatar url", () => {
    const { container } = render(
      <ChatRow room={room({ displayName: "Alice Smith", avatarUrl: "mxc://x/y" })} />,
    );
    expect(screen.getByText("AS")).toBeInTheDocument();
    expect(container.querySelector('img[src="mxc://x/y"]')).toBeNull();
    expect(container.querySelector("img")).toBeNull();
  });

  it("renders an img for an https:// avatar url", async () => {
    // Radix Avatar only mounts the <img> once the image reports "loaded"; jsdom
    // never fires load events, so stub window.Image to dispatch "load" once src
    // is set.
    const RealImage = window.Image;
    class LoadingImage {
      #listeners: Record<string, Array<(e: unknown) => void>> = {};
      referrerPolicy = "";
      crossOrigin: string | null = null;
      complete = false;
      naturalWidth = 0;
      #src = "";
      addEventListener(type: string, cb: (e: unknown) => void): void {
        const list = this.#listeners[type] ?? [];
        list.push(cb);
        this.#listeners[type] = list;
      }
      removeEventListener(): void {}
      get src(): string {
        return this.#src;
      }
      set src(value: string) {
        this.#src = value;
        queueMicrotask(() => {
          this.complete = true;
          this.naturalWidth = 1;
          for (const cb of this.#listeners.load ?? []) {
            cb({ currentTarget: this });
          }
        });
      }
    }
    window.Image = LoadingImage as unknown as typeof Image;
    try {
      const { container } = render(
        <ChatRow room={room({ avatarUrl: "https://cdn.example.org/a.png" })} />,
      );
      await waitFor(() => {
        expect(container.querySelector('img[src="https://cdn.example.org/a.png"]')).not.toBeNull();
      });
    } finally {
      window.Image = RealImage;
    }
  });

  it("calls onSelect with the account and room ids when clicked", () => {
    const onSelect = vi.fn();
    render(
      <ChatRow
        room={room({ accountId: "acctB", roomId: "!xyz:example.org" })}
        onSelect={onSelect}
      />,
    );
    fireEvent.click(screen.getByRole("button"));
    expect(onSelect).toHaveBeenCalledWith({ accountId: "acctB", roomId: "!xyz:example.org" });
  });

  it("marks the selected row with aria-current and a highlight", () => {
    render(<ChatRow room={room()} selected />);
    const button = screen.getByRole("button");
    expect(button).toHaveAttribute("aria-current", "true");
    expect(button).toHaveClass("bg-accent");
  });

  it("does not mark an unselected row with aria-current", () => {
    render(<ChatRow room={room()} selected={false} />);
    expect(screen.getByRole("button")).not.toHaveAttribute("aria-current");
  });

  it("read row: normal-weight name, no dot, no mention badge", () => {
    render(<ChatRow room={room({ isUnread: false, mentionCount: 0 })} />);
    expect(screen.getByText("Alice Smith")).toHaveClass("font-medium");
    expect(screen.getByText("Alice Smith")).not.toHaveClass("font-semibold");
    expect(screen.queryByTestId("unread-dot")).not.toBeInTheDocument();
    expect(screen.queryByTestId("mention-badge")).not.toBeInTheDocument();
  });

  it("unread without mention: bold name + neutral dot, no badge", () => {
    render(<ChatRow room={room({ isUnread: true, mentionCount: 0 })} />);
    expect(screen.getByText("Alice Smith")).toHaveClass("font-semibold");
    const dot = screen.getByTestId("unread-dot");
    expect(dot).toBeInTheDocument();
    expect(dot).toHaveClass("bg-muted-foreground");
    expect(screen.queryByTestId("mention-badge")).not.toBeInTheDocument();
  });

  it("unread mention: bold name + filled primary badge showing the count, no dot", () => {
    render(<ChatRow room={room({ isUnread: true, mentionCount: 3 })} />);
    expect(screen.getByText("Alice Smith")).toHaveClass("font-semibold");
    const badge = screen.getByTestId("mention-badge");
    expect(badge).toHaveTextContent("3");
    expect(badge).toHaveAttribute("data-variant", "default");
    expect(screen.queryByTestId("unread-dot")).not.toBeInTheDocument();
  });

  it("manually-marked unread with zero counts renders as unread (bold + dot)", () => {
    // Rust folds `is_marked_unread` into `isUnread`, so the row treats it as unread.
    render(<ChatRow room={room({ isUnread: true, mentionCount: 0 })} />);
    expect(screen.getByText("Alice Smith")).toHaveClass("font-semibold");
    expect(screen.getByTestId("unread-dot")).toBeInTheDocument();
  });

  it("context menu on an unread row shows Mark read and invokes markRoomRead", async () => {
    render(
      <ChatRow room={room({ accountId: "acctB", roomId: "!xyz:example.org", isUnread: true })} />,
    );
    fireEvent.contextMenu(screen.getByRole("button"));
    const item = await screen.findByText("Mark read");
    expect(screen.queryByText("Mark unread")).not.toBeInTheDocument();
    fireEvent.click(item);
    expect(markRoomRead).toHaveBeenCalledWith("acctB", "!xyz:example.org");
    expect(markRoomUnread).not.toHaveBeenCalled();
    // The overlay flipped the row to read within one frame.
    expect(roomsStore.getState().optimisticUnread.get("acctB|!xyz:example.org")).toBe(false);
  });

  it("context menu on a read row shows Mark unread and invokes markRoomUnread", async () => {
    render(
      <ChatRow room={room({ accountId: "acctB", roomId: "!xyz:example.org", isUnread: false })} />,
    );
    fireEvent.contextMenu(screen.getByRole("button"));
    const item = await screen.findByText("Mark unread");
    expect(screen.queryByText("Mark read")).not.toBeInTheDocument();
    fireEvent.click(item);
    expect(markRoomUnread).toHaveBeenCalledWith("acctB", "!xyz:example.org");
    expect(markRoomRead).not.toHaveBeenCalled();
    expect(roomsStore.getState().optimisticUnread.get("acctB|!xyz:example.org")).toBe(true);
  });

  it("reflects the optimistic overlay: a read row renders unread when overridden", () => {
    roomsStore.getState().setOptimisticUnread("acctB", "!xyz:example.org", true);
    render(
      <ChatRow room={room({ accountId: "acctB", roomId: "!xyz:example.org", isUnread: false })} />,
    );
    expect(screen.getByText("Alice Smith")).toHaveClass("font-semibold");
    expect(screen.getByTestId("unread-dot")).toBeInTheDocument();
  });

  it("optimistic read clears the mention badge in the same frame it un-bolds", () => {
    // A mention row optimistically marked read must drop the badge too, not just
    // un-bold — a read row never carries a mention badge.
    roomsStore.getState().setOptimisticUnread("acctB", "!xyz:example.org", false);
    render(
      <ChatRow
        room={room({
          accountId: "acctB",
          roomId: "!xyz:example.org",
          isUnread: true,
          mentionCount: 3,
        })}
      />,
    );
    expect(screen.getByText("Alice Smith")).not.toHaveClass("font-semibold");
    expect(screen.queryByTestId("mention-badge")).not.toBeInTheDocument();
    expect(screen.queryByTestId("unread-dot")).not.toBeInTheDocument();
  });

  it("carries the unread state in the button's accessible name", () => {
    const { rerender } = render(<ChatRow room={room({ isUnread: true, mentionCount: 0 })} />);
    expect(
      screen.getByRole("button", { name: "Conversation with Alice Smith, unread" }),
    ).toBeInTheDocument();

    rerender(<ChatRow room={room({ isUnread: true, mentionCount: 1 })} />);
    expect(
      screen.getByRole("button", { name: "Conversation with Alice Smith, 1 unread mention" }),
    ).toBeInTheDocument();

    rerender(<ChatRow room={room({ isUnread: true, mentionCount: 3 })} />);
    expect(
      screen.getByRole("button", { name: "Conversation with Alice Smith, 3 unread mentions" }),
    ).toBeInTheDocument();

    rerender(<ChatRow room={room({ isUnread: false })} />);
    expect(
      screen.getByRole("button", { name: "Conversation with Alice Smith" }),
    ).toBeInTheDocument();
  });

  it("context menu on a non-archived row shows Archive and invokes archiveRoom", async () => {
    render(
      <ChatRow
        room={room({ accountId: "acctB", roomId: "!xyz:example.org", isArchived: false })}
      />,
    );
    fireEvent.contextMenu(screen.getByRole("button"));
    const item = await screen.findByText("Archive");
    expect(screen.queryByText("Unarchive")).not.toBeInTheDocument();
    fireEvent.click(item);
    expect(archiveRoom).toHaveBeenCalledWith("acctB", "!xyz:example.org");
    expect(unarchiveRoom).not.toHaveBeenCalled();
  });

  it("context menu on an archived row shows Unarchive and invokes unarchiveRoom", async () => {
    render(
      <ChatRow room={room({ accountId: "acctB", roomId: "!xyz:example.org", isArchived: true })} />,
    );
    fireEvent.contextMenu(screen.getByRole("button"));
    const item = await screen.findByText("Unarchive");
    expect(screen.queryByText("Archive")).not.toBeInTheDocument();
    fireEvent.click(item);
    expect(unarchiveRoom).toHaveBeenCalledWith("acctB", "!xyz:example.org");
    expect(archiveRoom).not.toHaveBeenCalled();
  });

  it("context menu on an unpinned row shows Pin and invokes pinRoom", async () => {
    render(
      <ChatRow room={room({ accountId: "acctB", roomId: "!xyz:example.org", isPinned: false })} />,
    );
    fireEvent.contextMenu(screen.getByRole("button"));
    const item = await screen.findByText("Pin");
    expect(screen.queryByText("Unpin")).not.toBeInTheDocument();
    fireEvent.click(item);
    expect(pinRoom).toHaveBeenCalledWith("acctB", "!xyz:example.org");
    expect(unpinRoom).not.toHaveBeenCalled();
  });

  it("context menu on a pinned row shows Unpin and invokes unpinRoom", async () => {
    render(
      <ChatRow room={room({ accountId: "acctB", roomId: "!xyz:example.org", isPinned: true })} />,
    );
    fireEvent.contextMenu(screen.getByRole("button"));
    const item = await screen.findByText("Unpin");
    expect(screen.queryByText("Pin")).not.toBeInTheDocument();
    fireEvent.click(item);
    expect(unpinRoom).toHaveBeenCalledWith("acctB", "!xyz:example.org");
    expect(pinRoom).not.toHaveBeenCalled();
  });

  it("reverts the optimistic overlay when the mark command hard-rejects", async () => {
    vi.mocked(markRoomRead).mockRejectedValueOnce(new Error("inactive account"));
    render(
      <ChatRow room={room({ accountId: "acctB", roomId: "!xyz:example.org", isUnread: true })} />,
    );
    fireEvent.contextMenu(screen.getByRole("button"));
    fireEvent.click(await screen.findByText("Mark read"));
    // The override is dropped so the row falls back to the authoritative stream
    // rather than stranding a phantom-read overlay the stream never reconciles.
    await waitFor(() => {
      expect(roomsStore.getState().optimisticUnread.has("acctB|!xyz:example.org")).toBe(false);
    });
  });
});
