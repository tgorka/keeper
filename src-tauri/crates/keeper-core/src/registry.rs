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
use crate::vm::DockBadgeMode;

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
    // App-wide key/value settings (Story 2.6). Holds the non-secret `sdk_encryption`
    // posture; never any secret material (passphrases live only in the Keychain).
    conn.execute(
        "CREATE TABLE IF NOT EXISTS settings(\
            key TEXT PRIMARY KEY, \
            value TEXT NOT NULL\
        )",
        [],
    )
    .map_err(|e| CoreError::Internal(format!("could not ensure settings schema: {e}")))?;
    // Local pin membership + user-controlled order (Story 4.3). Pins have no
    // Matrix representation (no standard *notable* tag), so they persist locally,
    // keyed by (account, room), ordered by `sort_order` ascending across accounts.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS pins(\
            account_id TEXT NOT NULL, \
            room_id TEXT NOT NULL, \
            sort_order INTEGER NOT NULL, \
            PRIMARY KEY(account_id, room_id)\
        )",
        [],
    )
    .map_err(|e| CoreError::Internal(format!("could not ensure pins schema: {e}")))?;
    // Persistent per-chat composer drafts (Story 7.1, AD-15). Unsent text is durable,
    // keyed by (account, room), so switching chats / force-quitting / crashing never
    // loses a half-written message. Never any secret material; draft bodies are never
    // logged. Mirrors the `pins` precedent for per-(account, room) keeper-local state.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS drafts(\
            account_id TEXT NOT NULL, \
            room_id TEXT NOT NULL, \
            body TEXT NOT NULL, \
            updated_ts INTEGER NOT NULL, \
            PRIMARY KEY(account_id, room_id)\
        )",
        [],
    )
    .map_err(|e| CoreError::Internal(format!("could not ensure drafts schema: {e}")))?;
    // Per-chat Incognito override (Story 8.1). Tri-state: a present row's `enabled`
    // (0/1) overrides the account/global scopes for `(account, room)`; an absent row
    // means "inherit the next-broader scope". Mirrors the `drafts`/`pins` precedent
    // for per-(account, room) keeper-local state. Never any secret material.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS chat_incognito(\
            account_id TEXT NOT NULL, \
            room_id TEXT NOT NULL, \
            enabled INTEGER NOT NULL, \
            PRIMARY KEY(account_id, room_id)\
        )",
        [],
    )
    .map_err(|e| CoreError::Internal(format!("could not ensure chat_incognito schema: {e}")))?;
    // Persistent held-send outbox (Story 8.3, Undo-Send Window). An approved send with
    // a positive Undo-Send window is written here instead of the SDK send queue, then
    // dispatched by the per-account scheduler once `dispatch_at_ts` elapses. Durable in
    // WAL so a crash/restart never silently loses a held message (NFR-8). Unlike drafts
    // there can be MANY rows per (account, room), so the primary key is a unique `id`
    // (a fresh `TransactionId`), not `(account, room)`. Bodies are never logged.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS outbox(\
            id TEXT PRIMARY KEY, \
            account_id TEXT NOT NULL, \
            room_id TEXT NOT NULL, \
            body TEXT NOT NULL, \
            held_at_ts INTEGER NOT NULL, \
            dispatch_at_ts INTEGER NOT NULL\
        )",
        [],
    )
    .map_err(|e| CoreError::Internal(format!("could not ensure outbox schema: {e}")))?;
    // Per-Network mute set (Story 10.2, FR-52). A present row mutes every Chat bridged
    // to that Network's label across all accounts; an absent row means "not muted".
    // Matrix has no "network" concept, so this is keeper-local (evaluated in the notify
    // decision and at inbox emit). Keyed by the Network's display label — the same
    // cross-account identifier the Networks sidebar selects on. Never any secret material.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS muted_networks(\
            network_id TEXT PRIMARY KEY\
        )",
        [],
    )
    .map_err(|e| CoreError::Internal(format!("could not ensure muted_networks schema: {e}")))?;
    ensure_hue_index_column(&conn)?;
    ensure_provider_column(&conn)?;
    ensure_incognito_column(&conn)?;
    Ok(conn)
}

/// Read a single settings value by key, or `None` when unset.
///
/// Non-secret key/value store in `keeper.db` (Story 2.6). Never holds secret
/// material.
pub fn get_setting(data_dir: &Path, key: &str) -> Result<Option<String>, CoreError> {
    let conn = open(data_dir)?;
    let value = conn
        .query_row(
            "SELECT value FROM settings WHERE key = ?1",
            rusqlite::params![key],
            |r| r.get::<_, String>(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(CoreError::Internal(format!(
                "could not read setting: {other}"
            ))),
        })?;
    Ok(value)
}

/// Write (insert or overwrite) a single settings value by key.
pub fn set_setting(data_dir: &Path, key: &str, value: &str) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        rusqlite::params![key, value],
    )
    .map_err(|e| CoreError::Internal(format!("could not write setting: {e}")))?;
    Ok(())
}

/// Upsert a pin for `(account_id, room_id)` with the given `sort_order` (Story
/// 4.3). Idempotent per key: a repeated pin overwrites the stored order. Pins are
/// keeper-local because Matrix has no standard *notable* pin tag.
pub fn set_pin(
    data_dir: &Path,
    account_id: &str,
    room_id: &str,
    order: i64,
) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    conn.execute(
        "INSERT INTO pins(account_id, room_id, sort_order) VALUES (?1, ?2, ?3) \
         ON CONFLICT(account_id, room_id) DO UPDATE SET sort_order = excluded.sort_order",
        rusqlite::params![account_id, room_id, order],
    )
    .map_err(|e| CoreError::Internal(format!("could not write pin: {e}")))?;
    Ok(())
}

/// Remove the pin for `(account_id, room_id)` if present (Story 4.3). Idempotent —
/// unpinning an unpinned room is not an error.
pub fn remove_pin(data_dir: &Path, account_id: &str, room_id: &str) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    conn.execute(
        "DELETE FROM pins WHERE account_id = ?1 AND room_id = ?2",
        rusqlite::params![account_id, room_id],
    )
    .map_err(|e| CoreError::Internal(format!("could not remove pin: {e}")))?;
    Ok(())
}

/// List every pin as `(account_id, room_id, sort_order)`, ordered by `sort_order`
/// ascending (Story 4.3). Order is global across accounts — the Pins strip merges
/// pinned rooms from all accounts into one user-controlled sequence. Returns an
/// empty vector when nothing is pinned.
pub fn get_pins(data_dir: &Path) -> Result<Vec<(String, String, i64)>, CoreError> {
    let conn = open(data_dir)?;
    let mut stmt = conn
        .prepare("SELECT account_id, room_id, sort_order FROM pins ORDER BY sort_order ASC")
        .map_err(|e| CoreError::Internal(format!("could not prepare pin list: {e}")))?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, i64>(2)?,
            ))
        })
        .map_err(|e| CoreError::Internal(format!("could not query pin list: {e}")))?;
    let mut pins = Vec::new();
    for row in rows {
        pins.push(row.map_err(|e| CoreError::Internal(format!("could not read pin row: {e}")))?);
    }
    Ok(pins)
}

/// Upsert the composer draft for `(account_id, room_id)` with the given `body` and
/// `updated_ts` (Story 7.1). Idempotent per key: a repeated save overwrites the stored
/// body. Drafts are keeper-local pre-send state (no Matrix representation, no
/// cross-device mirror). The body is never logged.
pub fn set_draft(
    data_dir: &Path,
    account_id: &str,
    room_id: &str,
    body: &str,
    updated_ts: i64,
) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    conn.execute(
        "INSERT INTO drafts(account_id, room_id, body, updated_ts) VALUES (?1, ?2, ?3, ?4) \
         ON CONFLICT(account_id, room_id) DO UPDATE SET \
            body = excluded.body, updated_ts = excluded.updated_ts",
        rusqlite::params![account_id, room_id, body, updated_ts],
    )
    .map_err(|e| CoreError::Internal(format!("could not write draft: {e}")))?;
    Ok(())
}

/// Read the composer draft body for `(account_id, room_id)`, or `None` when no draft
/// is stored (Story 7.1). The composer seeds its local state from this on mount.
pub fn get_draft(
    data_dir: &Path,
    account_id: &str,
    room_id: &str,
) -> Result<Option<String>, CoreError> {
    let conn = open(data_dir)?;
    let body = conn
        .query_row(
            "SELECT body FROM drafts WHERE account_id = ?1 AND room_id = ?2",
            rusqlite::params![account_id, room_id],
            |r| r.get::<_, String>(0),
        )
        .map(Some)
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(CoreError::Internal(format!(
                "could not read draft: {other}"
            ))),
        })?;
    Ok(body)
}

/// Remove the composer draft for `(account_id, room_id)` if present (Story 7.1).
/// Idempotent — deleting an absent draft (send succeeded, or the body trimmed to
/// empty) is not an error.
pub fn delete_draft(data_dir: &Path, account_id: &str, room_id: &str) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    conn.execute(
        "DELETE FROM drafts WHERE account_id = ?1 AND room_id = ?2",
        rusqlite::params![account_id, room_id],
    )
    .map_err(|e| CoreError::Internal(format!("could not delete draft: {e}")))?;
    Ok(())
}

/// List every draft's `(account_id, room_id)` key (Story 7.1). Presence only — the
/// body is not returned, so the startup marker seed stays small. Cross-account, over
/// the whole table. Returns an empty vector when nothing is drafted.
pub fn list_drafts(data_dir: &Path) -> Result<Vec<(String, String)>, CoreError> {
    let conn = open(data_dir)?;
    let mut stmt = conn
        .prepare("SELECT account_id, room_id FROM drafts")
        .map_err(|e| CoreError::Internal(format!("could not prepare draft list: {e}")))?;
    let rows = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?)))
        .map_err(|e| CoreError::Internal(format!("could not query draft list: {e}")))?;
    let mut drafts = Vec::new();
    for row in rows {
        drafts
            .push(row.map_err(|e| CoreError::Internal(format!("could not read draft row: {e}")))?);
    }
    Ok(drafts)
}

/// List every draft as a full row `(account_id, room_id, body, updated_ts)` across
/// all accounts (Story 7.3, approval pane). Unlike [`list_drafts`] (keys only), this
/// carries the authoritative body and timestamp so the approval pane can render each
/// pending draft. Cross-account, over the whole table. Returns an empty vector when
/// nothing is drafted. The body is never logged.
///
/// A deterministic `ORDER BY account_id, updated_ts, room_id` is applied so the
/// grouped pane and its single roving tab-stop keep a stable order across re-queries
/// (a bare `SELECT` has unspecified SQLite row order).
pub fn list_draft_rows(data_dir: &Path) -> Result<Vec<(String, String, String, i64)>, CoreError> {
    let conn = open(data_dir)?;
    let mut stmt = conn
        .prepare(
            "SELECT account_id, room_id, body, updated_ts FROM drafts \
             ORDER BY account_id, updated_ts, room_id",
        )
        .map_err(|e| CoreError::Internal(format!("could not prepare draft-row list: {e}")))?;
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, String>(2)?,
                r.get::<_, i64>(3)?,
            ))
        })
        .map_err(|e| CoreError::Internal(format!("could not query draft-row list: {e}")))?;
    let mut drafts = Vec::new();
    for row in rows {
        drafts
            .push(row.map_err(|e| CoreError::Internal(format!("could not read draft-row: {e}")))?);
    }
    Ok(drafts)
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

/// Add the nullable `provider` column to `accounts` if it is not present yet
/// (Story 2.5).
///
/// Idempotent and non-destructive, exactly like [`ensure_hue_index_column`]:
/// reads the table's column list and only runs `ALTER TABLE ... ADD COLUMN` when
/// `provider` is missing, so an install that predates the column upgrades in
/// place without dropping any account row. A row created before this column
/// existed keeps `NULL` until [`backfill_provider`] infers and persists its tag.
fn ensure_provider_column(conn: &Connection) -> Result<(), CoreError> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(accounts)")
        .map_err(|e| CoreError::Internal(format!("could not inspect accounts schema: {e}")))?;
    let existing: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|e| CoreError::Internal(format!("could not read accounts columns: {e}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| CoreError::Internal(format!("could not read accounts columns: {e}")))?;
    drop(stmt);
    if !existing.iter().any(|c| c == "provider") {
        conn.execute("ALTER TABLE accounts ADD COLUMN provider TEXT", [])
            .map_err(|e| CoreError::Internal(format!("could not add provider column: {e}")))?;
    }
    Ok(())
}

/// Add the nullable `incognito` column to `accounts` if it is not present yet
/// (Story 8.1).
///
/// Idempotent and non-destructive, exactly like [`ensure_hue_index_column`]:
/// reads the table's column list and only runs `ALTER TABLE ... ADD COLUMN` when
/// `incognito` is missing, so an install that predates the column upgrades in place
/// without dropping any account row. The column is tri-state: `NULL` = inherit the
/// global scope, `0`/`1` = a per-Account override.
fn ensure_incognito_column(conn: &Connection) -> Result<(), CoreError> {
    let mut stmt = conn
        .prepare("PRAGMA table_info(accounts)")
        .map_err(|e| CoreError::Internal(format!("could not inspect accounts schema: {e}")))?;
    let existing: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))
        .map_err(|e| CoreError::Internal(format!("could not read accounts columns: {e}")))?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| CoreError::Internal(format!("could not read accounts columns: {e}")))?;
    drop(stmt);
    if !existing.iter().any(|c| c == "incognito") {
        conn.execute("ALTER TABLE accounts ADD COLUMN incognito INTEGER", [])
            .map_err(|e| CoreError::Internal(format!("could not add incognito column: {e}")))?;
    }
    Ok(())
}

/// The `settings` key holding the global Incognito default (Story 8.1). Stored as
/// `"1"`/`"0"`; absent = off (Incognito off by default).
const INCOGNITO_GLOBAL_KEY: &str = "incognito.global";

/// Read the global Incognito default (Story 8.1). Absent / unparsable ⇒ `false`
/// (off by default). Stored in the `settings` k/v table under `incognito.global`.
pub fn get_incognito_global(data_dir: &Path) -> Result<bool, CoreError> {
    Ok(get_setting(data_dir, INCOGNITO_GLOBAL_KEY)?.as_deref() == Some("1"))
}

/// Write the global Incognito default (Story 8.1). Persists `"1"`/`"0"` into the
/// `settings` k/v table under `incognito.global`.
pub fn set_incognito_global(data_dir: &Path, enabled: bool) -> Result<(), CoreError> {
    set_setting(
        data_dir,
        INCOGNITO_GLOBAL_KEY,
        if enabled { "1" } else { "0" },
    )
}

/// The `settings` key holding the "message previews" toggle (Story 10.1). Stored as
/// `"1"`/`"0"`; absent = on (previews enabled by default).
const NOTIFY_PREVIEWS_KEY: &str = "notify.previews_enabled";

/// Read the "message previews" toggle (Story 10.1). Absent ⇒ `true` (previews enabled
/// by default). Stored in the `settings` k/v table under `notify.previews_enabled`.
pub fn get_notify_previews(data_dir: &Path) -> Result<bool, CoreError> {
    // Default-on: only an explicit `"0"` disables previews; absent/anything-else is on.
    Ok(get_setting(data_dir, NOTIFY_PREVIEWS_KEY)?.as_deref() != Some("0"))
}

/// Write the "message previews" toggle (Story 10.1). Persists `"1"`/`"0"` into the
/// `settings` k/v table under `notify.previews_enabled`.
pub fn set_notify_previews(data_dir: &Path, enabled: bool) -> Result<(), CoreError> {
    set_setting(
        data_dir,
        NOTIFY_PREVIEWS_KEY,
        if enabled { "1" } else { "0" },
    )
}

/// The `settings` key holding the global Do-Not-Disturb switch (Story 10.2). Stored
/// as `"1"`/`"0"`; absent = off (DND off by default, so notifications post normally).
const NOTIFY_DND_GLOBAL_KEY: &str = "notify.dnd_global";

/// Read the global Do-Not-Disturb switch (Story 10.2). Absent / anything-but-`"1"` ⇒
/// `false` (off by default). Stored in the `settings` k/v table under
/// `notify.dnd_global`. When on, the notify decision silences every account/Chat while
/// unread still accrues everywhere.
pub fn get_dnd_global(data_dir: &Path) -> Result<bool, CoreError> {
    Ok(get_setting(data_dir, NOTIFY_DND_GLOBAL_KEY)?.as_deref() == Some("1"))
}

/// Write the global Do-Not-Disturb switch (Story 10.2). Persists `"1"`/`"0"` into the
/// `settings` k/v table under `notify.dnd_global`.
pub fn set_dnd_global(data_dir: &Path, enabled: bool) -> Result<(), CoreError> {
    set_setting(
        data_dir,
        NOTIFY_DND_GLOBAL_KEY,
        if enabled { "1" } else { "0" },
    )
}

/// The `settings` key holding the dock-badge mode (Story 10.3). Stored as the mode's
/// registry string (`"all"`/`"mentions"`/`"off"`); absent = `all` (badge all unreads
/// by default).
const NOTIFY_DOCK_BADGE_MODE_KEY: &str = "notify.dock_badge_mode";

/// Read the dock-badge mode (Story 10.3, FR-53). Absent / unparsable ⇒
/// [`DockBadgeMode::All`] (badge all unreads by default). Stored in the `settings` k/v
/// table under `notify.dock_badge_mode`.
pub fn get_dock_badge_mode(data_dir: &Path) -> Result<DockBadgeMode, CoreError> {
    match get_setting(data_dir, NOTIFY_DOCK_BADGE_MODE_KEY)? {
        Some(value) => Ok(DockBadgeMode::from_registry_str(&value)),
        None => Ok(DockBadgeMode::All),
    }
}

/// Write the dock-badge mode (Story 10.3, FR-53). Persists the mode's registry string
/// into the `settings` k/v table under `notify.dock_badge_mode`.
pub fn set_dock_badge_mode(data_dir: &Path, mode: DockBadgeMode) -> Result<(), CoreError> {
    set_setting(data_dir, NOTIFY_DOCK_BADGE_MODE_KEY, mode.as_registry_str())
}

/// The `settings` key holding the one-time iOS no-background-sync disclosure latch
/// (Story 14.2). Stored as `"1"` once the card has been shown; absent = not yet shown.
const UI_IOS_SYNC_DISCLOSURE_SHOWN_KEY: &str = "ui.ios_sync_disclosure_shown";

/// Read whether the one-time iOS no-background-sync disclosure has been shown
/// (Story 14.2, FR-61). Present `"1"` ⇒ `true`; absent ⇒ `false` (not yet shown).
/// Device-global — the disclosure is about the platform, not an Account. Stored in
/// the `settings` k/v table under `ui.ios_sync_disclosure_shown`.
pub fn get_ios_sync_disclosure_shown(data_dir: &Path) -> Result<bool, CoreError> {
    Ok(get_setting(data_dir, UI_IOS_SYNC_DISCLOSURE_SHOWN_KEY)?.as_deref() == Some("1"))
}

/// Latch the one-time iOS no-background-sync disclosure as shown (Story 14.2, FR-61).
/// Writes `"1"` into the `settings` k/v table under `ui.ios_sync_disclosure_shown`.
/// One-way — there is no unset; once acknowledged the card never re-appears.
pub fn set_ios_sync_disclosure_shown(data_dir: &Path) -> Result<(), CoreError> {
    set_setting(data_dir, UI_IOS_SYNC_DISCLOSURE_SHOWN_KEY, "1")
}

/// The `settings` key holding the recovered-session acknowledgement seen-set
/// (Story 20.3, FR-73). A JSON array of session-folder **basenames** — every
/// crash-recovered session the user has already been shown-and-dismissed. Absent
/// ⇒ nothing acknowledged yet. Mirrors the one-time
/// [`UI_IOS_SYNC_DISCLOSURE_SHOWN_KEY`] latch, but keyed as a set so multiple
/// distinct recovered sessions each surface exactly once without overloading the
/// wire-stable manifest `status`.
const UI_RECOVERED_SESSIONS_ACKNOWLEDGED_KEY: &str = "ui.recovered_sessions_acknowledged";

/// Read the acknowledged recovered-session basenames (Story 20.3, FR-73). Absent
/// / unparseable ⇒ empty (nothing acknowledged — every recovered session is
/// still due to surface). Stored as a JSON array in the `settings` k/v table
/// under `ui.recovered_sessions_acknowledged`. The recovery-list scan filters
/// these out so each session shows exactly once across restarts.
pub fn get_recovered_sessions_acknowledged(data_dir: &Path) -> Result<Vec<String>, CoreError> {
    match get_setting(data_dir, UI_RECOVERED_SESSIONS_ACKNOWLEDGED_KEY)? {
        Some(raw) => match serde_json::from_str::<Vec<String>>(&raw) {
            Ok(acknowledged) => Ok(acknowledged),
            // A corrupt/legacy value must not silently evaporate the whole
            // seen-set (which would re-surface every recovered notice) without
            // a trace — log and degrade to "nothing acknowledged".
            Err(e) => {
                tracing::warn!(
                    error = %e,
                    "recovered-sessions acknowledgement set is malformed; treating as empty"
                );
                Ok(Vec::new())
            }
        },
        None => Ok(Vec::new()),
    }
}

/// Latch a recovered session's folder basename into the acknowledgement seen-set
/// (Story 20.3, FR-73). Idempotent: a basename already present is a no-op (the
/// stored array never accumulates duplicates), and the write is one-way — an
/// acknowledged session never re-surfaces. Reads the current set, adds `session`
/// if absent, and persists the JSON array back under
/// `ui.recovered_sessions_acknowledged`.
pub fn add_recovered_session_acknowledged(data_dir: &Path, session: &str) -> Result<(), CoreError> {
    let mut acknowledged = get_recovered_sessions_acknowledged(data_dir)?;
    if acknowledged.iter().any(|entry| entry == session) {
        return Ok(());
    }
    acknowledged.push(session.to_owned());
    let json = serde_json::to_string(&acknowledged).map_err(|e| {
        CoreError::Internal(format!(
            "could not serialize acknowledged recovered sessions: {e}"
        ))
    })?;
    set_setting(data_dir, UI_RECOVERED_SESSIONS_ACKNOWLEDGED_KEY, &json)
}

/// The `settings` key holding the opt-in menu-bar (tray) presence toggle (Story 10.3).
/// Stored as `"1"`/`"0"`; absent = off (no tray by default).
const SYSTEM_MENU_BAR_PRESENCE_KEY: &str = "system.menu_bar_presence";

/// Read the menu-bar presence toggle (Story 10.3, FR-53). Absent / anything-but-`"1"` ⇒
/// `false` (off by default — the tray is opt-in). Stored in the `settings` k/v table
/// under `system.menu_bar_presence`.
pub fn get_menu_bar_presence(data_dir: &Path) -> Result<bool, CoreError> {
    Ok(get_setting(data_dir, SYSTEM_MENU_BAR_PRESENCE_KEY)?.as_deref() == Some("1"))
}

/// Write the menu-bar presence toggle (Story 10.3, FR-53). Persists `"1"`/`"0"` into the
/// `settings` k/v table under `system.menu_bar_presence`.
pub fn set_menu_bar_presence(data_dir: &Path, enabled: bool) -> Result<(), CoreError> {
    set_setting(
        data_dir,
        SYSTEM_MENU_BAR_PRESENCE_KEY,
        if enabled { "1" } else { "0" },
    )
}

/// List every muted Network label (Story 10.2, FR-52). Returns the `network_id`
/// (display-label) of each present row; an empty vector means no Network is muted.
/// Sorted ascending for determinism. Keeper-local — Matrix has no Network concept.
pub fn get_muted_networks(data_dir: &Path) -> Result<Vec<String>, CoreError> {
    let conn = open(data_dir)?;
    let mut stmt = conn
        .prepare("SELECT network_id FROM muted_networks ORDER BY network_id ASC")
        .map_err(|e| CoreError::Internal(format!("could not prepare muted-networks list: {e}")))?;
    let rows = stmt
        .query_map([], |r| r.get::<_, String>(0))
        .map_err(|e| CoreError::Internal(format!("could not query muted-networks list: {e}")))?;
    let mut networks = Vec::new();
    for row in rows {
        networks.push(
            row.map_err(|e| CoreError::Internal(format!("could not read muted-network row: {e}")))?,
        );
    }
    Ok(networks)
}

/// Set (or clear) the muted state for a Network label (Story 10.2, FR-52). `true`
/// inserts the row (idempotent — re-muting is a no-op via `OR IGNORE`); `false`
/// deletes it (idempotent — unmuting an unmuted Network is not an error). Keyed by the
/// Network's display label in `muted_networks`.
pub fn set_network_muted(data_dir: &Path, network_id: &str, muted: bool) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    if muted {
        conn.execute(
            "INSERT OR IGNORE INTO muted_networks(network_id) VALUES (?1)",
            rusqlite::params![network_id],
        )
        .map_err(|e| CoreError::Internal(format!("could not mute network: {e}")))?;
    } else {
        conn.execute(
            "DELETE FROM muted_networks WHERE network_id = ?1",
            rusqlite::params![network_id],
        )
        .map_err(|e| CoreError::Internal(format!("could not unmute network: {e}")))?;
    }
    Ok(())
}

/// Whether a single Network label is currently muted (Story 10.2). A thin
/// convenience over [`get_muted_networks`] for the per-Network IPC getter.
pub fn is_network_muted(data_dir: &Path, network_id: &str) -> Result<bool, CoreError> {
    Ok(get_muted_networks(data_dir)?
        .iter()
        .any(|n| n == network_id))
}

/// Read the per-Account Incognito override for `account_id` (Story 8.1). `None` =
/// inherit the global scope; `Some(bool)` = an explicit per-Account override. Reads
/// the nullable `accounts.incognito` column; a missing account row also reads `None`.
pub fn get_incognito_account(data_dir: &Path, account_id: &str) -> Result<Option<bool>, CoreError> {
    let conn = open(data_dir)?;
    let value = conn
        .query_row(
            "SELECT incognito FROM accounts WHERE account_id = ?1",
            rusqlite::params![account_id],
            |r| r.get::<_, Option<i64>>(0),
        )
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(CoreError::Internal(format!(
                "could not read account incognito: {other}"
            ))),
        })?;
    Ok(value.map(|v| v != 0))
}

/// Write the per-Account Incognito override for `account_id` (Story 8.1). `Some(bool)`
/// sets an explicit override; `None` clears it back to inherit (writes `NULL`).
/// Updates the `accounts.incognito` column; a no-op when the account row is absent.
pub fn set_incognito_account(
    data_dir: &Path,
    account_id: &str,
    value: Option<bool>,
) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    let stored: Option<i64> = value.map(|b| if b { 1 } else { 0 });
    conn.execute(
        "UPDATE accounts SET incognito = ?2 WHERE account_id = ?1",
        rusqlite::params![account_id, stored],
    )
    .map_err(|e| CoreError::Internal(format!("could not write account incognito: {e}")))?;
    Ok(())
}

/// Read the per-Chat Incognito override for `(account_id, room_id)` (Story 8.1).
/// `None` = inherit the account/global scope (no row); `Some(bool)` = an explicit
/// per-Chat override. Reads the `chat_incognito` table.
pub fn get_incognito_chat(
    data_dir: &Path,
    account_id: &str,
    room_id: &str,
) -> Result<Option<bool>, CoreError> {
    let conn = open(data_dir)?;
    let value = conn
        .query_row(
            "SELECT enabled FROM chat_incognito WHERE account_id = ?1 AND room_id = ?2",
            rusqlite::params![account_id, room_id],
            |r| r.get::<_, i64>(0),
        )
        .map(|v| Some(v != 0))
        .or_else(|e| match e {
            rusqlite::Error::QueryReturnedNoRows => Ok(None),
            other => Err(CoreError::Internal(format!(
                "could not read chat incognito: {other}"
            ))),
        })?;
    Ok(value)
}

/// Write the per-Chat Incognito override for `(account_id, room_id)` (Story 8.1).
/// `Some(bool)` upserts an explicit override; `None` clears it back to inherit
/// (deletes the row). Keyed by `(account_id, room_id)` in `chat_incognito`.
pub fn set_incognito_chat(
    data_dir: &Path,
    account_id: &str,
    room_id: &str,
    value: Option<bool>,
) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    match value {
        Some(enabled) => {
            conn.execute(
                "INSERT INTO chat_incognito(account_id, room_id, enabled) VALUES (?1, ?2, ?3) \
                 ON CONFLICT(account_id, room_id) DO UPDATE SET enabled = excluded.enabled",
                rusqlite::params![account_id, room_id, i64::from(enabled)],
            )
            .map_err(|e| CoreError::Internal(format!("could not write chat incognito: {e}")))?;
        }
        None => {
            conn.execute(
                "DELETE FROM chat_incognito WHERE account_id = ?1 AND room_id = ?2",
                rusqlite::params![account_id, room_id],
            )
            .map_err(|e| CoreError::Internal(format!("could not clear chat incognito: {e}")))?;
        }
    }
    Ok(())
}

/// Read all three Incognito scope values for `(account_id, room_id)` in one call
/// (Story 8.1), returning `(chat, account, global)` ready to feed
/// `signals::resolve_incognito`. `chat`/`account` are tri-state (`None` = inherit);
/// `global` is the plain default. Read at receipt-emission time so the effective
/// policy is resolved from live state.
pub fn incognito_scopes(
    data_dir: &Path,
    account_id: &str,
    room_id: &str,
) -> Result<(Option<bool>, Option<bool>, bool), CoreError> {
    let chat = get_incognito_chat(data_dir, account_id, room_id)?;
    let account = get_incognito_account(data_dir, account_id)?;
    let global = get_incognito_global(data_dir)?;
    Ok((chat, account, global))
}

/// The `settings` key holding the OS-global summon hotkey accelerator (Story 9.4).
/// Stored as an opaque accelerator string (e.g. `"Control+Alt+Space"`); absent ⇒
/// [`DEFAULT_GLOBAL_HOTKEY`]. `keeper-core` never parses this string — accelerator
/// parsing/registration lives only in the `keeper` shell crate (core stays Tauri-free).
const HOTKEY_GLOBAL_KEY: &str = "hotkey.global";

/// The default OS-global summon hotkey accelerator when the setting is absent
/// (Story 9.4). `⌃⌥Space`. An opaque string to `keeper-core` — the shell parses it.
pub const DEFAULT_GLOBAL_HOTKEY: &str = "Control+Alt+Space";

/// Read the OS-global summon hotkey accelerator (Story 9.4). Absent ⇒ the default
/// [`DEFAULT_GLOBAL_HOTKEY`]. Stored in the `settings` k/v table under `hotkey.global`.
/// The value is an opaque accelerator string; `keeper-core` never parses it.
pub fn get_global_hotkey(data_dir: &Path) -> Result<String, CoreError> {
    Ok(get_setting(data_dir, HOTKEY_GLOBAL_KEY)?
        .unwrap_or_else(|| DEFAULT_GLOBAL_HOTKEY.to_owned()))
}

/// Write the OS-global summon hotkey accelerator (Story 9.4). Persists the opaque
/// accelerator string into the `settings` k/v table under `hotkey.global`. The shell
/// crate validates + registers with the OS *before* calling this; core only stores it.
pub fn set_global_hotkey(data_dir: &Path, accelerator: &str) -> Result<(), CoreError> {
    set_setting(data_dir, HOTKEY_GLOBAL_KEY, accelerator)
}

/// The `settings` key holding the optional OS-global Start/Stop Recording hotkey
/// accelerator (Story 20.4, FR-50). A **second, independent** binding — it never
/// touches the summon binding's `hotkey.global` key. Stored as an opaque
/// accelerator string; absent ⇒ the empty string = **unset** (the shell registers
/// nothing). `keeper-core` never parses it — parsing/registration live only in
/// the `keeper` shell crate (core stays Tauri-free).
const HOTKEY_RECORDING_KEY: &str = "hotkey.recording";

/// Read the OS-global Start/Stop Recording hotkey accelerator (Story 20.4).
/// Absent ⇒ the empty string, meaning **unset by default** — unlike the summon
/// hotkey there is no shipped default chord. Stored in the `settings` k/v table
/// under `hotkey.recording`; the value is opaque to core.
pub fn get_recording_hotkey(data_dir: &Path) -> Result<String, CoreError> {
    Ok(get_setting(data_dir, HOTKEY_RECORDING_KEY)?.unwrap_or_default())
}

/// Write the OS-global Start/Stop Recording hotkey accelerator (Story 20.4).
/// Persists the opaque accelerator string under `hotkey.recording`; the empty
/// string persists "unset" (the shell's clear path). The shell validates +
/// registers with the OS *before* calling this; core only stores it.
pub fn set_recording_hotkey(data_dir: &Path, accelerator: &str) -> Result<(), CoreError> {
    set_setting(data_dir, HOTKEY_RECORDING_KEY, accelerator)
}

/// The `settings` key holding the Undo-Send window in whole seconds (Story 8.3).
/// Stored as a decimal string; absent / unparsable ⇒ the default of 10 s.
const UNDO_SEND_WINDOW_KEY: &str = "undo_send.window";

/// The default Undo-Send window in seconds when the setting is absent or unparsable.
pub const UNDO_SEND_WINDOW_DEFAULT: u16 = 10;

/// The maximum Undo-Send window in seconds; values are clamped to `0..=60`.
pub const UNDO_SEND_WINDOW_MAX: u16 = 60;

/// Read the Undo-Send window in seconds (Story 8.3). Absent / unparsable ⇒ the
/// default of 10 s; a stored value is clamped to `0..=60` defensively. Stored in the
/// `settings` k/v table under `undo_send.window`.
pub fn get_undo_send_window(data_dir: &Path) -> Result<u16, CoreError> {
    let raw = get_setting(data_dir, UNDO_SEND_WINDOW_KEY)?;
    let secs = raw
        .as_deref()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(UNDO_SEND_WINDOW_DEFAULT);
    Ok(secs.min(UNDO_SEND_WINDOW_MAX))
}

/// Write the Undo-Send window in seconds (Story 8.3), clamping to `0..=60` before
/// persisting. Persists a decimal string into the `settings` k/v table under
/// `undo_send.window`.
pub fn set_undo_send_window(data_dir: &Path, secs: u16) -> Result<(), CoreError> {
    let clamped = secs.min(UNDO_SEND_WINDOW_MAX);
    set_setting(data_dir, UNDO_SEND_WINDOW_KEY, &clamped.to_string())
}

/// The `settings` key holding the recording segment size in decimal MB (Story
/// 17.5, FR-72). Stored as a decimal string; absent / unparsable ⇒ the default of
/// 500 MB. Passed to the `keeper-rec` sidecar as `segmentMB` on every `start`.
const RECORDING_SEGMENT_MB_KEY: &str = "recording.segment_mb";

/// The default recording segment size in MB when the setting is absent or
/// unparsable (Epic 17's authored default; the sidecar's own fallback matches).
pub const RECORDING_SEGMENT_MB_DEFAULT: u32 = 500;

/// The smallest accepted recording segment size in MB; values are clamped to
/// `100..=5000` (authored bounds, adjustable on dogfooding evidence).
pub const RECORDING_SEGMENT_MB_MIN: u32 = 100;

/// The largest accepted recording segment size in MB; values are clamped to
/// `100..=5000` (authored bounds, adjustable on dogfooding evidence).
pub const RECORDING_SEGMENT_MB_MAX: u32 = 5000;

/// Read the recording segment size in MB (Story 17.5, FR-72). Absent /
/// unparsable ⇒ the default of 500; a stored value is clamped to `100..=5000`
/// defensively (a hand-edited row can never surface out of range). Stored in the
/// `settings` k/v table under `recording.segment_mb`.
pub fn get_recording_segment_mb(data_dir: &Path) -> Result<u32, CoreError> {
    let raw = get_setting(data_dir, RECORDING_SEGMENT_MB_KEY)?;
    let mb = raw
        .as_deref()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(RECORDING_SEGMENT_MB_DEFAULT);
    Ok(mb.clamp(RECORDING_SEGMENT_MB_MIN, RECORDING_SEGMENT_MB_MAX))
}

/// Write the recording segment size in MB (Story 17.5, FR-72), clamping to
/// `100..=5000` before persisting a decimal string under
/// `recording.segment_mb`. Applies to the next Recording Session only — a
/// running session's params are read once at start.
pub fn set_recording_segment_mb(data_dir: &Path, mb: u32) -> Result<(), CoreError> {
    let clamped = mb.clamp(RECORDING_SEGMENT_MB_MIN, RECORDING_SEGMENT_MB_MAX);
    set_setting(data_dir, RECORDING_SEGMENT_MB_KEY, &clamped.to_string())
}

/// The `settings` key holding the recording duration-cap fallback in whole
/// minutes (Story 17.5, FR-72). Stored as a decimal string; absent / unparsable
/// ⇒ the default of 30 min. Converted to seconds (`× 60`) and passed to the
/// sidecar as `maxSegmentSeconds` on every `start`.
const RECORDING_DURATION_CAP_MINUTES_KEY: &str = "recording.duration_cap_minutes";

/// The default recording duration cap in minutes when the setting is absent or
/// unparsable (30 min → the sidecar's own 1800 s fallback).
pub const RECORDING_DURATION_CAP_MINUTES_DEFAULT: u16 = 30;

/// The smallest accepted recording duration cap in minutes; values are clamped
/// to `1..=600` (authored bounds, adjustable on dogfooding evidence).
pub const RECORDING_DURATION_CAP_MINUTES_MIN: u16 = 1;

/// The largest accepted recording duration cap in minutes; values are clamped
/// to `1..=600` (authored bounds, adjustable on dogfooding evidence).
pub const RECORDING_DURATION_CAP_MINUTES_MAX: u16 = 600;

/// Read the recording duration cap in minutes (Story 17.5, FR-72). Absent /
/// unparsable ⇒ the default of 30; a stored value is clamped to `1..=600`
/// defensively. Stored in the `settings` k/v table under
/// `recording.duration_cap_minutes`.
pub fn get_recording_duration_cap_minutes(data_dir: &Path) -> Result<u16, CoreError> {
    let raw = get_setting(data_dir, RECORDING_DURATION_CAP_MINUTES_KEY)?;
    let minutes = raw
        .as_deref()
        .and_then(|v| v.parse::<u16>().ok())
        .unwrap_or(RECORDING_DURATION_CAP_MINUTES_DEFAULT);
    Ok(minutes.clamp(
        RECORDING_DURATION_CAP_MINUTES_MIN,
        RECORDING_DURATION_CAP_MINUTES_MAX,
    ))
}

/// Write the recording duration cap in minutes (Story 17.5, FR-72), clamping to
/// `1..=600` before persisting a decimal string under
/// `recording.duration_cap_minutes`. Applies to the next Recording Session only.
pub fn set_recording_duration_cap_minutes(data_dir: &Path, minutes: u16) -> Result<(), CoreError> {
    let clamped = minutes.clamp(
        RECORDING_DURATION_CAP_MINUTES_MIN,
        RECORDING_DURATION_CAP_MINUTES_MAX,
    );
    set_setting(
        data_dir,
        RECORDING_DURATION_CAP_MINUTES_KEY,
        &clamped.to_string(),
    )
}

/// The `settings` key holding the user-chosen recording destination folder
/// (Story 19.5, AD-25). Stored as the raw absolute path string; absent / empty ⇒
/// no explicit choice — the SHELL resolves the effective default
/// (`dirs::video_dir()/keeper`, falling back to the app data dir) because that
/// resolution needs a platform probe this core crate must not hold.
const RECORDING_DESTINATION_DIR_KEY: &str = "recording.destination_dir";

/// Read the user-chosen recording destination folder (Story 19.5). `None` when
/// the setting is absent or empty — the caller (shell) resolves the effective
/// default. No validation here: the path is validated at Start time by the
/// `recording_start` pre-flight (probe → `evaluate_destination`), never on read.
pub fn get_recording_destination_dir(data_dir: &Path) -> Result<Option<String>, CoreError> {
    let raw = get_setting(data_dir, RECORDING_DESTINATION_DIR_KEY)?;
    Ok(raw.filter(|v| !v.trim().is_empty()))
}

/// Write the user-chosen recording destination folder (Story 19.5) verbatim
/// under `recording.destination_dir`. Applies to the next Recording Session
/// only — `recording_start` reads it once at Start and never mid-session.
pub fn set_recording_destination_dir(data_dir: &Path, dir: &str) -> Result<(), CoreError> {
    set_setting(data_dir, RECORDING_DESTINATION_DIR_KEY, dir)
}

/// The `settings` key holding the recording frame rate (Story 19.5). Stored as
/// a decimal string; absent / unparsable / out-of-set ⇒ the default of 30.
/// Passed to the `keeper-rec` sidecar as `fps` on every `start`.
const RECORDING_FPS_KEY: &str = "recording.fps";

/// The default recording frame rate when the setting is absent, unparsable, or
/// out of the legal set (the epic's authored default; the sidecar's own
/// `normalizeFps` fallback matches).
pub const RECORDING_FPS_DEFAULT: u32 = 30;

/// The only non-default legal recording frame rate (the collapsed Advanced
/// control offers exactly {30, 60}).
pub const RECORDING_FPS_ALTERNATE: u32 = 60;

/// Normalize a frame-rate value to the legal set {30, 60}: anything that is not
/// exactly 60 becomes the default of 30 — a corrupted persisted value can never
/// surface as a degenerate `timescale` downstream (the sidecar normalizes again
/// defensively with the identical rule).
pub fn normalize_recording_fps(fps: u32) -> u32 {
    if fps == RECORDING_FPS_ALTERNATE {
        RECORDING_FPS_ALTERNATE
    } else {
        RECORDING_FPS_DEFAULT
    }
}

/// Read the recording frame rate (Story 19.5). Absent / unparsable ⇒ the
/// default of 30; a stored value is normalized to {30, 60} defensively (a
/// hand-edited row can never surface out of the set). Stored in the `settings`
/// k/v table under `recording.fps`.
pub fn get_recording_fps(data_dir: &Path) -> Result<u32, CoreError> {
    let raw = get_setting(data_dir, RECORDING_FPS_KEY)?;
    let fps = raw
        .as_deref()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(RECORDING_FPS_DEFAULT);
    Ok(normalize_recording_fps(fps))
}

/// Write the recording frame rate (Story 19.5), normalizing to {30, 60} before
/// persisting a decimal string under `recording.fps`. Applies to the next
/// Recording Session only — a running session's params are read once at start.
pub fn set_recording_fps(data_dir: &Path, fps: u32) -> Result<(), CoreError> {
    set_setting(
        data_dir,
        RECORDING_FPS_KEY,
        &normalize_recording_fps(fps).to_string(),
    )
}

/// The `settings` key holding the recording video codec (Story 21.1). Stored
/// as `"h264"` / `"hevc"`; absent / anything else ⇒ the default of `"h264"`.
/// Passed to the `keeper-rec` sidecar as the additive `codec` param on start.
const RECORDING_CODEC_KEY: &str = "recording.codec";

/// The maximum-compatibility default codec (Story 21.1).
pub const RECORDING_CODEC_DEFAULT: &str = "h264";

/// The opt-in hardware-efficient codec (Story 21.1; VideoToolbox hardware
/// encode on Apple Silicon).
pub const RECORDING_CODEC_HEVC: &str = "hevc";

/// Normalize a codec string to the legal set {"h264", "hevc"}: anything that is
/// not exactly `"hevc"` becomes the `"h264"` default (the sidecar normalizes
/// again defensively with the identical rule).
pub fn normalize_recording_codec(codec: &str) -> &'static str {
    if codec == RECORDING_CODEC_HEVC {
        RECORDING_CODEC_HEVC
    } else {
        RECORDING_CODEC_DEFAULT
    }
}

/// Read the recording codec (Story 21.1). Absent / unrecognized ⇒ `"h264"`.
pub fn get_recording_codec(data_dir: &Path) -> Result<String, CoreError> {
    let raw = get_setting(data_dir, RECORDING_CODEC_KEY)?;
    Ok(normalize_recording_codec(raw.as_deref().unwrap_or(RECORDING_CODEC_DEFAULT)).to_owned())
}

/// Write the recording codec (Story 21.1), normalized to {"h264", "hevc"}.
/// Applies to the next Recording Session only.
pub fn set_recording_codec(data_dir: &Path, codec: &str) -> Result<(), CoreError> {
    set_setting(
        data_dir,
        RECORDING_CODEC_KEY,
        normalize_recording_codec(codec),
    )
}

/// The `settings` key holding the capture scale percent (Story 21.2). Stored as
/// a decimal string; absent / unparsable / out-of-set ⇒ the default of 100.
/// Passed to the sidecar as the additive `scalePercent` start param.
const RECORDING_SCALE_KEY: &str = "recording.scale_percent";

/// The full-resolution default capture scale (Story 21.2).
pub const RECORDING_SCALE_DEFAULT: u32 = 100;

/// Normalize a capture scale to the legal set {100, 75, 50}: anything else
/// becomes the 100 default (the sidecar normalizes again defensively and also
/// rounds the scaled dimensions to even pixels for the encoder).
pub fn normalize_recording_scale(percent: u32) -> u32 {
    match percent {
        75 => 75,
        50 => 50,
        _ => RECORDING_SCALE_DEFAULT,
    }
}

/// Read the capture scale percent (Story 21.2). Absent / unparsable ⇒ 100.
pub fn get_recording_scale_percent(data_dir: &Path) -> Result<u32, CoreError> {
    let raw = get_setting(data_dir, RECORDING_SCALE_KEY)?;
    let percent = raw
        .as_deref()
        .and_then(|v| v.parse::<u32>().ok())
        .unwrap_or(RECORDING_SCALE_DEFAULT);
    Ok(normalize_recording_scale(percent))
}

/// Write the capture scale percent (Story 21.2), normalized to {100, 75, 50}.
/// Applies to the next Recording Session only.
pub fn set_recording_scale_percent(data_dir: &Path, percent: u32) -> Result<(), CoreError> {
    set_setting(
        data_dir,
        RECORDING_SCALE_KEY,
        &normalize_recording_scale(percent).to_string(),
    )
}

/// A single held-send row from the `outbox` table (Story 8.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OutboxRow {
    /// Opaque unique row id (a fresh `TransactionId`).
    pub id: String,
    /// Owning keeper account id.
    pub account_id: String,
    /// Target room id.
    pub room_id: String,
    /// The held message body (never logged).
    pub body: String,
    /// When the send was held, in milliseconds since the Unix epoch (UTC).
    pub held_at_ts: i64,
    /// When the hold elapses and the row must dispatch, in ms since the Unix epoch.
    pub dispatch_at_ts: i64,
}

/// Insert a held-send row into the `outbox` (Story 8.3). Keyed by the unique `id`, so
/// many rows may coexist for one `(account_id, room_id)`. The body is never logged.
pub fn insert_outbox(
    data_dir: &Path,
    id: &str,
    account_id: &str,
    room_id: &str,
    body: &str,
    held_at_ts: i64,
    dispatch_at_ts: i64,
) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    conn.execute(
        "INSERT INTO outbox(id, account_id, room_id, body, held_at_ts, dispatch_at_ts) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![id, account_id, room_id, body, held_at_ts, dispatch_at_ts],
    )
    .map_err(|e| CoreError::Internal(format!("could not insert outbox row: {e}")))?;
    Ok(())
}

/// Remove a held-send row by its unique `id` (Story 8.3). Idempotent — deleting an
/// already-dispatched or absent row is not an error (cancel and scheduler both rely on
/// this).
pub fn delete_outbox(data_dir: &Path, id: &str) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    conn.execute("DELETE FROM outbox WHERE id = ?1", rusqlite::params![id])
        .map_err(|e| CoreError::Internal(format!("could not delete outbox row: {e}")))?;
    Ok(())
}

/// List every held-send row for `account_id`, oldest first (ordered by `held_at_ts`
/// ascending), so the scheduler dispatches and the UI stacks oldest-first (Story 8.3).
pub fn list_outbox_rows_for_account(
    data_dir: &Path,
    account_id: &str,
) -> Result<Vec<OutboxRow>, CoreError> {
    let conn = open(data_dir)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, account_id, room_id, body, held_at_ts, dispatch_at_ts FROM outbox \
             WHERE account_id = ?1 ORDER BY held_at_ts ASC",
        )
        .map_err(|e| CoreError::Internal(format!("could not prepare outbox list: {e}")))?;
    let rows = stmt
        .query_map(rusqlite::params![account_id], map_outbox_row)
        .map_err(|e| CoreError::Internal(format!("could not query outbox list: {e}")))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| CoreError::Internal(format!("could not read outbox row: {e}")))?);
    }
    Ok(out)
}

/// List every held-send row across all accounts, oldest first (Story 8.3).
pub fn list_outbox_rows(data_dir: &Path) -> Result<Vec<OutboxRow>, CoreError> {
    let conn = open(data_dir)?;
    let mut stmt = conn
        .prepare(
            "SELECT id, account_id, room_id, body, held_at_ts, dispatch_at_ts FROM outbox \
             ORDER BY held_at_ts ASC",
        )
        .map_err(|e| CoreError::Internal(format!("could not prepare outbox list: {e}")))?;
    let rows = stmt
        .query_map([], map_outbox_row)
        .map_err(|e| CoreError::Internal(format!("could not query outbox list: {e}")))?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row.map_err(|e| CoreError::Internal(format!("could not read outbox row: {e}")))?);
    }
    Ok(out)
}

/// Map a `SELECT id, account_id, room_id, body, held_at_ts, dispatch_at_ts` row into
/// an [`OutboxRow`]. Shared by the two outbox list queries.
fn map_outbox_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<OutboxRow> {
    Ok(OutboxRow {
        id: r.get(0)?,
        account_id: r.get(1)?,
        room_id: r.get(2)?,
        body: r.get(3)?,
        held_at_ts: r.get(4)?,
        dispatch_at_ts: r.get(5)?,
    })
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
    /// The login-mechanism tag (`"password" | "oidc" | "beeper"`), or `None` for
    /// a legacy row created before the provider column existed and not yet
    /// backfilled by inference.
    pub provider: Option<String>,
}

/// Insert one account row with its assigned hue index and login-mechanism
/// `provider` tag. Fails if `account_id` already exists (PRIMARY KEY).
///
/// Takes each non-secret column positionally (one flat registry row); grouping
/// them into a struct would add a layer without changing the single call site in
/// `add_account`.
#[allow(clippy::too_many_arguments)]
pub fn insert_account(
    data_dir: &Path,
    account_id: &str,
    user_id: &str,
    homeserver_url: &str,
    device_id: &str,
    created_ts: i64,
    hue_index: u8,
    provider: &str,
) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    conn.execute(
        "INSERT INTO accounts(account_id, user_id, homeserver_url, device_id, created_ts, hue_index, provider) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            account_id,
            user_id,
            homeserver_url,
            device_id,
            created_ts,
            hue_index as i64,
            provider
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

/// Backfill a `NULL` `provider` for a legacy account row with an inferred tag
/// (Story 2.5). Idempotent: a row that already has a provider is left untouched
/// (the `UPDATE ... WHERE provider IS NULL` guard makes a second call a no-op).
/// The caller performs the inference (stored-session shape + homeserver host);
/// this only persists it once so the inference never runs again.
pub fn backfill_provider(
    data_dir: &Path,
    account_id: &str,
    provider: &str,
) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    conn.execute(
        "UPDATE accounts SET provider = ?1 WHERE account_id = ?2 AND provider IS NULL",
        rusqlite::params![provider, account_id],
    )
    .map_err(|e| CoreError::Internal(format!("could not backfill provider: {e}")))?;
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
    // Drop any pins the signed-out account owned (Story 4.3): a pin has no meaning
    // once its account is gone. Idempotent — an account with no pins deletes zero.
    conn.execute(
        "DELETE FROM pins WHERE account_id = ?1",
        rusqlite::params![account_id],
    )
    .map_err(|e| CoreError::Internal(format!("could not delete account pins: {e}")))?;
    // Drop any composer drafts the signed-out account owned (Story 7.1): a draft has
    // no meaning once its account is gone, leaving no orphaned draft or inbox marker.
    // Idempotent — an account with no drafts deletes zero.
    conn.execute(
        "DELETE FROM drafts WHERE account_id = ?1",
        rusqlite::params![account_id],
    )
    .map_err(|e| CoreError::Internal(format!("could not delete account drafts: {e}")))?;
    // Drop any held-send outbox rows the signed-out account owned (Story 8.3): a held
    // send has no meaning once its account is gone. Idempotent — an account with no
    // held sends deletes zero.
    conn.execute(
        "DELETE FROM outbox WHERE account_id = ?1",
        rusqlite::params![account_id],
    )
    .map_err(|e| CoreError::Internal(format!("could not delete account outbox rows: {e}")))?;
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
            "SELECT account_id, user_id, homeserver_url, device_id, created_ts, hue_index, provider \
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
                provider: r.get::<_, Option<String>>(6)?,
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
            "SELECT account_id, user_id, homeserver_url, device_id, created_ts, hue_index, provider \
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
                    provider: r.get::<_, Option<String>>(6)?,
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
            "password",
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
        assert_eq!(row.provider.as_deref(), Some("password"));

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
            "password",
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
            "oidc",
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
        insert_account(
            &dir,
            "a",
            "@a:e.org",
            "https://e.org/",
            "D",
            1,
            0,
            "password",
        )
        .expect("insert a");
        // hue 0 in use → next is 1.
        assert_eq!(next_hue_index(&dir).expect("next"), 1);
        insert_account(&dir, "b", "@b:e.org", "https://e.org/", "D", 2, 1, "oidc")
            .expect("insert b");
        assert_eq!(next_hue_index(&dir).expect("next"), 2);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn hue_assignment_reuses_freed_index_after_removal() {
        let dir = temp_dir();
        insert_account(
            &dir,
            "a",
            "@a:e.org",
            "https://e.org/",
            "D",
            1,
            0,
            "password",
        )
        .expect("insert a");
        insert_account(&dir, "b", "@b:e.org", "https://e.org/", "D", 2, 1, "oidc")
            .expect("insert b");
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
    fn migration_adds_provider_column_to_legacy_table_without_dropping_rows() {
        let dir = temp_dir();
        std::fs::create_dir_all(&dir).expect("create dir");
        // Create a pre-provider `accounts` table (it has hue_index but no
        // provider) and a row, as a Story-2.1/2.4 install would have on disk.
        {
            let conn = Connection::open(db_path(&dir)).expect("open legacy db");
            conn.execute(
                "CREATE TABLE accounts(\
                    account_id TEXT PRIMARY KEY, \
                    user_id TEXT NOT NULL, \
                    homeserver_url TEXT NOT NULL, \
                    device_id TEXT NOT NULL, \
                    created_ts INTEGER NOT NULL, \
                    hue_index INTEGER\
                )",
                [],
            )
            .expect("create legacy table");
            conn.execute(
                "INSERT INTO accounts(account_id, user_id, homeserver_url, device_id, created_ts, hue_index) \
                 VALUES ('legacy', '@old:e.org', 'https://matrix.beeper.com/', 'DEV', 1, 0)",
                [],
            )
            .expect("insert legacy row");
        }

        // The next `open` (via list) migrates in place: the legacy row survives
        // with a NULL provider.
        let rows = list_accounts(&dir).expect("list after migration");
        assert_eq!(rows.len(), 1, "legacy row must survive migration");
        assert_eq!(rows[0].account_id, "legacy");
        assert_eq!(rows[0].provider, None, "legacy row provider starts NULL");

        // Backfill persists the inferred tag and is idempotent.
        backfill_provider(&dir, "legacy", "beeper").expect("backfill");
        let row = get_account(&dir, "legacy")
            .expect("get")
            .expect("row present");
        assert_eq!(row.provider.as_deref(), Some("beeper"));
        // A second call with a different value is a no-op (WHERE provider IS NULL).
        backfill_provider(&dir, "legacy", "password").expect("backfill idempotent");
        let row = get_account(&dir, "legacy")
            .expect("get")
            .expect("row present");
        assert_eq!(
            row.provider.as_deref(),
            Some("beeper"),
            "backfill must not overwrite an already-tagged provider"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn setting_roundtrip_and_overwrite() {
        let dir = temp_dir();
        // Unset key reads as None.
        assert_eq!(
            get_setting(&dir, "sdk_encryption").expect("get unset"),
            None
        );
        // Write then read back.
        set_setting(&dir, "sdk_encryption", "on").expect("set on");
        assert_eq!(
            get_setting(&dir, "sdk_encryption").expect("get on"),
            Some("on".to_owned())
        );
        // Overwrite replaces the value (ON CONFLICT DO UPDATE).
        set_setting(&dir, "sdk_encryption", "off").expect("set off");
        assert_eq!(
            get_setting(&dir, "sdk_encryption").expect("get off"),
            Some("off".to_owned())
        );
        // An unrelated key is independent.
        assert_eq!(get_setting(&dir, "other").expect("get other"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn dnd_global_defaults_off_and_round_trips() {
        let dir = temp_dir();
        // Absent = off (DND off by default; notifications post normally).
        assert!(!get_dnd_global(&dir).expect("get default"));
        set_dnd_global(&dir, true).expect("set on");
        assert!(get_dnd_global(&dir).expect("get on"));
        set_dnd_global(&dir, false).expect("set off");
        assert!(!get_dnd_global(&dir).expect("get off"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn muted_networks_crud_and_idempotent() {
        let dir = temp_dir();
        // Fresh registry mutes nothing.
        assert!(get_muted_networks(&dir).expect("get empty").is_empty());
        assert!(!is_network_muted(&dir, "Telegram").expect("is_muted empty"));

        // Mute two Networks; list is sorted ascending and deduped.
        set_network_muted(&dir, "Telegram", true).expect("mute telegram");
        set_network_muted(&dir, "Signal", true).expect("mute signal");
        // Re-muting is idempotent (no duplicate row via OR IGNORE).
        set_network_muted(&dir, "Telegram", true).expect("re-mute telegram");
        assert_eq!(
            get_muted_networks(&dir).expect("list"),
            vec!["Signal".to_owned(), "Telegram".to_owned()]
        );
        assert!(is_network_muted(&dir, "Telegram").expect("is_muted telegram"));
        assert!(!is_network_muted(&dir, "WhatsApp").expect("is_muted whatsapp"));

        // Unmute is idempotent — clearing an unmuted Network is not an error.
        set_network_muted(&dir, "Telegram", false).expect("unmute telegram");
        set_network_muted(&dir, "Telegram", false).expect("unmute again ok");
        assert_eq!(
            get_muted_networks(&dir).expect("list after unmute"),
            vec!["Signal".to_owned()]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn pins_crud_upsert_and_order() {
        let dir = temp_dir();
        // Empty registry has no pins.
        assert!(get_pins(&dir).expect("get empty").is_empty());

        // Insert three pins out of order; get_pins returns them sorted by order asc.
        set_pin(&dir, "acctA", "!r1", 2).expect("set r1");
        set_pin(&dir, "acctA", "!r2", 0).expect("set r2");
        set_pin(&dir, "acctB", "!r3", 1).expect("set r3");
        let pins = get_pins(&dir).expect("list pins");
        assert_eq!(
            pins,
            vec![
                ("acctA".to_owned(), "!r2".to_owned(), 0),
                ("acctB".to_owned(), "!r3".to_owned(), 1),
                ("acctA".to_owned(), "!r1".to_owned(), 2),
            ]
        );

        // Upsert overwrites the stored order for an existing key (no duplicate row).
        set_pin(&dir, "acctA", "!r2", 5).expect("re-set r2");
        let pins = get_pins(&dir).expect("list after upsert");
        assert_eq!(pins.len(), 3, "upsert must not add a row");
        // r2 now sorts last (order 5).
        assert_eq!(
            pins.last().expect("last"),
            &("acctA".to_owned(), "!r2".to_owned(), 5)
        );

        // Remove is idempotent. After the upsert the order is r3(1), r2(5), so
        // removing r1 leaves [r3, r2] in ascending-order sequence.
        remove_pin(&dir, "acctA", "!r1").expect("remove r1");
        remove_pin(&dir, "acctA", "!r1").expect("remove missing r1 is ok");
        let ids: Vec<String> = get_pins(&dir)
            .expect("list")
            .into_iter()
            .map(|(_, r, _)| r)
            .collect();
        assert_eq!(ids, vec!["!r3".to_owned(), "!r2".to_owned()]);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_account_drops_its_pins() {
        let dir = temp_dir();
        set_pin(&dir, "acctA", "!r1", 0).expect("pin A r1");
        set_pin(&dir, "acctA", "!r2", 1).expect("pin A r2");
        set_pin(&dir, "acctB", "!r3", 2).expect("pin B r3");

        delete_account(&dir, "acctA").expect("delete acctA");
        let pins = get_pins(&dir).expect("list after account delete");
        // Only acctB's pin survives.
        assert_eq!(pins, vec![("acctB".to_owned(), "!r3".to_owned(), 2)]);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn drafts_crud_roundtrip_and_upsert() {
        let dir = temp_dir();
        // Absent draft reads as None; list is empty.
        assert_eq!(get_draft(&dir, "acctA", "!r1").expect("get absent"), None);
        assert!(list_drafts(&dir).expect("list empty").is_empty());

        // Write then read back.
        set_draft(&dir, "acctA", "!r1", "half a message", 100).expect("set r1");
        assert_eq!(
            get_draft(&dir, "acctA", "!r1").expect("get r1"),
            Some("half a message".to_owned())
        );

        // Upsert overwrites the stored body (no duplicate row).
        set_draft(&dir, "acctA", "!r1", "revised message", 200).expect("re-set r1");
        assert_eq!(
            get_draft(&dir, "acctA", "!r1").expect("get r1 after upsert"),
            Some("revised message".to_owned())
        );
        assert_eq!(
            list_drafts(&dir).expect("list after upsert").len(),
            1,
            "upsert must not add a row"
        );

        // Idempotent delete: removing twice is not an error, and the draft is gone.
        delete_draft(&dir, "acctA", "!r1").expect("delete r1");
        delete_draft(&dir, "acctA", "!r1").expect("delete missing r1 is ok");
        assert_eq!(
            get_draft(&dir, "acctA", "!r1").expect("get after delete"),
            None
        );

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_drafts_spans_accounts() {
        let dir = temp_dir();
        set_draft(&dir, "acctA", "!r1", "a1", 1).expect("set A r1");
        set_draft(&dir, "acctA", "!r2", "a2", 2).expect("set A r2");
        set_draft(&dir, "acctB", "!r3", "b3", 3).expect("set B r3");

        let mut keys = list_drafts(&dir).expect("list across accounts");
        keys.sort();
        assert_eq!(
            keys,
            vec![
                ("acctA".to_owned(), "!r1".to_owned()),
                ("acctA".to_owned(), "!r2".to_owned()),
                ("acctB".to_owned(), "!r3".to_owned()),
            ]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn incognito_global_round_trips_and_defaults_off() {
        let dir = temp_dir();
        // Absent global setting defaults off (Incognito off by default).
        assert!(!get_incognito_global(&dir).expect("get absent global"));
        set_incognito_global(&dir, true).expect("set global on");
        assert!(get_incognito_global(&dir).expect("get global on"));
        set_incognito_global(&dir, false).expect("set global off");
        assert!(!get_incognito_global(&dir).expect("get global off"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn notify_previews_round_trips_and_defaults_on() {
        let dir = temp_dir();
        // Absent setting defaults ON (previews enabled by default, Story 10.1).
        assert!(get_notify_previews(&dir).expect("get absent previews"));
        set_notify_previews(&dir, false).expect("set previews off");
        assert!(!get_notify_previews(&dir).expect("get previews off"));
        set_notify_previews(&dir, true).expect("set previews on");
        assert!(get_notify_previews(&dir).expect("get previews on"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn global_hotkey_defaults_and_round_trips() {
        let dir = temp_dir();
        // Absent setting reads the default accelerator (Story 9.4).
        assert_eq!(
            get_global_hotkey(&dir).expect("get absent hotkey"),
            DEFAULT_GLOBAL_HOTKEY
        );
        // Set then read back an opaque accelerator string (core never parses it).
        set_global_hotkey(&dir, "Control+Shift+K").expect("set hotkey");
        assert_eq!(
            get_global_hotkey(&dir).expect("get set hotkey"),
            "Control+Shift+K"
        );
        // Overwrite replaces the stored accelerator.
        set_global_hotkey(&dir, DEFAULT_GLOBAL_HOTKEY).expect("reset hotkey");
        assert_eq!(
            get_global_hotkey(&dir).expect("get reset hotkey"),
            DEFAULT_GLOBAL_HOTKEY
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recording_hotkey_defaults_unset_and_round_trips() {
        let dir = temp_dir();
        // Absent setting reads the empty string — unset by default (Story 20.4),
        // never the summon default chord.
        assert_eq!(get_recording_hotkey(&dir).expect("get absent hotkey"), "");
        // Set then read back an opaque accelerator string (core never parses it).
        set_recording_hotkey(&dir, "Control+Alt+R").expect("set hotkey");
        assert_eq!(
            get_recording_hotkey(&dir).expect("get set hotkey"),
            "Control+Alt+R"
        );
        // Overwrite replaces the stored accelerator.
        set_recording_hotkey(&dir, "Control+Shift+R").expect("overwrite hotkey");
        assert_eq!(
            get_recording_hotkey(&dir).expect("get overwritten hotkey"),
            "Control+Shift+R"
        );
        // Persisting the empty string clears the binding back to unset.
        set_recording_hotkey(&dir, "").expect("clear hotkey");
        assert_eq!(get_recording_hotkey(&dir).expect("get cleared hotkey"), "");
        // The independent summon binding is untouched throughout.
        assert_eq!(
            get_global_hotkey(&dir).expect("summon binding untouched"),
            DEFAULT_GLOBAL_HOTKEY
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn incognito_account_round_trips_tristate() {
        let dir = temp_dir();
        insert_account(
            &dir,
            "acctA",
            "@a:e.org",
            "https://e.org/",
            "D",
            1,
            0,
            "password",
        )
        .expect("insert acctA");
        // A fresh account inherits (NULL column) — absent account also reads None.
        assert_eq!(
            get_incognito_account(&dir, "acctA").expect("get inherit"),
            None
        );
        assert_eq!(
            get_incognito_account(&dir, "nope").expect("get missing"),
            None
        );
        // Set explicit true, then false, then clear back to inherit.
        set_incognito_account(&dir, "acctA", Some(true)).expect("set true");
        assert_eq!(
            get_incognito_account(&dir, "acctA").expect("get true"),
            Some(true)
        );
        set_incognito_account(&dir, "acctA", Some(false)).expect("set false");
        assert_eq!(
            get_incognito_account(&dir, "acctA").expect("get false"),
            Some(false)
        );
        set_incognito_account(&dir, "acctA", None).expect("clear");
        assert_eq!(
            get_incognito_account(&dir, "acctA").expect("get cleared"),
            None
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn incognito_chat_round_trips_and_clears() {
        let dir = temp_dir();
        // Absent row = inherit.
        assert_eq!(
            get_incognito_chat(&dir, "acctA", "!r1").expect("get absent"),
            None
        );
        set_incognito_chat(&dir, "acctA", "!r1", Some(true)).expect("set true");
        assert_eq!(
            get_incognito_chat(&dir, "acctA", "!r1").expect("get true"),
            Some(true)
        );
        // Upsert overwrites (no duplicate row).
        set_incognito_chat(&dir, "acctA", "!r1", Some(false)).expect("set false");
        assert_eq!(
            get_incognito_chat(&dir, "acctA", "!r1").expect("get false"),
            Some(false)
        );
        // None deletes the row back to inherit; idempotent.
        set_incognito_chat(&dir, "acctA", "!r1", None).expect("clear");
        set_incognito_chat(&dir, "acctA", "!r1", None).expect("clear again is ok");
        assert_eq!(
            get_incognito_chat(&dir, "acctA", "!r1").expect("get cleared"),
            None
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn incognito_scopes_reads_all_three() {
        let dir = temp_dir();
        insert_account(
            &dir,
            "acctA",
            "@a:e.org",
            "https://e.org/",
            "D",
            1,
            0,
            "password",
        )
        .expect("insert acctA");
        // Defaults: chat inherit, account inherit, global off.
        assert_eq!(
            incognito_scopes(&dir, "acctA", "!r1").expect("scopes default"),
            (None, None, false)
        );
        set_incognito_global(&dir, true).expect("global on");
        set_incognito_account(&dir, "acctA", Some(false)).expect("account off");
        set_incognito_chat(&dir, "acctA", "!r1", Some(true)).expect("chat on");
        assert_eq!(
            incognito_scopes(&dir, "acctA", "!r1").expect("scopes set"),
            (Some(true), Some(false), true)
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn list_draft_rows_round_trips_full_rows() {
        let dir = temp_dir();
        assert!(
            list_draft_rows(&dir).expect("list empty rows").is_empty(),
            "empty registry yields no draft rows"
        );

        // Insert deliberately out of the ORDER BY key (account_id, updated_ts,
        // room_id) so a passing assertion proves the query orders, not insertion luck.
        set_draft(&dir, "acctB", "!r3", "b3 body", 300).expect("set B r3");
        set_draft(&dir, "acctA", "!r2", "a2 body", 200).expect("set A r2");
        set_draft(&dir, "acctA", "!r1", "a1 body", 100).expect("set A r1");
        // Same account + same timestamp → room_id breaks the tie deterministically.
        set_draft(&dir, "acctA", "!r0", "a0 body", 100).expect("set A r0");

        // The query returns a deterministic ORDER BY account_id, updated_ts, room_id —
        // no local sort. This keeps the grouped pane + single roving tab-stop stable
        // across re-queries.
        let rows = list_draft_rows(&dir).expect("list draft rows");
        assert_eq!(
            rows,
            vec![
                (
                    "acctA".to_owned(),
                    "!r0".to_owned(),
                    "a0 body".to_owned(),
                    100
                ),
                (
                    "acctA".to_owned(),
                    "!r1".to_owned(),
                    "a1 body".to_owned(),
                    100
                ),
                (
                    "acctA".to_owned(),
                    "!r2".to_owned(),
                    "a2 body".to_owned(),
                    200
                ),
                (
                    "acctB".to_owned(),
                    "!r3".to_owned(),
                    "b3 body".to_owned(),
                    300
                ),
            ],
            "rows must come back in the deterministic ORDER BY order"
        );

        // Ordering is stable across a re-query (identical vector, no reshuffle).
        let rows_again = list_draft_rows(&dir).expect("re-list draft rows");
        assert_eq!(rows, rows_again, "row order is stable across re-queries");

        // Upsert is reflected in the projected body + timestamp (no duplicate row).
        set_draft(&dir, "acctA", "!r1", "a1 revised", 150).expect("re-set A r1");
        let rows = list_draft_rows(&dir).expect("list draft rows after upsert");
        assert_eq!(rows.len(), 4, "upsert must not add a row");
        let a1 = rows
            .iter()
            .find(|r| r.0 == "acctA" && r.1 == "!r1")
            .expect("acctA r1 present");
        assert_eq!(a1.2, "a1 revised");
        assert_eq!(a1.3, 150);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_account_drops_its_drafts() {
        let dir = temp_dir();
        set_draft(&dir, "acctA", "!r1", "a1", 1).expect("draft A r1");
        set_draft(&dir, "acctA", "!r2", "a2", 2).expect("draft A r2");
        set_draft(&dir, "acctB", "!r3", "b3", 3).expect("draft B r3");

        delete_account(&dir, "acctA").expect("delete acctA");
        let keys = list_drafts(&dir).expect("list after account delete");
        // Only acctB's draft survives.
        assert_eq!(keys, vec![("acctB".to_owned(), "!r3".to_owned())]);
        assert_eq!(get_draft(&dir, "acctA", "!r1").expect("get gone"), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn undo_send_window_defaults_and_clamps() {
        let dir = temp_dir();
        // Absent setting reads the default of 10.
        assert_eq!(
            get_undo_send_window(&dir).expect("get default"),
            UNDO_SEND_WINDOW_DEFAULT
        );
        // Round-trip an in-range value.
        set_undo_send_window(&dir, 25).expect("set 25");
        assert_eq!(get_undo_send_window(&dir).expect("get 25"), 25);
        // 0 disables and round-trips.
        set_undo_send_window(&dir, 0).expect("set 0");
        assert_eq!(get_undo_send_window(&dir).expect("get 0"), 0);
        // Out-of-range clamps to 60 on write.
        set_undo_send_window(&dir, 99).expect("set 99");
        assert_eq!(get_undo_send_window(&dir).expect("get clamped"), 60);
        // A stored garbage value falls back to the default on read.
        set_setting(&dir, UNDO_SEND_WINDOW_KEY, "not-a-number").expect("set garbage");
        assert_eq!(
            get_undo_send_window(&dir).expect("get garbage"),
            UNDO_SEND_WINDOW_DEFAULT
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recording_segment_mb_defaults_and_clamps() {
        let dir = temp_dir();
        // Absent setting reads the default of 500.
        assert_eq!(
            get_recording_segment_mb(&dir).expect("get default"),
            RECORDING_SEGMENT_MB_DEFAULT
        );
        // Round-trip an in-range value.
        set_recording_segment_mb(&dir, 800).expect("set 800");
        assert_eq!(get_recording_segment_mb(&dir).expect("get 800"), 800);
        // Below the floor clamps to 100 on write.
        set_recording_segment_mb(&dir, 10).expect("set 10");
        assert_eq!(
            get_recording_segment_mb(&dir).expect("get floor"),
            RECORDING_SEGMENT_MB_MIN
        );
        // Above the ceiling clamps to 5000 on write.
        set_recording_segment_mb(&dir, 99_999).expect("set 99999");
        assert_eq!(
            get_recording_segment_mb(&dir).expect("get ceiling"),
            RECORDING_SEGMENT_MB_MAX
        );
        // A stored garbage value falls back to the default on read.
        set_setting(&dir, RECORDING_SEGMENT_MB_KEY, "abc").expect("set garbage");
        assert_eq!(
            get_recording_segment_mb(&dir).expect("get garbage"),
            RECORDING_SEGMENT_MB_DEFAULT
        );
        // A hand-edited out-of-range row clamps on read too.
        set_setting(&dir, RECORDING_SEGMENT_MB_KEY, "7").expect("set raw 7");
        assert_eq!(
            get_recording_segment_mb(&dir).expect("get raw clamped"),
            RECORDING_SEGMENT_MB_MIN
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recording_duration_cap_minutes_defaults_and_clamps() {
        let dir = temp_dir();
        // Absent setting reads the default of 30.
        assert_eq!(
            get_recording_duration_cap_minutes(&dir).expect("get default"),
            RECORDING_DURATION_CAP_MINUTES_DEFAULT
        );
        // Round-trip an in-range value.
        set_recording_duration_cap_minutes(&dir, 45).expect("set 45");
        assert_eq!(
            get_recording_duration_cap_minutes(&dir).expect("get 45"),
            45
        );
        // Below the floor clamps to 1 on write (0 never disables the cap).
        set_recording_duration_cap_minutes(&dir, 0).expect("set 0");
        assert_eq!(
            get_recording_duration_cap_minutes(&dir).expect("get floor"),
            RECORDING_DURATION_CAP_MINUTES_MIN
        );
        // Above the ceiling clamps to 600 on write.
        set_recording_duration_cap_minutes(&dir, 5000).expect("set 5000");
        assert_eq!(
            get_recording_duration_cap_minutes(&dir).expect("get ceiling"),
            RECORDING_DURATION_CAP_MINUTES_MAX
        );
        // A stored garbage value falls back to the default on read.
        set_setting(&dir, RECORDING_DURATION_CAP_MINUTES_KEY, "abc").expect("set garbage");
        assert_eq!(
            get_recording_duration_cap_minutes(&dir).expect("get garbage"),
            RECORDING_DURATION_CAP_MINUTES_DEFAULT
        );
        // A hand-edited out-of-range row clamps on read too.
        set_setting(&dir, RECORDING_DURATION_CAP_MINUTES_KEY, "0").expect("set raw 0");
        assert_eq!(
            get_recording_duration_cap_minutes(&dir).expect("get raw clamped"),
            RECORDING_DURATION_CAP_MINUTES_MIN
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recording_destination_dir_defaults_to_none_and_round_trips() {
        let dir = temp_dir();
        // Absent setting reads `None` — the shell resolves the effective default.
        assert_eq!(
            get_recording_destination_dir(&dir).expect("get default"),
            None
        );
        // Round-trip a chosen folder verbatim (no clamp, no normalization).
        set_recording_destination_dir(&dir, "/Users/x/Recordings").expect("set folder");
        assert_eq!(
            get_recording_destination_dir(&dir).expect("get folder"),
            Some("/Users/x/Recordings".to_owned())
        );
        // An empty (or whitespace-only) stored value reads `None` — "cleared"
        // and "never set" are the same effective-default state.
        set_recording_destination_dir(&dir, "").expect("set empty");
        assert_eq!(
            get_recording_destination_dir(&dir).expect("get empty"),
            None
        );
        set_recording_destination_dir(&dir, "   ").expect("set blank");
        assert_eq!(
            get_recording_destination_dir(&dir).expect("get blank"),
            None
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recording_fps_defaults_and_normalizes() {
        let dir = temp_dir();
        // Absent setting reads the default of 30.
        assert_eq!(
            get_recording_fps(&dir).expect("get default"),
            RECORDING_FPS_DEFAULT
        );
        // Round-trip the only non-default legal value.
        set_recording_fps(&dir, 60).expect("set 60");
        assert_eq!(get_recording_fps(&dir).expect("get 60"), 60);
        // An out-of-set value normalizes to 30 on write.
        set_recording_fps(&dir, 45).expect("set 45");
        assert_eq!(
            get_recording_fps(&dir).expect("get normalized"),
            RECORDING_FPS_DEFAULT
        );
        // A stored garbage value falls back to the default on read.
        set_setting(&dir, RECORDING_FPS_KEY, "abc").expect("set garbage");
        assert_eq!(
            get_recording_fps(&dir).expect("get garbage"),
            RECORDING_FPS_DEFAULT
        );
        // A hand-edited out-of-set row normalizes on read too — never a
        // degenerate frame rate downstream.
        for raw in ["0", "45", "120", "4294967295"] {
            set_setting(&dir, RECORDING_FPS_KEY, raw).expect("set raw");
            assert_eq!(
                get_recording_fps(&dir).expect("get raw normalized"),
                RECORDING_FPS_DEFAULT,
                "raw {raw:?} must normalize to the default"
            );
        }
        set_setting(&dir, RECORDING_FPS_KEY, "60").expect("set raw 60");
        assert_eq!(get_recording_fps(&dir).expect("get raw 60"), 60);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn normalize_recording_fps_maps_everything_but_60_to_30() {
        assert_eq!(normalize_recording_fps(30), 30);
        assert_eq!(normalize_recording_fps(60), 60);
        for out_of_set in [0, 1, 29, 31, 45, 59, 61, 120, u32::MAX] {
            assert_eq!(normalize_recording_fps(out_of_set), 30);
        }
    }

    #[test]
    fn outbox_crud_insert_list_for_account_and_delete() {
        let dir = temp_dir();
        // Empty outbox lists nothing.
        assert!(
            list_outbox_rows(&dir).expect("list empty").is_empty(),
            "fresh outbox lists no rows"
        );
        assert!(list_outbox_rows_for_account(&dir, "acctA")
            .expect("list empty for account")
            .is_empty());

        // Insert three rows out of held-at order; list returns oldest-first.
        insert_outbox(&dir, "id2", "acctA", "!r1", "second", 200, 210_000).expect("ins id2");
        insert_outbox(&dir, "id1", "acctA", "!r1", "first", 100, 110_000).expect("ins id1");
        insert_outbox(&dir, "id3", "acctB", "!r9", "other", 150, 160_000).expect("ins id3");

        let a = list_outbox_rows_for_account(&dir, "acctA").expect("list acctA");
        assert_eq!(a.len(), 2, "acctA has two held rows");
        assert_eq!(a[0].id, "id1", "oldest (held_at 100) first");
        assert_eq!(a[1].id, "id2");
        assert_eq!(a[0].body, "first");
        assert_eq!(a[0].dispatch_at_ts, 110_000);

        // The cross-account list spans accounts, still oldest-first.
        let all = list_outbox_rows(&dir).expect("list all");
        assert_eq!(all.len(), 3);
        assert_eq!(all[0].id, "id1", "held_at 100");
        assert_eq!(all[1].id, "id3", "held_at 150");
        assert_eq!(all[2].id, "id2", "held_at 200");

        // Idempotent delete removes one row; deleting again is a no-op.
        delete_outbox(&dir, "id1").expect("delete id1");
        delete_outbox(&dir, "id1").expect("delete missing id1 is ok");
        let a = list_outbox_rows_for_account(&dir, "acctA").expect("list after delete");
        assert_eq!(a.len(), 1);
        assert_eq!(a[0].id, "id2");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unelapsed_outbox_row_survives_restart_read() {
        // Simulate crash/restart: a row written now with a future dispatch_at_ts must
        // be readable back from a freshly opened db (WAL durability), preserving its
        // countdown target so the scheduler waits and the UI resumes.
        let dir = temp_dir();
        insert_outbox(
            &dir,
            "held1",
            "acctA",
            "!r1",
            "surviving",
            1_000,
            9_999_999_999,
        )
        .expect("insert held");
        // A second `open` (implicit in every registry call) reads the same durable row.
        let rows = list_outbox_rows_for_account(&dir, "acctA").expect("re-read after restart");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "held1");
        assert_eq!(rows[0].body, "surviving");
        assert_eq!(rows[0].dispatch_at_ts, 9_999_999_999);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn delete_account_drops_its_outbox_rows() {
        let dir = temp_dir();
        insert_outbox(&dir, "o1", "acctA", "!r1", "a1", 1, 2).expect("outbox A o1");
        insert_outbox(&dir, "o2", "acctA", "!r2", "a2", 3, 4).expect("outbox A o2");
        insert_outbox(&dir, "o3", "acctB", "!r3", "b3", 5, 6).expect("outbox B o3");

        delete_account(&dir, "acctA").expect("delete acctA");
        let all = list_outbox_rows(&dir).expect("list after account delete");
        assert_eq!(all.len(), 1, "only acctB's held row survives");
        assert_eq!(all[0].id, "o3");
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
            "password",
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

    #[test]
    fn dock_badge_mode_defaults_all_and_round_trips() {
        let dir = temp_dir();
        // Absent ⇒ the honest default (badge all unreads).
        assert_eq!(
            get_dock_badge_mode(&dir).expect("read default"),
            DockBadgeMode::All
        );
        // Every mode persists and reads back identically.
        for mode in [
            DockBadgeMode::All,
            DockBadgeMode::Mentions,
            DockBadgeMode::Off,
        ] {
            set_dock_badge_mode(&dir, mode).expect("persist mode");
            assert_eq!(get_dock_badge_mode(&dir).expect("read back"), mode);
        }
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn ios_sync_disclosure_shown_defaults_false_and_round_trips() {
        let dir = temp_dir();
        // Absent ⇒ not yet shown (the card is due on the reduced tier).
        assert!(!get_ios_sync_disclosure_shown(&dir).expect("read default"));
        // Latching persists and reads back true; re-latching stays true (one-way).
        set_ios_sync_disclosure_shown(&dir).expect("persist latch");
        assert!(get_ios_sync_disclosure_shown(&dir).expect("read back"));
        set_ios_sync_disclosure_shown(&dir).expect("re-latch");
        assert!(get_ios_sync_disclosure_shown(&dir).expect("read after re-latch"));
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recovered_sessions_acknowledged_round_trips_and_is_idempotent() {
        let dir = temp_dir();
        // Absent ⇒ nothing acknowledged (every recovered session still due).
        assert!(get_recovered_sessions_acknowledged(&dir)
            .expect("read default")
            .is_empty());
        // Latching one basename persists and reads back.
        add_recovered_session_acknowledged(&dir, "keeper-rec a").expect("ack a");
        assert_eq!(
            get_recovered_sessions_acknowledged(&dir).expect("read a"),
            vec!["keeper-rec a".to_owned()]
        );
        // A distinct session adds a second entry (a set of many).
        add_recovered_session_acknowledged(&dir, "keeper-rec b").expect("ack b");
        assert_eq!(
            get_recovered_sessions_acknowledged(&dir).expect("read a+b"),
            vec!["keeper-rec a".to_owned(), "keeper-rec b".to_owned()]
        );
        // Re-acknowledging an already-present basename is a no-op (no dup).
        add_recovered_session_acknowledged(&dir, "keeper-rec a").expect("re-ack a");
        assert_eq!(
            get_recovered_sessions_acknowledged(&dir).expect("read after re-ack"),
            vec!["keeper-rec a".to_owned(), "keeper-rec b".to_owned()]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn recovered_sessions_acknowledged_degrades_to_empty_on_corrupt_value() {
        let dir = temp_dir();
        // A corrupt/legacy stored value (not a JSON string array) must not error
        // — it degrades to "nothing acknowledged" (and logs a warning) so a
        // later `add_` can re-establish the set rather than propagating.
        set_setting(&dir, UI_RECOVERED_SESSIONS_ACKNOWLEDGED_KEY, "{not-json")
            .expect("seed corrupt");
        assert!(get_recovered_sessions_acknowledged(&dir)
            .expect("corrupt value reads as empty, not an error")
            .is_empty());
        // Recovery from the corrupt state: a fresh latch persists and reads back.
        add_recovered_session_acknowledged(&dir, "keeper-rec c").expect("ack after corrupt");
        assert_eq!(
            get_recovered_sessions_acknowledged(&dir).expect("read after recovery"),
            vec!["keeper-rec c".to_owned()]
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn menu_bar_presence_defaults_off_and_round_trips() {
        let dir = temp_dir();
        // Absent ⇒ off (the tray is opt-in).
        assert!(!get_menu_bar_presence(&dir).expect("read default"));
        set_menu_bar_presence(&dir, true).expect("enable");
        assert!(get_menu_bar_presence(&dir).expect("read back on"));
        set_menu_bar_presence(&dir, false).expect("disable");
        assert!(!get_menu_bar_presence(&dir).expect("read back off"));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
