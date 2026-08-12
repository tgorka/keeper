/**
 * Colour arithmetic for tests that measure the UI rather than read it.
 *
 * `bun run check:design` recomputes the contrast of every TOKEN against every
 * surface of its own theme, and that is where token-level failures get caught.
 * What it structurally cannot see is a COMPONENT that spends a colour which is
 * not a token — `dark:bg-input/30` composites to #181e1b on the ground, #1c2420
 * on a pane and #212925 on a raised card, three surfaces for one variant, and a
 * gate that only knows about declared tokens is never asked about any of them.
 *
 * This module is the same arithmetic pointed at components. It lives here rather
 * than in one test file because three of them need it, and pasting a formula
 * three times is how the codebase ended up with 58 copies of a focus ring.
 */

import { readFileSync } from "node:fs";
import { join } from "node:path";

// `import.meta.url` is not a file URL under the jsdom transform, so this reads
// from the project root vitest is invoked at, exactly as the design gate does.
const CSS = readFileSync(join(process.cwd(), "src/index.css"), "utf8");

function tokensOf(selector: string): Record<string, string> {
  const start = CSS.indexOf(selector);
  const body = CSS.slice(start, CSS.indexOf("\n}", start));
  const out: Record<string, string> = {};
  for (const m of body.matchAll(/--([\w-]+):\s*(#[0-9a-fA-F]{6})\s*;/g)) out[m[1]] = m[2];
  return out;
}

export type Theme = Record<string, string>;

export const THEMES: Record<string, Theme> = {
  light: tokensOf(":root {"),
  dark: tokensOf(".dark {"),
};

/**
 * The three surfaces a component can find itself on. Identical to the gate's own
 * list, and a list rather than one value for the reason the gate gives: a colour
 * checked on the background and not on a card has simply moved its failure
 * somewhere less obvious. `--destructive` is exactly that shape — 5.46:1 on the
 * dark ground, 4.69:1 on `--secondary`.
 */
export const SURFACES = ["background", "card", "secondary"] as const;

/** WCAG 2.1 body-text floor, and the floor DESIGN.md binds metadata to as well. */
export const TEXT_FLOOR = 4.5;

/** WCAG 2.1 SC 1.4.11 / 2.4.11 floor for a non-text indicator or a control edge. */
export const INDICATOR_FLOOR = 3;

/** sRGB hex to linear light, per WCAG 2.1 — the input to both formulas below. */
function linearRgb(hex: string): [number, number, number] {
  const srgb = [1, 3, 5].map((i) => Number.parseInt(hex.slice(i, i + 2), 16) / 255);
  const light = srgb.map((c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4));
  return light as [number, number, number];
}

export function contrast(a: string, b: string): number {
  const [lighter, darker] = [a, b]
    .map((hex) => {
      const [r, g, blue] = linearRgb(hex);
      return 0.2126 * r + 0.7152 * g + 0.0722 * blue;
    })
    .sort((x, y) => y - x);
  return (lighter + 0.05) / (darker + 0.05);
}

/** OKLab, because `color-mix(in oklch, …)` is what the components actually emit. */
function toOklab(hex: string): [number, number, number] {
  const [r, g, b] = linearRgb(hex);
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  return [
    0.2104542553 * l + 0.793617785 * m - 0.0040720468 * s,
    1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s,
    0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s,
  ];
}

function fromOklab([L, a, b]: [number, number, number]): string {
  const l = (L + 0.3963377774 * a + 0.2158037573 * b) ** 3;
  const m = (L - 0.1055613458 * a - 0.0638541728 * b) ** 3;
  const s = (L - 0.0894841775 * a - 1.291485548 * b) ** 3;
  const linear = [
    4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
    -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
    -0.0041960863 * l - 0.7034186147 * m + 1.707614701 * s,
  ];
  const channels = linear
    .map((v) => (v <= 0.0031308 ? 12.92 * v : 1.055 * v ** (1 / 2.4) - 0.055))
    .map((v) => Math.round(Math.min(255, Math.max(0, v * 255))));
  return `#${channels.map((v) => v.toString(16).padStart(2, "0")).join("")}`;
}

/**
 * Utility values that carry no colour. Everything else has to be a token, and
 * this table plus the width rule below are the only ways to say otherwise.
 */
const NOT_A_COLOUR: Record<string, true> = {
  "clip-padding": true,
  transparent: true,
  sm: true,
  xs: true,
};

/** `ring-2`, `border-2`, `ring-[3px]` — a measurement, not a colour. */
const IS_A_WIDTH = /^(\d+|\[\d+px\])$/;

const MIX = /^\[color-mix\(in_oklch,var\(--([\w-]+)\),var\(--([\w-]+)\)_(\d+)%\)\]$/;

/**
 * Resolve one colour utility against one theme, or `null` if it paints nothing.
 *
 * It THROWS rather than shrugging at anything it does not recognise, and that is
 * the point: `bg-input/30`, `bg-destructive/10` and `bg-primary/80` are all
 * unresolvable here, because a translucent surface has no value until you know
 * what is behind it — which is the defect these tests exist to keep out. A new
 * legitimate pattern has to be taught to this function, in the open, rather than
 * arriving unmeasured.
 */
export function resolveColour(value: string, theme: Theme): string | null {
  if (value in NOT_A_COLOUR || IS_A_WIDTH.test(value)) return null;
  if (value in theme) return theme[value];
  const mix = MIX.exec(value);
  if (mix !== null) {
    const [, from, to, percent] = mix;
    if (!(from in theme) || !(to in theme)) {
      throw new Error(`color-mix over a token that does not exist: ${value}`);
    }
    const [a, b] = [toOklab(theme[from]), toOklab(theme[to])];
    const p = Number(percent) / 100;
    const mixed = [0, 1, 2].map((i) => a[i] * (1 - p) + b[i] * p);
    return fromOklab(mixed as [number, number, number]);
  }
  throw new Error(
    `"${value}" is not a token in src/index.css. A component may spend only ` +
      `named colour: an opacity like /30 has no value until you know its ` +
      `backdrop, and that is the bug this test was written for.`,
  );
}

export type Utility = { state: string; kind: string; value: string };

/** Every colour utility in a class list, with the state it applies in. */
export function colourUtilities(classes: string): Utility[] {
  const out: Utility[] = [];
  for (const cls of classes.split(" ")) {
    // `ring` is in the list because the focus indicator is the one place this
    // codebase has already shipped an unmeasured opacity: `ring-ring/50` read as
    // a style and measured 2.12:1. Lazy prefix, so `dark:hover:bg-input/50` is
    // read as one utility in the `dark:hover` state rather than skipped for
    // having two colons in it.
    const m = /^(.*?:)?(bg|text|border|ring)-([^:]*)$/.exec(cls);
    if (m === null) continue;
    const [, prefix, kind, value] = m;
    out.push({ state: (prefix ?? "").replace(/:$/, ""), kind, value });
  }
  return out;
}

/** The last utility wins, exactly as the cascade would resolve it. */
export function surfaceIn(u: Utility[], state: string, theme: Theme): string | null {
  const hits = u.filter((x) => x.state === state && x.kind === "bg");
  const hit = hits[hits.length - 1];
  return hit === undefined ? null : resolveColour(hit.value, theme);
}

/** A component with no `text-*` token of its own wears the page's ink. */
export function labelIn(u: Utility[], state: string, theme: Theme): string | null {
  const hits = u.filter((x) => x.state === state && x.kind === "text" && x.value in theme);
  const hit = hits[hits.length - 1];
  return hit === undefined ? null : theme[hit.value];
}

/** The edge a component draws in a given state, or `undefined` if it draws none. */
export function edgeIn(u: Utility[], state: string): string | undefined {
  const hits = u.filter((x) => x.state === state && x.kind === "border");
  return hits[hits.length - 1]?.value;
}
