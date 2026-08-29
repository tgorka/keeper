---
title: 'A task is something keeper remembers, and a schedule is a due-gate on the tick it already runs'
type: 'feature'
created: '2026-08-29'
status: 'done'
baseline_revision: '502612410003d32f881a877b6e6efd12181d2601'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/_bmad-output/planning-artifacts/epic-57-a-task-that-runs-when-it-should.md'
  - '{project-root}/_bmad-output/planning-artifacts/architecture/architecture-keeper-2026-07-03/ARCHITECTURE-SCHEDULED-TASKS.md'
warnings: []
---

<intent-contract>

## Intent

**Problem:** keeper remembers nothing about work it has done. `journal` is a queue whose
`db::complete` is `DELETE FROM journal WHERE id = ?1`, so a finished unit leaves no trace, and
`WorkKind` is a closed vocabulary of transfer primitives; `activity` is by its own doc "a
human-facing log, not a source of truth". So there is no name, no schedule, no last run and no last
result — and the owner asked for cron-like tasks on both hosts (FR-346…FR-348, NFR-42, NFR-43).

**Approach:** Two tables in the existing `sync.db` (AD-135), a pure `decide(state, schedule, now_ms)
-> Action` evaluated as a due-gate on the supervisor tick that already exists (AD-136 part 1), a
keeper-owned schedule parser that refuses at save time with the expression quoted (part 2), and a
`running_host` + `lease_until_ms` lease claimed in the same `UPDATE` that starts the run (part 3).

## Boundaries & Constraints

**Always:**
- `CREATE TABLE IF NOT EXISTS` inside the existing `db::migrate` batch. Schema addition only, so
  **no `meta` marker** — the tree's own rule: an `ALTER TABLE … ADD COLUMN` guarded by the column
  list is its own idempotence.
- One clock per host process (AD-62). The engine's existing `TICK_MS` supervisor tick is the only
  clock; the due-gate reads `self.platform.now_ms()`, never a system clock and never a sleep.
- The decision is pure: `decide` takes `&TaskState`, an `Option<&TaskSchedule>` and `now_ms: i64`,
  returns a `Copy` `Action`, touches no `self`, no clock, no database.
- Refusal, never coercion. A schedule keeper cannot parse — including one that parses to an instant
  that never arrives — is `SyncError::Config` at save time with the expression in the message.
- Exactly one runner, by lease: one `UPDATE … WHERE id = ?1 AND (running_host IS NULL OR
  lease_until_ms <= ?now)` whose affected-row count is the arbiter.
- Forward compatibility (NFR-43): a row whose `kind`, `mode` or `schedule` this build cannot read is
  **skipped and listed as unknown**, never fatal.
- NFR-42: a task that touches a working tree goes through the same `Engine::reserve` the tick's sync
  pass takes, so the two can never hold one git index at once; every task is idempotent and its
  abandonment is recorded rather than wedging the row.

**Block If:** nothing. Every decision below is derivable from AD-135/AD-136 and the tree.

**Never:**
- No new dependency, no new thread, no new `tokio::time::interval`, no `.timer` inside the process.
- **No `update` task kind.** `TaskKind` is a closed enum with no such variant and `upsert_task`
  takes the typed kind, so the only way to write one is raw SQL — where it is skipped as unknown.
- No CLI verb (57.3), no release-as-task wiring (57.4), no desktop host (57.5), no UI (57.6), no
  packaging (57.7). Nothing in the `keeper` shell crate.
- No `catch_up` policy, no cron field names (`MON`, `JAN`), no seconds field, no shell-string kinds.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| a cron schedule saved | `"0 3 * * *"` | stored; `next_due_ms` armed at the next 03:00 local | none |
| an alias saved | `"@daily"` | desugars to `0 0 * * *` — nightly means night, not "24 h from now" | none |
| an interval saved | `"every 30m"` | stored; fires every 30 min from the arming instant | none |
| a malformed schedule | `"0 3 * *"`, `"every 5x"`, `"@yearly"`, `"MON"` | refused **when written** | `SyncError::Config`, message ends `, got 0 3 * *` |
| a sub-minute schedule | `"every 30s"`, `"every 0m"` | refused | `SyncError::Config` naming the one-minute floor |
| a schedule that never arrives | `"0 0 30 2 *"` | refused at save time | `SyncError::Config`, "matches no instant" |
| first sight of a task | `next_due_ms IS NULL` | `Action::Arm` — window computed, **nothing runs** | none |
| the window opens | `next_due_ms <= now`, no lease | `Action::Run` | none |
| a live lease | `lease_until_ms > now` | `Action::None` on every other host | none |
| a dead holder | `lease_until_ms <= now` | reclaimable; the unfinished run is closed `abandoned` | none |
| two hosts race | two connections, one due task | exactly one `UPDATE` affects a row; one `task_runs` row | loser gets `Ok(None)`, not an error |
| an unknown kind | hand-written row `kind = 'teleport'` | skipped from `tasks`, listed in `unknown` | never fatal, never run |
| a task named `update` | hand-written row `kind = 'update'` | same: skipped and listed | never fatal, never run |
| the folder is syncing | reservation held, `Sync` task due | run recorded `busy`, nothing touches the index | `SyncError::Busy` is not a failure |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/tasks.rs` — NEW. The vocabulary (`TaskKind`, `TaskMode`,
  `TaskOutcome`), the dialect (`TaskSchedule`, `parse`), the pure `decide` and `next_due_after`.
- `src-tauri/crates/keeper-sync/src/lib.rs` — `pub mod tasks;`, beside `sparse`/`stability`.
- `src-tauri/crates/keeper-sync/src/db.rs` — `migrate` gains two tables and one index; `TaskRow`,
  `TaskRunRow`, `TaskListing`; `upsert_task`, `list_tasks`, `get_task`, `delete_task`,
  `arm_task`, `claim_task`, `finish_task_run`, `task_runs`, `TASK_RUNS_CAP`.
- `src-tauri/crates/keeper-sync/src/engine.rs` — `TASK_LEASE_MS`; `run_due_tasks` called once from
  `Engine::tick`; `perform_task` (closed match over `TaskKind`); the public door 57.3 will call:
  `save_task`, `tasks`, `task_history`, `forget_task`, `run_task_now`.
- `src-tauri/crates/keeper-sync/src/error.rs` — read only. `SyncError::Config` is the refusal type
  (`Retriability::Permanent`, `EXIT_CONFIG` = 2); `SyncError::Busy` already exits 0.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-sync/src/tasks.rs` -- write the vocabulary, the `TaskSchedule` parser
      over a 5-field cron / `@hourly|@daily|@weekly` / `every <n><unit>` grammar with a one-minute
      floor and a "matches no instant" refusal, `next_due_after(now_ms, utc_offset_minutes)`, and
      the pure `decide` -- one place holds the dialect and the state machine, testable against
      integer clocks with no engine and no database.
- [x] `src-tauri/crates/keeper-sync/src/tasks.rs` -- unit-test the matrix above: the refusal table
      (near misses plus positive boundaries, in the manner of `a_malformed_quiet_window_is_refused`),
      the alias desugaring, the cron boundary at the exact minute, `decide`'s arm/run/none/lease
      arms, and that `TaskKind::from_stored("update")` is `None`.
- [x] `src-tauri/crates/keeper-sync/src/lib.rs` -- declare the module.
- [x] `src-tauri/crates/keeper-sync/src/db.rs` -- add `tasks` and `task_runs` plus
      `task_runs_recent` to the `migrate` batch, and the row types and functions above; bound
      `task_runs` per task the way `record_activity` bounds `activity` (trim by `id`, in the same
      `unchecked_transaction` as the insert) -- the record has to survive an older binary and a
      dead host without growing without bound.
- [x] `src-tauri/crates/keeper-sync/src/db.rs` -- test: idempotent creation, an old database
      (a bare `Connection` carrying the pre-57 schema) migrating in place, the unknown-kind and
      `update`-kind skips, the bounded history, and **the lease raced by two real connections over
      one file** with the reclaim-after-expiry case.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- call `run_due_tasks` exactly once per tick
      beside the existing host-wide work, add `perform_task` dispatching `TaskKind::Sync` through
      the existing `sync_once` (which takes the same `reserve` the tick takes), and the four public
      methods 57.3 needs -- the tick is the only clock and the reservation is NFR-42's guarantee.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- test: a due task runs on the tick asserting
      **tick counts, never elapsed time**; the clock alone runs nothing; a held reservation records
      `busy` and syncs nothing; `run_task_now` records identically to a scheduled run.

**Acceptance Criteria:**
- Given a `sync.db` created by a pre-57 binary, when the engine opens it, then `tasks` and
  `task_runs` exist, no `meta` marker was written, and re-running `migrate` twice more changes
  nothing.
- Given a task whose schedule is `every 5m`, when `tick()` is called with the clock advanced by
  4 minutes 59 seconds and then by one more second, then `task_runs` holds exactly one row — and
  advancing the clock by an hour without calling `tick()` holds it at zero.
- Given one due task and two independent `rusqlite::Connection`s on one `sync.db` claiming it from
  two threads, when both `UPDATE`s have run, then exactly one reports a claim and `task_runs` holds
  exactly one row for that task.
- Given a claimed task whose host died (`lease_until_ms` in the past, `finished_ms IS NULL`), when
  another host claims it, then the claim succeeds, the abandoned run is closed with outcome
  `abandoned`, and a second run row begins.
- Given `TaskKind` as shipped, when a caller tries to name `update`, then no such variant exists to
  name; and a hand-written `kind = 'update'` row is skipped, listed as unknown, and never run.

## Design Notes

**Why `Sync` is the kind this wave implements, and `Release` is 57.4's.** A task needs at least one
kind whose effect is real, or "a due task runs" cannot be asserted without a stub. `sync --once` is
already documented as "the cron entry point", `sync_once` already exists, and it already opens with
`self.reserve(&profile.id).ok_or_else(|| SyncError::Busy(..))?` — which *is* NFR-42's guarantee,
provable rather than asserted. `release_expired` cannot be that kind: it carries its own hourly
`release_is_due` look-gate, so a task's schedule would not control it — threading a triggered-run
bypass through it, together with off/manual/scheduled, is exactly the substance of 57.4.

**`mode` decides who may trigger; `enabled` decides whether the row is live.** Only
`TaskMode::Scheduled` is ever *due*; `Manual` runs only through `run_task_now`; `Off` refuses even
that. Both columns exist per AD-135 and they answer different questions, so the gate reads both.

**Aliases desugar to cron, not to intervals.** `@daily` as "86 400 000 ms from whenever it was
armed" would make a nightly sweep drift to whatever time the host last restarted. `@hourly` →
`0 * * * *`, `@daily` → `0 0 * * *`, `@weekly` → `0 0 * * 0`, evaluated against
`SyncPlatform::utc_offset_minutes` — the crate's only zone authority. A fixed offset read at
evaluation time means no DST arithmetic exists to get wrong; the accepted cost is that a schedule
crossing a DST boundary fires at the new offset's wall-clock time, which is the behaviour a fixed
offset can honestly promise.

**The one-minute floor lands in two places.** `every <n><unit>` is refused below 60 000 ms, for the
reason `MIN_POLL_INTERVAL_MS` exists. A 5-field cron's finest resolution *is* one minute, so it
satisfies the floor by construction — stated in the parser's doc so nobody "fixes" it later.

**Refuse, do not clamp.** `tasks.schedule` is a brand-new field, so no stored row can carry a
legacy zero-by-omission: every out-of-range value is one a person typed and deserves to be told
about. That is the same reasoning `release_ttl_ms` records for diverging from the clamped
`MIN_POLL_INTERVAL_MS` next door.

**The claim is the arbiter, and `SQLITE_BUSY` means "not mine".** This crate sets no
`busy_timeout`, so on a real Linux box the daemon and the app contend immediately. A `BUSY`/`LOCKED`
primary code on the claiming `UPDATE` is mapped to `Ok(None)` — somebody else is writing this row,
which is precisely "I do not hold the lease" — and every other error propagates. The insert of the
`task_runs` row shares the claim's `unchecked_transaction`, so a crash between them cannot leave a
lease with no run.

```rust
// The pure gate. No `self`, no clock, no database.
pub fn decide(state: &TaskState, schedule: Option<&TaskSchedule>, now_ms: i64) -> Action {
    if !state.enabled || state.mode != TaskMode::Scheduled { return Action::None; }
    if schedule.is_none() { return Action::None; }
    if state.lease_until_ms.is_some_and(|until| now_ms < until) { return Action::None; }
    match state.next_due_ms {
        None => Action::Arm,                       // first sight arms, never runs
        Some(at) if now_ms >= at => Action::Run,
        Some(_) => Action::None,
    }
}
```

## Verification

**Commands:**
- `GIT_CONFIG_GLOBAL=/dev/null GIT_AUTHOR_NAME=keeper GIT_AUTHOR_EMAIL=dev@keeper.local GIT_COMMITTER_NAME=keeper GIT_COMMITTER_EMAIL=dev@keeper.local cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-core -p keeper-syncd` -- expected: 0 failed, total at or above the 3607 baseline.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` -- expected: clean.
- `cargo fmt --manifest-path src-tauri/Cargo.toml` -- expected: no diff afterwards.
- `bun run lint && bun run typecheck && bun run test` -- expected: unchanged from baseline (this wave touches no frontend file).

**Manual checks (if no CLI):**
- Each guard is mutated away in turn and the owning test must fail; the restore is verified by
  reading `git diff`, not by memory.

## Auto Run Result

Status: done

**Implemented.** Stories 57.1 and 57.2 together: `tasks` and `task_runs` in the existing
`sync.db` (AD-135), a keeper-owned schedule dialect refused at the write door, a pure
`decide(state, schedule, now_ms) -> Action` evaluated as a due-gate on the engine's existing
supervisor tick, and a `running_host` + `lease_until_ms` lease claimed by one conditional `UPDATE`
(AD-136). No new dependency, no new thread, no new interval, nothing in the `keeper` shell crate.

**Files changed**
- `src-tauri/crates/keeper-sync/src/tasks.rs` — new. The vocabulary (`TaskKind`, `TaskMode`,
  `TaskOutcome`), the dialect (`TaskSchedule::parse`, `next_due_after`, `CronSpec`), the pure
  `decide`, and the floor and ceiling.
- `src-tauri/crates/keeper-sync/src/db.rs` — the two tables and one index in `migrate`; `TaskRow`,
  `TaskRunRow`, `TaskListing`, `UnknownTask`, `TaskRunClose`; `upsert_task`, `list_tasks`,
  `get_task`, `arm_task`, `claim_task`, `finish_task_run`, `release_host_leases`, `task_runs`,
  `delete_task`; `delete_profile` now takes a folder's tasks.
- `src-tauri/crates/keeper-sync/src/engine.rs` — `TASK_LEASE_MS`, `TASK_RETRY_MS`, `TaskTrigger`;
  `run_due_tasks` on the existing tick; `arm_task_window`, `claim_and_run`, `next_task_window`,
  `perform_task`, `perform_sync_task`, `task_host`, `release_task_leases`; the public door
  `save_task`, `tasks`, `task_history`, `forget_task`, `run_task_now`.
- `src-tauri/crates/keeper-sync/src/lib.rs` — one line declaring the module.

**Verification**
- `cargo test -p keeper-sync -p keeper-core -p keeper-syncd`: **3667 passed, 0 failed** (baseline
  3607; +60 tests).
- `cargo clippy -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings`: clean.
- `cargo fmt`: applied, `--check` clean.
- `bun run typecheck` clean; `bun run lint` 4 warnings + 1 info; `bun run test` 297 files /
  4938 tests — all three exactly at baseline. No frontend file was touched.
- **Mutation proof:** 45 guards were mutated away one at a time, the owning test run, and the file
  restored and byte-compared against a pre-mutation snapshot. 44 killed their owning test. The one
  survivor was informative rather than a gap: it proved that the `busy_timeout` call added during
  the review was redundant, because rusqlite's own default already arms five seconds — so the call
  was removed and the fact pinned by a test instead.

**Residual risks**
- One deliberately unfalsifiable guard: the history trim's `finished_ms IS NOT NULL`. The insert
  ordering already makes the newest row safe, so the predicate protects nothing reachable through
  the public API. It is kept because it is the correct predicate and free, and its comment says so.
- The lease is not renewed. A run that outlives `TASK_LEASE_MS` is reclaimable, and the overrun
  case is now safe (the late finish cannot free the new holder's lease) but still means two hosts
  can be in one working tree if a single sync pass exceeds an hour. Deferred with a stated shape.
- A long host-wide run holds the supervisor tick and therefore delays shutdown. This is the shape
  `tick_profile` already has, so it is pre-existing rather than introduced; deferred.
