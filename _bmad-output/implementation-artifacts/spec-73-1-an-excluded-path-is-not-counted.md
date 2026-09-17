---
status: review
baseline_revision: 771fffd
final_revision: ''
---

# Story 73.1 — An excluded path is not counted, and a tracked one is named

<intent-contract>

## Problem

AD-45 has required since Epic 23 that an exclusion be **invisible** — "not staged, not queued, not counted, never reported as pending". The commit leg honoured the first two: a path the profile excludes is never staged. The walk's answer did not, because git reports a path it tracks whatever keeper thinks of it. Measured on hesperia, 2026-09-17: `10-notes/.keeper/index.json` — keeper's own notes index cache, tracked in git on the drive it syncs — left `tgdrive` reading `entries=1 scanned=156941 modified=1` on every poll for 22 hours, across 15 paced passes that committed nothing. Its untracked neighbour `.keeper/trash/` was correctly invisible (`untracked=0`), which is the proof that the exclusion works right up to the moment git carries the path. The same file then deadlocked the second clone (story 73.2).

## Approach

The engine takes excluded paths out of the walk's tracked buckets the moment the walk returns, before anything downstream reads them, and raises **one** anomaly per pass naming the tracked ones with the only remedy that works — untrack them.

## Always

Filter the tracked buckets (`added`, `modified`, `deleted`) by the profile's `ExcludeSet`, which is the built-in corpus plus the profile's own patterns. Report the tracked-and-excluded paths as a standing fact, once per pass, capped at `EXCLUDED_TRACKED_NAMED`.

## Block If

Nothing new is refused: this story removes counting, not capability.

## Never

Never touch `status.untracked` here — the pending list and the stager already filter untracked paths, and the commit walk is where a nested repository inside an excluded directory gets explained. Never emit one line per walk for a standing fact (Epic 70's 1.2 GB log).

## I/O and edge-case matrix

| Input | Observable result |
| --- | --- |
| tracked, excluded, modified (`.keeper/index.json`) | gone from `modified`; anomaly names it |
| tracked, excluded, added / deleted | gone from `added` / `deleted` |
| tracked, ordinary, modified | survives, and is committed |
| untracked, excluded | left in `untracked`, filtered downstream as before |
| a walk with nothing excluded | returned unchanged, no anomaly |
| thousands of excluded tracked paths | one line, first five named, count stated |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/engine.rs` — `Engine::drop_excluded` (new), called immediately after the commit path's `status_paths_reported`; `EXCLUDED_TRACKED_NAMED` beside the other engine constants.
- `src-tauri/crates/keeper-sync/src/exclude.rs:317` — `ExcludeSet::is_excluded`, the one predicate; the built-in corpus is what makes `.keeper/` excluded without any profile pattern.
- `src-tauri/crates/keeper-sync/src/anomaly.rs:30` — the four-field anomaly shape this reports through.
- `src-tauri/crates/keeper-sync/src/engine.rs` (poll path, ~:15882) — already skipped excluded paths when building the pending list; unchanged, and the reason the pane never showed the cache.

## Tasks & Acceptance

- [x] `drop_excluded` filters the three tracked buckets and leaves `untracked` alone.
- [x] One anomaly per pass, capped, naming the remedy.
- [x] Test: an excluded tracked path leaves the answer, an ordinary one stays, an untracked excluded one is untouched, and a clean walk is returned unchanged.
- [x] Mutation proof.

Acceptance, verbatim from the epic: *a fixture with one tracked, excluded, modified path reads `modified=0` from the walk and `clean` from the durable status, and the same fixture with one tracked, excluded, modified path **and** one ordinary modified path reads `modified=1` and commits exactly the ordinary one; the anomaly fires once per pass and names the path; a fixture with an untracked excluded path raises no anomaly (it is already invisible); the pane's pending count for the first fixture is zero.*

## Design Notes

The pending count was **already** zero: the poll path has filtered excluded paths since before this story (`engine.rs` poll bucket loop). That matters for the epic's honesty — the owner's "1 modified" came from the walk's log line and from the folder never committing, not from the pending list — and it is why this story is a correction of the walk's answer plus an anomaly, rather than a UI fix.

`status.untracked` is deliberately left alone. Filtering it here would also silence the nested-repository explanation the commit walk owns, which is a different promise to a different reader.

## Verification

- `cargo test --manifest-path src-tauri/Cargo.toml -p keeper-sync` — 1301 unit tests plus every integration suite, green (run on the Linux dev host; `keeper-sync` compiles and tests fully here).
- `cargo clippy -p keeper-core -p keeper-sync -p keeper-syncd --all-targets -- -D warnings` — clean. `cargo fmt --all` applied.
- Mutation: with the `drop_excluded` call removed from the commit path and the filter body emptied, `an_excluded_tracked_path_leaves_the_walks_answer_and_an_ordinary_one_stays` fails on `assertion left == right`; restoring the file byte-for-byte (md5 compared) makes it pass again.
- Field evidence this closes is in DW-255; the repository-level workaround applied on 2026-09-17 (`d8fe2668e` in the owner's `tgdrive`) is a repair of one folder, not of the code, and this story is what keeps the next folder from needing it.
