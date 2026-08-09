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
import { describe, expect, it, vi } from "vitest";
import { SLASH_COMMANDS, slashMenuSource } from "./slash-menu";

// jsdom does no layout, so CodeMirror's measure pass would throw out of the
// test on the first frame. Same shim, same reason, as `recording-embed.test.ts`.
if (!Range.prototype.getClientRects) {
  Range.prototype.getClientRects = () =>
    Object.assign([] as DOMRect[], { item: () => null }) as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
}

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
