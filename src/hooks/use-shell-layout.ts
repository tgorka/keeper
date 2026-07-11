import * as React from "react";

const PHONE_BREAKPOINT = 768;
const SIDEBAR_COLLAPSE_BREAKPOINT = 1080;
const DETAIL_FLOAT_BREAKPOINT = 1280;

export interface ShellLayout {
  /** Single-pane phone stack replaces the three-pane frame below 768px (Story 13.1). */
  phone: boolean;
  /** Sidebar collapses to a 48px icon rail below 1080px. */
  sidebarCollapsed: boolean;
  /** Detail panel opens as a Sheet (instead of pinned) below 1280px. */
  detailFloating: boolean;
}

export function useShellLayout(): ShellLayout {
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

  return layout;
}
