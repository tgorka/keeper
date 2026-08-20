/**
 * What the renderer puts on screen for Story 45.10's marks, and for the link
 * defect that story was sent to fix.
 *
 * **Every test here builds a real `EditorView` with the markdown language
 * loaded.** That is not decoration: DW-165 shipped for eight epics because the
 * one suite that built a view around `livePreview` built it *without* the
 * grammar, so `syntaxTree` produced no nodes and every branch under test was
 * dead. A renderer test that does not load the parser is a test of an empty
 * switch statement.
 *
 * Assertions are about rendered text and rendered classes — what a reader sees
 * — never about a decoration set, because the defect this file pins was
 * invisible at that level: the decorations were all constructed correctly and
 * the note came out blank.
 */
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { fireEvent } from "@testing-library/dom";
import { afterAll, afterEach, beforeAll, describe, expect, it } from "vitest";
import { withRangeRects } from "@/test/layout";
import { livePreview } from "./live-preview";
import { MARKDOWN_MARKS } from "./markdown-marks";

// jsdom has no `Range.getClientRects`, and CodeMirror's measure pass calls it
// on any animation frame that elapses mid-test — taking the run's exit code
// with it when it throws.
let removeRangeRects: (() => void) | null = null;
beforeAll(() => {
  removeRangeRects = withRangeRects();
});
afterAll(() => {
  removeRangeRects?.();
});

const views: EditorView[] = [];
afterEach(() => {
  for (const view of views.splice(0)) {
    view.destroy();
  }
});

/**
 * Mount the document with the caret parked out of the way.
 *
 * The renderer's reveal rule gives a line its source back when the selection
 * touches it, so a test that left the caret at offset zero would be asserting
 * about *source*, not about the render, and would pass for a renderer that did
 * nothing at all. Every document below therefore ends with a spare line, and
 * the caret goes there.
 */
function render(doc: string): EditorView {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      extensions: [
        // The parser the app loads; see the note in `format-commands.test.ts`.
        markdown({ base: markdownLanguage, extensions: [...MARKDOWN_MARKS] }),
        livePreview({ vaultId: "v1", assetUrl: (rel) => rel, onOpenLink: () => {} }),
      ],
    }),
  });
  view.dispatch({ selection: { anchor: view.state.doc.length } });
  views.push(view);
  return view;
}

/** What a reader sees on the first line, which is where every case is written. */
function shown(view: EditorView): string {
  return view.contentDOM.firstElementChild?.textContent ?? "";
}

describe("== renders as a highlight (Story 55.3)", () => {
  it("hides its delimiters and paints the text", () => {
    const view = render("say ==this== please\n\n");

    expect(shown(view)).toBe("say this please");
    const marked = view.contentDOM.querySelector(".cm-lp-mark");
    expect(marked?.textContent).toBe("this");
  });

  it("gives the source back when the caret lands on the line", () => {
    const view = render("say ==this== please\n\n");
    view.dispatch({ selection: { anchor: 2 } });

    // The same reveal rule every other mark obeys: the line you are editing is
    // the line you can see the syntax of.
    expect(shown(view)).toBe("say ==this== please");
  });

  it("leaves arithmetic unpainted", () => {
    const view = render("if a == b then\n\n");

    expect(shown(view)).toBe("if a == b then");
    expect(view.contentDOM.querySelector(".cm-lp-mark")).toBeNull();
  });
});

describe("links render (the defect 45.10 was sent to fix)", () => {
  /**
   * The headline failure. `URL` was in the renderer's blanket hide-list, but the
   * parser emits `URL` for three different roles and only one of them is a
   * destination the reader does not want. For an autolink the URL *is* the
   * text, so hiding it deleted the link from the note: the reader saw an empty
   * gap where they had typed an address, and the source on disk was fine.
   */
  it("shows an angle-bracket autolink instead of swallowing it", () => {
    const view = render("<https://example.com>\n\nend\n");

    expect(shown(view)).toBe("https://example.com");
  });

  it("shows a bare URL instead of swallowing it", () => {
    const view = render("see https://bare.example now\n\nend\n");

    expect(shown(view)).toBe("see https://bare.example now");
  });

  it("hides a destination only where the link has text of its own", () => {
    const view = render("see [the docs](https://example.com) here\n\nend\n");

    expect(shown(view)).toBe("see the docs here");
  });

  /** The destination is hidden, so hovering has to be able to answer "where
   *  does this go?" without moving the caret into the line to read the source. */
  it("carries the destination in a title, on both link shapes", () => {
    const inline = render("see [the docs](https://example.com) here\n\nend\n");
    expect(inline.contentDOM.querySelector(".cm-lp-link")).toHaveAttribute(
      "title",
      "https://example.com",
    );

    const bare = render("see https://bare.example now\n\nend\n");
    expect(bare.contentDOM.querySelector(".cm-lp-link")).toHaveAttribute(
      "title",
      "https://bare.example",
    );
  });

  it("shows a reference link's text without leaking its label", () => {
    const view = render("see [the docs][1] here\n\n[1]: https://x.example\n\nend\n");

    expect(shown(view)).toBe("see the docs here");
  });

  /** A definition line is metadata the writer typed and has to be able to read
   *  back. Hiding `URL` and the `:` by node name reduced it to a bare `[1]`. */
  it("leaves a link reference definition entirely alone", () => {
    const view = render("[1]: https://x.example\n\nend\n");

    expect(shown(view)).toBe("[1]: https://x.example");
  });

  /**
   * keeper never fetches a remote image, so a note cannot become a tracking
   * pixel. Hiding the destination by node name meant a remote embed rendered as
   * its alt text alone — indistinguishable from a word somebody typed, with no
   * hint that an image was meant or that keeper had declined to load it.
   */
  it("leaves a remote image as its whole source, not as its alt text", () => {
    const view = render("![a diagram](https://example.com/d.png)\n\nend\n");

    expect(shown(view)).toBe("![a diagram](https://example.com/d.png)");
  });

  it("still gives a vault-relative image its widget", () => {
    const view = render("![alt](pics/one.png)\n\nend\n");

    expect(view.contentDOM.querySelector(".cm-lp-image img")).not.toBeNull();
  });

  it("gives every link its source back when the caret lands on the line", () => {
    const view = render("<https://example.com> and [docs](https://x.example)\n\nend\n");
    view.dispatch({ selection: { anchor: 3 } });

    expect(shown(view)).toBe("<https://example.com> and [docs](https://x.example)");
  });
});

describe("subscript and superscript", () => {
  it("renders H~2~O as H2O with the 2 marked as a subscript", () => {
    const view = render("H~2~O\n\nend\n");

    expect(shown(view)).toBe("H2O");
    expect(view.contentDOM.querySelector(".cm-lp-sub")?.textContent).toBe("2");
  });

  it("renders x^2^ as x2 with the 2 marked as a superscript", () => {
    const view = render("x^2^ metres\n\nend\n");

    expect(shown(view)).toBe("x2 metres");
    expect(view.contentDOM.querySelector(".cm-lp-sup")?.textContent).toBe("2");
  });

  /** A single tilde is subscript and a double one is strikethrough, and the
   *  parser is what tells them apart — so the renderer must not. */
  it("does not confuse ~sub~ with ~~strikethrough~~", () => {
    const view = render("~~gone~~ and ~low~\n\nend\n");

    expect(view.contentDOM.querySelector(".cm-lp-strike")?.textContent).toBe("gone");
    expect(view.contentDOM.querySelector(".cm-lp-sub")?.textContent).toBe("low");
  });
});

describe("underline", () => {
  it("hides the two tags and underlines what is between them", () => {
    const view = render("an <u>underlined</u> word\n\nend\n");

    expect(shown(view)).toBe("an underlined word");
    expect(view.contentDOM.querySelector(".cm-lp-underline")?.textContent).toBe("underlined");
  });

  /**
   * The standing refusal to render HTML in a note body, still standing. Only
   * these two literal strings are delimiters; everything else stays the
   * characters the writer typed, because nothing here parses HTML.
   */
  it("leaves every other tag as the text it has always been", () => {
    const view = render("a <b>bold</b> and <script>x</script>\n\nend\n");

    expect(shown(view)).toBe("a <b>bold</b> and <script>x</script>");
    expect(view.contentDOM.querySelector("b")).toBeNull();
    expect(view.contentDOM.querySelector("script")).toBeNull();
  });

  it("leaves an unclosed tag alone rather than swallowing the rest of the line", () => {
    const view = render("an <u>unclosed run\n\nend\n");

    expect(shown(view)).toBe("an <u>unclosed run");
  });

  it("gives the tags back when the caret lands on the line", () => {
    const view = render("an <u>underlined</u> word\n\nend\n");
    view.dispatch({ selection: { anchor: 2 } });

    expect(shown(view)).toBe("an <u>underlined</u> word");
  });
});

describe("a task list", () => {
  it("renders each marker as a checkbox reflecting the source", () => {
    const view = render("- [ ] todo\n- [x] done\n\nend\n");

    const boxes = view.contentDOM.querySelectorAll<HTMLInputElement>("input.cm-lp-task");
    expect(boxes).toHaveLength(2);
    expect(boxes[0].checked).toBe(false);
    expect(boxes[1].checked).toBe(true);
  });

  /** The point of the story: a checkbox you can tick, and the file says so. */
  it("ticks by click, and the source changes to match", () => {
    const view = render("- [ ] todo\n\nend\n");

    fireEvent.click(view.contentDOM.querySelector("input.cm-lp-task") as HTMLInputElement);

    expect(view.state.doc.toString()).toBe("- [x] todo\n\nend\n");
  });

  it("unticks by click on a done item", () => {
    const view = render("- [x] done\n\nend\n");

    fireEvent.click(view.contentDOM.querySelector("input.cm-lp-task") as HTMLInputElement);

    expect(view.state.doc.toString()).toBe("- [ ] done\n\nend\n");
  });

  /**
   * The checkbox that is not the first one in the document.
   *
   * A widget that captured its position when it was built would tick the wrong
   * line here as soon as anything above it moved; the position is asked of the
   * view at click time instead.
   */
  it("ticks the item that was clicked, not the first one", () => {
    const view = render("- [ ] one\n- [ ] two\n- [ ] three\n\nend\n");

    const boxes = view.contentDOM.querySelectorAll<HTMLInputElement>("input.cm-lp-task");
    fireEvent.click(boxes[2]);

    expect(view.state.doc.toString()).toBe("- [ ] one\n- [ ] two\n- [x] three\n\nend\n");
  });

  it("ticks the right item after the text above it has changed length", () => {
    const view = render("intro\n- [ ] one\n- [ ] two\n\nend\n");
    view.dispatch({ changes: { from: 0, to: 5, insert: "a much longer introduction" } });
    view.dispatch({ selection: { anchor: view.state.doc.length } });

    const boxes = view.contentDOM.querySelectorAll<HTMLInputElement>("input.cm-lp-task");
    fireEvent.click(boxes[1]);

    expect(view.state.doc.toString()).toBe(
      "a much longer introduction\n- [ ] one\n- [x] two\n\nend\n",
    );
  });

  /** Clicking must not move the caret onto the line: the reveal rule would show
   *  the source and the checkbox would vanish out from under the click. */
  it("does not let a click on the box move the caret onto its line", () => {
    const view = render("- [ ] todo\n\nend\n");
    const before = view.state.selection.main.head;

    const box = view.contentDOM.querySelector("input.cm-lp-task") as HTMLInputElement;
    expect(fireEvent.mouseDown(box)).toBe(false);

    expect(view.state.selection.main.head).toBe(before);
  });

  it("shows the marker as source, with no checkbox, when the caret is on the line", () => {
    const view = render("- [ ] todo\n\nend\n");
    view.dispatch({ selection: { anchor: 2 } });

    expect(view.contentDOM.querySelector("input.cm-lp-task")).toBeNull();
    expect(shown(view)).toBe("- [ ] todo");
  });

  it("recognises a task in an ordered list and an uppercase tick", () => {
    const view = render("1. [X] shouted\n\nend\n");

    const box = view.contentDOM.querySelector<HTMLInputElement>("input.cm-lp-task");
    expect(box?.checked).toBe(true);
    fireEvent.click(box as HTMLInputElement);
    expect(view.state.doc.toString()).toBe("1. [ ] shouted\n\nend\n");
  });
});

describe("a fenced code block is not inline code", () => {
  it("gives a fence its own line class and no inline-code mark", () => {
    const view = render("before\n\n```ts\nconst a = 1;\n```\n\nend\n");

    expect(view.contentDOM.querySelectorAll(".cm-lp-fence").length).toBeGreaterThan(0);
    expect(view.contentDOM.querySelector(".cm-lp-code")).toBeNull();
  });

  it("gives inline code the inline mark and no fence line", () => {
    const view = render("a `snippet` here\n\nend\n");

    expect(view.contentDOM.querySelector(".cm-lp-code")?.textContent).toBe("snippet");
    expect(view.contentDOM.querySelector(".cm-lp-fence")).toBeNull();
  });

  /** The language is the one thing a fence says about itself, and hiding it by
   *  node name left every code block opening with a blank grey line. */
  it("keeps the language visible on the opening fence line", () => {
    const view = render("```ts\nconst a = 1;\n```\n\nend\n");

    expect(view.contentDOM.querySelector(".cm-lp-fence-info")?.textContent).toBe("ts");
  });

  it("keeps the body of a fence exactly as written", () => {
    const view = render("```\n**not bold**\n```\n\nend\n");

    expect(view.contentDOM.textContent).toContain("**not bold**");
  });
});

describe("DW-165: a mermaid fence in a real view", () => {
  /**
   * The crash, from the note editor's side rather than the file viewer's.
   *
   * `live-preview.ts` supplied `Decoration.replace({ block: true })` from a
   * `ViewPlugin`; CodeMirror refuses that and throws out of `EditorView`
   * construction, so any note containing a diagram could not be opened at all.
   * The fix moves that one decoration into `mermaidLayer`, a `StateField` —
   * `galleryLayer`'s shape, in the same extension list.
   */
  it("constructs, and replaces the fence with the diagram widget", () => {
    const view = render("intro\n\n```mermaid\ngraph TD;\nA-->B;\n```\n\nend\n");

    expect(view.contentDOM.querySelector(".cm-mermaid-block")).not.toBeNull();
    expect(view.contentDOM.textContent).not.toContain("graph TD;");
  });

  it("gives the diagram its source back when the caret is inside the fence", () => {
    const view = render("intro\n\n```mermaid\ngraph TD;\nA-->B;\n```\n\nend\n");
    view.dispatch({ selection: { anchor: view.state.doc.line(4).from } });

    expect(view.contentDOM.querySelector(".cm-mermaid-block")).toBeNull();
    expect(view.contentDOM.textContent).toContain("graph TD;");
  });

  it("leaves a fence in another language as a code block", () => {
    const view = render("```ts\nconst a = 1;\n```\n\nend\n");

    expect(view.contentDOM.querySelector(".cm-mermaid-block")).toBeNull();
    expect(view.contentDOM.textContent).toContain("const a = 1;");
  });
});
