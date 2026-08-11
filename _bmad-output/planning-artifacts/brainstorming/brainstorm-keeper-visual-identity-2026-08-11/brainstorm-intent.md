---
topic: keeper's visual identity and UI direction
source: _bmad-output/planning-artifacts/brainstorming/brainstorm-keeper-visual-identity-2026-08-11/.memlog.md
date: 2026-08-11
status: draft
---

# keeper visual identity - intent

## Direction

**The Keeper Room.** keeper is a museum job title (Keeper of Manuscripts, Keeper of Prints and Drawings), so the metaphor is the back-of-house workroom of a collection, at night, lit by one lamp - never the gallery. Galleries are polished, empty and lit for visitors; workrooms are dense, labelled and lit for work, and that distinction settles the impressive-vs-usable fight before it starts. Two registers fuse into one object: **custody** (accession numbers, labels, the register, condition reports, provenance, reversibility) supplies meaning, **instrument** (calibration ticks, readouts, lit/unlit lamps, the visor, tabular figures) supplies impressiveness at near-zero pixel cost. The brief is not to invent a personality - keeper's prose already has one (`panel-strip.tsx:49, 77, 91, 121`: full sentences, lowercase brand, no error codes, no marketing adjectives) - it is to give the pixels the voice the copy already has.

## Colour

Every neutral takes hue 155 chroma 0.014, never chroma 0. One search-and-replace; it is the difference between a themed shadcn app and a designed app.

**Dark (default), contrast against ground:**

| token | value | contrast |
|---|---|---|
| ground | `#0c120e` | - |
| raised | `#171e19` | - |
| overlay | `#222a25` | - |
| border | `#2d3530` | - |
| text | `#e1e9e2` | 15.3:1 |
| text-dim | `#99a19a` | 7.14:1 |
| text-faint | `#636b65` | 3.45:1 |
| accent (lichen) | `oklch(0.80 0.15 132)` = `#99d267` | 10.62:1 |

text-dim at 7.14:1 legally carries every timestamp, path and count; text-faint is decorative only, never information; the accent is text-grade, not decoration.

**Light theme is PAPER, not white** - the same room by daylight, not a second design: ground `#f4f8f3`, raised `#eaeee9`, ink `#131a14` (16.5:1), accent deepened to `#396d0a` (5.81:1 on paper).

Retire `#3ecfae` (`src/index.css:153`, mint/aqua at hue ~175), `#0f6e5c` (`src/index.css:73`), `oklch(1 0 0)` (`src/index.css:67`).

**Semantics.** Green is the brand, therefore **green cannot mean OK**: healthy is never coloured, only a lit neutral lamp plus text-dim. That retires `#16a34a` (`src/index.css:95`) and unbinds `--swipe-read` from `--primary` (`src/index.css:105`). **Amber is status only** - `--held #b45309` (`src/index.css:90`), degraded `#d97706` (`src/index.css:96`) - never a brand colour, because amber is tgsite's. **Red budget: exactly two roles** - the recording lamp (`--recording-red`, `src/index.css:94`) and destructive confirmation; nowhere else, ever. Colour marks only a deviation or the user's own attention.

Incognito drops the violet for **safelight**: the accent disappears entirely, everything falls to text-dim, the pane header carries a 45-degree hatch band. Account hues stay (eight accounts need telling apart) but come off the rainbow: drop violet and magenta, cut chroma, add a second axis (solid / dashed / double 3px edge bar) so the set survives colour-blindness and monochrome print.

## Typography

**Two faces.** One proportional for prose and labels; one **mono for every machine-generated string** - paths, ids, timestamps, byte sizes, durations, hashes, git refs, room ids - all of which currently set in the OS UI stack (`src/index.css:8-9`, where `--font-heading` is literally `var(--font-sans)`). The mono face is the accession number made visible: the label is printed, the note is handwritten.

Share tgsite's mono family (JetBrains Mono) for kinship; take a **different** proportional face so the products are siblings, not clones. **Inter is banned by name**; display type is never tracked tighter than `-0.01em`.

**Micro-label spec** (the specimen tag, and the pane-header grammar on all six surfaces): mono, uppercase, ~10px, `+0.08em` tracking, text-dim. Costs 14px of height, buys the identity.

**Tabular lining figures everywhere** - a usability fix for columned sizes, durations, counts and dates, and the most legible signal that someone designed this.

## The mark

A **visored sentinel head**: squared, slightly wider than tall, near-square corners (2px on a 24 grid), ONE horizontal visor slot, no eyes, no mouth, no antenna, no neck. One asymmetry so it cannot be a template: a hallmark notch bitten out of the upper-left corner, the keeper's punch mark (repeated exactly once in the UI, as the launch window's top-left corner cut). The head is the same rectangle as the app's panes.

No face, because keeper's AI is a **kept instrument, not a friend**. This pre-answers the bots surface: bots render as instruments with a state, listed like accounts (name, job, state, lamp), never an avatar or portrait; bot messages use the same dense row grammar as human ones, marked by a mono author label and a visor glyph.

**The visor is a state display:**

| state | visor |
|---|---|
| idle | full bar |
| syncing | broken / dashed bar |
| error | short centred bar |
| recording | filled dot |

One vocabulary across mark, tray family, bot list and sync indicator. A running bot shows the visor mid-state; a waiting bot unlit.

**Production constraint:** the visor is the only place the accent appears, so the rest is a single flat shape - because the mark must survive as a **22px monochrome macOS template icon**. The repo's `tray-idle/sync/recording/error-template.png` family is the real constraint, so the four states must be locked before any icon is redrawn.

Rail glyphs and the mark are ONE custom family on a 20 grid at stroke 1.25. lucide is permitted only below rail level, never beside a custom glyph.

## Where character lives

**Character lives in the chrome and is banned from the content.** Chrome = rail, pane headers, status band, command palette, tray, launch, about, empty states. Content = message list, note body, file table, deliberately boring. In content the accent appears only as a 2px edge bar, a selection, or a caret. Continuity is material: same chroma-0.014 neutrals, same hairline weight, same 4px grid; only ink density changes.

| device | rule |
|---|---|
| Label strips | pane header as mono uppercase name, hairline, count in tabular figures, right-aligned state; one grammar across all six surfaces |
| Calibration ticks | marks at a fixed interval along the vertical pane divider; one pixel of width |
| Status band | one line at the very bottom, mono: vault, branch, sync state, active bot. Zero pixels when there is nothing to say |
| Focus-by-light | the focused pane is lit from the upper left; unfocused panes lose their 1px top-edge highlight (~6% white). Drop shadows only on genuinely floating layers (dialog, palette, tooltip, context menu) |
| Ledger rule | a hairline every five rows in long tables; not zebra striping |
| Motion budget | exactly ONE animating element on screen at any time, always the sync/activity indicator; everything else is a `<=160ms` state transition; the bot's thinking indicator is a single static caret in the accent |
| Grain | ~2% tooth on chrome only; viewers render on a strictly neutral plate inside the tinted chrome |
| Time separation | silent while you work, expressive at transitions. Empty states get generous space, mono set larger, the mark at low contrast, and the sentences that already exist. Launch and About may be lavish, seen once each; About is an object label (name, date, maker, materials, accession number = the build hash) |
| Rigour as the flex | row height published as a token, everything on a 4px grid |

## Binding refusals

Each line is phrased to be enforceable by a check script.

- No hue in the range 260-330 anywhere. Named instances: `src/index.css:92` and `src/index.css:172` (incognito `#6d28d9` / `#a78bfa`), `src/index.css:190` (`--sidebar-primary: oklch(0.488 0.243 264.376)`, the unused shadcn default), `src/index.css:141` and `src/index.css:203` (account-hue-5 at 275), `src/index.css:142` and `src/index.css:204` (account-hue-6 at 320).
- No `backdrop-filter`, no vibrancy, no glass on keeper's own surfaces.
- No gradients in app chrome; no mesh gradients, no blurred colour blobs, no abstract background art.
- No shimmer and no skeleton sweep. Loading is a dimmed static row or one word.
- No bouncing-dot indicators; no confetti, no success animation, no celebratory motion.
- The mark has no eyes, no mouth, no smile arc, no two dots, no antennae, no ears, no rivets, no bolts, no 3/4 perspective render, no gloss highlight, no gradient body, no idle or blink animation.
- No emoji in UI chrome; the sparkle glyph is banned by codepoint.
- `Inter` is banned by name. Display tracking never tighter than `-0.01em`.
- No pure black and no pure white: `#000`, `#fff`, `oklch(1 0 0)` (`src/index.css:67`) forbidden as ground or ink.
- No neutral with chroma 0.
- `--radius` drops from 10px (`src/index.css:109`) to 4px for controls and 6px for floating layers. Buttons are rectangles at 4px; the only pill is a status chip, if one survives.
- No chat bubbles: no bubble tails, no alternating bubble fills, no avatar circles.
- No hero cards and no accent-tinted summary banner; surfaces open on their content.
- No three-across card grid with soft shadows.
- No illustrated people, spot illustrations or stock 3D renders.
- No marketing adjectives in microcopy.
- No accent fill at full chroma over an area larger than a button.
- No drop shadow on inline elements.
- No grain over a media viewer, video frame or PDF.
- No terminal-phosphor cosplay: no `#00ff00`, no scanlines, no glow. No acid lime or chartreuse. No `#10b981` (Tailwind emerald-500).
- No serif, no beige, no ornament, no brass - the room is the workroom, never the gallery.
- No sound except recording start/stop.
- No irreversible action without a named undo; every destructive or outbound action names what will happen to which object.

## Open questions and risks

- **Riskiest thing (named):** banishing character from content means the app's beauty is carried by roughly 4% of its pixels. Chrome executed at 90% instead of 100% is not a slightly-less-impressive app - it is an ordinary app with an odd green tint.
- **Accessibility caveat:** focus-by-light is the only genuinely novel interaction detail in the set and the riskiest for accessibility. It MUST be additive to a real focus ring, never a replacement.
- The frame/content split can read as two designs bolted together; mitigation is material continuity.
- A green-tinted ground fights the file and media viewers; mitigation is the 0.014 chroma cap plus neutral viewer plates, untested against real media.
- "Museum" risks reading as dusty or precious, death for a tool that must feel fast.
- Enforcement decays without a check script, which does not exist yet.
- Open: which proportional face pairs with JetBrains Mono without being Inter; whether any status chip survives; the exact account-hue set after dropping 275 and 320.
- Test: print a screenshot in greyscale. If hierarchy survives, colour is doing only its proper job.

## Sequencing

**Mechanical, and together these deliver most of the perceived redesign:**
1. Neutrals get chroma 0.014 at hue 155; install the ground/raised/overlay/border/ink ramp and the paper light theme.
2. Two-face typography: mono on every machine-generated string, micro-label spec, tabular figures.
3. Token refusals: delete every violet instance named above, retire `#3ecfae` / `#0f6e5c` / `#16a34a` / `oklch(1 0 0)`, drop radius to 4/6px.
4. Label strip, ledger rule, status band - cheap CSS, high system-legibility.

**Expensive, follows:**
5. The mark - lock the four visor states first, against the 22px monochrome tray family.
6. The custom rail glyph family on the 20 grid at stroke 1.25.
7. Calibration ticks, focus-by-light, safelight incognito, the reworked account-hue axis.
8. Surface work: contact-sheet Recordings, herbarium-sheet Files, logbook Sync, the bots list, the object-label About panel.
