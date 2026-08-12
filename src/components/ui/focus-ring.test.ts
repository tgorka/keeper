import { readdirSync, readFileSync } from "node:fs";
import { join } from "node:path";
import { describe, expect, it } from "vitest";
import {
  FOCUS_RING,
  FOCUS_RING_WITHIN,
  INVALID_RING,
  INVALID_RING_WITHIN,
} from "@/components/ui/focus-ring";
import {
  colourUtilities,
  contrast,
  INDICATOR_FLOOR,
  resolveColour,
  SURFACES,
  THEMES,
} from "@/test/colour";

/**
 * Two claims, and the second is the one that has already been broken once.
 *
 * 1. The rings clear 3:1 on every surface of both themes. That is arithmetic.
 * 2. Nothing in `src/components/ui` draws its own. That is the claim `button.tsx`
 *    made in a comment and did not keep: the ring was measured and fixed on one
 *    component while nine other primitives went on shipping the 2.12:1 default.
 *    A comment cannot hold a rule across ten files; this can.
 */

const UI = join(process.cwd(), "src/components/ui");

/** Every declaration this module publishes, so a new one cannot skip the floor. */
const RINGS: Record<string, string> = {
  FOCUS_RING,
  INVALID_RING,
  FOCUS_RING_WITHIN,
  INVALID_RING_WITHIN,
};

describe("the shared indicators clear their floor", () => {
  it.each(Object.entries(RINGS))("%s draws at full strength on every surface", (name, classes) => {
    const utilities = colourUtilities(classes);
    expect(utilities.length, `${name} declares no colour at all`).toBeGreaterThan(0);
    for (const [themeName, theme] of Object.entries(THEMES)) {
      for (const u of utilities) {
        // A width is a measurement, not a colour; the assertion below covers it.
        const colour = resolveColour(u.value, theme);
        if (colour === null) continue;
        for (const surfaceName of SURFACES) {
          expect(
            contrast(colour, theme[surfaceName]),
            `${name} ${u.kind}-${u.value} on --${surfaceName} (${themeName})`,
          ).toBeGreaterThanOrEqual(INDICATOR_FLOOR);
        }
      }
    }
  });

  it.each(
    Object.entries(RINGS),
  )("%s has a ring WIDTH, not only a ring colour", (_name, classes) => {
    // The badge shipped `aria-invalid:ring-destructive/20` with no `ring-N`
    // anywhere: a ring colour and no ring, a declaration that never rendered a
    // pixel and read in review as though the state was handled.
    expect(classes).toMatch(/:ring-2\b/);
  });
});

describe("one declaration, consumed", () => {
  /**
   * An opacity on an indicator is the original defect, not a style: `ring-ring/50`
   * measured 2.12:1 on the light card and `ring-destructive/20` measured 1.28:1
   * on a pane. Both looked deliberate in review for months.
   */
  it("no ui component spells a focus or invalid ring of its own", () => {
    const offenders: string[] = [];
    for (const file of readdirSync(UI)) {
      if (!file.endsWith(".tsx") || file.endsWith(".test.tsx")) continue;
      const source = readFileSync(join(UI, file), "utf8");
      // Comments in these files quote the old broken values on purpose, and a
      // quotation is not a declaration.
      const code = source.replace(/\/\/.*$/gm, "").replace(/\/\*[\s\S]*?\*\//g, "");
      for (const m of code.matchAll(
        /[\w[\]&>=|-]*(?:focus-visible|aria-invalid):[\w[\]&>=|/-]*ring[\w[\]&>=|/-]*/g,
      )) {
        // `ring-0` is a suppression — an inner control telling its wrapper to
        // draw the ring instead — and suppressing is not spelling one.
        if (/ring-0\b/.test(m[0])) continue;
        offenders.push(`${file}: ${m[0]}`);
      }
    }
    expect(offenders, "import FOCUS_RING / INVALID_RING instead").toEqual([]);
  });

  /**
   * Written out rather than discovered, so that a primitive which drops its ring
   * entirely fails here. A component with no focus indicator at all passes every
   * scan that only looks for the wrong one.
   */
  const FOCUSABLE = [
    "button.tsx",
    "badge.tsx",
    "input.tsx",
    "textarea.tsx",
    "select.tsx",
    "checkbox.tsx",
    "switch.tsx",
    "radio-group.tsx",
    "input-group.tsx",
    "tabs.tsx",
    "scroll-area.tsx",
    "overflow-value.tsx",
  ];

  it.each(FOCUSABLE)("%s consumes the shared ring", (file) => {
    const source = readFileSync(join(UI, file), "utf8");
    expect(source).toMatch(/import \{[^}]*FOCUS_RING[^}]*\} from "@\/components\/ui\/focus-ring"/);
    expect(source).toMatch(/\bFOCUS_RING(_WITHIN)?\b,/);
  });
});
