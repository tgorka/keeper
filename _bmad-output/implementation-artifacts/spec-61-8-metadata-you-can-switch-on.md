---
title: 'Story 61.8: metadata you can switch on'
type: 'feature'
created: '2026-09-02'
status: 'review'
baseline_revision: '9e06c2b'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
---

binds: FR-384 → AD-156; AD-139 (typed nullable columns, consumed), AD-27 (no affordance that lies, consumed)
depends on: 61.4 (the pane, `bot-message.tsx`, and the nullable columns on `BotMessageVm` that the caption reads)

<intent-contract>

## Intent

**Problem:** The owner asked for *"the chat and some metadata in the chat — maybe with switch on off"*. The epic found no precedent anywhere in the tree (`grep 'Show (details|source|raw|metadata|debug)|developer mode'` over `src` → 0 hits) and set the rule that matters: *"Absent numbers render as absent, never as zero: an endpoint that omits `usage` is a fact about the endpoint, and this is the app that already refuses to print a number it did not measure."* Story 61.4 shipped `bot-message-meta.tsx` as a 32-line null-returning seam; the columns were recorded, nothing showed them, and nothing could switch them on.

**Approach:** One persisted, default-off toggle — the Rust settings key `bots.message_details` — governs a one-line caption under every assistant row and a real disclosure expander for the rest. The toggle is reachable from a chip in the pane header and from ⌘K through a new Rust-authored `Bots` palette category, both calling the same `toggleBotMessageDetails` verb. Every formatter is one the tree already owns; no new formatter was written. The storage mechanism is `sessions.spaces_folded`'s, end to end.

## Boundaries & Constraints

**Always:**
- The toggle is a `settings` k/v key behind the TOML layer stack — never localStorage, never a cookie, never component state. Stored bytes are `"1"`/`"0"` (`Shape::Flag01`), asserted as raw text in the round-trip test for the reason the sessions test records: a writer that put `"true"` would round-trip through itself and still invert every `keeper.toml` that sets the key.
- The key `bots.message_details` is spelled in four Rust places and nowhere else: the `KeySpec` in `config/keys.rs` (`UserGlobal`, `AnyLayer`, `Flag01`, default `"0"` — the row the coverage test demands), the const plus `get_bots_message_details`/`set_bots_message_details` in `registry.rs`, the Platform-port wrappers `bots_message_details_get`/`_set` in `account.rs`, and the two `#[tauri::command]`s of the same names in `bots_ipc.rs`. `lib.rs` registers them, `client.ts` wraps them, and `docs/settings-keys.md` is regenerated from the registry, not hand-edited.
- An absent value contributes no part and no separator. Every nullable column drops its whole row from the expander. `toolCallCount` is the one non-nullable column, so its `0` is a count keeper took and is shown. Time to first token is printed in the unit it was measured in (`340 ms`), never through `formatElapsed`, which resolves to whole seconds and would print a measured 340 ms as `0:00` — a zero that is really a measurement.
- Nothing in the component is a live region. A metadata line over a streaming answer would re-announce on every delta; the one arriving-answer announcement stays where `bot-message.tsx` already owns it.
- The caption is in the flow, not hover-revealed (Open WebUI's `hover-reveal` + `aria-hidden` usage button is the named anti-pattern). The expander is a `<button>` with `aria-expanded` and `aria-controls`; the panel is absent, not hidden, when shut. The chip uses `aria-pressed` with `bg-accent` as the pressed paint (`note-filter-bar.tsx`'s idiom).
- The store mirror `metaShown` is deliberately **not** in `EMPTY`: `reset()` clears a conversation and must not un-set a preference somebody chose.
- The `Bots` palette category rides its own `bots` gate and not the `notes`/`sessions`/`tasks` flag: that flag is `sync && desktop`, `CapabilitiesVm.bots` is `cfg!(desktop)`, so a desktop build with folder sync off has a working Bots pane and would otherwise lose the verb.

**Block If:** nothing; the storage exemplar, the count formatter and the palette registry all existed.

**Never:** print a zero for a number the endpoint did not report; write "TTFT" or "finish reason" (the labels are *Time to first token* and *Why it stopped*); persist the toggle anywhere but Rust; add a mobile twin of the commands (`bots: cfg!(desktop)` means nothing on a phone calls them); hand-edit `docs/settings-keys.md` or `src/lib/ipc/gen/**`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Toggle off, or a question | any assistant row with the setting absent or `"0"`; any `role: "user"` row | nothing rendered, however much was recorded — a question has no model and no tokens | No error expected |
| Toggle on, all reported | model, tokens, ttft, duration present | `llama3.1:8b · 1,291 tokens · first token after 340 ms · answered in 0:12` | No error expected |
| Partial row | `partial = true` | caption ends `· answer incomplete`; a finished row says nothing about completeness | No error expected |
| Field absent | `totalTokens: null`, `ttftMs: null` | `llama3.1:8b · answered in 0:12` — no part, no separator | No error expected |
| Nothing measurable | every nullable column null | `This endpoint reported nothing about this answer.` | The sentence IS the handling |
| Zero tool calls | `toolCallCount: 0` | expander row `Tool calls 0` — a measurement, shown | No error expected |
| Expander | trigger pressed | `aria-expanded` flips; `<dl aria-label="Answer details">` mounted only while open; rows Model, Provider, Prompt tokens, Completion tokens, Total tokens, Time to first token, Total time, Why it stopped, Tool calls, Request id — each absent when null | No error expected |
| Hydration | `bots_message_details_get` resolves `true` | mirror set, caption shown | No error expected |
| Read failed | `bots_message_details_get` rejects | toggle stays off and says nothing — claiming it is on when keeper could not read the setting is the claim that lies | Silent, asserted |
| Palette with no pane mounted | `bots-toggle-metadata` dispatched | `toggleBotMessageDetails` reads the STORED value, inverts, writes through Rust, moves the mirror | Rust's error surfaces as for any invoke |
| `cfg!(desktop)` false | shell passes `bots = false` | the `Bots` section is absent from ⌘K, ⌘? and the native menu; `registry_sections_ordered_by_category` (all gates off) is unchanged | No error expected |

</intent-contract>

## Code Map

| Path | Change |
|---|---|
| `src/components/bots/bot-message-meta.tsx` | rewritten (32 → 310 lines): caption, expander, `metaFacts`/`metaCaption`, the `BotMetaToggle` chip, the shared `toggleBotMessageDetails` verb |
| `src/components/bots/bot-message-meta.test.tsx` | new, 24 tests |
| `src-tauri/crates/keeper-core/src/registry.rs` | append-only: `BOTS_MESSAGE_DETAILS_KEY` :750, getter :762, setter :771, round-trip test :3875 |
| `src-tauri/crates/keeper-core/src/config/keys.rs` | append-only: `// ---- bots ----` section, the `KeySpec` at :386 |
| `src-tauri/crates/keeper-core/src/account.rs` | append-only: `bots_message_details_get` :3788, `bots_message_details_set` :3800; `palette_query` gains the `bots` parameter |
| `src-tauri/crates/keeper-core/src/palette.rs` | `BOTS_CATEGORY`; action `bots-toggle-metadata` / *Toggle Answer Details* / no chord; `BOTS_CATEGORY` in `CATEGORY_ORDER` after Tasks, before Chat; `query_actions(needle, open_chat, recording, notes, bots)`; `PaletteIndex::query` and `registry_sections(recording, notes, bots)` gain the third gate; 28 in-crate test call sites updated; `registry_sections_covers_all_actions` now `registry_sections(true, true, true)` |
| `docs/settings-keys.md` | :51-52, regenerated by `cargo test -p keeper-core --lib config::keys::tests::docs::regenerate -- --ignored` |
| `src-tauri/crates/keeper/src/bots_ipc.rs` | append-only: the two commands at :1310 / :1322 — **not compiled on this host** |
| `src-tauri/crates/keeper/src/lib.rs` | :797-798 inside the single invoke-handler literal — **not compiled on this host** |
| `src-tauri/crates/keeper/src/ipc.rs` | `palette_query` :4081 passes `cfg!(desktop)` as the sixth argument; `cheat_sheet_sections` :4104 passes it as the third to `registry_sections` — **not compiled on this host** |
| `src-tauri/crates/keeper/src/menu.rs` | :112-115, the native menu builder's `registry_sections(recording_supported(), notes, cfg!(desktop))` — **not compiled on this host** |
| `src/lib/ipc/client.ts` | append-only: `botsMessageDetailsGet` :6966, `botsMessageDetailsSet` :6981 |
| `dev/mock-shell.ts` | `let botMessageDetails = false` and two handlers :2890-2894 — a real round trip, not a constant answer |
| `src/lib/stores/bots.ts` | `metaShown`/`setMetaShown` on `BotsState`; not in `EMPTY` |
| `src/lib/count-label.ts` | `TOKENS: CountNoun` in the house noun table beside NOTES/SESSIONS/ITEMS/RUNS |
| `src/components/command-palette/actions.ts` | one import and the handler `"bots-toggle-metadata"` :170 routed through `toggleBotMessageDetails` |
| `src/components/bots/bots-pane.tsx` | coordinator-owned wiring, 3 lines: the `BotMetaToggle` import and `<BotMetaToggle />` in the existing `<header>` |
| `src/components/bots/bot-message.tsx` | unchanged — `<BotMessageMeta message={message} />` was already mounted by 61.4 |

## Tasks & Acceptance

**Execution:**
- [x] `registry.rs` / `config/keys.rs` / `account.rs` / `bots_ipc.rs` / `lib.rs` / `client.ts` / `mock-shell.ts` — the key, spelled once per layer, following `sessions.spaces_folded` line for line.
- [x] `bot-message-meta.tsx` (+ test) — caption, expander, chip, one verb; `count-label.ts` gains `TOKENS`.
- [x] `palette.rs` and its three shell call sites — the `Bots` category on its own gate (signature change approved by the coordinator, since an `actions.ts` handler alone is dead code: the ⌘K row, the ⌘? block and the native menu item all come from the Rust catalog, and it held no bots entry at all).

**Acceptance Criteria:**
- Given an assistant row and the toggle off, when it renders, then no metadata is shown however much was recorded. **Met** — asserted, and mutation (c) below proves the guard.
- Given the toggle on and an endpoint that omitted `usage`, when the caption renders, then the token part is absent, not `0 tokens`. **Met** — nine generated absent-field cases, mutation (a).
- Given a fresh install, when nothing has been stored, then the toggle is off in Rust and in the store's initial state. **Met** — mutations (b1) and (b2).
- Given ⌘K, when a person types the words they would use, then *Toggle Answer Details* is found and dispatches the same verb the chip calls. **Met** in `keeper-core` and in the frontend; the shell's three call sites are unproven here.

## Design Notes

**Why the third gate is the wave's highest-risk lines.** `query_actions`/`registry_sections` are compiled and clippy-clean in `keeper-core` with the parameter proven both ways. But the shell passes `cfg!(desktop)` from three sites — `ipc.rs` `palette_query` and `cheat_sheet_sections`, and `menu.rs`'s native menu builder — and the `keeper` shell crate does not build on the Linux dev host. A wrong arity there is a compile error the macOS `cargo check -p keeper` will find in seconds, and nothing here found it. The two new commands in `bots_ipc.rs` and their two registration lines share that status.

**A one-item `Bots` submenu with no `Open Bots` above it.** The category currently holds one preference; Story 61.4 never registered the pane's own palette/menu entry, and registering it was outside this story. A `Bots ▸ Toggle Answer Details` submenu with no navigation item is a real oddity on the native menu bar. Recorded for the coordinator rather than fixed here. DW id: to be minted by the coordinator.

**The formatter reuse, and the one deliberate non-reuse.** Token counts go through `countLabel` with the new `TOKENS` noun; total time goes through `formatElapsed` (`use-recording-session.ts`), the same formatter the recordings row renders `durationMs` with. Story 61.13 observed that this import spells a recording hook from a bots file and confirmed it cannot make the zero-egress gate cry wolf (the gate reads files by glob, not by import); a neutral home under `src/lib/` would decouple the two surfaces and is a fair follow-up.

## Verification

- `bunx vitest run src/components/bots/bot-message-meta.test.tsx` → 24 passed. Plus `command-palette`, `cheat-sheet`, `count-label.test.ts` together → 6 files / 56 passed. `zero-egress.test.ts` → 1 passed.
- `cargo nextest run -p keeper-core -E 'test(bots_message_details) + test(palette) + …'` → 47 passed, among them `registry::tests::bots_message_details_defaults_off_and_round_trips`, `palette::tests::the_bots_section_rides_its_own_flag_and_not_the_notes_one`, `palette::tests::the_metadata_toggle_is_findable_by_the_words_a_person_would_type`; `-E 'test(config::keys)'` → 16 passed, including `docs::the_checked_in_table_matches_the_registry` and the four `coverage::` scans.
- `cargo clippy -p keeper-core --all-targets -- -D warnings` → clean. `bunx biome check` on the story's 8 files → clean.

| Mutation | Test that failed | Observed |
|---|---|---|
| (a) `if (message.promptTokens !== null)` → unconditional `countLabel(message.promptTokens ?? 0, TOKENS)` | *drops promptTokens entirely when the endpoint did not report it* | 1 failed / 23 passed |
| (b1) `registry.rs` getter `== Some("1")` → `!= Some("0")` (absent reads as on) | `bots_message_details_defaults_off_and_round_trips` | panicked at `registry.rs:3878`, 0 passed / 1 failed |
| (b2) `bots.ts` `metaShown: false` → `true` | *starts off when nothing has been stored* | 1 failed / 23 passed — survivable until `getInitialState().metaShown` was asserted, because `beforeEach` set the mirror explicitly |
| (c) `if (!shown \|\| role !== "assistant")` → `if (role !== "assistant")` | *renders nothing while the toggle is off, however much was recorded* | 1 failed / 23 passed |

All four restored. The coordinator's full gate afterwards: 4028 Rust tests, 5466 frontend tests, lint/typecheck/fmt/clippy clean, `node scripts/check-design.mjs` at zero violations.

## Auto Run Result

Status: implemented and gated; uncommitted; not reviewed.

**Not verified here, and why.** The three shell call sites, the two commands and their registrations were never compiled (no `keeper` crate build on Linux). The native menu bar and the ⌘? cheat sheet were not seen; `cfg!(desktop)` was not exercised as false on a real mobile target. No pixels: the caption's wrapping, the two-column `<dl>` at narrow widths and a long `requestId`'s overflow were not measured — jsdom asserts structure and text. `dev/mock-shell.ts` round-trips the setting, so `bun run dev` on a host with a browser shows the real thing.

## Coordinator addendum, 2026-09-02

The one-item `Bots` submenu is filed as **DW-218** — one action and one chip, in the story that owns
the palette registry.
