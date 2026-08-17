/**
 * The remembered view (Story 45.4).
 *
 * These are pure and take the jar as an argument, so what is asserted here is
 * the parsing and the defaulting. That the component actually consults them at
 * the right moments is asserted in `raw-rendered-view.test.tsx` against a real
 * render — "the preference is stored perfectly and read never" is a shape of
 * defect a pure test cannot see.
 */
import { describe, expect, it } from "vitest";
import {
  DEFAULT_VIEW_MODE,
  readViewModes,
  VIEW_MODE_COOKIE,
  VIEW_MODE_MAX_AGE,
  viewModeCookie,
  viewModeFor,
} from "./view-mode";

/** A jar as `document.cookie` hands it over: several names, one of them ours. */
function jar(value: string): string {
  return `theme=dark; ${VIEW_MODE_COOKIE}=${encodeURIComponent(value)}; sidebar_state=true`;
}

describe("readViewModes", () => {
  it("reads every remembered format out of a shared jar", () => {
    expect(readViewModes(jar("json:raw|csv:rendered"))).toEqual({
      json: "raw",
      csv: "rendered",
    });
  });

  it("is empty for a jar that has never held one", () => {
    expect(readViewModes("theme=dark")).toEqual({});
    expect(readViewModes("")).toEqual({});
  });

  it("drops what it cannot read instead of throwing", () => {
    // The jar is shared with every other cookie on the origin and with older
    // builds of keeper. A viewer that refuses to render because somebody's jar
    // has a stale entry is worse than a file opening in its default view.
    expect(readViewModes(jar("json:sideways|:raw|nocolon|9bad:raw|csv:rendered"))).toEqual({
      csv: "rendered",
    });
  });

  it("is not confused by another cookie whose name ends with ours", () => {
    expect(readViewModes(`not_${VIEW_MODE_COOKIE}=json%3Araw`)).toEqual({});
  });

  it("row 10: reads a jar written before Note mode existed, unchanged", () => {
    // The vocabulary widened in Story 51.5 and the old values are the same
    // values. A reader who chose Source for markdown in an older build still
    // gets Source.
    expect(readViewModes(jar("markdown:rendered|json:raw"))).toEqual({
      markdown: "rendered",
      json: "raw",
    });
  });

  it("row 11: reads `note`, rather than dropping a value it has never stored", () => {
    // The other direction of the same compatibility, and the reason it is
    // asserted: a build that did not know the word would drop this pair as
    // malformed and silently reset the reader's choice.
    expect(readViewModes(jar("markdown:note|csv:rendered"))).toEqual({
      markdown: "note",
      csv: "rendered",
    });
  });
});

describe("viewModeFor", () => {
  it("answers what was stored", () => {
    expect(viewModeFor(jar("json:raw"), "json")).toBe("raw");
  });

  it("answers the default for a format nothing was ever stored for", () => {
    // Total on purpose: every caller would otherwise repeat this fallback and
    // one of them would get it wrong.
    expect(viewModeFor(jar("json:raw"), "csv")).toBe(DEFAULT_VIEW_MODE);
    expect(DEFAULT_VIEW_MODE).toBe("rendered");
  });

  it("row 11: answers `note` for a format that was remembered in it", () => {
    expect(viewModeFor(jar("markdown:note"), "markdown")).toBe("note");
  });

  it("still answers the default for markdown, which is not the editor", () => {
    // Never Note by default: a person opening a file to read it must not land
    // in an editor.
    expect(viewModeFor(jar(""), "markdown")).toBe("rendered");
  });
});

describe("viewModeCookie", () => {
  it("keeps every other format's choice, because a cookie write replaces the name", () => {
    const next = viewModeCookie(jar("csv:rendered|json:rendered"), "json", "raw");
    expect(readViewModes(next)).toEqual({ csv: "rendered", json: "raw" });
  });

  it("forgets a format when asked, leaving nothing to re-adopt", () => {
    const next = viewModeCookie(jar("csv:rendered|json:raw"), "json", null);
    expect(readViewModes(next)).toEqual({ csv: "rendered" });
  });

  it("writes a path and a year, so the preference survives Monday", () => {
    const next = viewModeCookie("", "json", "raw");
    expect(next).toContain("path=/");
    expect(next).toContain(`max-age=${VIEW_MODE_MAX_AGE}`);
    expect(VIEW_MODE_MAX_AGE).toBe(60 * 60 * 24 * 365);
  });

  it("round-trips through its own reader", () => {
    expect(viewModeFor(viewModeCookie("", "markdown", "raw"), "markdown")).toBe("raw");
  });

  it("round-trips `note` too, so the third mode survives a restart", () => {
    expect(viewModeFor(viewModeCookie("", "markdown", "note"), "markdown")).toBe("note");
  });
});
