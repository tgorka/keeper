import { openSearchPanel, search, searchKeymap, searchPanelOpen } from "@codemirror/search";
import { EditorState } from "@codemirror/state";
import { EditorView, keymap, runScopeHandlers } from "@codemirror/view";
import { afterEach, expect, it } from "vitest";
import { clearSearchMarks, searchMarks, searchMarksField, setSearchMarks } from "./live-preview";

let view: EditorView | undefined;
afterEach(() => {
  view?.destroy();
});
it("closes Find before dismissing exact UTF-16 list marks", () => {
  view = new EditorView({
    parent: document.body,
    state: EditorState.create({
      doc: "ł😀tax",
      extensions: [keymap.of(searchKeymap), searchMarks(), search()],
    }),
  });
  view.dispatch({
    effects: setSearchMarks.of([[3, 6]]),
  });
  openSearchPanel(view);
  const ranges: number[][] = [];
  view.state.field(searchMarksField).between(0, 6, (from, to) => {
    ranges.push([from, to]);
  });
  expect(ranges).toEqual([[3, 6]]);
  expect(view.dom.querySelector(".cm-search-mark")).toHaveTextContent("tax");
  expect(runScopeHandlers(view, new KeyboardEvent("keydown", { key: "Escape" }), "editor")).toBe(
    true,
  );
  expect(searchPanelOpen(view.state)).toBe(false);
  expect(view.state.field(searchMarksField).size).toBe(1);
  expect(runScopeHandlers(view, new KeyboardEvent("keydown", { key: "Escape" }), "editor")).toBe(
    true,
  );
  expect(view.state.field(searchMarksField).size).toBe(0);
  expect(runScopeHandlers(view, new KeyboardEvent("keydown", { key: "Escape" }), "editor")).toBe(
    false,
  );
});
it("maps marks through edits and clears by effect", () => {
  const state = EditorState.create({ doc: "tax", extensions: [searchMarks()] });
  const marked = state.update({ effects: setSearchMarks.of([[0, 3]]) }).state;
  const moved = marked.update({ changes: { from: 0, insert: "a " } }).state;
  const ranges: number[][] = [];
  moved.field(searchMarksField).between(0, 5, (from, to) => {
    ranges.push([from, to]);
  });
  expect(ranges).toEqual([[2, 5]]);
  expect(
    moved.update({ effects: clearSearchMarks.of(null) }).state.field(searchMarksField).size,
  ).toBe(0);
});
