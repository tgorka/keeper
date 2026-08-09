import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcClient from "@/lib/ipc/client";
import type { RecordingNoteTargetVm } from "@/lib/ipc/client";
import { livePreview } from "./live-preview";
import {
  RECORDING_ASSET_SCHEME,
  RecordingEmbedWidget,
  recordingAssetUrl,
  releaseRecordingVideo,
  renderRecordingEmbedInto,
} from "./recording-embed";
import { WIKILINK_ATTR } from "./wikilink";

/** What the renderer's own widget — the one it constructs, with no seam for a
 *  test to inject through — reaches for. */
const indexed = vi.fn<typeof IpcClient.recordingNoteTargets>();

vi.mock("@/lib/ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof IpcClient>()),
  recordingNoteTargets: (sessionId: string) => indexed(sessionId),
}));

const SESSION = "01KYH5DXGP1XQRHTME8CJFVEJ6-01KZHS7EJB5QKR8T9CHXQ46RNS";

/** The folder as the note wrote it — before the Story 40.4 retitle below. */
const WRITTEN = "recordings/2026/2026-08-08 15.52 test";

/** Where the index says the session is NOW: same files, renamed folder. */
const FOUND = "recordings/2026/2026-08-08 15.52 pricing call";

const TARGETS: RecordingNoteTargetVm[] = [
  { relativePath: FOUND, absolutePath: `/Volumes/Rec/${FOUND}`, kind: "folder" },
  {
    relativePath: `${FOUND}/screen-0000.mov`,
    absolutePath: `/Volumes/Rec/${FOUND}/screen-0000.mov`,
    kind: "video",
  },
  {
    relativePath: `${FOUND}/manifest.json`,
    absolutePath: `/Volumes/Rec/${FOUND}/manifest.json`,
    kind: "file",
  },
];

/** A host holding the link the widget renders before it resolves anything. */
function host(target: string): HTMLElement {
  const node = document.createElement("span");
  node.className = "cm-lp-recording";
  const anchor = document.createElement("span");
  anchor.className = "cm-lp-wikilink";
  anchor.setAttribute(WIKILINK_ATTR, target);
  anchor.textContent = target;
  node.append(anchor);
  return node;
}

describe("recordingAssetUrl", () => {
  it("escapes each segment and keeps the separators, so a space cannot end the path", () => {
    expect(recordingAssetUrl(SESSION, `${FOUND}/screen-0000.mov`)).toBe(
      `${RECORDING_ASSET_SCHEME}://${SESSION}/recordings/2026/` +
        "2026-08-08%2015.52%20pricing%20call/screen-0000.mov",
    );
  });
});

describe("renderRecordingEmbedInto", () => {
  it("plays a video the session has, without autoplay and without preloading it", async () => {
    const node = host(`${WRITTEN}/screen-0000.mov`);

    await renderRecordingEmbedInto(node, SESSION, `${WRITTEN}/screen-0000.mov`, {
      load: async () => TARGETS,
    });

    const player = node.querySelector("video");
    expect(player).not.toBeNull();
    expect(player?.controls).toBe(true);
    expect(player?.preload).toBe("metadata");
    expect(player?.autoplay).toBe(false);
    // Resolved through the index, so the URL names where the session is now —
    // not the folder the note was written against before the retitle.
    expect(player?.getAttribute("src")).toContain("pricing%20call/screen-0000.mov");
    // The link it replaced is gone; a player and a link would be two answers.
    expect(node.querySelector(`[${WIKILINK_ATTR}]`)).toBeNull();
  });

  it("leaves manifest.json the link it was — a target is not the same as a playable one", async () => {
    const node = host(`${WRITTEN}/manifest.json`);

    await renderRecordingEmbedInto(node, SESSION, `${WRITTEN}/manifest.json`, {
      load: async () => TARGETS,
    });

    expect(node.querySelector("video")).toBeNull();
    expect(node.querySelector(`[${WIKILINK_ATTR}]`)?.textContent).toBe(`${WRITTEN}/manifest.json`);
  });

  it("degrades to the link when the session names no such file", async () => {
    const node = host(`${WRITTEN}/camera-0000.mov`);

    await renderRecordingEmbedInto(node, SESSION, `${WRITTEN}/camera-0000.mov`, {
      load: async () => TARGETS,
    });

    expect(node.querySelector("video")).toBeNull();
    expect(node.textContent).toBe(`${WRITTEN}/camera-0000.mov`);
  });

  it("degrades to the link when keeper cannot place the session at all", async () => {
    const node = host(`${WRITTEN}/screen-0000.mov`);

    // `null` is the honest answer for an unknown session, a folder that is not
    // on this machine, and a first run with no archive — all one fact here.
    await renderRecordingEmbedInto(node, SESSION, `${WRITTEN}/screen-0000.mov`, {
      load: async () => null,
    });

    expect(node.querySelector("video")).toBeNull();
    expect(node.querySelector(`[${WIKILINK_ATTR}]`)).not.toBeNull();
  });

  it("degrades to the link when the IPC call itself rejects, and never throws", async () => {
    const node = host(`${WRITTEN}/screen-0000.mov`);

    await expect(
      renderRecordingEmbedInto(node, SESSION, `${WRITTEN}/screen-0000.mov`, {
        load: () => Promise.reject(new Error("the archive is locked")),
      }),
    ).resolves.toBeUndefined();

    expect(node.querySelector("video")).toBeNull();
    expect(node.querySelector(`[${WIKILINK_ATTR}]`)).not.toBeNull();
  });

  it("puts the link back when the player cannot load what the index promised", async () => {
    const node = host(`${WRITTEN}/screen-0000.mov`);

    await renderRecordingEmbedInto(node, SESSION, `${WRITTEN}/screen-0000.mov`, {
      load: async () => TARGETS,
    });
    const player = node.querySelector("video") as HTMLVideoElement;
    // What a retitle between the resolve and the request looks like from here:
    // the URL 404s and the element fires `error`.
    player.dispatchEvent(new Event("error"));

    expect(node.querySelector("video")).toBeNull();
    expect(node.querySelector(`[${WIKILINK_ATTR}]`)?.textContent).toBe(
      `${WRITTEN}/screen-0000.mov`,
    );
  });

  it("does not attach a player to a host that was torn down while it resolved", async () => {
    const node = host(`${WRITTEN}/screen-0000.mov`);

    await renderRecordingEmbedInto(node, SESSION, `${WRITTEN}/screen-0000.mov`, {
      load: async () => TARGETS,
      cancelled: () => true,
    });

    expect(node.querySelector("video")).toBeNull();
  });
});

describe("RecordingEmbedWidget", () => {
  it("renders the ordinary link first, then puts the player in its place", async () => {
    const widget = new RecordingEmbedWidget(
      SESSION,
      `${WRITTEN}/screen-0000.mov`,
      `${WRITTEN}/screen-0000.mov`,
      { load: async () => TARGETS },
    );

    const dom = widget.toDOM();

    // Synchronously: the link, so the note never shows an empty box.
    expect(dom.querySelector(`[${WIKILINK_ATTR}]`)).not.toBeNull();
    await vi.waitFor(() => expect(dom.querySelector("video")).not.toBeNull());
  });

  it("releases the media element on teardown rather than leaving it holding the file", async () => {
    const widget = new RecordingEmbedWidget(
      SESSION,
      `${WRITTEN}/screen-0000.mov`,
      `${WRITTEN}/screen-0000.mov`,
      { load: async () => TARGETS },
    );
    const dom = widget.toDOM();
    await vi.waitFor(() => expect(dom.querySelector("video")).not.toBeNull());
    const player = dom.querySelector("video") as HTMLVideoElement;
    // Spied through to the real methods, never stubbed: teardown must survive
    // being called on a media element that never got as far as a decoder.
    const load = vi.spyOn(player, "load");
    const pause = vi.spyOn(player, "pause");

    widget.destroy(dom);

    // Paused, un-sourced and told to let the resource go: removing the node
    // alone would leave the range-request pipeline open against the volume.
    expect(pause).toHaveBeenCalled();
    expect(player?.hasAttribute("src")).toBe(false);
    expect(load).toHaveBeenCalled();
    expect(dom.childElementCount).toBe(0);
  });

  it("attaches no player after teardown, even when the index answers late", async () => {
    let answer: (targets: RecordingNoteTargetVm[]) => void = () => {};
    const widget = new RecordingEmbedWidget(
      SESSION,
      `${WRITTEN}/screen-0000.mov`,
      `${WRITTEN}/screen-0000.mov`,
      {
        load: () =>
          new Promise<RecordingNoteTargetVm[]>((resolve) => {
            answer = resolve;
          }),
      },
    );
    const dom = widget.toDOM();

    widget.destroy(dom);
    answer(TARGETS);
    await Promise.resolve();
    await Promise.resolve();

    expect(dom.querySelector("video")).toBeNull();
  });

  it("reuses the DOM for the same embed, so the caret moving cannot restart playback", () => {
    const one = new RecordingEmbedWidget(SESSION, "a/clip.mov", "a/clip.mov");
    const same = new RecordingEmbedWidget(SESSION, "a/clip.mov", "a/clip.mov");
    const other = new RecordingEmbedWidget(SESSION, "a/other.mov", "a/other.mov");
    const elsewhere = new RecordingEmbedWidget("01OTHER", "a/clip.mov", "a/clip.mov");

    expect(one.eq(same)).toBe(true);
    expect(one.eq(other)).toBe(false);
    expect(one.eq(elsewhere)).toBe(false);
  });

  it("keeps the player's own events and gives up the link's", async () => {
    const widget = new RecordingEmbedWidget(
      SESSION,
      `${WRITTEN}/screen-0000.mov`,
      `${WRITTEN}/screen-0000.mov`,
      { load: async () => TARGETS },
    );
    const dom = widget.toDOM();
    const anchor = dom.querySelector(`[${WIKILINK_ATTR}]`) as HTMLElement;

    // While it is a link, its click must reach the renderer's `mousedown`
    // handler — CodeMirror skips every handler under a widget that claims the
    // event, so claiming this one would break following the link.
    expect(widget.ignoreEvent({ target: anchor } as unknown as Event)).toBe(false);

    await vi.waitFor(() => expect(dom.querySelector("video")).not.toBeNull());
    const player = dom.querySelector("video") as HTMLVideoElement;

    // Once it is a player, pressing play must not move the caret: a revealed
    // line drops its decorations, which would un-render the thing just clicked.
    expect(widget.ignoreEvent({ target: player } as unknown as Event)).toBe(true);
  });
});

describe("releaseRecordingVideo", () => {
  it("does nothing to a host that never got a player", () => {
    const node = host("a/clip.mov");

    releaseRecordingVideo(node);

    expect(node.querySelector(`[${WIKILINK_ATTR}]`)).not.toBeNull();
  });
});

// jsdom performs no layout, so a `Range` reports no client rects at all and
// CodeMirror's measure pass — which runs in a `requestAnimationFrame`, after
// the assertions below have started awaiting — throws out of the test. Shimmed
// here rather than in the shared setup because this is the only suite that
// mounts a real `EditorView`; an empty rect list is exactly what "no layout"
// means, and CodeMirror falls back to its default metrics for it.
if (!Range.prototype.getClientRects) {
  Range.prototype.getClientRects = () =>
    Object.assign([] as DOMRect[], { item: () => null }) as unknown as DOMRectList;
  Range.prototype.getBoundingClientRect = () => new DOMRect();
}

/**
 * Through the real decoration layer, because everything above this point drives
 * the widget directly — and the wiring between "this line holds an `![[…]]`"
 * and "this note is a recording note" is where the feature actually lives.
 */
describe("livePreview, over a recording note", () => {
  beforeEach(() => {
    indexed.mockReset();
    indexed.mockResolvedValue(TARGETS);
  });

  function open(doc: string, session: string | null): EditorView {
    const parent = document.createElement("div");
    document.body.append(parent);
    return new EditorView({
      parent,
      state: EditorState.create({
        doc,
        extensions: [
          livePreview({
            assetUrl: (rel) => rel,
            onOpenLink: () => {},
            recordingSession: () => session,
          }),
        ],
      }),
    });
  }

  /**
   * Drain the microtasks the widget's resolve rides on, and nothing else.
   *
   * Deliberately not a timer or a `waitFor`: letting a frame run would start
   * CodeMirror's measure pass, and jsdom's zero-height layout makes it replace
   * the rendered lines with a viewport gap — which is a fact about jsdom, not
   * about this feature, and it would destroy the widget mid-assertion.
   */
  async function settle(): Promise<void> {
    for (let tick = 0; tick < 6; tick += 1) {
      await Promise.resolve();
    }
  }

  it("turns an embed of the session's video into a player", async () => {
    const view = open(`intro\n\n![[${WRITTEN}/screen-0000.mov]]\n\nafter\n`, SESSION);

    await settle();
    const player = view.contentDOM.querySelector("video");
    expect(player).not.toBeNull();
    expect(indexed).toHaveBeenCalledWith(SESSION);
    expect(player?.preload).toBe("metadata");
    // Where the session is NOW, not the folder the note's own text names: the
    // embed was written before the retitle and still resolves.
    expect(player?.getAttribute("src")).toBe(
      recordingAssetUrl(SESSION, `${FOUND}/screen-0000.mov`),
    );

    view.destroy();
  });

  it("leaves an embed alone in a note that is not about a recording", async () => {
    const view = open(`intro\n\n![[${WRITTEN}/screen-0000.mov]]\n\nafter\n`, null);

    await settle();
    expect(view.contentDOM.querySelector(".cm-lp-recording")).toBeNull();
    expect(view.contentDOM.querySelector("video")).toBeNull();
    // Still the wikilink it always was — that path is untouched, and no note
    // without a `session:` costs an IPC round trip.
    expect(view.contentDOM.querySelector(`[${WIKILINK_ATTR}]`)?.textContent).toBe(
      `${WRITTEN}/screen-0000.mov`,
    );
    expect(indexed).not.toHaveBeenCalled();

    view.destroy();
  });

  it("leaves an ordinary link a link, `!` being the whole of the difference", async () => {
    const view = open(`intro\n\n[[${WRITTEN}/screen-0000.mov]]\n\nafter\n`, SESSION);

    await settle();
    expect(view.contentDOM.querySelector(".cm-lp-recording")).toBeNull();
    expect(view.contentDOM.querySelector(`[${WIKILINK_ATTR}]`)).not.toBeNull();
    expect(indexed).not.toHaveBeenCalled();

    view.destroy();
  });

  it("degrades an embed of the manifest to the link, inside the real editor", async () => {
    // Off the first line: the caret starts at 0, and a line under the caret
    // shows its source instead of its decorations (UX-DR40).
    const view = open(`intro\n\n![[${WRITTEN}/manifest.json]]\n`, SESSION);

    await settle();
    expect(view.contentDOM.querySelector("video")).toBeNull();
    expect(view.contentDOM.querySelector(`[${WIKILINK_ATTR}]`)?.textContent).toBe(
      `${WRITTEN}/manifest.json`,
    );

    view.destroy();
  });
});
