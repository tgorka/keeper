/**
 * Story 55.5 — what the HTML view shows, what it refuses, and what it writes.
 *
 * The safety cases are first because they are the reason this module is allowed
 * to exist at all: a repo whose rule is `textContent`, never `innerHTML` is
 * entitled to ask a renderer of HTML to prove itself.
 */
import { describe, expect, it } from "vitest";
import {
  buildHtmlView,
  HTML_ADDRESS_CLASS,
  scanTextRuns,
  spliceText,
  TEXT_RUN_ATTR,
} from "./html-view";

/** What a reader sees, with the address captions folded in as they appear. */
function shown(source: string): string {
  const host = document.createElement("div");
  host.appendChild(buildHtmlView(source).node);
  return host.textContent ?? "";
}

function built(source: string): HTMLElement {
  const host = document.createElement("div");
  host.appendChild(buildHtmlView(source).node);
  return host;
}

describe("what never survives", () => {
  it("drops a script, its content included", () => {
    const host = built("<p>before</p><script>alert(1)</script><p>after</p>");

    expect(host.querySelector("script")).toBeNull();
    expect(host.textContent).toBe("beforeafter");
    // Not "the text of the script is shown": a program is not prose.
    expect(host.textContent).not.toContain("alert");
  });

  it("drops a style rather than applying it to this application's own DOM", () => {
    const host = built("<style>body { display: none }</style><p>hello</p>");

    expect(host.querySelector("style")).toBeNull();
    expect(host.textContent).toBe("hello");
  });

  it("drops an event handler and keeps the element", () => {
    const host = built('<p onclick="steal()" title="t">text</p>');

    const paragraph = host.querySelector("p");
    expect(paragraph).not.toBeNull();
    expect(paragraph?.getAttribute("onclick")).toBeNull();
    // `title` is not on `p`'s allowlist either — the list is per element, so an
    // attribute nobody chose for this element does not arrive by accident.
    expect(paragraph?.getAttribute("title")).toBeNull();
    expect(paragraph?.textContent).toBe("text");
  });

  it("refuses a javascript: href and keeps the link's text", () => {
    const host = built('<a href="javascript:alert(1)">click</a>');

    const anchor = host.querySelector("a");
    expect(anchor?.getAttribute("href")).toBeNull();
    expect(anchor?.textContent).toBe("click");
  });

  it("keeps an ordinary href", () => {
    expect(
      built('<a href="https://example.org/x">go</a>').querySelector("a")?.getAttribute("href"),
    ).toBe("https://example.org/x");
  });

  it("shows a remote image's address instead of requesting it", () => {
    const host = built('<img src="https://tracker.example/p.png" alt="">');

    // NFR-11: opening a document must not report that it was opened.
    expect(host.querySelector("img")).toBeNull();
    const caption = host.querySelector(`.${HTML_ADDRESS_CLASS}`);
    expect(caption?.textContent).toBe("image: https://tracker.example/p.png");
  });

  it("unwraps an element it does not know rather than losing its text", () => {
    expect(shown("<custom-thing>kept</custom-thing>")).toBe("kept");
  });

  it("builds nothing from an iframe", () => {
    const host = built('<iframe src="https://x/"></iframe><p>after</p>');
    expect(host.querySelector("iframe")).toBeNull();
    expect(host.textContent).toBe("after");
  });
});

describe("the text runs", () => {
  it("records each run's offsets in the source", () => {
    const source = "<p>alpha</p><p>beta</p>";
    const runs = scanTextRuns(source);

    expect(runs.map((run) => run.raw)).toEqual(["alpha", "beta"]);
    expect(source.slice(runs[0].from, runs[0].to)).toBe("alpha");
    expect(source.slice(runs[1].from, runs[1].to)).toBe("beta");
  });

  it("gives a script's content no run, so the runs after it do not shift", () => {
    // The alignment this whole module rests on: the parser makes no text node
    // for raw-text content, so the scanner must make no run for it.
    const source = "<p>a</p><script>var x = '<p>fake</p>';</script><p>b</p>";
    expect(scanTextRuns(source).map((run) => run.raw)).toEqual(["a", "b"]);
  });

  it("skips comments", () => {
    expect(scanTextRuns("<p>a</p><!-- note --><p>b</p>").map((r) => r.raw)).toEqual(["a", "b"]);
  });

  it("aligns every rendered span with the run it came from", () => {
    const source = "<h1>Title</h1><p>Body <b>bold</b> tail</p>";
    const view = buildHtmlView(source);
    const host = document.createElement("div");
    host.appendChild(view.node);

    for (const span of Array.from(host.querySelectorAll(`[${TEXT_RUN_ATTR}]`))) {
      const run = view.runs[Number(span.getAttribute(TEXT_RUN_ATTR))];
      expect(source.slice(run.from, run.to)).toBe(run.raw);
      expect(span.textContent).toBe(run.raw);
    }
  });

  it("renders an entity as its character while the run keeps the source", () => {
    const source = "<p>Tom &amp; Jerry</p>";
    const view = buildHtmlView(source);
    const host = document.createElement("div");
    host.appendChild(view.node);

    expect(host.textContent).toBe("Tom & Jerry");
    // The run is what the FILE says, which is what a splice has to match.
    expect(view.runs[0].raw).toBe("Tom &amp; Jerry");
  });

  it("finds nothing to edit in an empty file", () => {
    expect(buildHtmlView("").runs).toEqual([]);
    expect(shown("")).toBe("");
  });

  it("renders a fragment with no html or body element", () => {
    expect(shown("just text")).toBe("just text");
  });
});

describe("splicing an edit back", () => {
  const source = "<p>alpha</p><p>beta</p>";

  it("replaces one run and leaves every other byte alone", () => {
    const runs = scanTextRuns(source);
    const result = spliceText(source, runs[1], "BETA");

    expect(result).toEqual({ ok: true, text: "<p>alpha</p><p>BETA</p>" });
  });

  it("refuses when the bytes at the range are not the bytes it was built from", () => {
    const runs = scanTextRuns(source);
    // The buffer moved under the view — someone typed in Source, a sync landed.
    const moved = `<h1>heading</h1>${source}`;

    const result = spliceText(moved, runs[1], "BETA");

    expect(result.ok).toBe(false);
    // A mapping bug costs an edit. It must never cost a file.
    expect(result.ok === false && result.reason).toContain("changed underneath");
  });

  it("refuses a range that is not inside the source at all", () => {
    expect(spliceText("short", { raw: "x", from: 100, to: 200 }, "y").ok).toBe(false);
  });

  it("accepts an empty replacement, which is a reader deleting a paragraph's text", () => {
    const runs = scanTextRuns(source);
    expect(spliceText(source, runs[0], "")).toEqual({ ok: true, text: "<p></p><p>beta</p>" });
  });
});
