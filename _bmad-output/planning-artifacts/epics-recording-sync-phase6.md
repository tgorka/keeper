# Phase 6 — Recording × Sync (Epics 40–42)

status: draft
created: 2026-08-05
altitude: phase
source: `product-inputs-recording-sync-2026-08-05.md` (the numbering spine — FR-125…FR-146,
NFR-31…NFR-35, AD-64…AD-73, UX-DR45…UX-DR52, Epics 40–42, allocated there and nowhere else),
derived from `_bmad-output/brainstorming/brainstorm-recording-sync-archive-2026-08-05/`
epics:
  - `epic-40-a-recording-lands-where-you-can-find-it.md`
  - `epic-41-a-finished-segment-is-already-on-the-drive.md`
  - `epic-42-the-recordings-archive.md`

Owner-requested phase: a recording stops being a folder the app writes and forgets, and becomes an
addressable record whose path, durability, searchability and meaning all derive from one session
identity. Recordings are named date-first and nested by year, they live inside a folder keeper
already syncs, a closed segment reaches the drive without waiting on a timer, and the pile becomes
an archive worth having — searched, tagged with the same vocabulary as notes, and annotated in the
one minute the owner will ever spend annotating it.

The route is locked by the spine and must not be re-argued in a story:

- **Naming is a path template, not a set of switches.** One token template renders the whole
  relative path; year nesting is the default template's opinion, month nesting is a template edit.
  The token vocabulary is `keeper-sync`'s, reused verbatim (AD-64, AD-65).
- **A recordings destination is a `SyncProfile` plus a subfolder**, exactly as a notes vault is.
  `#[serde(default)]` is the migration; `NotesConfig::validate` is the model for the validator
  (AD-66).
- **Immutability, not impatience, unlocks the completeness gate.** A rotated segment is finished,
  not merely quiescent, so the answer is a narrow producer assertion that skips tier-2 —
  `StabilityGate::note_finished` — never a shortened timer (AD-67, AD-68). `.partial` plus an atomic
  rename is the in-progress marker, hidden by a tier-0 suffix rule (AD-69).
- **Durability is local, publication is a policy.** Commit at close; push on the profile's policy,
  default session end. The recorder asserts facts; the engine decides transport (AD-70).
- **The archive is the point.** A session becomes a row in `archive.db` with its own FTS5 table —
  a second table, never a generalisation of `events_fts` — and its tags resolve against the notes
  tag tree so one vocabulary serves both (AD-71, AD-72, AD-73).

## Epics

| epic | title | stories | binds |
|---|---|---|---|
| 40 | A recording lands where you can find it | 4 | FR-125–FR-129, FR-144, FR-145, AD-64, AD-65, AD-73, UX-DR45, UX-DR46 |
| 41 | A finished segment is already on the drive | 6 | FR-130–FR-138, FR-146, NFR-31, NFR-32, NFR-34, AD-66–AD-70, UX-DR47–UX-DR49 |
| 42 | The recordings archive: searchable, tagged, noted | 5 | FR-139–FR-143, NFR-33, AD-71, AD-72, UX-DR50–UX-DR52 |

## Sequencing

40 before 41 before 42, and within each epic the declared story dependencies hold. The reason is
not ceremony: 41.2 resolves a destination that only exists once 40.2 has a template to render, and
every row 42 writes is keyed by the session id 40.3 mints. The one story that can start in
parallel with epic 40 is 41.1, which touches only `keeper-sync`'s profile type.

## What this phase deliberately does not do

Transcription or any AI processing of media; cross-device dedup; a public publishing lane;
retention and pruning of verified LFS objects; per-tag routing to a second profile; chapter marks
from the segment ledger; linking a session to a Matrix room. The last four converged as "Could" and
are filed as deferred work — each needs a month of real use before its policy is chosen, and
guessing now would bake the wrong default into a profile blob that is hard to un-bake.
