import { act, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { LONG_PRESS_MS, useLongPress } from "@/hooks/use-long-press";

/**
 * Mock matchMedia at the given width so `useShellLayout().phone` resolves from
 * the simulated viewport (mirrors the phone-shell test convention).
 */
const originalMatchMedia = window.matchMedia;
function mockViewportWidth(width: number) {
  window.matchMedia = vi.fn().mockImplementation((query: string) => {
    const match = query.match(/max-width:\s*(\d+)px/);
    const maxWidth = match ? Number(match[1]) : Number.POSITIVE_INFINITY;
    return {
      matches: width <= maxWidth,
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

function Fixture({ onClick }: { onClick?: () => void }) {
  const handlers = useLongPress();
  return (
    <button type="button" data-testid="target" onClick={onClick} {...handlers}>
      press me
    </button>
  );
}

/** Render the fixture and spy on `contextmenu` events reaching the target. */
function renderFixture(onClick?: () => void) {
  render(<Fixture onClick={onClick} />);
  const target = screen.getByTestId("target");
  const onContextMenu = vi.fn((e: Event) => e.preventDefault());
  target.addEventListener("contextmenu", onContextMenu);
  return { target, onContextMenu };
}

beforeEach(() => {
  vi.useFakeTimers();
  mockViewportWidth(390);
});

afterEach(() => {
  window.matchMedia = originalMatchMedia;
  vi.useRealTimers();
});

describe("useLongPress", () => {
  it("dispatches a synthetic contextmenu at the press point after the hold", () => {
    const { target, onContextMenu } = renderFixture();
    fireEvent.pointerDown(target, { pointerId: 1, clientX: 40, clientY: 60 });
    expect(onContextMenu).not.toHaveBeenCalled();

    act(() => {
      vi.advanceTimersByTime(LONG_PRESS_MS);
    });
    expect(onContextMenu).toHaveBeenCalledTimes(1);
    const event = onContextMenu.mock.calls[0][0] as MouseEvent;
    expect(event.clientX).toBe(40);
    expect(event.clientY).toBe(60);
  });

  it("tolerates sub-threshold movement and still fires", () => {
    const { target, onContextMenu } = renderFixture();
    fireEvent.pointerDown(target, { pointerId: 1, clientX: 40, clientY: 60 });
    // 5px of drift is within the 10px tolerance — still a stationary press.
    fireEvent.pointerMove(target, { pointerId: 1, clientX: 45, clientY: 60 });
    act(() => {
      vi.advanceTimersByTime(LONG_PRESS_MS);
    });
    expect(onContextMenu).toHaveBeenCalledTimes(1);
  });

  it("cancels when the pointer moves past the tolerance (scroll/swipe intent)", () => {
    const { target, onContextMenu } = renderFixture();
    fireEvent.pointerDown(target, { pointerId: 1, clientX: 40, clientY: 60 });
    fireEvent.pointerMove(target, { pointerId: 1, clientX: 60, clientY: 60 });
    act(() => {
      vi.advanceTimersByTime(LONG_PRESS_MS);
    });
    expect(onContextMenu).not.toHaveBeenCalled();
  });

  it("cancels on an early lift (a normal tap never opens the menu)", () => {
    const { target, onContextMenu } = renderFixture();
    fireEvent.pointerDown(target, { pointerId: 1, clientX: 40, clientY: 60 });
    act(() => {
      vi.advanceTimersByTime(LONG_PRESS_MS - 100);
    });
    fireEvent.pointerUp(target, { pointerId: 1, clientX: 40, clientY: 60 });
    act(() => {
      vi.advanceTimersByTime(200);
    });
    expect(onContextMenu).not.toHaveBeenCalled();
  });

  it("cancels on pointercancel (native scroll took the gesture)", () => {
    const { target, onContextMenu } = renderFixture();
    fireEvent.pointerDown(target, { pointerId: 1, clientX: 40, clientY: 60 });
    fireEvent.pointerCancel(target, { pointerId: 1 });
    act(() => {
      vi.advanceTimersByTime(LONG_PRESS_MS);
    });
    expect(onContextMenu).not.toHaveBeenCalled();
  });

  it("cancels when a second pointer joins (pinch is never a long-press)", () => {
    const { target, onContextMenu } = renderFixture();
    fireEvent.pointerDown(target, { pointerId: 1, clientX: 40, clientY: 60 });
    fireEvent.pointerDown(target, { pointerId: 2, clientX: 80, clientY: 60 });
    act(() => {
      vi.advanceTimersByTime(LONG_PRESS_MS);
    });
    expect(onContextMenu).not.toHaveBeenCalled();
  });

  it("is a no-op off the phone tier", () => {
    mockViewportWidth(1024);
    const { target, onContextMenu } = renderFixture();
    fireEvent.pointerDown(target, { pointerId: 1, clientX: 40, clientY: 60 });
    act(() => {
      vi.advanceTimersByTime(LONG_PRESS_MS);
    });
    expect(onContextMenu).not.toHaveBeenCalled();
  });

  it("ignores mouse pointers (native right-click owns the menu there)", () => {
    const { target, onContextMenu } = renderFixture();
    fireEvent.pointerDown(target, {
      pointerId: 1,
      pointerType: "mouse",
      clientX: 40,
      clientY: 60,
    });
    act(() => {
      vi.advanceTimersByTime(LONG_PRESS_MS);
    });
    expect(onContextMenu).not.toHaveBeenCalled();
  });

  it("suppresses the click that follows a fired long-press, but not later clicks", () => {
    const onClick = vi.fn();
    const { target } = renderFixture(onClick);
    fireEvent.pointerDown(target, { pointerId: 1, clientX: 40, clientY: 60 });
    act(() => {
      vi.advanceTimersByTime(LONG_PRESS_MS);
    });
    fireEvent.pointerUp(target, { pointerId: 1, clientX: 40, clientY: 60 });
    // The lift's click is swallowed — the row under the menu never activates…
    fireEvent.click(target);
    expect(onClick).not.toHaveBeenCalled();
    // …and a subsequent normal tap activates as usual.
    fireEvent.click(target);
    expect(onClick).toHaveBeenCalledTimes(1);
  });
});
