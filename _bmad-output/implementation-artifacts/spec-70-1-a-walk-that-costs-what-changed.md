---
title: 'Story 70.1: a walk that costs what changed'
type: 'feature'
created: '2026-09-09'
status: 'done'
baseline_revision: '3cb3fd7'
final_revision: '766a11f'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
---

binds: FR-513, FR-514, FR-515, NFR-61 → AD-227; AD-34-11 (the wake survives its tick — kept), AD-45 (tier 2 is the gate's, the walk only observes — kept)
depends on: 70.5's `StabilityGate::holds_until_ms` (added on request; the one per-path deadline accessor the wake floor reads)

<intent-contract>

## Intent

**Problem:** hesperia walked `tgdrive` 1 371 times in one session, `scanned=155626` every time, 666 times in one hour while a recording ran (`review-sync-2026-09-08/lanes/scan.md`, caller trace). ~600 of those per hour were `Engine::path_durability` calling `git::repo::status_paths` — `WalkPolicy::read_only()`, dirwalk **on**, no claim, no floor — from the recording banner's 1 Hz poll with no in-flight guard. The rest were watcher wakes: `scan_due` is true on every tick while a file is being written, and the only pacing is the tick. The one signal that would make a walk proportional to the change — the watcher's paths, which `fold_watch_events` computes and index-checks per event — was thrown away into a per-profile boolean. `index.skipHash` was unset, so every walk SHA-1'd 26.4 MB, twice when it wrote back. The `status walk finished` line could not say who asked or what the 26/65 entries were.

**Approach:** four cuts, each at the layer that owns the fact.
1. `path_durability` answers from the index entry, one `lstat` and the HEAD tree entry — no walk, no claim. The comparison is the one gix itself makes first (`Stat::matches` under the repository's `stat_options`); a stat that has moved is answered *not committed* without hashing, because a wrong yes is the answer that loses a recording and a wrong no is floored by the surface.
2. `WalkPolicy` gains `include: Vec<PathBuf>` (repository-relative, spelled `:(literal)<path>` beside the `:(exclude,literal)` ones) and `read_only_tracked()`. gix narrows the index range by the includes' common prefix and skips the rest by string match before any syscall.
3. The watcher's paths are kept per profile in `WatchWake { paths, overflow, due }`, capped at `WATCH_PATHS_CAP = 4096`; overflow, a root/rescan event, an unnamed wake (`wake_now`), a path outside the folder or under `.git`, the first pass of a run, a degraded watcher and the untracked sweep all mean *full walk*. The walk that answers the wake consumes the set; the gate's held paths are added to the includes so a settling path is re-observed and `gate.retain` does not forget it.
4. Two floors on event-driven walks: a path the gate holds with a future deadline does not set the wake (that deadline already schedules the walk), and between two wake-driven walks at least `min(effective_settle_ms, 5 s)` passes; a wake inside the floor is kept, not dropped.
Plus `index.skipHash=true` beside `index.sparse=false`, three fields on the walk line (`caller`, `untracked`, `needs_update`), and an in-flight guard on the client poll.

## Boundaries & Constraints

**Always:**
- `path_durability` consumes no walk claim and moves no counter; every existing outcome (present / modified / missing / unreadable → last known state stands) keeps its answer.
- The unreadable memo still speaks: a path `status_paths` had to step over answers "unknown" here too, by asking `still_unreadable` about that one path.
- An include list is only ever handed to a `tracked_only`/`read_only_tracked` walk; a `full()` walk never carries one (an untracked sweep that is narrowed is not a sweep).
- The set is consumed by the walk that answers it; a set nobody consumes (claim refused, checkout unfinished) survives to the next walk, exactly as the boolean wake does today.
- Every held gate path is in the includes of a narrowed walk, or `gate.retain(&observed)` would forget it.
- `clear_watch_wake` keeps the paths and clears only `due`; `note_watch_wake(profile_id)` (unnamed, `wake_now`) marks `overflow` because a wake that names nothing must look at everything.
- `WalkPolicy::full()/tracked_only()/read_only()` stay `const fn` and equal to their old shapes (`include` empty).

**Block If:** nothing; the contract named every shape and 70.5 answered the one accessor question.

**Never:** edit `stability.rs`, `commit.rs`, the gate loop below the walk in `collect_stable_changes`, `record_failure`, or `git/cli.rs`; hash a recording on the 1 Hz path; drop a wake because a floor held it; reformat either file.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| One wake, big index | 10 000 tracked entries, watcher names `dir/f5000.txt` | one walk, `scanned ≤ 2`, `include = [that path]` | — |
| Overflow | `WATCH_PATHS_CAP + 1` distinct paths in one batch | one `tracked_only()` walk with no includes | — |
| Root / rescan event | event path == profile root | `overflow`, no includes | — |
| Unnamed wake | `wake_now` after named paths | `overflow`; walk covers whole index | — |
| Durability, present | committed segment, untouched | `committed=true`, `status_walks` unchanged | — |
| Durability, modified | segment appended after commit | `committed=false`, no claim | — |
| Durability, missing | segment deleted after commit | `committed=false`, no claim | — |
| Durability, unreadable | path in the unreadable memo | `committed=false` (last known state stands) | warn once |
| Settling path wakes again | gate holds `x` with `holds_until_ms > now`, event on `x` | `scan_due` false; `x` in the set | — |
| Two wakes 1 s apart | wake, walk, +1 s, wake | second `scan_due` false; at +5 s true (kept) | — |
| Floor vs settle | `effective_settle_ms = 2 s` | floor is 2 s, not 5 s | — |
| Fresh config | `enforce_local_config_with_filter` on a new clone | `index.skipHash=true` beside `index.sparse=false` | — |
| Walk line | any walk | `caller=`, `untracked=`, `needs_update=` present | — |
| Client poll | `recordingStatus()` still pending at the next tick | tick skipped; next tick after resolve fires | — |

</intent-contract>

## Code Map

| Path | Change |
|---|---|
| `src-tauri/crates/keeper-sync/src/git/repo.rs` | `WalkPolicy` gains `include` + `read_only_tracked()` + `including()`; `WalkCaller`; `status_paths`/`status_paths_reported`/`status_paths_excluding` take the caller, emit `:(literal)` includes, log `caller`/`untracked`/`needs_update`; `persist_observed_stats` takes the outcome; `enforce_local_config_with_filter` sets `index.skipHash`; `unreadable_memo_holds` for `path_durability` |
| `src-tauri/crates/keeper-sync/src/engine.rs` | `WATCH_PATHS_CAP`, `WAKE_WALK_MIN_INTERVAL`, `WatchWake`, `watch_paths` field; `fold_watch_events` keeps paths and applies the gate floor; `note_watch_wake`/`watch_wake_pending`/`clear_watch_wake`/`take_watch_paths`; `poll_walk_policy`/`commit_walk_policy` hand includes; `path_durability` direct answer; callers name themselves |
| `src-tauri/crates/keeper-sync/src/git/history.rs` | caller name only (`dirty_paths` reads `untracked`, so it keeps the dirwalk) |
| `src/hooks/use-recording-session.ts` | in-flight ref on the 1 Hz poll |
| `src/hooks/use-recording-session.test.ts` | the guard's test |

## Tasks & Acceptance

- [x] `path_durability` without a walk — `path_durability_takes_no_walk_claim_and_answers_present_modified_missing`.
- [x] `WalkPolicy::include` + `read_only_tracked()` — `an_include_pathspec_narrows_a_ten_thousand_entry_walk_to_the_named_path`, `a_read_only_tracked_walk_neither_walks_directories_nor_writes_the_index`.
- [x] `WatchWake` set, cap, overflow, consumption — `a_wake_naming_one_path_walks_one_entry_of_ten_thousand`, `overflowing_the_path_cap_produces_one_whole_index_walk`, `an_unnamed_wake_and_a_paced_walk_both_widen_the_list_to_the_whole_index`.
- [x] The two floors — `a_wake_for_a_settling_path_does_not_set_scan_due`, `two_wakes_one_second_apart_produce_one_walk`, `the_wake_floor_is_capped_by_the_settle_window`.
- [x] `index.skipHash` — `enforce_local_config_writes_skip_hash_so_a_walk_stops_hashing_the_index` (also proves git and gix both read the null-trailer index gix writes under it).
- [x] Walk line fields — `caller`, `untracked`, `needs_update`, `included` on `status walk finished`; `RepoStatus.scanned` carries the cost to callers.
- [x] Client in-flight guard + test — `use-recording-session.test.ts` (2 tests).

**Acceptance Criteria:**
- One wake on 10 000 entries ⇒ `scanned ≤ 2`: **Met** — both the wake's walk and the settle-deadline walk read `scanned=1`, and the settled file is committed by the second; `status_walks == 3` (sweep, wake, deadline).
- Overflow ⇒ one full walk: **Met** — 4 100 events, one walk, `scanned == 4100`, nothing owed after.
- `path_durability` takes no claim, answers present/modified/missing (and unreadable): **Met** — `status_walks` unchanged across all four.
- A settling path's wake does not set `scan_due`: **Met** — and a close-write on it still does.
- Two wakes 1 s apart ⇒ one walk: **Met** — the second is held through t=4 and opens the walk at t=5.
- `index.skipHash=true` in a freshly enforced config: **Met**.
- Frontend skips a tick while one is in flight: **Met**.

## Design Notes

**Why the gate's held paths ride along.** `collect_stable_changes` calls `gate.retain(&observed)` after the walk; `observed` is every path the walk reported. A narrowed walk that did not include a settling path would report nothing for it and the gate would forget the episode — the same bug `a_cadence_that_wakes_lets_a_file_settle_where_one_that_rescans_never_can` was written against. Reading `gate.export()` costs one clone per held entry, and the gate holds only what is mid-episode.

**Why `path_durability` does not hash on a stat miss.** gix would; but the question is asked at 1 Hz about a file that may be a gigabyte behind the LFS clean filter, and the surface floors a false "not committed" while nothing floors a false "committed". The racy-git window (a same-size rewrite inside the second the index was written) is accepted for the same reason.

**Why the poll consumes the set too, and only the set.** The contract names both policies; the poll is floored at one walk a minute, so at worst the commit leg pays one whole-index `tracked_only` walk per minute for a wake the poll already answered. The poll never narrows to the gate's held paths alone — it is a listing, and with nothing named it looks at the whole index.

**Why the paced backstop widens the list.** `scan_due` widens the list when the paced window fires: the backstop's job is the paths the watcher never saw, and a backstop narrowed to the watcher's own list could not do it. Only event-driven walks (wake, settle deadline) are narrowed.

## Verification

- `cargo test -p keeper-sync --lib -- git::repo::tests engine::tests` → 283 passed (siblings' in-flight tests included at the time). The story's own: `cargo test -p keeper-sync --lib -- a_wake_naming_one_path overflowing_the_path_cap path_durability_takes_no_walk a_wake_for_a_settling_path two_wakes_one_second the_wake_floor_is_capped an_unnamed_wake_and_a_paced enforce_local_config_writes_skip_hash a_read_only_tracked_walk an_include_pathspec_narrows` → 10 passed. Integration: `--test status_names_the_cause --test index_repair --test gitignore_is_respected --test lfs_pointer_stat` → all green. `cargo check -p keeper-sync --all-targets` and `cargo check -p keeper-syncd` clean. `bunx vitest run src/hooks/use-recording-session.test.ts` → 4 passed.
- One pre-existing test adjusted: `the_git_directorys_contents_are_filtered_but_the_root_and_the_git_node_are_not` now advances the clock past `WAKE_WALK_MIN_INTERVAL` between its two back-to-back wakes — the floor is the behaviour it was tripping on.

| Mutation | Test that failed |
|---|---|
| (1a) `path_is_as_committed` returns `Ok(true)` regardless of the stat | `path_durability_takes_no_walk_claim_and_answers_present_modified_missing` |
| (1b) `path_durability` takes `claim_walk` | same (counter assertion) |
| (3a) `fold_watch_events` drops the path instead of `named.push` | `a_wake_naming_one_path_walks_one_entry_of_ten_thousand`, `a_wake_for_a_settling_path_does_not_set_scan_due`, `an_unnamed_wake_and_a_paced_walk_both_widen_…` |
| (3b) `WatchWake::name` never overflows | `overflowing_the_path_cap_produces_one_whole_index_walk` |
| (4a) gate floor removed (`held = false`) | `a_wake_for_a_settling_path_does_not_set_scan_due` |
| (4b) floor clock never started (`last_walk_ms = None`) | `two_wakes_one_second_apart_produce_one_walk` |
| (5) `index.skipHash` written `false` | `enforce_local_config_writes_skip_hash_so_a_walk_stops_hashing_the_index` |
| (7) client guard removed | `skips a tick while the previous recordingStatus() is still in flight` (5 calls instead of 2) |

Each mutation was one line, reverted by the same `sed`, and the line re-read afterwards; final run of all story tests green.

**Not verified here, and why.** The hesperia number (walks/hour on `tgdrive` during a recording) is 70.8's install step. `keeper` shell crate not compiled (Linux); this story touches no shell-crate code. The poll's narrowing is exercised only through `poll_walk_policy` (no `pending()`-driven test) — `pending()` already has coverage of its walk and the policy function is shared with the commit leg.


### Addendum (2026-09-09, the macOS gate)

On hesperia both narrowing tests failed with `scanned=10000`: with `core.ignoreCase` gix adds the `icase` magic to every pathspec (`gix/src/status/index_worktree.rs:191-195`, no override) and `gix-pathspec`'s common prefix for an `icase` pattern is the empty prefix directory, so the index *range* is not narrowed on a case-insensitive volume — every entry is visited and rejected by a string match before its `lstat`. The `lstat`s, the cost that dominated, are still saved. Both tests now assert what each platform can promise (`git::repo::walks_case_insensitively`); `WalkPolicy::include`'s doc and `docs/sync.md` §19 say so.
