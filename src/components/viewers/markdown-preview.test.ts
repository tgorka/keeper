/**
 * Markdown's rendered view, against a real `EditorView` (Story 45.4, 45.10).
 *
 * **This file exists to perform the assembly nothing in this repository had
 * ever performed**: a real `EditorView` carrying BOTH `@codemirror/lang-markdown`
 * and `livePreview`. DW-165 was found by that assembly and nothing else —
 * `mermaid-widget.test.ts` drives the widget directly and never asks the
 * renderer to place one, and `recording-embed.test.ts`, the one suite that does
 * build a real view around `livePreview`, loads it WITHOUT the markdown
 * language, so `syntaxTree` yielded no `FencedCode`, the mermaid branch was
 * never entered, and a crash that had shipped since story 37.8 stayed invisible
 * for eight epics.
 *
 * A suite can be exhaustive about a widget and never once assemble the thing
 * the user assembles. That is why the assembly is still here after the fix.
 */

import { undo, undoDepth } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { ensureSyntaxTree } from "@codemirror/language";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterAll, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { livePreview } from "@/components/notes/editor/live-preview";
import { withRangeRects } from "@/test/layout";
import { mermaidFenceLine, mountMarkdownPreview } from "./markdown-preview";

/** What the note editor builds, minus the editing extensions: the grammar and
 *  the decoration layer, which is the pair DW-165 lives in. */
function realState(doc: string): EditorState {
  return EditorState.create({
    doc,
    extensions: [
      markdown({ base: markdownLanguage }),
      livePreview({ vaultId: "vault-1", assetUrl: (rel) => rel, onOpenLink: () => {} }),
    ],
  });
}

const MERMAID = "intro\n\n```mermaid\ngraph TD;\nA-->B;\n```\n\nafter\n";

// jsdom has no `Range.getClientRects`, and CodeMirror's measure pass calls it
// on any animation frame that elapses mid-test. See `withRangeRects`.
let removeRangeRects: (() => void) | null = null;
beforeAll(() => {
  removeRangeRects = withRangeRects();
});
afterAll(() => {
  removeRangeRects?.();
});
beforeEach(() => {
  vi.restoreAllMocks();
});

describe("DW-165, fixed", () => {
  /**
   * The inverse of the tripwire 45.4 left here. That test asserted the crash as
   * a passing fact so the wave's gate stayed green while the defect stayed
   * pinned, and said in as many words: when this fails, invert it and delete
   * the guard. Story 45.10 lifted the mermaid block decoration out of the
   * renderer's `ViewPlugin` into `mermaidLayer`, a `StateField`, so this is now
   * the same assembly asserting the same thing from the other side.
   */
  it("constructs a real EditorView over a mermaid fence instead of throwing", () => {
    const parent = document.createElement("div");
    const view = new EditorView({ parent, state: realState(MERMAID) });

    expect(view.contentDOM).not.toBeNull();
    // Not merely "did not throw": the fence is actually replaced by the widget,
    // which is the half a `try`/`catch` around construction could never prove.
    expect(view.contentDOM.querySelector(".cm-mermaid-block")).not.toBeNull();
    // And the fence's own three lines are gone from the rendered text, because
    // a block decoration that renders beside its source is not a replacement.
    expect(view.contentDOM.textContent).not.toContain("graph TD;");
    view.destroy();
  });

  it("gives the fence its source back when the caret is inside it", () => {
    const parent = document.createElement("div");
    const view = new EditorView({ parent, state: realState(MERMAID) });
    // Line 4 is `graph TD;`, inside the fence.
    view.dispatch({ selection: { anchor: view.state.doc.line(4).from } });

    expect(view.contentDOM.querySelector(".cm-mermaid-block")).toBeNull();
    expect(view.contentDOM.textContent).toContain("graph TD;");
    view.destroy();
  });

  it("does not throw for the same document without the markdown language", () => {
    // Exactly why the pre-45.4 suite was green: no grammar, no `FencedCode`
    // node, no mermaid branch, no crash. The extension set is the variable.
    const parent = document.createElement("div");
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc: MERMAID,
        extensions: [livePreview({ vaultId: "v", assetUrl: (r) => r, onOpenLink: () => {} })],
      }),
    });
    expect(view.contentDOM).not.toBeNull();
    view.destroy();
  });
});

describe("mermaidFenceLine", () => {
  it("finds the fence with the same grammar the renderer reads", () => {
    expect(mermaidFenceLine(realState(MERMAID), ensureSyntaxTree)).toBe(3);
  });

  it("finds a tilde fence and an indented one, which a regex would miss", () => {
    expect(mermaidFenceLine(realState("a\n\n~~~mermaid\ngraph TD;\n~~~\n"), ensureSyntaxTree)).toBe(
      3,
    );
    expect(mermaidFenceLine(realState("a\n\n  ```mermaid\nx\n  ```\n"), ensureSyntaxTree)).toBe(3);
  });

  it("is not fooled by another language, or by the word inside prose", () => {
    expect(mermaidFenceLine(realState("```ts\nconst a = 1;\n```\n"), ensureSyntaxTree)).toBeNull();
    expect(mermaidFenceLine(realState("I like mermaid diagrams.\n"), ensureSyntaxTree)).toBeNull();
    expect(mermaidFenceLine(realState("`mermaid`\n"), ensureSyntaxTree)).toBeNull();
  });
});

describe("mountMarkdownPreview", () => {
  it("renders a document into the host through the note editor's own decorations", async () => {
    const host = document.createElement("div");
    const preview = await mountMarkdownPreview(host, "# Title\n\nsome *emphasis*\n", {
      vaultId: "vault-1",
    });

    expect(preview.failure).toBeNull();
    // The decoration layer's own classes, so this is the renderer and not a
    // second one that happens to produce similar HTML.
    expect(host.querySelector(".cm-lp-h1")).not.toBeNull();
    expect(host.querySelector(".cm-lp-em")?.textContent).toBe("emphasis");
    preview.destroy();
  });

  it("row 6: is read-only when no caller named a destination for an edit", async () => {
    const host = document.createElement("div");
    const preview = await mountMarkdownPreview(host, "# Title\n", { vaultId: "vault-1" });

    // The clamp is the ABSENCE of `editing`, which is Story 51.5's shape for
    // "there is nowhere for a keystroke to go". Preview never grew a write path.
    expect(host.querySelector('[contenteditable="true"]')).toBeNull();
    preview.destroy();
  });

  it("renders an empty file without throwing and without a failure", async () => {
    const host = document.createElement("div");
    const preview = await mountMarkdownPreview(host, "", { vaultId: "vault-1" });

    expect(preview.failure).toBeNull();
    expect(host.querySelector(".cm-content")).not.toBeNull();
    preview.destroy();
  });

  it("renders outside a vault rather than refusing, and leaves an embed as its link", async () => {
    const host = document.createElement("div");
    const preview = await mountMarkdownPreview(host, "see [[notes/other]]\n", { vaultId: null });

    expect(preview.failure).toBeNull();
    expect(host.textContent).toContain("notes/other");
    preview.destroy();
  });

  it("renders a mermaid document instead of declining it (DW-165 is fixed)", async () => {
    const host = document.createElement("div");

    const preview = await mountMarkdownPreview(host, MERMAID, { vaultId: "vault-1" });

    expect(preview.failure).toBeNull();
    expect(host.querySelector(".cm-mermaid-block")).not.toBeNull();
    preview.destroy();
  });

  it("turns any other construction failure into a sentence rather than an exception", async () => {
    const info = vi.spyOn(console, "info").mockImplementation(() => {});
    // A real element, and a document with no fence in it, so the mermaid guard
    // above cannot be what caught this: the `try` has to be load-bearing on its
    // own for the throw nobody has found yet.
    const host = document.createElement("div");
    host.appendChild = () => {
      throw new Error("host is gone");
    };

    const preview = await mountMarkdownPreview(host, "# plain\n", { vaultId: "vault-1" });

    expect(preview.failure).toContain("host is gone");
    expect(info).toHaveBeenCalled();
    expect(host.childNodes).toHaveLength(0);
    // Safe to call after a failure — every caller does, and one of them would
    // otherwise have to remember not to.
    expect(() => preview.destroy()).not.toThrow();
    expect(() => preview.setContent("# other\n")).not.toThrow();
  });
});

/**
 * Note mode's half of this module (Story 51.5, FR-294).
 *
 * Asserted against a real `EditorView` for the same reason the rest of this file
 * is: what is being claimed is that the EDITABLE pane is the same assembly as
 * the read-only one plus a keymap, and a double of the mount could not tell that
 * from a second renderer.
 */
describe("mountMarkdownPreview, editable", () => {
  /** The mounted view behind a host, asserted rather than assumed. */
  function viewIn(host: HTMLElement): EditorView {
    const content = host.querySelector<HTMLElement>(".cm-content");
    expect(content, "nothing mounted a CodeMirror in that host").not.toBeNull();
    const view = EditorView.findFromDOM(content as HTMLElement);
    expect(view, "no EditorView is mounted in that content DOM").not.toBeNull();
    return view as EditorView;
  }

  /** The one caller shape: a destination for an edit, a destination for a save,
   *  and a name for the region. Presence is the mode. */
  function editing(recorded: { changes: string[]; saves: string[] }) {
    return {
      label: "Note of log.md",
      onChange: (next: string) => recorded.changes.push(next),
      onSave: (next: string) => recorded.saves.push(next),
    };
  }

  it("is editable, named, and drawn by the same decoration layer", async () => {
    const host = document.createElement("div");
    const recorded: { changes: string[]; saves: string[] } = { changes: [], saves: [] };
    const preview = await mountMarkdownPreview(host, "# Title\n\n*em*\n", {
      vaultId: "vault-1",
      editing: editing(recorded),
    });

    expect(preview.failure).toBeNull();
    expect(host.querySelector('[contenteditable="true"]')).not.toBeNull();
    expect(viewIn(host).state.readOnly).toBe(false);
    expect(host.querySelector(".cm-content")?.getAttribute("aria-label")).toBe("Note of log.md");
    // Not a second renderer: the note editor's own marks, in an editable view.
    expect(host.querySelector(".cm-lp-h1")).not.toBeNull();
    expect(host.querySelector(".cm-lp-em")?.textContent).toBe("em");
    preview.destroy();
  });

  it("reports a change as the exact buffer", async () => {
    const host = document.createElement("div");
    const recorded: { changes: string[]; saves: string[] } = { changes: [], saves: [] };
    const preview = await mountMarkdownPreview(host, "alpha\n", {
      vaultId: "vault-1",
      editing: editing(recorded),
    });
    const view = viewIn(host);

    view.dispatch({ changes: { from: view.state.doc.length, insert: "beta\n" } });

    // The whole document and not a patch: the host holds one buffer and saves
    // it, so a partial report is a file with a hole in it.
    expect(recorded.changes).toEqual(["alpha\nbeta\n"]);
    preview.destroy();
  });

  it("does not report an adoption as the reader's edit, which would be a loop", async () => {
    const host = document.createElement("div");
    const recorded: { changes: string[]; saves: string[] } = { changes: [], saves: [] };
    const preview = await mountMarkdownPreview(host, "alpha\n", {
      vaultId: "vault-1",
      editing: editing(recorded),
    });

    preview.setContent("gamma\n");

    expect(viewIn(host).state.doc.toString()).toBe("gamma\n");
    expect(recorded.changes).toEqual([]);
    preview.destroy();
  });

  it("leaves the caret alone when the text it is handed is the text it holds", async () => {
    const host = document.createElement("div");
    const recorded: { changes: string[]; saves: string[] } = { changes: [], saves: [] };
    const preview = await mountMarkdownPreview(host, "alpha\nbeta\n", {
      vaultId: "vault-1",
      editing: editing(recorded),
    });
    const view = viewIn(host);
    view.dispatch({ selection: { anchor: 3 } });

    preview.setContent("alpha\nbeta\n");

    // The no-op is what makes the controlled prop safe: the pane reports every
    // keystroke upward and the identical string comes back, and a dispatch of it
    // would map the selection to the end of the replacement on every character.
    expect(view.state.selection.main.head).toBe(3);
    preview.destroy();
  });

  it("hands `Mod-s` the document's own text, and writes nothing itself", async () => {
    const host = document.createElement("div");
    const recorded: { changes: string[]; saves: string[] } = { changes: [], saves: [] };
    const preview = await mountMarkdownPreview(host, "alpha\n", {
      vaultId: "vault-1",
      editing: editing(recorded),
    });
    const view = viewIn(host);
    view.dispatch({ changes: { from: view.state.doc.length, insert: "beta\n" } });

    // Ctrl and not Cmd: jsdom presents itself as something other than a Mac, so
    // CodeMirror binds `Mod` to Ctrl and a Cmd-flagged event would match
    // nothing, assert nothing, and still pass.
    view.contentDOM.dispatchEvent(
      new KeyboardEvent("keydown", { key: "s", ctrlKey: true, bubbles: true }),
    );

    // The text the VIEW holds, so a save cannot write a buffer it has moved
    // past — and the module itself reaches no write path to reach.
    expect(recorded.saves).toEqual(["alpha\nbeta\n"]);
    preview.destroy();
  });

  it("brings history with the keymap, so an edit can be taken back", async () => {
    const host = document.createElement("div");
    const recorded: { changes: string[]; saves: string[] } = { changes: [], saves: [] };
    const preview = await mountMarkdownPreview(host, "alpha\n", {
      vaultId: "vault-1",
      editing: editing(recorded),
    });
    const view = viewIn(host);

    view.dispatch({
      changes: { from: view.state.doc.length, insert: "beta\n" },
      userEvent: "input.type",
    });

    expect(undoDepth(view.state)).toBeGreaterThan(0);
    expect(undo(view)).toBe(true);
    expect(view.state.doc.toString()).toBe("alpha\n");
    preview.destroy();
  });

  it("keeps the file's own line endings, because a save follows this buffer", async () => {
    const host = document.createElement("div");
    const recorded: { changes: string[]; saves: string[] } = { changes: [], saves: [] };
    const preview = await mountMarkdownPreview(host, "alpha\r\nbeta\r\n", {
      vaultId: "vault-1",
      editing: editing(recorded),
    });

    // Without the `lineSeparator` facet CodeMirror hands back "\n" for every
    // line, so saving an untouched CRLF file would rewrite every line in it.
    expect(viewIn(host).state.doc.toString()).toBe("alpha\r\nbeta\r\n");
    preview.destroy();
  });

  it("answers null when an adoption lands, including the no-op", async () => {
    const host = document.createElement("div");
    const recorded: { changes: string[]; saves: string[] } = { changes: [], saves: [] };
    const preview = await mountMarkdownPreview(host, "alpha\n", {
      vaultId: "vault-1",
      editing: editing(recorded),
    });

    // One value to check for both outcomes, so a host cannot forget the failing
    // one: null is "the view is showing these bytes".
    expect(preview.setContent("gamma\n")).toBeNull();
    expect(preview.setContent("gamma\n")).toBeNull();
    preview.destroy();
  });

  it("turns an adoption the view refuses into a sentence, and stays reportable", async () => {
    const host = document.createElement("div");
    const recorded: { changes: string[]; saves: string[] } = { changes: [], saves: [] };
    const preview = await mountMarkdownPreview(host, "alpha\n", {
      vaultId: "vault-1",
      editing: editing(recorded),
    });
    const view = viewIn(host);
    const info = vi.spyOn(console, "info").mockImplementation(() => {});
    // The dispatch is the only place an adoption can fail, and a `StateField`
    // throwing inside one is DW-165's shape — `mermaidLayer` is such a field, and
    // CodeMirror swallows a view plugin's throw where it does not swallow a
    // field's. Made to throw here rather than contrived through a document,
    // because what is being asserted is this module's handling and not the
    // grammar's.
    const dispatch = vi.spyOn(view, "dispatch").mockImplementation(() => {
      throw new Error("a field refused this update");
    });

    // A throw would fail the test on this line, which is the whole contract: the
    // host's effect has no `try` around it, so an exception here takes the panel
    // down instead of falling back to the source (AD-88).
    const refusal = preview.setContent("gamma\n");

    expect(refusal).toContain("a field refused this update");
    expect(refusal).toContain("the source is below");
    expect(info).toHaveBeenCalled();

    // And the adoption flag was put back. A catch that skipped the reset would
    // leave `adopting` true for the life of the view, and every later keystroke
    // would be silently dropped instead of reported to the host that saves it —
    // a worse defect than the one being handled.
    dispatch.mockRestore();
    view.dispatch({ changes: { from: 0, insert: "x" }, userEvent: "input.type" });
    expect(recorded.changes).toEqual(["xalpha\n"]);
    preview.destroy();
  });
});
