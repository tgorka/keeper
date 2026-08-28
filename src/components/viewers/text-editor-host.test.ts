/**
 * The pieces of the editor host that a mounted view cannot show you.
 *
 * Three of these are **guard** tests rather than behaviour tests, and they earn
 * their place by failing on a change nobody would otherwise notice:
 *
 * - the edit limit is a number written down twice, in Rust and in TypeScript,
 *   and a mirror that can drift silently is worse than no mirror;
 * - `src/lib/viewers` names a language for every text row, and a row naming a
 *   language this host cannot load renders monochrome with nothing on screen to
 *   say so;
 * - the size measurement is in bytes, and the difference between bytes and
 *   UTF-16 units is invisible until somebody opens a file that is not English.
 *
 * `text-viewer.test.tsx` is the other half, and the half the ledger keeps
 * asking for: it mounts a real `EditorView` and types into it.
 */
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import { FILE_FORMAT_ENTRIES } from "@/lib/viewers/registry";
import {
  isOversizeForEditing,
  PLAIN_LANGUAGE_IDS,
  TEXT_EDIT_MAX_BYTES,
  TEXT_LANGUAGE_IDS,
} from "./text-editor-host";

const RUST_SOURCE = "src-tauri/crates/keeper-core/src/text_file.rs";

/**
 * Every language id the registry actually puts on a row.
 *
 * Derived from their table rather than from a list they export for this test:
 * a curated list is a third place the vocabulary is written down, and it would
 * stay green through exactly the change this guard exists to catch.
 */
const LANGUAGE_IDS_IN_USE: readonly string[] = [
  ...new Set(
    FILE_FORMAT_ENTRIES.map((entry) => entry.language).filter(
      (id): id is NonNullable<typeof id> => id !== null,
    ),
  ),
];

describe("the edit limit", () => {
  it("is the same number Rust decided", () => {
    // Parsed out of the Rust source rather than restated here. Rust owns the
    // limit — it is what decides how much of a file to read and send — and this
    // constant exists only so a surface holding a bare buffer can ask the same
    // question. Two numbers that must agree and no test between them is how a
    // banner comes to say "1.0 MB" over a file the backend refused at 512 kB.
    const source = readFileSync(RUST_SOURCE, "utf8");
    const declared = /pub const TEXT_EDIT_MAX_BYTES: u64 = ([0-9_]+);/.exec(source);

    // A regex that matches nothing must fail, not pass vacuously.
    expect(declared).not.toBeNull();
    expect(Number((declared as RegExpExecArray)[1].split("_").join(""))).toBe(TEXT_EDIT_MAX_BYTES);
  });

  it("is decimal, so the number and the label a person sees line up", () => {
    // 1 << 20 would render as "1.0 MB" too, and then "the limit is 1.0 MB"
    // would be true of a file of 1 000 000 bytes that the editor refused.
    expect(TEXT_EDIT_MAX_BYTES).toBe(1_000_000);
  });
});

describe("isOversizeForEditing", () => {
  it("is false at the limit and true one byte past it", () => {
    expect(isOversizeForEditing("a".repeat(TEXT_EDIT_MAX_BYTES))).toBe(false);
    expect(isOversizeForEditing("a".repeat(TEXT_EDIT_MAX_BYTES + 1))).toBe(true);
  });

  it("counts UTF-8 bytes, not UTF-16 units", () => {
    // 400 000 three-byte characters: 1 200 000 bytes, 400 000 `.length` units.
    // Measured with `.length` this returns false while Rust has already called
    // the same file oversize, and the surface offers a save Rust will refuse.
    const wide = "☃".repeat(400_000);

    expect(wide.length).toBeLessThan(TEXT_EDIT_MAX_BYTES);
    expect(isOversizeForEditing(wide)).toBe(true);
  });

  it("is false for the empty buffer", () => {
    expect(isOversizeForEditing("")).toBe(false);
  });
});

describe("the language table, against the registry's", () => {
  it("can load a grammar for every language id the registry uses, except php", () => {
    // The guard that keeps two tables from drifting. `src/lib/viewers` decides
    // extension -> language id; this module decides id -> grammar. Adding a row
    // to the registry with an id nobody wired here produces a file that opens
    // monochrome and a console line most people will never read — so the
    // failure is moved here, where it stops a commit.
    //
    // `php` is the one deliberate hole: `@codemirror/legacy-modes` has no PHP
    // tokeniser and `@codemirror/lang-php` would be a second dependency for a
    // single row. Named explicitly rather than allowed by a loose assertion, so
    // that closing it or adding a second hole both change this line.
    const wired = new Set<string>([...TEXT_LANGUAGE_IDS, ...PLAIN_LANGUAGE_IDS]);
    const missing = LANGUAGE_IDS_IN_USE.filter((id) => !wired.has(id));

    expect(LANGUAGE_IDS_IN_USE.length).toBeGreaterThan(5);
    expect(missing).toEqual(["php"]);
  });

  it("treats plain and csv as deliberate, not as unwired", () => {
    // The two ids that must never produce the "no grammar is wired" line: they
    // are text with no syntax to colour, and a log that cries wolf on every CSV
    // is a log nobody reads when a real row is missing.
    for (const id of ["plain", "csv"]) {
      expect(PLAIN_LANGUAGE_IDS).toContain(id);
      expect(TEXT_LANGUAGE_IDS as readonly string[]).not.toContain(id);
    }
  });

  it("names no id twice", () => {
    const all = [...TEXT_LANGUAGE_IDS, ...PLAIN_LANGUAGE_IDS];

    expect(new Set(all).size).toBe(all.length);
  });
});
