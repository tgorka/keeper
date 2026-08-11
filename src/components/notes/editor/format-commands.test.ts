/**
 * Story 44.9, at the level a command can be proven.
 *
 * Every assertion here is the document text produced from a named selection.
 * Not a class, not a command name, not "the state has a mark": the promise the
 * toolbar makes is about what ends up in the note, and the note is what
 * Obsidian and `git diff` read.
 *
 * The editor these commands run in has the same markdown extension the real one
 * does, because the toggles ask the markdown parser whether a mark is already
 * there. A test that assembled a bare `EditorState` would prove the commands
 * work over a syntax tree that does not exist in the product.
 *
 * `format-toolbar.test.tsx` is the other half, and the half the ledger keeps
 * asking for: this file cannot see whether a button is wired to any of it.
 */

import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { syntaxTree } from "@codemirror/language";
import { EditorSelection, EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { withRangeRects } from "@/test/layout";
import { type FormatAction, formatCommand, gfmTable } from "./format-commands";

// jsdom has no `Range.getClientRects`, so CodeMirror's measure pass throws on
// any animation frame that elapses during a test. The hand-rolled shim this
// replaced installed an EMPTY `DOMRectList`, so a measure that did run read
// `rects[0]` as undefined and threw anyway. That was a permanent fault that
// only SHOWED as an occasional red, because whether a frame elapses at all
// depends on how busy the box is. It was never an ordering problem: vitest
// isolates per file (measured, not assumed — `isolate` and `pool` are unset in
// vitest.config.ts and the default is true), so every file starts with a clean
// prototype and the shim's `if (!…)` guard was always true. `withRangeRects`
// hands back a real rect; its undo is mandatory because `Range.prototype` is
// shared with every other test in this file's run.
let restoreRects: (() => void) | null = null;

beforeAll(() => {
  restoreRects = withRangeRects();
});

afterAll(() => {
  restoreRects?.();
  restoreRects = null;
});

interface Opened {
  view: EditorView;
  text: () => string;
  /** The text the selection currently covers — what a second press acts on. */
  selected: () => string;
  apply: (action: FormatAction) => void;
}

/**
 * Mount a real editor over `doc` with `select` (the first occurrence) selected.
 *
 * Selecting by the text rather than by an offset is deliberate: an offset in a
 * test is a number nobody can check, and a wrong one silently turns an
 * assertion about bold into an assertion about the space before it.
 */
function open(doc: string, select?: string): Opened {
  const at = select === undefined ? doc.length : doc.indexOf(select);
  if (select !== undefined && at < 0) {
    throw new Error(`selection ${JSON.stringify(select)} is not in the document`);
  }
  const view = new EditorView({
    state: EditorState.create({
      doc,
      selection: EditorSelection.single(at, at + (select?.length ?? 0)),
      extensions: [markdown({ base: markdownLanguage })],
    }),
  });
  return {
    view,
    text: () => view.state.doc.toString(),
    selected: () =>
      view.state.sliceDoc(view.state.selection.main.from, view.state.selection.main.to),
    apply: (action) => {
      formatCommand(action)(view);
    },
  };
}

/** Apply `action` twice from `select` and report the document each time. */
function roundTrip(doc: string, select: string, action: FormatAction) {
  const editor = open(doc, select);
  editor.apply(action);
  const once = editor.text();
  editor.apply(action);
  const twice = editor.text();
  editor.view.destroy();
  return { once, twice };
}

describe("the inline marks", () => {
  const marks: readonly { action: FormatAction; on: string; name: string }[] = [
    { name: "bold", action: { kind: "bold" }, on: "**word**" },
    { name: "italic", action: { kind: "italic" }, on: "*word*" },
    { name: "strikethrough", action: { kind: "strikethrough" }, on: "~~word~~" },
    { name: "inline code", action: { kind: "code" }, on: "`word`" },
    // Story 45.10's three. They join the existing table rather than getting a
    // suite of their own, because the promise a toolbar makes is the same for
    // all of them and a mark that only round-trips in a test written for it is
    // a mark nobody proved.
    { name: "subscript", action: { kind: "subscript" }, on: "~word~" },
    { name: "superscript", action: { kind: "superscript" }, on: "^word^" },
    { name: "underline", action: { kind: "underline" }, on: "<u>word</u>" },
  ];

  for (const mark of marks) {
    it(`wraps a selection in ${mark.name} and takes it off again`, () => {
      const { once, twice } = roundTrip("a word here", "word", mark.action);

      expect(once).toBe(`a ${mark.on} here`);
      // The toggle-off case, and the whole reason these are commands: the
      // second press has to give back the document the first one was handed.
      expect(twice).toBe("a word here");
    });

    it(`takes ${mark.name} off when the delimiters are inside the selection`, () => {
      const editor = open(`a ${mark.on} here`, mark.on);
      editor.apply(mark.action);

      expect(editor.text()).toBe("a word here");
      expect(editor.selected()).toBe("word");

      editor.view.destroy();
    });

    it(`takes ${mark.name} off from a caret sitting inside it`, () => {
      const editor = open(`a ${mark.on} here`, "wor");
      editor.apply(mark.action);

      expect(editor.text()).toBe("a word here");

      editor.view.destroy();
    });
  }

  it("leaves the selection on the same words, so a second press is a true undo", () => {
    const editor = open("a word here", "word");
    editor.apply({ kind: "bold" });

    expect(editor.selected()).toBe("word");

    editor.view.destroy();
  });

  it("writes an empty pair and sits inside it when nothing is selected", () => {
    const editor = open("a ");
    editor.apply({ kind: "bold" });

    expect(editor.text()).toBe("a ****");
    expect(editor.view.state.selection.main.head).toBe(4);

    editor.view.destroy();
  });

  it("does not mistake one half of a bold run for an italic delimiter", () => {
    // The failure this test exists for: looking at the characters either side
    // of the selection says "there is a `*` there" for bold text too, so a
    // naive italic toggle eats one star from each side and quietly downgrades
    // bold to italic. The parser knows the difference.
    const editor = open("a **word** here", "word");
    editor.apply({ kind: "italic" });

    expect(editor.text()).toBe("a ***word*** here");

    editor.view.destroy();
  });

  it("removes the inner pair when bold is nested inside italic", () => {
    const editor = open("a ***word*** here", "word");
    editor.apply({ kind: "bold" });

    expect(editor.text()).toBe("a *word* here");

    editor.view.destroy();
  });

  it("removes the outer pair when italic is toggled on the same nest", () => {
    const editor = open("a ***word*** here", "word");
    editor.apply({ kind: "italic" });

    expect(editor.text()).toBe("a **word** here");

    editor.view.destroy();
  });

  /**
   * The spelling choice, asserted as bytes.
   *
   * Obsidian reads this vault. `~word~` and `^word^` render there as
   * themselves — literal, legible, and losslessly round-tripped back into
   * keeper — while `<u>` is a real underline in Obsidian, on GitHub and in
   * Pandoc. What is NOT written is the reason this test exists: `__word__` is
   * bold in CommonMark, so binding underline to it would make the same file
   * say two different things depending on who opened it, and this editor's own
   * parser would not be able to tell the two apart either.
   */
  it("never writes __ for underline, because __ is bold everywhere else", () => {
    const editor = open("a word here", "word");
    editor.apply({ kind: "underline" });

    expect(editor.text()).toBe("a <u>word</u> here");
    expect(editor.text()).not.toContain("__");

    editor.view.destroy();
  });

  it("takes underline off a caret sitting inside the run", () => {
    const editor = open("a <u>word</u> here", "wor");
    editor.apply({ kind: "underline" });

    expect(editor.text()).toBe("a word here");

    editor.view.destroy();
  });

  it("removes the innermost underline when two are nested", () => {
    const editor = open("a <u>big <u>word</u> here</u> now", "word");
    editor.apply({ kind: "underline" });

    expect(editor.text()).toBe("a <u>big word here</u> now");

    editor.view.destroy();
  });

  /** An unclosed `<u>` three paragraphs up must not pair with the `</u>` under
   *  the caret and delete a tag the writer cannot see. */
  it("does not pair an underline across a paragraph break", () => {
    const editor = open("a <u>run\n\nplain word here\n", "word");
    editor.apply({ kind: "underline" });

    expect(editor.text()).toBe("a <u>run\n\nplain <u>word</u> here\n");

    editor.view.destroy();
  });

  it("writes an empty underline pair and sits between the tags", () => {
    const editor = open("a ");
    editor.apply({ kind: "underline" });

    expect(editor.text()).toBe("a <u></u>");
    expect(editor.view.state.selection.main.head).toBe(5);

    editor.view.destroy();
  });

  /** A single tilde is subscript and a double one is strikethrough; the parser
   *  is what tells them apart, so neither toggle may eat the other's marks. */
  it("does not mistake a strikethrough's tildes for a subscript's", () => {
    const editor = open("a ~~word~~ here", "word");
    editor.apply({ kind: "subscript" });

    expect(editor.text()).toBe("a ~~~word~~~ here");

    editor.view.destroy();
  });
});

describe("the link action", () => {
  it("wraps the selection and selects the destination to type over", () => {
    const editor = open("see the docs today", "the docs");
    editor.apply({ kind: "link" });

    expect(editor.text()).toBe("see [the docs](url) today");
    expect(editor.selected()).toBe("url");

    editor.view.destroy();
  });

  it("unwraps a link back to the words it was wrapping", () => {
    const editor = open("see [the docs](https://x.test) today", "the docs");
    editor.apply({ kind: "link" });

    expect(editor.text()).toBe("see the docs today");
    expect(editor.selected()).toBe("the docs");

    editor.view.destroy();
  });

  it("round-trips a fresh link", () => {
    const { once, twice } = roundTrip("see the docs today", "the docs", { kind: "link" });

    expect(once).toBe("see [the docs](url) today");
    expect(twice).toBe("see the docs today");
  });
});

describe("the block actions, over a multi-line selection", () => {
  const THREE = "alpha\nbeta\ngamma\n";

  it("bullets every selected line and unbullets them together", () => {
    const { once, twice } = roundTrip(THREE, "alpha\nbeta\ngamma", { kind: "bullet" });

    expect(once).toBe("- alpha\n- beta\n- gamma\n");
    expect(twice).toBe(THREE);
  });

  it("numbers the selected lines from one, in document order", () => {
    const { once, twice } = roundTrip(THREE, "alpha\nbeta\ngamma", { kind: "ordered" });

    expect(once).toBe("1. alpha\n2. beta\n3. gamma\n");
    expect(twice).toBe(THREE);
  });

  it("quotes every selected line and unquotes them together", () => {
    const { once, twice } = roundTrip(THREE, "alpha\nbeta\ngamma", { kind: "quote" });

    expect(once).toBe("> alpha\n> beta\n> gamma\n");
    expect(twice).toBe(THREE);
  });

  it("sets a heading level on every selected line and clears it on the second press", () => {
    const { once, twice } = roundTrip(THREE, "alpha\nbeta\ngamma", { kind: "heading", level: 2 });

    expect(once).toBe("## alpha\n## beta\n## gamma\n");
    expect(twice).toBe(THREE);
  });

  it("turns every selected line into a task and takes the boxes off together", () => {
    const { once, twice } = roundTrip(THREE, "alpha\nbeta\ngamma", { kind: "task" });

    // `- [ ] `, which is GFM and is what Obsidian draws as a checkbox. The
    // spelling is not keeper's to invent: the vault is read by both.
    expect(once).toBe("- [ ] alpha\n- [ ] beta\n- [ ] gamma\n");
    expect(twice).toBe(THREE);
  });

  /**
   * The trap the prefix regex had to be widened for.
   *
   * A task is `- ` plus `[ ] `. If the marker group stopped at the bullet, the
   * checkbox would be content, "is this already a task?" would be false, and
   * pressing the button on a task would produce `- [ ] [ ] alpha`.
   */
  it("does not add a second checkbox to a line that already has one", () => {
    const editor = open("- [ ] alpha\n", "alpha");
    editor.apply({ kind: "task" });

    expect(editor.text()).toBe("alpha\n");

    editor.view.destroy();
  });

  it("adds a box to an existing bullet rather than stacking a second bullet", () => {
    const editor = open("- alpha\n", "alpha");
    editor.apply({ kind: "task" });

    expect(editor.text()).toBe("- [ ] alpha\n");

    editor.view.destroy();
  });

  it("turns a task back into a plain bullet when bullet is pressed on it", () => {
    const editor = open("- [ ] alpha\n", "alpha");
    editor.apply({ kind: "bullet" });

    expect(editor.text()).toBe("- alpha\n");

    editor.view.destroy();
  });

  it("keeps a ticked box's tick when the marker is left alone", () => {
    // Heading owns the same prefix group, so it must take the WHOLE marker —
    // leaving an orphan `[x]` behind would be a heading that reads `# [x] a`.
    const editor = open("- [x] alpha\n", "alpha");
    editor.apply({ kind: "heading", level: 1 });

    expect(editor.text()).toBe("# alpha\n");

    editor.view.destroy();
  });

  it("takes a task's box off a quoted line without touching the quote", () => {
    const editor = open("> - [ ] alpha\n", "alpha");
    editor.apply({ kind: "task" });

    expect(editor.text()).toBe("> alpha\n");

    editor.view.destroy();
  });

  it("changes the level rather than clearing it when a different level is asked for", () => {
    const editor = open("## alpha\n## beta\n", "## alpha\n## beta");
    editor.apply({ kind: "heading", level: 4 });

    expect(editor.text()).toBe("#### alpha\n#### beta\n");

    editor.view.destroy();
  });

  it("only toggles off when every selected line already has the marker", () => {
    // A mixed selection means "make these all bullets", never "remove the one
    // bullet that is there" — the second reading throws away the marker the
    // user could see and is the reason this is a two-step decision.
    const editor = open("- alpha\nbeta\n", "- alpha\nbeta");
    editor.apply({ kind: "bullet" });

    expect(editor.text()).toBe("- alpha\n- beta\n");

    editor.view.destroy();
  });

  it("swaps one list marker for the other rather than stacking them", () => {
    const editor = open("- alpha\n- beta\n", "- alpha\n- beta");
    editor.apply({ kind: "ordered" });

    expect(editor.text()).toBe("1. alpha\n2. beta\n");

    editor.view.destroy();
  });

  it("keeps a bullet's indent when it becomes a numbered item", () => {
    const editor = open("  - alpha\n  - beta\n", "- alpha\n  - beta");
    editor.apply({ kind: "ordered" });

    expect(editor.text()).toBe("  1. alpha\n  2. beta\n");

    editor.view.destroy();
  });

  it("quotes a bulleted line without destroying the bullet", () => {
    // Treating the whole prefix as one token turns `- a` into `> a`: the quote
    // lands and the list is gone. Two prefix groups is what prevents it.
    const editor = open("- alpha\n- beta\n", "- alpha\n- beta");
    editor.apply({ kind: "quote" });

    expect(editor.text()).toBe("> - alpha\n> - beta\n");

    editor.view.destroy();
  });

  it("bullets inside a quote rather than replacing it", () => {
    const editor = open("> alpha\n> beta\n", "alpha\n> beta");
    editor.apply({ kind: "bullet" });

    expect(editor.text()).toBe("> - alpha\n> - beta\n");

    editor.view.destroy();
  });

  it("skips the blank line between two selected paragraphs", () => {
    const editor = open("alpha\n\ngamma\n", "alpha\n\ngamma");
    editor.apply({ kind: "quote" });

    expect(editor.text()).toBe("> alpha\n\n> gamma\n");

    editor.view.destroy();
  });

  it("acts on the empty line itself when that is all there is", () => {
    const editor = open("");
    editor.apply({ kind: "bullet" });

    expect(editor.text()).toBe("- ");
    // The caret goes after the marker, not around it: the next thing this user
    // does is type the item.
    expect(editor.view.state.selection.main.head).toBe(2);

    editor.view.destroy();
  });

  it("keeps the selection over the same lines, so a second press can find them", () => {
    const editor = open("alpha\nbeta\n", "alpha\nbeta");
    editor.apply({ kind: "quote" });

    expect(editor.selected()).toBe("> alpha\n> beta");

    editor.view.destroy();
  });

  it("keeps a partial selection inside the line it started in", () => {
    // The reason the change is a splice of the marker and not a rewrite of the
    // line: replacing a whole line collapses any caret inside it to the line's
    // edge, and the user's selection would vanish on every block action.
    const editor = open("alpha beta\n", "beta");
    editor.apply({ kind: "quote" });

    expect(editor.text()).toBe("> alpha beta\n");
    expect(editor.selected()).toBe("beta");

    editor.view.destroy();
  });
});

describe("the code block action, which is not the inline code action", () => {
  /**
   * The confusion this suite exists to rule out. `code` writes one backtick
   * either side of a run *within* a line; `codeblock` writes three on lines of
   * their own. They produce different nodes, they get different decorations,
   * and the two buttons must never converge — a toolbar whose "code" and "code
   * block" produced the same bytes is a toolbar with one button on it twice.
   */
  it("writes a fence on its own lines, where inline code writes backticks in one", () => {
    const block = open("alpha\n", "alpha");
    block.apply({ kind: "codeblock" });
    expect(block.text()).toBe("```\nalpha\n```\n");
    block.view.destroy();

    const inline = open("alpha\n", "alpha");
    inline.apply({ kind: "code" });
    expect(inline.text()).toBe("`alpha`\n");
    inline.view.destroy();
  });

  it("wraps every line the selection touches, whole", () => {
    const editor = open("intro\nalpha\nbeta\nend\n", "alpha\nbeta");
    editor.apply({ kind: "codeblock" });

    expect(editor.text()).toBe("intro\n```\nalpha\nbeta\n```\nend\n");

    editor.view.destroy();
  });

  it("keeps the body selected, so a second press is a true undo", () => {
    const editor = open("alpha\n", "alpha");
    editor.apply({ kind: "codeblock" });

    expect(editor.selected()).toBe("alpha");

    editor.apply({ kind: "codeblock" });
    expect(editor.text()).toBe("alpha\n");

    editor.view.destroy();
  });

  it("unwraps from a caret inside the fence, keeping the body", () => {
    const editor = open("intro\n```\nalpha\n```\nend\n", "alpha");
    editor.apply({ kind: "codeblock" });

    expect(editor.text()).toBe("intro\nalpha\nend\n");

    editor.view.destroy();
  });

  it("unwraps a fence with a language, dropping the info string with the fence", () => {
    const editor = open("```ts\nconst a = 1;\n```\n", "const");
    editor.apply({ kind: "codeblock" });

    expect(editor.text()).toBe("const a = 1;\n");

    editor.view.destroy();
  });

  it("leaves a caret on the empty middle line when nothing is selected", () => {
    const editor = open("");
    editor.apply({ kind: "codeblock" });

    expect(editor.text()).toBe("```\n\n```");
    // Offset four is past the opening fence and its newline: the next
    // keystroke is code, not a fourth backtick.
    expect(editor.view.state.selection.main.head).toBe(4);

    editor.view.destroy();
  });

  /** An empty fence has no body, and the two "delete the line break too"
   *  ranges an unwrap would otherwise use overlap on it. */
  it("unwraps an empty fence without throwing", () => {
    const editor = open("intro\n```\n```\nend\n", "```\n```");
    editor.apply({ kind: "codeblock" });

    expect(editor.text()).toBe("intro\n\nend\n");

    editor.view.destroy();
  });

  /**
   * A fence the user has opened but not yet closed still toggles off, and the
   * one fence line is all that goes.
   *
   * The weak version of this test — "the result still contains alpha, and is
   * not exactly `alpha`" — survived a mutation that turned the unwrap loose on
   * a single `CodeMark` and left a stray blank line behind. Asserted as the
   * whole document instead, because "did not eat the prose" is only meaningful
   * as an exact string.
   */
  it("unwraps an unterminated fence without eating what follows it", () => {
    const editor = open("```\nalpha\n", "alpha");
    editor.apply({ kind: "codeblock" });

    expect(editor.text()).toBe("alpha\n");

    editor.view.destroy();
  });

  it("does not eat the prose under an unterminated fence when there is some", () => {
    const editor = open("```\nalpha\n\nplain prose\n", "alpha");
    editor.apply({ kind: "codeblock" });

    expect(editor.text()).toBe("alpha\n\nplain prose\n");

    editor.view.destroy();
  });
});

describe("the table builder", () => {
  it("writes an aligned 3x2 with a header", () => {
    expect(gfmTable({ rows: 3, columns: 2, header: true })).toBe(
      [
        "| Column 1 | Column 2 |",
        "| -------- | -------- |",
        "|          |          |",
        "|          |          |",
        "",
      ].join("\n"),
    );
  });

  it("writes an aligned 3x2 without one, and still writes the delimiter row", () => {
    // GFM has no table without a delimiter row, and the row above it is the
    // header whether or not the user wanted one. "No header" therefore means an
    // empty header row and three rows to type in — not two.
    expect(gfmTable({ rows: 3, columns: 2, header: false })).toBe(
      [
        "|     |     |",
        "| --- | --- |",
        "|     |     |",
        "|     |     |",
        "|     |     |",
        "",
      ].join("\n"),
    );
  });

  it("pads every column to its widest cell so the pipes line up", () => {
    const lines = gfmTable({ rows: 2, columns: 3, header: true }).split("\n");
    const pipes = lines
      .filter((line) => line !== "")
      .map((line) => [...line].flatMap((char, index) => (char === "|" ? [index] : [])));

    expect(new Set(pipes.map((row) => row.join(","))).size).toBe(1);
  });

  it("parses as a GFM table in the same parser the editor uses", () => {
    // The AC asks that the output parse as a table, and the editor already
    // contains the GFM parser that will decide — so the question is put to the
    // thing that answers it in the product rather than to a regex written here.
    const editor = open("");
    editor.apply({ kind: "table", rows: 3, columns: 2, header: true });
    const state = EditorState.create({
      doc: editor.text(),
      extensions: [markdown({ base: markdownLanguage })],
    });
    const nodes: string[] = [];
    syntaxTree(state).iterate({ enter: (node) => void nodes.push(node.name) });
    editor.view.destroy();

    expect(nodes).toContain("Table");
    expect(nodes).toContain("TableHeader");
    expect(nodes.filter((name) => name === "TableRow")).toHaveLength(2);
  });

  it("puts the caret in the first cell so the first column can be named", () => {
    const editor = open("");
    editor.apply({ kind: "table", rows: 2, columns: 2, header: true });

    expect(editor.view.state.selection.main.head).toBe(2);

    editor.view.destroy();
  });

  it("starts the table on its own line when the caret is mid-sentence", () => {
    const editor = open("notes: ");
    editor.apply({ kind: "table", rows: 1, columns: 1, header: false });

    expect(editor.text()).toBe("notes: \n|     |\n| --- |\n|     |\n");

    editor.view.destroy();
  });

  it("never writes a table with no column or no row", () => {
    expect(gfmTable({ rows: 0, columns: 0, header: false })).toBe("|     |\n| --- |\n|     |\n");
  });
});
