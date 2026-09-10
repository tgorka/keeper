//! A recording session as a row in `archive.db` (Story 42.1, FR-139, AD-71).
//!
//! Stories 21.5 and 22.3 gave a session a title, participants, a note, times,
//! tags and custom fields, and wrote all of it into `manifest.json` where —
//! apart from `meta.title` — nothing ever read it again. This module is the
//! other half of that sentence: two tables, `recordings` and
//! `recording_segments`, in the archive database the app already keeps, so a
//! session can be listed, filtered and (Story 42.2) searched.
//!
//! **The manifest is the truth; the row is a cache of it.** A session folder is
//! synced, opened on other machines and edited by other tools, so the portable
//! plain-text `manifest.json` inside the folder stays authoritative. The row is
//! derivable — [`rebuild_from_disk`] re-derives every one of them by walking the
//! session tree — which is why an absent or stale row is a rescan, never an
//! error, and why deleting `archive.db` loses nothing.
//!
//! **Every path here is RELATIVE to the destination root** ([`relative_session_path`]).
//! FR-145's rule, extended to the index: a row must survive the folder being
//! moved by a Story 40.4 retitle and the whole tree being cloned onto another
//! machine, and an absolute path survives neither.
//!
//! **One writer.** Nothing in this module opens a connection. The recording path
//! sends [`super::ArchiveMsg::UpsertRecording`] /
//! [`super::ArchiveMsg::UpsertRecordingSegment`] /
//! [`super::ArchiveMsg::SetRecordingDurability`] on the archive's existing
//! unbounded channel, and the one serialized writer task applies them on the one
//! connection it already owns. An index write is therefore a channel send, and a
//! failure to index is logged — never a recording failure.
//!
//! Clock-free like the rest of `keeper-core`: every timestamp arrives as a
//! parameter, or is parsed out of the manifest's own ISO-8601 stamps by
//! [`epoch_ms_from_rfc3339`].

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use rusqlite::Connection;
use serde::Deserialize;

use crate::error::ArchiveError;
use crate::notes::tags;
use crate::recording::{SegmentEntry, SessionManifest, RECOVERY_MAX_DEPTH, RECOVERY_MAX_VISITS};
use crate::vm::RecordingDurabilityState;

/// One recording session, in the shape the `recordings` table stores it.
///
/// Plain owned data with public fields: the shell builds one (through
/// [`RecordingRow::from_manifest`]) and hands it to the writer channel, so this
/// crosses no IPC boundary and is not a `Vm`. Every optional field is a fact the
/// session may genuinely not have — a pre-21.5 manifest has no start stamp, a
/// plain-folder destination has no profile, and nothing in the app currently
/// knows a session's encoded frame size.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordingRow {
    /// The session's immutable identity — `meta.session_id` (Story 40.3), or the
    /// derived fallback of [`fallback_session_id`] for a manifest written before
    /// it existed. The primary key, so a duplicate finalize replaces one row
    /// rather than adding a second.
    pub session_id: String,
    /// The device half of `session_id` (`<device ULID>-<session ULID>`), or
    /// `None` when the id carries no device half (a pre-40.3 fallback id).
    pub device_id: Option<String>,
    /// The session folder, relative to the destination root, `/`-joined.
    pub relative_path: String,
    /// Which kind of place the root is — the wire word of
    /// [`crate::vm::RecordingDestinationKind`], `"folder"` or `"profile"`.
    pub root_kind: String,
    /// The destination profile's ULID when the root is a sync profile.
    pub profile_id: Option<String>,
    /// Session start, ms since the Unix epoch, parsed from the manifest's
    /// `startedAt`. `None` for a pre-21.5 manifest that carries no stamp: a
    /// missing instant is stored as missing, never as 1970.
    pub started_ts: Option<i64>,
    /// Session end, ms since the Unix epoch; `None` while the session runs.
    pub ended_ts: Option<i64>,
    /// The user's title for the session.
    pub title: Option<String>,
    /// Who the recording is with, as JSON. The manifest carries this as one free
    /// text line, so the column holds that text's JSON *string* encoding — see
    /// the column's note on [`ensure_recordings_schema`] for why the shape is
    /// JSON rather than plain text.
    pub participants_json: Option<String>,
    /// The user's free-text note about the session.
    pub note: Option<String>,
    /// The session's tags as a JSON array of strings, **normalised** — the
    /// canonical form [`crate::notes::tags::normalise`] defines, deduplicated
    /// (Story 42.5). The manifest still holds what the user typed; this column
    /// holds what it means.
    pub tags_json: Option<String>,
    /// The session's custom name/value pairs as a JSON array of objects.
    pub custom_json: Option<String>,
    /// The video codec the session recorded with (`"h264"`/`"hevc"`), from the
    /// live session's parameters. `None` on a rebuild — the manifest has no
    /// video block to read it back from.
    pub codec: Option<String>,
    /// Encoded frame width. Always `None` today: nothing in the app knows it
    /// (the sidecar never reports it and no manifest field carries it).
    pub width: Option<u32>,
    /// Encoded frame height. Always `None` today, for the same reason.
    pub height: Option<u32>,
    /// Frames per second, from the live session's parameters; `None` on rebuild.
    pub fps: Option<u32>,
    /// How far the session's bytes have travelled, as the wire word of
    /// [`RecordingDurabilityState`] — build it with [`durability_label`]. Epic
    /// 41's floor applies on write: this can never pull a stored row backwards.
    pub durability: String,
    /// The `manifest.json` schema version the row was derived from.
    pub manifest_version: u32,
}

/// One closed segment of a session, in the shape `recording_segments` stores it.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordingSegmentRow {
    /// The owning session's [`RecordingRow::session_id`].
    pub session_id: String,
    /// The zero-based segment index within its track.
    pub index: u32,
    /// The track the segment belongs to: `"screen"`, `"camera"` or `"audio"`.
    /// Part of the key, because the camera file shares the screen file's index.
    pub track: String,
    /// The segment file relative to the destination root, `/`-joined — the
    /// session's own relative path plus the ledger's basename.
    pub relative_path: String,
    /// The segment's size in bytes, as the manifest's ledger reports it.
    pub bytes: u64,
    /// First sample PTS in original capture-clock seconds (Story 17.4), or
    /// `None` when the sidecar did not report bounds.
    pub pts_start: Option<f64>,
    /// Last sample PTS in original capture-clock seconds, or `None`.
    pub pts_end: Option<f64>,
    /// When the segment closed, ms since the Unix epoch, as the live path
    /// observed it. `None` on a rebuild: a manifest ledger carries no close
    /// time, and inventing one from the file's mtime would be a different fact
    /// wearing this one's name.
    pub closed_ts: Option<i64>,
}

/// The `recordings` columns that are nullable, and therefore addable to an
/// already-created table by the archive's additive-migration helper.
///
/// Every column here also appears in the `CREATE TABLE` below, exactly as the
/// `events` table's [`super::db`] columns do: on a fresh database the helper
/// finds them all present and does nothing, and on a database created by an
/// earlier build it adds only what is missing. The `NOT NULL` columns are absent
/// from this list because `ALTER TABLE … ADD COLUMN` cannot add one without a
/// default — which is the same reason they are the only columns a row can never
/// omit.
const RECORDINGS_ADDITIVE_COLUMNS: &[(&str, &str)] = &[
    ("device_id", "TEXT"),
    ("profile_id", "TEXT"),
    ("started_ts", "INTEGER"),
    ("ended_ts", "INTEGER"),
    ("title", "TEXT"),
    ("participants_json", "TEXT"),
    ("note", "TEXT"),
    ("tags_json", "TEXT"),
    ("custom_json", "TEXT"),
    ("codec", "TEXT"),
    ("width", "INTEGER"),
    ("height", "INTEGER"),
    ("fps", "INTEGER"),
];

/// The nullable `recording_segments` columns, on the same additive terms as
/// [`RECORDINGS_ADDITIVE_COLUMNS`].
const RECORDING_SEGMENTS_ADDITIVE_COLUMNS: &[(&str, &str)] = &[
    ("pts_start", "REAL"),
    ("pts_end", "REAL"),
    ("closed_ts", "INTEGER"),
];

/// Create the two recording tables and their indexes, and additively migrate an
/// existing pair (Story 42.1).
///
/// Called from [`super::db::open_archive_db`], so every connection the writer
/// task ever owns has them. Idempotent in the strict sense the AC asks for: the
/// first open creates, and the second and third change nothing —
/// `CREATE TABLE IF NOT EXISTS` never alters an existing table, and the additive
/// helper only adds columns `PRAGMA table_info` says are missing.
///
/// **The `recordings` shape.** `session_id` is the primary key because it is the
/// session's identity (Story 40.3) rather than its location: a Story 40.4
/// retitle moves the folder and leaves the id byte-identical, so the row follows
/// the session instead of the path. `relative_path` and every other path column
/// is root-relative for the same reason.
///
/// `participants_json`, `tags_json` and `custom_json` all hold JSON.
/// `tags_json`/`custom_json` are natural arrays; `participants_json` today holds
/// the JSON *string* encoding of the manifest's one free-text participants line.
/// Storing the text raw under a `_json` name would make every reader decode
/// something that is not JSON; storing it as JSON means Story 42.5 can widen
/// participants to an array without a migration, because a `serde_json::Value`
/// reader already handles both arms.
///
/// **The indexes are the three predicates the epic names for 42.2 and 42.3**, and
/// nothing else. `started_ts` alone serves the date range and the newest-first
/// list. `durability` and `profile_id` are each paired with `started_ts` rather
/// than indexed alone: both are low-cardinality (four states; a handful of
/// profiles), so an index on the bare column would rarely beat a scan — pairing
/// it with the sort key is what makes filter-then-order a single index walk and
/// earns the write cost. `recording_segments` gets no index at all: its primary
/// key already begins with `session_id`, which is the only way anything looks a
/// segment up.
pub fn ensure_recordings_schema(conn: &Connection) -> Result<(), ArchiveError> {
    conn.execute(
        "CREATE TABLE IF NOT EXISTS recordings(\
            session_id TEXT PRIMARY KEY, \
            device_id TEXT, \
            relative_path TEXT NOT NULL, \
            root_kind TEXT NOT NULL, \
            profile_id TEXT, \
            started_ts INTEGER, \
            ended_ts INTEGER, \
            title TEXT, \
            participants_json TEXT, \
            note TEXT, \
            tags_json TEXT, \
            custom_json TEXT, \
            codec TEXT, \
            width INTEGER, \
            height INTEGER, \
            fps INTEGER, \
            durability TEXT NOT NULL, \
            manifest_version INTEGER NOT NULL\
        )",
        [],
    )
    .map_err(|e| ArchiveError::Sqlite(format!("could not ensure recordings schema: {e}")))?;
    // `index` is a SQLite keyword, so the column is quoted everywhere it is
    // named. The epic's column list is the contract 42.2/42.3 are written
    // against, so the name stays and the quoting is ours to carry.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS recording_segments(\
            session_id TEXT NOT NULL, \
            \"index\" INTEGER NOT NULL, \
            track TEXT NOT NULL, \
            relative_path TEXT NOT NULL, \
            bytes INTEGER NOT NULL, \
            pts_start REAL, \
            pts_end REAL, \
            closed_ts INTEGER, \
            PRIMARY KEY(session_id, \"index\", track)\
        )",
        [],
    )
    .map_err(|e| {
        ArchiveError::Sqlite(format!("could not ensure recording_segments schema: {e}"))
    })?;
    super::db::add_missing_columns(conn, "recordings", RECORDINGS_ADDITIVE_COLUMNS)?;
    super::db::add_missing_columns(
        conn,
        "recording_segments",
        RECORDING_SEGMENTS_ADDITIVE_COLUMNS,
    )?;
    for (name, sql) in [
        (
            "idx_recordings_started_ts",
            "CREATE INDEX IF NOT EXISTS idx_recordings_started_ts ON recordings(started_ts)",
        ),
        (
            "idx_recordings_durability",
            "CREATE INDEX IF NOT EXISTS idx_recordings_durability \
             ON recordings(durability, started_ts)",
        ),
        (
            "idx_recordings_profile",
            "CREATE INDEX IF NOT EXISTS idx_recordings_profile \
             ON recordings(profile_id, started_ts)",
        ),
    ] {
        conn.execute(sql, []).map_err(|e| {
            ArchiveError::Sqlite(format!("could not ensure recordings index {name}: {e}"))
        })?;
    }
    Ok(())
}

/// The column word for one durability state — the single spelling the
/// `durability` column ever holds.
///
/// An exhaustive `match`, so a fifth [`RecordingDurabilityState`] cannot be added
/// without deciding what it is called here. The words are epic 41's own wire
/// spelling (the enum's `rename_all = "camelCase"` serialization), which
/// `tests::durability_labels_match_the_wire_spelling_of_the_durability_state`
/// pins so the two can never drift.
pub fn durability_label(state: RecordingDurabilityState) -> &'static str {
    match state {
        RecordingDurabilityState::Local => "local",
        RecordingDurabilityState::Committed => "committed",
        RecordingDurabilityState::Pushed => "pushed",
        RecordingDurabilityState::Verified => "verified",
    }
}

/// Read a stored `durability` word back into epic 41's state, or `None` when the
/// column holds something no state spells.
///
/// Deserialized through serde rather than a hand-written table so there is
/// exactly one place the ordering and the spelling live: the enum declaration in
/// [`crate::vm`], whose variant order IS the floor (its derived `Ord` is
/// documented there as load-bearing). Nothing in this module ranks the states
/// itself.
fn parse_durability(word: &str) -> Option<RecordingDurabilityState> {
    let de = serde::de::value::StrDeserializer::<serde::de::value::Error>::new(word);
    RecordingDurabilityState::deserialize(de).ok()
}

/// Apply epic 41's floor: the stronger of what the row already says and what the
/// caller is writing, as a column word.
///
/// **The floor is the rule, and this is the one place it is applied.** Story
/// 41.6 defines the durability a session reports as a floor — a `max` over
/// everything observed — so the row must not be the thing that undoes it. Both
/// write paths funnel through here, which is what makes it safe for the finalize
/// path to send whatever it happens to know: a session that reached `pushed`
/// mid-recording and then finalizes with a row still saying `local` stays
/// `pushed`.
///
/// An unreadable word (a column hand-edited, or a future build's state read by an
/// older one) ranks below every known state rather than aborting: a stored
/// unknown is replaced by anything known, and an incoming unknown never
/// overwrites a known stored value. The index must never be the thing that
/// refuses a recording.
fn floored_durability(stored: Option<&str>, incoming: &str) -> String {
    let stored_state = stored.and_then(parse_durability);
    let incoming_state = parse_durability(incoming);
    match (stored_state, incoming_state) {
        (Some(stored_state), Some(incoming_state)) if stored_state > incoming_state => {
            durability_label(stored_state).to_owned()
        }
        (Some(stored_state), None) => durability_label(stored_state).to_owned(),
        _ => incoming.to_owned(),
    }
}

/// Read the `durability` word currently stored for a session, or `None` when the
/// session has no row yet.
fn stored_durability(conn: &Connection, session_id: &str) -> Result<Option<String>, ArchiveError> {
    conn.query_row(
        "SELECT durability FROM recordings WHERE session_id = ?1",
        rusqlite::params![session_id],
        |r| r.get::<_, String>(0),
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(ArchiveError::Sqlite(format!(
            "could not read stored durability: {other}"
        ))),
    })
}

/// Cast a `u64` byte count to the `i64` SQLite stores. A segment larger than
/// 8 EiB cannot exist, so the clamp is unreachable — it is here because the
/// archive path never panics.
fn as_i64(value: u64) -> i64 {
    i64::try_from(value).unwrap_or(i64::MAX)
}

/// Write one session row, replacing any row already keyed on its `session_id`
/// (Story 42.1).
///
/// `INSERT OR REPLACE` is what makes session-start-then-finalize one row and a
/// duplicate finalize still one row. Two groups of column are exempt from that
/// blind overwrite, for one reason: a replace must never destroy a fact the
/// writer of the new row simply does not know.
///
/// - `durability` is floored (see [`floored_durability`]), so a late row
///   carrying a weaker state cannot walk the column backwards.
/// - `codec`, `fps`, `width` and `height` are kept when the incoming value is
///   `None`. These are the columns NO manifest carries — only the live
///   session's parameters ever know them, which is why
///   [`RecordingRow::from_manifest`] leaves all four empty and the finalize
///   path overrides them. Without the `COALESCE`, running [`rebuild_from_disk`]
///   over an already-indexed tree would erase precisely the facts that exist
///   nowhere on disk, and the epic's promise is that a rebuild loses nothing.
///   A `Some` always wins, so a writer that does know still corrects the value.
///
/// The `COALESCE` subqueries read the very row this statement is about to
/// replace: SQLite evaluates the `VALUES` expressions before the replace
/// deletes the conflicting row, so preserving costs no extra round trip, no
/// read-modify-write race and — the rule of this module — no second connection.
///
/// **The row and its search index are one unit of work** (Story 42.2). The
/// `INSERT OR REPLACE` above and the [`super::recordings_fts::index_recording`]
/// call below run inside one [`in_transaction`], so a process that dies between
/// them leaves neither half: a session whose row says "staffing review" and
/// whose index still says "pricing review" is a bug, not a state this module is
/// allowed to reach. The floor read is inside it too, which makes the
/// read-modify-write of `durability` atomic rather than merely serialized.
/// Reentrant by design — [`write_rebuilt_session`] calls this INSIDE its own
/// transaction, and a whole rebuilt session still commits exactly once (see
/// [`in_transaction`]).
///
/// The index write comes second because it describes the row: reading the
/// searchable text off the [`RecordingRow`] the statement just wrote is what
/// makes "exactly one index entry per session, always current" true for a
/// replace as well as an insert.
pub fn upsert_recording(conn: &Connection, row: &RecordingRow) -> Result<(), ArchiveError> {
    in_transaction(conn, "recording row", || {
        let stored = stored_durability(conn, &row.session_id)?;
        let durability = floored_durability(stored.as_deref(), &row.durability);
        write_recording_as(conn, row, &durability)
    })
}

/// The body of [`upsert_recording`] with the `durability` word decided by the
/// caller — the one write both spellings share.
///
/// [`upsert_recording`] floors it against the row on file; a rebuild
/// ([`write_rebuilt_session`]) has already read that row for another reason
/// and decides the word from what it read — floored where the session sits
/// where it sat, exact where it has moved — so it resolves the word itself
/// and comes here with it, rather than reading the row a second time.
fn write_recording_as(
    conn: &Connection,
    row: &RecordingRow,
    durability: &str,
) -> Result<(), ArchiveError> {
    in_transaction(conn, "recording row", || {
        conn.execute(
            "INSERT OR REPLACE INTO recordings(\
                session_id, device_id, relative_path, root_kind, profile_id, started_ts, \
                ended_ts, title, participants_json, note, tags_json, custom_json, codec, \
                width, height, fps, durability, manifest_version\
            ) VALUES (\
                ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, \
                COALESCE(?13, (SELECT codec FROM recordings WHERE session_id = ?1)), \
                COALESCE(?14, (SELECT width FROM recordings WHERE session_id = ?1)), \
                COALESCE(?15, (SELECT height FROM recordings WHERE session_id = ?1)), \
                COALESCE(?16, (SELECT fps FROM recordings WHERE session_id = ?1)), \
                ?17, ?18\
            )",
            rusqlite::params![
                row.session_id,
                row.device_id,
                row.relative_path,
                row.root_kind,
                row.profile_id,
                row.started_ts,
                row.ended_ts,
                row.title,
                row.participants_json,
                row.note,
                row.tags_json,
                row.custom_json,
                row.codec,
                row.width,
                row.height,
                row.fps,
                durability,
                row.manifest_version,
            ],
        )
        .map_err(|e| ArchiveError::Sqlite(format!("could not write recording row: {e}")))?;
        super::recordings_fts::index_recording(conn, row)
    })
}

/// Write one segment row, replacing any row already keyed on
/// `(session_id, index, track)` (Story 42.1).
///
/// A segment is re-reported whenever a session is rebuilt from disk, and its
/// byte count changes when a crash-orphaned `.partial` is finalised (Story
/// 41.3), so the conflict resolution is a replace rather than an ignore: the
/// latest reading of a segment is the true one.
///
/// `closed_ts` is the one column that survives a replace, on exactly
/// [`upsert_recording`]'s terms. Only the live path observes when a segment
/// closed; a ledger records no close time, so a row derived from a manifest
/// carries `None` and must not be allowed to erase the stamp a live run left
/// behind. The `COALESCE` keeps the stored value in the same statement that
/// writes the rest of the row.
pub fn upsert_segment(conn: &Connection, row: &RecordingSegmentRow) -> Result<(), ArchiveError> {
    conn.execute(
        "INSERT OR REPLACE INTO recording_segments(\
            session_id, \"index\", track, relative_path, bytes, pts_start, pts_end, closed_ts\
        ) VALUES (\
            ?1, ?2, ?3, ?4, ?5, ?6, ?7, \
            COALESCE(?8, (SELECT closed_ts FROM recording_segments \
                          WHERE session_id = ?1 AND \"index\" = ?2 AND track = ?3))\
        )",
        rusqlite::params![
            row.session_id,
            row.index,
            row.track,
            row.relative_path,
            as_i64(row.bytes),
            row.pts_start,
            row.pts_end,
            row.closed_ts,
        ],
    )
    .map_err(|e| ArchiveError::Sqlite(format!("could not write recording segment row: {e}")))?;
    Ok(())
}

/// Advance a session's durability, never regress it (Story 42.1, epic 41's
/// floor).
///
/// The `UPDATE` is issued only when the incoming state is genuinely stronger
/// than the stored one, so `pushed` → `committed` is a silent no-op rather than
/// a rewrite. A session with no row yet updates zero rows and is not an error:
/// the durability poll can outrun the start message, and the row that lands next
/// carries its own floor anyway.
pub fn set_durability(
    conn: &Connection,
    session_id: &str,
    durability: &str,
) -> Result<(), ArchiveError> {
    let Some(stored) = stored_durability(conn, session_id)? else {
        return Ok(());
    };
    let floored = floored_durability(Some(stored.as_str()), durability);
    if floored == stored {
        return Ok(());
    }
    conn.execute(
        "UPDATE recordings SET durability = ?2 WHERE session_id = ?1",
        rusqlite::params![session_id, floored],
    )
    .map_err(|e| ArchiveError::Sqlite(format!("could not update recording durability: {e}")))?;
    Ok(())
}

/// Run `write` as one transaction on the writer's connection: `BEGIN
/// IMMEDIATE`, then `COMMIT` on success or `ROLLBACK` on any error.
///
/// The shape [`super::db::delete_account_archive`] already uses, lifted here for
/// this module's multi-statement writes. Two of them exist — rebuilding one
/// session, and moving one — and both issue a delete followed by inserts or a
/// pair of updates. Left in autocommit, each statement is its own transaction,
/// so a reader on another connection can catch a session with its old segment
/// rows deleted and its new ones not yet written, and a fifty-session rebuild
/// pays fifty times the commits it needs. `IMMEDIATE` rather than a deferred
/// `BEGIN` so the write lock is taken up front instead of being upgraded half
/// way through, where it could fail as `SQLITE_BUSY` with statements already
/// applied.
///
/// `label` names the unit of work in the only two errors this adds of its own;
/// whatever `write` returns propagates unchanged, so a SQLite failure inside a
/// rebuilt session still surfaces as the error that raised it.
///
/// **Reentrant, because Story 42.2 made these units of work nest.** Every write
/// that touches a session row now also maintains that row's search index
/// ([`super::recordings_fts`]), and both [`upsert_recording`] — which a caller
/// may reach directly, in autocommit — and [`write_rebuilt_session`] — which
/// wraps it in a transaction of its own, so a whole rebuilt session commits
/// once — must be able to ask for one. SQLite has no nested `BEGIN`, so a
/// second one would fail with "cannot start a transaction within a
/// transaction". When a transaction is already active this therefore just runs
/// `write`: the OUTER transaction is what makes the work atomic, which is the
/// property being asked for either way. The one thing that must hold for that
/// to be true is that an inner error propagates to the outer `in_transaction`
/// rather than being swallowed, and every caller here does propagate it.
pub(super) fn in_transaction<T>(
    conn: &Connection,
    label: &str,
    write: impl FnOnce() -> Result<T, ArchiveError>,
) -> Result<T, ArchiveError> {
    if !conn.is_autocommit() {
        return write();
    }
    conn.execute_batch("BEGIN IMMEDIATE")
        .map_err(|e| ArchiveError::Sqlite(format!("could not begin {label}: {e}")))?;
    match write() {
        Ok(value) => {
            conn.execute_batch("COMMIT")
                .map_err(|e| ArchiveError::Sqlite(format!("could not commit {label}: {e}")))?;
            Ok(value)
        }
        Err(error) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(error)
        }
    }
}

/// Point a session's rows at the folder a Story 40.4 retitle moved it to
/// (Story 42.1, the matrix's "Retitled session" row).
///
/// `session_id` is never touched, and that is the whole reason the table is
/// keyed on the session's identity rather than its location: the folder moves,
/// the row follows it, and every reference to the session survives the rename.
///
/// Every segment row moves with it. A segment's stored path is by construction
/// the session folder's relative path plus the ledger line's basename (see
/// [`RecordingSegmentRow::from_entry`]), so each one is recomputed from that
/// basename rather than rewritten by a prefix `substr` in SQL. A prefix
/// substitution has to assume every stored path begins with exactly the prefix
/// the caller had in mind, and any row that does not — one written before an
/// earlier move, one from a hand-edited database — would come out silently
/// mangled. Recomputing cannot produce a wrong path, only the right one.
///
/// A session with no row updates nothing and returns `Ok(0)`. The index is a
/// cache of the folders, so retitling a session it never saw is not a failure:
/// the next [`rebuild_from_disk`] writes it at its new path anyway. Returns how
/// many `recordings` rows moved — 0 or 1, since `session_id` is the key.
///
/// **This writes nothing to the search index, and that is a decision rather
/// than an omission** (Story 42.2). A move rewrites paths, and a path is not
/// searchable text: the index covers a session's title, participants, note,
/// tags and custom values ([`super::recordings_fts::searchable_text`]), none of
/// which a retitle-MOVE can change. The retitle that renames the session — the
/// one that rewrites `meta.title` — arrives separately as an
/// [`upsert_recording`], which does reindex, inside its own transaction. So the
/// index entry this session already has stays exactly right, and reindexing
/// here would cost a write to produce byte-identical text. The rule the story
/// asks for still holds: every index write is inside the transaction of the row
/// it describes, and this transaction describes no indexed column.
pub fn move_session(
    conn: &Connection,
    session_id: &str,
    new_relative_path: &str,
) -> Result<usize, ArchiveError> {
    in_transaction(conn, "recording move", || {
        let moved = conn
            .execute(
                "UPDATE recordings SET relative_path = ?2 WHERE session_id = ?1",
                rusqlite::params![session_id, new_relative_path],
            )
            .map_err(|e| ArchiveError::Sqlite(format!("could not move recording row: {e}")))?;
        if moved == 0 {
            return Ok(0);
        }
        let segments: Vec<(u32, String, String)> = {
            let mut stmt = conn
                .prepare(
                    "SELECT \"index\", track, relative_path FROM recording_segments \
                     WHERE session_id = ?1",
                )
                .map_err(|e| {
                    ArchiveError::Sqlite(format!("could not read recording segment paths: {e}"))
                })?;
            let rows = stmt
                .query_map(rusqlite::params![session_id], |r| {
                    Ok((
                        r.get::<_, u32>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?,
                    ))
                })
                .map_err(|e| {
                    ArchiveError::Sqlite(format!("could not read recording segment paths: {e}"))
                })?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row.map_err(|e| {
                    ArchiveError::Sqlite(format!("could not read a recording segment path: {e}"))
                })?);
            }
            out
        };
        for (index, track, path) in &segments {
            // Stored paths are always `/`-joined ([`relative_session_path`]),
            // never the platform's separator, so the basename is the same split
            // on every OS. A path with no separator at all is already a bare
            // basename.
            let basename = path
                .rsplit_once('/')
                .map_or(path.as_str(), |(_, name)| name);
            conn.execute(
                "UPDATE recording_segments SET relative_path = ?4 \
                 WHERE session_id = ?1 AND \"index\" = ?2 AND track = ?3",
                rusqlite::params![
                    session_id,
                    index,
                    track,
                    format!("{new_relative_path}/{basename}"),
                ],
            )
            .map_err(|e| {
                ArchiveError::Sqlite(format!("could not move recording segment row: {e}"))
            })?;
        }
        // No index write: see this function's doc comment on why a move cannot
        // change a session's searchable text.
        Ok(moved)
    })
}

/// The root-relative, `/`-joined form of any path inside the destination root —
/// a session folder, or a segment file inside one.
///
/// `None` when the path is not under `root`, or when a component is not UTF-8.
/// Both refusals are deliberate: a row that cannot express its path relatively
/// is a row that would have to store an absolute one, and an absolute path
/// survives neither a retitle-move nor a clone onto another machine. The
/// separator is always `/`, never the platform's, so the same tree read on
/// another OS produces the same string.
pub fn relative_session_path(root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(root).ok()?;
    let mut out = String::new();
    for component in relative.components() {
        let std::path::Component::Normal(name) = component else {
            // `.`, `..`, a root or a prefix cannot appear in a path we built by
            // walking downwards from `root`, and a row must never carry one.
            return None;
        };
        let name = name.to_str()?;
        if !out.is_empty() {
            out.push('/');
        }
        out.push_str(name);
    }
    (!out.is_empty()).then_some(out)
}

/// The session id for a manifest written before Story 40.3 minted one.
///
/// **Nothing a pre-40.3 manifest carries is both unique per session and
/// invariant under a move.** `session` is the folder's basename, which a Story
/// 40.4 retitle rewrites; `startedAt` is absent before 21.5 and only
/// second-resolution when present, so two machines' sessions merged into one
/// synced root can collide on it — and a collision here is worse than any
/// duplicate, because `INSERT OR REPLACE` would silently make two sessions one.
/// Segment basenames are per-folder constants and identify nothing.
///
/// So the fallback is the relative path, namespaced with a `legacy:` prefix that
/// no real `<device ULID>-<session ULID>` can produce. **The consequence, stated
/// plainly:** if such a session's folder moves, a later rebuild mints a
/// different id for it, and the old row — a session no folder under its root
/// carries any more — is forgotten by that root's next reconcile
/// ([`rebuild_from_disk`]). Between the move and that rebuild the browser
/// lists the session twice, and a pre-40.3 session that was `pushed` under
/// the old id starts over at `local` under the new one, because nothing ties
/// the two rows together. Pre-40.3 sessions only — every session recorded
/// since carries `meta.session_id` and is immune.
pub fn fallback_session_id(relative_path: &str) -> String {
    format!("legacy:{relative_path}")
}

impl RecordingRow {
    /// Derive the row a manifest describes (Story 42.1).
    ///
    /// **The one derivation, used by both write paths.** The shell calls it at
    /// session start and again at finalize (overriding `codec`/`fps`, which the
    /// live session's parameters know and the manifest does not), and
    /// [`rebuild_from_disk`] calls it for every manifest it finds. That is what
    /// makes a rebuilt row identical to the row the recorder wrote rather than
    /// merely similar to it — the two cannot drift, because there is only one.
    ///
    /// `durability` starts at [`RecordingDurabilityState::Local`], the honest
    /// floor for a fact no manifest carries; [`upsert_recording`] then keeps
    /// whatever stronger state the row already reached.
    ///
    /// **This is where a recording's tags enter the one vocabulary** (Story
    /// 42.5, FR-143). `tags_json` is written through
    /// [`crate::notes::tags::normalise_all`], so `Client/Acme ` on a recording
    /// and `client/acme` on a note are the same tag everywhere downstream: the
    /// `tag:` predicate, the search text, the browser's chips and the tag tree
    /// all read this column and none of them normalises again. The manifest is
    /// NOT touched — it keeps saying what the user typed, and this row says what
    /// it means. A session whose tags all normalise away stores `NULL`, exactly
    /// as a session that carried none does; an empty tag is never stored.
    ///
    /// Both write paths get this for free, which is the point of there being one
    /// derivation: the live sink and [`rebuild_from_disk`] cannot disagree about
    /// what a tag is, and rebuilding an archive recorded before 42.5 canonicalises
    /// every row it rewrites without rewriting a single manifest.
    ///
    /// The `archive` → `notes::tags` dependency edge is taken deliberately, where
    /// [`days_from_civil`] and [`super::recordings_fts::TAG_PREDICATE_SQL`]
    /// decline theirs. Those duplicate a closed-form identity and a two-arm
    /// prefix test — things that cannot drift because they are fixed for all
    /// time. A tag vocabulary is the opposite: it is user-facing, it will change,
    /// and two copies of it drifting apart is the exact defect this story exists
    /// to delete.
    pub fn from_manifest(
        manifest: &SessionManifest,
        relative_path: String,
        root_kind: &str,
        profile_id: Option<&str>,
    ) -> Self {
        let meta = manifest.meta.as_ref();
        let session_id = meta
            .and_then(|m| m.session_id.clone())
            .unwrap_or_else(|| fallback_session_id(&relative_path));
        // `<device ULID>-<session ULID>` (Story 40.3): both halves are Crockford
        // and `-`-free, so the single separator splits the identity back into the
        // device that made the recording. A fallback id has no device half.
        let device_id = meta
            .and_then(|m| m.session_id.as_deref())
            .and_then(|id| id.split_once('-'))
            .filter(|(device, session)| !device.is_empty() && !session.is_empty())
            .map(|(device, _)| device.to_owned());
        RecordingRow {
            session_id,
            device_id,
            relative_path,
            root_kind: root_kind.to_owned(),
            profile_id: profile_id.map(str::to_owned),
            started_ts: manifest
                .started_at
                .as_deref()
                .and_then(epoch_ms_from_rfc3339),
            ended_ts: manifest.ended_at.as_deref().and_then(epoch_ms_from_rfc3339),
            title: meta.and_then(|m| m.title.clone()),
            participants_json: meta
                .and_then(|m| m.participants.as_ref())
                .and_then(|text| serde_json::to_string(text).ok()),
            note: meta.and_then(|m| m.note.clone()),
            tags_json: meta
                .and_then(|m| m.tags.as_ref())
                .map(|tags| tags::normalise_all(tags.iter().map(String::as_str)))
                .filter(|tags| !tags.is_empty())
                .and_then(|tags| serde_json::to_string(&tags).ok()),
            custom_json: meta
                .and_then(|m| m.custom.as_ref())
                .and_then(|custom| serde_json::to_string(custom).ok()),
            codec: None,
            width: None,
            height: None,
            fps: None,
            durability: durability_label(RecordingDurabilityState::Local).to_owned(),
            manifest_version: manifest.version,
        }
    }

    /// This row's tags — the canonical list the tag tree's second producer
    /// contributes (Story 42.5). See [`decode_tags`] for what an unreadable
    /// column yields.
    pub fn tags(&self) -> Vec<String> {
        decode_tags(self.tags_json.as_deref())
    }
}

/// Decode a stored `tags_json` column into the canonical tag list it holds
/// (Story 42.5).
///
/// Already normalised by construction (see [`RecordingRow::from_manifest`]), so
/// this only decodes. An absent column, or one holding something that is not a
/// JSON array of strings, yields an empty list rather than an error: this feeds
/// a sidebar count, and a hand-edited database must not be able to make the tag
/// tree fail.
fn decode_tags(tags_json: Option<&str>) -> Vec<String> {
    tags_json
        .and_then(|raw| serde_json::from_str::<Vec<String>>(raw).ok())
        .unwrap_or_default()
}

/// Every indexed session's canonical tags, for seeding the tag tree's second
/// producer (Story 42.5, FR-143).
///
/// **A seed, not a second truth.** The tag tree maintains its recording postings
/// incrementally, one finalized session at a time, exactly as it maintains its
/// note postings — but a freshly started process has an empty tree and an
/// archive full of sessions, so it needs the same cold read the note index does
/// off disk. This is that read: `recordings` is itself derived from the
/// manifests ([`rebuild_from_disk`]), so seeding from it is seeding from the
/// truth one step removed, never from a parallel copy of the counts.
///
/// Sessions with no tags are omitted — they contribute nothing and a caller
/// replacing its whole recording posting set should not carry them.
pub fn indexed_tags(conn: &Connection) -> Result<Vec<(String, Vec<String>)>, ArchiveError> {
    let mut statement = conn
        .prepare("SELECT session_id, tags_json FROM recordings WHERE tags_json IS NOT NULL")
        .map_err(|e| ArchiveError::Sqlite(format!("could not read the recording tags: {e}")))?;
    let rows = statement
        .query_map([], |r| {
            Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?))
        })
        .map_err(|e| ArchiveError::Sqlite(format!("could not read the recording tags: {e}")))?;

    let mut seeded: Vec<(String, Vec<String>)> = Vec::new();
    for row in rows {
        let (session_id, tags_json) = row.map_err(|e| {
            ArchiveError::Sqlite(format!("could not read a recording tag row: {e}"))
        })?;
        // Decoded exactly the way the live path decodes it, so a seeded session
        // and a reported one contribute the same list — including the shrug at a
        // hand-edited column, which yields no tags rather than an error.
        let tags = decode_tags(tags_json.as_deref());
        if !tags.is_empty() {
            seeded.push((session_id, tags));
        }
    }
    Ok(seeded)
}

impl RecordingSegmentRow {
    /// Derive one segment row from a manifest ledger entry (Story 42.1).
    ///
    /// `session_relative_path` is the session folder's root-relative path and
    /// `entry.file` its basename, so the segment's own path is the two joined —
    /// relative, like everything else stored here. `closed_ts` is `None`: the
    /// ledger records no close time, and a file mtime is a different fact.
    pub fn from_entry(session_id: &str, session_relative_path: &str, entry: &SegmentEntry) -> Self {
        RecordingSegmentRow {
            session_id: session_id.to_owned(),
            index: entry.index,
            track: entry.track.clone(),
            relative_path: format!("{session_relative_path}/{}", entry.file),
            bytes: entry.bytes,
            pts_start: entry.pts_start,
            pts_end: entry.pts_end,
            closed_ts: None,
        }
    }
}

/// What one root's repository says about a session folder's durability —
/// supplied by the shell, which has a sync engine, to a rebuild here, which
/// must not (`keeper-core` links no `keeper-sync`).
///
/// Asked once per session folder found under a PROFILE root, with the folder's
/// absolute path. `None` means the probe could not answer — a transient read
/// failure the shell has already logged — and the rebuild then treats the
/// folder as one no repository has spoken for: `local`, floored against the
/// row wherever the floor applies (see [`rebuild_from_disk`]), so silence
/// never downgrades anything. A rebuild of the plain-folder destination
/// carries no probe at all: nothing publishes what is recorded there, so
/// `local` is simply the truth.
pub type DurabilityProbeFn = Box<dyn Fn(&Path) -> Option<RecordingDurabilityState> + Send + Sync>;

/// A [`DurabilityProbeFn`] that can ride an [`super::ArchiveMsg`]: the message
/// derives `Debug`, and a boxed closure has none of its own.
pub struct DurabilityProbe(pub DurabilityProbeFn);

impl DurabilityProbe {
    /// Ask the repository about one session folder.
    pub fn ask(&self, folder: &Path) -> Option<RecordingDurabilityState> {
        (self.0)(folder)
    }
}

impl std::fmt::Debug for DurabilityProbe {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DurabilityProbe")
    }
}

/// One recordings root the archive follows, as the rebuild of ANOTHER root
/// needs to know it: enough to look for a session folder that a row says is
/// there (the archive follows every recordings root).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnownRoot {
    /// The absolute root.
    pub root: PathBuf,
    /// `"folder"` or `"profile"` — the `root_kind` column word.
    pub root_kind: String,
    /// The profile, when the root is a synced folder's.
    pub profile_id: Option<String>,
}

impl KnownRoot {
    /// The absolute folder a row's root-relative, `/`-joined path names under
    /// this root — [`relative_session_path`] run backwards.
    fn folder(&self, relative_path: &str) -> PathBuf {
        relative_path
            .split('/')
            .fold(self.root.clone(), |acc, part| acc.join(part))
    }

    /// Whether this is the root a row's `(root_kind, profile_id)` names.
    fn is(&self, root_kind: &str, profile_id: Option<&str>) -> bool {
        self.root_kind == root_kind && self.profile_id.as_deref() == profile_id
    }
}

/// Everything one rebuild of one root needs to know (the archive follows every
/// recordings root). The plain shape of [`super::ArchiveMsg::RebuildRecordings`].
#[derive(Debug)]
pub struct RebuildRequest {
    /// The recordings root to walk. Every path derived under it is stored
    /// relative to it.
    pub root: PathBuf,
    /// `"folder"` or `"profile"` — which kind of place that root is.
    pub root_kind: String,
    /// The profile, when the root is a synced folder's.
    pub profile_id: Option<String>,
    /// The repository's answer for a session folder's durability, when the
    /// root is a synced folder's; `None` for the plain folder, where nothing
    /// publishes and `local` is the truth.
    pub probe: Option<DurabilityProbe>,
    /// The session folders a recording in progress has reserved, as absolute
    /// paths. A reserved folder is neither rewritten, pruned nor reconciled:
    /// the recorder sends a segment's row before the manifest lists it, so a
    /// rebuild that read the manifest now would prune the newest segment, and
    /// its `closed_ts` exists nowhere else. Its row counts as found.
    pub skip: HashSet<PathBuf>,
    /// Every root the archive follows right now — this one may be among them.
    /// A session whose row names one of these, and whose folder is still
    /// there, is not re-homed by this rebuild (see [`rebuild_from_disk`]).
    pub followed_roots: Vec<KnownRoot>,
}

impl RebuildRequest {
    /// A request over one root with no probe, nothing reserved and no other
    /// root known — the plain-folder shape, and the shape every Story 42.1
    /// caller had.
    pub fn new(root: impl Into<PathBuf>, root_kind: &str, profile_id: Option<&str>) -> Self {
        Self {
            root: root.into(),
            root_kind: root_kind.to_owned(),
            profile_id: profile_id.map(str::to_owned),
            probe: None,
            skip: HashSet::new(),
            followed_roots: Vec::new(),
        }
    }

    /// The same request, with the root's repository answering for durability.
    pub fn with_probe(mut self, probe: DurabilityProbe) -> Self {
        self.probe = Some(probe);
        self
    }

    /// The same request, leaving these session folders alone.
    pub fn skipping(mut self, folders: impl IntoIterator<Item = PathBuf>) -> Self {
        self.skip.extend(folders);
        self
    }

    /// The same request, knowing where these roots are.
    pub fn beside(mut self, roots: impl IntoIterator<Item = KnownRoot>) -> Self {
        self.followed_roots.extend(roots);
        self
    }
}

/// What one [`rebuild_from_disk`] pass did to its root's rows.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RebuildOutcome {
    /// Sessions whose row was written from a manifest found under the root.
    pub written: usize,
    /// Rows under the root's `(root_kind, profile_id)` that no folder under it
    /// claims any more, and were therefore removed with their segments and
    /// search entries. Always zero when the root could not be read whole.
    pub removed: usize,
    /// The session ids of every session found under the root — the reserved
    /// folders' rows first, then the walk's in the order it found them — the
    /// set the removal above was taken against.
    pub found: Vec<String>,
}

/// Re-derive every row from the session folders under the request's root, and
/// reconcile the root's rows against what the walk found (Story 42.1; the
/// archive follows every recordings root).
///
/// **This is what makes the database a cache rather than a second truth.**
/// Deleting `archive.db` loses nothing the manifests do not carry, and this
/// puts it back. Every session folder under `root` (a folder holding a
/// `manifest.json`; the walk goes no deeper into one) is loaded and its row
/// writes the rows through the ordinary [`upsert_recording`] /
/// [`upsert_segment`] path — the same derivation ([`RecordingRow::from_manifest`])
/// the recorder used, so the result is identical rather than approximate.
///
/// **The walk is the recovery pass's walk**, and reuses its two caps
/// ([`RECOVERY_MAX_DEPTH`], [`RECOVERY_MAX_VISITS`]) rather than inventing a
/// third notion of "how far into a tree is far enough": the same tree, walked
/// for a different reason, is bounded the same way. Both skip symlinks and dot
/// entries. Entries are visited in name order, so a rebuild is reproducible.
///
/// **A session's segment ledger is reconciled, never cleared.** The manifest's
/// `segments` list is the truth for which segments exist (it was rebuilt from
/// disk), so a segment it no longer lists is deleted — but only that one.
/// Clearing the whole ledger first would be one statement shorter and would
/// destroy `closed_ts`, the one segment fact no manifest carries, before
/// [`upsert_segment`] ever got the chance to preserve it.
///
/// **A reserved folder is left alone.** `skip` holds the folders a recording
/// in progress has claimed. The recorder writes a segment's row the moment the
/// segment closes and the manifest lists it only later, so a rebuild that
/// loaded that manifest would prune the newest row as stale — and `closed_ts`
/// would be gone. A reserved folder is therefore not read at all, its row
/// (looked up by its root-relative path) counts as found so the reconcile
/// keeps it, and its id is claimed before the walk so a copy of it elsewhere
/// under the root cannot take the id from the live session.
///
/// **A session id is written at most once per run.** Two folders under one root
/// can carry the same `meta.session_id` — a session folder copied BESIDE its
/// original rather than moved — and `INSERT OR REPLACE` would otherwise let the
/// second silently overwrite the first's row and ledger. The walk's first
/// folder keeps it; the second is skipped with a warn naming both (root-relative
/// paths only, never absolute ones), and is not counted, so `written` never
/// claims more rows than the run produced. Ordering by name means the same
/// folder wins on every machine, but the rule is "one row per id, always the
/// first", not a judgement about which copy is real — that is the person's to
/// make, and the log line is what tells them to. Idempotent for the same reason
/// every other path is: rerun, the same folders win again, and the walk never
/// appends. `written` is the count of ids written, so a duplicate tree writes
/// fewer sessions than the table holds. A duplicate is a fact about the tree,
/// not a reason to lose a row.
///
/// **A session found under a different `(root_kind, profile_id)` than its row
/// names has moved between roots**, by hand or by a `mv`, and its row follows
/// it: the path, the root kind and the profile are all rewritten, and so is
/// its durability — written EXACTLY as `probe` answers for the folder (`local`
/// without an answer), never floored against the stored word. The floor
/// exists so a row cannot walk backwards while the session sits in one
/// repository; a session carried out of that repository and into another (or
/// into none) has not walked backwards, it has left, and what the old
/// repository committed says nothing about where the bytes are now. The one
/// exception: when the root the row names is among `followed_roots` and the
/// folder the row points at is STILL THERE, the session has been copied, not
/// moved, and the row stays with the first copy — "one session id under two
/// roots; keeping the first" — so a copied folder does not change roots on
/// every rebuild.
///
/// **A rename inside one root is not a move between roots.** The same
/// `(root_kind, profile_id)` with a different path is a folder relocated
/// within its own repository — a retitle, or a subfolder shuffle. The stored
/// word described the old path, so the repository's answer for the new one
/// replaces it when there is one; when the probe has no answer (or there is
/// no probe), the floor keeps the stronger stored word, because silence is
/// not a downgrade. Found where its row already says it is, a session keeps
/// the floor whatever the probe says, so an in-place rebuild — the only kind
/// Story 42.1 knew — still cannot lower anything.
///
/// **Durability under a profile root comes from `probe`**, the repository's own
/// answer, and under the plain folder (no probe) is `local`.
///
/// **After a walk that saw the whole root, the root's rows are reconciled.**
/// Every row under this root's `(root_kind, profile_id)` whose session no
/// folder under the root claims any more is removed with its segment rows and
/// its search entry ([`super::recordings_fts::unindex_recording`]) — the row was
/// a cache of a manifest that is gone, or of one that now lives under another
/// root, whose own rebuild has re-homed it. The scope is exactly one root's
/// rows: a pass over `tgdrive` never touches a row that says `neuradrive`, and
/// a pass over the plain folder never touches a profile's. Reconciliation runs
/// ONLY when the walk was complete — a root whose directory is absent (a drive
/// that is out), a directory or a manifest the walk could not read, a session
/// folder the walk could not name relative to the root, or a walk cut short by
/// the visit budget all leave the rows exactly as they were — AND when the
/// walk either wrote at least one session or the root held no rows before.
/// A root that is present, holds rows, and shows the walk nothing is far
/// more often a stale mountpoint or a subfolder that has not synced yet than
/// a folder someone emptied, and forgetting every row on that evidence would
/// be forgetting them because keeper looked in the wrong place. A row must
/// never be forgotten because the walk did not get to look.
///
/// **Each session commits once.** Its row and the whole reconcile of its ledger
/// go in one transaction, so no reader on another connection can catch a session
/// between the delete of a stale segment and the insert of its replacement, and
/// a fifty-session tree costs fifty commits instead of several hundred. The
/// removals commit once as a group, for the same reason.
///
/// Filesystem trouble is skipped and logged (and disarms the reconcile); a
/// SQLite failure propagates, because a rebuild that cannot write is not a
/// rebuild and its caller is an explicit maintenance action, never the
/// recorder. The failing session's transaction rolls back, so it is absent
/// rather than half-written.
pub fn rebuild_from_disk(
    conn: &Connection,
    request: &RebuildRequest,
) -> Result<RebuildOutcome, ArchiveError> {
    rebuild_from_disk_within(conn, request, RECOVERY_MAX_VISITS)
}

/// [`rebuild_from_disk`] with its visit budget as an argument, so the budget's
/// own behaviour can be proven against a tree of four sessions rather than one
/// of [`RECOVERY_MAX_VISITS`]. Every shipping caller goes through the public
/// entry point, which always passes the real budget.
pub fn rebuild_from_disk_within(
    conn: &Connection,
    request: &RebuildRequest,
    max_visits: usize,
) -> Result<RebuildOutcome, ArchiveError> {
    let root = request.root.as_path();
    let root_kind = request.root_kind.as_str();
    let profile_id = request.profile_id.as_deref();
    // How many rows this root had before anything was looked at: the reconcile
    // below refuses to run when a present root shows the walk nothing and
    // there was something to forget.
    let held_before = count_root_rows(conn, root_kind, profile_id)?;
    let mut written = 0usize;
    let mut visits = 0usize;
    // Whether the walk saw every session folder under the root. Cleared by
    // anything that could have hidden one — an absent or unreadable directory,
    // a manifest that would not load, a folder with no root-relative name, the
    // visit budget — because the reconcile below may only forget a session the
    // walk PROVED is gone.
    let mut complete = true;
    // Whether the root directory itself is not there — a drive that is out,
    // which is nothing to warn about.
    let mut absent = false;
    // Every session id this run has already written, mapped to the folder that
    // claimed it: what makes the first of two duplicate folders win, and what
    // lets the warn name both. Its keys are the `found` set the reconcile
    // keeps; the order they were found in is kept beside it for the outcome.
    let mut written_ids: HashMap<String, String> = HashMap::new();
    let mut found: Vec<String> = Vec::new();
    // The reserved folders' rows are found before the walk starts, so the walk
    // can neither prune them nor let a copy claim their ids.
    let reserved: Vec<String> = request
        .skip
        .iter()
        .filter_map(|folder| relative_session_path(root, folder))
        .collect();
    for (session_id, relative) in rows_at_paths(conn, root_kind, profile_id, &reserved)? {
        tracing::debug!(
            session_id = %session_id,
            "archive rebuild: a recording in progress holds this session; leaving its rows alone"
        );
        written_ids.insert(session_id.clone(), relative);
        found.push(session_id);
    }
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    'walk: while let Some((dir, depth)) = pending.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                complete = false;
                if depth == 0 {
                    absent = true;
                }
                continue;
            }
            Err(error) => {
                tracing::warn!(%error, "archive rebuild: unreadable directory (non-fatal)");
                complete = false;
                continue;
            }
        };
        // Collected and sorted before anything is examined: the walk order
        // decides which of two folders sharing a session id wins, so it has to
        // be the tree's own order and not the filesystem's.
        let mut entries: Vec<std::fs::DirEntry> = entries
            .filter_map(|entry| match entry {
                Ok(entry) => Some(entry),
                Err(error) => {
                    tracing::warn!(%error, "archive rebuild: skipping unreadable directory entry");
                    complete = false;
                    None
                }
            })
            .collect();
        entries.sort_by_key(std::fs::DirEntry::file_name);
        // Subdirectories go onto the LIFO worklist in reverse, so they come back
        // off it in name order and the whole walk is one reproducible
        // depth-first pass.
        let mut children = Vec::new();
        for entry in entries {
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(error) => {
                    tracing::warn!(%error, "archive rebuild: skipping entry with unreadable type");
                    complete = false;
                    continue;
                }
            };
            if file_type.is_symlink() || !file_type.is_dir() {
                continue;
            }
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            if visits == max_visits {
                tracing::warn!(
                    budget = max_visits,
                    written,
                    "archive rebuild: stopping at the visit budget; keeping the sessions found so far"
                );
                complete = false;
                break 'walk;
            }
            visits += 1;
            let folder = entry.path();
            if request.skip.contains(&folder) {
                // Not read, not rewritten, not descended into: whatever is in
                // there is being written right now.
                continue;
            }
            if folder.join("manifest.json").is_file() {
                match SessionManifest::load(&folder) {
                    Ok(manifest) => {
                        match write_rebuilt_session(
                            conn,
                            request,
                            &folder,
                            &manifest,
                            &mut written_ids,
                        )? {
                            SessionWrite::Written(session_id) => {
                                written += 1;
                                found.push(session_id);
                            }
                            SessionWrite::Kept => {}
                            SessionWrite::Unplaceable => complete = false,
                        }
                        continue;
                    }
                    Err(error) => {
                        tracing::warn!(
                            %error,
                            "archive rebuild: unreadable manifest; walking the directory instead"
                        );
                        complete = false;
                    }
                }
            }
            if depth + 1 < RECOVERY_MAX_DEPTH {
                children.push(folder);
            }
        }
        for folder in children.into_iter().rev() {
            pending.push((folder, depth + 1));
        }
    }
    let removed = if absent {
        tracing::debug!(
            root_kind,
            profile_id = profile_id.unwrap_or("-"),
            "archive rebuild: the root is not there, so its rows stay as they are"
        );
        0
    } else if !complete {
        tracing::info!(
            root_kind,
            profile_id = profile_id.unwrap_or("-"),
            written,
            "archive rebuild: the walk did not see the whole root, so no row is forgotten"
        );
        0
    } else if written == 0 && held_before > 0 {
        tracing::warn!(
            root_kind,
            profile_id = profile_id.unwrap_or("-"),
            held = held_before,
            "archive rebuild: the root is there but holds no session; refusing to forget its rows (a stale mountpoint, or a subfolder not yet synced?)"
        );
        0
    } else {
        remove_unfound_sessions(conn, root_kind, profile_id, &written_ids)?
    };
    Ok(RebuildOutcome {
        written,
        removed,
        found,
    })
}

/// What [`write_rebuilt_session`] did with one session folder.
enum SessionWrite {
    /// A row was written for this session id — the run counts it and the
    /// reconcile keeps it.
    Written(String),
    /// Nothing was written and nothing is wrong: another folder in this run,
    /// or a folder still standing under another root, already holds the id.
    Kept,
    /// The folder could not be named relative to the root, so nothing could
    /// be written for it and the walk did not see the whole root.
    Unplaceable,
}

/// Write one rebuilt session and reconcile its whole segment ledger.
///
/// [`SessionWrite::Kept`], never an error, when another folder in this same
/// run already claimed the session id (the first one keeps it — see
/// [`rebuild_from_disk`] on duplicates), or when the row on file names a root
/// among the request's `followed_roots` where the session's folder still
/// stands (a copy, not a move — the first root keeps it).
/// [`SessionWrite::Unplaceable`] when the folder cannot be expressed relative
/// to the root, which would force an absolute path into the row (the one
/// thing no column may hold) — the walk is then incomplete, so the row that
/// was not rewritten is not forgotten either.
///
/// The row's durability is `probe`'s answer for the folder — `local` without a
/// probe or without an answer — and whether the floor applies to it is decided
/// HERE, from the row already on file, read once: the same
/// `(root_kind, profile_id)` keeps the floor (unless the folder was relocated
/// within the root and the probe DID answer, in which case the answer stands),
/// any other root is a move and takes the answer exactly (see
/// [`rebuild_from_disk`]).
///
/// The row, its search index entry and the ledger reconcile share one
/// transaction, so a concurrent reader sees the session either wholly rebuilt or
/// wholly untouched — and a rebuild that dies part way through cannot leave a
/// row describing one thing and an index entry describing another (Story 42.2).
fn write_rebuilt_session(
    conn: &Connection,
    request: &RebuildRequest,
    folder: &Path,
    manifest: &SessionManifest,
    written_ids: &mut HashMap<String, String>,
) -> Result<SessionWrite, ArchiveError> {
    let Some(relative) = relative_session_path(&request.root, folder) else {
        tracing::warn!(
            "archive rebuild: a session folder has no root-relative name (outside the root, or not UTF-8); the walk is incomplete"
        );
        return Ok(SessionWrite::Unplaceable);
    };
    let mut row = RecordingRow::from_manifest(
        manifest,
        relative.clone(),
        &request.root_kind,
        request.profile_id.as_deref(),
    );
    if let Some(kept) = written_ids.get(&row.session_id) {
        // Both paths are root-relative, so this names positions inside the tree
        // the caller already chose and no location on the user's disk.
        tracing::warn!(
            session_id = %row.session_id,
            kept = %kept,
            skipped = %relative,
            "archive rebuild: two folders carry one session id; keeping the first"
        );
        return Ok(SessionWrite::Kept);
    }
    // The repository's word for this folder, when there is one. `None` — no
    // probe, or a probe that could not answer — leaves the row at `local`, the
    // word `from_manifest` starts every row at.
    let answer = request.probe.as_ref().and_then(|probe| probe.ask(folder));
    if let Some(state) = answer {
        row.durability = durability_label(state).to_owned();
    }
    let outcome = in_transaction(conn, "rebuilt recording session", || {
        let durability = match stored_row(conn, &row.session_id)? {
            None => row.durability.clone(),
            Some(stored)
                if stored.root_kind != row.root_kind || stored.profile_id != row.profile_id =>
            {
                let standing = request
                    .followed_roots
                    .iter()
                    .find(|known| known.is(&stored.root_kind, stored.profile_id.as_deref()))
                    .is_some_and(|known| {
                        known
                            .folder(&stored.relative_path)
                            .join("manifest.json")
                            .is_file()
                    });
                if standing {
                    tracing::warn!(
                        session_id = %row.session_id,
                        kept_root_kind = %stored.root_kind,
                        kept_profile_id = stored.profile_id.as_deref().unwrap_or("-"),
                        kept = %stored.relative_path,
                        skipped_root_kind = %row.root_kind,
                        skipped_profile_id = row.profile_id.as_deref().unwrap_or("-"),
                        skipped = %relative,
                        "archive rebuild: one session id under two roots; keeping the first"
                    );
                    return Ok(SessionWrite::Kept);
                }
                tracing::info!(
                    session_id = %row.session_id,
                    from_root_kind = %stored.root_kind,
                    from_profile_id = stored.profile_id.as_deref().unwrap_or("-"),
                    to_root_kind = %row.root_kind,
                    to_profile_id = row.profile_id.as_deref().unwrap_or("-"),
                    durability = %row.durability,
                    "archive rebuild: a session moved between roots; re-homing its row"
                );
                row.durability.clone()
            }
            Some(stored) if stored.relative_path != relative && answer.is_some() => {
                // Relocated within its own root, and the repository has spoken
                // for the new path: its word replaces the old path's.
                row.durability.clone()
            }
            Some(stored) => floored_durability(Some(&stored.durability), &row.durability),
        };
        // Reindexes the session too, inside THIS transaction: `write_recording_as`
        // owns that pairing and is reentrant, so a rebuilt session — row, index
        // entry and ledger — still commits exactly once (see `in_transaction`).
        write_recording_as(conn, &row, &durability)?;
        // Drop only what the ledger has stopped listing — see the note on
        // [`rebuild_from_disk`] about why this is not a wholesale clear.
        for (index, track) in stale_segment_keys(conn, &row.session_id, manifest)? {
            conn.execute(
                "DELETE FROM recording_segments \
                 WHERE session_id = ?1 AND \"index\" = ?2 AND track = ?3",
                rusqlite::params![row.session_id, index, track],
            )
            .map_err(|e| {
                ArchiveError::Sqlite(format!("could not clear a recording segment row: {e}"))
            })?;
        }
        for entry in &manifest.segments {
            upsert_segment(
                conn,
                &RecordingSegmentRow::from_entry(&row.session_id, &relative, entry),
            )?;
        }
        Ok(SessionWrite::Written(row.session_id.clone()))
    })?;
    if matches!(outcome, SessionWrite::Written(_)) {
        written_ids.insert(row.session_id, relative);
    }
    Ok(outcome)
}

/// What a session's row on file says about where it lives and how safe it is.
struct StoredRow {
    root_kind: String,
    profile_id: Option<String>,
    relative_path: String,
    durability: String,
}

/// The session's row on file — its place and its durability word, in one
/// read — or `None` when it has no row.
fn stored_row(conn: &Connection, session_id: &str) -> Result<Option<StoredRow>, ArchiveError> {
    conn.query_row(
        "SELECT root_kind, profile_id, relative_path, durability FROM recordings \
         WHERE session_id = ?1",
        rusqlite::params![session_id],
        |r| {
            Ok(StoredRow {
                root_kind: r.get(0)?,
                profile_id: r.get(1)?,
                relative_path: r.get(2)?,
                durability: r.get(3)?,
            })
        },
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(ArchiveError::Sqlite(format!(
            "could not read a stored recording row: {other}"
        ))),
    })
}

/// How many rows a root's `(root_kind, profile_id)` holds.
///
/// `profile_id IS ?2` rather than `=`: the plain folder's rows hold `NULL`, and
/// `NULL = NULL` is not true in SQL.
fn count_root_rows(
    conn: &Connection,
    root_kind: &str,
    profile_id: Option<&str>,
) -> Result<usize, ArchiveError> {
    conn.query_row(
        "SELECT COUNT(*) FROM recordings WHERE root_kind = ?1 AND profile_id IS ?2",
        rusqlite::params![root_kind, profile_id],
        |r| r.get::<_, i64>(0),
    )
    .map(|n| usize::try_from(n).unwrap_or(0))
    .map_err(|e| ArchiveError::Sqlite(format!("could not count a root's sessions: {e}")))
}

/// The `(session_id, relative_path)` of every row under `(root_kind,
/// profile_id)` whose path is one of `paths` — the rows of the reserved
/// folders, which the walk does not read.
fn rows_at_paths(
    conn: &Connection,
    root_kind: &str,
    profile_id: Option<&str>,
    paths: &[String],
) -> Result<Vec<(String, String)>, ArchiveError> {
    let mut out = Vec::new();
    for path in paths {
        let row = conn
            .query_row(
                "SELECT session_id, relative_path FROM recordings \
                 WHERE root_kind = ?1 AND profile_id IS ?2 AND relative_path = ?3",
                rusqlite::params![root_kind, profile_id, path],
                |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)),
            )
            .map(Some)
            .or_else(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => Ok(None),
                other => Err(ArchiveError::Sqlite(format!(
                    "could not read a reserved session's row: {other}"
                ))),
            })?;
        out.extend(row);
    }
    Ok(out)
}

/// Forget every row under `(root_kind, profile_id)` — with its segment rows
/// and its search entry — because keeper no longer knows that root at all: a
/// synced folder that was removed. The reconcile half of [`rebuild_from_disk`]
/// against an empty found set, and the only way a root's rows go without a
/// walk; a paused folder's rows are not touched, because a pause is not a
/// removal. Returns how many sessions went.
pub fn forget_root(
    conn: &Connection,
    root_kind: &str,
    profile_id: Option<&str>,
) -> Result<usize, ArchiveError> {
    remove_unfound_sessions(conn, root_kind, profile_id, &HashMap::new())
}

/// Remove every row under `(root_kind, profile_id)` whose session id is not in
/// `found`, with its segment rows and its search entry, in one transaction;
/// returns how many sessions went. The reconcile half of
/// [`rebuild_from_disk`], and only ever called after a complete walk (or, from
/// [`forget_root`], for a root keeper no longer follows).
///
/// `profile_id IS ?2` rather than `=`: the plain folder's rows hold `NULL`, and
/// `NULL = NULL` is not true in SQL. The ids are read out in full before the
/// first delete, the shape [`move_session`] uses for the same reason.
///
/// Logged with ids only, never content: a removed row may have carried a title
/// or a note, and those were the manifest's — the manifest that is no longer
/// anywhere this root can see.
fn remove_unfound_sessions(
    conn: &Connection,
    root_kind: &str,
    profile_id: Option<&str>,
    found: &HashMap<String, String>,
) -> Result<usize, ArchiveError> {
    let stored: Vec<String> = {
        let mut stmt = conn
            .prepare(
                "SELECT session_id FROM recordings \
                 WHERE root_kind = ?1 AND profile_id IS ?2",
            )
            .map_err(|e| ArchiveError::Sqlite(format!("could not read a root's sessions: {e}")))?;
        let rows = stmt
            .query_map(rusqlite::params![root_kind, profile_id], |r| {
                r.get::<_, String>(0)
            })
            .map_err(|e| ArchiveError::Sqlite(format!("could not read a root's sessions: {e}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row.map_err(|e| {
                ArchiveError::Sqlite(format!("could not read a root's session id: {e}"))
            })?);
        }
        out
    };
    let orphaned: Vec<String> = stored
        .into_iter()
        .filter(|session_id| !found.contains_key(session_id))
        .collect();
    if orphaned.is_empty() {
        return Ok(0);
    }
    in_transaction(conn, "recording reconcile", || {
        for session_id in &orphaned {
            delete_session(conn, session_id)?;
            tracing::info!(
                session_id = %session_id,
                root_kind,
                profile_id = profile_id.unwrap_or("-"),
                "archive rebuild: no folder under the root carries this session any more; forgetting its row"
            );
        }
        Ok(())
    })?;
    Ok(orphaned.len())
}

/// Delete one session's row, its segment rows and its search entry. Runs
/// inside the caller's transaction; the three deletes are one unit of work.
fn delete_session(conn: &Connection, session_id: &str) -> Result<(), ArchiveError> {
    conn.execute(
        "DELETE FROM recording_segments WHERE session_id = ?1",
        rusqlite::params![session_id],
    )
    .map_err(|e| ArchiveError::Sqlite(format!("could not delete recording segment rows: {e}")))?;
    super::recordings_fts::unindex_recording(conn, session_id)?;
    conn.execute(
        "DELETE FROM recordings WHERE session_id = ?1",
        rusqlite::params![session_id],
    )
    .map_err(|e| ArchiveError::Sqlite(format!("could not delete a recording row: {e}")))?;
    Ok(())
}

/// The `(index, track)` keys stored for a session that its manifest's ledger no
/// longer lists: the rows a rebuild must delete, and only those.
fn stale_segment_keys(
    conn: &Connection,
    session_id: &str,
    manifest: &SessionManifest,
) -> Result<Vec<(u32, String)>, ArchiveError> {
    let mut stmt = conn
        .prepare("SELECT \"index\", track FROM recording_segments WHERE session_id = ?1")
        .map_err(|e| ArchiveError::Sqlite(format!("could not read stored segment keys: {e}")))?;
    let rows = stmt
        .query_map(rusqlite::params![session_id], |r| {
            Ok((r.get::<_, u32>(0)?, r.get::<_, String>(1)?))
        })
        .map_err(|e| ArchiveError::Sqlite(format!("could not read stored segment keys: {e}")))?;
    let mut stale = Vec::new();
    for key in rows {
        let (index, track) = key.map_err(|e| {
            ArchiveError::Sqlite(format!("could not read a stored segment key: {e}"))
        })?;
        if !manifest
            .segments
            .iter()
            .any(|entry| entry.index == index && entry.track == track)
        {
            stale.push((index, track));
        }
    }
    Ok(stale)
}

/// Milliseconds since the Unix epoch for an RFC 3339 stamp, or `None` when the
/// stamp is not one.
///
/// The manifest records `startedAt`/`endedAt` as RFC 3339 with the offset the
/// machine was in (Story 21.5), because a session folder is portable text. The
/// archive needs an *instant* — 42.2 filters a date range on it and 42.3 orders
/// by it — and lexicographic ordering of stamps carrying different offsets is
/// subtly wrong, so the column is an integer and this is the conversion.
/// `keeper-core` takes no date dependency, so the arithmetic is here; the
/// parse is positional because RFC 3339's date-time is fixed-width by
/// specification, exactly as [`crate::notes::templates`]'s stamp reader is.
///
/// An offset (`Z` or `±HH:MM`/`±HHMM`) is REQUIRED: without one the stamp names
/// no instant, and guessing UTC would silently move a recording by hours. Such a
/// stamp yields `None`, and the column stores the missing value as missing.
/// Fractional seconds of any length are accepted and truncated — never rounded —
/// to milliseconds. The day is checked against the real length of its month
/// (leap years included) rather than a flat `1..=31`, because
/// [`days_from_civil`] is a closed-form identity that would otherwise roll an
/// impossible date such as `2026-02-30` forward to March 2 and store an instant
/// the recording never happened at. Nothing a caller can pass makes this
/// function panic: a stamp it does not recognise — including one whose offset
/// tail is not ASCII — is `None`, so the offset is read byte-wise instead of by
/// slicing a `&str` at positions that may not be char boundaries.
pub fn epoch_ms_from_rfc3339(stamp: &str) -> Option<i64> {
    let bytes = stamp.as_bytes();
    if bytes.len() < 19 {
        return None;
    }
    if bytes[4] != b'-' || bytes[7] != b'-' || bytes[13] != b':' || bytes[16] != b':' {
        return None;
    }
    if !matches!(bytes[10], b'T' | b't' | b' ') {
        return None;
    }
    let field = |from: usize, to: usize| -> Option<i64> {
        let slice = stamp.get(from..to)?;
        if !slice.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        slice.parse::<i64>().ok()
    };
    let (year, month, day) = (field(0, 4)?, field(5, 7)?, field(8, 10)?);
    let (hour, minute, second) = (field(11, 13)?, field(14, 16)?, field(17, 19)?);
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    // A leap second (`:60`) is a real RFC 3339 value; it lands on the next
    // second rather than being refused.
    if hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut rest = stamp.get(19..)?;
    let mut millis = 0i64;
    if let Some(fraction) = rest.strip_prefix('.') {
        let digits = fraction
            .as_bytes()
            .iter()
            .take_while(|b| b.is_ascii_digit())
            .count();
        if digits == 0 {
            return None;
        }
        // Millisecond precision is what the column stores; anything finer is
        // truncated rather than rounded, so the value never moves forward past
        // an instant that did happen. The loop is bounded by the *digit run*,
        // not by three bytes of whatever follows the dot: `…00.5Z` has one
        // digit, and reading a second byte would fold the `Z` (or, worse, the
        // `+` of an offset) into the milliseconds.
        for (place, digit) in fraction.bytes().take(digits.min(3)).enumerate() {
            millis += i64::from(digit - b'0') * 10i64.pow(2 - place as u32);
        }
        rest = fraction.get(digits..)?;
    }
    let offset_ms = match rest.as_bytes() {
        [b'Z' | b'z'] => 0,
        [sign @ (b'+' | b'-'), tail @ ..] => {
            // Byte patterns, not `&str` slicing: `tail.len()` is a *byte*
            // length, so a multi-byte tail (`+€1` is four bytes) would make
            // `&tail[0..2]` panic on a char boundary in a function documented to
            // answer `None` for anything that is not a stamp.
            let [h_tens, h_ones, m_tens, m_ones] = match tail {
                [h_tens, h_ones, b':', m_tens, m_ones] | [h_tens, h_ones, m_tens, m_ones] => {
                    [h_tens, h_ones, m_tens, m_ones]
                }
                _ => return None,
            };
            let two_digits = |tens: u8, ones: u8| -> Option<i64> {
                if !tens.is_ascii_digit() || !ones.is_ascii_digit() {
                    return None;
                }
                Some(i64::from(tens - b'0') * 10 + i64::from(ones - b'0'))
            };
            let hours = two_digits(*h_tens, *h_ones)?;
            let minutes = two_digits(*m_tens, *m_ones)?;
            if hours > 23 || minutes > 59 {
                return None;
            }
            let magnitude = (hours * 60 + minutes) * 60_000;
            if *sign == b'-' {
                -magnitude
            } else {
                magnitude
            }
        }
        // No offset names no instant, and this module refuses to invent one.
        _ => return None,
    };
    let days = days_from_civil(year, month, day);
    Some(days * 86_400_000 + (hour * 3_600 + minute * 60 + second) * 1_000 + millis - offset_ms)
}

/// Days from 1970-01-01 for a proleptic Gregorian date (Howard Hinnant's
/// `days_from_civil`, the standard branch-free formulation).
///
/// `crate::notes::query` carries the same eight lines for its `date:` predicate.
/// They are deliberately not shared: this is a closed-form identity with a
/// round-trip test on both sides, so it cannot drift, and an `archive` → `notes`
/// dependency edge for calendar arithmetic would couple two subsystems that
/// otherwise know nothing about each other — a worse thing to maintain than the
/// eight lines.
fn days_from_civil(year: i64, month: i64, day: i64) -> i64 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let mp = if month > 2 { month - 3 } else { month + 9 };
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

/// The number of days in `month` of `year`, proleptic Gregorian.
///
/// The stamp parser needs this because a flat `1..=31` day check lets
/// `2026-02-30` and `2023-02-29` through, and [`days_from_civil`] is a
/// closed-form identity with no notion of an invalid date: it rolls such a day
/// forward (`2026-02-30` becomes March 2) rather than refusing it. A silently
/// shifted instant in the column 42.2 range-filters and 42.3 orders by is worse
/// than a missing one, so an impossible day is refused at the parse.
///
/// `crate::notes::query` carries these same five arms for the identical reason —
/// it refuses `date:2026-02-30` rather than resolving it — and they are not
/// shared for the reason given on [`days_from_civil`] above: the Gregorian leap
/// rule is fixed for all time and tested on both sides, so an `archive` →
/// `notes` dependency edge would cost more than the duplication.
fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        // The Gregorian leap rule in full: every fourth year is long, except
        // centuries, except every fourth century — 1900 and 2100 are short,
        // 2000 is not.
        2 if year % 4 == 0 && (year % 100 != 0 || year % 400 == 0) => 29,
        2 => 28,
        // The month is range-checked before this is ever called; a month that
        // does not exist has no days, which refuses the date either way.
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::path::PathBuf;

    use crate::archive::db::open_archive_db;
    use crate::recording::{CaptureTarget, SessionDevices, SessionMeta, SessionMetaField};

    /// A scratch directory no other test can land in — the `db.rs` fixture
    /// verbatim, including its process-wide counter (two threads inside one
    /// clock tick would otherwise share a database file).
    fn temp_dir() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "keeper-archive-recordings-test-{}-{}-{n}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        dir
    }

    /// An in-memory archive carrying the recording schema and its search index —
    /// enough for every write test, and it never touches the filesystem.
    ///
    /// Both, in the order [`super::db::open_archive_db`] ensures them: since
    /// Story 42.2 a row write also maintains that row's index entry, inside the
    /// same transaction, so a connection without the index is a connection no
    /// session can be written to.
    fn memory_db() -> Connection {
        let conn = Connection::open_in_memory().expect("open in-memory archive");
        ensure_recordings_schema(&conn).expect("ensure recordings schema");
        crate::archive::recordings_fts::ensure_recordings_fts(&conn)
            .expect("ensure the recordings search index");
        conn
    }

    /// A minimal start row: what the recorder knows the instant a session
    /// begins, and nothing it does not.
    fn start_row(session_id: &str) -> RecordingRow {
        RecordingRow {
            session_id: session_id.to_owned(),
            device_id: Some("01DEVICE".to_owned()),
            relative_path: "2026/session".to_owned(),
            root_kind: "folder".to_owned(),
            profile_id: None,
            started_ts: Some(1_754_600_000_000),
            ended_ts: None,
            title: None,
            participants_json: None,
            note: None,
            tags_json: None,
            custom_json: None,
            codec: Some("h264".to_owned()),
            width: None,
            height: None,
            fps: Some(30),
            durability: durability_label(RecordingDurabilityState::Local).to_owned(),
            manifest_version: 1,
        }
    }

    /// Read every `recordings` and `recording_segments` row back as one stable
    /// string: column name, SQLite value and its type, ordered. Comparing two of
    /// these compares the databases themselves — a field added later is in the
    /// dump the day it exists, so no test has to be remembered and extended.
    fn dump(conn: &Connection) -> String {
        let mut out = String::new();
        for sql in [
            "SELECT * FROM recordings ORDER BY session_id",
            "SELECT * FROM recording_segments ORDER BY session_id, \"index\", track",
        ] {
            let mut stmt = conn.prepare(sql).expect("prepare dump");
            let names: Vec<String> = stmt.column_names().into_iter().map(str::to_owned).collect();
            let rows = stmt
                .query_map([], |r| {
                    let mut line = String::new();
                    for (i, name) in names.iter().enumerate() {
                        let value = r.get_ref(i)?;
                        line.push_str(&format!("{name}={value:?} "));
                    }
                    Ok(line)
                })
                .expect("query dump");
            for row in rows {
                out.push_str(&row.expect("read dump row"));
                out.push('\n');
            }
        }
        out
    }

    fn count(conn: &Connection, table: &str) -> i64 {
        conn.query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |r| r.get(0))
            .expect("count rows")
    }

    fn durability_of(conn: &Connection, session_id: &str) -> String {
        conn.query_row(
            "SELECT durability FROM recordings WHERE session_id = ?1",
            rusqlite::params![session_id],
            |r| r.get(0),
        )
        .expect("read durability")
    }

    /// [`rebuild_from_disk`] without a probe — the plain-folder shape, and the
    /// shape every Story 42.1 test was written against — reduced to the count
    /// of sessions written.
    fn rebuild(conn: &Connection, root: &Path, root_kind: &str, profile_id: Option<&str>) -> usize {
        rebuild_from_disk(conn, &request(root, root_kind, profile_id))
            .expect("rebuild")
            .written
    }

    /// A request over one root with nothing else: no probe, nothing reserved,
    /// no other root known.
    fn request(root: &Path, root_kind: &str, profile_id: Option<&str>) -> RebuildRequest {
        RebuildRequest::new(root, root_kind, profile_id)
    }

    /// One root as another root's rebuild knows it.
    fn known(root: &Path, root_kind: &str, profile_id: Option<&str>) -> KnownRoot {
        KnownRoot {
            root: root.to_path_buf(),
            root_kind: root_kind.to_owned(),
            profile_id: profile_id.map(str::to_owned),
        }
    }

    /// Copy a session folder — files and subfolders — the way a person's `cp
    /// -R` would, so two folders carry one manifest.
    fn copy_dir(from: &Path, to: &Path) {
        std::fs::create_dir_all(to).expect("create the copy's folder");
        for entry in std::fs::read_dir(from).expect("read the folder to copy") {
            let entry = entry.expect("read an entry");
            let target = to.join(entry.file_name());
            if entry.file_type().expect("entry type").is_dir() {
                copy_dir(&entry.path(), &target);
            } else {
                std::fs::copy(entry.path(), target).expect("copy a file");
            }
        }
    }

    /// A probe answering one fixed state for every folder it is asked about.
    fn probe(state: RecordingDurabilityState) -> DurabilityProbe {
        DurabilityProbe(Box::new(move |_folder: &Path| Some(state)))
    }

    /// Every `(root_kind, profile_id, relative_path, durability)` a session's
    /// row holds, or `None` when it has none.
    fn place_of(
        conn: &Connection,
        session_id: &str,
    ) -> Option<(String, Option<String>, String, String)> {
        conn.query_row(
            "SELECT root_kind, profile_id, relative_path, durability FROM recordings \
             WHERE session_id = ?1",
            rusqlite::params![session_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .ok()
    }

    /// How many search entries the index holds — `recordings_fts_docs` and
    /// `recordings_fts` must always agree.
    fn fts_entries(conn: &Connection) -> (i64, i64) {
        (
            count(conn, "recordings_fts_docs"),
            count(conn, "recordings_fts"),
        )
    }

    #[test]
    fn start_row_is_completed_by_finalize_and_a_duplicate_finalize_leaves_one_row() {
        let conn = memory_db();
        upsert_recording(&conn, &start_row("01DEVICE-01SESSION")).expect("start");
        assert_eq!(count(&conn, "recordings"), 1);
        let ended: Option<i64> = conn
            .query_row("SELECT ended_ts FROM recordings", [], |r| r.get(0))
            .expect("read ended_ts");
        assert_eq!(ended, None, "a live session has not ended");

        let mut finalize = start_row("01DEVICE-01SESSION");
        finalize.ended_ts = Some(1_754_600_900_000);
        finalize.title = Some("Pricing call".to_owned());
        finalize.tags_json = Some(r#"["client/acme"]"#.to_owned());
        upsert_recording(&conn, &finalize).expect("finalize");
        // The finalize path runs twice (the matrix's duplicate-finalize row).
        upsert_recording(&conn, &finalize).expect("finalize again");

        assert_eq!(count(&conn, "recordings"), 1, "one session, one row");
        let (title, ended, tags): (Option<String>, Option<i64>, Option<String>) = conn
            .query_row(
                "SELECT title, ended_ts, tags_json FROM recordings",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .expect("read completed row");
        assert_eq!(title.as_deref(), Some("Pricing call"));
        assert_eq!(ended, Some(1_754_600_900_000));
        assert_eq!(tags.as_deref(), Some(r#"["client/acme"]"#));
    }

    #[test]
    fn durability_advances_through_the_states_and_never_walks_back() {
        let conn = memory_db();
        upsert_recording(&conn, &start_row("01DEVICE-01SESSION")).expect("start");
        assert_eq!(durability_of(&conn, "01DEVICE-01SESSION"), "local");

        for state in [
            RecordingDurabilityState::Committed,
            RecordingDurabilityState::Pushed,
            RecordingDurabilityState::Verified,
        ] {
            set_durability(&conn, "01DEVICE-01SESSION", durability_label(state))
                .expect("advance durability");
            assert_eq!(
                durability_of(&conn, "01DEVICE-01SESSION"),
                durability_label(state)
            );
        }
        // Every weaker state is refused, from every direction.
        for state in [
            RecordingDurabilityState::Pushed,
            RecordingDurabilityState::Committed,
            RecordingDurabilityState::Local,
        ] {
            set_durability(&conn, "01DEVICE-01SESSION", durability_label(state))
                .expect("weaker durability is a no-op, not an error");
            assert_eq!(
                durability_of(&conn, "01DEVICE-01SESSION"),
                "verified",
                "durability is a floor: {} must not lower it",
                durability_label(state)
            );
        }
        // A session with no row is not an error — the poll can outrun the start.
        set_durability(&conn, "01DEVICE-99UNKNOWN", "pushed").expect("unknown session");
        assert_eq!(count(&conn, "recordings"), 1);
    }

    #[test]
    fn a_finalize_row_carrying_local_cannot_pull_a_pushed_row_back_down() {
        // The shell's finalize path builds its row from the manifest, which
        // knows nothing about durability, so it says `local`. If the row write
        // took that literally, every session that published mid-recording would
        // report itself unpublished the moment it ended.
        let conn = memory_db();
        upsert_recording(&conn, &start_row("01DEVICE-01SESSION")).expect("start");
        set_durability(&conn, "01DEVICE-01SESSION", "pushed").expect("push");

        let mut finalize = start_row("01DEVICE-01SESSION");
        finalize.ended_ts = Some(1_754_600_900_000);
        assert_eq!(finalize.durability, "local");
        upsert_recording(&conn, &finalize).expect("finalize");

        assert_eq!(durability_of(&conn, "01DEVICE-01SESSION"), "pushed");
        let ended: Option<i64> = conn
            .query_row("SELECT ended_ts FROM recordings", [], |r| r.get(0))
            .expect("read ended_ts");
        assert_eq!(ended, Some(1_754_600_900_000), "the rest of the row landed");
    }

    #[test]
    fn segment_rows_are_one_per_session_index_and_track_and_replace_on_conflict() {
        let conn = memory_db();
        let segment = |index: u32, track: &str, bytes: u64| RecordingSegmentRow {
            session_id: "01DEVICE-01SESSION".to_owned(),
            index,
            track: track.to_owned(),
            relative_path: format!("2026/session/{track}-{index:04}.mov"),
            bytes,
            pts_start: Some(0.0),
            pts_end: Some(4.0),
            closed_ts: Some(1_754_600_100_000),
        };
        // The camera track shares the screen track's index, which is exactly why
        // `track` is part of the key.
        upsert_segment(&conn, &segment(0, "screen", 100)).expect("screen 0");
        upsert_segment(&conn, &segment(0, "camera", 200)).expect("camera 0");
        upsert_segment(&conn, &segment(1, "screen", 300)).expect("screen 1");
        assert_eq!(count(&conn, "recording_segments"), 3);

        // A re-report of the same segment (a `.partial` finalised at recovery
        // grew it) replaces the reading rather than adding a row.
        upsert_segment(&conn, &segment(0, "screen", 4_096)).expect("screen 0 again");
        assert_eq!(count(&conn, "recording_segments"), 3);
        let bytes: i64 = conn
            .query_row(
                "SELECT bytes FROM recording_segments WHERE \"index\" = 0 AND track = 'screen'",
                [],
                |r| r.get(0),
            )
            .expect("read bytes");
        assert_eq!(bytes, 4_096);

        let paths: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT relative_path FROM recording_segments")
                .expect("prepare");
            let rows = stmt
                .query_map([], |r| r.get::<_, String>(0))
                .expect("query")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect");
            rows
        };
        for path in paths {
            assert!(
                !path.starts_with('/') && path.starts_with("2026/"),
                "segment paths are relative to the root: {path}"
            );
        }
    }

    #[test]
    fn three_successive_opens_settle_the_schema_once() {
        let dir = temp_dir();
        let mut shapes = Vec::new();
        for _ in 0..3 {
            let conn = open_archive_db(&dir).expect("open archive.db");
            let mut stmt = conn
                .prepare(
                    "SELECT type, name, COALESCE(sql, '') FROM sqlite_master \
                     WHERE name LIKE 'recording%' OR name LIKE 'idx_recording%' \
                     ORDER BY type, name",
                )
                .expect("prepare schema read");
            let shape: Vec<String> = stmt
                .query_map([], |r| {
                    Ok(format!(
                        "{}:{}:{}",
                        r.get::<_, String>(0)?,
                        r.get::<_, String>(1)?,
                        r.get::<_, String>(2)?
                    ))
                })
                .expect("query schema")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect schema");
            assert!(
                shape.iter().any(|s| s.contains("recordings"))
                    && shape.iter().any(|s| s.contains("recording_segments")),
                "both tables exist on every open: {shape:?}"
            );
            drop(stmt);
            drop(conn);
            shapes.push(shape);
        }
        assert_eq!(shapes[0], shapes[1], "the second open changes nothing");
        assert_eq!(shapes[1], shapes[2], "the third open changes nothing");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_additive_migration_adds_every_nullable_column_to_an_older_table() {
        // A database an earlier build created with only the columns it knew: the
        // shape `ALTER TABLE … ADD COLUMN` exists for. The migration must reach
        // it without a bespoke path.
        let conn = Connection::open_in_memory().expect("open in-memory");
        conn.execute(
            "CREATE TABLE recordings(\
                session_id TEXT PRIMARY KEY, relative_path TEXT NOT NULL, \
                root_kind TEXT NOT NULL, durability TEXT NOT NULL, \
                manifest_version INTEGER NOT NULL)",
            [],
        )
        .expect("create the older table");
        conn.execute(
            "CREATE TABLE recording_segments(\
                session_id TEXT NOT NULL, \"index\" INTEGER NOT NULL, track TEXT NOT NULL, \
                relative_path TEXT NOT NULL, bytes INTEGER NOT NULL, \
                PRIMARY KEY(session_id, \"index\", track))",
            [],
        )
        .expect("create the older segments table");
        conn.execute(
            "INSERT INTO recordings(session_id, relative_path, root_kind, durability, \
             manifest_version) VALUES ('01D-01S', '2026/s', 'folder', 'committed', 1)",
            [],
        )
        .expect("seed a pre-migration row");

        ensure_recordings_schema(&conn).expect("migrate");
        ensure_recordings_schema(&conn).expect("migrate again: a no-op");

        for (table, columns) in [
            ("recordings", RECORDINGS_ADDITIVE_COLUMNS),
            ("recording_segments", RECORDING_SEGMENTS_ADDITIVE_COLUMNS),
        ] {
            let mut stmt = conn
                .prepare(&format!("PRAGMA table_info({table})"))
                .expect("prepare table_info");
            let present: Vec<String> = stmt
                .query_map([], |r| r.get::<_, String>(1))
                .expect("query table_info")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect table_info");
            for (name, _) in columns {
                assert!(present.iter().any(|c| c == name), "{table}.{name} added");
            }
        }
        // The existing row survived untouched — an additive migration rewrites
        // nothing, least of all a durability the floor is meant to protect.
        assert_eq!(durability_of(&conn, "01D-01S"), "committed");
    }

    #[test]
    fn durability_labels_match_the_wire_spelling_of_the_durability_state() {
        for state in [
            RecordingDurabilityState::Local,
            RecordingDurabilityState::Committed,
            RecordingDurabilityState::Pushed,
            RecordingDurabilityState::Verified,
        ] {
            let label = durability_label(state);
            let wire = serde_json::to_string(&state).expect("serialize state");
            assert_eq!(
                format!("\"{label}\""),
                wire,
                "the column word is epic 41's own wire word"
            );
            assert_eq!(parse_durability(label), Some(state), "and it reads back");
        }
        assert_eq!(parse_durability("archived"), None);
        // An unknown stored word loses to anything known; an unknown incoming
        // word never overwrites a known stored one.
        assert_eq!(floored_durability(Some("nonsense"), "local"), "local");
        assert_eq!(floored_durability(Some("pushed"), "nonsense"), "pushed");
        assert_eq!(floored_durability(None, "committed"), "committed");
    }

    #[test]
    fn rfc3339_stamps_parse_to_epoch_milliseconds() {
        assert_eq!(epoch_ms_from_rfc3339("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(
            epoch_ms_from_rfc3339("2026-08-08T12:00:00Z"),
            // 20 673 days from the epoch to 2026-08-08, plus twelve hours.
            Some(1_786_190_400_000)
        );
        // Before the epoch the value is negative; the last second of 1969 is one
        // second short of zero. Nothing in the arithmetic clamps at the epoch.
        assert_eq!(epoch_ms_from_rfc3339("1969-12-31T23:59:59Z"), Some(-1_000));
        assert_eq!(
            epoch_ms_from_rfc3339("1900-01-01T00:00:00Z"),
            Some(-2_208_988_800_000)
        );
        // A leap second is a real RFC 3339 value and lands on the next second
        // rather than being refused: 11:59:60 is noon.
        assert_eq!(
            epoch_ms_from_rfc3339("2026-08-08T11:59:60Z"),
            Some(1_786_190_400_000)
        );
        // The date/time separator is `T`, `t` or a space, and `Z` may be either
        // case; all three name the same instant as the canonical spelling.
        for spelling in [
            "2026-08-08t12:00:00Z",
            "2026-08-08 12:00:00Z",
            "2026-08-08T12:00:00z",
        ] {
            assert_eq!(
                epoch_ms_from_rfc3339(spelling),
                Some(1_786_190_400_000),
                "{spelling}"
            );
        }
        // A stamp with no offset names no instant, and is refused rather than
        // guessed at.
        assert_eq!(epoch_ms_from_rfc3339("2026-08-08T12:00:00"), None);
        for bad in [
            "",
            "2026-08-08",
            "2026-13-08T12:00:00Z",
            "2026-00-08T12:00:00Z",
            "2026-08-08T24:00:00Z",
            "2026-08-08T12:60:00Z",
            "2026-08-08T12:00:61Z",
            "2026-08-08T12:00:00.Z",
            "2026-08-08T12:00:00+1:00",
            "2026-08-08T12:00:00+24:00",
            "2026-08-08T12:00:00+01:60",
            "2026-08-08T12:00:00+01:0",
            "2026-08-08T12:00:00Zulu",
            "2026/08/08T12:00:00Z",
            "not a timestamp at all",
        ] {
            assert_eq!(epoch_ms_from_rfc3339(bad), None, "{bad}");
        }
    }

    /// Offsets carry a sign, and the sign has a direction: a stamp written at
    /// `+02:00` happened two hours *earlier* in UTC than its wall clock reads,
    /// so the offset is subtracted from the epoch value. Every case here asserts
    /// the absolute instant rather than equality with another parse, because two
    /// parses that are wrong in the same direction agree with each other.
    #[test]
    fn rfc3339_offsets_are_subtracted_from_the_epoch_value_in_both_spellings() {
        const NOON: i64 = 1_786_190_400_000;
        for spelling in [
            "2026-08-08T12:00:00Z",
            "2026-08-08T14:00:00+02:00",
            "2026-08-08T14:00:00+0200",
            "2026-08-08T07:00:00-05:00",
            "2026-08-08T07:00:00-0500",
            "2026-08-08T23:45:00+11:45",
            "2026-08-08T11:30:00-00:30",
            "2026-08-08T12:00:00+00:00",
        ] {
            assert_eq!(epoch_ms_from_rfc3339(spelling), Some(NOON), "{spelling}");
        }
    }

    /// Fractional seconds are truncated to milliseconds, and the digit run is
    /// what gets read: a one-digit fraction is tenths, not tenths plus whatever
    /// byte happens to follow the digit. Reading a fixed three bytes past the dot
    /// folded the trailing `Z` into the value (`.5Z` became 920 ms) and made a
    /// one-digit fraction in front of a `+HH:MM` offset underflow on `b'+'`.
    #[test]
    fn rfc3339_fractional_seconds_read_only_the_digit_run_and_truncate() {
        const NOON: i64 = 1_786_190_400_000;
        for (stamp, expected) in [
            ("2026-08-08T12:00:00.5Z", NOON + 500),
            ("2026-08-08T12:00:00.05Z", NOON + 50),
            ("2026-08-08T12:00:00.12Z", NOON + 120),
            ("2026-08-08T12:00:00.281Z", NOON + 281),
            ("2026-08-08T12:00:00.2817Z", NOON + 281),
            ("2026-08-08T12:00:00.999999999Z", NOON + 999),
            ("2026-08-08T12:00:00.0009Z", NOON),
            ("2026-08-08T12:00:00.5z", NOON + 500),
            // The panic case: one fractional digit and then a signed offset.
            ("2026-08-08T13:00:00.5+01:00", NOON + 500),
            ("2026-08-08T13:00:00.5+0100", NOON + 500),
            ("2026-08-08T11:00:00.25-01:00", NOON + 250),
            ("2026-08-08T13:00:00.2817+01:00", NOON + 281),
        ] {
            assert_eq!(epoch_ms_from_rfc3339(stamp), Some(expected), "{stamp}");
        }
        // A dot with no digits after it is not a fraction.
        assert_eq!(epoch_ms_from_rfc3339("2026-08-08T12:00:00.+01:00"), None);
    }

    /// The offset is parsed byte-wise, so a tail that is not ASCII answers `None`
    /// like any other unrecognised stamp. Slicing the tail as a `&str` by byte
    /// offsets panicked instead — `+€1` is four bytes, so it took the `±HHMM`
    /// branch and cut the euro sign in half — and this function is documented to
    /// refuse, never to panic, whatever `&str` a manifest hands it.
    #[test]
    fn rfc3339_offset_tails_that_are_not_ascii_are_refused_rather_than_panicking() {
        for bad in [
            "2026-08-08T12:00:00+\u{20ac}1",
            "2026-08-08T12:00:00-\u{20ac}1",
            "2026-08-08T12:00:00.5+\u{20ac}1",
            "2026-08-08T12:00:00+0\u{20ac}",
            "2026-08-08T12:00:00+\u{20ac}:1",
            "2026-08-08T12:00:00+\u{fc}12",
            "2026-08-08T12:00:00+01:\u{fc}",
            "2026-08-08T12:00:00\u{20ac}",
        ] {
            assert_eq!(epoch_ms_from_rfc3339(bad), None, "{bad}");
        }
    }

    /// A day is only valid inside its own month. A flat `1..=31` check let
    /// `2026-02-30` through, and [`days_from_civil`] has no notion of an invalid
    /// date — it rolled that day forward to March 2 and stored an instant the
    /// recording never happened at, in the column 42.2 range-filters and 42.3
    /// orders by.
    #[test]
    fn rfc3339_days_are_validated_against_the_length_of_their_own_month() {
        for bad in [
            "2026-02-30T12:00:00Z",
            "2026-02-29T12:00:00Z",
            "2023-02-29T12:00:00Z",
            "2026-04-31T12:00:00Z",
            "2026-06-31T12:00:00Z",
            "2026-09-31T12:00:00Z",
            "2026-11-31T12:00:00Z",
            "2026-08-32T12:00:00Z",
            "2026-08-00T12:00:00Z",
            // Centuries are short unless they divide by 400.
            "1900-02-29T12:00:00Z",
            "2100-02-29T12:00:00Z",
        ] {
            assert_eq!(epoch_ms_from_rfc3339(bad), None, "{bad}");
        }
        // The days that do exist still parse, to their own instants.
        assert_eq!(
            epoch_ms_from_rfc3339("2024-02-29T00:00:00Z"),
            Some(1_709_164_800_000)
        );
        assert_eq!(
            epoch_ms_from_rfc3339("2000-02-29T00:00:00Z"),
            Some(951_782_400_000)
        );
        assert_eq!(
            epoch_ms_from_rfc3339("2026-01-31T00:00:00Z"),
            Some(1_769_817_600_000)
        );
        assert_eq!(
            epoch_ms_from_rfc3339("2026-04-30T00:00:00Z"),
            Some(1_777_507_200_000)
        );
        assert_eq!(days_in_month(2024, 2), 29);
        assert_eq!(days_in_month(2023, 2), 28);
        assert_eq!(days_in_month(1900, 2), 28);
        assert_eq!(days_in_month(2000, 2), 29);
        assert_eq!(days_in_month(2026, 4), 30);
        assert_eq!(days_in_month(2026, 12), 31);
    }

    /// Absolute day numbers, not differences: a constant offset error in
    /// [`days_from_civil`] cancels out of any subtraction of two of its results,
    /// so every case here names the day number it must produce. The years are
    /// chosen to evaluate the terms that exist for them — 1900 for `yoe / 100`,
    /// 2000 for the 400-year era, and years at or below zero for the `y < 0`
    /// branch that no test reached before.
    #[test]
    fn days_from_civil_matches_known_day_numbers_on_both_sides_of_the_epoch() {
        assert_eq!(days_from_civil(1970, 1, 1), 0);
        assert_eq!(days_from_civil(1969, 12, 31), -1);
        assert_eq!(days_from_civil(1900, 1, 1), -25_567);
        assert_eq!(days_from_civil(2000, 3, 1), 11_017);
        assert_eq!(days_from_civil(2024, 2, 29), 19_782);
        assert_eq!(days_from_civil(2026, 8, 8), 20_673);
        // The era term's own anchor, and the two dates either side of it that
        // take the `y - 399` path.
        assert_eq!(days_from_civil(0, 3, 1), -719_468);
        assert_eq!(days_from_civil(0, 1, 1), -719_528);
        assert_eq!(days_from_civil(-1, 3, 1), -719_834);
    }

    #[test]
    fn relative_paths_are_slash_joined_and_reject_anything_outside_the_root() {
        let root = Path::new("/tmp/keeper-root");
        assert_eq!(
            relative_session_path(root, &root.join("2026").join("august").join("call")),
            Some("2026/august/call".to_owned())
        );
        assert_eq!(relative_session_path(root, Path::new("/tmp/other")), None);
        assert_eq!(relative_session_path(root, root), None);
    }

    /// Put one session on disk the way a finished recording leaves one.
    #[allow(clippy::too_many_arguments)]
    fn seed_session(
        root: &Path,
        relative: &str,
        session_id: Option<&str>,
        title: Option<&str>,
        started_at: Option<&str>,
        segments: &[(u32, &str, u64)],
    ) -> PathBuf {
        let folder = relative
            .split('/')
            .fold(root.to_path_buf(), |acc, part| acc.join(part));
        let meta = session_id.map(|id| SessionMeta {
            session_id: Some(id.to_owned()),
            title: title.map(str::to_owned),
            participants: Some("Ada, Grace".to_owned()),
            note: Some("agreed the API shape".to_owned()),
            tags: Some(vec!["client/acme".to_owned(), "renewal".to_owned()]),
            custom: Some(vec![SessionMetaField {
                name: "room".to_owned(),
                value: "3B".to_owned(),
            }]),
        });
        let mut manifest = SessionManifest::create_with_meta(
            folder.clone(),
            CaptureTarget::display(None),
            SessionDevices {
                system_audio: true,
                microphone: false,
                camera: false,
            },
            meta,
            started_at.map(str::to_owned),
        )
        .expect("create session folder");
        for (index, track, bytes) in segments {
            manifest.segments.push(SegmentEntry {
                index: *index,
                file: format!("{track}-{index:04}.mov"),
                bytes: *bytes,
                track: (*track).to_owned(),
                pts_start: Some(f64::from(*index) * 4.0),
                pts_end: Some(f64::from(*index) * 4.0 + 4.0),
            });
        }
        manifest.set_ended_at("2026-08-08T12:15:00+02:00".to_owned());
        manifest.write().expect("write manifest");
        folder
    }

    #[test]
    fn an_older_manifest_without_meta_or_stamps_writes_a_row_with_defaults() {
        let dir = temp_dir();
        let root = dir.join("recordings");
        let folder = seed_session(&root, "2026/legacy-session", None, None, None, &[]);
        // A pre-21.5 manifest has no `endedAt` either.
        let mut manifest = SessionManifest::load(&folder).expect("load");
        manifest.ended_at = None;
        manifest.write().expect("rewrite without an end stamp");

        let conn = memory_db();
        let written = rebuild(&conn, &root, "folder", None);
        assert_eq!(written, 1, "an older manifest is still a session");

        let (session_id, device_id, started, title, participants): (
            String,
            Option<String>,
            Option<i64>,
            Option<String>,
            Option<String>,
        ) = conn
            .query_row(
                "SELECT session_id, device_id, started_ts, title, participants_json \
                 FROM recordings",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )
            .expect("read the defaulted row");
        assert_eq!(session_id, "legacy:2026/legacy-session");
        assert_eq!(device_id, None, "no meta means no device half");
        assert_eq!(started, None, "a missing stamp stores as missing, not 1970");
        assert_eq!(title, None);
        assert_eq!(participants, None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_retitle_move_changes_the_path_and_leaves_the_session_id_alone() {
        let dir = temp_dir();
        let root = dir.join("recordings");
        let before = seed_session(
            &root,
            "2026/1432",
            Some("01DEVICE-01SESSION"),
            None,
            Some("2026-08-08T12:00:00+02:00"),
            &[(0, "screen", 100)],
        );
        let conn = memory_db();
        rebuild(&conn, &root, "folder", None);

        // Story 40.4 moves the folder; the manifest (and its session id) rides
        // along byte-identical.
        let after = root.join("2026").join("1432 Standup");
        std::fs::rename(&before, &after).expect("move the session folder");
        rebuild(&conn, &root, "folder", None);

        assert_eq!(count(&conn, "recordings"), 1, "the same session, one row");
        let (session_id, path): (String, String) = conn
            .query_row(
                "SELECT session_id, relative_path FROM recordings",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("read the moved row");
        assert_eq!(session_id, "01DEVICE-01SESSION");
        assert_eq!(path, "2026/1432 Standup");
        let segment: String = conn
            .query_row("SELECT relative_path FROM recording_segments", [], |r| {
                r.get(0)
            })
            .expect("read the moved segment");
        assert_eq!(segment, "2026/1432 Standup/screen-0000.mov");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rebuild_forgets_a_session_whose_folder_is_gone_with_its_segments_and_search_entry() {
        let dir = temp_dir();
        let root = dir.join("recordings");
        let folder = seed_session(
            &root,
            "2026/deleted",
            Some("01DEVICE-01SESSION"),
            Some("Pricing review"),
            Some("2026-08-08T12:00:00+02:00"),
            &[(0, "screen", 100), (1, "screen", 200)],
        );
        seed_session(
            &root,
            "2026/kept",
            Some("01DEVICE-02KEPT"),
            Some("Standup"),
            Some("2026-08-08T13:00:00+02:00"),
            &[(0, "screen", 100)],
        );
        let conn = memory_db();
        let first = rebuild_from_disk(&conn, &request(&root, "folder", None)).expect("rebuild");
        assert_eq!((first.written, first.removed), (2, 0));
        assert_eq!(count(&conn, "recordings"), 2);
        assert_eq!(count(&conn, "recording_segments"), 3);
        assert_eq!(fts_entries(&conn), (2, 2));

        std::fs::remove_dir_all(&folder).expect("delete the session folder");
        let again =
            rebuild_from_disk(&conn, &request(&root, "folder", None)).expect("rebuild again");
        assert_eq!(again.written, 1, "the surviving session is rewritten");
        assert_eq!(again.removed, 1, "and the deleted one is counted out");
        assert_eq!(again.found, vec!["01DEVICE-02KEPT".to_owned()]);
        assert_eq!(
            session_ids(&conn),
            vec!["01DEVICE-02KEPT".to_owned()],
            "a folder found nowhere is a row found nowhere"
        );
        assert_eq!(
            segment_paths(&conn),
            vec!["2026/kept/screen-0000.mov".to_owned()],
            "its segments go with it"
        );
        assert_eq!(fts_entries(&conn), (1, 1), "and so does its search entry");
        let hits = crate::archive::recordings_fts::search_recordings(
            &conn,
            &crate::archive::recordings_fts::RecordingFilter {
                query: "pricing".to_owned(),
                ..Default::default()
            },
        )
        .expect("search");
        assert!(
            hits.is_empty(),
            "a forgotten session is not a hit: {hits:?}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The reconcile's one scope rule: a pass over one root never touches
    /// another root's rows, whichever kind of root either is.
    #[test]
    fn a_rebuild_forgets_only_the_rows_of_its_own_root() {
        let dir = temp_dir();
        let root = dir.join("recordings");
        seed_session(
            &root,
            "2026/here",
            Some("01DEVICE-01HERE"),
            None,
            Some("2026-08-08T12:00:00+02:00"),
            &[],
        );
        let conn = memory_db();
        // Rows that belong to other roots: the plain folder's, another
        // profile's, and one with no profile under the profile kind.
        for (id, root_kind, profile_id) in [
            ("01DEVICE-02FOLDER", "folder", None),
            ("01DEVICE-03OTHER", "profile", Some("02OTHER")),
            ("01DEVICE-04BARE", "profile", None),
        ] {
            let mut row = start_row(id);
            row.root_kind = root_kind.to_owned();
            row.profile_id = profile_id.map(str::to_owned);
            upsert_recording(&conn, &row).expect("seed a foreign row");
        }
        let mut stale = start_row("01DEVICE-05STALE");
        stale.root_kind = "profile".to_owned();
        stale.profile_id = Some("01PROFILE".to_owned());
        upsert_recording(&conn, &stale).expect("seed this root's orphan");

        let outcome = rebuild_from_disk(&conn, &request(&root, "profile", Some("01PROFILE")))
            .expect("rebuild");
        assert_eq!((outcome.written, outcome.removed), (1, 1));
        assert_eq!(
            session_ids(&conn),
            vec![
                "01DEVICE-01HERE".to_owned(),
                "01DEVICE-02FOLDER".to_owned(),
                "01DEVICE-03OTHER".to_owned(),
                "01DEVICE-04BARE".to_owned(),
            ],
            "only the row under (profile, 01PROFILE) that the walk did not find is gone"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The "drive out" row: a root whose directory is not there reconciles
    /// nothing — its rows are exactly as they were.
    #[test]
    fn a_rebuild_of_an_absent_root_leaves_every_row_untouched() {
        let dir = temp_dir();
        let root = dir.join("recordings");
        seed_session(
            &root,
            "2026/session",
            Some("01DEVICE-01SESSION"),
            None,
            Some("2026-08-08T12:00:00+02:00"),
            &[(0, "screen", 100)],
        );
        let conn = memory_db();
        assert_eq!(rebuild(&conn, &root, "profile", Some("01STICK")), 1);
        let before = dump(&conn);

        std::fs::remove_dir_all(&root).expect("unplug the drive");
        let outcome =
            rebuild_from_disk(&conn, &request(&root, "profile", Some("01STICK"))).expect("rebuild");
        assert_eq!(
            outcome,
            RebuildOutcome::default(),
            "nothing seen, nothing done"
        );
        assert_eq!(
            before,
            dump(&conn),
            "an unreadable root is not an empty one"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A root that is there, holds rows, and shows the walk no session at all
    /// — a stale mountpoint, a subfolder that has not synced yet — is not
    /// believed: nothing is forgotten on that evidence. A root that never held
    /// a row reconciles nothing either way, and a root that shows the walk at
    /// least one session is believed about the rest.
    #[test]
    fn a_rebuild_of_an_empty_but_present_root_refuses_to_forget_its_rows() {
        let dir = temp_dir();
        let root = dir.join("recordings");
        let folder = seed_session(
            &root,
            "2026/session",
            Some("01DEVICE-01SESSION"),
            None,
            Some("2026-08-08T12:00:00+02:00"),
            &[(0, "screen", 100)],
        );
        let conn = memory_db();
        assert_eq!(rebuild(&conn, &root, "profile", Some("01PROFILE")), 1);
        let before = dump(&conn);

        // The session folder goes; the root and its year folder stay, empty.
        std::fs::remove_dir_all(&folder).expect("empty the root");
        let outcome = rebuild_from_disk(&conn, &request(&root, "profile", Some("01PROFILE")))
            .expect("rebuild an empty root");
        assert_eq!((outcome.written, outcome.removed), (0, 0));
        assert_eq!(
            before,
            dump(&conn),
            "an empty root with rows is a root keeper may be looking at wrongly"
        );

        // A root that never held a row: the same walk, and nothing to refuse.
        let fresh = memory_db();
        let outcome = rebuild_from_disk(&fresh, &request(&root, "profile", Some("01PROFILE")))
            .expect("rebuild an empty root into an empty index");
        assert_eq!(outcome, RebuildOutcome::default());

        // One session back on disk beside the missing one: the walk saw a
        // session, so it is believed, and the missing one is forgotten.
        seed_session(
            &root,
            "2026/another",
            Some("01DEVICE-02ANOTHER"),
            None,
            Some("2026-08-08T13:00:00+02:00"),
            &[],
        );
        let outcome = rebuild_from_disk(&conn, &request(&root, "profile", Some("01PROFILE")))
            .expect("rebuild a root with one session");
        assert_eq!((outcome.written, outcome.removed), (1, 1));
        assert_eq!(session_ids(&conn), vec!["01DEVICE-02ANOTHER".to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A session folder the rebuild cannot name relative to its root is a
    /// session it could not vouch for: nothing is written for it, and the
    /// walk that met it is incomplete.
    #[test]
    fn a_session_folder_with_no_root_relative_name_makes_the_walk_incomplete() {
        let dir = temp_dir();
        let root = dir.join("recordings");
        let elsewhere = seed_session(
            &dir.join("elsewhere"),
            "session",
            Some("01DEVICE-01AWAY"),
            None,
            Some("2026-08-08T12:00:00+02:00"),
            &[],
        );
        let manifest = SessionManifest::load(&elsewhere).expect("load");
        let conn = memory_db();
        let mut written_ids = HashMap::new();
        let outcome = write_rebuilt_session(
            &conn,
            &request(&root, "folder", None),
            &elsewhere,
            &manifest,
            &mut written_ids,
        )
        .expect("a folder outside the root is not an error");
        assert!(matches!(outcome, SessionWrite::Unplaceable));
        assert_eq!(count(&conn, "recordings"), 0, "nothing was written for it");
        assert!(written_ids.is_empty(), "and it claimed no id");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The walk-level half of the test above, on the one filesystem family
    /// that will create a folder whose name is not UTF-8: the row of a session
    /// the walk could not name is not forgotten, because the walk was
    /// incomplete.
    #[cfg(target_os = "linux")]
    #[test]
    fn a_walk_that_meets_a_non_utf8_folder_name_forgets_nothing() {
        use std::os::unix::ffi::OsStrExt;
        let dir = temp_dir();
        let root = dir.join("recordings");
        let folder = seed_session(
            &root,
            "2026/session",
            Some("01DEVICE-01SESSION"),
            None,
            Some("2026-08-08T12:00:00+02:00"),
            &[],
        );
        let conn = memory_db();
        assert_eq!(rebuild(&conn, &root, "folder", None), 1);
        let renamed = root
            .join("2026")
            .join(std::ffi::OsStr::from_bytes(b"bad\xff"));
        std::fs::rename(&folder, &renamed).expect("a name that is not UTF-8");
        let outcome = rebuild_from_disk(&conn, &request(&root, "folder", None)).expect("rebuild");
        assert_eq!((outcome.written, outcome.removed), (0, 0));
        assert_eq!(
            count(&conn, "recordings"),
            1,
            "the row it could not name stays"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A manifest the walk cannot read is a session it cannot vouch for, so the
    /// pass writes what it can and forgets nothing.
    #[test]
    fn a_rebuild_that_meets_an_unreadable_manifest_forgets_nothing() {
        let dir = temp_dir();
        let root = dir.join("recordings");
        let broken = seed_session(
            &root,
            "2026/broken",
            Some("01DEVICE-01BROKEN"),
            None,
            Some("2026-08-08T12:00:00+02:00"),
            &[],
        );
        seed_session(
            &root,
            "2026/fine",
            Some("01DEVICE-02FINE"),
            None,
            Some("2026-08-08T12:00:00+02:00"),
            &[],
        );
        let conn = memory_db();
        assert_eq!(rebuild(&conn, &root, "folder", None), 2);
        std::fs::write(broken.join("manifest.json"), b"{ not json").expect("corrupt the manifest");

        let outcome = rebuild_from_disk(&conn, &request(&root, "folder", None)).expect("rebuild");
        assert_eq!((outcome.written, outcome.removed), (1, 0));
        assert_eq!(
            count(&conn, "recordings"),
            2,
            "the unreadable session keeps its row"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The recorder writes a segment's row the moment it closes and the
    /// manifest lists it only later. A rebuild that runs meanwhile must leave
    /// the reserved folder alone: the fresh row (and its `closed_ts`, which
    /// exists nowhere else) survives, the session still counts as found so the
    /// reconcile keeps it, and the rest of the root is rebuilt as usual.
    #[test]
    fn a_reserved_folders_fresh_segment_row_survives_a_rebuild() {
        let dir = temp_dir();
        let root = dir.join("recordings");
        let live = seed_session(
            &root,
            "2026/live",
            Some("01DEVICE-01LIVE"),
            Some("Still going"),
            Some("2026-08-08T12:00:00+02:00"),
            &[(0, "screen", 100)],
        );
        let gone = seed_session(
            &root,
            "2026/gone",
            Some("01DEVICE-02GONE"),
            None,
            Some("2026-08-08T13:00:00+02:00"),
            &[],
        );
        seed_session(
            &root,
            "2026/kept",
            Some("01DEVICE-03KEPT"),
            None,
            Some("2026-08-08T14:00:00+02:00"),
            &[],
        );
        let conn = memory_db();
        assert_eq!(rebuild(&conn, &root, "folder", None), 3);
        // Segment 1 has just closed: its row is here, the manifest has not
        // caught up, and its `closed_ts` is a fact only this row holds.
        upsert_segment(
            &conn,
            &RecordingSegmentRow {
                session_id: "01DEVICE-01LIVE".to_owned(),
                index: 1,
                track: "screen".to_owned(),
                relative_path: "2026/live/screen-0001.mov".to_owned(),
                bytes: 4096,
                pts_start: Some(4.0),
                pts_end: Some(8.0),
                closed_ts: Some(1_754_600_100_000),
            },
        )
        .expect("the fresh segment row");
        std::fs::remove_dir_all(&gone).expect("a session deleted meanwhile");

        let outcome = rebuild_from_disk(
            &conn,
            &request(&root, "folder", None).skipping([live.clone()]),
        )
        .expect("rebuild around a recording in progress");
        assert_eq!(
            outcome.written, 1,
            "only the session that is neither live nor gone is rewritten"
        );
        assert_eq!(outcome.removed, 1, "the deleted session is still forgotten");
        assert_eq!(
            outcome.found,
            vec!["01DEVICE-01LIVE".to_owned(), "01DEVICE-03KEPT".to_owned()],
            "the reserved folder's session is found without being read"
        );
        assert_eq!(
            segment_paths(&conn),
            vec![
                "2026/live/screen-0000.mov".to_owned(),
                "2026/live/screen-0001.mov".to_owned(),
            ],
            "the fresh row was not pruned"
        );
        let closed: Option<i64> = conn
            .query_row(
                "SELECT closed_ts FROM recording_segments WHERE session_id = '01DEVICE-01LIVE' \
                 AND \"index\" = 1",
                [],
                |r| r.get(0),
            )
            .expect("read closed_ts");
        assert_eq!(closed, Some(1_754_600_100_000));

        // A copy of the live session elsewhere under the root cannot take its
        // id: the reserved folder claimed it before the walk began.
        copy_dir(&live, &root.join("2026").join("live copy"));
        let outcome = rebuild_from_disk(
            &conn,
            &request(&root, "folder", None).skipping([live.clone()]),
        )
        .expect("rebuild with a copy of the live session");
        assert_eq!(outcome.written, 1, "the copy is kept out");
        assert_eq!(
            place_of(&conn, "01DEVICE-01LIVE").map(|place| place.2),
            Some("2026/live".to_owned()),
            "the row still names the live folder"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The "session moved between roots by hand" row, in both orders the two
    /// roots can be rebuilt in: one row, under the new root, with the
    /// durability the NEW root's repository reports — `local` when it has
    /// never committed the folder — however far the old row had climbed.
    #[test]
    fn a_session_moved_between_roots_is_re_homed_and_its_durability_re_derived() {
        for old_root_first in [true, false] {
            let dir = temp_dir();
            let old_root = dir.join("tgdrive");
            let new_root = dir.join("neuradrive").join("70-comms").join("meetings");
            let before = seed_session(
                &old_root,
                "2026/2026-09-08 10.01 gsd",
                Some("01DEVICE-01GSD"),
                Some("GSD"),
                Some("2026-09-08T10:01:00+02:00"),
                &[(0, "screen", 100), (1, "screen", 200)],
            );
            let conn = memory_db();
            let roots = [
                known(&old_root, "profile", Some("tgdrive")),
                known(&new_root, "profile", Some("neuradrive")),
            ];
            let old = || {
                request(&old_root, "profile", Some("tgdrive"))
                    .with_probe(probe(RecordingDurabilityState::Pushed))
                    .beside(roots.clone())
            };
            let new = || {
                request(&new_root, "profile", Some("neuradrive"))
                    .with_probe(probe(RecordingDurabilityState::Local))
                    .beside(roots.clone())
            };
            assert_eq!(
                rebuild_from_disk(&conn, &old())
                    .expect("index the old root")
                    .written,
                1
            );
            set_durability(&conn, "01DEVICE-01GSD", "verified").expect("the old root climbed");
            assert_eq!(
                place_of(&conn, "01DEVICE-01GSD"),
                Some((
                    "profile".to_owned(),
                    Some("tgdrive".to_owned()),
                    "2026/2026-09-08 10.01 gsd".to_owned(),
                    "verified".to_owned()
                ))
            );

            // Moved by hand: the folder, manifest and all, now sits under a
            // root that has never committed it.
            std::fs::create_dir_all(&new_root).expect("the new root");
            let after = new_root.join("2026-09-08 10.01 gsd");
            std::fs::rename(&before, &after).expect("move the session by hand");
            let (old_outcome, new_outcome) = if old_root_first {
                let o = rebuild_from_disk(&conn, &old()).expect("rebuild the old root");
                (
                    o,
                    rebuild_from_disk(&conn, &new()).expect("rebuild the new root"),
                )
            } else {
                let n = rebuild_from_disk(&conn, &new()).expect("rebuild the new root");
                (
                    rebuild_from_disk(&conn, &old()).expect("rebuild the old root"),
                    n,
                )
            };
            assert_eq!(new_outcome.written, 1, "order {old_root_first}");
            assert_eq!(old_outcome.written, 0, "order {old_root_first}");
            // The old root is present but now shows the walk nothing, so its
            // pass refuses to forget on that evidence; it is the new root's
            // pass that re-homes the row. Either order: one row.
            assert_eq!(old_outcome.removed, 0, "order {old_root_first}");
            assert_eq!(count(&conn, "recordings"), 1, "order {old_root_first}");
            assert_eq!(
                place_of(&conn, "01DEVICE-01GSD"),
                Some((
                    "profile".to_owned(),
                    Some("neuradrive".to_owned()),
                    "2026-09-08 10.01 gsd".to_owned(),
                    "local".to_owned()
                )),
                "order {old_root_first}: the row follows the folder and the old floor does not"
            );
            assert_eq!(
                segment_paths(&conn),
                vec![
                    "2026-09-08 10.01 gsd/screen-0000.mov".to_owned(),
                    "2026-09-08 10.01 gsd/screen-0001.mov".to_owned(),
                ],
                "order {old_root_first}: its segments moved with it"
            );
            assert_eq!(fts_entries(&conn), (1, 1), "order {old_root_first}");

            // And the floor is a floor again from here: a later advance climbs,
            // a later regression does not.
            set_durability(&conn, "01DEVICE-01GSD", "committed").expect("advance");
            assert_eq!(durability_of(&conn, "01DEVICE-01GSD"), "committed");
            set_durability(&conn, "01DEVICE-01GSD", "local").expect("a weaker word is a no-op");
            assert_eq!(durability_of(&conn, "01DEVICE-01GSD"), "committed");
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// Out of a synced folder and into the plain one: no repository, no probe,
    /// and the row reads `local` — exactly, whatever the old folder had
    /// pushed.
    #[test]
    fn a_session_moved_from_a_profile_to_the_plain_folder_reads_local() {
        let dir = temp_dir();
        let profile_root = dir.join("tgdrive").join("recordings");
        let folder_root = dir.join("Movies").join("keeper");
        let before = seed_session(
            &profile_root,
            "2026/call",
            Some("01DEVICE-01CALL"),
            None,
            Some("2026-08-08T12:00:00+02:00"),
            &[(0, "screen", 100)],
        );
        let conn = memory_db();
        rebuild_from_disk(
            &conn,
            &request(&profile_root, "profile", Some("tgdrive"))
                .with_probe(probe(RecordingDurabilityState::Verified)),
        )
        .expect("index the profile root");
        assert_eq!(durability_of(&conn, "01DEVICE-01CALL"), "verified");

        std::fs::create_dir_all(folder_root.join("2026")).expect("the plain folder");
        std::fs::rename(&before, folder_root.join("2026").join("call")).expect("move by hand");
        let outcome = rebuild_from_disk(
            &conn,
            &request(&folder_root, "folder", None).beside([
                known(&profile_root, "profile", Some("tgdrive")),
                known(&folder_root, "folder", None),
            ]),
        )
        .expect("rebuild the plain folder");
        assert_eq!(outcome.written, 1);
        assert_eq!(
            place_of(&conn, "01DEVICE-01CALL"),
            Some((
                "folder".to_owned(),
                None,
                "2026/call".to_owned(),
                "local".to_owned()
            )),
            "nothing publishes the plain folder, so nothing it holds is more than local"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A rename inside one root is not a move between roots. The repository's
    /// answer for the new path replaces the old path's word when there is one
    /// — `committed` over `verified`, because the old path's push says nothing
    /// about the new path — and when there is none, the floor keeps the
    /// stronger word: silence is not a downgrade.
    #[test]
    fn a_rename_within_a_root_takes_the_repositorys_answer_and_keeps_the_floor_without_one() {
        for (answer, expected) in [
            (Some(RecordingDurabilityState::Committed), "committed"),
            (None, "verified"),
        ] {
            let dir = temp_dir();
            let root = dir.join("recordings");
            let before = seed_session(
                &root,
                "2026/1432 Untitled",
                Some("01DEVICE-01SESSION"),
                None,
                Some("2026-08-08T12:00:00+02:00"),
                &[(0, "screen", 100)],
            );
            let conn = memory_db();
            assert_eq!(rebuild(&conn, &root, "profile", Some("01PROFILE")), 1);
            set_durability(&conn, "01DEVICE-01SESSION", "verified").expect("climb");

            std::fs::rename(&before, root.join("2026").join("1432 Standup")).expect("retitle");
            let silent_or_not = DurabilityProbe(Box::new(move |_folder: &Path| answer));
            let outcome = rebuild_from_disk(
                &conn,
                &request(&root, "profile", Some("01PROFILE")).with_probe(silent_or_not),
            )
            .expect("rebuild after the rename");
            assert_eq!((outcome.written, outcome.removed), (1, 0));
            assert_eq!(
                place_of(&conn, "01DEVICE-01SESSION"),
                Some((
                    "profile".to_owned(),
                    Some("01PROFILE".to_owned()),
                    "2026/1432 Standup".to_owned(),
                    expected.to_owned()
                )),
                "answer {answer:?}"
            );
            let _ = std::fs::remove_dir_all(&dir);
        }
    }

    /// A session folder COPIED into a second root, with the first copy still
    /// standing, is not a move: the row stays with the first root, on every
    /// rebuild of either, so it cannot flap between them. Once the first copy
    /// is gone, the second is the session, and the row follows it.
    #[test]
    fn a_session_id_under_two_roots_keeps_the_first_until_the_first_is_gone() {
        let dir = temp_dir();
        let first_root = dir.join("tgdrive");
        let second_root = dir.join("neuradrive");
        let original = seed_session(
            &first_root,
            "2026/talk",
            Some("01DEVICE-01TALK"),
            None,
            Some("2026-08-08T12:00:00+02:00"),
            &[(0, "screen", 100)],
        );
        let conn = memory_db();
        let roots = [
            known(&first_root, "profile", Some("tgdrive")),
            known(&second_root, "profile", Some("neuradrive")),
        ];
        let first = || {
            request(&first_root, "profile", Some("tgdrive"))
                .with_probe(probe(RecordingDurabilityState::Pushed))
                .beside(roots.clone())
        };
        let second = || {
            request(&second_root, "profile", Some("neuradrive"))
                .with_probe(probe(RecordingDurabilityState::Local))
                .beside(roots.clone())
        };
        assert_eq!(
            rebuild_from_disk(&conn, &first()).expect("index").written,
            1
        );
        let copy = second_root.join("2026").join("talk");
        copy_dir(&original, &copy);

        for round in 0..3 {
            let outcome = rebuild_from_disk(&conn, &second()).expect("rebuild the second root");
            assert_eq!((outcome.written, outcome.removed), (0, 0), "round {round}");
            let outcome = rebuild_from_disk(&conn, &first()).expect("rebuild the first root");
            assert_eq!((outcome.written, outcome.removed), (1, 0), "round {round}");
            assert_eq!(
                place_of(&conn, "01DEVICE-01TALK"),
                Some((
                    "profile".to_owned(),
                    Some("tgdrive".to_owned()),
                    "2026/talk".to_owned(),
                    "pushed".to_owned()
                )),
                "round {round}: the first root keeps it"
            );
        }
        assert_eq!(count(&conn, "recordings"), 1);

        // The original goes: the copy is the session now.
        std::fs::remove_dir_all(&original).expect("delete the original");
        let outcome = rebuild_from_disk(&conn, &second()).expect("rebuild the second root");
        assert_eq!(outcome.written, 1);
        assert_eq!(
            place_of(&conn, "01DEVICE-01TALK"),
            Some((
                "profile".to_owned(),
                Some("neuradrive".to_owned()),
                "2026/talk".to_owned(),
                "local".to_owned()
            )),
            "with the first copy gone the row follows the second"
        );
        assert_eq!(count(&conn, "recordings"), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The "durability re-derived" row: in place, the probe's answer is
    /// floored against the row like any other write — a repository that
    /// committed and pushed the folder lifts a `local` row to `pushed`, and a
    /// later advance still climbs from there.
    #[test]
    fn a_rebuild_in_place_takes_the_repositorys_word_and_keeps_the_floor() {
        let dir = temp_dir();
        let root = dir.join("recordings");
        seed_session(
            &root,
            "2026/session",
            Some("01DEVICE-01SESSION"),
            None,
            Some("2026-08-08T12:00:00+02:00"),
            &[],
        );
        let conn = memory_db();
        assert_eq!(rebuild(&conn, &root, "profile", Some("01PROFILE")), 1);
        assert_eq!(durability_of(&conn, "01DEVICE-01SESSION"), "local");

        rebuild_from_disk(
            &conn,
            &request(&root, "profile", Some("01PROFILE"))
                .with_probe(probe(RecordingDurabilityState::Pushed)),
        )
        .expect("rebuild with the repository's answer");
        assert_eq!(durability_of(&conn, "01DEVICE-01SESSION"), "pushed");

        // A probe that cannot answer says `local`, and in place the floor keeps
        // the stronger word the row already earned.
        let silent = DurabilityProbe(Box::new(|_folder: &Path| None));
        rebuild_from_disk(
            &conn,
            &request(&root, "profile", Some("01PROFILE")).with_probe(silent),
        )
        .expect("rebuild with a probe that cannot answer");
        assert_eq!(durability_of(&conn, "01DEVICE-01SESSION"), "pushed");

        set_durability(&conn, "01DEVICE-01SESSION", "verified").expect("a later advance");
        assert_eq!(durability_of(&conn, "01DEVICE-01SESSION"), "verified");
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A removed synced folder's rows go — row, segments and search entry —
    /// and nobody else's: another profile's and the plain folder's stay.
    #[test]
    fn forgetting_a_root_removes_exactly_its_rows() {
        let conn = memory_db();
        for (id, root_kind, profile_id) in [
            ("01DEVICE-01GONE", "profile", Some("gone")),
            ("01DEVICE-02GONE", "profile", Some("gone")),
            ("01DEVICE-03OTHER", "profile", Some("other")),
            ("01DEVICE-04FOLDER", "folder", None),
        ] {
            let mut row = start_row(id);
            row.root_kind = root_kind.to_owned();
            row.profile_id = profile_id.map(str::to_owned);
            row.title = Some(format!("Meeting {id}"));
            upsert_recording(&conn, &row).expect("seed");
            upsert_segment(
                &conn,
                &RecordingSegmentRow {
                    session_id: id.to_owned(),
                    index: 0,
                    track: "screen".to_owned(),
                    relative_path: "2026/session/screen-0000.mov".to_owned(),
                    bytes: 1,
                    pts_start: None,
                    pts_end: None,
                    closed_ts: None,
                },
            )
            .expect("seed a segment");
        }
        assert_eq!(fts_entries(&conn), (4, 4));

        assert_eq!(
            forget_root(&conn, "profile", Some("gone")).expect("forget"),
            2
        );
        assert_eq!(
            session_ids(&conn),
            vec![
                "01DEVICE-03OTHER".to_owned(),
                "01DEVICE-04FOLDER".to_owned()
            ]
        );
        assert_eq!(count(&conn, "recording_segments"), 2);
        assert_eq!(fts_entries(&conn), (2, 2));
        assert_eq!(
            forget_root(&conn, "profile", Some("gone")).expect("forget again"),
            0,
            "forgetting a root twice forgets nothing more"
        );
    }

    #[test]
    fn a_rebuild_skips_dot_dirs_and_never_descends_into_a_session() {
        let dir = temp_dir();
        let root = dir.join("recordings");
        seed_session(
            &root,
            "2026/real",
            Some("01DEVICE-01SESSION"),
            None,
            Some("2026-08-08T12:00:00+02:00"),
            &[],
        );
        // A manifest a user copied INSIDE a session must not become a second
        // row, and a dot directory is the OS's, not the user's recordings.
        seed_session(
            &root,
            "2026/real/copied-inside",
            Some("01DEVICE-02COPY"),
            None,
            Some("2026-08-08T12:00:00+02:00"),
            &[],
        );
        seed_session(
            &root,
            ".Trash/thrown-away",
            Some("01DEVICE-03TRASH"),
            None,
            Some("2026-08-08T12:00:00+02:00"),
            &[],
        );

        let conn = memory_db();
        let written = rebuild(&conn, &root, "folder", None);
        assert_eq!(written, 1);
        let ids: Vec<String> = {
            let mut stmt = conn
                .prepare("SELECT session_id FROM recordings")
                .expect("prepare");
            stmt.query_map([], |r| r.get::<_, String>(0))
                .expect("query")
                .collect::<Result<Vec<_>, _>>()
                .expect("collect")
        };
        assert_eq!(ids, vec!["01DEVICE-01SESSION".to_owned()]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// One seeded session, as the fixture knows it *before* anything is
    /// written — the independent side of the byte-identity comparison.
    struct Seeded {
        session_id: String,
        relative: String,
        segments: Vec<(u32, &'static str, u64)>,
    }

    /// The fifty-session corpus, nested the way the default `{yyyy}/` template
    /// nests them, with the metadata a real session carries.
    fn seed_fifty(root: &Path) -> Vec<Seeded> {
        let mut seeded = Vec::new();
        for n in 0..50u32 {
            let year = 2024 + n % 3;
            let relative = format!("{year}/{:02}/session-{n:03}", 1 + n % 12);
            let session_id = format!("01DEVICE{:02}-01SESSION{n:03}", n % 7);
            let segments: Vec<(u32, &'static str, u64)> = match n % 3 {
                0 => vec![(0, "screen", 1_000 + u64::from(n))],
                1 => vec![(0, "screen", 2_048), (1, "screen", 512)],
                _ => vec![(0, "screen", 4_096), (0, "camera", 128)],
            };
            seed_session(
                root,
                &relative,
                Some(&session_id),
                Some(&format!("Session {n}")),
                Some(&format!(
                    "{year}-0{}-0{}T09:{:02}:00+02:00",
                    1 + n % 9,
                    1 + n % 9,
                    n % 60
                )),
                &segments,
            );
            seeded.push(Seeded {
                session_id,
                relative,
                segments,
            });
        }
        seeded
    }

    /// When the live path saw one segment close — a fact the sidecar reports as
    /// it rolls the file and no `manifest.json` records anywhere.
    fn recorder_closed_ts(index: u32) -> i64 {
        1_754_640_000_000 + i64::from(index) * 4_000
    }

    /// Every `relative_path` in `recording_segments`, in key order.
    fn segment_paths(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare(
                "SELECT relative_path FROM recording_segments ORDER BY session_id, \"index\", track",
            )
            .expect("prepare segment paths");
        stmt.query_map([], |r| r.get::<_, String>(0))
            .expect("query segment paths")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect segment paths")
    }

    /// Every `session_id` in `recordings`, sorted.
    fn session_ids(conn: &Connection) -> Vec<String> {
        let mut stmt = conn
            .prepare("SELECT session_id FROM recordings ORDER BY session_id")
            .expect("prepare session ids");
        stmt.query_map([], |r| r.get::<_, String>(0))
            .expect("query session ids")
            .collect::<Result<Vec<_>, _>>()
            .expect("collect session ids")
    }

    #[test]
    fn rebuild_from_disk_reproduces_fifty_sessions_written_through_the_normal_path() {
        let dir = temp_dir();
        let root = dir.join("recordings");
        let seeded = seed_fifty(&root);

        // The normal path: the recorder wrote each row as it finished the
        // session. Built here from the FIXTURE's own inputs, not from the
        // rebuild's derivation, so the comparison below has something to prove
        // — and built the way the recorder ACTUALLY writes one, with `codec`,
        // `fps` and every `closed_ts` populated. Those are the shell's own
        // facts: no manifest carries them, which makes them exactly what a
        // careless rebuild erases.
        let normal = memory_db();
        for Seeded {
            session_id,
            relative,
            segments,
        } in &seeded
        {
            let manifest = SessionManifest::load(
                &relative
                    .split('/')
                    .fold(root.to_path_buf(), |acc, part| acc.join(part)),
            )
            .expect("load the seeded manifest");
            let (device_id, _) = session_id.split_once('-').expect("a two-part id");
            let title = manifest
                .meta
                .as_ref()
                .and_then(|m| m.title.clone())
                .expect("the fixture titles every session");
            let row = RecordingRow {
                session_id: session_id.clone(),
                device_id: Some(device_id.to_owned()),
                relative_path: relative.clone(),
                root_kind: "profile".to_owned(),
                profile_id: Some("01PROFILE".to_owned()),
                started_ts: epoch_ms_from_rfc3339(
                    manifest.started_at.as_deref().expect("a start stamp"),
                ),
                ended_ts: epoch_ms_from_rfc3339("2026-08-08T12:15:00+02:00"),
                title: Some(title),
                participants_json: Some("\"Ada, Grace\"".to_owned()),
                note: Some("agreed the API shape".to_owned()),
                tags_json: Some(r#"["client/acme","renewal"]"#.to_owned()),
                custom_json: Some(r#"[{"name":"room","value":"3B"}]"#.to_owned()),
                codec: Some("h264".to_owned()),
                width: None,
                height: None,
                fps: Some(30),
                durability: "local".to_owned(),
                manifest_version: 1,
            };
            upsert_recording(&normal, &row).expect("write the session row");
            for (index, track, bytes) in segments {
                upsert_segment(
                    &normal,
                    &RecordingSegmentRow {
                        session_id: session_id.clone(),
                        index: *index,
                        track: (*track).to_owned(),
                        relative_path: format!("{relative}/{track}-{index:04}.mov"),
                        bytes: *bytes,
                        pts_start: Some(f64::from(*index) * 4.0),
                        pts_end: Some(f64::from(*index) * 4.0 + 4.0),
                        closed_ts: Some(recorder_closed_ts(*index)),
                    },
                )
                .expect("write the segment row");
            }
        }

        // A rebuild over the rows the recorder wrote — a stale index, or an
        // explicit rescan. One assertion carries the whole AC in both
        // directions: every field the manifest carries re-derives to the byte
        // already stored, and the three it cannot carry are still there
        // afterwards. Any drift either way changes the dump.
        let before = dump(&normal);
        let rewritten = rebuild(&normal, &root, "profile", Some("01PROFILE"));
        assert_eq!(rewritten, 50);
        assert_eq!(
            before,
            dump(&normal),
            "a rebuild over an indexed tree reproduces every manifest field exactly and erases nothing else"
        );

        // And `archive.db` deleted outright: the same rows, short of precisely
        // the three columns no manifest can carry.
        let rebuilt = memory_db();
        let written = rebuild(&rebuilt, &root, "profile", Some("01PROFILE"));
        assert_eq!(written, 50);
        assert_eq!(count(&rebuilt, "recordings"), 50);
        assert_eq!(count(&normal, "recordings"), 50);
        normal
            .execute("UPDATE recordings SET codec = NULL, fps = NULL", [])
            .expect("forget what only the live session knew");
        normal
            .execute("UPDATE recording_segments SET closed_ts = NULL", [])
            .expect("forget the close stamps");
        assert_eq!(
            dump(&rebuilt),
            dump(&normal),
            "every rebuilt row is byte-identical to the one the recorder wrote, for every field a manifest carries"
        );

        // And running it again changes nothing: the ledger is reconciled, never
        // appended to.
        let before = dump(&rebuilt);
        rebuild(&rebuilt, &root, "profile", Some("01PROFILE"));
        assert_eq!(before, dump(&rebuilt), "a rebuild is idempotent");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_write_that_knows_no_codec_fps_or_close_stamp_keeps_the_stored_one() {
        let conn = memory_db();
        let mut live = start_row("01DEVICE-01SESSION");
        live.width = Some(1920);
        live.height = Some(1080);
        upsert_recording(&conn, &live).expect("the recorder's row");
        let segment = RecordingSegmentRow {
            session_id: "01DEVICE-01SESSION".to_owned(),
            index: 0,
            track: "screen".to_owned(),
            relative_path: "2026/session/screen-0000.mov".to_owned(),
            bytes: 100,
            pts_start: Some(0.0),
            pts_end: Some(4.0),
            closed_ts: Some(recorder_closed_ts(0)),
        };
        upsert_segment(&conn, &segment).expect("the recorder's segment row");

        // What a rebuild derives: the manifest carries none of those columns,
        // so it offers `None` for every one of them — while genuinely carrying
        // the title, which it must still be allowed to change.
        let mut derived = live.clone();
        derived.codec = None;
        derived.fps = None;
        derived.width = None;
        derived.height = None;
        derived.title = Some("Retitled".to_owned());
        upsert_recording(&conn, &derived).expect("the rebuild's row");
        let mut derived_segment = segment.clone();
        derived_segment.bytes = 140;
        derived_segment.closed_ts = None;
        upsert_segment(&conn, &derived_segment).expect("the rebuild's segment row");

        let (codec, fps, width, height, title) = conn
            .query_row(
                "SELECT codec, fps, width, height, title FROM recordings",
                [],
                |r| {
                    Ok((
                        r.get::<_, Option<String>>(0)?,
                        r.get::<_, Option<u32>>(1)?,
                        r.get::<_, Option<u32>>(2)?,
                        r.get::<_, Option<u32>>(3)?,
                        r.get::<_, Option<String>>(4)?,
                    ))
                },
            )
            .expect("read the rebuilt row");
        assert_eq!(
            codec.as_deref(),
            Some("h264"),
            "the codec exists nowhere on disk, so a rebuild must not be able to erase it"
        );
        assert_eq!(fps, Some(30));
        assert_eq!(width, Some(1920));
        assert_eq!(height, Some(1080));
        assert_eq!(
            title.as_deref(),
            Some("Retitled"),
            "everything the manifest DOES carry is still overwritten"
        );
        let (bytes, closed): (i64, Option<i64>) = conn
            .query_row("SELECT bytes, closed_ts FROM recording_segments", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .expect("read the rebuilt segment row");
        assert_eq!(bytes, 140, "the ledger's own facts are replaced");
        assert_eq!(
            closed,
            Some(recorder_closed_ts(0)),
            "the close stamp only the live path ever saw is not"
        );

        // A writer that DOES know the value always wins.
        let mut corrected = derived.clone();
        corrected.codec = Some("hevc".to_owned());
        corrected.fps = Some(60);
        upsert_recording(&conn, &corrected).expect("the finalize path knows better");
        let (codec, fps): (Option<String>, Option<u32>) = conn
            .query_row("SELECT codec, fps FROM recordings", [], |r| {
                Ok((r.get(0)?, r.get(1)?))
            })
            .expect("read the corrected row");
        assert_eq!(codec.as_deref(), Some("hevc"));
        assert_eq!(fps, Some(60));
        let mut restamped = derived_segment.clone();
        restamped.closed_ts = Some(recorder_closed_ts(9));
        upsert_segment(&conn, &restamped).expect("a live re-report of the same segment");
        let closed: Option<i64> = conn
            .query_row("SELECT closed_ts FROM recording_segments", [], |r| r.get(0))
            .expect("read the restamped row");
        assert_eq!(closed, Some(recorder_closed_ts(9)));
    }

    #[test]
    fn two_folders_sharing_one_session_id_keep_the_first_and_count_only_it() {
        let dir = temp_dir();
        let root = dir.join("recordings");
        // An ordinary copy/paste, or one synced tree mounted twice: the same
        // session folder appears twice under one root, carrying one
        // `meta.session_id`. The copy even has a longer ledger, so overwriting
        // the original would be visible in both directions.
        seed_session(
            &root,
            "2026/a-original",
            Some("01DEVICE-01SESSION"),
            Some("Standup"),
            Some("2026-08-08T12:00:00+02:00"),
            &[(0, "screen", 100)],
        );
        seed_session(
            &root,
            "2026/b-copy",
            Some("01DEVICE-01SESSION"),
            Some("Standup"),
            Some("2026-08-08T12:00:00+02:00"),
            &[(0, "screen", 100), (1, "screen", 200)],
        );

        let conn = memory_db();
        let written = rebuild(&conn, &root, "folder", None);
        assert_eq!(
            written, 1,
            "one id is one row, and the count never claims more rows than the run wrote"
        );
        assert_eq!(count(&conn, "recordings"), 1);

        let path: String = conn
            .query_row("SELECT relative_path FROM recordings", [], |r| r.get(0))
            .expect("read the surviving row");
        assert_eq!(
            path, "2026/a-original",
            "the first folder the sorted walk reaches keeps the id"
        );
        assert_eq!(
            segment_paths(&conn),
            vec!["2026/a-original/screen-0000.mov".to_owned()],
            "and the duplicate neither prunes nor rewrites the original's ledger"
        );

        // Deterministic rather than merely lucky: the same tree rebuilds to the
        // same row every time, on every machine.
        let again = rebuild(&conn, &root, "folder", None);
        assert_eq!(again, 1);
        let path: String = conn
            .query_row("SELECT relative_path FROM recordings", [], |r| r.get(0))
            .expect("read the surviving row again");
        assert_eq!(path, "2026/a-original");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_rebuild_stops_at_its_visit_budget_and_keeps_the_sessions_it_already_found() {
        let dir = temp_dir();
        let root = dir.join("recordings");
        for name in ["alpha", "bravo", "charlie", "delta"] {
            seed_session(
                &root,
                name,
                Some(&format!("01DEVICE-01{name}")),
                None,
                Some("2026-08-08T12:00:00+02:00"),
                &[],
            );
        }

        // The real [`RECOVERY_MAX_VISITS`] would need a fixture of four
        // thousand directories to exercise, so the budget's behaviour is proven
        // through the seam the public entry point delegates to, with a budget
        // of two against a root of four sessions.
        let capped = memory_db();
        let outcome = rebuild_from_disk_within(&capped, &request(&root, "folder", None), 2)
            .expect("capped rebuild");
        assert_eq!(
            outcome.written, 2,
            "the walk stops at the budget instead of running the root to its end"
        );
        assert_eq!(
            session_ids(&capped),
            vec!["01DEVICE-01alpha".to_owned(), "01DEVICE-01bravo".to_owned()],
            "and it is the sorted walk's first two, the same two on every machine"
        );

        // A walk the budget cut short did not see the whole root, so it may not
        // forget the sessions it never reached.
        let indexed = memory_db();
        assert_eq!(rebuild(&indexed, &root, "folder", None), 4);
        let outcome = rebuild_from_disk_within(&indexed, &request(&root, "folder", None), 2)
            .expect("capped rebuild over an indexed root");
        assert_eq!((outcome.written, outcome.removed), (2, 0));
        assert_eq!(
            count(&indexed, "recordings"),
            4,
            "charlie and delta were beyond the budget, not gone"
        );

        // The shipping entry point passes the real budget, which this tree is
        // nowhere near — so the truncation above is the budget's doing and
        // nothing else's.
        let whole = memory_db();
        assert_eq!(rebuild(&whole, &root, "folder", None), 4);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_sqlite_failure_part_way_through_a_session_rolls_that_whole_session_back() {
        let dir = temp_dir();
        let root = dir.join("recordings");
        seed_session(
            &root,
            "2026/session",
            Some("01DEVICE-01SESSION"),
            None,
            Some("2026-08-08T12:00:00+02:00"),
            &[(0, "screen", 100), (1, "poison", 200)],
        );

        let conn = memory_db();
        // A segment insert that fails half way through the ledger — the shape a
        // constraint violation or a full disk takes, made deterministic.
        conn.execute_batch(
            "CREATE TRIGGER refuse_poison BEFORE INSERT ON recording_segments \
             WHEN NEW.track = 'poison' \
             BEGIN SELECT RAISE(ABORT, 'poisoned segment'); END",
        )
        .expect("arm the failing insert");

        let error = rebuild_from_disk(&conn, &request(&root, "folder", None))
            .expect_err("the failure propagates");
        assert!(
            matches!(error, ArchiveError::Sqlite(_)),
            "a rebuild that cannot write fails loudly: {error:?}"
        );
        assert_eq!(
            count(&conn, "recordings"),
            0,
            "the session row written before the failure rolls back with it"
        );
        assert_eq!(
            count(&conn, "recording_segments"),
            0,
            "and so does the segment that did land — no half-written session is ever visible"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn move_session_repoints_the_row_and_every_segment_and_never_the_session_id() {
        let conn = memory_db();
        upsert_recording(&conn, &start_row("01DEVICE-01SESSION")).expect("start");
        for (index, track) in [(0u32, "screen"), (0, "camera"), (1, "screen")] {
            upsert_segment(
                &conn,
                &RecordingSegmentRow {
                    session_id: "01DEVICE-01SESSION".to_owned(),
                    index,
                    track: track.to_owned(),
                    relative_path: format!("2026/session/{track}-{index:04}.mov"),
                    bytes: 100,
                    pts_start: None,
                    pts_end: None,
                    closed_ts: Some(recorder_closed_ts(index)),
                },
            )
            .expect("segment");
        }
        // A second session, to prove the move is keyed on one id and not on a
        // path prefix.
        let mut other = start_row("01DEVICE-02OTHER");
        other.relative_path = "2026/session-other".to_owned();
        upsert_recording(&conn, &other).expect("another session");
        upsert_segment(
            &conn,
            &RecordingSegmentRow {
                session_id: "01DEVICE-02OTHER".to_owned(),
                index: 0,
                track: "screen".to_owned(),
                relative_path: "2026/session-other/screen-0000.mov".to_owned(),
                bytes: 100,
                pts_start: None,
                pts_end: None,
                closed_ts: None,
            },
        )
        .expect("the other session's segment");

        let moved = move_session(&conn, "01DEVICE-01SESSION", "2026/1432 Standup").expect("move");
        assert_eq!(moved, 1);

        let (session_id, path): (String, String) = conn
            .query_row(
                "SELECT session_id, relative_path FROM recordings WHERE session_id = ?1",
                rusqlite::params!["01DEVICE-01SESSION"],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .expect("read the moved row");
        assert_eq!(
            session_id, "01DEVICE-01SESSION",
            "a retitle moves a folder, never an identity"
        );
        assert_eq!(path, "2026/1432 Standup");
        assert_eq!(
            segment_paths(&conn),
            vec![
                "2026/1432 Standup/camera-0000.mov".to_owned(),
                "2026/1432 Standup/screen-0000.mov".to_owned(),
                "2026/1432 Standup/screen-0001.mov".to_owned(),
                "2026/session-other/screen-0000.mov".to_owned(),
            ],
            "every segment follows its own session, basename intact, and no other session moves"
        );
        let untouched: String = conn
            .query_row(
                "SELECT relative_path FROM recordings WHERE session_id = ?1",
                rusqlite::params!["01DEVICE-02OTHER"],
                |r| r.get(0),
            )
            .expect("read the other row");
        assert_eq!(untouched, "2026/session-other");
    }

    #[test]
    fn moving_a_session_the_index_never_saw_writes_nothing_and_is_not_an_error() {
        let conn = memory_db();
        upsert_recording(&conn, &start_row("01DEVICE-01SESSION")).expect("start");
        let before = dump(&conn);
        let moved = move_session(&conn, "01DEVICE-99MISSING", "2026/elsewhere")
            .expect("the index is a cache, never the thing that refuses a retitle");
        assert_eq!(moved, 0);
        assert_eq!(
            before,
            dump(&conn),
            "an unknown session moves nothing at all"
        );
    }

    #[test]
    fn no_column_anywhere_carries_the_destination_root() {
        let dir = temp_dir();
        let root = dir.join("recordings");
        seed_fifty(&root);
        let conn = memory_db();
        rebuild(&conn, &root, "profile", Some("01PROFILE"));

        let serialized = dump(&conn);
        let root_text = root.to_string_lossy().into_owned();
        assert!(
            !serialized.contains(&root_text),
            "a row must survive the tree being moved or cloned, so no column may name the root"
        );
        assert!(
            !serialized.contains(&dir.to_string_lossy().into_owned()),
            "nor any ancestor of it"
        );
        // Nothing that even looks like an absolute path.
        assert!(
            !serialized.contains("=Text(\"/"),
            "no column value starts with a path separator: {serialized}"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_row_normalises_the_tags_its_manifest_keeps_verbatim() {
        // Story 42.5's boundary, both halves at once: the row says what the tags
        // MEAN and the manifest still says what the user TYPED. Normalising the
        // manifest in place would be a rewrite of the user's own text, which the
        // story forbids outright.
        let root = temp_dir();
        let folder = root.join("2026").join("renewal-call");
        let typed = vec![
            "Client/Acme ".to_owned(),
            "client/acme".to_owned(),
            "  ".to_owned(),
            "Renewal".to_owned(),
        ];
        let manifest = SessionManifest::create_with_meta(
            folder.clone(),
            CaptureTarget::display(None),
            SessionDevices {
                system_audio: true,
                microphone: false,
                camera: false,
            },
            Some(SessionMeta {
                session_id: Some("01DEVICE-01TAGGED".to_owned()),
                title: Some("Renewal call".to_owned()),
                participants: None,
                note: None,
                tags: Some(typed.clone()),
                custom: None,
            }),
            Some("2026-08-08T10:00:00+01:00".to_owned()),
        )
        .expect("create the session folder");

        let row =
            RecordingRow::from_manifest(&manifest, "2026/renewal-call".to_owned(), "folder", None);
        // Canonical, deduplicated (the two casings are one tag), and the tag
        // that normalises to nothing is dropped rather than stored empty.
        assert_eq!(
            row.tags_json.as_deref(),
            Some(r#"["client/acme","renewal"]"#)
        );
        assert_eq!(row.tags(), vec!["client/acme", "renewal"]);

        // The manifest — in memory and on disk — is untouched.
        assert_eq!(
            manifest.meta.as_ref().and_then(|m| m.tags.clone()),
            Some(typed),
            "deriving a row must not edit the manifest it read"
        );
        let on_disk =
            std::fs::read_to_string(folder.join("manifest.json")).expect("read the manifest back");
        assert!(
            on_disk.contains("Client/Acme "),
            "the manifest still says what the user typed: {on_disk}"
        );

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn a_session_whose_tags_all_normalise_away_stores_no_tags_column() {
        // An empty tag is never stored — not as `[]`, not as `[""]`. A session
        // that typed only punctuation is a session with no tags.
        let root = temp_dir();
        let folder = root.join("2026").join("blank-tags");
        let manifest = SessionManifest::create_with_meta(
            folder,
            CaptureTarget::display(None),
            SessionDevices {
                system_audio: true,
                microphone: false,
                camera: false,
            },
            Some(SessionMeta {
                session_id: Some("01DEVICE-02BLANK".to_owned()),
                title: None,
                participants: None,
                note: None,
                tags: Some(vec!["  ".to_owned(), "///".to_owned(), "#---".to_owned()]),
                custom: None,
            }),
            None,
        )
        .expect("create the session folder");

        let row =
            RecordingRow::from_manifest(&manifest, "2026/blank-tags".to_owned(), "folder", None);
        assert_eq!(row.tags_json, None);
        assert!(row.tags().is_empty());

        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn indexed_tags_reports_every_tagged_session_and_skips_the_rest() {
        // The seed the tag tree cold-starts from. It must decode exactly what
        // the live report decodes, and it must not carry sessions that
        // contribute nothing.
        let conn = memory_db();
        let tagged = RecordingRow {
            title: Some("Renewal call".to_owned()),
            tags_json: Some(r#"["client/acme","renewal"]"#.to_owned()),
            ..start_row("01DEVICE-01TAGGED")
        };
        upsert_recording(&conn, &tagged).expect("index the tagged session");
        upsert_recording(&conn, &start_row("01DEVICE-02BARE")).expect("index an untagged session");
        let broken = RecordingRow {
            title: Some("Hand edited".to_owned()),
            tags_json: Some("client/acme, renewal".to_owned()),
            ..start_row("01DEVICE-03BROKEN")
        };
        upsert_recording(&conn, &broken).expect("index the hand-edited session");

        let seeded = indexed_tags(&conn).expect("read the recording tags");
        assert_eq!(
            seeded,
            vec![(
                "01DEVICE-01TAGGED".to_owned(),
                vec!["client/acme".to_owned(), "renewal".to_owned()]
            )],
            "the untagged session and the unreadable column contribute nothing, \
             and neither is an error"
        );
    }
}
