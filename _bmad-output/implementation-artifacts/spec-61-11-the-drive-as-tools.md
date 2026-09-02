---
title: 'Story 61.11: the drive as tools'
type: 'feature'
created: '2026-09-02'
status: 'review'
baseline_revision: '9e06c2b'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
---

binds: FR-389, FR-391, NFR-49 → AD-159; FR-390, NFR-48 → NFR-48; AD-65 (the one lexical containment rule, consumed verbatim), AD-89 (write routing, consumed), AD-40 (`keeper-core` never depends on `keeper-sync`, consumed), AD-158 (the grant checked before every body, consumed)
depends on: 61.10 — absolutely: the grant exists before the first tool can read a byte, and a story order that inverts that ships an unguarded filesystem for one PR's duration

<intent-contract>

## Intent

**Problem:** The owner asked for *"tool calls for access in the drive data as a filesystem — and use of agent.md, claude.md, okf etc"*. The epic's finding: every primitive exists and nothing composes them — `browse::resolve`, `plain_segments`, `WriteScope::route` are the containment rule (`browse.rs`, `files_write.rs`), already carried across a verb boundary once (`engine.rs:10159-10160`); a read may land on a *pointer* file, not content (epic 56); and only an Ollama model advertising `tools` can be given them, because Hermes executes its own tools on its own host. The epic also corrected "okf" twice on 2026-09-02: it is `keeper_core::notes::okf`, keeper's own OKF v0.2 frontmatter view, whose local record lives on the drive at `.okf/OKF-0.2-digest.md`, and whose trust family carries `ActorKind` — *did a person look at this, or only a model*.

**Approach:** Shipped in two halves. The **engine** half: `keeper_core::bots::tools` (the seven tool specs as closed JSON-schema objects, `parse_call`, `render_result`, `okf_facts`, the `ToolHost` port, `run_tool_loop` with its bounds), `keeper_core::bots::context_files` (the nearest-first walk, the budgeted merge, `UNTRUSTED_PREAMBLE`), `keeper_sync::bots_fs` (the bodies: `list`/`read`/`stat`/`glob`/`grep`/`plan_write`/`write_unmanaged`/`edited_text`, every path through `browse::resolve` + `plain_segments` + `WriteScope::route`), and `keeper::bots_tools::DriveToolHost` in the shell sequencing `grant::check` → `audit::append_intent` → `bots_fs` → `audit::complete`. The **UI** half: `BotToolCall` (a disclosure row per call — what it asked for, what the model was told, the bound, the provenance) and `BotContextNote` (*What the model was told about your drive* — the files included, the files dropped, the preamble verbatim), over `BotToolCallVm`/`BotContextBundleVm` projections whose `result` is derived by `render_result` so the row cannot show a body differing from the model's.

## Boundaries & Constraints

**Always:**
- **No path arithmetic exists in the new code.** Containment is `browse::resolve` (lexical join plus canonicalize on both ends, AD-59) / `plain_segments` / `WriteScope::route`, carried verbatim; a symlink pointing out of the root is refused *after* resolution; absolute, `.`, `..` and empty segments never reach the disk; the walk never follows a symlink (one pointing in is a second name for a file already walked, and a pair is a cycle). Mutation (c) below is the proof that the canonicalizing half is load-bearing.
- **Every cap is a named const in `keeper-core`, told to the model in the schema, and disclosed in the result:** `MAX_READ_BYTES` 64 KiB (~1500 lines, deliberately below `text_file::TEXT_EDIT_MAX_BYTES`: a person scrolls, a model pays per token); `MAX_LIST_ENTRIES` 200 (far below `browse::LISTING_CAP` 1000, which is sized for a scrollable pane); `MAX_GREP_MATCHES` 100; `MAX_MATCH_LINE_BYTES` 400 (a minified bundle is one two-megabyte line); `MAX_GLOB_PATHS` 100; `MAX_WALK_ENTRIES` 20 000 (a bound on *work*, not output — keeper's folders hold 100k files); `MAX_WRITE_BYTES` 1 000 000 (decimal, the same number as `TEXT_EDIT_MAX_BYTES`, so a file a model writes is one a person can still open); `MAX_TOOL_RESULT_BYTES` 80 KiB (above `MAX_READ_BYTES` so a full read plus its provenance header and truncation notice is never itself cut); `MAX_CONTEXT_FILE_BYTES` 32 KiB; `MAX_CONTEXT_TOTAL_BYTES` 64 KiB; `MAX_CONTEXT_DEPTH` 12.
- **The loop bounds:** `MAX_TOOL_ROUNDS` 8 — exhaustion answers every outstanding call with `ROUNDS_EXHAUSTED` and runs one final toolless completion, so the turn ends in prose rather than looping; `MAX_CALLS_PER_ROUND` 8 — parallel calls are supported, the rest are answered with `TOO_MANY_CALLS` so the model can re-ask rather than lose them silently.
- A read that lands on a pointer file returns `ToolOutcome::NotMaterialized { subpath, of_bytes, oid }` — the pointer's truth (real size, the act that would fetch it) — and **hydrates nothing**.
- A note read carries `OkfFacts`: `doc_type` (OKF `type:`, still `Option` — refusing to describe a doc that lacks it is the strictness OKF's conformance section forbids), `generated_by` and its `ActorKind`, the most recent `verified[].by` and its `ActorKind`, and `human_reviewed` — true iff any review has `ActorKind::Person`. Rendered as e.g. *Provenance: OKF type Playbook, generated by claude/opus-5 (a tool), reviewed by a person (human:marta)* / *nobody has reviewed it* / *checked but not by a person*. `okf_facts` reads through `notes::okf::read` over `Frontmatter::parse`; **nothing in this story writes OKF** — every note-body write still goes through the scanner, and a tool that reflowed frontmatter would break FR-121's byte-level promise.
- All file content, context files included, enters the request under `UNTRUSTED_PREAMBLE` as data. Context files (`CONTEXT_FILE_NAMES` = `AGENTS.md`, `CLAUDE.md`, `GEMINI.md`, `.cursorrules`, plus `OKF_DIGEST` = `.okf/OKF-0.2-digest.md`) are found nearest-first, the budget is spent nearest-first (so the nearest survives), and they are rendered root-first/nearest-last (so the nearest has the last word, CLAUDE.md's documented order); an oversize file is included as a stated prefix, never dropped; `ContextBundle` carries both what was included and what was skipped, which is what FR-391's *shown to the user as what the model was told* needs.
- A write is `plan_write` + temp-and-rename through the same router a human save uses, leaves no `.keeper.*.tmp` behind, and never creates a file; `edit` is read + single-occurrence substitution + the **same** routed write — not a second way to put bytes on the drive; a file above `MAX_READ_BYTES` cannot be edited at all (saving a prefix would delete everything past it).
- `grep` is a **literal** substring search, never a regex: the needle is written by a model from text it may have read off the drive, and a regex engine driven by untrusted input is a DoS surface with no upside. The description says so. Zero new dependencies.
- `tool_specs(GrantMode::Read)` returns exactly the five readers; `GrantMode::None` returns `[]`. `grant::check` runs inside `DriveToolHost::run`, once per call. `GrantVerdict::Ask` is an injected `Approver` port; `None` declines every ask — a missing approval UI must never read as consent.
- `bots_fs::Limits` has deliberately no `Default`: AD-40 forbids `keeper-core → keeper-sync`, and AD-55/56 put the numbers where the model is told about them; the shell's `limits()` maps the consts across in one place so no second set can drift.

**Block If:** nothing; the epic named every rule and every reuse.

**Never:** join a root and a subpath by hand; execute a shell string (D-3 stands, DW-213); follow a symlink in a walk; return a truncated result without saying so; write OKF; add a `keeper-core → keeper-sync` edge; obey an `AGENTS.md`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Bounded read; line range | a 1.2 MB file; `start_line` + `line_count` | the first 64 KiB and *Truncated at 65536 bytes of 1258291* in the text the model sees; the lines, and the result names which lines it returned | Disclosure IS the handling |
| Pointer file; binary file | a `Pointer::new(oid, size).render()` on disk; NUL-bearing content | `NotMaterialized` naming the real size and the act that would fetch it, nothing downloaded; refused rather than rendered as unreadable characters | `Refused { reason }` for the binary |
| Capped listing | 1240 entries | 200, sorted by name **before** the cap (a stable prefix, not a random subset), and *of 1240* | Disclosure IS the handling |
| Escape | `../x`, `/etc/x`, a symlink out | never becomes a target; the symlink is refused after resolution | `Refused`, and never a stream failure |
| Unknown verb / bad args / non-JSON arguments; round budget | from the model; the 9th round | a recoverable refusal answering the `tool_call_id`, never a panic; every outstanding call answered `ROUNDS_EXHAUSTED` and one final toolless completion | Sentence, not error; prose, not a loop |
| Grant denied | `check` says `Deny` | `BotsError::GrantDenied { reason }` quoting the `DENY_*` const — reaches the model **and** the user | Both surfaces |
| Note read | OKF frontmatter with `generated.by` a model and no person in `verified[]` | body plus provenance that never reads as human-reviewed | No error expected |
| Context budget | two at-cap files and a third | the third is named under *Found and left out* with the budget sentence | Disclosure IS the handling |
| UI, refused row; UI, no bundle | `outcome: refused`; `context: null` | ban glyph, dashed left rule, `data-refused="true"`, the refusing layer's sentence verbatim; nothing rendered — unknown is not none, and an empty bundle says keeper found no instruction files | No error expected |

</intent-contract>

## Code Map

| Half | Path | Change |
|---|---|---|
| engine | `src-tauri/crates/keeper-core/src/bots/tools.rs` | new, 1315 lines: `ToolName`/`ToolArgs`/`ToolCall`/`ToolOutcome` (+ `NotMaterialized`, `Text.okf`), `ToolHost`, `tool_specs(GrantMode)`, `parse_call`, `render_result`, `okf_facts`/`OkfFacts`, `ToolLoop` + `run_tool_loop` + events, every cap; `actor_word` made `pub` for the UI half |
| engine | `src-tauri/crates/keeper-core/src/bots/context_files.rs` | new, 371 lines: `CONTEXT_FILE_NAMES`, `OKF_DIGEST`, `UNTRUSTED_PREAMBLE`, `candidates()`, `merge()`, `ContextBundle::system_prompt()` |
| engine | `src-tauri/crates/keeper-sync/src/bots_fs.rs` (+ `lib.rs` `pub mod bots_fs;`) | new, 915 lines: the bodies over the containment rule; `Limits`; `FsRefusal` |
| engine | `src-tauri/crates/keeper-core/tests/bots_tools.rs`; `src-tauri/crates/keeper-sync/tests/bots_fs.rs` | new, 937 lines, 17 tests against a real scripted HTTP/1.1 SSE server and a fake `ToolHost`; new, 569 lines, 16 tests over real temp trees |
| engine | `src-tauri/crates/keeper/src/bots_tools.rs` (+ `lib.rs` `#[cfg(desktop)] mod bots_tools;`) | new, 492 lines: `DriveToolHost`, the routed write through `notes_vault`, the `Approver` port — **never compiled on this host**; registers no `#[tauri::command]` |
| engine | `src-tauri/crates/keeper-core/src/bots/error.rs`, `mod.rs`, `vm.rs` | `Tool { detail }`, `GrantDenied { reason }`; `pub mod context_files; pub mod tools;`; `BotToolCallVm` |
| UI | `src/components/bots/bot-tool-call.tsx` (+ test, 16) | replaced (26 → 330 lines): `BotToolCall`, `BotToolCallRow`, `botToolSummary`, `botToolTruncationNotice`, `botToolProvenanceLine`, `botToolClipNotice` (`BOT_TOOL_DOM_CHARS`, the pane clips at 2000 chars and says the model was given all of them) |
| UI | `src/components/bots/bot-context-note.tsx` (+ test, 8); `src-tauri/crates/keeper-core/tests/bots_tool_row_vm.rs` | new, 160 lines: `BotContextNote`, `botContextSummary`, `botContextFileLine`; new, 235 lines, 8 tests over the projection — the half a component test cannot see |
| UI | `src-tauri/crates/keeper-core/src/vm.rs` | `BotToolCallVm` extended (`arguments`, `result`, `outcome`, `bytes`, `truncatedAtBytes`, `ofBytes`, `entries`, `okf`, …; `compose(record, arguments, outcome)`); `BotToolOutcomeKind`, `BotOkfFactsVm`, `BotContextFileVm`, `BotContextSkipVm`, `BotContextBundleVm`; every `u64` carries `#[ts(type = "number")]` |
| UI | `src/lib/ipc/gen/BotToolCallVm.ts`, `BotToolOutcomeKind.ts`, `BotOkfFactsVm.ts`, `BotContextFileVm.ts`, `BotContextSkipVm.ts`, `BotContextBundleVm.ts`, `ToolName.ts` | regenerated; `client.ts` gains the 7 sorted type re-exports |
| UI | `src/components/bots/bot-message.tsx` | coordinator's file: `toolCalls = []`, `context = null` props; `<BotToolCall count={…} calls={toolCalls} />`; `<BotContextNote bundle={context} />` |

## Tasks & Acceptance

**Execution:**
- [x] `tools.rs`, `context_files.rs`, `bots_fs.rs`, `bots_tools.rs`, `error.rs` — the engine half, with the two integration suites.
- [x] `bot-tool-call.tsx`, `bot-context-note.tsx`, the VM projections and their Rust test — the UI half.

**Acceptance Criteria:**
- Given a symlink out of the root, when read, then it is refused after resolution. **Met** — `a_symlink_out_of_the_root_is_refused_after_resolution`; mutation (c).
- Given a 1.2 MB file, when read, then the model is told where the read stopped. **Met** — `a_read_is_bounded_and_says_where_it_stopped`, `a_truncated_read_discloses_the_bound_in_the_text_the_model_sees`; mutations (a), (b).
- Given a pointer file, when read, then the pointer's truth is returned and nothing is hydrated. **Met** — `a_read_of_a_pointer_returns_the_pointers_truth_and_hydrates_nothing`, `a_pointer_read_names_the_real_size_and_the_act_that_would_fetch_it`.
- Given a note, when read, then its OKF type and trust actor come with the body, and a model-only review never reads as human. **Met** — `a_note_read_carries_its_okf_type_and_trust_actor`, `a_note_reviewed_by_nobody_or_by_a_model_never_reads_as_human_reviewed`; UI mutation (b).
- Given a write, when it lands, then it is temp-and-rename with no temp left behind. **Met** — `a_write_is_routed_atomic_and_leaves_no_temp_behind` — against a real rename, not the watcher (see Auto Run Result).

## Design Notes

**Four defects found and fixed in the story's own code.** `list` took the first `max_entries` in readdir order and sorted afterwards, so a truncated listing was a random subset while its doc claimed a stable prefix — caught by `a_listing_is_capped_and_reports_the_real_count`. The first OKF fixture used a `\`-continued string literal, which strips the next line's leading whitespace — and YAML nesting *is* leading whitespace, so `generated:` was flattened and the test proved nothing. The first context-budget test used one oversize file, but 32 KiB is half of 64 KiB so nothing was ever dropped. `GlobResult` had no total-hit count, so the shell's `of_entries` was nonsense; `of_paths` added.

**Frozen-contract deviations, all additive.** `ToolOutcome::Text` gained `okf`; `Entries`/`Wrote` gained fields; `NotMaterialized` is a fifth variant. `run_tool_loop` takes `&ToolLoop { client, endpoint, host, default_profile_id }` because clippy's `too_many_arguments` (9/7) aborted the whole `keeper-core` lib compile and blocked 61.10's clippy runs. The nextest filter `-E 'test(bots_fs)'` matches nothing — `test()` matches names, not binaries; the working filters are `binary(bots_fs)` and `binary(bots_tools)`.

## Verification

- `cargo check -p keeper-core -p keeper-sync` and `cargo clippy … --all-targets -- -D warnings` → clean. `cargo nextest run -p keeper-core -E 'binary(bots_tools) + test(tools) + test(context)'` → 24 passed; `-p keeper-sync -E 'binary(bots_fs)'` → 16 passed; `-E 'binary(bots_tool_row_vm)'` → 8 passed.
- `bunx vitest run bot-tool-call.test.tsx bot-context-note.test.tsx` → 24 passed. `bunx tsc --noEmit` and `bunx biome check` → zero errors in any file this story owns.

| Mutation | Test that failed | Observed |
|---|---|---|
| (a) `bots_fs::read` `file.take(limits.max_read_bytes)` → `take(u64::MAX)` | `a_read_is_bounded_and_says_where_it_stopped` (+ the oversize-edit arm of `an_edit_replaces_exactly_one_occurrence`) | 2 failed of 16 |
| (b) `render_result` truncation block → `let _ = (truncated_at, of_bytes);` | `a_truncated_read_discloses_the_bound_in_the_text_the_model_sees` | 1 failed |
| (c) `resolve_existing`: `browse::resolve` → `browse::lexical_join` + `Path::exists` (AD-59's canonicalizing half dropped) | `a_symlink_out_of_the_root_is_refused_after_resolution` | 1 failed of 16 |
| (d) `tool_specs` filter → `mode != GrantMode::None` (a read-only grant is offered the writers) | `bots::tools::tests::a_read_only_grant_is_offered_no_writer` | 1 failed |
| (e) `run_tool_loop` `tools_offered = rounds < budget` → `true` | `the_round_budget_ends_the_turn_in_prose_rather_than_looping` | 1 failed of 24 |
| (a, UI) `botToolTruncationNotice` byte branch disabled | *discloses a bounded result on the collapsed row, in bytes* | 1 failed / 15 passed |
| (b, UI) review clause widened to `humanReviewed \|\| verifiedActor !== null` | *renders a note's OKF type and says when no person reviewed it* — rendered *reviewed by a person (bot:indexer)*, the manufactured claim | 1 failed / 15 passed |
| (c, UI) skipped-files block gated `false` | *names a file the budget dropped…* and *does not report an empty file as a dropped instruction file* | 2 failed / 6 passed |

Every restore `cmp`-verified byte-identical against the pre-mutation snapshot. The coordinator's full gate afterwards: 4028 Rust tests, 5466 frontend tests, lint/typecheck/fmt/clippy clean; `check:core-sync-free` stays green because `Limits` is an argument and not a shared const.

## Deferred

- No tool executes a shell string; the general exec kind remains Epic 60's, unbuilt. **DW-213.** Embeddings, RAG or any index over the drive — a second source of truth about the user's files. **DW-212.** Giving a Hermes bot the drive by keeper serving MCP — deferred, not rejected; it inverts the connection and needs its own threat model. **DW-215.**
- **The transport is not built.** The shell's `drive()` in `bots_ipc.rs` never calls `run_tool_loop` or `context_files::merge` — it forwards `ChatEvent::ToolCallDelta` names as `BotStreamEvent::ToolCall { name }` — and nothing persists a per-call row or a context bundle (`bot_messages` holds only the scalar `tool_call_count`). `DriveToolHost` is a Rust type whoever drives a conversation constructs and passes to `run_tool_loop`; the UI components are prop-driven and mounted, and the producing side is one `BotToolCallVm::compose(record, wire.arguments, outcome)` per recorded call plus one `BotContextBundleVm::compose(&bundle)` per turn. `BotMessageVm` was deliberately not widened mid-wave (every fixture in the tree would break in files other agents own), and no IPC command returning an empty list forever was added. DW id: to be minted by the coordinator.

## Auto Run Result

Status: implemented and gated; uncommitted; not reviewed.

**Not verified here, and why.** `bots_tools.rs` (492 lines) was never compiled — the `keeper` shell crate does not build on Linux — and it is the file that sequences `grant::check` → `append_intent` → effect → `complete`; 61.10's suite pins the ordering at the store level, nothing run here proves this call site honours it, and that is the single most important thing for a macOS run to check, along with the `notes_vault` signatures read only from `sync_ipc.rs`'s call sites and whether `Vault` is `Clone`. The epic asks for a real rename *under the sync watcher*; the test has the real rename, and "the watcher never sees a torn file" rests on the tier-0 `.keeper.*.tmp` exclusion (`exclude.rs`), not on an observed watcher pass. Every containment assertion ran on Linux; a read of `NOTES.MD` where `notes.md` exists is untested on a case-insensitive volume. The pointer fixture is keeper's own canonical encoder, not a pointer written by another git-lfs version. No serialized `BotToolCallVm` has crossed a real IPC boundary, so the `u64 → number` decision is asserted only by the generated `.ts` and tsc. No pixels: `data-refused`, the `.lucide-ban` glyph and `border-dashed` are asserted as attributes, never seen.

## Coordinator addendum, 2026-09-02 — the transport this spec recorded as unbuilt is built

This spec's *Deferred* section states that the shell never calls `run_tool_loop` or
`context_files::merge`, so no grant could ever be exercised. That was true when it was written and
was the epic's one genuine gap: `tool_specs`, `run_tool_loop`, `DriveToolHost` and the tool-row
components all existed while `bots_chat_send` offered no `tools` array and drove no loop — a
permission over nothing, which is the affordance AD-27 forbids.

It is now wired, and the decisions went to `keeper-core` where this host can test them:
`tools::offer_tools(kind, tools_supported, grants)` decides what goes in the request and why not
(reusing `grant::grant_offer`, so the pane's affordance and the request cannot disagree),
`context_files::context_targets(grants, profile_ids)` decides which drive files may be named, and
`tools::default_profile_id` decides what an unqualified path means. The shell's `arm_turn` reads the
live grants, and `drive()` runs `run_tool_loop_reporting` over a `DriveToolHost` on every turn — an
empty `tools` array is one completion through the same code path. `BotStreamEvent` gained `Context`,
`ToolResult` and `ApprovalAsked`; the approval round trip is wired end to end through
`bots_approval_answer` and blocks the turn until answered, rather than being replaced by a refusal.
14 new tests cover the offer arms, `context_targets` and `default_profile_id`, plus one over a real
socket that asserts each call is reported as it completes.

The `keeper` shell half of this was written on a host that cannot compile it; the macOS gate on
hesperia then found 12 type errors in it, all fixed, and now passes with 4630 tests.
