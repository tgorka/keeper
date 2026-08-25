import { markdown, markdownLanguage } from "@codemirror/lang-markdown";
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { afterEach, describe, expect, it, vi } from "vitest";
import { WINDOW_ROW_ATTR } from "@/components/ui/window-list";
import type { NoteGalleryItemVm, NoteGalleryVm } from "@/lib/ipc/client";
import { withListGeometry } from "@/test/layout";
import {
  GALLERY_PIN_LABEL,
  GALLERY_UNPIN_LABEL,
  galleryOrder,
  galleryRangeAt,
  gallerySummary,
  parseGalleryBlock,
  withoutPin,
  withPin,
} from "./gallery-block";
import { livePreview } from "./live-preview";
import { SLASH_COMMANDS } from "./slash-menu";
import { WIKILINK_ATTR } from "./wikilink";

const FOLDER = "Photos/Trip";

/** One entry as Rust composes it: the kind from the one classifier, and a URL
 *  for exactly the kinds `keeper-note://` will serve. */
function item(name: string, kind: NoteGalleryItemVm["kind"]): NoteGalleryItemVm {
  const relPath = `${FOLDER}/${name}`;
  return {
    name,
    relPath,
    // The vault frame, null only above the vault root (owner item 10). A
    // gallery block lists the vault, so every entry it can show is inside it.
    vaultRelPath: relPath,
    kind,
    url:
      kind === "video" || kind === "image" || kind === "audio"
        ? `keeper-note://vault-1/${relPath.split("/").map(encodeURIComponent).join("/")}`
        : null,
  };
}

/** A folder holding one of each kind, including two nothing renders. */
const MIXED: NoteGalleryItemVm[] = [
  item("a-clip.mov", "video"),
  item("b-hero.jpg", "image"),
  item("c-tone.wav", "audio"),
  item("manifest.json", "file"),
  item("board.sketchpad", "file"),
];

function listing(items: NoteGalleryItemVm[], over: Partial<NoteGalleryVm> = {}): NoteGalleryVm {
  return { folder: FOLDER, items, truncated: false, problem: null, ...over };
}

// --- The syntax, and what Obsidian is left with -----------------------------

describe("the gallery block's syntax", () => {
  it("reads the folder from the callout title and the pins from its links", () => {
    const block = parseGalleryBlock(
      `> [!gallery] ${FOLDER}\n> [[${FOLDER}/b-hero.jpg]]\n> [[${FOLDER}/a-clip.mov]]`,
    );

    expect(block).toEqual({
      folder: FOLDER,
      pins: [`${FOLDER}/b-hero.jpg`, `${FOLDER}/a-clip.mov`],
    });
  });

  it("is a gallery whatever the case of the callout, as Obsidian's own matching is", () => {
    expect(parseGalleryBlock(`> [!Gallery] ${FOLDER}`)?.folder).toBe(FOLDER);
    expect(parseGalleryBlock(`> [!GALLERY] ${FOLDER}`)?.folder).toBe(FOLDER);
  });

  it("is not a gallery when the blockquote is an ordinary quote or another callout", () => {
    expect(parseGalleryBlock("> a thing somebody said")).toBeNull();
    expect(parseGalleryBlock(`> [!note] ${FOLDER}`)).toBeNull();
    expect(parseGalleryBlock(`[!gallery] ${FOLDER}`)).toBeNull();
  });

  it("keeps prose inside the block rather than refusing the whole gallery", () => {
    const block = parseGalleryBlock(
      `> [!gallery] ${FOLDER}\n> the ones worth keeping:\n> [[${FOLDER}/b-hero.jpg]]`,
    );

    expect(block?.pins).toEqual([`${FOLDER}/b-hero.jpg`]);
  });

  it("names no folder when the callout has no title, rather than listing the vault root", () => {
    expect(parseGalleryBlock("> [!gallery]")?.folder).toBe("");
  });

  /** The block is only reachable if the menu that inserts it writes something
   *  this parser accepts. A row that produced text the renderer ignores would
   *  be a feature nobody can find. */
  it("is what the slash menu's Gallery row inserts", () => {
    const row = SLASH_COMMANDS.find((command) => command.label === "Gallery");
    expect(row).toBeDefined();
    expect(parseGalleryBlock(row?.text(new Date()) ?? "")).toEqual({ folder: "", pins: [] });
  });

  /**
   * The degrade assertion the story is graded on. Obsidian will never render
   * this widget, so what it renders instead has to be worth reading: a callout
   * titled with the folder, holding working links. Every line here is ordinary
   * markdown — a blockquote, a callout marker Obsidian defines, and wikilinks —
   * so there is nothing in the file that Obsidian shows as broken.
   */
  it("stays a callout of plain wikilinks, which is all Obsidian needs to render it", () => {
    const source = withPin(
      withPin(`> [!gallery] ${FOLDER}`, `${FOLDER}/b-hero.jpg`),
      `${FOLDER}/a-clip.mov`,
    );

    expect(source).toBe(
      `> [!gallery] ${FOLDER}\n> [[${FOLDER}/b-hero.jpg]]\n> [[${FOLDER}/a-clip.mov]]`,
    );
    for (const line of source.split("\n").slice(1)) {
      // A quoted line whose whole content is a wikilink: a link in Obsidian,
      // never an embed and never a fence of keeper's configuration language.
      expect(line).toMatch(/^> \[\[[^\]]+]]$/);
    }
    // No absolute path reaches the file (FR-145): every pin is vault-relative,
    // which is also the only form Obsidian resolves.
    expect(source).not.toMatch(/\[\[\//);
  });
});

// --- Pinning: a one-line splice into the note -------------------------------

describe("pinning writes into the note and nothing else", () => {
  const base = `> [!gallery] ${FOLDER}\n> a note to self\n> [[${FOLDER}/b-hero.jpg]]`;

  it("adds the new pin after the last one, leaving every other byte alone", () => {
    expect(withPin(base, `${FOLDER}/a-clip.mov`)).toBe(`${base}\n> [[${FOLDER}/a-clip.mov]]`);
  });

  it("adds the first pin directly under the head", () => {
    expect(withPin(`> [!gallery] ${FOLDER}\n> a note to self`, `${FOLDER}/a-clip.mov`)).toBe(
      `> [!gallery] ${FOLDER}\n> [[${FOLDER}/a-clip.mov]]\n> a note to self`,
    );
  });

  it("pins nothing twice", () => {
    expect(withPin(base, `${FOLDER}/b-hero.jpg`)).toBe(base);
  });

  it("round-trips: pinning then unpinning restores the note byte for byte", () => {
    expect(withoutPin(withPin(base, `${FOLDER}/a-clip.mov`), `${FOLDER}/a-clip.mov`)).toBe(base);
  });

  it("unpins the one line that names the item and nothing else", () => {
    expect(withoutPin(base, `${FOLDER}/b-hero.jpg`)).toBe(
      `> [!gallery] ${FOLDER}\n> a note to self`,
    );
    expect(withoutPin(base, `${FOLDER}/never-pinned.png`)).toBe(base);
  });

  /** FR-121 is a byte-level promise, and a CRLF vault is somebody's real vault:
   *  a splice that quietly normalised terminators would rewrite every line of
   *  the block in git. */
  it("keeps CRLF terminators when it splices a line into a CRLF block", () => {
    const crlf = `> [!gallery] ${FOLDER}\r\n> [[${FOLDER}/b-hero.jpg]]`;

    expect(withPin(crlf, `${FOLDER}/a-clip.mov`)).toBe(`${crlf}\r\n> [[${FOLDER}/a-clip.mov]]\r`);
  });

  it("copies the head line's own quote marker rather than assuming one", () => {
    expect(withPin(`>[!gallery] ${FOLDER}`, `${FOLDER}/a-clip.mov`)).toBe(
      `>[!gallery] ${FOLDER}\n>[[${FOLDER}/a-clip.mov]]`,
    );
  });
});

// --- What a gallery shows ---------------------------------------------------

describe("what a gallery shows", () => {
  it("skips a file nothing renders rather than offering a broken tile", () => {
    const order = galleryOrder(MIXED, []);

    expect(order.shown.map((each) => each.name)).toEqual([
      "a-clip.mov",
      "b-hero.jpg",
      "c-tone.wav",
    ]);
    expect(order.skipped).toBe(2);
  });

  it("floats the pinned items to the top, in the order the note lists them", () => {
    const order = galleryOrder(MIXED, [`${FOLDER}/c-tone.wav`, `${FOLDER}/b-hero.jpg`]);

    expect(order.shown.map((each) => each.name)).toEqual([
      "c-tone.wav",
      "b-hero.jpg",
      "a-clip.mov",
    ]);
    expect(order.missingPins).toBe(0);
  });

  it("counts a pin the folder no longer holds rather than dropping it silently", () => {
    const order = galleryOrder(MIXED, [`${FOLDER}/gone.png`, `${FOLDER}/manifest.json`]);

    expect(order.missingPins).toBe(2);
    expect(order.shown).toHaveLength(3);
  });

  it("says only the true clauses", () => {
    expect(gallerySummary({ shown: 3, skipped: 0, missingPins: 0, truncated: false })).toBe(
      "3 items",
    );
    expect(gallerySummary({ shown: 3, skipped: 2, missingPins: 1, truncated: true })).toBe(
      "3 items · 2 files are not media and are not shown · " +
        "1 pinned item is not in this folder · this folder holds more than the listing shows",
    );
  });
});

// --- Through the real decoration layer --------------------------------------

/**
 * Everything above drives the pure halves. This is where the feature lives: a
 * blockquote in a document becoming a windowed grid, and a pin becoming an edit
 * to the note that holds it.
 */
describe("livePreview, over a note with a gallery block", () => {
  const views: EditorView[] = [];
  let geometry: { undo: () => void } | null = null;

  afterEach(() => {
    for (const view of views.splice(0)) {
      view.destroy();
    }
    geometry?.undo();
    geometry = null;
  });

  function open(doc: string, list?: (folder: string) => Promise<NoteGalleryVm>): EditorView {
    const parent = document.createElement("div");
    document.body.append(parent);
    const view = new EditorView({
      parent,
      state: EditorState.create({
        doc,
        extensions: [
          // The real editor's grammar, because a gallery is a Blockquote node
          // and without the markdown language there are no nodes at all.
          markdown({ base: markdownLanguage }),
          livePreview({
            vaultId: "vault-1",
            assetUrl: (rel) => rel,
            onOpenLink: () => {},
            listFolder: list,
          }),
        ],
      }),
    });
    views.push(view);
    return view;
  }

  /** Drain the microtasks the listing rides on, and nothing else — the same
   *  reason `recording-embed.test.ts` refuses a timer here: a frame would start
   *  CodeMirror's measure pass, and jsdom's zero-height layout would replace
   *  the rendered lines with a viewport gap mid-assertion. */
  async function settle(): Promise<void> {
    for (let tick = 0; tick < 6; tick += 1) {
      await Promise.resolve();
    }
  }

  function block(pins: string[] = []): string {
    return [`> [!gallery] ${FOLDER}`, ...pins.map((pin) => `> [[${pin}]]`)].join("\n");
  }

  it("turns the block into a gallery of the folder's media", async () => {
    const view = open(`intro\n\n${block()}\n\nafter\n`, async () => listing(MIXED));

    await settle();
    const gallery = view.contentDOM.querySelector(".cm-lp-gallery");
    expect(gallery).not.toBeNull();
    expect(gallery?.querySelector(".cm-lp-gallery-folder")?.textContent).toBe(FOLDER);
    // One element per media kind, and none for the two files nothing renders.
    expect(view.contentDOM.querySelectorAll(".cm-lp-gallery-tile")).toHaveLength(3);
    expect(view.contentDOM.querySelectorAll("video")).toHaveLength(1);
    expect(view.contentDOM.querySelectorAll("img")).toHaveLength(1);
    expect(view.contentDOM.querySelectorAll("audio")).toHaveLength(1);
    expect(gallery?.querySelector(".cm-lp-gallery-note")?.textContent).toBe(
      "3 items · 2 files are not media and are not shown",
    );
  });

  it("never asks the protocol for a file nothing renders", async () => {
    const view = open(`intro\n\n${block()}\n\nafter\n`, async () => listing(MIXED));

    await settle();
    const sources = [...view.contentDOM.querySelectorAll("[src]")].map((each) =>
      each.getAttribute("src"),
    );
    expect(sources.some((src) => src?.includes("manifest.json") === true)).toBe(false);
    expect(sources).toHaveLength(3);
  });

  /**
   * AD-84, asserted by counting. Four hundred photographs is an ordinary
   * folder, and a surface that mounts four hundred tiles is one that stops
   * responding on the machine with the most to show.
   */
  it("mounts a bounded number of tiles over a folder of hundreds", async () => {
    geometry = withListGeometry({ viewport: 400, row: 0 });
    const many = Array.from({ length: 400 }, (_, index) =>
      item(`shot-${String(index).padStart(4, "0")}.jpg`, "image"),
    );
    const view = open(`intro\n\n${block()}\n\nafter\n`, async () => listing(many));

    await settle();
    const tiles = view.contentDOM.querySelectorAll(".cm-lp-gallery-tile");
    expect(tiles.length).toBeGreaterThan(0);
    // A window over a 400-item folder, not the folder: bounded by the viewport
    // plus the overscan, and nowhere near the 400 a naive grid would mount.
    expect(tiles.length).toBeLessThan(40);
    expect(view.contentDOM.querySelector(".cm-lp-gallery-note")?.textContent).toBe("400 items");
    // The canvas is still as tall as the whole folder, so the scrollbar tells
    // the truth about how much there is.
    const canvas = view.contentDOM.querySelector<HTMLElement>(".cm-lp-gallery-canvas");
    expect(Number.parseInt(canvas?.style.height ?? "0", 10)).toBeGreaterThan(400 * 40);
  });

  it("mounts the tiles the scroll position reaches and lets the earlier ones go", async () => {
    const layout = withListGeometry({ viewport: 400, row: 0 });
    geometry = layout;
    const many = Array.from({ length: 400 }, (_, index) =>
      item(`shot-${String(index).padStart(4, "0")}.jpg`, "image"),
    );
    const view = open(`intro\n\n${block()}\n\nafter\n`, async () => listing(many));
    await settle();

    const grid = view.contentDOM.querySelector(".cm-lp-gallery-grid");
    expect(grid).not.toBeNull();
    const firstRows = [...view.contentDOM.querySelectorAll(`[${WINDOW_ROW_ATTR}]`)].map((row) =>
      row.getAttribute(WINDOW_ROW_ATTR),
    );
    expect(firstRows).toContain("0");

    layout.scrollTo(grid as Element, 6_000);
    const laterRows = [...view.contentDOM.querySelectorAll(`[${WINDOW_ROW_ATTR}]`)].map((row) =>
      row.getAttribute(WINDOW_ROW_ATTR),
    );
    expect(laterRows).not.toContain("0");
    expect(laterRows.length).toBeLessThan(20);
    expect(view.contentDOM.querySelectorAll(".cm-lp-gallery-tile").length).toBeLessThan(40);
  });

  it("says what an unreadable folder said, in Rust's own words", async () => {
    const refusal = "this folder could not be read: Permission denied (os error 13)";
    const view = open(`intro\n\n${block()}\n\nafter\n`, async () =>
      listing([], { problem: refusal }),
    );

    await settle();
    expect(view.contentDOM.querySelector(".cm-lp-gallery-note")?.textContent).toBe(refusal);
    expect(view.contentDOM.querySelectorAll(".cm-lp-gallery-tile")).toHaveLength(0);
    // The block still names its folder and still offers its pins: a gallery
    // that could not list is not a gallery that lost the note's own text.
    expect(view.contentDOM.querySelector(".cm-lp-gallery-folder")?.textContent).toBe(FOLDER);
  });

  it("keeps the pinned links reachable when the listing never arrives", async () => {
    const view = open(`intro\n\n${block([`${FOLDER}/b-hero.jpg`])}\n\nafter\n`, async () => {
      throw new Error("no host");
    });

    await settle();
    expect(view.contentDOM.querySelector(".cm-lp-gallery-note")?.textContent).toBe(
      "this folder could not be listed just now.",
    );
    expect(view.contentDOM.querySelector(`[${WIKILINK_ATTR}]`)?.textContent).toBe(
      `${FOLDER}/b-hero.jpg`,
    );
  });

  it("says so rather than showing an empty grid when nothing here can list", async () => {
    const view = open(`intro\n\n${block()}\n\nafter\n`);

    await settle();
    expect(view.contentDOM.querySelector(".cm-lp-gallery-note")?.textContent).toBe(
      "keeper is not listing this folder here.",
    );
  });

  it("writes a pin into the note that holds the block", async () => {
    const list = vi.fn(async () => listing(MIXED));
    const view = open(`intro\n\n${block()}\n\nafter\n`, list);
    await settle();

    const pin = [...view.contentDOM.querySelectorAll<HTMLButtonElement>(".cm-lp-gallery-pin")].find(
      (button) => button.getAttribute("aria-label") === `${GALLERY_PIN_LABEL} c-tone.wav`,
    );
    expect(pin).toBeDefined();
    pin?.click();
    await settle();

    expect(view.state.doc.toString()).toBe(
      `intro\n\n${block([`${FOLDER}/c-tone.wav`])}\n\nafter\n`,
    );
    // Re-ordered in place: pinning one item must not cost a second listing of
    // a folder that may hold hundreds.
    expect(list).toHaveBeenCalledTimes(1);
    const tiles = [...view.contentDOM.querySelectorAll(".cm-lp-gallery-tile")];
    expect(tiles[0]?.querySelector(".cm-lp-gallery-caption")?.textContent).toBe("c-tone.wav");
    expect(tiles[0]?.getAttribute("data-gallery-pinned")).toBe("true");
    expect(tiles[0]?.querySelector("button")?.textContent).toBe(GALLERY_UNPIN_LABEL);
  });

  it("takes a pin back out of the note", async () => {
    const view = open(`intro\n\n${block([`${FOLDER}/c-tone.wav`])}\n\nafter\n`, async () =>
      listing(MIXED),
    );
    await settle();

    const unpin = [
      ...view.contentDOM.querySelectorAll<HTMLButtonElement>(".cm-lp-gallery-pin"),
    ].find((button) => button.getAttribute("aria-label") === `${GALLERY_UNPIN_LABEL} c-tone.wav`);
    expect(unpin).toBeDefined();
    unpin?.click();
    await settle();

    expect(view.state.doc.toString()).toBe(`intro\n\n${block()}\n\nafter\n`);
  });

  /**
   * The rule the story exists for. A pin is one note's opinion about a shared
   * folder; storing it beside the photographs would make it every note's.
   */
  it("keeps one note's pins out of another note over the same folder", async () => {
    const mine = open(`intro\n\n${block()}\n\nafter\n`, async () => listing(MIXED));
    const theirs = open(`theirs\n\n${block()}\n\nafter\n`, async () => listing(MIXED));
    await settle();

    const pin = [...mine.contentDOM.querySelectorAll<HTMLButtonElement>(".cm-lp-gallery-pin")].find(
      (button) => button.getAttribute("aria-label") === `${GALLERY_PIN_LABEL} c-tone.wav`,
    );
    pin?.click();
    await settle();

    expect(mine.state.doc.toString()).toContain(`> [[${FOLDER}/c-tone.wav]]`);
    // The other note over the same folder gained nothing, and still shows the
    // folder in the listing's own order.
    expect(theirs.state.doc.toString()).toBe(`theirs\n\n${block()}\n\nafter\n`);
    expect(parseGalleryBlock(block())?.pins).toEqual([]);
    const theirTiles = [...theirs.contentDOM.querySelectorAll(".cm-lp-gallery-tile")];
    expect(theirTiles[0]?.querySelector(".cm-lp-gallery-caption")?.textContent).toBe("a-clip.mov");
    expect(theirTiles.some((tile) => tile.hasAttribute("data-gallery-pinned"))).toBe(false);
  });

  it("shows the block's source when the caret is on it, so the text stays editable", async () => {
    const view = open(`intro\n\n${block()}\n\nafter\n`, async () => listing(MIXED));
    await settle();
    expect(view.contentDOM.querySelector(".cm-lp-gallery")).not.toBeNull();

    view.dispatch({ selection: { anchor: view.state.doc.line(3).from + 4 } });
    await settle();
    expect(view.contentDOM.querySelector(".cm-lp-gallery")).toBeNull();
    expect(view.contentDOM.textContent).toContain(`[!gallery] ${FOLDER}`);
  });

  it("leaves an ordinary blockquote alone", async () => {
    const list = vi.fn(async () => listing(MIXED));
    const view = open("intro\n\n> a thing somebody said\n\nafter\n", list);

    await settle();
    expect(view.contentDOM.querySelector(".cm-lp-gallery")).toBeNull();
    expect(list).not.toHaveBeenCalled();
  });
});

// --- Finding the block again from a position --------------------------------

describe("galleryRangeAt", () => {
  function docOf(text: string) {
    return EditorState.create({ doc: text }).doc;
  }

  it("finds the whole block from any position inside it", () => {
    const text = `intro\n\n> [!gallery] ${FOLDER}\n> [[${FOLDER}/b-hero.jpg]]\n\nafter\n`;
    const doc = docOf(text);
    const head = doc.line(3).from;

    for (const pos of [head, head + 3, doc.line(4).from + 2]) {
      const range = galleryRangeAt(doc, pos);
      expect(range?.text).toBe(`> [!gallery] ${FOLDER}\n> [[${FOLDER}/b-hero.jpg]]`);
      expect(range?.from).toBe(head);
    }
  });

  it("answers nothing outside a gallery, so a stale position splices nothing", () => {
    const doc = docOf(`intro\n\n> [!gallery] ${FOLDER}\n\nafter\n`);

    expect(galleryRangeAt(doc, 0)).toBeNull();
    expect(galleryRangeAt(doc, doc.line(5).from)).toBeNull();
    expect(galleryRangeAt(docOf("> just a quote"), 2)).toBeNull();
  });
});
