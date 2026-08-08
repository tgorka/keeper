---
title: 'Story 41.3: `.partial` While Writing, Final on Close'
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

**Problem:** A segment being written is a half-written file, and epic 26's completeness gate exists
because a half-written file committed once is a corrupt file forever. The gate's answer today is
quiescence — five seconds of silence — which is both slow and epistemically wrong for a file that may
be written again in the sixth second. Meanwhile nothing marks an in-progress segment as in-progress,
so the only thing standing between a growing `.mov` and a commit is a timer.

**Approach:** the sidecar writes each segment as `<name>.<ext>.partial` and renames it atomically to
its final name the moment `finishWriting` returns, emitting `SegmentClosed` with the FINAL path. Sync
learns one tier-0 name rule — a `.partial` suffix is excluded — so an in-progress segment is invisible
to `Engine::pending`, to the commit path and to the activity feed, without teaching the sync crate
anything about recording (AD-69). Startup recovery learns the suffix too: a `.partial` left by a crash
is finalised or discarded by the rules that already exist, never committed.

## Boundaries & Constraints

**Always:**
- The rename is atomic and within the same directory — no copy fallback, ever, because a copy of a
  multi-gigabyte segment is both slow and a second chance to be interrupted.
- `SegmentClosed.path` is the final path. Nothing downstream should ever learn the temporary name.
- The exclusion is a SUFFIX rule, not a glob over a directory: git sees a rename as add + delete, so
  the rule is what has to hide the partial, and it must hold wherever the file is.
- A `.partial` that recovery finalises enters the ledger; one it discards is removed and said so.
- Capture never degrades: if the rename fails, the segment is still on disk under its temporary name
  and the failure is reported, not swallowed.

**Block If:**
- The sidecar could not rename without a copy on the recording volume. It can: same directory, same
  filesystem.

**Never:**
- No knowledge of recording inside `keeper-sync` beyond the suffix string.
- No change to the segment ledger's shape, and no second in-progress marker (no lock file, no sidecar
  state file).

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Writing | a rotation in flight | the growing file is `screen-0003.mov.partial` | none |
| Close | `finishWriting` returns | atomic rename to `screen-0003.mov`; `SegmentClosed.path` is the final one | none |
| Sync during a recording | destination inside a synced profile | `Engine::pending` never lists a `.partial`, asserted while a real rotation is in flight | none |
| Commit path | a `.partial` present at commit time | never staged, never committed, never in the activity feed | none |
| Killed recorder | SIGKILL mid-segment | exactly one `.partial` remains and no commit references it | none |
| Recovery: finalisable | a `.partial` with usable bytes | finalised, present in the ledger, and the log says so | none |
| Recovery: unusable | a `.partial` with nothing usable | removed, and the log says so | none |
| Rename fails | destination unwritable | the failure is reported; the bytes are still on disk | typed |
| A user's own `.partial` | an unrelated `x.partial` in a synced folder | excluded like any tier-0 name — documented, since it is a behaviour change for that file | none |

</intent-contract>

## Code Map

- `tools/keeper-rec/` (Swift) — the segment writer's output path and the rename after `finishWriting`,
  plus the `SegmentClosed` payload.
- `src-tauri/crates/keeper-sync/src/exclude.rs` — the `.partial` suffix rule in `BUILTIN_EXCLUDES`.
- `src-tauri/crates/keeper-core/src/recording.rs` — the orphan-recovery rules that must learn the
  suffix.
- Read-only: `keeper/src/ipc.rs`'s `SegmentClosed` arm (story 41.5 wires the assertion; this story only
  guarantees the path it will receive is final).

## Tasks & Acceptance

**Execution:**
- [ ] Swift: write `<name>.<ext>.partial`, atomic rename on close, emit the final path.
- [ ] Swift tests: the rename is atomic, the emitted path is final, and a killed writer leaves one
      `.partial`.
- [ ] `exclude.rs`: the suffix rule, with a doc saying why a suffix and not a glob.
- [ ] `keeper-core` recovery: finalise-or-discard a leftover `.partial`, logging which.
- [ ] Rust tests: `pending` never lists a `.partial`; the commit path ignores one; recovery both ways.

**Acceptance Criteria:**
- Given a recording with a rotation in flight, when `Engine::pending` runs for the destination
  profile, then no `.partial` path is listed.
- Given a killed recorder, when the tree is inspected, then exactly one `.partial` exists and no commit
  references it.
- Given that `.partial`, when recovery runs, then it is either finalised and present in the ledger or
  removed, and the log says which.
- Given a segment close, when `SegmentClosed` is emitted, then its path is the final name and the
  rename performed no copy.

## Design Notes

(Written at the end of implementation.)
