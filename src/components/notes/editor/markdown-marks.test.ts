/**
 * Story 55.3 — what `==` may and may not become.
 *
 * The failure this guards is not "highlight does not work"; it is `a == b`
 * turning the rest of a paragraph yellow, which is what a delimiter without
 * flanking rules does to an arithmetic note.
 */
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { syntaxTree } from "@codemirror/language";
import { EditorState } from "@codemirror/state";
import { describe, expect, it } from "vitest";
import { MARKDOWN_MARKS } from "./markdown-marks";

/** Every node of `name`'s text, in document order. */
function nodesNamed(text: string, name: string): string[] {
  const state = EditorState.create({
    doc: text,
    extensions: [markdown({ base: markdownLanguage, extensions: [...MARKDOWN_MARKS] })],
  });
  const found: string[] = [];
  syntaxTree(state).iterate({
    enter: (node) => {
      if (node.name === name) {
        found.push(state.sliceDoc(node.from, node.to));
      }
    },
  });
  return found;
}

const highlights = (text: string): string[] => nodesNamed(text, "Highlight");
/** The built-in this extension is modelled on, for the parity assertion. */
const strikes = (text: string): string[] => nodesNamed(text, "Strikethrough");

describe("== as a highlight", () => {
  it("marks a delimited run", () => {
    expect(highlights("say ==this== please")).toEqual(["==this=="]);
  });

  it("marks more than one on a line, and nests inside emphasis", () => {
    expect(highlights("==a== and ==b==")).toEqual(["==a==", "==b=="]);
    expect(highlights("**==both==**")).toEqual(["==both=="]);
  });

  it("leaves arithmetic alone", () => {
    // The whole reason the flanking rules are copied rather than simplified.
    expect(highlights("a == b")).toEqual([]);
    expect(highlights("if x == y and y == z then")).toEqual([]);
  });

  it("ignores a run that is never closed", () => {
    expect(highlights("==open and nothing after")).toEqual([]);
  });

  it("never opens on three in a row, and degrades exactly as ~~ does", () => {
    // A run of three is not two marks and a spare, so no delimiter opens at its
    // first character. What a *longer* run resolves to is then CommonMark's
    // flanking algorithm's business, not this extension's — and the answer is
    // the same one the built-in `~~` gives on the same input, which is the
    // claim worth pinning: `a ~~~x~~~ b` yields `~~x~~~`.
    expect(highlights("a ===x=== b")).toEqual(["==x==="]);
    expect(strikes("a ~~~x~~~ b")).toEqual(["~~x~~~"]);
  });

  it("is not a setext heading underline", () => {
    expect(highlights("heading\n===")).toEqual([]);
  });

  it("closes against punctuation", () => {
    expect(highlights("==this==, then")).toEqual(["==this=="]);
  });
});
