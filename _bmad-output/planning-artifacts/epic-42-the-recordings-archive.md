# Epic 42 — The recordings archive: searchable, tagged, noted

status: draft
created: 2026-08-05
altitude: epic
parent: Phase 6 (Recording × Sync), Epic 5 (message archive + FTS), Epic 21/22 (session metadata), Phase 5 (Notes)
source: `product-inputs-recording-sync-2026-08-05.md` (the numbering spine), the divergent session in
`brainstorm-recording-sync-archive-2026-08-05/`, and a read-only survey of `keeper-core/src/archive/`
(`db.rs`, `fts.rs`, `mod.rs`) and `keeper-core/src/notes/tags.rs`
binds: FR-139–FR-143, NFR-33, AD-71, AD-72, UX-DR50–UX-DR52

## Why this epic exists

Stories 21.5 and 22.3 gave a session a title, participants, a note field, times, tags and custom
fields. All of it is written into `manifest.json` and, apart from `meta.title`, **never read
again**. That is the whole state of recording metadata today: write-once, search-never, list-never.
A folder of folders.

Meanwhile the app already contains a working archive: `archive.db` with an `events` table, an FTS5
trigram index (`events_fts`), one serialized writer task, a search command and a React search
surface. It is exclusively about Matrix events — there is no file, session or recording concept
anywhere in it. And a third subsystem, the notes tag tree (`keeper-core/src/notes/tags.rs`), already
implements exactly the hierarchical tag vocabulary that recording tags were re-invented as a
comma-split string beside.

Three subsystems, one missing set of edges. This epic draws them.

The owner's phrasing was "I want the recordings and the sync archive to enter after (maybe process
and integrate with notes and use tags)". *Enter after* is the load-bearing part: recording is a
capture act, and the value shows up later, when a question ("which call did we agree the API shape
on?") meets an index. Everything here serves that later moment.

### Where we take a position

**A second FTS table, not a generalised one.** `events_fts` is an FTS5 *external-content* table over
`events.body`, wired to that table's rowids and rebuilt from it. Generalising it into a polymorphic
search over two row kinds would put a migration and a consistency problem into the one subsystem
that currently has neither. `recordings_fts` is a second table in the same database, built with the
same trigram tokenizer, the same `ensure_fts` + incremental `index_body` pattern, and the same
migration convention (`PRAGMA table_info` + `ALTER TABLE ADD COLUMN`) — copied deliberately, because
matching an existing pattern is worth more here than sharing code (AD-71).

**The row is the record; the manifest is a cache of it.** A synced folder is opened on other
machines and edited by other tools, so the manifest must remain the portable, plain-text truth
inside the session folder. But the *queryable* record is the row, rebuildable from the manifests at
any time. That asymmetry has to be explicit or it becomes a consistency bug: an absent or stale row
is a rescan, never an error, exactly as the notes index is (AD-57's spirit, applied again).

**One tag vocabulary, two producers.** A recording tagged `client/acme` and a note tagged
`client/acme` must be the same tag or the feature is theatre. Recording tags resolve against
`keeper-core/src/notes/tags.rs`'s tree, get its completion affordance (UX-DR52), and appear in the
same sidebar (FR-143). This is the payoff for having built the tag tree pure and vault-agnostic.

**The note stub is written at the only moment it will ever be written.** Nobody documents a meeting
an hour later. The minute the recording stops is the entire window, so finalize writes a stub —
prefilled with what keeper already knows — and puts the cursor in it (UX-DR51). A stub the user
never touched is deleted rather than left as litter, because an archive full of empty notes is worse
than one with none.

## Stories

### Story 42.1: A Session Is a Row
**Rust-only (`keeper-core` + `keeper` shell).** Bindings: no. Binds FR-139, AD-71.

`keeper-core/src/archive/recordings.rs`: two tables in `archive.db` — `recordings` (session_id PK,
device_id, relative_path, root_kind, profile_id, started_ts, ended_ts, title, participants_json,
note, tags_json, custom_json, codec, width, height, fps, durability, manifest_version) and
`recording_segments` (session_id, index, track, relative_path, bytes, pts_start, pts_end, closed_ts,
PK (session_id, index, track)) — created with `CREATE TABLE IF NOT EXISTS` and evolved by the
existing additive-migration helper. Writes go through the existing archive writer channel as new
`ArchiveMsg` variants, so there is still exactly one writer. A row is inserted at session start and
completed at finalize; `durability` is updated as epic 41's state advances. A `rebuild_from_disk`
entry point re-derives rows by walking session folders and reading manifests.
AC: a session start writes a row and a finalize completes it, with `INSERT OR REPLACE` semantics that
survive a duplicate finalize; deleting `archive.db` and running `rebuild_from_disk` over a tree of
50 sessions reproduces byte-identical rows for every field the manifest carries; a manifest from an
older version loads with the missing columns defaulted and no error; the migration is idempotent
across three successive opens.

### Story 42.2: Searchable Sessions
**Rust-only (`keeper-core`).** Bindings: no. Depends on 42.1. Binds FR-140, NFR-33.

`recordings_fts` — an FTS5 trigram table over the concatenation of title, participants, note, tags
and custom values — created by an `ensure_fts`-shaped helper and maintained incrementally on insert
and update, mirroring `archive/fts.rs`'s structure including its sub-trigram `LIKE` fallback for
queries under three characters. `search_recordings(conn, &RecordingFilter)` supports free text plus
structured predicates: `tag:`, `participant:`, date range, durability, and profile.
AC: a session whose note mentions "pricing" is found by `pricing` and by `pric` (the fallback path);
`tag:client/acme` matches a session tagged `client/acme/renewal` and does not match `client/other`;
searching 10 000 synthetic sessions returns within the budget recorded in the bench, and the bench is
committed; retitling a session updates the index within the same transaction as the row.

### Story 42.3: The Recordings Browser
**Crosses the IPC boundary.** Bindings: **yes**. Depends on 42.2. Binds FR-141, UX-DR50.

`search_recordings` IPC command + `RecordingHitVm`, and a Recordings surface that is a search
first: a filter row above the fold (text, tag, participant, date range, durability), results as
rows showing title, date, duration, size, tags and a durability glyph, and per-row actions to
reveal in Finder, play, and copy the session id. Empty states are honest: no recordings yet, versus
no matches for this filter. Capability-gated exactly as the recording surface is — absent, not
disabled, where recording is unsupported.
AC: `bun run bindings:check` passes; filtering by a tag narrows the list without a round trip per
keystroke (debounced, asserted); a session recorded during the session under test appears without a
restart; reveal-in-Finder opens the real folder for a session whose folder was renamed by story
40.4; on a build without the recording capability the surface is absent from the DOM.

### Story 42.4: The Note Stub, at the Only Moment It Will Be Written
**Rust + frontend.** Bindings: **yes**. Depends on 42.1. Binds FR-142, AD-72, UX-DR51.

At finalize keeper composes a markdown stub — title, date, start/end, duration, participants, tags,
and a `session:` link carrying the immutable session id — and writes it beside the session folder
(or through the notes writer when the destination profile is also a notes vault, so the file lands
in the vault and is indexed there). The stop surface presents it with the cursor in the body; one
key dismisses. A stub whose body the user never edited is deleted on dismiss rather than left
behind.
AC: stopping a recording writes exactly one stub whose frontmatter round-trips through the notes
frontmatter parser unchanged; editing and saving keeps it and it appears in the notes index when the
destination is a vault; dismissing without editing leaves no file; the stub contains no absolute
path; two sessions stopped in the same minute produce two stubs with distinct names.

### Story 42.5: One Tag Vocabulary
**Rust + frontend.** Bindings: **yes**. Depends on 42.2, 42.4. Binds FR-143, UX-DR52.

Recording tags stop being a comma-split string: they are parsed, normalised and resolved against the
hierarchical tag tree in `keeper-core/src/notes/tags.rs`, gaining the same completion affordance the
notes surface uses. The tag tree's counts include recordings, so `client/acme` shows notes and
recordings together, and selecting a tag can scope either surface. Normalisation is documented and
one-way: case, whitespace and separator handling defined once, in the tag module, for both
producers.
AC: a recording tagged `Client/Acme ` and a note tagged `client/acme` resolve to the same tag node,
asserted in a Rust test over the tree; completion in the recording metadata card offers tags that
exist only on notes; removing the last recording carrying a leaf tag removes the leaf while a
sibling keeps the parent; the counts shown in the sidebar equal the sum of both producers.

## Out of scope

- Transcription, summarisation, keyword extraction, or any AI processing. Named explicitly because
  it is the first thing this epic invites, and it needs a separate decision about where inference
  runs and what leaves the machine.
- Chapters from `ptsStart`/`ptsEnd`, linking a session to a Matrix room, and retention/pruning:
  converged as "Could" and deferred with the rest of that list.
- A media player beyond handing the file to the system.
- Any change to the message archive's `events` table, `events_fts`, or its search command.
