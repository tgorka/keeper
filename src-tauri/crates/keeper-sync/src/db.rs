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

use std::collections::BTreeSet;

use rusqlite::{Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

use crate::error::{Result, SyncError};
use crate::profile::{self, ProfileState, SyncProfile};
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

        -- Recently synced files (Story 32.1). Deliberately append-only and
        -- bounded: it is a human-facing log, not a source of truth, so a
        -- profile that syncs a million files must not grow `sync.db` without
        -- limit. Rows hold repository-relative paths, never content.
        --
        -- `size_bytes` (Story 34.6) and `unit_id` (Story 34.16) are missing
        -- here on purpose: `ensure_activity_columns` adds them, so a fresh
        -- install and one that predates either column reach the same schema
        -- down one path.
        CREATE TABLE IF NOT EXISTS activity (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            profile_id  TEXT NOT NULL,
            ts_ms       INTEGER NOT NULL,
            kind        TEXT NOT NULL,
            path        TEXT NOT NULL
        );
        -- Newest-first per profile is the only way this table is ever read,
        -- and it is also how the cap is trimmed. Ordering by `id` rather than
        -- `ts_ms` is load-bearing: a batch commit stamps every row with the
        -- same millisecond, so a timestamp sort has no defined order within it.
        CREATE INDEX IF NOT EXISTS activity_recent
            ON activity (profile_id, id DESC);

        CREATE TABLE IF NOT EXISTS device (
            singleton   INTEGER PRIMARY KEY CHECK (singleton = 0),
            id          TEXT NOT NULL,
            label       TEXT NOT NULL
        );

        -- Migrations that rewrite CONTENT rather than schema, and must
        -- therefore run exactly once. Schema migrations need no marker: an
        -- `ALTER TABLE ... ADD COLUMN` guarded by the column list is its own
        -- idempotence. A row rewrite is not, because after it runs the old
        -- value becomes a legitimate one again.
        -- Paths this clone has ever held real content for.
        --
        -- The only way to tell an arriving object that REPLACES something from
        -- one that is simply new here, and it cannot be derived: a queued
        -- download always finds pointer text in the worktree, and git history
        -- answers a different question — a file added a week ago and never
        -- fetched here is new to this machine however old it is upstream.
        --
        -- One row per path, written when content lands. Cheap to keep and the
        -- only fact that makes the distinction true.
        --
        -- `last_used_ms`, `synced_at_ms`, `pinned`, `oid` and `size_bytes`
        -- (Story 56.2) are missing here on purpose, exactly as `activity`'s
        -- late columns are: `ensure_materialized_columns` adds them, so a
        -- fresh install and one that predates them reach the same schema down
        -- one path.
        CREATE TABLE IF NOT EXISTS materialized (
            profile_id  TEXT NOT NULL,
            path        TEXT NOT NULL,
            at_ms       INTEGER NOT NULL,
            PRIMARY KEY (profile_id, path)
        );

        CREATE TABLE IF NOT EXISTS meta (
            key         TEXT PRIMARY KEY,
            value       TEXT NOT NULL
        );
        "#,
    )?;
    ensure_activity_columns(conn)?;
    ensure_journal_columns(conn)?;
    ensure_materialized_columns(conn)?;
    ensure_prune_default(conn)?;
    Ok(())
}

/// The marker naming the one-shot in [`ensure_prune_default`].
const PRUNE_DEFAULT_MARKER: &str = "lfs_prune_local_default_on";

/// Carry a store written before releasing the redundant LFS object copy became
/// the default (`lfs_prune_local`).
///
/// A changed serde default cannot reach an install that already exists: a
/// profile is stored as its serialization, and serialization writes every
/// field, so every row written before the change holds a literal
/// `"lfsPruneLocal": false`. Without this, the new default would apply to new
/// folders only — and the folders with a second copy worth reclaiming are
/// precisely the old ones.
///
/// **Exactly once**, marked in `meta`, because `false` has two meanings after
/// the change: the old default, which this rewrites, and a deliberate opt-out,
/// which must survive every later `open`. Running once is the only thing that
/// keeps them apart. `keeper-syncd` applies its `config.toml` *after* `open`,
/// so a profile that asks for `lfsPruneLocal = false` there is written back on
/// the same boot and stays that way.
///
/// It does not touch `updated_ms`: the operator changed nothing, and a folder
/// reporting an edit nobody made is a worse lie than a stale timestamp.
fn ensure_prune_default(conn: &Connection) -> Result<()> {
    let applied: Option<String> = conn
        .query_row(
            "SELECT value FROM meta WHERE key = ?1",
            [PRUNE_DEFAULT_MARKER],
            |row| row.get(0),
        )
        .optional()?;
    if applied.is_some() {
        return Ok(());
    }
    // `json_set` rather than a read-modify-write through serde: it preserves
    // every other key byte for byte, including ones written by a keeper newer
    // than this one, which a round trip through `SyncProfile` would drop.
    let moved = conn.execute(
        "UPDATE profiles
            SET json = json_set(json, '$.lfsPruneLocal', json('true'))
          WHERE json_extract(json, '$.lfsPruneLocal') = 0",
        [],
    )?;
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)",
        (PRUNE_DEFAULT_MARKER, moved.to_string()),
    )?;
    if moved > 0 {
        tracing::info!(
            profiles = moved,
            "releasing the redundant local LFS object copy is now the default",
        );
    }
    Ok(())
}

/// Add the late nullable columns `activity` has grown, if they are not there
/// yet: `size_bytes` (Story 34.6) and `unit_id` (Story 34.16).
///
/// Idempotent and non-destructive, the shape `keeper-core`'s registry already
/// uses for its own late columns: read the table's column list once and only
/// `ALTER TABLE ... ADD COLUMN` for what is missing. An install that predates
/// either column keeps every one of its capped rows, which read back `NULL` —
/// and `NULL` is a fact in both cases rather than a default. For `size_bytes`
/// it means "nobody measured it", never zero. For `unit_id` it means "no work
/// unit is accountable for this row", which is why [`DeliveryState::Unknown`]
/// exists instead of a cheerful assumption that the file arrived.
///
/// One PRAGMA for both columns rather than a function per column: the read is
/// the expensive half, and a second copy of this loop is how the two would
/// drift.
fn ensure_activity_columns(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(activity)")?;
    // Column 1 of `table_info` is the column name.
    let existing: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for column in ["size_bytes", "unit_id"] {
        if !existing.iter().any(|c| c == column) {
            conn.execute(
                &format!("ALTER TABLE activity ADD COLUMN {column} INTEGER"),
                [],
            )?;
        }
    }
    Ok(())
}

/// Add the late nullable columns `materialized` has grown, if they are not
/// there yet (Story 56.2).
///
/// Five facts the ledger has to hold before anything can decide what to
/// release: when the content was last *read* (`last_used_ms`), when the remote
/// last confirmed it holds the object (`synced_at_ms`), whether the owner has
/// asked for this path to stay on the machine (`pinned`), and the object's
/// identity and length (`oid`, `size_bytes`) so a row still answers after the
/// worktree stops holding a pointer to consult.
///
/// Literally [`ensure_activity_columns`]'s shape, including the `drop(stmt)`
/// before the first `conn.execute` — `rusqlite` holds the connection while a
/// prepared statement lives, so an `ALTER TABLE` issued with the PRAGMA
/// statement still alive is a borrow error, not a runtime surprise. The one
/// difference is that these five columns are not all the same type, so the
/// loop carries the type beside the name rather than hard-coding `INTEGER`.
///
/// **Nullable and without a `DEFAULT`, and no `meta` marker.** `NULL` is the
/// honest reading of every one of them on a pre-existing row: nobody measured
/// a last use, no remote confirmation was recorded, nothing was pinned. An
/// `ALTER TABLE ... ADD COLUMN` guarded by the column list is its own
/// idempotence — the rule stated at the top of [`migrate`] — so a second
/// `migrate` on the same connection adds nothing and errors on nothing.
///
/// `pinned` is an `INTEGER` read as a boolean, which is how SQLite spells one;
/// [`materialized_rows`] narrows it, so no caller sees the encoding.
fn ensure_materialized_columns(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(materialized)")?;
    // Column 1 of `table_info` is the column name.
    let existing: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    for (column, kind) in [
        ("last_used_ms", "INTEGER"),
        ("synced_at_ms", "INTEGER"),
        ("pinned", "INTEGER"),
        ("oid", "TEXT"),
        ("size_bytes", "INTEGER"),
    ] {
        if !existing.iter().any(|c| c == column) {
            conn.execute(
                &format!("ALTER TABLE materialized ADD COLUMN {column} {kind}"),
                [],
            )?;
        }
    }
    Ok(())
}

/// Add `label`, the journal's one late column, if it is not there yet.
///
/// A unit's payload says what work to do; the label says what to *call* it
/// while it is being done. They are separate columns because they have
/// different identities: [`enqueue_unique`] deduplicates on the payload string,
/// so folding a display name into it would make two paths that share one object
/// — the ordinary case for duplicated content — into two downloads of the same
/// bytes.
///
/// Nullable, and read as "no better name than the work itself". A row written
/// before this column existed, or by a code path that has no path to offer,
/// reads back `NULL` and the UI falls back to the profile name.
fn ensure_journal_columns(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(journal)")?;
    let existing: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    if !existing.iter().any(|c| c == "label") {
        conn.execute("ALTER TABLE journal ADD COLUMN label TEXT", [])?;
    }
    Ok(())
}

/// Name a queued unit, if it does not have a name yet.
///
/// Separate from [`enqueue_unique`] rather than an argument to it, because the
/// unit that comes back may be one queued earlier: the caller is naming
/// *whatever will deliver this work*, which is exactly the covering unit that
/// function returns. `label IS NULL` makes the first path win — with several
/// paths sharing one object any of them is a truthful thing to display, and a
/// last-one-wins race would make the line flicker between them.
pub fn label_unit(conn: &Connection, id: i64, label: &str) -> Result<()> {
    conn.execute(
        "UPDATE journal SET label = ?2 WHERE id = ?1 AND label IS NULL",
        (id, label),
    )?;
    Ok(())
}

/// Every download this profile is still waiting for: its name, object and size.
///
/// Downloads only. An upload is also unfinished work, but the path it carries
/// is already reported by `git status` as a local change, and listing it twice
/// under two different sentences would be one fact wearing two hats.
pub fn queued_downloads(
    conn: &Connection,
    profile_id: &str,
) -> Result<Vec<(Option<String>, String, u64)>> {
    let mut stmt = conn.prepare(
        "SELECT label,
                json_extract(payload, '$.oid'),
                json_extract(payload, '$.size')
           FROM journal
          WHERE profile_id = ?1
            AND kind = 'lfsDownload'
            AND state != 'parked'
          ORDER BY id",
    )?;
    let rows = stmt.query_map([profile_id], |r| {
        Ok((
            r.get::<_, Option<String>>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (label, oid, size) = row?;
        out.push((label, oid, size.max(0) as u64));
    }
    Ok(out)
}

/// Record that content for `path` now exists on this machine.
///
/// Called when an object is materialized, which is the one moment the fact is
/// known. Materializing again is the ordinary case — a second version arriving
/// — and the newest timestamp is the useful one, so the conflicting row is
/// updated rather than rejected.
///
/// # Why this is an upsert and not `INSERT OR REPLACE`
///
/// It was `INSERT OR REPLACE` until Story 56.2, and that spelling became a
/// data-loss bug the moment this table grew a column: under the
/// `(profile_id, path)` primary key SQLite resolves a REPLACE by **deleting
/// the conflicting row and inserting a fresh one**, so every column this
/// statement does not name is silently reset to `NULL`. The first
/// re-materialization after [`ensure_materialized_columns`] ran would have
/// blanked `pinned`, `synced_at_ms` and `last_used_ms` — and `pinned` is the
/// hard floor a release sweep is not allowed to cross, so the loss would be
/// invisible until content the owner asked to keep was gone.
///
/// Naming `at_ms` explicitly in the `DO UPDATE` is therefore the whole point:
/// this function knows exactly one fact, and it is now incapable of touching
/// any other.
pub fn remember_materialized(
    conn: &Connection,
    profile_id: &str,
    path: &str,
    now_ms: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO materialized (profile_id, path, at_ms) VALUES (?1, ?2, ?3)
         ON CONFLICT(profile_id, path) DO UPDATE SET at_ms = excluded.at_ms",
        (profile_id, path, now_ms),
    )?;
    Ok(())
}

/// Every path this clone has held content for.
///
/// Read whole rather than asked per row: the caller is deciding a mark for a
/// list, and one statement beats a query per line.
pub fn materialized_paths(
    conn: &Connection,
    profile_id: &str,
) -> Result<std::collections::HashSet<String>> {
    let mut stmt = conn.prepare("SELECT path FROM materialized WHERE profile_id = ?1")?;
    let rows = stmt.query_map([profile_id], |r| r.get::<_, String>(0))?;
    let mut out = std::collections::HashSet::new();
    for row in rows {
        out.insert(row?);
    }
    Ok(out)
}

/// One row of the `materialized` ledger, late columns included (Story 56.2).
///
/// A struct rather than a tuple because five of the seven fields are
/// `Option`s of two types and four of them are timestamps: a transposition
/// between `last_used_ms` and `synced_at_ms` would compile, pass every type
/// check, and make a release decision on the wrong fact.
///
/// **Every late field is an `Option` and that is a fact, not a placeholder.**
/// A row written before Story 56.2 — or by the one writer that exists,
/// [`remember_materialized`], which sets `at_ms` and nothing else — reads back
/// `None` for all of them, and `None` means "nobody recorded this" rather than
/// zero, epoch, or absent-from-the-remote. Nothing in this story writes them;
/// they exist so the reader and the schema arrive together instead of a
/// migration landing in a story that cannot observe it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaterializedRow {
    /// Repository-relative, `/`-joined — the same frame every other path in
    /// this database is in.
    pub path: String,
    /// When content for this path last landed here. The one column with a
    /// writer.
    pub at_ms: i64,
    /// When the content was last read through keeper.
    pub last_used_ms: Option<i64>,
    /// When the remote last confirmed it holds the object.
    pub synced_at_ms: Option<i64>,
    /// The object this path's content is, when the row recorded one.
    pub oid: Option<String>,
    /// How large that object is, when the row recorded it. Narrowed from
    /// SQLite's signed integer the same way `list_activity` narrows a size: a
    /// negative byte count is not a byte count, and it means exactly what
    /// `NULL` means.
    pub size_bytes: Option<u64>,
    /// Whether the owner has asked for this path to stay on this machine.
    /// `false` for every row that has never said otherwise, including every
    /// row written before the column existed — an unpinned path is the
    /// default, so absence and `0` are the same answer and an `Option` here
    /// would be a distinction nobody can act on.
    pub pinned: bool,
}

/// Every `materialized` row this profile has, whole.
///
/// Wider than [`materialized_paths`], and beside it rather than replacing it:
/// that one answers a single yes/no per path, which is all the arrival
/// decision needs and all it should be able to see, while this one carries the
/// columns a listing reports. Read in one statement for the same reason it is:
/// the caller is building a list, and a query per row is how a folder of a
/// hundred thousand paths becomes a minute of SQLite.
pub fn materialized_rows(conn: &Connection, profile_id: &str) -> Result<Vec<MaterializedRow>> {
    let mut stmt = conn.prepare(
        "SELECT path, at_ms, last_used_ms, synced_at_ms, oid, size_bytes, pinned
           FROM materialized
          WHERE profile_id = ?1
          ORDER BY path",
    )?;
    let rows = stmt.query_map([profile_id], |r| {
        Ok((
            r.get::<_, String>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, Option<i64>>(2)?,
            r.get::<_, Option<i64>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<i64>>(5)?,
            r.get::<_, Option<i64>>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (path, at_ms, last_used_ms, synced_at_ms, oid, size, pinned) = row?;
        out.push(MaterializedRow {
            path,
            at_ms,
            last_used_ms,
            synced_at_ms,
            oid,
            // A row from before the column existed reads back `NULL`, and a
            // negative size is not a size: both mean the same thing here.
            size_bytes: size.and_then(|bytes| u64::try_from(bytes).ok()),
            pinned: pinned.unwrap_or(0) != 0,
        });
    }
    Ok(out)
}

/// Queued transfers that have no name yet, as oid → the units wanting it.
///
/// Every row queued before the `label` column existed is in here, which on an
/// install that upgraded mid-backlog is the entire queue: naming happens when
/// work is enqueued, and that already happened. Without a way to fill them in
/// afterwards the feature would arrive for a folder that has nothing left to do.
///
/// Keyed by oid rather than by unit because the answer is found by walking the
/// index once and asking "does anything want this object", which is O(index)
/// instead of O(index × units).
pub fn unlabelled_transfers(
    conn: &Connection,
    profile_id: &str,
) -> Result<std::collections::BTreeMap<String, Vec<i64>>> {
    let mut stmt = conn.prepare(
        "SELECT id, json_extract(payload, '$.oid') FROM journal
          WHERE profile_id = ?1
            AND state != 'parked'
            AND label IS NULL
            AND json_extract(payload, '$.oid') IS NOT NULL",
    )?;
    let rows = stmt.query_map([profile_id], |r| {
        Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?))
    })?;
    let mut out: std::collections::BTreeMap<String, Vec<i64>> = std::collections::BTreeMap::new();
    for row in rows {
        let (id, oid) = row?;
        out.entry(oid).or_default().push(id);
    }
    Ok(out)
}

/// How much transferable work is left for one profile: units, and their bytes.
///
/// Counted together in one statement so the two numbers can never disagree,
/// and restricted to rows that carry a size — a push or a commit is real work
/// but it is not a file, and counting it under "files left" would be a lie
/// told in the same breath as a byte total it contributes nothing to.
///
/// `parked` is excluded for the same reason it is excluded from `pending`: a
/// parked unit is not waiting, it has stopped.
pub fn queued_transfers(conn: &Connection, profile_id: &str) -> Result<(u32, u64)> {
    let (count, bytes): (i64, i64) = conn.query_row(
        "SELECT COUNT(*), COALESCE(SUM(json_extract(payload, '$.size')), 0)
           FROM journal
          WHERE profile_id = ?1
            AND state != 'parked'
            AND json_extract(payload, '$.size') IS NOT NULL",
        [profile_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    Ok((count.max(0) as u32, bytes.max(0) as u64))
}

// ---------------------------------------------------------------------------
// Device identity (Story 23.4)
// ---------------------------------------------------------------------------

/// This installation's identity.
///
/// Two halves that age differently. The `id` is minted once and never moves: it
/// is what `git::commit::author_for` derives the non-routable author address
/// from, and what a `Keeper-Device` trailer records beside the label so two
/// machines both called "MacBook Pro" stay distinguishable in a shared history.
/// The `label` is the human name, editable at any time (Story 34.5), and it is
/// what a conflict copy is named after.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeviceIdentity {
    pub id: String,
    pub label: String,
}

/// Read the device identity, minting it once on first call.
///
/// `default_label` seeds the label on the very first call and is ignored after
/// that, because the label is the user's from then on — see
/// [`set_device_label`]. The `CHECK (singleton = 0)` primary key makes "there is
/// exactly one device row" a schema invariant rather than a convention someone
/// can violate.
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

/// Rename this device, returning the label as stored.
///
/// The label rides every commit's `Keeper-Device` trailer and names the machine
/// in conflict-copy filenames, so a rename changes what commits made **from now
/// on** say and nothing about the ones already written: history is not rewritten
/// to match a new name, and a `git log` from before the rename stays true to the
/// machine as it was called then.
///
/// The id is deliberately not a parameter. It is the stable identity — moving it
/// would re-point the author address and make one machine read as two across a
/// shared history — so a rename cannot touch it even by accident.
///
/// Validation runs first, the way [`upsert_profile`] validates a profile, so no
/// route can store a label that makes a trailer meaningless: an empty one would
/// print `Keeper-Device:  (01J…)` on every commit from here on. The stored
/// (trimmed) label comes back so a caller holding an in-memory copy does not
/// have to re-derive the normalization and risk disagreeing with the row.
pub fn set_device_label(conn: &Connection, label: &str) -> Result<String> {
    let trimmed = label.trim();
    if trimmed.is_empty() {
        return Err(SyncError::Config("device label must not be empty".into()));
    }
    conn.execute(
        "UPDATE device SET label = ?1 WHERE singleton = 0",
        [trimmed],
    )?;
    Ok(trimmed.to_owned())
}

// ---------------------------------------------------------------------------
// Profiles
// ---------------------------------------------------------------------------

/// Forget every remembered path for one profile, leaving the profile itself.
///
/// Split out of [`delete_profile`]'s own delete because the two mean different
/// things: that one is tearing the profile down, this one is telling a profile
/// that still exists to stop trusting its memory of the tree. See
/// `Engine::rescan` for why that is ever the right thing to do.
pub fn clear_file_state(conn: &Connection, profile_id: &str) -> Result<usize> {
    Ok(conn.execute("DELETE FROM file_state WHERE profile_id = ?1", [profile_id])?)
}

/// Insert or replace a profile. Validation runs first so a bad profile can
/// never reach the database, whatever route it arrived by.
///
/// The folder tier is taken back off before the write (AD-98). Every read hands
/// out a profile with the folder's own `.keeper/*.toml` layered on, so without
/// [`profile::as_stored`] the first read-modify-write — `set_enabled` is the
/// plain one — would copy the file's values into the row, and deleting the file
/// later would reveal a copy of it rather than the value the user chose. That
/// is the `config.json` failure AD-98 exists to end, and this is the one write
/// funnel it can be prevented at.
pub fn upsert_profile(conn: &Connection, profile: &SyncProfile, now_ms: i64) -> Result<()> {
    profile.validate()?;
    let profile = profile::as_stored(profile, stored_profile(conn, &profile.id)?.as_ref());
    let json = serde_json::to_string(&profile)
        .map_err(|e| SyncError::Config(format!("profile is not serializable: {e}")))?;
    conn.execute(
        "INSERT INTO profiles (id, json, updated_ms) VALUES (?1, ?2, ?3)
         ON CONFLICT(id) DO UPDATE SET json = ?2, updated_ms = ?3",
        (&profile.id, &json, now_ms),
    )?;
    Ok(())
}

/// Every profile **as it is in force**: the stored row with the folder's own
/// config layered on top (Story 46.8).
pub fn list_profiles(conn: &Connection) -> Result<Vec<SyncProfile>> {
    let mut stmt = conn.prepare("SELECT json FROM profiles ORDER BY id")?;
    let rows = stmt.query_map([], |r| r.get::<_, String>(0))?;
    let mut out = Vec::new();
    for row in rows {
        let json = row?;
        match serde_json::from_str::<SyncProfile>(&json) {
            Ok(profile) => out.push(profile::in_force(profile)),
            // A profile written by a newer keeper must not brick an older one.
            // Skip it loudly rather than failing the whole listing.
            Err(err) => tracing::warn!(error = %err, "skipping unreadable sync profile row"),
        }
    }
    Ok(out)
}

/// One profile as it is in force, exactly as [`list_profiles`] reports it.
pub fn get_profile(conn: &Connection, id: &str) -> Result<Option<SyncProfile>> {
    Ok(stored_profile(conn, id)?.map(profile::in_force))
}

/// One profile as the **table** holds it, with no folder layer on top.
///
/// Private, and the only two callers are the ones that must not see the file:
/// [`get_profile`], which layers it itself, and [`upsert_profile`], which needs
/// the pre-write row to restore the fields a folder file owns.
fn stored_profile(conn: &Connection, id: &str) -> Result<Option<SyncProfile>> {
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

/// Delete a profile and every journal/file-state/activity row belonging to it.
///
/// Deliberately not a foreign-key cascade: the journal is intentionally
/// decoupled so a corrupt profile row can never take pending work with it.
pub fn delete_profile(conn: &Connection, id: &str) -> Result<()> {
    conn.execute("DELETE FROM journal WHERE profile_id = ?1", [id])?;
    conn.execute("DELETE FROM file_state WHERE profile_id = ?1", [id])?;
    // Otherwise a re-created profile reusing the id would inherit the deleted
    // one's history, and a removed profile would leave its file names behind.
    conn.execute("DELETE FROM activity WHERE profile_id = ?1", [id])?;
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
    /// The `kind` column spelling for a push, named so a query can filter for
    /// it without owning a [`WorkKind`] value to ask.
    pub const PUSH: &'static str = "push";
    /// The `kind` column spelling for one LFS object upload. Same reason.
    pub const LFS_UPLOAD: &'static str = "lfsUpload";
    /// The `kind` column spelling for a pull. Same reason — a rejected push
    /// queues one and has to be able to assert it did (DW-207).
    pub const PULL: &'static str = "pull";

    /// Whether an identical unit ALREADY RUNNING covers this work.
    ///
    /// A transfer is content-addressed: `LfsDownload { oid, size }` names
    /// immutable bytes, so a second unit for an object already in flight can
    /// only fetch what the first one is fetching. A push is the opposite — it
    /// publishes whatever the worktree holds *when it runs*, so a change made
    /// after it started genuinely needs its own unit, and treating the running
    /// one as cover would drop that change until something else queued a push.
    ///
    /// That asymmetry is why [`enqueue_unique`] ignored `running` for
    /// everything: correct for pushes, and quietly wrong for transfers.
    /// Measured on a folder pulling 53 GB — 106 queued units for 95 distinct
    /// objects, every duplicate a `running` object re-queued by the next scan.
    /// The visible symptom is a queue that never shrinks; the invisible one is
    /// the same bytes fetched twice.
    pub fn covered_while_running(&self) -> bool {
        matches!(self, Self::LfsUpload { .. } | Self::LfsDownload { .. })
    }

    /// Discriminant used as the journal's `kind` column, so a row can be
    /// filtered without deserializing its payload.
    pub fn tag(&self) -> &'static str {
        match self {
            Self::Pull => Self::PULL,
            Self::Push => Self::PUSH,
            Self::LfsUpload { .. } => Self::LFS_UPLOAD,
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
    /// What to call this unit while it runs — a repository-relative path, set
    /// by whoever queued it and never an absolute one, because progress lines
    /// end up in screenshots. `None` where the work has no single file to name
    /// (a push carries many), and the line then says the profile's name alone,
    /// exactly as it did before this column existed.
    pub label: Option<String>,
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

/// Enqueue only if an equivalent pending unit is not already queued, and
/// return the id of the unit that now covers this work either way.
///
/// The watcher can produce a burst of events for one profile; without this a
/// hundred file writes would queue a hundred identical pushes.
///
/// The return value is the *covering* unit rather than "did I insert one":
/// every caller discarded the old `Option`, and an activity row needs the id of
/// whatever unit will actually deliver it — which for the second of a hundred
/// identical pushes is the one already queued (Story 34.16).
pub fn enqueue_unique(
    conn: &Connection,
    profile_id: &str,
    kind: &WorkKind,
    now_ms: i64,
    not_before_ms: i64,
) -> Result<i64> {
    let payload = serde_json::to_string(kind)
        .map_err(|e| SyncError::Journal(format!("work item is not serializable: {e}")))?;
    // `running` counts as cover only for work whose identity is its content —
    // see [`WorkKind::covered_while_running`] for the asymmetry and what
    // ignoring it cost.
    let sql = if kind.covered_while_running() {
        "SELECT id FROM journal
          WHERE profile_id = ?1 AND payload = ?2
            AND state IN ('pending','deferred','running')
          LIMIT 1"
    } else {
        "SELECT id FROM journal
          WHERE profile_id = ?1 AND payload = ?2 AND state IN ('pending','deferred')
          LIMIT 1"
    };
    let existing: Option<i64> = conn
        .query_row(sql, (profile_id, &payload), |r| r.get(0))
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    enqueue(conn, profile_id, kind, now_ms, not_before_ms)
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
        "SELECT id, payload, attempts, last_error, label FROM journal
         WHERE profile_id = ?1 AND state = 'pending' AND not_before_ms <= ?2
         ORDER BY id LIMIT ?3",
    )?;
    let rows = stmt.query_map((profile_id, now_ms, limit), |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
        ))
    })?;

    let mut claimed = Vec::new();
    for row in rows {
        let (id, payload, attempts, last_error, label) = row?;
        match serde_json::from_str::<WorkKind>(&payload) {
            Ok(kind) => claimed.push(WorkItem {
                id,
                profile_id: profile_id.to_owned(),
                kind,
                attempts: attempts.max(0).saturating_add(1) as u32,
                last_error,
                label,
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
    // Collapse what the return can reveal. Until `enqueue_unique` learned to
    // treat a running transfer as cover, an object being downloaded could be
    // queued a second time by the next scan; the pair is invisible while one
    // half is `running` and becomes two identical pending rows the moment this
    // statement runs. Identical payload is identical work, so the older row
    // keeps the queue's place and the copy goes.
    //
    // Both halves of the fix are needed: this one clears the queues that
    // already exist, and the enqueue rule stops new ones forming.
    let collapsed = conn.execute(
        "DELETE FROM journal
          WHERE state = 'pending'
            AND id NOT IN (
                SELECT MIN(id) FROM journal
                 WHERE state = 'pending'
                 GROUP BY profile_id, payload
            )",
        [],
    )?;
    if collapsed > 0 {
        tracing::info!(count = collapsed, "collapsed duplicate queued work");
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

/// Move one *kind* of deferred work back to pending, undoing the wait itself.
///
/// The narrow sibling of [`undefer_profile`], and the exit a push held by
/// [`crate::error::SyncError::LfsUploadPending`] needs: that push is waiting on
/// a condition inside this same journal — its own uploads — so the moment the
/// last of them clears, something has to say so. Nothing else does.
/// [`undefer_profile`] cannot: it releases *everything* deferred, including a
/// unit waiting on an absent volume, which would spend an attempt to be
/// re-deferred straight away.
///
/// `kind` is the [`WorkKind::tag`] spelling, so this filters on the indexed
/// `kind` column without deserializing a payload.
///
/// # Why the wait is refunded
///
/// [`claim_ready`] charged an attempt to make the try that ended in the
/// deferral, and the deferral wrote the reason it is waiting into `last_error`.
/// Neither describes anything once the condition has cleared, and leaving them
/// was two distinct lies at once:
///
/// * [`DeliveryState`]'s `from_journal` reads `"pending"` with `attempts > 0` as
///   [`DeliveryState::Failed`], so between the release and the next claim every
///   file the released push delivers reported **Failed** — with a `last_error`
///   describing a wait that had already ended. A held row is supposed to read
///   [`DeliveryState::InProgress`], which is what it read while it was still
///   deferred.
/// * [`crate::backoff::Backoff`] is a function of `attempts`, so a folder
///   continuously fed large files walked its push up to the ceiling one held
///   publication at a time, and its first genuinely transient failure then
///   waited minutes.
///
/// So the row is put back the way it was found, the way [`unpark`] already puts
/// a retried unit back: the reason is cleared and the attempt is refunded.
/// `MAX(attempts - 1, 0)` rather than `0` because the count is still the unit's
/// history — a push that failed twice for real and was then held once has one
/// genuine failure refunded to nobody.
pub fn undefer_kind(conn: &Connection, profile_id: &str, kind: &str, now_ms: i64) -> Result<usize> {
    Ok(conn.execute(
        "UPDATE journal
            SET state = 'pending', not_before_ms = ?3, last_error = NULL,
                attempts = MAX(attempts - 1, 0)
          WHERE profile_id = ?1 AND kind = ?2 AND state = 'deferred'",
        (profile_id, kind, now_ms),
    )?)
}

/// How many units of one kind this profile still owes, parked ones included.
///
/// Parked rows count deliberately, and that is the whole point of the query:
/// this answers "is it safe to publish a pointer whose object may not be on the
/// remote", and an upload that has *stopped* being retried is the strongest
/// possible no. Counting only live work would let the push proceed exactly when
/// the object is most certainly missing.
pub fn outstanding_count(conn: &Connection, profile_id: &str, kind: &str) -> Result<u32> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM journal WHERE profile_id = ?1 AND kind = ?2",
        (profile_id, kind),
        |r| r.get(0),
    )?;
    Ok(n.max(0) as u32)
}

/// Every LFS oid any journal unit still references, whatever its kind or state.
///
/// The question prune asks is not "how many" but "which", and it must be
/// conservative in one direction only: an oid that is listed here is refused,
/// so a payload this fails to parse must never silently drop out. Unknown
/// shapes are therefore skipped without erroring — a unit whose payload is not
/// JSON, or carries no `oid`, cannot be an LFS transfer and cannot pin an
/// object — while a parse that succeeds contributes its oid regardless of kind
/// or state. Parked, pending and in-flight all pin equally: the engine
/// re-cleans an object it still owes, so releasing one would be undone anyway.
pub fn referenced_oids(conn: &Connection, profile_id: &str) -> Result<BTreeSet<String>> {
    let mut stmt = conn.prepare("SELECT payload FROM journal WHERE profile_id = ?1")?;
    let rows = stmt.query_map((profile_id,), |r| r.get::<_, String>(0))?;
    let mut out = BTreeSet::new();
    for payload in rows {
        let Ok(payload) = payload else { continue };
        let Ok(value) = serde_json::from_str::<serde_json::Value>(&payload) else {
            continue;
        };
        if let Some(oid) = value.get("oid").and_then(|v| v.as_str()) {
            out.insert(oid.to_owned());
        }
    }
    Ok(out)
}

/// How many units of one kind are still being *worked* on — parked excluded.
///
/// The deliberate counterpart of [`outstanding_count`], and the two are not
/// interchangeable. That one answers "may a pointer be published", where a
/// parked upload is the strongest possible no; this one answers "is anything
/// still moving", where a parked upload is the strongest possible no as well —
/// but of the *opposite* sign. A push held behind an upload that has stopped
/// being retried is not syncing, it is stopped, and reporting it as
/// `ProfileState::Syncing` left a tray glyph and a folder pane both showing
/// healthy progress for a folder that had permanently ceased publishing. See
/// `Engine::record_failure`.
pub fn live_count(conn: &Connection, profile_id: &str, kind: &str) -> Result<u32> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM journal
          WHERE profile_id = ?1 AND kind = ?2 AND state != 'parked'",
        (profile_id, kind),
        |r| r.get(0),
    )?;
    Ok(n.max(0) as u32)
}

pub fn pending_count(conn: &Connection, profile_id: &str) -> Result<u32> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM journal WHERE profile_id = ?1 AND state != 'parked'",
        [profile_id],
        |r| r.get(0),
    )?;
    Ok(n.max(0) as u32)
}

/// One stopped unit, as the problems view needs it.
///
/// `kind` is the journal's own `kind` column rather than a re-serialized
/// [`WorkKind`]: a row parked *because* its payload was unreadable
/// ([`claim_ready`]) is precisely the row a human most needs to see, and
/// deserializing to describe it would drop exactly those.
#[derive(Debug, Clone)]
pub struct ParkedRow {
    pub id: i64,
    pub kind: String,
    pub attempts: u32,
    pub last_error: Option<String>,
}

/// Every unit this profile has given up on.
///
/// Parked work is deliberately excluded from [`pending_count`], so without
/// this query it has no surface at all: the profile looks idle while its work
/// sits stopped forever.
pub fn list_parked(conn: &Connection, profile_id: &str) -> Result<Vec<ParkedRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, kind, attempts, last_error FROM journal
         WHERE profile_id = ?1 AND state = 'parked'
         ORDER BY id",
    )?;
    let rows = stmt.query_map([profile_id], |r| {
        Ok(ParkedRow {
            id: r.get(0)?,
            kind: r.get(1)?,
            attempts: r.get::<_, i64>(2)?.max(0) as u32,
            last_error: r.get(3)?,
        })
    })?;
    let mut out = Vec::new();
    for row in rows {
        out.push(row?);
    }
    Ok(out)
}

/// Put one parked unit back in the queue, ready immediately.
///
/// Scoped to `profile_id` so one profile can never retry another's work, and
/// restricted to `state = 'parked'` so a retry racing the supervisor cannot
/// yank a `running` unit out from under it. `attempts` is left alone: the
/// count is the unit's history, and resetting it would hide from the next
/// failure just how long this has been going on.
///
/// Returns whether a row actually moved, which is the only way the caller can
/// tell "not parked" and "not yours" apart from success.
pub fn unpark(conn: &Connection, profile_id: &str, unit_id: i64) -> Result<bool> {
    let n = conn.execute(
        "UPDATE journal SET state = 'pending', not_before_ms = 0, last_error = NULL
         WHERE id = ?1 AND profile_id = ?2 AND state = 'parked'",
        (unit_id, profile_id),
    )?;
    Ok(n > 0)
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

/// Replace the profile's cached state with `entries`, atomically.
///
/// A full replace rather than an upsert: a path that is no longer mid-episode
/// (it settled, or it was deleted) must not linger, or the table would grow
/// without bound on a busy profile.
///
/// One transaction for the whole replace, for two separate reasons.
///
/// **Correctness.** The `DELETE` and the inserts are one edit of one fact —
/// "everything this profile is currently holding". Un-transacted, a crash or a
/// concurrent reader between them sees a profile that is holding *nothing*, and
/// every quiescence window in flight silently restarts from zero: each of those
/// files then needs two fresh observations before it can sync.
///
/// **Cost.** Without an enclosing transaction SQLite commits every statement on
/// its own, so N rows cost N+1 commits — each a WAL frame write and, under
/// `synchronous=NORMAL`, a page-cache flush. This runs up to four times per sync
/// pass and is largest exactly when the tree is busiest, which is the worst
/// possible place to pay per row.
pub fn save_file_state(
    conn: &Connection,
    profile_id: &str,
    entries: &[(PathBuf, PersistedEntry)],
) -> Result<()> {
    // `unchecked_transaction` rather than `transaction`, as in
    // [`record_activity`]: the engine holds this connection behind a `Mutex` and
    // hands out `&Connection`, so there is no `&mut` to be had — and no second
    // handle that could nest a transaction.
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM file_state WHERE profile_id = ?1", [profile_id])?;
    {
        let mut stmt = tx.prepare(
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
    }
    tx.commit()?;
    Ok(())
}

// ---------------------------------------------------------------------------
// Recent activity (Story 32.1)
// ---------------------------------------------------------------------------

/// How many activity rows one profile keeps.
///
/// The list answers "what did sync just do to my folder", which nobody scrolls
/// past a few hundred entries of. Bounding it here rather than at read time is
/// what keeps `sync.db` a fixed cost on a profile that syncs continuously for
/// months.
pub const ACTIVITY_CAP: usize = 500;

/// What happened to one path.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum ActivityKind {
    Added,
    Modified,
    Deleted,
    /// A conflict copy was written beside the canonical path (AD-43). The
    /// recorded path is the *copy*, which is the file the user has to deal
    /// with.
    Conflict,
}

impl ActivityKind {
    /// Stable on-disk spelling, kept separate from the serde representation so
    /// a UI-facing rename can never invalidate stored rows — the same split
    /// [`profile_state_str`] makes.
    fn as_str(self) -> &'static str {
        match self {
            Self::Added => "added",
            Self::Modified => "modified",
            Self::Deleted => "deleted",
            Self::Conflict => "conflict",
        }
    }

    /// Not a `FromStr` impl: this parses the *stored* spelling, which is
    /// deliberately not the same vocabulary as the serde one.
    fn from_stored(value: &str) -> Option<Self> {
        match value {
            "added" => Some(Self::Added),
            "modified" => Some(Self::Modified),
            "deleted" => Some(Self::Deleted),
            "conflict" => Some(Self::Conflict),
            _ => None,
        }
    }
}

/// How far one recorded file has got toward the remote (Story 34.16).
///
/// Derived, never stored. An activity row records what a commit did to a file
/// and names the journal unit that has to *deliver* it; the delivery answer is
/// then whatever that unit's row currently says, which is why it is read from
/// the journal on every list rather than written down and left to rot.
///
/// The mapping is the journal's own vocabulary, read honestly:
///
/// | journal row | delivery |
/// |---|---|
/// | gone | [`Self::Success`] — `complete` deletes a unit only after its effect is durable |
/// | `running` | [`Self::InProgress`] |
/// | `pending`, never attempted | [`Self::InProgress`] |
/// | `deferred` | [`Self::InProgress`] — waiting on a condition is waiting, not failing |
/// | `pending`, attempted at least once | [`Self::Failed`] — keeper is still retrying |
/// | `parked` | [`Self::Abandoned`] — keeper stopped; a human must ask again |
/// | no unit named at all | [`Self::Unknown`] |
///
/// `Deferred` sitting under `InProgress` is the entry worth defending: a push
/// held by [`crate::error::SyncError::LfsUploadPending`] has a `last_error`
/// spelling out what it is waiting for, and rendering that as a failure would
/// accuse keeper of breaking while it is in fact doing the careful thing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DeliveryState {
    /// The unit that had to deliver this file completed.
    Success,
    /// A unit is queued, running, or waiting on a condition.
    InProgress,
    /// A unit failed and is still being retried.
    Failed,
    /// A unit stopped being retried.
    Abandoned,
    /// No unit is accountable for this row: it predates the `unit_id` column,
    /// or nothing journaled ever owned it — a conflict copy the pull wrote, for
    /// instance, whose own publication belongs to a later commit.
    Unknown,
}

impl DeliveryState {
    /// Read the journal's answer for one activity row.
    ///
    /// Only reached when a journal row genuinely exists: the caller has already
    /// told "names no unit" and "the unit it named is gone" apart, because those
    /// two are [`Self::Unknown`] and [`Self::Success`] and neither has a state to
    /// read.
    fn from_journal(state: &str, attempts: i64) -> Self {
        match state {
            "running" => Self::InProgress,
            "deferred" => Self::InProgress,
            "parked" => Self::Abandoned,
            // A pending unit that has already been claimed once failed to get
            // through; one that has not is simply queued.
            "pending" if attempts > 0 => Self::Failed,
            "pending" => Self::InProgress,
            // A state this build does not know is not a claim we can make.
            other => {
                tracing::debug!(
                    state = other,
                    "unrecognised journal state for an activity row"
                );
                Self::Unknown
            }
        }
    }
}

/// One entry in the recently-synced list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ActivityRow {
    pub ts_ms: i64,
    pub kind: ActivityKind,
    /// Repository-relative, always. An absolute path would leak the user's
    /// home directory into a list that is rendered verbatim.
    pub path: String,
    /// How many bytes the file held, or `None` when that is not knowable: a
    /// row written before Story 34.6, or a deletion whose previous size the
    /// index no longer records. Never zero standing in for unknown — an empty
    /// file and an unmeasured one are different facts, and only one of them is
    /// worth showing a size for.
    pub size_bytes: Option<u64>,
    /// How far this file has got toward the remote. See [`DeliveryState`].
    pub delivery: DeliveryState,
    /// The last error recorded against the delivering unit, verbatim.
    ///
    /// Kept even when the delivery reads [`DeliveryState::InProgress`], because
    /// that is exactly when it is most useful: a unit being retried after a
    /// failure, and a unit deferred on a named condition, both carry the reason
    /// they are not done yet.
    pub failure: Option<String>,
    /// The delivering unit, present only while it still exists.
    ///
    /// This is the id [`unpark`] takes, which is what lets a file row offer the
    /// same Retry the problems view does — for the same unit, named by the file
    /// a human actually recognises.
    pub unit_id: Option<i64>,
}

/// One row to append to the recently-synced list.
///
/// A struct rather than the tuple this used to be: four positional fields, two
/// of them optional integers, is a call site nobody can read or safely reorder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActivityEntry {
    pub kind: ActivityKind,
    /// Repository-relative.
    pub path: String,
    /// `None` when nobody measured it.
    pub size_bytes: Option<u64>,
    /// The journal unit whose success delivers this file, when one owns it.
    pub unit_id: Option<i64>,
}

/// Append what a sync just did, then trim the profile back to [`ACTIVITY_CAP`].
///
/// One transaction for the whole batch: a commit of 400 files is one event,
/// and a crash halfway through recording it would leave a list claiming a
/// commit that only partly happened. The insert and the trim share the
/// transaction for the same reason — a reader must never see the table above
/// its cap.
pub fn record_activity(
    conn: &Connection,
    profile_id: &str,
    ts_ms: i64,
    rows: &[ActivityEntry],
) -> Result<()> {
    if rows.is_empty() {
        return Ok(());
    }
    // `unchecked_transaction` rather than `transaction`: the engine holds this
    // connection behind a `Mutex` and hands out `&Connection`, so there is no
    // `&mut` to be had — and no second handle that could nest a transaction.
    let tx = conn.unchecked_transaction()?;
    {
        let mut stmt = tx.prepare(
            "INSERT INTO activity (profile_id, ts_ms, kind, path, size_bytes, unit_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        )?;
        for entry in rows {
            // A size no `i64` can hold is not a size anyone can render, so it
            // is stored as unknown rather than wrapped into a wrong number.
            let size = entry.size_bytes.and_then(|bytes| i64::try_from(bytes).ok());
            stmt.execute((
                profile_id,
                ts_ms,
                entry.kind.as_str(),
                &entry.path,
                size,
                entry.unit_id,
            ))?;
        }
    }
    // Trim by `id`, never by `ts_ms`. Every row of one commit carries the same
    // millisecond, so a timestamp cutoff would either spare the whole batch or
    // delete all of it, and the cap would not hold.
    tx.execute(
        "DELETE FROM activity
         WHERE profile_id = ?1
           AND id <= COALESCE(
                 (SELECT id FROM activity WHERE profile_id = ?1
                  ORDER BY id DESC LIMIT 1 OFFSET ?2),
                 -1)",
        (profile_id, ACTIVITY_CAP as i64),
    )?;
    tx.commit()?;
    Ok(())
}

/// The newest `limit` entries for a profile, most recent first.
///
/// A row whose `kind` is not one this build understands is skipped rather than
/// fatal, the same tolerance [`list_profiles`] gives an unreadable profile: a
/// newer keeper's activity must not brick an older one's list.
///
/// The `LEFT JOIN` is what makes [`DeliveryState`] derivable in one read. It is
/// safe against id reuse because `journal.id` is `AUTOINCREMENT`, which SQLite
/// guarantees is monotonic even across deletes — so an activity row can never
/// find a *different* unit wearing the id its own unit had. The join is scoped
/// to the same profile anyway, which costs nothing and says so out loud.
pub fn list_activity(
    conn: &Connection,
    profile_id: &str,
    limit: usize,
) -> Result<Vec<ActivityRow>> {
    let mut stmt = conn.prepare(
        "SELECT a.ts_ms, a.kind, a.path, a.size_bytes, a.unit_id,
                j.state, j.attempts, j.last_error
         FROM activity a
         LEFT JOIN journal j
                ON j.id = a.unit_id AND j.profile_id = a.profile_id
         WHERE a.profile_id = ?1
         ORDER BY a.id DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map((profile_id, limit as i64), |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, String>(2)?,
            r.get::<_, Option<i64>>(3)?,
            r.get::<_, Option<i64>>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, Option<i64>>(6)?,
            r.get::<_, Option<String>>(7)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (ts_ms, kind, path, size, unit_id, state, attempts, last_error) = row?;
        // A row from before the column existed reads back `NULL`, and a
        // negative size is not a size: both mean the same thing here.
        let size_bytes = size.and_then(|bytes| u64::try_from(bytes).ok());
        // Three distinguishable cases, and only the middle one is a success:
        // no unit was ever named; a unit was named and its row is gone, which
        // is the only way a unit ever leaves the journal; a unit was named and
        // is still there.
        let delivery = match (unit_id, state.as_deref()) {
            (None, _) => DeliveryState::Unknown,
            (Some(_), None) => DeliveryState::Success,
            (Some(_), Some(state)) => DeliveryState::from_journal(state, attempts.unwrap_or(0)),
        };
        match ActivityKind::from_stored(&kind) {
            Some(kind) => out.push(ActivityRow {
                ts_ms,
                kind,
                path,
                size_bytes,
                delivery,
                failure: last_error,
                // A unit that has completed is gone, and reporting its id would
                // offer a Retry for work there is nothing left to retry.
                unit_id: state.is_some().then_some(unit_id).flatten(),
            }),
            None => tracing::debug!(kind, "skipping an unrecognised activity row"),
        }
    }
    Ok(out)
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
        assert_eq!(
            set_device_label(&c, "  renamed  ").expect("relabel"),
            "renamed",
            "the caller is told what was stored, not what was passed"
        );
        let third = device_identity(&c, "ignored").expect("read");
        // The id is what the author address and a shared history's device
        // attribution are derived from: a rename must not move it.
        assert_eq!(third.id, first.id);
        assert_eq!(third.label, "renamed");
    }

    #[test]
    fn an_empty_device_label_is_refused_rather_than_stored() {
        // `Keeper-Device:  (01J…)` on every commit from here on is worse than a
        // rejected rename the user can see and correct.
        let c = conn();
        let before = device_identity(&c, "host-a").expect("mint");
        for blank in ["", "   ", "\n"] {
            assert!(
                set_device_label(&c, blank).is_err(),
                "{blank:?} must be refused"
            );
        }
        assert_eq!(device_identity(&c, "ignored").expect("read"), before);
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
        let first = enqueue_unique(&c, "p", &WorkKind::Push, 1, 0).expect("first");
        let second = enqueue_unique(&c, "p", &WorkKind::Push, 2, 0).expect("second");
        assert_eq!(
            second, first,
            "the second caller is told which unit already covers its work, so an \
             activity row can name it"
        );
        // A different object is a different unit.
        let obj = WorkKind::LfsUpload {
            oid: "aa".into(),
            size: 1,
        };
        let third = enqueue_unique(&c, "p", &obj, 3, 0).expect("third");
        assert_ne!(third, first);
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

    /// An activity row nothing is accountable for — the shape every test that
    /// is not about delivery wants.
    fn unowned(kind: ActivityKind, path: &str, size_bytes: Option<u64>) -> ActivityEntry {
        ActivityEntry {
            kind,
            path: path.to_owned(),
            size_bytes,
            unit_id: None,
        }
    }

    #[test]
    fn activity_round_trips_newest_first_and_stays_per_profile() {
        let c = conn();
        record_activity(
            &c,
            "p",
            10,
            &[
                unowned(ActivityKind::Added, "a.txt", Some(4_096)),
                unowned(ActivityKind::Deleted, "b.txt", None),
            ],
        )
        .expect("record");
        record_activity(
            &c,
            "q",
            11,
            &[unowned(ActivityKind::Modified, "other.txt", Some(7))],
        )
        .expect("record");

        let rows = list_activity(&c, "p", 10).expect("list");
        assert_eq!(
            rows,
            vec![
                ActivityRow {
                    ts_ms: 10,
                    kind: ActivityKind::Deleted,
                    path: "b.txt".to_owned(),
                    size_bytes: None,
                    delivery: DeliveryState::Unknown,
                    failure: None,
                    unit_id: None,
                },
                ActivityRow {
                    ts_ms: 10,
                    kind: ActivityKind::Added,
                    path: "a.txt".to_owned(),
                    size_bytes: Some(4_096),
                    delivery: DeliveryState::Unknown,
                    failure: None,
                    unit_id: None,
                },
            ],
            "newest first, each size as it was recorded, and one profile never \
             sees another's files"
        );
    }

    #[test]
    fn an_activity_table_predating_the_late_columns_upgrades_in_place() {
        // The exact shape `sync.db` had before Story 34.6. An install carrying
        // rows of it has to come through the upgrade with every row intact —
        // recreating the table empty, or refusing the next insert, would both
        // read to a user as history that vanished. The same applies to `unit_id`
        // (Story 34.16), which this schema also predates.
        let c = Connection::open_in_memory().expect("in-memory db");
        c.execute_batch(
            "CREATE TABLE activity (
                 id          INTEGER PRIMARY KEY AUTOINCREMENT,
                 profile_id  TEXT NOT NULL,
                 ts_ms       INTEGER NOT NULL,
                 kind        TEXT NOT NULL,
                 path        TEXT NOT NULL
             );
             INSERT INTO activity (profile_id, ts_ms, kind, path)
             VALUES ('p', 5, 'added', 'old.txt');",
        )
        .expect("plant the pre-34.6 schema");

        migrate(&c).expect("migrate an existing install");

        assert_eq!(
            list_activity(&c, "p", 10).expect("list"),
            vec![ActivityRow {
                ts_ms: 5,
                kind: ActivityKind::Added,
                path: "old.txt".to_owned(),
                size_bytes: None,
                delivery: DeliveryState::Unknown,
                failure: None,
                unit_id: None,
            }],
            "the old row survives, reading back with no size rather than zero \
             and no delivery claim rather than a cheerful one"
        );

        // And the upgraded table records both late columns from here on.
        let unit = enqueue(&c, "p", &WorkKind::Push, 6, 0).expect("enqueue");
        record_activity(
            &c,
            "p",
            6,
            &[ActivityEntry {
                kind: ActivityKind::Modified,
                path: "old.txt".to_owned(),
                size_bytes: Some(12_288),
                unit_id: Some(unit),
            }],
        )
        .expect("record");
        let row = &list_activity(&c, "p", 10).expect("list")[0];
        assert_eq!(row.size_bytes, Some(12_288));
        assert_eq!(row.unit_id, Some(unit));
    }

    #[test]
    fn a_materialized_table_predating_the_late_columns_upgrades_in_place() {
        // The exact shape `sync.db` had before Story 56.2. A row of it records
        // that content for a path landed on this machine, which is the only
        // evidence distinguishing an arriving object that REPLACES something
        // from one that is simply new here — so losing one is losing that
        // distinction permanently, and recreating the table empty would lose
        // every one at once.
        let c = Connection::open_in_memory().expect("in-memory db");
        c.execute_batch(
            "CREATE TABLE materialized (
                 profile_id  TEXT NOT NULL,
                 path        TEXT NOT NULL,
                 at_ms       INTEGER NOT NULL,
                 PRIMARY KEY (profile_id, path)
             );
             INSERT INTO materialized (profile_id, path, at_ms)
             VALUES ('p', '40-media/clip.mp4', 1_700);",
        )
        .expect("plant the pre-56.2 schema");

        migrate(&c).expect("migrate an existing install");

        assert_eq!(
            materialized_rows(&c, "p").expect("read the ledger"),
            vec![MaterializedRow {
                path: "40-media/clip.mp4".to_owned(),
                at_ms: 1_700,
                last_used_ms: None,
                synced_at_ms: None,
                oid: None,
                size_bytes: None,
                pinned: false,
            }],
            "the old row survives with its timestamp untouched, and every late \
             column reads as absent rather than as zero, epoch or unpinned-by-record"
        );

        // A second `migrate` on the same connection is the upgrade path an
        // ordinary `open` takes on every launch. `ALTER TABLE ... ADD COLUMN`
        // guarded by the column list is its own idempotence, which is why this
        // schema addition needs no `meta` marker — assert it rather than trust
        // it, because the failure mode is a daemon that cannot start.
        migrate(&c).expect("migrating twice changes nothing");
        assert_eq!(
            materialized_rows(&c, "p").expect("read the ledger").len(),
            1
        );
        assert_eq!(
            materialized_paths(&c, "p").expect("read the paths"),
            std::collections::HashSet::from(["40-media/clip.mp4".to_owned()]),
            "the narrow reader the arrival decision uses still answers the same"
        );
    }

    /// Re-materializing a path moves `at_ms` and touches nothing else (Story
    /// 56.2).
    ///
    /// This is the hazard the story exists to close, and it is invisible by
    /// construction: `remember_materialized` was `INSERT OR REPLACE`, and under
    /// the `(profile_id, path)` primary key SQLite resolves a REPLACE by
    /// DELETING the conflicting row and inserting a fresh one — so every column
    /// the statement does not name comes back `NULL`. Nothing would have
    /// complained; the first re-download after this table grew a `pinned`
    /// column would simply have un-pinned the path, and 56.5's release sweep
    /// reads `pinned` as the one thing it may not cross.
    ///
    /// The row is planted with all five late columns filled, because a test that
    /// only checked `pinned` would pass against a statement that named `pinned`
    /// and forgot the rest.
    #[test]
    fn re_materializing_a_pinned_path_moves_only_its_timestamp() {
        let c = conn();
        remember_materialized(&c, "p", "40-media/clip.mp4", 1_700).expect("first landing");
        c.execute(
            "UPDATE materialized
                SET pinned = 1,
                    last_used_ms = 1_750,
                    synced_at_ms = 1_760,
                    oid = 'abc123',
                    size_bytes = 4_194_304
              WHERE profile_id = 'p' AND path = '40-media/clip.mp4'",
            [],
        )
        .expect("fill the late columns the way a later story will");

        remember_materialized(&c, "p", "40-media/clip.mp4", 9_900).expect("a second landing");

        assert_eq!(
            materialized_rows(&c, "p").expect("read the ledger"),
            vec![MaterializedRow {
                path: "40-media/clip.mp4".to_owned(),
                at_ms: 9_900,
                last_used_ms: Some(1_750),
                synced_at_ms: Some(1_760),
                oid: Some("abc123".to_owned()),
                size_bytes: Some(4_194_304),
                pinned: true,
            }],
            "only the timestamp moved: an upsert that named more than `at_ms`, \
             or a REPLACE that named less, fails here"
        );
    }

    #[test]
    fn the_activity_cap_trims_the_oldest_and_keeps_the_newest() {
        // Every row of one batch shares a timestamp, so trimming by time would
        // either spare all of them or delete all of them. This is why the cap
        // is enforced by id.
        let c = conn();
        let batch: Vec<ActivityEntry> = (0..ACTIVITY_CAP + 25)
            .map(|n| unowned(ActivityKind::Added, &format!("f{n}.txt"), Some(n as u64)))
            .collect();
        record_activity(&c, "p", 7, &batch).expect("record");

        let rows = list_activity(&c, "p", ACTIVITY_CAP * 2).expect("list");
        assert_eq!(rows.len(), ACTIVITY_CAP, "the table is bounded");
        assert_eq!(
            rows[0].path,
            format!("f{}.txt", ACTIVITY_CAP + 24),
            "the newest row survives"
        );
        assert_eq!(
            rows[ACTIVITY_CAP - 1].path,
            "f25.txt",
            "exactly the oldest 25 were dropped"
        );
    }

    #[test]
    fn an_unrecognised_activity_kind_is_skipped_not_fatal() {
        // A newer keeper's vocabulary must not brick an older one's list.
        let c = conn();
        record_activity(
            &c,
            "p",
            1,
            &[unowned(ActivityKind::Added, "good.txt", None)],
        )
        .expect("record");
        c.execute(
            "INSERT INTO activity (profile_id, ts_ms, kind, path)
             VALUES ('p', 2, 'teleported', 'weird.txt')",
            [],
        )
        .expect("insert junk");
        let rows = list_activity(&c, "p", 10).expect("list");
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, "good.txt");
    }

    #[test]
    fn deleting_a_profile_takes_its_activity_with_it() {
        // Otherwise a re-created profile reusing the id inherits a stranger's
        // file names.
        let c = conn();
        upsert_profile(&c, &profile("01A"), 1).expect("insert");
        record_activity(&c, "01A", 1, &[unowned(ActivityKind::Added, "a.txt", None)])
            .expect("record");
        delete_profile(&c, "01A").expect("delete");
        assert!(list_activity(&c, "01A", 10).expect("list").is_empty());
    }

    /// The journal is the delivery answer, and every state it can be in has to
    /// map to something a file row can say without lying.
    #[test]
    fn a_file_reports_the_state_of_the_unit_that_has_to_deliver_it() {
        let c = conn();
        let unit = enqueue(&c, "p", &WorkKind::Push, 1, 0).expect("enqueue");
        record_activity(
            &c,
            "p",
            1,
            &[ActivityEntry {
                kind: ActivityKind::Added,
                path: "big.bin".to_owned(),
                size_bytes: Some(9),
                unit_id: Some(unit),
            }],
        )
        .expect("record");

        let delivery = |c: &Connection| list_activity(c, "p", 10).expect("list")[0].clone();

        // Queued and never attempted: on its way, with nothing to report.
        let row = delivery(&c);
        assert_eq!(row.delivery, DeliveryState::InProgress);
        assert_eq!(row.failure, None);
        assert_eq!(row.unit_id, Some(unit));

        // Claimed: still on its way.
        claim_ready(&c, "p", 1, 10).expect("claim");
        assert_eq!(delivery(&c).delivery, DeliveryState::InProgress);

        // Failed and being retried. This is the state the row must NOT call
        // success, and must not call abandoned either.
        reschedule(&c, unit, WorkState::Pending, 5, Some("connection reset")).expect("retry");
        let row = delivery(&c);
        assert_eq!(row.delivery, DeliveryState::Failed);
        assert_eq!(row.failure.as_deref(), Some("connection reset"));

        // Deferred is a wait, not a failure — a push held for its own uploads
        // lands here, and reading it as broken would accuse keeper of failing
        // while it is doing the careful thing.
        reschedule(
            &c,
            unit,
            WorkState::Deferred,
            5,
            Some("publishing is on hold"),
        )
        .expect("defer");
        let row = delivery(&c);
        assert_eq!(row.delivery, DeliveryState::InProgress);
        assert_eq!(row.failure.as_deref(), Some("publishing is on hold"));

        // Parked: keeper stopped, so the row offers the unit for a retry.
        reschedule(&c, unit, WorkState::Parked, 5, Some("bad credentials")).expect("park");
        let row = delivery(&c);
        assert_eq!(row.delivery, DeliveryState::Abandoned);
        assert_eq!(row.failure.as_deref(), Some("bad credentials"));
        assert_eq!(row.unit_id, Some(unit));

        // Completed. The unit leaves the journal, which is the ONLY way it ever
        // does, so its absence is the success signal — and there is nothing left
        // to retry, so no id is offered.
        complete(&c, unit).expect("complete");
        let row = delivery(&c);
        assert_eq!(row.delivery, DeliveryState::Success);
        assert_eq!(row.failure, None);
        assert_eq!(row.unit_id, None);
    }

    #[test]
    fn one_kind_of_deferred_work_can_be_released_without_disturbing_the_rest() {
        // A push held for its own uploads must come back when they land, and an
        // upload held for an absent volume must NOT be woken to fail again.
        let c = conn();
        let push = enqueue(&c, "p", &WorkKind::Push, 1, 0).expect("push");
        let upload = enqueue(
            &c,
            "p",
            &WorkKind::LfsUpload {
                oid: "aa".into(),
                size: 1,
            },
            1,
            0,
        )
        .expect("upload");
        claim_ready(&c, "p", 1, 10).expect("claim");
        reschedule(&c, push, WorkState::Deferred, 0, Some("waiting on uploads")).expect("defer");
        reschedule(&c, upload, WorkState::Deferred, 0, Some("media absent")).expect("defer");

        assert_eq!(
            undefer_kind(&c, "p", WorkKind::PUSH, 42).expect("release"),
            1
        );
        let ready = claim_ready(&c, "p", 42, 10).expect("claim");
        assert_eq!(
            ready.iter().map(|item| item.id).collect::<Vec<_>>(),
            vec![push],
            "only the push woke up"
        );
    }

    /// The window between a wait ending and the next claim, which is the one
    /// moment nobody thought about.
    ///
    /// A held push spends that window `pending` with an attempt already charged
    /// against it and the reason for the wait still written down, and both of
    /// those are read by things a user looks at: the delivery state of every file
    /// the push carries, and the backoff the next real failure gets.
    #[test]
    fn a_released_wait_does_not_read_as_a_failure() {
        let c = conn();
        let push = enqueue(&c, "p", &WorkKind::Push, 1, 0).expect("push");
        record_activity(
            &c,
            "p",
            1,
            &[ActivityEntry {
                kind: ActivityKind::Added,
                path: "clip.mp4".to_owned(),
                size_bytes: Some(200_000),
                unit_id: Some(push),
            }],
        )
        .expect("record");
        // The claim charges the attempt, and the deferral writes the reason —
        // exactly the two marks `Engine::reschedule_after` leaves on a push held
        // for its own uploads.
        claim_ready(&c, "p", 1, 10).expect("claim");
        reschedule(
            &c,
            push,
            WorkState::Deferred,
            1,
            Some("publishing is on hold until this folder's large files reach the remote"),
        )
        .expect("defer");
        assert_eq!(
            list_activity(&c, "p", 10).expect("list")[0].delivery,
            DeliveryState::InProgress,
            "a wait is not a failure while it is still waiting"
        );

        assert_eq!(
            undefer_kind(&c, "p", WorkKind::PUSH, 42).expect("release"),
            1
        );
        let row = list_activity(&c, "p", 10).expect("list")[0].clone();
        assert_eq!(
            row.delivery,
            DeliveryState::InProgress,
            "...and it is not a failure the instant it ends either: `pending` with \
             an attempt on the clock reads as Failed, so the release has to hand \
             the attempt back"
        );
        assert_eq!(
            row.failure, None,
            "the reason described a wait that is over; keeping it tells the user \
             their file failed for a condition that no longer holds"
        );

        // And the refund is a refund, not a reset: the next claim starts from
        // attempt one, so a folder continuously fed large files does not walk its
        // push up to the backoff ceiling one held publication at a time.
        let ready = claim_ready(&c, "p", 42, 10).expect("claim");
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].attempts, 1);
        assert_eq!(ready[0].last_error, None);
    }

    #[test]
    fn a_parked_unit_is_outstanding_but_it_is_not_moving() {
        // The two counts differ by exactly the parked rows, and that difference
        // is load-bearing in opposite directions: an abandoned upload forbids
        // publishing (`outstanding_count`) *and* means nothing is in progress
        // (`live_count`). A folder held behind one is stopped, not syncing.
        let c = conn();
        let upload = enqueue(
            &c,
            "p",
            &WorkKind::LfsUpload {
                oid: "aa".into(),
                size: 1,
            },
            1,
            0,
        )
        .expect("upload");
        assert_eq!(live_count(&c, "p", WorkKind::LFS_UPLOAD).expect("count"), 1);

        claim_ready(&c, "p", 1, 10).expect("claim");
        reschedule(&c, upload, WorkState::Parked, 1, Some("403")).expect("park");
        assert_eq!(
            outstanding_count(&c, "p", WorkKind::LFS_UPLOAD).expect("count"),
            1,
            "the remote still does not have the object"
        );
        assert_eq!(
            live_count(&c, "p", WorkKind::LFS_UPLOAD).expect("count"),
            0,
            "and nothing is going to put it there without a human"
        );
    }

    #[test]
    fn outstanding_work_counts_the_parked_units_too() {
        // This answers "is it safe to publish a pointer", and an upload that has
        // STOPPED being retried is the strongest possible no.
        let c = conn();
        let upload = enqueue(
            &c,
            "p",
            &WorkKind::LfsUpload {
                oid: "aa".into(),
                size: 1,
            },
            1,
            0,
        )
        .expect("upload");
        enqueue(&c, "p", &WorkKind::Push, 1, 0).expect("push");

        assert_eq!(
            outstanding_count(&c, "p", WorkKind::LFS_UPLOAD).expect("count"),
            1,
            "the push is not an upload"
        );
        claim_ready(&c, "p", 1, 10).expect("claim");
        reschedule(&c, upload, WorkState::Parked, 0, Some("401")).expect("park");
        assert_eq!(
            outstanding_count(&c, "p", WorkKind::LFS_UPLOAD).expect("count"),
            1,
            "a parked upload is still an object the remote does not have"
        );
        complete(&c, upload).expect("complete");
        assert_eq!(
            outstanding_count(&c, "p", WorkKind::LFS_UPLOAD).expect("count"),
            0
        );
    }

    #[test]
    fn parked_work_is_listable_and_can_be_put_back_by_its_own_profile_only() {
        let c = conn();
        let id = enqueue(&c, "p", &WorkKind::Push, 1, 0).expect("enqueue");
        claim_ready(&c, "p", 1, 10).expect("claim");
        reschedule(&c, id, WorkState::Parked, 999_999, Some("bad credentials")).expect("park");

        let parked = list_parked(&c, "p").expect("list");
        assert_eq!(parked.len(), 1);
        assert_eq!(parked[0].id, id);
        assert_eq!(parked[0].kind, "push");
        assert_eq!(parked[0].attempts, 1);
        assert_eq!(parked[0].last_error.as_deref(), Some("bad credentials"));

        // One profile must never be able to retry another's work.
        assert!(!unpark(&c, "q", id).expect("foreign unpark"));
        assert_eq!(list_parked(&c, "p").expect("list").len(), 1);

        assert!(unpark(&c, "p", id).expect("unpark"));
        assert!(list_parked(&c, "p").expect("list").is_empty());
        let claimed = claim_ready(&c, "p", 0, 10).expect("claim");
        assert_eq!(claimed.len(), 1, "not_before_ms is cleared, so it is ready");
        assert_eq!(
            claimed[0].attempts, 2,
            "the attempt history survives a retry rather than being reset"
        );
    }

    #[test]
    fn unparking_a_unit_that_is_not_parked_changes_nothing() {
        let c = conn();
        let id = enqueue(&c, "p", &WorkKind::Push, 1, 5_000).expect("enqueue");
        // A pending unit yanked to `not_before_ms = 0` would defeat its backoff,
        // and a running one would be pulled out from under the supervisor.
        assert!(!unpark(&c, "p", id).expect("unpark"));
        assert!(claim_ready(&c, "p", 100, 10).expect("claim").is_empty());
    }

    fn entry(mtime_ms: i64, close_write: bool) -> PersistedEntry {
        PersistedEntry {
            sample: FileSample {
                size: 42,
                mtime_ns: i128::from(mtime_ms) * 1_000_000,
                ctime_ns: i128::from(mtime_ms) * 1_000_000,
                inode: 7,
            },
            unchanged_since_ms: mtime_ms,
            pending_since_ms: mtime_ms,
            close_write,
        }
    }

    #[test]
    fn quiescence_state_round_trips_and_replaces_rather_than_accumulating() {
        let c = conn();
        let held = vec![
            (PathBuf::from("/w/a.bin"), entry(1_000, false)),
            (PathBuf::from("/w/b.bin"), entry(2_000, true)),
        ];
        save_file_state(&c, "p", &held).expect("save");
        let mut back = load_file_state(&c, "p").expect("load");
        back.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(back, held, "every field survives the schema round trip");

        // A path that settled must not linger, or the table grows forever.
        save_file_state(&c, "p", &held[..1]).expect("save");
        assert_eq!(load_file_state(&c, "p").expect("load").len(), 1);
        // ...and one profile's replace never touches another's rows.
        save_file_state(&c, "q", &held).expect("save");
        save_file_state(&c, "p", &[]).expect("save");
        assert_eq!(load_file_state(&c, "q").expect("load").len(), 2);
    }

    /// The `DELETE` and the inserts are one edit of one fact. If a row fails
    /// partway through, the delete must go back too — otherwise the profile is
    /// left holding nothing, and every quiescence window in flight silently
    /// restarts from zero.
    #[test]
    fn a_failed_row_rolls_the_whole_replace_back() {
        let c = conn();
        let held = vec![
            (PathBuf::from("/w/a.bin"), entry(1_000, false)),
            (PathBuf::from("/w/b.bin"), entry(2_000, false)),
        ];
        save_file_state(&c, "p", &held).expect("save");

        // Two entries for one path violate `PRIMARY KEY (profile_id, path)`, so
        // the second insert fails after the delete has already run.
        let doomed = vec![
            (PathBuf::from("/w/c.bin"), entry(3_000, false)),
            (PathBuf::from("/w/c.bin"), entry(4_000, false)),
        ];
        assert!(
            save_file_state(&c, "p", &doomed).is_err(),
            "a constraint violation must surface, not be swallowed"
        );

        let mut back = load_file_state(&c, "p").expect("load");
        back.sort_by(|a, b| a.0.cmp(&b.0));
        assert_eq!(
            back, held,
            "the previous state is intact: no delete without its inserts"
        );
    }

    /// A row written before releasing the redundant copy became the default
    /// holds a literal `"lfsPruneLocal": false` — serde writes every field — so
    /// the new default cannot reach it on its own, and the folders with a
    /// second copy worth reclaiming are exactly the old ones.
    ///
    /// A store that predates the change is simulated the only way an in-memory
    /// one can be: by dropping the marker, which is precisely what such a store
    /// does not have.
    #[test]
    fn a_store_written_before_the_flip_is_carried_onto_the_new_default() {
        let c = conn();
        c.execute("DELETE FROM meta WHERE key = ?1", [PRUNE_DEFAULT_MARKER])
            .expect("unmark");
        let legacy = r#"{
            "id": "01OLD", "name": "tgdrive", "localPath": "/w/tgdrive",
            "remoteUrl": "https://git.example/u/tgdrive.git", "branch": "main",
            "direction": "bidirectional", "lane": "main",
            "subpaths": [], "excludes": [], "removable": false, "volumeId": null,
            "lfsMode": "materialize", "lfsThresholdBytes": 4194304,
            "lfsPruneLocal": false, "lfsNever": ["*.md"],
            "settleMs": 5000, "pollIntervalMs": 15000, "tags": [],
            "commitSubjectTemplate": "", "authorOverride": null, "enabled": true
        }"#;
        c.execute(
            "INSERT INTO profiles (id, json, updated_ms) VALUES (?1, ?2, ?3)",
            ("01OLD", legacy, 7_i64),
        )
        .expect("insert a pre-flip row");

        migrate(&c).expect("the next open");

        let carried = stored_profile(&c, "01OLD")
            .expect("read")
            .expect("the row is still there");
        assert!(
            carried.lfs_prune_local,
            "an install that already exists is what the migration is for"
        );
        assert_eq!(
            carried.lfs_never,
            vec!["*.md".to_owned()],
            "json_set rewrites one key and leaves the rest byte for byte"
        );
        let updated: i64 = c
            .query_row(
                "SELECT updated_ms FROM profiles WHERE id = '01OLD'",
                [],
                |r| r.get(0),
            )
            .expect("timestamp");
        assert_eq!(
            updated, 7,
            "the operator edited nothing, and a folder reporting an edit \
             nobody made is the worse lie"
        );
    }

    /// After the one-shot, `false` means an opt-out — a folder that has to be
    /// restorable with no network. Re-running the rewrite on every open would
    /// undo that choice, which is the whole reason the marker exists.
    #[test]
    fn a_deliberate_opt_out_survives_every_later_open() {
        let c = conn();
        let mut p = profile("01KEEP");
        p.lfs_prune_local = false;
        upsert_profile(&c, &p, 1).expect("insert");

        migrate(&c).expect("a later open");

        assert!(
            !stored_profile(&c, "01KEEP")
                .expect("read")
                .expect("row")
                .lfs_prune_local,
            "the one-shot already ran; it must not run again"
        );
    }

    /// Two paths whose content is identical share one object, and one object is
    /// one download. The name is carried in a column rather than in the payload
    /// precisely so that stays true — folding it into the payload would make
    /// `enqueue_unique` see two different units and fetch the same bytes twice.
    #[test]
    fn two_paths_sharing_one_object_stay_one_download_under_the_first_name() {
        let c = conn();
        let unit = WorkKind::LfsDownload {
            oid: "a".repeat(64),
            size: 4_096,
        };
        let first = enqueue_unique(&c, "p", &unit, 1, 1).expect("first");
        label_unit(&c, first, "70-comms/keeper-rec/camera-0001.mov").expect("label");
        let second = enqueue_unique(&c, "p", &unit, 2, 2).expect("second");
        label_unit(&c, second, "40-media/a-copy-of-the-same-file.mov").expect("relabel");

        assert_eq!(first, second, "one object, one unit");
        let claimed = claim_ready(&c, "p", 10, 10).expect("claim");
        assert_eq!(claimed.len(), 1);
        assert_eq!(
            claimed[0].label.as_deref(),
            Some("70-comms/keeper-rec/camera-0001.mov"),
            "the first name wins; a last-one-wins race would make the line flicker"
        );
    }

    /// The pair the status line reads. A push is real work and is not a file:
    /// counting it would put a file count beside a byte total it contributes
    /// nothing to, and the two numbers would disagree in the same sentence.
    #[test]
    fn queued_transfers_counts_sized_work_only_and_ignores_parked() {
        let c = conn();
        enqueue(&c, "p", &WorkKind::Push, 1, 1).expect("push");
        enqueue(
            &c,
            "p",
            &WorkKind::LfsDownload {
                oid: "b".repeat(64),
                size: 1_000,
            },
            1,
            1,
        )
        .expect("download");
        let parked = enqueue(
            &c,
            "p",
            &WorkKind::LfsDownload {
                oid: "c".repeat(64),
                size: 9_000_000,
            },
            1,
            1,
        )
        .expect("second download");
        c.execute(
            "UPDATE journal SET state = 'parked' WHERE id = ?1",
            [parked],
        )
        .expect("park it");

        assert_eq!(
            queued_transfers(&c, "p").expect("counted"),
            (1, 1_000),
            "the push has no size and the parked unit is not waiting"
        );
        assert_eq!(
            queued_transfers(&c, "other").expect("counted"),
            (0, 0),
            "and it never reaches across profiles"
        );
    }

    /// The bug the queue tail made visible: 106 units for 95 objects on a
    /// folder pulling 53 GB, every duplicate a `running` object re-queued by
    /// the next scan. A transfer is content-addressed — the same oid names the
    /// same immutable bytes — so a second unit can only fetch what the first
    /// one is already fetching.
    #[test]
    fn a_running_transfer_covers_an_identical_enqueue() {
        let c = conn();
        let unit = WorkKind::LfsDownload {
            oid: "f".repeat(64),
            size: 4_096,
        };
        let first = enqueue_unique(&c, "p", &unit, 1, 1).expect("first");
        let claimed = claim_ready(&c, "p", 10, 10).expect("claim");
        assert_eq!(claimed.len(), 1, "it is running now");

        let second = enqueue_unique(&c, "p", &unit, 2, 2).expect("second");
        assert_eq!(second, first, "the running unit is the one that covers it");
        let total: i64 = c
            .query_row("SELECT COUNT(*) FROM journal", [], |r| r.get(0))
            .expect("count");
        assert_eq!(total, 1, "a queue that never shrinks is how this looked");
    }

    /// A push is NOT covered by one in flight: it publishes what the worktree
    /// holds when it runs, so a change made after it started needs its own
    /// unit. Dropping it would strand that change until something else queued
    /// a push — which is why the running state was ignored in the first place.
    #[test]
    fn a_running_push_does_not_cover_a_later_one() {
        let c = conn();
        let first = enqueue_unique(&c, "p", &WorkKind::Push, 1, 1).expect("first");
        claim_ready(&c, "p", 10, 10).expect("claim");

        let second = enqueue_unique(&c, "p", &WorkKind::Push, 2, 2).expect("second");
        assert_ne!(
            second, first,
            "the running push cannot publish a change made after it started"
        );
    }

    /// The queues that already exist. While one half is `running` the pair is
    /// invisible; the moment recovery returns it to `pending` there are two
    /// identical rows, and identical payload is identical work.
    #[test]
    fn recovery_collapses_a_duplicate_the_return_reveals() {
        let c = conn();
        let unit = WorkKind::LfsDownload {
            oid: "a".repeat(64),
            size: 8,
        };
        let kept = enqueue(&c, "p", &unit, 1, 1).expect("first");
        // Exactly the shape the old enqueue rule produced: a second row for an
        // object already in flight.
        c.execute("UPDATE journal SET state = 'running' WHERE id = ?1", [kept])
            .expect("run it");
        let copy = enqueue(&c, "p", &unit, 2, 2).expect("duplicate");

        recover_running(&c, 5).expect("recover");

        let rows: Vec<i64> = c
            .prepare("SELECT id FROM journal ORDER BY id")
            .expect("prepare")
            .query_map([], |r| r.get(0))
            .expect("query")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("rows");
        assert_eq!(rows, vec![kept], "the copy goes, the queue's place stays");
        assert!(!rows.contains(&copy));
    }
}
