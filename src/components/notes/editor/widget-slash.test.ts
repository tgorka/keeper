/**
 * The note widgets in the `/` menu, driven through a real `EditorView` with the
 * shared slash source beside them, as the note editor composes them.
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
import { GALLERY_HEAD } from "@/lib/notes/widgets";
import { withRangeRects } from "@/test/layout";
import { slashMenuSource } from "./slash-menu";
import { widgetSlashSource } from "./widget-slash";

let restoreRects: (() => void) | null = null;

beforeAll(() => {
  restoreRects = withRangeRects();
});

afterAll(() => {
  restoreRects?.();
});

const views: EditorView[] = [];

afterEach(() => {
  for (const view of views.splice(0)) view.destroy();
});

/** Type `typing` after `before`, as a keystroke — completion opens only for one. */
function type(before: string, typing: string, onPick = vi.fn()) {
  const parent = document.createElement("div");
  document.body.append(parent);
  const view = new EditorView({
    parent,
    state: EditorState.create({
      doc: before,
      selection: { anchor: before.length },
      extensions: [
        autocompletion({
          override: [widgetSlashSource(onPick), slashMenuSource()],
          interactionDelay: 0,
        }),
      ],
    }),
  });
  views.push(view);
  view.dispatch({
    changes: { from: before.length, insert: typing },
    selection: { anchor: before.length + typing.length },
    userEvent: "input.type",
  });
  return {
    view,
    onPick,
    offered: () => currentCompletions(view.state).map((option) => option.label),
  };
}

async function opened(view: EditorView): Promise<void> {
  await vi.waitFor(() => expect(completionStatus(view.state)).toBe("active"));
}

async function stayedShut(view: EditorView): Promise<void> {
  await expect(
    vi.waitFor(() => expect(completionStatus(view.state)).toBe("active"), { timeout: 400 }),
  ).rejects.toThrow();
}

describe("widgets in the slash menu", () => {
  it("offers the media player at the start of a line, beside the shared rows, once", async () => {
    const { view, offered } = type("", "/");

    await opened(view);

    expect(offered()).toContain("Media player…");
    expect(offered()).toContain("Task");
    // Gallery comes from the shared table here; the widget source adds no twin.
    expect(offered().filter((label) => label === "Gallery")).toHaveLength(1);
  });

  it("offers only the widgets after a space, so a path cannot become a subscript", async () => {
    const { view, offered } = type("see the", " /");

    await opened(view);

    expect(offered().sort()).toEqual(["Gallery", "Media player…"]);
  });

  it("is no menu for a slash glued to a word", async () => {
    // `/med` would reach "Media player…" if a glued slash opened anything.
    const { view } = type("and", "/med");

    await stayedShut(view);
  });

  it("removes what was typed and asks for a media pick at that place", async () => {
    const { view, onPick } = type("see the", " /med");
    await opened(view);

    expect(acceptCompletion(view)).toBe(true);

    expect(view.state.doc.toString()).toBe("see the ");
    expect(onPick).toHaveBeenCalledWith("see the ".length);
  });

  it("writes a gallery head on a line of its own when picked mid-line", async () => {
    const { view } = type("see the", " /gal");
    await opened(view);

    expect(acceptCompletion(view)).toBe(true);

    expect(view.state.doc.toString()).toBe(`see the \n${GALLERY_HEAD}`);
    expect(view.state.selection.main.head).toBe(view.state.doc.length);
  });
});
