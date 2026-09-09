---
title: 'Story 70.3: a merge that does not finish is undone, and the matrix decides'
type: 'bug-fix'
created: '2026-09-09'
status: 'in-progress'
baseline_revision: '3cb3fd7'
review_loop_iteration: 0
followup_review_recommended: true
context:
  - '{project-root}/docs/project-context.md'
---

binds: FR-519, FR-520 → AD-229; AD-43 (a modification always beats a deletion — made real)
source: `review-sync-2026-09-08/lanes/pullpush.md` F-PULLPUSH-1 (P0), -2, -11, -13; the measured conflict matrix and the git command table.

<intent-contract>

## Intent

**Problem:** Measured against keeper's exact merge vector, modify/delete, rename-vs-modify, file/directory and case-only divergences all exit 1 and leave `MERGE_HEAD`; every later `merge` and `merge --ff-only` then exits 128, forever, with `SyncError::GitCommand` classified `Transient` so the profile retries a state that cannot change. `write_tree_from_index` skips unmerged entries, so a commit taken over that index silently drops the contested path. `conflict::resolve` — the whole AD-43 matrix — has zero production callers. Conflict copies are untracked until a later pass picks them up, overwritable within one second, and named through `to_string_lossy`.

**Approach:** Three guards and one caller. (1) A merge verb that exits for any reason other than a resolvable conflict runs `git merge --abort` before returning. (2) `commit_local` aborts a `MERGE_HEAD` an old keeper left and refuses if it is still there — never tree-builds over an unmerged index. (3) `converge_with_conflict_copies` classifies every contested path from `diff --name-status` into `ChangeKind` for both sides, calls `conflict::resolve`, writes a copy only where the verdict is `ConflictCopy`, merges with `--no-commit`, resolves whatever `-X theirs` could not (`checkout --theirs|--ours` + `add`, driven by the same matrix from the index stages), stages the copies, commits once, and aborts if anything is still unmerged. `git` never sees a path on argv: pathspecs travel through `--pathspec-from-file` with `--pathspec-file-nul`, byte-faithful and unbounded by `ARG_MAX` (the folder that motivated `-X no-renames` had 138 311 contested paths).

## Boundaries & Constraints

**Always:**
- `merge_ff_only` and `merge_theirs` leave no `MERGE_HEAD` behind on a failure that is not a resolvable conflict; `merge_abort` treats "There is no merge to abort" as success.
- `commit_local` runs `merge_abort` once when `MERGE_HEAD` exists and refuses with `MERGE_IN_PROGRESS_SENTENCE` (`SyncError::Config`, `Permanent`) if it survives.
- A contested path's verdict is `conflict::resolve(local, remote)`; a copy is written only for `ConflictCopy`, only when the tips actually differ, only when the path is not `regenerable`.
- A copy is created with `create_new`; a collision gets `-2`, `-3`, …; the name is built from the file name's bytes, never `to_string_lossy` (`names.rs` doctrine: a lossy rendering is fine to *show* and must never be used to *reach*).
- Copies, resolved paths and rescue copies are staged and land in the merge commit, with keeper's provenance trailers in the message.
- An unmerged entry whose path is git's `<path>~HEAD[_n]` / `<path>~<ref>[_n]` rescue (file/directory, distinct-types) becomes a conflict copy of the canonical path and is reported as a `conflict` activity row; the rescue litter is not left in the tree.
- If anything is unmerged after resolution: `merge_abort`, copies written this pass removed, `SyncError::Diverged` naming the first path.
- `keeper-sync` stays `tauri`-free and `keeper-core`-free (AD-40).

**Block If:** nothing; every design question was answered by measuring git 2.53 in a throwaway repository (`/tmp/m70/probe*.sh`).

**Never:** pass a repository path on `git`'s argv; commit over an unmerged index; leave `MERGE_HEAD` after a failed pass; make a copy for a path where one side only deleted; touch `capture`/`run_as`/`classify_message` (70.2), `repair_recorded_pointers` (70.4), the walk (70.1/70.5).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| local delete / remote modify | `f.txt` removed here, edited there | `TakeRemote`: `f.txt` holds theirs; rc 0; no `MERGE_HEAD`; no copy | — |
| local modify / remote delete | `f.txt` edited here, removed there | `KeepLocal`: `f.txt` holds ours; rc 0; no `MERGE_HEAD`; no copy | — |
| rename vs modify | `git mv a b` here, `a` edited there | `b` (ours) and `a` (theirs) both tracked; no copy | — |
| file / directory | file `d` here, `d/x.txt` there | `d/x.txt` wins the name; ours becomes `d.sync-conflict-…`, in the merge commit, reported as a conflict; no `d~HEAD` left | — |
| directory / file | `d/x.txt` here, file `d` there | `d/x.txt` stays; theirs becomes `d.sync-conflict-…`; no `d~refs_remotes_origin_main*` left | — |
| case-only rename vs modify, `core.ignorecase=true` | `f.txt`→`F.txt` here, `f.txt` edited there | `F.txt` (ours) and `f.txt` (theirs) both tracked; rc 0; no `MERGE_HEAD` | — |
| add / add | same new path, different bytes | remote content at the path, local copy beside it, both in the merge commit | — |
| delete / delete | both removed | `Nothing`: path gone, no copy, rc 0 | — |
| modify / modify (text, binary, pointer/pointer) | both edited | remote wins, copy beside, both in `git show --name-only HEAD` | — |
| stale `MERGE_HEAD` from an old keeper | genuine conflicted `.git/MERGE_HEAD` + unmerged index before the pass | aborted in `commit_local`; the pass converges; `MERGE_HEAD` absent | — |
| `MERGE_HEAD` that will not abort | `.git/MERGE_HEAD` is a directory (git's abort exits 0 and leaves it) | `Config(MERGE_IN_PROGRESS_SENTENCE …)`, nothing committed | the refusal IS the handling |
| two passes in one second | same stamp, same path contested twice | second copy is `…-device-2.txt`; the first is untouched | — |
| non-UTF-8 path (Linux) | `na\xFFme.txt` contested | its own copy, bytes preserved in the name | — |
| resolution cannot be written | parent directory read-only during resolution | `merge_abort` runs; `MERGE_HEAD` absent; error returned; the next pass (writable) converges | error propagated |

</intent-contract>

## Code Map

| Path | Change |
|---|---|
| `src-tauri/crates/keeper-sync/src/git/cli.rs` | `merge_ff_only` (abort on failure), `merge_theirs` → `Result<MergeOutcome>` with `--no-commit` and abort on non-conflict failure; new verbs `merge_abort`, `checkout_side`, `add_paths`, `commit_merge`, `unmerged_entries`, `blob_bytes`, `diff_status` (replaces `diff_names`); `run_raw`; `PathspecFile`; arg builders + vector tests |
| `src-tauri/crates/keeper-sync/src/git/conflict.rs` | `conflict_name` → `OsString` from bytes; `Resolution::ConflictCopy { copy_name: OsString }`; `ChangeKind::from_status`, `ChangeKind::from_stages`; `rescue_origin` |
| `src-tauri/crates/keeper-sync/src/engine.rs` | `converge_with_conflict_copies` rewritten; `merge_head_path`; `MERGE_HEAD` precondition at the top of `commit_local`; `MERGE_IN_PROGRESS_SENTENCE`, `MERGE_UNRESOLVED_PREFIX` |
| `src-tauri/crates/keeper-sync/tests/conflict_matrix.rs` | new: the matrix over real `git` through `Engine::sync_once` |
| `src-tauri/crates/keeper/src/notes_vault.rs` | thin call-site adaptation to `OsString` (2 sites) — **not compiled on this host** |

## Tasks & Acceptance

**Execution:**
- [x] cli.rs verbs and builders
- [x] conflict.rs names and classification
- [x] engine.rs converge + precondition
- [x] tests/conflict_matrix.rs
- [x] mutation proofs

**Acceptance Criteria:** see the matrix; each row is a test in `tests/conflict_matrix.rs`.

## Design Notes

**`--no-commit` on every merge.** Without it a clean merge commits before the copies can be staged, and the copies land in a *later* commit or never (F-PULLPUSH-11). With it the sequence is one shape whether git found conflicts or not: merge, resolve, add, commit. The merge commit's message is passed again to `git commit -m`, so keeper's trailers do not depend on `MERGE_MSG` cleanup rules.

**The verdict is computed twice, from two sources, by one function.** Before the merge, from `diff --name-status` against the merge base (that is where the copies must be made, because after `-X theirs` the local bytes are gone). After the merge, from the index stages (`ls-files -u`): stage 2 says ours has content, stage 3 says theirs has, stage 1 says the base had. Both feed `conflict::resolve`, so AD-43 is stated once.

**Rescue paths.** Measured: git 2.53 records the unmerged entry *at* the rescue name (`d~HEAD`, `d~refs_remotes_origin_main_0`), not at `d`. The engine recognises the suffix, reads the blob from the stage, writes it under `conflict_name(d)`, and removes the rescue entry — so the user sees the product's one conflict-copy shape and the activity list names it.

**Error variant.** `SyncError::Refused` exists but is typed `ContentRefusal` (per-path LFS refusals, deliberately excluded from `needs_user_action`); growing it costs two exhaustive matches in crates this host cannot compile. The `MERGE_HEAD` refusal is `Config` (the contract's fallback); "still unmerged after resolution" is `Diverged`, which already means "a divergence policy could not resolve" and already needs a human.

**`git commit` identity.** The merge commit is made by `git`, as the merge itself always was, so it carries git's identity rather than keeper's gix signature — unchanged from today's `merge -m`.

## Verification

- `cargo test -p keeper-sync --test conflict_matrix` → 15 passed (the matrix rows, the two `MERGE_HEAD` guards, two passes in one second, non-UTF-8, the read-only resolution, the commit that dies mid-merge).
- `cargo test -p keeper-sync --lib -- git::cli git::conflict conflict_cop converge identical_change regenerable_path` → 65 passed (vector tests for every new builder, the pathspec file's bytes, the `-z` parsers, the modify/delete verbs against real `git`, the verb-level abort, the three existing engine converge tests).
- `cargo check -p keeper-sync --all-targets` clean for this story (one warning belongs to 70.2's in-flight `fetch::classify`); `cargo check -p keeper-syncd` clean.

| Mutation | Tests that failed | Observed |
|---|---|---|
| (1a) `merge_theirs`: `Err(err) => Err(err)` — the verb no longer aborts a merge that died after `MERGE_HEAD` | `git::cli::tests::a_merge_that_dies_after_writing_merge_head_is_undone_before_the_error_returns` | "the merge was undone before the error was returned" |
| (1b) converge's failure branch: `git.merge_abort` replaced by `Ok(())` | `a_merge_whose_commit_dies_is_undone_with_its_copies_and_finishes_next_pass` | 14 passed; 1 failed at "a merge that does not finish is undone" |
| (2) `commit_local`: `clear_stale_merge` call removed | `a_merge_an_old_keeper_left_in_progress_is_undone_and_the_folder_syncs_again`, `a_merge_head_that_will_not_abort_refuses_by_name_and_commits_nothing` | 13 passed; 2 failed |
| (3) `finish_merge`: `Some(Side::Local) if ours.is_none()` — a `KeepLocal` verdict recorded as a removal | `local_modify_remote_delete_keeps_the_modification` | 14 passed; 1 failed (`f.txt` missing) |
| (4) `write_conflict_copy`: `create_new(true)` → `create(true).truncate(true)` | `two_passes_within_one_second_keep_both_copies` | 14 passed; 1 failed ("both copies stand") |

Every mutation was a single-line edit and a single-line restore (shared worktree); the matrix ran green (15) after the last restore. The first attempt at (1b) survived: the read-only-directory row fails *inside* `git merge` (exit 2, no `MERGE_HEAD`), so it exercises error propagation, not the abort — the commit-dies row was added for that, measured first (`COMMIT_EDITMSG` as a directory → `commit` exits 128 with `MERGE_HEAD` set).

**Not verified here, and why.** `keeper/src/notes_vault.rs` (two thin call sites adapted to the `OsString` name) does not compile on Linux (GTK). The pointer/pointer row merges pointer *blobs* with no `filter=lfs` rule registered — the test fixture cannot run keeper's clean/smudge process (under `cargo test` it is the libtest binary), so "the local content re-matches the rule and gets its own object" (lane row) is not exercised. The case-only row runs on a case-sensitive filesystem: it proves the merge finishes with both names tracked; what a case-insensitive APFS does with `F.txt`/`f.txt` on disk afterwards is scan-10 (upstream, out of scope). The merge commit's author is git's identity, as before.
