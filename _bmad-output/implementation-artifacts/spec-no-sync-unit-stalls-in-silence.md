---
title: "A folder that stops syncing says so"
type: 'bugfix'
created: '2026-08-20'
status: 'done'
baseline_commit: '23498487'
review_loop_iteration: 0
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** A vault went four days without a fetch and nothing said so. Diagnosed from the running process, not guessed:

- the journal held one `pull` row in `running`, `last_error` empty, since the moment it was claimed;
- the folder card read **Idle — 1 waiting to sync**, its Activity and Pending panels stuck on "Loading…";
- `.git/FETCH_HEAD` was three days old and no object had arrived;
- the engine thread sat in `status_paths_excluding` → `gix::status::iter::next` → `recv_timeout`, waiting on results;
- the gix worker threads sat in `compare_blobs` → `convert_to_git` → `Client::invoke` → `io::copy`;
- **55 working-tree files were open at once, every read offset at EOF, not one byte moving in 45 seconds**;
- the `keeper lfs filter-process` workers sat in `pktline::read` on stdin, waiting for input that had already been delivered in full.

Unbounded status parallelism (`thread_limit: None` — one thread per core) put 55 LFS conversions in flight against a much smaller pool of filter processes. Every one blocked. No error was recorded because no code path reached one.

**Approach:** Two guards and a witness. Bound the conversions so the pool cannot be oversubscribed; give the walk a liveness deadline so it can be abandoned rather than waited on; give the filter a per-request deadline so a stuck child exits instead of hanging its parent. Then log enough that the next occurrence is read from the log rather than from a sampled process.

## Boundaries & Constraints

**Always:**
- **Fire on silence, never on duration.** A pass converting a gigabyte of video is slow and healthy. Only a pass that has stopped producing items is stuck, and only that one may be killed.
- An abandoned pass reports a sentence that names how far it got. "status failed" cannot tell "stuck on the first file" from "stuck on the last", and that difference is the next investigation.
- Interrupt, do not merely observe. gix polls `should_interrupt` from inside the walk, so setting it unwinds the stuck threads. A watchdog that only logged would leave the folder as stuck as before while claiming to have noticed.
- The filter exits rather than repairs. It cannot resume a half-consumed request — the pipe's position is protocol state and there is no packet meaning "start over". A closed pipe is something the parent already knows how to read.

**Ask First:**
- Any change to what a status pass *means* — which entries it emits, how untracked content is walked. This changes only how many run at once and when to give up.
- A general deadline on every work unit. A 20 GB push is legitimately long; a blanket timer would kill it.

**Never:**
- No sleep-and-hope. Every guard here either cancels the work or ends the process.
- No silent recovery. Everything this abandons is recorded in the journal and surfaced.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Healthy pass | items flowing | never interrupted, whatever the total duration | N/A |
| Slow single file | one 1 GB `.mov` | not interrupted; the filter logs it by name each minute | N/A |
| Deadlocked conversion | no item for 10 min | walk abandoned, sentence names the count, unit fails and retries | recorded in the journal |
| Stuck request in the filter | no progress for 15 min | filter logs the path and exits 75; parent's `invoke` fails | closed pipe, not a hang |
| Finished pass | walk returns | watcher told to stop; no thread left polling | N/A |
| Many filtered files | a worktree of recordings | at most `STATUS_THREAD_LIMIT` conversions in flight | N/A |

</frozen-after-approval>

## Code Map

- `src-tauri/crates/keeper-sync/src/git/repo.rs` -- `STATUS_THREAD_LIMIT`, `STATUS_SILENCE_LIMIT`, `StatusWatchdog`, and the two lines in `status_paths_excluding` that arm them
- `src-tauri/crates/keeper-sync/src/lfs/filter.rs` -- `REQUEST_LIMIT`, `RequestGuard`, armed around `serve_one`

## Tasks & Acceptance

- [x] bound the walk's conversions and pin the ceiling at compile time
- [x] a watchdog that fires on silence, interrupts through `should_interrupt`, and releases on drop
- [x] a per-request guard in the filter that names the path and exits
- [x] the shape of every pass at INFO: entries, elapsed, added/modified/deleted
- [x] tests: healthy walk never interrupted, silent walk interrupted, beats are the count, drop releases the watcher, the refusal names the count

**Acceptance Criteria:**
- Given a status pass that stops producing items, when the silence limit passes, then the walk is interrupted and the unit fails with a sentence naming the count — not left `running`.
- Given a pass that is merely slow, when it runs for longer than the limit while still emitting, then it is never interrupted.
- Given a filter request that never completes, when the request limit passes, then the filter logs the path and exits, and the parent sees a closed pipe.
- Given a worktree of filtered files, when a status pass runs, then at most `STATUS_THREAD_LIMIT` conversions are in flight.

## Verification

- `cargo nextest run -p keeper-sync` · `cargo clippy --all-targets -- -D warnings` · `cargo fmt --check`
