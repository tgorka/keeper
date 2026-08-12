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
button, and — at 1:1 with the mark's mouth — inside the icon itself.

## Components

**The mark: the hex-bot.** A honeycomb cell with a face. keeper → beekeeper → the hive: a hive is
many cells kept as one structure, which is literally what this product is — many networks bridged
into one kept archive — and the keeper is the one who tends it. The face is the **owner's
decision**, made with the earlier no-face draft on the table: the eyes are what make the mark a
someone in the menu bar rather than another rectangle, and the mouth is what makes it an
instrument — it is the state display. An earlier draft of this section argued for a faceless
accession tag; the research it rested on (grid arithmetic, the vendor-mark mush survey, the
one-vocabulary rule) all carries over — the silhouette changed, not the discipline.

Four constraints shaped it, and three of them are hard:

1. **It has to say what keeper is.** The cell is the product: one of many, kept, tended, part of a
   structure. A hexagon is also the only silhouette of its kind in a menu bar otherwise full of
   rectangles and circles — findable in peripheral vision rather than merely legible once found.
2. **Android's trademark forbids derivatives of the bugdroid**, which is a dome head with two thin
   antennae and two dot eyes. The hex-bot stays out of its shadow structurally: the head is a
   flat-topped hexagon, not a dome; **no template wears an antenna at all**; and the coloured app
   icon wears exactly one — a stem tipped with a smaller hexagon, the mark broadcasting itself,
   pointedly not a pair of feelers. Silhouette, count and grammar all differ.
3. **A mark dies as mush along its edges, so it is authored on the grid it is worn on.** The
   viewBox is **44 units**: 44 → 44px is 1:1 and 44 → 22px is exactly 0.5, and 22px is what macOS
   renders a menu-bar template at. The cell's horizontal bands sit on even coordinates and land on
   whole pixels at both tray sizes; the four diagonals run at slope exactly 1:2, so their fringe
   is one deterministic repeating pattern — a crisp angle, not mush.

   The rest of the face is **round, because the approved comp is round** — rounded cell corners,
   round eyes, round mouth ink. A first revision squared all of that off chasing whole-pixel
   purity, and the owner rejected it in the menu bar: the identity is the comp, not the raster
   arithmetic. What the whole-pixel doctrine actually defended — determinism — is kept in full:
   every glyph's partial-pixel count, enclosed-hole count and ink box at 22px is **pinned
   exactly** in the generator and its test, so any drift off the authored geometry still changes
   a number and fails. The zero-gate did not relax; it re-based onto the comp.

   16px cannot also be served (44/16 lands nothing whole; no grid serves 16 and 22 at once, and
   nothing ships a 16px alpha-only template). The 16px number is still measured and printed,
   because it is how this mark is compared to the vendor marks — Ollama 89% mush, the sparkle 83%,
   Zed 72%, all illegible; Perplexity 45% and Cursor 42% survive. The contract that separates them
   holds for the cell too: **the identity lives in the contrast between one closed contour and
   what it keeps inside** — a cell with things kept in it rather than a tag with holes punched
   out, which is, after all, the product. A mark whose idea is its colour (Mistral) dies at 16px
   even when its geometry survives.
4. **The face and its corners are a slotted state display.** The mouth speaks the lamp's four
   states: filled (`live` — a solid dot, on the record), hollow (`idle` — calm, empty), dashed
   (`working` — three dots, the "typing…" idiom, which a messenger gets to claim as its own),
   broken (`fault` — an exclamation grounded on the chin). The eyes are the identity and never
   carry information — with one earned exception: they close to two lids for `paused`, because a
   resting bot is what paused means. The **bottom-left corner** is the transport slot: a badge
   seated in the canvas corner the hexagon's cut leaves free, with a halo bitten out of the ring
   so it reads as a token pinned beside the cell — a hollow ring for sync armed, arrows for
   direction (up / down / both), an exclamation for sync warning. The **top-right corner** is
   reserved for the unread-messages dot and its `set_title` count (approved, not yet wired in
   `tray.rs`). The cell sits at a fixed translate in a fixed 44-unit canvas and macOS centres the
   bitmap, so a corner badge cannot move the head.

   So the mark, the macOS tray template family, the bot list and the sync indicator speak one
   vocabulary instead of four unrelated drawings — one silhouette, one face, two corner slots,
   one state grammar. A vocabulary having more words than the lamp's four is what makes it a
   vocabulary: sync direction and paused-versus-warning are shipped, tested behaviour, and
   collapsing them would delete information the menu bar currently tells the truth about.

**The face is the mark's, and only the mark's.** The smile exists on the coloured app icon alone —
in the menu bar the mouth is an instrument and must be empty at rest. No emoji in chrome, no
sparkles, and conversational agents do not each get a mascot: a bot in keeper is a kept
instrument, and it wears the same cell.

Every asset is cut from `src-tauri/crates/keeper/icons/mark.svg` by
`bun run scripts/gen-mark-icons.ts` — app icons, ten tray templates, the iOS AppIcon set, and
`favicon.png` plus `favicon.svg` in the repo root. The coloured tile is the owner's approved
comp: the mark in paper (light `--background`) on the healthy-hive green (light
`--bridge-healthy` — keeper's original brand green, which is where that colour now lives in the
palette), with neighbour cells ghosted in the dark ground at low opacity. All three colours are
read from `src/index.css`, so a retheme moves the icon with it. Do not hand-edit the PNGs.

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
- **No face anywhere but the mark, no emoji in chrome, no sparkles.** The hex-bot's face is the
  brand and it is singular: nothing else in the product grows eyes, and the mark itself smiles
  only on the coloured app icon — in the menu bar its mouth is a state display and stays empty at
  rest.

**Always:**

- Tabular figures for anything that lines up in a column.
- A hairline where two columns meet.
- The lamp vocabulary for every state indicator.
- Both themes checked, because one hex provably cannot serve both.
