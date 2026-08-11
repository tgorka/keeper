/**
 * `:shortcode:` emoji, driven through a real editor (Story 45.11).
 *
 * **Why nothing here inspects a `CompletionResult`.** Story 43.9: the `/` menu
 * had never opened for anybody, and every unit-level fact about it was true —
 * the trigger regex matched, the command table was populated, the `apply`
 * closures were correct. The one thing nobody had asserted was that a menu
 * appears, so that was the one thing that was false. Every test below types
 * into a real `EditorView` and reads back either the rows on offer or the text
 * in the note.
 *
 * The real generated table drives all of them but one. A `:tada:` that inserts
 * 🎉 is the feature; a `:tada:` that inserts a stub's idea of 🎉 is a test of
 * the test.
 */
import {
  acceptCompletion,
  autocompletion,
  completionStatus,
  currentCompletions,
} from "@codemirror/autocomplete";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterAll, afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { withRangeRects } from "@/test/layout";
import { emojiCompleteSource, emojiShortcodeCommit } from "./emoji-complete";

// CodeMirror measures with a `Range`, on any animation frame that elapses
// during a test. jsdom has no `Range.getClientRects`, so the throw lands outside
// every `try` a test can write and takes the run's exit code with it.
//
// Installed and handed back as a pair, because `Range.prototype` is shared with
// every other test. `afterAll` is the hook that can carry the undo — an
// `afterEach` undo restores the prototype while a just-destroyed view still has
// frames pending, which is a non-zero exit with every test reported as passing.
let restoreRects: (() => void) | null = null;
const mounted: EditorView[] = [];

beforeAll(() => {
  restoreRects = withRangeRects();
});

afterAll(() => {
  restoreRects?.();
  restoreRects = null;
});

afterEach(() => {
  for (const view of mounted.splice(0)) {
    view.destroy();
  }
  document.body.replaceChildren();
});

interface Typed {
  view: EditorView;
  text: () => string;
  /** The shortcodes the menu is offering, in the order it offers them. */
  offered: () => string[];
  /** Type more at the caret, the way a keystroke does. */
  type: (more: string) => void;
}

/**
 * Mount an editor holding `before`, then type `typing` at the caret.
 *
 * **One transaction per character**, because that is what typing is, and the
 * closing colon is a rule about a single inserted colon: a helper that inserted
 * `:tada:` in one change would be testing a paste and would find the commit
 * filter silent for a reason no user will ever reproduce.
 *
 * Every transaction carries `userEvent: "input.type"` because that is what real
 * typing carries, and both halves of the feature read it: a completion only
 * activates for a change that says it came from a keystroke, and the commit
 * filter only rewrites one. A test that forgot it would assert silence and pass.
 */
function typeInto(before: string, typing: string, extensions: readonly unknown[] = []): Typed {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc: before,
      selection: { anchor: before.length },
      extensions: [
        autocompletion({
          override: [emojiCompleteSource()],
          // Production keeps the default 75 ms, which exists so a popup landing
          // under a moving hand cannot be accepted by accident. It governs when
          // an accept may land, never what the menu offers.
          interactionDelay: 0,
        }),
        ...(extensions as never[]),
      ],
    }),
  });
  mounted.push(view);
  const type = (more: string) => {
    for (const character of more) {
      const at = view.state.selection.main.head;
      view.dispatch({
        changes: { from: at, insert: character },
        selection: { anchor: at + character.length },
        userEvent: "input.type",
      });
    }
  };
  type(typing);
  return {
    view,
    type,
    text: () => view.state.doc.toString(),
    offered: () => currentCompletions(view.state).map((option) => String(option.label)),
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
 * Absence has no signal to await, so this outlasts the plugin's activation
 * debounce several times over. The positive cases resolve on their first or
 * second poll, which is what makes this bound meaningful rather than a guess.
 */
async function stayedShut(view: EditorView): Promise<void> {
  await expect(
    vi.waitFor(() => expect(completionStatus(view.state)).toBe("active"), { timeout: 400 }),
  ).rejects.toThrow();
}

describe("the emoji menu", () => {
  it("opens on a colon plus a letter and offers matching shortcodes", async () => {
    const { view, offered } = typeInto("", ":tad");

    await opened(view);

    expect(offered()).toContain("tada");
  });

  it("shows a row a person can recognise — the character, then its name", async () => {
    const { view } = typeInto("", ":tad");
    await opened(view);

    const menu = document.querySelector(".cm-tooltip-autocomplete");

    expect(menu?.textContent).toContain("🎉");
    expect(menu?.textContent).toContain("tada");
  });

  it("inserts the emoji, not the shortcode, and takes the colon with it", async () => {
    const { view, text } = typeInto("party ", ":tad");
    await opened(view);

    expect(acceptCompletion(view)).toBe(true);

    // No `:` survives. What lands in the note is the character, which is the
    // only thing Obsidian and every other reader of this file will ever see.
    expect(text()).toBe("party 🎉");
  });

  it("narrows as more is typed", async () => {
    const { view, type, offered } = typeInto("", ":sm");
    await opened(view);
    const wide = offered().length;

    type("ile");
    await opened(view);

    expect(offered().length).toBeLessThan(wide);
    expect(offered()).toContain("smile");
  });

  it("matches a word in the middle of a shortcode", async () => {
    const { view, offered } = typeInto("", ":hands");

    await opened(view);

    expect(offered()).toContain("raised_hands");
  });

  it("stays shut for a bare colon at the start of a word", async () => {
    // The load-bearing half of the "no menu on every colon" rule. `Heading:`
    // below is refused for a DIFFERENT reason (the colon follows a letter), so
    // it cannot stand in for this: with the bare-colon rule deleted, this is
    // the case that opens a 50-row menu in the middle of somebody's prose.
    const { view } = typeInto("say ", ":");

    await stayedShut(view);
  });

  it("stays shut for a bare colon after a word", async () => {
    const { view } = typeInto("Heading", ":");

    await stayedShut(view);
  });

  it("stays shut for a colon followed by nothing but underscores", async () => {
    // `:___` cuts into no words, so it has narrowed nothing — the matcher would
    // hand back the head of the whole table for it.
    const { view } = typeInto("say ", ":___");

    await stayedShut(view);
  });

  it("stays shut for a colon in ordinary prose", async () => {
    const { view } = typeInto("", "Note: se");

    await stayedShut(view);
  });

  it("stays shut inside a clock time", async () => {
    const { view } = typeInto("meet at 12", ":30");

    await stayedShut(view);
  });

  it("stays shut inside a URL", async () => {
    const { view } = typeInto("see https", "://ex");

    await stayedShut(view);
  });

  it("stays shut after a key, where a colon separates a value", async () => {
    const { view } = typeInto("title", ":so");

    await stayedShut(view);
  });

  it("opens where a colon begins a word, even hard against punctuation", async () => {
    const { view, offered } = typeInto("(", ":tad");

    await opened(view);

    expect(offered()).toContain("tada");
  });

  it("offers nothing for a shortcode that does not exist", async () => {
    const { view } = typeInto("", ":zzzznotanemoji");

    await stayedShut(view);
  });

  it("asks its vocabulary for the typed word, and nothing else", async () => {
    // `from` sitting on the colon is exactly the Story 43.9 defect, and it is
    // invisible from outside until the day the matcher stops being ours. One
    // stub, driven through a real editor like everything else — what is faked
    // is the vocabulary, never the editor.
    const asked: string[] = [];
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: "",
        extensions: [
          autocompletion({
            override: [
              emojiCompleteSource((query) => {
                asked.push(query);
                return [{ shortcode: "tada", emoji: "🎉" }];
              }),
            ],
            interactionDelay: 0,
          }),
        ],
      }),
    });
    mounted.push(view);
    view.dispatch({
      changes: { from: 0, insert: ":tad" },
      selection: { anchor: 4 },
      userEvent: "input.type",
    });

    await opened(view);

    expect(asked).toContain("tad");
    expect(asked.some((query) => query.includes(":"))).toBe(false);
  });
});

describe("the closing colon", () => {
  it("turns a shortcode typed in full into its character", () => {
    // The affordance that makes this feel like nothing at all: somebody who
    // knows the shortcode types it end to end and never notices a menu.
    const { text } = typeInto("", ":tada:", [emojiShortcodeCommit()]);

    expect(text()).toBe("🎉");
  });

  it("leaves an unknown shortcode as the text it is", () => {
    const { text } = typeInto("", ":zzzznotanemoji:", [emojiShortcodeCommit()]);

    expect(text()).toBe(":zzzznotanemoji:");
  });

  it("leaves the caret after the character, ready for the next word", () => {
    const { view } = typeInto("party ", ":tada:", [emojiShortcodeCommit()]);

    expect(view.state.selection.main.head).toBe(view.state.doc.length);
    expect(view.state.doc.toString()).toBe("party 🎉");
  });

  it("commits a shortcode with punctuation in it", () => {
    const { text } = typeInto("", ":+1:", [emojiShortcodeCommit()]);

    expect(text()).toBe("👍");
  });

  it("folds case, so the menu can never offer what the colon refuses", () => {
    const { text } = typeInto("", ":TADA:", [emojiShortcodeCommit()]);

    expect(text()).toBe("🎉");
  });

  it("does not fire where the colon did not begin a word", () => {
    // `Reference:tada:` is not somebody writing an emoji, and the menu never
    // opened there either — the two halves ask the same question.
    const { text } = typeInto("Reference", ":tada:", [emojiShortcodeCommit()]);

    expect(text()).toBe("Reference:tada:");
  });

  it("does not fire on a bare `::`", () => {
    const { text } = typeInto("", "::", [emojiShortcodeCommit()]);

    expect(text()).toBe("::");
  });

  it("leaves a paste alone, even a paste of exactly one colon", () => {
    // A paste is not one person typing one colon. Rewriting it would be this
    // module deciding what a document says. The single-colon case is the one
    // that matters: a whole pasted line is refused by the shape of the change,
    // so only this one reaches the question of WHO typed it.
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({ doc: ":tada", extensions: [emojiShortcodeCommit()] }),
    });
    mounted.push(view);

    view.dispatch({ changes: { from: 5, insert: ":" }, userEvent: "input.paste" });
    view.dispatch({ changes: { from: 6, insert: " look: :tada:" }, userEvent: "input.paste" });

    expect(view.state.doc.toString()).toBe(":tada: look: :tada:");
  });

  it("leaves a reconcile from Rust alone", () => {
    // The note editor writes remote revisions into the buffer with no user
    // event on them. A filter that fired there would rewrite somebody else's
    // note behind their back.
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({ doc: ":tada", extensions: [emojiShortcodeCommit()] }),
    });
    mounted.push(view);

    view.dispatch({ changes: { from: 5, insert: ":" } });

    expect(view.state.doc.toString()).toBe(":tada:");
  });
});
