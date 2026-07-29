---
title: 'Files sync when they land, not when a timer says so'
type: 'feature'
created: '2026-07-28'
status: 'review'
baseline_revision: '5c40a22'
---

<intent-contract>

## Intent

**Problem:** Four defects with one reported symptom — "waiting for writes to stop takes ages".

1. **No watcher is wired.** `watch::FolderWatcher` is written, tested and exported, and
   `FolderWatcher::start` is called only from `watch.rs`'s own tests. Nothing in `engine.rs`,
   `keeper/src/` or `keeper-syncd/src/` ever built one. So `note_close_write` never fired, the 1 s
   `CLOSE_WRITE_SETTLE_MS` path was unreachable dead code, and the only trigger was the paced scan
   at `poll_interval_ms` (15 s default). Worse: a settling path needs a **second** observation to
   clear and observations happen only inside a walk, so a file that landed waited **two** poll
   intervals — 30 s — not its 5 s settle window. The settle window was never the floor (AD-34-11).
2. **"Up to date" was a lie.** `status_line`'s final arm was guarded only by `status.pending > 0`,
   and `pending` counts *journal rows*. A folder with thousands of files inside their settle window
   has no journal rows, reports `pending = 0`, and printed "up to date" over itself for as long as
   the writing lasted. The true count was already computed (`held` in `collect_stable_changes`) and
   thrown into a `tracing::debug!` (AD-34-10).
3. **`Engine::pending` ignored tier 0.** It built its buckets straight from git status with no
   `ExcludeSet`, so a `.DS_Store` not in `.gitignore` was listed as pending forever while the commit
   path correctly ignored it — contradicting `exclude.rs`'s own contract, "an excluded path is
   invisible … never reported as pending" (AD-34-15).
4. **`db::save_file_state` was N+1 commits.** A full `DELETE` plus one `INSERT` per row with no
   enclosing transaction, under WAL + `synchronous=NORMAL`, run up to four times per sync pass and
   largest exactly when the tree is busiest.

**Approach:** The supervisor owns a `FolderWatcher` per enabled profile and reconciles that
ownership on every tick, so "every enabled profile is watched" is a property re-established
continuously rather than three call sites that must remember. A delivered event becomes a sticky
wake; a delivered close-write additionally reaches `StabilityGate::note_close_write`. A new
`StabilityGate::next_stable_ms` tells the supervisor the earliest instant a held path could clear,
which is what replaces "wait a whole poll interval for the second observation". The paced scan stays
untouched as the backstop. The `held` count becomes `SyncStatus.settling` and `SyncStatusVm.settling`
with its own `status_line` arms. `Engine::pending` filters through the same `ExcludeSet` the commit
path uses. `save_file_state` becomes one `unchecked_transaction`. `BUILTIN_EXCLUDES` gains five
toolchain-reserved directory names — not eight (see Design Notes).

## Boundaries & Constraints

**Always:** The paced scan (`scan_is_due`) stays exactly as it was and is evaluated **first and
unconditionally** on every tick, so an event-driven walk can never starve the backstop that covers
what a watcher cannot see. Watcher failure degrades to that backstop *loudly*: a sticky, user-visible
warning naming the effective cadence and the watcher's own reason, re-asserted every tick the watcher
is down. A watcher never outlives its profile — pause, remove, media-detach and a moved root each
drop it, and dropping joins both of its threads. Every drop happens with the `watchers` map
**unlocked**, because `remove_profile` runs on an IPC thread. Lock order is `watchers → watch_wake`,
`watchers → status`, `gates → status`, `gates → db`; no cycles. `git status` remains the authority on
what changed; the event stream only says *when to look*.

**Block If:** (none — every decision this story needed was already fixed by AD-34-10, AD-34-11 and
AD-34-15, or is recorded in Design Notes below.)

**Never:** Do not turn the settle window off, shorten it, or make debouncing decide when a file syncs
— the field report asked for that and the epic refuses it. Do not walk the tree faster than the 1 Hz
tick. Do not add a `notify::PollWatcher` fallback: it re-stats the tree every 30 s, which is *slower*
than this engine's own paced scan. Do not put a countdown or any changing number in the degraded
warning (`warn` compares text before it notifies; a moving number would notify once per second). Do
not let `clear_watch_warning` become a general-purpose warning eraser. Do not add `target`, `dist` or
`build` to tier 0. Do not touch `keeper/src/sync.rs` or `keeper-syncd/src/commands.rs`: both hosts
drive `Engine::run`, so putting watcher lifetimes anywhere but the engine would be the second
implementation of one policy that AD-52 exists to prevent.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| 500 files dropped in | watcher delivers a debounced batch | next tick (≤1 s) folds the events, sets the wake, walks; on Linux each close-write shortens the window to 1 s, so the commit starts in ~2 s | none |
| file inside its window | gate holds it, no new events | `settle_window_elapsed` reopens the walk at `next_stable_ms`, not at the next poll | none |
| watcher wake, journal busy | `drain_journal` claims work instead of walking | wake stays set; the next idle tick walks. Only `scan_and_enqueue` spends it | walk failure spends it too, so a broken repo is not re-walked per tick |
| profile paused | `enabled = false` | `tick()`'s `retain_watchers` drops the watcher within one tick and joins both threads | none |
| profile resumed | `enabled = true` | `ensure_watcher` arms exactly one; the map is keyed by id, so no thread leaks per cycle | none |
| profile removed | `remove_profile` | watcher and wake dropped in the same call, not a tick later | keychain failure still aborts removal first (AD-34-14) |
| removable media detached | `volume_ready` false | watcher dropped; re-armed by the tick after the volume returns | none |
| profile root moved | `local_path` changed | `watcher.root()` no longer matches, so the old watcher is replaced (and joined), never kept beside the new one | none |
| watcher cannot arm | missing root, inotify limit exhausted | `ProfileWatch::Failed` remembered with a 60 s re-arm window; sticky warning naming the cadence and the reason; exactly one notification per onset | falls back to the paced scan; retried after the window |
| warning wiped by a success | `clear_warning` ran | next tick re-asserts the text **without** notifying | none |
| unrelated warning present | e.g. "drive nearly full" | arming a watcher leaves it alone (prefix match) | none |
| supervisor stops | `run` returns | `stop_all_watchers` leaves none armed, so a later `start_supervisor` cannot arm a second set | none |
| folder mid-write | `pending = 0`, `settling = 2` | line: `<name> — 2 waiting for writes to stop` | none |
| both kinds of wait | `pending = 2`, `settling = 5000` | line: `<name> — 2 waiting to sync, 5000 waiting for writes to stop` | none |
| transfer in flight | phase active, `settling > 0` | the active-phase arm still wins; the count does not displace it | none |
| one-shot `syncd status` | never ticks | `seed_status` seeds `settling` from `file_state`, so the CLI cannot print "up to date" over held files | a `file_state` read failure degrades to 0 |
| excluded file, not gitignored | `.DS_Store`, `node_modules/**`, `*.part` | absent from the Pending list; excluded untracked entries are dropped **before** `expand_untracked`'s per-entry `lstat` | none |
| stale `file_state` row now excluded | pattern added after the row | filtered from the list; the next walk prunes the row | none |
| excluded-but-tracked file modified | committed under an older build | never staged and never listed — tier 0's contract applied to a path already in history | pre-existing; a *deletion* of it is still committed |
| `save_file_state` row fails | duplicate path violates the PK | whole replace rolls back; the previous state is intact, so no window silently restarts | error propagates |
| user folder named `build` | photo library, woodworking archive | syncs normally; tier 0 does not claim it | `.gitignore` remains the authority |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/watch.rs` -- unchanged behaviour; adds a `const _: fn()` assertion
  that `FolderWatcher: Send`, which the engine now depends on.
- `src-tauri/crates/keeper-sync/src/stability.rs` -- new `StabilityGate::next_stable_ms(now_ms)`,
  mirroring `verdict` term for term (window, mtime, ceiling, future-mtime escape hatch).
- `src-tauri/crates/keeper-sync/src/exclude.rs` -- `BUILTIN_EXCLUDES` gains `node_modules`,
  `__pycache__`, `.venv`, `.next`, `.cache`, each with a name rule and a `**/name/**` subtree rule,
  plus the recorded reasoning for the three names deliberately left out.
- `src-tauri/crates/keeper-sync/src/progress.rs` -- `SyncStatus.settling: u32` (+ `idle`), and three
  `status_line` arms replacing the single `pending > 0` arm before "up to date".
- `src-tauri/crates/keeper-sync/src/db.rs` -- `save_file_state` wrapped in one
  `unchecked_transaction`.
- `src-tauri/crates/keeper-sync/src/engine.rs` -- `WATCH_REARM_INTERVAL_MS`,
  `WATCH_DEGRADED_PREFIX`, `enum ProfileWatch`, `Engine::{watchers, watch_wake}`; new
  `scan_due`, `settle_window_elapsed`, `ensure_watcher`, `warn_watch_degraded`,
  `clear_watch_warning`, `fold_watch_events`, `note_watch_wake`, `watch_wake_pending`,
  `clear_watch_wake`, `drop_watcher`, `retain_watchers`, `stop_all_watchers`, `ensure_gate`;
  edits to `open`, `seed_status`, `remove_profile`, `run`, `tick`, `tick_profile`,
  `refresh_pending`, `collect_stable_changes`, `scan_and_enqueue`, `pending`.
- `src-tauri/crates/keeper/src/sync_ipc.rs` -- `SyncStatusVm.settling` + its `From` arm.
- `src/lib/ipc/gen/SyncStatusVm.ts` -- regenerated by hand to match ts-rs byte for byte (9 added
  lines, 0 removed; the trailing space after `settling: number,` is ts-rs's, not a typo).
- `src/components/layout/sync-pane.test.tsx`, `src/components/settings/sync-section.test.tsx`,
  `src/lib/stores/sync.test.ts` -- `settling: 0` added to the three `statusVm` fixtures.

## Tasks & Acceptance

**Execution:**
- [x] `stability.rs` -- add `next_stable_ms(now_ms) -> Option<i64>`. -- Gives the supervisor the
  earliest instant a held path could clear, so the second observation stops waiting for a poll.
- [x] `engine.rs` -- add `ProfileWatch`, `watchers`, `watch_wake`; arm in `tick_profile` after the
  volume gate and inside the reservation; reconcile in `tick`; drop in `remove_profile`, on media
  absence and in `run`. -- A watcher per enabled profile, and none that outlives one.
- [x] `engine.rs` -- `fold_watch_events` drains without blocking, sets the sticky wake and folds
  close-writes into the gate through the new `ensure_gate`. -- Makes the 1 s `CLOSE_WRITE_SETTLE_MS`
  path reachable for the first time.
- [x] `engine.rs` -- `scan_due` = paced (evaluated first, unconditionally) OR wake OR elapsed
  deadline; `scan_and_enqueue` spends the wake. -- Event-driven latency without starving the
  backstop, and no wake swallowed by a tick that drained work instead.
- [x] `engine.rs` -- `warn_watch_degraded` splits sticky text (every tick) from notification (onset
  only); `clear_watch_warning` retires only its own message. -- Loud degradation that neither goes
  silent nor spams.
- [x] `progress.rs` + `engine.rs` + `sync_ipc.rs` + `SyncStatusVm.ts` -- carry `held` into
  `SyncStatus.settling`, refresh it beside `pending`, seed it at open, surface it, and add the
  `status_line` arms. -- "Up to date" becomes a claim that is true.
- [x] `engine.rs` -- `pending` builds one `ExcludeSet` and filters the settling list, the three git
  buckets, and the untracked list *before* `expand_untracked` stats it. -- AD-34-15.
- [x] `exclude.rs` -- five new directory conventions, both forms each. -- The noise the report named,
  minus the three names that are ordinary English words.
- [x] `db.rs` -- one transaction around the delete-and-reinsert. -- Atomic, and N+1 commits become 1.
- [x] Tests -- `stability.rs` (6 for `next_stable_ms`), `exclude.rs` (2 new + 8 corpus rows),
  `progress.rs` (2), `db.rs` (2), `engine.rs` (6). -- Every acceptance criterion below that can be
  checked without a real event queue.

**Acceptance Criteria:**
- Given 500 files dropped into a synced folder, when the watcher delivers its batch, then the first
  commit starts within a few seconds rather than ~15 s. (Machine check — see Verification.)
- Given files inside their settle window, when the status line is read, then it says how many are
  waiting and never "up to date".
- Given an excluded file that is not in `.gitignore`, when the Pending list is read, then it does not
  appear.
- Given a watcher that cannot arm, then a sticky user-visible warning names the fallback cadence and
  the reason, exactly one notification is raised per onset, and the warning returns if a success
  wipes it.
- Given a profile that is paused, removed, unplugged or moved, then its watcher is dropped and both
  its threads joined; a resume arms exactly one.
- Given a failure partway through `save_file_state`, then the previous quiescence state is intact.

## Design Notes

**`target`, `dist` and `build` are deliberately NOT tier 0.** The story listed eight names; five
shipped. Tier 0 is unconditional and invisible — an excluded path is never staged, never queued,
never counted and never reported as pending — so a wrong entry is silent, permanent data loss from
the user's point of view, with no pending row to reveal it. Every existing entry in the corpus is
either a machine-generated name with a distinctive shape (`*.crdownload`, `~$*`, `.~lock.*#`) or a
reserved-looking OS metadata directory (`.DS_Store`, `.Spotlight-V100`). `node_modules`,
`__pycache__`, `.venv`, `.next` and `.cache` fit that shape: no human names a photo folder
`__pycache__`, and the last three are dotfiles besides. `target`, `dist` and `build` do not. They are
ordinary English words, they are not hidden, and their build-output meaning is *contextual* — only a
`target` beside a `Cargo.toml` is Rust's, only a `build` beside a `CMakeLists.txt` is CMake's — while
this tier matches names with one compiled glob set and never touches the filesystem, which is exactly
what makes it cheap. Meanwhile `.gitignore` already handles them: git honours it, `cargo new` and
every real JS/Python project already list those three in it, and the user can read and edit it. So
tier 0 would buy almost nothing for projects and cost a woodworking archive its `build/` folder. A
user who does want them gone says so per profile through `SyncProfile.excludes`, which is visible and
reversible. The reasoning is recorded in the corpus itself and pinned by
`ordinary_english_build_directory_names_are_not_tier_zero`, so the next person to reach for them
reads the argument first. (Corroboration: `extra_profile_patterns_apply_with_gitignore_anchoring`
already uses `build/**` as a *user* pattern and asserts `crate/build/out.o` still syncs — that test
would have broken.)

**The deadline, not a 1 Hz walk.** "Rescan every tick while anything is settling" would have been two
lines, but it undoes wave 1's own fix: `next_scan_ms` exists precisely so the engine does not re-stat
a 100 000-file tree every second, and a continuously appended log would have pinned it there forever.
`next_stable_ms` is self-limiting instead — a file that keeps changing keeps pushing its own deadline
out, so the walk cadence degrades to the settle window, not to the tick.

**The wake is sticky, and lives outside `watchers`.** `scan_is_due` is consulted before anyone knows
whether a walk will happen, so a tick that claims journal work consumes the decision without walking.
A bool on the watcher entry would also die when the watcher is re-armed. A separate
`Mutex<HashSet<String>>` cleared by `scan_and_enqueue` — the one place a supervisor walk actually
happens — is what makes "an announced change is never lost" true. It is cleared *before* the walk,
not after: a walk that fails would otherwise re-run every tick against a repository that is not going
to get better, and the paced backstop already covers a lost attempt.

**No `PollWatcher` fallback, and no per-profile `force_poll` yet.** `WatchConfig::force_poll` remains
available but is always `false` here: `notify::PollWatcher` re-stats the whole tree every 30 s, so it
is a strictly worse fallback than this engine's own paced scan. Failure therefore degrades to the
paced scan directly. A user-facing "check by polling" toggle would need a `SyncProfile` field, which
is 34.5's surface, not this one's.

**No `EchoSuppressor` registration.** The module doc warns that a fetch materialising 400 files
"produces 400 local changes". That hazard belongs to a watcher-driven *change queue*; our consumer is
`git status`, which is authoritative — a checkout leaves the tree clean against the new HEAD, and the
racily-clean case is already handled by `lfs::stage::is_false_modification`. So an echo costs at most
one extra fruitless walk, not a re-commit loop. Wiring the suppressor would mean editing `do_pull`,
which this story does not own; the reasoning is recorded on `fold_watch_events` so it is a decision
rather than an omission.

**`held` vs `gate.tracked()`.** The walk's `held` is the more honest number: it counts paths held for
*any* reason, including one whose `lstat` failed, for which no gate entry exists. `refresh_pending`
can only read `tracked()` between walks, so the walk's count always wins when a walk happens. Both
are written; neither is recomputed from the other.

**Watcher lifetimes live in the engine, not in either host.** Both `keeper/src/sync.rs`
(`start_supervisor`) and `keeper-syncd`'s `run_supervisor` call `Engine::run`, so owning the watchers
there gives both hosts the behaviour for free and keeps AD-52's "no second implementation of this
policy on the server" true. Neither file was touched.

**Cross-story dependency.** `warn_watch_degraded` calls
`SyncProfile::effective_poll_interval_ms()`, introduced by story 34.5 in the same wave (agreed over
IRC before either landed). AD-34-8 requires the surface to name the cadence actually in force and not
re-derive it, so duplicating the 2 s floor here would have been a second copy of one fact.

## Verification

**Not run here.** No build, no linter, no formatter and no test suite were run for this story: the
parent agent runs them once for the whole wave, and `cargo build` for the `keeper` crate does not work
on this Linux box at all (tauri needs GTK/glib-sys). Static checks performed instead: brace/paren
balance on all six edited Rust files; a byte-level `git diff` of `SyncStatusVm.ts` confirming 9 added
lines, 0 removed, with the ts-rs trailing space preserved; verification against the vendored
`notify-8.2.0` and `notify-debouncer-full-0.7.0` sources that `Debouncer<RecommendedWatcher,
RecommendedCache>` is `Send` (the new `const _` assertion in `watch.rs` pins it) and that
`FolderWatcher::start` over a non-existent root fails on **both** shipped backends — inotify with
ENOENT, FSEvents via `append_path`'s explicit `path_not_found` — which is what makes the
watcher-failure test deterministic on the macOS runner.

**Commands for the parent:**
- `cargo test -p keeper-sync` -- the 18 new/extended Rust tests. Named individually:
  `next_stable_ms` (`the_next_stable_instant_is_never_later_than_the_verdict_it_predicts`,
  `a_fresh_mtime_pushes_the_deadline_out_as_far_as_the_verdict_does`,
  `a_close_write_pulls_the_deadline_in_to_the_short_window`,
  `a_file_that_never_quiesces_is_still_scheduled_by_the_ceiling`,
  `an_implausibly_future_mtime_does_not_push_the_deadline_out`,
  `the_deadline_is_the_soonest_of_every_held_path`); excludes
  (`ordinary_english_build_directory_names_are_not_tier_zero`,
  `every_directory_shaped_convention_carries_a_name_and_a_subtree_rule`, plus 8 new rows in
  `every_industry_convention_is_excluded`); status line
  (`a_folder_whose_files_are_still_being_written_is_never_up_to_date`,
  `an_active_phase_still_outranks_the_settling_count`); db
  (`quiescence_state_round_trips_and_replaces_rather_than_accumulating`,
  `a_failed_row_rolls_the_whole_replace_back`); engine
  (`a_watcher_is_armed_for_every_enabled_profile_and_outlives_none_of_them`,
  `a_watcher_that_cannot_arm_says_so_and_keeps_saying_so`,
  `arming_a_watcher_does_not_retire_someone_elses_warning`,
  `a_held_file_reopens_the_walk_on_its_own_window_not_the_poll_interval`,
  `a_delivered_close_write_reaches_the_gate_and_shortens_the_wait`,
  `a_walk_that_holds_files_says_how_many_instead_of_up_to_date`,
  `an_excluded_file_never_appears_in_the_pending_list`).
- `bun run bindings:check` -- must report no drift for `SyncStatusVm.ts`.
- `bun run check` -- the three `statusVm` fixtures now carry `settling`.

**None of the tests wait on filesystem event timing.** Watcher lifecycle is driven through
`ensure_watcher` / `retain_watchers` / `drop_watcher` / `stop_all_watchers` directly, and the one test
that exercises event handling arms a real watcher for its `Drop` behaviour but puts the `WatchEvent`
on the channel by hand — the same discipline `watch.rs`'s own tests use, where everything interesting
is decided in the pure `fold_batch`.

**Machine check on hesperia (the acceptance the parent cannot run here):**
1. `git -C <synced folder> log --oneline | head -1` and note the tip.
2. Prepare 500 small files *outside* the synced folder: `mkdir -p /tmp/drop && for i in $(seq 500);
   do head -c 4096 /dev/urandom > /tmp/drop/f$i.bin; done`.
3. Watch the engine: `log stream --predicate 'process == "keeper"' --style compact` (or the app's log
   file), then `cp /tmp/drop/*.bin <synced folder>/` and start a stopwatch.
4. **Expect** a `folder watch armed` line at app start, and within a few seconds — not ~15 s — a
   `Committing` phase and a new commit. On macOS the FSEvents backend has no close-write, so the
   bound is the 5 s settle window plus one tick, roughly 6–7 s; the previous behaviour was two 15 s
   poll intervals. On Linux it is ~2 s via `CLOSE_WRITE_SETTLE_MS`. The distinction matters: if it
   takes ~6 s on hesperia that is the fix working, not the fix failing.
5. While the copy is in flight, read the tray line and the Sync pane: **expect** "N waiting for writes
   to stop", never "up to date". `keeper-syncd status --json` shows the same `settling`.
6. `touch <synced folder>/.DS_Store` and `mkdir -p <synced folder>/node_modules/x && touch
   <synced folder>/node_modules/x/y.js`: **expect** neither in the Pending list, and no commit
   mentioning them.
7. Watcher failure, Linux only (macOS has no equivalent knob): drop
   `fs.inotify.max_user_instances` to 1 with another watcher already running, restart the supervisor,
   and **expect** one notification plus a persistent warning naming the fallback cadence and the
   inotify limit — and syncing that still works, on the paced scan.
8. Leak check: `Activity Monitor` (or `ls /proc/<pid>/task | wc -l`) across ten pause/resume cycles of
   one profile — **expect** a flat thread count, and on Linux a flat
   `ls -l /proc/<pid>/fd | grep -c inotify`.
