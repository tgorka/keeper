---
title: 'Story 59.7: help for writing a schedule'
type: 'feature'
created: '2026-09-01'
status: 'done'
baseline_revision: 'c7ae611'
final_revision: 'f73b34e'
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
- **An interval previews exactly one instant.** `TaskSchedule::Every` fires `interval_ms` after the
  **end of the previous run** (`tasks.rs:534-541`), and `Engine::next_task_window` re-derives it from
  `finished_ms`, so the second instant depends on how long the first run takes and nothing can know it
  before the task has ever run. A cron pattern names wall-clock instants and has no such dependency,
  so it previews the full count. `docs/sync.md:1995-1999` states the same rule from the other end, and
  it was written before this story: *"`every <n>` measures from the end of the previous run, not from a
  fixed origin"*. A chained interval preview would have advertised the fixed origin the engine
  explicitly does not have.
- **A hostile `count` is bounded, not trusted.** `preview_schedule` is `pub` and reserves for what it
  is asked, so `MAX_SCHEDULE_PREVIEW_INSTANTS` caps it inside `keeper-sync` rather than relying on the
  shell's `TASK_SCHEDULE_PREVIEW_COUNT`, which lives in a crate this one cannot see.
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
| An interval | `every 90m` | **one** instant, 90 minutes from now — the second depends on how long the first run takes, so nothing claims it | No error expected |
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
| Saturated interval | `every 1m` at `now_ms = i64::MAX` | `Fires(vec![])` — the saturated instant is not *strictly after*, so it is not offered | monotonicity guard |
| Cron at the end of time | `@hourly` at `now_ms = i64::MAX` | `Fires(vec![])` — `checked_add` overflows and answers `None` | No error expected |
| Pre-epoch clock | `0 3 * * *` at `now_ms = -DAY_MS` | the two 03:00s either side of the epoch, in order | No error expected |
| Hostile `count` | `preview_schedule(expr, …, usize::MAX)` | capped at `MAX_SCHEDULE_PREVIEW_INSTANTS`, and the reservation with it | No error expected |
| A malformed answer from the wire | `refusal` set **and** `instants` non-empty | the refusal wins and the instants are not shown — pinned, not incidental | shell's match is total; cannot arise |
| Save in flight | `saving` true | the offers menu is disabled with every other control | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/tasks.rs` -- **gains** `SchedulePreview` (infallible two-variant
  enum), `MAX_SCHEDULE_PREVIEW_INSTANTS` and
  `preview_schedule(expression, now_ms, utc_offset_minutes, count)`, placed between
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
  echoing default in `beforeEach`; `previewVm` factory; the nine-test
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
- [x] `src/components/sync/task-form.test.tsx` -- thirteen new tests over provenance, staleness,
  silence, precedence and the composed sentence.
- [x] Mutation proof, restored and verified by reading the diff.
- [x] Two parallel read-only review lenses, and the eight findings they returned resolved or accepted
  with reasons — see the Review Triage Log.

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

**Why an interval previews one instant and a cron pattern three.** This is the story's own defect,
found by reading the parser's doc rather than by a test — and worth recording as such, because the
first version of the cadence test *asserted the wrong answer as correct* and therefore defended the
bug against exactly the adversarial and mutation passes that should have caught it. `TaskSchedule::Every`
measures from the **end of the previous run**; `Engine::next_task_window` re-derives it from
`finished_ms`; `docs/sync.md:1995-1999` says so in prose written months earlier. So chaining an
interval forward produced instants two and three that depend on how long each run takes — no
TypeScript involved, no arithmetic error, and still a promise the engine had no intention of keeping.
A cron pattern is immune because it names wall-clock instants: only a run that overran a slot moves
one. The fix asserts the single instant **and** asserts the absence of the chain, so a later
"improvement" that reinstates it goes red rather than shipping a plausible fiction.

**Why the count of previewed instants is never named.** `Fires` is very often shorter than asked — one
for every interval, and fewer than asked for a cron near its search horizon. A sentence saying *the
next three* over a list of one is a small lie of exactly the family this story exists to remove, so
the sentence is `Next: ` and however many arrived.

**Why the wire type can represent a state the enum cannot, and what pins it.** `SchedulePreview` is an
enum, so *refused* and *fires* are exclusive by construction; `TaskSchedulePreviewVm` is a struct with
an `Option<String>` beside a `Vec<i64>`, so on the wire they are not. A tagged union was considered and
declined: it buys no behaviour, because the surface has to decide what a malformed answer looks like
either way, and it would diverge from `RecordingPathPreviewVm`, the precedent this read is modelled
on. So the exclusion is stated on the type as the shell's to keep, and the renderer's precedence — **a
refusal wins, and instants arriving beside one are not shown** — is pinned by a test rather than left
to whichever branch happened to be written first. Rendering a next-fire time under a sentence saying
the expression never fires is the one outcome worse than either half alone.

**What the preview means on an edit form, since the wording does not say.** It answers *if you saved
this expression now, when would it next fire* — not *when will the stored row next run*. For a cron
pattern those are the same instant. They differ for an interval (finish-anchored) and for a row whose
window was postponed or declined, and the row's own armed `nextDueMs` is shown by the Tasks pane
rather than here. Left as it is deliberately: labelling it *"if saved now"* would dilute the common
case, which is somebody writing a schedule on an add form, and the pane already owns the other
question.

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

**Measured, 2026-09-01, on `c7ae611` plus this story (`e7e5314` and its review revision):**

- `cargo test -p keeper-sync --lib tasks::` -- 30 passed / 0 failed (27 before this story).
- `cargo test -p keeper-core --lib tasks::` -- 65 passed / 0 failed; binding regenerated twice, once
  per doc change on the wire type.
- `cargo clippy -p keeper-core -p keeper-sync --all-targets -- -D warnings` -- clean. The
  `proc-macro-error2` future-incompat note is a pre-existing dependency warning, not a lint failure.
- `bun run vitest run src/components/sync/task-form.test.tsx` -- 45 passed / 0 failed (32 before).
- `bun run typecheck` -- clean, once the sibling-owned `client.ts` wrapper landed.
- `bun run lint` -- **at baseline: 4 warnings, 1 info, zero errors**, this story's three files clean.
  Corrected claim, and the correction is worth keeping because it is the same error class this story
  is about: an earlier reading of this line said there were three errors including a
  `lint/style/useTemplate` **error** in `src/components/viewers/markdown-preview.ts:424`, and that was
  wrong twice over. The three errors were biome *format* diffs on files two siblings had in flight
  (and one of my own, before I formatted), all transient; and the `useTemplate` finding is one of the
  four standing warnings, not an error. It was read off a collapsed grep over a dirty tree and
  attributed by position rather than by severity. A sibling checked it independently and was right.
- `bun run test` in full, once, mid-story: 301 files / 5109 tests, 4 failed — all four in
  `src/components/layout/tasks-pane.test.tsx`, a sibling's in-flight 59.1 restructure. Nothing this
  story owns was red.
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
additions, no deletions anywhere in the file) and the suite green again — 43/43 at the time, 45/45
after the review revision added two more.

## Review Triage Log

Two parallel read-only lenses over `e7e5314`: an adversarial pass and an exhaustive edge-case walk.
Both independently found the interval-cadence defect, which this story had also found by reading
minutes earlier — three arrivals at one finding, and the reason it needed three is recorded above: the
committed test asserted the wrong answer as correct, so no mutation of the implementation could
surface it. **That is the lesson of this story.** A mutation sweep proves a test notices a change; it
cannot prove the test wants the right thing. Only reading the contract the code is supposed to honour
does that.

**Fixed now:**

| # | Sev | Finding | Resolution |
|---|-----|---------|------------|
| 1 | high | An interval previewed a chained cadence the engine never keeps | `preview_schedule` answers one instant for `Every`, the full count for `Cron`; asserted, plus an absence assertion so reinstating the chain goes red |
| 2 | medium | The cadence test canonised finding 1 as expected behaviour | Assertion rewritten; the absence is now part of the claim |
| 3 | low | `Vec::with_capacity(count)` trusted a `pub` fn's caller for an eager reservation | `MAX_SCHEDULE_PREVIEW_INSTANTS` bounds it inside `keeper-sync`, which owns the allocation; asserted at `usize::MAX` |
| 4 | low | The saturated-`Every` monotonicity guard had no test | Asserted at `now_ms = i64::MAX` for both variants, plus a strictly-increasing assertion over a bounded walk, plus a pre-epoch clock |
| 5 | low | `Fires`' doc claimed "never empty" absolutely, which the two clock extremes falsify | Doc qualified to *"at any clock a machine can actually hold"*, with the exception named and asserted |
| 6 | low | The wire type can represent `refusal` **and** `instants` together, a state the enum cannot | Exclusion stated on the type as the shell's to keep; the renderer's precedence (a refusal wins) pinned by a test; tagged union considered and declined with the reason |
| 7 | low | `disabled={saving}` on the offers menu was a stated behaviour with no guard | Asserted, including that it re-enables when the save settles |
| 8 | info | `taskSchedulePeriodPhrase` assumes a positive whole number its signature permits violating | Assumption documented, with why the mirror guard is the right place to catch a Rust constant that stops dividing |

**Accepted as-is, with reasons:**

- **The offer descriptions are prose no test can prove.** Both lenses independently checked all nine
  against `CronSpec::parse`, `parse_field` and `weekday_from_days` and found all nine true, including
  Sunday-as-0 and vixie's step semantics. The residual risk is mitigated by the preview: a description
  that drifted from its expression is contradicted on screen by the engine.
- **The bounds note says "366 days" where Rust's ceiling refusal says "a year".** Both are computed
  from the same constant and 366 days is the exact bound (`every 366d` is accepted, `every 367d` is
  not), so the note is more precise than the refusal rather than inconsistent with it. Making them
  agree would mean rewording the parser's message, which is outside this story.
- **The preview on an edit form answers *if saved now*, not *when the stored row next runs*.** Design
  note above; the reviewer's own recommendation was to leave it, because the pane already owns the
  other question and re-labelling would dilute the add-form case.

**Out of scope, reported not fixed:** the sibling-owned files this story's contract depends on. (An
earlier version of this line also flagged a lint error in `src/components/viewers/markdown-preview.ts`
that does not exist — see the corrected `bun run lint` note above.) One of those sibling files did
break during the wave, and it is worth recording because it is a lesson about guards that read source
text: the shipped
`every_schedule_the_dev_harness_shows_is_one_this_dialect_accepts` went red on `"…"` — the ellipsis —
because a newly written comment in `dev/mock-shell.ts` contained the literal token `schedule: "…"`
while explaining the guard, and the blunt extractor read the prose as a fixture. Its owner reworded
the comment. The extractor was deliberately **not** taught to skip comments, and the new offers guard
copies it unchanged: it is cheap, its failure mode is a loud false positive rather than a silent pass,
and the alternative is a regex nobody can reason about standing guard over a claim that matters.

**Docs:** `docs/sync.md` §14 does not describe the preview, and deliberately does not yet. The chapter
belongs to Story 59.10 and to the agent who holds that file; agreed with them explicitly rather than
left to chance, on the grounds that two stories writing one chapter is how a chapter grows two
descriptions, and that prose must not go in front of an IPC verb that is not committed yet — Story
56.13's exact failure. A draft paragraph and the two claims in it that need re-checking against
committed code (the number of offered forms, and *up to* three instants for a cron pattern) were handed
over for 59.10.
