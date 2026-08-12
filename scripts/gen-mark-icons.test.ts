import { execFileSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { enclosedHoles, measure, nonBlackPixels, readPng } from "./lib/png-alpha";

// These run against the COMMITTED artwork rather than a fresh render, and that is
// the point: `gen-mark-icons.ts` already checks its own output, so re-checking a
// render would only prove the generator agrees with itself. What rots is the bytes
// in the repo — hand-edited, regenerated from a mangled source, or left behind
// when the geometry moved. Every failure below is a defect that was actually hit
// while building this family, not a hypothetical one.

const ICON_DIR = "src-tauri/crates/keeper/icons";
const MARK_SVG = `${ICON_DIR}/mark.svg`;

/** The @2x size of a 22pt menu-bar item, and what `tray.rs` hands to `set_icon`. */
const RETINA = 44;
const POINTS = 22;

const GLYPHS = [
  "tray-idle-template",
  "tray-live-template",
  "tray-working-template",
  "tray-fault-template",
  "tray-sync-template",
  "tray-sync-up-template",
  "tray-sync-down-template",
  "tray-sync-updown-template",
  "tray-sync-paused-template",
  "tray-sync-warning-template",
];

/**
 * The hex-bot's cell interior: one enclosed hole, with the eyes and mouth ink as
 * islands inside it. Extra ink only ever ADDS holes (the armed ring's core makes
 * more), so one is the floor for all ten glyphs — below it the cell has broken
 * open or filled in, which is the failure a bounding box cannot see.
 */
const MARK_HOLES = 1;

/**
 * Partial pixels at 22px, per glyph, pinned EXACTLY — the same numbers the
 * generator pins, restated here against the committed bytes.
 *
 * On the rectilinear tag this pin was zero. A hexagon owns four diagonals, and a
 * diagonal cannot land on whole pixels at any size — but every diagonal in the
 * cell is slope exactly 1:2, so the rasteriser's fringe is deterministic: 56
 * partial pixels for the ring in every glyph, plus 12 for an arrowhead's own
 * diagonals. Exact equality keeps the property the zero-gate had: an edge that
 * drifts off the even grid changes a count and fails.
 */
const EXPECTED_PARTIAL: Record<string, number> = {
  "tray-idle-template": 56,
  "tray-live-template": 56,
  "tray-working-template": 56,
  "tray-fault-template": 56,
  "tray-sync-template": 56,
  "tray-sync-up-template": 68,
  "tray-sync-down-template": 68,
  "tray-sync-updown-template": 56,
  "tray-sync-paused-template": 56,
  "tray-sync-warning-template": 56,
};

describe("the tray template family", () => {
  it.each(GLYPHS)("%s ships an @1x/@2x pair at the menu-bar sizes", (name) => {
    const at1x = readPng(`${ICON_DIR}/${name}.png`);
    const at2x = readPng(`${ICON_DIR}/${name}@2x.png`);

    expect([at1x.width, at1x.height]).toEqual([POINTS, POINTS]);
    expect([at2x.width, at2x.height]).toEqual([RETINA, RETINA]);
    // Exactly double, so the downscale macOS performs is a clean halving rather
    // than a resample — the whole reason the artwork sits on an even grid.
    expect(at2x.width).toBe(at1x.width * 2);
  });

  it.each(GLYPHS)("%s carries no colour, only black and alpha", (name) => {
    // macOS tints a template image by ALPHA and throws its RGB away. A glyph that
    // smuggles in colour looks right in a file viewer, renders wrong in the menu
    // bar, and inverts under a dark one — a defect no visual review of the PNG
    // catches, because the PNG itself looks fine.
    expect(nonBlackPixels(readPng(`${ICON_DIR}/${name}.png`))).toBe(0);
    expect(nonBlackPixels(readPng(`${ICON_DIR}/${name}@2x.png`))).toBe(0);
  });

  it.each(GLYPHS)("%s keeps the cell enclosed at the menu-bar size", (name) => {
    // The mark's identity is the contrast between its cell wall and what is kept
    // inside it. If the ring breaks open the interior drains into the background
    // and the face becomes line noise at exactly the size where that is all
    // anyone sees.
    expect(enclosedHoles(readPng(`${ICON_DIR}/${name}.png`)).length).toBeGreaterThanOrEqual(
      MARK_HOLES,
    );
  });

  it.each(GLYPHS)("%s rasterises at its pinned partial-pixel count", (name) => {
    // THE GATE THE 44-UNIT GRID EXISTS FOR, re-based for a silhouette that owns
    // diagonals. Every horizontal and vertical edge in the artwork is even, so it
    // lands whole at 22px; the ring's four 1:2 diagonals antialias in one
    // deterministic pattern. The pinned count is that pattern's exact cost — a
    // coordinate moved off the grid, a thickened wall, a mouth glyph nudged a
    // unit all change the number and fail here.
    const { partial } = measure(readPng(`${ICON_DIR}/${name}.png`));
    expect(partial).toBe(EXPECTED_PARTIAL[name]);
  });

  it("puts the head in identical pixels in every glyph", () => {
    // macOS centres a status-item image, so if the states put their ink in
    // different places the head visibly JUMPS the moment a sync starts and a
    // person cannot tell a state change from a glitch. This failed for real on an
    // earlier family whose direction marks sat in a reserved corner below the
    // head, which is why every state lives in the mouth now.
    const byBox = new Map<string, string[]>();
    for (const name of GLYPHS) {
      const box = measure(readPng(`${ICON_DIR}/${name}@2x.png`)).box.join(",");
      byBox.set(box, [...(byBox.get(box) ?? []), name]);
    }
    // The cell occupies canvas units x 6..38 and y 6..38, which at 44px are
    // inclusive pixel indices 6..37 both ways. Pinned rather than merely
    // "identical", so a family that moved together still fails.
    expect(Object.fromEntries(byBox)).toHaveProperty(["6,6,37,37"]);
    expect(byBox.size, `the head moved between glyphs: ${JSON.stringify([...byBox])}`).toBe(1);
  });

  it("draws all ten states differently", () => {
    // Two states that rasterise identically are a status indicator that lies.
    // This failed for real on an earlier family: a warning mark composed over the
    // fault state filled the fault's own gap back in, and two states shipped as
    // one picture.
    const owner = new Map<string, string>();
    for (const name of GLYPHS) {
      const key = Buffer.from(readPng(`${ICON_DIR}/${name}.png`).pixels).toString("base64");
      expect(owner.get(key), `${name} is pixel-identical to ${owner.get(key)}`).toBeUndefined();
      owner.set(key, name);
    }
    expect(owner.size).toBe(GLYPHS.length);
  });
});

describe("the mark on its own grid", () => {
  // Run on mark.svg itself rather than on a tray glyph, because mark.svg is the
  // source every shipped raster is cut from. Rasterised with the same
  // `tauri icon` (resvg) the generator uses — a devDependency, so it is present
  // wherever the suite runs.
  let work: string;

  beforeAll(() => {
    work = mkdtempSync(join(tmpdir(), "keeper-mark-test-"));
    execFileSync(
      "./node_modules/.bin/tauri",
      ["icon", MARK_SVG, "-o", work, "-p", "16", "-p", `${POINTS}`, "-p", `${RETINA}`],
      { stdio: ["ignore", "ignore", "pipe"] },
    );
  });

  afterAll(() => {
    rmSync(work, { recursive: true, force: true });
  });

  const at = (size: number) => readPng(join(work, `${size}x${size}.png`));

  it.each([
    [POINTS, 16, 16, 56],
    [RETINA, 32, 32, 112],
  ])("fills %ix its box at %ix%i with its pinned fringe", (size, w, h, partial) => {
    // The bbox proves the geometry still fills the box the grid promises; the
    // pinned partial count proves every straight edge is still even and the
    // diagonals still run at 1:2. Together they are the re-based form of the
    // tag's zero-mush gate — see EXPECTED_PARTIAL above for why zero is not
    // available to a hexagon.
    const m = measure(at(size));
    expect(m.partial).toBe(partial);
    expect([m.box[2] - m.box[0] + 1, m.box[3] - m.box[1] + 1]).toEqual([w, h]);
  });

  it.each([POINTS, RETINA])("keeps the cell interior enclosed at %ipx", (size) => {
    // One hole, and its proportions pinned: the interior spans the inner hex's
    // 24x24-unit box. "At least one hole" alone would pass a cell whose interior
    // had half filled in; the box says the whole room is still there.
    const holes = enclosedHoles(at(size));
    expect(holes).toHaveLength(MARK_HOLES);
    const scale = size / RETINA;
    expect(holes.map((x) => [x.box[2] - x.box[0] + 1, x.box[3] - x.box[1] + 1])).toEqual([
      [24 * scale, 24 * scale],
    ]);
  });

  it("still reads as the cell at 16px, where no grid can save it", () => {
    // 16px is where DESIGN.md's vendor comparison lives, and it is measured
    // rather than ignored — but what is asserted is SHAPE, not sharpness: the
    // mark keeps its full bounding box and the cell stays enclosed. Its edges are
    // grey, and no arrangement of even coordinates on a 44 grid can make them
    // otherwise (44/16 = 0.3636; nothing lands whole).
    //
    // Deliberately not a mush ceiling. Every edge is already fractional here, so
    // a coordinate moved to an odd unit barely moves this number. The 22px pin
    // above catches that mutation instantly.
    const png = at(16);
    const m = measure(png);
    expect([m.box[2] - m.box[0] + 1, m.box[3] - m.box[1] + 1]).toEqual([12, 12]);
    expect(enclosedHoles(png).length).toBeGreaterThanOrEqual(1);
  });
});

describe("the iOS AppIcon set", () => {
  const IOS_DIR = "src-tauri/crates/keeper/gen/apple/Assets.xcassets/AppIcon.appiconset";
  const IOS_ICONS = readdirSync(IOS_DIR).filter((f) => f.endsWith(".png"));

  it("ships the 18 files the asset catalog names", () => {
    // Contents.json fixes the filenames and Xcode fails the build on a missing
    // one, so the count is a contract rather than a coincidence.
    expect(IOS_ICONS).toHaveLength(18);
  });

  it.each(IOS_ICONS)("%s has no alpha channel at all", (name) => {
    // Apple rejects an app icon for HAVING an alpha channel, not for using it, so
    // a fully opaque RGBA icon fails submission just as a transparent one does.
    // Colour type 2 is RGB; 6 is RGBA. The rasteriser emits 6, which is why the
    // generator re-encodes — and why this asserts the file on disk, not the render.
    expect(readPng(`${IOS_DIR}/${name}`).colourType).toBe(2);
  });

  it.each(IOS_ICONS)("%s is fully opaque, corner to corner", (name) => {
    // An iOS icon is masked by the system, so it must be a full-bleed square. A
    // transparent or antialiased corner is the symptom of the desktop tile's
    // rounding leaking into a set that must not have any.
    const m = measure(readPng(`${IOS_DIR}/${name}`));
    expect([m.all - m.ink, m.partial]).toEqual([0, 0]);
  });
});

describe("the favicons", () => {
  // The two files that live in the REPO ROOT, so the project is identifiable at
  // a glance in a file browser, on GitHub, and in every tool that opens the
  // repo. Both are the same desktop tile `gen-mark-icons.ts` cuts: the PNG for
  // consumers that need a raster, the SVG for consumers that prefer a vector.
  const FAVICON = "favicon.png";
  const FAVICON_SVG = "favicon.svg";

  it("is a 1024px square", () => {
    // 1024 because it is a downscale for every consumer that reads it: 512 is the
    // set's next largest icon, 256 the biggest file-browser thumbnail, and 640 the
    // floor of GitHub's social-preview slot. A 512 square — the obvious choice —
    // would be the one that had to be enlarged.
    const png = readPng(FAVICON);
    expect([png.width, png.height]).toEqual([1024, 1024]);
  });

  it("wears the healthy-hive green from src/index.css, not black", () => {
    // Every other PNG this generator writes is either a pure-black template or an
    // opaque iOS tile. Cutting the favicon from the wrong one produces a file that
    // looks plausible in a diff and is invisible on a dark GitHub page — so the
    // check is that it carries the tile green the CSS currently defines (the
    // light theme's `--bridge-healthy`, keeper's original brand green — see the
    // generator for why the icon follows that token), which also catches a
    // favicon left behind by a retheme.
    const css = readFileSync("src/index.css", "utf8");
    const green = css
      .slice(css.indexOf(":root {"))
      .match(/--bridge-healthy:\s*(#[0-9a-fA-F]{6})\s*;/);
    expect(
      green,
      "--bridge-healthy is not defined in the :root block of src/index.css",
    ).not.toBeNull();
    const [r, g, b] = [1, 3, 5].map((i) =>
      Number.parseInt((green as string[])[1].slice(i, i + 2), 16),
    );

    const { pixels } = readPng(FAVICON);
    let tilePixels = 0;
    for (let i = 0; i < pixels.length; i += 4) {
      if (pixels[i] === r && pixels[i + 1] === g && pixels[i + 2] === b && pixels[i + 3] === 255) {
        tilePixels++;
      }
    }
    expect(tilePixels).toBeGreaterThan(0);

    // The SVG twin must wear the same green, by the same argument — and being a
    // text file, it can simply be read for it.
    expect(readFileSync(FAVICON_SVG, "utf8")).toContain((green as string[])[1]);
  });

  it("is actually referenced, not just present", () => {
    // A root-level image nothing points at is a file, not an identity. This is the
    // half of the job that is easy to leave undone: the icon renders, the diff
    // looks complete, and the browser tab still shows a blank page glyph.
    //
    // `capture.html` is deliberately absent from this list. It is the quick-capture
    // panel's own undecorated always-on-top document, which declares in its own
    // comment that it carries no icon and no theme bootstrap, and it has a 300 ms
    // paint budget to protect.
    const html = readFileSync("index.html", "utf8");
    expect(html).toContain(FAVICON);
    expect(html).toContain(FAVICON_SVG);
    expect(readFileSync("README.md", "utf8")).toContain(FAVICON);
  });
});
