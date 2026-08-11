import { execFileSync } from "node:child_process";
import { mkdtempSync, readdirSync, readFileSync, rmSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { afterAll, beforeAll, describe, expect, it } from "vitest";
import { enclosedHoles, type Hole, measure, nonBlackPixels, readPng } from "./lib/png-alpha";

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
 * Eyelet, rule, aperture. Extra ink only ever ADDS holes, so three is the floor
 * for all ten glyphs — not a per-glyph count.
 */
const MARK_HOLES = 3;

/** The two glyphs whose ink is an arrowhead, and so cannot be whole-pixel. */
const DIAGONAL_GLYPHS = ["tray-sync-up-template", "tray-sync-down-template"];

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

  it.each(GLYPHS)("%s keeps all three holes at the menu-bar size", (name) => {
    // The mark's identity is its holes. If a state's ink fills the aperture, or a
    // hole merges with the punched corner and drains into the background, the
    // glyph becomes a blob at exactly the size where a blob is all anyone sees.
    //
    // RAISED FROM 1 TO 3 with the 44-unit re-author, and that is a tightening
    // rather than a re-tune. On the 32-unit grid the mark had one hole, so one was
    // the whole contract; the tag has three, and a test that still asked for one
    // would pass with the eyelet and the rule filled solid.
    expect(enclosedHoles(readPng(`${ICON_DIR}/${name}.png`)).length).toBeGreaterThanOrEqual(
      MARK_HOLES,
    );
  });

  it.each(GLYPHS)("%s is drawn in whole pixels at the menu-bar size", (name) => {
    // THE GATE THE 44-UNIT GRID EXISTS FOR, and the one that replaced a 5%-of-ink
    // ceiling measured at 16px (see below for why that one had to go).
    //
    // 44 halves to 22 exactly and every coordinate in the artwork is even, so the
    // correct number of antialiased pixels is ZERO. Not "few": zero. Any edge that
    // lands on a half pixel shows up here as a non-zero count, which makes this
    // the only threshold in the file that cannot be quietly relaxed — there is no
    // slack in it to spend.
    //
    // The two arrows are exempt BY NAME rather than by a loosened number, because
    // an arrowhead is a diagonal and a diagonal has no whole-pixel form. Naming
    // them keeps the exemption auditable: a third glyph growing a curve fails.
    const { partial } = measure(readPng(`${ICON_DIR}/${name}.png`));
    expect(partial).toBe(DIAGONAL_GLYPHS.includes(name) ? 12 : 0);
  });

  it("puts the head in identical pixels in every glyph", () => {
    // macOS centres a status-item image, so if the states put their ink in
    // different places the head visibly JUMPS the moment a sync starts and a
    // person cannot tell a state change from a glitch. This failed for real while
    // the direction marks sat in a reserved corner below the head, which is why
    // they live inside the aperture now.
    const byBox = new Map<string, string[]>();
    for (const name of GLYPHS) {
      const box = measure(readPng(`${ICON_DIR}/${name}@2x.png`)).box.join(",");
      byBox.set(box, [...(byBox.get(box) ?? []), name]);
    }
    // The tag occupies canvas units x 6..38 and y 4..40, which at 44px are
    // inclusive pixel indices 6..37 and 4..39. Pinned rather than merely
    // "identical", so a family that moved together still fails.
    expect(Object.fromEntries(byBox)).toHaveProperty(["6,4,37,39"]);
    expect(byBox.size, `the head moved between glyphs: ${JSON.stringify([...byBox])}`).toBe(1);
  });

  it("draws all ten states differently", () => {
    // Two states that rasterise identically are a status indicator that lies.
    // This also failed for real: the warning mark was composed over the FAULT
    // aperture and its ink filled the bite back in, so warning and live were the
    // same picture.
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
  //
  // WHAT CHANGED HERE, AND WHY IT IS NOT A RELAXATION. This suite used to hold a
  // single number: partial pixels at 16px, under 5% of ink. That threshold was
  // calibrated for a 32-unit grid, where 32/16 = 2 made 16px the one size the
  // artwork landed whole at. On the 44-unit grid 44/16 = 0.3636, nothing lands
  // whole, and the mark measures 74% of ink there — so the old assertion could
  // only be met by keeping a grid that mushes at 22px, the size the menu bar
  // actually renders.
  //
  // It is replaced by a STRICTER gate at the sizes that ship: exactly zero
  // partial pixels at 22 and 44. Zero has no slack to quietly spend, which the
  // 5% did — it would have passed the old mark at 22px too, if anyone had ever
  // rasterised it there. The 16px number is still measured, and the generator
  // still prints it, because DESIGN.md rates this mark against vendor marks
  // measured at 16px alpha-only. What 16px no longer does is VETO the grid.
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
    [POINTS, 16, 18],
    [RETINA, 32, 36],
  ])("is drawn in whole pixels at %ipx, filling %ix%i", (size, w, h) => {
    const m = measure(at(size));
    expect(m.partial).toBe(0);
    expect([m.box[2] - m.box[0] + 1, m.box[3] - m.box[1] + 1]).toEqual([w, h]);
  });

  it.each([POINTS, RETINA])("keeps aperture, rule and eyelet enclosed at %ipx", (size) => {
    // Three holes and their exact proportions, largest first, because "three holes"
    // alone would pass a mark whose eyelet had shrunk back to a speck — which is
    // precisely what it was on the 32-unit grid. The aperture is much the largest;
    // it is the state display. The eyelet's 6 units are the number under pressure,
    // so they are pinned rather than bounded.
    const holes: Hole[] = enclosedHoles(at(size));
    expect(holes).toHaveLength(MARK_HOLES);
    const scale = size / RETINA;
    expect(holes.map((x) => [x.box[2] - x.box[0] + 1, x.box[3] - x.box[1] + 1])).toEqual([
      [24 * scale, 12 * scale],
      [18 * scale, 4 * scale],
      [6 * scale, 6 * scale],
    ]);
  });

  it("still reads as the tag at 16px, where no grid can save it", () => {
    // 16px is where DESIGN.md's vendor comparison lives, and it is measured rather
    // than ignored — but what is asserted is SHAPE, not sharpness. The mark keeps
    // its full bounding box and keeps the aperture open; its edges are grey, and
    // no arrangement of even coordinates on a 44 grid can make them otherwise.
    //
    // Deliberately not a mush ceiling. A mush ceiling here would be theatre: every
    // edge is already fractional, so an author moving a coordinate to an ODD unit
    // — the defect the old 16px threshold was built to catch — barely moves this
    // number at all. The 22px gate above catches that mutation instantly.
    const png = at(16);
    const m = measure(png);
    expect([m.box[2] - m.box[0] + 1, m.box[3] - m.box[1] + 1]).toEqual([12, 14]);
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

describe("favicon.png", () => {
  // The one raster that lives in the REPO ROOT, so the project is identifiable at
  // a glance in a file browser and on GitHub. Cut from the same coloured tile as
  // the desktop icon by `gen-mark-icons.ts`.
  const FAVICON = "favicon.png";

  it("is a 1024px square", () => {
    // 1024 because it is a downscale for every consumer that reads it: 512 is the
    // set's next largest icon, 256 the biggest file-browser thumbnail, and 640 the
    // floor of GitHub's social-preview slot. A 512 square — the obvious choice —
    // would be the one that had to be enlarged.
    const png = readPng(FAVICON);
    expect([png.width, png.height]).toEqual([1024, 1024]);
  });

  it("wears the accent from src/index.css, not black", () => {
    // Every other PNG this generator writes is either a pure-black template or an
    // opaque iOS tile. Cutting the favicon from the wrong one produces a file that
    // looks plausible in a diff and is invisible on a dark GitHub page — so the
    // check is that it carries the accent the CSS currently defines, which also
    // catches a favicon left behind by a retheme.
    const css = readFileSync("src/index.css", "utf8");
    const accent = css.slice(css.indexOf(".dark {")).match(/--primary:\s*(#[0-9a-fA-F]{6})\s*;/);
    expect(accent, "--primary is not defined in the .dark block of src/index.css").not.toBeNull();
    const [r, g, b] = [1, 3, 5].map((i) =>
      Number.parseInt((accent as string[])[1].slice(i, i + 2), 16),
    );

    const { pixels } = readPng(FAVICON);
    let accentPixels = 0;
    for (let i = 0; i < pixels.length; i += 4) {
      if (pixels[i] === r && pixels[i + 1] === g && pixels[i + 2] === b && pixels[i + 3] === 255) {
        accentPixels++;
      }
    }
    expect(accentPixels).toBeGreaterThan(0);
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
    expect(readFileSync("index.html", "utf8")).toContain(FAVICON);
    expect(readFileSync("README.md", "utf8")).toContain(FAVICON);
  });
});
