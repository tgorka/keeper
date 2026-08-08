//! Searchable sessions — the trigram index over a recording's own metadata
//! (Story 42.2, FR-140, NFR-33, AD-71).
//!
//! Story 42.1 made a session a row, and a row nobody can find is a folder with
//! extra steps. `recordings_fts` is an FTS5 `tokenize='trigram'` index over the
//! one text a session is searchable by — its title, its participants, its note,
//! its tags and the values of its custom fields, concatenated — and
//! [`search_recordings`] combines a free-text match with the structured
//! predicates the epic names: `tag:`, `participant:`, a date range, durability
//! and profile.
//!
//! **It behaves exactly like the message archive's search, deliberately.**
//! [`super::fts`] solved this once for `events`, and a user who has learned how
//! search works in one surface has learned it in both. So: the same trigram
//! tokenizer, the same sub-trigram [`LIKE`](escape_like) fallback below
//! [`TRIGRAM_MIN_CHARS`] Unicode scalar values, the same rule that the query is
//! bound as ONE double-quoted parameter so `AND`, `OR` and `*` are matched as
//! the text they are rather than parsed as operators, and the same bounded
//! [`DEFAULT_LIMIT`].
//!
//! **One deliberate divergence: this index is not external-content.**
//! `events_fts` is (`content='events'`), and DW-48 records the maintenance
//! hazard that carries: an external-content table is bound to its base table by
//! rowid, so a `VACUUM` — or anything else that renumbers an implicit rowid —
//! desynchronises the index with no error, no crash and no wrong answer until a
//! user notices their search has started lying. `recordings_fts` therefore owns
//! its own copy of the indexed text and is addressed through a key nothing can
//! renumber (see [`ensure_recordings_fts`]).
//!
//! **One writer, one transaction.** Nothing here opens a connection. Every
//! index write happens inside the transaction that writes the row it describes,
//! on the single serialized writer connection [`super::recordings`] already
//! owns, so a row and its index cannot disagree — not even if the process dies
//! mid-write. A failed index write rolls the row back with it, because a row
//! whose index is stale is a bug and not a state this system is allowed to
//! reach.
//!
//! **The index is derivable, like the row.** It is built from `recordings`
//! columns and nothing else, so [`super::recordings::rebuild_from_disk`] leaves
//! it consistent for free, and deleting `archive.db` loses nothing the
//! manifests do not carry.
//!
//! Tauri-free, gix-free and clock-free like the rest of `keeper-core`: this
//! module reads rows and writes rows, and every instant it compares against
//! arrives as a parameter.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};

use crate::error::ArchiveError;
use crate::recording::SessionMetaField;
use crate::vm::{RecordingFilterVm, RecordingHitVm};

use super::recordings::{in_transaction, RecordingRow};

/// The default (and maximum) number of hits a single [`search_recordings`]
/// returns. Bounded for [`super::fts::DEFAULT_LIMIT`]'s reasons and at its
/// value: it keeps NFR-33's latency budget honest and the payload Story 42.3
/// renders small, and a caller can never ask for an unbounded scan.
pub const DEFAULT_LIMIT: i64 = 200;

/// The minimum query length, in Unicode scalar values, the trigram tokenizer
/// can match. A shorter query cannot form a trigram, so [`search_recordings`]
/// dispatches to the `LIKE` fallback below this threshold — the same threshold,
/// counted the same way, as [`super::fts`] (AD-12).
const TRIGRAM_MIN_CHARS: usize = 3;

/// The two joins that put a session's indexed text in scope, in the only order
/// that is a keyed lookup in both directions: the free-text clause selects
/// `recordings_fts` rowids, each of which is one `recordings_fts_docs` primary
/// key probe, which is one `recordings` primary key probe.
///
/// Joined ONLY when there is text to match. An empty query has no text
/// predicate at all (see [`search_recordings`]), and an inner join to the index
/// would then be a promise about index coverage the structured-predicate path
/// has no reason to make.
const INDEX_JOIN: &str = " JOIN recordings_fts_docs \
        ON recordings_fts_docs.session_id = recordings.session_id \
     JOIN recordings_fts ON recordings_fts.rowid = recordings_fts_docs.doc_id";

/// The `tag:` predicate: does this session carry `?1`, or any tag descending
/// from it at a segment boundary?
///
/// **The segment boundary is the whole point.** `tag:client/acme` must match
/// `client/acme` and `client/acme/renewal` and must NOT match `client/acmecorp`
/// or `client/other`. A plain `LIKE 'client/acme%'` matches `client/acmecorp`
/// and silently widens every tag filter to its lexical neighbours, so the test
/// is two arms: equal to the prefix, or the prefix followed by `/`. This is the
/// identical rule `crate::notes::query::tag_descends` applies to note tags, and
/// it is still spelled out again here rather than shared. Story 42.5 unified
/// what a TAG is, which is the thing that was actually broken; how a hierarchy
/// descends is a two-arm prefix test that has been fixed since FR-104 and
/// cannot drift, and it is expressed here in SQL and there in Rust, so there
/// was never one definition to share.
///
/// **Matched over the stored JSON's decoded elements, not a normalised
/// sidecar.** `tags_json` on the row is the truth (Story 42.1), and a sidecar
/// tag table would be a second copy to keep in step through every write path —
/// a new way for the index to be wrong — bought for a predicate that is already
/// cheap, because the free-text clause and the `started_ts` index have narrowed
/// the row set before it is evaluated. `json_each` is used rather than a `LIKE`
/// over the raw JSON text because the raw text carries `[`, `"` and whatever
/// escapes serde emitted, and a predicate that depends on an encoder's
/// escaping choices is a predicate waiting to be wrong.
///
/// **Both sides of this comparison are now canonical** (Story 42.5): the stored
/// elements were normalised on their way into the column by
/// [`super::recordings::RecordingRow::from_manifest`], and the bound term is
/// normalised by [`search_recordings`] before it gets here. So this is a
/// straight comparison of two canonical tags, and there is no third place that
/// decides what a tag is.
///
/// `json_quote` is the fallback arm rather than `'[]'`: a column holding text
/// that is not JSON at all (hand-edited, or written by a tool that is not this
/// one) becomes a single-element document instead of an empty one, so its one
/// tag is still matchable. `json_valid(NULL)` is `NULL`, so a session with no
/// tags takes the same arm and `json_quote(NULL)` yields the JSON `null`
/// literal, whose one element is `NULL` and matches nothing.
///
/// Case-insensitive through `LOWER` on both sides, which is ASCII-range only —
/// exactly what [`super::fts`]'s fallback promises, and the same promise here.
const TAG_PREDICATE_SQL: &str = "EXISTS (\
        SELECT 1 FROM json_each(\
            CASE WHEN json_valid(recordings.tags_json) THEN recordings.tags_json \
                 ELSE json_quote(recordings.tags_json) END\
        ) AS tag \
        WHERE LOWER(tag.value) = LOWER(?) \
           OR LOWER(tag.value) LIKE LOWER(?) || '/%' ESCAPE '\\'\
     )";

/// The `participant:` predicate: a case-insensitive substring of any
/// participant the session records.
///
/// A substring rather than an equality because `participants_json` today holds
/// the JSON *string* encoding of one free-text line — `"Ada, Grace"` — so
/// `participant:ada` has to reach inside the line the user typed. Walked with
/// `json_each` on [`TAG_PREDICATE_SQL`]'s terms, which is also what makes this
/// survive Story 42.5 widening the column to an array without a migration: the
/// string arm yields one element, the array arm yields each of them, and this
/// predicate does not change.
const PARTICIPANT_PREDICATE_SQL: &str = "EXISTS (\
        SELECT 1 FROM json_each(\
            CASE WHEN json_valid(recordings.participants_json) \
                 THEN recordings.participants_json \
                 ELSE json_quote(recordings.participants_json) END\
        ) AS participant \
        WHERE LOWER(participant.value) LIKE '%' || LOWER(?) || '%' ESCAPE '\\'\
     )";

/// The columns [`RecordingHit`] is read from, in its field order.
const HIT_COLUMNS: &str = "recordings.session_id, recordings.relative_path, \
     recordings.root_kind, recordings.profile_id, recordings.started_ts, \
     recordings.ended_ts, recordings.title, recordings.participants_json, \
     recordings.note, recordings.tags_json, recordings.durability";

/// What a caller is looking for (Story 42.2). A plain keeper-core struct — Story
/// 42.3's IPC input VM maps INTO this, so the engine stays tauri-free, exactly
/// as [`super::fts::SearchFilter`] does.
///
/// Every field is optional in the sense that matters: an empty `query` is no
/// text predicate, an empty `tags` list is unrestricted, and every `Option` is
/// `None` when unset. All of them AND together — narrowing, never widening — so
/// `Default::default()` is "every session, newest first".
#[derive(Debug, Clone, Default, PartialEq)]
pub struct RecordingFilter {
    /// The user's free text. Length is counted in Unicode scalar values to pick
    /// the trigram-`MATCH` (≥ [`TRIGRAM_MIN_CHARS`]) or `LIKE` (below it) path;
    /// empty means no text predicate at all.
    pub query: String,
    /// Tags the session must carry, each matched hierarchically at the segment
    /// boundary (see [`TAG_PREDICATE_SQL`]). Several tags AND together, the way
    /// the notes surface's tag chips do: two chips narrow, they do not widen.
    /// Taken as typed and normalised by [`search_recordings`] (Story 42.5), so a
    /// chip reading `Client/Acme ` finds the same sessions the sidebar's
    /// `client/acme` node does. A tag that normalises to nothing — an empty
    /// chip, `///` — narrows nothing and is skipped.
    pub tags: Vec<String>,
    /// A case-insensitive substring of the session's participants; `None` (or
    /// empty) matches any.
    pub participant: Option<String>,
    /// Lower bound (inclusive) on `started_ts`, ms since the Unix epoch; `None`
    /// is unbounded below.
    pub start_ts: Option<i64>,
    /// Upper bound (inclusive) on `started_ts`, ms since the Unix epoch; `None`
    /// is unbounded above.
    pub end_ts: Option<i64>,
    /// Restrict to one durability state, as the column word
    /// [`super::recordings::durability_label`] produces; `None` is any state.
    pub durability: Option<String>,
    /// Restrict to sessions indexed under one destination profile; `None` is
    /// any profile, including sessions recorded to a plain folder.
    pub profile_id: Option<String>,
    /// Cap on the number of hits; `None` is [`DEFAULT_LIMIT`]. Clamped to
    /// `[1, DEFAULT_LIMIT]`.
    pub limit: Option<i64>,
}

impl From<RecordingFilterVm> for RecordingFilter {
    /// Map Story 42.3's IPC input VM onto this tauri-free engine filter,
    /// mirroring [`super::fts::SearchFilter`]'s seam field for field. A pure
    /// move: the shell decides nothing here, and the engine stays callable from
    /// a test with no shell at all.
    fn from(vm: RecordingFilterVm) -> Self {
        RecordingFilter {
            query: vm.query,
            tags: vm.tags,
            participant: vm.participant,
            start_ts: vm.start_ts,
            end_ts: vm.end_ts,
            durability: vm.durability,
            profile_id: vm.profile_id,
            limit: vm.limit,
        }
    }
}

/// One session a search matched (Story 42.2).
///
/// A plain keeper-core struct and deliberately NOT a `Vm`: Story 42.3 owns the
/// IPC boundary and maps this into `RecordingHitVm`, which keeps this crate free
/// of `ts-rs`-shaped decisions and keeps the engine callable from a test with no
/// shell at all.
///
/// It carries what a result row renders — the session's identity, where its
/// folder is, when it ran, what it was called, who was in it, its note, its
/// tags and how far its bytes have travelled. Not `custom_json`: custom fields
/// are searchable (see [`searchable_text`]) because a user remembers what they
/// typed, but no result row displays them, and a hit that carried every column
/// would be a second `RecordingRow` wearing a different name.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordingHit {
    /// The session's immutable identity ([`RecordingRow::session_id`]).
    pub session_id: String,
    /// The session folder, relative to its destination root, `/`-joined.
    pub relative_path: String,
    /// Which kind of place that root is — `"folder"` or `"profile"`.
    pub root_kind: String,
    /// The destination profile's ULID when the root is a sync profile.
    pub profile_id: Option<String>,
    /// Session start, ms since the Unix epoch; `None` for a pre-21.5 manifest
    /// that carries no stamp.
    pub started_ts: Option<i64>,
    /// Session end, ms since the Unix epoch; `None` while the session runs.
    pub ended_ts: Option<i64>,
    /// The user's title for the session.
    pub title: Option<String>,
    /// Who the recording is with, still in the column's JSON shape — Story 42.3
    /// decodes it for display, on the terms [`RecordingRow::participants_json`]
    /// states.
    pub participants_json: Option<String>,
    /// The user's free-text note about the session.
    pub note: Option<String>,
    /// The session's tags, still as the stored JSON array.
    pub tags_json: Option<String>,
    /// How far the session's bytes have travelled, as epic 41's wire word.
    pub durability: String,
}

/// Create the recordings search index if it does not exist, and index every
/// session row that has no entry yet (Story 42.2).
///
/// Called from [`super::db::open_archive_db`] immediately after
/// [`super::recordings::ensure_recordings_schema`] — which is a REQUIREMENT,
/// not a preference: the backfill below reads `recordings`, so the table has to
/// exist first. Every connection the writer task owns therefore has this index,
/// and no search or index write ever needs an open path of its own.
///
/// **Two objects, and the second one is why this can be trusted.**
///
/// - `recordings_fts` is an ordinary (content-owning) FTS5 trigram table over
///   one column, `text` — the concatenation [`searchable_text`] composes. Not
///   external-content: see this module's header on DW-48.
/// - `recordings_fts_docs` maps a session to the integer the index entry is
///   keyed by. An FTS5 entry is addressed by rowid, and no rowid `recordings`
///   already has can serve: its key is the TEXT `session_id`, and its implicit
///   rowid both changes on every `INSERT OR REPLACE` and can be renumbered by a
///   `VACUUM`. Keying the entry on a `session_id UNINDEXED` column instead
///   would be correct but slow in a way that matters — FTS5 has no secondary
///   indexes, so every replace would scan the whole index and a 10 000-session
///   `rebuild_from_disk` would be quadratic (NFR-33). So: `doc_id` is an
///   explicit `INTEGER PRIMARY KEY` (a rowid alias, which `VACUUM` never
///   renumbers) and `session_id` is `UNIQUE`, making both directions a single
///   b-tree probe.
///
/// **No auxiliary copy of tags or participants.** A search always starts from
/// `recordings` — a hit needs its path, its stamps and its durability, and the
/// date/durability/profile predicates are its columns — so `tags_json` and
/// `participants_json` are already in scope for [`TAG_PREDICATE_SQL`] and
/// [`PARTICIPANT_PREDICATE_SQL`] without a join this query was not making
/// anyway. A copy inside the index would buy no lookup and cost a second truth
/// to keep in step.
///
/// **Idempotent in the strict sense the AC asks for**, and self-healing rather
/// than once-only. `ensure_fts` rebuilds `events_fts` only on fresh creation,
/// because a second FTS5 `'rebuild'` would clobber the rows added incrementally
/// since; it pays for that with an index that stays empty forever if a crash
/// lands between the create and the rebuild. Here the backfill names exactly
/// the sessions that have no entry, so running it on every open is a no-op on a
/// healthy database (an anti-join returning nothing) and total when it is not:
/// a database written by a build without this index, a crash between the two
/// creates, or a hand-deleted entry all heal at the next open.
///
/// The create and the backfill share one transaction, so a crash leaves the
/// index either wholly absent (and rebuilt next open) or wholly correct.
///
/// A runtime failure of `CREATE VIRTUAL TABLE … USING fts5(… tokenize='trigram')`
/// means the bundled SQLite lacks FTS5 or the trigram tokenizer. It does not —
/// `events_fts` already depends on both — but the failure surfaces as an
/// [`ArchiveError::Sqlite`] rather than a silent degrade to no search at all.
pub fn ensure_recordings_fts(conn: &Connection) -> Result<(), ArchiveError> {
    in_transaction(conn, "recordings search index setup", || {
        conn.execute(
            "CREATE TABLE IF NOT EXISTS recordings_fts_docs(\
                doc_id INTEGER PRIMARY KEY, \
                session_id TEXT NOT NULL UNIQUE\
            )",
            [],
        )
        .map_err(|e| ArchiveError::Sqlite(format!("could not create recordings_fts_docs: {e}")))?;
        conn.execute(
            "CREATE VIRTUAL TABLE IF NOT EXISTS recordings_fts USING fts5(\
                text, tokenize='trigram')",
            [],
        )
        .map_err(|e| ArchiveError::Sqlite(format!("could not create recordings_fts: {e}")))?;
        index_unindexed_sessions(conn)?;
        Ok(())
    })
}

/// Index every `recordings` row that has no index entry, and return how many
/// there were — the upgrade path and the self-heal in one anti-join (see
/// [`ensure_recordings_fts`]).
///
/// The pending rows are read out in full before the first write. A statement
/// stepping over `recordings` while the same connection writes another table is
/// a hazard for no gain, and this is the shape
/// [`super::recordings::move_session`] already uses for the same reason.
fn index_unindexed_sessions(conn: &Connection) -> Result<usize, ArchiveError> {
    let pending: Vec<(String, String)> = {
        let mut stmt = conn
            .prepare(
                "SELECT session_id, title, participants_json, note, tags_json, custom_json \
                 FROM recordings \
                 WHERE session_id NOT IN (SELECT session_id FROM recordings_fts_docs)",
            )
            .map_err(|e| ArchiveError::Sqlite(format!("could not read unindexed sessions: {e}")))?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    // The one composer, so a backfilled entry is byte-identical
                    // to an incrementally written one.
                    compose_searchable_text(
                        r.get::<_, Option<String>>(1)?.as_deref(),
                        r.get::<_, Option<String>>(2)?.as_deref(),
                        r.get::<_, Option<String>>(3)?.as_deref(),
                        r.get::<_, Option<String>>(4)?.as_deref(),
                        r.get::<_, Option<String>>(5)?.as_deref(),
                    ),
                ))
            })
            .map_err(|e| ArchiveError::Sqlite(format!("could not read unindexed sessions: {e}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| {
                ArchiveError::Sqlite(format!("could not read an unindexed session: {e}"))
            })?);
        }
        out
    };
    for (session_id, text) in &pending {
        index_text(conn, session_id, text)?;
    }
    Ok(pending.len())
}

/// Index one session's searchable text, replacing whatever entry it already had
/// (Story 42.2).
///
/// Called by [`super::recordings::upsert_recording`] inside the transaction
/// that writes the row, which is the whole of this story's hardest rule: the
/// row and its index are one unit of work, so a process that dies mid-write
/// leaves neither half.
///
/// A session with no metadata at all is indexed with empty text rather than
/// skipped — [`super::fts::index_body`] skips a text-less body, but that index
/// only ever inserts, where this one replaces. An entry that exists is an entry
/// a later edit can replace in place, and an empty one matches no free text
/// anyway (the matrix's "no metadata at all" row).
pub fn index_recording(conn: &Connection, row: &RecordingRow) -> Result<(), ArchiveError> {
    index_text(conn, &row.session_id, &searchable_text(row))
}

/// Put `text` in the index under `session_id`, as its one and only entry.
///
/// Reserve-then-read rather than `RETURNING`, so the session's `doc_id` is
/// reached the same way whether the entry is new or a replacement, and the
/// delete is unconditional: `INSERT OR REPLACE`-ing a row that already had an
/// entry leaves exactly one entry, never two, and deleting an entry that is
/// already gone deletes nothing and is not an error.
///
/// `doc_id` is never recycled in practice because a `recordings_fts_docs` row
/// is never deleted — Story 42.1 never deletes a session row either, since a
/// missing folder is a fact for a later story to present rather than a reason
/// to forget the session. Even if one were, the unconditional delete below
/// means a reissued id inherits no stale text.
fn index_text(conn: &Connection, session_id: &str, text: &str) -> Result<(), ArchiveError> {
    conn.execute(
        "INSERT OR IGNORE INTO recordings_fts_docs(session_id) VALUES (?1)",
        rusqlite::params![session_id],
    )
    .map_err(|e| ArchiveError::Sqlite(format!("could not reserve a search doc id: {e}")))?;
    let doc_id: i64 = conn
        .query_row(
            "SELECT doc_id FROM recordings_fts_docs WHERE session_id = ?1",
            rusqlite::params![session_id],
            |r| r.get(0),
        )
        .map_err(|e| ArchiveError::Sqlite(format!("could not read a search doc id: {e}")))?;
    conn.execute(
        "DELETE FROM recordings_fts WHERE rowid = ?1",
        rusqlite::params![doc_id],
    )
    .map_err(|e| ArchiveError::Sqlite(format!("could not clear a recording index entry: {e}")))?;
    conn.execute(
        "INSERT INTO recordings_fts(rowid, text) VALUES (?1, ?2)",
        rusqlite::params![doc_id, text],
    )
    .map_err(|e| ArchiveError::Sqlite(format!("could not index a recording: {e}")))?;
    Ok(())
}

/// The one text a session is searchable by: its title, its participants, its
/// note, its tags and the VALUES of its custom fields, one per line.
///
/// **One derivation, two callers**, exactly as [`RecordingRow::from_manifest`]
/// is the one derivation of the row: the incremental write
/// ([`index_recording`]) and the backfill inside [`ensure_recordings_fts`] both
/// compose their text here, so a backfilled entry is byte-identical to an
/// incrementally written one and the two cannot drift.
///
/// **Newline-joined, and that matters.** A trigram index windows every three
/// consecutive scalar values, so running the fields together would mint
/// trigrams that straddle a field boundary and match text no user ever typed —
/// the tail of a title plus the head of a note. A newline forms trigrams too,
/// but no query a filter row can produce contains one, so nothing matches
/// across a boundary.
///
/// **Custom field VALUES are indexed; their names are not.** `custom` is a
/// schema the user invented — `Client`, `Room`, `Invoice #` — and indexing the
/// names would make every session that merely HAS a `Client` field a hit for
/// "client", drowning the sessions whose client actually is one. Tags ARE
/// values, and are indexed.
///
/// The JSON columns are decoded rather than indexed as raw JSON, so a search can
/// never match on `[`, `"` or a `\u00e9` escape. A column whose JSON does not
/// parse contributes its raw text instead: the alternative is a session that is
/// silently unsearchable, and this index would rather over-index a bracket than
/// lose a note.
pub fn searchable_text(row: &RecordingRow) -> String {
    compose_searchable_text(
        row.title.as_deref(),
        row.participants_json.as_deref(),
        row.note.as_deref(),
        row.tags_json.as_deref(),
        row.custom_json.as_deref(),
    )
}

/// [`searchable_text`] over the columns themselves, so the backfill — which
/// reads five columns and never builds a [`RecordingRow`] — composes through
/// the same code the write path does.
fn compose_searchable_text(
    title: Option<&str>,
    participants_json: Option<&str>,
    note: Option<&str>,
    tags_json: Option<&str>,
    custom_json: Option<&str>,
) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(title) = title {
        parts.push(title.to_owned());
    }
    if let Some(raw) = participants_json {
        parts.extend(json_text_values(raw));
    }
    if let Some(note) = note {
        parts.push(note.to_owned());
    }
    if let Some(raw) = tags_json {
        parts.extend(json_text_values(raw));
    }
    if let Some(raw) = custom_json {
        parts.extend(custom_field_values(raw));
    }
    // A field the user left blank contributes nothing but a blank line.
    parts.retain(|part| !part.trim().is_empty());
    parts.join("\n")
}

/// Every string a JSON metadata column carries, decoded.
///
/// `tags_json` is a JSON array of strings and `participants_json` is today the
/// JSON *string* encoding of one free-text line — Story 42.1 stores both under
/// a `_json` name for exactly this reason, so Story 42.5 can widen participants
/// to an array without a migration and a reader that handles both arms (this
/// one) needs no change when it does.
///
/// Anything else — a number, an array of objects, text that is not JSON at all
/// — yields the raw string: an unparseable column must still be searchable.
fn json_text_values(raw: &str) -> Vec<String> {
    match serde_json::from_str::<serde_json::Value>(raw) {
        Ok(serde_json::Value::String(text)) => vec![text],
        Ok(serde_json::Value::Array(items)) => {
            let strings: Vec<String> = items
                .iter()
                .filter_map(|item| item.as_str().map(str::to_owned))
                .collect();
            if strings.is_empty() && !items.is_empty() {
                // A non-empty array of something that is not strings is a shape
                // this column never holds; index its text rather than nothing.
                vec![raw.to_owned()]
            } else {
                strings
            }
        }
        Ok(_) | Err(_) => vec![raw.to_owned()],
    }
}

/// The VALUES of a session's custom fields (Story 22.3's name/value pairs).
///
/// Decoded through [`SessionMetaField`] — the very type the manifest serialises
/// — so the index reads exactly what the user typed and cannot drift from the
/// manifest's shape. A column this build cannot parse contributes its raw text,
/// on [`json_text_values`]'s terms.
fn custom_field_values(raw: &str) -> Vec<String> {
    match serde_json::from_str::<Vec<SessionMetaField>>(raw) {
        Ok(fields) => fields.into_iter().map(|field| field.value).collect(),
        Err(_) => vec![raw.to_owned()],
    }
}

/// Search the recordings archive (Story 42.2, FR-140).
///
/// **The free-text dispatch is [`super::fts::search`]'s, to the scalar value.**
/// A `filter.query` of at least [`TRIGRAM_MIN_CHARS`] Unicode scalars runs
/// `recordings_fts MATCH`, bound as ONE double-quoted parameter so `AND`, `OR`
/// and `*` are matched as text and never parsed as FTS operators (an embedded
/// double quote is doubled to stay inside that quoted string). A shorter query
/// cannot form a trigram, so it runs a case-insensitive `LIKE` substring scan
/// over the same indexed text, with its own metacharacters escaped
/// ([`escape_like`]) so `a%` matches the literal two characters.
///
/// **An empty query is no text predicate at all** — not a `LIKE '%%'` scan
/// wearing that name. It returns everything the structured predicates admit,
/// which is what the filter row means when its text box is empty, and it skips
/// the index join entirely.
///
/// Every structured predicate ANDs with the text one and with the others: tags
/// (each hierarchical, see [`TAG_PREDICATE_SQL`]), participant, an inclusive
/// `started_ts` range, durability and profile. A session with no `started_ts`
/// satisfies neither end of a date range — it names no instant, and a filter on
/// instants cannot honestly admit it.
///
/// Ordered `started_ts DESC` — newest first, the order the browser lists in —
/// with `session_id` as the tiebreak so the order is total and a paginated or
/// asserted result cannot depend on which row SQLite happened to visit first.
/// SQLite sorts NULL below everything, so a session with no start stamp sorts
/// last under `DESC`, which is where an undated session belongs.
///
/// A query that matches nothing returns an empty vector. Not an error.
pub fn search_recordings(
    conn: &Connection,
    filter: &RecordingFilter,
) -> Result<Vec<RecordingHit>, ArchiveError> {
    let limit = filter
        .limit
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, DEFAULT_LIMIT);
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();
    let mut clauses: Vec<String> = Vec::new();

    let index_join = if filter.query.is_empty() {
        ""
    } else {
        if filter.query.chars().count() >= TRIGRAM_MIN_CHARS {
            let quoted = filter.query.replace('"', "\"\"");
            clauses.push("recordings_fts MATCH ?".to_owned());
            params.push(Box::new(format!("\"{quoted}\"")));
        } else {
            clauses.push(
                "LOWER(recordings_fts.text) LIKE '%' || LOWER(?) || '%' ESCAPE '\\'".to_owned(),
            );
            params.push(Box::new(escape_like(&filter.query)));
        }
        INDEX_JOIN
    };

    // Story 42.5: a filter tag joins the one vocabulary here, at the boundary
    // where it becomes SQL. `crate::notes::query` normalises a `tag:` term the
    // same way for the same reason — a person typing `Client/Acme ` into a chip
    // means the tag the rows actually carry. A term that normalises to nothing
    // narrows nothing and is dropped, which is also what the old
    // `!tag.is_empty()` guard was for.
    for tag in crate::notes::tags::normalise_all(filter.tags.iter().map(String::as_str)) {
        clauses.push(TAG_PREDICATE_SQL.to_owned());
        // The equality arm takes the canonical tag; the descendant arm is a LIKE
        // and takes it escaped.
        params.push(Box::new(tag.clone()));
        params.push(Box::new(escape_like(&tag)));
    }
    if let Some(participant) = filter.participant.as_deref().filter(|p| !p.is_empty()) {
        clauses.push(PARTICIPANT_PREDICATE_SQL.to_owned());
        params.push(Box::new(escape_like(participant)));
    }
    if let Some(start_ts) = filter.start_ts {
        clauses.push("recordings.started_ts >= ?".to_owned());
        params.push(Box::new(start_ts));
    }
    if let Some(end_ts) = filter.end_ts {
        clauses.push("recordings.started_ts <= ?".to_owned());
        params.push(Box::new(end_ts));
    }
    if let Some(durability) = &filter.durability {
        clauses.push("recordings.durability = ?".to_owned());
        params.push(Box::new(durability.clone()));
    }
    if let Some(profile_id) = &filter.profile_id {
        clauses.push("recordings.profile_id = ?".to_owned());
        params.push(Box::new(profile_id.clone()));
    }

    let where_sql = if clauses.is_empty() {
        String::new()
    } else {
        format!(" WHERE {}", clauses.join(" AND "))
    };
    // `limit` is an i64 this function clamped, never caller text.
    let sql = format!(
        "SELECT {HIT_COLUMNS} FROM recordings{index_join}{where_sql} \
         ORDER BY recordings.started_ts DESC, recordings.session_id ASC \
         LIMIT {limit}"
    );

    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| ArchiveError::Sqlite(format!("could not prepare recording search: {e}")))?;
    let param_refs: Vec<&dyn rusqlite::ToSql> = params.iter().map(|p| p.as_ref()).collect();
    let rows = stmt
        .query_map(param_refs.as_slice(), |r| {
            Ok(RecordingHit {
                session_id: r.get(0)?,
                relative_path: r.get(1)?,
                root_kind: r.get(2)?,
                profile_id: r.get(3)?,
                started_ts: r.get(4)?,
                ended_ts: r.get(5)?,
                title: r.get(6)?,
                participants_json: r.get(7)?,
                note: r.get(8)?,
                tags_json: r.get(9)?,
                durability: r.get(10)?,
            })
        })
        .map_err(|e| ArchiveError::Sqlite(format!("could not run recording search: {e}")))?;
    let mut hits = Vec::new();
    for row in rows {
        hits.push(
            row.map_err(|e| ArchiveError::Sqlite(format!("could not read a recording hit: {e}")))?,
        );
    }
    Ok(hits)
}

/// Search the recordings archive and project the hits into the rows Story 42.3
/// renders (FR-141, UX-DR50).
///
/// **Why this exists rather than a plain `From<RecordingHit>`.** Two of the
/// three things a browser row shows are not on the hit and cannot be derived
/// from it alone: the session's total size, which lives in its
/// `recording_segments` rows, and the file Play hands to the system handler,
/// which is one of those rows' paths. Both need this connection. The shell
/// cannot read them itself — `keeper` has no `rusqlite` dependency and is not
/// getting one for a `SUM` — so the projection lives here, one layer above
/// [`search_recordings`], which it does not touch.
///
/// `destination_root` is the EFFECTIVE recordings destination, resolved by the
/// shell (Story 41.2) and passed in rather than discovered: this crate reads no
/// registry and knows no platform, and the row's absolute path must be the one
/// the recorder would write to, composed exactly once (AD-65).
///
/// Two prepared statements, reused across every hit rather than one aggregate
/// over the whole table: `recording_segments`' primary key begins with
/// `session_id`, so each of the at most [`DEFAULT_LIMIT`] lookups is a keyed
/// b-tree probe, where a `GROUP BY` over every segment ever recorded is a full
/// scan whose cost grows with the archive instead of with the answer.
pub fn search_recording_vms(
    conn: &Connection,
    filter: &RecordingFilter,
    destination_root: &Path,
) -> Result<Vec<RecordingHitVm>, ArchiveError> {
    // An `archive.db` written before Story 42.1 has no `recordings` table at
    // all, and a machine that has not re-opened the WRITER since upgrading
    // still has one — nothing ensures the schema on a read-only connection, and
    // nothing can. The browser must read that as "nothing recorded", exactly as
    // it reads an absent database, rather than as `no such table: recordings`:
    // it is the same fact for the same user, and the writer heals it the next
    // time anything records. One `sqlite_master` probe per query, against a
    // b-tree the connection has already opened.
    let indexed: Option<bool> = conn
        .query_row(
            "SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'recordings'",
            [],
            |_| Ok(true),
        )
        .optional()
        .map_err(|e| {
            ArchiveError::Sqlite(format!("could not probe for the recordings table: {e}"))
        })?;
    if indexed.is_none() {
        return Ok(Vec::new());
    }
    let hits = search_recordings(conn, filter)?;
    let mut total_bytes_stmt = conn
        .prepare("SELECT COALESCE(SUM(bytes), 0) FROM recording_segments WHERE session_id = ?1")
        .map_err(|e| ArchiveError::Sqlite(format!("could not prepare the segment sum: {e}")))?;
    // Screen first, then the lowest index, then the track name so the answer is
    // total: a session that captured no screen (an audio-only one) still has a
    // file to hand to the system handler, and two tracks sharing index 0 cannot
    // make the choice depend on which row SQLite visited first.
    let mut playable_stmt = conn
        .prepare(
            "SELECT relative_path FROM recording_segments WHERE session_id = ?1 \
             ORDER BY CASE track WHEN 'screen' THEN 0 ELSE 1 END, \"index\" ASC, track ASC \
             LIMIT 1",
        )
        .map_err(|e| ArchiveError::Sqlite(format!("could not prepare the segment lookup: {e}")))?;
    let mut vms = Vec::with_capacity(hits.len());
    for hit in hits {
        let total_bytes: i64 = total_bytes_stmt
            .query_row(rusqlite::params![&hit.session_id], |r| r.get(0))
            .map_err(|e| {
                ArchiveError::Sqlite(format!("could not sum a session's segment bytes: {e}"))
            })?;
        let playable_relative: Option<String> = playable_stmt
            .query_row(rusqlite::params![&hit.session_id], |r| r.get(0))
            .optional()
            .map_err(|e| {
                ArchiveError::Sqlite(format!("could not read a session's first segment: {e}"))
            })?;
        vms.push(recording_hit_vm(
            hit,
            destination_root,
            total_bytes,
            playable_relative.as_deref(),
        ));
    }
    Ok(vms)
}

/// One hit, plus what only the database could add, as the row Story 42.3
/// renders. Pure — every fact arrives as a parameter, so the derivations below
/// are asserted without a connection.
///
/// **Duration is `ended - started`, and only when both stamps exist.** A
/// session that is still running, or one a crash left without an end, has no
/// duration — "now minus the start" is elapsed time, a different fact wearing
/// this one's name, and read off a clock this crate deliberately does not own.
/// A pair whose end precedes its start (a clock stepped backwards mid-session)
/// yields `None` for the same reason: a negative duration is not a duration,
/// and a row would rather say nothing than say minus four minutes.
///
/// **Tags are decoded on [`json_text_values`]' terms**, which is what makes the
/// chips agree with [`TAG_PREDICATE_SQL`]: a column that is not a JSON array
/// contributes its raw text as one tag there and as one chip here, so a
/// hand-edited row filters by exactly what it displays. Empty strings are
/// dropped — an empty chip narrows nothing and renders as a gap.
fn recording_hit_vm(
    hit: RecordingHit,
    destination_root: &Path,
    total_bytes: i64,
    playable_relative: Option<&str>,
) -> RecordingHitVm {
    let duration_ms = hit
        .started_ts
        .zip(hit.ended_ts)
        .map(|(started, ended)| ended - started)
        .filter(|duration| *duration >= 0);
    let tags = hit
        .tags_json
        .as_deref()
        .map(json_text_values)
        .unwrap_or_default()
        .into_iter()
        .filter(|tag| !tag.is_empty())
        .collect();
    RecordingHitVm {
        absolute_path: path_string(&join_relative(destination_root, &hit.relative_path)),
        playable_path: playable_relative
            .map(|relative| path_string(&join_relative(destination_root, relative))),
        session_id: hit.session_id,
        relative_path: hit.relative_path,
        title: hit.title,
        started_ts: hit.started_ts,
        ended_ts: hit.ended_ts,
        duration_ms,
        total_bytes,
        durability: hit.durability,
        tags,
    }
}

/// Join a stored `/`-joined relative path onto the destination root, one
/// component at a time.
///
/// Component-wise rather than `root.join(relative)` because the stored form is
/// `/`-joined on every platform (Story 42.1) and Windows would otherwise be
/// handed a single component with separators inside it.
///
/// `.` and `..` are dropped rather than walked. Nothing writes them — every
/// stored path is a reduction over `Normal` components — but this function's
/// answer is a path the shell then opens, and a composition that can climb out
/// of the root is a file-disclosure primitive one bad row away. Dropping keeps
/// the result inside the root by construction, and the shell's containment
/// check still has the last word.
fn join_relative(root: &Path, relative: &str) -> PathBuf {
    let mut path = root.to_path_buf();
    for component in relative.split('/') {
        if component.is_empty() || component == "." || component == ".." {
            continue;
        }
        path.push(component);
    }
    path
}

/// A path as the string that crosses IPC. Lossy, deliberately: a path with a
/// non-UTF-8 component is a path the frontend cannot hold at all, and printing
/// its replacement characters beats dropping the row that has one.
fn path_string(path: &Path) -> String {
    path.to_string_lossy().into_owned()
}

/// Escape SQL `LIKE` metacharacters (`\`, `%`, `_`) so a substring scan matches
/// them literally. Paired with `ESCAPE '\'` in every `LIKE` clause here: the
/// backslash escapes itself and the two wildcards, and nothing else is special.
///
/// [`super::fts`] carries these same nine lines. They are deliberately not
/// shared: it is the whole of what one module owes the other, and a `recordings`
/// → `events` search dependency edge to save nine lines of a rule that cannot
/// change (SQL's `LIKE` metacharacters are fixed) would cost more than the
/// duplication, which both sides test.
fn escape_like(query: &str) -> String {
    let mut out = String::with_capacity(query.len());
    for ch in query.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::{Path, PathBuf};

    use crate::archive::recordings::{
        durability_label, ensure_recordings_schema, move_session, rebuild_from_disk,
        upsert_recording, upsert_segment, RecordingSegmentRow,
    };
    use crate::recording::{
        CaptureTarget, SessionDevices, SessionManifest, SessionMeta, SessionMetaField,
    };
    use crate::vm::RecordingDurabilityState;

    /// A scratch directory no other test can land in — the `recordings.rs`
    /// fixture verbatim, including its process-wide counter (two threads inside
    /// one clock tick would otherwise share a database file).
    fn temp_dir() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "keeper-archive-recordings-fts-test-{}-{}-{n}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        dir
    }

    /// An in-memory archive carrying the recording tables AND their search
    /// index, in the order `open_archive_db` ensures them.
    fn memory_db() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory archive");
        ensure_recordings_schema(&conn).expect("ensure recordings schema");
        ensure_recordings_fts(&conn).expect("ensure recordings search index");
        conn
    }

    /// A session row with no metadata at all: the shape the recorder writes the
    /// instant a session starts.
    fn bare_row(session_id: &str, started_ts: Option<i64>) -> RecordingRow {
        RecordingRow {
            session_id: session_id.to_owned(),
            device_id: Some("01DEVICE".to_owned()),
            relative_path: format!("2026/{session_id}"),
            root_kind: "folder".to_owned(),
            profile_id: None,
            started_ts,
            ended_ts: None,
            title: None,
            participants_json: None,
            note: None,
            tags_json: None,
            custom_json: None,
            codec: None,
            width: None,
            height: None,
            fps: None,
            durability: durability_label(RecordingDurabilityState::Local).to_owned(),
            manifest_version: 1,
        }
    }

    /// The same row with the metadata a finished session carries.
    fn row_with_meta(
        session_id: &str,
        started_ts: i64,
        title: &str,
        participants: &str,
        note: &str,
        tags: &[&str],
    ) -> RecordingRow {
        let mut row = bare_row(session_id, Some(started_ts));
        row.title = Some(title.to_owned());
        row.participants_json =
            Some(serde_json::to_string(participants).expect("encode the participants line"));
        row.note = Some(note.to_owned());
        row.tags_json = Some(serde_json::to_string(tags).expect("encode the tags"));
        row.custom_json = Some(
            serde_json::to_string(&[SessionMetaField {
                name: "room".to_owned(),
                value: "Kensington 3B".to_owned(),
            }])
            .expect("encode the custom fields"),
        );
        row
    }

    /// The session ids a filter matches, in the order the search returned them.
    fn found(conn: &Connection, filter: &RecordingFilter) -> Vec<String> {
        search_recordings(conn, filter)
            .expect("search")
            .into_iter()
            .map(|hit| hit.session_id)
            .collect()
    }

    /// The ids a bare free-text query matches.
    fn found_text(conn: &Connection, query: &str) -> Vec<String> {
        found(
            conn,
            &RecordingFilter {
                query: query.to_owned(),
                ..RecordingFilter::default()
            },
        )
    }

    /// The ids a single hierarchical `tag:` predicate matches.
    fn found_tag(conn: &Connection, tag: &str) -> Vec<String> {
        found(
            conn,
            &RecordingFilter {
                tags: vec![tag.to_owned()],
                ..RecordingFilter::default()
            },
        )
    }

    /// How many entries the index holds in total.
    fn index_entries(conn: &Connection) -> i64 {
        conn.query_row("SELECT COUNT(*) FROM recordings_fts", [], |r| r.get(0))
            .expect("count index entries")
    }

    /// Put one session on disk the way a finished recording leaves one, so a
    /// rebuild has something real to walk.
    fn seed_session(root: &Path, relative: &str, session_id: &str, title: &str) -> PathBuf {
        let folder = relative
            .split('/')
            .fold(root.to_path_buf(), |acc, part| acc.join(part));
        let manifest = SessionManifest::create_with_meta(
            folder.clone(),
            CaptureTarget::display(None),
            SessionDevices {
                system_audio: true,
                microphone: false,
                camera: false,
            },
            Some(SessionMeta {
                session_id: Some(session_id.to_owned()),
                title: Some(title.to_owned()),
                participants: Some("Ada, Grace".to_owned()),
                note: Some("agreed the pricing".to_owned()),
                tags: Some(vec!["client/acme/renewal".to_owned()]),
                custom: Some(vec![SessionMetaField {
                    name: "room".to_owned(),
                    value: "Kensington 3B".to_owned(),
                }]),
            }),
            Some("2026-08-08T09:00:00+02:00".to_owned()),
        )
        .expect("create session folder");
        manifest.write().expect("write manifest");
        folder
    }

    // ---------------------------------------------------------------------
    // The matrix, row by row. Its "Scale" row — 10 000 synthetic sessions
    // inside the committed budget — is the perf gate in
    // `keeper-core/tests/recordings_search_perf.rs`, because a budget is only
    // meaningful measured against a real database.
    // ---------------------------------------------------------------------

    #[test]
    fn pricing_and_pric_both_find_the_session_whose_note_mentions_pricing() {
        let conn = memory_db();
        upsert_recording(
            &conn,
            &row_with_meta(
                "01DEVICE-01PRICING",
                300,
                "Renewal call",
                "Ada, Grace",
                "agreed the pricing",
                &["client/acme"],
            ),
        )
        .expect("index the priced session");
        upsert_recording(
            &conn,
            &row_with_meta(
                "01DEVICE-02STANDUP",
                200,
                "Standup",
                "Bob",
                "nothing to report",
                &["internal"],
            ),
        )
        .expect("index another session");

        assert_eq!(
            found_text(&conn, "pricing"),
            vec!["01DEVICE-01PRICING".to_owned()],
            "the whole word, through the trigram index"
        );
        assert_eq!(
            found_text(&conn, "pric"),
            vec!["01DEVICE-01PRICING".to_owned()],
            "a partial word is still three or more scalars, so still the index"
        );
        assert_eq!(
            found_text(&conn, "PRICING"),
            vec!["01DEVICE-01PRICING".to_owned()],
            "trigram matching is case-insensitive"
        );
    }

    #[test]
    fn a_two_character_query_finds_the_same_session_through_the_like_fallback() {
        let conn = memory_db();
        upsert_recording(
            &conn,
            &row_with_meta(
                "01DEVICE-01PRICING",
                300,
                "Renewal call",
                "Ada, Grace",
                "agreed the pricing",
                &["client/acme"],
            ),
        )
        .expect("index the priced session");
        upsert_recording(
            &conn,
            &row_with_meta(
                "01DEVICE-02STANDUP",
                200,
                "Standup",
                "Bob",
                "nothing to report",
                &["internal"],
            ),
        )
        .expect("index another session");

        // Two scalars cannot form a trigram, so this is the fallback path — and
        // it must find exactly what the index path found.
        assert_eq!(
            found_text(&conn, "pr"),
            vec!["01DEVICE-01PRICING".to_owned()],
            "`pr` reaches `pricing` through the LIKE scan"
        );
        assert_eq!(
            found_text(&conn, "PR"),
            vec!["01DEVICE-01PRICING".to_owned()],
            "the fallback lowercases both sides"
        );
        assert!(
            found_text(&conn, "zq").is_empty(),
            "a short query that matches nothing is an empty vector, not an error"
        );
    }

    #[test]
    fn an_empty_query_returns_every_session_the_predicates_admit_newest_first() {
        let conn = memory_db();
        for (id, started) in [
            ("01DEVICE-01OLDEST", 100),
            ("01DEVICE-02NEWEST", 300),
            ("01DEVICE-03MIDDLE", 200),
        ] {
            upsert_recording(
                &conn,
                &row_with_meta(id, started, "Call", "Ada", "a note", &["internal"]),
            )
            .expect("index a session");
        }
        // A session with no start stamp at all: it sorts last rather than first.
        upsert_recording(&conn, &bare_row("01DEVICE-04UNDATED", None)).expect("index the undated");

        assert_eq!(
            found(&conn, &RecordingFilter::default()),
            vec![
                "01DEVICE-02NEWEST".to_owned(),
                "01DEVICE-03MIDDLE".to_owned(),
                "01DEVICE-01OLDEST".to_owned(),
                "01DEVICE-04UNDATED".to_owned(),
            ],
            "newest first, and an undated session last"
        );
        // The predicates still narrow with no text at all.
        assert_eq!(
            found(
                &conn,
                &RecordingFilter {
                    tags: vec!["internal".to_owned()],
                    start_ts: Some(150),
                    ..RecordingFilter::default()
                }
            ),
            vec![
                "01DEVICE-02NEWEST".to_owned(),
                "01DEVICE-03MIDDLE".to_owned()
            ],
            "an empty query is no text predicate, not a predicate that matches all text"
        );
    }

    #[test]
    fn a_query_of_and_is_matched_as_text_never_as_an_fts_operator() {
        let conn = memory_db();
        upsert_recording(
            &conn,
            &row_with_meta(
                "01DEVICE-01LITERAL",
                300,
                "Terms AND conditions",
                "Ada",
                "a note",
                &["legal"],
            ),
        )
        .expect("index the session whose title says AND");
        upsert_recording(
            &conn,
            &row_with_meta(
                "01DEVICE-02OTHER",
                200,
                "Terms",
                "Bob",
                "conditions",
                &["legal"],
            ),
        )
        .expect("index a session carrying both words but not the operator");

        assert_eq!(
            found_text(&conn, "AND"),
            vec!["01DEVICE-01LITERAL".to_owned()],
            "`AND` is three characters of text, not a conjunction"
        );
        assert_eq!(
            found_text(&conn, "Terms AND conditions"),
            vec!["01DEVICE-01LITERAL".to_owned()],
            "the whole query is one quoted phrase, so the session that merely \
             carries both words separately is not a hit"
        );
    }

    #[test]
    fn a_query_containing_a_star_is_matched_as_text_never_as_a_wildcard() {
        let conn = memory_db();
        upsert_recording(
            &conn,
            &row_with_meta(
                "01DEVICE-01STAR",
                300,
                "the a*b formula",
                "Ada",
                "a note",
                &["math"],
            ),
        )
        .expect("index the session whose title says a*b");
        upsert_recording(
            &conn,
            &row_with_meta(
                "01DEVICE-02AXB",
                200,
                "the axb formula",
                "Bob",
                "",
                &["math"],
            ),
        )
        .expect("index a session a prefix wildcard would wrongly match");

        assert_eq!(
            found_text(&conn, "a*b"),
            vec!["01DEVICE-01STAR".to_owned()],
            "`*` is a character, not an FTS prefix operator"
        );
        // The same rule on the fallback path, where `%` and `_` are the
        // metacharacters that would otherwise widen the scan.
        assert!(
            found_text(&conn, "a%").is_empty(),
            "`a%` is two literal characters, not `starts with a`"
        );
    }

    #[test]
    fn a_tag_predicate_matches_at_the_segment_boundary_and_never_a_lexical_neighbour() {
        let conn = memory_db();
        for (id, started, tags) in [
            ("01DEVICE-01RENEWAL", 400, &["client/acme/renewal"][..]),
            ("01DEVICE-02EXACT", 300, &["client/acme"][..]),
            ("01DEVICE-03CORP", 200, &["client/acmecorp"][..]),
            ("01DEVICE-04OTHER", 100, &["client/other"][..]),
        ] {
            upsert_recording(
                &conn,
                &row_with_meta(id, started, "Call", "Ada", "a note", tags),
            )
            .expect("index a tagged session");
        }

        assert_eq!(
            found_tag(&conn, "client/acme"),
            vec![
                "01DEVICE-01RENEWAL".to_owned(),
                "01DEVICE-02EXACT".to_owned()
            ],
            "the tag itself and its descendants, and nothing else"
        );
        assert_eq!(
            found_tag(&conn, "CLIENT/ACME"),
            vec![
                "01DEVICE-01RENEWAL".to_owned(),
                "01DEVICE-02EXACT".to_owned()
            ],
            "case-insensitively"
        );
        assert_eq!(
            found_tag(&conn, "client/other"),
            vec!["01DEVICE-04OTHER".to_owned()],
            "a sibling matches only itself"
        );
        assert!(
            found_tag(&conn, "client/acmec").is_empty(),
            "a partial segment is not a prefix — `client/acmecorp` is not a \
             descendant of `client/acmec`"
        );
        assert_eq!(
            found_tag(&conn, "client"),
            vec![
                "01DEVICE-01RENEWAL".to_owned(),
                "01DEVICE-02EXACT".to_owned(),
                "01DEVICE-03CORP".to_owned(),
                "01DEVICE-04OTHER".to_owned(),
            ],
            "a parent segment covers every descendant"
        );
        // Two tags narrow rather than widen.
        let mut both = RecordingFilter {
            tags: vec!["client/acme".to_owned(), "client/other".to_owned()],
            ..RecordingFilter::default()
        };
        assert!(
            found(&conn, &both).is_empty(),
            "no session carries both tags, and tags AND together"
        );
        both.tags = vec!["client/acme".to_owned(), String::new()];
        assert_eq!(
            found(&conn, &both),
            vec![
                "01DEVICE-01RENEWAL".to_owned(),
                "01DEVICE-02EXACT".to_owned()
            ],
            "an empty tag narrows nothing"
        );
    }

    #[test]
    fn a_participant_predicate_matches_the_participants_line_case_insensitively() {
        let conn = memory_db();
        upsert_recording(
            &conn,
            &row_with_meta(
                "01DEVICE-01ADA",
                300,
                "Call",
                "Ada, Grace",
                "a note",
                &["internal"],
            ),
        )
        .expect("index the session Ada was in");
        upsert_recording(
            &conn,
            &row_with_meta(
                "01DEVICE-02BOB",
                200,
                "Call",
                "Bob",
                "a note",
                &["internal"],
            ),
        )
        .expect("index a session she was not");

        let by = |participant: &str| {
            found(
                &conn,
                &RecordingFilter {
                    participant: Some(participant.to_owned()),
                    ..RecordingFilter::default()
                },
            )
        };
        assert_eq!(
            by("ada"),
            vec!["01DEVICE-01ADA".to_owned()],
            "a lowercase fragment finds `Ada` inside the line"
        );
        assert_eq!(
            by("GRACE"),
            vec!["01DEVICE-01ADA".to_owned()],
            "and so does the second name in it, in any case"
        );
        assert!(by("cyd").is_empty(), "nobody named Cyd was in either");
        assert_eq!(
            by(""),
            vec!["01DEVICE-01ADA".to_owned(), "01DEVICE-02BOB".to_owned()],
            "an empty participant narrows nothing"
        );
    }

    #[test]
    fn a_date_range_admits_the_session_inside_it_and_an_open_end_is_unbounded() {
        let conn = memory_db();
        for (id, started) in [
            ("01DEVICE-01EARLY", 100),
            ("01DEVICE-02INSIDE", 200),
            ("01DEVICE-03LATE", 300),
        ] {
            upsert_recording(
                &conn,
                &row_with_meta(id, started, "Call", "Ada", "a note", &["internal"]),
            )
            .expect("index a session");
        }
        upsert_recording(&conn, &bare_row("01DEVICE-04UNDATED", None)).expect("index the undated");

        let range = |start: Option<i64>, end: Option<i64>| {
            found(
                &conn,
                &RecordingFilter {
                    start_ts: start,
                    end_ts: end,
                    ..RecordingFilter::default()
                },
            )
        };
        assert_eq!(
            range(Some(150), Some(250)),
            vec!["01DEVICE-02INSIDE".to_owned()],
            "inside is returned, outside is not"
        );
        assert_eq!(
            range(Some(200), Some(200)),
            vec!["01DEVICE-02INSIDE".to_owned()],
            "both bounds are inclusive"
        );
        assert_eq!(
            range(Some(200), None),
            vec!["01DEVICE-03LATE".to_owned(), "01DEVICE-02INSIDE".to_owned()],
            "an open upper end is unbounded above"
        );
        assert_eq!(
            range(None, Some(200)),
            vec![
                "01DEVICE-02INSIDE".to_owned(),
                "01DEVICE-01EARLY".to_owned()
            ],
            "an open lower end is unbounded below"
        );
        assert!(
            !range(Some(0), Some(i64::MAX)).contains(&"01DEVICE-04UNDATED".to_owned()),
            "a session with no start stamp names no instant, so no range admits it"
        );
    }

    #[test]
    fn a_durability_predicate_returns_only_sessions_at_that_state() {
        let conn = memory_db();
        for (id, started, state) in [
            ("01DEVICE-01LOCAL", 300, RecordingDurabilityState::Local),
            ("01DEVICE-02PUSHED", 200, RecordingDurabilityState::Pushed),
            ("01DEVICE-03PUSHED", 100, RecordingDurabilityState::Pushed),
        ] {
            let mut row = row_with_meta(id, started, "Call", "Ada", "a note", &["internal"]);
            row.durability = durability_label(state).to_owned();
            upsert_recording(&conn, &row).expect("index a session");
        }

        assert_eq!(
            found(
                &conn,
                &RecordingFilter {
                    durability: Some(durability_label(RecordingDurabilityState::Pushed).to_owned()),
                    ..RecordingFilter::default()
                }
            ),
            vec![
                "01DEVICE-02PUSHED".to_owned(),
                "01DEVICE-03PUSHED".to_owned()
            ],
            "only the sessions whose bytes reached that state"
        );
        assert!(
            found(
                &conn,
                &RecordingFilter {
                    durability: Some(
                        durability_label(RecordingDurabilityState::Committed).to_owned()
                    ),
                    ..RecordingFilter::default()
                }
            )
            .is_empty(),
            "a state no session reached matches nothing"
        );
    }

    #[test]
    fn a_profile_predicate_returns_only_the_sessions_indexed_under_that_profile() {
        let conn = memory_db();
        for (id, started, profile) in [
            ("01DEVICE-01ONE", 300, Some("01PROFILEONE")),
            ("01DEVICE-02TWO", 200, Some("01PROFILETWO")),
            ("01DEVICE-03FOLDER", 100, None),
        ] {
            let mut row = row_with_meta(id, started, "Call", "Ada", "a note", &["internal"]);
            row.profile_id = profile.map(str::to_owned);
            row.root_kind = if profile.is_some() {
                "profile"
            } else {
                "folder"
            }
            .to_owned();
            upsert_recording(&conn, &row).expect("index a session");
        }

        assert_eq!(
            found(
                &conn,
                &RecordingFilter {
                    profile_id: Some("01PROFILEONE".to_owned()),
                    ..RecordingFilter::default()
                }
            ),
            vec!["01DEVICE-01ONE".to_owned()],
            "only the sessions recorded into that profile — a plain-folder \
             session has no profile and is never one of them"
        );
    }

    #[test]
    fn a_session_with_no_metadata_is_found_by_its_predicates_and_never_by_free_text() {
        let conn = memory_db();
        upsert_recording(&conn, &bare_row("01DEVICE-01BARE", Some(200)))
            .expect("index a session with nothing in it");

        assert_eq!(
            index_entries(&conn),
            1,
            "it is indexed, with empty text — not skipped"
        );
        assert_eq!(
            found(
                &conn,
                &RecordingFilter {
                    start_ts: Some(100),
                    durability: Some(durability_label(RecordingDurabilityState::Local).to_owned()),
                    ..RecordingFilter::default()
                }
            ),
            vec!["01DEVICE-01BARE".to_owned()],
            "the predicates find it"
        );
        assert!(
            found_text(&conn, "anything").is_empty(),
            "no free text does, through the index path"
        );
        assert!(
            found_text(&conn, "an").is_empty(),
            "nor through the fallback path"
        );
    }

    #[test]
    fn retitling_a_session_makes_the_index_forget_the_old_text_in_the_same_transaction() {
        let conn = memory_db();
        let before = row_with_meta(
            "01DEVICE-01SESSION",
            300,
            "Quarterly pricing review",
            "Ada",
            "the old note",
            &["client/acme"],
        );
        upsert_recording(&conn, &before).expect("the recorder's row");
        assert_eq!(
            found_text(&conn, "pricing"),
            vec![before.session_id.clone()]
        );

        // A Story 40.4 retitle (or any finalize that rewrites the metadata) is
        // one `INSERT OR REPLACE` — and the index moves with it.
        let mut after = before.clone();
        after.title = Some("Quarterly staffing review".to_owned());
        after.note = Some("the new note".to_owned());
        upsert_recording(&conn, &after).expect("the retitled row");

        assert!(
            found_text(&conn, "pricing").is_empty(),
            "the OLD text no longer matches — a stale index entry is a bug"
        );
        assert!(
            found_text(&conn, "old note").is_empty(),
            "and neither does the old note"
        );
        assert_eq!(
            found_text(&conn, "staffing"),
            vec![after.session_id.clone()],
            "the new text does"
        );
        assert_eq!(
            found_text(&conn, "new note"),
            vec![after.session_id.clone()],
            "and so does the new note"
        );
        assert_eq!(index_entries(&conn), 1, "still one entry, not two");
    }

    #[test]
    fn replacing_a_session_row_leaves_exactly_one_index_entry() {
        let conn = memory_db();
        let row = row_with_meta(
            "01DEVICE-01SESSION",
            300,
            "Renewal call",
            "Ada",
            "agreed the pricing",
            &["client/acme"],
        );
        // Start, finalize, and a duplicate finalize: three writes of one session.
        for _ in 0..3 {
            upsert_recording(&conn, &row).expect("write the session row");
        }

        assert_eq!(index_entries(&conn), 1, "one session, one index entry");
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM recordings_fts_docs", [], |r| r
                .get::<_, i64>(0))
                .expect("count doc ids"),
            1,
            "and one doc id, reserved once and reused"
        );
        assert_eq!(
            found_text(&conn, "pricing"),
            vec![row.session_id.clone()],
            "the session is returned once, not three times"
        );
    }

    #[test]
    fn a_failed_index_write_rolls_back_the_row_it_describes() {
        let conn = memory_db();
        // The only way to make the index write fail on a healthy connection is
        // to take the index away, which is exactly the crash-shaped case the
        // one-transaction rule exists for: whatever happens, a row and its
        // index are written together or not at all.
        conn.execute("DROP TABLE recordings_fts", [])
            .expect("drop the index");

        let row = row_with_meta(
            "01DEVICE-01SESSION",
            300,
            "Renewal call",
            "Ada",
            "agreed the pricing",
            &["client/acme"],
        );
        assert!(
            upsert_recording(&conn, &row).is_err(),
            "an index this write cannot maintain fails the write"
        );
        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM recordings", [], |r| r
                .get::<_, i64>(0))
                .expect("count rows"),
            0,
            "and the row it would have described rolled back with it"
        );
    }

    #[test]
    fn moving_a_session_leaves_its_searchable_text_untouched() {
        let conn = memory_db();
        let row = row_with_meta(
            "01DEVICE-01SESSION",
            300,
            "Renewal call",
            "Ada",
            "agreed the pricing",
            &["client/acme"],
        );
        upsert_recording(&conn, &row).expect("the recorder's row");

        let moved = move_session(&conn, &row.session_id, "2026/renamed").expect("move");
        assert_eq!(moved, 1);

        // A retitle-move rewrites paths, which are not searchable text: the
        // session is still found by the same query, and still once.
        let hits = search_recordings(
            &conn,
            &RecordingFilter {
                query: "pricing".to_owned(),
                ..RecordingFilter::default()
            },
        )
        .expect("search");
        assert_eq!(hits.len(), 1, "one hit, at its new path");
        assert_eq!(hits[0].relative_path, "2026/renamed");
        assert_eq!(index_entries(&conn), 1, "and one index entry");
    }

    #[test]
    fn a_rebuild_from_disk_leaves_every_row_indexed_exactly_once() {
        let dir = temp_dir();
        let root = dir.join("recordings");
        seed_session(&root, "2026/first", "01DEVICE-01FIRST", "Renewal call");
        seed_session(&root, "2026/second", "01DEVICE-02SECOND", "Standup");

        let conn = memory_db();
        // Rebuilt twice: an index a rebuild duplicates is an index that returns
        // a session twice, and the epic's promise is that a rebuild over an
        // already-indexed tree changes nothing.
        for _ in 0..2 {
            assert_eq!(
                rebuild_from_disk(&conn, &root, "folder", None).expect("rebuild"),
                2
            );
        }

        assert_eq!(
            conn.query_row("SELECT COUNT(*) FROM recordings", [], |r| r
                .get::<_, i64>(0))
                .expect("count rows"),
            2
        );
        assert_eq!(index_entries(&conn), 2, "one entry per row, no duplicates");
        assert_eq!(
            found_text(&conn, "pricing"),
            vec![
                "01DEVICE-01FIRST".to_owned(),
                "01DEVICE-02SECOND".to_owned()
            ],
            "the note every seeded manifest carries finds both, once each"
        );
        assert_eq!(
            found_text(&conn, "Renewal call"),
            vec!["01DEVICE-01FIRST".to_owned()],
            "and a title only the first one carries finds only it"
        );
        assert_eq!(
            found_tag(&conn, "client/acme"),
            vec![
                "01DEVICE-01FIRST".to_owned(),
                "01DEVICE-02SECOND".to_owned()
            ],
            "the manifests' `client/acme/renewal` tag survives the round trip"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn an_archive_written_before_this_index_existed_is_backfilled_on_the_next_open() {
        let conn = Connection::open_in_memory().expect("open in-memory archive");
        ensure_recordings_schema(&conn).expect("ensure recordings schema");
        // A build that knew the rows but not the index wrote this row straight
        // into the table, exactly as `upsert_recording` did before Story 42.2.
        conn.execute(
            "INSERT INTO recordings(session_id, relative_path, root_kind, started_ts, title, \
             note, tags_json, durability, manifest_version) \
             VALUES ('01DEVICE-01OLD', '2026/old', 'folder', 300, 'Renewal call', \
             'agreed the pricing', '[\"client/acme\"]', 'local', 1)",
            [],
        )
        .expect("seed a pre-index row");

        ensure_recordings_fts(&conn).expect("first open with the index");
        assert_eq!(
            found_text(&conn, "pricing"),
            vec!["01DEVICE-01OLD".to_owned()],
            "the open that created the index also indexed what was already there"
        );

        // Idempotent: a second and third open change nothing, and in particular
        // do not index the same session twice.
        ensure_recordings_fts(&conn).expect("second open");
        ensure_recordings_fts(&conn).expect("third open");
        assert_eq!(index_entries(&conn), 1, "still one entry");
        assert_eq!(
            found_text(&conn, "pricing"),
            vec!["01DEVICE-01OLD".to_owned()],
            "and still one hit"
        );

        // Self-healing: an entry lost to a hand-edited database comes back.
        conn.execute("DELETE FROM recordings_fts_docs", [])
            .expect("lose the mapping");
        conn.execute("DELETE FROM recordings_fts", [])
            .expect("lose the entry");
        assert!(found_text(&conn, "pricing").is_empty());
        ensure_recordings_fts(&conn).expect("the next open heals it");
        assert_eq!(
            found_text(&conn, "pricing"),
            vec!["01DEVICE-01OLD".to_owned()],
            "an index entry is derivable from the row, so a lost one is a rescan"
        );
    }

    #[test]
    fn the_indexed_text_carries_every_searchable_field_and_no_custom_field_name() {
        let row = row_with_meta(
            "01DEVICE-01SESSION",
            300,
            "Renewal call",
            "Ada, Grace",
            "agreed the pricing",
            &["client/acme", "renewal"],
        );
        assert_eq!(
            searchable_text(&row),
            "Renewal call\nAda, Grace\nagreed the pricing\nclient/acme\nrenewal\nKensington 3B",
            "title, participants, note, tags and the custom VALUE — decoded, \
             newline-joined, and never the custom field's NAME"
        );

        // A column that is not the JSON it claims to be still contributes its
        // text, and a session with nothing in it indexes to nothing.
        let mut broken = row.clone();
        broken.tags_json = Some("client/acme, renewal".to_owned());
        broken.custom_json = Some("room=3B".to_owned());
        assert_eq!(
            searchable_text(&broken),
            "Renewal call\nAda, Grace\nagreed the pricing\nclient/acme, renewal\nroom=3B",
            "an unparseable column is over-indexed rather than lost"
        );
        assert_eq!(
            searchable_text(&bare_row("01DEVICE-02BARE", None)),
            "",
            "a session with no metadata indexes to empty text"
        );
    }

    #[test]
    fn the_limit_is_clamped_to_the_default_and_never_unbounded() {
        let conn = memory_db();
        for n in 0..5i64 {
            upsert_recording(
                &conn,
                &row_with_meta(
                    &format!("01DEVICE-{n:02}SESSION"),
                    100 + n,
                    "Call",
                    "Ada",
                    "a note",
                    &["internal"],
                ),
            )
            .expect("index a session");
        }

        let limited = |limit: Option<i64>| {
            found(
                &conn,
                &RecordingFilter {
                    limit,
                    ..RecordingFilter::default()
                },
            )
            .len()
        };
        assert_eq!(limited(Some(2)), 2, "a caller's cap is honoured");
        assert_eq!(limited(Some(0)), 1, "zero clamps up to one, never to none");
        assert_eq!(
            limited(Some(-5)),
            1,
            "and so does a negative, which would otherwise mean `no limit` in SQL"
        );
        assert_eq!(
            limited(Some(DEFAULT_LIMIT * 10)),
            5,
            "a cap above the default is clamped to it, and five rows is under both"
        );
        assert_eq!(limited(None), 5, "no cap is the default cap");
    }

    /// Story 42.3: the input seam is a total field move. Every optional is
    /// carried, because a filter that silently dropped one would narrow by less
    /// than the user asked and look like a search bug rather than a mapping bug.
    #[test]
    fn the_filter_vm_seam_carries_every_field_including_the_optionals() {
        let vm = RecordingFilterVm {
            query: "standup".to_owned(),
            tags: vec!["client/acme".to_owned(), "internal".to_owned()],
            participant: Some("Ada".to_owned()),
            start_ts: Some(1_000),
            end_ts: Some(2_000),
            durability: Some("pushed".to_owned()),
            profile_id: Some("01PROFILE".to_owned()),
            limit: Some(7),
        };

        assert_eq!(
            RecordingFilter::from(vm),
            RecordingFilter {
                query: "standup".to_owned(),
                tags: vec!["client/acme".to_owned(), "internal".to_owned()],
                participant: Some("Ada".to_owned()),
                start_ts: Some(1_000),
                end_ts: Some(2_000),
                durability: Some("pushed".to_owned()),
                profile_id: Some("01PROFILE".to_owned()),
                limit: Some(7),
            }
        );
    }

    /// Story 42.3: an unset VM maps to the engine's "every session, newest
    /// first" filter — the question the browser asks before anyone types.
    #[test]
    fn an_empty_filter_vm_maps_to_the_unrestricted_engine_filter() {
        let vm = RecordingFilterVm {
            query: String::new(),
            tags: Vec::new(),
            participant: None,
            start_ts: None,
            end_ts: None,
            durability: None,
            profile_id: None,
            limit: None,
        };

        assert_eq!(RecordingFilter::from(vm), RecordingFilter::default());
    }

    /// One segment row for `session_id`, sized `bytes`, filed under the
    /// session's own relative path the way `recordings` stores one.
    fn segment(session_id: &str, index: u32, track: &str, bytes: u64) -> RecordingSegmentRow {
        RecordingSegmentRow {
            session_id: session_id.to_owned(),
            index,
            track: track.to_owned(),
            relative_path: format!("2026/{session_id}/{track}-{index:04}.mov"),
            bytes,
            pts_start: None,
            pts_end: None,
            closed_ts: None,
        }
    }

    /// The rows the browser would render for an unrestricted filter.
    fn browsed(conn: &Connection, root: &Path) -> Vec<RecordingHitVm> {
        search_recording_vms(conn, &RecordingFilter::default(), root).expect("browse")
    }

    /// Story 42.3: the row's two derived figures. Duration is the span between
    /// the stamps, and size is the sum over the session's segments — including
    /// the camera track, because the row states what the session cost on disk.
    #[test]
    fn a_row_derives_its_duration_from_the_stamps_and_its_size_from_the_segments() {
        let conn = memory_db();
        let mut row = row_with_meta("01DEVICE-01FULL", 1_000, "Standup", "Ada", "notes", &[]);
        row.ended_ts = Some(1_000 + 90_000);
        upsert_recording(&conn, &row).expect("index the session");
        upsert_segment(&conn, &segment("01DEVICE-01FULL", 0, "screen", 4_096)).expect("screen 0");
        upsert_segment(&conn, &segment("01DEVICE-01FULL", 1, "screen", 2_048)).expect("screen 1");
        upsert_segment(&conn, &segment("01DEVICE-01FULL", 0, "camera", 512)).expect("camera 0");

        let rows = browsed(&conn, Path::new("/recordings"));

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].duration_ms, Some(90_000));
        assert_eq!(rows[0].total_bytes, 4_096 + 2_048 + 512);
    }

    /// Story 42.3, the two absences the matrix names. A session that is still
    /// running has no end and therefore no duration — not zero, and not "now
    /// minus the start", which is a clock this crate does not read. A session
    /// with no closed segment has written nothing, and nothing is honestly
    /// zero bytes and no file to play.
    #[test]
    fn a_running_session_has_no_duration_and_a_segmentless_one_has_no_size_or_file() {
        let conn = memory_db();
        upsert_recording(&conn, &bare_row("01DEVICE-01LIVE", Some(1_000)))
            .expect("index the session");

        let rows = browsed(&conn, Path::new("/recordings"));

        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].started_ts, Some(1_000));
        assert_eq!(rows[0].ended_ts, None);
        assert_eq!(
            rows[0].duration_ms, None,
            "a session that has not ended has no duration to state"
        );
        assert_eq!(rows[0].total_bytes, 0);
        assert_eq!(
            rows[0].playable_path, None,
            "nothing was written, so there is no file to hand to the system handler"
        );
    }

    /// Story 42.3: a clock that stepped backwards mid-session yields an end
    /// before its start. A negative duration is not a duration, and the row
    /// says nothing rather than "-4 minutes".
    #[test]
    fn an_end_stamp_before_its_start_yields_no_duration_rather_than_a_negative_one() {
        let conn = memory_db();
        let mut row = bare_row("01DEVICE-01SKEW", Some(5_000));
        row.ended_ts = Some(4_000);
        upsert_recording(&conn, &row).expect("index the session");

        assert_eq!(
            browsed(&conn, Path::new("/recordings"))[0].duration_ms,
            None
        );
    }

    /// Story 42.3: both paths are composed from the destination root the shell
    /// resolved — component by component, so the stored `/`-joined form is a
    /// path on every platform — and Play gets the SCREEN segment even when a
    /// camera segment shares its index.
    #[test]
    fn the_row_resolves_its_folder_and_its_playable_file_under_the_destination_root() {
        let conn = memory_db();
        upsert_recording(&conn, &bare_row("01DEVICE-01PATH", Some(1_000))).expect("index");
        upsert_segment(&conn, &segment("01DEVICE-01PATH", 0, "camera", 1)).expect("camera 0");
        upsert_segment(&conn, &segment("01DEVICE-01PATH", 0, "screen", 2)).expect("screen 0");

        let root = Path::new("/recordings");
        let rows = browsed(&conn, root);

        assert_eq!(rows[0].relative_path, "2026/01DEVICE-01PATH");
        assert_eq!(
            rows[0].absolute_path,
            path_string(&root.join("2026").join("01DEVICE-01PATH"))
        );
        assert_eq!(
            rows[0].playable_path,
            Some(path_string(
                &root
                    .join("2026")
                    .join("01DEVICE-01PATH")
                    .join("screen-0000.mov")
            )),
            "the screen track is what Play opens, even though the camera segment shares index 0"
        );
    }

    /// Story 42.3: a session that captured no screen still has something to
    /// play. The ordering is total, so the answer cannot depend on which row
    /// SQLite visited first.
    #[test]
    fn a_session_with_no_screen_track_plays_its_first_segment_of_any_track() {
        let conn = memory_db();
        upsert_recording(&conn, &bare_row("01DEVICE-01AUDIO", Some(1_000))).expect("index");
        upsert_segment(&conn, &segment("01DEVICE-01AUDIO", 1, "audio", 8)).expect("audio 1");
        upsert_segment(&conn, &segment("01DEVICE-01AUDIO", 0, "audio", 8)).expect("audio 0");

        assert_eq!(
            browsed(&conn, Path::new("/recordings"))[0].playable_path,
            Some(path_string(Path::new(
                "/recordings/2026/01DEVICE-01AUDIO/audio-0000.mov"
            )))
        );
    }

    /// Story 42.3: the chips are the decoded tag array, and they agree with
    /// what the `tag:` predicate matches — an empty entry is neither a chip nor
    /// a filter.
    #[test]
    fn the_row_decodes_its_tags_and_drops_the_empty_ones() {
        let conn = memory_db();
        let mut row = bare_row("01DEVICE-01TAGS", Some(1_000));
        row.tags_json =
            Some(serde_json::to_string(&["client/acme", "", "internal"]).expect("encode the tags"));
        upsert_recording(&conn, &row).expect("index");
        let mut untagged = bare_row("01DEVICE-02TAGS", Some(900));
        untagged.tags_json = None;
        upsert_recording(&conn, &untagged).expect("index the untagged session");

        let rows = browsed(&conn, Path::new("/recordings"));

        assert_eq!(rows[0].session_id, "01DEVICE-01TAGS");
        assert_eq!(rows[0].tags, vec!["client/acme", "internal"]);
        assert_eq!(rows[1].session_id, "01DEVICE-02TAGS");
        assert!(
            rows[1].tags.is_empty(),
            "a session with no tags column has no chips, not one empty chip"
        );
    }

    /// Story 42.3: the projection is a projection — it applies the filter it is
    /// given and returns nothing when nothing matches, exactly as the engine
    /// under it does.
    #[test]
    fn the_browser_projection_honours_the_filter_and_returns_no_rows_for_no_matches() {
        let conn = memory_db();
        upsert_recording(
            &conn,
            &row_with_meta("01DEVICE-01MATCH", 1_000, "Standup", "Ada", "notes", &[]),
        )
        .expect("index");

        let matching = search_recording_vms(
            &conn,
            &RecordingFilter {
                query: "standup".to_owned(),
                ..RecordingFilter::default()
            },
            Path::new("/recordings"),
        )
        .expect("search");
        let missing = search_recording_vms(
            &conn,
            &RecordingFilter {
                query: "retrospective".to_owned(),
                ..RecordingFilter::default()
            },
            Path::new("/recordings"),
        )
        .expect("search");

        assert_eq!(matching.len(), 1);
        assert_eq!(matching[0].title.as_deref(), Some("Standup"));
        assert!(missing.is_empty());
    }

    /// Story 42.3: a stored path can never compose out of the destination root.
    /// Nothing writes `..` — every stored path is a reduction over plain
    /// components — but this composition's answer is a path the shell then
    /// opens, and "one bad row is a file-disclosure primitive" is not a
    /// property worth having.
    #[test]
    fn a_relative_path_can_never_climb_out_of_the_destination_root() {
        let root = Path::new("/recordings");

        assert_eq!(
            join_relative(root, "../../etc/passwd"),
            root.join("etc").join("passwd")
        );
        assert_eq!(
            join_relative(root, "2026//./x"),
            root.join("2026").join("x")
        );
        assert_eq!(join_relative(root, ""), root);
    }

    /// Story 42.3: an `archive.db` from a build before Story 42.1 has no
    /// recordings tables, and nothing can create them on a read-only
    /// connection. That is "nothing recorded" to the person browsing, not
    /// `no such table` in an error dialog.
    #[test]
    fn browsing_an_archive_that_predates_the_recordings_tables_yields_no_rows() {
        let conn = Connection::open_in_memory().expect("open in-memory archive");

        let rows =
            search_recording_vms(&conn, &RecordingFilter::default(), Path::new("/recordings"))
                .expect("an archive with no recordings tables is an empty answer, never an error");

        assert!(rows.is_empty());
    }

    #[test]
    fn a_tag_filter_is_normalised_before_it_becomes_a_predicate() {
        // Story 42.5: rows carry canonical tags, so a chip must be read as the
        // same vocabulary or the two would only agree by accident of `LOWER`.
        // `Client/Acme ` is the AC1 shape; the trailing space is what a plain
        // `LOWER` comparison would have missed.
        let conn = memory_db();
        upsert_recording(
            &conn,
            &row_with_meta(
                "01DEVICE-01RENEWAL",
                400,
                "Renewal call",
                "Ada",
                "agreed the pricing",
                &["client/acme/renewal"],
            ),
        )
        .expect("index a tagged session");

        for typed in [
            "Client/Acme ",
            "#client/acme",
            "client//acme/",
            "CLIENT/ACME",
        ] {
            assert_eq!(
                found_tag(&conn, typed),
                vec!["01DEVICE-01RENEWAL".to_owned()],
                "`{typed}` is the same tag as `client/acme`"
            );
        }
        // A term that is not a tag narrows nothing rather than matching nothing:
        // an empty chip must not empty the list.
        assert_eq!(
            found(
                &conn,
                &RecordingFilter {
                    tags: vec!["///".to_owned(), String::new()],
                    ..RecordingFilter::default()
                }
            ),
            vec!["01DEVICE-01RENEWAL".to_owned()]
        );
        // And the boundary still holds: `client/acmecorp` is not under it.
        assert!(found_tag(&conn, "Client/AcmeCorp").is_empty());
    }
}
