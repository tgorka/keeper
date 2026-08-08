---
title: 'Story 41.4: The Gate Learns the Word "Finished"'
type: 'feature'
created: '2026-08-07'
status: 'review'
blocking_condition: ''
baseline_revision: '8b3e0e2'
final_revision: ''
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-41-a-finished-segment-is-already-on-the-drive.md'
---

<intent-contract>

## Intent

**Problem:** The completeness gate's tier-2 test is quiescence: nothing has changed for `settle_ms`.
For a rotated segment that is both too slow and strictly weaker than the truth — `keeper-rec` closed
the file with `finishWriting` and will never touch those bytes again. The file is not quiescent, it is
FINISHED, and only the process that owns the writer can say so.

**Approach:** `StabilityGate::note_finished(path, now_ms)` lets an authoritative producer assert that
one absolute path is complete, so the next `collect_stable_changes` treats it as stable without waiting
out the window. This skips tier 2 ONLY: tier-0 exclusion still hides a `.partial`, and tier-4
verify-on-read still runs. `Engine::note_finished_path(profile_id, path)` exposes it guarded — the path
must resolve inside that profile's `recordings_root()` — and every unusable assertion (unknown
profile, disabled profile, path outside the root) degrades to the ordinary settle path rather than
becoming an error the recorder has to handle (NFR-31).

## Boundaries & Constraints

**Always:**
- Tier 0 wins: an excluded path that is asserted stays excluded.
- Tier 4 still runs: this is an assertion about writing being over, not about bytes being correct.
- Idempotent: asserting the same path twice is the same as asserting it once.
- A paused profile REMEMBERS the assertion and honours it when it resumes — losing it would mean the
  segment waits out a settle window that the pause made meaningless.
- Degrade, never fail: the recorder gets no error to handle, and an unusable assertion produces a
  `warn` line and the ordinary path.
- The assertion travels in the same direction as `watch_tap` (AD-68) — the recorder is not handed an
  `Engine`.

**Block If:**
- The gate's state could not carry an assertion across a restart or a pause. It can: the gate exports
  and imports its entries already.

**Never:**
- No new tier, no configuration knob, no way for a caller outside the producer's position to reach it
  (FR-135).
- No shortening of the default settle window for anything else.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Asserted file | written, then asserted | committed on the NEXT tick, with no settle wait — a test that fails if it takes more than one tick | none |
| Unasserted file | written only | still waits the window | none |
| Outside the root | a path elsewhere in the profile | refused, and it never becomes stable early | typed + `warn` |
| Unknown profile | a bad id | degrades to the ordinary path | `warn` |
| Disabled profile | paused | the assertion is recorded and honoured on resume | none |
| Excluded path | `x.mov.partial` asserted | stays excluded | none |
| Twice | the same path asserted twice | idempotent | none |
| Restart | asserted, then the process restarts | the assertion survives with the gate's exported state | none |
| Verify-on-read | asserted but unreadable at commit | tier 4 still refuses it | typed |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/stability.rs` — `note_finished`, beside `note_close_write` (the
  Linux `IN_CLOSE_WRITE` fast path this generalises) and the `prime_stable` story 40.4 added.
- `src-tauri/crates/keeper-sync/src/engine.rs` — `note_finished_path`, the recordings-root guard, and
  the fan-out seam.
- Read-only: `profile.rs`'s `recordings_root()` (story 41.1) and `exclude.rs`'s tier-0 rules.

## Tasks & Acceptance

**Execution:**
- [x] `StabilityGate::note_finished` with a doc stating exactly which tier it skips and which still
      apply.
- [x] `Engine::note_finished_path` with the recordings-root guard and the degrade-never-fail contract.
- [x] The assertion's delivery direction matching `watch_tap`.
- [x] Tests: every matrix row, including the one-tick proof and the paused-then-resumed case.

**Acceptance Criteria:**
- Given a file written and immediately asserted, when the engine ticks, then it is committed on that
  tick, proven by a test that fails if it takes more than one.
- Given a path outside the recordings root, when it is asserted, then it is refused and never becomes
  stable early.
- Given an excluded `.partial`, when it is asserted, then it stays excluded.
- Given the same path asserted twice, then the outcome is identical to asserting it once.
- Given a paused profile, when it resumes, then the assertion is still honoured.

## Design Notes

**One mechanism, two warrants.** `note_finished` and story 40.4's `prime_stable` want the identical
state change — an entry backdated by `SETTLE_CEILING_MS`, so `verdict`'s first condition clears it —
so they share a private `StabilityGate::declare_settled` and differ only in their doc and in who is
entitled to call them. That distinction *is* the safety argument for skipping tier 2, which is why
`declare_settled` is not public (FR-135): a primed path has been quiet for as long as it existed
under its old name, while a finished path may be a millisecond old and is vouched for by the process
holding the writer.

**Refusal vs. error, resolving a contradiction in the inputs.** The epic says a path outside the
recordings root is "a typed error and a `warn` line" two sentences before it says every unusable
assertion "degrades to the ordinary settle path — never to an error the recorder must handle". The
Boundaries section, the matrix's *Expected* column and the ACs all take the second reading, so that
is what shipped: unknown profile, no recordings root, and outside the root are `Ok(false)` + `warn`.
`Err` is reserved for a request the guard cannot evaluate at all — a relative path, or one carrying
a `..` component, since `Path::starts_with` compares components and would accept
`<root>/../../etc/passwd`. A producer cannot reach either state by accident; both are programming
errors, and `drain_finished_assertions` logs and discards them so nothing reaches the recorder.

**A paused profile is not a refusal.** It is the case the assertion matters most in, so the
assertion is recorded and mirrored to `file_state` exactly as for a running profile. `set_enabled`
drops the in-memory gate, and the resume's first walk re-imports it — which is what the paused test
asserts, by checking the gate map is empty before the commit that honours the assertion.

**The delivery direction (AD-68).** `watch_tap`'s outbound seam had no inbound sibling, so one was
added rather than exporting an `Engine`: `Engine::finished_tap() -> FinishedTap`, a cloneable
handle over a bounded `tokio::sync::mpsc` (64), whose only method states one fact about one path and
returns `bool` rather than `Result`. `Engine::tick` drains it before the profile loop — before the
`enabled` filter, so a paused profile's assertion still lands — into a local vector, so a producer's
`try_send` never queues behind the gate lock and a SQLite write.

**The one-tick proof.** Counted, not timed. The control half writes an unasserted segment and shows
it needs two passes plus `effective_settle_ms + 1`; the asserted half then loops `commit_local`
under a `PASS_BUDGET` of 1 and fails on the second pass. The injected clock is never advanced inside
that loop, and the test asserts `platform.now_ms()` is unchanged across it, so no amount of extra
passes could substitute for the window. Verified by mutation: with `note_finished` stubbed to record
nothing, 4 of the 9 new tests fail, including this one at "still held after 1 pass(es)".
