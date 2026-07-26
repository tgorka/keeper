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
 *  - The four `sync-N` frames are advanced by the tray's existing ~1 Hz tick.
 *    A rotating gap reads as motion at 1 fps where a pulse or a fade would just
 *    look like flicker.
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

const variants: Array<{ name: string; mark: Coverage }> = [
  // Armed: a complete cycle, static — sync is configured and healthy.
  { name: "tray-sync-template", mark: union(arc(90, GAP_SWEEP), head(90), head(270)) },
  // Four activity frames. The gap walks a quarter turn per tick, so the ring
  // reads as spinning even at the tray's 1 Hz refresh.
  ...[0, 1, 2, 3].map((i) => ({
    name: `tray-sync-${i + 1}-template`,
    mark: union(arc(i * 90, GAP_SWEEP), head(i * 90)),
  })),
  // Paused / media absent: two bars. Unambiguous at 44 px and shares no
  // silhouette with the activity frames.
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

for (const { name, mark } of variants) {
  const png = encodePng(SIZE, SIZE, withMark(base, mark));
  writeFileSync(`${ICON_DIR}/${name}.png`, png);
  console.log(`${name}.png  ${png.length} B`);
}
