/**
 * Story 55.4 — the vault's half of `![[…]]`, and every way it declines.
 *
 * The degrade paths are the interesting ones: a note is the durable record, and
 * a broken player where a working link used to be is a worse outcome than the
 * feature not existing. So each of them is here by name.
 */
import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { NoteEmbedPathVm } from "@/lib/ipc/client";

const notesEmbedPaths = vi.fn<(v: string, t: string[]) => Promise<(NoteEmbedPathVm | null)[]>>();
vi.mock("@/lib/ipc/client", () => ({
  notesEmbedPaths: (v: string, t: string[]) => notesEmbedPaths(v, t),
  notesEmbedRead: vi.fn(),
  notesEmbedWrite: vi.fn(),
  notesCsvSetCell: vi.fn(),
  recordingNoteTargets: vi.fn(async () => null),
  revealPath: vi.fn(),
  notesGallery: vi.fn(async () => ({ entries: [] })),
}));

import { livePreview } from "./live-preview";
import { drawableFor, renderVaultEmbedInto, VAULT_EMBED_CLASS } from "./vault-embed";

const resolved = (relPath: string, kind: NoteEmbedPathVm["kind"]): NoteEmbedPathVm => ({
  relPath,
  kind,
});

/** A host in the state `toDOM` leaves it: the plain link. */
function host(target: string): HTMLElement {
  const node = document.createElement("span");
  node.className = VAULT_EMBED_CLASS;
  const anchor = document.createElement("a");
  anchor.className = "cm-lp-wikilink";
  anchor.textContent = target;
  node.append(anchor);
  return node;
}

const assetUrl = (relPath: string) => `keeper-note://v1/${relPath}`;

describe("what a resolved file is drawn as", () => {
  it("takes Rust's word for the three media kinds", () => {
    expect(drawableFor(resolved("a.png", "image"))).toBe("image");
    expect(drawableFor(resolved("a.mov", "video"))).toBe("video");
    expect(drawableFor(resolved("a.m4a", "audio"))).toBe("audio");
  });

  it("refines a PDF inside the kind `file`, which is the registry's job", () => {
    // The one classification the frontend is allowed to make, and only within
    // the catch-all kind: `kind_for_file_name` calls a `.pdf` a `file`, and
    // FILE_FORMATS is what knows a PDF from a `.zip`.
    expect(drawableFor(resolved("report.pdf", "file"))).toBe("pdf");
    expect(drawableFor(resolved("REPORT.PDF", "file"))).toBe("pdf");
  });

  it("draws nothing it has no element for", () => {
    for (const [name, kind] of [
      ["archive.zip", "file"],
      ["sheet.xlsx", "file"],
      ["people.csv", "file"],
      ["photos", "folder"],
    ] as const) {
      expect(drawableFor(resolved(name, kind))).toBeNull();
    }
  });
});

describe("rendering into a note", () => {
  it("replaces the link with the file's element", async () => {
    const node = host("holiday.png");

    await renderVaultEmbedInto(node, "v1", "holiday.png", {
      // The bare name resolved inside `attachments/`, which is Rust's answer
      // and not something this module composed.
      resolve: async () => [resolved("attachments/holiday.png", "image")],
      assetUrl,
    });

    const image = node.querySelector("img");
    expect(image?.getAttribute("src")).toBe("keeper-note://v1/attachments/holiday.png");
    expect(image?.alt).toBe("holiday.png");
    expect(node.querySelector(".cm-lp-wikilink")).toBeNull();
  });

  it("leaves the link alone when the vault does not hold the file", async () => {
    const node = host("gone.png");

    await renderVaultEmbedInto(node, "v1", "gone.png", {
      resolve: async () => [null],
      assetUrl,
    });

    expect(node.querySelector(".cm-lp-wikilink")?.textContent).toBe("gone.png");
  });

  it("leaves the link alone when the resolver rejects", async () => {
    const node = host("holiday.png");

    // An unmounted volume, a vault that has gone away, a command that is not
    // there. None of them is this decoration's to report.
    await expect(
      renderVaultEmbedInto(node, "v1", "holiday.png", {
        resolve: async () => {
          throw new Error("volume not mounted");
        },
        assetUrl,
      }),
    ).resolves.toBeUndefined();

    expect(node.querySelector(".cm-lp-wikilink")?.textContent).toBe("holiday.png");
  });

  it("leaves the link alone when the command itself cannot be reached", async () => {
    const node = host("holiday.png");
    // Not the same failure as a rejected call: this one throws at the property
    // access, before a promise exists — and a throw out of `toDOM` is an
    // unhandled rejection that no `.catch()` on the caller can see. Found by
    // the pre-push hook, which reports vitest's error count and not only its
    // pass count.
    const options = {
      assetUrl,
      get resolve(): never {
        throw new Error("no such export");
      },
    };

    await expect(renderVaultEmbedInto(node, "v1", "holiday.png", options)).resolves.toBeUndefined();

    expect(node.querySelector(".cm-lp-wikilink")?.textContent).toBe("holiday.png");
  });

  it("leaves the link alone for a file it has no element for", async () => {
    const node = host("archive.zip");

    await renderVaultEmbedInto(node, "v1", "archive.zip", {
      resolve: async () => [resolved("archive.zip", "file")],
      assetUrl,
    });

    expect(node.querySelector(".cm-lp-wikilink")?.textContent).toBe("archive.zip");
  });

  it("puts the link back when the element fails to load", async () => {
    const node = host("holiday.png");

    await renderVaultEmbedInto(node, "v1", "holiday.png", {
      resolve: async () => [resolved("holiday.png", "image")],
      assetUrl,
    });
    node.querySelector("img")?.dispatchEvent(new Event("error"));

    // A dead player states that the file is broken. Usually the file is fine.
    expect(node.querySelector("img")).toBeNull();
    expect(node.querySelector(".cm-lp-wikilink")?.textContent).toBe("holiday.png");
  });

  it("touches nothing after the widget has been destroyed", async () => {
    const node = host("holiday.png");

    await renderVaultEmbedInto(node, "v1", "holiday.png", {
      resolve: async () => [resolved("holiday.png", "image")],
      assetUrl,
      cancelled: () => true,
    });

    // A render in flight when CodeMirror throws the host away must not write
    // into a detached node.
    expect(node.querySelector("img")).toBeNull();
    expect(node.querySelector(".cm-lp-wikilink")).not.toBeNull();
  });

  it("asks about one target and reads the answer by position", async () => {
    const resolveSpy = vi.fn(async () => [resolved("a.png", "image")]);
    await renderVaultEmbedInto(host("a.png"), "v1", "a.png", { resolve: resolveSpy, assetUrl });

    expect(resolveSpy).toHaveBeenCalledWith("v1", ["a.png"]);
  });
});

describe("in the renderer a note is actually drawn by", () => {
  beforeEach(() => {
    vi.clearAllMocks();
  });

  /** The decoration layer as `note-editor.tsx` builds it, caret parked off the
   *  embed so the line is rendered rather than revealed. */
  function open(doc: string, sessionId: string | null): EditorView {
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc,
        extensions: [
          markdown({ base: markdownLanguage }),
          livePreview({
            vaultId: "v1",
            assetUrl: (rel) => `keeper-note://v1/${rel}`,
            onOpenLink: () => {},
            recordingSession: () => sessionId,
          }),
        ],
      }),
    });
    view.dispatch({ selection: { anchor: view.state.doc.length } });
    return view;
  }

  /** Drain the widget's fired-and-forgotten render. */
  const settle = async (): Promise<void> => {
    for (let tick = 0; tick < 8; tick += 1) {
      await Promise.resolve();
    }
  };

  it("shows a photograph an ordinary note embeds", async () => {
    // The claim this story exists for, made against the real renderer: a test
    // that mounted the widget itself could not prove `live-preview.ts` mounts
    // it, and until now it did not.
    notesEmbedPaths.mockResolvedValue([resolved("attachments/holiday.png", "image")]);
    const view = open("intro\n\n![[holiday.png]]\n\nafter\n", null);

    await settle();

    // Inside the host, not `contentDOM.querySelector("img")`: CodeMirror puts
    // its own aria-hidden `cm-widgetBuffer` image either side of a widget, and
    // the first `img` in the line is one of those.
    const image = view.contentDOM
      .querySelector(`.${VAULT_EMBED_CLASS}`)
      ?.querySelector("img.cm-lp-recording-image");
    expect(image?.getAttribute("src")).toBe("keeper-note://v1/attachments/holiday.png");
    expect(notesEmbedPaths).toHaveBeenCalledWith("v1", ["holiday.png"]);

    view.destroy();
  });

  it("leaves a data file to Story 45.12's editable panel", async () => {
    notesEmbedPaths.mockResolvedValue([resolved("people.csv", "file")]);
    const view = open("intro\n\n![[people.csv]]\n\nafter\n", null);

    await settle();

    // The vault embed must not have been asked at all: a `.csv` is a panel you
    // can type into, and drawing it as anything else would take that away.
    expect(view.contentDOM.querySelector(`.${VAULT_EMBED_CLASS}`)).toBeNull();
    expect(notesEmbedPaths).not.toHaveBeenCalled();

    view.destroy();
  });

  it("leaves an ordinary link a link, `!` being the whole of the difference", async () => {
    const view = open("intro\n\n[[holiday.png]]\n\nafter\n", null);

    await settle();

    expect(view.contentDOM.querySelector(`.${VAULT_EMBED_CLASS}`)).toBeNull();
    expect(notesEmbedPaths).not.toHaveBeenCalled();

    view.destroy();
  });
});
