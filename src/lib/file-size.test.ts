import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { formatFileSize } from "@/lib/file-size";

/**
 * The vector table `keeper_core::size::format_file_size` is tested against
 * (Story 45.5, FR-178).
 *
 * Read from the Rust tree rather than copied into `src/`, and that direction is
 * deliberate: the fixture lives beside the canonical implementation, and the
 * mirror reaches for it. A copy would be a third thing to keep in step, which
 * is the problem this file exists to solve.
 *
 * `readFileSync` rather than an `import`: a JSON import outside the Vite root
 * depends on bundler configuration that a test should not be able to break, and
 * a missing file must be a loud failure here rather than an empty table that
 * passes.
 */
const FIXTURE = resolve(
  import.meta.dirname,
  "../../src-tauri/crates/keeper-core/src/file-size-vectors.json",
);

interface SizeVectors {
  base: number;
  vectors: { bytes: string; label: string; why: string }[];
}

const shared = JSON.parse(readFileSync(FIXTURE, "utf8")) as SizeVectors;

describe("formatFileSize", () => {
  /**
   * The whole point of this module. If this fails, either the TypeScript here
   * or `keeper-core/src/size.rs` has changed and the other has not, and two of
   * keeper's surfaces are about to disagree about how big a file is.
   */
  it("matches keeper-core on every shared vector", () => {
    expect(shared.base).toBe(1000);
    expect(shared.vectors.length).toBeGreaterThanOrEqual(25);
    for (const vector of shared.vectors) {
      expect(formatFileSize(BigInt(vector.bytes)), `${vector.bytes}: ${vector.why}`).toBe(
        vector.label,
      );
    }
  });

  /**
   * The same vectors again through the `number` overload, which is what the two
   * chat surfaces actually call.
   *
   * Skips the ones past `Number.MAX_SAFE_INTEGER`, because a `number` cannot
   * represent them and asserting a rounded input's output would be testing
   * IEEE-754 rather than this module.
   */
  it("agrees with itself whether given a number or a bigint", () => {
    for (const vector of shared.vectors) {
      const asBigInt = BigInt(vector.bytes);
      if (asBigInt > BigInt(Number.MAX_SAFE_INTEGER)) {
        continue;
      }
      expect(formatFileSize(Number(asBigInt))).toBe(vector.label);
    }
  });

  /**
   * The decimal decision, asserted where it is visible rather than only through
   * the shared table — so a reader of this file alone can see what base keeper
   * uses and a deletion of the fixture cannot take the claim with it.
   */
  it("is decimal, and never spells a binary unit", () => {
    expect(formatFileSize(999)).toBe("999 bytes");
    expect(formatFileSize(1_000)).toBe("1.0 kB");
    expect(formatFileSize(1_024)).toBe("1.0 kB");
    // The value that tells the two bases apart in a unit above kB: a 1024-based
    // formatter — which is what both chat surfaces used before this story —
    // renders this as "1.4 MB".
    expect(formatFileSize(1_500_000)).toBe("1.5 MB");
    for (const bytes of [1_024, 1_048_576, 1_073_741_824]) {
      expect(formatFileSize(bytes)).not.toMatch(/iB|KB/);
    }
  });

  /** Zero is a real size for a file, and one byte is singular. */
  it("spells small counts out, with a singular byte", () => {
    expect(formatFileSize(0)).toBe("0 bytes");
    expect(formatFileSize(1)).toBe("1 byte");
    expect(formatFileSize(2)).toBe("2 bytes");
  });

  /**
   * Remote data is not trusted. A Matrix event carrying a negative, fractional
   * or non-finite `size` must render something harmless, not blank a timeline
   * with a `RangeError` out of `BigInt()`.
   */
  it("renders a hostile size rather than throwing", () => {
    expect(formatFileSize(-1)).toBe("0 bytes");
    expect(formatFileSize(Number.NaN)).toBe("0 bytes");
    expect(formatFileSize(Number.POSITIVE_INFINITY)).toBe("0 bytes");
    expect(formatFileSize(1_500.7)).toBe("1.5 kB");
    expect(formatFileSize(-5n)).toBe("0 bytes");
  });
});
