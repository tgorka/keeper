/**
 * The app's focus and invalid indicators, declared once.
 *
 * This file exists because of a correction. The ring was measured and fixed in
 * `button.tsx` — shadcn's `ring-3 ring-ring/50` blends to #96a880 over the light
 * theme's card and measures 2.12:1 against it, under the 3:1 WCAG 2.4.11 asks of
 * a focus indicator, while passing at 3.14:1 in dark. That fix landed on exactly
 * ONE component. `input`, `textarea`, `select`, `checkbox`, `switch`,
 * `radio-group`, `input-group`, `tabs`, `scroll-area` and `badge` all kept the
 * broken default, so nine of the app's ten focusable primitives went on drawing
 * a focus ring nobody could see.
 *
 * The lesson is not "remember the other nine". A value that nine components must
 * agree on is a shared value, and pasting it nine times is how the codebase
 * ended up with 58 copies of the same patch the first time. It is declared here,
 * and a component that draws its own is a bug the tests fail on.
 *
 * Measured at full strength, against `--card` in each theme:
 *
 *   --ring          8.73:1 dark / 5.61:1 light   (focus, floor 3:1)
 *   --destructive   5.10:1 dark / 5.52:1 light   (invalid, floor 3:1)
 *
 * The invalid pair is the same story one state along. It drew at
 * `border-destructive/50` + `ring-destructive/20-40`, measuring 1.28:1 to 2.16:1
 * against a pane — which made the one moment a control must be unmissable the
 * least visible thing on the screen — and it drew at full strength in light and
 * half strength in dark, the one-value-two-themes trap again. One declaration,
 * both themes, full strength.
 *
 * These are literal strings and not composed ones on purpose: Tailwind scans
 * source text for class names, so a ring assembled at runtime from a prefix and
 * a suffix would generate no CSS at all. That is also why the `_WITHIN` pair is
 * spelled out rather than derived — it is the same decision projected through
 * `has-`, for the one primitive that draws its child's focus on its own edge.
 */

/** For anything that takes focus itself. */
export const FOCUS_RING = "focus-visible:border-ring focus-visible:ring-2 focus-visible:ring-ring";

/** For anything that can be invalid. */
export const INVALID_RING =
  "aria-invalid:border-destructive aria-invalid:ring-2 aria-invalid:ring-destructive";

/** For a wrapper that draws the focus of the control inside it. */
export const FOCUS_RING_WITHIN =
  "has-[[data-slot=input-group-control]:focus-visible]:border-ring has-[[data-slot=input-group-control]:focus-visible]:ring-2 has-[[data-slot=input-group-control]:focus-visible]:ring-ring";

/** For a wrapper that draws the validity of the control inside it. */
export const INVALID_RING_WITHIN =
  "has-[[data-slot][aria-invalid=true]]:border-destructive has-[[data-slot][aria-invalid=true]]:ring-2 has-[[data-slot][aria-invalid=true]]:ring-destructive";
