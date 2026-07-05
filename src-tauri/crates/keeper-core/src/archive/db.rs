//! `archive.db` schema, open path, and read helpers (Story 5.1, AD-21, epic 5).
//!
//! One `archive.db` for *all* Accounts at `<data_dir>/archive.db`, in WAL mode
//! for crash resilience (mirrors [`crate::registry`]'s `open()`). The `events`
//! table is append-only and keyed on `(account_id, event_id)`, so a re-synced
//! event is idempotently ignored rather than duplicated.
//!
//! All functions here are synchronous: a rusqlite [`Connection`] is never held
//! across an `.await`. The single archive writer task (see
//! [`crate::archive::ingest`]) owns one connection for the app's lifetime and
//! makes only synchronous calls on it between channel receives.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::fts;
use crate::error::ArchiveError;

/// Resolve the `archive.db` path under a data directory. The single canonical
/// path helper — every writer/reader connection resolves the file through here.
pub fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("archive.db")
}

/// How long a connection waits on a locked database before returning
/// `SQLITE_BUSY`. The long-lived writer connection and short-lived reader
/// connections (`event_count`/`get_event`, and downstream stories) share the one
/// file; a WAL checkpoint briefly needs the write lock, so a non-zero busy
/// timeout keeps concurrent reads from erroring out instead of waiting.
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Open `archive.db` in WAL mode, ensuring the data dir and `events` schema
/// exist. Every call is idempotent (`CREATE TABLE IF NOT EXISTS`), so reopening
/// the same file after a restart preserves every previously committed row.
///
/// The row captures the normalized event: `account_id`, `event_id`, `room_id`,
/// `sender`, `origin_ts` (ms epoch), `event_type`, `content_json`, an optional
/// `media_json` (media *metadata* only — never bytes), `inserted_ts`, the edit
/// linkage `relates_to_event_id`/`rel_type`, a `redacted_ts` retention marker, and
/// the indexed `body` text backing full-text search (Story 5.3). The primary key
/// `(account_id, event_id)` is what makes ingestion idempotent.
pub fn open_archive_db(data_dir: &Path) -> Result<Connection, ArchiveError> {
    std::fs::create_dir_all(data_dir)
        .map_err(|e| ArchiveError::Sqlite(format!("could not create data dir: {e}")))?;
    let conn = Connection::open(db_path(data_dir))
        .map_err(|e| ArchiveError::Sqlite(format!("could not open archive.db: {e}")))?;
    // Wait on a briefly-held lock rather than erroring immediately: the one
    // long-lived writer connection and short-lived reader connections share this
    // file (a WAL checkpoint needs the write lock momentarily).
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(|e| ArchiveError::Sqlite(format!("could not set busy timeout: {e}")))?;
    // WAL for crash resilience (epic 5 crash-safety requirement).
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| ArchiveError::Sqlite(format!("could not set WAL mode: {e}")))?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS events(\
            account_id TEXT NOT NULL, \
            event_id TEXT NOT NULL, \
            room_id TEXT NOT NULL, \
            sender TEXT NOT NULL, \
            origin_ts INTEGER NOT NULL, \
            event_type TEXT NOT NULL, \
            content_json TEXT NOT NULL, \
            media_json TEXT, \
            inserted_ts INTEGER NOT NULL, \
            relates_to_event_id TEXT, \
            rel_type TEXT, \
            redacted_ts INTEGER, \
            body TEXT, \
            PRIMARY KEY(account_id, event_id)\
        )",
        [],
    )
    .map_err(|e| ArchiveError::Sqlite(format!("could not ensure events schema: {e}")))?;
    // Story 5.2 durability columns + Story 5.3 `body`: idempotently add them to a
    // pre-5.1/5.2 `archive.db` that predates the extended schema above (`CREATE
    // TABLE IF NOT EXISTS` never alters an existing table). Every column is
    // nullable, so no existing row needs rewriting; re-running is a no-op.
    migrate_durability_columns(&conn)?;
    // Index the replace-relation join key so the version-chain lookup
    // (`edit_chain`) does not scan the account's rows.
    conn.execute(
        "CREATE INDEX IF NOT EXISTS idx_events_replace \
         ON events(account_id, relates_to_event_id)",
        [],
    )
    .map_err(|e| ArchiveError::Sqlite(format!("could not ensure replace index: {e}")))?;
    // Story 5.3: backfill the `body` of any row inserted before the column existed
    // (a pre-5.1/5.2 archive, or a 5.1/5.2 row inserted before this migration), then
    // ensure the FTS index exists (creating + `'rebuild'`-ing it once on fresh
    // creation). Both are idempotent no-ops on re-open.
    backfill_missing_bodies(&conn)?;
    fts::ensure_fts(&conn)?;
    Ok(conn)
}

/// Backfill the `body` column for every row where it is still `NULL` (Story 5.3).
///
/// `NULL` marks "not yet backfilled" (a row inserted before the `body` column
/// existed); a genuinely text-less row is stored as the empty string, never
/// `NULL`. Reads each such row's `content_json` and writes the shared
/// [`crate::archive::display_body_from_content`] extraction back, so the backfill
/// uses the exact same body fidelity as ingest. Idempotent: once every row has a
/// non-`NULL` `body`, this selects zero rows and is a no-op.
fn backfill_missing_bodies(conn: &Connection) -> Result<(), ArchiveError> {
    // Collect the (rowid, content_json) of unbackfilled rows first so we are not
    // iterating a live statement while issuing UPDATEs on the same connection.
    let pending: Vec<(i64, String)> = {
        let mut stmt = conn
            .prepare("SELECT rowid, content_json FROM events WHERE body IS NULL")
            .map_err(|e| ArchiveError::Sqlite(format!("could not prepare backfill scan: {e}")))?;
        let rows = stmt
            .query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?)))
            .map_err(|e| ArchiveError::Sqlite(format!("could not scan for backfill: {e}")))?;
        let mut out = Vec::new();
        for row in rows {
            out.push(
                row.map_err(|e| ArchiveError::Sqlite(format!("could not read backfill row: {e}")))?,
            );
        }
        out
    };
    if pending.is_empty() {
        return Ok(());
    }
    // One transaction for the whole backfill: on a large pre-5.3 archive this turns
    // up to N per-row WAL commits (which could hang startup for minutes) into a
    // single commit, and makes the backfill all-or-nothing so a crash can't leave a
    // half-populated `body` that the paired `ensure_fts` 'rebuild' would index
    // incompletely.
    conn.execute_batch("BEGIN")
        .map_err(|e| ArchiveError::Sqlite(format!("could not begin backfill: {e}")))?;
    let result = (|| {
        for (rowid, content_json) in &pending {
            let body = crate::archive::display_body_from_content(content_json);
            conn.execute(
                "UPDATE events SET body = ?2 WHERE rowid = ?1",
                rusqlite::params![rowid, body],
            )
            .map_err(|e| ArchiveError::Sqlite(format!("could not backfill body: {e}")))?;
        }
        Ok::<(), ArchiveError>(())
    })();
    match result {
        Ok(()) => conn
            .execute_batch("COMMIT")
            .map_err(|e| ArchiveError::Sqlite(format!("could not commit backfill: {e}"))),
        Err(e) => {
            let _ = conn.execute_batch("ROLLBACK");
            Err(e)
        }
    }
}

/// Insert one normalized event row through the writer connection (Story 5.1/5.3).
///
/// Idempotent on the `(account_id, event_id)` primary key (`INSERT OR IGNORE`).
/// Returns `Some(rowid)` when a row was actually inserted (rows-affected == 1) so
/// the caller can index it into `events_fts`, or `None` when the insert was a
/// no-op on a re-synced duplicate (rows-affected == 0) — the caller must never
/// double-index in that case. `media_json` is the pre-serialized media metadata (or
/// `None`); `body` is the shared body extraction (`""` for a text-less event).
pub fn insert_event(
    conn: &Connection,
    ev: &crate::archive::ArchiveEvent,
    media_json: Option<&str>,
    inserted_ts: i64,
) -> Result<Option<i64>, ArchiveError> {
    let affected = conn
        .execute(
            "INSERT OR IGNORE INTO events(\
                account_id, event_id, room_id, sender, origin_ts, event_type, \
                content_json, media_json, inserted_ts, relates_to_event_id, rel_type, body\
             ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
            rusqlite::params![
                ev.account_id,
                ev.event_id,
                ev.room_id,
                ev.sender,
                ev.origin_ts,
                ev.event_type,
                ev.content_json,
                media_json,
                inserted_ts,
                ev.relates_to_event_id,
                ev.rel_type,
                ev.body,
            ],
        )
        .map_err(|e| ArchiveError::Sqlite(format!("could not insert event: {e}")))?;
    Ok(if affected == 1 {
        Some(conn.last_insert_rowid())
    } else {
        None
    })
}

/// Open `archive.db` as a **read-only** connection for concurrent reads (Story
/// 5.3 search, and later read-only surfaces). WAL permits any number of readers
/// alongside the single writer, so this never blocks or races the writer task.
///
/// Unlike [`open_archive_db`] this performs no schema creation or migration — a
/// reader must never write. It assumes the file exists and has already been opened
/// (and migrated) at least once by the writer path. `SQLITE_OPEN_READ_ONLY` makes
/// any accidental write attempt fail fast rather than corrupt the file.
pub fn open_readonly_archive_db(data_dir: &Path) -> Result<Connection, ArchiveError> {
    let conn = Connection::open_with_flags(
        db_path(data_dir),
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|e| ArchiveError::Sqlite(format!("could not open archive.db read-only: {e}")))?;
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(|e| ArchiveError::Sqlite(format!("could not set busy timeout: {e}")))?;
    Ok(conn)
}

/// Open a throwaway **in-memory** `archive.db` with the full `events` schema
/// (Story 5.5). Used only by export when a never-synced install has no `archive.db`
/// file yet: the export then reads an empty archive and produces valid empty output
/// instead of erroring. Never used for ingestion or reads of real data.
pub fn open_empty_in_memory_archive_db() -> Result<Connection, ArchiveError> {
    let conn = Connection::open_in_memory()
        .map_err(|e| ArchiveError::Sqlite(format!("could not open in-memory archive: {e}")))?;
    conn.execute(
        "CREATE TABLE events(\
            account_id TEXT NOT NULL, event_id TEXT NOT NULL, room_id TEXT NOT NULL, \
            sender TEXT NOT NULL, origin_ts INTEGER NOT NULL, event_type TEXT NOT NULL, \
            content_json TEXT NOT NULL, media_json TEXT, inserted_ts INTEGER NOT NULL, \
            relates_to_event_id TEXT, rel_type TEXT, redacted_ts INTEGER, body TEXT, \
            PRIMARY KEY(account_id, event_id))",
        [],
    )
    .map_err(|e| ArchiveError::Sqlite(format!("could not create in-memory events: {e}")))?;
    Ok(conn)
}

/// The Story 5.2 durability columns, each nullable and each added by the
/// idempotent migration below when missing from a pre-5.1 `events` table.
const DURABILITY_COLUMNS: &[(&str, &str)] = &[
    ("relates_to_event_id", "TEXT"),
    ("rel_type", "TEXT"),
    ("redacted_ts", "INTEGER"),
    // Story 5.3: the nullable indexed display body. `NULL` on a freshly-added
    // column marks "not yet backfilled"; `backfill_missing_bodies` then fills it.
    ("body", "TEXT"),
];

/// Idempotently add the Story 5.2 durability columns to an existing `events`
/// table (Story 5.2). Reads `PRAGMA table_info(events)` to learn which columns
/// already exist and issues `ALTER TABLE … ADD COLUMN` only for the missing ones,
/// so it is safe to run on a fresh DB (columns already present via `CREATE
/// TABLE`), on a pre-5.1 DB (columns added, existing rows untouched), and again
/// after that (a no-op). All columns are nullable — no row is rewritten.
fn migrate_durability_columns(conn: &Connection) -> Result<(), ArchiveError> {
    let mut existing: Vec<String> = Vec::new();
    {
        let mut stmt = conn
            .prepare("PRAGMA table_info(events)")
            .map_err(|e| ArchiveError::Sqlite(format!("could not read table info: {e}")))?;
        let rows = stmt
            // `PRAGMA table_info` column 1 (`name`) is the column name.
            .query_map([], |r| r.get::<_, String>(1))
            .map_err(|e| ArchiveError::Sqlite(format!("could not read table info rows: {e}")))?;
        for name in rows {
            existing.push(
                name.map_err(|e| ArchiveError::Sqlite(format!("could not read column name: {e}")))?,
            );
        }
    }
    for (name, ty) in DURABILITY_COLUMNS {
        if !existing.iter().any(|c| c == name) {
            conn.execute(&format!("ALTER TABLE events ADD COLUMN {name} {ty}"), [])
                .map_err(|e| ArchiveError::Sqlite(format!("could not add column {name}: {e}")))?;
        }
    }
    Ok(())
}

/// Count the archived events for one account. A read helper for tests and
/// downstream stories; returns `0` for an account with no rows.
pub fn event_count(conn: &Connection, account_id: &str) -> Result<i64, ArchiveError> {
    conn.query_row(
        "SELECT COUNT(*) FROM events WHERE account_id = ?1",
        rusqlite::params![account_id],
        |r| r.get::<_, i64>(0),
    )
    .map_err(|e| ArchiveError::Sqlite(format!("could not count events: {e}")))
}

/// One archived event row read back from `archive.db`. Mirrors the normalized
/// insert shape; `media_json` is `None` for a non-media event.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredEvent {
    /// Opaque keeper account id (ULID) the event belongs to.
    pub account_id: String,
    /// Matrix event id — unique within an account (the dedupe key).
    pub event_id: String,
    /// Matrix room id the event was sent to.
    pub room_id: String,
    /// Matrix user id of the sender.
    pub sender: String,
    /// Origin server timestamp in milliseconds since the Unix epoch.
    pub origin_ts: i64,
    /// Matrix event type (e.g. `m.room.message`).
    pub event_type: String,
    /// The event content, serialized as JSON.
    pub content_json: String,
    /// Media metadata JSON (mxc/mimetype/size/dims/filename), or `None` for a
    /// non-media event. Never holds media bytes.
    pub media_json: Option<String>,
    /// Local insert time in milliseconds since the Unix epoch.
    pub inserted_ts: i64,
    /// For an edit event (`m.replace`), the target event id being replaced; `None`
    /// for a plain message (Story 5.2). This is the join key of the version chain.
    pub relates_to_event_id: Option<String>,
    /// The relation type (`"m.replace"` for an edit), or `None` for a plain
    /// message (Story 5.2).
    pub rel_type: Option<String>,
    /// When a remote redaction has marked this row: the redaction's timestamp in
    /// milliseconds since the Unix epoch, or `None` when the row is not redacted
    /// (Story 5.2). Marking never erases `content_json`/`media_json`.
    pub redacted_ts: Option<i64>,
}

/// Fetch a single archived event by `(account_id, event_id)`, or `None` when it
/// has not been ingested. A read helper for tests and downstream stories.
pub fn get_event(
    conn: &Connection,
    account_id: &str,
    event_id: &str,
) -> Result<Option<StoredEvent>, ArchiveError> {
    conn.query_row(
        "SELECT account_id, event_id, room_id, sender, origin_ts, event_type, content_json, \
                media_json, inserted_ts, relates_to_event_id, rel_type, redacted_ts \
         FROM events WHERE account_id = ?1 AND event_id = ?2",
        rusqlite::params![account_id, event_id],
        map_stored_event,
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(ArchiveError::Sqlite(format!(
            "could not read event: {other}"
        ))),
    })
}

/// Map a full `events` row (in the canonical column order used by `get_event`,
/// `edit_chain`, and `retrievable_content`) into a [`StoredEvent`]. One home for
/// the row shape so the readers never drift.
fn map_stored_event(r: &rusqlite::Row<'_>) -> rusqlite::Result<StoredEvent> {
    Ok(StoredEvent {
        account_id: r.get(0)?,
        event_id: r.get(1)?,
        room_id: r.get(2)?,
        sender: r.get(3)?,
        origin_ts: r.get(4)?,
        event_type: r.get(5)?,
        content_json: r.get(6)?,
        media_json: r.get::<_, Option<String>>(7)?,
        inserted_ts: r.get(8)?,
        relates_to_event_id: r.get::<_, Option<String>>(9)?,
        rel_type: r.get::<_, Option<String>>(10)?,
        redacted_ts: r.get::<_, Option<i64>>(11)?,
    })
}

/// The canonical `SELECT` column list for reading a full [`StoredEvent`] row.
const STORED_EVENT_COLUMNS: &str =
    "account_id, event_id, room_id, sender, origin_ts, event_type, content_json, \
     media_json, inserted_ts, relates_to_event_id, rel_type, redacted_ts";

/// Read the edit version chain for a target event (Story 5.2, FR-11).
///
/// Returns the original row `event_id` (when it has been ingested) plus every
/// edit row that replaces it (`relates_to_event_id = event_id AND rel_type =
/// 'm.replace'`), all ordered by `origin_ts` ascending — original first, newest
/// edit last. Ties on `origin_ts` (rapid edits sharing a server timestamp) break
/// deterministically by `inserted_ts` then `event_id`, so the "current" version
/// is stable. A target with no archived original and no edits yields an empty
/// vec. Read-only; never mutates a row.
pub fn edit_chain(
    conn: &Connection,
    account_id: &str,
    event_id: &str,
) -> Result<Vec<StoredEvent>, ArchiveError> {
    let sql = format!(
        "SELECT {STORED_EVENT_COLUMNS} FROM events \
         WHERE account_id = ?1 \
           AND (event_id = ?2 OR (relates_to_event_id = ?2 AND rel_type = 'm.replace')) \
         ORDER BY origin_ts ASC, inserted_ts ASC, event_id ASC"
    );
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| ArchiveError::Sqlite(format!("could not prepare edit chain: {e}")))?;
    let rows = stmt
        .query_map(rusqlite::params![account_id, event_id], map_stored_event)
        .map_err(|e| ArchiveError::Sqlite(format!("could not query edit chain: {e}")))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| ArchiveError::Sqlite(format!("could not read chain row: {e}")))?);
    }
    Ok(out)
}

/// Retrieve the archived content for an event, gated by the honor-deletions
/// policy (Story 5.2, FR-36). Returns the row unless the row is redacted
/// (`redacted_ts` set) *and* `honor_deletions` is `true`, in which case it
/// returns `None` — the pre-redaction content stays physically on disk regardless
/// (marking never erases). `None` is also returned when the event was never
/// ingested. Read-only.
pub fn retrievable_content(
    conn: &Connection,
    account_id: &str,
    event_id: &str,
    honor_deletions: bool,
) -> Result<Option<StoredEvent>, ArchiveError> {
    let row = get_event(conn, account_id, event_id)?;
    Ok(match row {
        Some(row) if honor_deletions && row.redacted_ts.is_some() => None,
        other => other,
    })
}

/// Mark an archived event as redacted by setting `redacted_ts` on the target row
/// (Story 5.2, FR-36). Marks only — `content_json`/`media_json` are never
/// touched. When the target is not in the archive the `UPDATE` affects zero rows
/// and returns without error. An already-marked row is overwritten with the new
/// timestamp (idempotent in effect).
pub fn mark_redacted(
    conn: &Connection,
    account_id: &str,
    event_id: &str,
    redacted_ts: i64,
) -> Result<(), ArchiveError> {
    conn.execute(
        "UPDATE events SET redacted_ts = ?3 WHERE account_id = ?1 AND event_id = ?2",
        rusqlite::params![account_id, event_id, redacted_ts],
    )
    .map(|_| ())
    .map_err(|e| ArchiveError::Sqlite(format!("could not mark redacted: {e}")))
}

/// The archive slice an export covers (Story 5.5). A plain keeper-core domain
/// struct (the IPC `ExportScopeKind` + ids map into this) so the readers stay
/// tauri-free. `account_id`/`room_id` are `None` for the wider scopes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExportScope {
    /// One Chat: both the account and room are pinned.
    Chat {
        /// The keeper account id owning the room.
        account_id: String,
        /// The Matrix room id.
        room_id: String,
    },
    /// One Account: every room of a single account.
    Account {
        /// The keeper account id.
        account_id: String,
    },
    /// Every account and every room in the archive.
    Everything,
}

impl ExportScope {
    /// The `WHERE` predicate + bound params selecting the scope's rows. `Everything`
    /// selects all rows (`1 = 1`). Kept in one place so the count and the reader
    /// never drift.
    fn where_clause(&self) -> (String, Vec<String>) {
        match self {
            ExportScope::Chat {
                account_id,
                room_id,
            } => (
                "account_id = ?1 AND room_id = ?2".to_owned(),
                vec![account_id.clone(), room_id.clone()],
            ),
            ExportScope::Account { account_id } => {
                ("account_id = ?1".to_owned(), vec![account_id.clone()])
            }
            ExportScope::Everything => ("1 = 1".to_owned(), Vec::new()),
        }
    }
}

/// Count the archived rows in an export scope (Story 5.5). This is the provability
/// denominator: the emitted lossless-JSON event count must equal this. Read-only.
pub fn scoped_event_count(conn: &Connection, scope: &ExportScope) -> Result<i64, ArchiveError> {
    let (where_clause, params) = scope.where_clause();
    let sql = format!("SELECT COUNT(*) FROM events WHERE {where_clause}");
    let param_refs: Vec<&dyn rusqlite::ToSql> =
        params.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
    conn.query_row(&sql, param_refs.as_slice(), |r| r.get::<_, i64>(0))
        .map_err(|e| ArchiveError::Sqlite(format!("could not count scoped events: {e}")))
}

/// Read every archived row in an export scope, chronologically ordered for a
/// transcript (Story 5.5). Ordered `origin_ts ASC` with a deterministic tie-break
/// (`inserted_ts ASC, event_id ASC`) so the transcript is stable and reproducible.
/// This returns **all** rows (every edit-chain version + redacted-retained rows) —
/// the lossless set whose length equals [`scoped_event_count`]. Read-only.
pub fn scoped_events_chronological(
    conn: &Connection,
    scope: &ExportScope,
) -> Result<Vec<StoredEvent>, ArchiveError> {
    let (where_clause, params) = scope.where_clause();
    let sql = format!(
        "SELECT {STORED_EVENT_COLUMNS} FROM events WHERE {where_clause} \
         ORDER BY origin_ts ASC, inserted_ts ASC, event_id ASC"
    );
    let param_refs: Vec<&dyn rusqlite::ToSql> =
        params.iter().map(|p| p as &dyn rusqlite::ToSql).collect();
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| ArchiveError::Sqlite(format!("could not prepare scoped read: {e}")))?;
    let rows = stmt
        .query_map(param_refs.as_slice(), map_stored_event)
        .map_err(|e| ArchiveError::Sqlite(format!("could not query scoped events: {e}")))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| ArchiveError::Sqlite(format!("could not read scoped row: {e}")))?);
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let mut dir = std::env::temp_dir();
        let unique = format!(
            "keeper-archive-db-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        );
        dir.push(unique);
        dir
    }

    #[test]
    fn open_creates_wal_db_and_empty_count() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open archive.db");
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .expect("read journal_mode");
        assert_eq!(mode.to_lowercase(), "wal");
        assert_eq!(event_count(&conn, "acctA").expect("count"), 0);
        assert_eq!(get_event(&conn, "acctA", "$e1").expect("get"), None);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn open_is_idempotent_and_reopen_preserves_rows() {
        let dir = temp_dir();
        {
            let conn = open_archive_db(&dir).expect("open");
            conn.execute(
                "INSERT INTO events(account_id, event_id, room_id, sender, origin_ts, \
                 event_type, content_json, media_json, inserted_ts) \
                 VALUES ('acctA', '$e1', '!r', '@u:e.org', 1, 'm.room.message', '{}', NULL, 2)",
                [],
            )
            .expect("insert");
        }
        // Reopen the same file: schema creation is a no-op and the row survives.
        let conn = open_archive_db(&dir).expect("reopen");
        assert_eq!(event_count(&conn, "acctA").expect("count"), 1);
        let row = get_event(&conn, "acctA", "$e1")
            .expect("get")
            .expect("row present");
        assert_eq!(row.event_type, "m.room.message");
        assert_eq!(row.media_json, None);
        assert_eq!(row.relates_to_event_id, None);
        assert_eq!(row.rel_type, None);
        assert_eq!(row.redacted_ts, None);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Insert one full `events` row directly. Kept terse so the durability tests
    /// read as data-then-assertion.
    #[allow(clippy::too_many_arguments)]
    fn insert_row(
        conn: &Connection,
        account_id: &str,
        event_id: &str,
        origin_ts: i64,
        content_json: &str,
        relates_to: Option<&str>,
        rel_type: Option<&str>,
    ) {
        conn.execute(
            "INSERT INTO events(account_id, event_id, room_id, sender, origin_ts, event_type, \
             content_json, media_json, inserted_ts, relates_to_event_id, rel_type, redacted_ts) \
             VALUES (?1, ?2, '!r:e.org', '@u:e.org', ?3, 'm.room.message', ?4, NULL, 0, ?5, ?6, NULL)",
            rusqlite::params![account_id, event_id, origin_ts, content_json, relates_to, rel_type],
        )
        .expect("insert row");
    }

    #[test]
    fn edit_chain_orders_original_then_edits_by_origin_ts() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        // Two edits arrive out of order (later ts first); the original in between.
        insert_row(
            &conn,
            "acctA",
            "$edit2",
            300,
            r#"{"body":"v3"}"#,
            Some("$orig"),
            Some("m.replace"),
        );
        insert_row(&conn, "acctA", "$orig", 100, r#"{"body":"v1"}"#, None, None);
        insert_row(
            &conn,
            "acctA",
            "$edit1",
            200,
            r#"{"body":"v2"}"#,
            Some("$orig"),
            Some("m.replace"),
        );
        // A replace targeting a *different* original must not leak in.
        insert_row(
            &conn,
            "acctA",
            "$other",
            250,
            r#"{"body":"x"}"#,
            Some("$else"),
            Some("m.replace"),
        );
        let chain = edit_chain(&conn, "acctA", "$orig").expect("chain");
        let ids: Vec<&str> = chain.iter().map(|r| r.event_id.as_str()).collect();
        assert_eq!(ids, vec!["$orig", "$edit1", "$edit2"]);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn edit_chain_absent_target_is_empty() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        assert!(edit_chain(&conn, "acctA", "$missing")
            .expect("chain")
            .is_empty());
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mark_redacted_sets_ts_without_erasing_content() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        insert_row(
            &conn,
            "acctA",
            "$e1",
            100,
            r#"{"body":"secret"}"#,
            None,
            None,
        );
        mark_redacted(&conn, "acctA", "$e1", 999).expect("mark");
        let row = get_event(&conn, "acctA", "$e1").expect("get").expect("row");
        assert_eq!(row.redacted_ts, Some(999));
        assert_eq!(
            row.content_json, r#"{"body":"secret"}"#,
            "content must be retained"
        );
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn mark_redacted_absent_target_is_a_no_op() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        // No row for $ghost — the UPDATE affects zero rows and must not error.
        mark_redacted(&conn, "acctA", "$ghost", 999).expect("mark no-op");
        assert_eq!(get_event(&conn, "acctA", "$ghost").expect("get"), None);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn retrievable_content_honors_deletion_only_when_on() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        insert_row(&conn, "acctA", "$e1", 100, r#"{"body":"kept"}"#, None, None);
        mark_redacted(&conn, "acctA", "$e1", 999).expect("mark");
        // honor OFF → returns the row (incl. pre-redaction content).
        let off = retrievable_content(&conn, "acctA", "$e1", false).expect("off");
        assert_eq!(off.expect("row").content_json, r#"{"body":"kept"}"#);
        // honor ON → None (not retrievable), but the row still exists on disk.
        let on = retrievable_content(&conn, "acctA", "$e1", true).expect("on");
        assert_eq!(on, None);
        assert!(get_event(&conn, "acctA", "$e1")
            .expect("still on disk")
            .is_some());
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn retrievable_content_non_redacted_returns_row_regardless() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        insert_row(&conn, "acctA", "$e1", 100, r#"{"body":"hi"}"#, None, None);
        assert!(retrievable_content(&conn, "acctA", "$e1", true)
            .expect("on")
            .is_some());
        assert!(retrievable_content(&conn, "acctA", "$e1", false)
            .expect("off")
            .is_some());
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Migration idempotency over a pre-5.1 schema: create the OLD 9-column table,
    /// insert a row, then run the open/migration path and assert the new columns
    /// exist, the old row is intact, and a second open is a no-op.
    #[test]
    fn migration_adds_columns_to_pre_5_1_schema_idempotently() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).expect("mkdir");
        {
            // Hand-build the pre-5.1 (Story 5.1) schema: 9 columns, no durability.
            let conn = Connection::open(db_path(&dir)).expect("open raw");
            conn.execute(
                "CREATE TABLE events(\
                    account_id TEXT NOT NULL, event_id TEXT NOT NULL, room_id TEXT NOT NULL, \
                    sender TEXT NOT NULL, origin_ts INTEGER NOT NULL, event_type TEXT NOT NULL, \
                    content_json TEXT NOT NULL, media_json TEXT, inserted_ts INTEGER NOT NULL, \
                    PRIMARY KEY(account_id, event_id))",
                [],
            )
            .expect("create old schema");
            conn.execute(
                "INSERT INTO events(account_id, event_id, room_id, sender, origin_ts, event_type, \
                 content_json, media_json, inserted_ts) \
                 VALUES ('acctA', '$old', '!r', '@u:e.org', 42, 'm.room.message', '{\"body\":\"old\"}', NULL, 7)",
                [],
            )
            .expect("insert old row");
        }
        // Reopen via the migration path.
        let conn = open_archive_db(&dir).expect("open migrates");
        // New columns exist and read as NULL for the pre-existing row.
        let row = get_event(&conn, "acctA", "$old")
            .expect("get")
            .expect("row");
        assert_eq!(row.content_json, r#"{"body":"old"}"#, "old row intact");
        assert_eq!(row.inserted_ts, 7);
        assert_eq!(row.relates_to_event_id, None);
        assert_eq!(row.rel_type, None);
        assert_eq!(row.redacted_ts, None);
        // The new helpers work against the migrated DB.
        mark_redacted(&conn, "acctA", "$old", 100).expect("mark on migrated");
        assert_eq!(
            get_event(&conn, "acctA", "$old")
                .expect("get")
                .expect("row")
                .redacted_ts,
            Some(100)
        );
        drop(conn);
        // Re-run: a second open is a no-op and the row still reads back.
        let conn = open_archive_db(&dir).expect("reopen no-op");
        assert!(get_event(&conn, "acctA", "$old").expect("get").is_some());
        assert_eq!(event_count(&conn, "acctA").expect("count"), 1);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
