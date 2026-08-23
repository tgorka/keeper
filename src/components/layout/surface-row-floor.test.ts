import { readdirSync, readFileSync, statSync } from "node:fs";
import { extname, join, resolve } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * A full-height row that grows may not be floored by what is inside it.
 *
 * The owner's report, three times: "notes still does not adapt to the size of
 * the screen when there is not enough room". Measured in Chromium against the
 * real container chain at a 1000px viewport — the shell row, the notes row, the
 * two surface columns, the panel strip, a panel, the note column, the format
 * toolbar and a wrapping CodeMirror:
 *
 *   the notes row, as shipped        1235px   (235px past the window)
 *   the same row with `min-w-0`      1000px
 *
 * `flex-1` makes an element a flex item, and a flex item's `min-width` defaults
 * to `auto`, which resolves to its CONTENT-BASED minimum. For a row holding two
 * sized columns and a panel wide enough for its toolbar that floor is wider than
 * a narrow window, flexbox refuses to shrink past it, and the row overflows. The
 * boxes inside are then laid out at the wider width and clipped by the window:
 * the toolbar cut mid-row, and prose wrapped somewhere off screen so every line
 * ended mid-word. Nothing scrolls, so the text is not merely awkward, it is
 * unreachable.
 *
 * Two earlier fixes went to elements that did not have the floor — `min-w-0` on
 * the note column, whose parent is a block box, and a `ResizeObserver`
 * re-measure, which cannot help a box nobody ever gave a smaller width. This
 * test exists because reading the CSS is what failed twice: the shape is
 * mechanical, so the check is mechanical.
 *
 * The shape is `flex` + `min-h-0` + `flex-1`: a row that fills its parent's
 * height and takes a share of its width. Every one of those is a flex item that
 * must be allowed to shrink. If a new surface copies the shape, this fails and
 * names the file.
 */
const SHAPE = /className="([^"]*\bflex\b[^"]*\bmin-h-0\b[^"]*\bflex-1\b[^"]*)"/g;

function sources(dir: string, out: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) {
      sources(path, out);
    } else if (extname(path) === ".tsx") {
      out.push(path);
    }
  }
  return out;
}

describe("a growing full-height row can always shrink", () => {
  it("keeps min-w-0 on every flex row that fills its parent and takes a share", () => {
    const root = resolve(__dirname, "../..");
    const offenders: string[] = [];

    for (const file of sources(root)) {
      const text = readFileSync(file, "utf8");
      for (const match of text.matchAll(SHAPE)) {
        const classes = match[1];
        // A column is not a row: its width is its parent's, and its
        // content-based minimum is in the axis this rule says nothing about.
        if (/\bflex-col\b/.test(classes)) {
          continue;
        }
        if (!/\bmin-w-0\b/.test(classes)) {
          const line = text.slice(0, match.index).split("\n").length;
          offenders.push(`${file.slice(root.length + 1)}:${line} — ${classes}`);
        }
      }
    }

    expect(offenders).toEqual([]);
  });
});
