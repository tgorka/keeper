import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { InboxRoomVm } from "@/lib/ipc/client";

// The strip round-trips reorder/unpin through the typed IPC client wrappers; mock
// them so tests assert the command without a live Tauri backend.
vi.mock("@/lib/ipc/client", async (importOriginal) => {
  const actual = await importOriginal<typeof import("@/lib/ipc/client")>();
  return {
    ...actual,
    reorderPins: vi.fn(async () => {}),
    unpinRoom: vi.fn(async () => {}),
  };
});

import { PinsStrip } from "@/components/layout/pins-strip";
import { reorderPins, unpinRoom } from "@/lib/ipc/client";

function room(id: string, overrides: Partial<InboxRoomVm> = {}): InboxRoomVm {
  return {
    accountId: "acctA",
    hueIndex: 0,
    roomId: id,
    displayName: id,
    lastMessage: null,
    timestamp: null,
    avatarUrl: null,
    isUnread: false,
    mentionCount: 0,
    isArchived: false,
    isPinned: true,
    isFavourite: false,
    network: null,
    networkId: null,
    muteState: "none",
    ...overrides,
  };
}

afterEach(() => {
  vi.clearAllMocks();
});

describe("PinsStrip", () => {
  it("renders nothing when there are no pins", () => {
    const { container } = render(<PinsStrip pins={[]} />);
    expect(container).toBeEmptyDOMElement();
    expect(screen.queryByLabelText("Pinned conversations")).not.toBeInTheDocument();
  });

  it("renders pinned rooms in the given (stream) order", () => {
    render(
      <PinsStrip
        pins={[room("!b", { displayName: "Bravo" }), room("!a", { displayName: "Alpha" })]}
      />,
    );
    const buttons = screen.getAllByRole("button");
    // Order is exactly the array order (Rust-authoritative) — no client re-sort.
    expect(buttons[0]).toHaveAccessibleName("Pinned conversation with Bravo");
    expect(buttons[1]).toHaveAccessibleName("Pinned conversation with Alpha");
  });

  it("selects the room on click", () => {
    const onSelect = vi.fn();
    render(<PinsStrip pins={[room("!a", { displayName: "Alpha" })]} onSelect={onSelect} />);
    fireEvent.click(screen.getByRole("button", { name: "Pinned conversation with Alpha" }));
    expect(onSelect).toHaveBeenCalledWith({ accountId: "acctA", roomId: "!a" });
  });

  it("dispatches reorderPins with the new full order after a drag-drop", () => {
    render(
      <PinsStrip
        pins={[
          room("!a", { displayName: "Alpha" }),
          room("!b", { displayName: "Bravo" }),
          room("!c", { displayName: "Charlie" }),
        ]}
      />,
    );
    const buttons = screen.getAllByRole("button");
    // Drag the first pin (Alpha) onto the third slot (Charlie).
    fireEvent.dragStart(buttons[0]);
    fireEvent.dragOver(buttons[2]);
    fireEvent.drop(buttons[2]);
    expect(reorderPins).toHaveBeenCalledTimes(1);
    expect(reorderPins).toHaveBeenCalledWith([
      { accountId: "acctA", roomId: "!b" },
      { accountId: "acctA", roomId: "!c" },
      { accountId: "acctA", roomId: "!a" },
    ]);
  });

  it("does not dispatch a reorder when dropped on itself", () => {
    render(<PinsStrip pins={[room("!a"), room("!b")]} />);
    const buttons = screen.getAllByRole("button");
    fireEvent.dragStart(buttons[0]);
    fireEvent.drop(buttons[0]);
    expect(reorderPins).not.toHaveBeenCalled();
  });

  it("does not reorder while filtered (reorderable=false) — a partial order would corrupt hidden pins", () => {
    render(
      <PinsStrip
        pins={[room("!a", { displayName: "Alpha" }), room("!b", { displayName: "Bravo" })]}
        reorderable={false}
      />,
    );
    const buttons = screen.getAllByRole("button");
    // Avatars are not draggable while an account filter is active…
    expect(buttons[0]).toHaveAttribute("draggable", "false");
    // …and even a synthetic drop dispatches nothing.
    fireEvent.dragStart(buttons[0]);
    fireEvent.drop(buttons[1]);
    expect(reorderPins).not.toHaveBeenCalled();
  });

  it("ignores a drop whose drag index is stale (pins shrank mid-drag)", () => {
    // Grab index 2, then the stream replaces the window with a shorter one before
    // the drop lands. The stale index must not splice an undefined element.
    const { rerender } = render(<PinsStrip pins={[room("!a"), room("!b"), room("!c")]} />);
    const buttons = screen.getAllByRole("button");
    fireEvent.dragStart(buttons[2]);
    rerender(<PinsStrip pins={[room("!a")]} />);
    fireEvent.drop(screen.getAllByRole("button")[0]);
    expect(reorderPins).not.toHaveBeenCalled();
  });

  it("invokes unpinRoom from the per-avatar context menu", async () => {
    render(<PinsStrip pins={[room("!a", { displayName: "Alpha" })]} />);
    fireEvent.contextMenu(screen.getByRole("button", { name: "Pinned conversation with Alpha" }));
    const unpin = await screen.findByText("Unpin");
    fireEvent.click(unpin);
    expect(unpinRoom).toHaveBeenCalledWith("acctA", "!a");
  });
});
