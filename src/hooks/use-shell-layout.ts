import * as React from "react";

const SIDEBAR_COLLAPSE_BREAKPOINT = 1080;
const DETAIL_FLOAT_BREAKPOINT = 1280;

export interface ShellLayout {
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
      return { sidebarCollapsed: false, detailFloating: false };
    }
    return {
      sidebarCollapsed: window.matchMedia(`(max-width: ${SIDEBAR_COLLAPSE_BREAKPOINT - 1}px)`)
        .matches,
      detailFloating: window.matchMedia(`(max-width: ${DETAIL_FLOAT_BREAKPOINT - 1}px)`).matches,
    };
  });

  React.useEffect(() => {
    const collapseQuery = window.matchMedia(`(max-width: ${SIDEBAR_COLLAPSE_BREAKPOINT - 1}px)`);
    const floatQuery = window.matchMedia(`(max-width: ${DETAIL_FLOAT_BREAKPOINT - 1}px)`);

    const onChange = () => {
      setLayout({
        sidebarCollapsed: collapseQuery.matches,
        detailFloating: floatQuery.matches,
      });
    };

    onChange();
    collapseQuery.addEventListener("change", onChange);
    floatQuery.addEventListener("change", onChange);

    return () => {
      collapseQuery.removeEventListener("change", onChange);
      floatQuery.removeEventListener("change", onChange);
    };
  }, []);

  return layout;
}
