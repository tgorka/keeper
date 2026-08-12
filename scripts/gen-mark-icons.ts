#!/usr/bin/env bun
/**
 * Generates every raster the keeper mark is shipped as: the app icon set Tauri bundles, the macOS
 * tray TEMPLATE family the menu bar tints, the iOS AppIcon set, `favicon.png` in the repo root —
 * and `favicon.svg` beside it, the one vector consumers that can read a vector should prefer.
 *
 * `src-tauri/crates/keeper/icons/mark.svg` is the only source of geometry — including the tray
 * badges and the halo that seats them. Nothing here knows the shape of anything: this file
 * composes ids the mark exports, places them on the tray canvas, and hands the result to a
 * rasteriser. Every check below asserts the rasterised result rather than trusting the artwork.
 *
 * That split is the lesson of `gen-tray-sync-icons.ts`, which this file replaced: it derived a
 * glyph family from a shipped PNG and carried constants measured off a brand two brands ago.
 * Those numbers were correct, and became meaningless the day the silhouette changed, with
 * nothing in the file able to notice.
 *
 * RASTERISER: `tauri icon`, from the `@tauri-apps/cli` devDependency, which embeds resvg. It is
 * the official path, it takes SVG directly, and it is deterministic, which is what makes the
 * committed PNGs reviewable in a diff.
 *
 * Run: bun run scripts/gen-mark-icons.ts
 */

import { execFileSync } from "node:child_process";
import {
  copyFileSync,
  mkdirSync,
  mkdtempSync,
  readdirSync,
  readFileSync,
  rmSync,
  writeFileSync,
} from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { enclosedHoles, encodeRgbPng, measure, nonBlackPixels, readPng } from "./lib/png-alpha";

const ICON_DIR = "src-tauri/crates/keeper/icons";
/** Fixed by the Xcode asset catalog; `AppIcon.appiconset/Contents.json` names every file. */
const IOS_ICON_DIR = "src-tauri/crates/keeper/gen/apple/Assets.xcassets/AppIcon.appiconset";
const MARK_SVG = `${ICON_DIR}/mark.svg`;
const TAURI_CLI = "./node_modules/.bin/tauri";

// ---------------------------------------------------------------------------
// The mark's grid, restated from mark.svg. These are the only things this file
// assumes about the artwork, and the run asserts them against the rasterised
// result rather than trusting them.
// ---------------------------------------------------------------------------

/**
 * The mark's viewBox: 44 units, which is the tray canvas — 1:1 at 44px, halved
 * exactly at 22px. The cell's horizontal bands land on even whole pixels; the
 * diagonals run at slope exactly 1:2; the face is round because the approved
 * comp is round. Determinism is enforced by PINNED per-glyph raster counts, not
 * by forbidding curves: any drift off the authored geometry changes a count.
 */
const MARK_GRID = 44;
/** The cell's bounding box — x 6..38 by y 6..38, so 32 by 32. */
const MARK_BOX = { x0: 6, y0: 6, x1: 38, y1: 38 } as const;
/**
 * The hero is the idle cell plus a smile and nothing above it — the antenna is
 * retired (see mark.svg), so the hero shares the cell's own box and can scale
 * up to fill the tile instead of budgeting headroom for a mast.
 */
const APP_SCALE = 1.25;

const MARK_W = MARK_BOX.x1 - MARK_BOX.x0;
const MARK_H = MARK_BOX.y1 - MARK_BOX.y0;

// ---------------------------------------------------------------------------
// Tray canvas and placement.
//
// 44 units, the @2x size of a 22pt menu-bar item — and the mark's own grid, so
// the mark is authored at the size it is worn and the translate below comes out
// at (0, 0). It is kept, and asserted, so a future edit to MARK_BOX still lands
// somewhere legal.
//
// THE HEAD CANNOT MOVE BETWEEN GLYPHS because every glyph is the same 44-unit
// canvas with the cell at the same translate, and macOS centres the BITMAP —
// which never changes size. That is what lets the transport badges sit in the
// canvas corner the hexagon's cut leaves free, as the approved comp draws them,
// without the head jumping: the ink bounding box differs per glyph (and is
// pinned per glyph below), but the cell's pixels are the same pixels every time.
// ---------------------------------------------------------------------------

const CANVAS = 44;
const DX = (CANVAS - MARK_W) / 2 - MARK_BOX.x0;
const DY = (CANVAS - MARK_H) / 2 - MARK_BOX.y0;

for (const [name, v] of [
  ["DX", DX],
  ["DY", DY],
] as const) {
  if (!Number.isInteger(v) || v % 2 !== 0) {
    throw new Error(
      `tray placement ${name} is ${v}, which is not an even whole unit: ` +
        `MARK_BOX ${MARK_W}x${MARK_H} cannot be centred on a ${CANVAS} canvas without ` +
        `leaving the pixel grid at ${CANVAS / 2}px`,
    );
  }
}

// ---------------------------------------------------------------------------
// The shipped family.
//
// The face carries the lamp state (mouth: filled / empty / dashed / broken;
// eyes close for paused); the bottom-left badge carries the transport facts the
// face cannot express. All ids live in mark.svg. `masked` applies the badge
// halo to the base, biting the ring open so the badge reads as a token pinned
// beside the cell — those glyphs' cell is deliberately not a closed loop, which
// the per-glyph hole pins below turn from an accident into a checked fact.
// ---------------------------------------------------------------------------

type Glyph = {
  name: string;
  base: "idle" | "live" | "working" | "fault" | "paused";
  badge?: "ring" | "up" | "down" | "updown" | "alert";
  note: string;
};

const GLYPHS: Glyph[] = [
  { name: "tray-idle-template", base: "idle", note: "presence only, no sync configured" },
  { name: "tray-live-template", base: "live", note: "a recording is running" },
  { name: "tray-working-template", base: "working", note: "sync active, nothing on the wire" },
  { name: "tray-fault-template", base: "fault", note: "a failed session holds the tray" },
  { name: "tray-sync-template", base: "idle", badge: "ring", note: "sync armed" },
  { name: "tray-sync-up-template", base: "idle", badge: "up", note: "uploading" },
  { name: "tray-sync-down-template", base: "idle", badge: "down", note: "downloading" },
  { name: "tray-sync-updown-template", base: "idle", badge: "updown", note: "both ways" },
  { name: "tray-sync-paused-template", base: "paused", note: "sync paused — the bot rests" },
  { name: "tray-sync-warning-template", base: "idle", badge: "alert", note: "sync warning" },
];

/**
 * The pinned rasters, per glyph at 22px: partial pixels, enclosed holes, and
 * the ink bounding box. Exact equality — not ceilings — so a coordinate that
 * drifts, a hole that fills, or ink that strays all change a number and fail.
 * Holes: a closed cell keeps its interior enclosed (1); a halo-bitten cell
 * drains it (0) unless the badge itself encloses one (the armed ring). Filled
 * in from the measured run; a mismatch prints both numbers.
 */
const EXPECTED: Record<string, { partial: number; holes: number; box: string }> = {
  "tray-idle-template": { partial: 72, holes: 1, box: "3,3,18,18" },
  "tray-live-template": { partial: 82, holes: 1, box: "3,3,18,18" },
  "tray-working-template": { partial: 90, holes: 1, box: "3,3,18,18" },
  "tray-fault-template": { partial: 84, holes: 1, box: "3,3,18,18" },
  "tray-sync-template": { partial: 98, holes: 1, box: "1,3,18,20" },
  "tray-sync-up-template": { partial: 87, holes: 0, box: "2,3,18,20" },
  "tray-sync-down-template": { partial: 87, holes: 0, box: "2,3,18,20" },
  "tray-sync-updown-template": { partial: 83, holes: 0, box: "1,3,18,20" },
  "tray-sync-paused-template": { partial: 60, holes: 1, box: "3,3,18,18" },
  "tray-sync-warning-template": { partial: 84, holes: 0, box: "3,3,18,20" },
};

// ---------------------------------------------------------------------------
// SVG composition. mark.svg's <defs> block is lifted verbatim, so the geometry
// is never restated here — only referenced. Ink colour rides on CSS
// `currentColor`, which is what lets the same defs serve the black templates
// and the coloured hero.
// ---------------------------------------------------------------------------

const markSource = readFileSync(MARK_SVG, "utf8");
const defs = /<defs>[\s\S]*<\/defs>/.exec(markSource)?.[0];
if (!defs) throw new Error(`${MARK_SVG} has no <defs> block to compose from`);
for (const id of [
  "mark-live",
  "mark-idle",
  "mark-working",
  "mark-fault",
  "mark-paused",
  "mark-hero",
  "bl-halo",
  "badge-ring",
  "badge-up",
  "badge-down",
  "badge-updown",
  "badge-alert",
]) {
  if (!defs.includes(`id="${id}"`)) {
    throw new Error(`${MARK_SVG} defines no #${id}; the state vocabulary has drifted`);
  }
}

/** Wraps a body in an SVG that can see the mark's defs. The one composition seam. */
function svg(viewBox: string, body: string): string {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${viewBox}">${defs}${body}</svg>`;
}

/** One tray glyph's body: the (possibly halo-bitten) base plus its badge. */
function glyphBody(g: Glyph): string {
  const base = g.badge
    ? `<g mask="url(#bl-halo)"><use href="#mark-${g.base}"/></g><use href="#badge-${g.badge}"/>`
    : `<use href="#mark-${g.base}"/>`;
  // design-allow color: template images are tinted by macOS through their ALPHA
  // and their RGB is ignored, so the ink must be pure black. A format
  // requirement, not a palette choice.
  return `<g style="color:#000000" transform="translate(${DX} ${DY})">${base}</g>`;
}

/**
 * The icon's colours are READ from `src/index.css` rather than repeated here.
 *
 * This is not tidiness. `tray.rs` once shipped a hardcoded teal as the
 * Linux/Windows tray repaint colour, because a copy of the palette in native
 * code has no way of hearing that the palette changed. A generator that
 * hardcodes a colour is the same bug with a longer fuse.
 */
function tokenFromCss(name: string, block: ":root {" | ".dark {"): string {
  const css = readFileSync("src/index.css", "utf8");
  const scope = css.slice(css.indexOf(block));
  const match = scope.match(new RegExp(`--${name}:\\s*(#[0-9a-fA-F]{3,8})\\s*;`));
  if (!match) {
    throw new Error(`--${name} is not defined in the ${block.slice(0, -2)} block of src/index.css`);
  }
  return match[1];
}

/**
 * The tile is GREEN and the mark is paper-white — the owner's approved comps,
 * held to on the second pass after a night-ink draft was rejected.
 *
 * The green is the light theme's `--bridge-healthy`: keeper's original brand
 * green, which is where that colour now lives in the palette — and a hive cell
 * radiating "bridged, healthy, kept" is the icon saying what the product does.
 * The mark is the light theme's paper (`--background`), the identity's white.
 * The neighbour cells ghost in the dark ground at low opacity, which reads as
 * a deeper green on the tile.
 */
const TILE_BG = tokenFromCss("bridge-healthy", ":root {");
const MARK_INK = tokenFromCss("background", ":root {");
const NEIGHBOUR_INK = tokenFromCss("background", ".dark {");
const NEIGHBOUR_OPACITY = 0.3;
const TILE = 64;
const TILE_INSET = 4;
/**
 * `favicon.png` in the REPO ROOT — the same coloured tile as the desktop icon,
 * cut once, large. 1024 because it is the only size that is a DOWNSCALE for
 * every consumer that reads it (512 is the set's next largest icon, 256 a file
 * browser's biggest thumbnail, 640 the floor of GitHub's social-preview slot).
 * `favicon.svg` is the same drawing as a vector, for consumers that can.
 */
const FAVICON = "favicon.png";
const FAVICON_SVG = "favicon.svg";
const FAVICON_SIZE = 1024;

/**
 * A flat-topped hexagon path, for the tile's neighbour cells. Decorative and
 * colour-rastered only, so off-grid coordinates are fine here.
 */
function hexPath(cx: number, cy: number, r: number): string {
  const h = r * 0.866;
  return (
    `M${cx - r / 2} ${cy - h}L${cx + r / 2} ${cy - h}L${cx + r} ${cy}` +
    `L${cx + r / 2} ${cy + h}L${cx - r / 2} ${cy + h}L${cx - r} ${cy}Z`
  );
}

/**
 * The app icon: the hex-bot hero in paper on the healthy green, with two
 * neighbour cells ghosted behind it — the hive the cell belongs to: many
 * networks, one kept structure. An icon has a container whether or not the
 * design draws one, and drawing it is what stops the mark floating in the dock.
 *
 * `inset` and `rx` differ by platform and both differences are required rather
 * than stylistic. Desktop draws its own rounded tile with clear space around it,
 * because macOS and Windows show the bitmap as authored. iOS gets a FULL-BLEED
 * square with square corners: the system applies its own superellipse mask, and
 * an iOS app icon may carry no alpha at all, which a rounded tile's antialiased
 * corners violate by construction.
 *
 * With the antenna retired the hero is the cell alone, so it scales up by
 * APP_SCALE to fill the tile the way an icon should — 40 of the inner 56 units
 * — instead of sitting small under headroom nothing uses any more. Colour
 * rasters antialiase, so the off-grid scale costs nothing the templates pay.
 */
function appIcon(inset: number, rx: number): string {
  const tx = TILE / 2 - APP_SCALE * (MARK_BOX.x0 + MARK_W / 2);
  const ty = TILE / 2 - APP_SCALE * (MARK_BOX.y0 + MARK_H / 2);
  const clipId = `tile-${inset}-${rx}`;
  return svg(
    `0 0 ${TILE} ${TILE}`,
    `<clipPath id="${clipId}"><rect x="${inset}" y="${inset}" ` +
      `width="${TILE - inset * 2}" height="${TILE - inset * 2}" rx="${rx}"/></clipPath>` +
      `<rect x="${inset}" y="${inset}" width="${TILE - inset * 2}" ` +
      `height="${TILE - inset * 2}" rx="${rx}" fill="${TILE_BG}"/>` +
      `<g clip-path="url(#${clipId})" fill="none" stroke="${NEIGHBOUR_INK}" ` +
      `stroke-opacity="${NEIGHBOUR_OPACITY}" stroke-width="2">` +
      `<path d="${hexPath(7, 9, 13)}"/><path d="${hexPath(57, 56, 13)}"/></g>` +
      `<g style="color:${MARK_INK}"><use href="#mark-hero" ` +
      `transform="translate(${tx} ${ty}) scale(${APP_SCALE})"/></g>`,
  );
}

// ---------------------------------------------------------------------------
// Rasterising. The alpha measurements the checks below run on live in
// `lib/png-alpha.ts`, shared with this script's test so the gate and the
// generator can never disagree about what "legible" means.
// ---------------------------------------------------------------------------

const work = mkdtempSync(join(tmpdir(), "keeper-mark-"));

function rasterise(source: string, sizes: number[], outDir: string): void {
  mkdirSync(outDir, { recursive: true });
  execFileSync(TAURI_CLI, ["icon", source, "-o", outDir, ...sizes.flatMap((s) => ["-p", `${s}`])], {
    stdio: ["ignore", "ignore", "pipe"],
  });
}

// ---------------------------------------------------------------------------
// Run.
// ---------------------------------------------------------------------------

const failures: string[] = [];
function check(ok: boolean, message: string): void {
  if (!ok) failures.push(message);
}

// --- the desktop app icon set ----------------------------------------------
//
// Rendered into a scratch directory, then only the files the build reads are
// placed. Generating straight into the repo would leave an Android mipmap tree
// and a 64px PNG behind, and this project has no Android target and no bundle
// entry naming either.
const desktopSvg = join(work, "app-icon.svg");
writeFileSync(desktopSvg, appIcon(TILE_INSET, 13));
const desktopSet = join(work, "desktop");
execFileSync(TAURI_CLI, ["icon", desktopSvg, "-o", desktopSet], {
  stdio: ["ignore", "ignore", "pipe"],
});
for (const entry of readdirSync(desktopSet, { withFileTypes: true })) {
  if (entry.isDirectory() || entry.name === "64x64.png") continue;
  copyFileSync(join(desktopSet, entry.name), `${ICON_DIR}/${entry.name}`);
}
console.log(`app icon set  <- ${MARK_SVG}, hero 1:1 in ${MARK_INK} on ${TILE_BG}`);

// --- favicon.svg and favicon.png, in the repo root ---------------------------
//
// Both cut from the SAME desktop tile source rather than drawn again. The owner
// asked for the project to be identifiable at a glance in a file browser, on
// GitHub, and in every tool that opens the repo; a second drawing of the mark to
// satisfy that is precisely how generators drift.
//
// The `<title>` is both the accessible name and what Biome's a11y lint requires
// of any SVG in the tree; the raster pipeline strips it, so only the vector pays
// the bytes.
writeFileSync(
  FAVICON_SVG,
  `${appIcon(TILE_INSET, 13).replace(">", "><title>keeper — the hex-bot</title>")}\n`,
);
check(
  readFileSync(FAVICON_SVG, "utf8").includes(TILE_BG),
  `${FAVICON_SVG} does not carry the tile green ${TILE_BG}`,
);
console.log(`${FAVICON_SVG}  the desktop tile as a vector — for tools that prefer one`);

const faviconDir = join(work, "favicon");
rasterise(desktopSvg, [FAVICON_SIZE], faviconDir);
copyFileSync(join(faviconDir, `${FAVICON_SIZE}x${FAVICON_SIZE}.png`), FAVICON);
const favicon = readPng(FAVICON);
check(
  favicon.width === FAVICON_SIZE && favicon.height === FAVICON_SIZE,
  `${FAVICON} is ${favicon.width}x${favicon.height}, not ${FAVICON_SIZE}px square`,
);
// It must carry the tile green. The failure this catches is not hypothetical:
// every other PNG this script writes is a pure-black template or an opaque RGB
// tile, and cutting the favicon from the wrong one would produce a file that
// looks plausible in a diff and is invisible on a dark GitHub page.
const tileRgb = [1, 3, 5].map((i) => Number.parseInt(TILE_BG.slice(i, i + 2), 16));
let tilePixels = 0;
for (let i = 0; i < favicon.pixels.length; i += 4) {
  if (
    favicon.pixels[i] === tileRgb[0] &&
    favicon.pixels[i + 1] === tileRgb[1] &&
    favicon.pixels[i + 2] === tileRgb[2] &&
    favicon.pixels[i + 3] === 255
  ) {
    tilePixels++;
  }
}
check(
  tilePixels > 0,
  `${FAVICON} carries no ${TILE_BG} pixel — it was cut from a template, not from the green tile`,
);
console.log(
  `${FAVICON}  ${FAVICON_SIZE}px, the desktop tile, ${tilePixels} px of ${TILE_BG} ` +
    `— referenced by index.html and README.md`,
);

// --- the iOS AppIcon set ----------------------------------------------------
//
// A different source: full-bleed and square-cornered, because iOS masks the icon
// itself and forbids alpha (see `appIcon`). `--ios-color` still names the tile
// so any pixel the CLI has to flatten lands on the brand green rather than on
// its default white.
const iosSvg = join(work, "app-icon-ios.svg");
writeFileSync(iosSvg, appIcon(0, 0));
const iosSet = join(work, "ios");
execFileSync(TAURI_CLI, ["icon", iosSvg, "-o", iosSet, "--ios-color", TILE_BG], {
  stdio: ["ignore", "ignore", "pipe"],
});

// The filenames and pixel sizes are fixed by `AppIcon.appiconset/Contents.json`,
// and the CLI emits exactly that set. Apple rejects an app icon for HAVING an
// alpha channel, not for using it, so a fully opaque RGBA icon still fails —
// which makes the re-encode below a contract this set had before and must keep.
let iosCount = 0;
for (const entry of readdirSync(join(iosSet, "ios"))) {
  const rendered = readPng(join(iosSet, "ios", entry));
  const m = measure(rendered);
  check(
    m.ink === m.all && m.partial === 0,
    `${entry} is not fully opaque before the alpha strip: ` +
      `${m.all - m.ink} transparent, ${m.partial} partial`,
  );
  writeFileSync(`${IOS_ICON_DIR}/${entry}`, encodeRgbPng(rendered));
  check(
    readPng(`${IOS_ICON_DIR}/${entry}`).colourType === 2,
    `${entry} still has an alpha channel after re-encoding`,
  );
  iosCount++;
}
console.log(`iOS AppIcon set  ${iosCount} files on ${TILE_BG}, RGB with no alpha channel`);

// --- the tray template family ----------------------------------------------
const RETINA = CANVAS;
const POINTS = CANVAS / 2;
console.log(
  `\ntray templates  ${POINTS}px @1x / ${RETINA}px @2x  ` +
    `cell x${MARK_BOX.x0 + DX}..${MARK_BOX.x1 + DX} y${MARK_BOX.y0 + DY}..${MARK_BOX.y1 + DY}, ` +
    `badge seat (8,36) r4-5.5 (mark space)\n`,
);

const report: string[][] = [];
/**
 * Every glyph's @1x alpha map, in family order, kept so the ten can be diffed
 * against each other once they all exist. Two glyphs that rasterise the same are
 * two menu-bar states a user cannot tell apart, and the tray would go on
 * reporting them as if they could.
 */
const alphaAt1x: { name: string; alpha: Uint8Array }[] = [];

for (const g of GLYPHS) {
  const dir = join(work, g.name);
  const source = join(work, `${g.name}.svg`);
  writeFileSync(source, svg(`0 0 ${CANVAS} ${CANVAS}`, glyphBody(g)));
  rasterise(source, [POINTS, RETINA], dir);
  // A copy rather than a rename: the scratch dir is under /tmp, which can be a
  // different device, and rename cannot cross one.
  copyFileSync(join(dir, `${POINTS}x${POINTS}.png`), `${ICON_DIR}/${g.name}.png`);
  copyFileSync(join(dir, `${RETINA}x${RETINA}.png`), `${ICON_DIR}/${g.name}@2x.png`);

  const at1x = readPng(`${ICON_DIR}/${g.name}.png`);
  const at2x = readPng(`${ICON_DIR}/${g.name}@2x.png`);
  const holes = enclosedHoles(at1x);
  const m = measure(at1x);
  const box = m.box.join(",");
  const want = EXPECTED[g.name] as (typeof EXPECTED)[string];

  check(at1x.width === POINTS && at1x.height === POINTS, `${g.name} @1x is not ${POINTS}px`);
  check(at2x.width === RETINA && at2x.height === RETINA, `${g.name} @2x is not ${RETINA}px`);
  check(nonBlackPixels(at1x) === 0, `${g.name} @1x carries non-black RGB`);
  check(nonBlackPixels(at2x) === 0, `${g.name} @2x carries non-black RGB`);
  check(
    holes.length === want.holes,
    `${g.name} @1x has ${holes.length} enclosed holes at ${POINTS}px, pinned at ${want.holes}`,
  );
  check(
    m.partial === want.partial,
    `${g.name} @1x has ${m.partial} partial pixels at ${POINTS}px, pinned at ${want.partial} — ` +
      `some edge moved off the authored geometry`,
  );
  check(
    box === want.box,
    `${g.name} @1x ink box ${box}, pinned at ${want.box} — ink strayed or vanished`,
  );
  alphaAt1x.push({
    name: g.name.replace(/^tray-|-template$/g, ""),
    alpha: at1x.pixels.filter((_, i) => i % 4 === 3),
  });

  report.push([
    g.name.replace(/^tray-|-template$/g, ""),
    g.base,
    g.badge ?? "-",
    `${holes.length}`,
    `${m.partial}`,
    box,
    g.note,
  ]);
}

const cols = [13, 8, 7, 5, 7, 11];
for (const row of [
  ["glyph", "face", "badge", "holes", "partial", "box", "shown when"],
  ...report,
]) {
  console.log(row.map((c, i) => c.padEnd(cols[i] ?? 0)).join(" "));
}

// --- no two glyphs may rasterise the same -----------------------------------
//
// Distinctness is not enough on its own, so the WEAKEST pair is printed rather
// than just asserted non-zero. An earlier mark in this repo passed the non-zero
// test while `live` and `fault` differed by three pixels out of 484 —
// technically distinct, and indistinguishable to anyone not already staring at
// the menu bar. A number on screen is what makes that visible next time.
const pairs: { pair: string; differing: number }[] = [];
for (let i = 0; i < alphaAt1x.length; i++) {
  for (let j = i + 1; j < alphaAt1x.length; j++) {
    const a = alphaAt1x[i] as (typeof alphaAt1x)[number];
    const b = alphaAt1x[j] as (typeof alphaAt1x)[number];
    let differing = 0;
    for (let k = 0; k < a.alpha.length; k++) if (a.alpha[k] !== b.alpha[k]) differing++;
    pairs.push({ pair: `${a.name}/${b.name}`, differing });
  }
}
pairs.sort((a, b) => a.differing - b.differing);
for (const p of pairs) {
  check(p.differing > 0, `${p.pair} rasterise identically at ${POINTS}px`);
}
console.log(
  `\nall ${pairs.length} glyph pairs differ at ${POINTS}px; weakest ` +
    pairs
      .slice(0, 4)
      .map((p) => `${p.pair} ${p.differing}px`)
      .join(", "),
);

// --- the pinned-raster proof, and the 16px report ----------------------------
//
// Run on mark.svg itself rather than on a tray glyph, because mark.svg is the
// source every other file in this run is cut from. 22 and 44 are gated at
// pinned partial-pixel counts and at the exact bbox the grid promises; 16 is
// reported, not gated (44/16 lands nothing whole, and nothing ships a 16px
// alpha-only template) — the number is still printed because DESIGN.md rates
// this mark against vendor marks measured at 16px alpha-only, and a benchmark
// nobody prints is a benchmark nobody meets.
const EXPECTED_MARK_PARTIAL: Record<number, number> = { 22: 72, 44: 200 };
const proofDir = join(work, "proof");
const proofSizes = [16, POINTS, RETINA];
rasterise(MARK_SVG, proofSizes, proofDir);
console.log("\nmark.svg alpha-only (no colour, no tile)   * = gated, pinned counts");
for (const size of proofSizes) {
  const gated = size === POINTS || size === RETINA;
  const png = readPng(join(proofDir, `${size}x${size}.png`));
  const m = measure(png);
  const holes = enclosedHoles(png);
  const w = m.box[2] - m.box[0] + 1;
  const h = m.box[3] - m.box[1] + 1;
  console.log(
    `  ${gated ? "*" : " "} ${String(size).padStart(2)}px  ` +
      `partial ${m.partial} (${((m.partial / m.ink) * 100).toFixed(1)}% of inked ${m.ink})  ` +
      `bbox ${w}x${h}  holes ${holes.length}  ` +
      `[${holes.map((x) => `${x.box[2] - x.box[0] + 1}x${x.box[3] - x.box[1] + 1}=${x.area}px`).join(" ") || "NONE"}]`,
  );
  if (!gated) continue;
  check(
    m.partial === EXPECTED_MARK_PARTIAL[size],
    `at ${size}px the mark has ${m.partial} partial pixels, pinned at ${EXPECTED_MARK_PARTIAL[size]}`,
  );
  check(
    holes.length === 1,
    `at ${size}px the mark has ${holes.length} enclosed holes, expected 1 (the cell)`,
  );
  check(
    w === (MARK_W * size) / MARK_GRID && h === (MARK_H * size) / MARK_GRID,
    `at ${size}px the mark's bbox is ${w}x${h}, not the grid's ` +
      `${(MARK_W * size) / MARK_GRID}x${(MARK_H * size) / MARK_GRID} — geometry has left the box`,
  );
}

rmSync(work, { recursive: true, force: true });

if (failures.length) {
  console.error(`\n${failures.length} check(s) FAILED:`);
  for (const f of failures) console.error(`  - ${f}`);
  process.exit(1);
}
console.log("\nall checks passed: pure black + alpha, pinned rasters, cell fixed in the canvas");
