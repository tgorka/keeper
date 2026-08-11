#!/usr/bin/env bun
/**
 * Generates every raster the keeper mark is shipped as: the app icon set Tauri bundles, the macOS
 * tray TEMPLATE family the menu bar tints, the iOS AppIcon set, and `favicon.png` in the repo root.
 *
 * `src-tauri/crates/keeper/icons/mark.svg` is the only source of geometry. Nothing here knows the
 * shape of the head — this file places the mark, drops extra ink into its aperture, and hands the
 * result to a rasteriser. Every number below is DERIVED from the mark's own boxes rather than
 * measured off a render, so editing the mark moves the family with it.
 *
 * That is the specific mistake this file exists not to repeat. It replaces `gen-tray-sync-icons.ts`,
 * which derived the sync family from the shipped idle PNG and carried constants — a bubble centre
 * at 21.5/18.5, a ring radius of 6.2, a badge at 34.2/37.2 — measured off the OLD brand, a speech
 * bubble. Those numbers were correct, and became meaningless the day the silhouette changed, with
 * nothing in the file able to notice.
 *
 * RASTERISER: `tauri icon`, from the `@tauri-apps/cli` devDependency, which embeds resvg. It is the
 * official path, it takes SVG directly, and it is the only rasteriser on this Linux box — there is
 * no rsvg-convert, inkscape, ImageMagick, sharp or cairosvg here. It is deterministic, which is
 * what makes the committed PNGs reviewable in a diff.
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
 * and is 1:1 at 44px. Both tray sizes therefore land on whole pixels.
 *
 * It does NOT halve to 16px (44/16 = 0.3636), and that is a chosen trade rather
 * than an oversight — see mark.svg, which proves no grid can serve both. Nothing
 * ships a 16px alpha-only template; 16px is a colour app icon on a tile, where
 * antialiasing is correct. The run still measures and prints 16px, because a
 * number you have stopped looking at is a number you have stopped defending.
 */
const MARK_GRID = 44;
/** The tag's bounding box — x 6..38 by y 4..40, so 32 wide by 36 tall. */
const MARK_BOX = { x0: 6, y0: 4, x1: 38, y1: 40 } as const;
/** The aperture: the mark's state hole, and the surface every glyph's extra ink is drawn on. */
const APERTURE = { x0: 10, y0: 24, x1: 34, y1: 36 } as const;
/**
 * Eyelet, rule, aperture — the three holes the tag's identity lives in, and the
 * floor every glyph is held to. Extra ink can only ADD holes (the dashed
 * aperture makes five, the armed ring four); dropping below three means a hole
 * has filled in or leaked into the outside, which is the failure a bounding box
 * cannot see.
 */
const MARK_HOLES = 3;

const MARK_W = MARK_BOX.x1 - MARK_BOX.x0;
const MARK_H = MARK_BOX.y1 - MARK_BOX.y0;

/** The four aperture states, in the vocabulary shared with the lamp component. */
type State = "live" | "idle" | "working" | "fault";

/**
 * The drawable field inside the aperture, inset far enough that ink dropped here
 * never touches the aperture walls. The margin is what keeps a state readable as
 * something INSIDE a hole rather than as the hole changing shape — and at 2 units
 * it is a whole pixel at both tray sizes.
 */
const FIELD_INSET = 2;
const FIELD = {
  x0: APERTURE.x0 + FIELD_INSET,
  y0: APERTURE.y0 + FIELD_INSET,
  x1: APERTURE.x1 - FIELD_INSET,
  y1: APERTURE.y1 - FIELD_INSET,
} as const;
const FIELD_CX = (FIELD.x0 + FIELD.x1) / 2;

// ---------------------------------------------------------------------------
// Tray canvas and placement.
//
// 44 units, because that is the @2x size of a 22pt menu-bar item — and now also
// the mark's own grid, so the mark is authored at the size it is worn and the
// placement below comes out at (0, 0). That is the point of the re-author rather
// than a coincidence to be tidied away: while the artwork was 32 units and the
// canvas was 44, the mark was drawn 27% smaller than the surface it ships on and
// nothing in either file said so. The translate is kept, and asserted, so a
// future edit to MARK_BOX still lands somewhere legal.
//
// THE HEAD IS CENTRED AND IDENTICAL IN ALL TEN GLYPHS, and that is a hard
// requirement rather than tidiness. macOS centres a status-item image, so a glyph
// whose ink sits high renders high; if the bare states and the sync states put
// their ink in different places, the head visibly JUMPS the moment a sync starts,
// and a user cannot tell a state change from a glitch. Reserving an external
// corner for badges is what causes that, so the badges went inside the aperture
// instead — where, by the previous author's own measurement, they are also bigger.
// ---------------------------------------------------------------------------

const CANVAS = 44;
const DX = (CANVAS - MARK_W) / 2 - MARK_BOX.x0;
const DY = (CANVAS - MARK_H) / 2 - MARK_BOX.y0;

/**
 * The whole-pixel rule, asserted rather than commented. A half-unit or odd
 * translate would put every even coordinate in the artwork onto a half pixel at
 * 22px, which is the exact failure the 44-unit grid exists to remove — and it
 * would do it silently, because the glyph would still look right at 44px.
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
// Aperture ink.
//
// Everything below is drawn in the MARK's coordinate space and rides the same
// translate as the head, so these numbers can be read straight against mark.svg.
// All of it is additive: the base mark is never re-cut, so the silhouette cannot
// drift between glyphs.
// ---------------------------------------------------------------------------

function rect(x: number, y: number, w: number, h: number): string {
  return `<rect x="${x}" y="${y}" width="${w}" height="${h}"/>`;
}

/**
 * A vertical arrow filling the field: `dir` -1 points up, +1 points down. The
 * head stops 2 units short of the tail and a stem fills the rest.
 *
 * The triangle's diagonals are the only edges in the whole family that do not
 * land on the pixel grid, which is why these two glyphs are the only two that
 * report any partial pixels at all. A diagonal has to be antialiased or it has
 * to be a staircase, and at 3px tall a staircase is not a diagonal.
 *
 * The head is 16 units across on a 20-unit field — 8px at 22px, where the
 * 32-unit mark's was 6px. An arrowhead is the one glyph in the family that is
 * read by its ANGLE rather than by its area, and a wider base is a shallower,
 * more obviously directional angle at the same 3px of height.
 */
function arrow(dir: -1 | 1): string {
  const apexY = dir === -1 ? FIELD.y0 : FIELD.y1;
  const tailY = dir === -1 ? FIELD.y1 : FIELD.y0;
  const baseY = tailY + dir * 2;
  return (
    `<path d="M${FIELD_CX} ${apexY}L${FIELD_CX - 8} ${baseY}L${FIELD_CX + 8} ${baseY}Z"/>` +
    rect(FIELD_CX - 2, Math.min(baseY, tailY), 4, 2)
  );
}

/**
 * Armed — sync is configured and healthy with nothing in flight: the core drawn
 * HOLLOW, a 2-unit ring with the ground showing through it. The lamp's own idle
 * is a hollow ring for the same reason, so this is the vocabulary rather than a
 * new picture: present, lit, nothing happening.
 *
 * Static on purpose. The rotating frames this family used to carry said
 * "something is happening" and nothing about what, which is why the tray stopped
 * advancing a frame counter.
 */
const ARMED = `<path fill-rule="evenodd" d="M${FIELD.x0} ${FIELD.y0}H${FIELD.x1}V${FIELD.y1}H${FIELD.x0}Z M${FIELD.x0 + 2} ${FIELD.y0 + 2}H${FIELD.x1 - 2}V${FIELD.y1 - 2}H${FIELD.x0 + 2}Z"/>`;

/**
 * Paused — two bars, the universal pause: 6 wide, the field's full height, with a
 * 4-unit gap. Every edge even, so it is crisp at both sizes. The bars sit 2 in
 * from the field's ends rather than flush to them, because a pause read as two
 * bars needs the gap between them to be narrower than the bars, and flush bars on
 * a 20-unit field put the gap at 8.
 */
const PAUSED = rect(FIELD.x0 + 2, FIELD.y0, 6, 8) + rect(FIELD.x1 - 8, FIELD.y0, 6, 8);

/**
 * Warning — the core broken into a long piece and a short one. It is an
 * exclamation laid along the aperture, and more usefully it is the only state
 * whose ink is INTERRUPTED asymmetrically, so it cannot be confused with paused
 * (two equal bars) or with fault (one solid core with a bite).
 *
 * 10 + gap 6 + 4, not 12 + 4 + 4. The gap is the ONLY thing that distinguishes
 * warning from live, so the gap is the glyph: at 4 units it was 8 differing
 * pixels against `live` at 22px and the weakest pair in the family; at 6 it is 12
 * and it is not.
 */
const WARNING = rect(FIELD.x0, FIELD.y0, 10, 8) + rect(FIELD.x1 - 4, FIELD.y0, 4, 8);

/**
 * Transferring both ways — two blocks passing each other, one flush to the top of
 * the field and one flush to the bottom, offset so neither shares a column.
 *
 * NOT two arrows, and that was measured rather than preferred. Half-width arrows
 * put a 5px-wide triangle in the menu bar, which rasterises to a smear of partial
 * pixels that reads as noise rather than as two directions — the previous author
 * hit the same floor and said so about the corner badges. Two offset blocks carry
 * the same idea (two things moving, opposite ways) on even edges, so they are
 * crisp instead of grey, and the pair is rotationally symmetric, which is what
 * "both ways" looks like when there is no room to draw an arrowhead.
 */
const TRANSFER = rect(FIELD.x0, FIELD.y0, 8, 4) + rect(FIELD.x1 - 8, FIELD.y1 - 4, 8, 4);

// ---------------------------------------------------------------------------
// The shipped family.
//
// The aperture carries the lamp state; the extra ink carries the fact the four
// states cannot express. One silhouette and one hole cover ten menu-bar
// conditions without any of them losing information: sync direction and
// paused-versus-warning still have their own pictures, they just no longer need
// their own brand.
//
// `Active` deliberately has no extra ink. It used to get a circular-arrows badge
// meaning "something is happening", which is precisely what a dashed aperture
// already says, and a redundant mark at this size costs legibility for nothing.
// ---------------------------------------------------------------------------

/**
 * `diagonals` marks the glyphs whose ink cannot be whole-pixel. Exactly two carry
 * it, and they carry it because an arrowhead IS a diagonal — everything else in
 * the family is axis-aligned on even units and is gated at zero mush.
 */
type Glyph = { name: string; state: State; ink?: string; diagonals?: true; note: string };

const GLYPHS: Glyph[] = [
  { name: "tray-idle-template", state: "idle", note: "presence only, no sync configured" },
  { name: "tray-live-template", state: "live", note: "a recording is running" },
  { name: "tray-working-template", state: "working", note: "sync active, nothing on the wire" },
  { name: "tray-fault-template", state: "fault", note: "a failed session holds the tray" },
  { name: "tray-sync-template", state: "idle", ink: ARMED, note: "sync armed" },
  {
    name: "tray-sync-up-template",
    state: "idle",
    ink: arrow(-1),
    diagonals: true,
    note: "uploading",
  },
  {
    name: "tray-sync-down-template",
    state: "idle",
    ink: arrow(1),
    diagonals: true,
    note: "downloading",
  },
  {
    name: "tray-sync-updown-template",
    state: "idle",
    ink: TRANSFER,
    note: "transferring both ways",
  },
  { name: "tray-sync-paused-template", state: "idle", ink: PAUSED, note: "sync paused" },
  { name: "tray-sync-warning-template", state: "idle", ink: WARNING, note: "sync warning" },
];

// ---------------------------------------------------------------------------
// SVG composition. mark.svg's <defs> block is lifted verbatim, so the geometry is
// never restated here — only referenced.
// ---------------------------------------------------------------------------

const markSource = readFileSync(MARK_SVG, "utf8");
const defs = /<defs>[\s\S]*<\/defs>/.exec(markSource)?.[0];
if (!defs) throw new Error(`${MARK_SVG} has no <defs> block to compose from`);
for (const state of ["live", "idle", "working", "fault"] as State[]) {
  if (!defs.includes(`id="mark-${state}"`)) {
    throw new Error(`${MARK_SVG} defines no #mark-${state}; the state vocabulary has drifted`);
  }
}

/** Wraps a body in an SVG that can see the mark's defs. The one composition seam. */
function svg(viewBox: string, body: string): string {
  return `<svg xmlns="http://www.w3.org/2000/svg" viewBox="${viewBox}">${defs}${body}</svg>`;
}

/**
 * The app icon: the mark in lichen on the workroom's ground. An icon has a
 * container whether or not the design draws one, and drawing it is what stops the
 * mark floating in the dock.
 *
 * `inset` and `rx` differ by platform and both differences are required rather
 * than stylistic. Desktop draws its own rounded tile with clear space around it,
 * because macOS and Windows show the bitmap as authored. iOS gets a FULL-BLEED
 * square with square corners: the system applies its own superellipse mask, so a
 * rounded tile inside it reads as a rounded icon pasted on another rounded icon —
 * and, more bluntly, an iOS app icon may carry no alpha at all, which a rounded
 * tile's antialiased corners violate by construction.
 *
 * The mark is placed at 1:1 rather than scaled. On the 32-unit grid it had to be
 * blown up 1.25x to fill a 64-unit tile, and that scale was the one place in this
 * file that left the whole-pixel grid. A 44-unit tag on a 64-unit tile needs no
 * scale at all: 32x36 inside a 56x56 inner tile is already the proportion an app
 * icon wants, and every edge stays on an integer at every size the set is cut to.
 */
/**
 * The icon's two colours are READ from `src/index.css` rather than repeated here.
 *
 * This is not tidiness. `tray.rs` shipped `#3ecfae` — the teal DESIGN.md rejects
 * by name — as the Linux/Windows tray repaint colour, because a copy of the
 * palette in native code has no way of hearing that the palette changed. A
 * generator that hardcodes the accent is the same bug with a longer fuse: the
 * next person to retheme the app would change the CSS, rebuild, and ship an icon
 * still wearing the old green.
 *
 * The dark theme is the source: the icon is a lit mark on a dark tile in both
 * appearances, because an app icon has no theme — it sits on whatever the user's
 * dock or Finder happens to be.
 */
function tokenFromCss(name: string): string {
  const css = readFileSync("src/index.css", "utf8");
  const darkBlock = css.slice(css.indexOf(".dark {"));
  const match = darkBlock.match(new RegExp(`--${name}:\\s*(#[0-9a-fA-F]{3,8})\\s*;`));
  if (!match) {
    throw new Error(`--${name} is not defined in the .dark block of src/index.css`);
  }
  return match[1];
}

const ACCENT = tokenFromCss("primary");
const GROUND = tokenFromCss("background");
const TILE = 64;
const TILE_INSET = 4;
const APP_SCALE = 1;
/**
 * `favicon.png` in the REPO ROOT — the same coloured tile as the desktop icon,
 * cut once, large.
 *
 * 1024 because it is the only size that is a DOWNSCALE for every consumer that
 * reads it. The repo's largest existing raster is `icons/icon.png` at 512, a file
 * browser's biggest thumbnail is 256, and GitHub's social-preview slot is
 * 1280x640 with a 640x320 floor — so a 512 square, the obvious choice, would be
 * the one that has to be enlarged, and enlarging hard flat edges fringes them.
 * 1024 is exactly twice the set's existing top size rather than a new scale.
 */
const FAVICON = "favicon.png";
const FAVICON_SIZE = 1024;

function appIcon(inset: number, rx: number): string {
  const tx = TILE / 2 - APP_SCALE * (MARK_BOX.x0 + MARK_W / 2);
  const ty = TILE / 2 - APP_SCALE * (MARK_BOX.y0 + MARK_H / 2);
  return svg(
    `0 0 ${TILE} ${TILE}`,
    `<rect x="${inset}" y="${inset}" width="${TILE - inset * 2}" ` +
      `height="${TILE - inset * 2}" rx="${rx}" fill="${GROUND}"/>` +
      `<use href="#mark-idle" fill="${ACCENT}" ` +
      `transform="translate(${tx} ${ty}) scale(${APP_SCALE})"/>`,
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
console.log(`app icon set  <- ${MARK_SVG}, mark ${APP_SCALE}x in ${ACCENT} on ${GROUND}`);

// --- favicon.png, in the repo root ------------------------------------------
//
// Cut from the SAME desktop tile source rather than drawn again. The owner asked
// for the project to be identifiable at a glance in a file browser and on GitHub;
// a second drawing of the mark to satisfy that is precisely how the retired
// `gen-ios-icons.swift` drifted from this file without anything noticing.
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
const accentRgb = [1, 3, 5].map((i) => Number.parseInt(ACCENT.slice(i, i + 2), 16));
let accentPixels = 0;
for (let i = 0; i < favicon.pixels.length; i += 4) {
  if (
    favicon.pixels[i] === accentRgb[0] &&
    favicon.pixels[i + 1] === accentRgb[1] &&
    favicon.pixels[i + 2] === accentRgb[2] &&
    favicon.pixels[i + 3] === 255
  ) {
    accentPixels++;
  }
}
check(
  accentPixels > 0,
  `${FAVICON} carries no ${ACCENT} pixel — it was cut from a template, not from the coloured tile`,
);
console.log(
  `${FAVICON}  ${FAVICON_SIZE}px, the desktop tile, ${accentPixels} px of ${ACCENT} ` +
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
execFileSync(TAURI_CLI, ["icon", iosSvg, "-o", iosSet, "--ios-color", GROUND], {
  stdio: ["ignore", "ignore", "pipe"],
});

// The filenames and pixel sizes are fixed by `AppIcon.appiconset/Contents.json`,
// and the CLI emits exactly that set — which is what retired
// `gen-ios-icons.swift`, a second, hand-coded CoreGraphics drawing of the mark
// that only ran on macOS and could drift from this one without anything noticing.
//
// The one thing that script did which the CLI does not: it rendered into an RGB
// context so the files carry NO ALPHA CHANNEL. Apple rejects an app icon for
// HAVING that channel, not for using it, so a fully opaque RGBA icon still fails
// — which makes the re-encode below a contract this set had before and must keep.
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
console.log(`iOS AppIcon set  ${iosCount} files on ${GROUND}, RGB with no alpha channel`);

// --- the tray template family ----------------------------------------------
const RETINA = CANVAS;
const POINTS = CANVAS / 2;
console.log(
  `\ntray templates  ${POINTS}px @1x / ${RETINA}px @2x  ` +
    `head x${MARK_BOX.x0 + DX}..${MARK_BOX.x1 + DX} y${MARK_BOX.y0 + DY}..${MARK_BOX.y1 + DY}  ` +
    `field x${FIELD.x0}..${FIELD.x1} y${FIELD.y0}..${FIELD.y1} (mark space)\n`,
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
  // A copy rather than a rename: the scratch dir is under /tmp, which is a
  // different device here, and rename cannot cross one.
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
      `${MARK_HOLES} — a hole has filled in or leaked into the outside`,
  );
  // Zero partial pixels, because on the 44-unit grid 22px is an exact half and
  // every coordinate is even. The two arrows are exempt BY NAME rather than by a
  // relaxed threshold: a triangle has diagonals, diagonals cannot be whole
  // pixels, and a cap that let any glyph mush a little would let all of them.
  if (g.diagonals) {
    check(
      m.partial <= 12,
      `${g.name} @1x has ${m.partial} partial pixels; its arrowhead's diagonals should cost at most 12`,
    );
  } else {
    check(
      m.partial === 0,
      `${g.name} @1x has ${m.partial} partial pixels at ${POINTS}px, expected none — ` +
        `some edge is not on an even unit`,
    );
  }
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
    `${((m.partial / m.all) * 100).toFixed(1)}%`,
    g.note,
  ]);
}

const cols = [13, 8, 5, 5, 5, 6];
for (const row of [
  ["glyph", "aperture", "ink", "holes", "hole", "mush", "shown when"],
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
// than just asserted non-zero. On the 32-unit mark this family passed the
// non-zero test while `live` and `fault` differed by three pixels out of 484 —
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

// --- the whole-pixel proof, and the 16px report -----------------------------
//
// Run on mark.svg itself rather than on a tray glyph, because mark.svg is the
// source every other file in this run is cut from. A tray glyph would only prove
// the composition still centres; the artwork is where a coordinate leaves the
// grid.
//
// 22 AND 44 ARE GATED AT ZERO PARTIAL PIXELS. The grid divides both exactly, so
// zero is the only correct answer and anything else is geometry that has left the
// grid. This gate is new, and it is the one this whole re-author is for: the old
// run asserted a whole-pixel BBOX, which the 32-unit mark passed at 16px and 32px
// while rendering 21.7% mush at 22px — the size it is actually worn at — because
// nothing ever rasterised it there. A bounding box cannot see the inside of a
// drawing.
//
// 16 IS REPORTED, NOT GATED, and that is a decision rather than an omission.
// 44/16 = 0.3636 and nothing lands whole; mark.svg shows why no grid can serve 16
// and 22 at once, and 22 is the one that ships. The number is still printed
// because DESIGN.md rates this mark against vendor marks measured at 16px
// alpha-only, and a benchmark nobody prints is a benchmark nobody meets.
const proofDir = join(work, "proof");
const proofSizes = [16, POINTS, RETINA];
rasterise(MARK_SVG, proofSizes, proofDir);
console.log("\nmark.svg alpha-only (no colour, no tile)   * = gated, the grid divides it exactly");
for (const size of proofSizes) {
  // Every coordinate in the artwork is even, so the finest edge it can carry is
  // 2 units; the raster is whole-pixel exactly when 2 units is a whole pixel.
  const wholePixel = (2 * size) % MARK_GRID === 0;
  const png = readPng(join(proofDir, `${size}x${size}.png`));
  const m = measure(png);
  const holes = enclosedHoles(png);
  const w = m.box[2] - m.box[0] + 1;
  const h = m.box[3] - m.box[1] + 1;
  console.log(
    `  ${wholePixel ? "*" : " "} ${String(size).padStart(2)}px  ` +
      `mush ${((m.partial / m.all) * 100).toFixed(1)}% of all / ` +
      `${((m.partial / m.ink) * 100).toFixed(1)}% of inked  (${m.partial} partial, ${m.ink} inked)  ` +
      `bbox ${w}x${h}  holes ${holes.length}  ` +
      `[${holes.map((x) => `${x.box[2] - x.box[0] + 1}x${x.box[3] - x.box[1] + 1}=${x.area}px`).join(" ") || "NONE"}]`,
  );
  if (!wholePixel) continue;
  check(m.partial === 0, `at ${size}px the mark has ${m.partial} partial pixels, expected none`);
  check(
    holes.length === MARK_HOLES,
    `at ${size}px the mark has ${holes.length} enclosed holes, expected ${MARK_HOLES} ` +
      `(eyelet, rule, aperture)`,
  );
  check(
    w === (MARK_W * size) / MARK_GRID && h === (MARK_H * size) / MARK_GRID,
    `at ${size}px the mark's bbox is ${w}x${h}, not the grid's ` +
      `${(MARK_W * size) / MARK_GRID}x${(MARK_H * size) / MARK_GRID} — geometry has left whole pixels`,
  );
}

rmSync(work, { recursive: true, force: true });

if (failures.length) {
  console.error(`\n${failures.length} check(s) FAILED:`);
  for (const f of failures) console.error(`  - ${f}`);
  process.exit(1);
}
console.log(
  "\nall checks passed: pure black + alpha, holes intact, whole-pixel geometry, head fixed",
);
