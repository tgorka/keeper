import { fireEvent, render, screen } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { type SwipeSideConfig, useSwipeActions } from "@/hooks/use-swipe-actions";

/** Mock every rect at the given width so the swipe reads a real drag range. */
function mockRectWidth(width: number) {
  vi.spyOn(Element.prototype, "getBoundingClientRect").mockReturnValue({
    width,
    height: 64,
    top: 0,
    left: 0,
    right: width,
    bottom: 64,
    x: 0,
    y: 0,
    toJSON: () => ({}),
  } as DOMRect);
}

interface FixtureProps {
  enabled?: boolean;
  leading?: SwipeSideConfig;
  trailing?: SwipeSideConfig;
  onRowClick?: () => void;
}

function Fixture({ enabled = true, leading, trailing, onRowClick }: FixtureProps) {
  const swipe = useSwipeActions({ enabled, leading, trailing });
  return (
    <div>
      <button
        type="button"
        data-testid="row"
        data-dx={swipe.dx}
        data-dragging={swipe.dragging}
        data-committing={swipe.committing ?? "none"}
        data-revealed={swipe.revealed ?? "none"}
        onClick={onRowClick}
        {...swipe.handlers}
      />
      <button type="button" data-testid="close" onClick={swipe.close}>
        close
      </button>
    </div>
  );
}

/**
 * Dispatch a pointer event with an explicit `timeStamp` so a release can be
 * made deliberately slow (below flick velocity) — jsdom stamps synchronously
 * fired events microseconds apart, which would read as a flick otherwise.
 */
function firePointer(
  el: Element,
  type: "pointerdown" | "pointermove" | "pointerup",
  init: { pointerId: number; clientX: number; clientY: number; timeStamp?: number },
) {
  const event = new MouseEvent(type, {
    bubbles: true,
    cancelable: true,
    clientX: init.clientX,
    clientY: init.clientY,
  });
  Object.defineProperty(event, "pointerId", { value: init.pointerId });
  if (init.timeStamp !== undefined) {
    Object.defineProperty(event, "timeStamp", { value: init.timeStamp });
  }
  fireEvent(el, event);
}

afterEach(() => {
  vi.restoreAllMocks();
});

describe("useSwipeActions", () => {
  it("tracks a clamped live dx once horizontal intent is established", () => {
    mockRectWidth(320);
    const onCommit = vi.fn();
    render(<Fixture trailing={{ onCommit }} />);
    const row = screen.getByTestId("row");

    fireEvent.pointerDown(row, { pointerId: 1, clientX: 200, clientY: 30 });
    fireEvent.pointerMove(row, { pointerId: 1, clientX: 120, clientY: 30 });
    expect(row.dataset.dx).toBe("-80");
    expect(row.dataset.dragging).toBe("true");
    expect(row.dataset.committing).toBe("none");
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("clamps the drag to sides that have actions", () => {
    mockRectWidth(320);
    render(<Fixture trailing={{ onCommit: vi.fn() }} />);
    const row = screen.getByTestId("row");

    // No leading config: a rightward drag never moves the row.
    fireEvent.pointerDown(row, { pointerId: 1, clientX: 100, clientY: 30 });
    fireEvent.pointerMove(row, { pointerId: 1, clientX: 260, clientY: 30 });
    expect(row.dataset.dx).toBe("0");
  });

  it("flags committing once the drag crosses half the width", () => {
    mockRectWidth(320);
    render(<Fixture trailing={{ onCommit: vi.fn() }} />);
    const row = screen.getByTestId("row");

    fireEvent.pointerDown(row, { pointerId: 1, clientX: 300, clientY: 30 });
    fireEvent.pointerMove(row, { pointerId: 1, clientX: 120, clientY: 30 });
    expect(row.dataset.dx).toBe("-180");
    expect(row.dataset.committing).toBe("trailing");
  });

  it("commits on release past half the width and resets dx", () => {
    mockRectWidth(320);
    const onCommit = vi.fn();
    render(<Fixture trailing={{ onCommit }} />);
    const row = screen.getByTestId("row");

    fireEvent.pointerDown(row, { pointerId: 1, clientX: 300, clientY: 30 });
    fireEvent.pointerMove(row, { pointerId: 1, clientX: 120, clientY: 30 });
    fireEvent.pointerUp(row, { pointerId: 1, clientX: 120, clientY: 30 });
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(row.dataset.dx).toBe("0");
    expect(row.dataset.revealed).toBe("none");
  });

  it("commits a fast flick below half the width", () => {
    mockRectWidth(320);
    const onCommit = vi.fn();
    render(<Fixture trailing={{ onCommit }} />);
    const row = screen.getByTestId("row");

    // ~60px within the few ms between synchronously-fired events: a flick.
    fireEvent.pointerDown(row, { pointerId: 1, clientX: 200, clientY: 30 });
    fireEvent.pointerMove(row, { pointerId: 1, clientX: 140, clientY: 30 });
    fireEvent.pointerUp(row, { pointerId: 1, clientX: 140, clientY: 30 });
    expect(onCommit).toHaveBeenCalledTimes(1);
  });

  it("snaps back on a slow release below every threshold", () => {
    mockRectWidth(320);
    const onCommit = vi.fn();
    render(<Fixture trailing={{ onCommit }} />);
    const row = screen.getByTestId("row");

    firePointer(row, "pointerdown", { pointerId: 1, clientX: 200, clientY: 30, timeStamp: 1000 });
    firePointer(row, "pointermove", { pointerId: 1, clientX: 140, clientY: 30, timeStamp: 1200 });
    // 60px over 400ms = 0.15 px/ms — no flick, below half width: snap back.
    firePointer(row, "pointerup", { pointerId: 1, clientX: 140, clientY: 30, timeStamp: 1400 });
    expect(onCommit).not.toHaveBeenCalled();
    expect(row.dataset.dx).toBe("0");
    expect(row.dataset.revealed).toBe("none");
  });

  it("settles open at the reveal width on a slow release past half the reveal", () => {
    mockRectWidth(320);
    const onCommit = vi.fn();
    render(<Fixture trailing={{ onCommit, revealPx: 144 }} />);
    const row = screen.getByTestId("row");

    firePointer(row, "pointerdown", { pointerId: 1, clientX: 300, clientY: 30, timeStamp: 1000 });
    firePointer(row, "pointermove", { pointerId: 1, clientX: 200, clientY: 30, timeStamp: 1200 });
    // 100px over 400ms: no flick; ≥ 72 (revealPx/2) but < 160 (half width).
    firePointer(row, "pointerup", { pointerId: 1, clientX: 200, clientY: 30, timeStamp: 1400 });
    expect(onCommit).not.toHaveBeenCalled();
    expect(row.dataset.revealed).toBe("trailing");
    expect(row.dataset.dx).toBe("-144");
  });

  it("closes a settled reveal via close()", () => {
    mockRectWidth(320);
    render(<Fixture trailing={{ onCommit: vi.fn(), revealPx: 144 }} />);
    const row = screen.getByTestId("row");

    firePointer(row, "pointerdown", { pointerId: 1, clientX: 300, clientY: 30, timeStamp: 1000 });
    firePointer(row, "pointermove", { pointerId: 1, clientX: 200, clientY: 30, timeStamp: 1200 });
    firePointer(row, "pointerup", { pointerId: 1, clientX: 200, clientY: 30, timeStamp: 1400 });
    expect(row.dataset.revealed).toBe("trailing");

    fireEvent.click(screen.getByTestId("close"));
    expect(row.dataset.revealed).toBe("none");
    expect(row.dataset.dx).toBe("0");
  });

  it("continues a drag from the settled reveal into a commit", () => {
    mockRectWidth(320);
    const onCommit = vi.fn();
    render(<Fixture trailing={{ onCommit, revealPx: 144 }} />);
    const row = screen.getByTestId("row");

    firePointer(row, "pointerdown", { pointerId: 1, clientX: 300, clientY: 30, timeStamp: 1000 });
    firePointer(row, "pointermove", { pointerId: 1, clientX: 200, clientY: 30, timeStamp: 1200 });
    firePointer(row, "pointerup", { pointerId: 1, clientX: 200, clientY: 30, timeStamp: 1400 });
    expect(row.dataset.revealed).toBe("trailing");

    // A second drag starts from -144: another 40px crosses half the width.
    firePointer(row, "pointerdown", { pointerId: 2, clientX: 200, clientY: 30, timeStamp: 2000 });
    firePointer(row, "pointermove", { pointerId: 2, clientX: 160, clientY: 30, timeStamp: 2200 });
    expect(row.dataset.dx).toBe("-184");
    expect(row.dataset.committing).toBe("trailing");
    firePointer(row, "pointerup", { pointerId: 2, clientX: 160, clientY: 30, timeStamp: 2400 });
    expect(onCommit).toHaveBeenCalledTimes(1);
    expect(row.dataset.revealed).toBe("none");
  });

  it("bails out to native scrolling when vertical intent wins", () => {
    mockRectWidth(320);
    const onCommit = vi.fn();
    render(<Fixture trailing={{ onCommit }} />);
    const row = screen.getByTestId("row");

    fireEvent.pointerDown(row, { pointerId: 1, clientX: 200, clientY: 100 });
    fireEvent.pointerMove(row, { pointerId: 1, clientX: 195, clientY: 140 });
    expect(row.dataset.dx).toBe("0");
    expect(row.dataset.dragging).toBe("false");
    // Even a later large horizontal move stays bailed for this pointer.
    fireEvent.pointerMove(row, { pointerId: 1, clientX: 40, clientY: 140 });
    expect(row.dataset.dx).toBe("0");
    fireEvent.pointerUp(row, { pointerId: 1, clientX: 40, clientY: 140 });
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("commits the leading side independently", () => {
    mockRectWidth(320);
    const onLeading = vi.fn();
    const onTrailing = vi.fn();
    render(<Fixture leading={{ onCommit: onLeading }} trailing={{ onCommit: onTrailing }} />);
    const row = screen.getByTestId("row");

    fireEvent.pointerDown(row, { pointerId: 1, clientX: 60, clientY: 30 });
    fireEvent.pointerMove(row, { pointerId: 1, clientX: 260, clientY: 30 });
    expect(row.dataset.committing).toBe("leading");
    fireEvent.pointerUp(row, { pointerId: 1, clientX: 260, clientY: 30 });
    expect(onLeading).toHaveBeenCalledTimes(1);
    expect(onTrailing).not.toHaveBeenCalled();
  });

  it("is inert while disabled", () => {
    mockRectWidth(320);
    const onCommit = vi.fn();
    render(<Fixture enabled={false} trailing={{ onCommit }} />);
    const row = screen.getByTestId("row");

    fireEvent.pointerDown(row, { pointerId: 1, clientX: 300, clientY: 30 });
    fireEvent.pointerMove(row, { pointerId: 1, clientX: 100, clientY: 30 });
    fireEvent.pointerUp(row, { pointerId: 1, clientX: 100, clientY: 30 });
    expect(row.dataset.dx).toBe("0");
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("resets the drag on pointercancel without committing", () => {
    mockRectWidth(320);
    const onCommit = vi.fn();
    render(<Fixture trailing={{ onCommit }} />);
    const row = screen.getByTestId("row");

    fireEvent.pointerDown(row, { pointerId: 1, clientX: 300, clientY: 30 });
    fireEvent.pointerMove(row, { pointerId: 1, clientX: 100, clientY: 30 });
    fireEvent.pointerCancel(row, { pointerId: 1 });
    expect(row.dataset.dx).toBe("0");
    expect(onCommit).not.toHaveBeenCalled();
  });

  it("suppresses the synthetic click after a drag that snaps back", () => {
    mockRectWidth(320);
    const onRowClick = vi.fn();
    render(<Fixture trailing={{ onCommit: vi.fn() }} onRowClick={onRowClick} />);
    const row = screen.getByTestId("row");

    // Drag past the intent slop, then release back at the origin (snap-back).
    fireEvent.pointerDown(row, { pointerId: 1, clientX: 300, clientY: 30 });
    fireEvent.pointerMove(row, { pointerId: 1, clientX: 280, clientY: 30 });
    fireEvent.pointerUp(row, { pointerId: 1, clientX: 300, clientY: 30 });
    fireEvent.click(row);
    expect(onRowClick).not.toHaveBeenCalled();
  });

  it("does not suppress a plain tap (no drag past the slop)", () => {
    mockRectWidth(320);
    const onRowClick = vi.fn();
    render(<Fixture trailing={{ onCommit: vi.fn() }} onRowClick={onRowClick} />);
    const row = screen.getByTestId("row");

    fireEvent.pointerDown(row, { pointerId: 1, clientX: 300, clientY: 30 });
    fireEvent.pointerUp(row, { pointerId: 1, clientX: 300, clientY: 30 });
    fireEvent.click(row);
    expect(onRowClick).toHaveBeenCalledTimes(1);
  });

  it("does not suppress the deliberate tap that follows a settled reveal", () => {
    mockRectWidth(320);
    const onRowClick = vi.fn();
    render(<Fixture trailing={{ onCommit: vi.fn(), revealPx: 144 }} onRowClick={onRowClick} />);
    const row = screen.getByTestId("row");

    // Settle open (past revealPx/2, below the commit threshold); a slow release
    // (explicit timestamps) so it is not misread as a flick.
    firePointer(row, "pointerdown", { pointerId: 1, clientX: 300, clientY: 30, timeStamp: 1000 });
    firePointer(row, "pointermove", { pointerId: 1, clientX: 200, clientY: 30, timeStamp: 1200 });
    firePointer(row, "pointerup", { pointerId: 1, clientX: 200, clientY: 30, timeStamp: 1400 });
    expect(row.dataset.revealed).toBe("trailing");
    // The next tap is deliberate and must reach onClick (to close the reveal).
    fireEvent.click(row);
    expect(onRowClick).toHaveBeenCalledTimes(1);
  });
});
