---
title: 'Story 61.13: the gates, the docs, and the honest row'
type: 'chore'
created: '2026-09-02'
status: 'review'
baseline_revision: '9e06c2b'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
---

binds: no FR of its own — AD-53 (egress is computed, never hand-listed; extended here with the provider chapter), AD-148 (a provider is a derived, host-only egress destination; now documented), D-4 (minted here in `docs/decisions.md`); guards FR-371's disclosure and the recording feature's zero-egress promise
depends on: everything — 61.1 through 61.12; it is last because it corrects what the twelve before it made false

<intent-contract>

## Intent

**Problem:** The epic's own words: *"The checks that would now cry wolf, fixed in the same epic."* Twelve stories added a fifth surface that talks to an endpoint, and three house mechanisms were written before any such surface existed: `docs/egress.md` is diffed by the release workflow against the previous tag, so a new destination class with no chapter is a diff that says nothing; `docs/decisions.md` records refusals *before* they are proposed as conveniences, and a chat feature is where a default endpoint, a hosted model and telemetry are first proposed; and the recording zero-egress gate (`zero-egress.test.ts`) scans a set of globs for upload/transcription tokens, so a bots file that drifted into one of those globs would either fail the gate falsely or, worse, teach the next reader to widen it. `docs/project-context.md` also had none of the epic's invariants, and `scripts/check-design.mjs` — the gate this epic's palette work made worth reading — was reporting one pre-existing violation on `main`, which would have hidden the next real one.

**Approach:** Documentation and gates only; no Rust, no manifest change (see Design Notes for why `deny.toml` needed nothing). A new egress chapter and table row; D-4; six lines in project-context; a new convention test that *derives* the recording gate's globs by reading the gate's own source, so a refactor of the gate breaks this test instead of narrowing it; and the one-line design fix on `tasks-pane.tsx` carried here with its reason.

## Boundaries & Constraints

**Always:**
- **The egress chapter** — *An AI provider is a destination you typed, disclosed as a host* — sits after the sync-remote chapter it builds on and states: the row exists only because the user typed it (no default endpoint, hosted model or proxy); it is derived — `EgressKind::BotProvider` computed by `egress::compute_egress` from `bots::store::provider_base_urls` (AD-53); it is reduced by the same `egress::remote_host` git remotes use, so the list shows a host and never `/v1` or `/p/<profile>`; two providers on one host are one entry; a loopback or private-network host is the normal case, accepted by `bots::url::parse_base_url`, marked `is_private`, and disclosed rather than hidden; and the release workflow's egress diff note fires on this chapter **by design** — that is the visibility working. The no-telemetry section is untouched and quoted verbatim in the new chapter. The *What keeper connects to* table gains one row: each configured AI provider's host, absent when no provider is configured, which is how keeper ships.
- **D-4 — The endpoint is yours, never keeper's** — is appended in the file's exact entry format: intro paragraph, then *What is built* (a provider is a record you create; the base URL is **required, not defaulted**, because keeper ships no endpoint — a fresh install has no provider row, no egress row and no pane content beyond the sentence that says so), *Why no default, no hosted model, no proxy* (each would make keeper a party to the conversation; the client-only posture NFR-11 is a posture about keeper, not about Matrix), *Why a Hermes bot gets no drive grant* (`tool_execution: "server"`, no client tool route, `unattended_mode: deny` — the coordinator's 61.10 correction, cited to research §2.6, §2.13), *The honest route to that capability, deferred and not rejected* (keeper serving MCP, DW-215), *Why voice and the wake word are Epic 62*, *Revisit triggers* (a third provider kind with a real endpoint, DW-214; the MCP threat model, DW-215; the model-weight licences resolving, Epic 62 — none reopens the first paragraph: a default endpoint is not a revisit, it is a different product), *Status / owner: decided*.
- **`docs/project-context.md`** gains five rule lines under *Rust Rules* and one anti-pattern: bots decisions live in `keeper-core::bots`, the shell is a call site; path containment is `keeper-sync`'s `browse::resolve`/`plain_segments`/`WriteScope::route`, never new path arithmetic; a capability keeper could not read is `None` on `BotModelVm.vision/tools/reasoning: Option<bool>`, never `Some(false)`; file content reaching a model is data, never instructions; a write is never auto-approved by a grant alone — `bots::grant::decide` at every call, audit row before effect; and nothing from the bots surface lives in a recording path, naming both gates. `rule_count` 42 → 48, `Last Updated` 2026-09-02. The rule lines name what is in the tree: the architecture companion says `grants::decide` and `Capability { True, False, Unknown }`; the shipped code is `bots::grant::decide` and `Option<bool>`.
- **The recording gate is left untouched and unwidened.** The new test `src/test/bots-surface-stays-out-of-recording.test.ts` reads `zero-egress.test.ts` off disk, extracts the body of `function recordingSources()`, whitespace-collapses it, and interprets every `scanDir(HERE|join(SRC, "<dir>"), (name) => <predicate>)` call and every bare `join(SRC, "<file>")`; predicates are compiled from a closed grammar (`name.startsWith("x")` / `name.endsWith("y")` joined by `&&`) without evaluating source, and an item-count assertion fails loudly if the gate gains a shape the interpreter does not understand. The bots surface is likewise derived by namespace — every production `.ts`/`.tsx` under `src/` in a `bots/` directory or with a `bot-`/`bots-`/`bots.`/`use-bots` basename, 24 files today — the same way the gate namespaces recording by prefix.
- **`tasks-pane.tsx:614`**: `TASKS_COLUMN_CLASS` gains `last:border-r-0`, the spelling every other column in the tree uses. Pre-existing on `main`; carried here because this epic is the change that made `check-design.mjs`'s output worth reading (it now measures the bot palette in both themes), and a one-violation baseline would hide the next real one.

**Block If:** the gate's globs could not be derived from its source — in which case a copied list would be the very drift this test exists to prevent, and the instruction was to report rather than copy.

**Never:** widen or edit `zero-egress.test.ts`; hand-copy its globs; add a bots verb to `command-palette/actions.ts` the scan would read as an upload or a transcription; add a `deny.toml` entry for a crate that needs none; touch any Rust source or any manifest.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Interpreter sanity | the gate file | exists; a rule over the gate's own directory was found; ≥ 4 `scanDir` rules; ≥ 3 explicit files; `components/command-palette/actions.ts` (the one shared file this epic touched) is in the interpreted set; the concrete scanned set is > 5 | test failure |
| (a) overlap | bots surface (> 10 files, never vacuous) ∩ the gate's concrete scanned set | empty | test failure listing the file |
| (b) per-file | every bots file | no `scanDir` rule claims it; not an explicitly named gate file; basename carries none of the gate's prefixes (`recording-`, `use-record`); its source does not spell `components/recording/` — the one glob that swallows every `.tsx` placed there | test failure naming the rule |
| Gate refactored | `recordingSources()` gains a shape the grammar cannot read | the item-count assertion fails before any set is compared | test failure, loud |
| The deliberate failure | `src/components/recording/bot-probe-card.tsx` created | 2 failed, 1 passed — (a) `expected [] … received ["…/bot-probe-card.tsx"]`; (b) `…would be claimed by the gate rule scanDir(…/src/components/recording, name.endsWith(".tsx"))`; file removed, suite back to 4 passed | Both halves catch it |
| Design gate | `node scripts/check-design.mjs` | `check:design — clean (397 web + 218 native files, src/index.css arithmetic verified, bot palette measured in both themes)`, exit 0 — was 1 violation at `tasks-pane.tsx:613` | exit code |
| Release diff | a tag is cut | the egress diff note fires on `docs/egress.md`'s new chapter | not observable locally |

</intent-contract>

## Code Map

| Path | Change |
|---|---|
| `docs/egress.md` | +45: the table row and the chapter *An AI provider is a destination you typed, disclosed as a host* (`:85`) |
| `docs/decisions.md` | +51: `## D-4 — The endpoint is yours, never keeper's` (`:116`) in the file's entry format, with AD, epic, research and DW references |
| `docs/project-context.md` | +8/−2: five *Rust Rules* lines, one anti-pattern line, `rule_count: 48`, `Last Updated` |
| `src/test/bots-surface-stays-out-of-recording.test.ts` | new, 219 lines: the interpreter, the namespace derivation, assertions (a) and (b) |
| `src/components/layout/tasks-pane.tsx` | +2/−1 at `:613-614`: `last:border-r-0` on `TASKS_COLUMN_CLASS` |
| `src-tauri/deny.toml` | unchanged — see Design Notes |

## Tasks & Acceptance

**Execution:**
- [x] `docs/egress.md` — the row and the chapter.
- [x] `docs/decisions.md` — D-4.
- [x] `docs/project-context.md` — the six lines and the counters.
- [x] `src/test/bots-surface-stays-out-of-recording.test.ts` — derived, not copied; proven by the deliberate failure.
- [x] `tasks-pane.tsx` — the design fix, with its reason recorded here.

**Acceptance Criteria:**
- Given a configured provider, when `docs/egress.md` is read, then it explains the row a person sees in Settings → About and why it is a host. **Met.**
- Given a proposal for a default endpoint, when `docs/decisions.md` is searched, then D-4 refuses it with the reason and names the revisit triggers that do not reopen it. **Met.**
- Given a bots file placed under a recording glob, when the suite runs, then the new test fails naming the file and the rule, and `zero-egress.test.ts` is unchanged. **Met** — the deliberate failure above.
- Given `scripts/check-design.mjs`, when run, then zero violations. **Met.**

## Design Notes

**Why `deny.toml` needed nothing.** The only crate this epic added to a manifest is `base64` (Story 61.12, `keeper-core/Cargo.toml` with its justification comment), and it is MIT OR Apache-2.0, already a workspace dependency since Story 34.15 with its justification at `src-tauri/Cargo.toml`; the allow-list needs no entry and the bans list is unaffected. `cargo deny … check licenses bans sources` → `bans ok, licenses ok, sources ok`, with one pre-existing warning unrelated to this epic (`"OpenSSL"` in the allow-list is unmatched by any crate in the graph).

**The one narrowing in the new test, recorded rather than allow-listed.** A first draft of (b) also banned any bots file from *spelling* a gate prefix in its source. That flagged `bot-message-meta.tsx`'s `import { formatElapsed } from "@/hooks/use-recording-session"` — Story 61.8's deliberate reuse of the recordings row's duration formatter. That import cannot make the gate cry wolf: the gate reads text by glob, and a bots file outside its globs stays unread whatever it imports. So the content check was narrowed to the real placement hazard — spelling the gate's directory — and the observation is left here: a neutral home under `src/lib/` for `formatElapsed` would decouple the two surfaces, and it is 61.8's file.

## Verification

- `node scripts/check-design.mjs` → clean, exit 0 (was 1 violation before the `tasks-pane.tsx` fix).
- `bunx vitest run src/test/bots-surface-stays-out-of-recording.test.ts src/components/recording/zero-egress.test.ts` → 2 files / 4 passed.
- `bunx biome check` on the new test and `tasks-pane.tsx` → clean, after one `--write` pass re-wrapped a template literal in the new test.
- `cargo deny --manifest-path src-tauri/Cargo.toml check licenses bans sources` (cargo-deny 0.20.2, installed on this host for the purpose) → `bans ok, licenses ok, sources ok`.
- The deliberate failure, as the mutation proof: the probe file in the gate's directory turned the suite `2 failed | 1 passed`, both halves naming it; removed, `4 passed`.
- The coordinator's full gate afterwards: 4028 Rust tests, 5466 frontend tests, lint/typecheck/fmt/clippy clean, `check-design` at zero violations.

## Auto Run Result

Status: implemented and gated; uncommitted; not reviewed.

**Not verified here, and why.** The release workflow's egress diff note actually firing on `docs/egress.md` runs in `release.yml` on a tag, not locally; the chapter's claim rests on the file's own existing description of that mechanism. The `keeper` shell crate was not built — this story changes no Rust and no manifest, so nothing it owns needed it. The Settings → About screen rendering the provider row was not driven visually; the row's derivation is Story 61.1's `keeper-core::egress` unit tests, read but not re-run here. Repo-wide `bun run check`/`tsc` were the coordinator's; the `tasks-pane.tsx` change is a string-constant edit that biome accepted and the design gate now passes on.
