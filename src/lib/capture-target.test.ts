/**
 * The capture key, pinned to Rust's (Story 45.15, FR-191).
 *
 * **The shared vector table is the point of this file.** `captureKey` is here
 * and `keeper_core::capture::capture_key` is in Rust; they never meet at
 * runtime, and a capture window asks Rust about a placement row stored under
 * the key Rust built. A drift throws nothing and renders nothing wrong — every
 * window simply forgets where it was put, forever, with no error anywhere. A
 * table both suites load is the only thing that can catch that, and it is the
 * mechanism `file-size.ts` and `file-asset-url.ts` already use for the same
 * hazard.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import type { CaptureTargetVm } from "@/lib/ipc/client";
import { captureKey, captureTargetFromSearch, DRAFT_CAPTURE_KEY } from "./capture-target";

/**
 * Read from the Rust tree rather than a copy in `src/`, following
 * `file-asset-url.test.ts`: the fixture lives beside the function it was
 * written for, and a missing file must be a loud failure here rather than an
 * empty table that passes.
 */
const FIXTURE = resolve(
  import.meta.dirname,
  "../../src-tauri/crates/keeper-core/src/capture-key-vectors.json",
);

const VECTORS = JSON.parse(readFileSync(FIXTURE, "utf8")) as {
  target: CaptureTargetVm;
  key: string;
  search: string;
}[];

describe("the shared vector table", () => {
  it("keys every vector exactly as the Rust mirror does", () => {
    for (const vector of VECTORS) {
      expect(captureKey(vector.target)).toBe(vector.key);
    }
  });

  it("parses back the exact URL Rust composes for every vector", () => {
    // The other half of the same seam, and the half this file owns: Rust builds
    // the window's query string (`keeper_core::capture::capture_search`) and
    // nothing here builds one. A drift is a window that renders "not found" on
    // a note the person just watched keeper accept.
    for (const vector of VECTORS) {
      // The blank-id vector is the one deliberate asymmetry, and it is the
      // contract rather than an exception: `?vault=&note=` names no note, so it
      // reads as the draft window. Round-tripping it would mean opening a
      // window on a note whose id is the empty string.
      const blank =
        vector.target.kind === "note" &&
        (vector.target.vaultId === "" || vector.target.noteId === "");
      expect(captureTargetFromSearch(vector.search)).toEqual(
        blank ? { kind: "draft" } : vector.target,
      );
    }
    expect(VECTORS.some((vector) => vector.search === "")).toBe(true);
  });

  it("carries enough vectors to be worth loading", () => {
    // A table someone empties makes both suites pass while the two languages
    // agree about nothing.
    expect(VECTORS.length).toBeGreaterThanOrEqual(6);
    // And a table of nothing but ASCII agrees about nothing that matters: the
    // encoding only differs from the identity on bytes a developer does not
    // type by hand.
    expect(VECTORS.some((vector) => vector.key.includes("%"))).toBe(true);
  });
});

describe("captureKey", () => {
  it("keeps two ids that would collide unescaped apart", () => {
    // Without the encoding these are one key, and two different notes then
    // share one window, one draft and one remembered position. A note id is
    // derived from a path, so a slash in one is ordinary.
    const slashInNote = captureKey({ kind: "note", vaultId: "a", noteId: "b/c" });
    const slashInVault = captureKey({ kind: "note", vaultId: "a/b", noteId: "c" });
    expect(slashInNote).not.toBe(slashInVault);
    expect(slashInNote).toBe("note:a/b%2Fc");
    expect(slashInVault).toBe("note:a%2Fb/c");
  });

  it("gives the draft window the one key Rust also spells `draft`", () => {
    expect(captureKey({ kind: "draft" })).toBe(DRAFT_CAPTURE_KEY);
    expect(DRAFT_CAPTURE_KEY).toBe("draft");
  });
});

describe("captureTargetFromSearch", () => {
  it("reads no query at all as the draft window", () => {
    for (const search of ["", "?"]) {
      expect(captureTargetFromSearch(search)).toEqual({ kind: "draft" });
    }
  });

  it("refuses half a target rather than guessing the missing half", () => {
    // A note id is unique only inside its vault, so a URL naming a note and no
    // vault cannot be resolved — guessing a vault would open a DIFFERENT note
    // under this note's name, which is the one outcome worse than opening the
    // draft.
    for (const search of [
      "?note=note-1",
      "?vault=vault-a",
      "?vault=&note=note-1",
      "?vault=vault-a&note=",
      "?something=else",
    ]) {
      expect(captureTargetFromSearch(search)).toEqual({ kind: "draft" });
    }
  });

  it("survives a query it did not write", () => {
    // Tauri and the dev server both append parameters of their own; a window
    // that stopped resolving its note because something added a cache-buster
    // would be a bug nobody could reproduce twice.
    expect(captureTargetFromSearch("?theme=dark&vault=v&note=n&t=1699")).toEqual({
      kind: "note",
      vaultId: "v",
      noteId: "n",
    });
  });
});
