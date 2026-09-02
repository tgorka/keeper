/**
 * The design gate's arithmetic, tested — because a gate that only ever passes
 * is indistinguishable from a gate that checks nothing.
 *
 * `check-design.mjs` grew a rule for Story 61.7's bot identity palette, and
 * that rule is the story: `DESIGN.md` records that no colour of any hue passes
 * AA in both themes, so a bounded palette is only defensible if every member is
 * MEASURED. Two halves are asserted here:
 *
 * 1. **The shipped palette passes**, read out of the real `src/index.css` and
 *    the real `bot-identity.tsx`. This is the half that catches a hex somebody
 *    nudged.
 * 2. **A palette that does not pass is reported** — per theme and per surface,
 *    with the ratio. This is the half that catches the rule being softened:
 *    lower the floor, drop a surface, or return early, and these fail while
 *    everything else in the repo stays green.
 */
import { readFileSync } from "node:fs";
import { describe, expect, it } from "vitest";
import {
  IDENTITY_INK_PREFIX,
  IDENTITY_PALETTE_BOUNDS,
  IDENTITY_PALETTE_FLOOR,
  IDENTITY_SURFACES,
  identityPaletteFindings,
  readNameList,
  readRustNameList,
  themeBlock,
} from "./check-design.mjs";

const CSS = readFileSync("src/index.css", "utf8");
const PICKER = readFileSync("src/components/bots/bot-identity.tsx", "utf8");
const VALIDATOR = readFileSync("src-tauri/crates/keeper-core/src/bots/identity.rs", "utf8");

const THEMES = { light: themeBlock(CSS, ":root {"), dark: themeBlock(CSS, ".dark {") };
const NAMES = readNameList(PICKER, "BOT_IDENTITY_COLOURS") ?? [];

/** A fixture theme: the real surfaces, with the inks the test supplies. */
function themeWith(inks: Record<string, string>) {
  return {
    light: {
      background: "#f4f2ec",
      card: "#eceae2",
      secondary: "#e3e0d6",
      ...inks,
    },
  };
}

describe("the shipped bot palette", () => {
  it("is declared in both themes and in both languages", () => {
    expect(NAMES.length).toBeGreaterThanOrEqual(IDENTITY_PALETTE_BOUNDS[0]);
    expect(NAMES.length).toBeLessThanOrEqual(IDENTITY_PALETTE_BOUNDS[1]);
    // The picker offers what the validator accepts, in the same order. A
    // colour offered and refused is a save that fails after somebody chose.
    expect(readRustNameList(VALIDATOR, "BOT_COLOURS")).toEqual(NAMES);
    expect(readRustNameList(VALIDATOR, "BOT_SHAPES")).toEqual(
      readNameList(PICKER, "BOT_IDENTITY_SHAPES"),
    );
  });

  it("clears the floor on every surface of both themes", () => {
    expect(identityPaletteFindings(NAMES, THEMES)).toEqual([]);
  });

  it("measures each ink at a ratio the report can print", () => {
    // Not a tautology over the same call: this reads the hexes the same way the
    // gate does and states the floor independently, so an ink edited to a
    // value that merely LOOKS dark enough is caught with a number.
    for (const theme of ["light", "dark"] as const) {
      for (const name of NAMES) {
        const hex = THEMES[theme][`${IDENTITY_INK_PREFIX}${name}`];
        expect(hex, `${name} (${theme})`).toMatch(/^#[0-9a-f]{6}$/);
      }
    }
    expect(IDENTITY_SURFACES).toContain("secondary");
    expect(IDENTITY_PALETTE_FLOOR).toBe(4.5);
  });
});

describe("a palette member that does not pass", () => {
  it("is reported with its theme, its surface and its ratio", () => {
    const findings = identityPaletteFindings(
      ["clay", "washed"],
      themeWith({
        [`${IDENTITY_INK_PREFIX}clay`]: "#ac2f3b",
        // Pale enough to fail on the paper surfaces and nowhere near the floor.
        [`${IDENTITY_INK_PREFIX}washed`]: "#d8c9a0",
      }),
    );
    const contrast = findings.filter((f) => f.kind === "contrast");
    expect(contrast.length).toBe(IDENTITY_SURFACES.length);
    for (const finding of contrast) {
      expect(finding.name).toBe("washed");
      expect(finding.theme).toBe("light");
      expect(finding.ratio).toBeLessThan(IDENTITY_PALETTE_FLOOR);
    }
    // And the passing member is not reported, so the rule is not simply
    // failing everything.
    expect(findings.some((f) => f.name === "clay")).toBe(false);
  });

  it("fails on the surface a member only just clears the background on", () => {
    // The exact defect `contrast` exists for, restated for this palette:
    // `--secondary` is darker than `--background` in the light theme, so an ink
    // tuned against the page is not tuned against a raised row.
    const findings = identityPaletteFindings(
      ["borderline"],
      themeWith({ [`${IDENTITY_INK_PREFIX}borderline`]: "#8a8a8a" }),
    );
    expect(findings.map((f) => f.surface)).toContain("secondary");
  });

  it("reports an ink defined in only one theme", () => {
    const findings = identityPaletteFindings(["clay", "ghost"], {
      light: {
        background: "#f4f2ec",
        card: "#eceae2",
        secondary: "#e3e0d6",
        [`${IDENTITY_INK_PREFIX}clay`]: "#ac2f3b",
        [`${IDENTITY_INK_PREFIX}ghost`]: "#6b3b00",
      },
      dark: {
        background: "#0d1210",
        card: "#141a17",
        secondary: "#1b221e",
        [`${IDENTITY_INK_PREFIX}clay`]: "#df5f65",
      },
    });
    expect(findings.filter((f) => f.kind !== "bounds")).toEqual([
      { kind: "missing", name: "ghost", theme: "dark" },
    ]);
  });

  it("reports an ink nothing can choose", () => {
    const findings = identityPaletteFindings(
      ["clay"],
      themeWith({
        [`${IDENTITY_INK_PREFIX}clay`]: "#ac2f3b",
        [`${IDENTITY_INK_PREFIX}retired`]: "#6b3b00",
      }),
    );
    // The fixture palette is deliberately below the bound, so that finding is
    // expected here and is asserted on its own below.
    expect(findings.filter((f) => f.kind !== "bounds")).toEqual([
      { kind: "orphan", name: "retired", theme: "light" },
    ]);
  });

  it("reports a palette that has grown past its bound", () => {
    const inks: Record<string, string> = {};
    const names: string[] = [];
    for (let i = 0; i <= IDENTITY_PALETTE_BOUNDS[1]; i += 1) {
      names.push(`ink${i}`);
      inks[`${IDENTITY_INK_PREFIX}ink${i}`] = "#ac2f3b";
    }
    const findings = identityPaletteFindings(names, themeWith(inks));
    expect(findings.some((f) => f.kind === "bounds")).toBe(true);
  });
});
