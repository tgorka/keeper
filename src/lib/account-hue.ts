/**
 * Per-account hue mapping (Story 2.1).
 *
 * The backend assigns each account a stable hue index (0–7) on the 8-hue wheel
 * and streams it on every account/inbox view model. Color values live only in
 * the theming layer (`src/index.css` defines `--account-hue-0..7`); this maps an
 * index to the corresponding CSS custom property, so the frontend never hardcodes
 * a color. An out-of-range index wraps into `0..8` defensively.
 */

/** Number of hues on the wheel — must match the backend `HUE_WHEEL_SIZE`. */
export const HUE_WHEEL_SIZE = 8;

/**
 * The CSS `var(--account-hue-N)` reference for a hue index, wrapping any
 * out-of-range value into `0..8`.
 */
export function accountHueVar(hueIndex: number): string {
  const n = ((Math.trunc(hueIndex) % HUE_WHEEL_SIZE) + HUE_WHEEL_SIZE) % HUE_WHEEL_SIZE;
  return `var(--account-hue-${n})`;
}
