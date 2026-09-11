---
title: 'The pass takes its own second look'
type: 'refactor'
created: '2026-09-10'
status: 'done'
review_loop_iteration: 1
baseline_commit: '9b09b427b4c14792de900eef22cb8346775ffd77'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** a path the gate holds that the index does not carry can only be observed by a walk that reads the directories, so since "a file the sweep finds is a file that gets committed" (`1c26f53a`) every commit-leg pass while such a path settles buys a full directory walk: `collect_stable_changes` sets `untracked_appeared` whenever an untracked path is `Settling`, `commit_walk_policy` spends it on `full()`, and `walk_policy`'s guard widens whenever the flag is gone. On the reference folder a full walk is 1.6–6.8 s (measured 2026-09-09) against ~0.2 s for the narrowed one, and a sweep-found file still being written costs one every settle window (5 s) until it stops — the cost `deferred-work.md` records against that story.

**Approach:** the pass observes held paths itself. After its walk, `collect_stable_changes` samples every held path the walk could not report — one the index does not carry, still on disk — through the gate (`is_stable`): `Stable` stages it as an addition, exactly as a walk-reported one (the commit path reads it under its sample and tier 4); `Settling` keeps it held; `Vanished` forgets it. With the pass doing that, a held unindexed path needs no directory walk: the `Settling`-untracked flag and the guard's widening go, the narrowed walk keeps carrying held paths as includes, and the full walks that remain are the ones that discover *new* paths — the sweep, the first pass of a run, a watcher `Create`, a degraded watcher.

## Boundaries & Constraints

**Always:** a re-sampled path goes through the same gate verdict, tier-0 exclusion, `expand_untracked` collapse rules, `truncated_media`/`dataless` refusals and the tier-4 read as a walk-reported one; the set of held paths comes from the gate's own export, the "not carried by the index" question from one index probe per pass (the existing `held_paths_the_index_lacks` probe, reused, outside the gate lock — AD-232, 70.6); the poll leg is untouched; the sweep cadence, the first-pass sweep and the watcher's `Create` full walk stay; the restart shapes keep working because `gate_for` imports `file_state` before the loop; the guard's `warn` line is retired with the guard, and `docs/sync.md` §4 and §21 say the second look is the pass's own sample; tests from `1c26f53a` are adapted to assert the new shape (no full walk between the two looks), never deleted.

**Ask First:** if re-sampling more than a few hundred held paths per pass measurably costs more than the narrowed walk it replaces, ask before adding a cap.

**Never:** stage a path the gate did not clear; observe into the gate from the poll leg; touch `prime_moved_paths`, the watcher, or `StabilityGate`'s verdict arithmetic; change `UNTRACKED_SWEEP_INTERVAL`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Sweep finds a file | live watcher, file written before it armed | pass 1 holds it; pass 2 is the narrowed walk, re-samples it, commits it; no full walk between | — |
| File still being written | sweep-found, rewritten every pass | re-sampled each pass, `Settling`, no directory walk; commits when it stops | — |
| Held path vanished | file deleted before its second look | forgotten; nothing staged; no error | — |
| Restart between the looks | `file_state` carries the entry | first pass of the new run re-samples after seeding, commits when settled | — |
| Poll walked first after restart | sweep consumed by the poll leg, gate unseeded | commit leg's pass seeds, re-samples, commits — no widening needed | — |
| Held path now tracked | another leg committed it meanwhile | index carries it; the normal modified/clean path applies, no re-sample | — |
| Index unreadable | probe errors | held unindexed paths are re-sampled anyway (the probe's answer is only "which"); one warn | — |

</frozen-after-approval>

## Code Map

- `src-tauri/crates/keeper-sync/src/engine.rs` -- `collect_stable_changes` (:9499): the gate loop (:9630–9655, `Settling` arm and `untracked_settling |= unindexed` :9649), `gate.retain(&observed)` (:9659), the flag insert (:9674) — the re-sample runs between the loop and `retain`, adding its paths to `observed`; `expand_untracked` (:9399); `commit_walk_policy` (:2043, the flag consume :2052); `walk_policy` (:2076) with the guard (:2099–2145) to retire; `held_paths` (:2174) and `held_paths_the_index_lacks` (:2207) to reuse for "which held paths are unindexed" (after review loop 1 the probe is folded into `paths_for_the_second_look`, which takes the gate's export from the caller and returns a `SecondLook` — sample / held / collapsed); `WIDENING_WARN` and its tests.
- `src-tauri/crates/keeper-sync/src/git/commit.rs` -- `stage_and_commit_inner` reads `changes.added` under `changes.samples` (:374–415) — a re-sampled addition is staged exactly like a reported one.
- `src-tauri/crates/keeper-sync/src/stability.rs` -- `is_stable` (:632), `forget` (:699), `retain` (:739), `export` (:750).
- Tests in `engine.rs` from `1c26f53a`: `a_file_the_sweep_found_is_committed_once_it_settles`, `a_held_path_the_index_does_not_carry_makes_the_commit_walk_a_full_one`, `a_sweep_that_finds_nothing_new_buys_no_walk`, both restart tests, `priming_moved_paths_makes_the_next_walk_a_full_one` — the shapes to adapt.
- `docs/sync.md` -- §4 paragraph (:182), §21 sweep row (:3271); `deferred-work.md` entry (:6022) to close.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- in `collect_stable_changes`, after the gate loop: for every held path (gate export) not in `observed` that the index does not carry and that still exists, take the verdict through `gate.is_stable` — `Stable` ⇒ `new_paths`, `Settling` ⇒ `held += 1`, `Vanished` ⇒ nothing — and add it to `observed`; the samples for staged ones are taken where the others are -- the second look.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- remove the `Settling`-untracked flag insert and the guard's widening in `walk_policy` (held paths still ride as includes; the probe helper stays for the re-sample); retire `WIDENING_WARN`; `commit_walk_policy` still honours the watcher's flag -- no directory walk for a held path.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` tests -- adapt the `1c26f53a` tests to the matrix: the sweep-found file commits on a *narrowed* second pass (assert no `untracked sweep` line and `find_untracked == false` for pass 2); a file rewritten between passes stays held with no full walk across three passes; vanished held path forgotten; both restart shapes; the prime test keeps asserting the flag from `prime_moved_paths` -- the rows.
- [x] `docs/sync.md` + `deferred-work.md` -- §4 and §21 say the gate's second look at a path git has never seen is the pass's own sample of it, not a directory walk; the guard's warn line leaves §21; close the deferred entry -- the record.

Review patches (step 04, loop 1 — amendments to the diff, the diff is kept):
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- `paths_for_the_second_look` propagates a non-`NotFound` stat error from `expand_untracked` with `?`, so one held path under a directory that lost permission fails every commit-leg pass before `retain`/`save_file_state` — the entry can never be forgotten and nothing in the folder commits. Fix: mirror the gate's fail-closed rule — such a path is held (`held += 1`, kept in `observed`) with one `warn` per pass naming it and the error; `NotFound` keeps meaning "gone, forget". Test: `chmod 000` on the parent after the sweep held the file ⇒ the pass succeeds, the row stays, one warn; restoring the permission lets it commit.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- a re-sampled path must pass git's own exclusion like a walk-reported one: consult the repository's excludes (gix `repo.excludes(...)` / `excludes.at_path(...)`) and drop a candidate that is ignored or lies inside a nested repository (it is then forgotten by `retain`, with an info line); pass the `_collapsed` half to `report_collapsed` as the walk does. Tests: a `.gitignore` written after the sweep held the file ⇒ not staged, forgotten; a held path that became a nested repository ⇒ reported collapsed, not staged.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- `collect_stable_changes` already holds the gate `Arc`; hand its export (or the gate) to `paths_for_the_second_look` instead of a second `held_paths` → `existing_gate` round trip; document that `observed` is absolute paths and the return is repository-relative.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` tests -- pin the rows the reviewers mutated green: (a) "held path now tracked" — commit `tracked.txt`, rewrite (held), write the original bytes back, next pass ⇒ `file_state` empty, `settling == 0`, nothing committed, and `paths_for_the_second_look(&p, &repo, &empty)` returns `[]` while `held_paths` still lists it; (b) the sweep-found file's activity row is `ActivityKind::Added`, not `Modified`; (c) `last_walk` matches fields by equality (`included=1` never matches `included=10`); (d) a timing test: 500 held unindexed paths' second look completes under one second on this machine (the Ask-First evidence).
- [x] `docs/sync.md` + `deferred-work.md` + spec (non-frozen) -- §4 cost arithmetic: two `lstat` per held unindexed path per pass plus the read of one that stages; "gone" rather than "`Vanished`" (the stat filters it before the gate); the unreadable-index warn is per pass and, through `commit_local`, reachable only by a race with the walk — say so; a new deferred entry: `prime_moved_paths` still buys a full directory walk for a rename the pass would now sample itself (the Never list keeps it out of this story); the spec's Verification notes the PATH/git-2.23 caveat (`/opt/homebrew/bin` first).

**Acceptance Criteria:**
- Given a file only the sweep found, when it settles, then it is committed by a pass whose walk had no directory scan.
- Given a sweep-found file rewritten every pass, when three passes run, then no full walk runs and the file is still held.
- Given a held path deleted before its second look, when the pass runs, then it is forgotten without error.

## Verification

**Commands** (with `/opt/homebrew/bin` first on `PATH`: the `/usr/local/bin` git is 2.23 and lacks `git init -b`, which fails unrelated fixture tests):
- `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-sync sweep held settle second_look` -- expected: adapted and new tests pass.
- `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-sync` -- expected: all green.
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-sync --all-targets -- -D warnings && cargo fmt --manifest-path src-tauri/Cargo.toml -p keeper-sync -- --check` -- expected: clean.

## Suggested Review Order

**The second look — the pass samples what its walk could not report**

- Entry point: held paths the walk did not report, sorted into sample / held / collapsed — the index probe, git's exclusion, the stat that fails closed.
  [`engine.rs:2211`](../../src-tauri/crates/keeper-sync/src/engine.rs#L2211)
- What the sort answers with, and why three buckets.
  [`engine.rs:1643`](../../src-tauri/crates/keeper-sync/src/engine.rs#L1643)
- The gate's export taken once and handed in; the sampled paths join the gate loop as additions, the held ones stay held, the collapsed ones are reported.
  [`engine.rs:9777`](../../src-tauri/crates/keeper-sync/src/engine.rs#L9777)
- A path inside a nested repository is git's to ignore, so the second look does too.
  [`engine.rs:2359`](../../src-tauri/crates/keeper-sync/src/engine.rs#L2359)

**What no longer widens**

- The guard and the `Settling`-untracked flag are gone; held paths ride the narrowed walk as includes.
  [`engine.rs:2085`](../../src-tauri/crates/keeper-sync/src/engine.rs#L2085)
- The watcher's and the prime's flag still buy a full walk — for paths nobody has held yet.
  [`engine.rs:2049`](../../src-tauri/crates/keeper-sync/src/engine.rs#L2049)

**The record**

- §4: the second look is the pass's own sample, and what it costs.
  [`sync.md:182`](../../docs/sync.md#L182)
- §21: the sweep row, its remaining log lines, and the race that is the only way to the unreadable-index line.
  [`sync.md:3280`](../../docs/sync.md#L3280)
- The deferred entry closed, and the one this story leaves (`prime_moved_paths`).
  [`deferred-work.md:6024`](./deferred-work.md#L6024) · [`deferred-work.md:6069`](./deferred-work.md#L6069)

**Tests**

- A sweep-found file commits on a narrowed second pass — no directory walk between the looks, and it lands as `Added`.
  [`engine.rs:25896`](../../src-tauri/crates/keeper-sync/src/engine.rs#L25896)
- Still being written: held across three passes, no full walk, commits when the writer stops.
  [`engine.rs:25998`](../../src-tauri/crates/keeper-sync/src/engine.rs#L25998)
- Gone before its second look: forgotten.
  [`engine.rs:26133`](../../src-tauri/crates/keeper-sync/src/engine.rs#L26133)
- A parent that lost its permission: held with one warn, the pass succeeds, commits once it is back.
  [`engine.rs:26255`](../../src-tauri/crates/keeper-sync/src/engine.rs#L26255)
- A later `.gitignore`, or a nested repository: not staged, forgotten or reported collapsed.
  [`engine.rs:26344`](../../src-tauri/crates/keeper-sync/src/engine.rs#L26344) · [`engine.rs:26412`](../../src-tauri/crates/keeper-sync/src/engine.rs#L26412)
- A held path the index carries is the walk's business.
  [`engine.rs:26494`](../../src-tauri/crates/keeper-sync/src/engine.rs#L26494)
- Five hundred held paths' second look inside a second — the Ask-First's evidence.
  [`engine.rs:26569`](../../src-tauri/crates/keeper-sync/src/engine.rs#L26569)
- The restart whose first walk was the poll's, now without any widening.
  [`engine.rs:26753`](../../src-tauri/crates/keeper-sync/src/engine.rs#L26753)
