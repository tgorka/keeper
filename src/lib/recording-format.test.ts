import { describe, expect, it } from "vitest";
import { bytesToWholeMb, formatSize } from "@/lib/recording-format";

describe("formatSize", () => {
  it("renders whole decimal MB below 1000 MB", () => {
    // Mirrors the Rust `format_size` doc example (truncating whole MB).
    expect(formatSize(412_800_000)).toBe("412 MB");
  });

  it("truncates rather than rounding up (never overstates disk usage)", () => {
    expect(formatSize(431_800_000)).toBe("431 MB");
  });

  it("rolls to one-decimal GB at ≥ 1000 MB", () => {
    expect(formatSize(1_290_000_000)).toBe("1.2 GB");
  });

  it("renders 0 MB for an empty session", () => {
    expect(formatSize(0)).toBe("0 MB");
  });

  it("crosses the GB boundary exactly at 1000 MB", () => {
    expect(formatSize(999_999_999)).toBe("999 MB");
    expect(formatSize(1_000_000_000)).toBe("1.0 GB");
  });
});

describe("bytesToWholeMb", () => {
  it("rounds a byte count to whole decimal MB", () => {
    expect(bytesToWholeMb(412_000_000)).toBe(412);
  });

  it("truncates rather than rounding up (stays consistent with the size line)", () => {
    expect(bytesToWholeMb(412_600_000)).toBe(412);
    expect(bytesToWholeMb(999_900_000)).toBe(999);
  });

  it("is 0 for an empty segment", () => {
    expect(bytesToWholeMb(0)).toBe(0);
  });
});
