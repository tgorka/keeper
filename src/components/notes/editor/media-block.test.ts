/**
 * The `keeper-media` block's layer, through a real `EditorView` with the real
 * markdown grammar and the real `livePreview` — the facts most likely to break
 * (a block decoration from a `StateField`, the reveal, the splice at the range
 * found again) are CodeMirror's, and mocking it would test nothing.
 *
 * The React panel is replaced by a spy mount: what is asserted here is what
 * the layer hands it and what it does with the panel's answers.
 */
import { history, undo } from "@codemirror/commands";
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it, type Mock, vi } from "vitest";
import { livePreview } from "./live-preview";
import {
  insertOnOwnLine,
  MEDIA_BLOCK_BODY_CLASS,
  MEDIA_BLOCK_CLASS,
  MEDIA_BLOCK_NO_DRIVE,
  type MediaBlockMountArgs,
  MediaBlockWidget,
  type MountedMediaBlock,
  mediaFences,
  seekMediaBlock,
} from "./media-block";

const SESSION = "01J8A-01J8B";
const BLOCK = `\`\`\`keeper-media\nsession = "${SESSION}"\n\`\`\``;

interface Mounted {
  args: MediaBlockMountArgs;
  update: Mock<(args: MediaBlockMountArgs) => void>;
  unmount: Mock<() => void>;
}

type MountSpy = Mock<(container: HTMLElement, args: MediaBlockMountArgs) => MountedMediaBlock>;

/** A mount that records what each panel was handed, and mounts nothing. */
function mountSpy(): { mount: MountSpy; mounted: Mounted[] } {
  const mounted: Mounted[] = [];
  const mount: MountSpy = vi.fn((_container: HTMLElement, args: MediaBlockMountArgs) => {
    const entry: Mounted = { args, update: vi.fn(), unmount: vi.fn() };
    entry.update.mockImplementation((next) => {
      entry.args = next;
    });
    mounted.push(entry);
    return { update: entry.update, unmount: entry.unmount };
  });
  return { mount, mounted };
}

const views: EditorView[] = [];

afterEach(() => {
  for (const view of views.splice(0)) {
    view.destroy();
  }
});

function open(
  doc: string,
  over: { vaultId?: string; mount?: MountSpy; readOnly?: boolean; history?: boolean } = {},
): EditorView {
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
          noteLink: () => "Kelly sync",
          mountMedia: over.mount,
        }),
        ...(over.readOnly ? [EditorState.readOnly.of(true)] : []),
        ...(over.history ? [history()] : []),
      ],
    }),
  });
  views.push(view);
  return view;
}

/** Drain the microtasks the mount rides on; a frame would start a measure pass
 *  over jsdom's zero-height layout and replace the lines mid-assertion. */
async function settle(): Promise<void> {
  for (let tick = 0; tick < 6; tick += 1) {
    await Promise.resolve();
  }
}

describe("finding the block", () => {
  it.each([
    ["backticks", `intro\n\n${BLOCK}\n`],
    ["tildes", `intro\n\n~~~keeper-media\nsession = "${SESSION}"\n~~~\n`],
    ["an indent", `intro\n\n  \`\`\`keeper-media\n  session = "${SESSION}"\n  \`\`\`\n`],
    [
      "a list item",
      `intro\n\n- item\n\n  \`\`\`keeper-media\n  session = "${SESSION}"\n  \`\`\`\n`,
    ],
    [
      "more words in the info string",
      `intro\n\n\`\`\`keeper-media clip\nsession = "${SESSION}"\n\`\`\`\n`,
    ],
  ])("finds a fence written with %s", async (_, doc) => {
    const { mount, mounted } = mountSpy();
    const view = open(doc, { mount });

    await settle();

    expect(view.contentDOM.querySelectorAll(`.${MEDIA_BLOCK_CLASS}`)).toHaveLength(1);
    // The body between the fences, verbatim — indentation included: Rust reads
    // it, this layer does not (AD-351).
    expect(mounted[0]?.args.source).toContain(`session = "${SESSION}"`);
    expect(mounted[0]?.args.profileId).toBe("vault-1");
  });

  it.each([
    ["mermaid", "```mermaid\ngraph TD\n```"],
    ["toml", '```toml\nsession = "x"\n```'],
    ["no info string", '```\nsession = "x"\n```'],
    ["a longer word", '```keeper-media-extra\nsession = "x"\n```'],
  ])("leaves a %s fence alone", async (_, fence) => {
    const { mount } = mountSpy();
    const view = open(`intro\n\n${fence}\n`, { mount });

    await settle();

    expect(view.contentDOM.querySelector(`.${MEDIA_BLOCK_CLASS}`)).toBeNull();
    expect(mount).not.toHaveBeenCalled();
  });

  it("draws nothing over a fence still being typed", async () => {
    const { mount } = mountSpy();
    // No closing fence: the parse tree runs it to the end of the note, and a
    // player there would swallow everything below.
    const view = open(`intro\n\n\`\`\`keeper-media\nsession = "${SESSION}"\n\nmore text\n`, {
      mount,
    });

    await settle();

    expect(view.contentDOM.querySelector(`.${MEDIA_BLOCK_CLASS}`)).toBeNull();
  });

  it("takes a clip's words callout directly after the fence into the block", () => {
    const words = "> [!transcript]- Kelly sync · 00:12:00–00:15:30\n> **[00:12:03] Kelly:** So.";
    const state = EditorState.create({
      doc: `intro\n\n${BLOCK}\n${words}\n\nafter\n`,
      extensions: [markdown({ base: markdownLanguage })],
    });

    const [fence] = mediaFences(state);

    expect(state.doc.sliceString(fence.from, fence.to)).toBe(`${BLOCK}\n${words}`);
    // The body is still only what is between the fences.
    expect(fence.source).toBe(`session = "${SESSION}"`);
  });

  it("leaves a callout after a blank line to the person", () => {
    const state = EditorState.create({
      doc: `intro\n\n${BLOCK}\n\n> [!transcript]- mine\n> kept\n`,
      extensions: [markdown({ base: markdownLanguage })],
    });

    const [fence] = mediaFences(state);

    expect(state.doc.sliceString(fence.from, fence.to)).toBe(BLOCK);
  });
});

describe("the block in the note", () => {
  it("shows the fence's own text until the panel arrives, then the panel", () => {
    const view = open(`intro\n\n${BLOCK}\n`, { mount: undefined });
    const host = view.contentDOM.querySelector(`.${MEDIA_BLOCK_CLASS}`);

    // Synchronously the source — never an empty box (UX-DR124's resolving).
    expect(host?.textContent).toContain(`session = "${SESSION}"`);
  });

  it("says why it cannot draw where there is no drive", async () => {
    const { mount } = mountSpy();
    const view = open(`intro\n\n${BLOCK}\n`, { vaultId: "", mount });

    await settle();

    expect(view.contentDOM.querySelector(`.${MEDIA_BLOCK_CLASS}`)?.textContent).toContain(
      MEDIA_BLOCK_NO_DRIVE,
    );
    expect(mount).not.toHaveBeenCalled();
  });

  it("gives its source back when the caret enters it, and unmounts the panel", async () => {
    const { mount, mounted } = mountSpy();
    const view = open(`intro\n\n${BLOCK}\n\nafter\n`, { mount });
    await settle();

    view.dispatch({ selection: { anchor: view.state.doc.line(4).from } });
    await settle();

    expect(view.contentDOM.querySelector(`.${MEDIA_BLOCK_CLASS}`)).toBeNull();
    expect(view.contentDOM.textContent).toContain(`session = "${SESSION}"`);
    expect(mounted[0]?.unmount).toHaveBeenCalled();
  });

  it("reveals nothing on a click anywhere on a mounted block — the menu does that", async () => {
    const widget = new MediaBlockWidget("", "", {});
    // Before the panel arrives (its import still in flight) the fence's text
    // has no menu, so a click on it reveals the source like any fence.
    const loading = open(`intro\n\n${BLOCK}\n`, { mount: undefined });
    const fenceText = loading.contentDOM.querySelector(`.${MEDIA_BLOCK_CLASS} pre`);
    expect(widget.ignoreEvent({ target: fenceText } as unknown as Event)).toBe(false);

    const { mount } = mountSpy();
    const view = open(`intro\n\n${BLOCK}\n`, { mount });
    await settle();
    const host = view.contentDOM.querySelector(`.${MEDIA_BLOCK_CLASS}`) as HTMLElement;
    const body = host.querySelector(`.${MEDIA_BLOCK_BODY_CLASS}`) as HTMLElement;

    expect(widget.ignoreEvent({ target: body } as unknown as Event)).toBe(true);
    // The block's own edge, outside the panel: its head used to reveal here.
    expect(widget.ignoreEvent({ target: host } as unknown as Event)).toBe(true);
    expect(widget.ignoreEvent({ target: view.contentDOM } as unknown as Event)).toBe(false);
  });

  it("offers no marker edits where the text cannot be written", async () => {
    const { mount, mounted } = mountSpy();
    open(`intro\n\n${BLOCK}\n`, { mount, readOnly: true });
    await settle();

    expect(mounted[0]?.args.editable).toBe(false);
  });

  it("hands the panel the note's own link target, for Copy link", async () => {
    const { mount, mounted } = mountSpy();
    open(`intro\n\n${BLOCK}\n`, { mount });
    await settle();

    expect(mounted[0]?.args.noteLink).toBe("Kelly sync");
  });

  it("is not resolved again by a keystroke elsewhere in the note", async () => {
    const { mount } = mountSpy();
    const view = open(`intro\n\n${BLOCK}\n\nafter\n`, { mount });
    await settle();

    view.dispatch({ changes: { from: view.state.doc.length, insert: "more" } });
    await settle();

    expect(mount).toHaveBeenCalledTimes(1);
  });

  /**
   * The owner's hang: a transcribed recording note, Properties unfolded above
   * the editor, keeper stops answering. What a fold does to the editor is
   * geometry — its box resizes, CodeMirror measures again — and the transactions
   * that ride along carry no change to the block: effects, a caret elsewhere, the
   * store handing back the same text. If any of those re-created the widget, or
   * handed the panel a "new" body, the panel would mount a player, resolve and
   * re-measure — and the measure that follows is the next turn of the same loop.
   * So each turn is counted: across many of them the panel is mounted once,
   * never updated, never unmounted, and the block keeps its DOM.
   */
  it("mounts its panel once and keeps it across a fold opening and closing above the note", async () => {
    const { mount, mounted } = mountSpy();
    const view = open(`# Kelly sync\n\n${BLOCK}\n\nFollow-ups: send the quote.\n`, { mount });
    await settle();
    const host = view.contentDOM.querySelector(`.${MEDIA_BLOCK_CLASS}`);
    const parent = view.dom.parentElement as HTMLElement;

    for (let turn = 0; turn < 40; turn += 1) {
      parent.style.height = turn % 2 === 0 ? "300px" : "900px";
      view.requestMeasure();
      view.dispatch({});
      view.dispatch({ selection: { anchor: view.state.doc.length } });
      view.dispatch({ selection: { anchor: 0 } });
      await settle();
    }

    expect(mount).toHaveBeenCalledTimes(1);
    expect(mounted[0]?.update).not.toHaveBeenCalled();
    expect(mounted[0]?.unmount).not.toHaveBeenCalled();
    expect(view.contentDOM.querySelector(`.${MEDIA_BLOCK_CLASS}`)).toBe(host);
  });
});

describe("the block's menu verbs", () => {
  const WORDS = "> [!transcript]- Kelly sync · 00:00:06–00:00:24\n> **[00:00:06] Alex:** So.";

  it("removes the block and the words riding with it, and one undo brings both back", async () => {
    const { mount, mounted } = mountSpy();
    const doc = `intro\n\n${BLOCK}\n${WORDS}\n\nafter\n`;
    const view = open(doc, { mount, history: true });
    await settle();

    mounted[0]?.args.remove();

    expect(view.state.doc.toString()).toBe("intro\n\n\nafter\n");
    undo(view);
    expect(view.state.doc.toString()).toBe(doc);
  });

  it("removes a block that ends the note without leaving its line behind", async () => {
    const { mount, mounted } = mountSpy();
    const view = open(`intro\n${BLOCK}`, { mount });
    await settle();

    mounted[0]?.args.remove();

    expect(view.state.doc.toString()).toBe("intro");
  });

  it("shows the fence's text with the caret inside it on Edit block source", async () => {
    const { mount, mounted } = mountSpy();
    const view = open(`intro\n\n${BLOCK}\n\nafter\n`, { mount });
    await settle();

    mounted[0]?.args.editSource();
    await settle();

    expect(view.state.selection.main.head).toBe(view.state.doc.toString().indexOf("session"));
    expect(view.contentDOM.querySelector(`.${MEDIA_BLOCK_CLASS}`)).toBeNull();
    expect(view.contentDOM.textContent).toContain(`session = "${SESSION}"`);
  });

  it("hands the panel the editor's own scroller to grow in", async () => {
    const { mount, mounted } = mountSpy();
    const view = open(`intro\n\n${BLOCK}\n`, { mount });
    await settle();

    expect(mounted[0]?.args.scroller).toBe(view.scrollDOM);
  });
});

describe("writing a marker", () => {
  const MARKED = `session = "${SESSION}"\n\n[[marker]]\nname = "Deal"\nat = "00:13:05"`;

  it("splices Rust's new body over the block's body and nothing else, as one undoable edit", async () => {
    const { mount, mounted } = mountSpy();
    const view = open(`intro\n\n${BLOCK}\n\nafter\n`, { mount });
    await settle();

    expect(mounted[0]?.args.replaceSource(MARKED)).toBe(true);

    expect(view.state.doc.toString()).toBe(
      `intro\n\n\`\`\`keeper-media\n${MARKED}\n\`\`\`\n\nafter\n`,
    );
  });

  it("lands inside the block when the text above it changed since the panel mounted", async () => {
    const { mount, mounted } = mountSpy();
    const view = open(`intro\n\n${BLOCK}\n\nafter\n`, { mount });
    await settle();
    const replace = mounted[0]?.args.replaceSource;

    view.dispatch({ changes: { from: 0, insert: "a new paragraph above\n\n" } });
    await settle();

    expect(replace?.(MARKED)).toBe(true);
    expect(view.state.doc.toString()).toBe(
      `a new paragraph above\n\nintro\n\n\`\`\`keeper-media\n${MARKED}\n\`\`\`\n\nafter\n`,
    );
  });

  it("writes nothing when the block no longer holds what the panel read", async () => {
    const { mount, mounted } = mountSpy();
    const view = open(`intro\n\n${BLOCK}\n\nafter\n`, { mount });
    await settle();
    const replace = mounted[0]?.args.replaceSource;
    // Someone edited the body by hand since the panel asked Rust: the edit Rust
    // answered was computed against text that is no longer there.
    const at = view.state.doc.toString().indexOf(SESSION);
    view.dispatch({ changes: { from: at, to: at + SESSION.length, insert: "OTHER" } });
    await settle();
    const edited = view.state.doc.toString();

    expect(replace?.(MARKED)).toBe(false);
    expect(view.state.doc.toString()).toBe(edited);
  });

  it("hands the new body to the mounted panel rather than mounting a new player", async () => {
    const { mount, mounted } = mountSpy();
    open(`intro\n\n${BLOCK}\n\nafter\n`, { mount });
    await settle();

    mounted[0]?.args.replaceSource(MARKED);
    await settle();

    expect(mount).toHaveBeenCalledTimes(1);
    expect(mounted[0]?.update).toHaveBeenCalled();
    expect(mounted[0]?.args.source).toBe(MARKED);
    expect(mounted[0]?.unmount).not.toHaveBeenCalled();
  });
});

describe("one player at a time, and a marker link's seek", () => {
  it("pauses the block that was playing when another starts, and only that one", async () => {
    const { mount, mounted } = mountSpy();
    open(`intro\n\n${BLOCK}\n`, { mount });
    await settle();
    const claim = mounted[0]?.args.claimPlayback;
    const pauseFirst = vi.fn();
    const pauseSecond = vi.fn();

    // The registry is the view's: two claims are two blocks starting in turn.
    const releaseFirst = claim?.(pauseFirst);
    claim?.(pauseSecond);
    releaseFirst?.();
    claim?.(vi.fn());

    expect(pauseFirst).toHaveBeenCalledTimes(1);
    // A released claim is not paused again; the second, still playing, is.
    expect(pauseSecond).toHaveBeenCalledTimes(1);
  });

  it("seeks the block the link names by its place in the note", async () => {
    const { mount, mounted } = mountSpy();
    const view = open(`intro\n\n${BLOCK}\n\nmiddle\n\n${BLOCK.replace(SESSION, "OTHER")}\n`, {
      mount,
    });
    await settle();
    const first = vi.fn();
    mounted[0]?.args.register({ seekTo: first });

    // The second block is not drawn yet (it is below the viewport): its seek
    // waits for it, and the first block is not moved.
    expect(seekMediaBlock(view, 1, 785)).toBe(true);
    expect(first).not.toHaveBeenCalled();
    expect(seekMediaBlock(view, 0, 12)).toBe(true);
    expect(first).toHaveBeenCalledWith(12);
    expect(seekMediaBlock(view, 2, 1)).toBe(false);
  });

  it("keeps the seek for a panel that registers after the link was followed", async () => {
    const { mount, mounted } = mountSpy();
    const view = open(`intro\n\n${BLOCK}\n`, { mount });
    await settle();

    seekMediaBlock(view, 0, 42);
    const seek = vi.fn();
    mounted[0]?.args.register({ seekTo: seek });

    expect(seek).toHaveBeenCalledWith(42);
  });
});

describe("inserting a widget", () => {
  function plain(doc: string, anchor: number): EditorView {
    const view = new EditorView({ state: EditorState.create({ doc, selection: { anchor } }) });
    views.push(view);
    return view;
  }

  it("starts a line of its own when the caret is mid-sentence", () => {
    const view = plain("before after", 6);

    insertOnOwnLine(view, `${BLOCK}\n`);

    expect(view.state.doc.toString()).toBe(`before\n${BLOCK}\n after`);
  });

  it("writes on the empty line it was asked for, at the position given", () => {
    const view = plain("one\n\ntwo", 0);

    insertOnOwnLine(view, `${BLOCK}\n`, 4);

    expect(view.state.doc.toString()).toBe(`one\n${BLOCK}\n\ntwo`);
    expect(view.state.selection.main.head).toBe(4 + BLOCK.length + 1);
  });
});
