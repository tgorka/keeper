---
title: 'Story 61.7: pinned bots with an identity'
type: 'feature'
created: '2026-09-02'
status: 'review'
baseline_revision: '9e06c2b'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
---

binds: FR-383 — AD-155; `DESIGN.md:168` ("no colour of any hue passes AA on both themes") and `:172` ("every state colour is paired with a shape") consumed; `registry.rs:260-296` (`reorder_pins`) and `layout/pins-strip.tsx` reused
depends on: 61.4 (`bot-pins-strip.tsx`'s prop shape `{bots, selectedBotId, onSelect}`, `botsStore.applyBots`) and 61.1 (`pin_order`, `identity_shape`, `identity_colour`, `identity_mark`, `reorder_bots`)

<intent-contract>

## Intent

**Problem:** The owner asked to pin bots with "shape, color and image/emoticon". Hand-ordered pins and a chosen icon exist; a chosen colour
does not, and `DESIGN.md` already records why an unconstrained pick cannot ship. **A pin is a shape and a mark first, a colour second.**
**Approach:** Four shapes, seven inks, one mark. The shapes are a closed set worn on the mark's hexagon. The inks are a bounded palette of
`--bot-ink-<name>` hex pairs in `src/index.css`, one per theme, every member recomputed by a new `identity-palette` rule in
`scripts/check-design.mjs` against `--background`, `--card` and `--secondary` of both themes at a 4.5:1 floor. The mark is an existing
`space-icons.ts` key or a typed grapheme. Order is hand-set and persisted through `plan_reorder` then `store::reorder_bots`' one `BEGIN
IMMEDIATE`. **A free colour picker is refused, not deferred**, and `DESIGN.md` now says so in its own voice.

## Boundaries & Constraints

**Always** (each invariant with the tests that pin it):
- `BOT_SHAPES = ["filled", "hollow", "dashed", "notched"]`: solid cell; 2u stroke; stroke with dasharray 4.045/4.045 (the 64.72u perimeter
  divides into exactly 8 dashes); solid with a wedge cut from the right side — the lamp's fault geometry. `BOT_COLOURS = [clay, ochre,
  olive, verdigris, steel, lapis, madder]`, materials in wheel order, with a **hole where lichen would be** (OKLCH hue 110-160 excluded:
  the accent is singular) and purple absent (260-330). A name outside either set is refused with the closed set in the sentence —
  `the_palette_is_closed_and_bounded`, `an_unknown_colour_token_is_refused_with_the_palette`, `an_unknown_shape_is_refused`, *is bounded,
  and refuses a token from outside itself*, *paints a distinct drawing for every shape, so two shapes never collide*, *treats a colour token it does not know as absent, without redrawing it*.
- **Colour is the second channel.** A colour without a shape is refused by `parse_identity` and not painted by `BotIdentityCell`; a shape
  alone is a whole identity — `a_colour_without_a_shape_is_refused`, `a_shape_alone_is_a_whole_identity`, `a_colour_with_a_shape_round_trips`,
  `blank_fields_clear_the_identity`, *an ink is not painted without a shape*, *refuses a colour with no shape before it sends anything*,
  *leaves a colour out of the words when there is no shape to pair it with*, *two bots differing only in colour are still two identities*.
- The gate reads three files — `index.css` hexes, `bot-identity.tsx`'s picker set, `identity.rs`' validator set — and fails on a member under
  the 4.5 floor on any surface of either theme (with theme, surface and ratio), an ink in one theme only, an ink nothing can choose, a palette
  past `IDENTITY_PALETTE_BOUNDS`, or the picker and validator disagreeing. The floor is the **text** floor because a mark may be a letter
  somebody typed and the ink also sets the bot's name; the palette is hex on purpose, since the gate's arithmetic skips `oklch()` — *is declared
  in both themes and in both languages*, *clears the floor on every surface of both themes*, *measures each ink at a ratio the report can print*, *is reported with its theme, its surface and its ratio*, *fails on the surface a member only just clears the background on*, *reports an ink defined in only one theme*, *reports an ink nothing can choose*, *reports a palette that has grown past its bound*.
- A mark is an icon key (resolved with `spaceIcon`, searched with `matchSpaceIcons`) or a literal of at most `MAX_MARK_CHARS` (4) with no
  whitespace or control characters; `is_icon_mark` tells the two apart — `a_one_grapheme_mark_is_kept_and_a_word_is_refused`,
  `a_mark_with_a_space_is_refused`, `an_icon_name_is_told_apart_from_a_literal_mark`, *says an icon by name and a typed mark as the character it is*, *picks a mark out of the existing icon set*, *saves the shape, the colour and the mark, and hands back the stored row*.
- `plan_reorder(known, order)` refuses a partial order, a stranger or a duplicate **before** the transaction; `reorder_bots` is one `BEGIN
  IMMEDIATE`, and a failure part-way leaves no `pin_order` rewritten — `reorder_accepts_a_permutation`, `reorder_refuses_a_partial_order`,
  `reorder_refuses_a_stranger_and_a_duplicate`, `a_refused_reorder_plan_never_reaches_the_write`, `a_reorder_that_fails_partway_leaves_no_pin_order_rewritten`, `a_bot_round_trips_its_chosen_identity_and_can_clear_it`.
- The strip is absent at 0 bots (it used to hide below 2; it now carries identity and unpin as well as switching), draws a bounded window,
  never hides the bot being talked to, says how many it is not drawing while still submitting a complete order; Alt+Arrow moves a bot from
  the keyboard and a bare arrow is left to the strip's own focus movement; unpin is behind an `AlertDialog` naming the bot, its token and
  what survives — *draws everything while everything fits*, *bounds the row and counts the rest*, *never hides the bot being talked to*, *is
  absent entirely when nothing is pinned*, *carries each bot's identity in its accessible name*, *moves a bot from the keyboard and persists
  the whole new order*, *moves the other way, and refuses to walk off either end*, *leaves a bare arrow to the strip's own focus movement*, *says how many bots it is not drawing, and still submits a complete order*, *selects a bot on a plain click*, *unpins only after a confirmation that names what happens to what*, *is hidden from the accessible tree, because its owner speaks it*.

**Block If:** nothing. **Never:** a free colour picker anywhere in the product (now a `DESIGN.md` *Never*); a bare dot as identity; an `oklch()` ink the gate cannot measure; a second icon grid (the exported `IconChoice` is reused).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Save | `shape=hollow, colour=lapis, mark=flask-conical` | stored, `BotVm` returned, cell painted `--bot-ink-lapis` | — |
| Colour, no shape; unknown ink | `shape=null, colour=clay`; `colour=hotpink` | refused before anything is sent, and at `parse_identity`; sentence lists the seven | `BotIdentityError` |
| Mark | `"AI"` / `"🦉"` / `"hello"` / `"a b"` | kept / kept / refused (a word) / refused (space) | `BotIdentityError` |
| Reorder | a permutation; partial / stranger / duplicate; `RAISE(ABORT)` trigger mid-way | written as a unit; refused, no write; rollback, no `pin_order` rewritten | `BotOrderError` |
| Gate | `--bot-ink-lapis` light → `#7fb3d8` | 3 `identity-palette` violations with measured ratios, exit 1 | — |

</intent-contract>

## Code Map

| file | state | what shipped |
|---|---|---|
| `src-tauri/crates/keeper-core/src/bots/identity.rs` | new, 417 | `BOT_SHAPES`, `BOT_COLOURS`, `MAX_MARK_CHARS`, `MAX_ICON_NAME_CHARS`, `BotIdentityError`, `BotOrderError`, `is_bot_shape`/`is_bot_colour`/`is_icon_mark`, `parse_identity`, `plan_reorder`; 13 tests |
| `src/components/bots/bot-identity.tsx` (+ test, 13) | new, 495 | `BOT_IDENTITY_SHAPES`/`COLOURS`, `INK_CLASS`, `CELL`/`CELL_NOTCHED`, `SHAPE_GEOMETRY`, `isBotIdentityShape`/`Colour`, `botIdentityPhrase`, `botPinLabel`, `BotIdentityCell`, `BotIdentityPicker` |
| `src/components/bots/bot-pins-strip.tsx` (+ test, 11) | rewritten, 420 | identity cells, pointer drag copied from `layout/pins-strip.tsx` (`usePointerDrag`, slop 10, midpoint hit-test), Alt+Arrow, context menu, unpin dialog, `botPinsWindow`; same prop shape |
| `scripts/check-design.mjs` (+ `check-design.test.ts`, 8) | +199 | the `identity-palette` rule: `themeBlock`, `readNameList`, `readRustNameList`, `identityPaletteFindings`, `IDENTITY_*`, `checkIdentityPalette()`; main wrapped in `if (import.meta.main)` so rules are importable |
| `src/index.css`; `DESIGN.md` | +21; +29 | 7 `--bot-ink-*` in `:root`, 7 in `.dark`, 7 `--color-bot-ink-*` in `@theme inline`; Shapes: the bot cell, the seven inks with the lichen hole, the rejected free picker, colour-as-second-channel; Never: no free colour picker; Always: two lines |
| `src-tauri/crates/keeper/src/bots_ipc.rs`, `lib.rs`, `src/lib/ipc/client.ts`, `dev/mock-shell.ts` | append-only | `bots_bot_identity_save(botId, shape, colour, mark) -> BotVm`, `bots_bots_reorder(order) -> Vec<BotVm>`; wrappers; handlers that mutate the fixture, and one bot given `hollow/lapis/flask-conical` |
| `src-tauri/crates/keeper-core/src/bots/mod.rs`, `store.rs` | +1; tests only | `pub mod identity;`; the identity round-trip test given real closed-set values, plus two reorder tests with trigger-based fault injection (`registry.rs:2530`'s technique) |

Measured (light / dark; background · card · secondary): clay `#ac2f3b` 5.82·5.41·4.93 / `#df5f65` 5.37·5.01·4.61 · ochre `#994d00` 5.49·5.10·4.65 /
`#d66e01` 5.48·5.12·4.71 · olive `#795f05` 5.43·5.05·4.61 / `#ac8909` 5.70·5.33·4.90 · verdigris `#026e65` 5.48·5.10·4.65 / `#09a397` 6.03·5.63·5.18 ·
steel `#066c7e` 5.43·5.05·4.60 / `#0a9eb8` 5.94·5.54·5.10 · lapis `#0466a1` 5.48·5.10·4.65 / `#0b94e7` 5.76·5.38·4.95 · madder `#9e3378` 5.86·5.45·4.97 / `#d162a5` 5.40·5.04·4.64.

## Tasks & Acceptance

**Execution:** [x] closed sets and refusals in core · [x] tokens in both themes · [x] the gate rule and its self-test · [x] cell, picker, phrase
· [x] the strip rewrite with keyboard reorder and unpin · [x] two commands · [x] `DESIGN.md`. **Acceptance Criteria:** hand order persisted
with the `registry.rs` pattern — **met**; identity is a shape from a closed set, a mark from the existing picker extended with a grapheme, and a colour from a bounded, contrast-checked palette — **met**; a free colour picker rejected, not deferred — **met**, in `DESIGN.md`'s own text.

## Design Notes

**Divergences from AD-155, flagged.** The AD names `sort_order`, `palette_index` and `--bot-hue-N` tokens checked by the existing `contrast`
rule; shipped are 61.1's `pin_order` / `identity_colour` (a **name**, not an index — a stored index would be a number the gate then has to agree with), `--bot-ink-<name>`, and a dedicated `identity-palette` rule at the 4.5 floor. AD-155 should record the shipped spelling.

## Deferred

`--account-hue-*`: the existing eight-hue account wheel is written as `oklch()`, which the gate's hex arithmetic skips entirely, so those
eight colours have never been contrast-checked. Converting them is the same defect family and is outside this story. DW id: to be minted by the coordinator.

## Verification

- `bunx vitest run src/components/bots/bot-pins-strip.test.tsx src/components/bots/bot-identity.test.tsx scripts/check-design.test.ts
  src/components/bots/bots-pane.test.tsx` → 4 files, 41 tests. `cargo nextest run -p keeper-core -E 'test(identity) + test(reorder)'` → 27
  passed. Clippy `-D warnings`, `bunx tsc --noEmit`, biome clean. Coordinator's tree-wide gate: 4028 Rust / 5466 frontend tests green, `node scripts/check-design.mjs` at zero violations (the two mid-wave findings, `tasks-pane.tsx:613` and `bot-answer.tsx:50`, were proven pre-existing / 61.5's by running HEAD's own gate script against this tree, and are gone).

| Mutation | Caught by | Observed |
|---|---|---|
| (a) `--bot-ink-lapis` light `#0466a1` → `#7fb3d8` | `node scripts/check-design.mjs`; *clears the floor on every surface of both themes* | 3 `identity-palette` violations, 2.01 / 1.87 / 1.70 needs 4.5, exit 1; 1 failed / 7 |
| (b) `const ink = shape !== null && isBotIdentityColour(…)` → `isBotIdentityColour(…)` | *an ink is not painted without a shape* | 1 failed / 12 |
| (c) `tx.commit()` in `store::reorder_bots` made a no-op | `a_refused_reorder_plan_never_reaches_the_write` | 26 passed, 1 failed — the first version of that test asserted only `[0,1,2]`, which survives a rollback because the untouched rows already hold `0,1,2`; strengthened to `(id, pin_order)` pairs before re-running |

Each restored; md5 matches the pre-mutation backup (both `tx.commit()` sites, since the sed hit two). **Not verified here:** `bots_bot_identity_save`
and `bots_bots_reorder` are written and unbuilt — the shell crate does not compile on Linux; they are thin and every decision is in core.
Atomicity under a real process kill is not reachable without a fault-injecting VFS; the trigger proves the rollback path. No pixel: the hexagon
at 24 px, the notch wedge, the 8-dash pattern and a mark in a filled cell drawn in `--background` are asserted as SVG markup and classes, never seen. The pointer drag path is copied from `layout/pins-strip.tsx` and not exercised — jsdom's `getBoundingClientRect` is zeroes; Alt+Arrow and the full-order dispatch are.

## Coordinator addendum, 2026-09-02

The unmeasured `oklch()` account-hue wheel is filed as **DW-217**. It is the same defect family this
story closed, one layer older, and it stays out for the reason the story gives: those hues are
assigned by the app, not chosen by a person, so converting them is a separate palette decision.
