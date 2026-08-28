---
title: 'Story 41.6: Durability You Can Read'
type: 'feature'
created: '2026-08-07'
status: 'done'
blocking_condition: ''
baseline_revision: '69f3a22'
final_revision: '5cb8ed986bd371ccc2de2be56bb317d135f229fc'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-41-a-finished-segment-is-already-on-the-drive.md'
---

<intent-contract>

## Intent

**Problem:** Story 41.5 made a closed segment commit itself and, on policy, push itself — and the
person recording cannot see any of it. The banner says a recording is running; it does not say whether
what has been recorded so far would survive this laptop being dropped. Worse, when a push is rejected
the only thing that exists is a sync error in a log, which is both invisible and the wrong words: the
recording did not fail, its publication did.

**Approach:** every session carries one durability state — `local`, `committed`, `pushed`,
`verified` — derived from what the engine already knows, carried on the status the recording surface
already polls, and reduced into the tray composition that already exists. The banner gains ONE line in
the recorder's own words: "on this Mac" → "committed" → "on the drive". A rejected push reads
"recorded, not pushed", with the reason available and the state left at `committed` — never a generic
sync error, never a modal, and never a stop.

## Boundaries & Constraints

**Always:**
- The words are the recorder's, not git's (UX-DR48). "committed" and "on the drive" describe what
  happened to the user's recording; `push rejected: non-fast-forward` is the reason, available, not the
  headline.
- The state is DERIVED, never stored: it is a reading of the ledger and the engine, so it cannot go
  stale or disagree with the thing it describes.
- It never regresses within a session. A session that reached `pushed` does not read `committed` again
  because a later segment is still in flight — the state describes the session's floor, which is what
  "would this survive?" actually asks.
- Recording still wins the tray icon; sync never forces tray presence (the epic's rule, unchanged).
- A plain-folder destination is `local` and says so plainly. There is no profile, so there is no
  further promise to make.
- Capture never degrades: every failure to COMPUTE the state degrades to the last known state and a
  log line, and nothing here can stop or slow the recorder.
- On a build without the recording capability, none of it renders.

**Block If:**
- The state needed a new poll or a new stream. It does not: `recording_status` is already polled at
  ~1 Hz by the surface that shows the banner.

**Never:**
- No modal, no toast, no error dialog on a rejected push.
- No new persistence: nothing about durability is written to disk.
- No blocking call on the poll path — the engine answer must be cheap or cached, never a network round
  trip.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Plain folder | destination is a folder | `local`, "on this Mac" | none |
| First segment closing | profile destination, nothing committed yet | `local` | none |
| After a commit | the segment is in a commit | `committed`, "committed" | none |
| After a push | the commit is on the remote | `pushed`, "on the drive" | none |
| Verified | the engine has verified the objects | `verified` (same words as pushed, or its own — one line, honest either way) | none |
| Mid-session mix | segment 3 pushed, segment 4 still local | the session's FLOOR is what shows; it never regresses | none |
| Network killed mid-session | pushes fail from segment 5 on | stays `committed`; the tray shows its warning glyph; recording continues to disk | none |
| Protected-branch rejection | remote refuses the push | "recorded, not pushed" with the reason available; state stays `committed` | none |
| Engine unavailable (no git) | destination resolved to a folder anyway | `local`; no error surfaces | none |
| Engine query fails | a transient read failure | the last known state persists, plus one log line | none |
| No recording capability | iOS or a build without it | nothing renders; no command is called | none |
| Session ends | finalize | the final state is whatever the session actually reached, and the banner's last word is honest | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-sync/src/engine.rs` — a cheap, non-networking durability read for a path
  inside a profile (is it committed? is its commit pushed? is there a recorded push failure and what
  did it say?).
- `src-tauri/crates/keeper-core/src/vm.rs` — `RecordingDurabilityVm { state, detail }` +
  `RecordingDurabilityState` enum; `RecordingStatusVm` carries it. Bindings regenerate.
- `src-tauri/crates/keeper/src/ipc.rs` — the derivation on the status path, the never-regress floor,
  and the degrade-to-last-known rule.
- `src-tauri/crates/keeper/src/tray.rs` — the reduction into the existing composition.
- `src/components/recording/active-recording-banner.tsx` — the one line.

## Tasks & Acceptance

**Execution:**
- [ ] Engine: the durability read, cheap and local-only.
- [ ] VM + enum + `RecordingStatusVm` field; regenerate bindings.
- [ ] Derivation with the never-regress floor and the degrade rule.
- [ ] Tray reduction (recording still wins the icon).
- [ ] The banner's line and the "recorded, not pushed" reading with its reason.
- [ ] Tests: every matrix row, Rust and frontend.

**Acceptance Criteria:**
- Given a real rotation, when segments commit and push, then the banner's line advances and never
  regresses.
- Given the network killed mid-session, then the banner stays at "committed", the tray shows its
  warning glyph, and recording continues to disk.
- Given a protected-branch rejection, then the line reads "recorded, not pushed", the reason is
  available, and the state stays `committed`.
- Given a build without the recording capability, then none of this renders.
- Given `bun run bindings:check`, then it exits 0.

## Design Notes

(Written at the end of implementation.)
