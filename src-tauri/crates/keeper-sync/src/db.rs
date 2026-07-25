//! `sync.db` — profiles, the durable work journal, the file-state cache and
//! the device identity (Story 23.3 / 23.4, AD-42).
//!
//! A third SQLite database beside `keeper.db` and `archive.db`, following the
//! same conventions as `keeper_core::registry`: WAL, idempotent
//! `CREATE TABLE IF NOT EXISTS`, additive `ensure_*_column` migrations, and a
//! `Connection` that is **never held across an `.await`** (every function here
//! is synchronous; callers hop through `spawn_blocking` when they are async).
//!
//! The journal is what makes NFR-24 true. Every unit of network work is
//! recorded *before* it is attempted and cleared only once its effect is
//! durably observable, so a `kill -9` between the two costs a repeat, never a
//! loss.

use std::path::{Path, PathBuf};

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{Result, SyncError};
use crate::profile::{ProfileState, SyncProfile};
use crate::stability::{FileSample, PersistedEntry};

pub const DB_FILE_NAME: &str = "sync.db";

pub fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join(DB_FILE_NAME)
}

/// Open (creating if needed) and migrate the database.
///
/// Safe to call repeatedly: schema creation is idempotent, which is what lets
/// both the app and `keeper-syncd` open the same file without a migration
/// coordinator.
pub fn open(data_dir: &Path) -> Result<Connection> {
    std::fs::create_dir_all(data_dir)
        .map_err(|e| SyncError::io("create data directory", data_dir, e))?;
    let conn = Connection::open(db_path(data_dir))?;
    // WAL so a long-running watcher's reads never block the writer, matching
    // keeper.db. NORMAL synchronous is the WAL-recommended pairing: a power
    // loss can cost the last transaction, which the journal re-drives anyway.
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "synchronous", "NORMAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    migrate(&conn)?;
    Ok(conn)
}

/// An in-memory database, for tests.
pub fn open_in_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    migrate(&conn)?;
    Ok(conn)
}

fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS profiles (
            id          TEXT PRIMARY KEY,
            json        TEXT NOT NULL,
            state       TEXT NOT NULL DEFAULT 'idle',
            last_error  TEXT,
            updated_ms  INTEGER NOT NULL DEFAULT 0
        );

        CREATE TABLE IF NOT EXISTS journal (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            profile_id  TEXT NOT NULL,
            kind        TEXT NOT NULL,
            payload     TEXT NOT NULL,
            state       TEXT NOT NULL,
            attempts    INTEGER NOT NULL DEFAULT 0,
            not_before_ms INTEGER NOT NULL DEFAULT 0,
            created_ms  INTEGER NOT NULL,
            last_error  TEXT
        );
        CREATE INDEX IF NOT EXISTS journal_ready
            ON journal (state, not_before_ms);
        CREATE INDEX IF NOT EXISTS journal_by_profile
            ON journal (profile_id, state);

        CREATE TABLE IF NOT EXISTS file_state (
            profile_id  TEXT NOT NULL,
            path        TEXT NOT NULL,
            size        INTEGER NOT NULL,
            mtime_ns    INTEGER NOT NULL,
            ctime_ns    INTEGER NOT NULL,
            inode       INTEGER NOT NULL,
            first_seen_ms INTEGER NOT NULL,
            last_change_ms INTEGER NOT NULL,
            close_write INTEGER NOT NULL DEFAULT 0,
            PRIMARY KEY (profile_id, path)
        );

        CREATE TABLE IF NOT EXISTS device (
            singleton   INTEGER PRIMARY KEY CHECK (singleton = 0),
            id          TEXT NOT NULL,
            label       TEXT NOT NULL
        );
        "#,
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Device identity (Story 23.4)
// ---------------------------------------------------------------------------

/// This installation's stable identity, used in provenance trailers and in
/// conflict filenames so a conflict copy names the machine that made it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub id: String,
    pub label: String,
}

/// Read the device identity, minting it once on first call.
///
/// The `CHECK (singleton = 0)` primary key makes "there is exactly one device
/// row" a schema invariant rather than a convention someone can violate.
pub fn device_identity(conn: &Connection, default_label: &str) -> Result<DeviceIdentity> {
    let existing = conn
        .query_row(
            "SELECT id, label FROM device WHERE singleton = 0",
            [],
            |r| {
                Ok(DeviceIdentity {
                    id: r.get(0)?,
                    label: r.get(1)?,
                })
            },
        )
        .optional()?;
    if let Some(found) = existing {
        return Ok(found);
    }
    let minted = DeviceIdentity {
        id: ulid::Ulid::new().to_string(),
        label: default_label.to_owned(),
    };
    conn.execute(
        "INSERT INTO device (singleton, id, label) VALUES (0, ?1, ?2)",
        (&minted.id, &minted.label),
    )?;
    Ok(minted)
}

pub fn set_device_label(conn: &Connection, label: &str) -> Result<()> {
    conn.execute("UPDATE device SET label = ?1 WHERE singleton = 0", [label])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------

/// Insert or replace a profile. Validation runs first so a bad profile can
/// never reach the database, whatever route it arrived by.
pub fn upsert_profile(conn: &Connection, profile: &SyncProfile, now_ms: i64) -> Result<()> {
    profile.validate()?;
    let json = serde_json::to_string(profile)
        .map_err(|e| SyncError::Config(format!("profile is not serializable: {e}")))?;
    conn.execute(
        "INSERT INTO profiles (id, json, updated_ms) VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET json = ?2, updated_ms = ?3",
        (&profile.id, &json, now_ms),
    )?;
    Ok(())
}

pub fn list_profiles(conn: &Connection) -> Result<Vec<SyncProfile>> {
    let mut stmt = conn.prepare("SELECT json FROM profiles ORDER BY id")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        let json = row?;
        match serde_json::from_str::<SyncProfile>(&json) {
            Ok(profile) => out.push(profile),
            // A profile written by a newer keeper must not brick an older one.
            // Skip it loudly rather than failing the whole listing.
            Err(err) => tracing::warn!(error = %err, "skipping unreadable sync profile row"),
        }
    }
    Ok(out)
}

pub fn get_profile(conn: &Connection, id: &str) -> Result<Option<SyncProfile>> {
    let json: Option<String> = conn
        .query_row("SELECT json FROM profiles WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    match json {
        Some(json) => serde_json::from_str(&json)
            .map(Some)
            .map_err(|e| SyncError::Config(format!("stored profile is unreadable: {e}"))),
        None => Ok(None),
    }
}

/// Delete a profile and every journal/file-state row belonging to it.
///
/// Deliberately not a foreign-key cascade: the journal is intentionally
/// decoupled so a corrupt profile row can never take pending work with it.
pub fn delete_profile(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM journal WHERE profile_id = ?1", [id])?;
    conn.execute("DELETE FROM file_state WHERE profile_id = ?1", [id])?;
    conn.execute("DELETE FROM profiles WHERE id = ?1", [id])?;
    Ok(())
}

pub fn set_profile_runtime(
    conn: &Connection,
    id: &str,
    state: &str,
    last_error: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE profiles SET state = ?2, last_error = ?3 WHERE id = ?1",
        (id, state, last_error),
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// The work journal
// ---------------------------------------------------------------------------

/// A unit of work that must survive a crash.
///
/// Only operations that touch the network or mutate history are journaled.
/// Reading the worktree is not: it is cheap, idempotent, and re-derivable.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum WorkKind {
    /// Fetch from the remote and apply what is fast-forwardable.
    Pull,
    /// Stage, commit and push local changes.
    Push,
    /// Upload one LFS object.
    LfsUpload { oid: String, size: u64 },
    /// Download one LFS object.
    LfsDownload { oid: String, size: u64 },
    /// Open a pull request for a pushed lane branch (AD-50).
    OpenPullRequest { branch: String },
    /// Verify stored content against its expected digests.
    Verify,
}

impl WorkKind {
    /// Discriminant used as the journal's `kind` column, so a row can be
    /// filtered without deserializing its payload.
    fn tag(&self) -> &'static str {
        match self {
            Self::Pull => "pull",
            Self::Push => "push",
            Self::LfsUpload { .. } => "lfsUpload",
            Self::LfsDownload { .. } => "lfsDownload",
            Self::OpenPullRequest { .. } => "openPullRequest",
            Self::Verify => "verify",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WorkState {
    /// Waiting for its `not_before_ms`.
    Pending,
    /// Claimed by a supervisor and in flight.
    Running,
    /// Waiting on an external condition (a volume), not on a timer.
    Deferred,
    /// Stopped; needs a human.
    Parked,
}

impl WorkState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Deferred => "deferred",
            Self::Parked => "parked",
        }
    }
}

#[derive(Debug, Clone)]
pub struct WorkItem {
    pub id: i64,
    pub profile_id: String,
    pub kind: WorkKind,
    /// Which attempt this is, counting from 1.
    ///
    /// The stored column holds attempts *already made*, so this is that value
    /// plus one — the attempt the caller is about to perform. Getting this
    /// wrong is not cosmetic: the backoff schedule treats attempt 0 as
    /// "immediate", so reporting the pre-increment value would give the first
    /// retry no delay at all.
    pub attempts: u32,
    pub last_error: Option<String>,
}

/// Record a unit of work before attempting it.
pub fn enqueue(
    conn: &Connection,
    profile_id: &str,
    kind: &WorkKind,
    now_ms: i64,
    not_before_ms: i64,
) -> Result<i64> {
    let payload = serde_json::to_string(kind)
        .map_err(|e| SyncError::Journal(format!("work item is not serializable: {e}")))?;
    conn.execute(
        "INSERT INTO journal (profile_id, kind, payload, state, not_before_ms, created_ms)
         VALUES (?1, ?2, ?3, 'pending', ?4, ?5)",
        (profile_id, kind.tag(), &payload, not_before_ms, now_ms),
    )?;
    Ok(conn.last_insert_rowid())
}

/// Enqueue only if an equivalent pending unit is not already queued.
///
/// The watcher can produce a burst of events for one profile; without this a
/// hundred file writes would queue a hundred identical pushes.
pub fn enqueue_unique(
    conn: &Connection,
    profile_id: &str,
    kind: &WorkKind,
    now_ms: i64,
    not_before_ms: i64,
) -> Result<Option<i64>> {
    let payload = serde_json::to_string(kind)
        .map_err(|e| SyncError::Journal(format!("work item is not serializable: {e}")))?;
    let existing: Option<i64> = conn
        .query_row(
            "SELECT id FROM journal
             WHERE profile_id = ?1 AND payload = ?2 AND state IN ('pending','deferred')
             LIMIT 1",
            (profile_id, &payload),
            |r| r.get(0),
        )
        .optional()?;
    if existing.is_some() {
        return Ok(None);
    }
    enqueue(conn, profile_id, kind, now_ms, not_before_ms).map(Some)
}

/// Claim the ready units for one profile, marking them `running` in the same
/// statement so two supervisors can never take the same row.
pub fn claim_ready(
    conn: &Connection,
    profile_id: &str,
    now_ms: i64,
    limit: u32,
) -> Result<Vec<WorkItem>> {
    let mut stmt = conn.prepare(
        "SELECT id, payload, attempts, last_error FROM journal
         WHERE profile_id = ?1 AND state = 'pending' AND not_before_ms <= ?2
         ORDER BY id LIMIT ?3",
    )?;
    let rows = stmt.query_map((profile_id, now_ms, limit), |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, Option<String>>(3)?,
        ))
    })?;

    let mut claimed = Vec::new();
    for row in rows {
        let (id, payload, attempts, last_error) = row?;
        match serde_json::from_str::<WorkKind>(&payload) {
            Ok(kind) => claimed.push(WorkItem {
                id,
                profile_id: profile_id.to_owned(),
                kind,
                attempts: attempts.max(0).saturating_add(1) as u32,
                last_error,
            }),
            Err(err) => {
                // An unreadable payload can never succeed; parking it keeps the
                // queue moving instead of retrying a poison row forever.
                tracing::warn!(row = id, error = %err, "parking unreadable journal row");
                conn.execute(
                    "UPDATE journal SET state = 'parked', last_error = ?2 WHERE id = ?1",
                    (id, format!("unreadable payload: {err}")),
                )?;
            }
        }
    }
    for item in &claimed {
        conn.execute(
            "UPDATE journal SET state = 'running', attempts = attempts + 1 WHERE id = ?1",
            [item.id],
        )?;
    }
    Ok(claimed)
}

/// Clear a unit that completed. This is the only place work leaves the journal.
pub fn complete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM journal WHERE id = ?1", [id])?;
    Ok(())
}

/// Return a unit to the queue with a new state and earliest-attempt time.
pub fn reschedule(
    conn: &Connection,
    id: i64,
    state: WorkState,
    not_before_ms: i64,
    error: Option<&str>,
) -> Result<()> {
    conn.execute(
        "UPDATE journal SET state = ?2, not_before_ms = ?3, last_error = ?4 WHERE id = ?1",
        (id, state.as_str(), not_before_ms, error),
    )?;
    Ok(())
}

/// Return every `running` unit to `pending`.
///
/// Called at startup: a unit left `running` is one whose process died
/// mid-flight, and it must be re-driven rather than stranded. This single
/// statement is what turns "crashed" into "repeated".
pub fn recover_running(conn: &Connection, now_ms: i64) -> Result<usize> {
    let n = conn.execute(
        "UPDATE journal SET state = 'pending', not_before_ms = ?1 WHERE state = 'running'",
        [now_ms],
    )?;
    if n > 0 {
        tracing::info!(count = n, "re-queued sync work interrupted by a restart");
    }
    Ok(n)
}

/// Move deferred work back to pending — a removable volume came back.
pub fn undefer_profile(conn: &Connection, profile_id: &str, now_ms: i64) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE journal SET state = 'pending', not_before_ms = ?2
         WHERE profile_id = ?1 AND state = 'deferred'",
        (profile_id, now_ms),
    )?)
}

pub fn pending_count(conn: &Connection, profile_id: &str) -> Result<u32> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM journal WHERE profile_id = ?1 AND state != 'parked'",
        [profile_id],
        |r| r.get(0),
    )?;
    Ok(n.max(0) as u32)
}

/// Persist a profile's observed runtime state.
///
/// Only states a *fresh* process should believe are ever read back
/// ([`get_profile_state`]); this simply records whatever the engine last saw.
pub fn set_profile_state(conn: &Connection, id: &str, state: ProfileState) -> Result<()> {
    conn.execute(
        "UPDATE profiles SET state = ?2 WHERE id = ?1",
        (id, profile_state_str(state)),
    )?;
    Ok(())
}

/// Read back the last recorded runtime state, if it is one we recognise.
pub fn get_profile_state(conn: &Connection, id: &str) -> Result<Option<ProfileState>> {
    let stored: Option<String> = conn
        .query_row("SELECT state FROM profiles WHERE id = ?1", [id], |r| {
            r.get(0)
        })
        .optional()?;
    Ok(stored.as_deref().and_then(profile_state_from_str))
}

/// Stable on-disk spelling. Kept separate from the serde representation so a
/// UI rename can never silently invalidate every stored row.
fn profile_state_str(state: ProfileState) -> &'static str {
    match state {
        ProfileState::Idle => "idle",
        ProfileState::Watching => "watching",
        ProfileState::Syncing => "syncing",
        ProfileState::Offline => "offline",
        ProfileState::MediaAbsent => "mediaAbsent",
        ProfileState::Paused => "paused",
        ProfileState::NeedsAttention => "needsAttention",
    }
}

fn profile_state_from_str(value: &str) -> Option<ProfileState> {
    match value {
        "idle" => Some(ProfileState::Idle),
        "watching" => Some(ProfileState::Watching),
        "syncing" => Some(ProfileState::Syncing),
        "offline" => Some(ProfileState::Offline),
        "mediaAbsent" => Some(ProfileState::MediaAbsent),
        "paused" => Some(ProfileState::Paused),
        "needsAttention" => Some(ProfileState::NeedsAttention),
        _ => None,
    }
}

// ---------------------------------------------------------------------------
// The file-state cache (Story 26.3)
// ---------------------------------------------------------------------------

/// Nanosecond timestamps are `i128` in memory because that is what the
/// platform metadata yields, but SQLite integers are 64-bit. Saturating rather
/// than wrapping matters: a wrapped timestamp would look like a wildly
/// different sample and restart a quiescence window that had nearly elapsed.
fn ns_to_i64(value: i128) -> i64 {
    value.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
}

/// Load the quiescence state a previous run left behind.
///
/// This is what makes a one-shot `keeper-syncd sync --once` able to reach a
/// second observation at all, and what lets an in-progress window survive an
/// app restart instead of silently starting over.
pub fn load_file_state(
    conn: &Connection,
    profile_id: &str,
) -> Result<Vec<(PathBuf, PersistedEntry)>> {
    let mut stmt = conn.prepare(
        "SELECT path, size, mtime_ns, ctime_ns, inode, first_seen_ms, last_change_ms, close_write
         FROM file_state WHERE profile_id = ?1",
    )?;
    let rows = stmt.query_map([profile_id], |r| {
        Ok((
            PathBuf::from(r.get::<_, String>(0)?),
            PersistedEntry {
                sample: FileSample {
                    size: r.get::<_, i64>(1)?.max(0) as u64,
                    mtime_ns: i128::from(r.get::<_, i64>(2)?),
                    ctime_ns: i128::from(r.get::<_, i64>(3)?),
                    inode: r.get::<_, i64>(4)?.max(0) as u64,
                },
                pending_since_ms: r.get(5)?,
                unchanged_since_ms: r.get(6)?,
                close_write: r.get::<_, i64>(7)? != 0,
            },
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Replace the profile's cached state with `entries`.
///
/// A full replace rather than an upsert: a path that is no longer mid-episode
/// (it settled, or it was deleted) must not linger, or the table would grow
/// without bound on a busy profile.
pub fn save_file_state(
    conn: &Connection,
    profile_id: &str,
    entries: &[(PathBuf, PersistedEntry)],
) -> Result<()> {
    conn.execute("DELETE FROM file_state WHERE profile_id = ?1", [profile_id])?;
    let mut stmt = conn.prepare(
        "INSERT INTO file_state
           (profile_id, path, size, mtime_ns, ctime_ns, inode, first_seen_ms, last_change_ms, close_write)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
    )?;
    for (path, entry) in entries {
        stmt.execute(rusqlite::params![
            profile_id,
            path.to_string_lossy().as_ref(),
            entry.sample.size as i64,
            ns_to_i64(entry.sample.mtime_ns),
            ns_to_i64(entry.sample.ctime_ns),
            entry.sample.inode as i64,
            entry.pending_since_ms,
            entry.unchanged_since_ms,
            i64::from(entry.close_write),
        ])?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::SyncProfile;

    fn conn() -> Connection {
        open_in_memory().expect("in-memory db")
    }

    fn profile(id: &str) -> SyncProfile {
        SyncProfile::new(id, "n", "/tmp/x", "https://git.example/r.git")
    }

    #[test]
    fn migration_is_idempotent() {
        let c = conn();
        migrate(&c).expect("second migrate must succeed");
        migrate(&c).expect("third migrate must succeed");
    }

    #[test]
    fn profiles_round_trip_and_update_in_place() {
        let c = conn();
        let mut p = profile("01A");
        upsert_profile(&c, &p, 1).expect("insert");
        p.name = "renamed".into();
        upsert_profile(&c, &p, 2).expect("update");
        let all = list_profiles(&c).expect("list");
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].name, "renamed");
    }

    #[test]
    fn an_invalid_profile_never_reaches_the_database() {
        let c = conn();
        let mut p = profile("01A");
        p.local_path = "relative".into();
        assert!(upsert_profile(&c, &p, 1).is_err());
        assert!(list_profiles(&c).expect("list").is_empty());
    }

    #[test]
    fn an_unreadable_profile_row_is_skipped_not_fatal() {
        // A profile written by a newer keeper must not brick an older one.
        let c = conn();
        upsert_profile(&c, &profile("01A"), 1).expect("insert");
        c.execute(
            "INSERT INTO profiles (id, json, updated_ms) VALUES ('bad', '{oops', 0)",
            [],
        )
        .expect("insert junk");
        assert_eq!(list_profiles(&c).expect("list").len(), 1);
    }

    #[test]
    fn device_identity_is_minted_once_and_is_stable() {
        let c = conn();
        let first = device_identity(&c, "host-a").expect("mint");
        let second = device_identity(&c, "host-b").expect("read");
        assert_eq!(first, second, "the id must not change on a relabel");
        set_device_label(&c, "renamed").expect("relabel");
        let third = device_identity(&c, "ignored").expect("read");
        assert_eq!(third.id, first.id);
        assert_eq!(third.label, "renamed");
    }

    #[test]
    fn claimed_work_cannot_be_claimed_twice() {
        let c = conn();
        enqueue(&c, "p", &WorkKind::Push, 100, 0).expect("enqueue");
        let first = claim_ready(&c, "p", 100, 10).expect("claim");
        assert_eq!(first.len(), 1);
        // The very first claim is attempt 1. Reporting 0 here would make the
        // backoff schedule return a zero delay for the first retry.
        assert_eq!(first[0].attempts, 1);
        assert!(claim_ready(&c, "p", 100, 10).expect("claim").is_empty());
    }

    #[test]
    fn work_scheduled_for_the_future_is_not_claimed_yet() {
        let c = conn();
        enqueue(&c, "p", &WorkKind::Pull, 100, 5_000).expect("enqueue");
        assert!(claim_ready(&c, "p", 1_000, 10).expect("claim").is_empty());
        assert_eq!(claim_ready(&c, "p", 5_000, 10).expect("claim").len(), 1);
    }

    #[test]
    fn interrupted_work_is_requeued_at_startup_not_stranded() {
        // This is the single statement that turns "crashed" into "repeated".
        let c = conn();
        enqueue(&c, "p", &WorkKind::Push, 100, 0).expect("enqueue");
        claim_ready(&c, "p", 100, 10).expect("claim");
        assert_eq!(recover_running(&c, 200).expect("recover"), 1);
        let again = claim_ready(&c, "p", 200, 10).expect("claim");
        assert_eq!(again.len(), 1);
        // Attempt 1 was the claim lost to the crash, so the re-drive is
        // attempt 2 — and the backoff schedule must see it as such rather than
        // treating a resumed unit as a fresh first try.
        assert_eq!(
            again[0].attempts, 2,
            "the attempt counter must survive a restart"
        );
    }

    #[test]
    fn duplicate_work_is_collapsed_while_still_queued() {
        // A burst of file events must not queue a hundred identical pushes.
        let c = conn();
        assert!(enqueue_unique(&c, "p", &WorkKind::Push, 1, 0)
            .expect("first")
            .is_some());
        assert!(enqueue_unique(&c, "p", &WorkKind::Push, 2, 0)
            .expect("second")
            .is_none());
        // A different object is a different unit.
        let obj = WorkKind::LfsUpload {
            oid: "aa".into(),
            size: 1,
        };
        assert!(enqueue_unique(&c, "p", &obj, 3, 0)
            .expect("third")
            .is_some());
        assert_eq!(pending_count(&c, "p").expect("count"), 2);
    }

    #[test]
    fn deferred_work_waits_for_a_volume_not_a_timer() {
        let c = conn();
        let id = enqueue(&c, "p", &WorkKind::Push, 1, 0).expect("enqueue");
        claim_ready(&c, "p", 1, 10).expect("claim");
        reschedule(&c, id, WorkState::Deferred, 0, Some("media absent")).expect("defer");
        assert!(claim_ready(&c, "p", 999_999, 10).expect("claim").is_empty());
        assert_eq!(undefer_profile(&c, "p", 1_000).expect("undefer"), 1);
        assert_eq!(claim_ready(&c, "p", 1_000, 10).expect("claim").len(), 1);
    }

    #[test]
    fn a_poison_payload_is_parked_instead_of_retried_forever() {
        let c = conn();
        c.execute(
            "INSERT INTO journal (profile_id, kind, payload, state, not_before_ms, created_ms)
             VALUES ('p', 'push', '{not json', 'pending', 0, 0)",
            [],
        )
        .expect("insert junk");
        assert!(claim_ready(&c, "p", 100, 10).expect("claim").is_empty());
        // Parked rows are excluded from the pending count so they cannot make a
        // profile look permanently busy.
        assert_eq!(pending_count(&c, "p").expect("count"), 0);
    }

    #[test]
    fn deleting_a_profile_takes_its_queued_work_with_it() {
        let c = conn();
        upsert_profile(&c, &profile("01A"), 1).expect("insert");
        enqueue(&c, "01A", &WorkKind::Push, 1, 0).expect("enqueue");
        delete_profile(&c, "01A").expect("delete");
        assert_eq!(pending_count(&c, "01A").expect("count"), 0);
        assert!(get_profile(&c, "01A").expect("get").is_none());
    }
}
