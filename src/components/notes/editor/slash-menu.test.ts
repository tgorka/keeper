/**
 * Story 43.9. The menu's failure was total and silent: it opened for nobody,
 * and every unit-level fact about it — the trigger regex, the command table,
 * the `apply` closures — was correct while the feature did not exist.
 *
 * So nothing here asserts a position or a regex. Each test drives the real
 * source through a real `EditorView` and asks the two questions a user asks:
 * is there a menu, and does picking a row put the right text in my note.
 */
import {
  acceptCompletion,
  autocompletion,
  completionStatus,
  currentCompletions,
} from "@codemirror/autocomplete";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterAll, beforeAll, describe, expect, it, vi } from "vitest";
import { withRangeRects } from "@/test/layout";
import { formatCommand } from "./format-commands";
import { SLASH_COMMANDS, slashMenuSource } from "./slash-menu";

// jsdom has no `Range.getClientRects`, so CodeMirror's measure pass throws on
// any animation frame that elapses during a test. The hand-rolled shim this
// replaced installed an EMPTY `DOMRectList`, so a measure that did run read
// `rects[0]` as undefined and threw anyway. That was a permanent fault that
// only SHOWED as an occasional red, because whether a frame elapses at all
// depends on how busy the box is. It was never an ordering problem: vitest
// isolates per file (measured, not assumed — `isolate` and `pool` are unset in
// vitest.config.ts and the default is true), so every file starts with a clean
// prototype and the shim's `if (!…)` guard was always true. `withRangeRects`
// hands back a real rect; its undo is mandatory — `Range.prototype` is shared.
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
  /** The labels the menu is currently offering, in the order it offers them. */
  offered: () => string[];
}

/** Mount an editor holding `before`, then type `typing` at the caret the way a
 *  user would — a completion only opens for a transaction that says it came
 *  from a keystroke. */
function type(before: string, typing: string): Opened {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc: before,
      selection: { anchor: before.length },
      extensions: [
        autocompletion({
          override: [slashMenuSource()],
          // Production keeps the default 75 ms, which exists so a popup landing
          // under a moving hand cannot be accepted by accident. It governs when
          // an accept may land, never what the menu offers.
          interactionDelay: 0,
        }),
      ],
    }),
  });
  view.dispatch({
    changes: { from: before.length, insert: typing },
    selection: { anchor: before.length + typing.length },
    userEvent: "input.type",
  });

  return {
    view,
    text: () => view.state.doc.toString(),
    offered: () => currentCompletions(view.state).map((option) => option.label),
  };
}

/** Wait for the completion plugin's own signal rather than for a duration. */
async function opened(view: EditorView): Promise<void> {
  await vi.waitFor(() => {
    expect(completionStatus(view.state)).toBe("active");
  });
}

/**
 * Assert the menu never opens.
 *
 * Absence has no signal to await, so this polls the same status the positive
 * cases do and concludes when it has outlasted the plugin's own activation
 * debounce several times over. The positive cases resolve on their first or
 * second poll, which is what makes this bound meaningful rather than a guess.
 */
async function stayedShut(view: EditorView): Promise<void> {
  await expect(
    vi.waitFor(() => expect(completionStatus(view.state)).toBe("active"), { timeout: 400 }),
  ).rejects.toThrow();
}

describe("the slash menu", () => {
  it("opens on a bare slash and offers every command", async () => {
    const { view, offered } = type("", "/");

    await opened(view);

    // As a set: with an empty pattern CodeMirror orders the rows itself, and
    // this story is about the menu existing, not about its ordering.
    expect(offered().sort()).toEqual(SLASH_COMMANDS.map((c) => c.label).sort());

    view.destroy();
  });

  it("shows the rows a user can read", async () => {
    const { view } = type("", "/");
    await opened(view);

    const menu = document.querySelector(".cm-tooltip-autocomplete");

    expect(menu?.textContent).toContain("Task");
    expect(menu?.textContent).toContain("- [ ] …");

    view.destroy();
  });

  it("narrows to the command being typed", async () => {
    const { view, offered } = type("", "/tas");

    await opened(view);

    // Fuzzy, not prefix — `tas` also reaches "Today's date" through
    // T-od-a-y'-s. What the user needs is the command they are typing toward
    // at the top of the list, which is where the ranking puts it.
    expect(offered()[0]).toBe("Task");
    expect(offered().length).toBeLessThan(SLASH_COMMANDS.length);

    view.destroy();
  });

  it("inserts the command and eats the slash with it", async () => {
    const { view, text } = type("", "/tas");
    await opened(view);

    expect(acceptCompletion(view)).toBe(true);

    // No `/` survives: what lands in the note is what the user would have
    // typed by hand, which is the only thing Obsidian will ever see.
    expect(text()).toBe("- [ ] ");

    view.destroy();
  });

  it("inserts a multi-line skeleton whole", async () => {
    const { view, text } = type("", "/tab");
    await opened(view);
    acceptCompletion(view);

    // Story 44.9 replaced 43.9's hand-written skeleton with the toolbar's
    // aligned builder, on purpose: one table command, one output. The pipes
    // line up and the columns are told apart, which is what makes the table
    // editable by hand in Obsidian and legible in a diff.
    expect(text()).toBe(
      "| Column 1 | Column 2 |\n| -------- | -------- |\n|          |          |\n",
    );

    view.destroy();
  });

  it("opens on a slash starting a line further down the note", async () => {
    const { view, offered } = type("# Heading\n\n", "/co");

    await opened(view);

    // The line is line three, so a menu that only ever worked at offset zero
    // would show nothing here.
    expect(offered()).toEqual(["Code fence"]);

    view.destroy();
  });

  /**
   * The fast-typist case, and the reason `validFor` had to move with `from`.
   *
   * `validFor` tells CodeMirror which continuations the open result still
   * covers, measured over the same span `from` names. Left describing a span
   * that starts at the `/` while `from` starts after it, every further
   * keystroke invalidates the result: the menu goes back to pending with no
   * rows, and an accept in that window refuses — the note keeps `/tas` and the
   * user's Enter did nothing.
   */
  it("keeps offering, and can be accepted, while the user is still typing", async () => {
    const { view, text, offered } = type("", "/");
    await opened(view);

    // A second keystroke transaction, then accept in the same tick.
    view.dispatch({
      changes: { from: 1, insert: "tas" },
      selection: { anchor: 4 },
      userEvent: "input.type",
    });

    expect(offered()[0]).toBe("Task");
    expect(acceptCompletion(view)).toBe(true);
    expect(text()).toBe("- [ ] ");

    view.destroy();
  });

  /**
   * The grammar this menu was given in Story 37.6, still enforced. It is the
   * reason the source cannot simply offer itself everywhere: a slash inside a
   * path or a fraction is a slash.
   */
  it("stays shut for a slash inside a sentence", async () => {
    const { view } = type("see docs", "/notes");

    await stayedShut(view);
    expect(completionStatus(view.state)).toBeNull();

    view.destroy();
  });

  it("stays shut when the caret is not at the end of the line", async () => {
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: "trailing",
        selection: { anchor: 0 },
        extensions: [autocompletion({ override: [slashMenuSource()], interactionDelay: 0 })],
      }),
    });
    view.dispatch({
      changes: { from: 0, insert: "/" },
      selection: { anchor: 1 },
      userEvent: "input.type",
    });

    await stayedShut(view);

    view.destroy();
  });
});

/**
 * Story 45.10's rows, and the caret they leave behind.
 *
 * A row that inserts a *pair* has to put the caret between the delimiters.
 * Without that the menu writes `^^` and parks the caret past both, so the user
 * types their exponent outside its own marks and gets `^^2` — which is the kind
 * of thing a person does once and then stops using the menu. Every case here
 * therefore asserts the offset as well as the bytes.
 */
describe("the marks Story 45.10 added", () => {
  const pairs: readonly { typed: string; label: string; inserted: string; caret: number }[] = [
    { typed: "/subs", label: "Subscript", inserted: "~~", caret: 1 },
    { typed: "/supe", label: "Superscript", inserted: "^^", caret: 1 },
    { typed: "/unde", label: "Underline", inserted: "<u></u>", caret: 3 },
  ];

  for (const pair of pairs) {
    it(`inserts ${pair.label} and leaves the caret between its delimiters`, async () => {
      const { view, text, offered } = type("", pair.typed);
      await opened(view);
      expect(offered()[0]).toBe(pair.label);

      expect(acceptCompletion(view)).toBe(true);

      expect(text()).toBe(pair.inserted);
      expect(view.state.selection.main.head).toBe(pair.caret);

      view.destroy();
    });
  }

  /**
   * The row that was already here and had the wrong caret.
   *
   * "Code fence" has been in this table since Story 37.6, and it parked the
   * caret after the closing fence — so the one thing you do next, type code,
   * happened below the block instead of inside it.
   */
  it("puts the caret inside the code fence, not after it", async () => {
    const { view, text } = type("", "/cod");
    await opened(view);
    acceptCompletion(view);

    expect(text()).toBe("```\n\n```\n");
    // Line two, the empty one between the fences.
    expect(view.state.doc.lineAt(view.state.selection.main.head).number).toBe(2);

    view.destroy();
  });

  it("puts the caret in the mermaid diagram's body, under `graph TD`", async () => {
    const { view, text } = type("", "/mer");
    await opened(view);
    acceptCompletion(view);

    expect(text()).toBe("```mermaid\ngraph TD\n\n```\n");
    expect(view.state.doc.lineAt(view.state.selection.main.head).number).toBe(3);

    view.destroy();
  });

  /**
   * The `/` menu and the toolbar must write the SAME bytes for the same mark.
   * Two doors into one note that disagree about how underline is spelled would
   * put both spellings in one vault, and only one of them would ever render.
   */
  it("spells each mark exactly as the toolbar's command spells it", () => {
    const spelling = (kind: "subscript" | "superscript" | "underline"): string => {
      const view = new EditorView({
        state: EditorState.create({ doc: "", selection: { anchor: 0 } }),
      });
      formatCommand({ kind })(view);
      const out = view.state.doc.toString();
      view.destroy();
      return out;
    };

    const rowFor = (label: string): string =>
      SLASH_COMMANDS.find((command) => command.label === label)?.text(new Date()) ?? "";

    expect(rowFor("Subscript")).toBe(spelling("subscript"));
    expect(rowFor("Superscript")).toBe(spelling("superscript"));
    expect(rowFor("Underline")).toBe(spelling("underline"));
  });
});
