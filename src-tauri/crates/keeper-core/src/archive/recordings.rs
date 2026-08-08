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

use std::collections::HashMap;
use std::path::Path;

use rusqlite::Connection;
use serde::Deserialize;

use crate::error::ArchiveError;
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
    /// The session's tags as a JSON array of strings.
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
/// different id for it and the old row stays behind, pointing at a path that is
/// no longer there. That is the epic's rule anyway (a row is never deleted
/// because a folder is missing), and it costs a duplicate entry for
/// pre-40.3 sessions only — every session recorded since carries
/// `meta.session_id` and is immune.
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
                .and_then(|tags| serde_json::to_string(tags).ok()),
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

/// Re-derive every row from the session folders under `root` (Story 42.1).
///
/// **This is what makes the database a cache rather than a second truth.**
/// Deleting `archive.db` loses nothing the manifests do not carry, and this
/// function is the proof: it walks the tree, reads each `manifest.json`, and
/// writes the rows through the ordinary [`upsert_recording`] /
/// [`upsert_segment`] path — the same derivation ([`RecordingRow::from_manifest`])
/// the recorder used, so the result is identical rather than approximate.
/// Returns how many sessions it wrote.
///
/// **The walk is the recovery pass's walk**, and reuses its two caps
/// ([`RECOVERY_MAX_DEPTH`], [`RECOVERY_MAX_VISITS`]) rather than inventing a
/// third pair that could disagree with them: the root is a folder the USER chose
/// and may be a whole media library, so a rebuild must cost a bounded number of
/// `read_dir` calls. Like that pass it walks iteratively (a pathological tree
/// costs heap, never stack), skips symlinks by `DirEntry::file_type` so it can
/// never leave the tree or loop back into it, skips dot entries (`.Trash` and
/// friends are the OS's, not the user's recordings), and treats a directory
/// whose `manifest.json` LOADS as the session itself — never descending into it,
/// so a manifest a user copied inside a session cannot surface as a second row.
/// A manifest that fails to load is walked like any ordinary directory rather
/// than hiding every real session beneath it. It also sorts each directory's
/// entries by name before examining them, which that pass has no need to do:
/// `read_dir` order is whatever the filesystem happens to say, and the
/// duplicate rule below has to name the same winner on every machine.
///
/// **A session's segment ledger is reconciled, not cleared and rewritten.** The
/// manifest's ledger is authoritative (the terminal reconcile rebuilds it from
/// disk), so a segment it no longer lists is deleted — but only that one.
/// Clearing the whole ledger first would be one statement shorter and would
/// destroy `closed_ts`, the one segment fact no manifest carries, before
/// [`upsert_segment`] ever got the chance to preserve it. Session rows are never
/// deleted at all: absence on disk is a fact for a later story to present, not a
/// reason to forget the session.
///
/// **A session id is written at most once per run.** Two folders under one root
/// can carry the same `meta.session_id` — a session folder copied BESIDE its
/// original, or one synced tree mounted twice — and the primary key cannot hold
/// both. The first folder the (sorted, therefore reproducible) walk reaches
/// keeps the row; the second is logged at `warn` naming both relative paths, and
/// is neither written nor counted. Letting it through would be destructive
/// rather than merely wrong: its write would move the row onto the copy's path
/// and reconcile the ledger against the copy's manifest, so rebuilding an
/// untouched tree would DELETE rows, and the returned count would name more
/// sessions than the table holds. A duplicate is a fact about the tree, not a
/// reason to lose a row.
///
/// **Each session commits once.** Its row and the whole reconcile of its ledger
/// go in one transaction, so no reader on another connection can catch a session
/// between the delete of a stale segment and the insert of its replacement, and
/// a fifty-session tree costs fifty commits instead of several hundred.
///
/// Filesystem trouble is skipped and logged; a SQLite failure propagates,
/// because a rebuild that cannot write is not a rebuild and its caller is an
/// explicit maintenance action, never the recorder. The failing session's
/// transaction rolls back, so it is absent rather than half-written.
pub fn rebuild_from_disk(
    conn: &Connection,
    root: &Path,
    root_kind: &str,
    profile_id: Option<&str>,
) -> Result<usize, ArchiveError> {
    rebuild_from_disk_within(conn, root, root_kind, profile_id, RECOVERY_MAX_VISITS)
}

/// [`rebuild_from_disk`] with its visit budget as an argument, so the budget's
/// own behaviour can be proven against a tree of a dozen directories rather than
/// one of [`RECOVERY_MAX_VISITS`] — the seam
/// [`crate::recording::recover_orphaned_sessions`] keeps for exactly the same
/// reason, and for the same reason a truncated walk must be reachable from a
/// test at all. Every shipping caller goes through [`rebuild_from_disk`], which
/// always passes the real budget.
pub fn rebuild_from_disk_within(
    conn: &Connection,
    root: &Path,
    root_kind: &str,
    profile_id: Option<&str>,
    max_visits: usize,
) -> Result<usize, ArchiveError> {
    let mut written = 0usize;
    let mut visits = 0usize;
    // Every session id this run has already written, mapped to the folder that
    // claimed it: what makes the first of two duplicate folders win, and what
    // lets the warn name both.
    let mut written_ids: HashMap<String, String> = HashMap::new();
    let mut pending = vec![(root.to_path_buf(), 0usize)];
    'walk: while let Some((dir, depth)) = pending.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                tracing::warn!(%error, "archive rebuild: unreadable directory (non-fatal)");
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
                break 'walk;
            }
            visits += 1;
            let folder = entry.path();
            if folder.join("manifest.json").is_file() {
                match SessionManifest::load(&folder) {
                    Ok(manifest) => {
                        if write_rebuilt_session(
                            conn,
                            root,
                            &folder,
                            &manifest,
                            root_kind,
                            profile_id,
                            &mut written_ids,
                        )? {
                            written += 1;
                        }
                        continue;
                    }
                    Err(error) => tracing::warn!(
                        %error,
                        "archive rebuild: unreadable manifest; walking the directory instead"
                    ),
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
    Ok(written)
}

/// Write one rebuilt session and reconcile its whole segment ledger, returning
/// whether a row was written — and therefore whether the run counts it.
///
/// `Ok(false)`, never an error, in exactly two cases: the folder cannot be
/// expressed relative to `root`, which would force an absolute path into the row
/// (the one thing no column may hold), or another folder in this same run
/// already claimed the session id, in which case the first one keeps it (see
/// [`rebuild_from_disk`] on duplicates).
///
/// The row, its search index entry and the ledger reconcile share one
/// transaction, so a concurrent reader sees the session either wholly rebuilt or
/// wholly untouched — and a rebuild that dies part way through cannot leave a
/// row describing one thing and an index entry describing another (Story 42.2).
fn write_rebuilt_session(
    conn: &Connection,
    root: &Path,
    folder: &Path,
    manifest: &SessionManifest,
    root_kind: &str,
    profile_id: Option<&str>,
    written_ids: &mut HashMap<String, String>,
) -> Result<bool, ArchiveError> {
    let Some(relative) = relative_session_path(root, folder) else {
        tracing::warn!("archive rebuild: skipping a session that is not under the root");
        return Ok(false);
    };
    let row = RecordingRow::from_manifest(manifest, relative.clone(), root_kind, profile_id);
    if let Some(kept) = written_ids.get(&row.session_id) {
        // Both paths are root-relative, so this names positions inside the tree
        // the caller already chose and no location on the user's disk.
        tracing::warn!(
            session_id = %row.session_id,
            kept = %kept,
            skipped = %relative,
            "archive rebuild: two folders carry one session id; keeping the first"
        );
        return Ok(false);
    }
    in_transaction(conn, "rebuilt recording session", || {
        // Reindexes the session too, inside THIS transaction: `upsert_recording`
        // owns that pairing and is reentrant, so a rebuilt session — row, index
        // entry and ledger — still commits exactly once (see `in_transaction`).
        upsert_recording(conn, &row)?;
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
        Ok(())
    })?;
    written_ids.insert(row.session_id, relative);
    Ok(true)
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
        let written = rebuild_from_disk(&conn, &root, "folder", None).expect("rebuild");
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
        rebuild_from_disk(&conn, &root, "folder", None).expect("rebuild");

        // Story 40.4 moves the folder; the manifest (and its session id) rides
        // along byte-identical.
        let after = root.join("2026").join("1432 Standup");
        std::fs::rename(&before, &after).expect("move the session folder");
        rebuild_from_disk(&conn, &root, "folder", None).expect("rebuild after the move");

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
    fn a_rebuild_never_forgets_a_session_whose_folder_is_gone() {
        let dir = temp_dir();
        let root = dir.join("recordings");
        let folder = seed_session(
            &root,
            "2026/deleted",
            Some("01DEVICE-01SESSION"),
            None,
            Some("2026-08-08T12:00:00+02:00"),
            &[],
        );
        let conn = memory_db();
        rebuild_from_disk(&conn, &root, "folder", None).expect("rebuild");
        assert_eq!(count(&conn, "recordings"), 1);

        std::fs::remove_dir_all(&folder).expect("delete the session folder");
        let written = rebuild_from_disk(&conn, &root, "folder", None).expect("rebuild again");
        assert_eq!(written, 0, "nothing on disk to write");
        assert_eq!(
            count(&conn, "recordings"),
            1,
            "absence on disk is a fact for a later story, never a deletion"
        );
        let _ = std::fs::remove_dir_all(&dir);
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
        let written = rebuild_from_disk(&conn, &root, "folder", None).expect("rebuild");
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
        let rewritten =
            rebuild_from_disk(&normal, &root, "profile", Some("01PROFILE")).expect("rebuild");
        assert_eq!(rewritten, 50);
        assert_eq!(
            before,
            dump(&normal),
            "a rebuild over an indexed tree reproduces every manifest field exactly and erases nothing else"
        );

        // And `archive.db` deleted outright: the same rows, short of precisely
        // the three columns no manifest can carry.
        let rebuilt = memory_db();
        let written =
            rebuild_from_disk(&rebuilt, &root, "profile", Some("01PROFILE")).expect("rebuild");
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
        rebuild_from_disk(&rebuilt, &root, "profile", Some("01PROFILE")).expect("rebuild twice");
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
        let written = rebuild_from_disk(&conn, &root, "folder", None).expect("rebuild");
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
        let again = rebuild_from_disk(&conn, &root, "folder", None).expect("rebuild twice");
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
        let written =
            rebuild_from_disk_within(&capped, &root, "folder", None, 2).expect("capped rebuild");
        assert_eq!(
            written, 2,
            "the walk stops at the budget instead of running the root to its end"
        );
        assert_eq!(
            session_ids(&capped),
            vec!["01DEVICE-01alpha".to_owned(), "01DEVICE-01bravo".to_owned()],
            "and it is the sorted walk's first two, the same two on every machine"
        );

        // The shipping entry point passes the real budget, which this tree is
        // nowhere near — so the truncation above is the budget's doing and
        // nothing else's.
        let whole = memory_db();
        assert_eq!(
            rebuild_from_disk(&whole, &root, "folder", None).expect("whole rebuild"),
            4
        );
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

        let error =
            rebuild_from_disk(&conn, &root, "folder", None).expect_err("the failure propagates");
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
        rebuild_from_disk(&conn, &root, "profile", Some("01PROFILE")).expect("rebuild");

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
}
