/**
 * Story 43.1. Every assertion here is about the document text or about whether
 * the keystroke reached the platform — never about what the keymap contains. A
 * keymap table can hold the right entry and still lose to precedence, to a
 * `contenteditable` that acted first, or to a command that declined to run.
 *
 * The stack under test is the one `note-editor.tsx` builds: the same keymaps in
 * the same order, the same markdown language, the same autocompletion. Bindings
 * are cheap to assert in isolation and worthless there, because the bug this
 * story fixes was a *gap* between extensions rather than a fault inside one.
 */
import { autocompletion, completionKeymap, completionStatus } from "@codemirror/autocomplete";
import { defaultKeymap, history, historyKeymap } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap } from "@codemirror/view";
import { describe, expect, it, vi } from "vitest";
import { indentBindings } from "./indent-keymap";
import { slashMenuSource } from "./slash-menu";
import { tagCompleteSource } from "./tag-complete";

/** Enough of a vault vocabulary for the popup to have something to offer. */
const VOCABULARY = ["work", "work/clients"];

// jsdom performs no layout, so a `Range` reports no client rects and
// CodeMirror's measure pass — which runs in a frame, after the awaits below
// have started — throws out of the test. Same shim, same reason, as
// `recording-embed.test.ts`.
if (!Range.prototype.getClientRects) {
  Range.prototype.getClientRects = () =>
    Object.assign([] as DOMRect[], { item: () => null }) as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
}

interface Opened {
  view: EditorView;
  /** The document, as the file on disk would read it. */
  text: () => string;
  /** Send a real keydown at the content DOM and hand back the event, so a test
   *  can ask the one question that matters for this story: did CodeMirror stop
   *  it, or did it fall through to the web view? */
  press: (key: string, options?: { shift?: boolean }) => KeyboardEvent;
}

/** Mount the editor's real extension stack over `doc`, caret at `caret`. */
function open(doc: string, caret = doc.length, head = caret): Opened {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc,
      selection: { anchor: caret, head },
      extensions: [
        EditorView.lineWrapping,
        history(),
        keymap.of([...defaultKeymap, ...historyKeymap, ...completionKeymap, ...indentBindings]),
        markdown({ base: markdownLanguage }),
        autocompletion({
          override: [tagCompleteSource(async () => VOCABULARY), slashMenuSource()],
          // Production keeps the default 75 ms, which exists so a popup that
          // appears under a hand already moving cannot be accepted by accident.
          // A test cannot out-wait a wall clock without becoming a timing
          // guess, and the delay governs WHEN an accept may land, never whether
          // Tab routes to the completion at all — which is the claim here.
          interactionDelay: 0,
        }),
      ],
    }),
  });

  return {
    view,
    text: () => view.state.doc.toString(),
    press: (key, options = {}) => {
      const event = new KeyboardEvent("keydown", {
        key,
        code: key,
        // `w3c-keyname` and the view's own tab-focus check both read
        // `keyCode`, and jsdom does not derive it from `key`.
        keyCode: key === "Tab" ? 9 : key === "Escape" ? 27 : key.charCodeAt(0),
        shiftKey: options.shift === true,
        bubbles: true,
        cancelable: true,
      });
      view.contentDOM.dispatchEvent(event);
      return event;
    },
  };
}

describe("Tab, in the note editor", () => {
  /**
   * The reported symptom, at its cause.
   *
   * Before this story nothing bound Tab, so CodeMirror never called
   * `preventDefault` and the keystroke went on to the web view, which edits a
   * `contenteditable` on its own terms. Asserting `defaultPrevented` is not a
   * proxy for the document — it is the fact that the platform never got a turn,
   * which is the only place the stray whitespace could have come from.
   */
  it("never reaches the web view, and adds no line to the document", () => {
    const { text, press, view } = open("first paragraph\n\nsecond paragraph\n", 15);

    const event = press("Tab");

    expect(event.defaultPrevented).toBe(true);
    expect(text()).toBe("  first paragraph\n\nsecond paragraph\n");
    expect(view.state.doc.lines).toBe(4);
    expect(text()).not.toContain("\t");

    view.destroy();
  });

  it("adds no line however many times it is pressed", () => {
    const { text, press, view } = open("alpha\n", 0);

    press("Tab");
    press("Tab");
    press("Tab");

    // Three units of indent on one line. Not three lines, not a tab, and
    // nothing at all before or after the text.
    expect(text()).toBe("      alpha\n");
    expect(view.state.doc.lines).toBe(2);

    view.destroy();
  });

  it("indents the caret's line and leaves the caret in the text", () => {
    const { text, press, view } = open("alpha\nbeta\n", 8);

    press("Tab");

    expect(text()).toBe("alpha\n  beta\n");
    // The caret rode the insertion rather than being stranded before it: it is
    // still between `b` and `eta`.
    expect(view.state.selection.main.head).toBe(10);

    view.destroy();
  });
});

describe("Shift-Tab, in the note editor", () => {
  it("outdents the caret's line", () => {
    const { text, press, view } = open("    alpha\n", 9);

    const event = press("Tab", { shift: true });

    expect(event.defaultPrevented).toBe(true);
    expect(text()).toBe("  alpha\n");

    view.destroy();
  });

  it("leaves a line that is already at the margin exactly as it was", () => {
    const { text, press, view } = open("alpha\nbeta\n", 3);

    press("Tab", { shift: true });

    expect(text()).toBe("alpha\nbeta\n");

    view.destroy();
  });

  it("undoes what Tab did, byte for byte", () => {
    const doc = "# Heading\n\nsome prose here\n";
    const { text, press, view } = open(doc, 14);

    press("Tab");
    expect(text()).not.toBe(doc);
    press("Tab", { shift: true });

    expect(text()).toBe(doc);

    view.destroy();
  });
});

describe("Tab over a selection", () => {
  it("indents every selected line once, and only the selected lines", () => {
    const doc = "one\ntwo\nthree\nfour\n";
    // From inside line 1 to inside line 3 — a partial selection still indents
    // whole lines, which is what makes it usable on a paragraph.
    const { text, press, view } = open(doc, 1, 10);

    press("Tab");

    expect(text()).toBe("  one\n  two\n  three\nfour\n");
    expect(view.state.doc.lines).toBe(5);

    view.destroy();
  });

  it("outdents every selected line once", () => {
    const { text, press, view } = open("    one\n    two\nthree\n", 1, 10);

    press("Tab", { shift: true });

    // `three` was never selected and keeps its margin; the two that were lose
    // exactly one unit each rather than all their indentation.
    expect(text()).toBe("  one\n  two\nthree\n");

    view.destroy();
  });
});

describe("Tab inside a list", () => {
  it("nests a bullet item", () => {
    const { text, press, view } = open("- alpha\n- beta\n", 14);

    press("Tab");

    // Two spaces is the content column of `- `, so CommonMark — and Obsidian
    // reading the same file — sees `beta` as a child of `alpha`.
    expect(text()).toBe("- alpha\n  - beta\n");

    view.destroy();
  });

  it("nests a task item without disturbing its checkbox", () => {
    const { text, press, view } = open("- [ ] alpha\n- [ ] beta\n", 18);

    press("Tab");

    expect(text()).toBe("- [ ] alpha\n  - [ ] beta\n");

    view.destroy();
  });

  it("nests several items at once", () => {
    const { text, press, view } = open("- alpha\n- beta\n- gamma\n", 10, 20);

    press("Tab");

    expect(text()).toBe("- alpha\n  - beta\n  - gamma\n");

    view.destroy();
  });

  /**
   * The one named limitation, asserted so it stays named.
   *
   * `1. ` has a content column of three, so a single two-space unit is not yet
   * a nesting — a second Tab gets there. Widening the unit to four would nest
   * an ordered item on the first press and would also turn any Tab on a plain
   * paragraph line into a CommonMark indented code block, which is a corrupted
   * document rather than a less-good indent. This is the deliberate side of
   * that trade.
   */
  it("takes two presses to nest an ordered item, and never inserts a tab", () => {
    const { text, press, view } = open("1. alpha\n2. beta\n", 16);

    press("Tab");
    expect(text()).toBe("1. alpha\n  2. beta\n");

    press("Tab");
    expect(text()).toBe("1. alpha\n    2. beta\n");
    expect(text()).not.toContain("\t");

    view.destroy();
  });
});

describe("the accessibility escape hatch", () => {
  /**
   * CodeMirror's default is to leave Tab to the platform *precisely* so a
   * keyboard user is not trapped. Claiming the key has to keep a way out, and
   * the way out is `@codemirror/view`'s own: Escape arms a two-second window in
   * which the next Tab is dropped before any keymap handler sees it.
   *
   * This holds because the binding goes through the view's own event dispatch.
   * A bare `addEventListener` on the content DOM would fire after the view has
   * declined the event and would trap the user; `tab-wiring.test.tsx` is where
   * that mutation is caught, because only a mounted editor has a content DOM
   * for a stray listener to attach to.
   */
  it("lets Escape then Tab leave the editor, changing nothing", () => {
    const { text, press, view } = open("alpha\n", 5);

    press("Escape");
    const tab = press("Tab");

    // Not prevented: the browser is free to move focus, which is the whole
    // contract. And the document is untouched on the way out.
    expect(tab.defaultPrevented).toBe(false);
    expect(text()).toBe("alpha\n");

    view.destroy();
  });

  it("re-arms Tab for indentation once the user types again", () => {
    const { text, press, view } = open("alpha\n", 5);

    press("Escape");
    press("Tab");
    // Any ordinary key spends the window; the hatch is a way out, not a mode.
    press("a");

    const tab = press("Tab");

    expect(tab.defaultPrevented).toBe(true);
    expect(text()).toBe("  alpha\n");

    view.destroy();
  });
});

describe("Tab while a completion is open", () => {
  /**
   * `indentMore` inserts at the LINE START, so a Tab pressed over an open popup
   * would push whitespace in front of the very `#` the popup is matching on:
   * the popup closes and the user is left with an indent they did not ask for,
   * which is this story's symptom wearing a different hat.
   */
  it("accepts the completion instead of pushing whitespace in front of it", async () => {
    const { text, press, view } = open("", 0);

    // A user-input transaction, because that is what opens the popup — and the
    // popup arrives on the completion plugin's own schedule, so the test waits
    // for the state it needs rather than for a duration.
    view.dispatch({
      changes: { from: 0, insert: "#w" },
      selection: { anchor: 2 },
      userEvent: "input.type",
    });
    await vi.waitFor(() => {
      expect(completionStatus(view.state)).toBe("active");
    });

    const tab = press("Tab");

    expect(tab.defaultPrevented).toBe(true);
    expect(text()).toBe("#work");

    view.destroy();
  });

  it("still indents when no completion is open", () => {
    const { text, press, view } = open("prose\n", 5);

    expect(completionStatus(view.state)).toBeNull();
    press("Tab");

    expect(text()).toBe("  prose\n");

    view.destroy();
  });
});
