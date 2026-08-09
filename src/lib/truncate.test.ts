import { describe, expect, it } from "vitest";
import { ELLIPSIS, truncateGraphemes } from "@/lib/truncate";

/**
 * FR-168's "truncates rather than clipping mid-glyph", for the one place keeper
 * truncates in JavaScript rather than in CSS: the preview of a properties block
 * it has decided not to parse.
 */
describe("truncateGraphemes", () => {
  it("leaves a value that already fits exactly as it was", () => {
    expect(truncateGraphemes("pensive, mostly", 400)).toBe("pensive, mostly");
    expect(truncateGraphemes("abcd", 4)).toBe("abcd");
  });

  it("never emits half a character", () => {
    // Each of these is more than one UTF-16 code unit, and `slice` cuts inside
    // them: a family emoji is five code points joined by ZWJ, a flag is two
    // regional indicators, and an accented letter can be a base plus a mark.
    const emoji = "👨‍👩‍👧‍👦";
    const flag = "🇵🇱";
    const combined = "e\u0301";
    const text = `${emoji}${flag}${combined}tail`;

    const cut = truncateGraphemes(text, 2);

    expect(cut).toBe(`${emoji}${flag}${ELLIPSIS}`);
    // A lone surrogate is what `slice` produces here, and it renders as ``.
    expect(/[\uD800-\uDBFF](?![\uDC00-\uDFFF])|(?<![\uD800-\uDBFF])[\uDC00-\uDFFF]/.test(cut)).toBe(
      false,
    );
    expect(cut).not.toContain("\uFFFD");
  });

  it("is what a naive slice is not", () => {
    const text = "👍".repeat(10);

    // The bug this replaces, stated: `slice` counts code units, and one thumb
    // is two of them, so an odd cut lands inside the pair.
    expect(text.slice(0, 5).endsWith("\uD83D")).toBe(true);
    expect(truncateGraphemes(text, 5)).toBe(`${"👍".repeat(5)}${ELLIPSIS}`);
  });

  it("marks that something was removed, and only when something was", () => {
    expect(truncateGraphemes("abcde", 4)).toBe(`abcd${ELLIPSIS}`);
    expect(truncateGraphemes("abcde", 5)).toBe("abcde");
    expect(truncateGraphemes("abcde", 5)).not.toContain(ELLIPSIS);
  });

  it("keeps newlines, because a frontmatter block is lines", () => {
    expect(truncateGraphemes("a\nb\nc", 3)).toBe(`a\nb${ELLIPSIS}`);
  });
});
