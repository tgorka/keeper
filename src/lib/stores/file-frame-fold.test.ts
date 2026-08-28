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
  return rawJar(encodeURIComponent(value));
}

/**
 * The same jar with the file pane's value written RAW, byte for byte.
 *
 * {@link jar} goes through `encodeURIComponent`, which CANNOT produce a value
 * that is not valid percent-encoding — a `%` comes back as `%25` and decodes
 * cleanly. A malformed escape only ever arrives from outside this build, so a
 * test for one has to write the bytes itself.
 */
function rawJar(encoded: string): string {
  return [
    "theme=dark",
    `${NOTES_RAIL_FOLD_COOKIE}=${encodeURIComponent("spaces:1|tags:1|files:0")}`,
    `${FILE_FRAME_FOLD_COOKIE}=${encoded}`,
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
   * The form OPEN and the caveat folded, and neither half is arbitrary (Story
   * 54.2). This asserted `{ properties: true, caveat: true }` while the file
   * surface copied the notes surface's closed default, and the analogy it stood
   * on is false: on a note the frontmatter is a separate store field
   * (`notes-editor.ts:16-19`) so a closed panel hides nothing, while on a file
   * the buffer IS the whole file, so a closed form put the `---` block into the
   * reader's prose. `caveat` folded is AD-102's fact in one line rather than
   * four — never in none.
   */
  it("opens the form and folds the caveat for a keeper that has never folded anything", () => {
    expect(readFileFrameFold("")).toEqual({ properties: false, caveat: true });
    expect(fileFrameFolded()).toEqual({ properties: false, caveat: true });
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
    //
    // Both axes still discriminate after Story 54.2 flipped the properties
    // default: `properties:yes` is junk and answers `false`, which a parser that
    // read junk as folded would get wrong, and `caveat:0` is a real value that
    // differs from that band's default.
    expect(readFileFrameFold(jar("properties:yes|caveat:0|nosuch:1|garbage"))).toEqual({
      properties: false,
      caveat: false,
    });
  });

  it("leaves both bands at their defaults for a value that is not percent-encoding", () => {
    // Junk this parser can READ is dropped by the test above. This is junk it
    // cannot: `decodeURIComponent` throws a `URIError` on a lone `%` and on a
    // truncated escape, and the read happens inside `TextFileFrame`'s mount
    // effect — so unguarded, a jar an extension rewrote or an interrupted write
    // left behind did not fold a band wrong, it took the whole panel down.
    //
    // Both bands VISIBLY at their defaults: the values inside the truncated
    // entry are the OPPOSITE of both defaults — `properties:1` against a default
    // of open, `caveat:0` against a default of folded — so a parser that read
    // half of it before failing would answer differently on either band.
    expect(readFileFrameFold(rawJar("%"))).toEqual(fileFrameFolded());
    expect(readFileFrameFold(rawJar("properties:1|caveat:0%7"))).toEqual(fileFrameFolded());

    // And it costs its own surface only: one unreadable value in a shared jar
    // must not lose the rail the fold beside it remembers.
    expect(readNotesRailFold(rawJar("%"))).toEqual({ spaces: true, tags: true, files: false });
  });

  it("reads its own cookie and never the notes rail's", () => {
    // The rail's own `files` key is in this jar and says something different.
    // A shared namespace would make folding one surface fold the other. Both
    // values are the opposite of this store's defaults, so a read that answered
    // from the defaults could not pass.
    const jarBoth = jar("properties:1|caveat:0");
    expect(readFileFrameFold(jarBoth)).toEqual({ properties: true, caveat: false });
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

    // Folded, from the open default Story 54.2 ships.
    expect(fileFrameFoldStore.getState().bands).toEqual({ properties: true, caveat: true });
    // Persisted, not just held: the frame is unmounted by a panel fold and by
    // every surface switch.
    expect(readFileFrameFold(document.cookie)).toEqual({ properties: true, caveat: true });

    fileFrameFoldStore.getState().toggleBand("caveat");
    expect(readFileFrameFold(document.cookie)).toEqual({ properties: true, caveat: false });
  });

  it("restores once, so a second frame cannot undo a fold the reader just changed", () => {
    // A cookie that folds the form, which is the opposite of the default — so a
    // build that never hydrated could not pass the first assertion.
    hydrateFileFrameFold(jar("properties:1|caveat:1"));
    expect(fileFrameFoldStore.getState().bands.properties).toBe(true);

    fileFrameFoldStore.getState().toggleBand("properties");
    // React's double-invoked development effect, or the second file pane in a
    // strip of four mounting a moment later.
    hydrateFileFrameFold(jar("properties:1|caveat:1"));

    expect(fileFrameFoldStore.getState().bands.properties).toBe(false);
  });
});
