---
title: 'Story 70.5: the gate keeps its promises'
type: 'feature'
created: '2026-09-09'
status: 'done'
baseline_revision: '3cb3fd7'
final_revision: '766a11f'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-70-a-pass-that-costs-what-changed-and-a-folder-that-says-when-it-is-cut-off.md'
  - '{project-root}/_bmad-output/planning-artifacts/review-sync-2026-09-08/lanes/gate-watch.md'
---

binds: FR-524, FR-525 → AD-232; AD-45 (four tiers, none decides alone — tiers 3 and 4 put on the commit path), AD-48 (absence is not deletion — closed across the walk)
lane findings: F-GATE-1, F-GATE-2, F-GATE-3, F-GATE-7, F-GATE-8, F-GATE-14
depends on: nothing in wave A; 70.1 owns the walk call above the gate loop and the `untracked_appeared` flag this story sets one line into

<intent-contract>

## Intent

**Problem:** The gate's safety argument (`stability.rs:1-19`) is that tiers 0–3 may be approximations *because tier 4 is a proof*. On the commit path tier 4 did not exist: `commit.rs:241` read a non-LFS blob with a bare `std::fs::read`, `open_writer_veto` had no caller at all, and `verify_while_reading` ran only in the audit verb (F-GATE-2). Three more promises were written and not kept: a "finished" or "primed" entry was `Stable` for life because `observe` never moved `pending_since_ms` off the backdated anchor (F-GATE-3); `forget_all`, documented for detach, was called by its own unit test only, so an entry that straddled a detach was past the 60 s ceiling the instant the drive came back (F-GATE-8); nothing re-checked the volume between a walk that takes up to 61 s on hesperia and the commit that trusts it, and the only mass-deletion guard fired for an *empty* index — inert for the 155 626-entry index a yanked drive leaves behind (F-GATE-1). `prime_moved_paths` primed destinations the next `tracked_only` walk could not observe, and `gate.retain` pruned them (F-GATE-7). `docs/sync.md` §4 promised "optionally open file descriptors" for a tier that reads `/proc/locks` and nothing else (F-GATE-14).

**Approach:** Put each promise where it is kept, and make the record say what runs.
- `observe` restarts the episode (`pending_since_ms = now`) on a changed sample. The ceiling now bounds how long the *current* bytes are held; an assertion or prime clears exactly once, for the bytes it was made about.
- `is_stable` asks tier 3 of every path tier 2 clears (Linux; a constant `false` elsewhere). A vetoed path keeps its entry, restarts its unchanged run, and stays `Settling`.
- `stability::read_verified(path, approved)` is tier 4 for the staging read: `fstat`-read-`fstat` on the open descriptor, plus a refusal before the first byte when the descriptor does not match the sample the gate cleared the path on. `StagedChange` carries that sample (`samples`), recorded by the walk beside `sizes`. `stage_and_commit` reads every non-LFS blob through it; the strict form fails the commit on `Integrity`, the engine's `stage_and_commit_skipping_torn` leaves the path out, lists it, and commits the rest. The engine warns once through the sticky `warn` with `TORN_READ_SENTENCE`, records activity only for what the commit holds, and the next walk commits the path once it is quiet.
- `tick_profile`'s detach arm calls `forget_settle_windows`: `gate.forget_all()` plus `db::clear_file_state`, on every absent tick (a PK-prefix `DELETE` that matches nothing after the first), so a restart while the drive is out also comes back clean.
- `collect_stable_changes` re-asserts the marker (`volume::find_mount_root`, one `is_file` per ancestor) after the walk and before anything the walk said is kept, returning `MediaAbsent` if it is gone. `stage_and_commit` refuses a change set on a removable profile whose deletions exceed `MASS_DELETION_FRACTION = 0.5` of the index, with `MASS_DELETION_PREFIX N MASS_DELETION_SENTENCE` as a `SyncError::Diverged` (Permanent → NeedsAttention: a folder that needs a look, not a retry loop). The empty-index guard is kept.
- `prime_moved_paths` sets `untracked_appeared` when it primed anything.
- §4 names the real call sites of tiers 3 and 4 and what the ceiling now measures.

**Why `Diverged` and not a new variant.** `error.rs` is 70.2's region this wave; `Refused` carries a per-path `ContentRefusal` from `lfs/hydrate.rs` (70.4's) and grows two exhaustive matches in the shell; `Config` renders "invalid sync configuration: …" in front of a sentence about a drive. `Diverged { profile, reason }` renders `<profile>: <reason>`, is Permanent, and its doc already says "a human decision is the point" — which is exactly this. Its doc comment's "bidirectional profiles never produce this" is now one sentence too strong; flagged to the coordinator rather than edited in a file this story does not own.

**Why the ceiling changes meaning, and what that costs.** With the anchor moving on every change, a file rewritten between every walk is never forced through — before, it was committed once a minute as whatever torn snapshot the walk found, and F-GATE-2 meant nothing caught it. The cost is that a forever-appended log commits only when its writer pauses for a window. AD-232 takes that trade explicitly ("never again for new bytes"); tier 4 on the staging read is what would refuse the torn snapshot anyway. Two existing tests that encoded the old contract are re-authored, not deleted, and say what is now true.

## Boundaries & Constraints

**Always:**
- Every user-visible sentence is a `const` in `git/commit.rs` (`MASS_DELETION_PREFIX`, `MASS_DELETION_SENTENCE`, `TORN_READ_SENTENCE`) and formatted, never retyped.
- Tier 4's refusal never fails the engine's pass: torn paths are skipped, warned once (sticky, edge-triggered toast), and retried by the next walk.
- Tier 3 is paid per settled file, never per walk, and only on Linux.
- The post-walk volume check runs before `save_file_state`, so a vanished volume mirrors nothing.
- The mass-deletion guard runs before the index write, like the empty-index guard beside it, and only when `profile.removable`.
- `StagedChange` gains a field; no call site of `stage_and_commit` changes signature.

**Block If:** nothing; every design question was answered by the epic's decisions or a neighbouring pattern (`rescan`'s clear, `warn`'s edge, `verify_while_reading_hooked`'s test hook).

**Never:** touch `WalkPolicy`, the walk call, `fold_watch_events`, `error.rs`, `commit_local`, `conflict.rs`, `lfs/*`; run a project-wide gate; `cargo fmt`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Torn between verdict and read | `collect_stable_changes` clears `notes.md`; file rewritten; `commit` | `notes.md` absent from the commit, the quiet sibling committed, card warns `notes.md <TORN_READ_SENTENCE>`, no activity row for it; next pass after a window commits it | `Integrity` → skipped, not fatal |
| Torn twice | second torn pass | warning still on the card, exactly one toast | sticky `warn` |
| Only torn paths | change set = torn | `Ok(None)`, no commit, no empty root commit on an unborn branch | — |
| Strict caller | any non-engine caller, stale sample | `Err(Integrity)`, nothing committed | fails the commit |
| Asserted then appended | `note_finished`, append, `is_stable` | `Settling`, counted in `tracked` | — |
| Asserted and untouched | `note_finished`, `is_stable` | `Stable` on the first look | — |
| Changed sample late in an episode | observe at 0, changed at 59 s, verdict at 60 s | `Settling`; `next_stable_ms` = last change + ceiling | — |
| `flock(LOCK_EX)` held (Linux) | zero window, child holds the lock | `Settling`; `Stable` on the look after release | skips if `flock(1)` absent |
| Marker vanishes during the walk | removable, files removed, hook deletes marker after the walk | `Err(MediaAbsent)`, HEAD unchanged, `file_state` empty | Deferred → `MediaAbsent` |
| 10 % deleted, removable | 1 of 10 | committed (1) | — |
| 66 % deleted, removable | 6 of 9 | `Err(Diverged)` with `keeper will not record 6 deletions from a drive that may have been unplugged; recheck the folder`; HEAD and index unchanged | Permanent → NeedsAttention |
| Exactly 50 % | 2 of 4 | committed — the threshold is "more than" | — |
| Same deletions, fixed disk | `removable = false` | committed | — |
| Prime | `prime_moved_paths` primed ≥ 1, live watcher, sweep spent | next `commit_walk_policy` is `full()`; a prime of nothing buys nothing | — |
| Detach / re-attach | episode open, marker removed, one tick, marker back 120 s later | `file_state` empty after the tick; first `is_stable` is `Settling` | — |

</intent-contract>

## Code Map

| Path | Change |
|---|---|
| `src-tauri/crates/keeper-sync/src/stability.rs` | module doc tiers 3/4 (:11-23); `observe` restarts `pending_since_ms` (:273-315); docs on `prime_stable` (:317-357), `note_finished` (:381-391), `declare_settled` (:415-431), the ceiling comment in `verdict` (:477-489); `holds_until_ms` (:546-559, asked for by 70.1); tier 3 in `is_stable` (:666-693); `forget_all` doc (:704-709); `open_writer_veto` doc (:789-791); `read_verified` / `read_verified_hooked` / shared `read_guarded` (:936-1036); tests re-authored (`a_file_that_never_quiesces_is_held_until_its_writer_pauses`, `a_file_that_never_quiesces_is_scheduled_one_ceiling_after_its_last_change`) and added (`a_changed_sample_restarts_the_ceiling`, `an_asserted_path_appended_to_afterwards_is_settling_on_its_next_look`, `a_write_locked_file_is_refused_by_tier_3_until_the_lock_is_released` (linux), `read_verified_returns_the_bytes_and_refuses_a_file_that_moved`) |
| `src-tauri/crates/keeper-sync/src/git/commit.rs` | `StagedChange.samples` (:108-120) and `without` (:134-160); `MASS_DELETION_FRACTION`, `MASS_DELETION_PREFIX`, `MASS_DELETION_SENTENCE`, `TORN_READ_SENTENCE` (:170-193); `stage_and_commit` → thin strict wrapper, new `stage_and_commit_skipping_torn`, body in `stage_and_commit_inner` (:195-281); the mass-deletion guard beside the empty-index guard (:334-361); the staging read through `read_verified` with the torn arm (:400-426); the post-loop `without` and empty check (:486-503); tests `a_mass_deletion_is_refused_on_removable_media_only_and_never_staged`, `a_path_whose_sample_no_longer_matches_is_torn_and_handled_by_the_form_asked_for` |
| `src-tauri/crates/keeper-sync/src/engine.rs` | `tick_profile` detach arm (:3495-3508); `forget_settle_windows` beside `drop_watcher` (:4306-4321); `prime_moved_paths` sets `untracked_appeared` (:4385-4395); `collect_stable_changes`: `after_walk::fire` + post-walk volume check (:7175-7196), samples recorded beside sizes (:7307-7319); `commit`: `stage_and_commit_skipping_torn`, the sticky warning, `staged.without(&torn)` before activity rows (:7555-7599); `after_walk` test seam module before `mod tests`; tests `a_file_rewritten_after_the_verdict_is_skipped_with_one_warning_and_committed_next_pass`, `a_volume_that_vanishes_during_the_walk_is_absence_and_commits_nothing`, `a_mass_deletion_on_a_removable_profile_is_refused_and_a_small_one_commits`, `priming_moved_paths_makes_the_next_walk_a_full_one`, `a_detach_forgets_the_settle_windows_so_a_reattach_starts_fresh` |
| `docs/sync.md` | §4: tier 3/4 rows, "Where tiers 3 and 4 actually run", the ceiling row and paragraph, the tier-4 gap bullet |

Line numbers are as of this writing in a worktree four other agents are editing; `engine.rs` numbers drift.

## Tasks & Acceptance

**Execution:**
- [x] `observe` restarts the episode; docs at `prime_stable`/`note_finished`/`declare_settled`/`verdict` say what is true.
- [x] Detach arm forgets the gate and the rows.
- [x] Tier 4 on the staging read, torn paths skipped and warned once, activity rows honest.
- [x] Tier 3 from `is_stable` on Linux.
- [x] Post-walk volume re-check → `MediaAbsent`.
- [x] Mass-deletion refusal with the sentence; empty-index guard kept.
- [x] `prime_moved_paths` sets the flag (field name confirmed with WalkCost).
- [x] §4.

**Acceptance Criteria (epic 70.5):**
- Given a file rewritten between `Stable` and the staging read, when the pass commits, then it is skipped with one warning and committed on the next pass. **Test:** `a_file_rewritten_after_the_verdict_is_skipped_with_one_warning_and_committed_next_pass`.
- Given a path asserted finished and then appended, when observed, then `Settling`. **Test:** `an_asserted_path_appended_to_afterwards_is_settling_on_its_next_look`.
- Given a Linux `flock`, when tier 2 clears the path, then tier 3 refuses. **Test:** `a_write_locked_file_is_refused_by_tier_3_until_the_lock_is_released`.
- Given a marker that vanishes between walk and commit, then `MediaAbsent` and nothing committed. **Test:** `a_volume_that_vanishes_during_the_walk_is_absence_and_commits_nothing`.
- Given 60 % deletions on a removable fixture, then refused with the sentence; 10 % committed. **Tests:** `a_mass_deletion_on_a_removable_profile_is_refused_and_a_small_one_commits`, `a_mass_deletion_is_refused_on_removable_media_only_and_never_staged`.
- Given `prime_moved_paths` on a fresh destination, then the next walk is `full()`. **Test:** `priming_moved_paths_makes_the_next_walk_a_full_one`.
- Given a detach and re-attach, then the first observation is `Settling`. **Test:** `a_detach_forgets_the_settle_windows_so_a_reattach_starts_fresh`.

## Design Notes

**The approved sample is taken a moment after the verdict, not at it.** `is_stable` forgets the entry on `Stable`, and the lstat that fills `sizes` already runs after the gate lock is dropped, deliberately (every other profile's scan queues behind that lock). The sample is recorded there. The window between the verdict and that lstat is microseconds inside one pass; a change inside it is caught by the `fstat` pair around the read like any other.

**Torn paths are skipped inside `stage_and_commit`, not pre-read by the engine.** Reading every non-LFS blob up front into `substitutions` would hold every staged file in memory at once (a 10 000-file first sync at up to 256 KiB each) and would leave the bare read in place for every other caller. The strict/skipping split keeps the 25 existing call sites unchanged and makes the skip an explicit choice of the one caller that has a next pass.

**The warning names one path.** A card holds one sentence; the rest are `info!` lines with the path, and the sticky edge means a recurring torn read toasts once until a unit succeeds.

**`commit_local`'s count is not corrected for torn paths.** It is 70.3's region this wave, and `commit()` returns `()`; the progress bar's final frame and `SyncOutcome::files_changed` may overstate by the torn count on a pass that skipped something. Noted for the coordinator; the activity rows and the commit itself are exact.

**Tier 3 is outside `stable_at_ms`.** A lock cannot be predicted, so `tracked`/`next_stable_ms` do not see it; the veto restarts `unchanged_since_ms` so the deadline moves a window out rather than sitting in the past and buying a walk per tick. With a zero window (the flock test) that is nothing, which is why that test does not assert `tracked`.

**The after-walk seam is a `cfg(test)` module-level map keyed on the profile root**, not a field on `Engine`: the struct and its constructor are shared regions, and a one-shot hook keyed on the tempdir cannot fire for a parallel test.

## Verification

- `cargo check -p keeper-sync --all-targets` → clean (once siblings' in-flight edits settled). `cargo check -p keeper-syncd` → clean. `cargo clippy -p keeper-sync --all-targets` → no findings in `stability.rs`, `git/commit.rs`, or the `engine.rs` regions this story owns (one `too_many_arguments` on the eighth-argument public form, allowed like its inner twin).
- `cargo test -p keeper-sync --lib -- stability:: git::commit:: a_file_rewritten_after_the_verdict a_volume_that_vanishes_during_the_walk a_mass_deletion_on_a_removable_profile priming_moved_paths_makes_the_next_walk a_detach_forgets_the_settle_windows priming_a_moved_file` → 67 passed.
- Whole lib suite at the time of the run: 1184 passed, 6 failed — all six in sibling regions mid-edit (`fold_watch_events`/wake floor, fetch deadline/offline counter, `cli` parse), none in this story's tests or files.
- The `flock` test ran for real on this box (util-linux `flock(1)` present; `/proc/locks` showed the child's `FLOCK ADVISORY WRITE`).

| Mutation | Tests that failed | Observed |
|---|---|---|
| (1) `observe`: drop `entry.pending_since_ms = now_ms` | `a_file_that_never_quiesces_is_held_until_its_writer_pauses`, `a_file_that_never_quiesces_is_scheduled_one_ceiling_after_its_last_change`, `a_changed_sample_restarts_the_ceiling`, `an_asserted_path_appended_to_afterwards_is_settling_on_its_next_look` | 38 passed; 4 failed |
| (3) staging read: `read_verified(&absolute, None)` — approved sample ignored | `a_path_whose_sample_no_longer_matches_is_torn_and_handled_by_the_form_asked_for`, `a_file_rewritten_after_the_verdict_is_skipped_with_one_warning_and_committed_next_pass` | 19 passed; 2 failed |
| (3′) tier 3: `if false && open_writer_veto(..)` | `a_write_locked_file_is_refused_by_tier_3_until_the_lock_is_released` | 0 passed; 1 failed |
| (5) post-walk volume check: `if false && profile.removable && …` | `a_volume_that_vanishes_during_the_walk_is_absence_and_commits_nothing` — failed with `Diverged { … "keeper will not record 3 deletions …" }`: the belt caught what the brace no longer did | 0 passed; 1 failed |
| (6) mass-deletion guard: `if false && profile.removable` | `a_mass_deletion_is_refused_on_removable_media_only_and_never_staged`, `a_mass_deletion_on_a_removable_profile_is_refused_and_a_small_one_commits` | 0 passed; 2 failed |

Each restore was made by re-issuing the original line and confirmed by the editor's snapshot tag returning to its pre-mutation value (`#9B59` for `stability.rs`, `#D5D0` for `git/commit.rs`) and by a grep for the exact restored line; the scoped run above is the post-restore run.

**Not verified here, and why.** Nothing in this story touches the `keeper` shell crate, so nothing was left uncompiled. The tier-4 skip with a *real* concurrent writer was not exercised by a thread race — the deterministic seam (`collect_stable_changes` → rewrite → `commit`) drives the same two functions, and the read-window half is proven by `read_verified_hooked`; a thread test on a fixture this small would be flaky by construction. The post-walk volume check was proven through the `cfg(test)` after-walk seam, not by a physical unplug; on hesperia the exact gix behaviour when a mountpoint disappears mid-walk (ENOENT per entry vs a hard failure) is still the open question the lane recorded, and the mass-deletion guard is what covers the ENOENT-per-entry shape either way. `commit_local`'s progress count on a pass that skipped a torn path is not corrected (70.3's region); the commit, the tree, the activity rows and the warning are exact.
