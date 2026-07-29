---
title: 'A kill during a reference update must not strand a folder'
type: 'bugfix'
created: '2026-07-29'
status: 'review'
baseline_revision: '483d316'
---

<intent-contract>

## Intent

**Problem:** A `SIGKILL` inside the first few milliseconds of a sync leaves debris in `.git` that
nothing on any recovery path knows about, and the folder then never syncs again — not after a
retry, not after a reboot, not ever, until a human deletes a file they have no reason to know
exists. Three distinct states do it, all found by the durability matrix, all the failure NFR-24
forbids.

1. **An abandoned reference lock.** `gix::Repository::commit_as` publishes a commit through a
   reference transaction over `HEAD`, which locks `.git/HEAD` and then the branch `HEAD` resolves
   to. A kill inside that window leaves `HEAD.lock` or `refs/heads/<branch>.lock` on disk with
   nobody holding it, and **which one decides which of two symptoms the user gets**:
   - `HEAD.lock` — the transaction's *root* edit — fails cleanly, so every later pass exits 1 with
     `git object store failure: commit failed: A lock could not be obtained for reference "HEAD"`.
     This is the CI failure on `release/v0.6.3` (PR #21, macos-latest): twelve changes committed
     locally, `left: 0` published, forever.
   - `refs/heads/<branch>.lock` — a *child* edit, produced by dereferencing `HEAD` — does not fail
     at all. `gix-ref` 0.66.0 builds the reference name for a failed acquisition by walking the
     edit's parent chain and never advances its cursor once it reaches the root
     (`store/file/transaction/prepare.rs:398-410`), so the call becomes an infinite loop on a full
     core with no error and no output. **This is the ninety-six-minute hang** that
     `spec-34-9`'s follow-up recorded as unexplained and that `.config/nextest.toml` was written to
     bound. One root cause, two faces.
2. **A `.git` that is not a repository yet.** `adopt` begins with `gix::init`, which is a sequence
   of filesystem steps: `info/`, `hooks/`, `objects/`, `refs/`, then `HEAD`, then `config`. A kill
   inside it leaves a `.git` directory that exists and will not open. `Engine::open_repo` then
   takes its "repository already exists" branch forever — precisely *because* `.git` exists — never
   calls `adopt` again, and fails every sync with `does not appear to be a git repository` or
   `could not read .git/config`. Exactly the shape `ensure_remote` already repairs, one field over.
3. **A convergence deadline the engine never promised.** `STALE_INDEX_LOCK` is deliberately sixty
   seconds, but `durability_matrix`'s `sync_to_completion` asserts convergence inside about one, so
   a kill that left an `index.lock` failed the sweep on a timing accident rather than a defect.

**This predates epic 34.** `commit_as(…, "HEAD", …)` has been the commit path since `a4b25e4`, the
first sync-engine commit, and that revision's `repo.rs` contains zero occurrences of `.lock`.
Story 31.2 (`73cdf42`) added `index.lock` recovery and stopped there. `git log -S 'HEAD.lock'` and
`-S 'ref_lock'` over `src-tauri/` return nothing at any revision. Epic 34's entire diff to `repo.rs`
is seven lines inside `mod tests`, and `Cargo.lock` has not moved gitoxide since `a4b25e4`. Epic 34
changed *timing* around the commit path — `SyncSource` threading, the staging progress sink, 246
added lines in `commit.rs` — which is a plausible reason the race started landing, and no part of
the gap.

**Approach:** Extend the recovery that already exists at `git::repo::open` — the one that clears an
abandoned `index.lock` — to loose reference locks and to unfinished inits, keeping each recovery's
discrimination rule matched to what the thing being recovered actually is. Reference locks are
judged by **watching** rather than by age. Unfinished inits are judged by a **whitelist** of what
`gix::init` writes, and refuse loudly on any sign of history. Separately, `stage_and_commit`
declines to enter a reference transaction somebody else is already in, because for a held branch
lock gitoxide's failure mode is a hang rather than an error and no recovery may take a live
writer's lock.

## Boundaries & Constraints

**Always:** A lock is only debris once it has been observed doing nothing for the whole window, so
the removal is never a decision made on sight. Every repair is logged at `warn` with the path it
touched — a silent repair of somebody's git directory is a surprise, not a repair. Every recovery
is best-effort: a removal that races another process leaves the sync to fail on its own terms
rather than turning a cleanup into a hard error. `discard_unfinished_init` logs its intent *before*
it deletes, because if the call is ever wrong that line is the only evidence left. The user's own
files are never involved in any of this; only `.git` is touched.

**Block If:** (none — every decision was derivable from the evidence, gitoxide's sources and the
existing `index.lock` precedent.)

**Never:** Do not remove a `.lock` unconditionally. Do not widen `core.filesRefLockTimeout` (see
Design Notes — it provably cannot help). Do not shorten `STALE_INDEX_LOCK`; sixty seconds is
protecting a live writer of a real index and shortening it trades this bug for a worse one. Do not
touch `packed-refs.lock`. Do not touch `.config/nextest.toml` — it is the only bound on the residual
upstream defect. Do not delete a `.git` that holds one object, one reference, one `index`, one
`logs/`, or anything else `gix::init` does not write.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| kill inside the ref transaction | abandoned `.git/HEAD.lock` | watched for 2 s, released, `warn` names it; the same pass commits and publishes | best-effort; a failed removal warns and the pass fails normally |
| kill inside the ref transaction | abandoned `refs/heads/<branch>.lock` | same; the pass that used to spin forever now recovers in ~2.1 s | as above |
| kill during a fetch | abandoned `refs/remotes/origin/<branch>.lock` | same rule, same window — the recovery is per loose reference, not per branch | as above |
| human `git commit` in the folder | lock present and being written | identity changes on the first poll (≤25 ms); recovery returns, lock untouched | commit refused; transient, retried |
| human `git commit` finishes mid-watch | lock disappears | recovery returns at once, removes nothing | none |
| a second writer takes the lock mid-watch | lock replaced, different identity | left alone — a watch that started before it existed may not collect it | none |
| live writer holds the *branch* lock | `refs/heads/main.lock` held across the pass | `stage_and_commit` refuses before calling gitoxide; no spin, no hot core | transient `SyncError::Git`; next tick retries |
| live writer holds `HEAD.lock` | root edit would fail | same refusal, for consistency rather than necessity | as above |
| `git gc` / `git pack-refs` running | `packed-refs.lock` held for minutes | out of scope by design; never inspected, never removed | none |
| clock jumped backwards or forwards | any lock mtime | irrelevant — the window is our own elapsed time, not the file's timestamp | none |
| `.git/refs` unreadable | permissions | the walk skips it; nothing is removed | none |
| kill before `gix::init` wrote `HEAD` | `.git` with scaffold only | discarded, re-adopted in the same pass, `warn` at both ends | if removal fails, the io error propagates |
| kill after `HEAD`, before `config` | scaffold + `HEAD` | same | same |
| a real repository that will not open | `HEAD` deleted, objects present | **refused**, loudly; the caller's original error stands | folder stays stranded and says so |
| `.git` is a `gitdir:` file | linked worktree or submodule | refused — not a directory, not an init of ours | as above |
| `.git` holds `index` / `logs/` / `packed-refs` / `ORIG_HEAD` / `modules/` / `worktrees/` | any one of them | refused; each is pinned by a test row | as above |
| interrupted clone left objects | partial pack in `objects/` | refused — a partial pack is indistinguishable from history | the existing empty-remote path already clears its own partial `.git` |
| kill leaves `index.lock` | fresh lock, real user or debris | unchanged: left alone for 60 s, then released. Production behaviour is deliberately not modified | the test ages the lock instead |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/git/repo.rs`
  - `STALE_REF_LOCK` (2 s) and `REF_LOCK_POLL` (25 ms).
  - `open` now also calls `release_stale_ref_locks`, beside the existing `release_stale_index_lock`.
  - `release_stale_ref_locks` / `release_ref_locks_unheld_for` / `loose_ref_locks` /
    `release_ref_lock_if_abandoned` / `lock_identity` — the watch, and the safety argument.
  - `reference_lock_path` / `ensure_head_unlocked` — the pre-commit refusal.
  - `INIT_SCAFFOLD` / `discard_unfinished_init` / `signs_of_real_history` / `first_entry_under` —
    the unfinished-init recovery and its whitelist.
  - Ten new tests.
- `src-tauri/crates/keeper-sync/src/git/commit.rs` — `stage_and_commit` calls
  `repo::ensure_head_unlocked` immediately before `commit_as`. Six lines, one comment.
- `src-tauri/crates/keeper-sync/src/engine.rs` — `open_repo`'s `.git`-exists branch becomes
  `match self.open_existing_repo(…)`, falling through to adoption when
  `discard_unfinished_init` clears a half-made directory; the body it used to hold moves verbatim
  into the new `open_existing_repo`.
- `src-tauri/crates/keeper-syncd/tests/durability_matrix.rs` — `Peer::sync_once`,
  `Peer::age_any_index_lock` (called once at the top of `sync_to_completion`), and three new tests.
- **Unchanged on purpose:** `STALE_INDEX_LOCK`, `.config/nextest.toml`, `Cargo.lock`.

## Tasks & Acceptance

**Execution:**
- [x] `repo.rs` — release loose reference locks at `open`, judged by a watched window rather than by
  mtime. — Closes the strand; matches the `index.lock` precedent's shape, discipline and log
  register.
- [x] `repo.rs` + `commit.rs` — refuse to start a reference transaction while a lock it needs is
  held. — Turns gitoxide's infinite loop into a deferred pass, on the user's machine and not only
  in CI.
- [x] `repo.rs` + `engine.rs` — discard a `.git` that is provably an unfinished `gix::init` and
  adopt again in the same pass; refuse, loudly, on any sign of history. — Closes failure modes (1)
  and (2) of the interrupted-adopt family that `ensure_remote` half-covered.
- [x] `durability_matrix.rs` — age an `index.lock` a kill left before driving the profile to
  completion, and say why in full. — Stops the sweep asserting a deadline production never promised
  without weakening the production rule.
- [x] Tests — ten in `repo.rs`, three in `durability_matrix.rs`, including the two that fail without
  the fix and the four that fail if somebody "simplifies" the safety rules away.

**Acceptance Criteria:**
- Given a folder killed mid-reference-update, when a later pass runs, then it publishes without
  human intervention and the log names the lock it released.
- Given a reference lock a live writer holds, when a pass runs, then the lock is left exactly as it
  was, nothing is published, and the daemon returns instead of spinning.
- Given a `.git` that holds any history at all, when it will not open, then it is never removed and
  the refusal names its evidence.
- Given a `.git` that is an unfinished init, when a pass runs, then it is re-created and the user's
  files are byte-identical afterwards.
- Given the durability matrix run thirty times, then it is green thirty times.

## Design Notes

**Widening the timeout provably cannot fix this, and I checked.** gitoxide already retries
acquisition with quadratic backoff bounded by `core.filesRefLockTimeout`, which defaults to 100 ms
for loose references and 1000 ms for `packed-refs` (`gix-0.86.0/src/config/cache/access.rs:229`);
keeper sets neither, so those defaults are what runs. Backoff is the right answer for a lock a live
writer holds and *no* answer for one whose owner is dead: there is nobody left to release it, so
every timeout, however generous, expires against the same file. Raising it converts "fails at once,
forever" into "hangs first, then fails, forever", and setting it to `-1` (infinite) converts it into
a hang with no end at all. This is why the fix is a recovery and not a knob.

**Why a watch and not an age.** The `index.lock` rule reads the file's mtime and calls anything
older than sixty seconds debris. That is right for an index — real writers genuinely hold one for
seconds, and you cannot stall a sync for a minute to find out — but it is wrong here for two
reasons. First, sixty seconds of latency would not have satisfied the durability matrix, whose six
passes run inside one second; a rule that only recovers after the test is over is a rule the test
cannot defend. Second, an mtime is a claim by whichever machine wrote the file, and on removable
media (AD-48) that is routinely not this one. Watching costs our own measured time and trusts no
clock: the lock is polled every 25 ms for two seconds, and released only if it was neither rewritten
nor let go while we looked. The asymmetry is the point — a live writer costs one poll, an abandoned
lock costs the full window, and the full window is paid once per crashed repository.

**Why two seconds.** git publishes its own bound: `core.filesRefLockTimeout` defaults to 100 ms,
which is how long git is willing to wait for a loose reference lock somebody else holds before
giving up. That is git stating that a live holder releases in a fraction of a second — as it must,
since publishing a loose reference is a 41-byte write and a rename. Two seconds is twenty times
that, with room for a filesystem having a bad day.

**And if the judgement is ever wrong, it cannot corrupt anything.** A reference is published by
renaming its lock over it. A writer whose lock we removed fails its own `rename` with `ENOENT` and
reports an error, while the reference keeps the value it already had. A torn or half-written
reference is not a state this can produce. That is the bound on being wrong, not the licence for
it — the watch is the licence.

**What keeper cannot know, and what git does instead.** The lock file names no owner: no pid, no
host, nothing to ask whether the holder still exists. keeper cannot assume it is the only writer
either — these folders are meant to be usable with plain `git`, and there is no single-instance
guard, no pid file and no unclean-shutdown marker anywhere in the daemon. git never removes a stale
lock automatically and is right not to: git is run by a person at a terminal, so it can print
"another git process seems to be running" and let them decide. keeper is a daemon syncing a folder
nobody is looking at. There is no one to read the message, which is the whole reason NFR-24 exists.

**`packed-refs.lock` is deliberately out of scope.** It is the one reference lock a legitimate
operation holds for a long time — `git gc` and `git pack-refs` hold it while rewriting every
reference in the repository, which is not bounded by the 100 ms above — and keeper's own commit and
fetch paths never create one, because `commit_as` writes a loose reference even when the branch is
packed. Nothing observed has ever stranded on it. Covering it would put a guess on the same footing
as a measurement, so a test pins the exclusion instead.

**The recovery is at open, not at the commit.** That is where `release_stale_index_lock` and
`ensure_remote` already live, and it is the right place for the same reason: a kill's debris is a
property of the repository, discovered once when it is opened, not a condition to re-litigate on
every write. It also means the lock is gone *before* `commit_as` is reached, which is what keeps
keeper out of the gitoxide loop below in the case that actually strands folders.

**The pre-commit refusal is a decision to attempt, not a second lock.** `ensure_head_unlocked` runs
in the instant before the reference transaction opens — as late as possible, because that is when
its answer is most nearly still true — and only declines to call gitoxide when it can already see
the call is pointless. The check-then-act window is real and is not closed: a writer can still take
the lock between the check and gitoxide's acquisition. What it removes is the whole *duration* of
somebody else's hold — seconds for a person typing a commit message, unbounded for debris — leaving
a window a few microseconds wide. Closing it entirely would need gitoxide to expose its own
acquisition, which it does not. A refused pass is a normal outcome: nothing is staged away, the
working tree still holds the change, and `SyncError::Git` is already classified `Transient`
precisely for "a momentarily locked file", so the scheduler retries after backoff and a human's
commit is over long before the sticky three-failure warning could fire. No new error variant: no
surface reacts differently to this, and the taxonomy's own rule forbids a variant that no surface
distinguishes.

**Discarding a half-made `.git` is finishing keeper's init, never repairing a repository.** Being
wrong here deletes somebody's history, so nothing is assumed. Every name at the top level must be
one `gix::init` itself writes (`INIT_SCAFFOLD`, taken from `gix::create::into`), and `refs/` and
`objects/` — the only two places history can be — must contain nothing but the empty directories it
leaves. One reference, one object, one `index`, one `logs/`, one `packed-refs`, a `modules/`, a
`worktrees/`, or a `.git` that is a file pointing into another repository, and it refuses and says
what it saw. Anything that cannot be *established* counts as a sign too: "I could not read it" and
"there is nothing there" must never collapse into the same answer when the answer authorises a
deletion. A stranded folder that says so is recoverable by a human; a deleted history is not.
Recovery is then "remove and re-init" rather than "write the missing `HEAD` and `config`", so
`gix::init` stays the only implementation of `gix::init`.

**`STALE_INDEX_LOCK` is unchanged, and the test moved instead.** The sweep failed roughly one run
in thirty on a fresh `index.lock`, and the tempting fix — shorten the threshold — would trade a
recoverable stall for a corrupted index, because sixty seconds is what protects a real `git add` on
a large tree. The engine is entitled to that minute; the test was asserting it away. `sync_to_completion`
now backdates a lock a kill left, once, before any pass runs, which is the passage of time and not a
weakened rule: the daemon still has to notice the lock and clear it. Reference locks are pointedly
*not* aged, so the sweep exercises the new watch exactly as a user's machine would.

**Upstream defect, filable as written.** gitoxide `gix-ref` 0.66.0 (via `gix` 0.86.0, pinned in
`src-tauri/Cargo.lock`), `src/store/file/transaction/prepare.rs:398-410`:

```rust
let mut cursor = change.parent_index;
let mut ref_name = change.name();
while let Some(parent_idx) = cursor {
    let parent = &updates[parent_idx];
    if parent.parent_index.is_none() {
        ref_name = parent.name();      // `cursor` is never advanced here
    } else {
        cursor = parent.parent_index;
    }
}
```

When the root is reached, `cursor` keeps its value and the loop never terminates. Reproduction, with
no keeper code involved: `gix::init` a repository, make one commit, plant an empty
`.git/refs/heads/<branch>.lock`, and call `commit_as(…, "HEAD", …)` again — it never returns (killed
at 15 s, exit 124, one core at 100 %). Planting `.git/HEAD.lock` instead returns
`Err("A lock could not be obtained for reference \"HEAD\"")` in 134 ms, because that edit is the root
and the loop body never runs. This change removes the only observed trigger but not the defect; a
live writer holding a branch lock across our 100 ms backoff can still reach it, which is what
`ensure_head_unlocked` narrows and what `.config/nextest.toml` bounds. Tracked as **DW-120**.

## Verification

**Run here, on this Linux box.** `cargo build`/`clippy` for the `keeper` crate does not work here
(tauri needs GTK), so the desktop crate was not built; nothing in this change touches it, and no
`SyncError` variant was added precisely so `to_ipc_error` would not need to.

- `cargo test -p keeper-sync -p keeper-syncd` — **495 passed, 0 failed** (395 `keeper-sync` lib
  tests, up from 386; 6 `durability_matrix`, up from 3; the rest unchanged).
- `cargo clippy -p keeper-sync -p keeper-syncd --all-targets` — clean, no warnings.
- `cargo fmt -p keeper-sync -p keeper-syncd -- --check` — clean.

**Both directions proved by disabling the fix, not by assertion.**

- With `release_stale_ref_locks` unwired, `a_reference_lock_left_by_a_kill_does_not_strand_a_folder`
  fails with the verbatim CI text: `git object store failure: commit failed: A lock could not be
  obtained for reference "HEAD"`.
- With `ensure_head_unlocked` unwired, `a_branch_lock_a_live_writer_holds_never_hangs_the_daemon`
  fails after 30.72 s having had to kill a daemon that was still spinning.
- `a_reference_lock_a_live_writer_is_using_is_left_alone`,
  `a_reference_lock_is_watched_before_it_is_broken`, `a_packed_refs_lock_is_never_touched`,
  `a_repository_holding_history_is_never_discarded` and
  `every_sign_of_a_finished_repository_refuses_the_discard` are the guards against a later
  simplification; each fails if the corresponding rule is loosened.

**Durability matrix, thirty runs each under `timeout 240`, `--test-threads=4`.**

| Build | Green | Failing test(s) |
|-------|-------|-----------------|
| baseline (recoveries unwired) | 1 / 30 | ref-lock recovery 29/30; sweep 2/30; LFS 1/30 |
| ref-lock recovery only | 25 / 30 | sweep 4/30, orphan 1/30 — unfinished init ×4, `index.lock` ×1 |
| this change, complete | 30 / 30 | none |

The middle row is why modes (2) and (3) are in this change at all: fixing the reference lock alone
left a gate that failed one run in six, which is a gate people learn to re-run.

**Per-test timings, against the nextest guard.** Measured with `--test-threads=1` on this box, five
runs of the sweep: `a_kill_at_any_instant_costs_no_data_and_corrupts_no_index` 8.22–8.53 s (its
range on the CI runner was 12.05–21.54 s before this change, and the recoveries add nothing to a
pass that finds no debris); `a_reference_lock_left_by_a_kill_does_not_strand_a_folder` 2.29 s — the
2 s window plus a pass; `a_branch_lock_a_live_writer_holds_never_hangs_the_daemon` 0.89 s;
`a_reference_lock_a_live_writer_holds_is_left_alone` 0.77 s. Nothing approaches the 60 s warn, let
alone the 240 s kill, so `.config/nextest.toml` needs no adjustment and gets none. The 8–38 s spread
in the thirty-run table above is four-way parallelism on a box shared with other work, not any
single test.

**Manual reproduction, on the daemon rather than in a test.** Register a profile, sync once, add a
file, plant `.git/refs/heads/main.lock`, and run `keeper-syncd sync --once`. Before: 100 % of one
core, no output, no exit — killed at 180 s. After: recovers in 2.1 s with

```
WARN keeper_sync::git::repo: released a reference lock left behind by a killed run
  lock=…/work/.git/refs/heads/main.lock watched_ms=2000
INFO keeper_sync::engine: committed profile="dur" files=1
```

and the file reaches the remote.

**Not covered, and honest about it.** The check-then-act window in `ensure_head_unlocked` is bounded
but not closed (Design Notes). The gitoxide loop itself is upstream and unfixed; `.config/nextest.toml`
is the only bound on a recurrence in CI and must stay as it is. The `packed-refs.lock` case is
excluded by argument, not by evidence that it cannot happen. And while the macOS runner is where all
three symptoms were first seen, every measurement above is from Linux — the reasoning is
platform-independent (POSIX rename semantics, gitoxide's own sources) but the timings are not.
