/**
 * The ICONDIR of a Windows `.ico` — which sizes it actually carries.
 *
 * Shared by `scripts/gen-mark-icons.ts`, which reads it off the file it has just
 * placed, and `scripts/gen-mark-icons.test.ts`, which reads it off the COMMITTED
 * bytes, for the same reason `png-alpha.ts` is shared: a generator and a gate
 * that measure "carries 16px" two different ways will eventually disagree, and
 * the one that is wrong will be the one nobody re-runs.
 *
 * An `.ico` is a 6-byte header — reserved `0`, type `1` for an icon, then the
 * entry count — followed by one 16-byte directory entry per image. Only the
 * directory is read: the images themselves are PNGs `tauri icon` wrote, and
 * `png-alpha.ts` is what measures pixels. Anything that is not an icon file
 * throws rather than reporting zero sizes, because "no entries" and "not an ico"
 * must not look the same to a check.
 */

import { readFileSync } from "node:fs";

/** One image the file offers, as its directory advertises it. */
export type IcoEntry = { width: number; height: number; bytes: number; offset: number };

export function readIcoEntries(path: string): IcoEntry[] {
  const bytes = new Uint8Array(readFileSync(path));
  const view = new DataView(bytes.buffer, bytes.byteOffset, bytes.byteLength);
  if (bytes.length < 6 || view.getUint16(0, true) !== 0 || view.getUint16(2, true) !== 1) {
    throw new Error(`${path} is not a Windows icon file`);
  }
  const count = view.getUint16(4, true);
  if (bytes.length < 6 + count * 16) {
    throw new Error(`${path} claims ${count} entries but is only ${bytes.length} bytes`);
  }
  return Array.from({ length: count }, (_, i) => {
    const at = 6 + i * 16;
    return {
      // A dimension is one byte, so 256 — the largest size an ico may hold — is
      // stored as 0. Reporting a 0 here would make the biggest entry read as the
      // smallest, which is exactly the size a check most wants to see.
      width: bytes[at] === 0 ? 256 : bytes[at],
      height: bytes[at + 1] === 0 ? 256 : bytes[at + 1],
      bytes: view.getUint32(at + 8, true),
      offset: view.getUint32(at + 12, true),
    };
  });
}

/** The square sizes the file carries, ascending — what a consumer can pick from. */
export function icoSizes(path: string): number[] {
  return readIcoEntries(path)
    .filter((entry) => entry.width === entry.height)
    .map((entry) => entry.width)
    .sort((a, b) => a - b);
}
