#!/usr/bin/env bun
/**
 * Generates every raster the keeper mark is shipped as: the app icon set Tauri bundles, the macOS
 * tray TEMPLATE family the menu bar tints, the iOS AppIcon set, `favicon.png` in the repo root —
 * and `favicon.svg` beside it, the one vector consumers that can read a vector should prefer.
 *
 * `src-tauri/crates/keeper/icons/mark.svg` is the only source of geometry. Nothing here knows the
 * shape of the head — this file places the mark, drops extra ink into its mouth field, and hands
 * the result to a rasteriser. Every number below is DERIVED from the mark's own boxes rather than
 * measured off a render, so editing the mark moves the family with it.
 *
 * That is the specific mistake this file exists not to repeat. It replaces `gen-tray-sync-icons.ts`,
 * which derived the sync family from the shipped idle PNG and carried constants measured off a
 * brand two brands ago. Those numbers were correct, and became meaningless the day the silhouette
 * changed, with nothing in the file able to notice.
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
 * The mark's viewBox: 44 units, which is the tray canvas, halves to 22px exactly
 * and is 1:1 at 44px. Both tray sizes therefore land on whole pixels for every
 * horizontal and vertical edge. The hexagon's four diagonals cannot land whole at
 * any size — see mark.svg for the trade and why every diagonal is slope 1:2 —
 * so partial pixels are not gated at zero here. They are gated at EXACT PINNED
 * COUNTS per glyph instead: the antialiasing the authored geometry produces is
 * deterministic, so any drift off the grid still changes a number and fails.
 */
const MARK_GRID = 44;
/** The cell's bounding box — x 6..38 by y 6..38, so 32 by 32. */
const MARK_BOX = { x0: 6, y0: 6, x1: 38, y1: 38 } as const;
/**
 * The hero's bounding box: the cell plus the antenna above it (stem to y -4,
 * tip hexagon to y -10.1, held as -10). Only the coloured tiles use it; no
 * template ever wears the antenna.
 */
const HERO_BOX = { x0: 6, y0: -10, x1: 38, y1: 38 } as const;
/**
 * The cell interior is ONE enclosed hole, and the eyes and mouth ink are islands
 * inside it. The armed ring adds a second. Below one means the ring has broken
 * open or the interior has filled in — the failure a bounding box cannot see.
 */
const MARK_HOLES = 1;

const MARK_W = MARK_BOX.x1 - MARK_BOX.x0;
const MARK_H = MARK_BOX.y1 - MARK_BOX.y0;

/** The four face states, in the vocabulary shared with the lamp component. */
type State = "live" | "idle" | "working" | "fault";

/**
 * The mouth field: the drawable region inside the cell the sync facts are drawn
 * on, the same surface the face states use. Its corners clear the inner
 * diagonals by at least a unit at every y it spans (verified in mark.svg's
 * comments edge by edge), so ink on this field reads as something IN the mouth
 * rather than the cell changing shape.
 */
const FIELD = { x0: 14, y0: 22, x1: 30, y1: 32 } as const;
const FIELD_CX = (FIELD.x0 + FIELD.x1) / 2;

// ---------------------------------------------------------------------------
// Tray canvas and placement.
//
// 44 units, because that is the @2x size of a 22pt menu-bar item — and also the
// mark's own grid, so the mark is authored at the size it is worn and the
// placement below comes out at (0, 0). The translate is kept, and asserted, so a
// future edit to MARK_BOX still lands somewhere legal.
//
// THE HEAD IS CENTRED AND IDENTICAL IN ALL TEN GLYPHS, and that is a hard
// requirement rather than tidiness. macOS centres a status-item image, so a glyph
// whose ink sits high renders high; if the states put their ink in different
// places the head visibly JUMPS the moment a sync starts, and a user cannot tell
// a state change from a glitch. This is also why the sync facts live in the
// mouth rather than in a corner badge: a badge outside the cell but inside its
// box has at most 2 units of room (the cell's cut corners are that thin), and a
// badge outside the box moves the head. The mouth is where the room is — the
// same conclusion the previous tag reached about its aperture.
// ---------------------------------------------------------------------------

const CANVAS = 44;
const DX = (CANVAS - MARK_W) / 2 - MARK_BOX.x0;
const DY = (CANVAS - MARK_H) / 2 - MARK_BOX.y0;

/**
 * The whole-pixel rule, asserted rather than commented. A half-unit or odd
 * translate would put every even coordinate in the artwork onto a half pixel at
 * 22px — and it would do it silently, because the glyph would still look right
 * at 44px.
 */
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
// Mouth ink for the sync facts.
//
// Everything below is drawn in the MARK's coordinate space and rides the same
// translate as the head, so these numbers can be read straight against mark.svg.
// All of it is additive over the idle face: the base mark is never re-cut, so
// the silhouette cannot drift between glyphs — and the eyes stay open in every
// one of them, because the eyes are the identity, not a state.
// ---------------------------------------------------------------------------

function rect(x: number, y: number, w: number, h: number): string {
  return `<rect x="${x}" y="${y}" width="${w}" height="${h}"/>`;
}

/**
 * A vertical arrow on the mouth field: `dir` -1 points up, +1 points down. A
 * 16-wide head — read by its ANGLE, so as wide as the field allows — and a 4x4
 * stem. The head's diagonals are the only mouth ink in the family that is not
 * axis-aligned; they ride on top of the ring's own pinned antialiasing.
 */
function arrow(dir: -1 | 1): string {
  const apexY = dir === -1 ? FIELD.y0 : FIELD.y1;
  const baseY = dir === -1 ? FIELD.y0 + 6 : FIELD.y1 - 6;
  const stemY = dir === -1 ? baseY : baseY - 4;
  return (
    `<path d="M${FIELD_CX} ${apexY}L${FIELD.x0} ${baseY}L${FIELD.x1} ${baseY}Z"/>` +
    rect(FIELD_CX - 2, stemY, 4, 4)
  );
}

/**
 * Armed — sync configured and healthy with nothing in flight: a hollow 12x8
 * rectangle, a 2-unit wall with the mouth showing through it. The lamp's own
 * idle is a hollow ring for the same reason: present, lit, nothing happening.
 */
const ARMED = `<path fill-rule="evenodd" d="M16 24H28V32H16Z M18 26H26V30H18Z"/>`;

/**
 * Paused — two 4x8 bars with a 4-unit gap, the universal pause, filling the
 * mouth the way the live core does so the two read as the same MASS in two
 * states: running solid, held apart.
 */
const PAUSED = rect(16, 24, 4, 8) + rect(24, 24, 4, 8);

/**
 * Warning — the mouth broken into a long piece and a short one: an exclamation
 * laid on its side. The 4-unit gap is the glyph: it is the only thing that
 * separates warning from live, and at 2px in the menu bar it is the widest gap
 * the field can afford while keeping the short piece 4 units.
 */
const WARNING = rect(14, 24, 8, 6) + rect(26, 24, 4, 6);

/**
 * Transferring both ways — two 6x4 blocks passing each other, rotationally
 * symmetric about the field's centre (22, 27). NOT two arrows: half-width
 * arrowheads at this size rasterise to smear, a floor two previous authors hit
 * independently. Two offset blocks carry the idea — two things moving, opposite
 * ways — on even edges.
 */
const TRANSFER = rect(16, 22, 6, 4) + rect(22, 28, 6, 4);

// ---------------------------------------------------------------------------
// The shipped family.
//
// The mouth carries the lamp state; the extra ink carries the fact the four
// states cannot express. One silhouette, one face, one field cover ten menu-bar
// conditions without any of them losing information: sync direction and
// paused-versus-warning still have their own pictures, they just no longer need
// their own brand.
// ---------------------------------------------------------------------------

type Glyph = { name: string; state: State; ink?: string; note: string };

const GLYPHS: Glyph[] = [
  { name: "tray-idle-template", state: "idle", note: "presence only, no sync configured" },
  { name: "tray-live-template", state: "live", note: "a recording is running" },
  { name: "tray-working-template", state: "working", note: "sync active, nothing on the wire" },
  { name: "tray-fault-template", state: "fault", note: "a failed session holds the tray" },
  { name: "tray-sync-template", state: "idle", ink: ARMED, note: "sync armed" },
  { name: "tray-sync-up-template", state: "idle", ink: arrow(-1), note: "uploading" },
  { name: "tray-sync-down-template", state: "idle", ink: arrow(1), note: "downloading" },
  {
    name: "tray-sync-updown-template",
    state: "idle",
    ink: TRANSFER,
    note: "transferring both ways",
  },
  { name: "tray-sync-paused-template", state: "idle", ink: PAUSED, note: "sync paused" },
  { name: "tray-sync-warning-template", state: "idle", ink: WARNING, note: "sync warning" },
];

/**
 * Partial pixels at 22px, PER GLYPH, pinned exactly. The ring's four 1:2
 * diagonals antialias identically in every glyph; the two arrows add their own
 * head diagonals on top. Exact equality — not a ceiling — so a coordinate that
 * drifts off the even grid changes a count and fails, which is the same defect
 * the old zero-gate caught, re-based onto a silhouette that legitimately owns
 * diagonals. Filled in from the measured run; a mismatch prints both numbers.
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

// ---------------------------------------------------------------------------
// SVG composition. mark.svg's <defs> block is lifted verbatim, so the geometry is
// never restated here — only referenced.
// ---------------------------------------------------------------------------

const markSource = readFileSync(MARK_SVG, "utf8");
const defs = /<defs>[\s\S]*<\/defs>/.exec(markSource)?.[0];
if (!defs) throw new Error(`${MARK_SVG} has no <defs> block to compose from`);
for (const state of ["live", "idle", "working", "fault", "hero"] as const) {
  if (!defs.includes(`id="mark-${state}"`)) {
    throw new Error(`${MARK_SVG} defines no #mark-${state}; the state vocabulary has drifted`);
  }
}

/** Wraps a body in an SVG that can see the mark's defs. The one composition seam. */
function svg(viewBox: string, body: string): string {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${viewBox}">${defs}${body}</svg>`;
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
 * The tile is GREEN and the mark is night-ground ink — the owner's approved
 * comps, held to.
 *
 * The green is the light theme's `--bridge-healthy`, and that is a reading of
 * the palette rather than a raid on it: the tile is a hive cell, and the one
 * thing this product's icon should radiate is "bridged, healthy, kept". It is
 * also keeper's original brand green, which the state palette inherited — so
 * the token is where that colour now lives, and the icon follows the token.
 * The mark is the dark theme's ground: the workroom's own near-black, the ink
 * tone the whole identity is drawn against. The ghosted neighbour cells are
 * the same ink at low opacity, which reads as a deeper green on the tile.
 */
const TILE_BG = tokenFromCss("bridge-healthy", ":root {");
const MARK_INK = tokenFromCss("background", ".dark {");
/** The neighbour cells' opacity: quiet shadow-cells, not a second colour. */
const LINE_OPACITY = 0.3;
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
 * The app icon: the hex-bot hero in night ink on the healthy green, with two
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
 * The hero is placed at 1:1 — a 32-wide cell plus antenna on a 64 tile needs no
 * scale, and every template-relevant edge stays on the unit grid.
 */
function appIcon(inset: number, rx: number): string {
  const heroW = HERO_BOX.x1 - HERO_BOX.x0;
  const heroH = HERO_BOX.y1 - HERO_BOX.y0;
  const tx = TILE / 2 - (HERO_BOX.x0 + heroW / 2);
  const ty = TILE / 2 - (HERO_BOX.y0 + heroH / 2);
  const clipId = `tile-${inset}-${rx}`;
  return svg(
    `0 0 ${TILE} ${TILE}`,
    `<clipPath id="${clipId}"><rect x="${inset}" y="${inset}" ` +
      `width="${TILE - inset * 2}" height="${TILE - inset * 2}" rx="${rx}"/></clipPath>` +
      `<rect x="${inset}" y="${inset}" width="${TILE - inset * 2}" ` +
      `height="${TILE - inset * 2}" rx="${rx}" fill="${TILE_BG}"/>` +
      `<g clip-path="url(#${clipId})" fill="none" stroke="${MARK_INK}" ` +
      `stroke-opacity="${LINE_OPACITY}" stroke-width="2">` +
      `<path d="${hexPath(7, 9, 13)}"/><path d="${hexPath(57, 56, 13)}"/></g>` +
      `<use href="#mark-hero" fill="${MARK_INK}" transform="translate(${tx} ${ty})"/>`,
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
// It must carry the accent. The failure this catches is not hypothetical: every
// other PNG this script writes is a pure-black template or an opaque RGB tile,
// and cutting the favicon from the wrong one would produce a file that looks
// plausible in a diff and is invisible on a dark GitHub page.
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
// itself and forbids alpha (see `appIcon`). `--ios-color` still names the ground
// so any pixel the CLI has to flatten lands on the workroom's colour rather than
// on its default white.
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
    `cell x${MARK_BOX.x0 + DX}..${MARK_BOX.x1 + DX} y${MARK_BOX.y0 + DY}..${MARK_BOX.y1 + DY}  ` +
    `mouth x${FIELD.x0}..${FIELD.x1} y${FIELD.y0}..${FIELD.y1} (mark space)\n`,
);

const report: string[][] = [];
let sharedBox: string | undefined;
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
  writeFileSync(
    source,
    svg(
      `0 0 ${CANVAS} ${CANVAS}`,
      // design-allow color: template images are tinted by macOS through their ALPHA
      // and their RGB is ignored, so the ink must be pure black. A format
      // requirement, not a palette choice.
      `<g fill="#000000" transform="translate(${DX} ${DY})">` +
        `<use href="#mark-${g.state}"/>${g.ink ?? ""}</g>`,
    ),
  );
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

  check(at1x.width === POINTS && at1x.height === POINTS, `${g.name} @1x is not ${POINTS}px`);
  check(at2x.width === RETINA && at2x.height === RETINA, `${g.name} @2x is not ${RETINA}px`);
  check(nonBlackPixels(at1x) === 0, `${g.name} @1x carries non-black RGB`);
  check(nonBlackPixels(at2x) === 0, `${g.name} @2x carries non-black RGB`);
  check(
    holes.length >= MARK_HOLES,
    `${g.name} @1x has ${holes.length} enclosed holes at ${POINTS}px, expected at least ` +
      `${MARK_HOLES} — the cell has broken open or filled in`,
  );
  check(
    m.partial === EXPECTED_PARTIAL[g.name],
    `${g.name} @1x has ${m.partial} partial pixels at ${POINTS}px, pinned at ` +
      `${EXPECTED_PARTIAL[g.name]} — some edge moved off the authored geometry`,
  );
  // The head must land in exactly the same pixels in every glyph, or it jumps in
  // the menu bar when the state changes.
  sharedBox ??= box;
  check(box === sharedBox, `${g.name} ink box ${box} differs from ${sharedBox}: the head moved`);
  alphaAt1x.push({
    name: g.name.replace(/^tray-|-template$/g, ""),
    alpha: at1x.pixels.filter((_, i) => i % 4 === 3),
  });

  report.push([
    g.name.replace(/^tray-|-template$/g, ""),
    g.state,
    g.ink ? "yes" : "-",
    `${holes.length}`,
    `${holes[0]?.area ?? 0}`,
    `${m.partial}`,
    g.note,
  ]);
}

const cols = [13, 8, 5, 5, 5, 7];
for (const row of [
  ["glyph", "mouth", "ink", "holes", "hole", "partial", "shown when"],
  ...report,
]) {
  console.log(row.map((c, i) => c.padEnd(cols[i] ?? 0)).join(" "));
}
console.log(
  `\nink box identical in all ${GLYPHS.length} glyphs: ${sharedBox} — the head never moves`,
);

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
// source every other file in this run is cut from. A tray glyph would only prove
// the composition still centres; the artwork is where a coordinate leaves the
// grid.
//
// 22 AND 44 ARE GATED AT PINNED PARTIAL-PIXEL COUNTS. On the rectilinear tag the
// pin was zero; a hexagon owns four diagonals and cannot be zero, so the pin is
// the exact count the authored 1:2 slopes produce. Exact equality keeps the
// property the zero-gate had: an edge that drifts off the even grid changes the
// count and fails. Filled in from the measured run.
//
// 16 IS REPORTED, NOT GATED. 44/16 = 0.3636 and nothing lands whole; nothing
// ships a 16px alpha-only template. The number is still printed because DESIGN.md
// rates this mark against vendor marks measured at 16px alpha-only, and a
// benchmark nobody prints is a benchmark nobody meets.
const EXPECTED_MARK_PARTIAL: Record<number, number> = { 22: 56, 44: 112 };
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
    holes.length === MARK_HOLES,
    `at ${size}px the mark has ${holes.length} enclosed holes, expected ${MARK_HOLES} (the cell)`,
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
console.log("\nall checks passed: pure black + alpha, cell enclosed, pinned rasters, head fixed");
