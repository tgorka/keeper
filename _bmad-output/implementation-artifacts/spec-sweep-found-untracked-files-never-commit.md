---
title: 'A file the sweep finds is a file that gets committed'
type: 'bugfix'
created: '2026-09-09'
status: 'done'
review_loop_iteration: 0
baseline_commit: '60def3b686c7fa53f21a4a9142398fb9a16a1c5b'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** An untracked file that only the untracked sweep discovers — moved in by hand, renamed, created while keeper was down, or announced by an event the backend dropped — is never committed. The sweep's full walk finds it and the gate holds it (`Settling`: one sample is not two); the settle walk 5 s later is `tracked_only().including(held)`, which has no directory walk and cannot report an untracked path, so `gate.retain(&observed)` forgets the episode and `file_state` empties. The next sweep (24 h) restarts the loop. Field evidence, hesperia 2026-09-09: neuradrive `untracked=160` then `untracked=0 included=159` eight seconds later, 62.8 GB never committed; tgdrive-light 36 `.gitkeep` across seven sweeps on two builds. The same shape was fixed for renames as F-GATE-7 and not for the sweep.

**Approach:** A pass whose walk found untracked paths the gate is now holding buys the next commit-leg walk a directory scan — the same `untracked_appeared` flag a watcher `Create` and a prime set — so the second observation is taken by a walk that can see the path. And the policy refuses to hand a held path the index does not carry to a walk without a directory scan: it widens to `full()` and says so at `warn`, because that is a held path about to be forgotten.

## Boundaries & Constraints

**Always:** sweep cadence, `UNTRACKED_SWEEP_INTERVAL`, `POLL_WALK_MIN_INTERVAL` and the poll leg's policy unchanged; the flag is set by the pass that observed the path and consumed by the walk that answers it, as today; index and `lstat` probes run outside the gate lock (AD-232, 70.6); no `.unwrap()`; the guard logs at `warn` with the count and one path; `docs/sync.md` §21's sweep row says what happens after `untracked=N`.

**Ask First:** if the guard's index probe measurably adds more than one second to a commit-leg pass on the reference folder, ask before keeping it in the policy rather than in `collect_stable_changes`.

**Never:** the poll leg does not observe into the gate; a `full()` walk is never narrowed; `prime_moved_paths`, the watcher and `StabilityGate`'s verdict arithmetic are not touched; no special case for `.gitkeep` or zero-byte files — the mechanism is the same for every untracked path.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Sweep finds a file | live watcher, file written before the watcher armed, first pass of the run | pass 1 holds it and sets `untracked_appeared`; pass 2 (≥ settle) is `full()`, observes it again, commits it; `file_state` never drops it in between | — |
| Flag consumed, path still held | gate holds an untracked path, `untracked_appeared` absent | `commit_walk_policy` answers `full()`, one `warn` naming the count and a path | — |
| Held path is tracked | gate holds a modified tracked file only | policy unchanged: `tracked_only().including(held)`, no warn | — |
| Sweep finds nothing new | `untracked=0` | flag untouched, no walk bought | — |
| Restart between pass 1 and 2 | `file_state` carries the entry, flag lost with the process | the first pass of a run sweeps anyway, observes the entry and sets the flag again | — |

</frozen-after-approval>

## Code Map

- `src-tauri/crates/keeper-sync/src/engine.rs` -- `walk_policy` (:1942–1983): the sweep branch (:1951–1963) returns `full()` without touching `untracked_appeared`; the narrowed return at :1982 is where held paths ride along — the guard goes just before it. `commit_walk_policy` (:1911) consumes the flag; `poll_walk_policy` (:1890) stays. `collect_stable_changes` (:8684): walk at :8747, untracked group from `expand_untracked` at :8796, gate loop :8817–8831 (`Settling` arm :8827), the prune `gate.retain(&observed)` :8836, `"files still settling"` debug :8929. Precedents for the one-line flag: :4995 (`fold_watch_events`) and :5385 (`prime_moved_paths`, F-GATE-7). Index probe idiom: `repo.index_or_empty()` + `entry_index_by_path` (:4945–4950).
- `src-tauri/crates/keeper-sync/src/git/repo.rs` -- read-only: `WalkPolicy` (:1760), `find_untracked` (:1790), `including()` (:1870) refuses includes on a dirwalk policy; the inverse case has no guard.
- `src-tauri/crates/keeper-sync/src/stability.rs` -- read-only: `verdict` (:476) "one sample is not two", `retain` (:739), `export` (:750).
- Tests in `engine.rs` -- fixtures `adoptable` (:22466), `commit_after_settling` (:22476), `TestPlatform::advance_ms` (`platform.rs:725`); live-watcher idiom in `priming_moved_paths_makes_the_next_walk_a_full_one` (:24343); `engine.activity` is async; `db::load_file_state` (`db.rs:2623`).
- `docs/sync.md` -- §21 sweep row (:3153), §4 (:131).
- `_bmad-output/planning-artifacts/review-sync-2026-09-08/lanes/gate-watch.md` -- F-GATE-7, the precedent's argument.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- red test `a_file_the_sweep_found_is_committed_once_it_settles`: write a file, arm a live watcher (no `Create`), `commit_local` ×2 with `advance_ms(settle + 1)` between; assert the flag is set after pass 1, `load_file_state` still holds the path before pass 2, and `activity` names it after pass 2 -- the bug, end to end.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- red test `a_held_path_the_index_does_not_carry_makes_the_commit_walk_a_full_one`: after a full walk held an untracked path, remove the flag by hand, assert `commit_walk_policy` is `full()`; with only a tracked held path it stays narrowed -- the guard.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- in `collect_stable_changes`, after the gate loop, set `untracked_appeared` when any path of the untracked group was `Settling` -- the fix, one line beside its two precedents.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- in `walk_policy`, before the narrowed return, probe the index for the held paths (outside the gate lock); any miss ⇒ `tracing::warn!` (count, first path, "a narrowed walk cannot observe it; widening") and return `full()` -- the guard.
- [x] `docs/sync.md` -- §21 sweep row: after `untracked=N`, the path is held and the next commit-leg walk reads the directories again so the gate's second look can see it; §4 one sentence naming the same promise -- the record.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- test for matrix row "Sweep finds nothing new": a clean adopted tree, live watcher, one `commit_local` (the first pass of a run sweeps, `untracked=0`); assert `untracked_appeared` does not contain the profile and the next `commit_walk_policy` is `tracked_only()` -- the flag is set only by a held untracked path.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- test for matrix row "Restart between pass 1 and 2": after pass 1 held the sweep's find, `drop(engine)` and `Engine::open` again on the same `TestPlatform` (idiom: `interrupted_work_is_requeued_when_the_engine_reopens`, :22525), arm the watcher, advance past settle, run passes until the file commits; assert `file_state` carried the path across the reopen and `activity` names it -- the flag is process-local, the sweep on the first pass of a run is what covers it.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- in `a_held_path_the_index_does_not_carry_makes_the_commit_walk_a_full_one`, capture the guard's `warn` with the crate's own idiom (`tracing::subscriber::set_default`, :30089) and assert one line carrying `missing=1` and `found.bin` -- matrix row "Flag consumed, path still held" promises the line, not only the policy.

Review patches (step 04, all `patch` category — the diff is kept, these are amendments to it):
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- `held_paths` reads only `existing_gate`, which is `None` until the first commit-leg walk seeds the gate; after a restart with the Sync pane open the *poll* leg's first walk stamps the run's sweep clock (`walk_policy` stamps for both legs), so the commit leg's first walk is `tracked_only()` with an unseeded gate, `gate_for` then imports the held untracked path from `file_state` and `retain` forgets it — the original loss on a realistic ordering (the field log shows `caller="poll"` first after the 02:18 restart). Fix: when no gate exists, `held_paths` answers from `db::load_file_state` (repository-relative), so the probe sees persisted held paths too. Test: `a_sweeps_find_survives_a_restart_whose_first_walk_was_the_polls` — run 1 holds `found.bin`; reopen; call `poll_walk_policy` first (it consumes the sweep), then assert `commit_walk_policy` is `full()` with the warn, and the file commits.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- the guard's `full()` return does not stamp `untracked_sweep`, unlike the sweep branch and the flag branch in `commit_walk_policy`; a sweep falling due right after repeats the directory walk. Fix: stamp the clock in the guard branch (a full walk answers every path).
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- `held_paths_the_index_lacks` reports "missing" for every path when the index cannot be read, and the warn then says "the index does not carry", which sends an operator after a phantom untracked file. Fix: an unreadable index is its own outcome — a distinct warn carrying the error (`index=unreadable`, `%err`), still widening; the "does not carry" warn keeps `missing` and `path`. Also reword "a narrowed walk cannot observe it" to "a walk without a directory scan cannot observe it" — the guard also fires before the overflow's whole-index walk.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- doc comments now false: `commit_walk_policy` ("a pass with no wake pending has nothing new for a directory scan to find") and `walk_policy`'s `with_held` sentence (held paths can now widen the walk). Fix: bring both in line with the guard, and note on `RecordedLog::fields_of` that fields are rendered `key=value` by the recorder (`Debug` for non-`str`), not by the real subscriber.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- tests pin the mechanism: in `a_file_the_sweep_found_is_committed_once_it_settles` assert the widening warn was *not* emitted (the flag is the ordinary path) and that the policy after the commit is `tracked_only()`; in `a_held_path_the_index_does_not_carry_makes_the_commit_walk_a_full_one` assert `poll_walk_policy` stays `tracked_only()` with no warn while the unindexed path is held (the poll leg is immune by `with_held`), and add the overflow branch: after `widen_watch_paths(&p.id)` the commit policy is `full()` with one warn; in the restart test assert after the new run's first pass that either it committed or the flag is set.
- [x] `docs/sync.md` -- §21: the backstop row's "no directory walk" is no longer unconditional (a held path git does not carry widens it); the sweep row's log column gains the widening warn line; §4's new paragraph follows the future-mtime sentence, so "held the same way" has the wrong antecedent — anchor it to the four tiers, and say the second look repeats on every pass until the path settles.

**Acceptance Criteria:**
- Given a live watcher and a file it never announced, when two passes run a settle window apart, then the second commits it and `file_state` was never emptied between them.
- Given the gate holds an untracked path and no flag, when the commit leg asks its policy, then it is `full()` and one `warn` line names the path.
- Given the gate holds only tracked paths, when the commit leg asks its policy, then it is the narrowed walk with no warn.
- Given `untracked=0`, when a sweep completes, then no directory walk is bought and the flag stays clear.

## Design Notes

Two changes, not one: the flag is the cheap, precedented path and covers the ordinary case in one line; the guard makes "a held path is re-observed" a property of the policy instead of something every caller must remember — 70.1's boundary "every held gate path is in the includes of a narrowed walk" was only ever true for tracked paths. The guard costs one index probe per held path, only when held paths exist and the policy has no dirwalk; the walk parses the same index a moment later.

Ask-First resolved without asking: the probe's cost is one index parse, and today's log on the reference folder puts a whole `tracked_only` walk over 155 626 entries — parse included — at 237–1 656 ms (`status walk finished … scanned=155626 elapsed_ms=237`, 02:50:46 UTC), so the probe alone is well under the one-second line. The guard sits before both no-dirwalk returns (overflow and narrowed) because the Intent's rule names any walk without a directory scan.

## Verification

**Commands:**
- `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-sync a_file_the_sweep_found_is_committed_once_it_settles a_held_path_the_index_does_not_carry_makes_the_commit_walk_a_full_one` -- expected: red before the fix, green after.
- `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-sync` -- expected: all green (the Story 70.1/70.5 policy tests still hold).
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-sync --all-targets -- -D warnings && cargo fmt --manifest-path src-tauri/Cargo.toml --check` -- expected: clean.

## Suggested Review Order

**The fix — a held untracked path buys the next walk a directory scan**

- Entry point: the one-line flag beside its two precedents; why the settle walk could never see the sweep's find.
  [`engine.rs:8999`](../../src-tauri/crates/keeper-sync/src/engine.rs#L8999)
- The gate loop learns which group the index does not carry — the only group a no-scan walk is blind to.
  [`engine.rs:8942`](../../src-tauri/crates/keeper-sync/src/engine.rs#L8942)
- `Settling` on an unindexed path is what owes the scan; tracked paths owe nothing.
  [`engine.rs:8974`](../../src-tauri/crates/keeper-sync/src/engine.rs#L8974)

**The guard — the policy refuses a held path it cannot observe**

- Where held paths reach a no-scan walk; both returns are guarded, overflow included.
  [`engine.rs:1988`](../../src-tauri/crates/keeper-sync/src/engine.rs#L1988)
- Three answers from one index probe; unreadable is not "missing", and both widen.
  [`engine.rs:2009`](../../src-tauri/crates/keeper-sync/src/engine.rs#L2009)
- Held paths come from the gate, or from `file_state` before the gate is seeded — the poll-first restart.
  [`engine.rs:2063`](../../src-tauri/crates/keeper-sync/src/engine.rs#L2063)
- The probe itself: repository opened after the gate lock is released (AD-232, 70.6).
  [`engine.rs:2096`](../../src-tauri/crates/keeper-sync/src/engine.rs#L2096)
- The enum that makes the unreadable index its own log line.
  [`engine.rs:1524`](../../src-tauri/crates/keeper-sync/src/engine.rs#L1524)
- `commit_walk_policy`'s doc now names both mechanisms behind the flag.
  [`engine.rs:1932`](../../src-tauri/crates/keeper-sync/src/engine.rs#L1932)

**The record**

- §21: after `untracked=N`, what happens to what was found, and the widening warn an operator may see.
  [`sync.md:3159`](../../docs/sync.md#L3159)
- §21: the backstop's "no directory walk" is conditional now.
  [`sync.md:3158`](../../docs/sync.md#L3158)
- §4: the second look for a path git has never seen, and that it repeats until settle.
  [`sync.md:182`](../../docs/sync.md#L182)
- Deferred: re-sampling held unindexed paths without a walk — the cost this story accepts.
  [`deferred-work.md:6021`](./deferred-work.md#L6021)

**Tests — the bug, the guard, the rows**

- The bug end to end: sweep finds, gate holds, second pass commits; flag set, no widening warn.
  [`engine.rs:24580`](../../src-tauri/crates/keeper-sync/src/engine.rs#L24580)
- The guard alone: flag removed by hand, poll leg immune, overflow branch, one warn with `missing=1 path=found.bin`.
  [`engine.rs:24652`](../../src-tauri/crates/keeper-sync/src/engine.rs#L24652)
- Restart whose first walk was the poll's — the ordering the `file_state` fallback exists for.
  [`engine.rs:24919`](../../src-tauri/crates/keeper-sync/src/engine.rs#L24919)
- Restart between the two looks; the episode is never forgotten on the way.
  [`engine.rs:24828`](../../src-tauri/crates/keeper-sync/src/engine.rs#L24828)
- A sweep that finds nothing buys nothing.
  [`engine.rs:24773`](../../src-tauri/crates/keeper-sync/src/engine.rs#L24773)
- The one changed assertion: a still-held untracked prime now widens by the guard (was: no scan owed).
  [`engine.rs:24562`](../../src-tauri/crates/keeper-sync/src/engine.rs#L24562)
- The log recorder now keeps fields, so a test can promise what a line carries.
  [`engine.rs:30284`](../../src-tauri/crates/keeper-sync/src/engine.rs#L30284)
