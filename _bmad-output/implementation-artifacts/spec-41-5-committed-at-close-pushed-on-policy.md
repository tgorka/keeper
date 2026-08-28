---
title: 'Story 41.5: Committed at Close, Pushed on Policy'
type: 'feature'
created: '2026-08-07'
status: 'done'
blocking_condition: ''
baseline_revision: 'd5d2169'
final_revision: 'a9e8d0f406c3e091d42ae688b08a84918084fd46'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-41-a-finished-segment-is-already-on-the-drive.md'
---

<intent-contract>

## Intent

**Problem:** Stories 41.1–41.4 built the vocabulary — a profile can hold recordings, a destination
resolves to one, an in-progress segment is invisible, and a finished file can say so — and nothing
speaks it. The `RecordingEvent::SegmentClosed` arm of the driver sink writes the ledger and notifies
nobody. That closure is the single integration point in the whole epic, and it is empty.

**Approach:** on `SegmentClosed`, the sink appends its ledger line and asserts the finished path to the
destination profile, so the segment is committed on the next pass rather than after a settle window.
The session's LFS rule is written into `.gitattributes` ONCE at session start, not on first commit, so
the working tree does not change under a running recorder. `manifest.json` is written once at finalize
(FR-146), so a four-hour session commits 48 segments and rewrites no metadata file 48 times. Push obeys
the profile's own policy: `Immediate` per commit, `SessionEnd` once at finalize, `Window` in the quiet
hours. Everything else — LFS staging at the 4 MiB threshold, the outstanding-object gate that refuses
to publish a pointer ahead of its object — is unchanged and keeps working.

## Boundaries & Constraints

**Always:**
- Capture never degrades. Every failure here — no profile, paused profile, absent volume, unreachable
  remote, rejected push, LFS upload dying at 90 % — downgrades DURABILITY and says so in a log line.
  None of them stop the recorder or fail a command.
- Durability and publication are different promises. Committing is local, cheap and immediate; pushing
  a 2 GB object is neither, and doing it during the meeting eats the uplink the meeting runs on.
- The working tree does not change under a running recorder: `.gitattributes` is written at session
  start, and `manifest.json` once at finalize.
- The assertion is the one from 41.4 and it is best-effort by contract: the recorder never handles an
  error from it.
- A segment closed while the volume is absent is committed when it returns, and is never deleted in the
  meantime.
- Counters, not inspection: the AC is stated in commits, writes and pushes per session, and the tests
  assert those counts.

**Block If:**
- Asserting from the sink required handing the recorder an `Engine`. It does not: 41.4 delivered
  `Engine::finished_tap()`, a cloneable handle over a bounded channel.

**Never:**
- No push during a session under the default policy, and no policy read per segment that could change
  mid-session — the policy in force is the one read at start.
- No re-commit of a file the engine just committed, and no metadata rewrite per rotation.
- No new IPC surface, no VM, no bindings. Story 41.6 is what surfaces this.

## I/O & Edge-Case Matrix

Destination = a recordings-flagged profile; template default; 48 rotations over four hours.

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Session start | destination resolves to a profile | the media extension's LFS rule is written to `.gitattributes` exactly once | none |
| Session start, rule present | `.gitattributes` already carries it | no write at all | none |
| Segment closed | one rotation | one ledger line appended; the final path asserted to that profile | none |
| Four-hour session | 48 rotations | 48 commits, ONE `.gitattributes` write, ONE `manifest.json` write, a bounded journal — asserted by counters | none |
| Push policy `SessionEnd` | the default | no push until finalize; exactly one push after it | none |
| Push policy `Immediate` | configured | one push per commit | none |
| Push policy `Window` | outside the quiet hours | commits accumulate; the push happens in the window | none |
| Remote unreachable throughout | every segment | every segment is committed locally; the outstanding push drains on reconnect; no pointer is published ahead of its object | none |
| Volume absent (`MediaAbsent`) | a segment closes | committed when the volume returns; never deleted meanwhile | none |
| Destination is a plain folder | no profile | the sink appends the ledger line and asserts nothing; no sync work at all | none |
| Profile paused | a segment closes | the assertion is recorded (41.4) and honoured on resume; nothing fails | none |
| Finalize | the session ends | `manifest.json` written once; the `SessionEnd` push fired once | none |
| Finalize with no profile | plain folder | manifest written; no push attempted | none |
| Assertion refused | path outside the recordings root | logged, ordinary settle path, recorder unaffected | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper/src/ipc.rs` — the `SegmentClosed` arm of the driver sink (the epic's single
  integration point), the session-start hook that writes the LFS rule, and the finalize hook that
  writes `manifest.json` once and fires the `SessionEnd` push.
- `src-tauri/crates/keeper-sync/src/engine.rs` — an API to ensure the session's LFS rule exists
  (`.gitattributes`, written once), and one to push a profile now under its policy; both total and
  logged rather than fatal.
- `src-tauri/crates/keeper-sync/src/profile.rs` — `PushPolicy` is read here (41.1).
- Read-only: `lfs::stage::applies` (the 4 MiB threshold), `do_push`'s `lfs_uploads_outstanding` gate,
  and 41.4's `finished_tap`.

## Tasks & Acceptance

**Execution:**
- [ ] Session start: resolve the destination profile once, write the LFS rule once, remember the policy
      in force for this session.
- [ ] `SegmentClosed`: ledger line + finished assertion, both best-effort.
- [ ] Finalize: one `manifest.json` write, then the policy's push.
- [ ] Engine: ensure-LFS-rule and push-now APIs, total and logged.
- [ ] Tests, by counters: the 48-rotation synthetic session; each push policy; the unreachable remote;
      the absent volume; the plain-folder destination.

**Acceptance Criteria:**
- Given a four-hour synthetic session of 48 rotations, when it completes, then there are 48 commits,
  one `.gitattributes` write, one `manifest.json` write and a bounded journal, asserted by counters.
- Given `push = SessionEnd`, when the session runs, then no push occurs until finalize and exactly one
  does after it.
- Given an unreachable remote throughout, when the session completes, then every segment is committed
  locally and the outstanding push drains on reconnect without publishing a pointer ahead of its
  object.
- Given a segment closed while the profile's volume is absent, when the volume returns, then the
  segment is committed and was never deleted.

## Design Notes

(Written at the end of implementation.)
