---
title: 'Story 58.5: a window nobody ran is still a fact'
type: 'feature'
created: '2026-08-31'
status: 'done'
baseline_revision: 'e315918'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
warnings: []
---

<intent-contract>

## Intent

**Problem:** Story 58.4 shipped a policy that can decline a window, and a declined window **leaves
no row anywhere**. `task_runs` rows are minted only by `db::claim_task` (`db.rs:3331`), which runs
only when a host is present and reaches the task, so `on_missed = skip` writes one `info` line and
then goes quiet: the Tasks view's *last run* stays silently stale, the run list says nothing, and
`tasks status` reports a task that has never run as though nothing had ever been due. That is
exactly the invisible-non-execution shape this whole feature exists to close — the engine already
treats it as the one place in the feature where a log level is load-bearing (`engine.rs:2160-2172`).

**Approach:** A sixth `TaskOutcome`, `Declined`, written as a **closed, zero-duration `task_runs`
row** in the same transaction as the forward-only window write, with `detail` naming the instant
declined, the policy that declined it, and the instant armed in its place. No existing outcome is
reused, and the story states why against their own doc comments.

## Boundaries & Constraints

**Always:**
- The row is **closed** (`finished_ms == started_ms`) and takes **no lease**: nothing ran, so a
  reader must never find an in-flight run or a held `running_host`.
- The record and the forward window write are **one transaction**. A declined window and the fact
  that it was declined are one fact; a crash between them would leave either a moved window nobody
  can account for, or a record of a decline that did not happen.
- `Declined` must survive `TaskOutcome::from_stored`'s round trip and must be **printed by
  `task_run_lines`** and carried on `TaskRunVm` — a fact nobody can read is not a fact.
- `task_exit_code` is total and gains its arm explicitly: a declined window did **not run and
  nothing is wrong**, so `EXIT_DEFERRED`, beside `Busy` and `Deferred` which mean the same thing to
  a wrapper script.
- The cap is respected: a declined row is trimmed by the same statement every other run row is, so
  a `skip` task cannot grow `sync.db` by declining every window forever.
- Only `skip` declines. `delay` does not — see Design Notes.

**Block If:** the record cannot be written in the same transaction as the window write.

**Never:** reuse `Busy`, `Deferred` or `Abandoned`; add `Declined` to `next_task_window`'s
`Busy | Deferred` retry group (that group means *"do not consume the window"*, and a decline has
already consumed it forward); raise a fault or a toast for a declined window (`note_task_outcome` is
never reached — nothing claimed a lease); write a declined row for a `delay`; touch
`src/components/layout/tasks-pane.tsx` or its test (owned by `Story582`, who adds the
`OUTCOME_LABELS` entry).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| A skip is recorded | `on_missed = skip`, window open past the grace | one `task_runs` row, `outcome = declined`, `finished_ms == started_ms`, `detail` naming the declined instant and the policy | No error expected |
| It cannot be read as a run that happened | the same row | `finished_ms` is set (so not in flight), `outcome` is neither `ok` nor `failed`, and duration is zero | No error expected |
| The last run moves | a task that had never run, then a skip | `TaskVm.lastRun` is the declined row, not `null` | No error expected |
| No lease is taken | the same row | `running_host` and `lease_until_ms` stay `NULL` on the task | No error expected |
| Two hundred declined windows | `skip`, absent for two hundred windows | **one** declined row, not two hundred (the policy declines one window, and the next decision is about a later one) | No error expected |
| The other host got there first | the compare-and-set affects no row | **no** record either: nothing was declined by this host | Logged at debug |
| Round trip | `outcome = 'declined'` in the store | reads back as `TaskOutcome::Declined` | An unknown spelling is still `unknown_outcome` |
| CLI | `tasks status` on a task with a declined run | the outcome column reads `declined` and the detail is printed | No error expected |
| Exit code | `task_exit_code(Declined)` | `EXIT_DEFERRED` — did not run, nothing wrong | No error expected |
| A delayed window | `on_missed = delay`, inside the grace | **no** declined row; the record is the run it eventually gets | No error expected |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/tasks.rs` -- `TaskOutcome` gains `Declined` with `as_str` and
  `from_stored` (`:282-330` after 58.4), and the enum's doc comment — which counts the non-failures
  — is corrected rather than left saying "three of the five".
- `src-tauri/crates/keeper-sync/src/db.rs` -- `skip_task_window` writes the row and trims, in the
  transaction it already opens (`:3378-3410`); its signature gains the facts a row needs (`host`,
  and the `detail` its caller composes).
- `src-tauri/crates/keeper-sync/src/engine.rs` -- `skip_task_window` composes the detail and passes
  `task_host()` (`:2189-2248`); `next_task_window` is **not** touched, and the story says why.
- `src-tauri/crates/keeper-syncd/src/commands.rs` -- `task_exit_code` (`:2972`) gains its arm;
  `run_outcome_word` and `task_run_lines` already carry any `as_str`, which the test asserts rather
  than assumes.
- `src/components/sync/task-form.tsx` -- untouched. The frontend change 58.5 needs is one
  `OUTCOME_LABELS` entry in `tasks-pane.tsx`, which `Story582` owns and adds.

## Tasks & Acceptance

**Execution:**
- [ ] `keeper-sync/src/tasks.rs` -- add `TaskOutcome::Declined`, quoting in its doc why none of the
  other five can carry it and why `Deferred` in particular must not.
- [ ] `keeper-sync/src/db.rs` -- write the closed zero-duration row and trim it, inside
  `skip_task_window`'s existing transaction, and only when the compare-and-set actually won.
- [ ] `keeper-sync/src/engine.rs` -- compose the detail from the instants the decision already
  holds, and pass this host's name.
- [ ] `keeper-syncd/src/commands.rs` -- `task_exit_code`'s new arm, with the reason beside it.
- [ ] `keeper-sync/src/db.rs`, `engine.rs`, `keeper-syncd/src/commands.rs` -- one test per matrix
  row above, including the negative that a declined row cannot be confused with a run that happened.

**Acceptance Criteria:**
- Given a task whose policy declines a window, when the tick that declines it runs, then a reader
  asking "when did this last run and what happened" gets an answer instead of silence.
- Given that same row, when any surface renders it, then nothing reports it as a run that succeeded,
  a run that failed, or a run still in flight.
- Given two hundred declined windows, when the history is read, then it holds one declined row.

## Design Notes

**Why no existing outcome can carry it, in their own words.** All five require a host to have been
present and to have reached the task, because all five are written by a host that took the lease.
`Busy` is *"the work could not start because its target was already in use"*; `Deferred` is *"the
work did not run because a condition it waits on was not met"*; `Abandoned` is *"the run was never
closed by the host that started it"* and is written *"by the next host when it reclaims an expired
lease"*; `Ok` and `Failed` both assert that the work ran. A declined window is the one case where
**no host ever claimed anything** — the decision is taken and recorded without a lease — so there is
no honest slot among them.

**Why `Deferred` in particular must not be reused.** `next_task_window` consumes it to retry within
`TASK_RETRY_MS` — `min(scheduled, finished + 60 s)` (`engine.rs:2295-2301`) — so `Deferred` means
*"try again very soon"*. Overloading it would silently turn `on_missed = skip` into
`on_missed = retry in a minute`, which is its exact opposite. And `Declined` must stay **out** of
that group for the mirror-image reason: a decline has already moved the window forward, so treating
it as a run that did not happen would rewind it and re-decide the same window a minute later.

**Why `delay` writes no record.** A delayed window is not declined — it is going to be served, and
its record is the run it gets. `decide` answers `None` on every tick inside the grace, so a row per
decision would be nine hundred rows for one fifteen-minute delay, and a row *once* would need state
the pure layer does not have and a column AD-139 forbids. So the vocabulary stays honest: `declined`
means *this window was abandoned and the next one armed*, and nothing else.

**Why the row is written where the window moves.** `skip_task_window` already opens the statement
that decides whether this host is the one declining the window — the compare-and-set on the observed
instant. `false` there means the other host got in first, and in that case there is nothing to
record: writing a declined row anyway would claim this host declined a window it did not. So the
record hangs off the `affected == 1` branch, in the same transaction, and the two facts cannot come
apart.

```
declined  0s ago  dev#4188  skip: this window opened 2h ago and was not run; \
                            the next one is armed for 1756700000000
```

## Verification

**Commands:**
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd`
  (with the git identity prefix) -- expected: at or above 3776 passed / 0 failed.
- `cargo clippy … -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` --
  expected: clean.
- `cargo fmt` -- expected: applied.

**Manual checks (if no CLI):**
- No shell-crate symbol is touched by this story: `TaskRunVm.outcome` is already a `String` and
  carries any `as_str` unchanged, so `keeper/src/sync_ipc.rs` needs no edit and the macOS gate has
  nothing new to check for 58.5.

## Auto Run Result

Status: done

**Rust:** 3779 passed / 0 failed (`-p keeper-sync -p keeper-core -p keeper-syncd`), against 58.4's
3776 / 0. `cargo clippy` over the three crates with `-D warnings`: clean. `cargo fmt` applied.

**No frontend change and no bindings change.** `TaskRunVm.outcome` is already a `String` and carries
any `as_str` unchanged, so `keeper/src/sync_ipc.rs` is untouched and the macOS gate has nothing new
to check for this story. The one frontend change 58.5 needs is the `OUTCOME_LABELS` entry for
`"declined"` in `src/components/layout/tasks-pane.tsx`, which `Story582` owns and was sent the exact
spelling and wording for.

**One thing this story changed that 58.4 had asserted.** 58.4's engine test
`a_skipped_window_is_declined_forward_and_the_task_runs_on_the_next_one` asserted that a declined
window left an **empty** history — which was true and was the defect. It now asserts one row whose
outcome is `Declined`, and the assertion's own message says why: before this outcome existed the
claim could only have been made about an absence, and an absence is exactly what invisible
non-execution looks like.

**One refactor, named because it was not asked for.** `claim_task`'s inline run-row trim became
`trim_task_runs`, because a declined row has to go through the same cap and a second copy of that
`DELETE` is a second chance to get its two conditions wrong — one rule, one place, two consumers,
which is the discipline the module already states about `upsert_task`'s three service edges.

**Mutation proof.** Two guards mutated, each owning test confirmed to fail, restore verified by
`md5sum` against the pre-mutation copy:

| mutation | owning test that failed |
|---|---|
| record written unconditionally instead of on the `affected == 1` branch | `declining_a_window_moves_it_forward_and_only_from_the_window_it_saw` |
| the row written as `Deferred` instead of `Declined` | `a_declined_window_is_recorded_and_cannot_be_read_as_a_run_that_happened` |
