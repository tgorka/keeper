/**
 * The `keeper-file://` composer, pinned to the Rust parser (Story 45.7, AD-65).
 *
 * **The whole point of this file is the shared vector table.** The composer is
 * here and the parser is in `keeper_core::file_asset`; they never meet at
 * runtime, so nothing except a table both suites load can stop them drifting.
 * A folder with a space in it becoming a 404 nobody can reproduce is the exact
 * failure this prevents, and it is the failure that is cheapest to ship,
 * because every name a developer types by hand is ASCII and unremarkable.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";
import { FILE_ASSET_SCHEME, fileAssetUrl } from "./file-asset-url";

/**
 * Read from the Rust tree rather than copied into `src/`, and that direction is
 * deliberate: the fixture lives beside the parser, and the composer reaches for
 * it. `readFileSync` rather than an `import`, following `file-size.test.ts`: a
 * JSON import outside the Vite root depends on bundler configuration a test
 * should not be able to break, and a missing file must be a loud failure here
 * rather than an empty table that passes.
 */
const FIXTURE = resolve(
  import.meta.dirname,
  "../../../src-tauri/crates/keeper-core/src/file-asset-url-vectors.json",
);

interface Vector {
  profile_id: string;
  relative_path: string;
  url: string;
}

const VECTORS = JSON.parse(readFileSync(FIXTURE, "utf8")) as {
  ok: Vector[];
  refused: Vector[];
};

describe("the shared vector table", () => {
  it("composes every vector exactly as the Rust parser expects to receive it", () => {
    // Both lists: a dot segment is still COMPOSED, deliberately, so a traversal
    // attempt reaches the log as visible text rather than as a path that
    // already collapsed. Rust's side of this file asserts every one of those is
    // then refused on resolution.
    for (const vector of [...VECTORS.ok, ...VECTORS.refused]) {
      expect(fileAssetUrl(vector.profile_id, vector.relative_path)).toBe(vector.url);
    }
  });

  it("carries enough vectors to be worth loading", () => {
    // A table someone empties is a table that makes both suites pass while the
    // two languages agree about nothing.
    expect(VECTORS.ok.length).toBeGreaterThanOrEqual(8);
    expect(VECTORS.refused.length).toBeGreaterThanOrEqual(1);
  });
});

describe("fileAssetUrl", () => {
  it("keeps `/` a separator and escapes everything a segment could hide behind", () => {
    // A space, a `#` and a `?` each end a URL's path in some parser; a
    // separator that survived would let one segment become two.
    expect(fileAssetUrl("01P", "a b/c#d/e?f.png")).toBe(
      `${FILE_ASSET_SCHEME}://01P/a%20b/c%23d/e%3Ff.png`,
    );
  });

  it("escapes the sub-delims encodeURIComponent leaves behind", () => {
    // Stricter than `encodeURIComponent` on purpose: `!'()*` are legal in a
    // path segment and therefore a question about what a webview normalises
    // before the request reaches the handler. Escaping them removes the
    // question, and matches `notes_vault::asset_url`'s set exactly.
    expect(fileAssetUrl("01P", "it's (a) *star*!.png")).toBe(
      `${FILE_ASSET_SCHEME}://01P/it%27s%20%28a%29%20%2Astar%2A%21.png`,
    );
  });

  it("leaves the unreserved set legible", () => {
    // Without this every ordinary filename renders as `a%2Db%2Ec.png` in the
    // DOM and in every log line that quotes it.
    expect(fileAssetUrl("01P", "my-file_name~2.mov")).toBe(
      `${FILE_ASSET_SCHEME}://01P/my-file_name~2.mov`,
    );
  });

  it("encodes a dot segment whole rather than dropping it", () => {
    // Dropping it would make the refusal invisible; encoding it means the
    // attempt is in the log as text and Rust refuses it on resolution.
    expect(fileAssetUrl("01P", "../secrets.mov")).toBe(
      `${FILE_ASSET_SCHEME}://01P/%2E%2E/secrets.mov`,
    );
    expect(fileAssetUrl("01P", "a/./b.mov")).toBe(`${FILE_ASSET_SCHEME}://01P/a/%2E/b.mov`);
  });

  it("escapes the profile id too", () => {
    // Ids are ULIDs and need nothing escaped today. The escape is here because
    // a host that is not escaped is a host that can carry a `/`, and one `/`
    // in the host makes the first path segment part of the id.
    expect(fileAssetUrl("a/b", "clip.mov")).toBe(`${FILE_ASSET_SCHEME}://a%2Fb/clip.mov`);
  });

  it("names the same scheme the Rust handler registers", () => {
    expect(FILE_ASSET_SCHEME).toBe("keeper-file");
  });
});
