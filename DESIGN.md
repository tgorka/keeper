---
version: '1.0'
name: The Keeper Room
description: >
  Design identity for keeper — a museum keeper's back-of-house workroom at night, never the
  gallery. Dense, labelled, lit for work. One lichen-green lamp over warm graphite. Custody
  supplies the meaning; instrument supplies the impressiveness at almost no pixel cost.
status: draft
created: 2026-08-11
sources:
  - _bmad-output/planning-artifacts/brainstorming/brainstorm-keeper-visual-identity-2026-08-11/brainstorm-intent.md
  - _bmad-output/planning-artifacts/research/design-research-keeper.md
  - _bmad-output/planning-artifacts/research/ui-inventory-keeper.md
colors:
  # --- dark: the workroom at night (hero theme) ---
  ground: '#0d1210'
  surface: '#141a17'
  surface-raised: '#1b221e'
  line: '#28312c'
  text: '#e4e9e4'
  text-dim: '#98a49b'
  text-faint: '#6b756e'
  primary: '#8fc659'
  accent: '#8fc659'
  accent-quiet: '#6c934c'
  on-accent: '#0d1210'
  # --- light: the workroom by day, ink on card stock ---
  ground-light: '#f4f2ec'
  surface-light: '#eceae2'
  surface-raised-light: '#e3e0d6'
  line-light: '#d2cec1'
  text-light: '#1a1f1c'
  text-dim-light: '#5c655e'
  text-faint-light: '#7a817b'
  accent-light: '#41651f'
  accent-quiet-light: '#4b6d2b'
  on-accent-light: '#f4f2ec'
  # --- state: hue-separated from the accent on purpose (see Colors) ---
  ok: '#2fb8a0'
  warn: '#d9a441'
  danger: '#e0625a'
  ok-light: '#0f6e5c'
  warn-light: '#895912'
  danger-light: '#a8322b'
typography:
  display:
    fontFamily: Instrument Sans
    fontSize: 22px
    fontWeight: '600'
    lineHeight: '1.2'
    letterSpacing: -0.01em
  title:
    fontFamily: Instrument Sans
    fontSize: 15px
    fontWeight: '600'
    lineHeight: '1.3'
  body:
    fontFamily: Instrument Sans
    fontSize: 13px
    fontWeight: '400'
    lineHeight: '1.5'
  meta:
    fontFamily: Instrument Sans
    fontSize: 11px
    fontWeight: '400'
    lineHeight: '1.4'
    letterSpacing: 0.02em
  label-caps:
    fontFamily: Instrument Sans
    fontSize: 11px
    fontWeight: '500'
    lineHeight: '1.4'
    letterSpacing: 0.08em
  figure:
    fontFamily: JetBrains Mono
    fontSize: 12px
    fontWeight: '400'
    lineHeight: '1.5'
    fontFeature: 'tnum'
rounded:
  sm: 3px
  DEFAULT: 5px
  lg: 8px
  full: 9999px
spacing:
  unit: 4px
  row-y: 6px
  row-x: 10px
  gutter: 8px
  pane-pad: 12px
components:
  nav-row:
    textColor: '{colors.text-dim}'
    typography: '{typography.body}'
    rounded: '{rounded.DEFAULT}'
    height: 32px
  nav-row-active:
    backgroundColor: '{colors.surface-raised}'
    textColor: '{colors.text}'
  button-primary:
    backgroundColor: '{colors.accent}'
    textColor: '{colors.on-accent}'
    rounded: '{rounded.DEFAULT}'
    height: 32px
  section-label:
    textColor: '{colors.text-faint}'
    typography: '{typography.label-caps}'
  pane-header:
    backgroundColor: '{colors.surface}'
    textColor: '{colors.text}'
    typography: '{typography.title}'
    height: 40px
---

## Overview

**keeper is a Keeper Room.** Not the gallery — the back-of-house workroom where the Keeper of
Manuscripts actually works: dense, labelled, every drawer numbered, one good lamp, nothing on the
walls for visitors. The job title is the product name and it is not a coincidence worth wasting:
a keeper is the custodian of a collection *and* the person who maintains its register.

That metaphor settles the brief's central tension before the argument starts. "Impressive but
usable" reads as a compromise only if impressive means *gallery* — polished, empty, lit for
visitors. A workroom is impressive the way a well-kept workshop is: because everything is to hand,
labelled, and evidently in use. **Density is the aesthetic, not the cost of it.**

Two registers do the work, and both are nearly free in pixels:

- **Custody** supplies meaning — accession numbers, labels, registers, condition, provenance.
  keeper already has all of it (note ids, tags, sync state, revisions, the deferred-work ledger);
  it just renders them as generic UI. Naming them like a register is a copy and typography change.
- **Instrument** supplies the impressiveness — tabular figures, calibration ticks, lit and unlit
  lamps, readouts that are accurate rather than decorative. An instrument looks expensive because
  it is precise, and precision costs no whitespace, which a dense app cannot spend anyway.

**The riskiest thing about this direction, stated at the top so nobody discovers it late:**
character lives almost entirely in the chrome — roughly 4% of the pixels. Executed at 90% this does
not produce a slightly-less-impressive app; it produces an ordinary app with an odd green tint. The
chrome has no margin for error, and that is the argument for the enforcement gate in *Do's and
Don'ts* rather than a style guide nobody re-reads.

## Colors

**The accent is lichen green, and getting there required moving something else.** The owner asked
for green. The obvious objection is real and was measured: green is also the universal "healthy /
online / success" colour, and keeper already spends one — `--bridge-healthy` at OKLCH hue 149°.
Today's `--primary` avoids the collision by not being green at all: it sits at hue 175°, which is
teal, and the separation it buys (ΔE 44.8 normal, 37.8 deuteranopic) is genuinely excellent because
teal opens the S-cone axis while green rides the damaged red-green one.

So the accent could not simply become green with everything else held still. **What frees it is a
fix that is owed anyway:** the bridge status triad currently carries its meaning on hue alone —
mutual luminance ratios 1.03 / 1.52 / 1.47, all far below the 3:1 that SC 1.4.1 requires for a
non-text redundancy, collapsing to ΔE 16.3 under protanopia. A lightness ladder cannot rescue it:
forcing three colours to AA on one background confines them to a narrow luminance band by
definition. **The redundant channel has to be shape**, and once status is legible by shape, status
no longer owns green.

- **Accent `#8fc659` (dark) / `#41651f` (light)** — lichen. Warm yellow-green at hue ~130, chosen
  against the four obvious greens and each rejection is a real one: not phosphor (retro cosplay,
  and it is the first thing every terminal-styled app reaches for), not acid lime (the 2026 trend,
  dated on arrival), not Tailwind emerald (the framework default, which the codebase already leaks
  in two dialogs), and not today's `#3ecfae`, which is fintech mint and not a green.
  Lichen earns the name: it grows only in clean air, grows slowly, grows on stone, and is used to
  date the surfaces it grows on. One lamp in a dark room.
- **Two accent hexes, and that is arithmetic rather than taste.** Solving the contrast formula for
  a luminance band gives L\* ≤ 46.8 to clear 4.5:1 on warm paper and L\* ≥ 51.9 to clear it on
  near-black. **The intersection is empty.** No colour of any hue passes AA on both themes. A
  design that ships one accent hex has silently chosen which theme to fail.
- **State keeps a hue budget, away from the accent.** `ok` moves to teal `#2fb8a0` — the hue the
  accent vacated, and the one measured to survive dichromacy best. `warn` amber, `danger` red.
  Every state colour is paired with a shape: a filled/hollow/dashed lamp, never a bare dot.
- **Neutrals are not grey.** Every neutral carries hue 155 at chroma ≈ 0.014 — a graphite with a
  green cast, invisible as colour and unmistakable as temperature. This is the cheapest line in
  the whole document and the difference between a themed shadcn app and a designed one.

Contrast floors, binding: body text ≥ 4.5:1 in both themes; `text-dim` carries metadata and is held
to 4.5:1 too, because metadata in this app is information rather than decoration. `text-faint` is
reserved for `aria-hidden` glyphs and section labels ≥ 3:1, and never carries a fact.

**Three defects in the shipped code that this palette fixes rather than inherits:** `text-bridge-healthy`
renders at 3.30:1 on white (a plain AA failure, passing at 6.01:1 in dark — the exact one-hex-two-themes
trap); the status triad's hue-only encoding above; and a third, unchosen green — raw `text-emerald-600`
in two settings dialogs, ΔE 23.0 from `--primary` and 17.5 under deuteranopia.

## Typography

**keeper has no typographic identity today, and this is the largest cheap win available.**
`--font-heading` is literally `var(--font-sans)` — an alias, not a face — and it already has 21 call
sites waiting for it. `--font-mono` is never defined at all while 26 files use `font-mono`.

Two faces, self-hosted, subset to latin + latin-ext:

- **Instrument Sans** is the room's voice. A grotesque with slightly narrowed forms — it sets 13px
  UI text densely without the roundness that makes Inter read as a web app. Weights 400/500/600
  only.
- **JetBrains Mono** is the register's voice, and it is used **only where a figure must line up**:
  sizes, counts, timestamps, hashes, paths, the recording clock. Tabular numerals are on
  (`tnum`). A monospace used for atmosphere is terminal cosplay; a monospace used for columns is an
  instrument.

Six steps and no more. The census says the app already lives at five — `text-xs` (372 uses),
`text-sm` (236), `text-[11px]` (39), plus a long tail of ones and twos — so this is codifying a de
facto scale, not imposing one. `label-caps` at 11px/0.08em is the register's label; it is the one
place letterspacing is allowed.

## Layout & Spacing

**A 4px sub-grid, because the app already has one.** 88 uses of half-step gaps (`gap-0.5`,
`gap-1.5`) say the 8px grid was already too coarse for these rows. Rows are 32px, the sub-grid is
4px, and the pane gutter is 8px.

**Columns are the composition.** keeper is columns all the way down — a rail, a list, a panel — and
the design leans into that rather than apologising for it. Every column boundary is a hairline in
`line`, never a shadow and never a gap: a shadow says "these float", a hairline says "these are
drawers in one cabinet".

**Load-bearing dimensions that the design may not move**, because a virtualiser or a stored cookie
depends on them: the note row height, the 48px folded-column strip, the 32px control height, and
the capture window's 320×240 floor. Everything else is negotiable.

## Elevation & Depth

**One raking light, and nothing else.** Depth in this room comes from a single 1px top-edge
highlight on raised surfaces and from hairlines — the way light falls across a workbench, not the
way cards float on a page.

No shadows on panes. No glass anywhere in content. **The glass ban needs its reasoning updated
rather than repeated:** Apple shipped system-level glassmorphism in macOS Tahoe, so on keeper's own
platform this is now a deliberate refusal of a vendor convention rather than a refusal of a trend.
It stands because Apple is already walking it back under legibility pressure — a "Tinted" option
added to appearance controls, and their own developer forums citing visual fatigue during prolonged
use. keeper is a prolonged-use app. Modal scrims are exempt; they are not glass, they are a scrim.

## Shapes

Small radii — 5px default, 3px on inline chips. A workroom's fittings are square-ish; a 12px radius
reads consumer. `full` is for shapes whose ROUNDNESS IS THEIR MEANING: avatars, lamps, and count
badges. A count badge is a pill because a pill reads as a token dropped onto the row rather than a
region of it — and there must be exactly ONE count badge component, because two implementations of
one idea is how a UI starts looking slightly wrong without anyone being able to say why.

**The lamp is the app's one repeated shape.** A 6px round indicator with four states carried by
fill, not by hue alone: filled (live), hollow (idle), dashed ring (working), filled with a bite
taken out (fault). It appears in the sidebar, on sync rows, on bridge cards, on the recording
button, and — at 1:1 with the mark's aperture — inside the icon itself.

## Components

**The mark: an accession tag.** The punched, ruled label a museum keeper ties to every object in a
collection so the thing and its record never come apart. A stepped head with an **eyelet**, a
**rule**, and an **aperture** — three holes, read top to bottom, growing as the tag fills in. No
face, no eyes, no antennae.

Four constraints shaped it, and three of them are hard:

1. **It has to say what keeper is.** A tag is the only one of six measured candidates whose idea is
   the product rather than a nice shape, and its stepped outline is the only asymmetric silhouette
   in a menu bar otherwise full of rectangles — which is what makes it findable in peripheral
   vision rather than merely legible once found.
2. **Android's trademark forbids derivatives of the bugdroid**, which is a dome head with two thin
   antennae and two dot eyes. Any antennae-and-two-eyes robot is legally and visually in its
   shadow. This is why keeper's mark is an object rather than a character.
3. **A mark dies as mush along its edges, so it is authored on the grid it is worn on.** The
   viewBox is **44 units**: 44 → 44px is 1:1 and 44 → 22px is exactly 0.5, and 22px is what macOS
   renders a menu-bar template at. Every coordinate is even, so every edge is a whole pixel at both
   tray sizes and the shipped glyphs measure **zero** antialiased pixels — not "few", zero, which
   is a threshold with no slack in it to quietly spend.

   16px is a different size and cannot also be served: a coordinate lands whole at both 16 and 22
   only if it is a multiple of half the grid, so one of the two has to be chosen. 22px is the one
   that ships; nothing renders a 16px alpha-only template. The 16px number is still measured and
   printed, because it is how this mark is compared to the vendor marks — Ollama 89% mush, the
   sparkle 83%, Zed 72%, all illegible; Perplexity 45% and Cursor 42% survive. The contract that
   separates them holds either way: **the outer contour is one filled shape and the identity lives
   in the holes.** A mark whose idea is its colour (Mistral) dies at 16px even when its geometry
   survives.

   The previous 32-unit grid is instructive about what this actually costs. It was chosen because
   "32 halves to 16 exactly", and the consequence nobody measured was that a 28×26 drawing centred
   in a 44-unit tray canvas is 14×13 px of the 22 available — the mark was a quarter smaller than
   the surface it shipped on, and the starved aperture made `live` and `fault` differ by **three
   pixels** in the menu bar. The tag is 16×18 px, and they differ by ten.
4. **The aperture is a state display**, at the same four states as the lamp. So the mark, the macOS
   tray template family, the bot list and the sync indicator speak one vocabulary instead of four
   unrelated drawings. That is what makes the mark functional instead of decorative, and it is the
   reason this direction is worth more than a nicer picture.

   **"One vocabulary" is a claim about the visual language, not a budget of four files.** The tray
   legitimately ships more glyphs than the lamp has states, because it carries facts the lamp does
   not: sync direction (up / down / both) and paused-versus-warning. Those are shipped, tested
   behaviour and collapsing them would delete information the menu bar currently tells the truth
   about. The rule is one silhouette, one aperture, one state grammar — a vocabulary having more
   words than four is what makes it a vocabulary.

**No face — and neither does any bot.** When conversational agents arrive they get the same tag
with a different aperture state, never eyes and never a smile. keeper's AI is a kept instrument,
not a friend; the product's whole argument is that this runs on your machine.

Every raster is cut from `src-tauri/crates/keeper/icons/mark.svg` by
`bun run scripts/gen-mark-icons.ts` — app icons, ten tray templates, the iOS AppIcon set, and
`favicon.png` in the repo root. The two colours are read from `src/index.css`, so a retheme moves
the icon with it. Do not hand-edit the PNGs.

## Do's and Don'ts

These are enforced by `bun run check:design`, not by review. The gate exists because this identity
lives in 4% of the pixels and a style guide nobody re-reads cannot defend that.

**Never:**

- **No hue 260–330.** No purple, no violet, anywhere. The codebase currently ships five, including
  `--incognito: #6d28d9` (Tailwind violet-700 verbatim) and `--sidebar-primary` (an un-overridden
  shadcn default that is not even used). Incognito becomes a *safelight*: accent withdrawn,
  everything to `text-dim`, a hatched band on the header. A darkroom, not a wizard.
- **No raw colour literals outside the token file.** 19 exist today; 13 are real and must go.
- **No gradients, meshes, glass, blur or shimmer** in content. See *Elevation*.
- **No second green.** One accent, one `ok`, and a hue budget that keeps them apart. `text-emerald-*`
  is banned by name.
- **No colour-only status.** Every state carries a shape. This is a WCAG requirement the app
  currently fails, not a preference.
- **No face on the mark, no emoji in chrome, no sparkles.**

**Always:**

- Tabular figures for anything that lines up in a column.
- A hairline where two columns meet.
- The lamp vocabulary for every state indicator.
- Both themes checked, because one hex provably cannot serve both.
