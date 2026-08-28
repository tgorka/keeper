/**
 * Every label the menu-bar tray's notes section shows comes from the palette
 * registry, and each of the three words is spelled exactly once (Story 47.4,
 * DW-195).
 *
 * **Why a TypeScript test for two Rust files.** `keeper-core`'s own tests prove
 * the projection produces the registry's words — `tray_notes_labels`,
 * `TrayNotesLabels::painted` and the assertions around them all run on every
 * host. What they cannot prove is that `tray.rs` *uses* it: the `keeper` shell
 * crate does not compile on Linux (no GTK/webkit), so on the machine most of
 * this epic was written on, a hand-typed label reintroduced into
 * `build_notes_items` is invisible to every gate. That is exactly how the defect
 * arrived — Story 46.16 projected the recording verbs and the notes section
 * stayed hand-built for a whole epic, in words that had already drifted (the
 * tray spelled `Today’s Journal` with a typographic apostrophe against the
 * registry's `Today's Journal`, and nothing anywhere noticed).
 *
 * **And why a spelled-once count rather than an equality.** A Rust test can only
 * compare the projected label to the registry's title by VALUE, so it passes
 * just as happily on `new_note: "New Note".to_owned()` — the duplication itself,
 * one layer up, satisfying the test written to catch it. Duplication is a fact
 * about source text, so it is checked as one. This survived a mutation sweep
 * until it did.
 *
 * It follows `capture-capability.test.ts` and `command-registration.test.ts`,
 * this repo's existing idiom for an invariant about a Rust file the frontend
 * host cannot build.
 */
import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

const TRAY_RS = readFileSync(
  resolve(import.meta.dirname, "../../src-tauri/crates/keeper/src/tray.rs"),
  "utf8",
);
const PALETTE_RS = readFileSync(
  resolve(import.meta.dirname, "../../src-tauri/crates/keeper-core/src/palette.rs"),
  "utf8",
);

/**
 * A Rust file with its comment-only lines removed.
 *
 * Doc comments are where a decision is explained and they name the words freely
 * — `NotesTray`'s own doc says "New Note and Today's Journal stay enabled", and
 * `palette.rs` quotes both while explaining why they are not written twice. A
 * check that could not tell prose from code would either fail on those
 * sentences or drive the explanation out of the file, and the explanation is
 * the thing worth keeping.
 *
 * Comments TRAILING a line of code are deliberately not stripped: telling a
 * `//` inside a string from one that starts a comment needs a parser, and the
 * repair for a false positive is to move the sentence to a doc comment above,
 * which is where it reads better anyway.
 */
const codeOf = (source: string): string =>
  source
    .split("\n")
    .filter((line) => !line.trim().startsWith("//"))
    .join("\n");

const TRAY_CODE = codeOf(TRAY_RS);
const PALETTE_CODE = codeOf(PALETTE_RS);

/** The three verbs the tray's notes section shows, as the registry spells them. */
const REGISTRY_TITLES = {
  "notes-new": "New Note",
  "notes-capture": "Quick Capture",
  "notes-journal-today": "Today's Journal",
} as const;

/**
 * One Rust function's source, from its signature to the `}` in column zero.
 *
 * Crude on purpose: the three functions this is used on are top-level, so the
 * first unindented `}` is their end, and a parser here would be a second
 * language's worth of machinery to check a property one `indexOf` can see.
 */
const bodyOf = (source: string, signature: string): string => {
  const start = source.indexOf(signature);
  expect(start, `${signature} is in the file`).toBeGreaterThan(-1);
  const rest = source.slice(start).split("\n");
  return rest.slice(0, rest.indexOf("}") + 1).join("\n");
};

describe("the tray's notes labels are the registry's", () => {
  it("registers each of the three verbs under the id the tray projects", () => {
    // The premise. If the registry stopped shipping one of these,
    // `tray_notes_labels` would answer `None`, the tray would build no notes
    // section at all, and every assertion below would be about a surface nobody
    // sees.
    for (const [id, title] of Object.entries(REGISTRY_TITLES)) {
      expect(PALETTE_CODE).toContain(`"${id}"`);
      expect(PALETTE_CODE).toContain(`"${title}"`);
    }
  });

  it("spells each of the three words exactly once in the whole projection", () => {
    // The defect DW-195 names, stated as the thing it actually is: one word in
    // two places. The registry entry is the one place. A second spelling
    // anywhere in either file — in the tray, or hand-typed back into
    // `tray_notes_labels` itself — is a word a retitle will not reach.
    for (const title of Object.values(REGISTRY_TITLES)) {
      const quoted = `"${title}"`;
      expect({
        title,
        palette: PALETTE_CODE.split(quoted).length - 1,
        tray: TRAY_CODE.split(quoted).length - 1,
      }).toEqual({ title, palette: 1, tray: 0 });
    }
  });

  it("carries no text at all through the three functions that move the labels", () => {
    // The counting test above is beaten by a title assembled out of pieces —
    // `format!("Quick {}", "Capture")` never spells the whole word and produces
    // it anyway. That mutation survived the first sweep. The property that
    // actually holds is stronger and simpler: these three functions move words
    // from the registry to a menu handle and originate none, so not one string
    // literal belongs in any of them.
    //
    // `TrayNotesLabels::painted` is deliberately NOT in this list: the two
    // empty-state suffixes are its own words, and it is the one place they are
    // written.
    for (const [file, source, signature] of [
      ["palette.rs", PALETTE_RS, "pub fn tray_notes_labels("],
      ["tray.rs", TRAY_RS, "fn build_notes_items("],
      ["tray.rs", TRAY_RS, "fn paint_notes("],
    ] as const) {
      expect({
        file,
        signature,
        quotes: codeOf(bodyOf(source, signature)).split('"').length - 1,
      }).toEqual({ file, signature, quotes: 0 });
    }
  });

  it("lets the tray spell none of the three verbs", () => {
    // Not only as a whole quoted title: a label assembled out of pieces, or the
    // typographic-apostrophe spelling the tray actually shipped, is the same
    // duplication wearing a hat.
    expect(TRAY_CODE).not.toMatch(/New Note|Quick Capture|Journal/);
    expect(TRAY_CODE).not.toContain("Today\u2019s");
  });

  it("lets the tray spell neither empty-state suffix", () => {
    // The composition moved to `keeper-core` with the base word, because a
    // sentence assembled in `tray.rs` is a sentence no host but macOS can check.
    // A suffix left behind here would be the same duplication one layer down.
    expect(TRAY_CODE).not.toMatch(/no vault yet|hotkey unavailable/);
    expect(PALETTE_CODE).toContain("(no vault yet)");
    expect(PALETTE_CODE).toContain("hotkey unavailable");
  });

  it("takes its labels from the projection at build time and at paint time", () => {
    // Absence alone is satisfiable by a tray with no notes labels at all. These
    // are the two call sites that have to exist: the menu is built once and only
    // mutated (AD-61), so the words arrive twice — once into the handles, once
    // per model change.
    expect(TRAY_CODE).toContain("let labels = tray_notes_labels(enabled)?;");
    expect(TRAY_CODE).toContain("let labels = items.labels.painted(TrayNotesState {");

    for (const field of ["new_note", "capture", "journal"] as const) {
      expect(TRAY_CODE).toContain(`&labels.${field}`);
    }
  });

  it("keeps the registry to the words and never to the click", () => {
    // DW-195's second warning. The tray's "Open Recordings Folder" reveals the
    // LIVE session's output path while the registry's verb reveals the
    // CONFIGURED destination — same words, two different folders — so a
    // projection that carried anything but words would silently re-point a verb.
    // The notes verbs keep their own `tray-note-*` ids and their own handlers.
    for (const id of ["tray-note-new", "tray-note-capture", "tray-note-journal"]) {
      expect(TRAY_CODE).toContain(`"${id}"`);
    }
    // And no registry id is spelled in the tray's notes code: one there would
    // mean the projection had reached the dispatch.
    for (const id of Object.keys(REGISTRY_TITLES)) {
      expect(TRAY_CODE).not.toContain(`"${id}"`);
    }
  });
});
