---
title: 'Story 42.1: A Session Is a Row'
type: 'feature'
created: '2026-08-07'
status: 'review'
blocking_condition: ''
baseline_revision: 'd600eeb'
final_revision: ''
review_loop_iteration: 1
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-42-the-recordings-archive.md'
---

<intent-contract>

## Intent

**Problem:** stories 21.5 and 22.3 gave a session a title, participants, a note, times, tags and custom
fields, and all of it is written into `manifest.json` and — apart from `meta.title` — never read
again. Recording metadata today is write-once, search-never, list-never: a folder of folders. The app
already contains a working archive (`archive.db`, one serialized writer, an FTS index) that knows only
about Matrix events, and a session has no representation in it at all.

**Approach:** a session becomes a row. Two tables in the archive database — `recordings` and
`recording_segments` — written through the EXISTING archive writer channel as new message variants, so
there is still exactly one writer and no second connection to reason about. The row is inserted at
session start and completed at finalize, with `INSERT OR REPLACE` semantics so a duplicate finalize is
not a duplicate row. `durability` follows epic 41's state as it advances. And because a database is a
cache of what the folders already say, `rebuild_from_disk` re-derives every row by walking the session
tree and reading manifests.

## Boundaries & Constraints

**Always:**
- One writer. Every write goes through the archive's existing serialized writer channel — no second
  connection, no direct handle from the recording path.
- The database is derivable. Deleting `archive.db` loses nothing that the manifests do not carry, and
  `rebuild_from_disk` proves it by reproducing the rows.
- Paths in rows are RELATIVE to the destination root, never absolute (FR-145's rule, extended to the
  index): the row must survive the folder being moved by a retitle and the tree being cloned onto
  another machine.
- Additive migrations only, through the helper the archive already uses, and idempotent across
  repeated opens.
- A manifest written by an older build loads with the missing columns defaulted and no error — the
  index must never be the thing that refuses a recording.
- Nothing here blocks or slows the recorder: the row write is a message on a channel, and a failure to
  index is logged, never surfaced as a recording failure.

**Block If:**
- The archive's writer channel could not carry a new message kind without a second writer. It can:
  `ArchiveMsg` is an enum and the writer task already switches on it.

**Never:**
- No FTS in this story (42.2), no IPC command or VM (42.3), no note stub (42.4), no tag resolution
  (42.5).
- No absolute path in any column.
- No deletion of a row because a folder is missing: absence on disk is a fact for a later story to
  present, not a reason to forget the session.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Session start | a recording begins | one row with the identity, the relative path, the root kind, the profile (when there is one) and `started_ts` | none |
| Finalize | the session ends | the same row completed: `ended_ts`, title, participants, note, tags, custom, codec, dimensions, fps, manifest version | none |
| Duplicate finalize | the finalize path runs twice | one row, not two — `INSERT OR REPLACE` on the session id | none |
| Durability advances | 41.6's state moves `local` → `committed` → `pushed` | the column follows; it never regresses | none |
| Segments | each closed segment | one `recording_segments` row per (session, index, track), with a RELATIVE path | none |
| Rebuild | `archive.db` deleted, 50 sessions on disk | `rebuild_from_disk` reproduces byte-identical rows for every field the manifests carry | none |
| Older manifest | no `sessionId`, no meta | the row is written with defaults and no error; the session id falls back to a derived one and the fallback is documented | none |
| Missing folder | a row whose folder was deleted | the row stays; nothing is silently forgotten | none |
| Migration idempotency | three successive opens | the schema settles once and the second and third opens change nothing | none |
| Writer failure | the channel is closed | logged; the recorder is unaffected | none |
| Retitled session | 40.4 moves the folder | the row's relative path is updated, and `session_id` is untouched | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/archive/recordings.rs` (new) — the two tables, their migration, the
  insert/complete/durability/segment writes, and `rebuild_from_disk`.
- `src-tauri/crates/keeper-core/src/archive/db.rs` — the migration helper and the schema's home.
- `src-tauri/crates/keeper-core/src/archive/mod.rs` — `ArchiveMsg` gains the recording variants; the
  single writer task learns them.
- `src-tauri/crates/keeper/src/ipc.rs` — the recording path sends those messages: start, each closed
  segment, finalize, and the durability advance.
- Read-only: `archive/fts.rs` (42.2's shape), `keeper-core/src/recording.rs` (the manifest that
  `rebuild_from_disk` reads).

## Tasks & Acceptance

**Execution:**
- [x] The two tables + additive, idempotent migration.
- [x] The writer-channel variants and their handling in the one writer task.
- [x] Insert at start, complete at finalize, per-segment rows, durability updates.
- [x] `rebuild_from_disk`, and a production caller for it.
- [x] Shell wiring on the recording path, best-effort throughout.
- [x] Tests: every matrix row, plus the 50-session rebuild.

**Acceptance Criteria:**
- Given a session start and a finalize, then one row exists, completed, and a duplicate finalize does
  not produce a second.
- Given `archive.db` deleted and 50 sessions on disk, when `rebuild_from_disk` runs, then every row is
  reproduced byte-identically for every field the manifest carries.
- Given a manifest from an older version, then the row loads with defaults and no error.
- Given three successive opens, then the migration is idempotent.

## Design Notes

**The row is a cache, and that had to be made operationally true, not just architecturally true.**
The first pass shipped `rebuild_from_disk` with no production caller: three separate degrade paths
justified themselves in comments with "a rebuild fixes it later", and nothing could ever run one. It
is now sent as `ArchiveMsg::RebuildRecordings` from the startup orphan-recovery pass, which already
walks the same tree under the same root. It runs on the writer's own connection for the reason
everything else does — a rebuild reads and rewrites exactly the rows the recorder appends to, so a
second connection would be worst precisely there — and startup is the one moment nothing is
recording.

**`INSERT OR REPLACE` had to learn a second exemption.** `durability` was already floored so a late
write could not walk epic 41's state backwards. `codec`, `fps`, `width`, `height` and a segment's
`closed_ts` needed the same protection for the opposite reason: they are the only facts a row holds
that NO manifest carries, so a rebuild — which can only reproduce what the folders say — sends them
as `None` and would have nulled them. Both writes now `COALESCE` against the stored value, so `Some`
wins and `None` preserves. The 50-session acceptance test was rewritten because its comparison side
had been hand-built with these fields already null, which made it pass over the bug rather than
catch it.

**A retitle repoints the row rather than rewriting it.** Matrix row 11 was unimplemented: the Story
40.4 rename moved the folder and never told the index. It now sends `ArchiveMsg::MoveRecording` with
the new relative path and nothing else. Sending a freshly derived row instead would have been the
obvious move and the wrong one — a retitle knows no codec and no frame rate, so it would have
written nulls over the two columns above. `session_id` is untouched, which is the entire reason the
row is keyed on identity rather than location. A pre-Story-40.3 session, whose id is derived FROM
its path, mints a different key at the new location and so repoints nothing; that is the consequence
`fallback_session_id` already documents, and it is pinned by a test so the alternative reading —
silently repointing some other session's row — cannot creep in.

**The RFC 3339 parser was the riskiest thing in the story.** It is hand-rolled because `keeper-core`
takes no date dependency, and it had two panics reachable from any `manifest.json` on disk: the
fractional-seconds loop read a fixed three bytes past the dot rather than the digit run (so `.5+01:00`
underflowed `u8`), and the `±HHMM` arm sliced a `&str` at byte offsets with no char-boundary check.
Both are fixed, the day is now validated against the length of its own month, and the tests assert
absolute epoch constants — including 1900, 2000 and a pre-1970 date, the three cases
`days_from_civil`'s century and era terms exist for and none of which the original test reached.

**A duplicated session folder is a fact about the tree, not a reason to lose a row.** Two folders
carrying one `meta.session_id` — an ordinary copy/paste, or one synced tree mounted twice — used to
collapse silently, with the second folder overwriting the first's path and pruning its segments. The
walk is now lexicographic depth-first on every machine, the first folder wins, the duplicate is
logged naming both paths, and the returned count no longer over-reports.

**What is still only compile-checked, stated plainly.** The `archive.started(&manifest)` call site
inside `recording_start` itself is not covered by a test: reaching it needs a Tauri `State`, a
registry and a live sidecar. Its decision half is tested (`index_retitled_session_on` and the sink's
segment/finalize/durability sites are all driven through real code), and the writer seam is now
tested end to end through a real channel and real SQLite in `ingest.rs` — but the single line that
enqueues the start row is proven by the macOS gate compiling it and by nothing else.
