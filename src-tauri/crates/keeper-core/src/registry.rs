//! Non-secret account registry backed by `keeper.db` (AD-3, NFR-8).
//!
//! `keeper.db` is a WAL-mode SQLite database at `<data_dir>/keeper.db` holding
//! the `accounts` registry. It stores **only** non-secret fields — there is no
//! token column. Access tokens live exclusively in the macOS Keychain; the SDK
//! store lives under `accounts/<account_id>/sdk/`.
//!
//! All functions here are synchronous: a rusqlite [`Connection`] is never held
//! across an `.await`. Callers open, operate, and drop the connection within a
//! single synchronous scope.

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use crate::error::{CoreError, PlatformError};

/// Resolve the `keeper.db` path under a data directory.
fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("keeper.db")
}

/// Open `keeper.db` in WAL mode, ensuring the data dir and `accounts` schema
/// exist. Every call is idempotent (`CREATE TABLE IF NOT EXISTS`).
fn open(data_dir: &Path) -> Result<Connection, CoreError> {
    std::fs::create_dir_all(data_dir).map_err(|e| {
        CoreError::Platform(PlatformError::DirUnavailable(format!(
            "could not create data dir: {e}"
        )))
    })?;
    let conn = Connection::open(db_path(data_dir))
        .map_err(|e| CoreError::Internal(format!("could not open keeper.db: {e}")))?;
    // WAL for crash resilience (NFR-8). `pragma_update` runs the PRAGMA.
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| CoreError::Internal(format!("could not set WAL mode: {e}")))?;
    conn.execute(
        "CREATE TABLE IF NOT EXISTS accounts(\
            account_id TEXT PRIMARY KEY, \
            user_id TEXT NOT NULL, \
            homeserver_url TEXT NOT NULL, \
            device_id TEXT NOT NULL, \
            created_ts INTEGER NOT NULL\
        )",
        [],
    )
    .map_err(|e| CoreError::Internal(format!("could not ensure accounts schema: {e}")))?;
    Ok(conn)
}

/// A single non-secret account row from the registry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountRow {
    /// Opaque keeper account id (ULID).
    pub account_id: String,
    /// Matrix user id.
    pub user_id: String,
    /// Resolved homeserver base URL.
    pub homeserver_url: String,
    /// Matrix device id issued at login.
    pub device_id: String,
    /// Creation time in milliseconds since the Unix epoch (UTC).
    pub created_ts: i64,
}

/// Insert one account row. Fails if `account_id` already exists (PRIMARY KEY).
pub fn insert_account(
    data_dir: &Path,
    account_id: &str,
    user_id: &str,
    homeserver_url: &str,
    device_id: &str,
    created_ts: i64,
) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    conn.execute(
        "INSERT INTO accounts(account_id, user_id, homeserver_url, device_id, created_ts) \
         VALUES (?1, ?2, ?3, ?4, ?5)",
        rusqlite::params![account_id, user_id, homeserver_url, device_id, created_ts],
    )
    .map_err(|e| CoreError::Internal(format!("could not insert account row: {e}")))?;
    Ok(())
}

/// Delete an account row by id. Idempotent — deleting a missing row is not an
/// error, so this is safe to call from the login rollback path.
pub fn delete_account(data_dir: &Path, account_id: &str) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    conn.execute(
        "DELETE FROM accounts WHERE account_id = ?1",
        rusqlite::params![account_id],
    )
    .map_err(|e| CoreError::Internal(format!("could not delete account row: {e}")))?;
    Ok(())
}

/// Fetch a single account row by id, if present.
pub fn get_account(data_dir: &Path, account_id: &str) -> Result<Option<AccountRow>, CoreError> {
    let conn = open(data_dir)?;
    let row = conn
        .query_row(
            "SELECT account_id, user_id, homeserver_url, device_id, created_ts \
             FROM accounts WHERE account_id = ?1",
            rusqlite::params![account_id],
            |r| {
                Ok(AccountRow {
                    account_id: r.get(0)?,
                    user_id: r.get(1)?,
                    homeserver_url: r.get(2)?,
                    device_id: r.get(3)?,
                    created_ts: r.get(4)?,
                })
            },
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(CoreError::Internal(format!(
                "could not read account row: {other}"
            ))),
        })?;
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        let mut dir = std::env::temp_dir();
        let unique = format!(
            "keeper-registry-test-{}-{}",
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
    fn insert_read_back_and_delete_round_trip() {
        let dir = temp_dir();

        insert_account(
            &dir,
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "@alice:example.org",
            "https://matrix.example.org/",
            "DEVID123",
            1_720_000_000_000,
        )
        .expect("insert should succeed");

        let row = get_account(&dir, "01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .expect("read should succeed")
            .expect("row should exist");
        assert_eq!(row.user_id, "@alice:example.org");
        assert_eq!(row.homeserver_url, "https://matrix.example.org/");
        assert_eq!(row.device_id, "DEVID123");
        assert_eq!(row.created_ts, 1_720_000_000_000);

        delete_account(&dir, "01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("delete should succeed");
        let gone = get_account(&dir, "01ARZ3NDEKTSV4RRFFQ69G5FAV").expect("read after delete");
        assert!(gone.is_none(), "row should be gone after delete");

        // Cleanup best-effort.
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_missing_row_is_not_an_error() {
        let dir = temp_dir();
        // No insert; deleting a non-existent row must succeed (rollback safety).
        delete_account(&dir, "does-not-exist").expect("delete of missing row should be ok");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn db_uses_wal_journal_mode() {
        let dir = temp_dir();
        insert_account(
            &dir,
            "01BX5ZZKBKACTAV9WEVGEMMVRZ",
            "@bob:example.org",
            "https://matrix.example.org/",
            "DEVID456",
            1,
        )
        .expect("insert should succeed");

        let conn = Connection::open(db_path(&dir)).expect("reopen db");
        let mode: String = conn
            .query_row("PRAGMA journal_mode", [], |r| r.get(0))
            .expect("read journal_mode");
        assert_eq!(mode.to_lowercase(), "wal");
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
