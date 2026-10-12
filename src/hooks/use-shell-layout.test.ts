import { act, renderHook } from "@testing-library/react";
import { afterEach, describe, expect, it, vi } from "vitest";
import { useShellLayout } from "@/hooks/use-shell-layout";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { platformStore } from "@/lib/stores/platform";

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

/**
 * A viewport of `width` × `height` that answers `max-width`, `min-width` and
 * `min-height` features, joined by `and`, the way a browser does. `resize`
 * changes it and fires every registered `change` listener, as a browser does
 * when the on-screen keyboard resizes the layout viewport.
 */
function mockViewport(width: number, height: number) {
  const viewport = { width, height };
  const listeners = new Set<() => void>();
  window.matchMedia = vi.fn().mockImplementation((query: string) => ({
    get matches() {
      return query.split(" and ").every((feature) => {
        const [, name, px] = feature.match(/(max-width|min-width|min-height):\s*(\d+)px/) ?? [];
        const bound = Number(px);
        return name === "max-width"
          ? viewport.width <= bound
          : name === "min-width"
            ? viewport.width >= bound
            : name === "min-height"
              ? viewport.height >= bound
              : true;
      });
    },
    media: query,
    onchange: null,
    addEventListener: (_: string, listener: () => void) => listeners.add(listener),
    removeEventListener: (_: string, listener: () => void) => listeners.delete(listener),
    addListener: vi.fn(),
    removeListener: vi.fn(),
    dispatchEvent: vi.fn(),
  }));
  return {
    resize(nextWidth: number, nextHeight: number) {
      viewport.width = nextWidth;
      viewport.height = nextHeight;
      for (const listener of listeners) {
        listener();
      }
    },
  };
}

// ── The tablet (Epic 98, story 98.3 #5, UX-DR146) ─────────────────────────────
// Android's capabilities are the phone's: every `cfg!(desktop)` flag false.
// Whether the device is a tablet is Rust's fact (`PlatformVm.tablet`).
describe("useShellLayout on an Android tablet", () => {
  const ANDROID: typeof DEFAULT_CAPABILITIES = {
    ...DEFAULT_CAPABILITIES,
    bots: true,
    sync: true,
    notes: true,
    sessions: true,
  };
  const facts = (tablet: boolean) => ({ urlForm: "localhost" as const, tablet, disclosure: null });

  afterEach(() => {
    platformStore.getState().applySnapshot({ urlForm: "scheme", tablet: false, disclosure: null });
  });

  it.each([
    // [width, height, device is a tablet, tablet, split]
    [800, 1280, true, true, false], // a tablet upright: the phone's stack, plus the dock
    [1280, 800, true, true, true], // a tablet on its side: rooms list and room side by side
    [840, 600, true, true, true], // the first split width
    [839, 1280, true, true, false], // one dp short of the split
    [430, 932, false, false, false], // a phone upright
    [932, 430, false, false, false], // a phone on its side is still a phone
    [1000, 700, false, false, false], // a phone-class device with a large viewport stays a phone
  ])("at %i × %i on a device Rust calls a tablet: %s, is a tablet: %s, side by side: %s", (width, height, device, tablet, split) => {
    mockViewport(width, height);
    capabilitiesStore.getState().applySnapshot(ANDROID);
    platformStore.getState().applySnapshot(facts(device));
    const { result } = renderHook(() => useShellLayout());
    expect(result.current).toMatchObject({ phone: true, tablet, split });
  });

  it("stays a tablet, side by side, when the keyboard shrinks the viewport", () => {
    const viewport = mockViewport(1280, 800);
    capabilitiesStore.getState().applySnapshot(ANDROID);
    platformStore.getState().applySnapshot(facts(true));
    const { result } = renderHook(() => useShellLayout());
    expect(result.current).toMatchObject({ tablet: true, split: true });
    // A landscape keyboard takes well over 200 dp of the layout viewport
    // (`interactive-widget=resizes-content`).
    act(() => viewport.resize(1280, 450));
    expect(result.current).toMatchObject({ phone: true, tablet: true, split: true });
  });

  it("never calls a desktop window a tablet, however large", () => {
    mockViewport(1280, 800);
    capabilitiesStore.getState().applySnapshot({
      ...DEFAULT_CAPABILITIES,
      trayIcon: true,
      globalHotkey: true,
    });
    platformStore.getState().applySnapshot(facts(true));
    const { result } = renderHook(() => useShellLayout());
    expect(result.current).toMatchObject({ phone: false, tablet: false, split: false });
  });
});
