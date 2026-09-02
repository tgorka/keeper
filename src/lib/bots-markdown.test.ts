/**
 * The answer parser's contract (Epic 61, Story 61.5).
 *
 * Two families of test, and the second is the reason the story exists. The
 * first walks the supported subset. The second walks the same subset one
 * character at a time, because the renderer is called on every content delta
 * and a construct that is half-typed is the normal case, not the edge one.
 */
import { describe, expect, it } from "vitest";

import { type MdBlock, type MdInline, parseAnswer } from "@/lib/bots-markdown";

/** The plain text of a run tree, for assertions that do not care about the
 *  emphasis structure carrying it. */
function textOf(runs: MdInline[]): string {
  return runs
    .map((run) => {
      switch (run.kind) {
        case "text":
        case "code":
          return run.text;
        default:
          return textOf(run.children);
      }
    })
    .join("");
}

/** Every block's kind, in order — the shape of a parse at a glance. */
function kinds(blocks: MdBlock[]): string[] {
  return blocks.map((block) => block.kind);
}

describe("the supported markdown subset", () => {
  it("reads a paragraph, keeping the model's own line breaks", () => {
    const [block] = parseAnswer("one\ntwo");
    expect(block?.kind).toBe("paragraph");
    expect(block?.kind === "paragraph" && textOf(block.children)).toBe("one\ntwo");
  });

  it("reads ATX and setext headings with their level", () => {
    const blocks = parseAnswer("# One\n\n### Three\n\nSetext\n======");
    expect(kinds(blocks)).toEqual(["heading", "heading", "heading"]);
    expect(blocks.map((b) => (b.kind === "heading" ? b.level : 0))).toEqual([1, 3, 1]);
    expect(blocks[1]?.kind === "heading" && textOf(blocks[1].children)).toBe("Three");
  });

  it("reads bold, italic, strikethrough and inline code as structure", () => {
    const [block] = parseAnswer("a **b** _c_ ~~d~~ `e`");
    const runs = block?.kind === "paragraph" ? block.children : [];
    expect(runs.map((run) => run.kind)).toEqual([
      "text",
      "strong",
      "text",
      "emphasis",
      "text",
      "strike",
      "text",
      "code",
    ]);
    expect(textOf(runs)).toBe("a b c d e");
  });

  it("reads a fenced code block with its language, and strips the fence", () => {
    const [block] = parseAnswer("```python\nprint(1)\nprint(2)\n```");
    expect(block).toMatchObject({
      kind: "code",
      language: "python",
      text: "print(1)\nprint(2)",
      closed: true,
    });
  });

  it("reads an indented code block, de-indented and with no language", () => {
    const [block] = parseAnswer("    one\n    two\n");
    expect(block).toMatchObject({ kind: "code", language: null, text: "one\ntwo", closed: true });
  });

  it("reads nested and ordered lists, keeping the number a list starts at", () => {
    const blocks = parseAnswer("- a\n  - b\n\n5. five\n6. six");
    expect(kinds(blocks)).toEqual(["list", "list"]);
    const outer = blocks[0];
    expect(outer?.kind === "list" && outer.ordered).toBe(false);
    const nested = outer?.kind === "list" ? outer.items[0]?.blocks : [];
    expect(kinds(nested ?? [])).toEqual(["paragraph", "list"]);
    const ordered = blocks[1];
    expect(ordered?.kind === "list" && ordered.ordered).toBe(true);
    expect(ordered?.kind === "list" && ordered.start).toBe(5);
    expect(ordered?.kind === "list" && ordered.items).toHaveLength(2);
  });

  it("reads a block quote without printing its markers back into the prose", () => {
    const [block] = parseAnswer("> a\n> b");
    expect(block?.kind).toBe("quote");
    const inner = block?.kind === "quote" ? block.blocks[0] : undefined;
    expect(inner?.kind === "paragraph" && textOf(inner.children)).toBe("a\nb");
  });

  it("reads a horizontal rule", () => {
    expect(kinds(parseAnswer("a\n\n---\n\nb"))).toEqual(["paragraph", "rule", "paragraph"]);
  });

  it("reads a table's header and rows", () => {
    const [block] = parseAnswer("| a | b |\n|---|---|\n| 1 | 2 |\n| 3 | 4 |");
    expect(block?.kind).toBe("table");
    if (block?.kind !== "table") {
      return;
    }
    expect(block.header.map(textOf)).toEqual(["a", "b"]);
    expect(block.rows.map((row) => row.map(textOf))).toEqual([
      ["1", "2"],
      ["3", "4"],
    ]);
  });

  it("keeps a link's label and its URL, and hands back no destination of its own", () => {
    const [block] = parseAnswer("see [docs](htt" + "ps://example.invalid/x) now");
    const runs = block?.kind === "paragraph" ? block.children : [];
    const link = runs.find((run) => run.kind === "link");
    expect(link).toMatchObject({ kind: "link", url: "htt" + "ps://example.invalid/x" });
    expect(link?.kind === "link" && textOf(link.children)).toBe("docs");
  });

  it("unescapes a backslash escape to the one character it meant", () => {
    const [block] = parseAnswer("a \\* b");
    expect(block?.kind === "paragraph" && textOf(block.children)).toBe("a * b");
  });
});

describe("what is outside the subset stays visible", () => {
  it("renders an HTML block as its own source, never as a construct", () => {
    const blocks = parseAnswer("<script>alert(1)</script>");
    expect(kinds(blocks)).toEqual(["literal"]);
    expect(blocks[0]?.source).toBe("<script>alert(1)</script>");
  });

  it("renders an inline HTML tag as the characters the model sent", () => {
    const [block] = parseAnswer("a<b>c</b>d");
    expect(block?.kind === "paragraph" && textOf(block.children)).toBe("a<b>c</b>d");
  });

  it("leaves an HTML entity undecoded — decoding is the first half of rendering HTML", () => {
    const [block] = parseAnswer("&amp; &lt;i&gt;");
    expect(block?.kind === "paragraph" && textOf(block.children)).toBe("&amp; &lt;i&gt;");
  });

  it("loses no character of the source, whatever the input", () => {
    // Every block's source, concatenated, must account for the whole answer
    // minus the blank lines between blocks.
    const source = "# h\n\ntext\n\n- a\n\n> q\n\n```\nc\n```\n\n<div>x</div>\n";
    const covered = parseAnswer(source)
      .map((block) => block.source)
      .join("\n\n");
    expect(covered.replace(/\s+/g, "")).toBe(source.replace(/\s+/g, ""));
  });
});

describe("a half-typed answer (the streaming contract)", () => {
  it("closes an unterminated fence itself and says it is open", () => {
    const [block] = parseAnswer("```js\nconst a = 1;\n");
    expect(block).toMatchObject({ kind: "code", language: "js", closed: false });
    expect(block?.kind === "code" && block.text).toBe("const a = 1;\n");
  });

  it("does not bold the rest of an answer from an unterminated marker", () => {
    const blocks = parseAnswer("**bold not closed\n\nand a second paragraph");
    expect(kinds(blocks)).toEqual(["paragraph", "paragraph"]);
    for (const block of blocks) {
      const runs = block.kind === "paragraph" ? block.children : [];
      expect(runs.every((run) => run.kind === "text")).toBe(true);
    }
    expect(blocks[0]?.kind === "paragraph" && textOf(blocks[0].children)).toBe("**bold not closed");
  });

  it("gives a settled block the same key at every prefix that contains it", () => {
    const answer = "# Title\n\nfirst paragraph\n\n- a\n- b\n\n```js\nx\n```\n\nlast";
    const settled = new Map<string, string>();
    for (let length = 1; length <= answer.length; length += 1) {
      const blocks = parseAnswer(answer.slice(0, length));
      // Every block except the one still growing must never change identity.
      for (const block of blocks.slice(0, -1)) {
        const seen = settled.get(block.key);
        if (seen !== undefined) {
          expect(seen, `block ${block.key} changed under it at length ${length}`).toBe(
            block.source,
          );
        }
        settled.set(block.key, block.source);
      }
    }
    // The walk actually covered the answer's blocks rather than one of them.
    expect(settled.size).toBeGreaterThanOrEqual(4);
  });

  it("keys blocks by position, so a growing answer never renumbers its start", () => {
    const first = parseAnswer("one");
    const later = parseAnswer("one\n\ntwo\n\nthree");
    expect(first[0]?.key).toBe(later[0]?.key);
    expect(later.map((block) => block.key)).toEqual(["0", "1", "2"]);
  });

  it("parses the final prefix to exactly what the whole answer parses to", () => {
    const answer = "para\n\n> quote\n\n| a |\n|---|\n| 1 |\n\n```r\nz\n```\n";
    let streamed: MdBlock[] = [];
    for (let length = 1; length <= answer.length; length += 1) {
      streamed = parseAnswer(answer.slice(0, length));
    }
    expect(streamed).toEqual(parseAnswer(answer));
  });

  it("parses a 200 kB answer inside a stated budget", () => {
    const chunk =
      "## Section\n\nSome **prose** with `code`.\n\n- one\n- two\n\n```js\nx();\n```\n\n";
    let answer = "";
    while (answer.length < 200_000) {
      answer += chunk;
    }
    const started = performance.now();
    const blocks = parseAnswer(answer);
    const elapsed = performance.now() - started;
    expect(blocks.length).toBeGreaterThan(100);
    // A parse of a 200 kB answer, once per delta, has to stay far under the
    // time a person would notice. 1 s is the loud-failure bound, not the
    // target: it measures ~110 ms on this machine for ~11 400 blocks.
    expect(elapsed).toBeLessThan(1000);
  });
});
