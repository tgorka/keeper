import { afterEach, beforeEach, describe, expect, it } from "vitest";
import {
  FILE_FRAME_BANDS,
  FILE_FRAME_FOLD_COOKIE,
  fileFrameFoldCookie,
  fileFrameFolded,
  fileFrameFoldStore,
  hydrateFileFrameFold,
  readFileFrameFold,
  resetFileFrameFoldForTest,
} from "@/lib/stores/file-frame-fold";
import { NOTES_RAIL_FOLD_COOKIE, readNotesRailFold } from "@/lib/stores/notes-rail-fold";

/**
 * What this file deliberately cannot prove is that anything MOUNTS the restore.
 * `hydrateFileFrameFold` is called by `TextFileFrame`, and a store-level test
 * passes unchanged on a build where that call was deleted (DW-172). That
 * assertion lives in `text-file-frame.test.tsx`, against the real frame and a
 * real cookie.
 */

/**
 * A jar with other people's cookies in it, and the file pane's own inside.
 *
 * One of the foreign entries is the NOTES RAIL's fold, which has a `files` key of
 * its own. That is the collision `fold-cookie.ts` names — two surfaces, one word
 * — and a jar without it could not tell "found the right name" from "found the
 * only name".
 */
function jar(value: string): string {
  return [
    "theme=dark",
    `${NOTES_RAIL_FOLD_COOKIE}=${encodeURIComponent("spaces:1|tags:1|files:0")}`,
    `${FILE_FRAME_FOLD_COOKIE}=${encodeURIComponent(value)}`,
  ].join("; ");
}

beforeEach(resetFileFrameFoldForTest);

afterEach(() => {
  resetFileFrameFoldForTest();
  // biome-ignore lint/suspicious/noDocumentCookie: arranging or clearing cookie state is this test's subject
  document.cookie = `${FILE_FRAME_FOLD_COOKIE}=; path=/; max-age=0`;
});

describe("readFileFrameFold", () => {
  /**
   * Both folded is the default, and neither half of that is arbitrary:
   * `properties` matches the notes surface, whose `showProperties` has defaulted
   * closed since Story 49, and `caveat` folded is AD-102's fact in one line
   * rather than four — never in none.
   */
  it("folds both bands for a keeper that has never folded anything", () => {
    expect(readFileFrameFold("")).toEqual({ properties: true, caveat: true });
    expect(fileFrameFolded()).toEqual({ properties: true, caveat: true });
  });

  it("round-trips every combination, both directions", () => {
    for (const properties of [true, false]) {
      for (const caveat of [true, false]) {
        const fold = { properties, caveat };
        expect(readFileFrameFold(jar(`properties:${+properties}|caveat:${+caveat}`))).toEqual(fold);
        // Through the encoder this store actually writes, so the pair cannot
        // agree on a spelling neither of them uses.
        expect(readFileFrameFold(`x=1; ${fileFrameFoldCookie(fold)}`)).toEqual(fold);
      }
    }
  });

  it("leaves a band at its default when the jar's entry is junk", () => {
    // A key this build does not know, a value that is neither 0 nor 1, and an
    // entry with no colon in it. Written by an older build, or by a typo: the
    // answer is the default rather than a refusal to draw the pane.
    expect(readFileFrameFold(jar("properties:yes|caveat:0|nosuch:1|garbage"))).toEqual({
      properties: true,
      caveat: false,
    });
  });

  it("reads its own cookie and never the notes rail's", () => {
    // The rail's own `files` key is in this jar and says something different.
    // A shared namespace would make folding one surface fold the other.
    const jarBoth = jar("properties:0|caveat:0");
    expect(readFileFrameFold(jarBoth)).toEqual({ properties: false, caveat: false });
    expect(readNotesRailFold(jarBoth)).toEqual({ spaces: true, tags: true, files: false });
  });
});

describe("fileFrameFoldCookie", () => {
  it("writes every band, so unfolded is not the same as unwritten", () => {
    const assignment = fileFrameFoldCookie({ properties: false, caveat: true });

    expect(assignment).toContain(`${FILE_FRAME_FOLD_COOKIE}=`);
    for (const band of FILE_FRAME_BANDS) {
      expect(decodeURIComponent(assignment)).toContain(`${band}:`);
    }
    // A year, so a preference does not expire every Monday.
    expect(assignment).toContain("max-age=31536000");
    expect(assignment).toContain("path=/");
  });
});

describe("the store", () => {
  it("toggles one band and writes the whole set out", () => {
    fileFrameFoldStore.getState().toggleBand("properties");

    expect(fileFrameFoldStore.getState().bands).toEqual({ properties: false, caveat: true });
    // Persisted, not just held: the frame is unmounted by a panel fold and by
    // every surface switch.
    expect(readFileFrameFold(document.cookie)).toEqual({ properties: false, caveat: true });

    fileFrameFoldStore.getState().toggleBand("caveat");
    expect(readFileFrameFold(document.cookie)).toEqual({ properties: false, caveat: false });
  });

  it("restores once, so a second frame cannot undo a fold the reader just changed", () => {
    hydrateFileFrameFold(jar("properties:0|caveat:1"));
    expect(fileFrameFoldStore.getState().bands.properties).toBe(false);

    fileFrameFoldStore.getState().toggleBand("properties");
    // React's double-invoked development effect, or the second file pane in a
    // strip of four mounting a moment later.
    hydrateFileFrameFold(jar("properties:0|caveat:1"));

    expect(fileFrameFoldStore.getState().bands.properties).toBe(true);
  });
});
