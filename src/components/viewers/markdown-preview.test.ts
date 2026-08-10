/**
 * Markdown's rendered view, against a real `EditorView` (Story 45.4).
 *
 * **This file exists to perform the assembly nothing in this repository has
 * ever performed**: a real `EditorView` carrying BOTH `@codemirror/lang-markdown`
 * and `livePreview`. DW-165 says in as many words why that matters —
 * `mermaid-widget.test.ts` drives the widget directly and never asks the
 * renderer to place one, and `recording-embed.test.ts`, the one suite that does
 * build a real view around `livePreview`, loads it WITHOUT the markdown
 * language, so `syntaxTree` yields no `FencedCode`, the mermaid branch is never
 * entered, and a crash that has shipped since story 37.8 stayed invisible.
 *
 * A suite can be exhaustive about a widget and never once assemble the thing
 * the user assembles.
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

describe("DW-165 tripwire", () => {
  /**
   * **When this test fails, DW-165 has been fixed — invert it and delete the
   * guard in `markdown-preview.ts`.** Written as a passing assertion about the
   * broken behaviour rather than as a failing test, so the wave's gate stays
   * green while the defect stays pinned; 45.10's `StateField` lift is then
   * verified by a test whose author had no reason to shape it around the fix.
   */
  it("still throws when a real EditorView is given the markdown language and a mermaid fence", () => {
    const parent = document.createElement("div");
    expect(() => new EditorView({ parent, state: realState(MERMAID) })).toThrow(
      /block decorations/i,
    );
  });

  it("does not throw for the same document without the markdown language", () => {
    // Exactly why the existing suite is green: no grammar, no `FencedCode`
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

  it("declines a mermaid document by name and line instead of throwing or blanking", async () => {
    const info = vi.spyOn(console, "info").mockImplementation(() => {});
    const host = document.createElement("div");

    const preview = await mountMarkdownPreview(host, MERMAID, { vaultId: "vault-1" });

    expect(preview.failure).toContain("mermaid");
    expect(preview.failure).toContain("DW-165");
    expect(preview.failureLine).toBe(3);
    // Nothing half-drawn is left behind for the raw view to render on top of.
    expect(host.childNodes).toHaveLength(0);
    // A path that declines to act says so where the packaged app can see it —
    // `console.debug` never reaches the on-disk log (DW-162).
    expect(info).toHaveBeenCalledWith(expect.stringContaining("DW-165"));
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
    expect(preview.failureLine).toBeNull();
    expect(info).toHaveBeenCalled();
    expect(host.childNodes).toHaveLength(0);
    // Safe to call after a failure — every caller does, and one of them would
    // otherwise have to remember not to.
    expect(() => preview.destroy()).not.toThrow();
  });
});
