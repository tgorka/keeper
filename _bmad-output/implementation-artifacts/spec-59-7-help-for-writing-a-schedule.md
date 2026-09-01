---
title: 'Story 59.7: help for writing a schedule'
type: 'feature'
created: '2026-09-01'
status: 'done'
baseline_revision: 'c7ae611'
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
---

<intent-contract>

## Intent

**Problem:** The schedule box is a bare text input with one placeholder (`0 3 * * *`) and one sentence
naming the grammar in passing (`task-form.tsx:174-183`). The dialect it accepts is bigger than that
sentence and smaller than cron: five-field cron with `*`, lists, ranges and steps, the three aliases
`@hourly` / `@daily` / `@weekly`, and `every <n><unit>` over `s|sec|secs|second|seconds`,
`m|min|mins|minute|minutes`, `h|hr|hrs|hour|hours`, `d|day|days` — with a floor of once a minute
(`MIN_SCHEDULE_INTERVAL_MS`), a ceiling of one year (`MAX_SCHEDULE_INTERVAL_MS`), no month or weekday
**names**, and a fourth refusal for a pattern that parses but names a date the calendar has no room
for (`0 0 30 2 *`). All of that was read out of `keeper-sync/src/tasks.rs:617-947` rather than out of
the epic's table, and the table is accurate. Nothing on screen offered any of it, so writing a
schedule meant knowing cron by heart or provoking refusals until one stopped arriving — and there was
no way at all to find out *when* an expression would actually fire before saving it and reading the
row back.

**Approach:** Two additions, and the second one is where the whole risk of this story lives.

*Offering the dialect* is a native `<select>` under the box whose options are nine expressions that
get **typed into the box**, each labelled expression-first (`0 3 * * * — every day at 03:00`). It is
an action, not a second view of the value: it always shows its own placeholder, never what was
picked. Plus one composed sentence naming the floor and the ceiling.

*Showing the next fire* is **one new IPC read**, and the rule it obeys is the one
`recording-destination-controls.tsx:16-25` already states: both the clock and the renderer belong to
Rust. `keeper_sync::tasks::preview_schedule` runs the same `TaskSchedule::parse` the write door runs
and the same `next_due_after` the 1 Hz tick runs, chaining each instant from the one before it; the
shell wraps it in `sync_task_schedule_preview`; the form renders what came back and computes nothing.

**Why no cron parser in TypeScript, stated as the decision it is.** A browser-side preview was the
obvious cheap route and it is the one thing this story must not do. It would have needed the dialect,
the calendar, vixie's surprising day rule (`tasks.rs:841-866`) and an opinion about the zone — four
things to drift, and the first symptom of any of them drifting is not a crash but a form that
promises a time the engine has no intention of keeping. A preview that can disagree with the engine
is worse than no preview, so the browser gains no parser, no regex and no arithmetic. The only date
work in TypeScript is `new Date(ms).toLocaleString()` over an instant Rust computed, which is this
repo's existing way to stamp an absolute instant (`recording-row.tsx:101`).

**Why the refusal is data and not an error.** The verb is asked on every keystroke, so most of what it
sees is half-typed. `SchedulePreview` is therefore an infallible two-variant enum — `Refused(String)`
or `Fires(Vec<i64>)` — which puts the classification in the type instead of in the shell's judgement:
there is no second class of failure for a caller to guess at, so the IPC arm's match is total. The
refusal travels as `Option<String>` on a **successful** read, so a half-typed expression is help
rather than a failed command.

## Boundaries & Constraints

**Always:**
- **One implementation of the dialect.** Every accepted form, every refusal sentence and every instant
  comes from `keeper-sync/src/tasks.rs`. Asserted from Rust's side, because only Rust can run the
  parser: `every_schedule_the_form_offers_is_one_this_dialect_accepts` reads
  `src/components/sync/schedule-offers.ts`, extracts each `expression` literal and parses it.
- **The offered list is help with *input*, never a validator.** Nothing is trimmed, nothing is
  pre-checked, no save is disabled, no refusal is pre-empted. The box's contents still go to
  `TaskSchedule::parse` verbatim, whitespace and all — the `=== ""`-not-`.trim()` rule the same field
  already follows, and the reason the id is sent untrimmed two lines above it.
- **A refusal arrives in Rust's own words.** Preview and save show the same sentence because both come
  off the same `Display`: `sync_ipc_error` puts `err.to_string()` in `message`, and
  `preview_schedule` puts `err.to_string()` in `Refused`. Pinned by
  `preview_schedule_answers_a_refusal_in_the_save_doors_own_words`, which compares against
  `TaskSchedule::parse`'s own error rather than a copied string.
- **Any sentence naming a constant is composed from it.** `taskScheduleBoundsNote(floorMinutes,
  ceilingDays)`, mirrored constants pinned to Rust's source text by a new guard beside 58.9's, and
  asserted at *other* values so a hand-typed number fails. Story 58.9 exists because this very form
  shipped "fifteen minutes" against a thirty-minute constant.
- **An empty box asks nothing.** `""` means *store no schedule*, which is what the note promises and
  what `submit` sends as `null`. The decision not to ask lives in the caller; the verb itself refuses
  `""` exactly as the write door does, so the two doors never disagree about which strings are a
  schedule at all.
- **A preview is never shown against text the box no longer holds.** Two independent hazards, two
  guards: the effect's cleanup abandons a superseded read, and the echo comparison on
  `TaskSchedulePreviewVm.expression` blanks an already-landed answer the moment the box changes. The
  second is not redundant — a keystroke cannot un-answer a read that already completed.
- **The preview is computed on the platform the tick uses.** The verb takes `tauri::State` and reaches
  the supervisor's platform instance, because `ShellSyncPlatform` *overrides* `utc_offset_minutes`
  (`keeper/src/sync.rs:114-131`) precisely to avoid the port default's `gix` fallback to UTC
  (`keeper-sync/src/platform.rs:335-336`). A stateless verb would have previewed in UTC while the save
  reasoned in local time — the same lie in a different costume.
- **`keeper-core` stays `keeper-sync`-free** (AD-40). `TaskSchedulePreviewVm` is the wire shape and
  holds no dialect knowledge; it could not call the parser even if it wanted to. The shell composes it.

**Block If:** the next-fire instants could only be obtained by parsing the dialect in TypeScript.

**Never:** implement cron, the `@` aliases or `every <n><unit>` in TypeScript; disable the save button
on a client-side verdict; trim or normalise the schedule box; reword a Rust refusal; name the number
of previewed instants in prose; type a floor, ceiling or default as a literal in a sentence; edit
`keeper-sync/src/engine.rs`, `docs/**`, `src/components/layout/tasks-pane.tsx`,
`keeper/src/sync_ipc.rs`, `src/lib/ipc/client.ts` or `dev/mock-shell.ts` (owned by siblings this wave);
edit `keeper-core/src/tasks.rs` between the paced-work block's bounds; touch
`_bmad-output/planning-artifacts/**`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Empty box | `schedule` is `""` | no verb call, no preview paragraph; save sends `null` | No error expected |
| Box cleared after an answer | typed `@daily`, preview shown, then cleared | preview disappears with the text | No error expected |
| Whitespace only | `" "` | asked about, and comes back refused quoting `""` — not treated as empty | `Refused`, rendered as help |
| A valid cron | `0 3 * * *` | the next instants Rust chained, each stamped absolutely | No error expected |
| An alias | `@weekly` | desugared by Rust to `0 0 * * 0`; instants are Sundays | No error expected |
| An interval | `every 90m` | instants 90 minutes apart, each from the previous | No error expected |
| Malformed | `eee`, `@daily 03:00` | `Refused` with the 5-field/alias/interval sentence, quoting what was typed | `Refused`, rendered as help |
| Below the floor | `every 30s` | `Refused` naming the one-minute floor — not an unknown unit | `Refused`, rendered as help |
| Above the ceiling | `every 400d` | `Refused` naming the one-year ceiling and pointing at cron | `Refused`, rendered as help |
| Parses, matches no instant | `0 0 30 2 *` | `Refused` with *matches no instant*, in Rust's own words | `Refused`, rendered as help |
| Save while a refusal is previewed | any refused expression | save is **live**, sends the text verbatim, and shows Rust's refusal in the error paragraph | `SyncError::Config` at the write door |
| Out-of-order replies | slow answer for `0 3 * * `, fast for `0 3 * * *` | the stale answer never reaches state | No error expected |
| Answer for the previous text | answered `@daily`, then typed `0 0 30 2 *` | preview blank until the new answer lands | No error expected |
| Preview read fails | verb rejects | silent: no preview, and no second error paragraph | swallowed deliberately |
| Parsed, no instants | `Fires(vec![])` | nothing rendered — no empty `Next:`, no sentence about a search window | No error expected |
| Choosing an offer | any menu option | typed into the box; menu returns to its placeholder | No error expected |
| Offer then edit | `@daily`, then edited to `0 3 * * 1 ` | the edited text is what is sent, trailing space included | No error expected |
| `count == 0` asked of Rust | `preview_schedule(expr, …, 0)` | parses first, then `Fires(vec![])` — not a refusal | No error expected |
| Saturated interval | `Every` at `i64::MAX` | the walk stops rather than repeating one instant | strictly-increasing guard |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/tasks.rs` -- **gains** `SchedulePreview` (infallible two-variant
  enum) and `preview_schedule(expression, now_ms, utc_offset_minutes, count)`, placed between
  `impl TaskSchedule` and `impl CronSpec`; plus three tests at the end of `mod tests`:
  `every_schedule_the_form_offers_is_one_this_dialect_accepts`,
  `preview_schedule_walks_the_dialects_own_cadence` and
  `preview_schedule_answers_a_refusal_in_the_save_doors_own_words`. Nothing existing is modified —
  `TaskSchedule::parse` (`:630`), `next_due_after` (`:742`), `MIN_SCHEDULE_INTERVAL_MS` (`:31`) and
  `MAX_SCHEDULE_INTERVAL_MS` (`:42`) are read and not touched. The new offers guard is a deliberate
  copy of the shape of `every_schedule_the_dev_harness_shows_is_one_this_dialect_accepts` (`:1783`).
- `src-tauri/crates/keeper-core/src/tasks.rs` -- **gains** `TaskSchedulePreviewVm` at the file tail,
  below `plural_words` and above `mod tests`. Deliberately outside the paced-work block (`:720-1100`),
  which a sibling agent owned this wave.
- `src/components/sync/schedule-offers.ts` -- **new.** `TaskScheduleOffer`, `TASK_SCHEDULE_OFFERS`
  (nine forms), `TASK_SCHEDULE_FLOOR_MINUTES`, `TASK_SCHEDULE_CEILING_DAYS`,
  `taskSchedulePeriodPhrase`, `taskScheduleBoundsNote` and `TASK_SCHEDULE_BOUNDS_NOTE`. Imported only
  by `task-form.tsx`, and read as a text file by the Rust guard above.
- `src/components/sync/task-form.tsx` -- the header's schedule bullet gains the 59.7 paragraph; new
  copy (`TASK_FORM_SCHEDULE_OFFER_LABEL`, `_PLACEHOLDER`, `_NOTE`, `TASK_FORM_SCHEDULE_PREVIEW_TESTID`,
  `TASK_FORM_SCHEDULE_REFUSAL_PREFIX`, `taskFormScheduleOfferText`, `taskFormScheduleFiresNote`, and
  `TASK_SCHEDULE_BOUNDS_NOTE` re-exported); `schedulePreview` state, the preview effect keyed on
  `form.schedule`, the `shownPreview` echo gate; and three new renders under the schedule row. The
  existing schedule note, placeholder, `submit` and every other control are untouched.
- `src/components/sync/task-form.test.tsx` -- `syncTaskSchedulePreview` in the module mock and an
  echoing default in `beforeEach`; `previewVm` factory; the seven-test
  `TaskForm, help for writing a schedule` describe; and a new four-test describe
  `the schedule bounds note states the numbers Rust refuses at`, beside 58.9's guard rather than
  inside it.
- `src/lib/ipc/gen/TaskSchedulePreviewVm.ts` -- **new**, generated by `cargo test -p keeper-core`.
- **Written by `StoryPaced` to this story's agreed contract, not by this story** (their files by the
  wave's ownership split, and named here so a review can find them):
  `keeper/src/sync_ipc.rs` (`sync_task_schedule_preview`, `TASK_SCHEDULE_PREVIEW_COUNT = 3`),
  `keeper/src/lib.rs` (registration), `src/lib/ipc/client.ts` (`syncTaskSchedulePreview`),
  `dev/mock-shell.ts` (a real-dialect mock arm). **Shell crate: proved by the macOS gate, not here.**

## Tasks & Acceptance

**Execution:**
- [x] `keeper-sync/src/tasks.rs` -- `SchedulePreview`, `preview_schedule`, three tests.
- [x] `keeper-core/src/tasks.rs` -- `TaskSchedulePreviewVm`.
- [x] Agree the IPC contract with the owner of `sync_ipc.rs` / `client.ts` / `mock-shell.ts` before
  writing either side; hand them the compiled signature rather than a sketch.
- [x] `src/components/sync/schedule-offers.ts` -- the offered forms, the mirrored bounds, the composed
  sentence.
- [x] `src/components/sync/task-form.tsx` -- the offers menu, the bounds note, the preview and its two
  staleness guards.
- [x] `src/components/sync/task-form.test.tsx` -- eleven new tests over provenance, staleness,
  silence and the composed sentence.
- [x] Mutation proof, restored and verified by reading the diff.

**Acceptance Criteria:**
- Given somebody who does not know cron, when they open the form, then every shape of the dialect is
  offered as a labelled expression that types itself into the box, and each offered expression is one
  `TaskSchedule::parse` accepts — asserted by Rust over the TypeScript source.
- Given `0 3 * * *` typed, when the preview lands, then the instants shown are the ones the view model
  carried, and no cron arithmetic exists in TypeScript to have produced them.
- Given `0 0 30 2 *` typed, then the form says it matches no instant in Rust's own words, the typed
  text is still on screen unchanged, and the save button is still live and still sends that text.
- Given a reply that lands after a newer keystroke, or an answer already on screen when the box
  changes, then nothing is shown about text the box no longer holds.
- Given `MIN_SCHEDULE_INTERVAL_MS` or `MAX_SCHEDULE_INTERVAL_MS` changing, then the bounds sentence
  changes with it and the guard fails if it does not.

## Design Notes

**Why `preview_schedule` returns no `Result`.** The first draft was
`Result<Vec<i64>, SyncError>` and the IPC owner rejected it, correctly: it forces the shell to decide
which `Err` means *you typed it wrong* and which means *keeper is broken*, guessing at variants it does
not own. Every failure this function can have is a sentence for the person who typed the expression,
and that fact belongs in the type. The two-variant enum makes the shell's match total and removes a
classification nobody should have had to invent.

**Why the offered list lives in TypeScript and is proved from Rust.** The list has to render on first
paint, so it cannot be an IPC read; it is copy. But copy that names expressions is copy that can name
a refusal, which is precisely what Story 58.4 shipped when three dev-harness fixtures described
`@daily 03:00` — a syntax the dialect has never had. Only Rust can run the parser, so the guard is a
Rust test reading the TypeScript file. That direction is unusual here (this repo's other
cross-language guards read Rust from TypeScript) and it is the only direction in which the claim can
be *checked* rather than restated.

*Footnote worth keeping, because it is the best evidence the guard shape works:* while this story was
in flight the pre-existing sibling guard went red on `"…"` — the ellipsis — because a newly written
comment in `dev/mock-shell.ts` contained the literal token `schedule: "…"` while explaining the guard,
and the blunt extractor read the prose as a fixture. The extractor was deliberately left blunt rather
than taught to skip comments: it is cheap, its failure mode is a loud false positive, and the
alternative is a regex nobody can reason about guarding a claim that matters.

**Why the descriptions beside each expression are allowed to be prose.** `says` is the one part of
this story that no test can prove — nothing can mechanically check that `@weekly` "means" Sunday. It
is safe anyway, and the preview is why: choosing an offer fills the box, the box asks Rust, and Rust
answers with real instants. A description that drifted from its expression is contradicted on screen
by the engine rather than believed. The one fact worth stating explicitly, and stated, is that this
dialect counts weekdays from Sunday.

**Why the count of previewed instants is never named.** `Fires` may be shorter than asked, because the
search window is finite. A sentence saying *the next three* over a list of two is a small lie of
exactly the family this story exists to remove, so the sentence is `Next: ` and however many arrived.

**Why a parsed expression with no instants renders nothing.** Unreachable in practice —
`matches_any_date` has already refused the patterns that name no date, and `SEARCH_DAYS` covers eight
years — so any sentence written for it would be copy nobody can ever check. Rendering nothing is the
honest answer and it is asserted.

**Why the preview is muted text and not an alert.** It is help about text somebody is still writing.
The form has exactly one paragraph that reports a failure, and that one reports a save that actually
happened. A `role="alert"` on a per-keystroke hint would announce a refusal to a screen reader for
every intermediate state of every expression typed.

**Why the offers menu never shows what was picked.** A `<select>` that appeared to mirror the box
would be claiming to know which of its options some hand-written expression is, and for anything typed
the honest answer is *none of them*. So its `value` is pinned to `""` and it reads as an instruction.
This is the same class of trap `template-select.tsx` records and the profile picker above it guards
against — a control that misreports the value it appears to show.

**What was NOT built.** No debounce. One read per change of the box, which is `recording_path_preview`'s
own cost profile and the same order of traffic as the folder picker; the arithmetic is integer and
allocation-free (`tasks.rs:786-812` searches by day, not by minute). If this ever needs throttling, the
place for it is the caller and the guard for it already exists.

## Verification

**Commands:**
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync --lib tasks::` -- the dialect, the
  preview's cadence, the refusal wording and both offered-list guards.
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core` -- the wire type, and it regenerates
  `src/lib/ipc/gen/TaskSchedulePreviewVm.ts`.
- `bun run vitest run src/components/sync/task-form.test.tsx` -- provenance, staleness, silence, the
  composed bounds sentence.
- `bun run typecheck`, `bun run lint` (`biome check --write` over this story's three files only).
- `cargo fmt --manifest-path src-tauri/Cargo.toml --all` -- run before each commit, because the
  pre-commit hook formats the **whole tree** and therefore gates every agent on every other agent's
  formatting.
- **Not run here:** `cargo clippy --workspace`, `bun run test` in full and `scripts/check-macos.sh`.
  Three agents shared this worktree; the coordinator runs the project-wide gates once, and the
  `keeper` shell crate cannot link on Linux (`gobject-sys`).

**Measured, 2026-09-01, on `c7ae611` plus this story:**

- `cargo test -p keeper-sync --lib tasks::` -- 30 passed / 0 failed (27 before).
- `cargo test -p keeper-core` -- 2359 passed / 0 failed / 1 ignored; binding regenerated.
- `bun run vitest run src/components/sync/task-form.test.tsx` -- 43 passed / 0 failed (32 before).
- `bun run typecheck` -- clean.
- `bun run lint` -- this story's three files clean; the tree's remaining findings belong to a sibling's
  in-flight files plus one pre-existing committed `lint/style/useTemplate` error in
  `src/components/viewers/markdown-preview.ts:424`, which this story has not modified.
- **Shell-crate symbols this story's contract touches, for the macOS gate:**
  `keeper::sync_ipc::sync_task_schedule_preview`, `TASK_SCHEDULE_PREVIEW_COUNT`, and its
  `generate_handler!` registration in `keeper::lib`. Written by `StoryPaced`; unlinkable on this host.

**Mutation proof, observed.**

The echo gate (`shownPreview`) reduced to `const shownPreview = schedulePreview;` —
`never shows an answer about text the box no longer holds` failed with
*expected `<p …(2)></p>` to be null*, received the rendered paragraph
`Next: 10/9/2025, 8:54:20 AM` — `@daily`'s instant standing under `0 0 30 2 *`, which fires at no
instant at all. That is the exact defect the gate exists to stop, and it is the half the effect's
cleanup cannot cover, because a keystroke cannot un-answer a read that already completed. Restored;
verified by reading `git diff -- src/components/sync/task-form.tsx` (the gate's four lines present as
additions, no deletions anywhere in the file) and 43/43 green again.

## Review Triage Log

Not yet reviewed.
