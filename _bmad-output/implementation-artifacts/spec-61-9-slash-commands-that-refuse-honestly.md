---
title: 'Story 61.9: slash commands that refuse honestly'
type: 'feature'
created: '2026-09-02'
status: 'review'
baseline_revision: '9e06c2b'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
---

binds: FR-385 → AD-157; AD-20 (Rust filters and ranks, the frontend renders by id — consumed), AD-27 (no affordance that lies, consumed)
depends on: 61.4 (the composer `bot-composer.tsx` and the pane that mounts it)

<intent-contract>

## Intent

**Problem:** The owner asked for slash commands *"(look telegram, passeo, t3code how is it used)"*. The tree had a working, tested `/` menu in the note editor (`src/components/notes/editor/slash-menu.ts`) and none in any composer. The epic fixed the shape before any code: *"The registry is client-side, and it says so"* — an unknown command is a refusal naming the nearest match and is **not** sent to the model as prose; a literal leading slash has an escape, because a model may legitimately be asked about `/etc`; and Hermes' own server-side commands are **not** proxied, because they are enumerable only over a websocket RPC keeper does not speak (research §14) and a picker that silently dropped `/compact` on Ollama would be an affordance that lies.

**Approach:** Resolution lives in exactly one place, `keeper_core::bots::commands`, and the composer reaches it over one new stateless IPC command, `bots_command_preview`, obeying the verdict it gets back. Rust owns the registry, the resolution order, the refusal sentences, the availability reasons, the nearest-match and the escape. TypeScript owns only what a textarea must answer locally: the trigger rule, the token a dismissal belongs to, the keyboard model, and the send/accept plumbing. The precedents followed are `paletteQuery` (AD-20) and `syncTaskSchedulePreview` (called as somebody types; the refusal arrives as data; the draft is echoed back for the out-of-order guard). The autocomplete surface is cloned from `slash-menu.ts`.

**Why a TypeScript matcher beside the Rust registry was rejected.** It would have been two opinions about what `/mod` means. The nearest-match, the alias table and the availability reasons would have needed a second, hand-mirrored copy; the first time the two disagreed the composer would have offered a command the registry refused or refused one it offered, which is exactly the drift AD-55/AD-56 exist to prevent. One round trip per keystroke on a `/` line is the price, and it is the price `syncTaskSchedulePreview` already pays.

## Boundaries & Constraints

**Always:**
- The registry order is a decision, not an accident: `model` precedes `metadata` so `/m` is the model picker; `history` precedes `help` so `/h` has to be an *alias* to reach `/help` — which makes rule 1 (exact name or alias beats an earlier prefix match) load-bearing rather than incidental.
- An unknown command is `Resolution::Refusal`, never `Prose`; it names the nearest match by Levenshtein distance or says there is none.
- The escape is a doubled slash: `//etc` sends `/etc` as text; exactly one slash is removed (`///etc` → `//etc`). It is unwrapped in `resolve_in` and nowhere else, only for a single-line draft within `MAX_COMMAND_CHARS`. AD-157's draft spelled the escape `\/`; the shipped form is `//`, taught by the `ESCAPE_HINT` const carried on every `BotCommandPreviewVm`.
- A command's needs (`provider`, `bot`) are reported outermost-missing-first, and an unavailable command is listed with its reason and refused when run — never hidden.
- `/grant` is Ollama-tools-only: absent on a Hermes bot (`HERMES_TOOLS_SERVER_SIDE`), absent when the endpoint said `tools: false`, and listed with `GRANT_TOOLS_UNKNOWN` when the endpoint said nothing — keeper could not read it, which is its own sentence and never `false` (AD-27).
- The Hermes sentence (`HERMES_COMMANDS_NOT_PROXIED`) appears only when the preview carries one, i.e. only for a Hermes bot.
- A trimmed draft not starting with `/` is sent with no round trip: no command and no escape can begin with anything else, so it is the boundary `isCommandLine` already draws. This fast path is also what un-broke `bots-pane.test.tsx` mid-wave.
- Every refusal and note is a `const` in `commands.rs`, quoted by the UI and never retyped in TypeScript. `CommandNeed` carries no TS derive: it never crosses the wire.

**Block If:** nothing; `slash-menu.ts`'s keyboard model and the two preview precedents answered every design question.

**Never:** send an unknown command to the model as prose; proxy or hand-copy Hermes' `commands.catalog`; ship `/image` (see Design Notes); resolve a command in TypeScript; touch `bot-empty-state.tsx` (unowned this wave — see Deferred).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Exact beats prefix | registry lists `news` before `new`; draft `/new` | `new` | No error expected |
| Alias beats prefix | `/h` | `help` (alias), though `/history` is the earlier prefix hit; `/hi` → `history` | No error expected |
| Unknown | `/mdoel foo`; `/zzzzzz` | refusal naming `model`; refusal saying nothing is close — in both cases nothing sent, draft kept | The refusal IS the handling |
| Escape | `//etc`; `///etc` | prose `/etc`, no menu opens; prose `//etc` — exactly one slash removed | No error expected |
| Needs unmet | `/new` with a provider and no bot; `/model x` with neither | listed as unavailable with the bot reason and refused when run; the provider reason first (outermost missing need) | Sentence rendered |
| `/grant` | Hermes bot; Ollama with `tools: None` | absent from the menu, `HERMES_TOOLS_SERVER_SIDE` if typed; listed with `GRANT_TOOLS_UNKNOWN` | Sentence rendered |
| Plain prose | `hello /etc` | sent, no round trip | No error expected |
| Keyboard | ↑/↓ wrap, Tab and Enter accept, Escape dismisses for that token; a modified chord or any other key passes through | claims exactly the five keys it owns and nothing while closed | No error expected |
| `/metadata`, `/grant`, `/history` run | this wave | a sentence naming where the control is (`BOT_COMMAND_METADATA_IS_A_TOGGLE`, `_GRANT_IS_A_SURFACE`, `_HISTORY_IS_A_LIST`) shown as a refusal — never silence | Sentence rendered |

The shipped registry: `new` (`clear`, `reset`; no args; needs provider+bot), `bot` (`bots`; required *a bot name*; needs provider), `model` (`m`; required *a model to send to*; needs provider+bot), `metadata` (`meta`; optional *on or off*), `grant` (`grants`; no args; needs provider+bot; Ollama-tools-only), `history` (`sessions`; optional *a word to search for*; needs provider), `help` (`h`, `commands`; no args).

</intent-contract>

## Code Map

| Path | Change |
|---|---|
| `src-tauri/crates/keeper-core/src/bots/commands.rs` | new, 615 lines: registry, `Context`, offered/availability, `resolve_in`/`resolve`, `menu_in`/`menu`, `note`, `command_token`, `nearest_in`, Levenshtein, every copy const |
| `src-tauri/crates/keeper-core/tests/bots_commands.rs` | new, 27 tests over the rules |
| `src-tauri/crates/keeper-core/src/bots/mod.rs` | `pub mod commands;` |
| `src-tauri/crates/keeper-core/src/vm.rs` | `BotCommandContextReq` (+ `.context()`), `BotCommandRowVm`, `BotCommandVerdictVm` (prose \| command \| refusal), `BotCommandPreviewVm` (+ compose), appended before `#[cfg(test)]` |
| `src/lib/ipc/gen/ArgMode.ts`, `BotCommandContextReq.ts`, `BotCommandPreviewVm.ts`, `BotCommandRowVm.ts`, `BotCommandVerdictVm.ts` | generated by `cargo test -p keeper-core --lib export_bindings_botcommand*` |
| `src-tauri/crates/keeper/src/bots_ipc.rs` | one import and `bots_command_preview` :925 — 26 lines, one compose call, reads nothing — **not compiled on this host** |
| `src-tauri/crates/keeper/src/lib.rs` | one registration in the shared literal — **not compiled on this host** |
| `src/lib/ipc/client.ts` | 4 type re-exports, 2 type imports, the `botsCommandPreview` wrapper |
| `dev/mock-shell.ts` | `mockCommandPreview`, labelled a crude fake: mirrors names and descriptions, does not reimplement nearest-match, availability or the escape |
| `src/components/bots/bot-slash-menu.ts` | rewritten, 290 lines: `isCommandLine`, `slashToken`, `menuKeyAction`, `nextIndex`, `acceptedDraft`, `argumentHint`, `BotCommandContext`/`Invocation`, the `botCommandContext` and `botCommandHost` factories and their copy |
| `src/components/bots/bot-slash-menu.test.ts` | new, 13 tests |
| `src/components/bots/bot-composer.tsx` | rewritten, 456 lines: wave B's keyboard model kept verbatim, plus the slash path, the menu, the help panel, the refusal region, and Story 61.12's two paste props (`pasteContext`, `onPaste`) |
| `src/components/bots/bot-composer.test.tsx` | replaced wave B's, 25 tests |
| `src/components/bots/bots-pane.tsx` | coordinator-owned wiring: one import and two props on the existing `<BotComposer>` — `commandContext={botCommandContext({…})}`, `onCommand={botCommandHost({…})}`; no switch statement in the pane |

## Tasks & Acceptance

**Execution:**
- [x] `commands.rs` (+ integration tests) — registry, resolution order, availability, nearest-match, the escape, all copy.
- [x] `vm.rs` + generated bindings + `bots_ipc.rs` + `lib.rs` + `client.ts` + `mock-shell.ts` — the stateless preview command end to end.
- [x] `bot-slash-menu.ts` (+ test) and `bot-composer.tsx` (+ test) — the surface, proven against an injected resolver.

**Acceptance Criteria:**
- Given `/h`, when resolved, then it is `help` by alias even though `history` is the earlier prefix hit. **Met** — `commands_exact_alias_beats_an_earlier_prefix_match`; mutation (a).
- Given an unknown command, when sent, then a refusal names the nearest match and nothing reaches the model. **Met** — three Rust tests and the composer's *shows a refusal, sends nothing, and keeps the draft*; mutation (b).
- Given `//etc`, when sent, then the model receives `/etc` and no menu opened. **Met** — `commands_doubled_slash_sends_one_slash_as_text`, the composer and the menu tests; mutation (c).
- Given a Hermes bot, when the menu opens, then the not-proxied sentence is shown and `/grant` is absent with its reason; on Ollama neither appears. **Met** — `commands_the_hermes_sentence_appears_only_for_a_hermes_bot`, `commands_grant_is_absent_on_a_hermes_bot_and_says_why`.

## Design Notes

**`/image` is not shipped, and that is deliberate.** The epic prose and AD-157 list eight commands; the story shipped seven. Story 61.12 owns the paste gesture, and a command that could not attach anything is exactly the affordance AD-27 forbids; if one is wanted later it belongs beside 61.12's `useBotImagePaste`, not here. **Three commands land on surfaces this same wave built.** `/metadata` (61.8's toggle), `/grant` (61.10's bar and dialog) and `/history` (61.6's list) return a sentence naming where the control is rather than staying silent. Those sentences stay true after integration; the coordinator can replace any of them with a real focus/open call by editing `botCommandHost` in `bot-slash-menu.ts`, and no pane code changes.

## Deferred

- Proxying Hermes' server-side commands — enumerable only over the websocket RPC `commands.catalog`; a hand-copied list would rot on the next Hermes release. **DW-209.**
- The empty state teaching the escape. `bot-empty-state.tsx` had no owner in this wave's collision map and sits beside the coordinator's integration, so it was not edited. `ESCAPE_HINT` is taught in two places strictly closer to the moment of relevance — under the open menu whenever a leading slash is on screen, and in the `/help` panel — and the empty state can quote the same const in one line. DW id: to be minted by the coordinator.

## Verification

- `cargo test -p keeper-core --test bots_commands` → 27 passed; `cargo nextest run -p keeper-core -E 'binary(bots_commands) + test(commands)'` → 36 passed. `cargo clippy -p keeper-core --all-targets -- -D warnings` → clean (a `manual_contains` in `exact_in`, reported by Story 61.8's agent, fixed).
- `bunx vitest run bot-slash-menu.test.ts bot-composer.test.tsx` → 38 passed; with `bots-pane.test.tsx` → 47 passed across 3 files. `bunx tsc --noEmit` → clean, whole repo. `bunx biome check` on the story's 6 files → clean. `no-user-agent-gating.test.ts` + `zero-egress.test.ts` → 3 passed.

| Mutation | Tests that failed | Observed |
|---|---|---|
| (a) `resolve_in`: `prefix_in(…).or_else(\|\| exact_in(…))` — exact-beats-prefix inverted | `commands_exact_name_beats_an_earlier_prefix_match`, `commands_exact_alias_beats_an_earlier_prefix_match` | 25 passed; 2 failed |
| (b) unknown → `Resolution::Prose(draft)` | `commands_unknown_name_is_never_prose`, `…_refuses_and_names_the_nearest_match`, `…_with_no_near_match_names_none` | 24 passed; 3 failed |
| (c) escape emits `format!("//{rest}")` | `commands_doubled_slash_sends_one_slash_as_text` | 26 passed; 1 failed |
| (d) `availability()` returns `Available` first | `commands_needing_a_bot_are_unavailable_with_a_reason`, `commands_report_the_outermost_missing_need_first`, `commands_grant_distinguishes_a_refused_capability_from_an_unread_one` | 24 passed; 3 failed |

Restore proven by `diff` against the pre-mutation copy (byte-identical) and 27 passed again. The coordinator's full gate afterwards: 4028 Rust tests, 5466 frontend tests, lint/typecheck/fmt/clippy clean.

## Auto Run Result

Status: implemented and gated; uncommitted; not reviewed.

**Not verified here, and why.** `bots_command_preview` and its registration were never compiled — the `keeper` shell crate does not build on Linux; the risk is a compile error, not a behaviour error, since the compose path is exercised by the Rust tests. The menu as pixels: row highlight is asserted through `aria-current`, not computed colour, so the `bg-accent` paint is unseen. `dev/mock-shell.ts`'s fake is reachable only in the browser dev shell, which cannot run here; it is typed against the generated VMs, so tsc is the only check it got.

## Coordinator addendum, 2026-09-02 — an unread capability warns, it does not refuse

Verified in a real browser against the harness after this spec was written: `/grant` was absent from
the menu on the harness' Ollama bot, whose tool capability is unread, while the pane's grant bar —
following story 61.10's own mid-flight correction — offered the control and warned. Two layers, two
opposite answers to *may I set a grant right now*, which is exactly the drift the one-matcher
decision was meant to prevent.

Resolved in the decision home, not the surface. `Availability` gains
`AvailableWithWarning(&'static str)`; `availability()` returns it for `ollama_tools_only` with
`model_tools: None`; `reason()` still answers `None` for it, so no caller can turn a caveat into a
refusal by accident, and `warning()` carries the sentence. `BotCommandRowVm` gains `warning`, the
composer prints it under the row, and `GRANT_TOOLS_UNKNOWN` was reworded from *"it cannot say a
grant would ever be used"* to *"a grant here may reach nothing"* so the two layers quote the same
caveat. `commands_grant_distinguishes_a_refused_capability_from_an_unread_one` now asserts the
warning arm, that the command is runnable, and that `reason()` is `None`; `Some(false)` remains the
only input that hides the row. Re-verified in the browser: the row appears, runnable, with the
caveat beneath it.
