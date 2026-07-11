import { act, renderHook } from "@testing-library/react";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { useKeyboardInset } from "@/hooks/use-keyboard-inset";

/**
 * A minimal `visualViewport` stand-in: an `EventTarget` (real add/remove/dispatch
 * semantics) carrying the mutable `height`/`offsetTop` the hook reads. jsdom has
 * no VisualViewport, so tests install this on `window` per-case.
 */
class MockVisualViewport extends EventTarget {
  height: number;
  offsetTop: number;
  scale = 1;

  constructor(height: number, offsetTop = 0) {
    super();
    this.height = height;
    this.offsetTop = offsetTop;
  }
}

const LAYOUT_HEIGHT = 700;

function installVisualViewport(viewport: MockVisualViewport | undefined) {
  Object.defineProperty(window, "visualViewport", {
    value: viewport as unknown as VisualViewport | undefined,
    configurable: true,
    writable: true,
  });
}

/** The current `--kb-inset` inline value on the document root ("" when unset). */
function kbInset(): string {
  return document.documentElement.style.getPropertyValue("--kb-inset");
}

beforeEach(() => {
  Object.defineProperty(window, "innerHeight", {
    value: LAYOUT_HEIGHT,
    configurable: true,
    writable: true,
  });
});

afterEach(() => {
  installVisualViewport(undefined);
  document.documentElement.style.removeProperty("--kb-inset");
});

describe("useKeyboardInset", () => {
  it("writes the keyboard-covered height to --kb-inset on a visualViewport resize", () => {
    const viewport = new MockVisualViewport(LAYOUT_HEIGHT);
    installVisualViewport(viewport);
    renderHook(() => useKeyboardInset({ enabled: true }));

    // Keyboard closed: the visual viewport fills the layout viewport → 0px.
    expect(kbInset()).toBe("0px");

    // Keyboard opens: the visual viewport shrinks by the covered height.
    act(() => {
      viewport.height = 400;
      viewport.dispatchEvent(new Event("resize"));
    });
    expect(kbInset()).toBe("300px");
  });

  it("returns --kb-inset to 0px when the keyboard dismisses (viewport restores)", () => {
    const viewport = new MockVisualViewport(400);
    installVisualViewport(viewport);
    renderHook(() => useKeyboardInset({ enabled: true }));
    expect(kbInset()).toBe("300px");

    act(() => {
      viewport.height = LAYOUT_HEIGHT;
      viewport.dispatchEvent(new Event("resize"));
    });
    expect(kbInset()).toBe("0px");
  });

  it("recomputes on visualViewport scroll, subtracting offsetTop", () => {
    const viewport = new MockVisualViewport(400);
    installVisualViewport(viewport);
    renderHook(() => useKeyboardInset({ enabled: true }));
    expect(kbInset()).toBe("300px");

    // The visual viewport scrolls down within the layout viewport: the band the
    // keyboard covers at the bottom shrinks by the top offset.
    act(() => {
      viewport.offsetTop = 100;
      viewport.dispatchEvent(new Event("scroll"));
    });
    expect(kbInset()).toBe("200px");
  });

  it("clamps the inset at 0 (a zoomed visual viewport never goes negative)", () => {
    const viewport = new MockVisualViewport(LAYOUT_HEIGHT + 50);
    installVisualViewport(viewport);
    renderHook(() => useKeyboardInset({ enabled: true }));
    expect(kbInset()).toBe("0px");
  });

  it("suppresses the phantom inset while pinch-zoomed (scale > 1)", () => {
    const viewport = new MockVisualViewport(400);
    viewport.scale = 2;
    installVisualViewport(viewport);
    renderHook(() => useKeyboardInset({ enabled: true }));

    // A zoomed visual viewport shrinks for zoom, not the keyboard — no inset.
    expect(kbInset()).toBe("0px");

    // Restoring scale re-enables the real keyboard inset on the next event.
    act(() => {
      viewport.scale = 1;
      viewport.dispatchEvent(new Event("resize"));
    });
    expect(kbInset()).toBe("300px");
  });

  it("recomputes on a window resize (orientation change updating innerHeight)", () => {
    const viewport = new MockVisualViewport(400);
    installVisualViewport(viewport);
    renderHook(() => useKeyboardInset({ enabled: true }));
    expect(kbInset()).toBe("300px");

    // A rotate can change the layout viewport (innerHeight) without a
    // visualViewport event; the window resize listener keeps the inset fresh.
    act(() => {
      Object.defineProperty(window, "innerHeight", {
        value: 500,
        configurable: true,
        writable: true,
      });
      window.dispatchEvent(new Event("resize"));
    });
    expect(kbInset()).toBe("100px");
  });

  it("restores 0px and removes its listeners on unmount", () => {
    const viewport = new MockVisualViewport(400);
    installVisualViewport(viewport);
    const removeSpy = vi.spyOn(viewport, "removeEventListener");
    const { unmount } = renderHook(() => useKeyboardInset({ enabled: true }));
    expect(kbInset()).toBe("300px");

    unmount();
    expect(kbInset()).toBe("0px");
    expect(removeSpy).toHaveBeenCalledWith("resize", expect.any(Function));
    expect(removeSpy).toHaveBeenCalledWith("scroll", expect.any(Function));

    // A late viewport event after cleanup must not resurrect the inset.
    act(() => {
      viewport.height = 200;
      viewport.dispatchEvent(new Event("resize"));
    });
    expect(kbInset()).toBe("0px");
  });

  it("does nothing while disabled (desktop/tablet tier)", () => {
    const viewport = new MockVisualViewport(400);
    installVisualViewport(viewport);
    const addSpy = vi.spyOn(viewport, "addEventListener");
    renderHook(() => useKeyboardInset({ enabled: false }));

    expect(kbInset()).toBe("");
    expect(addSpy).not.toHaveBeenCalled();
  });

  it("is a no-op when window.visualViewport is absent (jsdom / old webviews)", () => {
    installVisualViewport(undefined);
    const { unmount } = renderHook(() => useKeyboardInset({ enabled: true }));
    expect(kbInset()).toBe("");
    unmount();
    expect(kbInset()).toBe("");
  });
});
