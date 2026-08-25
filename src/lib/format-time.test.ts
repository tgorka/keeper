import { describe, expect, it } from "vitest";
import {
  formatDraftAge,
  formatMessageTime,
  formatReleaseIn,
  formatReleaseSpoken,
  formatRoomTimestamp,
} from "@/lib/format-time";

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

describe("formatDraftAge", () => {
  const now = new Date(2026, 6, 4, 18, 0, 0).getTime();

  it('shows "just now" for a draft under a minute old', () => {
    expect(formatDraftAge(now - 30_000, now)).toBe("just now");
    expect(formatDraftAge(now, now)).toBe("just now");
  });

  it("shows whole minutes for a draft under an hour old", () => {
    const out = formatDraftAge(now - 5 * 60_000, now);
    // Relative-time string mentions "5" and a minute unit; never a clock or date.
    expect(out).toMatch(/5/);
    expect(out.toLowerCase()).toMatch(/min/);
  });

  it("shows whole hours for a draft under a day old", () => {
    const out = formatDraftAge(now - 2 * 3_600_000, now);
    expect(out).toMatch(/2/);
    expect(out.toLowerCase()).toMatch(/h/);
  });

  it("falls back to a short date for a draft older than a day", () => {
    const twoDaysAgo = new Date(2026, 6, 2, 9, 0, 0).getTime();
    const out = formatDraftAge(twoDaysAgo, now);
    // The date fallback carries no relative "ago"/"in" phrasing.
    expect(out.length).toBeGreaterThan(0);
    expect(out.toLowerCase()).not.toMatch(/ago|in /);
  });

  it('clamps a future / clock-skewed timestamp to "just now"', () => {
    expect(formatDraftAge(now + 60_000, now)).toBe("just now");
  });

  it("returns an empty string for non-finite or non-positive timestamps", () => {
    expect(formatDraftAge(Number.NaN, now)).toBe("");
    expect(formatDraftAge(0, now)).toBe("");
    expect(formatDraftAge(-1, now)).toBe("");
    expect(formatDraftAge(Number.POSITIVE_INFINITY, now)).toBe("");
  });
});

describe("formatReleaseIn", () => {
  const now = new Date(2026, 6, 4, 18, 0, 0).getTime();

  it("shows whole seconds in the final minute", () => {
    expect(formatReleaseIn(now + 45_000, now)).toBe("45s");
    expect(formatReleaseIn(now + 1_000, now)).toBe("1s");
  });

  it("rounds the final minute up, so a live deadline never reads 0s", () => {
    // 45.4s left is still a 46th second the owner has.
    expect(formatReleaseIn(now + 45_400, now)).toBe("46s");
    expect(formatReleaseIn(now + 1, now)).toBe("1s");
  });

  it("shows whole minutes under an hour", () => {
    expect(formatReleaseIn(now + 12 * 60_000, now)).toBe("12 min");
    // Minutes are floored — the leftover seconds are not rounded into the next.
    expect(formatReleaseIn(now + 12 * 60_000 + 59_000, now)).toBe("12 min");
  });

  it("shows whole hours under a day", () => {
    expect(formatReleaseIn(now + 23 * 3_600_000, now)).toBe("23 hr");
    expect(formatReleaseIn(now + 3_600_000 + 59 * 60_000, now)).toBe("1 hr");
  });

  it("shows whole days beyond a day, singular at exactly one", () => {
    expect(formatReleaseIn(now + 86_400_000, now)).toBe("1 day");
    expect(formatReleaseIn(now + 2 * 86_400_000, now)).toBe("2 days");
    expect(formatReleaseIn(now + 6 * 86_400_000 + 3_600_000, now)).toBe("6 days");
  });

  it("changes rung exactly at the minute, hour and day boundaries", () => {
    expect(formatReleaseIn(now + 59_999, now)).toBe("60s");
    expect(formatReleaseIn(now + 60_000, now)).toBe("1 min");
    expect(formatReleaseIn(now + 3_599_999, now)).toBe("59 min");
    expect(formatReleaseIn(now + 3_600_000, now)).toBe("1 hr");
    expect(formatReleaseIn(now + 86_399_999, now)).toBe("23 hr");
    expect(formatReleaseIn(now + 86_400_000, now)).toBe("1 day");
  });

  it('reads "due" for a deadline exactly at now', () => {
    expect(formatReleaseIn(now, now)).toBe("due");
  });

  it('reads "due" for a past deadline, never a negative figure', () => {
    for (const deadlineMs of [now - 1, now - 90_000, now - 40 * 86_400_000]) {
      const out = formatReleaseIn(deadlineMs, now);
      expect(out).toBe("due");
      // A skewed clock must not leak a minus sign into the cell.
      expect(out).not.toMatch(/[-\u2212]/);
    }
  });

  it("returns an empty string for unrenderable deadlines", () => {
    expect(formatReleaseIn(0, now)).toBe("");
    expect(formatReleaseIn(-1, now)).toBe("");
    expect(formatReleaseIn(Number.NaN, now)).toBe("");
    expect(formatReleaseIn(Number.POSITIVE_INFINITY, now)).toBe("");
    // Past `MAX_DATE_MS` (8.64e15): an instant no `Date` can represent.
    expect(formatReleaseIn(8.64e15 + 1, now)).toBe("");
  });
});

describe("formatReleaseSpoken", () => {
  const now = new Date(2026, 6, 4, 18, 0, 0).getTime();

  it("speaks a live countdown as a phrase that stands on its own", () => {
    expect(formatReleaseSpoken(now + 23 * 3_600_000, now)).toBe("Releases in 23 hr");
    expect(formatReleaseSpoken(now + 45_000, now)).toBe("Releases in 45s");
    expect(formatReleaseSpoken(now + 86_400_000, now)).toBe("Releases in 1 day");
  });

  it('speaks "Release is due" at and past the deadline', () => {
    expect(formatReleaseSpoken(now, now)).toBe("Release is due");
    expect(formatReleaseSpoken(now - 90_000, now)).toBe("Release is due");
  });

  it("speaks nothing when there is no figure to draw", () => {
    expect(formatReleaseSpoken(0, now)).toBe("");
    expect(formatReleaseSpoken(Number.NaN, now)).toBe("");
    expect(formatReleaseSpoken(8.64e15 + 1, now)).toBe("");
  });
});
