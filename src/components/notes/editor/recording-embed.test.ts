import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcClient from "@/lib/ipc/client";
import type { RecordingNoteTargetVm } from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { livePreview } from "./live-preview";
import {
  FRAME_PRIME_SECONDS,
  primeFirstFrame,
  RECORDING_ASSET_SCHEME,
  RECORDING_EMBED_COPY_PATH_LABEL,
  RECORDING_EMBED_REVEAL_LABEL,
  RecordingEmbedWidget,
  recordingAssetUrl,
  releaseRecordingMedia,
  renderRecordingEmbedInto,
} from "./recording-embed";
import {
  BACK_LABEL,
  MAX_DRIFT_SECONDS,
  MUTE_LABEL,
  PLAY_LABEL,
  SCRUB_LABEL,
  SKIP_SECONDS,
  VOLUME_LABEL,
} from "./recording-transport";
import { WIKILINK_ATTR } from "./wikilink";

/** What the renderer's own widget — the one it constructs, with no seam for a
 *  test to inject through — reaches for. */
const indexed = vi.fn<typeof IpcClient.recordingNoteTargets>();
const revealed = vi.fn<typeof IpcClient.revealPath>();

vi.mock("@/lib/ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof IpcClient>()),
  recordingNoteTargets: (sessionId: string) => indexed(sessionId),
  revealPath: (path: string) => revealed(path),
}));

const SESSION = "01KYH5DXGP1XQRHTME8CJFVEJ6-01KZHS7EJB5QKR8T9CHXQ46RNS";

/** The folder as the note wrote it — before the Story 40.4 retitle below. */
const WRITTEN = "recordings/2026/2026-08-08 15.52 test";

/** Where the index says the session is NOW: same files, renamed folder. */
const FOUND = "recordings/2026/2026-08-08 15.52 pricing call";

/** One target of each kind, as Rust composes them: a session folder holding a
 *  video, an image, an audio sidecar and two files nothing can render. */
function target(name: string, kind: RecordingNoteTargetVm["kind"]): RecordingNoteTargetVm {
  return {
    relativePath: `${FOUND}/${name}`,
    absolutePath: `/Volumes/Rec/${FOUND}/${name}`,
    kind,
  };
}

const TARGETS: RecordingNoteTargetVm[] = [
  { relativePath: FOUND, absolutePath: `/Volumes/Rec/${FOUND}`, kind: "folder" },
  target("screen-0000.mov", "video"),
  target("whiteboard.png", "image"),
  target("room-tone.wav", "audio"),
  target("manifest.json", "file"),
  // An extension keeper has never heard of. Rust classified it `file`, which is
  // the whole point of the catch-all: it is an attachment, not a broken player.
  target("board.sketchpad", "file"),
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

/** Reveal is gated on the platform having a file manager, so every test states
 *  which platform it is on rather than inheriting the last one's. */
beforeEach(() => {
  revealed.mockReset();
  revealed.mockResolvedValue(undefined);
  // jsdom lacks a clipboard by default.
  Object.assign(navigator, { clipboard: { writeText: vi.fn(() => Promise.resolve()) } });
  capabilitiesStore
    .getState()
    .applySnapshot({ ...DEFAULT_CAPABILITIES, revealInFileManager: true });
});

afterEach(() => {
  capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
});

describe("recordingAssetUrl", () => {
  it("escapes each segment and keeps the separators, so a space cannot end the path", () => {
    expect(recordingAssetUrl(SESSION, `${FOUND}/screen-0000.mov`)).toBe(
      `${RECORDING_ASSET_SCHEME}://${SESSION}/recordings/2026/` +
        "2026-08-08%2015.52%20pricing%20call/screen-0000.mov",
    );
  });
});

/**
 * Story 44.1's defect and its price.
 *
 * **What jsdom can say and what it cannot.** jsdom implements no media
 * playback: `readyState` is 0 forever, no frame is ever decoded, and a canvas
 * readback of a `<video>` here would be meaningless. So nothing below proves a
 * frame appeared — that was measured in a real WKWebView, on the owner's own
 * two-track session, and the pixel counts are in the spec. What these assert
 * is the POLICY: who gets asked for a frame, who is left alone, and how often.
 * Getting that wrong is a control that moves the recording under the reader's
 * hand, which is the more dangerous half.
 */
describe("primeFirstFrame", () => {
  /** A `<video>` with a settable `readyState`, which jsdom's is not. */
  function video(readyState: number): HTMLVideoElement {
    const element = document.createElement("video");
    Object.defineProperty(element, "readyState", { configurable: true, value: readyState });
    return element;
  }

  it("buys a frame for an element that has metadata and nothing to show", () => {
    const player = video(1);
    primeFirstFrame(player);

    player.dispatchEvent(new Event("loadedmetadata"));

    expect(player.currentTime).toBe(FRAME_PRIME_SECONDS);
  });

  it("asks before the metadata arrives and not a moment sooner", () => {
    const player = video(0);
    primeFirstFrame(player);

    // A seek issued at HAVE_NOTHING is recorded as a default start position and
    // raises nothing — the element would never confirm it and the transport
    // would sit on Seeking for a request no one can answer.
    expect(player.currentTime).toBe(0);
  });

  it("leaves an element the reader or the transport already moved exactly where it is", () => {
    const player = video(1);
    primeFirstFrame(player);
    // By the time metadata lands, the pair has been placed at 37.5 s — either
    // by a scrub or by a transport seating a late-joining track.
    player.currentTime = 37.5;

    player.dispatchEvent(new Event("loadedmetadata"));

    expect(player.currentTime).toBe(37.5);
  });

  it("buys nothing for an element that already has a frame", () => {
    // HAVE_CURRENT_DATA: there is a frame, so the range request would be spent
    // for nothing on a file that may be on a pendrive.
    const player = video(2);
    primeFirstFrame(player);

    player.dispatchEvent(new Event("loadedmetadata"));

    expect(player.currentTime).toBe(0);
  });

  it("buys one frame and not one per event, even for a reader back at the top", () => {
    const player = video(1);
    primeFirstFrame(player);
    player.dispatchEvent(new Event("loadedmetadata"));
    expect(player.currentTime).toBe(FRAME_PRIME_SECONDS);

    // Back to exactly zero — a reader who scrubbed to the start. The "has it
    // been moved" guard cannot tell this apart from an untouched element, so
    // the only thing stopping a second `loadedmetadata` (a source change, a
    // reload) from moving them again is that the prime is genuinely once.
    player.currentTime = 0;
    player.dispatchEvent(new Event("loadedmetadata"));

    expect(player.currentTime).toBe(0);
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

  /** Render one of the session's files into a fresh host and hand it back. */
  async function render(name: string): Promise<HTMLElement> {
    const node = host(`${WRITTEN}/${name}`);
    await renderRecordingEmbedInto(node, SESSION, `${WRITTEN}/${name}`, {
      load: async () => TARGETS,
    });
    return node;
  }

  /**
   * Every element the widget can produce. A kind's test asserts its own element
   * is there AND that every other one is absent, because the failure this story
   * is most exposed to is a branch that falls through to the wrong medium — an
   * `<audio>` for a photo, a `<video>` for a `.zip`.
   */
  const ELEMENTS = ["video", "img", "audio", ".cm-lp-recording-chip"] as const;

  it.each([
    ["screen-0000.mov", "video"],
    ["whiteboard.png", "img"],
    ["room-tone.wav", "audio"],
    ["manifest.json", ".cm-lp-recording-chip"],
  ] as const)("renders %s as %s and as nothing else", async (name, expected) => {
    const node = await render(name);

    expect(node.querySelector(expected)).not.toBeNull();
    for (const other of ELEMENTS.filter((element) => element !== expected)) {
      expect(node.querySelector(other)).toBeNull();
    }
    // Whichever element it is, it replaced the link rather than joining it.
    expect(node.querySelector(`[${WIKILINK_ATTR}]`)).toBeNull();
  });

  it("shows an image without fetching it before it is scrolled to", async () => {
    const node = await render("whiteboard.png");

    const image = node.querySelector("img") as HTMLImageElement;
    expect(image.loading).toBe("lazy");
    // The file name, never an empty alt: an embedded photo of a whiteboard is
    // content, and content announced as decorative is content lost.
    expect(image.alt).toBe("whiteboard.png");
    expect(image.getAttribute("src")).toBe(recordingAssetUrl(SESSION, `${FOUND}/whiteboard.png`));
  });

  it("plays audio with controls and no preload, exactly as it plays video", async () => {
    const node = await render("room-tone.wav");

    const player = node.querySelector("audio") as HTMLAudioElement;
    expect(player.controls).toBe(true);
    expect(player.preload).toBe("metadata");
    expect(player.autoplay).toBe(false);
    expect(player.getAttribute("aria-label")).toBe("room-tone.wav");
  });

  it("makes an extension keeper has never seen a chip, never a broken player", async () => {
    const node = await render("board.sketchpad");

    const chip = node.querySelector(".cm-lp-recording-chip");
    expect(chip).not.toBeNull();
    expect(node.querySelector("video, audio, img")).toBeNull();
    // The name is on screen; the absolute path is not, anywhere (FR-145).
    expect(chip?.querySelector(".cm-lp-recording-chip-name")?.textContent).toBe("board.sketchpad");
    expect(node.textContent).not.toContain("/Volumes/Rec");
  });

  it("reveals and copies the absolute path from a chip's own actions", async () => {
    const node = await render("manifest.json");

    const reveal = node.querySelector<HTMLButtonElement>(
      `[aria-label="${RECORDING_EMBED_REVEAL_LABEL} manifest.json"]`,
    );
    const copy = node.querySelector<HTMLButtonElement>(
      `[aria-label="${RECORDING_EMBED_COPY_PATH_LABEL} manifest.json"]`,
    );
    expect(reveal).not.toBeNull();
    expect(copy).not.toBeNull();

    reveal?.click();
    copy?.click();

    // The absolute path, composed in Rust — the argument of an action, never
    // the note's text and never on screen.
    expect(revealed).toHaveBeenCalledWith(`/Volumes/Rec/${FOUND}/manifest.json`);
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(
      `/Volumes/Rec/${FOUND}/manifest.json`,
    );
  });

  it("offers no Reveal on a platform with no file manager, and still copies", async () => {
    capabilitiesStore
      .getState()
      .applySnapshot({ ...DEFAULT_CAPABILITIES, revealInFileManager: false });

    const node = await render("manifest.json");

    // Absent, not disabled: an affordance that cannot work is worse than none.
    expect(
      node.querySelector(`[aria-label="${RECORDING_EMBED_REVEAL_LABEL} manifest.json"]`),
    ).toBeNull();
    expect(
      node.querySelector(`[aria-label="${RECORDING_EMBED_COPY_PATH_LABEL} manifest.json"]`),
    ).not.toBeNull();
  });

  it("keeps a chip's actions clickable by claiming their events from CodeMirror", async () => {
    const widget = new RecordingEmbedWidget(
      SESSION,
      `${WRITTEN}/manifest.json`,
      `${WRITTEN}/manifest.json`,
      { load: async () => TARGETS },
    );
    const dom = widget.toDOM();
    await vi.waitFor(() => expect(dom.querySelector("button")).not.toBeNull());
    const button = dom.querySelector("button") as HTMLButtonElement;
    const name = dom.querySelector(".cm-lp-recording-chip-name") as HTMLElement;

    // A claimed event is one CodeMirror runs no handler for, so the caret stays
    // put and the line keeps its decorations — without which pressing Copy path
    // would un-render the chip instead of copying.
    expect(widget.ignoreEvent({ target: button } as unknown as Event)).toBe(true);
    // The chip's own name is not a control and behaves like the link it stands
    // for: clicking it reveals the source.
    expect(widget.ignoreEvent({ target: name } as unknown as Event)).toBe(false);
  });

  it("puts the link back when an image or an audio track cannot load", async () => {
    for (const [name, selector] of [
      ["whiteboard.png", "img"],
      ["room-tone.wav", "audio"],
    ] as const) {
      const node = await render(name);
      const element = node.querySelector(selector) as HTMLElement;

      element.dispatchEvent(new Event("error"));

      expect(node.querySelector(selector)).toBeNull();
      expect(node.querySelector(`[${WIKILINK_ATTR}]`)?.textContent).toBe(`${WRITTEN}/${name}`);
    }
  });

  it("leaves an embed of the session folder the link it was", async () => {
    const folderName = "2026-08-08 15.52 pricing call";
    const node = host(folderName);

    await renderRecordingEmbedInto(node, SESSION, folderName, { load: async () => TARGETS });

    // A directory is a target, and there is no element for a directory.
    expect(node.querySelector("video, audio, img, .cm-lp-recording-chip")).toBeNull();
    expect(node.querySelector(`[${WIKILINK_ATTR}]`)?.textContent).toBe(folderName);
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

describe("releaseRecordingMedia", () => {
  it("does nothing to a host that never got a media element", () => {
    const node = host("a/clip.mov");

    releaseRecordingMedia(node);

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
            vaultId: "vault-1",
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

  it("turns an embed of the manifest into a chip, inside the real editor", async () => {
    // Off the first line: the caret starts at 0, and a line under the caret
    // shows its source instead of its decorations (UX-DR40).
    const view = open(`intro\n\n![[${WRITTEN}/manifest.json]]\n`, SESSION);

    await settle();
    // Scoped to the widget's own host: CodeMirror pads a block widget with its
    // own `img.cm-widgetBuffer`, which is the editor's furniture and not ours.
    const embed = view.contentDOM.querySelector(".cm-lp-recording") as HTMLElement;
    // Not a player, and not a bare line of text either: an attachment keeper
    // cannot render is still an attachment.
    expect(embed.querySelector("video, audio, img")).toBeNull();
    const chip = embed.querySelector(".cm-lp-recording-chip");
    expect(chip?.querySelector(".cm-lp-recording-chip-name")?.textContent).toBe("manifest.json");
    expect(
      chip?.querySelector(`[aria-label="${RECORDING_EMBED_COPY_PATH_LABEL} manifest.json"]`),
    ).not.toBeNull();

    view.destroy();
  });

  /**
   * The impure shell, as far as it goes on this machine.
   *
   * Everything below drives REAL `<video>` elements, created by the real
   * widget, inside a real `EditorView`, through the real decoration layer — the
   * seam where the two embeds have to find each other. What jsdom will not do
   * is play: `play()` and `pause()` are stubbed because jsdom's raise "not
   * implemented", and `seeking`/`seeked`/`timeupdate` are dispatched by hand
   * because jsdom's media elements never fire them. So what is proved here is
   * the wiring and the reaction, not that two `.mov` files decode in step.
   */
  describe("two videos of one session", () => {
    /** The same session, now with a camera track beside the screen track. */
    const PAIRED = [...TARGETS, target("camera-0000.mov", "video")];

    const SCREEN = `${WRITTEN}/screen-0000.mov`;
    const CAMERA = `${WRITTEN}/camera-0000.mov`;

    // jsdom's own `play()` and `pause()` raise "not implemented", so the two
    // calls the transport makes are stubbed. Which elements were asked is the
    // assertion; what a decoder would have done with the answer is not
    // reachable here or on any machine running this suite.
    const played = vi.fn<() => Promise<void>>();

    beforeEach(() => {
      indexed.mockResolvedValue(PAIRED);
      played.mockReset();
      played.mockResolvedValue(undefined);
      vi.spyOn(HTMLMediaElement.prototype, "play").mockImplementation(played);
      vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
    });

    afterEach(() => {
      vi.restoreAllMocks();
    });

    /** Both embeds, off the first line so neither sits under the caret. */
    function openPair(): EditorView {
      return open(`intro\n\n![[${SCREEN}]]\n\n![[${CAMERA}]]\n`, SESSION);
    }

    function videosOf(view: EditorView): HTMLVideoElement[] {
      return Array.from(view.contentDOM.querySelectorAll("video"));
    }

    /** jsdom raises no `seeked` of its own, so the platform's confirmation is
     *  supplied here — without it the transport correctly stays "seeking". */
    function confirmSeeks(view: EditorView): void {
      for (const video of videosOf(view)) {
        video.dispatchEvent(new Event("seeked"));
      }
    }

    /**
     * Report a duration from real `<video>` elements.
     *
     * jsdom decodes nothing, so `duration` is `NaN` forever and the transport
     * correctly disables a scrub bar for a pair with no span. The span has to
     * come from somewhere for the scrub to be exercised at all, and this is the
     * smallest lie that lets the real elements be driven.
     */
    function measure(view: EditorView, seconds: number): void {
      for (const video of videosOf(view)) {
        Object.defineProperty(video, "duration", { configurable: true, value: seconds });
        video.dispatchEvent(new Event("durationchange"));
      }
    }

    it("puts both players under one transport and takes their own controls away", async () => {
      const view = openPair();
      await settle();

      const videos = videosOf(view);
      expect(videos).toHaveLength(2);
      // A native transport carries its own scrub bar, and a second scrub bar is
      // a second clock.
      expect(videos.map((video) => video.controls)).toEqual([false, false]);
      expect(view.contentDOM.querySelectorAll(".cm-lp-recording-transport")).toHaveLength(1);
      // Under the first embed in the note, and inside that widget's own host.
      const bar = view.contentDOM.querySelector(".cm-lp-recording-transport");
      expect(bar?.parentElement?.contains(videos[0])).toBe(true);
      // Volume and mute stayed per track: two mixers, one beside each video.
      expect(view.contentDOM.querySelectorAll(".cm-lp-recording-mix")).toHaveLength(2);

      view.destroy();
    });

    it("plays, scrubs and skips both real players from the one bar", async () => {
      const view = openPair();
      await settle();
      const videos = videosOf(view);
      measure(view, 120);
      const bar = view.contentDOM.querySelector(".cm-lp-recording-transport") as HTMLElement;

      bar.querySelector<HTMLButtonElement>(`[aria-label="${PLAY_LABEL}"]`)?.click();
      await settle();

      // `play()` on BOTH — the promise `allSettled` waits on is per element.
      expect(played).toHaveBeenCalledTimes(2);

      const scrub = bar.querySelector<HTMLInputElement>(`[aria-label="${SCRUB_LABEL}"]`);
      if (scrub === null) {
        throw new Error("the transport has no scrub bar");
      }
      scrub.value = "40";
      scrub.dispatchEvent(new Event("input"));

      expect(videos.map((video) => video.currentTime)).toEqual([40, 40]);

      confirmSeeks(view);
      bar.querySelector<HTMLButtonElement>(`[aria-label="${BACK_LABEL}"]`)?.click();

      expect(videos.map((video) => video.currentTime)).toEqual([
        40 - SKIP_SECONDS,
        40 - SKIP_SECONDS,
      ]);

      view.destroy();
    });

    it("mutes one real player and leaves the other audible", async () => {
      const view = openPair();
      await settle();
      const videos = videosOf(view);

      view.contentDOM
        .querySelector<HTMLButtonElement>(`[aria-label="${MUTE_LABEL} camera-0000.mov"]`)
        ?.click();

      expect(videos[1].muted).toBe(true);
      expect(videos[0].muted).toBe(false);

      view.destroy();
    });

    it("turns one real player down and leaves the other where it was", async () => {
      const view = openPair();
      await settle();
      const videos = videosOf(view);
      const volume = view.contentDOM.querySelector<HTMLInputElement>(
        `[aria-label="${VOLUME_LABEL} screen-0000.mov"]`,
      );
      if (volume === null) {
        throw new Error("the screen track has no volume control");
      }

      volume.value = "0.25";
      volume.dispatchEvent(new Event("input"));

      // Turning the screen recording down under the camera is a MIXING
      // decision, and the one thing a two-view recording actually needs. It is
      // the only control the transport deliberately did not centralise.
      expect(videos[0].volume).toBe(0.25);
      expect(videos[1].volume).toBe(1);

      view.destroy();
    });

    it("claims the transport's slider events, so a drag cannot un-render the slider", async () => {
      const view = openPair();
      await settle();
      const scrub = view.contentDOM.querySelector(`input[aria-label="${SCRUB_LABEL}"]`);
      const widget = new RecordingEmbedWidget(SESSION, SCREEN, SCREEN);

      // A claimed event is one CodeMirror runs no handler for. Letting a
      // `mousedown` on the scrub bar through would reveal the line, a revealed
      // line drops its decorations, and the reader's drag would destroy the
      // control mid-gesture.
      expect(widget.ignoreEvent({ target: scrub } as unknown as Event)).toBe(true);

      view.destroy();
    });

    it("corrects a real player that drifted past the threshold", async () => {
      const view = openPair();
      await settle();
      const videos = videosOf(view);

      // Two real `<video>` elements whose clocks disagree. Assigning
      // `currentTime` is all the drift there is: neither element announces it,
      // which is exactly why nothing but the transport can notice.
      videos[1].currentTime = 12 + MAX_DRIFT_SECONDS + 0.5;
      videos[0].currentTime = 12;
      // A real webview would have confirmed those two assignments with
      // `seeked`; jsdom raises nothing, so the confirmation is supplied here.
      // Without it the transport would correctly refuse to trust either clock.
      confirmSeeks(view);
      videos[0].dispatchEvent(new Event("timeupdate"));

      expect(videos[0].currentTime).toBe(12);
      expect(videos[1].currentTime).toBe(12);

      view.destroy();
    });

    it("gives the remaining player its own controls back when an embed is deleted", async () => {
      const view = openPair();
      await settle();
      expect(videosOf(view)).toHaveLength(2);

      // The author deletes the camera embed. The pair is a single video again,
      // and a single video is the platform's job.
      const line = view.state.doc.line(5);
      view.dispatch({ changes: { from: line.from, to: line.to, insert: "" } });
      await settle();

      const videos = videosOf(view);
      expect(videos).toHaveLength(1);
      expect(videos[0].controls).toBe(true);
      expect(view.contentDOM.querySelector(".cm-lp-recording-transport")).toBeNull();
      expect(view.contentDOM.querySelector(".cm-lp-recording-mix")).toBeNull();
      // And the survivor is back in its OWN host, not left inside the furniture
      // the group built around it: the note reads exactly as a one-video note.
      expect(view.contentDOM.querySelector(".cm-lp-recording-stage")).toBeNull();
      expect(view.contentDOM.querySelector(".cm-lp-recording-track")).toBeNull();
      expect(videos[0].parentElement?.className).toBe("cm-lp-recording");

      view.destroy();
    });

    /**
     * Story 44.1. Grouping moves each `<video>` out of its own widget host and
     * into the leader's stage, which is the only way two embeds on two
     * CodeMirror lines can be rendered side by side — and it puts a hole
     * straight through the teardown path, because `releaseRecordingMedia`
     * searches the host for the element it must release. A follower's host is
     * empty. Nothing here is hypothetical: without the host-keyed release these
     * two fail with the follower's video still mounted and still holding its
     * file.
     */
    it("stages the pair as one player, each track boxed with its own mixer", async () => {
      const view = openPair();
      await settle();

      const stage = view.contentDOM.querySelector(".cm-lp-recording-stage");
      expect(stage).not.toBeNull();
      // One stage for the pair, not one per embed.
      expect(view.contentDOM.querySelectorAll(".cm-lp-recording-stage")).toHaveLength(1);

      const boxes = Array.from(
        view.contentDOM.querySelectorAll<HTMLElement>(".cm-lp-recording-track"),
      );
      expect(boxes).toHaveLength(2);
      const videos = videosOf(view);
      for (const [index, box] of boxes.entries()) {
        // The video and the control that governs it are inside one boundary —
        // the whole of the "the mute slider sits away from its track" report.
        expect(box.contains(videos[index])).toBe(true);
        expect(box.querySelectorAll(".cm-lp-recording-mix")).toHaveLength(1);
      }
      // Both tracks, including the follower's, are inside the leading embed's
      // own host — that is what "reads as one player" means in the DOM.
      const leader = view.contentDOM.querySelectorAll(".cm-lp-recording")[0];
      expect(leader.contains(videos[0])).toBe(true);
      expect(leader.contains(videos[1])).toBe(true);

      view.destroy();
    });

    it("releases a follower whose element the stage is holding, not just one in its own host", async () => {
      const view = openPair();
      await settle();
      const camera = videosOf(view)[1];
      // Proof the hole is real and not merely guarded against: the follower's
      // element is genuinely not in the host the widget will be torn down with.
      const followerHost = view.contentDOM.querySelectorAll(".cm-lp-recording")[1];
      expect(followerHost.contains(camera)).toBe(false);

      const line = view.state.doc.line(5);
      view.dispatch({ changes: { from: line.from, to: line.to, insert: "" } });
      await settle();

      // Released means gone from the document AND told to let go of the file:
      // a `<video>` still holding a `src` keeps a decoder and an open range
      // pipeline against a volume the user may be trying to eject.
      expect(videosOf(view)).toHaveLength(1);
      expect(camera.isConnected).toBe(false);
      expect(camera.getAttribute("src")).toBeNull();

      view.destroy();
    });

    it("puts a failed follower back to a link with no dead player under it", async () => {
      const view = openPair();
      await settle();
      const videos = videosOf(view);
      const followerHost = view.contentDOM.querySelectorAll<HTMLElement>(".cm-lp-recording")[1];

      // The camera's file went away between the resolve and the request — a
      // retitle, or an unmounted volume.
      videos[1].dispatchEvent(new Event("error"));
      await settle();

      // The host shows the link the note actually says, and nothing else. The
      // ordering this defends is exact: the track has to LEAVE the group before
      // the host is restored, because leaving hands the element back to this
      // host and a restore that ran first would be undone by it — a dead
      // `<video>` sitting under the link.
      expect(followerHost.querySelector("video")).toBeNull();
      expect(followerHost.querySelector(".cm-lp-wikilink")).not.toBeNull();
      // And the survivor is a lone video again.
      expect(videos[0].controls).toBe(true);
      expect(view.contentDOM.querySelector(".cm-lp-recording-stage")).toBeNull();

      view.destroy();
    });

    it("hands the whole group down when the LEADING embed is deleted", async () => {
      const view = openPair();
      await settle();
      measure(view, 120);
      const survivor = videosOf(view)[1];

      // The author deletes the screen embed. A third track would still make
      // this a group, so here the pair simply falls to one — but the survivor
      // must come back to its own host rather than leaving with the stage.
      const line = view.state.doc.line(3);
      view.dispatch({ changes: { from: line.from, to: line.to, insert: "" } });
      await settle();

      const videos = videosOf(view);
      expect(videos).toHaveLength(1);
      expect(videos[0]).toBe(survivor);
      expect(survivor.controls).toBe(true);
      expect(survivor.isConnected).toBe(true);
      expect(survivor.parentElement?.className).toBe("cm-lp-recording");
      expect(view.contentDOM.querySelector(".cm-lp-recording-stage")).toBeNull();

      view.destroy();
    });

    it("primes both real players for a frame once the metadata lands", async () => {
      const view = openPair();
      await settle();
      const videos = videosOf(view);

      // jsdom raises no media events at all, so the one a real webview raises
      // when it has a duration and no frame is supplied here. What is being
      // asserted is that the real widget wired the prime to the real elements
      // — the pixels it buys were counted in a real WKWebView, not here.
      for (const video of videos) {
        video.dispatchEvent(new Event("loadedmetadata"));
      }

      expect(videos.map((video) => video.currentTime)).toEqual([
        FRAME_PRIME_SECONDS,
        FRAME_PRIME_SECONDS,
      ]);

      view.destroy();
    });
  });
});
