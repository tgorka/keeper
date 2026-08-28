#!/usr/bin/env node
//
// The design gate. `DESIGN.md` says the Keeper Room identity lives in roughly 4%
// of the pixels, and that executed at 90% it does not yield a slightly-less-
// impressive app but an ordinary one with an odd green tint. A style guide cannot
// defend that, because nobody re-reads a style guide. This can.
//
// Two kinds of rule live here, and the second kind is the reason the file is
// worth its length:
//
//   - BANS, which keep the palette from re-growing the things it was cleaned of.
//   - ARITHMETIC, which recomputes the contrast of every token against every
//     surface of its own theme. keeper shipped `text-bridge-healthy` at 3.30:1 on
//     white while it passed at 6.01:1 in dark — a plain AA failure that no ban
//     could have caught and no reviewer did. That check is rule `contrast`.
//
// Exit 1 with `file:line message` per violation, exit 0 with a one-line summary.
// No dependencies; Node >= 20.

import { readdirSync, readFileSync, statSync } from "node:fs";
import { extname, join, relative } from "node:path";

const ROOT = new URL("..", import.meta.url).pathname;
const TOKENS = "src/index.css";

// ---------------------------------------------------------------------------
// Colour arithmetic. Duplicated from nothing — the app never needs this at
// runtime, only the gate does.
// ---------------------------------------------------------------------------

/** sRGB channel to linear light, per WCAG 2.1. */
const linear = (c) => (c <= 0.04045 ? c / 12.92 : ((c + 0.055) / 1.055) ** 2.4);

function parseHex(hex) {
  let h = hex.replace("#", "");
  if (h.length === 3 || h.length === 4) h = [...h].map((c) => c + c).join("");
  if (h.length !== 6 && h.length !== 8) return null;
  return [0, 2, 4].map((i) => Number.parseInt(h.slice(i, i + 2), 16) / 255);
}

function luminance(hex) {
  const rgb = parseHex(hex);
  if (!rgb) return null;
  const [r, g, b] = rgb.map(linear);
  return 0.2126 * r + 0.7152 * g + 0.0722 * b;
}

function contrast(a, b) {
  const la = luminance(a);
  const lb = luminance(b);
  if (la === null || lb === null) return null;
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

/** OKLCH hue in degrees, for the purple ban. */
function hue(hex) {
  const rgb = parseHex(hex);
  if (!rgb) return null;
  const [r, g, b] = rgb.map(linear);
  const l = Math.cbrt(0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b);
  const m = Math.cbrt(0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b);
  const s = Math.cbrt(0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b);
  const A = 1.9779984951 * l - 2.428592205 * m + 0.4505937099 * s;
  const B = 0.0259040371 * l + 0.7827717662 * m - 0.808675766 * s;
  // Chroma below this is a neutral with a cast, not a colour: the palette's
  // graphites sit at chroma 0.014 on purpose and must not trip the hue ban.
  if (Math.hypot(A, B) < 0.03) return null;
  return ((Math.atan2(B, A) * 180) / Math.PI + 360) % 360;
}

// ---------------------------------------------------------------------------
// Files
// ---------------------------------------------------------------------------

function walk(dir, out = [], exts = [".tsx", ".ts", ".css", ".html"]) {
  for (const entry of readdirSync(dir)) {
    const p = join(dir, entry);
    if (statSync(p).isDirectory()) {
      if (entry === "node_modules" || entry === "gen") continue;
      walk(p, out, exts);
    } else if (exts.includes(extname(p))) {
      out.push(p);
    }
  }
  return out;
}

const violations = [];
const report = (file, line, rule, message) =>
  violations.push({ file: relative(ROOT, file), line, rule, message });

/**
 * Comment and issue-reference stripping. Without this the gate is useless: the
 * codebase cites upstream issues as `matrix-rust-sdk#3935` and `tauri#14371`,
 * which a naive `#[0-9a-f]{3,8}` reads as colours. A `#` that follows a word
 * character is a reference, never a colour.
 */
const stripNoise = (text) =>
  text
    .replace(/\/\*[\s\S]*?\*\//g, (m) => m.replace(/\S/g, " "))
    .replace(/(^|[^:])\/\/.*$/gm, (m) => m.replace(/\S/g, " "));

// ---------------------------------------------------------------------------
// Rules
// ---------------------------------------------------------------------------

const BANNED_FAMILIES =
  "red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose|slate|gray|zinc|neutral|stone";

/** A scrim is not glass. `DESIGN.md` → Elevation & Depth exempts it by name. */
const isScrim = (line) => /fixed\s+inset-0/.test(line) && /\bz-\d+/.test(line);

/**
 * keeper is a MESSENGER. Emoji are its content, not its decoration, so the ban
 * in `DESIGN.md` ("no emoji in chrome") cannot be read as a ban on the product
 * handling emoji at all. These are the modules where an emoji IS the payload —
 * a reaction picker and the shortcode table behind it. Everywhere else a glyph
 * like ⚠ or ⚡ is decoration standing in for the lamp vocabulary, and that is
 * exactly what the rule is for.
 */
const EMOJI_IS_CONTENT = /(^|\/)(lib\/emoji\/|.*reaction-)/;

/**
 * A trailing vertical edge, and the cancel that keeps it off the window frame.
 *
 * `DESIGN.md` → Elevation & Depth: the earlier sibling owns its trailing edge
 * and the LAST child cancels, because an edge with nothing beyond it is a line
 * against the window. Every full-width pane in this app drew one; exactly one
 * file had noticed, and its `last:border-r-0` is the spelling copied here.
 *
 * A variant-prefixed edge is exempt — `data-[side=left]:border-r` in a Sheet is
 * already conditional, which is what the cancel is for. Hence the lookbehind:
 * the token must not be preceded by a `:` or by more class characters, so
 * `border-r-0`, `border-r-2` and `hover:border-r` are all left alone.
 */
const DRAWS_RIGHT_EDGE = /(?<![\w:-])border-r(?![-\w])/;
const CANCELS_RIGHT_EDGE = /\blast:border-r-0\b/;

/**
 * Components that own an edge of their own, and what a caller may not spell
 * back at them.
 *
 * `PaneHeader` owned no edge and no height until this rule's story, so each of
 * its three callers spelled its own: two `border-b`s and one
 * `border-border border-b`, over heights of 40px, 40px and 44px, all three
 * nominally implementing the same `pane-header.height: 40px`. A boundary is a
 * property of the thing that owns it; a caller that draws it again draws it
 * twice.
 *
 * Only the props before the first NESTED element are inspected: past that the
 * lines belong to a slot's own content, whose padding and height are its own
 * business. A caller composing its class through `cn()` is not inspected
 * either — see the note on this rule's limits at the call site.
 */
const EDGE_OWNERS = [
  {
    tag: "PaneHeader",
    forbidden: /\b(?:border-b|border-y|py-[\d.]+|h-\d+)\b/,
    owns: "its own bottom edge and its 40px height",
  },
];

function checkSource(file) {
  const raw = readFileSync(file, "utf8");
  const clean = stripNoise(raw);
  const lines = clean.split("\n");
  lines.forEach((line, i) => {
    const n = i + 1;

    // 1. Raw colour literals. Tokens carry colour; components spend it.
    for (const m of line.matchAll(
      /(^|[^\w&])(#(?:[0-9a-fA-F]{3,4}|[0-9a-fA-F]{6}|[0-9a-fA-F]{8}))\b/g,
    )) {
      report(file, n, "raw-color", `raw colour literal ${m[2]} — use a token from ${TOKENS}`);
    }
    for (const m of line.matchAll(/\b(rgba?|hsla?|oklch)\(/g)) {
      report(file, n, "raw-color", `raw ${m[1]}() literal — use a token from ${TOKENS}`);
    }

    // 2. Tailwind palette classes. A framework default is somebody else's taste,
    //    and `emerald` in particular is the third unchosen green.
    for (const m of line.matchAll(
      new RegExp(
        `\\b(?:bg|text|border|ring|from|to|via|fill|stroke|shadow)-(?:${BANNED_FAMILIES})-\\d{2,3}\\b`,
        "g",
      ),
    )) {
      report(
        file,
        n,
        "palette-default",
        `${m[0]} is a framework palette default — use a semantic token`,
      );
    }

    // 3. Gradients, glass and shimmer. Scrims are exempt; see above.
    for (const m of line.matchAll(
      /\b(bg-gradient[\w-]*|bg-linear[\w-]*|bg-radial[\w-]*|bg-conic[\w-]*)\b/g,
    )) {
      report(file, n, "no-gradient", `${m[1]} — the room has one raking light, not gradients`);
    }
    for (const m of line.matchAll(
      /\b(backdrop-blur[\w-]*|backdrop-filter|blur-(?!none)[\w-]+)\b/g,
    )) {
      if (isScrim(line)) continue;
      report(
        file,
        n,
        "no-glass",
        `${m[1]} outside a modal scrim — see DESIGN.md → Elevation & Depth`,
      );
    }

    // 4. The mark has no face, and neither does the chrome.
    if (!EMOJI_IS_CONTENT.test(relative(ROOT, file))) {
      for (const m of line.matchAll(/[\u{1F300}-\u{1FAFF}\u{2728}\u{2600}-\u{26FF}]/gu)) {
        report(
          file,
          n,
          "no-emoji",
          `emoji ${m[0]} is decoration — use the lamp vocabulary or an icon`,
        );
      }
    }

    // 5. Opacity modifiers on text colour. A token is verified at a contrast
    //    ratio; `/60` discards that verification silently and the result still
    //    looks deliberate in review. Three of the four sites that existed when
    //    this rule was written measured 2.45:1, 3.88:1 and 4.26:1 — a 75% defect
    //    rate on one pattern. The fourth was a gauge track, which is why the
    //    escape hatch exists and why it has to say what it is for.
    if (!/design-allow opacity/.test(raw.split("\n")[i])) {
      for (const m of line.matchAll(/\btext-[a-z-]+\/\d{1,3}\b/g)) {
        report(
          file,
          n,
          "opacity-on-text",
          `${m[0]} discards the contrast its token was verified at — use a quieter token (faint is held to 3:1)`,
        );
      }
    }

    // 6. The type scale is six steps. Anything else is a habit, not a decision.
    //    Two exemptions, both measured rather than assumed: a glyph sized to the
    //    chip that contains it is a GLYPH METRIC (the lamp is 6px for the same
    //    reason), and an emoji is a picture, not type.
    const isGlyphInAChip = /\bsize-\d|\bh-\d/.test(line);
    // An emoji is almost never on the same line as the class that sizes it: the
    // span carries the class and the next line renders `{emoji}`. Look at the
    // render window, not just the class, or the rule bans the three places where
    // the app legitimately draws an emoji at picture size.
    const window = lines.slice(i, i + 3).join("\n");
    const isPicture =
      /[\u{1F300}-\u{1FAFF}\u{2728}\u{2600}-\u{26FF}]/u.test(window) || /\bemoji\b/i.test(window);
    if (!isPicture) {
      for (const m of line.matchAll(/\btext-(lg|xl|[2-9]xl|base)\b/g)) {
        // Safari refuses to skip zoom below 16px, so a phone-tier input floor of
        // `text-base` paired with `md:text-sm` is load-bearing, not a stray size.
        if (m[1] === "base" && /md:text-sm/.test(line)) continue;
        report(
          file,
          n,
          "type-scale",
          `text-${m[1]} is outside the six steps — use display/title/sm/xs/meta`,
        );
      }
      if (!isGlyphInAChip) {
        for (const m of line.matchAll(/\btext-\[(\d+)px\]/g)) {
          report(
            file,
            n,
            "type-scale",
            `text-[${m[1]}px] is an arbitrary size — name it or use a step`,
          );
        }
      }
    }

    // 7. Seams. `DESIGN.md` → Elevation & Depth: a seam has exactly one owner,
    //    the earlier sibling owns its trailing edge, and the last child
    //    cancels. Drawn by BOTH neighbours a seam is 2px; drawn by NEITHER it
    //    disappears. Both halves shipped, which is why this is a rule and not
    //    a paragraph.
    //
    //    WHAT THIS CANNOT SEE, stated here rather than discovered later: it
    //    cannot tell whether two ADJACENT elements both draw the shared seam,
    //    because adjacency is a runtime tree fact assembled somewhere else —
    //    which pane is last in `app-shell` is a `primaryView` branch, and a
    //    column's neighbour is a hook's return value in a third file. So the
    //    genuinely double-drawn seam is caught by the two halves that ARE
    //    local: an edge nobody cancels, and a component's edge re-spelled by
    //    its caller. The third case a reviewer still has to look for is two
    //    different files drawing the two sides of one boundary.

    //    (a) An edge with nothing beyond it. `last:border-r-0` costs exactly
    //        nothing when there IS a neighbour, so it is required rather than
    //        suggested — that way a pane that gains a right-hand neighbour
    //        later (the Files tree did, when the panel strip arrived) grows
    //        its seam automatically instead of having carried a line against
    //        the window in the meantime.
    //
    //        Deliberately NOT extended to `border-b`. A bottom edge at the end
    //        of a scrolling column is a hairline against content, not against
    //        the window frame, and most of this app's 60 `border-b`s really
    //        are mid-stack bands — a rule that flagged them all would be
    //        wrong far more often than right, and a gate that cries wolf is a
    //        gate people learn to skip.
    if (DRAWS_RIGHT_EDGE.test(line) && !CANCELS_RIGHT_EDGE.test(line)) {
      report(
        file,
        n,
        "seam-uncancelled",
        "border-r with no `last:border-r-0` — the last child cancels, or the edge is a line against the window",
      );
    }

    //    (b) A component's own edge, spelled again by its caller.
    for (const owner of EDGE_OWNERS) {
      if (!new RegExp(`<${owner.tag}\\b`).test(line)) {
        continue;
      }
      for (let ahead = i + 1; ahead < Math.min(i + 12, lines.length); ahead += 1) {
        const prop = lines[ahead];
        // The first nested element ends the props this rule owns.
        if (/<[A-Za-z]/.test(prop)) {
          break;
        }
        const passed = /className="([^"]*)"/.exec(prop);
        if (passed === null) {
          continue;
        }
        const offence = owner.forbidden.exec(passed[1]);
        if (offence !== null) {
          report(
            file,
            ahead + 1,
            "seam-doubled",
            `${owner.tag} owns ${owner.owns}; \`${offence[0]}\` here states it a second time — pass horizontal padding only`,
          );
        }
        break;
      }
    }
  });
}

/**
 * The token file is held to a higher standard than the components: it is the one
 * place colour may be written literally, so it is the one place the arithmetic
 * can be checked.
 */
function checkTokens() {
  const file = join(ROOT, TOKENS);
  const css = readFileSync(file, "utf8");
  const lineOf = (needle) => css.slice(0, css.indexOf(needle)).split("\n").length;

  const block = (name) => {
    const start = css.indexOf(name);
    if (start === -1) return {};
    const body = css.slice(start, css.indexOf("\n}", start));
    const out = {};
    for (const m of body.matchAll(/--([\w-]+):\s*(#[0-9a-fA-F]{3,8})\s*;/g)) out[m[1]] = m[2];
    return out;
  };

  const themes = { light: block(":root {"), dark: block(".dark {") };

  // 4a. Light/dark parity. A token defined in one theme and not the other is the
  //     one-hex-two-themes trap: AA needs L* <= 46.8 on warm paper and L* >= 51.9
  //     on near-black, and the intersection is EMPTY. One hex cannot serve both.
  for (const [name] of Object.entries(themes.light)) {
    if (!(name in themes.dark)) {
      report(
        file,
        lineOf(`--${name}:`),
        "theme-parity",
        `--${name} is defined in :root but not in .dark`,
      );
    }
  }
  for (const [name] of Object.entries(themes.dark)) {
    if (!(name in themes.light)) {
      report(
        file,
        lineOf(`--${name}:`),
        "theme-parity",
        `--${name} is defined in .dark but not in :root`,
      );
    }
  }

  // 4b. No purple. Hue 260-330 is banned outright; the codebase shipped five.
  for (const [theme, tokens] of Object.entries(themes)) {
    for (const [name, hex] of Object.entries(tokens)) {
      const h = hue(hex);
      if (h !== null && h >= 260 && h <= 330) {
        report(
          file,
          lineOf(`--${name}:`),
          "no-purple",
          `--${name} (${theme}) is ${hex}, OKLCH hue ${h.toFixed(0)}° — purple is banned`,
        );
      }
    }
  }

  // 4c. Contrast. Every foreground token must clear its floor against EVERY
  //     surface of its own theme, because a token that passes on the background
  //     and fails on a card has simply moved the failure somewhere less obvious.
  const SURFACES = {
    light: ["background", "card", "secondary"],
    dark: ["background", "card", "secondary"],
  };
  // 3:1 is for glyphs and section labels that carry no fact; everything that can
  // carry a fact is held to 4.5:1, metadata included.
  const FLOORS = { faint: 3 };
  const FOREGROUNDS =
    /^(foreground|.*-foreground|muted-foreground|primary|destructive|held|incognito|recording-red|bridge-.*|text-.*)$/;

  for (const [theme, tokens] of Object.entries(themes)) {
    for (const [name, hex] of Object.entries(tokens)) {
      if (!FOREGROUNDS.test(name)) continue;
      // `*-foreground` tokens sit on their OWN colour, not on a surface.
      if (name.endsWith("-foreground") && name !== "muted-foreground") {
        const own = tokens[name.replace(/-foreground$/, "")];
        if (!own) continue;
        const r = contrast(hex, own);
        if (r !== null && r < 4.5) {
          report(
            file,
            lineOf(`--${name}:`),
            "contrast",
            `--${name} on --${name.replace(/-foreground$/, "")} (${theme}) is ${r.toFixed(2)}:1, needs 4.5`,
          );
        }
        continue;
      }
      const floor = FLOORS[name] ?? 4.5;
      for (const surfaceName of SURFACES[theme]) {
        const surface = tokens[surfaceName];
        if (!surface) continue;
        const r = contrast(hex, surface);
        if (r !== null && r < floor) {
          report(
            file,
            lineOf(`--${name}:`),
            "contrast",
            `--${name} on --${surfaceName} (${theme}) is ${r.toFixed(2)}:1, needs ${floor}`,
          );
        }
      }
    }
  }
}

// ---------------------------------------------------------------------------

/**
 * The frontend is not the only place that spends colour, and assuming it was is
 * how `#3ecfae` — the teal DESIGN.md rejects by name — survived in `tray.rs` as
 * the colour the tray glyph is repainted in on Linux and Windows. A fourth
 * unchosen green, shipping, invisible to a gate that only walked `src/`.
 *
 * Rust and the generator scripts get the colour-literal rule only: the type
 * scale, glass and Tailwind palettes are meaningless outside the web layer.
 */
function checkNonWebColour(file) {
  const raw = readFileSync(file, "utf8").split("\n");
  const lines = stripNoise(raw.join("\n")).split("\n");
  lines.forEach((line, i) => {
    // The escape hatch, same convention as `design-allow opacity`: a colour that
    // is a FORMAT requirement rather than a palette choice — macOS template ink
    // must be pure black — may stay, but it has to say why within three lines of
    // itself, where the next reader will actually see it.
    if (/design-allow color/.test(raw.slice(Math.max(0, i - 3), i + 1).join("\n"))) return;
    for (const m of line.matchAll(/(^|[^\w&])(#(?:[0-9a-fA-F]{6}|[0-9a-fA-F]{8}))\b/g)) {
      report(
        file,
        i + 1,
        "raw-color",
        `raw colour literal ${m[2]} outside the token file — the palette has one home`,
      );
    }
  });
}

const sources = walk(join(ROOT, "src")).filter(
  (f) =>
    !f.endsWith(TOKENS.replace("src/", "src/")) &&
    !/\.test\.[tj]sx?$/.test(f) &&
    !f.includes("/test/"),
);
for (const f of sources) checkSource(f);

const nonWeb = [
  ...walk(join(ROOT, "src-tauri"), [], [".rs"]),
  ...walk(join(ROOT, "scripts"), [], [".ts", ".mjs"]),
].filter(
  (f) => !/\.test\.[tj]s$/.test(f) && !f.includes("/tests/") && !f.endsWith("check-design.mjs"),
);
for (const f of nonWeb) checkNonWebColour(f);

checkTokens();

if (violations.length === 0) {
  console.log(
    `check:design — clean (${sources.length} web + ${nonWeb.length} native files, ${TOKENS} arithmetic verified)`,
  );
  process.exit(0);
}
for (const v of violations) console.error(`${v.file}:${v.line}  [${v.rule}] ${v.message}`);
console.error(`\ncheck:design — ${violations.length} violation(s)`);
process.exit(1);
