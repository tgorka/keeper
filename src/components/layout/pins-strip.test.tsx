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

/**
 * Lay the pin list items out horizontally: 60 px slots starting at x=0. The rect
 * is computed from the item's *current* position among its siblings so a mid-drag
 * preview reorder relocates the slots exactly like real layout.
 *
 * jsdom has no layout and `src/test/setup.ts` answers a zero rect with one full
 * viewport, so without this every pin's midpoint is the same number and every
 * release lands in slot 0.
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

/**
 * Press an avatar, move the pointer to `x`, release it there — the gesture a real
 * pointer produces, and the only one that can reorder this strip. HTML5 drag was
 * removed in Story 53.1 because Tauri claims the drop in Rust before WebKit can
 * perform it (`use-pointer-drag.ts` carries the source lines).
 */
function dragPin(avatar: HTMLElement, x: number) {
  fireEvent.pointerDown(avatar, { pointerId: 1, button: 0, clientX: 30, clientY: 30 });
  fireEvent.pointerMove(avatar, { pointerId: 1, clientX: x, clientY: 30 });
  fireEvent.pointerUp(avatar, { pointerId: 1, clientX: x, clientY: 30 });
}

/**
 * Record the pointer captures taken on one element.
 *
 * An own property rather than `vi.spyOn`: `setPointerCapture` is inherited from
 * `Element.prototype`, where `src/test/setup.ts` stubs it once for the whole
 * suite, so a spy installed there is shared by every element and cannot say
 * *which* one took the capture — which is the whole question in these tests.
 */
function capturesOn(element: HTMLElement) {
  const taken = vi.fn();
  element.setPointerCapture = taken;
  return taken;
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

  it("reorders by pointer on the desktop, and persists the new full order", () => {
    // Until Story 53.1 this was HTML5 drag, which on macOS never delivered a
    // `drop` at all: Tauri's own handler claims `performDragOperation:` in Rust
    // before WebKit performs it. jsdom can drive the pointer sequence honestly.
    render(
      <PinsStrip
        pins={[
          room("!a", { displayName: "Alpha" }),
          room("!b", { displayName: "Bravo" }),
          room("!c", { displayName: "Charlie" }),
        ]}
      />,
    );
    mockPinSlots();
    // Alpha, pressed and carried onto the third slot (x = 150).
    dragPin(screen.getAllByRole("button")[0], 150);
    expect(reorderPins).toHaveBeenCalledTimes(1);
    expect(reorderPins).toHaveBeenCalledWith([
      { accountId: "acctA", roomId: "!b" },
      { accountId: "acctA", roomId: "!c" },
      { accountId: "acctA", roomId: "!a" },
    ]);
  });

  it("takes the capture back when the preview moves the pressed pin, and still lands the reorder", () => {
    // The defect this strip shipped twice. The move that crosses the slop is the
    // move that paints the preview, and the preview reorders a keyed list: React
    // moves the pressed node, `insertBefore` REMOVES it from its parent first,
    // and the removing steps are what Pointer Events hooks for the implicit
    // release of pointer capture. So the drag lost its capture on its own first
    // step, every later move and the release were discarded, and the strip
    // snapped back — green in jsdom, dead on WebKit, because nothing here used
    // to dispatch `lostpointercapture`.
    render(
      <PinsStrip
        pins={[
          room("!a", { displayName: "Alpha" }),
          room("!b", { displayName: "Bravo" }),
          room("!c", { displayName: "Charlie" }),
        ]}
      />,
    );
    mockPinSlots();
    const alpha = screen.getAllByRole("button")[0];
    const captured = capturesOn(alpha);
    fireEvent.pointerDown(alpha, { pointerId: 1, button: 0, clientX: 30, clientY: 30 });
    fireEvent.pointerMove(alpha, { pointerId: 1, clientX: 150, clientY: 30 });
    // Taken on the crossing, and the preview has moved this very node: same
    // element, third slot.
    expect(captured).toHaveBeenCalledTimes(1);
    expect(screen.getAllByRole("button")[2]).toBe(alpha);
    // What WebKit does next, and jsdom never did on its own.
    fireEvent.lostPointerCapture(alpha, { pointerId: 1 });
    // Still connected, still mounted: the gesture is live and the capture comes
    // back rather than the drag being torn down.
    expect(captured).toHaveBeenCalledTimes(2);
    fireEvent.pointerMove(alpha, { pointerId: 1, clientX: 150, clientY: 30 });
    fireEvent.pointerUp(alpha, { pointerId: 1, clientX: 150, clientY: 30 });
    expect(reorderPins).toHaveBeenCalledTimes(1);
    expect(reorderPins).toHaveBeenCalledWith([
      { accountId: "acctA", roomId: "!b" },
      { accountId: "acctA", roomId: "!c" },
      { accountId: "acctA", roomId: "!a" },
    ]);
  });

  it("ends the gesture and frees the next click when the pressed pin unmounts mid-drag", () => {
    // The other cause of the same event, which wants the opposite answer: the
    // captured element is gone for good, so there is nothing to take back — and
    // the click this drag was going to swallow will never be dispatched at a node
    // that no longer exists, so the swallow flag has to go with the press.
    const onSelect = vi.fn();
    const { rerender } = render(
      <PinsStrip
        pins={[
          room("!a", { displayName: "Alpha" }),
          room("!b", { displayName: "Bravo" }),
          room("!c", { displayName: "Charlie" }),
        ]}
        onSelect={onSelect}
      />,
    );
    mockPinSlots();
    const alpha = screen.getAllByRole("button")[0];
    fireEvent.pointerDown(alpha, { pointerId: 1, button: 0, clientX: 30, clientY: 30 });
    fireEvent.pointerMove(alpha, { pointerId: 1, clientX: 150, clientY: 30 });
    // The stream unpins Alpha mid-drag.
    rerender(
      <PinsStrip
        pins={[room("!b", { displayName: "Bravo" }), room("!c", { displayName: "Charlie" })]}
        onSelect={onSelect}
      />,
    );
    expect(alpha.isConnected).toBe(false);
    fireEvent.lostPointerCapture(alpha, { pointerId: 1 });
    expect(reorderPins).not.toHaveBeenCalled();
    // The next tap on another pin is that pin's own.
    fireEvent.click(screen.getByRole("button", { name: "Pinned conversation with Bravo" }));
    expect(onSelect).toHaveBeenCalledWith({ accountId: "acctA", roomId: "!b" });
  });

  it("keeps a drag whose pointer left the avatar before the slop: the strip hears the move", () => {
    // 44 px avatars and a 10 px tolerance: a press 4 px from the edge leaves the
    // avatar before the press has become a drag, and before the crossing there is
    // no capture. The move lands on the list, which the avatar sits below rather
    // than above — so with handlers on the avatar alone nothing hears it and the
    // drag silently never starts.
    render(
      <PinsStrip
        pins={[
          room("!a", { displayName: "Alpha" }),
          room("!b", { displayName: "Bravo" }),
          room("!c", { displayName: "Charlie" }),
        ]}
      />,
    );
    mockPinSlots();
    const list = screen.getByRole("list", { name: "Pinned conversations" });
    const alpha = screen.getAllByRole("button")[0];
    const onAvatar = capturesOn(alpha);
    const onList = capturesOn(list);
    fireEvent.pointerDown(alpha, { pointerId: 1, button: 0, clientX: 40, clientY: 30 });
    fireEvent.pointerMove(list, { pointerId: 1, clientX: 150, clientY: 30 });
    fireEvent.pointerUp(list, { pointerId: 1, clientX: 150, clientY: 30 });
    expect(reorderPins).toHaveBeenCalledWith([
      { accountId: "acctA", roomId: "!b" },
      { accountId: "acctA", roomId: "!c" },
      { accountId: "acctA", roomId: "!a" },
    ]);
    // The capture belongs to the pressed avatar, never to the box a stray move
    // happened to land on.
    expect(onAvatar).toHaveBeenCalledWith(1);
    expect(onList).not.toHaveBeenCalled();
  });

  it("hands the capture to a second press that begins while the first is in flight", () => {
    // A second finger, or a release this strip never heard. The new press must not
    // inherit the old one's capture hold: a hold that outlives its press is never
    // released — its own element's release names a pointer that has gone — and no
    // later gesture on this strip could capture again.
    render(
      <PinsStrip
        pins={[
          room("!a", { displayName: "Alpha" }),
          room("!b", { displayName: "Bravo" }),
          room("!c", { displayName: "Charlie" }),
        ]}
      />,
    );
    mockPinSlots();
    const alpha = screen.getByRole("button", { name: "Pinned conversation with Alpha" });
    const bravo = screen.getByRole("button", { name: "Pinned conversation with Bravo" });
    const onAlpha = capturesOn(alpha);
    const onBravo = capturesOn(bravo);
    fireEvent.pointerDown(alpha, { pointerId: 1, button: 0, clientX: 30, clientY: 30 });
    fireEvent.pointerMove(alpha, { pointerId: 1, clientX: 150, clientY: 30 });
    expect(onAlpha).toHaveBeenCalledTimes(1);
    // Alpha's release never arrives. Bravo is pressed and carried to the front.
    fireEvent.pointerDown(bravo, { pointerId: 2, button: 0, clientX: 90, clientY: 30 });
    fireEvent.pointerMove(bravo, { pointerId: 2, clientX: 30, clientY: 30 });
    expect(onBravo).toHaveBeenCalledTimes(1);
    fireEvent.pointerUp(bravo, { pointerId: 2, clientX: 30, clientY: 30 });
    expect(reorderPins).toHaveBeenCalledTimes(1);
    expect(reorderPins).toHaveBeenCalledWith([
      { accountId: "acctA", roomId: "!b" },
      { accountId: "acctA", roomId: "!a" },
      { accountId: "acctA", roomId: "!c" },
    ]);
  });

  it("previews the reorder while the pin is being carried, and persists nothing yet", () => {
    // With no HTML5 ghost left to lean on, the preview IS the desktop's cue.
    render(
      <PinsStrip
        pins={[
          room("!a", { displayName: "Alpha" }),
          room("!b", { displayName: "Bravo" }),
          room("!c", { displayName: "Charlie" }),
        ]}
      />,
    );
    mockPinSlots();
    const alpha = screen.getAllByRole("button")[0];
    fireEvent.pointerDown(alpha, { pointerId: 1, button: 0, clientX: 30, clientY: 30 });
    fireEvent.pointerMove(alpha, { pointerId: 1, clientX: 150, clientY: 30 });
    expect(screen.getAllByRole("button").map((b) => b.getAttribute("aria-label"))).toEqual([
      "Pinned conversation with Bravo",
      "Pinned conversation with Charlie",
      "Pinned conversation with Alpha",
    ]);
    expect(reorderPins).not.toHaveBeenCalled();
    fireEvent.pointerCancel(alpha, { pointerId: 1 });
    // Cancelled: the preview is gone and nothing was written.
    expect(screen.getAllByRole("button")[0]).toHaveAccessibleName("Pinned conversation with Alpha");
    expect(reorderPins).not.toHaveBeenCalled();
  });

  it("keeps a press that does not travel a click, and swallows the click after a drag", () => {
    const onSelect = vi.fn();
    render(
      <PinsStrip
        pins={[room("!a", { displayName: "Alpha" }), room("!b", { displayName: "Bravo" })]}
        onSelect={onSelect}
      />,
    );
    mockPinSlots();
    const alpha = screen.getAllByRole("button")[0];
    // A press with a hand's worth of jitter is still a click.
    fireEvent.pointerDown(alpha, { pointerId: 1, button: 0, clientX: 30, clientY: 30 });
    fireEvent.pointerMove(alpha, { pointerId: 1, clientX: 33, clientY: 31 });
    fireEvent.pointerUp(alpha, { pointerId: 1, clientX: 33, clientY: 31 });
    fireEvent.click(alpha);
    expect(onSelect).toHaveBeenCalledWith({ accountId: "acctA", roomId: "!a" });
    expect(reorderPins).not.toHaveBeenCalled();
    // A drag is not: releasing a carried pin must not also open the room.
    onSelect.mockClear();
    dragPin(screen.getAllByRole("button")[0], 90);
    expect(reorderPins).toHaveBeenCalledTimes(1);
    fireEvent.click(screen.getAllByRole("button")[0]);
    expect(onSelect).not.toHaveBeenCalled();
  });

  it("does not dispatch a reorder when released on its own slot", () => {
    render(<PinsStrip pins={[room("!a"), room("!b")]} />);
    mockPinSlots();
    const alpha = screen.getAllByRole("button")[0];
    fireEvent.pointerDown(alpha, { pointerId: 1, button: 0, clientX: 30, clientY: 30 });
    // Carried over the second slot and then back home: a real drag that resolves
    // to the slot it started in, which is a move to nowhere and not a write.
    fireEvent.pointerMove(alpha, { pointerId: 1, clientX: 90, clientY: 30 });
    fireEvent.pointerMove(alpha, { pointerId: 1, clientX: 30, clientY: 30 });
    fireEvent.pointerUp(alpha, { pointerId: 1, clientX: 30, clientY: 30 });
    expect(reorderPins).not.toHaveBeenCalled();
  });

  it("does not reorder while filtered (reorderable=false) — a partial order would corrupt hidden pins", () => {
    render(
      <PinsStrip
        pins={[room("!a", { displayName: "Alpha" }), room("!b", { displayName: "Bravo" })]}
        reorderable={false}
      />,
    );
    mockPinSlots();
    dragPin(screen.getAllByRole("button")[0], 90);
    expect(reorderPins).not.toHaveBeenCalled();
  });

  it("ignores a release whose pressed index is stale (pins shrank mid-drag)", () => {
    // Grab index 2, then the stream replaces the window with a shorter one before
    // the release lands. The stale index must not splice an undefined element.
    const { rerender } = render(<PinsStrip pins={[room("!a"), room("!b"), room("!c")]} />);
    mockPinSlots();
    const third = screen.getAllByRole("button")[2];
    fireEvent.pointerDown(third, { pointerId: 1, button: 0, clientX: 150, clientY: 30 });
    fireEvent.pointerMove(third, { pointerId: 1, clientX: 30, clientY: 30 });
    rerender(<PinsStrip pins={[room("!a")]} />);
    fireEvent.pointerUp(screen.getAllByRole("button")[0], {
      pointerId: 1,
      clientX: 30,
      clientY: 30,
    });
    expect(reorderPins).not.toHaveBeenCalled();
  });

  it("carries no HTML5 drag anywhere in the strip", () => {
    // The mechanism that could not work under Tauri on macOS is gone, not parked
    // beside the one that can.
    const { container } = render(<PinsStrip pins={[room("!a"), room("!b")]} />);
    expect(container.querySelectorAll("[draggable]")).toHaveLength(0);
  });

  it("reorders nothing in answer to an HTML5 drag", () => {
    // `draggable` is the attribute; this is the behaviour. Nothing in the strip
    // answers `dragstart`, `dragover` or `drop`, so the mechanism that could not
    // work under Tauri cannot come back one handler at a time either.
    render(<PinsStrip pins={[room("!a", { displayName: "Alpha" }), room("!b")]} />);
    mockPinSlots();
    const [alpha, bravo] = screen.getAllByRole("button");
    fireEvent.dragStart(alpha);
    fireEvent.dragOver(bravo);
    fireEvent.drop(bravo);
    fireEvent.dragEnd(alpha);
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

  it("opens the room on the tap after a long-press reorder", () => {
    // A touch drag ends with no synthesised click, so nothing eats the swallow
    // flag the lift set — and the phone gate returns before `drag.begin`, the
    // other site that clears it. Left leaking, the next tap is eaten and the room
    // does not open until the second.
    const onSelect = vi.fn();
    render(<PinsStrip pins={pins()} onSelect={onSelect} />);
    mockPinSlots();
    const alpha = screen.getByRole("button", { name: "Pinned conversation with Alpha" });
    const captured = capturesOn(alpha);
    fireEvent.pointerDown(alpha, { pointerId: 1, clientX: 30, clientY: 30 });
    act(() => {
      vi.advanceTimersByTime(500);
    });
    fireEvent.pointerMove(alpha, { pointerId: 1, clientX: 150, clientY: 30 });
    fireEvent.pointerUp(alpha, { pointerId: 1, clientX: 150, clientY: 30 });
    expect(reorderPins).toHaveBeenCalledTimes(1);
    // The lift captured at the hold; the slop crossing asks again for the same
    // pointer on the same element and is skipped, so one capture, one release
    // listener, one gesture.
    expect(captured).toHaveBeenCalledTimes(1);
    const bravo = screen.getByRole("button", { name: "Pinned conversation with Bravo" });
    fireEvent.pointerDown(bravo, { pointerId: 2, clientX: 90, clientY: 30 });
    fireEvent.pointerUp(bravo, { pointerId: 2, clientX: 90, clientY: 30 });
    fireEvent.click(bravo);
    expect(onSelect).toHaveBeenCalledWith({ accountId: "acctA", roomId: "!b" });
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
