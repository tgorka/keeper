import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterAll, afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import type * as IpcClient from "@/lib/ipc/client";
import type { RecordingNoteTargetVm } from "@/lib/ipc/client";
import { capabilitiesStore, DEFAULT_CAPABILITIES } from "@/lib/stores/capabilities";
import { withRangeRects } from "@/test/layout";
import { livePreview } from "./live-preview";
import {
  MEDIA_CHIP_COPY_PATH_LABEL,
  MEDIA_CHIP_REVEAL_LABEL,
  PLAY_IN_PLAYER_LABEL,
} from "./media-chip";
import { recordingAssetUrl } from "./media-playback";
import {
  RecordingEmbedWidget,
  type RecordingTargetLoader,
  renderRecordingEmbedInto,
} from "./recording-embed";
import { WIKILINK_ATTR } from "./wikilink";

/** What the renderer's own widget — the one it constructs, with no seam for a
 *  test to inject through — reaches for. */
const indexed = vi.fn<typeof IpcClient.recordingNoteTargets>();
const revealed = vi.fn<typeof IpcClient.revealPath>();
const forEmbed = vi.fn<typeof IpcClient.mediaBlockForEmbed>();

vi.mock("@/lib/ipc/client", async (importOriginal) => ({
  ...(await importOriginal<typeof IpcClient>()),
  recordingNoteTargets: (sessionId: string) => indexed(sessionId),
  revealPath: (path: string) => revealed(path),
  mediaBlockForEmbed: (profileId: string, body: string, line: number, target: string) =>
    forEmbed(profileId, body, line, target),
}));

const SESSION = "01KYH5DXGP1XQRHTME8CJFVEJ6-01KZHS7EJB5QKR8T9CHXQ46RNS";

/** The folder as the note wrote it — before the Story 40.4 retitle below. */
const WRITTEN = "recordings/2026/2026-08-08 15.52 test";

/** Where the index says the session is NOW: same files, renamed folder. */
const FOUND = "recordings/2026/2026-08-08 15.52 pricing call";

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
  target("camera-0000.mov", "video"),
  target("whiteboard.png", "image"),
  target("room-tone.wav", "audio"),
  target("manifest.json", "file"),
];

/** A host holding the link the widget renders before it resolves anything. */
function host(embedded: string): HTMLElement {
  const node = document.createElement("span");
  node.className = "cm-lp-recording";
  const anchor = document.createElement("span");
  anchor.className = "cm-lp-wikilink";
  anchor.setAttribute(WIKILINK_ATTR, embedded);
  anchor.textContent = embedded;
  node.append(anchor);
  return node;
}

beforeEach(() => {
  revealed.mockReset();
  revealed.mockResolvedValue(undefined);
  forEmbed.mockReset();
  Object.assign(navigator, { clipboard: { writeText: vi.fn(() => Promise.resolve()) } });
  capabilitiesStore
    .getState()
    .applySnapshot({ ...DEFAULT_CAPABILITIES, revealInFileManager: true });
});

afterEach(() => {
  capabilitiesStore.getState().applySnapshot(DEFAULT_CAPABILITIES);
});

describe("renderRecordingEmbedInto", () => {
  async function render(name: string): Promise<HTMLElement> {
    const node = host(`${WRITTEN}/${name}`);
    await renderRecordingEmbedInto(node, SESSION, `${WRITTEN}/${name}`, {
      load: async () => TARGETS,
    });
    return node;
  }

  it.each([
    ["screen-0000.mov", ".cm-lp-media-chip"],
    ["room-tone.wav", ".cm-lp-media-chip"],
    ["whiteboard.png", "img"],
    ["manifest.json", ".cm-lp-recording-chip"],
  ] as const)("renders %s as %s, and never as a player", async (name, expected) => {
    const node = await render(name);

    expect(node.querySelector(expected)).not.toBeNull();
    // The note mounts no media element for an embed: the media block is where
    // a recording plays.
    expect(node.querySelector("video, audio")).toBeNull();
    expect(node.querySelector(`[${WIKILINK_ATTR}]`)).toBeNull();
  });

  it("names a video by its file, with its kind's icon, and fetches nothing", async () => {
    const node = await render("screen-0000.mov");

    const chip = node.querySelector(".cm-lp-media-chip") as HTMLElement;
    expect(chip.querySelector(".cm-lp-media-chip-name")?.textContent).toBe("screen-0000.mov");
    // The tooltip is the index's relative path, never the absolute one (FR-145).
    expect(chip.querySelector(".cm-lp-media-chip-name")?.getAttribute("title")).toBe(
      `${FOUND}/screen-0000.mov`,
    );
    expect(chip.querySelector("svg.cm-lp-media-chip-icon")).not.toBeNull();
    expect(node.textContent).not.toContain("/Volumes/Rec");
    expect(node.querySelector("[src]")).toBeNull();
  });

  it("shows an image without fetching it before it is scrolled to", async () => {
    const node = await render("whiteboard.png");

    const image = node.querySelector("img") as HTMLImageElement;
    expect(image.loading).toBe("lazy");
    expect(image.alt).toBe("whiteboard.png");
    expect(image.getAttribute("src")).toBe(recordingAssetUrl(SESSION, `${FOUND}/whiteboard.png`));
  });

  it.each([
    "screen-0000.mov",
    "manifest.json",
  ])("reveals and copies the absolute path from %s's chip", async (name) => {
    const node = await render(name);

    node
      .querySelector<HTMLButtonElement>(`[aria-label="${MEDIA_CHIP_REVEAL_LABEL} ${name}"]`)
      ?.click();
    node
      .querySelector<HTMLButtonElement>(`[aria-label="${MEDIA_CHIP_COPY_PATH_LABEL} ${name}"]`)
      ?.click();

    expect(revealed).toHaveBeenCalledWith(`/Volumes/Rec/${FOUND}/${name}`);
    expect(navigator.clipboard.writeText).toHaveBeenCalledWith(`/Volumes/Rec/${FOUND}/${name}`);
  });

  it("offers no Reveal on a platform with no file manager, and still copies", async () => {
    capabilitiesStore
      .getState()
      .applySnapshot({ ...DEFAULT_CAPABILITIES, revealInFileManager: false });

    const node = await render("screen-0000.mov");

    expect(node.querySelector(`[aria-label^="${MEDIA_CHIP_REVEAL_LABEL}"]`)).toBeNull();
    expect(node.querySelector(`[aria-label^="${MEDIA_CHIP_COPY_PATH_LABEL}"]`)).not.toBeNull();
  });

  it("offers Play in a player only where it can write the block", async () => {
    // No view to splice into — a data-file panel asking the session first.
    const node = await render("screen-0000.mov");

    expect(node.querySelector(`[aria-label^="${PLAY_IN_PLAYER_LABEL}"]`)).toBeNull();
  });

  it("puts the link back when an image cannot load", async () => {
    const node = await render("whiteboard.png");

    node.querySelector("img")?.dispatchEvent(new Event("error"));

    expect(node.querySelector("img")).toBeNull();
    expect(node.querySelector(`[${WIKILINK_ATTR}]`)?.textContent).toBe(`${WRITTEN}/whiteboard.png`);
  });

  it("leaves an embed of the session folder the link it was", async () => {
    const folderName = "2026-08-08 15.52 pricing call";
    const node = host(folderName);

    await expect(
      renderRecordingEmbedInto(node, SESSION, folderName, { load: async () => TARGETS }),
    ).resolves.toBe(false);

    expect(node.querySelector(`[${WIKILINK_ATTR}]`)?.textContent).toBe(folderName);
  });

  const cases: [string, RecordingTargetLoader, string][] = [
    ["names no such file", async () => TARGETS, `${WRITTEN}/other.mov`],
    ["cannot place the session", async () => null, `${WRITTEN}/screen-0000.mov`],
    [
      "cannot be asked at all",
      () => Promise.reject(new Error("the archive is locked")),
      `${WRITTEN}/screen-0000.mov`,
    ],
  ];
  it.each(
    cases,
  )("degrades to the link when the index %s, and never throws", async (_, load, embedded) => {
    const node = host(embedded);

    await expect(renderRecordingEmbedInto(node, SESSION, embedded, { load })).resolves.toBe(false);

    expect(node.querySelector(".cm-lp-media-chip")).toBeNull();
    expect(node.querySelector(`[${WIKILINK_ATTR}]`)?.textContent).toBe(embedded);
  });

  it("draws nothing into a host that was torn down while it resolved", async () => {
    const node = host(`${WRITTEN}/screen-0000.mov`);

    await renderRecordingEmbedInto(node, SESSION, `${WRITTEN}/screen-0000.mov`, {
      load: async () => TARGETS,
      cancelled: () => true,
    });

    expect(node.querySelector(".cm-lp-media-chip")).toBeNull();
  });
});

describe("RecordingEmbedWidget", () => {
  it("is the same widget for the same embed in the same drive", () => {
    const one = new RecordingEmbedWidget(SESSION, "a/clip.mov", "a/clip.mov", "v1");

    expect(one.eq(new RecordingEmbedWidget(SESSION, "a/clip.mov", "a/clip.mov", "v1"))).toBe(true);
    expect(one.eq(new RecordingEmbedWidget(SESSION, "a/other.mov", "a/other.mov", "v1"))).toBe(
      false,
    );
    expect(one.eq(new RecordingEmbedWidget("01OTHER", "a/clip.mov", "a/clip.mov", "v1"))).toBe(
      false,
    );
    expect(one.eq(new RecordingEmbedWidget(SESSION, "a/clip.mov", "a/clip.mov", "v2"))).toBe(false);
  });
});

let restoreRects: (() => void) | null = null;

beforeAll(() => {
  restoreRects = withRangeRects();
});

afterAll(() => {
  restoreRects?.();
});

/** Through the real decoration layer: where "this line holds an `![[…]]`" and
 *  "this note is a recording note" meet. */
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
            // The block the edit writes is drawn by its own layer; its panel is
            // not what these tests are about.
            mountMedia: () => ({ unmount: () => {}, update: () => {} }),
          }),
        ],
      }),
    });
  }

  /** Drain the microtasks the resolve rides on, and no frame: a measure pass
   *  over jsdom's zero-height layout would replace the lines with a gap. */
  async function settle(): Promise<void> {
    for (let tick = 0; tick < 6; tick += 1) {
      await Promise.resolve();
    }
  }

  it("turns an embed of the session's video into a chip with Play in a player", async () => {
    const view = open(`intro\n\n![[${WRITTEN}/screen-0000.mov]]\n\nafter\n`, SESSION);

    await settle();
    expect(indexed).toHaveBeenCalledWith(SESSION);
    expect(view.contentDOM.querySelector("video, audio")).toBeNull();
    expect(
      view.contentDOM.querySelector(`[aria-label="${PLAY_IN_PLAYER_LABEL} screen-0000.mov"]`),
    ).not.toBeNull();

    view.destroy();
  });

  it("replaces the session's embeds with the block Rust composed, as one undoable edit", async () => {
    const doc = `# Pricing\n\n![[${WRITTEN}/screen-0000.mov]]\n![[${WRITTEN}/camera-0000.mov]]\n\nafter\n`;
    // Rust's answer for line 3: the block over the first embed, the second
    // embed's line deleted.
    forEmbed.mockResolvedValue([
      { firstLine: 3, lastLine: 3, text: `\`\`\`keeper-media\nsession = "${SESSION}"\n\`\`\`` },
      { firstLine: 4, lastLine: 4, text: null },
    ]);
    const view = open(doc, SESSION);
    await settle();

    view.contentDOM
      .querySelector<HTMLButtonElement>(`[aria-label="${PLAY_IN_PLAYER_LABEL} screen-0000.mov"]`)
      ?.click();
    await settle();

    // The note's whole text, the embed's own line and the target as written:
    // Rust decides what collapses (AD-65).
    expect(forEmbed).toHaveBeenCalledWith("vault-1", doc, 3, `${WRITTEN}/screen-0000.mov`);
    expect(view.state.doc.toString()).toBe(
      `# Pricing\n\n\`\`\`keeper-media\nsession = "${SESSION}"\n\`\`\`\n\nafter\n`,
    );

    view.destroy();
  });

  it("asks again when the note changed while Rust was answering", async () => {
    const doc = `intro\n\n![[${WRITTEN}/room-tone.wav]]\n`;
    const block = `\`\`\`keeper-media\n[[part]]\nfile = "x.wav"\n\`\`\``;
    const view = open(doc, SESSION);
    await settle();
    // The first answer arrives after a line was typed above the embed: its line
    // number is stale, and applying it would overwrite the typed line.
    forEmbed.mockImplementationOnce(async () => {
      view.dispatch({ changes: { from: 0, insert: "typed\n" } });
      return [{ firstLine: 3, lastLine: 3, text: block }];
    });
    forEmbed.mockImplementationOnce(async () => [{ firstLine: 4, lastLine: 4, text: block }]);

    view.contentDOM
      .querySelector<HTMLButtonElement>(`[aria-label="${PLAY_IN_PLAYER_LABEL} room-tone.wav"]`)
      ?.click();
    await settle();
    await settle();

    expect(forEmbed).toHaveBeenCalledTimes(2);
    expect(forEmbed.mock.calls[1]?.[2]).toBe(4);
    expect(view.state.doc.toString()).toBe(`typed\nintro\n\n${block}\n`);

    view.destroy();
  });

  it("says Rust's refusal on the chip and writes nothing", async () => {
    const doc = `intro\n\n![[${WRITTEN}/room-tone.wav]]\n`;
    forEmbed.mockRejectedValue({
      code: "notesInvalid",
      message: "That file is not in this drive.",
    });
    const view = open(doc, SESSION);
    await settle();

    view.contentDOM
      .querySelector<HTMLButtonElement>(`[aria-label="${PLAY_IN_PLAYER_LABEL} room-tone.wav"]`)
      ?.click();
    await settle();

    expect(view.state.doc.toString()).toBe(doc);
    expect(view.contentDOM.querySelector(".cm-lp-media-chip-status")?.textContent).toBe(
      "That file is not in this drive.",
    );

    view.destroy();
  });

  it("never asks the recordings index about a note that is not about a recording", async () => {
    const view = open(`intro\n\n![[${WRITTEN}/screen-0000.mov]]\n\nafter\n`, null);

    await settle();

    expect(indexed).not.toHaveBeenCalled();
    expect(view.contentDOM.querySelector(".cm-lp-recording")).toBeNull();

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
});
