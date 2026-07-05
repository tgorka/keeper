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
/// `media_json` (media *metadata* only — never bytes), and `inserted_ts`. The
/// primary key `(account_id, event_id)` is what makes ingestion idempotent.
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
            PRIMARY KEY(account_id, event_id)\
        )",
        [],
    )
    .map_err(|e| ArchiveError::Sqlite(format!("could not ensure events schema: {e}")))?;
    Ok(conn)
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
                media_json, inserted_ts \
         FROM events WHERE account_id = ?1 AND event_id = ?2",
        rusqlite::params![account_id, event_id],
        |r| {
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
            })
        },
    )
    .map(Some)
    .or_else(|e| match e {
        rusqlite::Error::QueryReturnedNoRows => Ok(None),
        other => Err(ArchiveError::Sqlite(format!(
            "could not read event: {other}"
        ))),
    })
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
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
