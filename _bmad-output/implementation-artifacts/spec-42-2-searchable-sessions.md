---
title: 'Story 42.2: Searchable Sessions'
type: 'feature'
created: '2026-08-08'
status: 'done'
blocking_condition: ''
baseline_revision: 'a95cb85'
final_revision: '6d2b8f20f939e7155ef0052e43700305672459af'
review_loop_iteration: 0
followup_review_recommended: false
context:
  - '{project-root}/docs/project-context.md'
  - '{project-root}/_bmad-output/planning-artifacts/epic-42-the-recordings-archive.md'
  - '{project-root}/_bmad-output/implementation-artifacts/spec-42-1-a-session-is-a-row.md'
---

<intent-contract>

## Intent

**Problem:** story 42.1 made a session a row, and a row you cannot find is a folder with extra steps.
The metadata that matters — the title someone typed, who was in the room, the note about what the
session was for, the tags — is now in columns that only an exact `WHERE` can reach. The message
archive already solved this once for `events`: an FTS5 trigram index with a sub-trigram `LIKE`
fallback, in `archive/fts.rs`. Recordings need the same thing, and need it to behave the same way,
because a user who has learned how search works in one surface has learned it in both.

**Approach:** `recordings_fts`, an FTS5 trigram table over the searchable text of a session, and
`search_recordings(conn, &RecordingFilter)` combining free text with structured predicates. The index
is maintained by the same single writer that owns the row, inside the same transaction, so a row and
its index cannot disagree. Everything is `keeper-core`: no IPC, no VM, no surface — 42.3 owns those.

## Boundaries & Constraints

**Always:**
- One writer, one transaction. Every index write happens inside the transaction that writes the row
  it describes, through the existing serialized writer. A row whose index is stale is a bug, not a
  a state the system is allowed to be in.
- The index is derivable, like the row. `rebuild_from_disk` must leave the index consistent, and
  deleting `archive.db` must lose nothing the manifests do not carry.
- Mirror `archive/fts.rs`'s shape and its decisions: trigram tokenizer, the `< 3` Unicode-scalar
  `LIKE` fallback, the query bound as one parameter and quoted so FTS operators are matched
  literally, a bounded default limit.
- A query that matches nothing returns an empty vector. Not an error.
- Tag matching is hierarchical and prefix-wise at the segment boundary: `tag:client/acme` matches
  `client/acme` and `client/acme/renewal` and never `client/acmecorp` or `client/other`.

**Block If:**
- The bundled SQLite lacks FTS5 or the trigram tokenizer. It does not — `events_fts` already depends
  on both — but the failure is surfaced as `ArchiveError::Sqlite` rather than a silent degrade.

**Never:**
- No IPC command, no `Vm`, no UI (42.3). No tag normalisation or resolution against the notes tag
  tree (42.5) — this story matches the tag text as stored.
- No external-content FTS table. `events_fts` is one and DW-48 records the maintenance hazard it
  carries; the recordings index owns its own copy of the text so a `VACUUM` or a rowid change can
  never desynchronise it silently.
- No change to `events`, `events_fts`, or `search`.

## I/O & Edge-Case Matrix

| Scenario | Input / State | Expected | Error |
|---|---|---|---|
| Free text, ≥3 chars | a session whose note says "pricing" | found by `pricing`, through the trigram index | none |
| Free text, <3 chars | the same session | found by `pric`… and by `pr`, through the `LIKE` fallback | none |
| Empty query | no text, only predicates | every session the predicates admit, newest first | none |
| FTS operators as text | a query of `AND` or `a*b` | matched literally, never parsed as an operator | none |
| `tag:` prefix | session tagged `client/acme/renewal` | `tag:client/acme` matches; `tag:client/other` and `tag:client/acmecorp` do not | none |
| `participant:` | participants "Ada, Grace" | `participant:ada` matches, case-insensitively | none |
| Date range | `started_ts` inside / outside | inside is returned, outside is not; an open end is unbounded | none |
| Durability | `durability = pushed` | only sessions at that state | none |
| Profile | `profile_id` set | only sessions indexed under it | none |
| A session with no metadata at all | no title, note, tags | indexed with empty text; found by predicates, never by free text | none |
| Metadata edited | a finalize (or retitle) rewrites the row | the index reflects the NEW text in the same transaction | none |
| Rebuild | `rebuild_from_disk` over an indexed tree | index consistent with rows, no duplicates | none |
| Row replaced | `INSERT OR REPLACE` on an existing session | exactly one index entry, not two | none |
| Scale | 10 000 synthetic sessions | search returns inside the committed bench budget | none |

</intent-contract>

## Code Map

- `src-tauri/crates/keeper-core/src/archive/recordings_fts.rs` (new) — the table, its migration, the
  incremental maintenance, and `search_recordings`.
- `src-tauri/crates/keeper-core/src/archive/recordings.rs` — `upsert_recording` and `move_session`
  index inside their existing transaction; `RecordingFilter` lives wherever the search signature
  reads best.
- `src-tauri/crates/keeper-core/src/archive/db.rs` — the new table joins the schema the writer's
  connection ensures on open.
- `src-tauri/crates/keeper-core/benches/` (or the repo's existing bench home) — the 10 000-session
  bench, committed with its measured budget.
- Read-only: `archive/fts.rs` (the shape to mirror), `spec-42-1` (the row this indexes).

## Tasks & Acceptance

**Execution:**
- [x] `recordings_fts` + its idempotent, additive migration.
- [x] Incremental maintenance inside the row's own transaction, for insert, replace and move.
- [x] `search_recordings` with free text, `tag:`, `participant:`, date range, durability, profile.
- [x] The sub-trigram `LIKE` fallback.
- [x] The 10 000-session bench, committed, with the measured budget written down.
- [x] Tests: every matrix row.

**Acceptance Criteria:**
- A session whose note mentions "pricing" is found by `pricing` and by `pric`.
- `tag:client/acme` matches a session tagged `client/acme/renewal` and does not match `client/other`.
- Searching 10 000 synthetic sessions returns within the budget recorded in the committed bench.
- Retitling a session updates the index within the same transaction as the row.

## Design Notes

**Two objects, not one.** `recordings_fts` is an FTS5 trigram table with a single indexed `text`
column, paired with a plain `recordings_fts_docs(doc_id INTEGER PRIMARY KEY, session_id TEXT UNIQUE)`.
An FTS5 entry is addressed by rowid, and no rowid `recordings` already has can serve one: its key is
TEXT, its implicit rowid changes on every `INSERT OR REPLACE`, and `VACUUM` can renumber it — the
DW-48 hazard, restated. Keying the entry on a `session_id UNINDEXED` column would be correct and
quadratic, because FTS5 has no secondary indexes and every replace would scan the whole index; over
10 000 sessions that is an NFR-33 problem rather than a style one. An explicit `INTEGER PRIMARY KEY`
is the one thing `VACUUM` never renumbers, so both directions are a single b-tree probe.

**The index is not external-content, and that is the point.** `events_fts` is, and DW-48 records what
that costs to maintain. This one owns its copy of the text, so nothing about the base table's rowids
can silently desynchronise it.

**`tag:` is matched over decoded JSON, not over JSON text.** The predicate walks `json_each` and
applies two arms — `LOWER(value) = LOWER(?)` or `LOWER(value) LIKE LOWER(?) || '/%'` — which is
exactly the segment rule `notes/query.rs::tag_descends` already implements. A raw-text `LIKE` would
depend on serde's escaping choices and, worse, `LIKE 'client/acme%'` matches `client/acmecorp`, which
the AC explicitly forbids. The rule is duplicated rather than shared for the same reason
`days_from_civil` is: an `archive` → `notes` dependency edge to borrow six lines of string comparison
would couple two subsystems that otherwise know nothing about each other. Story 42.5 is where the two
tag vocabularies actually converge.

**The backfill is an anti-join on every open, not a once-only FTS5 `'rebuild'`.** `ensure_fts` cannot
do this — a second `'rebuild'` would clobber its incremental rows — but this index can, and it makes
three separate failures self-heal at the next launch: an archive that predates 42.2, a crash between
the two `CREATE`s, and an entry deleted by hand. On a healthy database it touches zero rows.

**`in_transaction` became reentrant.** `upsert_recording` now needs a transaction of its own so the
row write, the durability floor read and the index write are one atomic unit, but
`write_rebuilt_session` already calls it inside one, and SQLite has no nested `BEGIN`. The helper
short-circuits when the connection is already in a transaction, so a whole rebuilt session still
commits exactly once — and an inner error must still propagate, which is stated on the helper because
it is the one way to get this wrong.

**A move writes no index.** `move_session` changes paths, and a path is not searchable text. Said
plainly in its doc comment so a later reader cannot mistake the absence for an oversight.

**The budget is a promise about one shape.** The measured p95 over 10 000 sessions is 32.4 ms, and
33.8 ms of that is the sub-trigram `LIKE` fallback — the path that by construction cannot use the
trigram index. Every FTS-served shape lands in single-digit milliseconds or less. The bench records
the machine, the per-shape worst, and the yardstick that scales the budget for slower hardware, so
the number can be re-derived rather than trusted.
