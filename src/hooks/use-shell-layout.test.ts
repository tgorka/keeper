import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useShellLayout } from "@/hooks/use-shell-layout";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";

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
  capabilitiesStore.setState({ capabilities: DEFAULT_CAPABILITIES, hydrated: false });
});

describe("useShellLayout", () => {
  it("keeps sidebar expanded and detail pinned at wide widths (>=1280)", () => {
    mockViewportWidth(1440);
    const { result } = renderHook(() => useShellLayout());
    expect(result.current.phone).toBe(false);
    expect(result.current.sidebarCollapsed).toBe(false);
    expect(result.current.detailFloating).toBe(false);
  });

  it("floats the detail panel but keeps the sidebar between 1080 and 1280", () => {
    mockViewportWidth(1200);
    const { result } = renderHook(() => useShellLayout());
    expect(result.current.phone).toBe(false);
    expect(result.current.sidebarCollapsed).toBe(false);
    expect(result.current.detailFloating).toBe(true);
  });

  it("collapses the sidebar and floats the detail below 1080", () => {
    mockViewportWidth(1000);
    const { result } = renderHook(() => useShellLayout());
    expect(result.current.phone).toBe(false);
    expect(result.current.sidebarCollapsed).toBe(true);
    expect(result.current.detailFloating).toBe(true);
  });

  it("activates the phone tier below 768", () => {
    mockViewportWidth(700);
    const { result } = renderHook(() => useShellLayout());
    expect(result.current.phone).toBe(true);
    // The narrower tiers still report collapsed/floating — the phone flag is
    // additive, not a replacement for the existing tiers.
    expect(result.current.sidebarCollapsed).toBe(true);
    expect(result.current.detailFloating).toBe(true);
  });

  it("keeps the phone tier off at exactly 768 with the tablet flags unchanged", () => {
    mockViewportWidth(768);
    const { result } = renderHook(() => useShellLayout());
    expect(result.current.phone).toBe(false);
    expect(result.current.sidebarCollapsed).toBe(true);
    expect(result.current.detailFloating).toBe(true);
  });

  it("turns the phone tier on at 767, the last phone width", () => {
    mockViewportWidth(767);
    const { result } = renderHook(() => useShellLayout());
    expect(result.current.phone).toBe(true);
  });

  // ── The tier is the platform's, not the width's (Epic 65, AD-189) ──────────
  it("is the phone tier on a reduced-capability platform at a landscape width", () => {
    // An iPhone 14 Pro Max rotated: 932px wide, which the width rule alone read
    // as the desktop frame at 430px tall. Every tier-telling flag absent and
    // hydrated is what `isReducedCapabilityPlatform` calls the phone.
    mockViewportWidth(932);
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    const { result } = renderHook(() => useShellLayout());
    expect(result.current.phone).toBe(true);
  });

  it("keeps the desktop frame on a desktop at 1440", () => {
    mockViewportWidth(1440);
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, nativeMenuBar: true });
    const { result } = renderHook(() => useShellLayout());
    expect(result.current.phone).toBe(false);
  });

  it("keeps the width rule on a desktop below 768", () => {
    // The dev harness and a narrow Mac window: the platform is not reduced, so
    // the width still decides, exactly as before.
    mockViewportWidth(700);
    capabilitiesStore.getState().applySnapshot({ ...DEFAULT_CAPABILITIES, nativeMenuBar: true });
    const { result } = renderHook(() => useShellLayout());
    expect(result.current.phone).toBe(true);
  });

  it("leaves the column rules to the width on a reduced platform", () => {
    // Only the tier is the platform's; the phone flag stays additive over the
    // collapsed/floating tiers, which at 932px both read as the wide desktop.
    mockViewportWidth(932);
    capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    const { result } = renderHook(() => useShellLayout());
    expect(result.current.sidebarCollapsed).toBe(true);
    expect(result.current.detailFloating).toBe(true);
  });

  it("decides by width alone until the platform has answered", () => {
    // Unhydrated: the width is the only fact, and it says desktop at 932.
    mockViewportWidth(932);
    const { result } = renderHook(() => useShellLayout());
    expect(result.current.phone).toBe(false);
  });

  it("switches to the phone tier the moment the platform answers reduced", () => {
    mockViewportWidth(932);
    const { result } = renderHook(() => useShellLayout());
    expect(result.current.phone).toBe(false);
    act(() => {
      capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
    });
    expect(result.current.phone).toBe(true);
  });
});
