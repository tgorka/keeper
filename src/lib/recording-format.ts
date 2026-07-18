/**
 * Recording size formatting (Story 18.3) — the TypeScript twin of the Rust
 * `keeper::tray::format_size`.
 *
 * The in-app active-recording banner and segment meter render byte figures that
 * must read identically to the menu-bar tray. Both surfaces consume the same
 * enriched `RecordingStatusVm` snapshot; these helpers mirror the tray's
 * (Rust-side) whole-MB / one-decimal-GB convention exactly so the two never
 * disagree by a digit.
 *
 * Decimal MB throughout (`10^6`, matching the sidecar's `segmentMB` and the
 * cap), truncating (never rounding up) so a figure never overstates what has
 * reached disk — {@link bytesToWholeMb} truncates too, so the meter's
 * `used / cap` caption never reads "full" (`1000 / 1000`) while its bar sits at
 * 99.9%.
 */

/** One decimal megabyte, in bytes (the `segmentMB` convention: `10^6`). */
const BYTES_PER_MB = 1_000_000;

/**
 * Format a byte count in whole decimal MB, rolling to one-decimal GB at
 * ≥ 1000 MB — the exact mirror of the Rust `format_size`: `412_800_000` →
 * `"412 MB"`, `1_290_000_000` → `"1.2 GB"`. Truncates (never rounds up), so the
 * figure never overstates what has reached disk.
 */
export function formatSize(bytes: number): string {
  const mb = Math.floor(bytes / BYTES_PER_MB);
  if (mb >= 1000) {
    // Whole tenths of a GB: 1_290_000_000 → 12 tenths → "1.2 GB".
    const tenths = Math.floor(bytes / 100_000_000);
    return `${Math.floor(tenths / 10)}.${tenths % 10} GB`;
  }
  return `${mb} MB`;
}

/**
 * A byte count as a whole number of decimal MB — the segment meter's `used` /
 * `cap` caption figures (`412_000_000` → `412`). Truncates like {@link formatSize}
 * (never rounds up), so the caption stays consistent with the truncating size
 * line and never reads "full" before the bar is; never negative.
 */
export function bytesToWholeMb(bytes: number): number {
  return Math.max(0, Math.floor(bytes / BYTES_PER_MB));
}
