---
title: 'Story 58.6: two hosts, one missed window, one run'
type: 'bugfix'
created: '2026-08-31'
status: 'done'
baseline_revision: '8d921ff'
final_revision: '2d37856'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
warnings: []
---

<intent-contract>

## Intent

**Problem:** A Linux box running **both** `keeper-syncd watch` and Story 57.7's `Persistent=true`
timer gets **two** runs for one missed window. The timer's `keeper-syncd tasks run` arrives as
`TaskTrigger::Requested`, which sets `due_at_most = None` and so bypasses `db::claim_task`'s
`next_due_ms <= ?5` window condition entirely, while the daemon's next tick independently claims the
same past window as `Scheduled`. Both claims succeed. It is reachable in ordinary operation rather
than in theory — `Persistent=true` fires a missed trigger at boot with no ordering against the
daemon's first tick, and a `Busy`/`Deferred` run already sets `next_due_ms` to
`min(scheduled, now + TASK_RETRY_MS)`, which can be past (`deferred-work.md:5023-5042`).

**Approach:** `due_at_most = None` exists for a real reason and keeps it: *a person asking is not
asking about a window*. So the fix is not to narrow what a request may claim — it is to stop calling
a **timer** a person. A third `TaskTrigger`, `Timer`, reached through an explicit CLI flag rather
than inferred from context, claims like a scheduled driver when an in-process host is also pacing the
task and like a request when nothing else paces it.

## Boundaries & Constraints

**Always:**
- **A hand-run with nothing due still runs.** That is what `due_at_most: None` is for, and the fix
  may not turn *run it now* into *run it if due*. Asserted directly.
- The distinction is **explicit in the CLI surface**, not inferred. A timer is a scheduled driver
  wearing a manual verb, and no amount of context-sniffing inside the engine can tell one from a
  person at a prompt — only the caller knows.
- A `Timer` run must still work in the unit's **primary documented arrangement**: timer only, no
  daemon, `--mode manual`. Such a task has `next_due_ms IS NULL`, so demanding an open window
  unconditionally would make the timer never run — a fix that silently breaks the configuration the
  shipped unit recommends first.
- The trigger is a **type**, not a `bool`, at every call site: this tree's own comments reject two
  adjacent booleans for exactly the reason that applies here.
- Recording is unchanged: a timer-driven run leaves the same `task_runs` row a scheduled one does.

**Block If:** the per-mode rule below cannot be stated in one place.

**Never:** narrow what `TaskTrigger::Requested` may claim; infer the driver from an environment
variable, a parent process or a TTY check; change `next_task_window`'s `Busy | Deferred` group; touch
any file `Story582` owns.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| The defect | one overdue window, `--mode scheduled`; the timer's run and the daemon's tick both reach it | **one** run recorded, not two | the loser records nothing |
| Hand-run, nothing due | `tasks run nightly` with the window in the future | runs, as it always did | No error expected |
| Hand-run, task overdue | `tasks run` by a person on an open window | runs; the window is served, so the next tick does not run it again | No error expected |
| Timer only, `--mode manual` | `tasks run --timer` on a task nothing schedules (`next_due_ms IS NULL`) | runs — the timer **is** the schedule here | No error expected |
| Timer, nothing due, `--mode scheduled` | `tasks run --timer` with the window in the future | does **not** run; exits `4` (did not run, nothing wrong) | reported as a deferral, not a failure |
| Off or disabled | either driver | refused, exit `2`, unchanged | an "off" that runs when asked is not off |
| Provenance | a timer-driven sync task's commit | names `cli`, distinguishing it from the engine's own `watch` and from a person's `manual` | No error expected |
| macOS | any | unaffected, and the story says so rather than testing a host that does not exist there | — |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/tasks.rs` -- `TaskRunDriver { Person, Timer }`, new: the public
  type the CLI and the IPC command pass, since `TaskTrigger` is private to the engine.
- `src-tauri/crates/keeper-sync/src/engine.rs` -- `TaskTrigger` gains `Timer` (`:553-575`);
  `claim_and_run`'s `due_at_most` match (`:2270-2290` after 58.5) gains the per-mode rule;
  `next_task_window`'s trigger match (`:2360-2390`); `run_task_now`'s signature (`:8000`).
- `src-tauri/crates/keeper-syncd/src/commands.rs` -- `TaskCommand::Run` gains `--timer` with its
  reason and its exit codes in the clap doc; `cmd_task_run` passes the driver.
- `src-tauri/crates/keeper-syncd/packaging/keeper-syncd-tasks@.service` -- `ExecStart` passes
  `--timer`, and the header's double-run warning is rewritten as a solved problem.
- `src-tauri/crates/keeper-syncd/packaging/keeper-syncd-tasks@.timer` -- the `--mode scheduled`
  paragraph, which currently tells the operator to prefer `--mode manual` to avoid this.
- `src-tauri/crates/keeper/src/sync_ipc.rs` -- `sync_task_run_now` passes `Person`. **Shell crate:
  one more symbol for the macOS gate.**
- `_bmad-output/implementation-artifacts/deferred-work.md` -- the entry at `:5023-5042`, closed.

## Tasks & Acceptance

**Execution:**
- [ ] `keeper-sync/src/tasks.rs` -- add `TaskRunDriver`, documenting why a timer is not a person.
- [ ] `keeper-sync/src/engine.rs` -- `TaskTrigger::Timer`, the per-mode `due_at_most` rule stated in
  one place, `source()` mapping it to `SyncSource::Cli`, and `run_task_now` taking the driver.
- [ ] `keeper-syncd/src/commands.rs` -- `--timer` on `tasks run`, threaded through `cmd_task_run`.
- [ ] the two packaging units -- pass the flag, and rewrite the warning the fix retires.
- [ ] `keeper/src/sync_ipc.rs` -- pass `Person`.
- [ ] `keeper-sync/src/engine.rs` -- the two tests the epic names: both drivers against **one**
  overdue window yielding one run, and a hand-run with nothing due still running.
- [ ] `deferred-work.md` -- close `:5023-5042` with the reasoning.

**Acceptance Criteria:**
- Given one overdue window and both drivers, when both reach the task, then exactly one run is
  recorded and the other is declined by the claim.
- Given a task whose window is in the future, when a person runs it by hand, then it runs.
- Given a `--mode manual` task and the timer, when the timer fires, then it runs.

## Design Notes

**Why the driver cannot be inferred.** The engine sees an id and a clock. Whether the caller is a
person at a prompt or a systemd `OnCalendar` is not a fact about the process, the environment or the
terminal — `keeper-syncd-tasks@.service` runs the same binary with the same argv shape a person
would type, and a person may equally run it from a script. Only the caller knows, so the caller says.
That is also why this is a flag rather than a heuristic: a heuristic here would be wrong silently, on
the one box where the defect exists.

**The one rule, stated once.** A `Timer` run demands an open window **exactly when an in-process host
is also pacing the task** — that is, when `mode == Scheduled`. The two arrangements the shipped unit
documents fall out of it:

```
--mode scheduled + watch : the timer and the daemon race for one window, claim_task's
                           single UPDATE gives it to one, and the loser records nothing.
--mode manual   + timer  : nothing in-process paces the task, next_due_ms is NULL, and
                           the timer IS the schedule — so it claims like a request.
```

**Why `SyncSource::Cli` for the timer.** `TaskTrigger::source`'s doc says `Cli` is *"deliberately not
used: it would make an in-process due-gate indistinguishable from the real `keeper-syncd` verb"*.
That reasoning is about the `Scheduled` trigger, and it argues **for** `Cli` here: a timer run *is*
the real verb, driven by policy rather than by a person, and a commit that says `cli` is the honest
record of it. `Watch` would claim the engine's own tick did it; `Manual` would claim a person did.

## Verification

**Commands:**
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd`
  (with the git identity prefix) -- expected: at or above 3780 passed / 0 failed.
- `cargo clippy … -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` --
  expected: clean.
- `cargo fmt` -- expected: applied.

**Manual checks (if no CLI):**
- macOS is unaffected: `keeper-syncd` has no launchd plist anywhere in the tree, so the app is the
  only host there and the two-driver arrangement cannot arise. Stated rather than tested.
- Shell-crate symbol for the macOS gate: `keeper/src/sync_ipc.rs::sync_task_run_now`.

## Auto Run Result

Status: done

**Rust:** 3783 passed / 0 failed (`-p keeper-sync -p keeper-core -p keeper-syncd`), against 3780 / 0
at `8d921ff`. `cargo clippy` over the three crates with `-D warnings`: clean. `cargo fmt` applied.

**No frontend change, no bindings change.** `sync_task_run_now` passes `TaskRunDriver::Person` and
its wire types are untouched.

**Mutation proof — and the first pass found a bad test, which is the finding worth recording.**
Mutating the per-mode rule away left `one_missed_window_and_both_drivers_yield_exactly_one_run`
**passing**. The test drove the timer first and the daemon's tick second, and in that order the
double run does not occur *whatever* `due_at_most` says: `next_task_window` moves the window on the
first run whichever driver made it, so the second driver finds nothing open. The assertion was true
for a reason that had nothing to do with the fix.

Corrected to the order that actually reveals the defect — the daemon's tick serves the missed window
and moves it forward, and *then* the timer fires, which is exactly what `Persistent=true` firing with
no ordering against `keeper-syncd watch` produces. The reverse order is noted in the test's own
comment as the one that proves nothing, so nobody re-derives it. Three mutations, each owning test
confirmed to fail, every restore verified by `md5sum` plus a re-read of the mutated site:

| mutation | owning test that failed |
|---|---|
| `TaskTrigger::Timer => None` (a timer claims like a person again) | `one_missed_window_and_both_drivers_yield_exactly_one_run` |
| `TaskTrigger::Timer => Some(now_ms)` (the naive fix: a timer always demands a window) | `a_timer_is_the_whole_schedule_when_nothing_in_process_paces_the_task` |
| `--timer` dropped from the unit's `ExecStart=` | `the_shipped_task_service_runs_a_verb_this_binary_has` |

The third matters more than it looks: nothing else in the tree reads that line, so without an
assertion on it the flag could be dropped in a packaging edit and the defect would return silently.

**Deferred entry closed.** `deferred-work.md:5023-5042` now carries `status: done 2026-08-31` and a
resolution naming the mechanism, the fix, the three tests and the macOS answer.

**macOS gate — shell-crate symbol touched:** `keeper/src/sync_ipc.rs::sync_task_run_now`.
