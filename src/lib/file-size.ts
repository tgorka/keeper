/**
 * The TypeScript mirror of `keeper_core::size::format_file_size` (Story 45.5,
 * FR-178).
 *
 * # Why a mirror exists at all
 *
 * It should not, and almost nowhere does. Every size keeper shows for a file on
 * disk is formatted once in Rust and crosses the wire as a finished string —
 * `FilesEntryVm.size.label`. A surface that has a view model reads that label
 * and calls nothing here.
 *
 * The exception is the chat composer, which shows the size of a `File` the user
 * has just picked in a file dialog and has not uploaded yet. Those bytes exist
 * only in the webview; there is no Rust in that path and no view model to hang
 * a label on. Before this story that surface — and the media bubble beside it —
 * each carried their own formatter dividing by **1024 while printing `MB`**, so
 * a 1 500 000-byte attachment read as "1.4 MB" in keeper and "1.5 MB" in
 * Finder.
 *
 * # Why this cannot drift from Rust
 *
 * `src-tauri/crates/keeper-core/src/file-size-vectors.json` is a checked-in
 * table of byte counts and their exact renderings. The Rust unit test loads it
 * with `include_str!` and this module's test loads the same file from disk. A
 * change to either implementation fails on the commit that introduces it, which
 * is the difference between a mirror and a comment claiming there is one.
 *
 * The rules, restated because the code should be readable without the Rust
 * open: decimal (1 kB = 1000 bytes, matching Finder), SI spelling (`kB`, never
 * `KB` and never `KiB`), exact spelled-out counts below 1000 with a singular
 * "1 byte", one decimal place below ten and none at or above it, and truncation
 * rather than rounding so a figure never carries out of its own unit.
 *
 * A directory has no size. There is no directory branch here for the same
 * reason there is none in Rust: the absence is modelled by the caller holding
 * `null`, so a folder can never be handed a zero to render.
 */

/**
 * The unit ladder, decimal, largest first. Mirrors `UNITS` in
 * `keeper-core/src/size.rs`; `EB` is the top rung so that `u64::MAX` renders as
 * "18 EB" rather than as a figure in a unit that ran out.
 */
const UNITS: readonly (readonly [bigint, string])[] = [
  [1_000_000_000_000_000_000n, "EB"],
  [1_000_000_000_000_000n, "PB"],
  [1_000_000_000_000n, "TB"],
  [1_000_000_000n, "GB"],
  [1_000_000n, "MB"],
  [1_000n, "kB"],
] as const;

/**
 * Format a byte count exactly as `keeper_core::size::format_file_size` does.
 *
 * Accepts a `bigint` as well as a `number` because the shared vector table goes
 * up to `u64::MAX`, which is far past `Number.MAX_SAFE_INTEGER` — a mirror that
 * could not be asked the same questions as the original would not be pinned to
 * it. A non-finite or negative input is treated as zero: this renders a label,
 * and a label is never the right place to throw.
 *
 * The arithmetic is `bigint` throughout, matching Rust's integer division. A
 * float implementation agrees for every size a person will ever see and
 * disagrees at the top of the table, which is precisely where a formatter's
 * bugs live.
 */
export function formatFileSize(bytes: number | bigint): string {
  // Clamp before formatting. A `File.size` is always a non-negative integer,
  // but `media.size` comes off a Matrix event's `info` block, which is remote
  // data: another client can and does write a negative, fractional or absurd
  // size into a room. Coercing here is why a malformed event renders a
  // wrong-but-harmless "0 bytes" rather than "NaN bytes" or a thrown
  // `RangeError` that blanks a whole timeline.
  let count: bigint;
  if (typeof bytes === "bigint") {
    count = bytes < 0n ? 0n : bytes;
  } else {
    count = Number.isFinite(bytes) && bytes > 0 ? BigInt(Math.trunc(bytes)) : 0n;
  }
  if (count < 1_000n) {
    // The one place the unit is a word rather than a symbol, and the one place
    // the count is exact.
    return count === 1n ? "1 byte" : `${count} bytes`;
  }
  for (const [divisor, unit] of UNITS) {
    if (count < divisor) {
      continue;
    }
    // Divide the divisor before dividing by it, exactly as Rust does: the
    // alternative multiplies first and overflows a u64 near the top of the
    // ladder, and a mirror that is only correct where the original is easy is
    // not a mirror.
    const tenths = count / (divisor / 10n);
    const whole = tenths / 10n;
    const frac = tenths % 10n;
    return whole < 10n ? `${whole}.${frac} ${unit}` : `${whole} ${unit}`;
  }
  // Unreachable: `count >= 1000n` and the last rung is 1000n. Spelled as the
  // exact-byte form rather than a throw, for the reason above.
  return `${count} bytes`;
}
