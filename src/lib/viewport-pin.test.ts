import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const { frontendErrorReport } = vi.hoisted(() => ({ frontendErrorReport: vi.fn(async () => {}) }));
vi.mock("@/lib/ipc/client", () => ({ frontendErrorReport }));

import { pinViewport } from "./viewport-pin";

describe("pinViewport", () => {
  let scrolledTo: [number, number][] = [];
  beforeEach(() => {
    scrolledTo = [];
    vi.spyOn(window, "scrollTo").mockImplementation(((x: number, y: number) => {
      scrolledTo.push([x, y]);
      Object.defineProperty(window, "scrollY", { configurable: true, value: y });
    }) as typeof window.scrollTo);
    pinViewport();
  });
  afterEach(() => {
    Object.defineProperty(window, "scrollY", { configurable: true, value: 0 });
    vi.restoreAllMocks();
  });

  it("puts a scrolled window back and says in the app log what reached below it", () => {
    Object.defineProperty(window, "scrollY", { configurable: true, value: 206 });
    window.dispatchEvent(new Event("scroll"));

    expect(scrolledTo).toContainEqual([0, 0]);
    expect(frontendErrorReport).toHaveBeenCalledWith(
      "layout",
      expect.stringContaining("the window scrolled to 206px"),
      expect.anything(),
      null,
    );
  });

  it("gives the document no overflow of its own", () => {
    expect(document.documentElement.style.overflow).toBe("hidden");
    expect(document.body.style.overflow).toBe("hidden");
  });
});
