/**
 * The guard that keeps this repo down to ONE file classifier (Story 45.2,
 * AD-87; 43.5's AD-73).
 *
 * The registry refines by extension only INSIDE the kind `file`, which is
 * 43.5's declared catch-all. The argument that this cannot contradict
 * `kind_for_file_name` is sound, and an argument is not a guarantee: nothing
 * in TypeScript stops somebody adding `png` to `FILE_FORMATS`, and nothing in
 * Rust stops somebody adding `svg` to a table it already shares with the
 * frontend's ideas.
 *
 * So this reads the Rust source and asserts the two vocabularies are disjoint.
 * The tables live in different languages in different crates and there is no
 * type that can hold both — the same situation 43.5 met between the kind
 * tables and `note_protocol::mime_for`, and it solved it the same way, by
 * walking both and asserting.
 *
 * **A parse that finds nothing must fail, not pass.** A regex that silently
 * matches zero arrays would make every disjointness assertion vacuously true
 * and this file would go green forever while guarding nothing. Each table is
 * therefore checked against the length Rust declares in its own type
 * (`[&str; 8]`) and against a member we know is there.
 */

import { readFileSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";
import { describe, expect, it } from "vitest";
import { FILE_FORMATS } from "@/lib/viewers";

// `fileURLToPath` is handed a STRING, not a URL object: the jsdom environment
// replaces the global `URL`, and Node rejects a foreign instance with "the URL
// must be of scheme file" even when the href is one. This is the shape
// `src/test/no-user-agent-gating.test.ts` already uses.
const CLASSIFIER_SOURCE = path.resolve(
  path.dirname(fileURLToPath(import.meta.url)),
  "../../../src-tauri/crates/keeper-core/src/archive/recordings_fts.rs",
);

const source = readFileSync(CLASSIFIER_SOURCE, "utf8");

/**
 * One `pub const NAME: [&str; N] = ["a", "b"];` out of the Rust source, with
 * the declared arity beside the members so a partial match is a failure rather
 * than a shorter list nobody notices.
 */
function rustExtensionTable(name: string): { declared: number; members: string[] } {
  const declaration = new RegExp(
    `pub const ${name}: \\[&str; (\\d+)\\] = \\[([^\\]]*)\\];`,
    "m",
  ).exec(source);
  if (declaration === null) {
    throw new Error(
      `${name} was not found in ${CLASSIFIER_SOURCE}. If it was renamed or reshaped, this guard must be updated, not deleted.`,
    );
  }
  const members = [...declaration[2].matchAll(/"([^"]+)"/g)].map((match) => match[1]);
  return { declared: Number(declaration[1]), members };
}

const RUST_TABLES = {
  VIDEO_EXTENSIONS: rustExtensionTable("VIDEO_EXTENSIONS"),
  IMAGE_EXTENSIONS: rustExtensionTable("IMAGE_EXTENSIONS"),
  AUDIO_EXTENSIONS: rustExtensionTable("AUDIO_EXTENSIONS"),
};

describe("the Rust classifier's tables were actually read", () => {
  it.each([
    ["VIDEO_EXTENSIONS", "mov"],
    ["IMAGE_EXTENSIONS", "png"],
    ["AUDIO_EXTENSIONS", "wav"],
  ] as const)("%s parsed to its declared arity and contains %s", (name, known) => {
    const table = RUST_TABLES[name];
    expect(table.members).toHaveLength(table.declared);
    expect(table.members.length).toBeGreaterThan(0);
    expect(table.members).toContain(known);
  });
});

describe("the registry cannot become a second classifier", () => {
  it("claims no extension that keeper-core already classifies as media", () => {
    const media = new Set([
      ...RUST_TABLES.VIDEO_EXTENSIONS.members,
      ...RUST_TABLES.IMAGE_EXTENSIONS.members,
      ...RUST_TABLES.AUDIO_EXTENSIONS.members,
    ]);
    const overlap = [...FILE_FORMATS.keys()].filter((extension) => media.has(extension));
    expect(overlap).toEqual([]);
  });

  it("compares the two vocabularies in the same case", () => {
    // Rust matches with `eq_ignore_ascii_case`, so an uppercase member there
    // would slip past a case-sensitive set test here and the overlap check
    // above would pass on an overlap that exists.
    for (const table of Object.values(RUST_TABLES)) {
      for (const extension of table.members) {
        expect(extension).toBe(extension.toLowerCase());
      }
    }
  });
});
