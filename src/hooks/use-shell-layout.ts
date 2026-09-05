import * as React from "react";
import {
  capabilitiesStore,
  isReducedCapabilityPlatform,
  useIsReducedCapabilityPlatform,
} from "@/lib/stores/capabilities";

const PHONE_BREAKPOINT = 768;
/**
 * Where the drawer becomes the 48px rail — exported because the width of the
 * sidebar is an input to the layout's own floor arithmetic, and the surface with
 * the most boxes in it (Tasks) has to fit both sides of this line.
 * `src/lib/window-minimum.test.ts` is the only reader.
 */
export const SIDEBAR_COLLAPSE_BREAKPOINT = 1080;
const DETAIL_FLOAT_BREAKPOINT = 1280;

export interface ShellLayout {
  /**
   * Single-pane phone stack instead of the three-pane frame (Story 13.1).
   *
   * True on a reduced-capability platform at EVERY width (Epic 65, AD-189):
   * an iPhone rotated to 932px is still an iPhone, and the desktop frame it
   * used to render there was clipped at 430px tall and wrote folds the phone
   * tier then inherited. Off such a platform the width rule stands — below
   * 768px the desktop dev harness and a narrow Mac window get the stack, as
   * they always have.
   *
   * Before Rust has answered `capabilities()` the platform is unknown and the
   * width is the only fact, so the width rule decides alone for those frames.
   * That is the honest default rather than "phone until told otherwise",
   * because it is exactly what every build has rendered until now and it
   * corrects itself the moment the answer lands; in practice the answer is
   * already in the store when the shell mounts (`App` requests it in the same
   * effect batch as `session_restore`, which the shell waits on). The reverse
   * guess would flash the phone stack over every desktop window instead.
   */
  phone: boolean;
  /** Sidebar collapses to a 48px icon rail below 1080px. */
  sidebarCollapsed: boolean;
  /** Detail panel opens as a Sheet (instead of pinned) below 1280px. */
  detailFloating: boolean;
}

/**
 * The phone-tier rule as one imperative read, for a caller with no render to
 * subscribe in (the palette's action map): the same two facts
 * {@link useShellLayout} folds — a reduced-capability platform at any width,
 * or a viewport below the breakpoint — read at the moment of the call.
 */
export function isPhoneTier(): boolean {
  return (
    isReducedCapabilityPlatform(capabilitiesStore.getState()) ||
    (typeof window !== "undefined" &&
      typeof window.matchMedia === "function" &&
      window.matchMedia(`(max-width: ${PHONE_BREAKPOINT - 1}px)`).matches)
  );
}

export function useShellLayout(): ShellLayout {
  const reduced = useIsReducedCapabilityPlatform();
  const [layout, setLayout] = React.useState<ShellLayout>(() => {
    // Initialize synchronously from the current viewport so a narrow window
    // does not flash the wide layout for one frame before the effect runs.
    if (typeof window === "undefined" || !window.matchMedia) {
      return { phone: false, sidebarCollapsed: false, detailFloating: false };
    }
    return {
      phone: window.matchMedia(`(max-width: ${PHONE_BREAKPOINT - 1}px)`).matches,
      sidebarCollapsed: window.matchMedia(`(max-width: ${SIDEBAR_COLLAPSE_BREAKPOINT - 1}px)`)
        .matches,
      detailFloating: window.matchMedia(`(max-width: ${DETAIL_FLOAT_BREAKPOINT - 1}px)`).matches,
    };
  });

  React.useEffect(() => {
    const phoneQuery = window.matchMedia(`(max-width: ${PHONE_BREAKPOINT - 1}px)`);
    const collapseQuery = window.matchMedia(`(max-width: ${SIDEBAR_COLLAPSE_BREAKPOINT - 1}px)`);
    const floatQuery = window.matchMedia(`(max-width: ${DETAIL_FLOAT_BREAKPOINT - 1}px)`);

    const onChange = () => {
      setLayout({
        phone: phoneQuery.matches,
        sidebarCollapsed: collapseQuery.matches,
        detailFloating: floatQuery.matches,
      });
    };

    onChange();
    phoneQuery.addEventListener("change", onChange);
    collapseQuery.addEventListener("change", onChange);
    floatQuery.addEventListener("change", onChange);

    return () => {
      phoneQuery.removeEventListener("change", onChange);
      collapseQuery.removeEventListener("change", onChange);
      floatQuery.removeEventListener("change", onChange);
    };
  }, []);

  // Only the tier is the platform's; the column rules stay the viewport's,
  // because they only ever matter on the desktop frame.
  return reduced ? { ...layout, phone: true } : layout;
}
