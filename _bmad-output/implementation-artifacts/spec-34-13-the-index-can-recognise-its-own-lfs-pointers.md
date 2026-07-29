---
title: 'The index can recognise its own LFS pointers'
type: 'bugfix'
created: '2026-07-29'
status: 'review'
baseline_revision: '88452c1'
---

<intent-contract>

## Intent

**Problem:** `lfs::stage::indexed_pointer` returned `None` for every real LFS file. It asked
whether the entry could be a pointer by testing `entry.stat.size > MAX_POINTER_BYTES`, and an LFS
entry's stat is **the worktree file's, on purpose** — that is the whole hinge of AD-46, spelled out
at `git/commit.rs:193-199` ("the blob is the ~130-byte pointer while the worktree keeps the real
bytes. The entry's stat below is still taken from the WORKTREE file") and written at
`git/commit.rs:222` as `Stat::from_fs(&metadata)` over the *worktree* `lstat`. So the number the
function tested was the gigabytes the pointer stands in for, never the ~130 bytes actually staged,
and the test rejected precisely the entries the function exists to recognise. A function whose
whole job is "is this an LFS entry?" answered "no" for every LFS entry and "no" for everything
else, which is why nothing ever looked wrong.

The consequence is one dead guard. `Engine::collect_stable_changes` gates the racily-clean case on
`indexed_pointer` returning `Some`, so for LFS paths the guard never ran: an entry that git re-read
because its mtime was not older than the index reported the worktree's bytes as differing from the
pointer blob, keeper called that an edit, and the next commit pass re-cleaned the whole file —
hashing gigabytes a second time — to write back the pointer it already had. Story 34.6 met the same
mis-diagnosis from the other side and sidestepped it: `removed_size` reads the *blob's* header
length instead of the entry's stat, and its Design Note ("Why the index blob and not the index's
stat block for a deletion") is the correct reading of the same fact.

**Approach:** Ask the blob. `pointer_blob` reads the object header — `find_header(...).size()`, no
object body loaded — and parses only when the blob itself is under `MAX_POINTER_BYTES`. That is
`removed_size`'s approach reused rather than a second one invented; the two differ only in what
they answer for a non-pointer (a size versus `None`).

Turning the guard back on then required reading it, because it had never run in production. It was
not correct for LFS entries. `is_false_modification` compared the worktree file's **length** to the
pointer's `size`, which mistakes two other states for the racily-clean one, so both were fixed here
rather than shipped newly-reachable (see Design Notes). The guard now asks the three questions that
actually separate "git re-read it and found the design" from "git re-read it and found work": the
worktree's stat still matches the entry's, the staged blob is a pointer, and `HEAD` already records
that same blob.

## Boundaries & Constraints

**Always:** The stat comparison uses `gix::index::entry::Stat::matches` with the repository's own
`stat_options()` (`core.trustCtime`, `core.checkStat`, …), because that is the identical comparison
`gix::status` just made — the guard has to be the exact inverse of the decision it is second-
guessing, not a lookalike. It runs first, before anything touches the object database, so an
ordinary modified file (whose stat has moved) costs one `lstat` and nothing else. Every failure to
read — no index entry, unreadable config, missing file, unborn branch — answers "this is a real
modification", so the path is staged rather than dismissed.

**Block If:** (none)

**Never:** Do not read `entry.stat.size` to decide whether a blob is a pointer; it is the worktree
file's size and always will be — that is what makes `gix::status` fast on an LFS repository. Do not
suppress a modification for an ordinary (non-pointer) blob: for those, a racily-clean re-read
finding different bytes is a real edit caught inside the race window. Do not compare the worktree's
length to the pointer's `size` as the discriminator. Do not touch `git/repo.rs`, `keeper/src/ipc.rs`
or the front end — other Epic 34 agents own them.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| LFS entry | blob = 133-byte pointer, stat.size = 200 000 | `indexed_pointer` → `Some(pointer)` | — |
| Ordinary large file | blob = 200 000 bytes | `None`, decided from the header; body never loaded | — |
| Ordinary small file | blob = 5 bytes, not a pointer | `None` after a parse that fails | — |
| Path not in the index | untracked or already staged away | `None` | no entry → `None` |
| Racily clean pointer | stat matches, index mtime ≤ entry mtime | `is_false_modification` → true; the path is not re-staged | — |
| Racily clean pointer, working `filter.lfs.clean` | keeper-syncd host | git cleans the re-read back into the pointer and calls the path unchanged; the guard is never consulted | — |
| Edit that changes length | worktree grew or shrank | stat differs → false → staged | — |
| Edit at identical length | same byte count, new bytes, new mtime | stat differs (mtime) → false → staged | — |
| Ordinary file, racily clean | non-pointer blob, real edit in the race window | `pointer_blob` → `None` → false → staged | — |
| Pointer staged ahead of `HEAD` | crash between the index write and the commit | `head_records` → false → staged, so the next pass re-drives it (NFR-24) | — |
| Unborn branch | nothing committed yet | `head_tree()` errs → false → staged | Err ⇒ not dismissed |
| File deleted since the scan | worktree path gone | `lstat` fails → false → staged | Err ⇒ not dismissed |
| Unreadable `core.*` stat config | malformed value | `stat_options()` errs → false → staged | Err ⇒ not dismissed |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/lfs/stage.rs` -- new private `index_key` (`:172`) and
  `pointer_blob` (`:185`); `indexed_pointer` (`:197`) now asks `pointer_blob`; `is_false_modification`
  (`:230`) rewritten and re-signed as `(repo, rela, absolute)`; new private `head_records` (`:260`).
  The length-only unit test it superseded was removed.
- `src-tauri/crates/keeper-sync/src/engine.rs` -- the guard in `collect_stable_changes` (`:2067-2076`)
  collapses to `else if !lfs::stage::is_false_modification(&repo, rela, &absolute)`; new test
  `a_pointer_staged_ahead_of_head_survives_the_scan_and_is_committed` (`:3945`).
- `src-tauri/crates/keeper-sync/tests/lfs_roundtrip.rs` -- new fixtures `commit_pointer_for_clip`,
  `backdate_index_to`, `advance_mtime`, `register_working_clean_filter`, and four tests
  (`:290-494`).
- Read but deliberately unchanged: `git/commit.rs:193-222` (the worktree-stat decision this bug
  misread) and `engine.rs:2333` (`removed_size`, whose approach is reused — its own contract differs,
  so the two stay separate functions).

## Tasks & Acceptance

**Execution:**
- [x] `stage.rs` -- `pointer_blob`: `find_header(blob).size()` against `MAX_POINTER_BYTES`, then
  `Pointer::parse` on the body. -- The blob's own length decides, and nothing bigger is loaded.
- [x] `stage.rs` -- `indexed_pointer` drops the `entry.stat.size` test and delegates. -- A real LFS
  entry is recognised.
- [x] `stage.rs` -- `is_false_modification` takes the repository and the relative path, and answers
  from three checks in cost order: `Stat::from_fs(lstat).matches(entry.stat, repo.stat_options())`,
  then `pointer_blob`, then `head_records`. -- Only the racily-clean case is dismissed.
- [x] `stage.rs` -- `head_records`: `HEAD`'s tree entry for the path must name the staged blob. --
  A staged-but-uncommitted pointer is re-driven, not stranded.
- [x] `engine.rs` -- the guard becomes one condition; the comment explains what re-staging would
  cost rather than restating the code.
- [x] Tests -- `lfs_roundtrip.rs`: a committed pointer whose entry stat is 200 000 is recognised
  (and the test asserts that stat, so the mis-diagnosis cannot come back unnoticed) while a
  200 000-byte ordinary blob, a 5-byte non-pointer and an unknown path are not; a backdated index
  makes the entry racily clean and the guard dismisses it, then an edit at identical length is not
  dismissed; a pointer swapped into the index ahead of `HEAD` is not dismissed; and the same
  fixture run with and without a working `filter.lfs.clean` shows which of the two configurations
  the guard is load-bearing in. `engine.rs`: the call site honours a `false` answer end to end
  through `collect_stable_changes` / `commit_local`.

**Acceptance Criteria:**
- Given a path committed through LFS, when `indexed_pointer` reads its index entry, then it returns
  the staged pointer even though the entry's stat reports the multi-gigabyte worktree file.
- Given a 200 000-byte ordinary blob, when `indexed_pointer` reads it, then it returns `None`
  without loading the blob body.
- Given an LFS entry that git re-read only because it is racily clean, when the scan runs, then the
  path is not staged and the file is not hashed again.
- Given an LFS file edited in place to exactly the same byte count, when the scan runs, then the
  edit is staged and committed.
- Given an LFS pointer written to the index but not yet committed, when the scan runs, then the path
  is staged, so the commit the crash interrupted is re-driven.

## Design Notes

**The original mis-diagnosis, recorded so nobody re-introduces it.** `entry.stat.size` and the size
of the blob the entry names are the same number for every file in the repository *except* the ones
LFS exists for. Reading the stat is the obvious move, it compiles, it is fast, and on an ordinary
file it is even right — the mistake only shows up on the exact input the code was written to
handle, and it fails silently, as `None`, which is also the correct answer for the common case. The
guard it disabled had no symptom of its own beyond a large file being hashed twice, so nothing ever
pointed at it. If a future change wants a cheap pre-filter here, the only honest cheap number is the
blob's header length; the entry's stat is, by AD-46's design, a fact about the worktree.

**What the fix turns back on, and why the guard needed fixing before it ran.** With `indexed_pointer`
answering truthfully, `collect_stable_changes` reaches `is_false_modification` for LFS paths for the
first time. Read as it stood, the guard was a length comparison — worktree `len()` against the
pointer's `size` — and it is wrong for LFS entries in two ways, both newly reachable:

1. **An in-place edit that preserves the byte count would be dropped, permanently.** The guard's own
   doc comment claimed "the caller only reaches here for a path whose stat tuple the completeness
   gate already saw as unchanged". The completeness gate does not say that. `StabilityGate::is_stable`
   says the file *stopped changing* (two identical samples a settle window apart); it never compares
   anything to the index. So a settled same-length edit passes the gate, matches the pointer's
   `size`, is dismissed as a re-read — and is dismissed again on every later scan, because git keeps
   reporting it. The user's work would never be committed. That is a data-loss-class outcome, and
   the guard's length check could not distinguish it even in principle.

2. **A pointer staged but not yet committed would be stranded.** `stage_and_commit` writes the index
   *before* the commit deliberately, so a crash between the two leaves work the next pass re-drives
   (`git/commit.rs:267-269`, NFR-24). The re-drive happens only because `status_paths` reports the
   path — `Item::TreeIndex` modifications land in the same `modified` bucket as index-versus-worktree
   ones — so dismissing it would leave a committed-nowhere pointer until the file changed again.

Both are fixed here rather than left as a finding, because both are small and both are caused by
this change becoming live. The replacement discriminator is not a heuristic: `gix::status` reports a
path unchanged exactly when `new_stat.matches(&entry.stat, options)` holds and the entry is not racy
(`gix-status-0.33.0/src/index_as_worktree/function.rs:472-484`). Asking the same question with the
same options therefore answers "was raciness the only reason it spoke up?" precisely, and for a
pointer entry the differing content the re-read then found is the design. Anything else — a changed
mtime, size, ctime, inode — is a genuine touch and is staged.

**Where the residual ambiguity is.** `Stat::matches` compares mtime *seconds* unless
`core.checkStat` asks for nanoseconds, so an edit landing in the same wall-clock second as the stat
the index recorded is invisible to it. That is git's own resolution limit, not one this guard
introduces, and two things make it unreachable in practice: the entry's stat is taken after the
file has been quiet for a full settle window (5 s by default) and `StabilityGate` samples mtime and
ctime in **nanoseconds**, so a second edit inside that second cannot settle. Being wrong in the
other direction is cheap by construction: a guard that declines to dismiss costs one re-clean, whose
identical pointer leaves the tree unchanged and produces no commit.

**Third finding: which configuration the guard is alive in, and the comment that says otherwise.**
Measured, not reasoned (`a_working_clean_filter_settles_the_racily_clean_case_before_the_guard_does`,
and confirmed against plain `git` as well as gitoxide). A racily-clean LFS entry is *not* reported
modified when `filter.lfs.clean` works: git re-reads the worktree, cleans it back into the pointer
the index already holds, and calls the path unchanged — the guard is never consulted. It is only
reported modified when the filter is absent or fails, because `enforce_local_config_with_filter`
sets `filter.lfs.required = false` and a filter that fails is a filter that is not there.

`Engine::open` registers `std::env::current_exe()` as that filter and its comment claims "`current_exe`
is the daemon in a CLI run and the app binary in a desktop run; **both** understand `lfs clean|smudge`"
(`engine.rs:342-344`). The second half is false: `keeper-syncd` implements `lfs clean`
(`commands.rs:1621`), and `crates/keeper/src` contains no `lfs` subcommand at all — grep finds
`lfs_mode` and `lfs_threshold_bytes` in `sync_ipc.rs` and nothing else. So on desktop the clean
filter fails on every invocation, the raw bytes are hashed, and this guard is the only thing
standing between the user and a full re-clean of every LFS file after every commit. On a
`keeper-syncd` host it is dead code.

That is a real defect and it is **not fixed here**: the fix is either the app binary learning the
subcommand or `enforce_local_config_with_filter` refusing to register a program that cannot serve —
and that function lives in `git/repo.rs`, which another agent owns in this batch. It is recorded
rather than silently absorbed, because the guard's value depends entirely on it.

**Why `removed_size` was not merged with `pointer_blob`.** They share four lines and no contract.
`removed_size` answers "how big was the file that used to be here", so a non-pointer blob answers
with its own length; `pointer_blob` answers "is this a pointer", so a non-pointer answers `None`.
Folding them would need a caller-side flag to pick which meaning was wanted, which is worse than the
duplication. `index_key` — the forward-slash conversion both need — is now a named function in
`stage.rs`; `removed_size` keeps its own copy with the comment that already points at
`indexed_pointer`, because that file belongs to a different story in this batch.

## Verification

**Ran here (this slice owns `keeper-sync`, which compiles on this box):**
- `cargo test -p keeper-sync` — **396 lib tests, 8 `lfs_roundtrip`, 1 `lfs_pointer_stat`, 1
  `gitignore_is_respected`, 1 `index_refresh`, all passing, and no output on stderr.**
- `cargo fmt -p keeper-sync -- --check` — clean.
- **The tests were proven to fail before the fix, not merely to pass after it.** With
  `indexed_pointer`'s `entry.stat.size` test temporarily restored on top of everything else, three
  `lfs_roundtrip` tests fail: recognition returns `None`
  (`an_indexed_pointer_is_recognised_…`), the path stops being seen as LFS-tracked
  (`a_racily_clean_pointer_…`), and the staged-ahead pointer is unrecognised
  (`a_pointer_staged_ahead_of_head_…`). With the engine guard temporarily inverted (dropping the
  `!`), the engine test fails — so it genuinely exercises the call site rather than passing on the
  strength of the rest of the pipeline.
- **The engine test's scope is deliberate and stated in its own doc comment.** It covers only the
  guard's `false` answer. Reaching the `true` answer needs a racily-clean entry, and git only gets
  there by re-reading the worktree *through* `filter.lfs.clean` — which under `cargo test` is the
  libtest harness, since `Engine` registers `current_exe()`. An earlier draft did drive that state
  and printed two `error: Unrecognized option: 'repo'` lines from the harness into a green run
  while proving nothing about the filter. The test now uses a `HEAD`-to-index difference, which git
  reports from objects alone, and explicitly ages the index past the worktree file so that a whole
  test finishing inside one wall-clock second cannot make the entry racy by accident. The guard's
  decision, in both filter configurations, is covered in `tests/lfs_roundtrip.rs`, which registers
  the filter itself.
- Not run, per the batch constraint: anything that builds the `keeper` crate (needs GTK here),
  `cargo clippy`, and the front-end suites — this slice touches no TypeScript and no generated IPC
  type.

**Checked by reading:**
- `gix-index-0.54.0/src/entry/stat.rs:17-30` (`is_racy`), `:38-76` (`matches`), `:86-119`
  (`from_fs`, and the deliberate 32-bit truncation of `size`) — the definitions the guard now
  depends on.
- `gix-status-0.33.0/src/index_as_worktree/function.rs:462-535` — the racily-clean branch: a stat
  match plus a non-racy entry returns "unchanged" and returns early; a racy one falls through to a
  full content comparison. This is what makes the inverted question exact.
- `gix-status-0.33.0/src/index_as_worktree/traits.rs:100-119` (`FastEq`) — its size shortcut
  compares `entry.stat.size` to the worktree length, and for an LFS entry those are the *same*
  number, so it never fires: the comparison always falls through to `HashEq`, which streams the
  file through `filter.lfs.clean`. That is why even an edit at identical length runs the filter,
  and why the filter's presence is what decides whether the guard is reached at all.
- `gix-index-0.54.0/src/file/init.rs:94-97` — the index `timestamp` is the index file's own
  mtime, which is what `backdate_index_to` manipulates to reach the racily-clean state a test cannot
  wait for.
- `gix-0.86.0`: `Repository::head_tree` (`repository/reference.rs:298`), `Repository::stat_options`
  (`repository/config/mod.rs:53`, `index`-gated and the crate already uses `index`),
  `Tree::lookup_entry_by_path` (`object/tree/mod.rs:175`) and `Entry::object_id`
  (`object/tree/entry.rs:32`).
- Every caller of the two changed signatures was re-greped across `src-tauri` after the edits:
  `is_false_modification` has exactly one production call site (`engine.rs:2069`) and
  `indexed_pointer` none outside tests and `stage.rs` itself. No other crate names either.
- `lfs::pointer::Pointer::parse` already refuses `bytes.len() >= MAX_POINTER_BYTES`
  (`pointer.rs:113-115`), so the header check is a *cost* guard, not a correctness one — the
  "large ordinary blob" test asserts the contract (`None`), and cannot observe the avoided read.
