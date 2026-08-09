import "@testing-library/jest-dom/vitest";
import { vi } from "vitest";

// jsdom does not implement matchMedia; the shell hook and theme provider need it.
if (!window.matchMedia) {
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    matches: false,
    media: query,
    onchange: null,
    addListener: vi.fn(),
    removeListener: vi.fn(),
    addEventListener: vi.fn(),
    removeEventListener: vi.fn(),
    dispatchEvent: vi.fn(),
  }));
}

// jsdom lacks the Pointer Capture and scrollIntoView APIs that Radix UI
// (DropdownMenu / AlertDialog and friends) touch on open. Stub them so those
// primitives can mount and open under Testing Library's `fireEvent`.
if (!Element.prototype.hasPointerCapture) {
  Element.prototype.hasPointerCapture = vi.fn(() => false);
}
if (!Element.prototype.setPointerCapture) {
  Element.prototype.setPointerCapture = vi.fn();
}
if (!Element.prototype.releasePointerCapture) {
  Element.prototype.releasePointerCapture = vi.fn();
}
if (!Element.prototype.scrollIntoView) {
  Element.prototype.scrollIntoView = vi.fn();
}

// jsdom lacks ResizeObserver, which Radix's Popper (DropdownMenu content) uses
// to position itself; without it the menu content never mounts.
if (!globalThis.ResizeObserver) {
  globalThis.ResizeObserver = class {
    observe() {}
    unobserve() {}
    disconnect() {}
  };
}

// jsdom performs no layout, so every element reports a 0x0 bounding rect. A
// virtualised list asks the scroll element how tall it is and renders a window
// of that height, which means an unshimmed jsdom renders exactly zero rows and
// every assertion about the note list fails for a reason that has nothing to do
// with the component.
//
// The shim answers with one screen — and ONLY when the real answer is all
// zeros, so any test that arranges a real geometry still sees its own numbers.
const VIEWPORT = { width: 1024, height: 768 };
const measure = Element.prototype.getBoundingClientRect;
Element.prototype.getBoundingClientRect = function shimmedRect(this: Element): DOMRect {
  const real = measure.call(this);
  if (real.width !== 0 || real.height !== 0 || real.x !== 0 || real.y !== 0) {
    return real;
  }
  return {
    ...VIEWPORT,
    x: 0,
    y: 0,
    top: 0,
    left: 0,
    right: VIEWPORT.width,
    bottom: VIEWPORT.height,
    toJSON: () => ({}),
  } as DOMRect;
};
