---
title: 'Story 59.6: how long is the delay'
type: 'feature'
created: '2026-08-31'
status: 'done'
baseline_revision: '925bdf4'
final_revision: 'd2493d7'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
---

<intent-contract>

## Intent

**Problem:** The owner asked *"if window missed when choose delay add option how much delay"*. Today
the answer is a compile-time constant with no override anywhere:
`TASK_MISSED_DELAY_MS = 30 * 60_000` (`keeper-sync/src/tasks.rs:95`), read by **exactly one**
production expression — `now_ms.saturating_add(tasks::TASK_MISSED_DELAY_MS)` in
`Engine::move_task_window` (`engine.rs:2295`) — and mirrored in three pieces of prose
(`docs/sync.md:2352`, `tasks set --help`, `TASK_MISSED_DELAY_MINUTES` in `task-form.tsx:180`).
Verified by grep over the whole tree; the epic's claim of one production reader is **correct**.
So `delay` means half an hour on every task on every install, and a person who wants four hours
after a laptop wakes has nowhere to say so.

**Approach:** One additive nullable column, `missed_delay_ms INTEGER` with **no** `DEFAULT`, added by
the existing `ensure_task_columns`. `NULL` means *use the constant*, which is what keeps every row
written before this column existed meaning exactly what it meant — `ensure_journal_columns`' nullable
rule, not a `DEFAULT 1800000` that would freeze today's number into old rows and make a later change
to the constant a lie about them. The resolution lives in one named pure function,
`tasks::effective_missed_delay_ms`, and the one engine reader calls it. The refusal lives in one
named pure function, `tasks::validate_missed_delay_ms`, called from the write door
(`db::upsert_task`). Reachable from **both** writers in this story: `--missed-delay` /
`--no-missed-delay` on `keeper-syncd tasks set`, and a control in `task-form.tsx` that appears only
when the policy is `delay`.

**The anchor does not move.** 58.4's review moved it to *the instant a host noticed the window*, and
that is precisely what makes a per-task value coherent: anchored on the window, one row's chosen
fifteen minutes and another's chosen four hours are both already in the past for any absence longer
than either, so both collapse to `run_now` and the new knob would be as dead as the old option was.

## Boundaries & Constraints

**Always:**
- **`None` means the constant.** Asserted directly (`effective_missed_delay_ms(None) ==
  TASK_MISSED_DELAY_MS`) and asserted `!= 0`, because `unwrap_or(0)` is the one typo this function can
  contain and it would silently turn every `delay` task into a `run_now` task at a cost of one extra
  write and one `postponed` row per absence.
- **Nullable, no `DEFAULT`.** The column diverges from `on_missed`'s `NOT NULL DEFAULT 'run_now'` on
  purpose: there the default is a *value*, here the default is *"whatever this build's constant is"*,
  which no SQL default can express. `upsert_task`'s `INSERT` names its columns, so an older binary
  writing against the newer schema must still succeed — a nullable column with no default satisfies
  that for free.
- **The floor is `TASK_MISSED_GRACE_MS`, inclusive.** The grace is the interval that concludes nobody
  was home; a delay shorter than it would elapse before the window it holds back counted as missed,
  so the next tick would serve it and `delay` would be `run_now` wearing another name. Equal to the
  grace is accepted — impatient but coherent.
- **The ceiling is `MAX_SCHEDULE_INTERVAL_MS`** (one year), the schedule's own, for the schedule's own
  reason: the delay is stored as the instant the window is held back to, so one that far ahead is a
  row that reports itself enabled and scheduled while nothing ever runs — the exact shape
  `every 100000000d` is refused for.
- **One reader.** `effective_missed_delay_ms` has one production caller. A second is a second chance
  to read a `None` as a zero, and the doc says so.
- The anchor stays `now_ms` — the noticing. Not `next_due_ms`, ever.
- The form's note is **computed** from the effective value, and the DEFAULT stays pinned to Rust's
  source text by the 58.9 mirror guard, re-pointed rather than deleted.
- AD-139's no-second-instant rule is untouched: the delay is a *duration* column, and the
  postponement is still written into `next_due_ms` itself as one forward instant. Nothing enumerates.

**Block If:** honouring a per-task delay would need a second stored instant, or would require the
anchor to move back to `next_due_ms`.

**Never:** clamp a stored value on the read path (the bounds are a write-door rule, exactly as
`validate_id`'s are — `db::get_task` does not re-check ids either); re-implement the bounds in
TypeScript beyond the mirrored minutes the 58.9 guard already pins; add a second reader of the
constant; touch `src/components/layout/tasks-pane.tsx` or its test (owned by `Main` for 59.1);
touch `_bmad-output/planning-artifacts/**`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| A row from before the column | `migrate` over a store with no `missed_delay_ms` | reads back `None`, and `delay` waits exactly `TASK_MISSED_DELAY_MS` — behaviour identical to today | No error expected |
| Older binary's write | `INSERT` naming only the pre-59.6 columns | succeeds; the row reads `None` | No error expected |
| No override, `delay` | `missed_delay_ms = NULL`, host back two hours late | window held to `noticed + 30 min` | No error expected |
| Own override, `delay` | `missed_delay_ms = 4 h`, host back two hours late | window held to `noticed + 4 h`; ticks past the *constant* run nothing; the run happens once, past four hours | No error expected |
| Override below the grace | `--missed-delay 5` / `missedDelayMs: 300000` | refused, quoting the value and saying the grace period is the interval that concludes nobody was home | `SyncError::Config` at the write door |
| Override at the grace | exactly `TASK_MISSED_GRACE_MS` | accepted | No error expected |
| Override above a year | `MAX_SCHEDULE_INTERVAL_MS + 1`, `i64::MAX` | refused, naming the ceiling it shares with the schedule and why | `SyncError::Config` |
| Minute overflow from the CLI | `--missed-delay 9223372036854775807` | refused by the ceiling rather than wrapping | saturating conversion, then the ceiling refusal |
| Clearing the override | `tasks set nightly --no-missed-delay`, or an empty form box | back to `NULL`, i.e. back to the constant | No error expected |
| CLI omission | `tasks set nightly --schedule '@daily'` with a stored override | keeps the stored override, as `--on-missed` does | No error expected |
| `run_now` / `skip` with an override stored | any | the override is stored and reads back, and changes nothing: only `Delay` reaches `move_task_window`'s postpone branch | No error expected |
| Form, policy not `delay` | `onMissed` is `run_now` or `skip` | the delay control is **absent**, and the note carries the default | No error expected |
| Form, policy `delay` | the control shown, a value typed | sent as `missedDelayMs` in ms; the note states *that* number of minutes | Rust's refusal rendered verbatim |
| A newer keeper's out-of-range stored value | `missed_delay_ms = 5` written elsewhere | honoured as stored, not clamped; the postponement's `detail` names the instant chosen | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/tasks.rs` -- `TASK_MISSED_DELAY_MS`'s doc gains the
  default-rather-than-only-value paragraph; `validate_missed_delay_ms` and
  `effective_missed_delay_ms`, both pure, between `validate_id` and `decide`; two tests at the end of
  `mod tests`. `TaskState` is **not** touched: `decide` still answers only `Postpone`, and the
  duration is the host's to turn into an instant.
- `src-tauri/crates/keeper-sync/src/db.rs` -- `ensure_task_columns` gains the second column;
  `TASK_COLUMNS`; `StoredTask`; `read_task`; `decode_task`; `TaskRow.missed_delay_ms`;
  `upsert_task`'s `INSERT`/`DO UPDATE` and its call to `validate_missed_delay_ms`. `TaskRow::state`
  is **not** touched.
- `src-tauri/crates/keeper-sync/src/engine.rs` -- the one reader in `move_task_window` becomes
  `tasks::effective_missed_delay_ms(task.missed_delay_ms)`, with the comment saying why the anchor is
  what makes the column coherent; one new test beside 58.4's. **Committed by the coordinator** — see
  the Spec Change Log.
- `src-tauri/crates/keeper-core/src/tasks.rs` -- `TaskVm.missed_delay_ms` and
  `TaskSaveReq.missed_delay_ms`, both `Option<i64>` with `#[ts(type = "number | null")]`.
- `src-tauri/crates/keeper/src/sync_ipc.rs` -- `task_vm`'s projection and `sync_task_save`'s
  `TaskRow`. **Shell crate: proved by the macOS gate, not here.**
- `src-tauri/crates/keeper-syncd/src/commands.rs` -- `TaskSetArgs.missed_delay` /
  `no_missed_delay`; `cmd_task_set`'s three-way rule; the help guard extended to pin the new
  sentence's numbers to the constants. **Committed by the coordinator.**
- `src/components/sync/task-form.tsx` + `.test.tsx` -- `taskFormOnMissedNote(minutes)`,
  `TASK_FORM_ON_MISSED_NOTE` as its default composition, the conditional control, and the re-pointed
  mirror guard.
- `dev/mock-shell.ts` -- the new key on every `TaskVm` fixture and in `sync_task_save`'s echo.
  **Committed by the coordinator.**
- `src/lib/ipc/gen/TaskVm.ts`, `TaskSaveReq.ts` -- regenerated by `cargo test -p keeper-core`.

## Tasks & Acceptance

**Execution:**
- [x] `keeper-sync/src/tasks.rs` -- `validate_missed_delay_ms` (floor = grace, ceiling = the
  schedule's), `effective_missed_delay_ms`, the constant's amended doc, two tests.
- [x] `keeper-sync/src/db.rs` -- the nullable column through `ensure_task_columns`, `TASK_COLUMNS`,
  `StoredTask`, `read_task`, `decode_task`, `TaskRow` and `upsert_task`; the write-door refusal.
- [x] `keeper-sync/src/engine.rs` -- the one reader; the per-task-delay test.
- [x] `keeper-core/src/tasks.rs` -- the two wire fields.
- [x] `keeper/src/sync_ipc.rs` -- project and read them.
- [x] `keeper-syncd/src/commands.rs` -- `--missed-delay` / `--no-missed-delay`, the omission rule,
  and the extended help guard.
- [x] `src/components/sync/task-form.tsx` + `.test.tsx` -- the conditional control, the computed
  note, the re-pointed mirror guard.
- [x] `dev/mock-shell.ts` -- the fixtures and the echo.

**Acceptance Criteria:**
- Given a store written before the column existed, when `migrate` runs, then every row reads
  `missed_delay_ms = None` and every `delay` task waits exactly the constant.
- Given a `delay` task carrying four hours, when a host notices its window two hours late, then the
  window is held to `noticed + 4 h`, ticks past thirty minutes run nothing, and the run happens once.
- Given a delay below `TASK_MISSED_GRACE_MS`, when it is written from either door, then it is refused
  with a sentence naming the grace period as the interval that concludes nobody was home.
- Given the form's policy control set to anything but `delay`, then no delay control is rendered; set
  to `delay`, then the note states the effective number of minutes rather than a literal.

## Spec Change Log

### 2026-08-31 — three files moved out of this story's commit, by the coordinator

Four stories of Epic 59's Wave 2/3 ran in parallel in one worktree, and three files ended up holding
hunks from three different stories each: `keeper-sync/src/engine.rs`, `keeper-syncd/src/commands.rs`
and `dev/mock-shell.ts`. Because `git commit --only` takes worktree content rather than one agent's
hunks, there is no per-story ordering in which every commit builds alone — 59.9's `TaskKind::Verify`
arm in `engine.rs` needs 59.9's `tasks.rs`, this story's delay reader needs this story's `db.rs`
column, and 59.5's fixture line needs 59.5's `TaskRow` field. The coordinator therefore commits those
three files once, attributed to all the stories in them, in this repo's existing multi-story
convention (`fix(58.4,58.5): …`). This story's own commit carries everything else; the three files
above were written, compiled and tested here and are named in the Code Map so the review can find
them.

## Design Notes

**Why the resolution is a function and not an `unwrap_or`.** *"Absent means the constant"* is a rule,
and a rule spelled inline at its only call site is a rule that gets copied wrong the second time
somebody needs it. The named function also gives the mutation proof a target: reverting it to a bare
`TASK_MISSED_DELAY_MS` is a one-token edit, and a test suite that does not go red on it is a test
suite that never checked the column was read.

**Why the ceiling is the schedule's and not something new.** Both numbers answer the same question —
*how far forward may one stored instant be before the row is effectively silent* — and inventing a
second bound would create two places to disagree. The refusal says so out loud, so a reader who meets
it does not go looking for a delay-specific rationale that does not exist.

**Why the bounds are not re-checked on the read path.** `validate_id` is the precedent: `upsert_task`
refuses a padded id, and `get_task` does not refuse it again. A read on a 1 Hz tick has to answer, and
the two answers available to it — clamp, or refuse the row — are both worse than honouring what is
stored. Clamping would hold a window back to an instant nobody chose and leave no trace;
`move_task_window` already writes the instant it computed into a `detail` line a person reads, so an
unusual delay explains itself where it matters.

**Why the form control is conditional.** `delay` is one of three settings and the delay is a question
about only that one. A number box permanently beside a `<select>` set to `skip` is a control that
invites a value with no effect — and `TASK_FORM_ON_MISSED_NOTE`'s whole history is about not telling
somebody a thing that is not true.

**Why the note had to become a function.** The 58.9 guard exists because this sentence shipped with
the wrong number, and it works by asserting *"runs it N minutes after a host noticed it"* against
Rust's source text. With a per-task value, a fixed sentence would be wrong for every task that chose
otherwise — the same defect class, arrived at from the other side. So the sentence is composed from
the effective value, and the guard keeps pinning the *default* composition to Rust. Both halves are
asserted, and each number is asserted in its role rather than merely present, which is the lesson the
existing test's own comment records.

**Why the CLI takes minutes and the wire takes milliseconds.** Every number a person reads about this
setting is in minutes — the help, the form note, `docs/sync.md` — and `--missed-delay 240` is what
somebody types. The wire and the column are milliseconds because everything else on the row is, and
because `next_due_ms` arithmetic must not acquire a unit conversion. The conversion is saturating, so
`--missed-delay 9223372036854775807` meets the ceiling's refusal rather than wrapping into a
plausible small number.

## Verification

**Commands:**
- `cargo test -p keeper-sync --lib tasks::` and `... --lib db::` -- the pure rules, the column, the
  migration and the write-door refusal.
- `cargo test -p keeper-sync --lib engine::` -- the one reader.
- `cargo test -p keeper-syncd --lib` -- `--missed-delay` and the extended help guard.
- `bun run vitest run src/components/sync/task-form.test.tsx` -- the conditional control, the
  computed note, the mirror guard.
- `cargo test -p keeper-core` -- regenerates `src/lib/ipc/gen/TaskVm.ts` and `TaskSaveReq.ts`.
- **Not run here:** `bun run test`, `bun run lint`, `cargo clippy --workspace`,
  `scripts/check-macos.sh`. Four agents shared this worktree; the coordinator runs the project-wide
  gates once, and the `keeper` shell crate cannot link on Linux.

**Measured, 2026-08-31, on `0e17aca` plus the three coordinator-held files:**

- `cargo test -p keeper-sync --lib` -- 1110 passed / 0 failed.
- `cargo test -p keeper-syncd` -- 135 + 6 + 12 passed / 0 failed.
- `bun run vitest run src/components/sync/task-form.test.tsx` -- 31 passed / 0 failed.
- `cargo test -p keeper-core` -- passed; regenerated both bindings, committed from that run.
- `bun run typecheck` -- one remaining error, and it is the intended one:
  `src/components/layout/tasks-pane.test.tsx:156` needs `missedDelayMs: null` on its `taskVm()`
  factory. That file belongs to 59.1's owner, who asked for the break rather than an optional field.

**Mutation proof, both observed.**

- Engine reader reverted to a bare `tasks::TASK_MISSED_DELAY_MS`:
  `a_task_that_carries_its_own_delay_waits_that_long_and_not_the_constant` failed at
  `engine.rs:14168` --
  *assertion `left == right` failed: the row's own delay, anchored on the same noticing … left:
  Some(1700011800000) right: Some(1700024400000)* — half an hour where four hours was chosen.
  Restored; green again.
- `taskFormOnMissedNote`'s `${delayMinutes}` reverted to `${TASK_MISSED_DELAY_MINUTES}`: two tests
  failed — *composes the delay's number from the value it is given, not from the constant* and *is
  the sentence the form actually renders, at the default and at a chosen value*. Restored; 31 green.

## Review Triage Log
