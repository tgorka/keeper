import { describe, expect, it } from "vitest";
import { formatMessageTime, formatRoomTimestamp } from "@/lib/format-time";

describe("formatRoomTimestamp", () => {
  it("shows a clock time (HH:MM) for a same-day timestamp", () => {
    const now = new Date(2026, 6, 4, 18, 0, 0).getTime();
    const earlierToday = new Date(2026, 6, 4, 9, 30, 0).getTime();
    const out = formatRoomTimestamp(earlierToday, now);
    // Contains hour and minute separated by a colon; no month name.
    expect(out).toMatch(/\d{1,2}:\d{2}/);
    expect(out).not.toMatch(/[A-Za-z]{3,}/);
  });

  it("shows a short date for a timestamp on a different day", () => {
    const now = new Date(2026, 6, 4, 18, 0, 0).getTime();
    const yesterday = new Date(2026, 6, 3, 9, 30, 0).getTime();
    const out = formatRoomTimestamp(yesterday, now);
    // A short date has no clock (no HH:MM).
    expect(out).not.toMatch(/\d{1,2}:\d{2}/);
    expect(out.length).toBeGreaterThan(0);
  });

  it("shows a date for a timestamp in a previous year", () => {
    const now = new Date(2026, 6, 4, 18, 0, 0).getTime();
    const lastYear = new Date(2025, 6, 4, 9, 30, 0).getTime();
    const out = formatRoomTimestamp(lastYear, now);
    expect(out).not.toMatch(/\d{1,2}:\d{2}/);
  });

  it("treats midnight boundaries as a different day", () => {
    const now = new Date(2026, 6, 4, 0, 5, 0).getTime();
    const justBeforeMidnight = new Date(2026, 6, 3, 23, 55, 0).getTime();
    const out = formatRoomTimestamp(justBeforeMidnight, now);
    expect(out).not.toMatch(/\d{1,2}:\d{2}/);
  });

  it("returns an empty string for non-finite or non-positive timestamps", () => {
    expect(formatRoomTimestamp(Number.NaN)).toBe("");
    expect(formatRoomTimestamp(0)).toBe("");
    expect(formatRoomTimestamp(-1)).toBe("");
    expect(formatRoomTimestamp(Number.POSITIVE_INFINITY)).toBe("");
  });
});

describe("formatMessageTime", () => {
  it("shows a clock time (HH:MM) for a valid timestamp", () => {
    const ms = new Date(2026, 6, 4, 9, 30, 0).getTime();
    const out = formatMessageTime(ms);
    expect(out).toMatch(/\d{1,2}:\d{2}/);
    // Never a date part — just the clock.
    expect(out).not.toMatch(/[A-Za-z]{3,}/);
  });

  it("returns an empty string for non-finite or non-positive timestamps", () => {
    expect(formatMessageTime(Number.NaN)).toBe("");
    expect(formatMessageTime(0)).toBe("");
    expect(formatMessageTime(-1)).toBe("");
    expect(formatMessageTime(Number.POSITIVE_INFINITY)).toBe("");
  });
});
