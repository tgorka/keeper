---
title: 'Story 70.6: one folder does not stall another, and a pass does its own work'
type: 'feature'
created: '2026-09-09'
status: 'review'
baseline_revision: 'bba8412'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-70-a-pass-that-costs-what-changed-and-a-folder-that-says-when-it-is-cut-off.md'
  - '{project-root}/_bmad-output/planning-artifacts/review-sync-2026-09-08/lanes/engine.md'
---

binds: FR-526 → AD-233
lane findings: F-engine-3, F-engine-4 (a), F-engine-5 (remainder), F-engine-7 (a), F-engine-8, F-engine-9, F-engine-10, F-engine-11, F-engine-12, F-scan-11, F-db-13
depends on: wave A (`bba8412`) — 70.1's `WatchWake`/`watch_wake_pending`/`untracked_sweep`, 70.4's `lfs_moved`/`next_prune_ms`, 70.5's `forget_settle_windows`; 70.7's `age_out_materialized` (one call placed in `mark_synced` by agreement)

<intent-contract>

## Intent

**Problem:** `tick` iterated profiles serially and every expensive thing a profile does — the status walk (p90 5.3 s, max 60.8 s on hesperia), the first clone, the prune plan over 155 626 tracked paths — ran inline on the async worker (F-engine-3), so `neuradrive` and `tgdrive-light` got no tick while `tgdrive` walked or waited out a connect timeout. Every scan enqueued a `Pull` (F-engine-4), so three profiles on a 15 s poll fetched from one Forgejo every ~5 s for a folder that changes a few times a day. `sync_once` walked the tree up to four times per pass (F-engine-8). `wake_now` reset the hourly footprint sweep, a second 155 k `lstat` pass, on every notes commit (F-engine-9, F-scan-11, F-db-13). One process-wide `gates` mutex was held across `open_repo`, attribute-stack builds and `platform.notify()` (F-engine-10), so one folder's classification blocked every other folder's settle check and the tray's `settling` count. A quit signalled the supervisor but set `interrupt` only after the loop exited, so a 2 GB upload was waited out (F-engine-11). The watcher's 15-minute root rescan forced a full dirwalk through `untracked_appeared`, beside the untracked-sweep clock that forces the same walk (F-engine-12).

**Approach:**
- `Engine::run` and `tick` take `self: &Arc<Self>`; `tick` runs the serial preamble (finished assertions, watcher retirement, `run_due_tasks`) and then one `tokio::task::JoinSet` task per enabled profile (`tick_one`). The per-profile `busy` reservation already makes two passes over one folder impossible. The app already holds the engine in an `Arc`; `keeper-syncd`'s `run_supervisor` wraps once.
- Blocking work runs through `Engine::blocking`: `tokio::task::block_in_place` on the multi-thread runtime both hosts use (the borrow of `self` is kept, so `sync_once`, `tick_profile` and every test keep `&self`), the closure inline on a current-thread runtime, where `block_in_place` would panic and nothing else is waiting anyway. Fenced: `scan_and_enqueue`, both `commit_local` legs, `finish_first_checkout`, `prune_lfs_store`. The fetch keeps its `spawn_blocking`.
- `sync_once` walks once: `do_pull`/`do_push`/`reconcile_and_retry_push` take a `TreeState` — `Committed` when this pass's own `commit_local` already walked, `Unknown` for a journaled unit — and the closing drain no longer scans. The journaled `Pull` skips its pre-fetch commit when `local_work_may_be_pending` is false: watcher live, no wake due, no unspent path list, a gate holding nothing.
- The remote poll has its own clock: `next_remote_poll_ms`, `REMOTE_POLL_MS = 300 000`, absent means now (first sight pulls); a push owed in the same pass or a pending wake pulls at once; every enqueue re-arms.
- `scan_is_due` paces at `LIVE_WATCH_BACKSTOP_MS = 300 000` (or the profile's interval, whichever is longer) while the watcher is `Live`, and at `effective_poll_interval_ms` otherwise — the cadence `warn_watch_degraded` already names.
- `wake_now` clears `next_scan_ms` and the remote-poll deadline only; `rescan` forgets the sweep, release, cursor and footprint memo by name.
- `report_blobs_over_threshold` memoises `(HEAD, threshold) → (files, bytes)` per profile and re-emits from the memo, with no `lstat`, when neither moved (`EngineCounters::footprint_sweeps`). The anomaly is reported on the engine side of the await.
- `gates` holds `Arc<Mutex<StabilityGate>>` per profile (`gate_for`/`existing_gate`, `GateGuard`); the map lock is held only to fetch the `Arc`. `fold_watch_events` asks the index after the guard is dropped; `collect_stable_changes` takes verdicts under the guard and classifies (`truncated_media`, `is_false_modification`), warns and `lstat`s after it. A `#[cfg(test)]` thread-local depth counter panics in `open_repo` while any gate guard is held.
- `request_stop` sets `interrupt` before the hosts signal; `run` sets it when the signal lands mid-tick and lets the tick finish; `drain` checks it before each unit.
- A root/rescan event from the watcher marks the untracked sweep due (`untracked_sweep.remove`) instead of `untracked_appeared`; the next walk is full and re-stamps the one clock.

## Boundaries & Constraints

**Always:**
- `tick_profile`, `sync_once`, `drain`, `commit_local` keep `&self`; only `run`/`tick` need the `Arc`.
- `run_due_tasks` and `drain_finished_assertions` stay serial and before the fan-out.
- A `Pull` is still enqueued at once for a wake, a push owed, `wake_now`, and `sync_once`'s own pull leg.
- No `open_repo`, index read, attribute-stack build or `platform.notify()` under a gate guard.
- `warn_watch_degraded`'s sentence stays true: the degraded cadence is `effective_poll_interval_ms`.

**Never:** edit `tasks.rs`, `db.rs`, `stability.rs`'s API, `WalkPolicy`, `git/*`, `lfs/stage.rs`; cancel an in-flight unit on shutdown (the flag is checked between units; the fetch reads it itself); reformat.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Two profiles, one slow | the slow profile's walk waits on the quick one's | the quick walk runs while the slow one is open; both commit on the tick | — |
| Idle scan | no push owed, no wake, clock not passed | no `Pull` row | — |
| Push owed | commit landed this pass | `Pull` row too, clock re-armed | — |
| Wake pending | watcher named a path / `wake_now` | `Pull` row at once | — |
| Live watcher, `pollIntervalMs=15000` | 200 ticks × 15 s, no wakes | 9–11 walks | — |
| Degraded watcher, same profile | 20 ticks × 15 s | ≥ 19 walks | — |
| `wake_now` | sweep, release windows and cursor armed | untouched; scan and remote poll cleared | — |
| Footprint, HEAD unchanged | second sweep | anomaly re-emitted, `footprint_sweeps` unchanged | — |
| Footprint, HEAD or threshold moved | a commit / a new threshold | one more sweep | — |
| Gate guard held | `open_repo` entered | test panic (hook) | — |
| `sync_once` | fixture with one settled edit, bare remote | `status_walks` +1, pulled, pushed, published | — |
| Shutdown mid-drain | `interrupt` set inside the first unit's walk, two units claimed | the second unit never starts; `finalize` returns it to the queue | — |
| Root rescan event | watcher reports the root | untracked sweep due; `untracked_appeared` not set; next commit walk `find_untracked` | — |

</intent-contract>

## Code Map

| Path | Change |
|---|---|
| `src-tauri/crates/keeper-sync/src/engine.rs` | `run`/`tick` on `Arc`; `tick_one`; `blocking`; `TreeState`; `REMOTE_POLL_MS`, `LIVE_WATCH_BACKSTOP_MS`, `next_remote_poll_ms`, `remote_poll_due`/`arm_remote_poll`; `scan_is_due` backstop, `watch_is_live`; `local_work_may_be_pending`; `wake_now`/`rescan`; `FootprintMemo`, `footprint_memo`, `footprint_sweeps`; `gates` as `Arc<Mutex<_>>`, `GateGuard`, `gate_depth`, `gate_for`, `existing_gate`; `fold_watch_events`/`collect_stable_changes` restructured; `request_stop`; `drain`'s interrupt check; the root-event arm; `mark_synced` calls 70.7's `age_out_materialized`; tests |
| `src-tauri/crates/keeper-sync/src/profile/mod.rs` | `effective_poll_interval_ms` doc: what it paces now |
| `src-tauri/crates/keeper-syncd/src/commands.rs` | `run_supervisor`: `Arc::new`, `request_stop` before the signal (agreed with Maintain) |
| `src-tauri/crates/keeper/src/sync.rs` | `stop_supervisor`: `request_stop` before the signal (shell; not compiled here) |

## Tasks & Acceptance

- [x] Concurrent `tick_profile` — `a_slow_walk_in_one_folder_does_not_delay_anothers_commit`.
- [x] Blocking work fenced; `sync_once` walks once — `sync_once_walks_the_tree_exactly_once`.
- [x] Remote poll clock — `a_pull_is_queued_once_per_remote_poll_when_idle_and_at_once_when_a_push_is`.
- [x] Backstop — `a_live_watcher_backstops_at_five_minutes_whatever_the_poll_interval_says`; `ten_minutes_of_excluded_write_bursts_cost_not_one_walk_beyond_the_paced_ones` re-authored (40 → 2 paced walks in 600 s).
- [x] `wake_now` — `wake_now_leaves_the_sweep_and_release_windows_alone`.
- [x] Footprint memo — `the_footprint_sweep_re_emits_from_the_memo_while_head_stands`.
- [x] Gate locks — the panic hook across the module; `no_repository_is_opened_under_a_gate_guard`.
- [x] Shutdown — `a_stop_request_returns_the_drain_within_one_unit`.
- [x] Root rescan — `the_watchers_root_rescan_rides_the_untracked_sweep_clock`.

## Design Notes

**Why `block_in_place` and not `spawn_blocking` for the walk.** `spawn_blocking` needs `'static`, and `commit_local`, `scan_and_enqueue` and `prune_lfs_store` read the whole engine through `&self`. Re-plumbing the engine behind an `Arc` would have changed `sync_once`'s call shape in both hosts and ~80 test sites; `block_in_place` keeps the borrow, and on the multi-thread runtime both hosts run it gives the worker's queue away for the duration, which is the property F-engine-3 asks for. The one place `'static` is unavoidable — a task per profile — is `tick`, so only `run`/`tick` take `&Arc<Self>`.

**Why the release cursor is no longer reset by `wake_now`.** Its comment tied the reset to the window drop ("across a re-add"); with the window kept, a reset on every wake would starve paths past `RELEASE_BUDGET_OBJECTS` on any folder that wakes oftener than the hourly look — the exact defect the cursor was added for. `rescan` resets all of it by name.

**Why the `LfsRouting` hoist was not done.** `LfsRouting` is private to `lfs/stage.rs` (70.4's, frozen). Moving `is_false_modification` out from under the gate guard removes the contention F-engine-10 measured; the per-path stack build remains, paid by the one folder that has modified files. Hoisting needs a public one-repo-many-paths shape in `stage.rs` — flagged to the coordinator.

**Why the task panic is re-raised only under test.** In production a profile's task panicking is logged and the next tick retries, which is strictly better than the serial loop's supervisor death. Under test the `open_repo` gate-depth hook has to fail the test that reached it, and a `JoinError` would have swallowed it.

## Verification

- `cargo check -p keeper-sync -p keeper-syncd --all-targets` clean.
- `cargo test -p keeper-sync --lib -- engine::` → 232 passed. `cargo test -p keeper-sync --lib` → 1218 passed. Integration: `--test conflict_matrix --test dehydrate_entry --test lfs_filter_process --test lfs_filter_shadowing --test lfs_listing --test materialize_entry --test phone_note_save --test release_sweep --test virtual_arrival --test virtual_state_is_not_a_fault` → all green.

| Mutation (one line, exact-string replace, reverted and re-run green) | Test that failed |
|---|---|
| (1) `tick` joins each profile task right after spawning it (serial) | `a_slow_walk_in_one_folder_does_not_delay_anothers_commit` |
| (3) `scan_and_enqueue` enqueues `Pull` on every scan | `a_pull_is_queued_once_per_remote_poll_when_idle_and_at_once_when_a_push_is` |
| (4) `interval.max(0)` instead of `interval.max(LIVE_WATCH_BACKSTOP_MS)` | `a_live_watcher_backstops_at_five_minutes_whatever_the_poll_interval_says` |
| (7) a gate guard held across the index open in `fold_watch_events` | `a_wake_naming_one_path_walks_one_entry_of_ten_thousand` ("a repository is being opened under a gate guard") |

**Not verified here, and why.** The `keeper` shell crate (`crates/keeper/src/sync.rs` `stop_supervisor`) does not compile on Linux — one call added, `engine.request_stop()` before the send. The hesperia number (`status walk finished` per hour on an idle day) is 70.8's install step. `block_in_place` on the tauri runtime was not exercised on macOS.
