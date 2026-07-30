#!/usr/bin/env bun
/**
 * Generates the sync tray glyph set (Story 29.2, AD-51).
 *
 * The three original tray glyphs (`tray-{idle,recording,error}-template.png`)
 * were committed as opaque binaries with no generator, so nobody could produce
 * a matching variant without redrawing the brand mark by hand. This script
 * fixes that for the sync set: it reads the shipped idle glyph, keeps its
 * speech-bubble outline pixel-for-pixel, and composites a mark into the empty
 * interior. Brand consistency is therefore mechanical, not a matter of care.
 *
 * Constraints that shape every decision here:
 *
 *  - macOS template images MUST be monochrome + alpha. `icon_as_template(true)`
 *    makes the system recolour them for light/dark menu bars, so **colour
 *    carries no information** — state must be legible from SHAPE alone
 *    (recorded in epic-21-context.md, and re-asserted by AD-51).
 *  - The existing glyphs are 44x44 RGBA8. A test in `tray.rs` asserts every
 *    state icon decodes at identical dimensions, so the output size is fixed.
 *  - State must be legible at the retina downscale to ~22 px, which is the
 *    binding constraint on every size below: a mark whose stroke lands under
 *    ~1.5 px merges into its neighbours and stops carrying information.
 *
 * Run: bun run scripts/gen-tray-sync-icons.ts
 */

import { readFileSync, writeFileSync } from "node:fs";
import { deflateSync, inflateSync } from "node:zlib";

const SIZE = 44;
const BYTES_PER_PIXEL = 4;
const ICON_DIR = "src-tauri/crates/keeper/icons";
const SOURCE_GLYPH = `${ICON_DIR}/tray-idle-template.png`;

/** 4x supersampling: enough to keep a 1.5 px stroke smooth at 44 px. */
const SUPERSAMPLE = 4;

/**
 * Bubble interior, measured from the shipped idle glyph. The mark must clear
 * the outline stroke or the two shapes visually merge into a blob at menu-bar
 * size.
 */
const CENTER_X = 21.5;
const CENTER_Y = 18.5;
const RING_RADIUS = 6.2;
const STROKE = 1.6;

type Rgba = Uint8Array;

// ---------------------------------------------------------------------------
// Minimal PNG read/write. Only the subset the shipped glyphs actually use:
// 8-bit RGBA, no interlace, no palette.
// ---------------------------------------------------------------------------

function crc32(buf: Uint8Array): number {
  let c = ~0;
  for (let i = 0; i < buf.length; i++) {
    c ^= buf[i];
    for (let k = 0; k < 8; k++) c = (c >>> 1) ^ (0xedb88320 & -(c & 1));
  }
  return ~c >>> 0;
}

function chunk(type: string, data: Uint8Array): Uint8Array {
  const out = new Uint8Array(12 + data.length);
  const view = new DataView(out.buffer);
  view.setUint32(0, data.length);
  for (let i = 0; i < 4; i++) out[4 + i] = type.charCodeAt(i);
  out.set(data, 8);
  view.setUint32(8 + data.length, crc32(out.subarray(4, 8 + data.length)));
  return out;
}

function decodePng(bytes: Uint8Array): { width: number; height: number; pixels: Rgba } {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let pos = 8;
  let width = 0;
  let height = 0;
  const idat: Uint8Array[] = [];
  while (pos < bytes.length) {
    const len = view.getUint32(pos);
    const type = String.fromCharCode(...bytes.subarray(pos + 4, pos + 8));
    const data = bytes.subarray(pos + 8, pos + 8 + len);
    if (type === "IHDR") {
      width = view.getUint32(pos + 8);
      height = view.getUint32(pos + 12);
      const bitDepth = bytes[pos + 16];
      const colorType = bytes[pos + 17];
      if (bitDepth !== 8 || colorType !== 6) {
        throw new Error(`expected 8-bit RGBA, got depth ${bitDepth} colour type ${colorType}`);
      }
    } else if (type === "IDAT") {
      idat.push(new Uint8Array(data));
    }
    pos += 12 + len;
  }
  const raw = new Uint8Array(inflateSync(Buffer.concat(idat.map((c) => Buffer.from(c)))));
  const stride = width * BYTES_PER_PIXEL;
  const pixels = new Uint8Array(height * stride);
  let src = 0;
  for (let y = 0; y < height; y++) {
    const filter = raw[src++];
    const line = raw.subarray(src, src + stride);
    src += stride;
    const row = pixels.subarray(y * stride, (y + 1) * stride);
    const prior = y > 0 ? pixels.subarray((y - 1) * stride, y * stride) : new Uint8Array(stride);
    for (let x = 0; x < stride; x++) {
      const a = x >= BYTES_PER_PIXEL ? row[x - BYTES_PER_PIXEL] : 0;
      const b = prior[x];
      const c = x >= BYTES_PER_PIXEL ? prior[x - BYTES_PER_PIXEL] : 0;
      let add = 0;
      if (filter === 1) add = a;
      else if (filter === 2) add = b;
      else if (filter === 3) add = (a + b) >> 1;
      else if (filter === 4) {
        const p = a + b - c;
        const pa = Math.abs(p - a);
        const pb = Math.abs(p - b);
        const pc = Math.abs(p - c);
        add = pa <= pb && pa <= pc ? a : pb <= pc ? b : c;
      }
      row[x] = (line[x] + add) & 0xff;
    }
  }
  return { width, height, pixels };
}

function encodePng(width: number, height: number, pixels: Rgba): Uint8Array {
  const stride = width * BYTES_PER_PIXEL;
  // Filter type 0 on every row: these glyphs are tiny and mostly transparent,
  // so deflate already gets them under 1 KB and adaptive filtering would only
  // make the output non-reproducible across zlib versions.
  const raw = new Uint8Array(height * (stride + 1));
  for (let y = 0; y < height; y++) {
    raw[y * (stride + 1)] = 0;
    raw.set(pixels.subarray(y * stride, (y + 1) * stride), y * (stride + 1) + 1);
  }
  const ihdr = new Uint8Array(13);
  const view = new DataView(ihdr.buffer);
  view.setUint32(0, width);
  view.setUint32(4, height);
  ihdr[8] = 8;
  ihdr[9] = 6;
  const deflated = new Uint8Array(deflateSync(Buffer.from(raw), { level: 9 }));
  const parts = [
    new Uint8Array([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", deflated),
    chunk("IEND", new Uint8Array(0)),
  ];
  const total = parts.reduce((n, p) => n + p.length, 0);
  const out = new Uint8Array(total);
  let at = 0;
  for (const p of parts) {
    out.set(p, at);
    at += p.length;
  }
  return out;
}

// ---------------------------------------------------------------------------
// Mark rendering. Everything is a signed-distance test evaluated on a
// supersampled grid, which keeps strokes even without a rasteriser dependency.
// ---------------------------------------------------------------------------

type Coverage = (x: number, y: number) => boolean;

/** Ring with a gap, used for the rotating activity frames. */
function arc(gapStartDeg: number, gapSweepDeg: number): Coverage {
  return (x, y) => {
    const dx = x - CENTER_X;
    const dy = y - CENTER_Y;
    const dist = Math.hypot(dx, dy);
    if (Math.abs(dist - RING_RADIUS) > STROKE / 2) return false;
    // Screen coordinates put +y downward, so negate to get a conventional
    // counter-clockwise angle; the gap then advances visually clockwise.
    let deg = (Math.atan2(-dy, dx) * 180) / Math.PI;
    if (deg < 0) deg += 360;
    let rel = deg - gapStartDeg;
    if (rel < 0) rel += 360;
    return rel > gapSweepDeg;
  };
}

/** Filled disc, for the arrow heads that make the ring read as a cycle. */
function disc(cx: number, cy: number, r: number): Coverage {
  return (x, y) => Math.hypot(x - cx, y - cy) <= r;
}

/** Axis-aligned rounded bar, for the pause mark. */
function bar(cx: number, cy: number, halfW: number, halfH: number): Coverage {
  return (x, y) => Math.abs(x - cx) <= halfW && Math.abs(y - cy) <= halfH;
}

function union(...shapes: Coverage[]): Coverage {
  return (x, y) => shapes.some((s) => s(x, y));
}

// ---------------------------------------------------------------------------
// Where the direction / refresh mark goes.
//
// `corner` is the requested design: bottom-RIGHT, outside the bubble. That
// quadrant is genuinely free — the outline stops at x=38,y=30 and the tail hangs
// off the bottom-*left* — so the mark collides with nothing and needs no notch
// punched out of the brand shape. Every number below is measured off the shipped
// glyph's alpha map, not guessed.
//
// `interior` is the same marks drawn centred in the bubble, where the existing
// state glyphs live.
//
// The trade is legibility, and it is not close. A menu bar renders these at
// ~22 px, so the free corner region is about 6 px across and an arrow inside it
// lands near 3 px — under the ~1.5 px stroke floor this file already warns about
// twice, and in a 22 px downscale the three corner marks are hard to tell apart.
// Centred, the same arrows get ~12x8 px and read cleanly. Both sets are kept
// because that is a product call, not a technical one: flip `PLACEMENT` and
// re-run. Renders of both are attached to the PR that introduced this.
// ---------------------------------------------------------------------------

type Placement = "corner" | "interior";

/** The shipped placement. See the trade-off note above before changing it. */
const PLACEMENT: Placement = "corner";

/**
 * Geometry per placement. The corner set is scaled down because the free region
 * is smaller, not because the marks want to be — which is the whole legibility
 * problem in one comment.
 */
const GEOMETRY = {
  corner: {
    cx: 34.2,
    cy: 37.2,
    // No notch: the mark sits clear of the outline rather than biting into it.
    // An earlier revision punched a moat here and ate the bubble's whole bottom
    // edge, which read as damage rather than as a badge.
    notch: 0,
    arrowHalfH: 4.8,
    stemHalfW: 0.9,
    headHalfW: 2.3,
    headH: 3.1,
    pairDx: 2.9,
    refreshR: 4.0,
    refreshHeadLen: 2.6,
    refreshHeadHalfW: 1.5,
  },
  interior: {
    cx: CENTER_X,
    cy: CENTER_Y,
    notch: 0,
    arrowHalfH: 6.6,
    stemHalfW: 1.15,
    headHalfW: 3.4,
    headH: 3.6,
    pairDx: 3.9,
    refreshR: 5.2,
    refreshHeadLen: 3.3,
    refreshHeadHalfW: 1.9,
  },
} as const satisfies Record<Placement, unknown>;

const G = GEOMETRY[PLACEMENT];

const BADGE_CX = G.cx;
const BADGE_CY = G.cy;

/**
 * Transparent moat between the mark and the bubble outline, when the mark
 * overlaps it at all.
 *
 * Zero for both shipped placements — `corner` clears the outline by geometry and
 * `interior` draws inside an interior `clearInterior` has already emptied. Kept
 * because a placement that *does* overlap needs it: without a moat the two
 * shapes merge into one blob at menu-bar size, the same failure `clearInterior`
 * exists to prevent.
 */
const NOTCH_R = G.notch;

/** Punch the badge's moat out of the base glyph. */
function clearDisc(base: Rgba, cx: number, cy: number, r: number): Rgba {
  const out = new Uint8Array(base);
  for (let y = 0; y < SIZE; y++) {
    for (let x = 0; x < SIZE; x++) {
      if (Math.hypot(x - cx, y - cy) <= r) {
        out[(y * SIZE + x) * BYTES_PER_PIXEL + 3] = 0;
      }
    }
  }
  return out;
}

/**
 * Isosceles triangle from three vertices, for arrow heads.
 *
 * Half-plane sign test rather than barycentric coordinates: the vertices are
 * authored counter-clockwise in screen space, so all three cross products share
 * a sign for an interior point.
 */
function triangle(
  ax: number,
  ay: number,
  bx: number,
  by: number,
  cx: number,
  cy: number,
): Coverage {
  const side = (px: number, py: number, x1: number, y1: number, x2: number, y2: number) =>
    (x2 - x1) * (py - y1) - (y2 - y1) * (px - x1);
  return (x, y) => {
    const s1 = side(x, y, ax, ay, bx, by);
    const s2 = side(x, y, bx, by, cx, cy);
    const s3 = side(x, y, cx, cy, ax, ay);
    return (s1 >= 0 && s2 >= 0 && s3 >= 0) || (s1 <= 0 && s2 <= 0 && s3 <= 0);
  };
}

/** Arrow geometry, sized so a pair still separates at menu-bar scale. */
const ARROW_HALF_H = G.arrowHalfH;
const ARROW_STEM_HALF_W = G.stemHalfW;
const ARROW_HEAD_HALF_W = G.headHalfW;
const ARROW_HEAD_H = G.headH;

/** Horizontal offset of each arrow in the two-arrow (both-directions) badge. */
const ARROW_PAIR_DX = G.pairDx;

/**
 * Vertical arrow: a stem from the tail to the head's base, plus the head.
 *
 * `dir` is -1 for up (screen +y points down), +1 for down. The stem deliberately
 * stops where the head begins — running it on to the tip fattens the point and
 * costs the arrow its direction at 22 px.
 */
function arrow(cx: number, cy: number, dir: -1 | 1): Coverage {
  const tailY = cy - dir * ARROW_HALF_H;
  const tipY = cy + dir * ARROW_HALF_H;
  const headBaseY = tipY - dir * ARROW_HEAD_H;
  return union(
    bar(cx, (tailY + headBaseY) / 2, ARROW_STEM_HALF_W, Math.abs(headBaseY - tailY) / 2),
    triangle(cx, tipY, cx - ARROW_HEAD_HALF_W, headBaseY, cx + ARROW_HEAD_HALF_W, headBaseY),
  );
}

/** Radius of the refresh mark's two arcs. */
const REFRESH_R = G.refreshR;

/**
 * A tangential arrow head sitting at one end of an arc.
 *
 * The base is the arc's radial cross-section at `angleDeg` and the tip runs
 * along the tangent, which is what makes the head read as continuing the curve
 * rather than as a blob stuck to it. `sign` picks which way round: +1 follows
 * increasing angle.
 *
 * Screen +y points down, so the radial unit vector negates y and the tangent is
 * its derivative in that same flipped frame.
 */
function arcHead(angleDeg: number, r: number, sign: -1 | 1): Coverage {
  const a = (angleDeg * Math.PI) / 180;
  const px = BADGE_CX + Math.cos(a) * r;
  const py = BADGE_CY - Math.sin(a) * r;
  const tx = -Math.sin(a) * sign;
  const ty = -Math.cos(a) * sign;
  const nx = Math.cos(a);
  const ny = -Math.sin(a);
  return triangle(
    px + tx * REFRESH_HEAD_LEN,
    py + ty * REFRESH_HEAD_LEN,
    px + nx * REFRESH_HEAD_HALF_W,
    py + ny * REFRESH_HEAD_HALF_W,
    px - nx * REFRESH_HEAD_HALF_W,
    py - ny * REFRESH_HEAD_HALF_W,
  );
}

/**
 * Head geometry for the refresh mark.
 *
 * Deliberately wider than the arc stroke: a head flush with the stroke leaves a
 * plain ring, and a plain ring is exactly the *armed* glyph. The two states have
 * to differ by silhouette, so the heads must visibly protrude.
 */
const REFRESH_HEAD_LEN = G.refreshHeadLen;
const REFRESH_HEAD_HALF_W = G.refreshHeadHalfW;

/**
 * Circular-arrow refresh mark: two arcs, each ending in a tangential head, so
 * the mark reads as a cycle in motion.
 *
 * Two arcs and not one: a single gapped ring is what the *armed* glyph already
 * is, and the two must not share a silhouette. The heads are what carry that
 * difference, which is why they are sized against the armed ring rather than
 * against the arc they terminate.
 */
function refreshBadge(): Coverage {
  const strokeHalf = STROKE / 2 + 0.15;
  const shell =
    (from: number, to: number): Coverage =>
    (x, y) => {
      const dx = x - BADGE_CX;
      const dy = y - BADGE_CY;
      if (Math.abs(Math.hypot(dx, dy) - REFRESH_R) > strokeHalf) return false;
      let deg = (Math.atan2(-dy, dx) * 180) / Math.PI;
      if (deg < 0) deg += 360;
      return from <= to ? deg >= from && deg <= to : deg >= from || deg <= to;
    };
  // Two arcs a half turn apart, each stopping short so its head has somewhere
  // to go. The gaps sit left and right rather than top and bottom: a horizontal
  // break is what distinguishes this from the armed ring's single top gap.
  return union(
    shell(30, 150),
    arcHead(150, REFRESH_R, 1),
    shell(210, 330),
    arcHead(330, REFRESH_R, 1),
  );
}

/**
 * Composite a mark onto a copy of the base glyph.
 *
 * Template images are black + alpha, so the mark only ever raises alpha —
 * writing a colour would be silently discarded by the system's template
 * recolouring and would make the intent unclear to the next reader.
 */
function withMark(base: Rgba, mark: Coverage): Rgba {
  const out = new Uint8Array(base);
  const step = 1 / SUPERSAMPLE;
  const samples = SUPERSAMPLE * SUPERSAMPLE;
  for (let y = 0; y < SIZE; y++) {
    for (let x = 0; x < SIZE; x++) {
      let hits = 0;
      for (let sy = 0; sy < SUPERSAMPLE; sy++) {
        for (let sx = 0; sx < SUPERSAMPLE; sx++) {
          if (mark(x + (sx + 0.5) * step, y + (sy + 0.5) * step)) hits++;
        }
      }
      if (hits === 0) continue;
      const alpha = Math.round((hits / samples) * 255);
      const i = (y * SIZE + x) * BYTES_PER_PIXEL;
      out[i] = 0;
      out[i + 1] = 0;
      out[i + 2] = 0;
      out[i + 3] = Math.max(out[i + 3], alpha);
    }
  }
  return out;
}

/** Strip the bubble's interior so a mark never collides with leftover pixels. */
function clearInterior(base: Rgba): Rgba {
  const out = new Uint8Array(base);
  for (let y = 0; y < SIZE; y++) {
    for (let x = 0; x < SIZE; x++) {
      if (Math.hypot(x - CENTER_X, y - CENTER_Y) <= RING_RADIUS + STROKE) {
        out[(y * SIZE + x) * BYTES_PER_PIXEL + 3] = 0;
      }
    }
  }
  return out;
}

const source = decodePng(new Uint8Array(readFileSync(SOURCE_GLYPH)));
if (source.width !== SIZE || source.height !== SIZE) {
  throw new Error(`source glyph is ${source.width}x${source.height}, expected ${SIZE}x${SIZE}`);
}
const base = clearInterior(source.pixels);

/** Small head that turns a plain ring into a directional cycle. */
function head(angleDeg: number): Coverage {
  const rad = (angleDeg * Math.PI) / 180;
  return disc(CENTER_X + Math.cos(rad) * RING_RADIUS, CENTER_Y - Math.sin(rad) * RING_RADIUS, 1.7);
}

const GAP_SWEEP = 70;

/**
 * The badge states, drawn on a base whose bottom-right corner is notched out.
 *
 * These four replaced the four rotating `sync-N` frames. The frames animated a
 * ring gap a quarter turn per tick, which said "something is happening" and
 * nothing about *what* — so a 40 GB upload and a directory scan were the same
 * picture. A direction reads off the badge with no animation at all, which is
 * also why the tray no longer needs a frame counter.
 */
const badged = NOTCH_R > 0 ? clearDisc(base, BADGE_CX, BADGE_CY, NOTCH_R) : base;

const variants: Array<{ name: string; mark: Coverage; on?: Rgba }> = [
  // Armed: a complete cycle, static — sync is configured and healthy.
  { name: "tray-sync-template", mark: union(arc(90, GAP_SWEEP), head(90), head(270)) },
  // Uploading: one arrow up. The bubble interior stays empty, so the badge is
  // the only mark and reads without competition.
  {
    name: "tray-sync-up-template",
    mark: arrow(BADGE_CX, BADGE_CY, -1),
    on: badged,
  },
  // Downloading: the same arrow, mirrored.
  {
    name: "tray-sync-down-template",
    mark: arrow(BADGE_CX, BADGE_CY, 1),
    on: badged,
  },
  // Both at once: two arrows side by side, the universal transfer glyph. Up on
  // the left so the pair reads in the same order as the words.
  {
    name: "tray-sync-updown-template",
    mark: union(
      arrow(BADGE_CX - ARROW_PAIR_DX, BADGE_CY, -1),
      arrow(BADGE_CX + ARROW_PAIR_DX, BADGE_CY, 1),
    ),
    on: badged,
  },
  // Working with nothing on the wire: circular arrows, the refresh glyph.
  {
    name: "tray-sync-refresh-template",
    mark: refreshBadge(),
    on: badged,
  },
  // Paused / media absent: two bars. Unambiguous at 44 px and shares no
  // silhouette with the badge states.
  {
    name: "tray-sync-paused-template",
    mark: union(bar(CENTER_X - 2.6, CENTER_Y, 1.1, 4.4), bar(CENTER_X + 2.6, CENTER_Y, 1.1, 4.4)),
  },
  // Warning: an exclamation inside the OUTLINED bubble. The recording error
  // glyph is a FILLED bubble with a punched-out exclamation, so the two remain
  // distinguishable by silhouette rather than by colour.
  {
    name: "tray-sync-warning-template",
    // The stem/dot gap has to survive template recolouring and the retina
    // downscale to ~22 px; below ~1.5 px the two merge into one bar and the
    // mark stops reading as an exclamation.
    mark: union(bar(CENTER_X, CENTER_Y - 1.8, 1.2, 3.2), disc(CENTER_X, CENTER_Y + 4.6, 1.4)),
  },
];

for (const { name, mark, on } of variants) {
  const png = encodePng(SIZE, SIZE, withMark(on ?? base, mark));
  writeFileSync(`${ICON_DIR}/${name}.png`, png);
  console.log(`${name}.png  ${png.length} B`);
}
