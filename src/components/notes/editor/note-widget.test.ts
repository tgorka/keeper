/**
 * The markdown widgets (FR-264), asserted the way `gallery-block.test.ts`
 * asserts its block: the pure half directly, and the rendering half through a
 * real `EditorView` with a real markdown grammar and a real `livePreview`.
 *
 * Nothing here mocks CodeMirror. The two facts most likely to break — that a
 * block decoration must come from a `StateField` (DW-165), and that a widget
 * un-renders when the caret enters it — are facts about CodeMirror, and a test
 * that mocked it would assert only that this file's own idea of CodeMirror is
 * self-consistent.
 */
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";
import type { WidgetKind } from "@/lib/ipc/client";
import { livePreview } from "./live-preview";
import {
  type NoteWidgetOptions,
  parseWidgetBlock,
  WIDGET_BLOCK_CLASS,
  WIDGET_BODY_CLASS,
  WIDGET_KINDS,
  WIDGET_NO_HOST,
} from "./note-widget";

/** The injection point's own type, so a spy is checked against the signature
 *  the renderer will actually call rather than against `any`. */
type MountWidget = NonNullable<NoteWidgetOptions["mount"]>;

/** A mount that records its arguments and mounts nothing. What each test then
 *  asserts is the coordinates the panel was handed — the panel itself is
 *  `note-widget-host.tsx`'s business and has its own tests. */
function mountSpy() {
  return vi.fn<MountWidget>(() => ({ unmount: () => {} }));
}

// --- The syntax, and what Obsidian is left with -----------------------------

describe("the widget callout's syntax", () => {
  it("reads the kind and hands the query on verbatim", () => {
    expect(parseWidgetBlock("> [!board] tag:task path:projects/keeper")).toEqual({
      kind: "board",
      argument: "tag:task path:projects/keeper",
    });
  });

  it("reads a callout with no query as the kind's default, which Rust decides", () => {
    // Empty rather than a query composed here: `effective_query` in
    // `notes/widget.rs` is the one place that knows what a bare `[!log]` means.
    expect(parseWidgetBlock("> [!log]")).toEqual({ kind: "log", argument: "" });
  });

  it("matches the marker case-insensitively, as a callout does", () => {
    expect(parseWidgetBlock("> [!Refs] tag:ref")?.kind).toBe("refs");
  });

  it("knows exactly the three kinds Rust knows", () => {
    // The `Record<WidgetKind, true>` in the module is the compile-time half of
    // this; the array's contents are the runtime half, because the pattern is
    // built from them.
    expect(WIDGET_KINDS).toEqual<WidgetKind[]>(["board", "log", "refs"]);
  });

  it("is not a widget when the callout names something else", () => {
    // `> [!warning]` is Obsidian's own and must keep rendering as one.
    expect(parseWidgetBlock("> [!warning] mind the gap")).toBeNull();
    expect(parseWidgetBlock("> [!gallery] Photos/Trip")).toBeNull();
    expect(parseWidgetBlock("just a paragraph")).toBeNull();
  });
});

// --- The block, in a document -----------------------------------------------

describe("livePreview, over a note with a widget block", () => {
  const views: EditorView[] = [];

  afterEach(() => {
    for (const view of views.splice(0)) {
      view.destroy();
    }
  });

  function open(doc: string, over: { vaultId?: string; mount?: MountWidget } = {}): EditorView {
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc,
        extensions: [
          markdown({ base: markdownLanguage }),
          livePreview({
            vaultId: over.vaultId ?? "vault-1",
            assetUrl: (rel) => rel,
            onOpenLink: () => {},
            mountWidget: over.mount,
          }),
        ],
      }),
    });
    views.push(view);
    return view;
  }

  /** Drain the microtasks the mount rides on, and nothing else — a timer would
   *  start CodeMirror's measure pass and jsdom's zero-height layout would
   *  replace the rendered lines with a viewport gap mid-assertion. The same
   *  reason `gallery-block.test.ts` refuses one. */
  async function settle(): Promise<void> {
    for (let tick = 0; tick < 6; tick += 1) {
      await Promise.resolve();
    }
  }

  it("replaces the callout with a host and mounts the panel into it", async () => {
    const mount = mountSpy();
    const view = open("intro\n\n> [!board] tag:task\n\nafter\n", { mount });

    await settle();
    const host = view.contentDOM.querySelector(`.${WIDGET_BLOCK_CLASS}`);
    expect(host).not.toBeNull();
    expect(host?.querySelector(`.${WIDGET_BODY_CLASS}`)).not.toBeNull();
    expect(mount).toHaveBeenCalledTimes(1);
    // The coordinates the panel is given: the editor's vault, the kind, and the
    // callout's own text. No path and no query is composed here (AD-65).
    expect(mount.mock.calls[0]?.[1]).toEqual({
      vaultId: "vault-1",
      kind: "board",
      argument: "tag:task",
    });
  });

  it("shows the callout's own text before the panel arrives", () => {
    // No injected mount, so this takes the production path: the panel is behind
    // a dynamic `import()` and cannot possibly be on screen this tick. What IS
    // on screen is the callout, because a block that rendered blank and then
    // filled would be indistinguishable from a query that selected nothing.
    const view = open("intro\n\n> [!log] tag:log\n");

    const head = view.contentDOM.querySelector(".cm-note-widget-head");
    expect(head?.textContent).toContain("log");
    expect(head?.textContent).toContain("tag:log");
  });

  it.each(WIDGET_KINDS)("mounts the panel for a %s widget", async (kind) => {
    // One view per kind rather than three blocks in one document: each widget
    // reports a 240px `estimatedHeight`, and in jsdom — where every line
    // measures zero — three of them overflow CodeMirror's viewport and the last
    // is left as a `cm-gap`. That is jsdom's layout, not the widget's, and a
    // test asserting around it would be asserting on the wrong thing.
    const mount = mountSpy();
    open(`intro\n\n> [!${kind}]\n`, { mount });

    await settle();
    expect(mount).toHaveBeenCalledTimes(1);
    expect(mount.mock.calls[0]?.[1]).toEqual({ vaultId: "vault-1", kind, argument: "" });
  });

  it("says so rather than querying when there is no vault to query", async () => {
    const mount = mountSpy();
    const view = open("intro\n\n> [!refs]\n", { vaultId: "", mount });

    await settle();
    expect(view.contentDOM.querySelector(".cm-note-widget-note")?.textContent).toBe(WIDGET_NO_HOST);
    expect(mount).not.toHaveBeenCalled();
  });

  it("gives the source back when the caret enters the block", async () => {
    const mount = mountSpy();
    const view = open("intro\n\n> [!board] tag:task\n\nafter\n", { mount });

    await settle();
    expect(view.contentDOM.querySelector(`.${WIDGET_BLOCK_CLASS}`)).not.toBeNull();

    // Into the callout's own line: the marker and the query are text, and
    // editing them is how a person changes what the widget shows.
    view.dispatch({ selection: { anchor: view.state.doc.line(3).from + 4 } });
    await settle();
    expect(view.contentDOM.querySelector(`.${WIDGET_BLOCK_CLASS}`)).toBeNull();
    expect(view.contentDOM.textContent).toContain("[!board] tag:task");
  });

  it("unmounts the panel when the widget goes away", async () => {
    const unmount = vi.fn();
    const view = open("intro\n\n> [!board] tag:task\n", {
      mount: vi.fn<MountWidget>(() => ({ unmount })),
    });

    await settle();
    // Delete the block entirely — the widget is destroyed, and the React root
    // it owns must go with it rather than being left attached to a node
    // CodeMirror has dropped.
    const block = view.state.doc.line(3);
    view.dispatch({ changes: { from: block.from, to: block.to, insert: "" } });
    await settle();
    expect(unmount).toHaveBeenCalledTimes(1);
  });

  /**
   * The caret starts at 0, so a note whose very first line is a widget opens
   * showing its source — the same rule `galleryLayer` has, and worth stating
   * because it looks like a bug until you know it is the reveal.
   *
   * It is also self-correcting: click anywhere else and the widget draws. What
   * it costs is one surprising first impression; what it buys is that the block
   * at the top of a note is editable without a special case, and the two would
   * not both be possible.
   */
  it("opens showing the source when the caret's home is inside the first block", async () => {
    const mount = mountSpy();
    const view = open("> [!board] tag:task\n\nafter\n", { mount });

    await settle();
    expect(view.contentDOM.querySelector(`.${WIDGET_BLOCK_CLASS}`)).toBeNull();

    view.dispatch({ selection: { anchor: view.state.doc.length - 1 } });
    await settle();
    expect(view.contentDOM.querySelector(`.${WIDGET_BLOCK_CLASS}`)).not.toBeNull();
  });

  it("keeps one panel mounted while the caret moves outside it", async () => {
    const mount = mountSpy();
    const view = open("intro\n\n> [!board] tag:task\n\nafter\n", { mount });

    await settle();
    view.dispatch({ selection: { anchor: 2 } });
    await settle();
    view.dispatch({ selection: { anchor: view.state.doc.length - 2 } });
    await settle();
    // `eq` compares the block's source, so a redraw that produced an equal
    // widget reuses the mounted one — which is what keeps a board's drag state
    // and its scroll position across an unrelated keystroke.
    expect(mount).toHaveBeenCalledTimes(1);
  });

  it("leaves a callout inside somebody's quotation alone", async () => {
    const mount = mountSpy();
    const view = open("> quoting a plan:\n> [!board] tag:task\n", { mount });

    await settle();
    // The marker is on the second line of a blockquote that opened above it, so
    // it is part of that quotation — rendering it would swallow the line above.
    expect(view.contentDOM.querySelector(`.${WIDGET_BLOCK_CLASS}`)).toBeNull();
    expect(mount).not.toHaveBeenCalled();
  });

  it("draws a widget that follows another block without merging them", async () => {
    const mount = mountSpy();
    const view = open("intro\n\n> [!board] tag:task\n\n> [!refs]\n", { mount });

    await settle();
    expect(view.contentDOM.querySelectorAll(`.${WIDGET_BLOCK_CLASS}`)).toHaveLength(2);
  });
});
