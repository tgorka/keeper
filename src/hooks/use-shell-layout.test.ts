import { renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useShellLayout } from "@/hooks/use-shell-layout";

/**
 * Mock matchMedia so that any query with a `max-width: <bp>` matches when the
 * simulated viewport width is below that breakpoint.
 */
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

afterEach(() => {
  vi.restoreAllMocks();
});

describe("useShellLayout", () => {
  it("keeps sidebar expanded and detail pinned at wide widths (>=1280)", () => {
    mockViewportWidth(1440);
    const { result } = renderHook(() => useShellLayout());
    expect(result.current.sidebarCollapsed).toBe(false);
    expect(result.current.detailFloating).toBe(false);
  });

  it("floats the detail panel but keeps the sidebar between 1080 and 1280", () => {
    mockViewportWidth(1200);
    const { result } = renderHook(() => useShellLayout());
    expect(result.current.sidebarCollapsed).toBe(false);
    expect(result.current.detailFloating).toBe(true);
  });

  it("collapses the sidebar and floats the detail below 1080", () => {
    mockViewportWidth(1000);
    const { result } = renderHook(() => useShellLayout());
    expect(result.current.sidebarCollapsed).toBe(true);
    expect(result.current.detailFloating).toBe(true);
  });
});
