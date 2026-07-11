import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
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

  it("offers no Move up/Move down off the phone tier (desktop menu unchanged)", async () => {
    render(<PinsStrip pins={[room("!a", { displayName: "Alpha" }), room("!b")]} />);
    fireEvent.contextMenu(screen.getByRole("button", { name: "Pinned conversation with Alpha" }));
    expect(await screen.findByText("Unpin")).toBeInTheDocument();
    expect(screen.queryByText("Move up")).not.toBeInTheDocument();
    expect(screen.queryByText("Move down")).not.toBeInTheDocument();
  });
});

// ── Phone touch idioms (Story 13.6) ──────────────────────────────────────────
describe("PinsStrip phone touch idioms", () => {
  const originalMatchMedia = window.matchMedia;

  /** Mock matchMedia at a phone-tier width (mirrors the phone-shell tests). */
  function mockPhoneViewport() {
    window.matchMedia = vi.fn().mockImplementation((query: string) => {
      const match = query.match(/max-width:\s*(\d+)px/);
      const maxWidth = match ? Number(match[1]) : Number.POSITIVE_INFINITY;
      return {
        matches: query.includes("prefers-reduced-motion") ? false : 390 <= maxWidth,
        media: query,
        onchange: null,
        addEventListener: vi.fn(),
        removeEventListener: vi.fn(),
        addListener: vi.fn(),
        removeListener: vi.fn(),
        dispatchEvent: vi.fn(),
      };
    });
  }

  /**
   * Lay the pin list items out horizontally: 60px slots starting at x=0. The
   * rect is computed from the item's *current* position among its siblings so
   * a mid-drag preview reorder relocates the slots exactly like real layout.
   */
  function mockPinSlots() {
    const items = document.querySelectorAll("li");
    items.forEach((item) => {
      (item as HTMLElement).getBoundingClientRect = () => {
        const index = Array.from(item.parentElement?.children ?? []).indexOf(item);
        const left = index * 60;
        return {
          width: 60,
          height: 60,
          top: 0,
          left,
          right: left + 60,
          bottom: 60,
          x: left,
          y: 0,
          toJSON: () => ({}),
        } as DOMRect;
      };
    });
  }

  const pins = () => [
    room("!a", { displayName: "Alpha" }),
    room("!b", { displayName: "Bravo" }),
    room("!c", { displayName: "Charlie" }),
  ];

  beforeEach(() => {
    mockPhoneViewport();
    vi.useFakeTimers();
  });

  afterEach(() => {
    window.matchMedia = originalMatchMedia;
    vi.useRealTimers();
  });

  it("opens the pin menu (Unpin + Move up/down) on a stationary long-press", async () => {
    render(<PinsStrip pins={pins()} />);
    const pin = screen.getByRole("button", { name: "Pinned conversation with Bravo" });
    fireEvent.pointerDown(pin, { pointerId: 1, clientX: 90, clientY: 30 });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    // A stationary lift releases into the menu (drag never started).
    fireEvent.pointerUp(pin, { pointerId: 1, clientX: 90, clientY: 30 });
    vi.useRealTimers();
    expect(await screen.findByText("Unpin")).toBeInTheDocument();
    expect(screen.getByText("Move up")).toBeInTheDocument();
    expect(screen.getByText("Move down")).toBeInTheDocument();
  });

  it("persists Move down via reorderPins (the non-gesture reorder)", async () => {
    render(<PinsStrip pins={pins()} />);
    const pin = screen.getByRole("button", { name: "Pinned conversation with Alpha" });
    fireEvent.pointerDown(pin, { pointerId: 1, clientX: 30, clientY: 30 });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    fireEvent.pointerUp(pin, { pointerId: 1, clientX: 30, clientY: 30 });
    vi.useRealTimers();
    fireEvent.click(await screen.findByText("Move down"));
    expect(reorderPins).toHaveBeenCalledWith([
      { accountId: "acctA", roomId: "!b" },
      { accountId: "acctA", roomId: "!a" },
      { accountId: "acctA", roomId: "!c" },
    ]);
  });

  it("disables Move up on the first pin and Move down on the last", async () => {
    render(<PinsStrip pins={pins()} />);
    const pin = screen.getByRole("button", { name: "Pinned conversation with Alpha" });
    fireEvent.pointerDown(pin, { pointerId: 1, clientX: 30, clientY: 30 });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    fireEvent.pointerUp(pin, { pointerId: 1, clientX: 30, clientY: 30 });
    vi.useRealTimers();
    const moveUp = await screen.findByText("Move up");
    expect(moveUp.closest("[data-disabled]")).not.toBeNull();
    expect(screen.getByText("Move down").closest("[data-disabled]")).toBeNull();
  });

  it("reorders via long-press-drag and persists the full order", () => {
    render(<PinsStrip pins={pins()} />);
    mockPinSlots();
    const pin = screen.getByRole("button", { name: "Pinned conversation with Alpha" });
    // Long-press lifts the pin…
    fireEvent.pointerDown(pin, { pointerId: 1, clientX: 30, clientY: 30 });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    // …dragging moves it over the third slot (x = 150 → slot index 2)…
    fireEvent.pointerMove(pin, { pointerId: 1, clientX: 150, clientY: 30 });
    // …and the drop persists the new full order.
    fireEvent.pointerUp(pin, { pointerId: 1, clientX: 150, clientY: 30 });
    expect(reorderPins).toHaveBeenCalledTimes(1);
    expect(reorderPins).toHaveBeenCalledWith([
      { accountId: "acctA", roomId: "!b" },
      { accountId: "acctA", roomId: "!c" },
      { accountId: "acctA", roomId: "!a" },
    ]);
  });

  it("shows a reorder preview while the lifted pin drags", () => {
    render(<PinsStrip pins={pins()} />);
    mockPinSlots();
    const pin = screen.getByRole("button", { name: "Pinned conversation with Alpha" });
    fireEvent.pointerDown(pin, { pointerId: 1, clientX: 30, clientY: 30 });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    fireEvent.pointerMove(pin, { pointerId: 1, clientX: 150, clientY: 30 });
    const names = screen
      .getAllByRole("button")
      .map((b) => b.getAttribute("aria-label"))
      .filter((label) => label?.startsWith("Pinned conversation"));
    expect(names).toEqual([
      "Pinned conversation with Bravo",
      "Pinned conversation with Charlie",
      "Pinned conversation with Alpha",
    ]);
    // The preview is ephemeral: nothing persisted until the drop.
    expect(reorderPins).not.toHaveBeenCalled();
    fireEvent.pointerCancel(pin, { pointerId: 1 });
  });

  it("does not drag-reorder while filtered (reorderable=false): long-press opens the menu with Move disabled", async () => {
    render(<PinsStrip pins={pins()} reorderable={false} />);
    mockPinSlots();
    const pin = screen.getByRole("button", { name: "Pinned conversation with Alpha" });
    fireEvent.pointerDown(pin, { pointerId: 1, clientX: 30, clientY: 30 });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    vi.useRealTimers();
    // The menu opened straight from the hold (no lift), with Move items disabled.
    expect(await screen.findByText("Unpin")).toBeInTheDocument();
    expect(screen.getByText("Move up").closest("[data-disabled]")).not.toBeNull();
    expect(screen.getByText("Move down").closest("[data-disabled]")).not.toBeNull();
    expect(reorderPins).not.toHaveBeenCalled();
  });
});
