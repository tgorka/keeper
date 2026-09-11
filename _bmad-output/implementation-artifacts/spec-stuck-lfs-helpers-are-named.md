---
title: 'A stuck LFS helper is a helper keeper can name'
type: 'bugfix'
created: '2026-09-09'
status: 'done'
review_loop_iteration: 0
baseline_commit: 'dfe24913825cd387e6beb6b3fe23786a8bd7ec47'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** keeper registers itself as `filter.lfs.process` in every folder it syncs, so any `git` a person or tool runs there starts a `keeper lfs filter-process` helper — and keeper has no view of those helpers. On hesperia 2026-09-09, 18 orphaned `git status` processes from an external caller each held one, idle for 3 h 22 min on `/Volumes/merope/neuradrive`; keeper's own walks on that volume went from seconds to 200–240 s and one never finished, and nothing in the log named a helper. The helper's `REQUEST_LIMIT` (900 s) is armed only inside a request — between requests it waits on stdin by design, because an idle exit is recorded under `required=false` as a filter that produced zero bytes (DW-140) — and `GIT_DEADLINE` covers only git children keeper spawned.

**Approach:** every long-running helper registers itself under its repository's LFS store — a `helpers/<pid>` file it holds an exclusive lock on for its lifetime and touches after every request — and keeper looks at that directory on an hourly clock: a file whose lock can be taken is a dead helper's leftover and is removed; a locked file idle longer than 30 minutes is a stuck helper. Keeper counts them, names the oldest's idle time and its parent process's command line, logs one line per look, raises one sticky warning per onset and retires it when none remain. Keeper never kills a helper.

## Boundaries & Constraints

**Always:** the helper's request loop, `REQUEST_LIMIT`, `required=false` and the exit-on-stall behaviour stay as they are; registration failures never fail the helper (a helper that cannot register still serves — log to stderr and go on); the look is housekeeping and never fails a pass; the lock is `fs4` (workspace dependency) and the idle signal is the file's mtime, set with `File::set_modified`; parent command lines come from `ps` at look time only for stuck helpers (the idiom `tests/lfs_filter_process.rs` already uses); one `tracing::info!` per look with `alive`, `stuck`, `removed`; the sticky warning goes through `Engine::warn` and is cleared only when the snapshot's warning is this one (the `clear_watch_warning` idiom); `docs/sync.md` §21 gains the row and the `filter.lfs.process` section the sentence; no `.unwrap()`.

**Ask First:** if the hourly look measurably costs more than one `read_dir` plus one `try_lock` per file on the reference folder, ask before adding a cache.

**Never:** kill, signal or close a helper or its parent; exit a helper because it is idle; add `libc`/`nix`; put the marker anywhere but the repository's own `.git/lfs/`; change the poll/scan cadences.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Helper alive and busy | locked marker, mtime < 30 min | counted `alive`, not stuck, no warning | — |
| Helper stuck | locked marker, mtime ≥ 30 min, parent pid alive | `stuck=1`, warning names count, oldest idle, parent command; anomaly line | `ps` failure ⇒ parent shown as `unknown` |
| Dead helper's leftover | unlocked marker | removed, counted `removed`, never stuck | unlink failure logged at debug, not fatal |
| Stuck helper goes away | stuck last look, none now | warning cleared, `stuck=0` logged | — |
| Another warning is up | degraded-watcher warning present | helper look does not clear it; a new helper onset replaces it as today's `warn` does | — |
| Registration impossible | `helpers/` not creatable | helper serves normally, one stderr line | — |

</frozen-after-approval>

## Code Map

- `src-tauri/crates/keeper-sync/src/lfs/filter.rs` -- `run_process` (:389) is the long-running helper: after `store.ensure_layout()` register the marker and keep its locked `File` alive to the end of the function; touch mtime after `serve_one` (:409–412). `RequestGuard` (:446) shows the per-request watchdog — read-only.
- `src-tauri/crates/keeper-sync/src/lfs/store.rs` -- `LfsStore` (:121), `in_git_dir` (:134), `tmp_dir` (:162), `ensure_layout` (:235) gains `helpers/`; `sweep_scratch` (:193) and `ScratchSweep` (:172) are the shape for `look_at_helpers` and its `HelperLook`; `SCRATCH_DEBRIS_AGE` (:171) the shape for `STUCK_HELPER_IDLE`.
- `src-tauri/crates/keeper-sync/src/engine.rs` -- `tick_one` (:2916) calls `sweep_scratch_if_due` (:4477) — the new look sits beside it on its own hourly clock, keyed like `next_release_ms` (:1258) with `RELEASE_LOOK_EVERY_MS` (:597) as the constant's shape; `release_is_due` (:4684) is the due-check idiom; `warn` (:2616) sticky per onset, `clear_watch_warning` (:2601) the keyed clear idiom; `Anomaly::report` (`anomaly.rs:42`).
- `src-tauri/crates/keeper-sync/Cargo.toml` -- add `fs4 = { workspace = true }` (workspace: `src-tauri/Cargo.toml:92`, feature `sync`).
- `src-tauri/crates/keeper-sync/tests/lfs_filter_process.rs` -- `HARNESS` (`CARGO_BIN_EXE_lfs-filter-harness`), `pointer_fixture` (:69) registers the harness as the filter and drives a real gix status through it; `process_table` (:291) is the `ps` idiom.
- `src-tauri/crates/keeper-sync/src/bin/lfs-filter-harness.rs` -- the binary tests spawn; it calls `run_process` (:28), so it registers like the app.
- `docs/sync.md` -- §21 table (rows `LFS prune`/`scratch sweep` at :3163/:3166); "Keeper owns `filter.lfs.process`" (:317–345).

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-sync/Cargo.toml` -- add `fs4` -- the lock.
- [x] `src-tauri/crates/keeper-sync/src/lfs/store.rs` -- `helpers/` in the layout; `register_helper(pid) -> Option<HelperMarker>` (create, lock exclusive, keep the file; `touch()` sets mtime now; unlock+unlink on drop); `look_at_helpers(now, idle) -> HelperLook { alive, stuck: Vec<(pid, idle)>, removed }` (try-lock each file: success ⇒ dead ⇒ remove; locked ⇒ alive, stuck if mtime older than `idle`); unit tests for every matrix row that is store-level -- the mechanism, testable without a process.
- [x] `src-tauri/crates/keeper-sync/src/lfs/filter.rs` -- `run_process` registers after `ensure_layout`, touches after each request, drops at return -- the signal.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- `look_at_helpers_if_due` on an hourly clock (and the first tick of a run), called from `tick_one`; parent command via `ps -o ppid= -p` then `ps -o command= -p`; INFO line; anomaly + sticky warning on stuck>0; keyed clear on stuck==0; engine test with a marker the test itself locks and back-dates: onset warns once, second look does not repeat, unlock clears -- the eyes.
- [x] `src-tauri/crates/keeper-sync/tests/lfs_filter_process.rs` -- a real helper started through gix creates a locked marker under `.git/lfs/helpers/<pid>` while it serves, and the marker is unlocked or gone once gix drops it -- the contract end to end.
- [x] `docs/sync.md` -- §21 row `helper look` (what/why/when/log line/where) and one sentence in the `filter.lfs.process` section: keeper watches its helpers and names a stuck one, and why it never kills one -- the record.

Review patches (step 04, all `patch` — amendments to the diff above, the diff is kept):
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- `Engine::clear_warning` runs after every successful unit and wipes the snapshot's warning, and `warn` keys its toast on `None → Some`; a stuck helper does not stop units succeeding, so in the field the helper warning re-toasts every hour. Fix: keep the onset yourself — a `helper_warned: Mutex<HashSet<String>>` (the `warn_watch_degraded` re-assert-silently split): first look with stuck>0 notifies via `warn`, later looks re-set the snapshot's warning text without notifying, `clear_helper_warning` removes the profile from the set. Test: a successful unit between two looks (or a direct `clear_warning`) does not produce a second toast.
- [x] `src-tauri/crates/keeper-sync/src/lfs/store.rs` -- `ensure_layout` now creates `helpers/`, so a `helpers` path that cannot be created fails `run_process` at `ensure_layout()?` before registration — the spec says registration never fails the helper. Fix: `ensure_layout` keeps its original three directories; only `register_helper` creates `helpers/` (it already does). Test: a `helpers` regular file in the store still lets `run_process` serve.
- [x] `src-tauri/crates/keeper-sync/src/lfs/store.rs` -- pid-reuse race in `look_at_helpers`: after taking a leftover's lock, `remove_file(&path)` can unlink a NEW helper's marker that was renamed into place under the same pid. Fix: before unlinking, compare the opened file's identity (`dev`+`ino` via `MetadataExt`) with `metadata(&path)`; on mismatch leave the path alone. Test: simulate by renaming a fresh locked marker over the path between open and unlink (a seam or a two-step helper in tests).
- [x] `src-tauri/crates/keeper-sync/src/lfs/store.rs` + `engine.rs` -- a filesystem whose `try_lock` errors makes every marker `continue` at debug, and the look reports zeros that read as healthy. Fix: `HelperLook` gains `skipped` (open/lock errors), the INFO line logs it, and when `skipped > 0` the look falls back to pid liveness for those markers (`ps -p <pid>` exit status) so they still count as alive/stuck; an `Anomaly` line when `skipped > 0` names the directory. Also count `removed` only for pid-named files; leave `.<pid>.registering` staging files alone unless older than the idle threshold.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- the parent named is always `git`; the process a person must quit is git's own parent. Fix: `helper_parent_command(pid)` walks the ppid chain (`ps -o ppid=,command= -p`), at most 5 hops, until the first command whose basename is not `git`, and returns "`<git cmd>` (pid N), run by `<ancestor cmd>` (pid M)"; a ppid of 1 is reported as "orphaned: its git has exited and pid 1 holds its stdin". Wrap the whole `ps` walk in `tokio::time::timeout(10 s)` around the blocking task; timeout or `JoinError` ⇒ `unknown` with one `warn` line. Move the function so the existing rustdoc for `impl ContentSource for Engine` stays attached to that impl. Unit test on the test's own pid: result contains its parent pid and is not `unknown`.
- [x] `src-tauri/crates/keeper-sync/src/lfs/filter.rs` -- touch the marker on request arrival as well as after `serve_one`, and on the unknown-command path; include the registration error text in the stderr line (return it from `register_helper` or log it there).
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- `look_at_helpers_if_due`: a `JoinError` from the look must log at `warn`, not return silently.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` + `src/lfs/store.rs` tests -- add: a look over two stuck markers with different idle picks the older (`oldest_stuck`, `stuck.len()==2`, warning says `2 LFS filter helper(s)`); a `rescan` after an armed hour makes `helper_look_is_due` true again; a tick-level test driving `engine.tick().await` with a locked back-dated marker asserts `status(..).warning` starts with `HELPER_STUCK_PREFIX` (the wiring, not the private method).
- [x] `docs/sync.md` -- "every `git` a person or a tool runs in the folder starts one" overstates it: only a git that cleans or smudges a tracked path does; say so. Mention `helpers/` beside `objects/`, `incomplete/`, `tmp/` where the store layout is described in §8, and that the warning names the git and the process that ran it.

**Acceptance Criteria:**
- Given a helper started by a foreign git that stops sending for 30 minutes, when the hourly look runs, then the log carries `stuck=1` with the parent's command line and the folder's card shows one warning naming it.
- Given the same helper then exits, when the next look runs, then its marker is gone, `stuck=0`, and the warning is retired.
- Given a helper busy on a 40-minute conversion, when the look runs, then it is `alive` and not stuck, because every request touches the marker.
- Given no helpers, when the look runs, then one INFO line with zeros and nothing else.

## Verification

**Commands:**
- `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-sync helper` -- expected: the new store, engine and harness tests pass.
- `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-sync` -- expected: all green.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-sync --all-targets -- -D warnings && cargo fmt --manifest-path src-tauri/Cargo.toml -p keeper-sync -- --check && cargo deny check --manifest-path src-tauri/Cargo.toml licenses` -- expected: clean (fs4 is already in the workspace graph).

## Suggested Review Order

**The signal — a helper says "I am here" without ending its wait**

- Entry point: the helper registers after the layout, touches on arrival and after each answer, drops last.
  [`filter.rs:402`](../../src-tauri/crates/keeper-sync/src/lfs/filter.rs#L402)
- The marker: a locked file the kernel releases when the process dies — no pid table needed.
  [`store.rs:178`](../../src-tauri/crates/keeper-sync/src/lfs/store.rs#L178)
- Registration is staged and renamed in, so no unlocked marker ever exists for a look to mistake.
  [`store.rs:342`](../../src-tauri/crates/keeper-sync/src/lfs/store.rs#L342)
- Unlock then unlink, as the helper's last act.
  [`store.rs:201`](../../src-tauri/crates/keeper-sync/src/lfs/store.rs#L201)

**The look — dead leftovers removed, stuck helpers named, nothing killed**

- The look: try the lock; takeable ⇒ leftover, locked ⇒ alive, idle ≥ 30 min ⇒ stuck; lock errors ⇒ skipped, not dropped.
  [`store.rs:389`](../../src-tauri/crates/keeper-sync/src/lfs/store.rs#L389)
- The pid-reuse guard: unlink only if the path is still the file that was opened.
  [`store.rs:484`](../../src-tauri/crates/keeper-sync/src/lfs/store.rs#L484)
- The answer shape, and why the oldest is the one named.
  [`store.rs:141`](../../src-tauri/crates/keeper-sync/src/lfs/store.rs#L141)
- The idle threshold beside the scratch-debris age it mirrors.
  [`store.rs:322`](../../src-tauri/crates/keeper-sync/src/lfs/store.rs#L322)

**The eyes — the hourly clock, the warning, the parent**

- The look on its clock: INFO line, anomaly, sticky warning with its own onset (a success between looks does not re-toast).
  [`engine.rs:4715`](../../src-tauri/crates/keeper-sync/src/engine.rs#L4715)
- The `ps` walk up the parent chain to the process a person can actually quit; orphaned at pid 1 is its own sentence; 10 s bound.
  [`engine.rs:16063`](../../src-tauri/crates/keeper-sync/src/engine.rs#L16063)
- The onset kept apart from `clear_warning`'s sweep.
  [`engine.rs:1279`](../../src-tauri/crates/keeper-sync/src/engine.rs#L1279)
- Keyed clear: only this warning is retired.
  [`engine.rs:4816`](../../src-tauri/crates/keeper-sync/src/engine.rs#L4816)
- The clock and its wiring into the tick.
  [`engine.rs:4667`](../../src-tauri/crates/keeper-sync/src/engine.rs#L4667) · [`engine.rs:2962`](../../src-tauri/crates/keeper-sync/src/engine.rs#L2962)

**The record**

- Why keeper never kills a helper, and what the warning names instead.
  [`sync.md:341`](../../docs/sync.md#L341)
- §21 row: what, when, the log line.
  [`sync.md:3182`](../../docs/sync.md#L3182)

**Tests — the contract end to end**

- A real helper over a real pipe: locked while serving, touched by a request, gone after EOF; then a real gix status leaves none behind.
  [`lfs_filter_process.rs:280`](../../src-tauri/crates/keeper-sync/tests/lfs_filter_process.rs#L280)
- The warning lifecycle: onset once, no re-toast after a success, unlock retires.
  [`engine.rs:20172`](../../src-tauri/crates/keeper-sync/src/engine.rs#L20172)
- The wiring: a real `tick()` puts the warning on the card.
  [`engine.rs:20417`](../../src-tauri/crates/keeper-sync/src/engine.rs#L20417)
- Two stuck helpers: the count, and the older one named.
  [`engine.rs:20338`](../../src-tauri/crates/keeper-sync/src/engine.rs#L20338)
- The parent walk resolves on the test's own pid.
  [`engine.rs:20460`](../../src-tauri/crates/keeper-sync/src/engine.rs#L20460)
- The race and the lock-error path, on the production function through seams.
  [`store.rs:1315`](../../src-tauri/crates/keeper-sync/src/lfs/store.rs#L1315) · [`store.rs:1369`](../../src-tauri/crates/keeper-sync/src/lfs/store.rs#L1369)
- A helper that cannot register still serves.
  [`filter.rs:1266`](../../src-tauri/crates/keeper-sync/src/lfs/filter.rs#L1266)
