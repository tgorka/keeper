/**
 * What a rendered answer is, and what it refuses to become (Epic 61, Story
 * 61.5).
 *
 * `bots-markdown.test.ts` owns the parse. This file owns the four things only a
 * DOM can show:
 *
 * - a `<script>` the model wrote is text on the page and not a node in it,
 * - a code block carries its language and a copy control that reaches the
 *   clipboard the same way the recordings row does,
 * - a settled block keeps its DOM node across a delta — asserted by node
 *   identity, which is the only assertion a remount cannot pass,
 * - feeding the answer token by token lands on the same HTML as rendering it
 *   in one shot.
 */
import { fireEvent, render, screen } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { BotAnswer } from "@/components/bots/bot-answer";
import { BOT_CODE_COPIED_LABEL, BOT_CODE_COPY_LABEL } from "@/lib/bots-markdown";

beforeEach(() => {
  // jsdom lacks a clipboard by default.
  Object.assign(navigator, { clipboard: { writeText: vi.fn(() => Promise.resolve()) } });
});

describe("a rendered answer", () => {
  it("renders the supported constructs as elements, not as punctuation", () => {
    const { container } = render(
      <BotAnswer
        body={[
          "# Title",
          "",
          "Some **bold** and _italic_ and ~~gone~~ and `code`.",
          "",
          "- one",
          "- two",
          "",
          "1. first",
          "",
          "> quoted",
          "",
          "---",
          "",
          "| a | b |",
          "|---|---|",
          "| 1 | 2 |",
        ].join("\n")}
      />,
    );
    expect(screen.getByRole("heading", { level: 1, name: "Title" })).toBeInTheDocument();
    expect(container.querySelector("strong")?.textContent).toBe("bold");
    expect(container.querySelector("em")?.textContent).toBe("italic");
    expect(container.querySelector("s")?.textContent).toBe("gone");
    expect(container.querySelector("code")?.textContent).toBe("code");
    expect(container.querySelectorAll("ul > li")).toHaveLength(2);
    expect(container.querySelectorAll("ol > li")).toHaveLength(1);
    expect(container.querySelector("blockquote")?.textContent).toContain("quoted");
    expect(container.querySelector("hr")).not.toBeNull();
    expect(container.querySelectorAll("table th")).toHaveLength(2);
    expect(container.querySelectorAll("table td")).toHaveLength(2);
  });

  it("shows a link's label and its URL, and creates no destination", () => {
    const url = "htt" + "ps://example.invalid/doc";
    const { container } = render(<BotAnswer body={`read [the docs](${url}) first`} />);
    expect(container.textContent).toContain("the docs");
    expect(container.textContent).toContain(url);
    // No anchor, no image, nothing that could fetch: an answer is a string an
    // agent wrote, and `note_protocol.rs` already settled what that may do.
    expect(container.querySelector("a")).toBeNull();
    expect(container.querySelector("img")).toBeNull();
  });

  it("renders a script tag the model wrote as text, never as a node", () => {
    const { container } = render(
      <BotAnswer body={"before\n\n<script>alert(1)</script>\n\nafter"} />,
    );
    expect(container.querySelector("script")).toBeNull();
    expect(container.textContent).toContain("<script>alert(1)</script>");
  });

  it("renders an inline HTML tag as its characters", () => {
    const { container } = render(<BotAnswer body="a<b>c</b>d" />);
    expect(container.querySelector("b")).toBeNull();
    expect(container.textContent).toBe("a<b>c</b>d");
  });
});

describe("a code block", () => {
  it("labels its language and puts its text on the clipboard, confirming it", async () => {
    render(<BotAnswer body={"```python\nprint(1)\n```"} />);
    const block = screen.getByTestId("bot-code-block");
    expect(block.dataset.language).toBe("python");
    expect(block.dataset.closed).toBe("true");
    expect(screen.getByText("python")).toBeInTheDocument();

    fireEvent.click(screen.getByRole("button", { name: BOT_CODE_COPY_LABEL }));

    expect(navigator.clipboard.writeText).toHaveBeenCalledWith("print(1)");
    expect(await screen.findByText(BOT_CODE_COPIED_LABEL)).toBeInTheDocument();
  });

  it("says nothing and shows no error when the webview refuses the clipboard", async () => {
    Object.assign(navigator, {
      clipboard: { writeText: vi.fn(() => Promise.reject(new Error("denied"))) },
    });
    render(<BotAnswer body={"```\nx\n```"} />);

    fireEvent.click(screen.getByRole("button", { name: BOT_CODE_COPY_LABEL }));

    expect(screen.queryByRole("alert")).not.toBeInTheDocument();
    expect(screen.getByRole("button", { name: BOT_CODE_COPY_LABEL })).toBeInTheDocument();
  });

  it("closes an unterminated fence itself rather than swallowing what arrived", () => {
    render(<BotAnswer body={"```js\nconst a = 1;"} />);
    const block = screen.getByTestId("bot-code-block");
    expect(block.dataset.closed).toBe("false");
    expect(block.textContent).toContain("const a = 1;");
  });
});

describe("streaming", () => {
  const ANSWER = [
    "# Answer",
    "",
    "First paragraph with **bold**.",
    "",
    "```js",
    "const a = 1;",
    "```",
    "",
    "- one",
    "- two",
    "",
    "Last line.",
  ].join("\n");

  it("lands on the same DOM whether it arrived at once or token by token", () => {
    const streamed = render(<BotAnswer body="" />);
    for (let length = 1; length <= ANSWER.length; length += 1) {
      streamed.rerender(<BotAnswer body={ANSWER.slice(0, length)} />);
    }
    const oneShot = render(<BotAnswer body={ANSWER} />);
    expect(streamed.container.innerHTML).toBe(oneShot.container.innerHTML);
  });

  it("keeps the DOM node of a settled block across the deltas that follow it", () => {
    const view = render(<BotAnswer body="First paragraph." />);
    const first = view.container.querySelector("p");
    expect(first?.textContent).toBe("First paragraph.");

    // Everything the rest of an answer can do to the block above it.
    for (const suffix of ["\n", "\n\n", "\n\nSecond", "\n\nSecond paragraph.", "\n\n```js\nx"]) {
      view.rerender(<BotAnswer body={`First paragraph.${suffix}`} />);
      expect(
        view.container.querySelector("p"),
        `the settled first block was remounted by "${suffix.replace(/\n/g, "\\n")}"`,
      ).toBe(first);
    }
  });

  it("does not bold an answer retroactively from a marker still being typed", () => {
    const view = render(<BotAnswer body="**bold" />);
    expect(view.container.querySelector("strong")).toBeNull();
    expect(view.container.textContent).toBe("**bold");
    view.rerender(<BotAnswer body="**bold**" />);
    expect(view.container.querySelector("strong")?.textContent).toBe("bold");
  });

  it("renders a 200 kB answer inside a stated budget", () => {
    const chunk =
      "## Section\n\nSome **prose** with `code`.\n\n- one\n- two\n\n```js\nx();\n```\n\n";
    let body = "";
    while (body.length < 200_000) {
      body += chunk;
    }
    const started = performance.now();
    const { container } = render(<BotAnswer body={body} />);
    const elapsed = performance.now() - started;
    expect(container.querySelectorAll("[data-testid='bot-code-block']").length).toBeGreaterThan(
      100,
    );
    // Parse plus a full jsdom mount of ~11 400 blocks, 2 858 of them code
    // blocks. jsdom is slower at this than a real engine and this is the
    // pathological case — an answer nobody sends — so the bound is generous on
    // purpose: it exists to catch a quadratic, not to police a millisecond.
    // Measured ~1.4 s here.
    expect(elapsed).toBeLessThan(10_000);
  }, 60_000);
});
