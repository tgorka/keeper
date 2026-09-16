---
status: in-progress
baseline_revision: a8eb9c2
final_revision: ''
---

# Story 72.7 — The task row carries every kind

<intent-contract>
## Problem
Bot task saves discard their three payload columns; copy jobs cannot be scheduled or bounded by modification time.
## Approach
Persist all eight kind-specific fields, expose them through generated task bindings, add the copy kind and dispatch the existing verified-copy job on a blocking worker.
## Always
Keep absolute picker paths verbatim. Date windows include the lower bound and exclude the upper bound. Name excluded files in the report and count them in progress. Recreate directories regardless of dates. Preserve unknown-kind forward compatibility.
## Block If
Refuse Bot without bot or prompt, Copy without source or destination, missing sources and destinations inside sources. With active bounds, unreadable modification times are explicitly skipped rather than admitted.
## Never
Create profiles, journal work, relationships, convergence, deletion propagation, or a git-sync date filter. Never widen bot grants.
## I/O and edge-case matrix
| Input | Observable result |
| --- | --- |
| Bot insert then update with bot/prompt/model | All three columns survive both writes |
| Bot missing bot or prompt; Copy missing either path | Actionable Config refusal; no row written |
| Copy fixture tree | Verified bytes, file counts, destination log, no profile/journal additions |
| Copy window between fixture mtimes | New file copied, old file named skipped |
| File exactly at lower / upper bound | Included / skipped respectively |
| Directory outside window | Recreated |
| Unreadable mtime with active bounds | Named skipped entry |
| Unknown stored kind | from_stored returns None |
</intent-contract>

## Code Map
- `src-tauri/crates/keeper-sync/src/db.rs:500`: migrations; `:3075`: columns/row; `:3275`: decoding; `:3444`: write door.
- `src-tauri/crates/keeper-sync/src/tasks.rs:171`: kind vocabulary and round-trip/form guards.
- `src-tauri/crates/keeper-sync/src/engine.rs:4029`: dispatch and copy runner.
- `src-tauri/crates/keeper-sync/src/copy.rs:188`: options; `:876`: plan; `:967`: classification.
- `src-tauri/crates/keeper-core/src/tasks.rs:277`: TaskVm; `:393`: TaskSaveReq; `:1022`: sweep comment.
- `src-tauri/crates/keeper-syncd/src/commands.rs:891`: CLI vocabulary; `:4123`: task row.
- `docs/sync.md:2143`: task kind documentation.

## Tasks & Acceptance
**Acceptance:** a db test writes a bot row and reads all three columns back (today's INSERT drops them — this test fails before the change); a db test refuses a `Bot` row missing a bot or a prompt and a `Copy` row missing either path; an engine test copies a fixture tree through a `Copy` task and reports its bytes, and a second run with `modified_after_ms` between the two fixtures copies only the newer file and names the skipped one; the vocabulary guards pass with `bot` and `copy` offered (`NEVER_OFFERED` empty), the CLI-vs-`docs/sync.md` guard passes, and the round-trip test enumerates both new kinds.

## Design Notes
AD-244, AD-245 and AD-C1. No date bounds means existing copy behavior. Active bounds require a readable mtime, otherwise the plan explicitly skips the file. Full per-file results remain in the destination copy log; task history stores a one-line summary. Sibling lanes own shell callsites and frontend fixtures.

AD-233: copy uses the engine's existing `Engine::blocking` fence. On the production multi-thread runtime, `block_in_place` hands the current worker's queue to a replacement thread; it does not retain the only scheduler worker while copying. **Narrowed after review, and the difference matters:** that claim is about the tokio *runtime*, not about keeper's own scheduling. `Engine::tick` awaits `run_due_tasks` before it fans the per-folder passes out (`engine.rs:3584-3589`), so while a copy runs, no folder on this host takes a sync pass and no other due task runs. Every kind shares that shape, but the others are bounded by the folder they reserved and a copy walks an arbitrary external tree — recorded as deferred work rather than fixed here, because moving the copy arm off the serial pre-pass is an AD-233 scheduling change and not a change about a task kind. The same entry records that nothing renews the one-hour lease while a run is in flight. This preserves the borrowed engine required by `ContentSource::materialize` (live database plus `materialize_entry`), without changing every task-dispatch caller to own an Arc. The focused proof runs on a runtime with one worker: from a spawned worker task, the fence waits synchronously for a second engine's queued async tick to finish. Without the fence that worker cannot run the pending tick. Current-thread test runtimes retain the existing inline fallback; production hosts both use multi-thread runtimes. A blocking copy still occupies its own task execution until it finishes, like every other awaited task run; this proof concerns scheduler starvation, not parallelizing the task dispatcher.

## Verification
Pending targeted task, copy, core tasks and CLI vocabulary tests. Bot-column regression runs before persistence edits. Date-window mutation must fail, then exact source restoration must pass. No formatters, git operations, or repository-wide gates in this lane.
