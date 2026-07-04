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

/// Total number of hues on the per-account hue wheel (0..8).
pub const HUE_WHEEL_SIZE: u8 = 8;

/// Open `keeper.db` in WAL mode, ensuring the data dir and `accounts` schema
/// exist. Every call is idempotent (`CREATE TABLE IF NOT EXISTS`).
///
/// Runs a non-destructive, idempotent migration that adds the nullable
/// `hue_index` column to a pre-existing `accounts` table (Story 2.1). A row
/// created before this column existed keeps `NULL` until it is backfilled; no
/// existing row is ever dropped or rewritten destructively (spec Block-If).
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
    ensure_hue_index_column(&conn)?;
    Ok(conn)
}

/// Add the nullable `hue_index` column to `accounts` if it is not present yet.
///
/// Idempotent and non-destructive: reads the table's column list and only runs
/// `ALTER TABLE ... ADD COLUMN` when `hue_index` is missing, so an install that
/// predates the column upgrades in place without dropping any account row.
fn ensure_hue_index_column(conn: &Connection) -> Result<(), CoreError> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(accounts)")
        .map_err(|e| CoreError::Internal(format!("could not inspect accounts schema: {e}")))?;
    let existing: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|e| CoreError::Internal(format!("could not read accounts columns: {e}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| CoreError::Internal(format!("could not read accounts columns: {e}")))?;
    drop(stmt);
    if !existing.iter().any(|c| c == "hue_index") {
        conn.execute("ALTER TABLE accounts ADD COLUMN hue_index INTEGER", [])
            .map_err(|e| CoreError::Internal(format!("could not add hue_index column: {e}")))?;
    }
    Ok(())
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
    /// Per-account hue index (0..8), or `None` for a legacy row created before
    /// the hue column existed and not yet backfilled.
    pub hue_index: Option<u8>,
}

/// Insert one account row with its assigned hue index. Fails if `account_id`
/// already exists (PRIMARY KEY).
pub fn insert_account(
    data_dir: &Path,
    account_id: &str,
    user_id: &str,
    homeserver_url: &str,
    device_id: &str,
    created_ts: i64,
    hue_index: u8,
) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    conn.execute(
        "INSERT INTO accounts(account_id, user_id, homeserver_url, device_id, created_ts, hue_index) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            account_id,
            user_id,
            homeserver_url,
            device_id,
            created_ts,
            hue_index as i64
        ],
    )
    .map_err(|e| CoreError::Internal(format!("could not insert account row: {e}")))?;
    Ok(())
}

/// Choose the hue index to assign to a new account: the lowest index in
/// `0..HUE_WHEEL_SIZE` not currently in use, or — when all eight are taken —
/// `total_count % HUE_WHEEL_SIZE` (spec I/O matrix). Pure over the set of
/// already-used indices and the current account count.
fn choose_hue_index(used: &[u8], total_count: usize) -> u8 {
    for candidate in 0..HUE_WHEEL_SIZE {
        if !used.contains(&candidate) {
            return candidate;
        }
    }
    (total_count % HUE_WHEEL_SIZE as usize) as u8
}

/// Assign the next hue index for a new account: read the hue indices already in
/// use, pick the lowest unused in `0..8`, else `count % 8`. Reads the registry
/// (creating it if absent), so it is safe to call before the new row is written.
pub fn next_hue_index(data_dir: &Path) -> Result<u8, CoreError> {
    let rows = list_accounts(data_dir)?;
    let used: Vec<u8> = rows.iter().filter_map(|r| r.hue_index).collect();
    Ok(choose_hue_index(&used, rows.len()))
}

/// Backfill a `NULL` hue index for a legacy account row, assigning it the next
/// available hue (idempotent: a row that already has a hue is left untouched).
/// Returns the row's effective hue index.
pub fn backfill_hue_index(data_dir: &Path, account_id: &str) -> Result<u8, CoreError> {
    if let Some(row) = get_account(data_dir, account_id)? {
        if let Some(hue) = row.hue_index {
            return Ok(hue);
        }
    }
    let hue = next_hue_index(data_dir)?;
    let conn = open(data_dir)?;
    conn.execute(
        "UPDATE accounts SET hue_index = ?1 WHERE account_id = ?2 AND hue_index IS NULL",
        rusqlite::params![hue as i64, account_id],
    )
    .map_err(|e| CoreError::Internal(format!("could not backfill hue_index: {e}")))?;
    Ok(hue)
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

/// List every account row in the registry, in insertion order.
///
/// Returns an empty vector when the registry has no rows (a cold, never-signed-in
/// install). Used by the session-restore path to find a persisted account.
pub fn list_accounts(data_dir: &Path) -> Result<Vec<AccountRow>, CoreError> {
    let conn = open(data_dir)?;
    let mut stmt = conn
        .prepare(
            "SELECT account_id, user_id, homeserver_url, device_id, created_ts, hue_index \
             FROM accounts ORDER BY created_ts ASC",
        )
        .map_err(|e| CoreError::Internal(format!("could not prepare account list: {e}")))?;
    let rows = stmt
        .query_map([], |r| {
            Ok(AccountRow {
                account_id: r.get(0)?,
                user_id: r.get(1)?,
                homeserver_url: r.get(2)?,
                device_id: r.get(3)?,
                created_ts: r.get(4)?,
                hue_index: r.get::<_, Option<i64>>(5)?.map(|h| h as u8),
            })
        })
        .map_err(|e| CoreError::Internal(format!("could not query account list: {e}")))?;
    let mut accounts = Vec::new();
    for row in rows {
        accounts.push(
            row.map_err(|e| CoreError::Internal(format!("could not read account row: {e}")))?,
        );
    }
    Ok(accounts)
}

/// Fetch a single account row by id, if present.
pub fn get_account(data_dir: &Path, account_id: &str) -> Result<Option<AccountRow>, CoreError> {
    let conn = open(data_dir)?;
    let row = conn
        .query_row(
            "SELECT account_id, user_id, homeserver_url, device_id, created_ts, hue_index \
             FROM accounts WHERE account_id = ?1",
            rusqlite::params![account_id],
            |r| {
                Ok(AccountRow {
                    account_id: r.get(0)?,
                    user_id: r.get(1)?,
                    homeserver_url: r.get(2)?,
                    device_id: r.get(3)?,
                    created_ts: r.get(4)?,
                    hue_index: r.get::<_, Option<i64>>(5)?.map(|h| h as u8),
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
            0,
        )
        .expect("insert should succeed");

        let row = get_account(&dir, "01ARZ3NDEKTSV4RRFFQ69G5FAV")
            .expect("read should succeed")
            .expect("row should exist");
        assert_eq!(row.user_id, "@alice:example.org");
        assert_eq!(row.homeserver_url, "https://matrix.example.org/");
        assert_eq!(row.device_id, "DEVID123");
        assert_eq!(row.created_ts, 1_720_000_000_000);
        assert_eq!(row.hue_index, Some(0));

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
    fn list_accounts_empty_then_returns_inserted_rows() {
        let dir = temp_dir();

        // Empty registry lists nothing.
        let empty = list_accounts(&dir).expect("list on empty registry");
        assert!(empty.is_empty(), "fresh registry should list no accounts");

        insert_account(
            &dir,
            "01ARZ3NDEKTSV4RRFFQ69G5FAV",
            "@alice:example.org",
            "https://matrix.example.org/",
            "DEVID123",
            1,
            0,
        )
        .expect("insert first");
        insert_account(
            &dir,
            "01BX5ZZKBKACTAV9WEVGEMMVRZ",
            "@bob:example.org",
            "https://matrix.example.org/",
            "DEVID456",
            2,
            1,
        )
        .expect("insert second");

        let rows = list_accounts(&dir).expect("list two rows");
        assert_eq!(rows.len(), 2);
        // Ordered by created_ts ascending.
        assert_eq!(rows[0].account_id, "01ARZ3NDEKTSV4RRFFQ69G5FAV");
        assert_eq!(rows[0].user_id, "@alice:example.org");
        assert_eq!(rows[1].account_id, "01BX5ZZKBKACTAV9WEVGEMMVRZ");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn choose_hue_picks_lowest_unused_then_wraps_when_full() {
        // Lowest unused with a gap.
        assert_eq!(choose_hue_index(&[0, 1, 3], 3), 2);
        // Empty registry → 0.
        assert_eq!(choose_hue_index(&[], 0), 0);
        // All eight in use → total_count % 8 (9 accounts → hue 1).
        assert_eq!(choose_hue_index(&[0, 1, 2, 3, 4, 5, 6, 7], 9), 1);
    }

    #[test]
    fn next_hue_index_assigns_lowest_unused_across_inserts() {
        let dir = temp_dir();
        // Fresh registry → hue 0.
        assert_eq!(next_hue_index(&dir).expect("next"), 0);
        insert_account(&dir, "a", "@a:e.org", "https://e.org/", "D", 1, 0).expect("insert a");
        // hue 0 in use → next is 1.
        assert_eq!(next_hue_index(&dir).expect("next"), 1);
        insert_account(&dir, "b", "@b:e.org", "https://e.org/", "D", 2, 1).expect("insert b");
        assert_eq!(next_hue_index(&dir).expect("next"), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hue_assignment_reuses_freed_index_after_removal() {
        let dir = temp_dir();
        insert_account(&dir, "a", "@a:e.org", "https://e.org/", "D", 1, 0).expect("insert a");
        insert_account(&dir, "b", "@b:e.org", "https://e.org/", "D", 2, 1).expect("insert b");
        // Free hue 0.
        delete_account(&dir, "a").expect("delete a");
        // The lowest unused is now 0 again.
        assert_eq!(next_hue_index(&dir).expect("next"), 0);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn migration_adds_hue_column_to_legacy_table_without_dropping_rows() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).expect("create dir");
        // Create a pre-hue `accounts` table and a row, exactly as an Epic 1
        // install would have on disk (no hue_index column).
        {
            let conn = Connection::open(db_path(&dir)).expect("open legacy db");
            conn.execute(
                "CREATE TABLE accounts(\
                    account_id TEXT PRIMARY KEY, \
                    user_id TEXT NOT NULL, \
                    homeserver_url TEXT NOT NULL, \
                    device_id TEXT NOT NULL, \
                    created_ts INTEGER NOT NULL\
                )",
                [],
            )
            .expect("create legacy table");
            conn.execute(
                "INSERT INTO accounts(account_id, user_id, homeserver_url, device_id, created_ts) \
                 VALUES ('legacy', '@old:e.org', 'https://e.org/', 'DEV', 1)",
                [],
            )
            .expect("insert legacy row");
        }

        // The next `open` (via list) migrates in place: the legacy row survives
        // with a NULL hue.
        let rows = list_accounts(&dir).expect("list after migration");
        assert_eq!(rows.len(), 1, "legacy row must survive migration");
        assert_eq!(rows[0].account_id, "legacy");
        assert_eq!(rows[0].hue_index, None, "legacy row hue starts NULL");

        // Backfill assigns the next hue and is idempotent.
        let hue = backfill_hue_index(&dir, "legacy").expect("backfill");
        assert_eq!(hue, 0);
        let again = backfill_hue_index(&dir, "legacy").expect("backfill idempotent");
        assert_eq!(again, 0);
        let row = get_account(&dir, "legacy")
            .expect("get")
            .expect("row present");
        assert_eq!(row.hue_index, Some(0));
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
            0,
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
