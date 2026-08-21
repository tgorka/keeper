import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * A drift guard for one line, and the measurement it stands for.
 *
 * jsdom lays out no flexbox, so no test here can watch a note escape its pane.
 * Measured instead against a real CodeMirror with `EditorView.lineWrapping` and
 * a single uncontained block, in a 600px pane:
 *
 *   uncontained child, no floor   →  `.cm-content` 1796px
 *   the same child, with a floor  →  `.cm-content`  600px
 *
 * 1796px is exactly what the pane was showing: every paragraph laid out at the
 * width of one wide child and clipped at the edge. The floor is what makes that
 * impossible rather than what makes it unlikely — each wide block keeps its own
 * `overflow-x`, so it scrolls inside itself instead of taking the prose with it.
 */
describe("the editor's content box has a width floor", () => {
  it("keeps min-width: 0 on .cm-content, so no child can widen the document", () => {
    const source = readFileSync(resolve(__dirname, "./live-preview.ts"), "utf8");
    expect(source).toMatch(/"\.cm-content":\s*\{\s*minWidth:\s*"0"/);
  });
});
