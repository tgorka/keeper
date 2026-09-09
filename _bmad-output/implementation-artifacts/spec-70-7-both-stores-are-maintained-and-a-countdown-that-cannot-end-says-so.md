---
title: 'Story 70.7: both stores are maintained, and a countdown that cannot end says so'
type: 'feature'
created: '2026-09-09'
status: 'done'
baseline_revision: 'bba8412'
final_revision: '766a11f'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
---

binds: FR-527, FR-528 → AD-234, AD-235; AD-48 (absence is not deletion — the `.git`-missing arm is left alone), AD-125 (`Unknown` refuses), AD-41 (the shim's verbs — `gc` finally has a caller)
findings: F-engine-13, F-PULLPUSH-9, F-db-4, F-db-5, F-db-7, F-db-8, F-db-11, F-VF-1, F-VF-2, F-VF-3, F-VF-4, F-VF-7

<intent-contract>

## Intent

**Problem:** Nothing maintains either store. `GitCli::gc` has no production caller; gc on hesperia is a launchd cron the owner wrote (403 loose objects, 6 packs). `materialized` holds 89 289 rows, is never pruned and survives `delete_profile` — which is six auto-commit statements, not a transaction. `enqueue_unique` dedups on an unindexed TEXT payload from a per-object loop (`O(n²)` to queue a backlog), and three docstrings claim indexes that do not exist. The log has no rotation and no per-target filter: 1.9 M of hesperia's 4.35 M lines are two gitoxide targets, written per path per lookup. And on the Mac a materialized row counts down to a release the platform cannot perform — `probe_open_file_state` is constant `Unknown` off Linux, `release_resolved` refuses `OpenUnknown` **after** hashing up to 1 GiB and calling the server, `release_schedule` has no platform term, and a candidate whose path left the index is re-selected every pass, paying a repository open and a full index parse each time to be told the path is gone.

**Approach:**
1. `TaskKind::Gc` (tag `gc`), keeper's own verb `GitCli::gc` behind the profile reservation and the walk claim (the quiet window: no unit running, no walk in flight). Seeded once per desktop profile (`meta` marker `gc_task_seeded:<id>`) as `gc-<id>`, `every 7d`, `run_now` — on `Engine::open` for the profiles already there and on `upsert_profile` for the ones that arrive later; never on a `Gix` engine, which refuses the verb with the host sentence. `detail` records loose objects before and after.
2. `db::age_out_materialized(conn, profile_id, older_than_ms)` deletes rows whose `released_at_ms` is older than `MATERIALIZED_RETENTION_MS` (90 d), called from the success edge; `db::delete_profile` deletes `materialized` rows and runs as one `unchecked_transaction`.
3. `journal_dedup (profile_id, payload)` and `journal_kind (profile_id, kind, state)` in `migrate`; the three docstrings say what is true.
4. `keeper_sync::logfile`: `RotatingFile` (one open handle, rotates at `LOG_ROTATE_BYTES` = 64 MiB to `.1`, two generations) and `default_filter(level)` = `<level>,gix_attributes=error,gix_worktree_state=error,gix_dir=warn`. Both subscribers use them; `RUST_LOG` still wins.
5. `ReleaseSchedule::Held { platform }` (word `Held`, sentence naming the platform) when `open_file_state` cannot answer, probed once per `release_schedules` call against the profile root; added to `FILES_RELEASE_REFUSED_HOLDS`.
6. `release_resolved` takes the cheap probe immediately after `release_path_gate` and refuses `OpenUnknown` there, keeping the final TOCTOU check; the sweep retracts on `NotTracked` from the arm where the repository opened; `release_expired` reads `indexed_pointers` once per sweep and hands each candidate its `(pointer, blob)`.
7. `release_schedules` is wrapped in `spawn_blocking` at its IPC call site (the smaller change; the query is unchanged).

## Boundaries & Constraints

**Always:**
- A `gc` task takes the same reservation `tick_profile` takes and the same walk claim every full-tree walk takes; `Busy` when either is held.
- A phone (`GitEngine::Gix`) seeds no `gc` task and answers a hand-written one with the shim's own sentence, `Deferred` — listed, never run, and said (NFR-43's shape, the `Bot` arm's precedent).
- Ageing deletes only rows with `released_at_ms` set and older than the horizon; a live row is never touched, a pinned row is never touched (a pin keeps `released_at_ms` `NULL` by construction).
- `Held` is chosen before any per-row work: one probe, on the profile root, per call.
- `NotTracked` retraction is scoped to the arm where the index was read and lacks the path — never the `.git`-missing arm (AD-48).

**Never:** touch `tick`/`tick_profile`/`mark_synced`'s structure, the `gates` field, `WalkPolicy`, `StabilityGate`, `git/*`; reformat a file.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Seed on open | two profiles, no tasks, Binary engine | two `gc-<id>` rows, `every 7d`, scheduled, `run_now` | — |
| Seed is one-shot | `gc-<id>` deleted, engine re-opened | not re-created (marker) | — |
| Phone | Gix engine, hand-written `gc` row | `Deferred`, the phone sentence | — |
| Repack | fixture with loose objects | loose count drops, `task_runs` row with before/after | — |
| Busy | reservation held | `Busy`, retried within `TASK_RETRY_MS` | — |
| Age out | rows released 91 d and 89 d ago | 91 d gone, 89 d kept | — |
| Delete profile | ledger rows present | none left; failure mid-delete leaves the profile row | rolled back |
| Dedup index | `EXPLAIN QUERY PLAN` of `enqueue_unique`'s `SELECT` | names `journal_dedup` | — |
| Rotate | 64 MiB written | `keeper.log.1` holds the old bytes, `.2` deleted | best effort |
| Filter | `gix_attributes` WARN | not enabled; keeper's own INFO is | — |
| Held | platform answers `Unknown` | every row `Held`, no hash, no batch | — |
| Cheap refusal | `Unknown` platform, materialized row | `OpenUnknown` before `content_oid`/`remote_serves` | — |
| Left the index | candidate path removed from index | row retracted after one sweep | — |

</intent-contract>

## Code Map

| Path | Change |
|---|---|
| `src-tauri/crates/keeper-sync/src/tasks.rs` | `TaskKind::Gc`, tag `gc`, vocabulary doc |
| `src-tauri/crates/keeper-sync/src/db.rs` | `journal_dedup`, `journal_kind`; `seed_gc_task`; `age_out_materialized`; `delete_profile` transactional + `materialized`; three docstrings |
| `src-tauri/crates/keeper-sync/src/engine.rs` | `perform_gc_task`, `loose_object_count`, seeding in `open_with_engine`/`upsert_profile`; `age_out_materialized`; `ReleaseSchedule::Held`, `RELEASE_HELD_SENTENCE`, `release_schedules` probe; `release_resolved` early probe + optional committed pointer; `release_expired` one open per sweep + `NotTracked` retraction |
| `src-tauri/crates/keeper-sync/src/logfile.rs` | new: `RotatingFile`, `LOG_ROTATE_BYTES`, `default_filter`, `LOG_TARGET_DIRECTIVES` |
| `src-tauri/crates/keeper-syncd/src/commands.rs` | `TaskKindArg::Gc` |
| `src-tauri/crates/keeper-syncd/src/main.rs` | file layer on `RotatingFile`; default filter |
| `src-tauri/crates/keeper/src/debug_log.rs` | `GatedWriter` writes through one `RotatingFile`; default filter |
| `src-tauri/crates/keeper/src/sync_ipc.rs` | `release_schedules` in `spawn_blocking` |
| `src/lib/stores/sync.ts`, `src/components/sync/task-form.tsx` | `gc` offered with its sentence |
| `src/components/layout/files-pane.tsx` (+ test) | `"Held"` in `FILES_RELEASE_REFUSED_HOLDS` |
| `docs/sync.md` §14 | the `gc` row the syncd doc guard demands |

## Tasks & Acceptance

- [x] `TaskKind::Gc` + default task + CLI + form — `every_desktop_folder_is_seeded_a_weekly_gc_task_once`, `a_gc_task_repacks_the_folder_and_records_the_loose_count`, `a_phone_seeds_no_gc_task_and_refuses_one_with_the_host_sentence`, syncd `a_folder_lists_its_seeded_gc_task_and_the_cli_accepts_the_kind`, `tasks.rs` round-trip and the form/`from_stored` coupling guard, `task-form.test.tsx` (offers `gc`, has a sentence).
- [x] `age_out_materialized` + transactional `delete_profile` — `db::tests::ageing_forgets_rows_released_past_the_horizon_and_no_others`, `deleting_a_profile_takes_its_ledger_and_is_all_or_nothing` (trigger-injected failure), `release_sweep::a_success_edge_forgets_rows_released_more_than_ninety_days_ago` (real `sync_once`).
- [x] journal indexes + docstrings — `the_journal_dedup_and_kind_indexes_serve_their_statements` (`EXPLAIN QUERY PLAN`); the three docstrings now say `journal_kind` / profile-prefixed scan.
- [x] `logfile` module + both subscribers — `a_log_past_the_boundary_rotates_and_the_third_generation_is_deleted`, `an_existing_file_is_measured_on_open`, `a_removed_directory_is_recreated_on_the_next_write`, `the_default_filter_silences_gix_attributes_warnings_and_keeps_keepers_info` (real `EnvFilter`).
- [x] `ReleaseSchedule::Held` + surface — `release_schedule_pairs_an_instant_with_words_exactly_one_way` (eight variants), `release_sweep::on_a_machine_that_cannot_see_open_files_every_row_is_held_and_names_the_platform`, `files-pane.test.tsx` withholds Release on `Held`.
- [x] `release_resolved` early probe, `NotTracked` retraction, one open per sweep — `a_machine_that_cannot_tell_refuses_before_hashing_or_asking_the_remote` (order proof: same-length edit answers `OpenUnknown`, not `Modified`), `a_candidate_whose_path_left_the_index_is_retracted_after_one_sweep`.
- [x] `release_schedules` off the runtime at the IPC site — `sync_ipc.rs` `spawn_blocking` (the smaller change; the query is unchanged). Shell crate: not compiled here.

**Acceptance Criteria:** every item of the epic's 70.7 list has a test named above; all **Met** on Linux. The ⌘8 row: the shell's task list is wire-driven (`TaskVm.kind` is a `String`) and the form's `TASK_KINDS` now offers `gc` — the tasks pane needs no VM change.

## Design Notes

**Why the seed is once by marker and not "ensure on every open".** A deleted row has two meanings after seeding — never offered, and forgotten by an operator — and only a `meta` marker keeps them apart; the same shape as `ensure_prune_default`. Seeding also runs from `upsert_profile`, because `keeper-syncd` writes its `config.toml` folders through that door after `open`, and an invariant that waited for a restart is a migration.

**Why `gc` takes the walk claim as well as the reservation.** `git gc` rewrites packs under a gix that may be mid-`status`; the reservation fences the sync pass, `claim_walk` fences the poll's walk. Both claims are the existing ones — nothing new is locked — and both refused answer `Busy`, which `next_task_window` retries within a minute rather than next week.

**Why `Held`'s sentence names Linux.** `release_resolved` refuses `OpenUnknown` on every door, so "release it yourself" would be a lie on the platform that draws the word; what is true is that a keeper on Linux syncing the same folder releases it.

**Why the sweep builds a map before the loop.** `gix::Repository` is not `Send` and the loop awaits per candidate; the open and the one index parse happen up front, each candidate's `(pointer, blob)` is looked up from the cached index, and `None` from a map that was built is `NotTracked`-retract while an absent `.git` never reaches the map (AD-48).

## Verification

- `cargo check -p keeper-sync -p keeper-syncd --all-targets` → clean.
- `cargo test -p keeper-sync --lib -- db::tests tasks::tests logfile::` → 145 passed.
- `cargo test -p keeper-sync --lib -- gc_task every_desktop_folder a_phone_seeds release_schedule_pairs` → 4 passed.
- `cargo test -p keeper-sync --test release_sweep --test dehydrate_entry --test materialize_entry --test virtual_arrival --test lfs_listing --test virtual_state_is_not_a_fault` → 28 + 4 + 10 + 38 + 11 + 20 passed.
- `cargo test -p keeper-syncd` → 140 + 6 + 12 passed.
- `bunx vitest run src/components/layout/files-pane.test.tsx src/components/sync/task-form.test.tsx` → 202 passed; `bun run typecheck` clean.
- One pre-existing test adjusted: `release_sweep::with_no_task_rows_at_all_a_successful_sync_releases_what_it_always_did` now expects the one seeded `gc` row and asserts no *release* row was invented.

| Mutation | Test that failed |
|---|---|
| (1a) `perform_gc_task` never calls `git.gc` | `a_gc_task_repacks_the_folder_and_records_the_loose_count` |
| (1b) `seed_gc_tasks()` removed from `open_with_engine` | `every_desktop_folder_is_seeded_a_weekly_gc_task_once` |
| (2) `age_out_materialized` predicate `< 0` | `ageing_forgets_rows_released_past_the_horizon_and_no_others`, `a_success_edge_forgets_rows_released_more_than_ninety_days_ago` |
| (3) `journal_dedup` on `(state, payload)` | `the_journal_dedup_and_kind_indexes_serve_their_statements` |
| (4) generation shift loop removed in `rotate` | `a_log_past_the_boundary_rotates_and_the_third_generation_is_deleted` |
| (5a) `release_schedules` probe keyed on `Open` | `on_a_machine_that_cannot_see_open_files_every_row_is_held_and_names_the_platform` |
| (5b) `"Held"` removed from `FILES_RELEASE_REFUSED_HOLDS` | `withholds Release exactly where Rust's word says the request cannot succeed` |
| (6a) early `OpenUnknown` arm falls through | `a_machine_that_cannot_tell_refuses_before_hashing_or_asking_the_remote` (answered `Modified`) |
| (6b) `NotTracked` dropped from the retraction alternation | `a_candidate_whose_path_left_the_index_is_retracted_after_one_sweep` |

Each mutation one line, reverted, and the line re-read afterwards. Note on (4): removing only the `remove_file` of `.2` does **not** fail on Linux, because `rename` over an existing name replaces it; the delete is load-bearing on Windows, and the doc says so.

**Not verified here, and why.** The `keeper` shell crate (`debug_log.rs`, `sync_ipc.rs`) does not compile on Linux — the edits are thin (a `LazyLock<Mutex<RotatingFile>>`, `default_filter("info")`, a `spawn_blocking` wrapper, `.into_owned()`) and the rotation/filter logic they call is tested in `keeper-sync`. No `src/lib/bindings/*.ts` exist in this checkout, so nothing is owed on hesperia for bindings; `FilesReleaseVm.hold` is a `String` on the wire. The hesperia measurement of the log filter's effect is 70.8's install step.
