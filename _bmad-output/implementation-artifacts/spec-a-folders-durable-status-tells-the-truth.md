---
title: "A folder's durable status tells the truth"
type: 'bugfix'
created: '2026-09-09'
status: 'done'
review_loop_iteration: 2
baseline_commit: 'a8d9e2cbcd1de94637e1c4be28aa38997e27b703'
context: []
---

<frozen-after-approval reason="human-owned intent — do not modify unless human renegotiates">

## Intent

**Problem:** `profiles.state` and `profiles.last_error` are what `keeper-syncd status` and a fresh launch read, and on hesperia 2026-09-09 both lied. neuradrive read `watching` through 1 044 consecutive push failures: `record_failure` sets `Offline` on a network error, but every `sync_once` pass ends with `set_state(Watching)` and `refresh_pending` flips `Syncing → Watching` when nothing is pending, so a pass that committed nothing while its push unit was still owed and failing overwrote `offline`. tgdrive-light carried a `last_error` from 2026-08-31 ("this folder's first copy never finished…") days after the copy completed: `seed_status` loads `state` but never `last_error`, and `clear_warning` retires the column only when the in-memory snapshot had an error, so an error persisted by an earlier run can never be retired by a later run's successes. And neither writer stamps `updated_ms`, so a reader cannot tell a live row from one last touched in August.

**Approach:** the pass-end and pending-refresh transitions to `Watching` yield to a failure state that is still true — a profile in the engine's `offline` set stays `Offline`, one carrying an error stays where the error put it — and only a unit that succeeds moves it on, as `note_unit_succeeded` already does. `seed_status` seeds the snapshot's error from the row, so the existing retire-on-success path reaches errors from earlier runs. Both durable writers stamp `updated_ms`.

## Boundaries & Constraints

**Always:** `record_failure`'s classification (`Offline` for network, `NeedsAttention` after three transient failures or a permanent error, `MediaAbsent` deferred) is unchanged; `Offline` ends only when the remote answers — a unit that actually round-tripped (`note_unit_succeeded` on `Reached`) or a failure the remote answered with (auth, forbidden, quota, diverged, moved, integrity), never a pass that touched no remote (amended 2026-09-10, loop 2); `set_profile_state` and `set_profile_error` stay separate writers (their doc says why) and both take the engine's `now_ms`; a persisted `offline` seeds as `Offline` while its `updated_ms` is younger than the retry backoff's cap plus the longest single attempt (`OFFLINE_SEED_MAX_AGE_MS`, 20 min — a daemon at the cap re-stamps every 10 min and an attempt can take up to 10 min more), otherwise as `Idle`; `syncing` always seeds as `Idle`; a persisted `NeedsAttention`/`MediaAbsent` keeps its state and its error, and any row carrying an error seeds `NeedsAttention` whatever its word (amended by the owner 2026-09-10); `docs/sync.md` §12 says what `keeper-syncd status` reads and what `updated_ms` means; no `.unwrap()`; every claim below has a test.

**Ask First:** if keeping a folder `Offline` through a pass that committed locally would hide a *local* commit from the pane in a way the existing `Syncing` phase does not already cover, ask before changing how the phase is published.

**Never:** change `keeper-syncd status` output shape or the IPC view model; touch the `activity` table (verified live, not frozen); add a new state word; retire an error on anything but a successful unit or an explicit resume/recheck.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected Output / Behavior | Error Handling |
|----------|--------------|---------------------------|----------------|
| Push keeps failing | offline set holds the profile, a pass commits nothing | pass end leaves `offline` in memory and in the row; `refresh_pending` does not flip it | — |
| Push then succeeds | unit succeeds | `note_unit_succeeded` clears offline; pass end writes `watching` | — |
| Error stands, pass quiet | snapshot.error is Some (NeedsAttention) | pass end and `refresh_pending` leave the state and the error | — |
| Restart with a persisted error | row has `last_error`, state `needsAttention` | snapshot seeded with both; `status` shows the error; first successful unit retires both in memory and in the row | — |
| Restart with persisted `offline` | row `state=offline`, no error | seeds `Offline` while `updated_ms` is fresh (the daemon may still be failing), else `Idle`; either way the sticky set is empty, the first tick re-probes, and the next failure re-enters `offline` as today (amended 2026-09-10) | — |
| Any state or error write | `set_state` / `set_error` | `updated_ms` = engine `now_ms` on the row | write failure logged at debug, never fails the pass (as today) |

</frozen-after-approval>

## Code Map

- `src-tauri/crates/keeper-sync/src/engine.rs` -- pass end `set_state(Watching)` at :11119 (after `mark_synced`); `refresh_pending` flip at :6936; `record_failure` :6735–6800 (`offline` insert, `set_error` on NeedsAttention/Permanent); `note_unit_succeeded` :6891 (the only exit from `offline`); `offline: Mutex<HashSet>` :1178, `transient_failures` :1168; `seed_status` :2247–2280 (reads `get_profile_state`, never the error); `set_state` :2569, `set_error` :2600, `clear_warning` :2694 (`snapshot.error.take().is_some()` gate).
- `src-tauri/crates/keeper-sync/src/db.rs` -- `set_profile_error` :1792 (doc :1776–1791 explains the split), `set_profile_state` :2561, `get_profile_state` :2570; schema `updated_ms INTEGER NOT NULL DEFAULT 0` :78; `upsert` stamps it at :1647.
- `src-tauri/crates/keeper-syncd/src/commands.rs` -- `status` reads `state` (:1498, :3061) — a consumer, read-only here.
- Tests in `engine.rs` -- `a_network_failure_reads_as_offline_not_as_broken` :23231, `a_unit_completing_after_offline_logs_reachable_again_once` :31211 and `a_remote_that_accepts_and_never_answers_is_offline_within_the_deadline` :31158 (the failing-remote idiom), `three_failed_units_through_the_tick_reach_needs_attention` :31093, `stored_last_error` helper :30294.
- `docs/sync.md` -- §12 "Progress and warnings" :1455; `keeper-syncd status` :1629.

## Tasks & Acceptance

**Execution:**
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- a `settled_state(profile_id) -> ProfileState` (or equivalent) that answers `Offline` while the profile is in `offline`, keeps `NeedsAttention`/`MediaAbsent` while the snapshot carries an error or that state, else `Watching`; use it at the pass end (:11119) and in `refresh_pending` (:6936) -- the state that is still true outranks the pass's "I am done".
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- `seed_status` seeds `snapshot.error` from the row (new `db::get_profile_error`, or one `get_profile_runtime` returning both) so `clear_warning`'s retire reaches it -- an error from a previous run is retired like any other.
- [x] `src-tauri/crates/keeper-sync/src/db.rs` -- `set_profile_state`/`set_profile_error` take `now_ms` and stamp `updated_ms`; update the callers -- a reader can tell live from stale.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` tests -- one per matrix row: a failing remote through `tick`/`sync_once` leaves `offline` after a quiet pass (row 1) and returns to `watching` after success (row 2); a persisted error is visible in `status` after `Engine::open` and gone from memory and row after the first successful unit (row 4); a fresh persisted `offline` seeds `Offline`, a stale one `Idle` (row 5); a `NeedsAttention` profile's quiet pass keeps state and error (row 3); `updated_ms` moves on both writers (row 6) -- the rows.
- [x] `docs/sync.md` -- §12: what `keeper-syncd status` reads (`state`, `last_error`, `updated_ms`), that `offline` stands until a unit succeeds, and that an error survives a restart until then -- the record.

Review patches (step 04, loop 1 — amendments to the diff, the diff is kept):
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- `mark_synced` calls `note_unit_succeeded` unconditionally, so a pass whose push failed (recorded by `record_failure`, profile in `offline`) still clears `offline`, retires the error and logs "sync reachable again" — the real writer of `watching`. Fix: `note_unit_succeeded` runs from `mark_synced` only when this pass's legs actually exchanged with the remote and none failed (`do_pull`/`do_push` report it; a pass that touched no remote proves nothing); keep the pass-end `settled_state` after it. Test: drive `sync_once` with a remote whose push fails and whose fetch succeeds (or is skipped) and assert the row still reads `offline` and no "reachable again" line is logged.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- `settle`: keep `NeedsAttention` when the snapshot's *state* is `NeedsAttention` or its `warning`/`error` stands (two writers set it via `warn` without `set_error`: the foreign-volume arm of `volume_ready`, the parked `LfsUploadPending` arm), keep `MediaAbsent`, and never overwrite `Paused`. Table test over every `(offline, state, error)` combination.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- `refresh_pending`: state and phase move under one status lock (the split introduced a window in which a concurrent `record_failure` is clobbered), and the row is written only when the word changed.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- `note_unit_succeeded` is the only exit from `offline`, so it also moves a snapshot wearing `Offline` to the settled word and writes the row (a push with nothing to send never publishes `Syncing`, so `refresh_pending` never sees the edge). Test: unit succeeds after offline without an active phase ⇒ row reads `watching`.
- [x] `src-tauri/crates/keeper-sync/src/db.rs` + `engine.rs` -- `seed_status` reads `state`, `last_error`, `updated_ms` in one query (`get_profile_runtime`); seeds per the amended Always (fresh `offline` stays `Offline`, any error ⇒ `NeedsAttention` with the sentence); a read error is logged at debug, not swallowed. Tests: the legacy `watching`+error row seeds `NeedsAttention` and is retired by the first success; `offline` with `updated_ms` 1 min old seeds `Offline`, 1 h old seeds `Idle`.
- [x] `src-tauri/crates/keeper-sync/src/db.rs` -- `updated_ms` now moves on state and error writes too: widen the column's doc at :272 (`ensure_prune_default`) and its test to "last write of json, state or error", and say so in the §12 table's "written by" cell.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- (owner-applied) `remote_within_reach` keys on the sticky `offline` set, not the word: a seeded `Offline` from a fresh row has no journal row of its own to wait for, so a gate on the word would hold the folder off until the row went stale, or forever. Test `a_seeded_offline_word_does_not_hold_the_first_tick_off`: fresh row, reopen, the first `tick()` walks. `state_of` removed as dead code.

Review patches (step 04, loop 2 — amendments to the diff, the diff is kept):
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- `drain` calls `note_unit_succeeded` on every `Ok` from `execute`, ignoring the `RemoteContact` the unit returned; a `Push` that answers `Skipped` (nothing committed, remote already holds every local commit by local refs) empties the sticky set and logs "reachable again" — the same defect one level down. Fix: reset only on `Reached`; a unit whose error is not `Network` (a 403, a refusal) joins `Reached` — the remote answered — and `record_failure` for such an error removes the profile from `offline` so `settle` shows the `NeedsAttention` it earned rather than a stale `offline`. Tests: a `Skipped` push through `tick` leaves the set and logs nothing; a permanent error after offline reads `needsAttention`, not `offline`.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- `execute` maps `Checkout`, `LfsDownload`, `LfsUpload`, `Verify` to `Reached` on the claim that they always round-trip; `do_checkout` adopts in place, `do_lfs` returns `Ok` under `LfsMode::Disabled` or when the object is already in the store, `verify` asks the server nothing. Fix: those legs answer for themselves — `Reached` only after a transfer, clone or fetch actually completed; `Skipped` otherwise (`Verify` is `Skipped`; its journal kind is never enqueued). Tests for the disabled-LFS and adopted-checkout shapes.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- `has_problem` reads `error` only: `warn` is also used for informational sentences (an open pull request, conflict copies kept, the degraded watcher, a skipped cloud placeholder) that must never make `keeper-syncd doctor` report a folder as stopped; `NeedsAttention` by word is still kept. Fix the `has_problem` doc (the parked-upload arm sets the word with neither warn nor error; the foreign-volume arm is the only warn-only writer). Test: a warning-only snapshot at a `Skipped` pass end settles to `Watching`, the warning still shown.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` -- the pass end still computes `settled_state` and writes in separate lock holds, and a successful pass now writes the row twice (`note_unit_succeeded`'s `persist_state` then `set_state`). Fix: one `settle_and_persist(profile_id)` that decides and assigns under a single status lock and persists only when the word moved; use it at the pass end and in `refresh_pending`.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` + `db.rs` -- `OFFLINE_SEED_MAX_AGE_MS` = backoff cap plus longest attempt (20 min; derive from the backoff's max and `GIT_DEADLINE`/the fetch deadline, not a bare number); a negative age (row stamped in the future) is not believed (`(0..max).contains(&age)`); `get_profile_runtime` reads `state` as `Option<String>` so a NULL word keeps the sentence beside it. Tests: age at the old 10-min boundary still seeds `Offline`; future stamp seeds `Idle`; NULL state with an error seeds `NeedsAttention`.
- [x] `src-tauri/crates/keeper-sync/src/engine.rs` tests -- add: the positive `mark_synced` path through `sync_once` after `go_offline` against the loopback bare remote (set emptied, one "reachable again", row `watching`, `updated_ms` = now); a pure-`Skipped` `sync_once` (push-only fixture, nothing committed) leaves the set and the row; a `RemoteContact::join` table; a disabled profile with a persisted error seeds `Paused` with the sentence kept.
- [x] `docs/sync.md` -- §12: the `state` "written by" cell names `persist_state`'s two callers; the `offline` paragraph says `Paused` outranks the sticky set and that only a leg that actually round-tripped resets `offline` (a push with nothing to send does not); the seed window is 20 min with its reason.
- [x] `_bmad-output/implementation-artifacts/deferred-work.md` -- append: a `NeedsAttention` set by the parked-upload arm of `record_failure` carries no sentence, so any other unit's success retires it (pre-existing: `refresh_pending` flipped it to `Watching` before this story too); the fix is a sentence beside the word and a retire keyed to the upload moving.

**Acceptance Criteria:**
- Given a folder whose push has failed with a network error, when later passes commit nothing, then `keeper-syncd status` reads `offline` until a push succeeds.
- Given a folder that stopped with an error in a previous run, when the next run's first unit succeeds, then the row's `last_error` is `NULL` and `updated_ms` is that moment.
- Given any state or error write, then the row's `updated_ms` is the engine clock at the write.

## Spec Change Log

- 2026-09-10, loop 2 close-out: the Always sentence "the only source that ends `Offline` is a unit succeeding" amended to include a failure the remote answered with — prescribed by the loop-2 patch (a 403 after an outage must read `needsAttention`, not a stale `offline`) and pinned by a test; the parked `LfsUploadPending` arm deliberately does not exit. Flagged to the owner at the step-05 summary.

- 2026-09-10, review loop 2 (patch, code kept; two frozen details amended under the owner's 2026-09-10 authorisation): (a) the matrix row "Restart with persisted `offline`" and the row-5 task still said "seeds `Idle`" against the amended Always — aligned; (b) the seed window was "two remote-poll intervals (10 min)", equal to the retry backoff's cap with a strict `<`, so `status` would print `idle` between a capped daemon's stamps — now the cap plus the longest attempt (20 min). Findings folded into the patches: the drain path reset `offline` on any `Ok` unit whatever its `RemoteContact`; `execute` mapped checkout/LFS/verify to `Reached` without a round trip; `has_problem` promoted informational warnings to `NeedsAttention`; the pass end kept a lock split and a double row write; `get_profile_runtime` failed on a NULL `state`; a negative age was believed. KEEP: `RemoteContact` and its fold, `settle`'s `Paused` > `Offline` > `NeedsAttention` > `MediaAbsent` > `Watching`, the sticky-set gate in `remote_within_reach`, the per-row tests. Deferred: a `NeedsAttention` set by a parked upload carries no sentence and is retired by any other unit's success (pre-existing; see deferred-work).

- 2026-09-10, review loop 1 (intent_gap resolved by the owner, code kept): the reviewers showed that `keeper-syncd status` is a fresh process whose `seed_status` mapped a persisted `offline` to `Idle`, so the acceptance criterion "reads `offline` until a push succeeds" could not hold under the original Always sentence. The owner amended the frozen sentence to seed `Offline` from a fresh row (`updated_ms` < 10 min) and `NeedsAttention` from any row carrying an error. Known-bad state avoided: the verb printing `idle` beside a row that says `offline`. KEEP: the writer split, `settle`'s precedence, the per-row tests, the §12 doc section, the `updated_ms` stamping. Also found and folded into the patches below: `mark_synced` calls `note_unit_succeeded` on every pass end, which emptied the `offline` set on passes that reached no remote (13 false "reachable again" lines on hesperia 2026-09-09) — the actual source of the `watching` row; the pass-end guard as first written ran after it and was inert.

## Verification

**Commands:**
- `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-sync offline status_ error` -- expected: the new tests and the existing offline/NeedsAttention tests pass.
- `cargo nextest run --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-syncd` -- expected: all green (syncd's `status` consumer unchanged).
- `cargo clippy --manifest-path src-tauri/Cargo.toml -p keeper-sync -p keeper-syncd --all-targets -- -D warnings && cargo fmt --manifest-path src-tauri/Cargo.toml -p keeper-sync -- --check` -- expected: clean.

## Suggested Review Order

**The signal — did this pass reach the remote at all?**

- Entry point: three answers a leg can give, and the fold that lets `Failed` outrank `Reached` outrank `Skipped`.
  [`engine.rs:123`](../../src-tauri/crates/keeper-sync/src/engine.rs#L123)
- The reset happens only on `Reached`; a pass that touched no remote proves nothing.
  [`engine.rs:7721`](../../src-tauri/crates/keeper-sync/src/engine.rs#L7721)
- Every unit answers for itself: the sticky set and the counter reset on `Reached`, the sentence retires on any completion.
  [`engine.rs:7231`](../../src-tauri/crates/keeper-sync/src/engine.rs#L7231)
- A clone reaches; an adoption does not.
  [`engine.rs:7867`](../../src-tauri/crates/keeper-sync/src/engine.rs#L7867)
- A failure the remote answered with also ends `offline` — the earned word must show.
  [`engine.rs:7049`](../../src-tauri/crates/keeper-sync/src/engine.rs#L7049)

**The word — what a pass end is allowed to write**

- Decide and assign under one lock; write the row only when the word moved.
  [`engine.rs:2745`](../../src-tauri/crates/keeper-sync/src/engine.rs#L2745)
- The precedence: `Paused` > sticky `Offline` > `NeedsAttention` > `MediaAbsent` > `Watching`.
  [`engine.rs:2812`](../../src-tauri/crates/keeper-sync/src/engine.rs#L2812)
- A problem is an error, not a warning — informational sentences never make a folder "stopped".
  [`engine.rs:2783`](../../src-tauri/crates/keeper-sync/src/engine.rs#L2783)
- The pass end and the pending refresh both go through it.
  [`engine.rs:11611`](../../src-tauri/crates/keeper-sync/src/engine.rs#L11611) · [`engine.rs:7304`](../../src-tauri/crates/keeper-sync/src/engine.rs#L7304)

**The seed — what a fresh process believes**

- Any error ⇒ `NeedsAttention`; a fresh `offline` is believed, a stale one re-probed; a future stamp is not believed.
  [`engine.rs:2399`](../../src-tauri/crates/keeper-sync/src/engine.rs#L2399)
- The window: the backoff cap plus the longest attempt, derived, not a bare number.
  [`engine.rs:581`](../../src-tauri/crates/keeper-sync/src/engine.rs#L581) · [`backoff.rs:23`](../../src-tauri/crates/keeper-sync/src/backoff.rs#L23)
- One query for state, sentence and stamp; a NULL word keeps the sentence.
  [`db.rs:2624`](../../src-tauri/crates/keeper-sync/src/db.rs#L2624)
- The tick gate reads the set, so a seeded word never holds the first tick off.
  [`engine.rs:4704`](../../src-tauri/crates/keeper-sync/src/engine.rs#L4704)
- Both writers stamp `updated_ms`.
  [`db.rs:2579`](../../src-tauri/crates/keeper-sync/src/db.rs#L2579) · [`db.rs:1799`](../../src-tauri/crates/keeper-sync/src/db.rs#L1799)

**The record**

- What `keeper-syncd status` reads, and when each column is allowed to change.
  [`sync.md:1618`](../../docs/sync.md#L1618)

**Tests**

- The hesperia shape through a real `sync_once`: the drained push fails, the row stays `offline`, no "reachable again".
  [`engine.rs:32664`](../../src-tauri/crates/keeper-sync/src/engine.rs#L32664)
- The positive path: a pass that round-trips after an outage empties the set once.
  [`engine.rs:32507`](../../src-tauri/crates/keeper-sync/src/engine.rs#L32507)
- A unit that made no round trip leaves the set; a pass that skipped every leg leaves `offline` standing; an answered failure reads `needsAttention`.
  [`engine.rs:32507`](../../src-tauri/crates/keeper-sync/src/engine.rs#L32507) · [`engine.rs:32614`](../../src-tauri/crates/keeper-sync/src/engine.rs#L32614) · [`engine.rs:32408`](../../src-tauri/crates/keeper-sync/src/engine.rs#L32408)
- The precedence table, every combination; the fold table.
  [`engine.rs:32309`](../../src-tauri/crates/keeper-sync/src/engine.rs#L32309) · [`engine.rs:32238`](../../src-tauri/crates/keeper-sync/src/engine.rs#L32238)
- The legacy `watching`+error row every install carries, retired by the first success.
  [`engine.rs:32263`](../../src-tauri/crates/keeper-sync/src/engine.rs#L32263)
- Fresh vs stale `offline` at seed; the seeded word does not hold the first tick off.
  [`engine.rs:32015`](../../src-tauri/crates/keeper-sync/src/engine.rs#L32015) · [`engine.rs:32072`](../../src-tauri/crates/keeper-sync/src/engine.rs#L32072)
- Row writes happen only when the word moved; both writers stamp.
  [`engine.rs:32802`](../../src-tauri/crates/keeper-sync/src/engine.rs#L32802) · [`engine.rs:32854`](../../src-tauri/crates/keeper-sync/src/engine.rs#L32854)
