import * as React from "react";

const REDUCED_MOTION_QUERY = "(prefers-reduced-motion: reduce)";

/**
 * Whether the user prefers reduced motion (Story 13.2). The single source for
 * the phone stack's cut-vs-slide decision: `true` renders push/pop as instant
 * cuts instead of transform slides. Mirrors the synchronous-init +
 * `change`-listener pattern of `use-shell-layout.ts` so a reduced-motion user
 * never sees one animated frame before the effect runs.
 */
export function useReducedMotion(): boolean {
  const [reduced, setReduced] = React.useState<boolean>(() => {
    // Initialize synchronously from the current preference so the first render
    // already honors it (no animated flash before the effect subscribes).
    if (typeof window === "undefined" || !window.matchMedia) {
      return false;
    }
    return window.matchMedia(REDUCED_MOTION_QUERY).matches;
  });

  React.useEffect(() => {
    const query = window.matchMedia(REDUCED_MOTION_QUERY);

    const onChange = () => {
      setReduced(query.matches);
    };

    onChange();
    query.addEventListener("change", onChange);

    return () => {
      query.removeEventListener("change", onChange);
    };
  }, []);

  return reduced;
}
