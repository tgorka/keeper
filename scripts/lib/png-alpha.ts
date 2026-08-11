/**
 * The alpha-map measurements the keeper mark is held to.
 *
 * Shared by `scripts/gen-mark-icons.ts`, which runs them while writing the icons,
 * and `scripts/gen-mark-icons.test.ts`, which runs them against the COMMITTED
 * files. Both matter and they are not the same check: the generator proves the
 * artwork can be produced correctly, the test proves the bytes in the repo are
 * still that artwork. A generator nobody re-runs is not a gate.
 *
 * Only the PNG subset resvg emits is supported: 8-bit RGBA, no interlace, no
 * palette. Anything else throws rather than guessing, because a silent
 * mis-decode here would report a mark as legible when nobody had measured it.
 */

import { readFileSync } from "node:fs";
import { deflateSync, inflateSync } from "node:zlib";

/** Always RGBA in memory, whatever the file's colour type was. */
export type Png = { width: number; height: number; pixels: Uint8Array; colourType: number };

export function decodePng(bytes: Uint8Array): Png {
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  let pos = 8;
  let width = 0;
  let height = 0;
  let colourType = 6;
  const idat: Uint8Array[] = [];
  while (pos < bytes.length) {
    const len = view.getUint32(pos);
    const type = String.fromCharCode(...bytes.subarray(pos + 4, pos + 8));
    if (type === "IHDR") {
      width = view.getUint32(pos + 8);
      height = view.getUint32(pos + 12);
      const [depth, colour, , , interlace] = bytes.subarray(pos + 16, pos + 21);
      // 6 is RGBA, 2 is RGB. Both appear in this repo's icons on purpose: tray
      // templates are RGBA because alpha IS the artwork, and iOS app icons are RGB
      // because Apple rejects an icon that has an alpha channel at all.
      if (depth !== 8 || (colour !== 6 && colour !== 2) || interlace !== 0) {
        throw new Error(`unsupported PNG: depth ${depth} colour ${colour} interlace ${interlace}`);
      }
      colourType = colour;
    } else if (type === "IDAT") idat.push(bytes.subarray(pos + 8, pos + 8 + len));
    pos += 12 + len;
  }
  const ch = colourType === 6 ? 4 : 3;
  const raw = new Uint8Array(inflateSync(Buffer.concat(idat)));
  const stride = width * ch;
  const planar = new Uint8Array(stride * height);
  let p = 0;
  for (let y = 0; y < height; y++) {
    const filter = raw[p++];
    const row = planar.subarray(y * stride, (y + 1) * stride);
    row.set(raw.subarray(p, p + stride));
    p += stride;
    const prior = y > 0 ? planar.subarray((y - 1) * stride, y * stride) : new Uint8Array(stride);
    for (let i = 0; i < stride; i++) {
      const a = i >= ch ? row[i - ch] : 0;
      const b = prior[i];
      const c = i >= ch ? prior[i - ch] : 0;
      if (filter === 1) row[i] = (row[i] + a) & 255;
      else if (filter === 2) row[i] = (row[i] + b) & 255;
      else if (filter === 3) row[i] = (row[i] + ((a + b) >> 1)) & 255;
      else if (filter === 4) {
        const pa = Math.abs(b - c);
        const pb = Math.abs(a - c);
        const pc = Math.abs(a + b - 2 * c);
        row[i] = (row[i] + (pa <= pb && pa <= pc ? a : pb <= pc ? b : c)) & 255;
      }
    }
  }
  if (ch === 4) return { width, height, pixels: planar, colourType };
  const rgba = new Uint8Array(width * height * 4);
  for (let i = 0; i < width * height; i++) {
    rgba[i * 4] = planar[i * 3];
    rgba[i * 4 + 1] = planar[i * 3 + 1];
    rgba[i * 4 + 2] = planar[i * 3 + 2];
    rgba[i * 4 + 3] = 255;
  }
  return { width, height, pixels: rgba, colourType };
}

export function readPng(path: string): Png {
  return decodePng(new Uint8Array(readFileSync(path)));
}

const CRC_TABLE = Array.from({ length: 256 }, (_, n) => {
  let c = n;
  for (let k = 0; k < 8; k++) c = c & 1 ? 0xedb88320 ^ (c >>> 1) : c >>> 1;
  return c >>> 0;
});

function chunk(type: string, data: Uint8Array): Uint8Array {
  const out = new Uint8Array(data.length + 12);
  const view = new DataView(out.buffer);
  view.setUint32(0, data.length);
  for (let i = 0; i < 4; i++) out[4 + i] = type.charCodeAt(i);
  out.set(data, 8);
  let crc = 0xffffffff;
  for (let i = 4; i < data.length + 8; i++) crc = CRC_TABLE[(crc ^ out[i]) & 0xff] ^ (crc >>> 8);
  view.setUint32(data.length + 8, (crc ^ 0xffffffff) >>> 0);
  return out;
}

/**
 * Re-encode as RGB, dropping the alpha channel entirely.
 *
 * This exists for one reason: **Apple rejects an app icon that has an alpha
 * channel**, and it rejects it for HAVING the channel, not for using it — a fully
 * opaque RGBA icon still fails. The iOS set therefore has to be colour type 2,
 * which the rasteriser does not emit, so the pixels are re-packed here.
 *
 * Throws rather than silently flattening if any pixel is not fully opaque:
 * discarding real transparency would bake whatever happened to be in the RGB
 * channels into the icon, which is how icons acquire black fringes.
 */
export function encodeRgbPng({ width, height, pixels }: Png): Uint8Array {
  const stride = width * 3;
  const raw = new Uint8Array((stride + 1) * height);
  for (let y = 0; y < height; y++) {
    raw[y * (stride + 1)] = 0;
    for (let x = 0; x < width; x++) {
      const src = (y * width + x) * 4;
      if (pixels[src + 3] !== 255) {
        throw new Error(`pixel ${x},${y} has alpha ${pixels[src + 3]}; refusing to drop it`);
      }
      const dst = y * (stride + 1) + 1 + x * 3;
      raw[dst] = pixels[src];
      raw[dst + 1] = pixels[src + 1];
      raw[dst + 2] = pixels[src + 2];
    }
  }
  const ihdr = new Uint8Array(13);
  const view = new DataView(ihdr.buffer);
  view.setUint32(0, width);
  view.setUint32(4, height);
  ihdr[8] = 8;
  ihdr[9] = 2;
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", ihdr),
    chunk("IDAT", new Uint8Array(deflateSync(raw))),
    chunk("IEND", new Uint8Array(0)),
  ]);
}

export type Hole = { area: number; box: [number, number, number, number] };

/**
 * Enclosed background regions: 4-connected runs of transparency that never reach
 * the image border, largest first.
 *
 * This is the measurement behind "the identity lives in the holes". An aperture
 * is only a hole while it is one of these — if the count falls the mark has
 * filled in, and if a hole ever merged with the tag's punched corner it would
 * stop being enclosed and join the outside, which is the failure a bounding box
 * cannot see.
 */
export function enclosedHoles(png: Png): Hole[] {
  const { width: w, height: h, pixels } = png;
  const seen = new Uint8Array(w * h);
  const found: Hole[] = [];
  for (let s = 0; s < w * h; s++) {
    if (seen[s] || pixels[s * 4 + 3] >= 128) continue;
    const stack = [s];
    seen[s] = 1;
    let area = 0;
    let edge = false;
    let x0 = w;
    let y0 = h;
    let x1 = -1;
    let y1 = -1;
    while (stack.length) {
      const i = stack.pop() as number;
      const x = i % w;
      const y = (i / w) | 0;
      area++;
      if (x === 0 || y === 0 || x === w - 1 || y === h - 1) edge = true;
      if (x < x0) x0 = x;
      if (y < y0) y0 = y;
      if (x > x1) x1 = x;
      if (y > y1) y1 = y;
      for (const [nx, ny] of [
        [x - 1, y],
        [x + 1, y],
        [x, y - 1],
        [x, y + 1],
      ]) {
        if (nx < 0 || ny < 0 || nx >= w || ny >= h) continue;
        const j = ny * w + nx;
        if (!seen[j] && pixels[j * 4 + 3] < 128) {
          seen[j] = 1;
          stack.push(j);
        }
      }
    }
    if (!edge) found.push({ area, box: [x0, y0, x1, y1] });
  }
  return found.sort((a, b) => b.area - a.area);
}

/** What `measure` reports about one raster's alpha map. */
export type Measurement = {
  /** Pixels that are neither fully on nor fully off — the mush. */
  partial: number;
  /** Pixels carrying any alpha at all. */
  ink: number;
  /** Every pixel in the image, inked or not. */
  all: number;
  /** Inclusive ink bounding box: x0, y0, x1, y1. */
  box: readonly [number, number, number, number];
};

/**
 * Ink extent and the partial-alpha count — the mush a small mark dies of.
 *
 * `partial` counts pixels that are neither fully on nor fully off. At 16px that
 * fraction is what separates marks that survive from marks that smear, and it is
 * mostly a property of whether the geometry lands on whole pixels.
 */
export function measure(png: Png): Measurement {
  let partial = 0;
  let ink = 0;
  let x0 = png.width;
  let y0 = png.height;
  let x1 = -1;
  let y1 = -1;
  for (let i = 0; i < png.width * png.height; i++) {
    const a = png.pixels[i * 4 + 3];
    if (a === 0) continue;
    ink++;
    if (a < 255) partial++;
    const x = i % png.width;
    const y = (i / png.width) | 0;
    if (x < x0) x0 = x;
    if (y < y0) y0 = y;
    if (x > x1) x1 = x;
    if (y > y1) y1 = y;
  }
  return { partial, ink, all: png.width * png.height, box: [x0, y0, x1, y1] as const };
}

/**
 * Pixels carrying non-black RGB.
 *
 * macOS tints a template image by ALPHA and ignores its colour entirely, so a
 * template that smuggles in any colour looks correct in a file viewer and wrong
 * in the menu bar — and inverts under a dark one. Fully transparent pixels are
 * exempt because their RGB is unobservable.
 */
export function nonBlackPixels(png: Png): number {
  let bad = 0;
  for (let i = 0; i < png.pixels.length; i += 4) {
    if (png.pixels[i + 3] === 0) continue;
    if (png.pixels[i] !== 0 || png.pixels[i + 1] !== 0 || png.pixels[i + 2] !== 0) bad++;
  }
  return bad;
}
