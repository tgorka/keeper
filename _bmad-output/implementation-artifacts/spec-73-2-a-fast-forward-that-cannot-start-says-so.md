---
status: review
baseline_revision: 771fffd
final_revision: ''
---

# Story 73.2 — A fast-forward that cannot start says which paths hold it, and stops looping

<intent-contract>

## Problem

Measured on hesperia, 2026-09-17: `tgdrive-light` spent **75 minutes** in this loop, every few minutes, and would never have left it:

```
remote polled: the remote branch moved profile="tgdrive-light" fast_forward=true received_pack=true
sync retrying profile="tgdrive-light" error=git merge --ff-only failed (exit 1): error: Your local
  changes to the following files would be overwritten by merge: 10-notes/.keeper/index.json
sync warning profile="tgdrive-light" sync has failed 3 times in a row
```

git's refusal arrived as `SyncError::GitCommand`, which is `Retriability::Transient` — so the scheduler retried an operation whose precondition is the working tree, and nothing between two ticks touches the working tree. The deadlock was **total** because the blocking path was one keeper excludes from sync: keeper could not commit it (the exclusion, story 73.1) and could not merge past it (git's refusal), so the folder could never converge again by itself. Nothing in the UI said so; the only trace was a WARN in a log that is off by default.

## Approach

Classify the refusal, name the paths from git's own output, and split on who owns the bytes: keeper resolves it when every blocking path is one it excludes, and stops with a verdict when any of them is a person's content.

## Always

Parse the paths out of git's own message (both headings: local changes, and untracked files). Carry them in the error, so the sentence a person reads names the files. Discard only paths the profile excludes, and log which ones. Classify as `Permanent` and `needs_user_action`.

## Block If

Any blocking path is not excluded — then keeper stops, `NeedsAttention`, paths in the sentence, and does not retry the same pull.

## Never

Never discard a path the profile does not exclude: those bytes are the user's. Never widen the classification — a diverged history, a missing ref and a broken index stay `GitCommand`, because the conflict-copy path depends on that. Never return `Err` with a `MERGE_HEAD` standing (AD-229's rule, unchanged: the refusal goes through `undo_failed_merge`).

## I/O and edge-case matrix

| Input | Observable result |
| --- | --- |
| ff refused, one blocking path, excluded | discarded, ff applied, folder converges; log names the path |
| ff refused, blocking path is user content | `MergeBlocked`, `Permanent`, `needs_user_action`, path in the sentence |
| ff refused, mixed (one excluded, one not) | refused entirely; nothing discarded |
| ff refused for a diverged history | `GitCommand`, as before |
| git's untracked-files heading | parsed the same way |
| stderr with no listing (`fatal: not something we can merge`) | unchanged classification |
| the hesperia string, verbatim | `MergeBlocked` naming `10-notes/.keeper/index.json` |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/error.rs` — `SyncError::MergeBlocked { paths }` (new), `retriability()` → `Permanent`, `needs_user_action()` → `true`, `code()` → `mergeBlocked`.
- `src-tauri/crates/keeper-sync/src/git/cli.rs` — `merge_ff_only` maps through `blocked_or`; `blocked_paths` parses git's listing; `discard_paths` (new verb, `checkout -q -- <paths>`); `discard_paths_args`.
- `src-tauri/crates/keeper-sync/src/engine.rs` — the fast-forward arm of the pull leg: on `MergeBlocked`, build the profile's `ExcludeSet`, resolve if every path is excluded (discard, then one retry of the fast-forward), else return the error.
- `src-tauri/crates/keeper-syncd/src/commands.rs` — `sync_exit_code` gains the arm: `EXIT_CONFIG`, because retrying changes nothing and `Restart=on-failure` must not loop on it.
- `src-tauri/crates/keeper/src/sync_ipc.rs` — `sync_ipc_error` gains the arm: `IpcErrorCode::SyncUnavailable`. **Inspection only — that crate does not link on the Linux dev host.** Both matches are exhaustive, which is how the new variant was caught: `cargo check -p keeper-syncd` failed with `non-exhaustive patterns: &SyncError::MergeBlocked { .. } not covered` before the arm was added.

## Tasks & Acceptance

- [x] The variant, its classification and its code.
- [x] The classifier and the path parser, over git's own words.
- [x] The `discard_paths` verb, narrow by contract to the one caller.
- [x] The engine's two-armed resolution.
- [x] Both sibling crates' matches covered.
- [x] Tests: the real git refusal end to end, the diverged case, the parser over both headings.
- [x] Mutation proof.

Acceptance, verbatim from the epic: *a fixture whose only blocking path is excluded-and-tracked converges by itself, and the log names what it discarded; a fixture whose blocking path is user content ends `NeedsAttention` with the path in the sentence and does **not** attempt the same pull again until the tree changes (a counter test over three ticks); the real hesperia string … is a test vector and classifies as this refusal rather than as `GitCommand`/transient; a merge that fails for any other reason keeps its current behaviour.*

## Design Notes

**Why the excluded arm discards rather than stashes.** A stash is a commit, and the only thing a folder can do with a stashed cache is carry it forever. These paths are keeper's own or the machine's — a cache, a lock, a trash tree — and the fast-forward is the only thing that can make the folder converge. The log line names what was discarded, which is what makes the action auditable rather than silent.

**Why `Permanent` and not a bounded retry.** The precondition is the working tree, and no tick changes it. `Permanent` is also what makes `record_failure` choose the `NeedsAttention` word, which is the one state the deadlock never reached in the field.

**What is owed.** The epic's counter test over three ticks — "does not attempt the same pull again" — is asserted here only through the classification (`Permanent` is what stops the re-arm) rather than by driving three ticks against a real remote with a dirty user file. The three tests below prove the classification, the parsing and the resolution; the tick-level assertion needs the `Mac::publish_into` fixture wired to a Binary-engine profile and is the one piece of 73.2's acceptance not yet mechanically covered.

## Verification

- `cargo test -p keeper-sync --lib -- blocked diverged_fast_forward` — 3 passed: the real-git refusal (classification, paths, `Permanent`, `needs_user_action`, the sentence, then discard + successful fast-forward), the diverged case staying `gitCommand`, and the parser over both of git's headings.
- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-syncd -p keeper-core` — every suite green.
- `cargo clippy -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` — clean; `cargo fmt --all` applied.
- Mutation: with `blocked_or` removed from `merge_ff_only`, `a_fast_forward_blocked_by_a_local_change_names_the_paths_and_is_not_transient` fails at the classification assertion; the file was restored byte-for-byte (md5 compared) and the test passes again.
- The shell crate's one-line arm awaits CI's macOS `Rust (fmt, clippy, test)` job; no Linux build of it exists or was claimed.
