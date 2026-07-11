import * as React from "react";

/**
 * Drive the `--kb-inset` CSS var from the `visualViewport` (Story 13.5, UX-DR25).
 *
 * The keyboard-avoidance engine for WKWebView, which neither resizes the layout
 * viewport under the on-screen keyboard nor implements
 * `interactive-widget=resizes-content`: the covered height is
 * `layoutViewportHeight - visualViewport.height - visualViewport.offsetTop`
 * (clamped ≥ 0) and is written to `--kb-inset` on `document.documentElement`,
 * where the phone composer footer consumes it as
 * `calc(var(--kb-inset, 0px) + var(--safe-bottom))`. Where the layout viewport
 * *does* resize (Chromium honoring the meta flag), the visual viewport matches
 * it and the computed inset is ≈ 0 — the two levers never double-count.
 *
 * Enabled only on the phone tier (mounted by `PhoneShell` with
 * `enabled: phone`); a no-op when `window.visualViewport` is absent
 * (desktop webviews without the API, jsdom). Cleanup removes the listeners and
 * restores `--kb-inset` to `0px` so a disable/unmount never strands an inset.
 * The var shift is layout, not animation — no transitions are attached, so
 * reduced-motion users see the same instantaneous reflow as everyone else.
 */
export function useKeyboardInset({ enabled }: { enabled: boolean }): void {
  React.useEffect(() => {
    if (!enabled) {
      return;
    }
    const viewport = window.visualViewport;
    if (!viewport) {
      // No visualViewport (jsdom, old webviews): --kb-inset stays 0px and the
      // composer sits at the safe-area bottom only.
      return;
    }
    const root = document.documentElement;

    const update = () => {
      // Pinch-zoom (`scale > 1`) shrinks the visual viewport and shifts its
      // offset for zoom, not the keyboard — that would otherwise read as a
      // phantom inset shoving the composer up, so suppress it while zoomed.
      if (viewport.scale > 1) {
        root.style.setProperty("--kb-inset", "0px");
        return;
      }
      // The layout viewport height minus the visual viewport's height and top
      // offset is the band the keyboard covers at the bottom; clamp ≥ 0 so a
      // pinch-zoomed visual viewport can never produce a negative inset.
      const inset = Math.max(0, window.innerHeight - viewport.height - viewport.offsetTop);
      root.style.setProperty("--kb-inset", `${inset}px`);
    };

    update();
    viewport.addEventListener("resize", update);
    viewport.addEventListener("scroll", update);
    // Orientation/rotation can update `window.innerHeight` (the layout viewport)
    // without emitting a visualViewport event; a window `resize` listener keeps
    // the inset from going stale after a rotate.
    window.addEventListener("resize", update);

    return () => {
      viewport.removeEventListener("resize", update);
      viewport.removeEventListener("scroll", update);
      window.removeEventListener("resize", update);
      root.style.setProperty("--kb-inset", "0px");
    };
  }, [enabled]);
}
