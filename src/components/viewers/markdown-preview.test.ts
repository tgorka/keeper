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

  it("is read-only, because editing markdown is the note editor and its save path", async () => {
    const host = document.createElement("div");
    const preview = await mountMarkdownPreview(host, "# Title\n", { vaultId: "vault-1" });

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
  });
});
