---
title: 'Progress you can watch'
type: 'feature'
created: '2026-07-28'
status: 'review'
baseline_revision: '5c40a22'
---

<intent-contract>

## Intent

**Problem:** Two halves of the same complaint — a bar that moves without telling you anything.

(1) **No transfer rate exists anywhere in the crate.** There is no `bytes_per_second`, `rate`,
`elapsed` or `eta` on `SyncProgress`, `SyncStatus`, `SyncProgressVm` or `TransferTally`. The single
`Instant` in the transfer path (`git/fetch.rs:327`, `ProgressSink::started`) is a callback throttle
and is never read for a rate. So a user watching a 4 GB push sees a percentage and no sense of
whether it will finish this minute or this evening.

(2) **The commit leg does not move per file.** `commit_local` published one frame with
`files_done: 0` and `current: Self::first_staged(&staged)`, then one frame with `files_done: count`
after the whole commit returned. Between them — the part that actually takes time, reading and
hashing every staged path — nothing was published at all, so the counter jumped 0 → total in one
step and the detail line named the first staged path for the entire commit. On a 10 000-file first
sync that is a frozen line under a frozen bar for minutes.

**Approach:** A `RateMeter` in `progress.rs` measures whole bytes per second over a rolling window
and answers `None` whenever no honest figure exists. `TransferTally` owns one, so the LFS leg gets a
rate through the `apply` it already calls; `do_pull`'s fetch fold owns one too, fed the `fetched`
high-water mark rather than the raw per-node counter. The figure rides to the frontend as
`SyncProgressVm.bytesPerSecond`. For the commit leg, `stage_and_commit` takes an optional
`StagingSink` and reports `(files_done, path)` as it walks the change set, coalesced at the crate's
usual 100 ms with the last path always forced; `Engine::commit` builds the sink that turns those
into `Committing` frames. The pane renders both figures on the in-flight line under the bar.

## Boundaries & Constraints

**Always:** `Option<u64>`, not a float — a rate finer than 1 B/s is noise, and an integer cannot be
negative, infinite or NaN, so the acceptance criterion is a property of the type rather than of the
arithmetic. It also keeps `SyncProgress`'s `PartialEq, Eq` derives, which the tray's repeat-state
guard (34.1) depends on. The meter never yields `Some(0)`: `None` and "nothing is moving" are one
answer, which is what lets the UI render `null` as nothing without laundering a zero. The counter fed
to the meter must be cumulative, and both feeds are monotonic by construction. Coalescing follows
`lfs::basic::ProgressCoalescer` at `DEFAULT_PROGRESS_INTERVAL`, and the terminal frame is forced past
it exactly as `git::fetch`'s emitter forces its completion tick and `copy.rs` forces via
`emit(true)`. `syncLiveRate` gates on the polled status before reading the stream, in the same order
and for the same reason as `syncLiveFraction`.

**Block If:** (none — the epic and the batch contract fixed every decision this story needed)

**Never:** Do not add `bytes` to `SyncOutcomeVm` — the epic's 34.8 paragraph asks for it, the batch
contract assigns it to 34.10, and the contract wins. Do not put the rate on `SyncStatus`/
`SyncStatusVm`: 34.9 owns that struct this wave, and the tray line was not asked to carry a rate. Do
not derive the rate from `do_pull`'s raw `done` figure — it is whichever gitoxide node ticked last,
its unit is that node's (objects while deltas resolve), and it restarts at zero on every phase. Do
not touch `SyncActivityList`, the card header, `parse_req`, `sync_once`, the watcher wiring or any
other region another Epic 34 story owns.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| First observation | one sample, no history | `None` — a point is not a rate | n/a |
| Window under 1 s | 1 MB at +100 ms | `None` (or the held previous figure) — one producer tick can be one whole buffered chunk | n/a |
| Full window | 2 MB at +1 000 ms | `Some(2_000_000)` | n/a |
| Same instant twice | two observations, elapsed 0 | `None`; elapsed floors at 1 ms, so nothing divides by no time | no panic, no infinity |
| Nothing moved in a window | same byte count at +1 100 ms | `None` — "0 B/s" would claim a measurement of an idle wire | n/a |
| Under 1 B/s | 50 bytes in 60 s | `None` — integer division floors to 0, which is not a rate | n/a |
| Retry restarts an object | `Progress{oid, 0}` after 1 MB | tally's high-water mark holds, so no new bytes: the rate decays and then falls silent, never negative | `saturating_sub` absorbs it |
| Counter drops (defensive) | fed a smaller cumulative count | reports no movement, re-anchors within one window | never a negative rate |
| Stall after a full window | window reopens, nothing arrives | `None`, instead of an average since the transfer began still claiming 5 MB/s | n/a |
| Fetch phase rollover | pack done, deltas resolving | `fetched` stays flat, so the meter goes quiet rather than charging an object count to a byte rate | n/a |
| Staging 3 paths, tiny | 2 added + 1 deleted | frames `(0, a.txt)` and forced `(2, gone.txt)`; every frame advances and names the path at its index | n/a |
| Staging 10 000 paths | fast loop | bounded frames (2 in a no-work loop; ~10/s of real staging), never 10 000 | n/a |
| Sink answers `false` | receiver gone | dropped after exactly one call; the commit still completes | journaled work is never abandoned |
| No sink at all | `progress: None` | zero clock reads per file | n/a |
| Stream event, poll says settled | active `false` | no path, no counter, no rate | n/a |
| No stream event yet | window just mounted | no path, no counter, no rate; the polled Rust line still says what is happening | n/a |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/progress.rs` -- `SyncProgress.bytes_per_second`; new
  `RATE_MIN_WINDOW_MS` / `RATE_WINDOW_MS` constants and `RateMeter`; `TransferTally.rate`, its
  `fold`/`fold_at` split and `apply`.
- `src-tauri/crates/keeper-sync/src/git/commit.rs` -- new `StagingSink` type and `report` helper; the
  staging loop and the deletion walk report through them; `stage_and_commit` takes `progress`.
- `src-tauri/crates/keeper-sync/src/engine.rs` -- `do_pull`'s fetch fold stamps a rate;
  `Engine::commit` builds the staging sink and passes it down.
- `src-tauri/crates/keeper-sync/src/git/repo.rs`, `tests/lfs_roundtrip.rs` -- `None` at the two
  out-of-module `stage_and_commit` call sites.
- `src-tauri/crates/keeper/src/sync_ipc.rs` -- `SyncProgressVm.bytes_per_second` and its mapping.
- `src/lib/ipc/gen/SyncProgressVm.ts` -- regenerated to match, byte for byte.
- `src/lib/stores/sync-detail.ts` -- `syncLiveRate`, beside `syncLiveFraction`.
- `src/components/layout/sync-pane.tsx` -- the in-flight line becomes path · counter · rate.

## Tasks & Acceptance

**Execution:**
- [x] `progress.rs` -- Add `bytes_per_second: Option<u64>` to `SyncProgress` and `idle()`. -- The
  payload can carry a rate without losing `Eq`.
- [x] `progress.rs` -- Add `RateMeter` with `observe(bytes, now)` and `bytes_per_second()`, plus the
  two window constants. -- One place derives the figure, and time is a parameter so its boundaries
  are testable.
- [x] `progress.rs` -- Give `TransferTally` a `rate`, split `fold` into `fold`/`fold_at(now)`, and
  stamp the rate in `apply`. -- The LFS leg gains a rate with no change to `do_lfs`.
- [x] `git/commit.rs` -- Add `StagingSink` and `report`; report per path in both walks; take
  `progress` as the last parameter. -- `files_done` and `current` advance during staging.
- [x] `engine.rs` -- Build the staging sink in `Engine::commit`; stamp a rate in `do_pull`'s fold. --
  Both legs publish what the crate now measures.
- [x] `sync_ipc.rs` + `gen/SyncProgressVm.ts` -- Carry `bytesPerSecond: number | null` across. -- The
  frontend can read it and `bindings:check` stays clean.
- [x] `sync-detail.ts` -- `syncLiveRate(status, progress)`. -- The poll still decides whether a
  folder is working.
- [x] `sync-pane.tsx` -- Render `path · N/M files · R/s` under the bar with `formatCopyBytes`. -- The
  two figures the story is named for are on screen.
- [x] Tests -- `progress.rs`: four `RateMeter` timelines (waits for a second, never zero or
  infinite, restart falls silent and recovers, the window reopens) and one tally-level retry test.
  `git/commit.rs`: per-path frames with the forced last one, and a refusing sink called exactly once.
  `engine.rs`: the existing commit-progress test now expects the middle frame. `sync-pane.test.tsx`:
  the rendered rate and counter, the null-rate case, and `syncLiveRate`'s ordering.

**Acceptance Criteria:**
- Given a folder pushing one large file, when the transfer runs, then the card shows a rate that
  moves and a file counter that climbs.
- Given a retry or a phase rollover, when the byte counter stalls or restarts, then the rate is
  never negative, infinite, NaN or zero — it decays and then reports nothing.
- Given a change set of ten thousand paths, when it is staged, then the number of published events is
  bounded by elapsed time, not by path count.
- Given a progress event arriving while the poll still calls the folder settled, then no bar, no
  counter and no rate are drawn.
- `SyncOutcomeVm` is unchanged by this story.

## Design Notes

**Why a rolling window and not "a start instant".** The epic says the rate is derived "from a start
instant and `bytes_done`". A single fixed start instant gives the average since the transfer began,
and that average cannot fall: minutes after a connection drops to a crawl it would still read
"12 MB/s", and on the fetch leg — where the numerator freezes when the pack finishes while `elapsed`
keeps growing — it would decay toward zero forever without ever going quiet. So the window closes and
reopens every `RATE_WINDOW_MS` (2 s), which is still "a start instant and `bytes_done`", just one
that moves. The floor of `RATE_MIN_WINDOW_MS` (1 s) is what makes the figure steady rather than
jittery: both producers sample at ~100 ms (`git::fetch::REPORT_INTERVAL_MS`,
`lfs::basic::DEFAULT_PROGRESS_INTERVAL`) and one sample can carry a whole buffered chunk, so a 100 ms
window reports a burst as the sustained rate. The last full-window figure is held while the next one
fills, so the display does not blink to nothing every two seconds.

**Why `Option<u64>` and never `Some(0)`.** An integer rate cannot be negative, infinite or NaN, so
the "never a bad number" criterion is discharged by the type. The no-zero rule is the more
interesting one: a window that carried no bytes has no rate to *report*, and printing "0 B/s" would
be a claim about an idle wire — a pack that has finished arriving while its deltas resolve, or an
object being retried in place. Making the backend never emit a zero means the frontend rule ("a null
rate renders as nothing, never 0 B/s") is satisfied by construction rather than by a special case.
`syncLiveRate` still filters non-positive values, so the rendered invariant holds where it is read
rather than resting on the other process's promise.

**Why the fetch leg reads `fetched` and not `done`.** `FlatProgress` (`git/fetch.rs:344`) flattens
gitoxide's progress *tree* onto one `(done, total)` callback, and each node keeps its own counter in
its own unit — `init` takes a `unit` and discards it (`:418`). `done` is therefore whichever node
ticked last: bytes during the pack read, objects while deltas resolve, and it restarts at zero on
every phase. Dividing that by a second would produce "objects per second" labelled B/s. `fetched`
(`fetched.max(done)`) is the figure this function *already* treats as bytes received — it is what
`add_transferred` records — and because byte counts dwarf object counts during a real fetch the
maximum is the pack. It is monotonic, so the rate is never negative, and it stays flat through a
phase that is moving no bytes, which is exactly when the meter should fall silent. This is the
"emit null rather than a wrong number" answer the pull leg needed.

**Where the staging sink coalesces, and why it is not `ProgressSink`.** Producer-side, matching
`git::fetch` and `lfs::basic`: the alternative is 10 000 calls into a closure that takes two engine
mutexes and allocates a `String` each time. The sink is `Fn(u64, &Path) -> bool` rather than the
crate's `ProgressSink`, for the reason `lfs::basic::TransferSink` is also its own type — staging
knows the path it is reading and the count behind it, and nothing else; profiles, phases and
denominators belong to the engine. The `-> bool` "stop producing" contract is identical, and
`report` drops the sink on `false`.

**Why the first staging frame is suppressed and the last is forced.** `commit_local` must publish
before `commit` runs — `lfs::stage::prepare` can spend real time hashing before staging starts, and a
blank detail line through that window would be a regression — so it names the first path itself. The
staging sink's frame for that same path would be byte-identical, which AD-34-1 says must not reach
the tray, so the engine's closure returns early on `files_done == 0`. At the other end the last path
is forced past the coalescer, for the reason `git::fetch`'s emitter always forwards its completion
tick: a detail line left naming the second of ten thousand files while the commit object is written
is worse than one that updates a little less often. Together these make the frame count deterministic
— entering, one per path after the first, completing — which is what lets the tests assert a
sequence rather than a timing.

**Why the counter is duplicated on the card.** `status_line` already renders `— 1/2 files` inside the
Rust-composed sentence, but the status poll runs at `SYNC_ACTIVE_POLL_MS` (2 s) while the stream
delivers at ~100 ms. A path flickering ten times a second above a counter frozen for two seconds
looks broken. So the in-flight line carries its own counter off the stream, worded exactly as Rust
words it (`N/M files`) so the two read as one quantity sampled at two rates. The card already mixes a
stream-refined figure with a polled sentence by design — `percent` comes from `syncLiveFraction`
while `aria-valuetext` stays the Rust line.

**Deviation from the epic.** The epic's 34.8 paragraph also asks for `bytes` on `SyncOutcomeVm`. The
batch contract assigns that field to story 34.10, which owns `SyncOutcomeVm` and `sync_folder_now`
this wave, so it is deliberately not here.

## Verification

**Not run:** the project build, `cargo clippy`, `cargo fmt`, `bun run check` and the test suites. Five
agents were editing this worktree concurrently, so a workspace build would have reported other
stories' in-flight edits rather than this one's, and `cargo build` for the `keeper` crate does not
work on this Linux box at all (tauri needs GTK/glib-sys). The parent runs all of it once at the end.

**What was verified, and how.** The two pieces of new logic with non-obvious arithmetic were
extracted *verbatim* from the source and compiled and run standalone with `rustc --test`, which is
immune to the concurrent edits:

- `RateMeter` and `TransferTally` (constants, struct, `impl`s and all five new tests copied out of
  `progress.rs` with only intra-doc links rewritten): 5 passed, 0 failed. This exercises every row of
  the rate half of the I/O matrix, including the same-instant, nothing-moved, sub-1 B/s, retry and
  stall cases.
- `StagingSink`, `report` and `ProgressCoalescer` (verbatim from `git/commit.rs` and
  `lfs/basic.rs`) driven through a copy of the two staging walks with the gix work removed: 3 paths
  produce exactly the frames `(0, a.txt)` and `(2, gone.txt)`; 10 000 paths produce 2 frames, not
  10 000; a sink answering `false` is called exactly once. These are the three commit-leg acceptance
  criteria, and the numbers are the ones the in-repo tests assert.
- `formatCopyBytes(4_100_000)` was run through the pane's own implementation to confirm the rendered
  string the new pane test expects is `4.1 MB/s`.

**Commands for the parent to run:**
- `bun run test:rust` -- expected: the four new `RateMeter` tests and the new tally test in
  `progress.rs`; the two new staging tests in `git/commit.rs`; the updated
  `a_commit_publishes_a_file_count_the_bar_can_use` in `engine.rs` (now three `Committing` frames).
- `bun run bindings:check` -- expected: no diff; `SyncProgressVm.ts` was updated to match the struct.
- `bun run check` -- expected: the three new `sync-pane.test.tsx` cases pass, and no other suite
  moves.

**Manual check on hesperia:** push a folder holding one large file and watch the card — the rate
moves and the counter climbs; then drop 500 small files in and confirm the detail line advances
through them instead of naming the first one for the whole commit.
