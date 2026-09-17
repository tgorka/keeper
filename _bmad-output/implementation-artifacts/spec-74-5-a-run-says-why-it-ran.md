---
status: review
baseline_revision: 6e5695e
final_revision: ''
---

# Story 74.5 — A run says why it ran and how late

<intent-contract>

## Problem

The engine knows exactly why a task is running — `TaskTrigger::{Scheduled, Requested, Timer}` (`engine.rs:770-818`) decides claim semantics, the git commit's `SyncSource` and the next window — and then throws it away: the `TaskRunClose` literal at `engine.rs:3966-3975` has no field for it, and `task_runs` has no column. So the owner cannot tell a run he asked for from one the clock asked for.

"Late" is worse than missing: `finish_task_run` overwrites the served `next_due_ms`, so a run served forty minutes after its window leaves no trace that it was late. The missed-window machinery records *declined* and *postponed* windows — never a served-late one.

## Approach

Write down what the claim already knows, at the moment it knows it.

## Always

`task_runs.trigger` and `task_runs.late_by_ms`, written by `claim_task`; `late_by_ms` computed from the window the row carried **before** `finish_task_run` overwrites it, and zero for an on-time run. `TaskRunVm` carries both, the runs list shows both, the `tracing::info!` names both, and the mark file's name carries the trigger word so provenance survives without the database. One path for every kind — no kind-specific branch.

## Block If

Nothing.

## Never

Never infer the trigger at read time from timestamps. Never move the `columns_of` schema-pinning test's meaning: it moves with the schema, in the same change.

</intent-contract>

## Code Map

- `keeper-sync/src/db.rs:223-231` (schema), `:4140-4143` (`claim_task` INSERT), `:4277-4330` (`TaskRunClose`, `finish_task_run`), `:4340-4344` (the read), `:7111-7117` (`columns_of`).
- `keeper-sync/src/engine.rs:770-818` (`TaskTrigger`), `:3895-3975` (the claim, the run, the discard), `:4020-4024` (`next_task_window`), `:12062-12081` (`run_task_now`'s Person/Timer mapping).
- `keeper-core/src/tasks.rs:238-262` — `TaskRunVm`.

## Tasks & Acceptance

Acceptance, verbatim from the epic: *a run claimed by the scheduler, one requested by a person and one served after its window produce three distinguishable rows and three distinguishable file names; `late_by_ms` is zero for an on-time run and the measured lateness otherwise, computed from the window the row actually carried before `finish_task_run` overwrote it; every kind (sync, release, verify, bot, copy, gc) is covered by the same path — no kind-specific branch.*

## Verification

- `task_runs` gained `trigger TEXT` and `late_by_ms INTEGER` through a new `ensure_task_run_columns` guarded by `PRAGMA table_info`, additive-only; both are written inside `claim_task`'s transaction, from the trigger the caller passes and `served_window_ms` — `late_by_ms = (now_ms − window).max(0)`, so a clock that stepped backwards reads as on time rather than as negative lateness. `move_task_window`'s insert deliberately names neither: nothing ran, so a `0` there would be a measurement nobody took.
- The engine's discard at the `TaskRunClose` literal is closed: `claim_task` now receives `trigger.run_trigger()` and `task.next_due_ms`, captured **before** `finish_task_run` overwrites the window. `TaskTrigger::run_trigger` is the one mapping between the engine's control-flow enum and the persisted vocabulary, so a future divergence is a compile error.
- One path for every kind — the claim is kind-agnostic and no arm of `perform_task` branches on provenance.
- `TaskRunVm` carries `trigger: Option<String>` and `late_by_ms: Option<i64>`; `null` is "never measured" and is kept distinct from `0` on purpose, because a surface that rendered them alike would be inventing punctuality. The runs list renders the word and, only when there is lateness, the lateness: `says why a run happened, and how late, only when the run knows` pins all three cases including the pre-AD-253 row that says nothing.
- `cargo test -p keeper-sync -p keeper-syncd` green; the `columns_of` schema-pinning test moved with the schema; the full frontend suite green.
