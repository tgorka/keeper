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

describe("a link's predicates render as chips", () => {
  /** Every chip on the line, in the order a reader meets them. */
  function chips(view: EditorView): string[] {
    return [...view.contentDOM.querySelectorAll(".cm-lp-predicate")].map(
      (chip) => chip.textContent ?? "",
    );
  }

  it("draws one chip for one CURIE, and no braces", () => {
    const view = render("[Satoshi](Satoshi.md){schema:creator}\n\nend\n");

    expect(chips(view)).toEqual(["schema:creator"]);
    expect(shown(view)).toBe("Satoshischema:creator");
  });

  it("draws a chip per predicate in a comma-separated block", () => {
    const view = render("[Satoshi](Satoshi.md){schema:creator, foaf:knows}\n\nend\n");

    expect(chips(view)).toEqual(["schema:creator", "foaf:knows"]);
    expect(shown(view)).not.toContain("{");
  });

  /** Two blocks written back to back are one list, in the order they were
   *  written — the same merge the graph does. */
  it("draws adjacent blocks as one run of chips", () => {
    const view = render("[Satoshi](Satoshi.md){dcterms:source}{schema:status}\n\nend\n");

    expect(chips(view)).toEqual(["dcterms:source", "schema:status"]);
    expect(shown(view)).not.toContain("{");
  });

  /**
   * The empty prefix is the document's default vocabulary, and the chip shows
   * the BARE name. The colon is noise on screen — the name is what the registry
   * calls the predicate and what a query will be written against — and showing
   * it would make `{:depends_on}` and `{depends_on}` look like two things.
   */
  it("draws a default-vocabulary predicate without its colon", () => {
    const view = render("[JWT](auth.md){:depends_on}\n\nend\n");

    expect(chips(view)).toEqual(["depends_on"]);
    expect(shown(view)).toBe("JWTdepends_on");
    expect(view.contentDOM.querySelector(".cm-lp-predicate-prefix")).toBeNull();
  });

  /** Semantic Markdown V0's property-attribute rule: a bare word is a property
   *  name. It renders identically to the `:`-marked spelling of the same name,
   *  because it IS the same predicate. */
  it("draws a bare word as the same chip the colon spelling draws", () => {
    const view = render("[JWT](auth.md){depends_on}\n\nend\n");

    expect(chips(view)).toEqual(["depends_on"]);
    expect(view.contentDOM.querySelector(".cm-lp-predicate-prefix")).toBeNull();
  });

  /**
   * `rel="cites"` is folded into a predicate name by `IndexProjection`, so the
   * graph holds `cites` and the links panel shows it. This assertion is the
   * reverse of what it was before the projection existed, when the editor was
   * the only reader of this syntax: a chip that stayed away now would have the
   * editor and the panel disagreeing about which tokens are edges, on one
   * screen, about one link.
   */
  it("draws rel as the edge the index makes of it", () => {
    const view = render('[Satoshi](Satoshi.md){rel="cites"}\n\nend\n');

    expect(chips(view)).toEqual(["cites"]);
    expect(shown(view)).toBe("Satoshicites");
  });

  /** The spelling keeper shipped first, rendering as it always did: one chip,
   *  no prefix, and the same label a screen reader has always been given. */
  it("still draws the legacy reference spelling as one chip", () => {
    const view = render('[Belief](belief.md){reference="supports"}\n\nend\n');

    expect(chips(view)).toEqual(["supports"]);
    expect(view.contentDOM.querySelector(".cm-lp-predicate")).toHaveAttribute(
      "aria-label",
      "link kind: supports",
    );
    expect(view.contentDOM.querySelector(".cm-lp-predicate-prefix")).toBeNull();
  });

  /** A CURIE's vocabulary is quieter than its term, and the split is by weight
   *  because `--faint` on the chip's surface measures 3.32:1 — see the comment
   *  on `predicateChip`. What is pinned here is that both halves are there. */
  it("splits a chip into its prefix and its local part", () => {
    const view = render("[Satoshi](Satoshi.md){schema:creator}\n\nend\n");

    expect(view.contentDOM.querySelector(".cm-lp-predicate-prefix")?.textContent).toBe("schema:");
    expect(view.contentDOM.querySelector(".cm-lp-predicate-local")?.textContent).toBe("creator");
  });

  /**
   * A literal object is data, not a vocabulary term, and it says so by shape:
   * an `=` — the only part of a chip carrying no fact, so the only part
   * `--faint`'s 3.32:1 is spendable on — and the value in its own span, italic
   * at the resting weight against the local part's 500. The value keeps
   * `--muted-foreground` (5.01:1 light, 6.82:1 dark on `--muted`) because every
   * quieter token misses the 4.5:1 that anything carrying a fact needs.
   */
  it("draws a colon-marked pair as its predicate and its literal object", () => {
    const view = render('[Revenue](m.md){:type="Metric"}\n\nend\n');

    expect(shown(view)).toBe("Revenuetype=Metric");
    expect(view.contentDOM.querySelector(".cm-lp-predicate-equals")?.textContent).toBe("=");
    expect(view.contentDOM.querySelector(".cm-lp-predicate-object")?.textContent).toBe("Metric");
    // Distinct from a bare predicate, which has neither span — that is the
    // whole claim, and it is the one a reader depends on.
    const bare = render("[Revenue](m.md){:type}\n\nend\n");
    expect(bare.contentDOM.querySelector(".cm-lp-predicate-equals")).toBeNull();
    expect(bare.contentDOM.querySelector(".cm-lp-predicate-object")).toBeNull();
  });

  /** A screen reader gets the statement, not two loose words: without the verb
   *  `type Metric` is indistinguishable from two adjacent predicates. */
  it("names the literal object to a screen reader", () => {
    const view = render('[Revenue](m.md){:type="Metric"}\n\nend\n');

    expect(view.contentDOM.querySelector(".cm-lp-predicate")).toHaveAttribute(
      "aria-label",
      "link kind: type is Metric",
    );
  });

  /**
   * A chip on a fence line needs a different surface, and this is the one
   * defect in this feature that no DOM assertion found — it was found by
   * screenshotting the owner's own document in Chromium and reading the
   * pixels.
   *
   * A chip's background is `--muted`. So is a code block's. Both resolved to
   * `rgb(236, 234, 226)`, so the pill was drawn, correct, and invisible: the
   * annotation read as bare text on the fence line while identical chips one
   * line below read as labels. Two colours being equal is not a bug in any
   * function, which is exactly why only a rendered pixel could catch it.
   *
   * The fix measures better than the ordinary case rather than worse:
   * `--muted-foreground` on `--background` is 5.32:1 light and 7.34:1 dark,
   * against 4.96:1 and 6.82:1 on `--muted`.
   */
  it("gives a fence's chips a surface the code block does not already own", () => {
    const view = render('```json { :type="Metric" }\nbody\n```\n\nend\n');

    const onCode = view.contentDOM.querySelector(".cm-lp-predicates-on-code");
    expect(onCode).not.toBeNull();
    expect(onCode?.querySelector(".cm-lp-predicate")?.textContent).toBe("type=Metric");

    // A link's chips are on ordinary prose, so they keep the ordinary surface —
    // the marker means "this one is on code", not "predicates".
    const link = render('[Revenue](m.md){ :type="Metric" }\n\nend\n');
    expect(link.contentDOM.querySelector(".cm-lp-predicates")).not.toBeNull();
    expect(link.contentDOM.querySelector(".cm-lp-predicates-on-code")).toBeNull();
  });

  /**
   * kramdown's presentational tokens draw nothing and must not be mistaken for
   * predicates: a chip saying `highlight` would put a CSS class into a graph
   * somebody queries.
   */
  it("draws nothing for a class, an id or a bare IAL marker", () => {
    const view = render("[Satoshi](Satoshi.md){: .highlight #revenue}\n\nend\n");

    expect(chips(view)).toEqual([]);
    // No predicate in the block, so it is source — which is also the only way
    // the author can see the class they wrote.
    expect(shown(view)).toBe("Satoshi{: .highlight #revenue}");
  });

  it("draws the predicate of a block that also carries a class", () => {
    const view = render("[Satoshi](Satoshi.md){.highlight :depends_on}\n\nend\n");

    expect(chips(view)).toEqual(["depends_on"]);
  });

  /**
   * A block keeper cannot read stays exactly as the author typed it. Drawing a
   * chip over it would show what keeper understood and swallow the rest, which
   * is how somebody comes to trust a predicate they never wrote.
   */
  it("leaves a block of junk exactly as it is", () => {
    const view = render("[Satoshi](Satoshi.md){oops! 9nope}\n\nend\n");

    expect(chips(view)).toEqual([]);
    expect(shown(view)).toBe("Satoshi{oops! 9nope}");
  });

  /** `{class="wide"}` writes no predicate, so it is source — the same treatment
   *  a presentational pair has always had. */
  it("leaves a block of presentational pairs alone", () => {
    const view = render('[Satoshi](Satoshi.md){class="wide"}\n\nend\n');

    expect(chips(view)).toEqual([]);
    expect(shown(view)).toBe('Satoshi{class="wide"}');
  });

  /**
   * The braces stay in the file. This is a rendering decision over portable
   * markdown: the vault has to open in any editor, and a decoration that
   * rewrote the source would be a migration nobody asked for.
   */
  it("never changes the document text", () => {
    const doc = '[Satoshi](Satoshi.md){schema:creator, foaf:knows}{:type="Metric"}{oops!}\n\nend\n';
    const view = render(doc);

    expect(view.state.doc.toString()).toBe(doc);
  });

  it("gives the raw syntax back when the caret lands on the line", () => {
    const view = render("[Satoshi](Satoshi.md){schema:creator, foaf:knows}\n\nend\n");
    view.dispatch({ selection: { anchor: 3 } });

    expect(shown(view)).toBe("[Satoshi](Satoshi.md){schema:creator, foaf:knows}");
    expect(chips(view)).toEqual([]);
  });

  /** An external link carries them too: the predicate is about the link, not
   *  about where it points. */
  it("draws chips on a link to a URL", () => {
    const view = render("[keeper](https://x.example){schema:codeRepository}\n\nend\n");

    expect(chips(view)).toEqual(["schema:codeRepository"]);
  });

  /**
   * The owner's own spelling: the block goes after the emphasis markers that
   * CLOSE the link. A reader that scanned from the link's end would find `**`,
   * stop, and leave the whole annotation on screen as punctuation — which is
   * the common case in the notes this story was written for.
   */
  describe("a block written after the emphasis that closes the link", () => {
    it.each([
      ["strong", "**[JWT Auth Service](https://github.com)**{ :depends_on }"],
      ["emphasis", "*[JWT Auth Service](https://github.com)*{ :depends_on }"],
      ["underscores", "__[JWT Auth Service](https://github.com)__{ :depends_on }"],
    ])("draws the chip and hides the block, through %s", (_kind, line) => {
      const view = render(`${line}\n\nend\n`);

      expect(chips(view)).toEqual(["depends_on"]);
      // No stray `**` between the bolded text and the chip. Two decorations do
      // that between them — `EmphasisMark` hides the markers, the block's own
      // replace draws the chip — and this is the assertion that would catch
      // either of them letting go.
      expect(shown(view)).toBe("JWT Auth Servicedepends_on");
    });

    it("draws the owner's block-level annotation", () => {
      const view = render(
        "**[Managed by the Platform Team](https://company.internal)**{ :owned_by }\n\nend\n",
      );

      expect(chips(view)).toEqual(["owned_by"]);
      expect(shown(view)).toBe("Managed by the Platform Teamowned_by");
    });

    /** The block qualifies the emphasis's last word here, not the link, so it
     *  belongs to neither and nothing is drawn. Attaching it to the link would
     *  put an edge on the wrong subject. */
    it("draws nothing when the link is not what the emphasis closes on", () => {
      const view = render("**[JWT](https://github.com) and more**{ :depends_on }\n\nend\n");

      expect(chips(view)).toEqual([]);
      expect(shown(view)).toBe("JWT and more{ :depends_on }");
    });
  });
});

/**
 * A fence's own annotation, on the tail of its opening info string. Every
 * assertion about WHICH lines have one is really an assertion about the parser
 * agreeing with CommonMark, which is exactly why the renderer anchors on
 * `CodeInfo` and writes none of those rules a second time.
 */
describe("a fence's info string carries predicates", () => {
  /** Every chip in the document, in the order a reader meets them. */
  function chips(view: EditorView): string[] {
    return [...view.contentDOM.querySelectorAll(".cm-lp-predicate")].map(
      (chip) => chip.textContent ?? "",
    );
  }

  /** What a reader sees on the opening fence line. */
  function fenceLine(view: EditorView): string {
    return view.contentDOM.firstElementChild?.textContent ?? "";
  }

  it("draws the owner's whole annotation as chips beside the language", () => {
    const view = render(
      '```json { :type="Metric" :owned_by="https://company.internal" }\n{ "a": 1 }\n```\n\nend\n',
    );

    expect(chips(view)).toEqual(["type=Metric", "owned_by=https://company.internal"]);
    // The language stays: it is the one thing a fence says about itself, and the
    // space the author typed before the brace goes with the block so the line
    // does not gain a gap.
    expect(fenceLine(view)).toBe("jsontype=Metricowned_by=https://company.internal");
  });

  it("draws nothing, and does not throw, for an info string with no block", () => {
    const view = render("```json\nconst a = 1;\n```\n\nend\n");

    expect(chips(view)).toEqual([]);
    expect(fenceLine(view)).toBe("json");
  });

  /** A tilde fence is a fence. CommonMark gives it an info string on the same
   *  terms, and the parser produces the same `CodeInfo`. */
  it("annotates a tilde fence", () => {
    const view = render('~~~json { :type="Metric" }\nx\n~~~\n\nend\n');

    expect(chips(view)).toEqual(["type=Metric"]);
  });

  /** An unclosed fence still opens one, so its own line is still annotated —
   *  which is the state a note is in while somebody is typing it. */
  it("annotates a fence the author has not closed yet", () => {
    const view = render('```json { :type="Metric" }\nx\n');

    expect(chips(view)).toEqual(["type=Metric"]);
  });

  /**
   * Only the OUTERMOST opening fence is a fence. A four-backtick block holds a
   * three-backtick line as CONTENT, and annotating it would rewrite what is
   * inside somebody's code sample — the one thing a code fence promises not to
   * do.
   */
  it("leaves a nested ``` line inside a 4-backtick block alone", () => {
    const view = render('````md\n```json { :type="Metric" }\nx\n```\n````\n\nend\n');

    expect(chips(view)).toEqual([]);
    expect(view.contentDOM.textContent).toContain('```json { :type="Metric" }');
  });

  /** Four spaces of indent is an indented code block, not a fence, so there is
   *  no info string to read and the text stays verbatim. */
  it("leaves a four-space-indented fence line alone", () => {
    const view = render('    ```json { :type="Metric" }\nx\n\nend\n');

    expect(chips(view)).toEqual([]);
    expect(view.contentDOM.textContent).toContain('```json { :type="Metric" }');
  });

  /** Under four is a fence, and its annotation is read. */
  it("annotates a fence indented less than four spaces", () => {
    const view = render('   ```json { :type="Metric" }\nx\n```\n\nend\n');

    expect(chips(view)).toEqual(["type=Metric"]);
  });

  /**
   * A backtick fence's info string may not contain a backtick, so this line
   * opens no fence at all and the whole thing is a paragraph. Nothing is
   * annotated, and nothing is hidden.
   */
  it("draws nothing when the info string holds a backtick", () => {
    const view = render('```json { :a="`b`" }\nx\n```\n\nend\n');

    expect(chips(view)).toEqual([]);
  });

  /**
   * A closing fence carries nothing: CommonMark allows only spaces after it, so
   * a line with a block on it is not a closer and stays inside the code. The
   * annotation belongs to the opening fence, once.
   */
  it("never reads a block off a closing fence", () => {
    const view = render('```json { :type="Metric" }\nx\n``` { :nope }\n\nend\n');

    expect(chips(view)).toEqual(["type=Metric"]);
    expect(view.contentDOM.textContent).toContain("``` { :nope }");
  });

  /** The reveal rule holds here too: the line you are editing is the line you
   *  can see the syntax of. */
  it("gives the info string back when the caret lands on the fence line", () => {
    const view = render('```json { :type="Metric" }\nx\n```\n\nend\n');
    view.dispatch({ selection: { anchor: 2 } });

    expect(fenceLine(view)).toBe('```json { :type="Metric" }');
  });

  /** Braces do not nest, so a brace inside a quoted value leaves the run unable
   *  to reach the end of the info string. Nothing is drawn rather than a chip
   *  for `y`, which is a predicate the author never wrote. */
  it("draws nothing for an info string whose braces do not close cleanly", () => {
    const view = render('```json { :a="x{y}" }\nx\n```\n\nend\n');

    expect(chips(view)).toEqual([]);
  });

  /**
   * The block has to be the TAIL of the info string. `json { :a } and more` is
   * an info string with braces in it, exactly as `[a](b) {schema:creator}` is a
   * sentence with braces in it, and only the author knows which they meant.
   */
  it("draws nothing when the block is not the end of the info string", () => {
    const view = render('```json { :type="Metric" } and more\nx\n```\n\nend\n');

    expect(chips(view)).toEqual([]);
    expect(fenceLine(view)).toBe('json { :type="Metric" } and more');
  });

  /** A fence's junk block keeps its source for the same reason a link's does:
   *  chips over it would show the token keeper read and swallow the one the
   *  author has to fix. */
  it("leaves a fence's junk block exactly as it is", () => {
    const view = render("```json { oops! }\nx\n```\n\nend\n");

    expect(chips(view)).toEqual([]);
    expect(fenceLine(view)).toBe("json { oops! }");
  });

  it("never changes the document text", () => {
    const doc = '```json { :type="Metric" }\n{ "a": 1 }\n```\n\nend\n';
    const view = render(doc);

    expect(view.state.doc.toString()).toBe(doc);
  });
});
