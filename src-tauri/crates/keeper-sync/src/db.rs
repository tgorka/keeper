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
use crate::tasks::{TaskKind, TaskMode, TaskOutcome, TaskSchedule, TaskState};

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
    // No `busy_timeout` is set here, and that is not an omission: `rusqlite`'s
    // own `Connection::open` already arms a five-second one, which the test
    // `the_shared_database_waits_for_a_writer_rather_than_failing_at_once` pins
    // so nobody has to take it on trust. It matters because this file is shared
    // by two processes — the app's in-process engine and `keeper-syncd` — and a
    // writer that failed instantly would, for a task lease, leave the run
    // recorded nowhere and the lease held until it expired. WAL keeps readers
    // out of the contention entirely, so the timeout only ever paces two
    // writers, and a deadlock-prone upgrade still returns at once: SQLite
    // answers `SQLITE_BUSY_SNAPSHOT` without consulting the handler, so a write
    // here can be delayed but never hangs.
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
        -- (Story 56.2) and `local_origin` (Story 56.5) are missing here on
        -- purpose, exactly as `activity`'s late columns are:
        -- `ensure_materialized_columns` adds them, so a fresh install and one
        -- that predates them reach the same schema down one path.
        CREATE TABLE IF NOT EXISTS materialized (
            profile_id  TEXT NOT NULL,
            path        TEXT NOT NULL,
            at_ms       INTEGER NOT NULL,
            PRIMARY KEY (profile_id, path)
        );

        -- What keeper remembers about work it is supposed to do (Story 57.1,
        -- AD-135). Neither existing table can be this record: `db::complete`
        -- is `DELETE FROM journal WHERE id = ?1`, so a finished unit leaves no
        -- trace at all, and `WorkKind` is a closed vocabulary of transfer
        -- primitives with no room for "sync this folder nightly"; `activity`
        -- is by its own doc above "a human-facing log, not a source of truth"
        -- and is capped per profile, so a schedule kept there would be
        -- forgotten by the thousandth file.
        --
        -- `profile_id IS NULL` means host-wide: a task that belongs to the
        -- machine rather than to one folder.
        --
        -- `running_host` + `lease_until_ms` are the lease, and they are on the
        -- task rather than in a lock file because the affected-row count of one
        -- conditional `UPDATE` is the only arbiter two hosts sharing this file
        -- can both trust.
        --
        -- Any column added to either table later MUST be nullable or carry a
        -- DEFAULT, and MUST go through an additive `ensure_task_columns` rather
        -- than into this batch. The read side is carefully tolerant — an unknown
        -- kind, mode, schedule or outcome is skipped and listed — but every
        -- INSERT here names its columns, so a NOT NULL column with no default
        -- would make an older binary's writes fail against a newer schema. That
        -- is NFR-43's other half and it is only a rule if it is written down.
        CREATE TABLE IF NOT EXISTS tasks (
            id             TEXT PRIMARY KEY,
            profile_id     TEXT,
            kind           TEXT NOT NULL,
            schedule       TEXT,
            mode           TEXT NOT NULL,
            next_due_ms    INTEGER,
            enabled        INTEGER NOT NULL DEFAULT 1,
            updated_ms     INTEGER NOT NULL,
            running_host   TEXT,
            lease_until_ms INTEGER
        );

        -- One row per attempt, appended and bounded exactly the way `activity`
        -- is — but unlike `activity` this one IS the source of truth for "when
        -- did this last run and what happened", which is the whole question a
        -- task exists to answer.
        --
        -- Deliberately no foreign key, in either direction. A host-wide task
        -- has a NULL profile, so there is no parent to point at; and a deleted
        -- profile must not silently take a task's history with it as a side
        -- effect nobody wrote down. `delete_task` removes both halves itself.
        CREATE TABLE IF NOT EXISTS task_runs (
            id          INTEGER PRIMARY KEY AUTOINCREMENT,
            task_id     TEXT NOT NULL,
            started_ms  INTEGER NOT NULL,
            finished_ms INTEGER,
            outcome     TEXT,
            detail      TEXT,
            host        TEXT NOT NULL
        );
        -- Newest-first per task is the only way this table is ever read, and
        -- it is also how the cap is trimmed. By `id` rather than `started_ms`
        -- for `activity_recent`'s reason: two runs can share a millisecond.
        CREATE INDEX IF NOT EXISTS task_runs_recent
            ON task_runs (task_id, id DESC);

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
/// there yet (Stories 56.2, 56.5, 56.14 and 56.17).
///
/// Eight facts the ledger has to hold before anything can decide what to
/// release: when the content was last *read* (`last_used_ms`), when the remote
/// last confirmed it holds the object (`synced_at_ms`), whether the owner has
/// asked for this path to stay on the machine (`pinned`), the object's
/// identity and length (`oid`, `size_bytes`) so a row still answers after the
/// worktree stops holding a pointer to consult, which way the content
/// travelled (`local_origin`, Story 56.5) so the release sweep knows which of
/// the two clocks applies to this path, when the content was released
/// (`released_at_ms`, Story 56.14) so those facts survive the release, and the
/// instant the owner asked for this one path to be let go again
/// (`release_at_ms`, Story 56.17).
///
/// Literally [`ensure_activity_columns`]'s shape, including the `drop(stmt)`
/// before the first `conn.execute` — `rusqlite` holds the connection while a
/// prepared statement lives, so an `ALTER TABLE` issued with the PRAGMA
/// statement still alive is a borrow error, not a runtime surprise. The one
/// difference is that these eight columns are not all the same type, so the
/// loop carries the type beside the name rather than hard-coding `INTEGER`.
///
/// **Nullable and without a `DEFAULT`, and no `meta` marker.** `NULL` is the
/// honest reading of every one of them on a pre-existing row: nobody measured
/// a last use, no remote confirmation was recorded, nothing was pinned. An
/// `ALTER TABLE ... ADD COLUMN` guarded by the column list is its own
/// idempotence — the rule stated at the top of [`migrate`] — so a second
/// `migrate` on the same connection adds nothing and errors on nothing.
///
/// `pinned` and `local_origin` are `INTEGER`s read as booleans, which is how
/// SQLite spells one; [`materialized_rows`] narrows both, so no caller sees
/// the encoding.
///
/// **`NULL` in `local_origin` means *arrived from the remote*, and that is the
/// truth of the existing data rather than a default** (AD-131). The only two
/// writers this table had before Story 56.5 — `materialize_landed` and
/// `materialize_pending` — both publish content *over a pointer*, so every row
/// that can already exist was written by an arrival. A row this clone authored
/// carries a `1`, written by [`note_local_authorship`].
///
/// **`NULL` in `released_at_ms` means *the content is here*, which is what
/// every row written before Story 56.14 meant by existing at all.** A release
/// used to `DELETE` the row, so the column's absence and its `NULL` say the
/// same thing about historical data, and [`forget_materialized`] is the only
/// writer that ever fills it.
///
/// **`NULL` in `release_at_ms` means *this path is on the folder's own
/// window*** (Story 56.17), which is what every row written before that story
/// meant by existing at all. A non-`NULL` value is an absolute epoch-ms
/// instant somebody named — `keeper-syncd materialize --for 2h`, or the Files
/// row's own choice — and [`crate::engine::release_due_at`] reads it *instead
/// of* the folder's `releaseTtlMs` arithmetic, in both directions. Two
/// timestamps that look alike and are not: `released_at_ms` is when content
/// left, `release_at_ms` is when it may. [`set_release_at`] is the only writer
/// that fills it and [`forget_materialized`] is the only one that clears it,
/// because the instruction is spent the moment the content goes.
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
        ("local_origin", "INTEGER"),
        ("released_at_ms", "INTEGER"),
        ("release_at_ms", "INTEGER"),
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

/// Add the journal's late columns — `label` and `urgency` — if they are not
/// there yet.
///
/// A unit's payload says what work to do; the label says what to *call* it
/// while it is being done, and the urgency says whether anybody is waiting for
/// it. All three are separate columns because they have different identities:
/// [`enqueue_unique`] deduplicates on the payload string, so folding either of
/// the other two into it would make two paths that share one object — the
/// ordinary case for duplicated content — into two downloads of the same
/// bytes.
///
/// Both nullable and both without a `DEFAULT`, which is the honest reading of
/// a row written before they existed: no better name than the work itself, and
/// nobody waiting. `ALTER TABLE ... ADD COLUMN` guarded by the column list is
/// its own idempotence — the rule stated at the top of [`migrate`] — so this
/// needs no `meta` marker and a second `migrate` adds nothing.
///
/// One column at a time rather than [`ensure_materialized_columns`]' typed-pair
/// loop: two additions do not earn a table, and `PRAGMA table_info`'s statement
/// must be dropped before any `conn.execute` runs on the same connection.
fn ensure_journal_columns(conn: &Connection) -> Result<()> {
    let mut stmt = conn.prepare("PRAGMA table_info(journal)")?;
    let existing: Vec<String> = stmt
        .query_map([], |r| r.get::<_, String>(1))?
        .collect::<rusqlite::Result<Vec<_>>>()?;
    drop(stmt);
    if !existing.iter().any(|c| c == "label") {
        conn.execute("ALTER TABLE journal ADD COLUMN label TEXT", [])?;
    }
    if !existing.iter().any(|c| c == "urgency") {
        conn.execute("ALTER TABLE journal ADD COLUMN urgency INTEGER", [])?;
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

/// Mark a queued unit as something somebody is waiting for, and make it
/// claimable now (Story 56.3).
///
/// Two writes rather than one, because "somebody is waiting" is worth nothing
/// on a row [`claim_ready`] cannot see. Both are narrow, both are by id, and
/// both are idempotent.
///
/// # Why urgency is not a payload field
///
/// This is a **separate narrow UPDATE** on whatever id [`enqueue_unique`]
/// returned, and that is not a style choice. `enqueue_unique` deduplicates on
/// the serialized payload, so an urgency field inside
/// [`WorkKind::LfsDownload`] would make the requested unit a *different* unit
/// from the background one and fetch the same immutable bytes twice — the
/// defect [`WorkKind::covered_while_running`] records with its own measurement
/// (106 queued units for 95 distinct objects). The urgency belongs to the row
/// that will deliver the work, and that row is the covering one.
///
/// [`recover_running`] is the second reason it has to live there rather than on
/// a duplicate: its `MIN(id)` collapse deletes every pending row but the oldest
/// for a payload, so an urgency written onto a row that was going to be
/// collapsed would simply vanish at the next restart.
///
/// # `MAX`, where `label_unit` is first-writer-wins
///
/// The tie-break is inverted on purpose. A label must not flicker — with
/// several paths sharing one object any of their names is truthful, so the
/// first one wins. A request must be able to raise a row a background scan
/// queued minutes ago, and must never be able to lower one another request
/// raised, so it takes the maximum. `COALESCE` because the column is nullable
/// for every row written before it existed.
///
/// # Why it also lifts a deferral and a backoff
///
/// `enqueue_unique` counts `deferred` as cover, and [`claim_ready`] only ever
/// offers `state = 'pending'`. A request that deduplicated onto a deferred row
/// — an `LfsDownload` whose removable remote was absent, say — would otherwise
/// report a unit id for work nothing will claim until an unrelated resume or
/// volume re-attach happens to run [`undefer_profile`]. The same holds for a
/// pending row whose `not_before_ms` is a retry backoff in the future. So a
/// promotion returns the row to `pending` and brings its earliest attempt
/// forward to now: the person asking is a reason to try again immediately, and
/// `attempts` is deliberately left alone so a second failure backs off exactly
/// as far as it would have.
pub fn promote_unit(conn: &Connection, id: i64, now_ms: i64) -> Result<()> {
    conn.execute(
        "UPDATE journal SET urgency = MAX(COALESCE(urgency, 0), ?2) WHERE id = ?1",
        (id, Urgency::Requested.level()),
    )?;
    conn.execute(
        "UPDATE journal
            SET state = 'pending', not_before_ms = MIN(not_before_ms, ?2)
          WHERE id = ?1 AND state IN ('pending', 'deferred')",
        (id, now_ms),
    )?;
    Ok(())
}

/// Whether anybody is waiting for one unit, read at the moment the answer
/// matters (Story 56.3).
///
/// The journal is the memory, and this is what makes that claim true rather
/// than decorative: the level is read when the completion arm decides what to
/// publish, not copied out of the row when the unit was claimed. A request that
/// arrives while its covering download is already `running` — which
/// [`WorkKind::covered_while_running`] makes the ordinary case — raises the
/// urgency of a row the supervisor read minutes ago, and a snapshot taken at
/// claim time would drop that request on the floor.
///
/// A row that is gone reads [`Urgency::Background`]: the only way to be asked
/// about a deleted row is a completion arm running after [`complete`], and the
/// conservative answer there is the arrival policy the profile configured.
pub fn unit_urgency(conn: &Connection, id: i64) -> Result<Urgency> {
    let level: Option<Option<i64>> = conn
        .query_row("SELECT urgency FROM journal WHERE id = ?1", [id], |r| {
            r.get::<_, Option<i64>>(0)
        })
        .optional()?;
    Ok(Urgency::from_level(level.flatten().unwrap_or(0)))
}

/// Where one journal unit stands, and why (Story 56.13).
///
/// The question a **one-shot** caller has to answer after draining: the drain
/// swallows a transfer's own error into the row (`Engine::reschedule_after`
/// writes `last_error` and returns `Ok`), which is right for a supervisor and
/// useless to a process that is about to tell its caller whether the bytes
/// arrived. Four answers, and each of them is a different sentence:
///
/// * [`UnitStanding::Settled`] — the row is gone, which is the only thing
///   [`complete`] does, so the work succeeded.
/// * [`UnitStanding::Waiting`] — `pending` or `deferred`: something will try
///   again. `reason` is the last recorded failure, which may predate this run.
/// * [`UnitStanding::InFlight`] — `running`: somebody has claimed it and is
///   attempting it *now*. For a caller that has just finished its own drain
///   that somebody is necessarily **another process**, and it is the difference
///   between "this failed" and "this is happening without you".
/// * [`UnitStanding::Parked`] — given up on. `claim_ready` never offers a
///   parked row, so nothing retries it until a human does.
///
/// An unreadable `state` is reported as `Waiting`, the answer that claims least:
/// it neither promises the work is done nor accuses it of having stopped.
pub fn unit_standing(conn: &Connection, id: i64) -> Result<UnitStanding> {
    let row: Option<(String, Option<String>)> = conn
        .query_row(
            "SELECT state, last_error FROM journal WHERE id = ?1",
            [id],
            |r| Ok((r.get::<_, String>(0)?, r.get::<_, Option<String>>(1)?)),
        )
        .optional()?;
    let Some((state, reason)) = row else {
        return Ok(UnitStanding::Settled);
    };
    Ok(match state.as_str() {
        "running" => UnitStanding::InFlight,
        "parked" => UnitStanding::Parked { reason },
        _ => UnitStanding::Waiting { reason },
    })
}

/// Where one journal unit stands. See [`unit_standing`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnitStanding {
    /// The row is gone: the work succeeded.
    Settled,
    /// Something will attempt it again, and this is the last thing that went
    /// wrong — possibly on an earlier attempt than the caller's own.
    Waiting { reason: Option<String> },
    /// Claimed and being attempted right now, by somebody else.
    InFlight,
    /// Given up on, and nothing retries it until a human unparks it.
    Parked { reason: Option<String> },
}

impl UnitStanding {
    /// Whether waiting longer could still change the answer.
    ///
    /// The drain loop's primary exit: `Settled` is the answer, and `Parked` and
    /// `InFlight` are answers this process cannot improve on — one because
    /// nothing will retry it, the other because another process owns the
    /// attempt and `claim_ready` will not offer a `running` row twice.
    pub fn worth_waiting_for(&self) -> bool {
        matches!(self, Self::Waiting { .. })
    }
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
///
/// # Why `released_at_ms` is the one other column it clears (Story 56.14)
///
/// It is not a second fact. Since Story 56.14 [`forget_materialized`] retains
/// the row and stamps `released_at_ms` instead of deleting it, so that column
/// is the *negation* of this function's only fact: a row with it set says
/// "content for this path is not here". Landing content and the content being
/// here are the same statement, so leaving the stamp would make the row assert
/// both at once — and [`materialized_paths`], which filters on exactly that
/// column, would go on telling `Engine::pending` that the machine holds
/// nothing at a path it has just materialized.
pub fn remember_materialized(
    conn: &Connection,
    profile_id: &str,
    path: &str,
    now_ms: i64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO materialized (profile_id, path, at_ms) VALUES (?1, ?2, ?3)
         ON CONFLICT(profile_id, path)
         DO UPDATE SET at_ms = excluded.at_ms, released_at_ms = NULL",
        (profile_id, path, now_ms),
    )?;
    Ok(())
}

/// Record that this machine was *found* holding content for one path, without
/// claiming to know when it landed (Story 56.14).
///
/// The writer for the one arm that had none: `Engine::materialize_held`'s
/// already-held exit. A human named a path, keeper looked, and the content was
/// already there — put there by `git lfs pull`, by a keeper that predates this
/// ledger, by a second profile sharing the store, or by the documented
/// same-length tie. Before this function that arm wrote nothing at all, so a
/// path somebody explicitly asked for was invisible to the release clocks
/// (FR-334): the sweep reads [`materialized_rows`], and a path with no row is
/// a path with no candidate and no recorded use.
///
/// # Why this is not [`remember_materialized`]
///
/// `at_ms` means *content for this path landed here*, and this caller does not
/// know when that was. So the insert sets it to now — the earliest instant
/// this clone can honestly prove the content was here — and **the conflict arm
/// leaves it alone**, because a row that already records a landing knows
/// better than this one does. Using `remember_materialized` here would move an
/// existing landing clock forward on every read of the file, which is the one
/// fact the sweep's fallback (`a_row_with_no_recorded_use_falls_back_to_when
/// _the_content_landed`) depends on.
///
/// What it does record on both arms is the *use*: this call happens because
/// somebody asked for the path, which is precisely what `last_used_ms` means.
/// It clears `released_at_ms` for [`remember_materialized`]'s reason — the
/// content is here, and that column says it is not.
///
/// # It writes the identity, because the caller is holding the pointer
///
/// `oid` and `size_bytes` on both arms, from the committed pointer the
/// already-held arm looked up to get here — the same fact [`note_arrival`] and
/// [`note_local_authorship`] record for the same reason, and this is the third
/// moment the pointer is in hand.
///
/// Not optional bookkeeping. `Engine::release_expired` hands
/// `row.size_bytes.unwrap_or(u64::MAX)` to `release_path_gate`, whose floor is
/// `size < over_bytes`, so a row with a `NULL` size clears **any**
/// `virtualOverBytes` floor — a folder that keeps small files materialized on
/// purpose would release the 2 MB file a person had just explicitly asked for.
/// A `NULL` size also contributes nothing to `RELEASE_BUDGET_BYTES`, so one
/// pass could release `RELEASE_BUDGET_OBJECTS` files of any size while holding
/// the profile's reservation, which is exactly the residual
/// [`note_arrival`]'s doc says these columns exist to bound. And
/// `Engine::release_schedules` resolves the same row, so a Files-pane countdown
/// would be computed against a size nobody measured.
///
/// It does not touch `local_origin`, so an inserted row reads `NULL` — *arrived
/// from the remote*. That is the honest default and the safe one: it selects
/// the `last_used_ms` clock, which this call has just set to now, and nothing
/// is ever deleted on a clock alone — `Engine::release_resolved` takes a fresh
/// remote proof at the moment of deletion. A path this clone actually authored
/// is corrected the next time its commit runs, by
/// [`note_local_authorship`]'s upsert.
pub fn observe_materialized(
    conn: &Connection,
    profile_id: &str,
    path: &str,
    now_ms: i64,
    oid: &str,
    size_bytes: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO materialized
             (profile_id, path, at_ms, last_used_ms, oid, size_bytes)
         VALUES (?1, ?2, ?3, ?3, ?4, ?5)
         ON CONFLICT(profile_id, path)
         DO UPDATE SET last_used_ms = excluded.last_used_ms,
                       oid = excluded.oid, size_bytes = excluded.size_bytes,
                       released_at_ms = NULL",
        (
            profile_id,
            path,
            now_ms,
            oid,
            i64::try_from(size_bytes).unwrap_or(i64::MAX),
        ),
    )?;
    Ok(())
}

/// Record that the content at this path was read through keeper just now
/// (Story 56.5, `last_used_ms`).
///
/// One fact, one column, in [`label_unit`]'s shape. It deliberately does not
/// touch `local_origin`: a *use* says nothing about who wrote the bytes, and
/// the whole point of AD-131's two clocks is that a use must never make an
/// unconfirmed locally-authored path eligible for release.
///
/// **UPDATE-only.** A use of a path this clone holds no materialization row
/// for is a fact this table cannot hold: `at_ms` is `NOT NULL` and means
/// *content landed here*, which this function has no way to know. An absent
/// row therefore stays absent and the call succeeds — the caller is a
/// best-effort memo on a read path, not a transaction.
pub fn note_use(conn: &Connection, profile_id: &str, path: &str, now_ms: i64) -> Result<()> {
    conn.execute(
        "UPDATE materialized SET last_used_ms = ?3 WHERE profile_id = ?1 AND path = ?2",
        (profile_id, path, now_ms),
    )?;
    Ok(())
}

/// Record that the content now at this path arrived from the remote, and which
/// object it is (Story 56.5, `last_used_ms` + `local_origin = 0` + `oid` +
/// `size_bytes`).
///
/// Called immediately after [`remember_materialized`] on every publish-over-a-
/// pointer path. Four columns because they are one fact: *this path's content
/// arrived from the remote just now, and it is this object*. Arriving is the
/// first thing that ever happened to it here, so the remote-origin clock starts
/// now rather than at some `NULL` a later reader would have to interpret; and
/// an arrival is one of only two moments the committed pointer is in hand, so
/// it is one of only two honest places to record which object is at a path.
///
/// # Why the identity is written here and was written nowhere before
///
/// `oid` and `size_bytes` had **no writer at all** until this call. Story 56.2
/// added the columns and [`ensure_materialized_columns`]' own note says why
/// they exist — so a row still answers "after the worktree stops holding a
/// pointer to consult" — but nothing ever filled them, so the release sweep's
/// byte ceiling read `None` for every candidate and bounded nothing. One pass
/// could hash thirty-two objects of any size while holding the profile's
/// reservation, which is precisely the cost `RELEASE_BUDGET_BYTES`' own doc
/// says it exists to prevent.
///
/// The length binds as `i64::try_from(size_bytes).unwrap_or(i64::MAX)`, because
/// SQLite's integer is signed and this one is not. Saturating is the safe
/// direction: an `as` cast would wrap a `u64` above `i64::MAX` negative,
/// [`materialized_rows`] narrows a negative to `None`, and `None` is the one
/// answer that makes a candidate contribute nothing to the budget meant to
/// bound it. A saturated length reads back as enormous, so the pass stops at
/// that candidate after giving it its one attempt.
///
/// It deliberately does not touch `synced_at_ms`: the object is upstream by
/// construction, but NFR-40's proof is taken fresh at the moment of deletion,
/// and this clock's job is only to select which timer applies. It also does
/// not touch `pinned` — see [`remember_materialized`]'s doc for what a
/// statement that knew more than it needed to cost last time.
///
/// **UPDATE-only**, for [`note_use`]'s reason: `remember_materialized` writes
/// the row an instant earlier, so the row is there, and if it somehow is not
/// then this table cannot hold the fact.
pub fn note_arrival(
    conn: &Connection,
    profile_id: &str,
    path: &str,
    now_ms: i64,
    oid: &str,
    size_bytes: u64,
) -> Result<()> {
    conn.execute(
        "UPDATE materialized SET last_used_ms = ?3, local_origin = 0, oid = ?4, size_bytes = ?5
          WHERE profile_id = ?1 AND path = ?2",
        (
            profile_id,
            path,
            now_ms,
            oid,
            i64::try_from(size_bytes).unwrap_or(i64::MAX),
        ),
    )?;
    Ok(())
}

/// Record that this clone created or modified the content now at this path,
/// and which object it is (Story 56.5, `local_origin = 1` **and**
/// `synced_at_ms = NULL`, with `oid` and `size_bytes`).
///
/// One fact: *this clone wrote the content now at this path, and it is this
/// object*. New local bytes are, by definition, bytes the remote has not
/// confirmed, so clearing `synced_at_ms` is not bookkeeping tidiness — a path
/// confirmed upstream last week and re-edited this morning would otherwise be
/// released a TTL after *the old confirmation*, discarding content that exists
/// nowhere else. FR-341 is the `?` in `Engine::release_due_at`, and this is
/// what puts the `NULL` there.
///
/// The identity travels with the same fact and for the same reason it does in
/// [`note_arrival`]: a local clean is the other moment the committed pointer is
/// in hand, so it is the other honest place to record which object is at a
/// path. Before Story 56.5 `oid` and `size_bytes` had no writer anywhere —
/// [`ensure_materialized_columns`] records why they exist, "so a row still
/// answers after the worktree stops holding a pointer to consult", and until
/// these two writers nothing ever answered. `size_bytes` binds through the same
/// saturating `i64::try_from(size_bytes).unwrap_or(i64::MAX)`; see
/// [`note_arrival`] for why saturating rather than wrapping is the safe
/// direction for a number the release budget reads.
///
/// **An upsert, unlike the clock writers.** A local authorship is discovered at
/// commit time from the worktree, which may be the first thing this ledger ever
/// hears about the path — a file the owner created here has no arrival to have
/// written a row. `at_ms` is honest on insert: content for this path does exist
/// on this machine, that is precisely why there is something to upload. It
/// never touches `pinned` or `last_used_ms`.
///
/// # It clears `released_at_ms`, and that is the one arm that must (Story 56.14)
///
/// For [`remember_materialized`]'s reason — the column is the negation of "this
/// clone holds content for this path", which is exactly what a local authorship
/// asserts — and because this is the ONLY writer that can reach a released row.
/// Content the owner puts back at a released path is not pointer text, so
/// `pending_smudges` skips it and neither `remember_materialized` nor
/// [`observe_materialized`] is ever called for it; the sweep cannot reach it
/// either, because its candidates come from [`materialized_rows`]. Without the
/// clear, the row stays retired forever: bytes the owner just authored would be
/// invisible to the release sweep at any age, carry no schedule in a listing,
/// and read to [`materialized_paths`] as content this machine does not have.
pub fn note_local_authorship(
    conn: &Connection,
    profile_id: &str,
    path: &str,
    now_ms: i64,
    oid: &str,
    size_bytes: u64,
) -> Result<()> {
    conn.execute(
        "INSERT INTO materialized
             (profile_id, path, at_ms, local_origin, synced_at_ms, oid, size_bytes)
         VALUES (?1, ?2, ?3, 1, NULL, ?4, ?5)
         ON CONFLICT(profile_id, path)
           DO UPDATE SET local_origin = 1, synced_at_ms = NULL,
                         oid = excluded.oid, size_bytes = excluded.size_bytes,
                         released_at_ms = NULL",
        (
            profile_id,
            path,
            now_ms,
            oid,
            i64::try_from(size_bytes).unwrap_or(i64::MAX),
        ),
    )?;
    Ok(())
}

/// Record that the remote was observed holding the object for this path
/// (Story 56.5, `synced_at_ms`).
///
/// Written only where a per-path proof already exists — an upload unit that
/// completed, or an object `lfs::audit::serves` affirmed — and never at
/// `mark_synced`, which is a per-*profile* edge that says nothing about one
/// path (AD-131).
///
/// **A stored confirmation authorizes eligibility, never a deletion.** It is a
/// memo taken at some earlier instant; `Engine::remote_serves` re-proves the
/// object at the moment the content would go, and NFR-40 is unmoved by this
/// column existing.
///
/// **UPDATE-only**, for [`note_use`]'s reason, and it deliberately does not
/// touch `local_origin`: confirming an upload does not make the path
/// remote-authored, it makes it a *confirmed local* path, which is the state
/// the second clock exists to measure.
pub fn note_synced(conn: &Connection, profile_id: &str, path: &str, now_ms: i64) -> Result<()> {
    conn.execute(
        "UPDATE materialized SET synced_at_ms = ?3 WHERE profile_id = ?1 AND path = ?2",
        (profile_id, path, now_ms),
    )?;
    Ok(())
}

/// Record the owner's standing instruction about one path (Story 56.5,
/// `pinned`), the writer [`is_pinned`] has been reading for.
///
/// One column either way, naming exactly `pinned`: it must not disturb either
/// clock, `local_origin`, the recorded identity or `at_ms`, and unpinning must
/// leave a path's accumulated history exactly as it was so the path simply
/// becomes a candidate again.
///
/// # Why the two directions are not one statement
///
/// **Pinning upserts.** A pin is a standing instruction about a path, not an
/// observation about content, so pinning a path whose content is not here yet
/// is the ordinary case: an owner pre-pins what they are about to materialize.
/// `at_ms` on insert is the instant the instruction was given, which is the
/// only timestamp this call knows; the arrival that follows overwrites it with
/// the truth through [`remember_materialized`].
///
/// **Unpinning is UPDATE-only**, and the asymmetry is load-bearing. One upsert
/// serving both directions meant `keeper-syncd unpin media clip.mp4` on a path
/// with no ledger row *INSERTed* one, asserting "content landed here now" for
/// content that is not here. `Engine::release_due_at` reads that phantom as a
/// candidate forever — not pinned, `local_origin` `false`, both clocks `NULL`,
/// so the `at_ms` fallback applies and it comes due a TTL later — and it can
/// only ever refuse `AlreadyPointer`, spending a per-pass budget slot every
/// pass to do it. It also lies to the readers that take this table to mean
/// *paths this clone has held content for*: [`materialized_paths`], which feeds
/// `Engine::pending_files`' `replacing` flag, and `lfs::listing::collect`.
/// Withdrawing an instruction the ledger never recorded is nothing at all,
/// exactly as [`note_use`] and [`note_synced`] are UPDATE-only for the same
/// reason.
pub fn set_pinned(
    conn: &Connection,
    profile_id: &str,
    path: &str,
    pinned: bool,
    now_ms: i64,
) -> Result<()> {
    if pinned {
        conn.execute(
            "INSERT INTO materialized (profile_id, path, at_ms, pinned) VALUES (?1, ?2, ?3, 1)
             ON CONFLICT(profile_id, path) DO UPDATE SET pinned = 1",
            (profile_id, path, now_ms),
        )?;
    } else {
        conn.execute(
            "UPDATE materialized SET pinned = 0 WHERE profile_id = ?1 AND path = ?2",
            (profile_id, path),
        )?;
    }
    Ok(())
}

/// Record the instant the owner asked for one path's content to be let go
/// again, or withdraw that instruction (Story 56.17, `release_at_ms`).
///
/// One column either way, naming exactly `release_at_ms`, in [`set_pinned`]'s
/// two-direction shape and for its reasons: it must not disturb either clock,
/// `local_origin`, the recorded identity, `pinned` or `at_ms`. `at_ms` on
/// insert is the instant the instruction was given, which is the only
/// timestamp this call knows.
///
/// `at_ms` here is an absolute epoch-ms **deadline**, not a duration: a
/// duration serialized into a ledger would be stale by however long the row
/// sat there, and [`crate::engine::release_due_at`] compares against a clock.
/// The caller resolves the person's `2h` against the injected clock once, so a
/// verb that writes this twice — `Engine::materialize_entry_now` observes its
/// own request a second time after draining — writes the same instant rather
/// than a later one.
///
/// # Why the insert stamps `released_at_ms`, and the conflict arm never does
///
/// A duration is recorded when the person asks, and for an object this machine
/// does not hold yet that is *before* any content lands: the ask queues a
/// download and the row has to be waiting when the bytes arrive. An inserted
/// row with `released_at_ms` `NULL` would then tell [`materialized_paths`],
/// [`materialized_rows`] and `lfs::listing::collect` that this clone holds
/// content it does not — the phantom-row loss [`set_pinned`]'s own doc records
/// for its pin arm. The honest value is available for free here, because that
/// column's documented meaning is exactly *the content is not here*, which for
/// a queued path is true; and [`remember_materialized`],
/// [`observe_materialized`] and [`note_local_authorship`] all clear it the
/// moment content does land, so nothing else has to know.
///
/// The conflict arm must **not** carry it: a row that already exists is a row
/// about content whose presence somebody else established, and marking it
/// released would hide a materialized path from every present-tense reader.
///
/// # Withdrawing is UPDATE-only
///
/// [`set_pinned`]'s asymmetry verbatim. Withdrawing an instruction the ledger
/// never recorded is nothing at all, and an upsert here would insert a row
/// asserting "content landed here now" for content that is not here — which
/// [`crate::engine::release_due_at`] reads as a candidate forever.
///
/// `None` is also what *indefinite* writes: a materialize with no duration,
/// `--for 0`, and the row's own Indefinitely are three spellings of one
/// instruction — put this path back on the folder's window — and they must not
/// mean three things. On a path nobody ever named a time for the statement
/// sets `NULL` to `NULL`, which is why the plain verb's behaviour is unchanged.
pub fn set_release_at(
    conn: &Connection,
    profile_id: &str,
    path: &str,
    at_ms: Option<i64>,
    now_ms: i64,
) -> Result<()> {
    match at_ms {
        Some(at_ms) => {
            conn.execute(
                "INSERT INTO materialized
                     (profile_id, path, at_ms, release_at_ms, released_at_ms)
                 VALUES (?1, ?2, ?3, ?4, ?3)
                 ON CONFLICT(profile_id, path)
                 DO UPDATE SET release_at_ms = excluded.release_at_ms",
                (profile_id, path, now_ms, at_ms),
            )?;
        }
        None => {
            conn.execute(
                "UPDATE materialized SET release_at_ms = NULL
                  WHERE profile_id = ?1 AND path = ?2",
                (profile_id, path),
            )?;
        }
    }
    Ok(())
}

/// Every path this clone holds content for **right now**.
///
/// Read whole rather than asked per row: the caller is deciding a mark for a
/// list, and one statement beats a query per line.
///
/// **`released_at_ms IS NULL` is load-bearing, not tidiness** (Story 56.14).
/// Since a release retains the row rather than deleting it, the table also
/// holds paths whose content is gone, and this function's whole contract is
/// the present tense: `Engine::pending` reads it as the `replacing` flag, so a
/// released row leaking through would announce a queued download as replacing
/// content that is not there — the exact false statement
/// [`forget_materialized`]'s `DELETE` was there to prevent.
pub fn materialized_paths(
    conn: &Connection,
    profile_id: &str,
) -> Result<std::collections::HashSet<String>> {
    let mut stmt = conn.prepare(
        "SELECT path FROM materialized
          WHERE profile_id = ?1 AND released_at_ms IS NULL",
    )?;
    let rows = stmt.query_map([profile_id], |r| r.get::<_, String>(0))?;
    let mut out = std::collections::HashSet::new();
    for row in rows {
        out.insert(row?);
    }
    Ok(out)
}

/// One row of the `materialized` ledger, late columns included (Stories 56.2,
/// 56.5 and 56.17).
///
/// A struct rather than a tuple because six of the nine fields are
/// `Option`s of two types and five of them are timestamps: a transposition
/// between `last_used_ms` and `synced_at_ms` would compile, pass every type
/// check, and make a release decision on the wrong fact.
///
/// **Every late timestamp is an `Option` and that is a fact, not a
/// placeholder.** A row written before Story 56.2 — or by
/// [`remember_materialized`], which sets `at_ms` and nothing else — reads back
/// `None` for all of them, and `None` means "nobody recorded this" rather than
/// zero, epoch, or absent-from-the-remote. Story 56.5's writers
/// ([`note_use`], [`note_arrival`], [`note_local_authorship`], [`note_synced`],
/// [`set_pinned`]) fill them in, each one fact at a time.
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
    /// Which way the content at this path travelled (Story 56.5, AD-131).
    /// `true` means this clone created or modified it; `false` means it
    /// arrived from the remote. `false` for every row that has never said
    /// otherwise, including every row written before the column existed —
    /// which is honest rather than a default, because the only writers this
    /// table had before Story 56.5 were arrival paths. It selects which
    /// release clock applies: `synced_at_ms` for local content, so content
    /// this machine authored and has not yet pushed is never eligible, and
    /// `last_used_ms` for content that came from the remote.
    pub local_origin: bool,
    /// The absolute epoch-ms instant the owner asked for this one path's
    /// content to be let go again (Story 56.17), or `None` for a path on the
    /// folder's own `releaseTtlMs` window — which is every path nobody has
    /// named a time for, and every row written before the column existed.
    ///
    /// [`crate::engine::release_due_at`] reads it **instead of** the folder's
    /// window, in both directions: an hour chosen inside a day-long window
    /// goes sooner, and two days chosen inside it stay longer. It does not
    /// outrank [`Self::pinned`], and it does not reach past FR-341 — a path
    /// this clone authored that nothing has confirmed the remote holds is
    /// still on no clock at any age, because a chosen duration must not become
    /// a way around the barrier that stops keeper deleting bytes which exist
    /// on one machine.
    pub release_at_ms: Option<i64>,
}

/// Every `materialized` row whose content this profile **still holds**, whole.
///
/// Wider than [`materialized_paths`], and beside it rather than replacing it:
/// that one answers a single yes/no per path, which is all the arrival
/// decision needs and all it should be able to see, while this one carries the
/// columns a listing reports. Read in one statement for the same reason it is:
/// the caller is building a list, and a query per row is how a folder of a
/// hundred thousand paths becomes a minute of SQLite.
///
/// **It carries [`materialized_paths`]' `released_at_ms IS NULL` filter, and
/// for a sharper reason** (Story 56.14). Its two readers are `Engine::lfs_files`
/// and the release sweep's candidate list, and both are about content that is
/// on this disk now: a released row reaching the sweep would be handed to
/// `Engine::release_resolved`, which would spend an index read and an `lstat`
/// per pass to refuse `AlreadyPointer` forever — and the sweep's own retraction
/// arm would then try to retract a row that is already retracted, in a loop
/// bounded only by the byte budget.
pub fn materialized_rows(conn: &Connection, profile_id: &str) -> Result<Vec<MaterializedRow>> {
    let mut stmt = conn.prepare(
        "SELECT path, at_ms, last_used_ms, synced_at_ms, oid, size_bytes, pinned, local_origin,
                release_at_ms
           FROM materialized
          WHERE profile_id = ?1 AND released_at_ms IS NULL
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
            r.get::<_, Option<i64>>(7)?,
            r.get::<_, Option<i64>>(8)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (path, at_ms, last_used_ms, synced_at_ms, oid, size, pinned, local_origin, release_at) =
            row?;
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
            // `NULL` is *arrived from the remote*: see the column's own note
            // in [`ensure_materialized_columns`].
            local_origin: local_origin.unwrap_or(0) != 0,
            // `NULL` is *this path is on the folder's own window*, which is
            // the only reading of a row nobody named a time for (Story 56.17).
            release_at_ms: release_at,
        });
    }
    Ok(out)
}

/// Has the owner asked for this one path's content to stay on this machine?
///
/// One column, one path, one statement. Beside [`materialized_rows`] rather
/// than expressed through it for the reason [`materialized_paths`] is beside
/// it: the caller here is deciding about a single path and reading the whole
/// ledger to answer would be a table scan to fetch one bit.
///
/// **An absent row and a `NULL` are both `false`**, which is
/// [`MaterializedRow::pinned`]'s documented rule, not a convenience: an
/// unpinned path is the default, so a row that has never said otherwise —
/// including every row written before the column existed — means the same
/// thing as no row at all. There is no third answer for a caller to act on.
///
/// This function knows exactly one fact, and [`remember_materialized`]'s doc
/// records what a statement that knew more than it needed to cost last time.
pub fn is_pinned(conn: &Connection, profile_id: &str, path: &str) -> Result<bool> {
    let pinned = conn
        .query_row(
            "SELECT pinned FROM materialized WHERE profile_id = ?1 AND path = ?2",
            (profile_id, path),
            |r| r.get::<_, Option<i64>>(0),
        )
        .optional()?;
    Ok(pinned.flatten().unwrap_or(0) != 0)
}

/// Record that this machine no longer holds content for one path.
///
/// The counterpart to [`remember_materialized`], called once the content has
/// actually gone.
///
/// It is **not** what makes `lfs::listing` call a path virtual. That comes from
/// the worktree (`listing::collect` reads `worktree_pointer(..).is_some()`);
/// the ledger only supplies the timestamps and the pin.
///
/// Touches exactly `(profile_id, path)` and nothing else. Retracting an absent
/// row succeeds — the same contract [`crate::platform::SyncPlatform::secret_delete`]
/// states, and for the same reason: the caller wants the fact gone, and it
/// already being gone is that.
///
/// # Why this stamps a column instead of deleting the row (Story 56.14)
///
/// It was a `DELETE` until Story 56.14, and the argument for that was sound as
/// far as it went: the row's headline meaning is "content for this path exists
/// here", it does not, and a retained row would mislead
/// [`materialized_paths`] — which feeds `Engine::pending`'s `replacing` flag —
/// into announcing a queued download as replacing content that is no longer
/// there.
///
/// What that argument missed is the rest of the row. [`ensure_materialized_columns`]
/// says `oid` and `size_bytes` exist "so a row still answers after the worktree
/// stops holding a pointer to consult", and calls the five late columns facts
/// the ledger has to hold *before anything can decide what to release* — and a
/// `DELETE` discarded `last_used_ms`, `synced_at_ms` and `local_origin` at the
/// exact instant they were designed to still answer. The recency history a TTL
/// sweep reasons with was erased by the sweep itself, so a path released and
/// re-materialized came back looking like a path nobody had ever touched.
///
/// Both halves are satisfied by keeping the row and stamping `released_at_ms`:
/// every present-tense reader ([`materialized_paths`], [`materialized_rows`])
/// filters `released_at_ms IS NULL`, so nothing can read a released row as
/// content that is here, while the columns survive. [`remember_materialized`]
/// and [`observe_materialized`] clear the stamp when content lands again, and
/// [`note_use`]'s bare `UPDATE` goes on recording a read of a released path,
/// which is honest — somebody opened it — and is the fact a later
/// re-materialization wants.
///
/// # What it costs
///
/// The table now grows with the number of distinct paths this profile has ever
/// hydrated rather than with the number it currently holds, and **nothing
/// prunes a released row**: there is no `DELETE FROM materialized` anywhere in
/// this crate. So the bound is paths-ever-hydrated, which for a folder with
/// renames, dated exports or a rolling archive grows with time and not with the
/// cone. Both filtered readers stay correct and index-ranged either way; the
/// cost is disk, and it is small per row. Bounding it needs a rule about which
/// released rows are worth keeping — an age relative to the folder's TTL, plus
/// an index read to know the path is gone upstream — which is a decision rather
/// than an edit, and it is recorded as deferred work rather than guessed at
/// here.
///
/// # Why the statement will not touch a pinned row
///
/// A pinned path never reaches here: the refusal is upstream, in
/// [`crate::engine::Engine::dehydrate_entry`], which reads [`is_pinned`] twice
/// and declines. `AND COALESCE(pinned, 0) = 0` is the belt to that braces, and
/// it is here for the loss class [`remember_materialized`]'s own doc records:
/// `pinned` is the hard floor a release sweep may not cross, so losing it is
/// "invisible until content the owner asked to keep was gone". A statement that
/// is structurally incapable of retracting a pinned row cannot participate in
/// that, whatever a future caller does. `COALESCE` because a `NULL` **is**
/// unpinned — [`MaterializedRow::pinned`]'s documented rule, and every row
/// written before the column existed reads back that way.
///
/// # Why `release_at_ms` is the one column it clears (Story 56.17)
///
/// A chosen release time is a standing instruction, and this is the moment it
/// is **served**: the content the owner asked to keep for two hours is gone.
/// Left standing it would be a deadline in the past, so the same path
/// materialized again with no duration — [`remember_materialized`] clears
/// `released_at_ms`, and the row comes back live — would be eligible for
/// release on the very next sweep, hours before the folder's own window says
/// so. It is cleared beside the stamp rather than by a second statement
/// because the two are one fact: this content left, and nobody is any longer
/// waiting for it to.
pub fn forget_materialized(
    conn: &Connection,
    profile_id: &str,
    path: &str,
    now_ms: i64,
) -> Result<()> {
    conn.execute(
        "UPDATE materialized SET released_at_ms = ?3, release_at_ms = NULL
          WHERE profile_id = ?1 AND path = ?2 AND COALESCE(pinned, 0) = 0",
        (profile_id, path, now_ms),
    )?;
    Ok(())
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
    // A task naming a folder that no longer exists is a task whose every run
    // fails permanently, on a schedule, forever — and 57.6 will notify on the
    // onset of that. There is deliberately no foreign key (see the schema), so
    // the history goes first and by hand.
    conn.execute(
        "DELETE FROM task_runs
          WHERE task_id IN (SELECT id FROM tasks WHERE profile_id = ?1)",
        [id],
    )?;
    conn.execute("DELETE FROM tasks WHERE profile_id = ?1", [id])?;
    conn.execute("DELETE FROM profiles WHERE id = ?1", [id])?;
    Ok(())
}

/// Record the sentence a person reads beside this folder, or clear it
/// (Story 56.15).
///
/// # This column had no writer at all
///
/// `profiles.last_error` has existed since the schema was written and nothing
/// ever set it: [`set_profile_state`] writes `state` only, and the sole
/// function that touched `last_error` — a `set_profile_runtime` this replaces
/// — had no callers in any crate. So the column was `NULL` for every profile
/// in every state, which is exactly what the owner's `sync.db` was measured
/// holding for a folder whose first clone had died three minutes in:
/// `state = 'idle'`, `last_error = NULL`, and no other record anywhere that
/// anything had gone wrong.
///
/// Split from [`set_profile_state`] rather than folded into it because they
/// answer different questions and change at different moments: a folder can
/// go `Syncing → Watching` a hundred times while one error stands, and a
/// state write that also cleared the error would erase the reason on the very
/// next tick.
pub fn set_profile_error(conn: &Connection, id: &str, last_error: Option<&str>) -> Result<()> {
    conn.execute(
        "UPDATE profiles SET last_error = ?2 WHERE id = ?1",
        (id, last_error),
    )?;
    Ok(())
}

/// Whether this profile owns a journal unit that is claimable right now.
///
/// One indexed lookup on `journal_ready` (`state, not_before_ms`), stopping at
/// the first row: the question is "is there work due", never "how much".
///
/// Exists so an offline profile's tick can be paced by the queue rather than
/// by a clock of its own — see `Engine::remote_within_reach`. A unit that
/// failed on the network is rescheduled with the engine's backoff, so "a unit
/// is due" is precisely "the backoff says try the remote again now".
pub fn has_ready_unit(conn: &Connection, profile_id: &str, now_ms: i64) -> Result<bool> {
    let found: Option<i64> = conn
        .query_row(
            "SELECT 1 FROM journal
              WHERE profile_id = ?1 AND state = 'pending' AND not_before_ms <= ?2
              LIMIT 1",
            (profile_id, now_ms),
            |row| row.get(0),
        )
        .optional()?;
    Ok(found.is_some())
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
    /// Make this folder's working copy exist: clone it, adopt it, or finish a
    /// checkout that stopped part-way (Story 56.15).
    ///
    /// Journaled, where the rest of `open_repo` is not, because it is the one
    /// piece of repository setup that can fail *after* touching the network
    /// and must be retried on a backoff rather than on the scan cadence. A
    /// profile whose first clone never finished is not idle and has no tree to
    /// scan, so `scan_is_due` — which decides whether a walk is worth its cost
    /// — is the wrong authority for whether it is retried at all.
    Checkout,
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
    /// The `kind` column spelling for one LFS object download. Same reason —
    /// [`crate::engine::Engine::materialize_entry_now`] drains until this
    /// profile owes no more of them, and it has no `WorkKind` value to ask
    /// with because the object it is waiting for may already be gone from the
    /// journal.
    pub const LFS_DOWNLOAD: &'static str = "lfsDownload";
    /// The `kind` column spelling for a pull. Same reason — a rejected push
    /// queues one and has to be able to assert it did (DW-207).
    pub const PULL: &'static str = "pull";
    /// The `kind` column spelling for a checkout. Same reason —
    /// [`crate::engine::Engine::tick_profile`] drains this kind ALONE for a
    /// folder with no working copy, and has to name it to do that.
    pub const CHECKOUT: &'static str = "checkout";

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
        matches!(
            self,
            // A clone of a 16 GB folder runs for minutes and is idempotent:
            // the second unit could only clone what the first one is cloning,
            // and queueing one per tick would put an hour of duplicate rows
            // behind a single running checkout.
            Self::Checkout | Self::LfsUpload { .. } | Self::LfsDownload { .. }
        )
    }

    /// Discriminant used as the journal's `kind` column, so a row can be
    /// filtered without deserializing its payload.
    pub fn tag(&self) -> &'static str {
        match self {
            Self::Pull => Self::PULL,
            Self::Checkout => Self::CHECKOUT,
            Self::Push => Self::PUSH,
            Self::LfsUpload { .. } => Self::LFS_UPLOAD,
            Self::LfsDownload { .. } => Self::LFS_DOWNLOAD,
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

/// Whether anybody is waiting for a queued unit (Story 56.3).
///
/// Two levels and no more. The question a claim has to answer is "did a human
/// ask for this, or did a scan find it", and a numeric priority space would
/// invite a third answer nobody can define — the journal has no notion of how
/// *much* somebody wants a file.
///
/// # It is a column, not a second queue
///
/// [`claim_ready`] is one statement over `state = 'pending' AND not_before_ms
/// <= now`; urgency prepends one term to its `ORDER BY` and changes nothing
/// else. A separate "urgent" table would duplicate the claim, and a
/// `not_before_ms = 0` trick would lie about scheduling and still lose to an
/// older background row. One nullable integer keeps FIFO order within a level,
/// keeps `CLAIM_LIMIT` per profile per tick, and keeps the batch bounded: a
/// requested unit takes a slot, it does not take the tick, so nothing is
/// starved.
///
/// # It is not part of the payload
///
/// See [`raise_urgency`] — putting it in the payload would change
/// [`enqueue_unique`]'s dedup key and re-download bytes this machine is already
/// fetching.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord)]
pub enum Urgency {
    /// A scan found this work. The level a row written before the column
    /// existed reads back as, which is why it is the default.
    #[default]
    Background,
    /// A person asked for this path by name, so it is claimed ahead of
    /// background work in the same tick and published even under
    /// [`crate::profile::LfsMode::PointerOnly`].
    Requested,
}

impl Urgency {
    /// The stored integer. Ordered, because the whole point is that
    /// `COALESCE(urgency, 0) DESC` sorts by it and `MAX` raises by it.
    pub fn level(self) -> i64 {
        match self {
            Self::Background => 0,
            Self::Requested => 1,
        }
    }

    /// The level a stored integer names.
    ///
    /// Saturating rather than exact: `NULL` arrives here as `0` from the
    /// caller's `unwrap_or`, and anything a future release writes above
    /// `Requested` still reads as "somebody is waiting" rather than as an error
    /// that would park an otherwise perfectly good row.
    pub fn from_level(level: i64) -> Self {
        if level >= Self::Requested.level() {
            Self::Requested
        } else {
            Self::Background
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
    /// Whether anybody is waiting for this unit.
    ///
    /// Read from the row rather than remembered anywhere else, so it is the
    /// *journal* that remembers a human asked — which is what makes the fact
    /// survive a restart, a `recover_running` and a re-drive.
    pub urgency: Urgency,
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
    //
    // `ORDER BY id`, so "the covering row" is a defined row and not whichever
    // one the query planner reached first. It matters for the two writes a
    // caller makes onto the returned id: [`recover_running`]'s duplicate
    // collapse keeps `MIN(id)`, so a label or an urgency written onto the
    // higher of a legacy duplicate pair would be thrown away at the next
    // restart.
    let sql = if kind.covered_while_running() {
        "SELECT id FROM journal
          WHERE profile_id = ?1 AND payload = ?2
            AND state IN ('pending','deferred','running')
          ORDER BY id LIMIT 1"
    } else {
        "SELECT id FROM journal
          WHERE profile_id = ?1 AND payload = ?2 AND state IN ('pending','deferred')
          ORDER BY id LIMIT 1"
    };
    let existing: Option<i64> = conn
        .query_row(sql, (profile_id, &payload), |r| r.get(0))
        .optional()?;
    if let Some(id) = existing {
        return Ok(id);
    }
    enqueue(conn, profile_id, kind, now_ms, not_before_ms)
}

/// Claim the ready units for one profile, marking them `running` so two
/// supervisors can never take the same row.
///
/// # The order is urgency, then age — and one slot is kept back
///
/// `COALESCE(urgency, 0) DESC, id` puts a unit somebody asked for ahead of
/// every background unit in the same batch, and leaves background work in its
/// existing FIFO order among itself (Story 56.3). `COALESCE` because the
/// column is nullable for every row queued before it existed, and `DESC` on a
/// two-level [`Urgency`] rather than a priority number for the reason that
/// type gives.
///
/// That order alone would starve background work outright, and the first
/// version of this shipped believing it could not. `limit` is a bound per
/// *tick*, not per request: ask for sixteen files and every slot in the batch
/// is a requested download, `drain_journal` runs the batch serially, and the
/// `Push` that carries a local edit to the server waits for all sixteen
/// transfers — then for the next batch, for as long as requests keep arriving.
/// A folder that stops publishing local work while somebody browses media is
/// unbacked-up data, which outranks any latency a request can lose.
///
/// So a full batch is never *entirely* requested work while background work is
/// waiting: the youngest requested row gives its slot back to the oldest
/// background row. One slot is enough — `drain_journal` runs every tick, so the
/// background queue drains at one unit per second in the worst case while
/// fifteen sixteenths of the batch still serve the person waiting. The batch
/// size, and therefore `limit`'s meaning, does not change.
pub fn claim_ready(
    conn: &Connection,
    profile_id: &str,
    now_ms: i64,
    limit: u32,
) -> Result<Vec<WorkItem>> {
    claim(conn, profile_id, now_ms, limit, None)
}

/// [`claim_ready`] narrowed to one `kind` (Story 56.13).
///
/// For a caller that is draining to satisfy **one request** rather than to move
/// the folder along: `keeper-syncd materialize` waits for its own transfer, and
/// a verb documented as "fetch one path's content" must not commit, merge,
/// publish a branch or open a pull request on the way. `claim_ready` selects on
/// `profile_id` and `state` alone, so without this the whole ready queue rides
/// along — and a `Pull` drained that way can move the very pointer the request
/// is about.
///
/// Every other property is `claim_ready`'s, including the background-slot
/// carve-out: within one kind it still stops a full batch of requested work
/// from starving the oldest background row.
pub fn claim_ready_of_kind(
    conn: &Connection,
    profile_id: &str,
    now_ms: i64,
    limit: u32,
    kind: &str,
) -> Result<Vec<WorkItem>> {
    claim(conn, profile_id, now_ms, limit, Some(kind))
}

/// The claim both doors above run. `kind` of `None` means every kind.
fn claim(
    conn: &Connection,
    profile_id: &str,
    now_ms: i64,
    limit: u32,
    kind: Option<&str>,
) -> Result<Vec<WorkItem>> {
    let mut rows = ready_rows(
        conn,
        "SELECT id, payload, attempts, last_error, label, urgency FROM journal
         WHERE profile_id = ?1 AND state = 'pending' AND not_before_ms <= ?2
           AND (?4 IS NULL OR kind = ?4)
         ORDER BY COALESCE(urgency, 0) DESC, id LIMIT ?3",
        profile_id,
        now_ms,
        limit,
        kind,
    )?;
    // Only when the batch is full AND holds nothing but requested work: with a
    // spare slot the background row was claimed by the statement above anyway,
    // and with one row in the batch there is no slot to keep back.
    let all_requested = rows
        .iter()
        .all(|(_, _, _, _, _, urgency)| urgency.unwrap_or(0) > 0);
    if limit > 1 && rows.len() as u32 == limit && all_requested {
        let background = ready_rows(
            conn,
            "SELECT id, payload, attempts, last_error, label, urgency FROM journal
             WHERE profile_id = ?1 AND state = 'pending' AND not_before_ms <= ?2
               AND COALESCE(urgency, 0) = 0
               AND (?4 IS NULL OR kind = ?4)
             ORDER BY id LIMIT ?3",
            profile_id,
            now_ms,
            1,
            kind,
        )?;
        if let Some(oldest) = background.into_iter().next() {
            // The youngest requested row, which is the last one: it keeps its
            // place in the queue and is claimed on the next tick.
            rows.pop();
            rows.push(oldest);
        }
    }

    let mut claimed = Vec::new();
    for (id, payload, attempts, last_error, label, urgency) in rows {
        match serde_json::from_str::<WorkKind>(&payload) {
            Ok(kind) => claimed.push(WorkItem {
                id,
                profile_id: profile_id.to_owned(),
                kind,
                attempts: attempts.max(0).saturating_add(1) as u32,
                last_error,
                label,
                urgency: Urgency::from_level(urgency.unwrap_or(0)),
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

/// One claim query's raw rows, in the order the statement returned them.
///
/// Split out because [`claim_ready`] runs the same projection twice — once for
/// the batch and once for the background row it keeps a slot for — and two
/// spellings of six column indices is how the two come to disagree.
type ReadyRow = (
    i64,
    String,
    i64,
    Option<String>,
    Option<String>,
    Option<i64>,
);

fn ready_rows(
    conn: &Connection,
    sql: &str,
    profile_id: &str,
    now_ms: i64,
    limit: u32,
    kind: Option<&str>,
) -> Result<Vec<ReadyRow>> {
    let mut stmt = conn.prepare(sql)?;
    // `?4` is bound whether or not the statement narrows on it: `(?4 IS NULL OR
    // kind = ?4)` is how one projection serves both doors, and two spellings of
    // six column indices is what this helper exists to prevent.
    let rows = stmt.query_map((profile_id, now_ms, limit, kind), |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, Option<String>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<i64>>(5)?,
        ))
    })?;
    rows.collect::<rusqlite::Result<Vec<_>>>()
        .map_err(SyncError::from)
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
///
/// Neither statement below touches `label` or `urgency`, and that is what makes
/// a request survive a crash: the row comes back `pending` still carrying the
/// level [`raise_urgency`] wrote, so the person who asked keeps their place in
/// the next claim. It is also the second reason urgency must live on the
/// *covering* row rather than on a duplicate — the `MIN(id)` collapse below
/// deletes every pending row but the oldest for a payload, so an urgency
/// written onto a duplicate would be thrown away here.
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

// ---------------------------------------------------------------------------
// Tasks (Story 57.1, AD-135)
// ---------------------------------------------------------------------------

/// How many runs of ONE task are kept, for [`ACTIVITY_CAP`]'s reason: a task
/// that fires every five minutes writes a row every five minutes forever, and
/// `sync.db` must not grow because a schedule is doing its job. Fifty is what
/// answers "has this been working lately", which is the only question the
/// history is ever asked.
pub const TASK_RUNS_CAP: usize = 50;

/// Every column of `tasks`, in the order [`read_task`] decodes them. One
/// constant so a reader added later cannot drift out of step with the decoder.
const TASK_COLUMNS: &str = "id, profile_id, kind, schedule, mode, next_due_ms, \
                            enabled, updated_ms, running_host, lease_until_ms";

/// One stored task.
///
/// `schedule` holds the expression **as written**, never a normalized form: it
/// is what the operator typed and what a refusal has to be able to quote back,
/// and re-rendering a parsed schedule would silently rewrite `@daily` into
/// something the person who typed it did not choose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRow {
    pub id: String,
    /// `None` means host-wide: this task belongs to the machine, not a folder.
    pub profile_id: Option<String>,
    pub kind: TaskKind,
    /// The expression as written, or `None` for a task nothing schedules.
    pub schedule: Option<String>,
    pub mode: TaskMode,
    /// `None` means never armed, which is what makes a first sight arm rather
    /// than run.
    pub next_due_ms: Option<i64>,
    pub enabled: bool,
    pub updated_ms: i64,
    /// The host holding the lease, if a run is in flight.
    pub running_host: Option<String>,
    pub lease_until_ms: Option<i64>,
}

impl TaskRow {
    /// Project the row onto exactly the four facts [`crate::tasks::decide`]
    /// reads, so the pure gate never sees a `Connection` or a whole row it
    /// could be tempted to consult.
    pub fn state(&self) -> TaskState {
        TaskState {
            enabled: self.enabled,
            mode: self.mode,
            next_due_ms: self.next_due_ms,
            lease_until_ms: self.lease_until_ms,
        }
    }

    /// The parsed schedule, or `Ok(None)` when nothing schedules this task.
    ///
    /// A parse error propagates unchanged rather than becoming `None`: "no
    /// schedule" and "a schedule this build cannot read" are different facts,
    /// and treating the second as the first would quietly disarm a task the
    /// operator believes is running.
    pub fn parsed_schedule(&self) -> Result<Option<TaskSchedule>> {
        match self.schedule.as_deref() {
            None => Ok(None),
            Some(expression) => TaskSchedule::parse(expression).map(Some),
        }
    }
}

/// A stored task this build cannot act on, and why (NFR-43).
///
/// Reported beside the readable ones rather than swallowed, so a UI can say
/// "this folder has a task your keeper is too old to understand" instead of
/// showing a folder with no tasks at all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownTask {
    pub id: String,
    pub reason: String,
}

/// What [`list_tasks`] found: the rows this build can run, and the rows it
/// cannot.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TaskListing {
    pub tasks: Vec<TaskRow>,
    pub unknown: Vec<UnknownTask>,
}

/// One attempt at one task.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskRunRow {
    pub id: i64,
    pub task_id: String,
    pub started_ms: i64,
    /// `None` means still in flight — or that the host running it died, which
    /// is why [`claim_task`] closes what it reclaims.
    pub finished_ms: Option<i64>,
    /// The outcome, when this build has a variant for it.
    ///
    /// `None` together with a `None` [`Self::unknown_outcome`] means the run is
    /// still in flight.
    pub outcome: Option<TaskOutcome>,
    /// The stored spelling, when this build has no variant for it (NFR-43).
    ///
    /// A run recorded by a newer keeper is still a run that happened, and the
    /// newest one is the one every surface reports as "last": it is carried
    /// here rather than dropped so nothing shows an older attempt as current.
    pub unknown_outcome: Option<String>,
    pub detail: Option<String>,
    pub host: String,
}

/// The `tasks` row as SQLite hands it over, before the vocabulary is applied.
type StoredTask = (
    String,
    Option<String>,
    String,
    Option<String>,
    String,
    Option<i64>,
    i64,
    i64,
    Option<String>,
    Option<i64>,
);

/// Read one `tasks` row, tolerating every column but the primary key.
///
/// SQLite is dynamically typed, so a row written by a keeper that changed a
/// column's type — or by a hand at a prompt — hands back a value this build's
/// `get` refuses. Strict reads here would make `query_map` yield an error for
/// that one row and, before Story 57.2's review, took the whole listing with
/// it: one unreadable row and **no task on the host ever ran again**. That is
/// precisely the outcome NFR-43 forbids.
///
/// So every column but `id` falls back to its default, which routes the row
/// straight to [`decode_task`]'s unknown path: a defaulted `kind` or `mode` is
/// an empty string, and no variant answers to that. `id` stays strict because a
/// row whose primary key cannot be read cannot be named in a report either, and
/// [`list_tasks`] handles that failure separately.
fn read_task(row: &rusqlite::Row<'_>) -> rusqlite::Result<StoredTask> {
    Ok((
        row.get(0)?,
        row.get(1).unwrap_or_default(),
        row.get(2).unwrap_or_default(),
        row.get(3).unwrap_or_default(),
        row.get(4).unwrap_or_default(),
        row.get(5).unwrap_or_default(),
        row.get(6).unwrap_or_default(),
        row.get(7).unwrap_or_default(),
        row.get(8).unwrap_or_default(),
        row.get(9).unwrap_or_default(),
    ))
}

/// Apply this build's vocabulary to a stored row, or say why it cannot be.
///
/// Three ways a row can be unreadable and all three are the same answer: a
/// `kind` with no variant here, a `mode` with no variant here, and a schedule
/// this parser rejects. Running any of them would mean guessing what somebody
/// else's keeper meant.
fn decode_task(stored: StoredTask) -> std::result::Result<TaskRow, UnknownTask> {
    let (
        id,
        profile_id,
        kind,
        schedule,
        mode,
        next_due_ms,
        enabled,
        updated_ms,
        running_host,
        lease_until_ms,
    ) = stored;
    let unknown = |reason: String| UnknownTask {
        id: id.clone(),
        reason,
    };
    let Some(kind) = TaskKind::from_stored(&kind) else {
        return Err(unknown(format!("unknown task kind '{kind}'")));
    };
    let Some(mode) = TaskMode::from_stored(&mode) else {
        return Err(unknown(format!("unknown task mode '{mode}'")));
    };
    let row = TaskRow {
        id: id.clone(),
        profile_id,
        kind,
        schedule,
        mode,
        next_due_ms,
        enabled: enabled != 0,
        updated_ms,
        running_host,
        lease_until_ms,
    };
    if let Err(err) = row.parsed_schedule() {
        return Err(unknown(format!("unreadable schedule: {err}")));
    }
    Ok(row)
}

/// The write door, and therefore the refusal point (FR-347).
///
/// Validation runs before any SQL, the way [`set_device_label`] and
/// [`upsert_profile`] validate first: a schedule is refused *when it is saved*,
/// with the expression quoted, because the alternative is a row that reports
/// itself enabled and silently never fires.
///
/// # Runtime state is not the caller's to write
///
/// `running_host`, `lease_until_ms` and `next_due_ms` are owned by
/// [`claim_task`], [`finish_task_run`] and [`arm_task`]. This function binds the
/// lease columns to `NULL` on insert and never touches them on conflict, and it
/// ignores the caller's `next_due_ms` entirely. Every one of those was a real
/// hole before Story 57.2's review: a `TaskRow` carrying a stale window — which
/// is exactly what a read-modify-write from a settings form produces — rewound
/// the schedule and re-ran the window on the next tick, or, with a fresh
/// `next_due_ms: None` saved on every keystroke, postponed an `every 5m` task a
/// full interval per save and so *never* fired it; and a row inserted with a
/// future `lease_until_ms` and no `running_host` was held off by
/// [`crate::tasks::decide`] forever.
///
/// The window is cleared when the **schedule text changes**, so a new schedule
/// takes effect on the next tick rather than after the old one's window elapses.
/// `IS NOT` rather than `<>` because SQLite's `<>` is NULL-poisoned and a task
/// gaining or losing its schedule is precisely the case that must be noticed.
pub fn upsert_task(conn: &Connection, task: &TaskRow) -> Result<()> {
    if task.id.trim().is_empty() {
        return Err(SyncError::Config("task id must not be empty".into()));
    }
    // Refused rather than trimmed: the id is a primary key, it is what 57.3's
    // CLI reads from argv and what `task_runs.task_id` joins on, and silently
    // accepting three spellings of one intended task is worse than saying so.
    if task.id.trim() != task.id {
        return Err(SyncError::Config(format!(
            "task id must not begin or end with whitespace, got {:?}",
            task.id
        )));
    }
    // The parser's own refusal, propagated unchanged: it already names the
    // rule and quotes the expression, and a second layer of prose around it
    // would bury the one line that says what is wrong.
    let schedule = task.parsed_schedule()?;
    if task.mode == TaskMode::Scheduled && schedule.is_none() {
        return Err(SyncError::Config(format!(
            "task '{}' is scheduled with no schedule: it would report itself enabled and never run",
            task.id
        )));
    }
    // A row this build cannot read belongs to a newer keeper, and overwriting it
    // would rewrite its kind to one of ours — the write half of NFR-43, which
    // the read half is useless without. `get_task` answers `None` for such a
    // row, so a create-if-absent caller would walk straight into it.
    let stored_kind: Option<String> = conn
        .query_row("SELECT kind FROM tasks WHERE id = ?1", [&task.id], |row| {
            row.get(0)
        })
        .optional()?;
    if let Some(kind) = stored_kind {
        if TaskKind::from_stored(&kind).is_none() {
            return Err(SyncError::Config(format!(
                "task '{}' is stored as kind '{kind}', which this keeper cannot read: \
                 refusing to overwrite it",
                task.id
            )));
        }
    }
    conn.execute(
        "INSERT INTO tasks (id, profile_id, kind, schedule, mode, next_due_ms, enabled,
                            updated_ms, running_host, lease_until_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, NULL, ?6, ?7, NULL, NULL)
         ON CONFLICT(id) DO UPDATE SET
             profile_id  = excluded.profile_id,
             kind        = excluded.kind,
             schedule    = excluded.schedule,
             mode        = excluded.mode,
             next_due_ms = CASE WHEN excluded.schedule IS NOT tasks.schedule
                                THEN NULL ELSE tasks.next_due_ms END,
             enabled     = excluded.enabled,
             updated_ms  = excluded.updated_ms",
        (
            &task.id,
            &task.profile_id,
            task.kind.as_str(),
            &task.schedule,
            task.mode.as_str(),
            i64::from(task.enabled),
            task.updated_ms,
        ),
    )?;
    Ok(())
}

/// Every task, by id, with the unreadable rows listed rather than dropped.
///
/// A row this build cannot read is skipped and named, never fatal — the same
/// tolerance [`list_activity`] and [`list_profiles`] already give: a newer
/// keeper's task must not brick an older one's list, and both binaries share
/// one `sync.db` on purpose.
pub fn list_tasks(conn: &Connection) -> Result<TaskListing> {
    let mut stmt = conn.prepare(&format!("SELECT {TASK_COLUMNS} FROM tasks ORDER BY id"))?;
    let rows = stmt.query_map([], read_task)?;
    let mut listing = TaskListing::default();
    for row in rows {
        // A per-row error, not a listing error. The mapping closure only fails
        // when even `id` will not read, and the statement is still stepping, so
        // the remaining rows are still worth having: one corrupt row must not be
        // able to stop every task on the host.
        let decoded = match row {
            Ok(stored) => decode_task(stored),
            Err(err) => Err(UnknownTask {
                id: String::new(),
                reason: format!("unreadable task row: {err}"),
            }),
        };
        match decoded {
            Ok(task) => listing.tasks.push(task),
            Err(unknown) => {
                tracing::debug!(
                    task = unknown.id,
                    reason = unknown.reason,
                    "skipping a task this build cannot read"
                );
                listing.unknown.push(unknown);
            }
        }
    }
    Ok(listing)
}

/// One task by id, or `Ok(None)`.
///
/// An unreadable row answers `Ok(None)` for [`list_tasks`]'s reason: a caller
/// asking for one row by id wants to know whether there is something here it
/// can act on, and the honest answer for a row from a newer keeper is "no" —
/// not an error that would poison every tick the engine takes.
pub fn get_task(conn: &Connection, id: &str) -> Result<Option<TaskRow>> {
    let stored = conn
        .query_row(
            &format!("SELECT {TASK_COLUMNS} FROM tasks WHERE id = ?1"),
            [id],
            read_task,
        )
        .optional()?;
    Ok(stored.and_then(|stored| match decode_task(stored) {
        Ok(task) => Some(task),
        Err(unknown) => {
            tracing::debug!(
                task = unknown.id,
                reason = unknown.reason,
                "a task this build cannot read reads as absent"
            );
            None
        }
    }))
}

/// Arm an **unarmed** task's first window, and nothing else.
///
/// Arming is not running: the tick that first sees a task computes its window
/// and stops there, so this must not touch the lease, the mode or the schedule.
///
/// `WHERE next_due_ms IS NULL` is what makes it safe on a machine with two
/// hosts. A decision computed from a listing read earlier in this tick can
/// arrive after the other host has already armed and even run the task, and an
/// unconditional write would then rewind the window to a past instant and run it
/// twice. First sight can only happen once, so the statement says so.
pub fn arm_task(conn: &Connection, id: &str, next_due_ms: Option<i64>, now_ms: i64) -> Result<()> {
    conn.execute(
        "UPDATE tasks SET next_due_ms = ?2, updated_ms = ?3
          WHERE id = ?1 AND next_due_ms IS NULL",
        (id, next_due_ms, now_ms),
    )?;
    Ok(())
}

/// Take the lease and open a run, or report that somebody else has it.
///
/// The arbiter is the affected-row count of ONE conditional `UPDATE`. Two hosts
/// sharing `sync.db` need no lock file and no coordinator, because SQLite
/// serializes the two statements and the second one's `WHERE` then fails.
/// `Ok(None)` is not an error: it means the task is off or disabled, its window
/// is not open, or a live lease is held elsewhere.
///
/// `due_at_most` is the instant the window must already have opened by. The
/// due-gate passes `Some(now)`, which closes the hole a lease alone cannot: two
/// hosts ticking together on a task whose work is fast both see a free lease,
/// and the first one's release lets the second run **the same window** a
/// millisecond later. A requested run passes `None`, because a person asking is
/// not asking about a window.
///
/// `mode` is checked in SQL and not only in the caller. The claim is the door
/// the design calls the only arbiter, so the invariant "an off task runs for
/// nobody" has to hold at the door rather than in whichever caller remembered.
///
/// Claim, abandonment and the run row share one transaction. A crash between
/// them would otherwise leave a lease with no run (a task that looks busy
/// forever) or a run with no lease (two hosts doing the same work).
pub fn claim_task(
    conn: &Connection,
    id: &str,
    host: &str,
    now_ms: i64,
    lease_ms: i64,
    due_at_most: Option<i64>,
) -> Result<Option<i64>> {
    // `unchecked_transaction` for [`record_activity`]'s reason: the engine
    // holds this connection behind a `Mutex` and hands out `&Connection`.
    let tx = conn.unchecked_transaction()?;
    let lease_until_ms = now_ms.saturating_add(lease_ms);
    let won = match tx.execute(
        "UPDATE tasks SET running_host = ?2, lease_until_ms = ?3
         WHERE id = ?1 AND enabled = 1 AND mode <> 'off'
           AND (running_host IS NULL OR lease_until_ms IS NULL OR lease_until_ms <= ?4)
           AND (?5 IS NULL OR (next_due_ms IS NOT NULL AND next_due_ms <= ?5))",
        (id, host, lease_until_ms, now_ms, due_at_most),
    ) {
        Ok(rows) => rows,
        Err(err) if is_row_contention(&err) => {
            tracing::debug!(task = id, host, error = %err, "another host is claiming this task");
            return Ok(None);
        }
        Err(err) => return Err(err.into()),
    };
    if won == 0 {
        return Ok(None);
    }
    // A host that was killed mid-run leaves `finished_ms IS NULL` forever.
    // Reclaiming the lease has to record what happened to that attempt, or the
    // history claims a run that never ended. If the previous holder was in fact
    // alive and merely overran its lease, its own `finish_task_run` will write
    // the true outcome over this one when it returns — the run really did end,
    // and the later fact is the better one.
    tx.execute(
        "UPDATE task_runs SET finished_ms = ?2, outcome = ?3
         WHERE task_id = ?1 AND finished_ms IS NULL",
        (id, now_ms, TaskOutcome::Abandoned.as_str()),
    )?;
    tx.execute(
        "INSERT INTO task_runs (task_id, started_ms, host) VALUES (?1, ?2, ?3)",
        (id, now_ms, host),
    )?;
    let run_id = tx.last_insert_rowid();
    // Trim in the same transaction as the insert, for `record_activity`'s
    // reason: a reader must never see the table above its cap. By `id` because
    // two runs of one task can share a millisecond.
    //
    // `finished_ms IS NOT NULL` because a run still in flight is not history: a
    // frequent task whose one long run outlives fifty later claims would
    // otherwise have the row it is about to close deleted underneath it, and
    // `finish_task_run` would update nothing and say nothing.
    tx.execute(
        "DELETE FROM task_runs
         WHERE task_id = ?1
           AND finished_ms IS NOT NULL
           AND id <= COALESCE(
                 (SELECT id FROM task_runs WHERE task_id = ?1
                  ORDER BY id DESC LIMIT 1 OFFSET ?2),
                 -1)",
        (id, TASK_RUNS_CAP as i64),
    )?;
    tx.commit()?;
    Ok(Some(run_id))
}

/// Hand back every lease this host holds, closing the runs it was executing.
///
/// Called from the supervisor's shutdown path. Without it a
/// `systemctl restart keeper-syncd` in the middle of a run left the lease held
/// by `{device}#{a pid that no longer exists}` for the whole of
/// `TASK_LEASE_MS` — and the restarted daemon cannot tell that pid is dead,
/// because on Linux the app shares the device row and may legitimately be
/// holding one. NFR-42 asks for a bounded finalize, and this is what bounds it.
///
/// Returns how many leases were released, so a caller can log the fact rather
/// than assume it.
pub fn release_host_leases(conn: &Connection, host: &str, now_ms: i64) -> Result<usize> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE task_runs SET finished_ms = ?2, outcome = ?3
          WHERE host = ?1 AND finished_ms IS NULL",
        (host, now_ms, TaskOutcome::Abandoned.as_str()),
    )?;
    let released = tx.execute(
        "UPDATE tasks SET running_host = NULL, lease_until_ms = NULL, updated_ms = ?2
          WHERE running_host = ?1",
        (host, now_ms),
    )?;
    tx.commit()?;
    Ok(released)
}

/// `SQLITE_BUSY`/`SQLITE_LOCKED` on the claiming `UPDATE` means "not mine".
///
/// A connection from [`open`] carries a five-second `busy_timeout` (rusqlite's
/// own default; see that function), so reaching this means a writer held the row
/// for longer than any statement in this module takes — or that the caller
/// opened its own connection and disarmed it. Either way the failure carries
/// exactly the fact the claim was asking for: another host is writing this row,
/// so this host does not hold the lease. Every other code is a real fault and
/// propagates.
fn is_row_contention(err: &rusqlite::Error) -> bool {
    matches!(
        err,
        rusqlite::Error::SqliteFailure(err, _)
            if matches!(
                err.code,
                rusqlite::ErrorCode::DatabaseBusy | rusqlite::ErrorCode::DatabaseLocked
            )
    )
}

/// Everything one finished run records.
///
/// A struct rather than seven positional arguments, for [`ActivityEntry`]'s
/// reason: two `&str`s, an `Option<&str>` and two integers in a row is a call
/// site nobody can read or safely reorder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TaskRunClose<'a> {
    /// The `task_runs` row this closes.
    pub run_id: i64,
    pub task_id: &'a str,
    /// The host that took the lease. The release only touches the task row
    /// while that is still the holder — see this struct's function.
    pub host: &'a str,
    pub finished_ms: i64,
    pub outcome: TaskOutcome,
    pub detail: Option<&'a str>,
    /// The window the task should carry afterwards.
    pub next_due_ms: Option<i64>,
}

/// Close the run and release the lease, in one transaction.
///
/// Recording what happened and letting the next host in are one fact: a
/// released lease with no outcome invites a repeat that reports nothing, and an
/// outcome with the lease still held wedges the task until it expires.
///
/// `WHERE running_host = ?host` on the task half is load-bearing and was absent
/// until Story 57.2's review. A host whose pass outlived its lease finds the row
/// already reclaimed; releasing it unconditionally would have freed the **new**
/// holder's lease while it was running, and overwritten its window with a stale
/// one — turning one overrun into any number of concurrent passes over one git
/// working tree. Affecting no task row here is therefore normal, not an error:
/// the run row is still closed with the truth, which is this call's other half.
pub fn finish_task_run(conn: &Connection, close: TaskRunClose<'_>) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute(
        "UPDATE task_runs SET finished_ms = ?2, outcome = ?3, detail = ?4 WHERE id = ?1",
        (
            close.run_id,
            close.finished_ms,
            close.outcome.as_str(),
            close.detail,
        ),
    )?;
    tx.execute(
        "UPDATE tasks
            SET running_host = NULL, lease_until_ms = NULL,
                next_due_ms = ?2, updated_ms = ?3
          WHERE id = ?1 AND running_host = ?4",
        (
            close.task_id,
            close.next_due_ms,
            close.finished_ms,
            close.host,
        ),
    )?;
    tx.commit()?;
    Ok(())
}

/// The newest `limit` runs of one task, most recent first.
///
/// Three states, and they are three because conflating any two of them tells a
/// reader something false. A NULL `outcome` is a run **still in flight**. A
/// stored outcome with a variant here is that outcome. A stored outcome from a
/// newer keeper is reported as `outcome: None` **with** `unknown_outcome` set,
/// which is [`list_tasks`]' skip-and-list discipline applied to history: dropping
/// the row instead — as this did until Story 57.2's review — silently removed the
/// *newest* run and made a surface report an older attempt as the current one.
pub fn task_runs(conn: &Connection, task_id: &str, limit: usize) -> Result<Vec<TaskRunRow>> {
    let mut stmt = conn.prepare(
        "SELECT id, task_id, started_ms, finished_ms, outcome, detail, host
         FROM task_runs WHERE task_id = ?1 ORDER BY id DESC LIMIT ?2",
    )?;
    // A negative `LIMIT` means "every row" to SQLite, which is what
    // `limit as i64` produced for a `usize` above `i64::MAX`.
    let limit = i64::try_from(limit).unwrap_or(i64::MAX);
    let rows = stmt.query_map((task_id, limit), |r| {
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, String>(1)?,
            r.get::<_, i64>(2)?,
            r.get::<_, Option<i64>>(3)?,
            r.get::<_, Option<String>>(4)?,
            r.get::<_, Option<String>>(5)?,
            r.get::<_, String>(6)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, task_id, started_ms, finished_ms, stored, detail, host) = row?;
        let mut outcome = None;
        let mut unknown_outcome = None;
        if let Some(stored) = stored {
            match TaskOutcome::from_stored(&stored) {
                Some(known) => outcome = Some(known),
                None => {
                    tracing::debug!(run = id, outcome = stored, "an unreadable task outcome");
                    unknown_outcome = Some(stored);
                }
            }
        }
        out.push(TaskRunRow {
            id,
            task_id,
            started_ms,
            finished_ms,
            outcome,
            unknown_outcome,
            detail,
            host,
        });
    }
    Ok(out)
}

/// Forget a task and its history together.
///
/// There is deliberately no foreign key (see the schema), so the history has to
/// be removed here — and in the same transaction, or a re-created task reusing
/// the id inherits a stranger's last result.
pub fn delete_task(conn: &Connection, id: &str) -> Result<()> {
    let tx = conn.unchecked_transaction()?;
    tx.execute("DELETE FROM task_runs WHERE task_id = ?1", [id])?;
    tx.execute("DELETE FROM tasks WHERE id = ?1", [id])?;
    tx.commit()?;
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

    /// AC 3 is a property of ONE statement, so it is asserted on that statement
    /// (Story 56.3).
    ///
    /// Twenty background downloads, then one a person asked for, then one claim
    /// of `CLAIM_LIMIT`-many rows. The requested unit must be first and the
    /// background units behind it must still be in id order — that second half
    /// is what stops "urgency" from quietly becoming "reshuffle the queue", and
    /// it is the property no clock and no sleeping is needed to see.
    #[test]
    fn a_requested_unit_is_claimed_ahead_of_background_work_without_reordering_it() {
        let c = conn();
        let background: Vec<i64> = (0..20)
            .map(|n| {
                enqueue(
                    &c,
                    "p",
                    &WorkKind::LfsDownload {
                        oid: format!("{n:064x}"),
                        size: 10,
                    },
                    1,
                    0,
                )
                .expect("queue background work")
            })
            .collect();
        let asked = enqueue(
            &c,
            "p",
            &WorkKind::LfsDownload {
                oid: format!("{:064x}", 999),
                size: 10,
            },
            1,
            0,
        )
        .expect("queue the requested unit");
        assert!(
            asked > background[19],
            "the requested unit is the YOUNGEST row, so id order alone would put \
             it last: that is the whole test"
        );
        promote_unit(&c, asked, 1).expect("promote");

        // 16 is `engine::CLAIM_LIMIT`, spelled here because this crate's db
        // layer must not depend on the engine's constant to state its own
        // property.
        let claimed = claim_ready(&c, "p", 1, 16).expect("claim");
        assert_eq!(claimed.len(), 16, "the batch is still bounded by the limit");
        assert_eq!(
            claimed[0].id, asked,
            "the unit somebody is waiting for is claimed first"
        );
        assert_eq!(claimed[0].urgency, Urgency::Requested);
        assert_eq!(
            claimed[1..].iter().map(|item| item.id).collect::<Vec<_>>(),
            background[..15].to_vec(),
            "and background work keeps its FIFO order among itself"
        );
        assert!(claimed[1..]
            .iter()
            .all(|item| item.urgency == Urgency::Background));

        // The rest are still there and claimable: a requested unit took a slot,
        // not the tick.
        let rest = claim_ready(&c, "p", 1, 16).expect("second claim");
        assert_eq!(
            rest.iter().map(|item| item.id).collect::<Vec<_>>(),
            background[15..].to_vec()
        );
    }

    /// A batch is never entirely requested work while background work waits.
    ///
    /// The starvation the first version of the urgency order shipped with, in
    /// the shape that matters: the `Push` carrying a local edit to the server is
    /// the oldest row in the queue, and the user then asks for more files than
    /// one batch holds. Under a pure `urgency DESC, id` order the push waits for
    /// sixteen transfers, then sixteen more, for as long as requests keep
    /// arriving — a folder that stops publishing local work while somebody
    /// browses media. One slot is kept for it, so it runs on the very next tick.
    #[test]
    fn a_burst_of_requests_cannot_starve_the_push_that_backs_up_local_work() {
        let c = conn();
        let push = enqueue(&c, "p", &WorkKind::Push, 1, 0).expect("queue the push first");
        for n in 0..20 {
            let id = enqueue(
                &c,
                "p",
                &WorkKind::LfsDownload {
                    oid: format!("{n:064x}"),
                    size: 10,
                },
                1,
                0,
            )
            .expect("queue a requested download");
            promote_unit(&c, id, 1).expect("promote");
        }

        let claimed = claim_ready(&c, "p", 1, 16).expect("claim");
        assert_eq!(claimed.len(), 16, "the batch is still full");
        assert_eq!(
            claimed.iter().filter(|i| i.id == push).count(),
            1,
            "the oldest background unit is in the batch although sixteen \
             requested units could have filled it: {:?}",
            claimed.iter().map(|i| i.id).collect::<Vec<_>>()
        );
        assert_eq!(
            claimed[..15]
                .iter()
                .filter(|i| i.urgency == Urgency::Requested)
                .count(),
            15,
            "and the other fifteen slots still serve the person waiting, ahead \
             of the push in execution order"
        );
    }

    /// A request may raise a queued row and nothing may lower one.
    ///
    /// `MAX` is the inversion of [`label_unit`]'s first-writer-wins, and the
    /// direction matters: a background scan that re-enqueued the covering row
    /// must not be able to demote a file a person is sitting and waiting for.
    #[test]
    fn urgency_can_be_raised_and_never_lowered() {
        let c = conn();
        let id = enqueue(&c, "p", &WorkKind::Pull, 1, 0).expect("enqueue");

        promote_unit(&c, id, 1).expect("promote");
        // The only writer of a lower level is a fresh row, so this is the shape
        // that could demote: the same UPDATE with `Background`'s level.
        c.execute(
            "UPDATE journal SET urgency = MAX(COALESCE(urgency, 0), ?2) WHERE id = ?1",
            (id, Urgency::Background.level()),
        )
        .expect("try to lower");
        assert_eq!(
            claim_ready(&c, "p", 1, 10).expect("claim")[0].urgency,
            Urgency::Requested,
            "a later background write must not demote a row somebody asked for"
        );
    }

    /// A promotion lifts a deferral and a backoff, or the request is a unit
    /// nothing will ever claim.
    ///
    /// `enqueue_unique` counts a `deferred` row as cover, and `claim_ready` only
    /// ever offers `pending`. Without this the CLI would print "queued as unit
    /// N" for a download whose removable remote was absent an hour ago and wait
    /// for a resume nobody is going to perform.
    #[test]
    fn promoting_a_deferred_or_backed_off_unit_makes_it_claimable_now() {
        let c = conn();
        let deferred = enqueue(&c, "p", &WorkKind::Pull, 1, 0).expect("enqueue");
        // Claimed and then deferred, which is the state an absent volume leaves
        // behind — and it is the shape that makes the attempt assertion below
        // mean something, because the claim is what spends an attempt.
        claim_ready(&c, "p", 1, 10).expect("first attempt");
        reschedule(
            &c,
            deferred,
            WorkState::Deferred,
            9_000,
            Some("media absent"),
        )
        .expect("defer it, as an absent volume does");
        assert!(
            claim_ready(&c, "p", 1_000, 10).expect("claim").is_empty(),
            "a deferred row is not claimable, which is the premise"
        );

        promote_unit(&c, deferred, 1_000).expect("promote");
        let claimed = claim_ready(&c, "p", 1_000, 10).expect("claim after the promotion");
        assert_eq!(claimed.len(), 1, "the person asking is a reason to try now");
        assert_eq!(claimed[0].id, deferred);
        assert_eq!(claimed[0].urgency, Urgency::Requested);
        assert_eq!(
            claimed[0].attempts, 2,
            "the promotion neither spends nor refunds an attempt, so a second \
             failure backs off exactly as far as it would have"
        );
    }

    /// The journal is what remembers a human asked, so a crash must not forget
    /// it.
    #[test]
    fn urgency_survives_a_restart() {
        let c = conn();
        let id = enqueue(
            &c,
            "p",
            &WorkKind::LfsDownload {
                oid: format!("{:064x}", 7),
                size: 10,
            },
            1,
            0,
        )
        .expect("enqueue");
        promote_unit(&c, id, 1).expect("promote");
        claim_ready(&c, "p", 1, 10).expect("claim it, then die");

        assert_eq!(recover_running(&c, 200).expect("recover"), 1);
        let again = claim_ready(&c, "p", 200, 10).expect("re-claim");
        assert_eq!(again.len(), 1);
        assert_eq!(
            again[0].urgency,
            Urgency::Requested,
            "the row is pending again and still requested"
        );
    }

    /// The urgency a completion arm acts on is read from the row, not from a
    /// snapshot taken when the row was claimed.
    ///
    /// The sequence that made this necessary: a background download is already
    /// `running` when the person asks for its path.
    /// `WorkKind::covered_while_running` makes that running row the covering
    /// unit, so `promote_unit` writes onto a row the supervisor read minutes
    /// ago. [`unit_urgency`] is what lets the completion arm see it.
    #[test]
    fn a_running_units_urgency_is_still_readable_after_it_was_claimed() {
        let c = conn();
        let id = enqueue(
            &c,
            "p",
            &WorkKind::LfsDownload {
                oid: format!("{:064x}", 11),
                size: 10,
            },
            1,
            0,
        )
        .expect("enqueue");
        let claimed = claim_ready(&c, "p", 1, 10).expect("claim");
        assert_eq!(claimed[0].urgency, Urgency::Background);
        assert_eq!(
            unit_urgency(&c, id).expect("read"),
            Urgency::Background,
            "nobody has asked yet"
        );

        // The request arrives mid-transfer and deduplicates onto the running
        // row, exactly as `enqueue_unique` promises.
        promote_unit(&c, id, 2).expect("promote the running row");
        assert_eq!(
            unit_urgency(&c, id).expect("read"),
            Urgency::Requested,
            "the row the transfer is running for now says somebody is waiting"
        );
        complete(&c, id).expect("complete");
        assert_eq!(
            unit_urgency(&c, id).expect("read a row that is gone"),
            Urgency::Background,
            "a deleted row falls back to the profile's own arrival policy"
        );
    }

    /// A `journal` planted at the pre-56.3 DDL upgrades in place, and its rows
    /// still claim.
    ///
    /// The failure mode this guards is a daemon that cannot start: `claim_ready`
    /// now names `urgency` in its projection, so an install whose table predates
    /// the column would raise `no such column` on every tick. `ALTER TABLE ...
    /// ADD COLUMN` guarded by the column list is its own idempotence, which is
    /// why this needs no `meta` marker — asserted rather than trusted, twice,
    /// because the second `migrate` is what every launch performs.
    #[test]
    fn a_journal_predating_the_urgency_column_upgrades_in_place() {
        let c = Connection::open_in_memory().expect("in-memory db");
        c.execute_batch(
            "CREATE TABLE journal (
                 id          INTEGER PRIMARY KEY AUTOINCREMENT,
                 profile_id  TEXT NOT NULL,
                 kind        TEXT NOT NULL,
                 payload     TEXT NOT NULL,
                 state       TEXT NOT NULL,
                 attempts    INTEGER NOT NULL DEFAULT 0,
                 not_before_ms INTEGER NOT NULL DEFAULT 0,
                 created_ms  INTEGER NOT NULL,
                 last_error  TEXT,
                 label       TEXT
             );
             INSERT INTO journal
                 (profile_id, kind, payload, state, not_before_ms, created_ms, label)
             VALUES ('p', 'pull', '{\"kind\":\"pull\"}', 'pending', 0, 1, 'old');",
        )
        .expect("plant the pre-56.3 schema");

        migrate(&c).expect("migrate an existing install");
        let claimed = claim_ready(&c, "p", 1, 10).expect("the old row still claims");
        assert_eq!(claimed.len(), 1);
        assert_eq!(claimed[0].label.as_deref(), Some("old"));
        assert_eq!(
            claimed[0].urgency,
            Urgency::Background,
            "NULL is the honest reading of a row queued before anybody could ask"
        );

        migrate(&c).expect("migrating twice changes nothing");
        recover_running(&c, 2).expect("put it back");
        assert_eq!(
            claim_ready(&c, "p", 2, 10)
                .expect("claim after a second migrate")
                .len(),
            1
        );
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
                local_origin: false,
                release_at_ms: None,
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
                local_origin: false,
                release_at_ms: None,
            }],
            "only the timestamp moved: an upsert that named more than `at_ms`, \
             or a REPLACE that named less, fails here"
        );
    }

    /// The pin is the one thing a release may not cross, so every way of not
    /// being pinned has to answer the same `false` (Story 56.4).
    #[test]
    fn is_pinned_reads_one_path_and_treats_absence_and_null_alike() {
        let c = conn();
        assert!(
            !is_pinned(&c, "p", "40-media/clip.mp4").expect("no row"),
            "a path this machine has never materialized is not pinned"
        );

        remember_materialized(&c, "p", "40-media/clip.mp4", 1_700).expect("landing");
        remember_materialized(&c, "p", "40-media/other.mp4", 1_700).expect("landing");
        assert!(
            !is_pinned(&c, "p", "40-media/clip.mp4").expect("null column"),
            "the one writer sets `at_ms` and nothing else, so `pinned` reads \
             back NULL — which means the default, not a third answer"
        );

        c.execute(
            "UPDATE materialized SET pinned = 1
              WHERE profile_id = 'p' AND path = '40-media/clip.mp4'",
            [],
        )
        .expect("pin it the way 56.5's writer will");
        assert!(is_pinned(&c, "p", "40-media/clip.mp4").expect("pinned"));
        assert!(
            !is_pinned(&c, "p", "40-media/other.mp4").expect("sibling"),
            "one path's pin is not its neighbour's"
        );
        assert!(
            !is_pinned(&c, "q", "40-media/clip.mp4").expect("other profile"),
            "and not another folder's, even at the same path"
        );
    }

    /// Each clock writer knows exactly one fact and cannot touch another
    /// (Story 56.5).
    ///
    /// The whole row is planted first, so a statement that named more columns
    /// than its own fact fails here rather than in a release six months later
    /// — which is the loss class `remember_materialized`'s doc records.
    #[test]
    fn each_clock_writer_moves_its_own_column_and_no_other() {
        let c = conn();
        remember_materialized(&c, "p", "40-media/clip.mp4", 1_700).expect("landing");
        set_pinned(&c, "p", "40-media/clip.mp4", true, 1_700).expect("pin");
        note_synced(&c, "p", "40-media/clip.mp4", 1_710).expect("confirm");
        note_use(&c, "p", "40-media/clip.mp4", 1_720).expect("use");

        let only = |c: &Connection| materialized_rows(c, "p").expect("read").remove(0);
        assert_eq!(
            only(&c),
            MaterializedRow {
                path: "40-media/clip.mp4".to_owned(),
                at_ms: 1_700,
                last_used_ms: Some(1_720),
                synced_at_ms: Some(1_710),
                oid: None,
                size_bytes: None,
                pinned: true,
                local_origin: false,
                release_at_ms: None,
            },
            "four writers, four facts, none of them disturbing another"
        );

        note_use(&c, "p", "40-media/clip.mp4", 9_000).expect("a later use");
        let row = only(&c);
        assert_eq!(row.last_used_ms, Some(9_000));
        assert!(row.pinned, "a use does not withdraw the owner's pin");
        assert_eq!(
            row.synced_at_ms,
            Some(1_710),
            "nor does it forge a remote confirmation"
        );
        assert!(
            !row.local_origin,
            "and reading a file is not authoring it — the whole point of AD-131's \
             two clocks is that a use cannot make an unconfirmed local path eligible"
        );
    }

    /// An arrival records one fact whole: the content came from upstream, it is
    /// here now, and it is this object (Story 56.5).
    #[test]
    fn an_arrival_starts_the_remote_origin_clock_and_says_where_it_came_from() {
        let c = conn();
        remember_materialized(&c, "p", "clip.mp4", 1_700).expect("landing");
        set_pinned(&c, "p", "clip.mp4", true, 1_700).expect("pin");
        note_synced(&c, "p", "clip.mp4", 1_650).expect("the remote held the old bytes");
        note_local_authorship(&c, "p", "clip.mp4", 1_700, "aaa111", 64)
            .expect("this clone wrote it once");
        assert!(materialized_rows(&c, "p").expect("read")[0].local_origin);

        // A later pull replaces the content with the remote's version, so the
        // provenance moves back with it — and so does the identity, because the
        // object at this path is not the one this clone wrote any more.
        note_arrival(&c, "p", "clip.mp4", 2_000, "bbb222", 4_194_304).expect("arrival");
        let row = materialized_rows(&c, "p").expect("read").remove(0);
        assert_eq!(row.last_used_ms, Some(2_000));
        assert!(
            !row.local_origin,
            "the bytes at this path came from upstream now, whoever wrote the last ones"
        );
        assert_eq!(
            row.oid.as_deref(),
            Some("bbb222"),
            "an arrival is one of the two moments the committed pointer is in hand, \
             so it is one of the two places the identity can honestly be recorded"
        );
        assert_eq!(
            row.size_bytes,
            Some(4_194_304),
            "and the length travels with it — the sweep's byte ceiling reads this \
             column, and bounded nothing at all while it had no writer"
        );
        assert_eq!(
            row.at_ms, 1_700,
            "and the arrival memo does not move the landing timestamp — \
             `remember_materialized` owns that one"
        );
        assert!(row.pinned, "nor does it withdraw the owner's pin");
        assert_eq!(
            row.synced_at_ms, None,
            "nor forge a remote confirmation: the authorship cleared it, and an \
             arrival knows nothing about NFR-40's per-object proof"
        );
    }

    /// A local modification invalidates any prior confirmation, because the
    /// new bytes are not the ones the remote agreed to (Story 56.5, FR-341).
    ///
    /// This is the data-loss case the second column exists for: a path
    /// confirmed upstream last week and re-edited this morning would otherwise
    /// be released a TTL after the OLD confirmation, discarding content that
    /// exists nowhere else.
    #[test]
    fn authoring_over_confirmed_content_clears_the_confirmation() {
        let c = conn();
        remember_materialized(&c, "p", "clip.mp4", 1_700).expect("landing");
        note_arrival(&c, "p", "clip.mp4", 1_700, "aaa111", 64).expect("arrival");
        note_synced(&c, "p", "clip.mp4", 1_800).expect("the remote holds it");
        set_pinned(&c, "p", "clip.mp4", true, 1_800).expect("pin");

        note_local_authorship(&c, "p", "clip.mp4", 2_000, "bbb222", 4_096)
            .expect("the owner edited it");

        let row = materialized_rows(&c, "p").expect("read").remove(0);
        assert!(row.local_origin, "these bytes are this clone's");
        assert_eq!(
            row.synced_at_ms, None,
            "and the remote has never seen them, whatever it confirmed about the old ones"
        );
        assert_eq!(
            row.oid.as_deref(),
            Some("bbb222"),
            "the conflict path records the object this clone wrote, not the one \
             that arrived"
        );
        assert_eq!(row.size_bytes, Some(4_096), "and its length with it");
        assert!(row.pinned, "the owner's standing instruction is untouched");
        assert_eq!(
            row.last_used_ms,
            Some(1_700),
            "and so is the use clock, which this writer knows nothing about"
        );
    }

    /// A local authorship or a pin may be the first thing this ledger hears
    /// about a path, so both insert; the three clock writers and a *withdrawn*
    /// pin may not (Story 56.5).
    ///
    /// `at_ms` is `NOT NULL` and means *content landed here*, which a use, a
    /// confirmation or an `unpin` has no way to know — so a fact about a path
    /// with no row is a fact this table cannot hold, and the conservative
    /// outcome is no candidate rather than a fabricated one.
    #[test]
    fn only_the_upsert_writers_may_create_a_row() {
        let c = conn();
        note_use(&c, "p", "ghost.mp4", 1_700).expect("a use of a path with no row");
        note_arrival(&c, "p", "ghost.mp4", 1_700, "aaa111", 64).expect("an arrival with no row");
        note_synced(&c, "p", "ghost.mp4", 1_700).expect("a confirmation with no row");
        // The phantom-candidate guard. `keeper-syncd unpin` on a path this
        // ledger has never heard of used to INSERT a row asserting content
        // landed here now, and `Engine::release_due_at` reads exactly that row
        // as a candidate a TTL later — forever, one budget slot a pass, for a
        // release that can only ever refuse `AlreadyPointer`.
        set_pinned(&c, "p", "ghost.mp4", false, 1_700).expect("an unpin with no row");
        assert!(
            materialized_rows(&c, "p").expect("read").is_empty(),
            "none of the four UPDATE-only writers invented a row, and none errored"
        );

        // The owner creating a file here is the case with no arrival to have
        // written a row, and the pin is a standing instruction about a path
        // whose content may not be here yet. Both insert.
        note_local_authorship(&c, "p", "written-here.mp4", 2_000, "bbb222", 4_096)
            .expect("authorship");
        set_pinned(&c, "p", "not-here-yet.mp4", true, 2_100).expect("pre-pin");
        let rows = materialized_rows(&c, "p").expect("read");
        assert_eq!(
            rows.iter().map(|row| row.path.as_str()).collect::<Vec<_>>(),
            vec!["not-here-yet.mp4", "written-here.mp4"]
        );
        assert_eq!(rows[1].at_ms, 2_000);
        assert!(rows[1].local_origin);
        assert_eq!(rows[1].oid.as_deref(), Some("bbb222"));
        assert_eq!(rows[1].size_bytes, Some(4_096));
        assert!(rows[0].pinned);
        assert!(
            !rows[0].local_origin,
            "pinning a path says nothing about who wrote it"
        );
        assert_eq!(
            rows[0].oid, None,
            "nor about which object is there: a pin is an instruction, not an \
             observation about content"
        );
    }

    /// Unpinning gives a path back to the sweep and takes nothing else with it
    /// (Story 56.5, FR-334).
    #[test]
    fn withdrawing_a_pin_makes_the_path_a_candidate_again() {
        let c = conn();
        remember_materialized(&c, "p", "clip.mp4", 1_700).expect("landing");
        note_arrival(&c, "p", "clip.mp4", 1_700, "aaa111", 4_096).expect("arrival");
        note_synced(&c, "p", "clip.mp4", 1_750).expect("the remote holds it");
        set_pinned(&c, "p", "clip.mp4", true, 1_800).expect("pin");
        assert!(is_pinned(&c, "p", "clip.mp4").expect("pinned"));

        set_pinned(&c, "p", "clip.mp4", false, 1_900).expect("unpin");
        assert_eq!(
            materialized_rows(&c, "p").expect("read").remove(0),
            MaterializedRow {
                path: "clip.mp4".to_owned(),
                at_ms: 1_700,
                last_used_ms: Some(1_700),
                synced_at_ms: Some(1_750),
                oid: Some("aaa111".to_owned()),
                size_bytes: Some(4_096),
                pinned: false,
                local_origin: false,
                release_at_ms: None,
            },
            "the floor is gone and nothing else moved: both clocks, the \
             provenance, the identity and the landing instant are exactly where \
             they were, so the path is due whenever it would have been due had it \
             never been pinned"
        );

        // A pin is idempotent, which is what lets the CLI verb be re-run.
        set_pinned(&c, "p", "clip.mp4", true, 2_000).expect("pin");
        set_pinned(&c, "p", "clip.mp4", true, 2_100).expect("pin again");
        assert!(is_pinned(&c, "p", "clip.mp4").expect("still pinned"));
        assert_eq!(
            materialized_rows(&c, "p").expect("read").remove(0).at_ms,
            1_700,
            "and re-pinning a row that exists does not restamp the landing instant"
        );
    }

    /// A chosen release time is recorded as one column, withdrawn as one
    /// column, and disturbs nothing else (Story 56.17).
    #[test]
    fn set_release_at_writes_one_fact_and_withdrawing_it_writes_one_fact() {
        let c = conn();
        remember_materialized(&c, "p", "clip.mp4", 1_700).expect("landing");
        note_arrival(&c, "p", "clip.mp4", 1_700, "aaa111", 4_096).expect("arrival");
        note_synced(&c, "p", "clip.mp4", 1_750).expect("the remote holds it");
        set_pinned(&c, "p", "clip.mp4", true, 1_800).expect("pin");

        set_release_at(&c, "p", "clip.mp4", Some(9_000), 1_900).expect("keep it two hours");
        assert_eq!(
            materialized_rows(&c, "p").expect("read").remove(0),
            MaterializedRow {
                path: "clip.mp4".to_owned(),
                at_ms: 1_700,
                last_used_ms: Some(1_700),
                synced_at_ms: Some(1_750),
                oid: Some("aaa111".to_owned()),
                size_bytes: Some(4_096),
                pinned: true,
                local_origin: false,
                release_at_ms: Some(9_000),
            },
            "one column: neither clock, the provenance, the identity, the pin \
             nor the landing instant may move for an instruction about when \
             the content may go"
        );

        // A later instruction replaces an earlier one rather than joining it.
        set_release_at(&c, "p", "clip.mp4", Some(20_000), 2_000).expect("make it eight hours");
        assert_eq!(
            materialized_rows(&c, "p")
                .expect("read")
                .remove(0)
                .release_at_ms,
            Some(20_000)
        );

        set_release_at(&c, "p", "clip.mp4", None, 2_100).expect("indefinitely, after all");
        let row = materialized_rows(&c, "p").expect("read").remove(0);
        assert_eq!(row.release_at_ms, None, "withdrawn");
        assert!(row.pinned, "and the pin is still the pin");
        assert_eq!(row.at_ms, 1_700, "and the landing instant never moved");
    }

    /// Naming a time for content that is not here yet writes a row **no
    /// present-tense reader can see** (Story 56.17).
    ///
    /// The queued case is the whole reason the instruction is recorded when it
    /// is asked for rather than when the bytes land, and it is also where a
    /// naive upsert does real damage: a row with `released_at_ms` `NULL` claims
    /// this clone holds the content. `Engine::pending` reads that as the
    /// `replacing` flag, the sweep reads it as a candidate that can only refuse
    /// `AlreadyPointer`, and the listing reads it as a materialized path. So
    /// the insert says what is true — the content is not here — and the landing
    /// writers clear it.
    #[test]
    fn a_deadline_for_content_that_has_not_landed_is_invisible_until_it_does() {
        let c = conn();
        set_release_at(&c, "p", "coming.mp4", Some(9_000), 1_000).expect("keep it two hours");

        assert!(
            materialized_paths(&c, "p").expect("read").is_empty(),
            "nothing here claims this machine holds the content"
        );
        assert!(
            materialized_rows(&c, "p").expect("read").is_empty(),
            "and the sweep has no candidate to spend a budget slot refusing"
        );

        // The download lands, through the writer the arrival path uses.
        remember_materialized(&c, "p", "coming.mp4", 5_000).expect("landing");
        let row = materialized_rows(&c, "p").expect("read").remove(0);
        assert_eq!(
            row.at_ms, 5_000,
            "and the landing instant is the truth, not the instant the \
             instruction was given"
        );
        assert_eq!(
            row.release_at_ms,
            Some(9_000),
            "the deadline was waiting for the bytes, which is the point of \
             recording it when the person asked"
        );
    }

    /// Releasing content spends the instruction that was waiting for it
    /// (Story 56.17).
    ///
    /// Left standing it would be a deadline in the past, so the same path
    /// materialized again with no duration — `remember_materialized` clears
    /// `released_at_ms`, so the row comes back live — would be eligible on the
    /// very next sweep, hours before the folder's own window says so.
    #[test]
    fn releasing_a_path_withdraws_the_deadline_that_asked_for_it() {
        let c = conn();
        remember_materialized(&c, "p", "clip.mp4", 1_000).expect("landing");
        set_release_at(&c, "p", "clip.mp4", Some(2_000), 1_000).expect("keep it an hour");

        forget_materialized(&c, "p", "clip.mp4", 2_500).expect("released");
        assert!(materialized_rows(&c, "p").expect("read").is_empty());

        remember_materialized(&c, "p", "clip.mp4", 9_000).expect("asked for again, indefinitely");
        let row = materialized_rows(&c, "p").expect("read").remove(0);
        assert_eq!(
            row.release_at_ms, None,
            "the served instruction is gone, so this path is on the folder's \
             own window and not on a deadline that expired hours ago"
        );
        assert_eq!(row.at_ms, 9_000);
    }

    /// A length SQLite's signed integer cannot hold saturates, and never wraps
    /// (Story 56.5).
    ///
    /// This column is the release sweep's byte ceiling, so the direction of the
    /// degradation is a safety property rather than a rounding choice. An `as`
    /// cast would turn a `u64` above `i64::MAX` negative, `materialized_rows`
    /// narrows a negative to `None`, and `None` is the one answer that makes a
    /// candidate contribute *nothing* to the budget meant to bound it — so the
    /// largest object in the folder would be the one the ceiling could not see.
    /// Saturating reads back as enormous instead, which stops the pass at that
    /// candidate after giving it its one attempt.
    #[test]
    fn a_length_larger_than_the_column_can_hold_saturates_rather_than_wrapping() {
        let c = conn();
        remember_materialized(&c, "p", "arrived.mp4", 1_700).expect("landing");
        note_arrival(&c, "p", "arrived.mp4", 1_700, "aaa111", u64::MAX).expect("arrival");
        note_local_authorship(&c, "p", "written-here.mp4", 1_800, "bbb222", u64::MAX)
            .expect("authorship");

        for row in materialized_rows(&c, "p").expect("read") {
            assert_eq!(
                row.size_bytes,
                Some(i64::MAX as u64),
                "{} saturated at what the column can say rather than wrapping \
                 negative into `None`",
                row.path
            );
        }

        let smallest: i64 = c
            .query_row(
                "SELECT MIN(size_bytes) FROM materialized WHERE profile_id = 'p'",
                [],
                |r| r.get(0),
            )
            .expect("read the raw column");
        assert!(
            smallest > 0,
            "nothing negative reached the column: {smallest}"
        );
    }

    /// One folder's facts are not another's, at the same path (Story 56.5).
    #[test]
    fn every_clock_writer_is_scoped_to_one_profile() {
        let c = conn();
        remember_materialized(&c, "p", "clip.mp4", 1_700).expect("landing");
        remember_materialized(&c, "q", "clip.mp4", 1_700).expect("landing");

        note_use(&c, "p", "clip.mp4", 5_000).expect("use");
        note_synced(&c, "p", "clip.mp4", 5_100).expect("confirm");
        set_pinned(&c, "p", "clip.mp4", true, 5_200).expect("pin");
        note_local_authorship(&c, "p", "clip.mp4", 5_300, "aaa111", 64).expect("authorship");

        let other = materialized_rows(&c, "q").expect("read").remove(0);
        assert_eq!(
            other,
            MaterializedRow {
                path: "clip.mp4".to_owned(),
                at_ms: 1_700,
                last_used_ms: None,
                synced_at_ms: None,
                oid: None,
                size_bytes: None,
                pinned: false,
                local_origin: false,
                release_at_ms: None,
            },
            "the neighbour folder's row at the same path is untouched"
        );
    }

    /// Forgetting one path leaves every other fact in the ledger alone — and
    /// the owner's pin is one of those facts.
    #[test]
    fn forget_materialized_removes_exactly_one_row() {
        let c = conn();
        remember_materialized(&c, "p", "a.mp4", 1_700).expect("landing");
        remember_materialized(&c, "p", "b.mp4", 1_800).expect("landing");
        remember_materialized(&c, "q", "a.mp4", 1_900).expect("landing");

        forget_materialized(&c, "p", "a.mp4", 9_000).expect("forget");
        assert_eq!(
            materialized_rows(&c, "p")
                .expect("read")
                .into_iter()
                .map(|row| row.path)
                .collect::<Vec<_>>(),
            vec!["b.mp4".to_owned()],
            "the sibling in the same folder survives"
        );
        assert_eq!(
            materialized_rows(&c, "q").expect("read").len(),
            1,
            "and so does the same path in another folder"
        );

        forget_materialized(&c, "p", "a.mp4", 9_100)
            .expect("forgetting an absent row must succeed, as deleting an absent secret does");
        forget_materialized(&c, "p", "never-here.mp4", 9_200).expect("nor is a path it never knew");

        // And a pinned row is not one it can take, whatever a caller asks. The
        // refusal that keeps a pinned path away from here lives in the engine;
        // this is the statement being incapable of the loss on its own.
        remember_materialized(&c, "p", "kept.mp4", 2_000).expect("landing");
        c.execute(
            "UPDATE materialized SET pinned = 1 WHERE profile_id = 'p' AND path = 'kept.mp4'",
            [],
        )
        .expect("pin it the way 56.5's writer will");
        forget_materialized(&c, "p", "kept.mp4", 9_300).expect("the statement succeeds");
        assert!(
            is_pinned(&c, "p", "kept.mp4").expect("still pinned"),
            "the row — and with it the owner's pin — survived the retraction"
        );
        assert!(
            materialized_paths(&c, "p")
                .expect("read")
                .contains("kept.mp4"),
            "and a row the statement declined to retract still reads as held"
        );
    }

    /// Story 56.14. A release used to `DELETE` the row, which discarded
    /// `last_used_ms`, `synced_at_ms` and `local_origin` at the exact moment
    /// `ensure_materialized_columns` says they exist to still answer. The row
    /// now stays and carries a `released_at_ms` stamp instead: absent from every
    /// present-tense reader, and still holding the history a re-materialization
    /// wants.
    ///
    /// Without the change the two `materialized_rows(..)` reads below return an
    /// empty vector, and the raw-column assertion cannot even find a row.
    #[test]
    fn a_released_row_keeps_its_clocks_and_leaves_every_present_tense_reader() {
        let c = conn();
        remember_materialized(&c, "p", "clip.mp4", 1_700).expect("landing");
        note_arrival(&c, "p", "clip.mp4", 5_000, "aaa111", 4_194_304).expect("arrival");
        note_synced(&c, "p", "clip.mp4", 5_100).expect("confirm");

        forget_materialized(&c, "p", "clip.mp4", 9_000).expect("release");

        assert!(
            !materialized_paths(&c, "p")
                .expect("read")
                .contains("clip.mp4"),
            "a released path must not read as content this machine holds: it is \
             `Engine::pending`'s `replacing` flag"
        );
        assert!(
            materialized_rows(&c, "p").expect("read").is_empty(),
            "nor may it reach the release sweep's candidate list or a listing"
        );

        let (last_used, synced, origin, released): (
            Option<i64>,
            Option<i64>,
            Option<i64>,
            Option<i64>,
        ) = c
            .query_row(
                "SELECT last_used_ms, synced_at_ms, local_origin, released_at_ms
                   FROM materialized WHERE profile_id = 'p' AND path = 'clip.mp4'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .expect("the row is still there");
        assert_eq!(
            (last_used, synced, origin, released),
            (Some(5_000), Some(5_100), Some(0), Some(9_000)),
            "every clock the sweep reasons with survived the release"
        );

        // And landing content again un-retires the row without inventing a new
        // landing instant for the history it kept.
        remember_materialized(&c, "p", "clip.mp4", 12_000).expect("re-landing");
        let row = materialized_rows(&c, "p").expect("read").remove(0);
        assert_eq!(
            (row.at_ms, row.last_used_ms, row.synced_at_ms),
            (12_000, Some(5_000), Some(5_100)),
            "the row is present-tense again and its recency history is intact"
        );
    }

    /// Story 56.14. `materialize_held`'s already-held arm wrote nothing at all,
    /// so a path a human explicitly asked for had no row and could not be a
    /// release candidate (FR-334). `observe_materialized` is that writer, and it
    /// must not forge a landing instant for a row that already records one.
    ///
    /// Without the function the first assertion has no row to read.
    #[test]
    fn observing_content_records_the_use_and_never_moves_the_landing_clock() {
        let c = conn();

        observe_materialized(&c, "p", "found.mp4", 4_000, "aaa111", 4_096).expect("first sighting");
        let row = materialized_rows(&c, "p").expect("read").remove(0);
        assert_eq!(
            (row.at_ms, row.last_used_ms, row.local_origin, row.pinned),
            (4_000, Some(4_000), false, false),
            "a path nobody had a row for gets one, dated the earliest instant \
             this clone can prove the content was here"
        );

        remember_materialized(&c, "p", "landed.mp4", 1_000).expect("landing");
        observe_materialized(&c, "p", "landed.mp4", 7_000, "bbb222", 8_192)
            .expect("second sighting");
        let row = materialized_rows(&c, "p")
            .expect("read")
            .into_iter()
            .find(|row| row.path == "landed.mp4")
            .expect("the landed row");
        assert_eq!(
            (row.at_ms, row.last_used_ms),
            (1_000, Some(7_000)),
            "an existing row's landing clock is left alone; only the use is new"
        );

        // A sighting of a released path brings it back into the present tense,
        // because looking at it is how we found out the content is here.
        forget_materialized(&c, "p", "landed.mp4", 8_000).expect("release");
        observe_materialized(&c, "p", "landed.mp4", 9_000, "bbb222", 8_192)
            .expect("third sighting");
        assert!(
            materialized_paths(&c, "p")
                .expect("read")
                .contains("landed.mp4"),
            "a sighting un-retires the row it found"
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

    // -----------------------------------------------------------------------
    // Tasks (Story 57.1)
    // -----------------------------------------------------------------------

    /// A profile-scoped `Sync` task, armed by nobody yet.
    fn task(id: &str, schedule: Option<&str>, mode: TaskMode) -> TaskRow {
        TaskRow {
            id: id.to_owned(),
            profile_id: Some("p".to_owned()),
            kind: TaskKind::Sync,
            schedule: schedule.map(str::to_owned),
            mode,
            next_due_ms: None,
            enabled: true,
            updated_ms: 1,
            running_host: None,
            lease_until_ms: None,
        }
    }

    /// A row written the only way a row this build cannot read ever arrives:
    /// past the typed write door, by a newer keeper or by hand.
    fn raw_task(c: &Connection, id: &str, kind: &str, mode: &str, schedule: Option<&str>) {
        c.execute(
            "INSERT INTO tasks (id, profile_id, kind, schedule, mode, next_due_ms, enabled, updated_ms)
             VALUES (?1, NULL, ?2, ?3, ?4, NULL, 1, 1)",
            (id, kind, schedule, mode),
        )
        .expect("insert a raw task row");
    }

    fn columns_of(c: &Connection, table: &str) -> Vec<String> {
        c.prepare(&format!("PRAGMA table_info({table})"))
            .expect("pragma")
            .query_map([], |r| r.get::<_, String>(1))
            .expect("query")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("columns")
    }

    fn meta_keys(c: &Connection) -> Vec<String> {
        c.prepare("SELECT key FROM meta ORDER BY key")
            .expect("prepare")
            .query_map([], |r| r.get::<_, String>(0))
            .expect("query")
            .collect::<rusqlite::Result<Vec<_>>>()
            .expect("keys")
    }

    #[test]
    fn a_pre_57_database_gains_the_task_tables_and_no_new_meta_key() {
        // The pre-57 schema by hand, including the one-shot marker such a
        // binary had already applied — otherwise `ensure_prune_default` writes
        // its own key and the meta comparison below would be testing that.
        let c = Connection::open_in_memory().expect("bare db");
        c.execute_batch(
            r#"
            CREATE TABLE profiles (
                id          TEXT PRIMARY KEY,
                json        TEXT NOT NULL,
                state       TEXT NOT NULL DEFAULT 'idle',
                last_error  TEXT,
                updated_ms  INTEGER NOT NULL DEFAULT 0
            );
            CREATE TABLE journal (
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
            CREATE TABLE meta (
                key         TEXT PRIMARY KEY,
                value       TEXT NOT NULL
            );
            INSERT INTO meta (key, value)
                 VALUES ('lfs_prune_local_default_on', '0');
            "#,
        )
        .expect("pre-57 schema");
        let meta_before = meta_keys(&c);

        migrate(&c).expect("an old database migrates in place");
        assert_eq!(
            columns_of(&c, "tasks"),
            vec![
                "id",
                "profile_id",
                "kind",
                "schedule",
                "mode",
                "next_due_ms",
                "enabled",
                "updated_ms",
                "running_host",
                "lease_until_ms",
            ]
        );
        assert_eq!(
            columns_of(&c, "task_runs"),
            vec![
                "id",
                "task_id",
                "started_ms",
                "finished_ms",
                "outcome",
                "detail",
                "host",
            ]
        );
        let tasks_after_first = columns_of(&c, "tasks");

        migrate(&c).expect("second migrate");
        migrate(&c).expect("third migrate");
        assert_eq!(columns_of(&c, "tasks"), tasks_after_first);
        assert_eq!(
            meta_keys(&c),
            meta_before,
            "a table addition is schema, not content, and writes no marker"
        );
    }

    #[test]
    fn a_task_round_trips_through_the_write_door_including_the_host_wide_case() {
        let c = conn();
        let mut cron = task("01A", Some("0 3 * * *"), TaskMode::Scheduled);
        // Offered, and deliberately ignored: the window is runtime state owned
        // by `arm_task`/`finish_task_run`, so a caller's copy of it — which is
        // what a read-modify-write from a form carries — must never reach the
        // row and rewind the schedule.
        cron.next_due_ms = Some(9_000);
        upsert_task(&c, &cron).expect("save");
        let stored = TaskRow {
            next_due_ms: None,
            ..cron.clone()
        };
        let host_wide = TaskRow {
            profile_id: None,
            ..task("01B", None, TaskMode::Manual)
        };
        upsert_task(&c, &host_wide).expect("save the host-wide task");

        assert_eq!(
            get_task(&c, "01A").expect("get"),
            Some(stored.clone()),
            "everything the caller owns round-trips, and the window it does not own is unarmed"
        );
        assert_eq!(
            get_task(&c, "01B").expect("get"),
            Some(host_wide.clone()),
            "a host-wide task carries no profile and must survive the round trip as one"
        );
        let listing = list_tasks(&c).expect("list");
        assert_eq!(listing.tasks, vec![stored, host_wide]);
        assert!(listing.unknown.is_empty());
        assert_eq!(
            cron.state(),
            TaskState {
                enabled: true,
                mode: TaskMode::Scheduled,
                next_due_ms: Some(9_000),
                lease_until_ms: None,
            },
            "the row projects into exactly the four facts the pure gate reads"
        );
    }

    #[test]
    fn an_upsert_over_a_running_task_does_not_free_its_lease() {
        let c = conn();
        upsert_task(&c, &task("01U", Some("every 5m"), TaskMode::Scheduled)).expect("save");
        claim_task(&c, "01U", "hostA", 0, 60_000, None)
            .expect("claim")
            .expect("the first claim wins");

        let mut edited = task("01U", Some("every 10m"), TaskMode::Scheduled);
        edited.updated_ms = 42;
        upsert_task(&c, &edited).expect("save again while the run is in flight");

        let row = get_task(&c, "01U").expect("get").expect("row");
        assert_eq!(row.schedule.as_deref(), Some("every 10m"));
        assert_eq!(row.updated_ms, 42);
        assert_eq!(
            row.running_host.as_deref(),
            Some("hostA"),
            "a save must not silently free a lease somebody is running under"
        );
        assert_eq!(row.lease_until_ms, Some(60_000));
    }

    #[test]
    fn a_malformed_schedule_is_refused_by_the_write_door_and_nothing_is_stored() {
        let c = conn();
        let bad = task("01M", Some("0 3 * *"), TaskMode::Scheduled);
        assert!(
            matches!(upsert_task(&c, &bad), Err(SyncError::Config(_))),
            "refusal, never coercion, and it happens where the row is written"
        );
        assert!(
            get_task(&c, "01M").expect("get").is_none(),
            "a refused schedule leaves nothing behind: a row that got written anyway would run"
        );
    }

    #[test]
    fn a_scheduled_task_with_no_schedule_is_refused() {
        let c = conn();
        let nonsense = task("01N", None, TaskMode::Scheduled);
        assert!(matches!(
            upsert_task(&c, &nonsense),
            Err(SyncError::Config(_))
        ));
        assert!(get_task(&c, "01N").expect("get").is_none());
    }

    #[test]
    fn an_empty_task_id_is_refused() {
        let c = conn();
        assert!(matches!(
            upsert_task(&c, &task("", Some("every 5m"), TaskMode::Scheduled)),
            Err(SyncError::Config(_))
        ));
    }

    #[test]
    fn a_task_kind_this_build_cannot_read_is_skipped_and_listed_not_fatal() {
        // A newer keeper's task must not brick an older one's list.
        let c = conn();
        upsert_task(&c, &task("01A", Some("every 5m"), TaskMode::Scheduled)).expect("save");
        raw_task(&c, "01T", "teleport", "scheduled", Some("every 5m"));
        // `update` is the one refusal the spec calls structural: `TaskKind` has
        // no such variant, so raw SQL is the only way to write one — and it
        // gets exactly the treatment a fictional kind gets.
        raw_task(&c, "01X", "update", "scheduled", Some("0 3 * * *"));

        let listing = list_tasks(&c).expect("list");
        assert_eq!(
            listing
                .tasks
                .iter()
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>(),
            vec!["01A"]
        );
        assert_eq!(
            listing
                .unknown
                .iter()
                .map(|u| u.id.as_str())
                .collect::<Vec<_>>(),
            vec!["01T", "01X"]
        );
        assert!(
            get_task(&c, "01X").expect("get").is_none(),
            "asking for one row by id answers 'nothing here I can act on', never a poisoned engine"
        );
    }

    #[test]
    fn an_unknown_mode_or_an_unparseable_schedule_is_skipped_and_listed_too() {
        let c = conn();
        raw_task(&c, "01M", "sync", "whenever", None);
        raw_task(&c, "01S", "sync", "scheduled", Some("every 5x"));

        let listing = list_tasks(&c).expect("list");
        assert!(
            listing.tasks.is_empty(),
            "neither row is one this build could honestly run"
        );
        assert_eq!(
            listing
                .unknown
                .iter()
                .map(|u| u.id.as_str())
                .collect::<Vec<_>>(),
            vec!["01M", "01S"]
        );
        assert!(
            listing.unknown.iter().all(|u| !u.reason.is_empty()),
            "the reason is what a caller shows in place of the task"
        );
    }

    #[test]
    fn the_lease_admits_exactly_one_of_two_connections_racing_over_one_file() {
        let dir = tempfile::tempdir().expect("tempdir");
        let created = open(dir.path()).expect("open");
        upsert_task(
            &created,
            &task("01R", Some("every 5m"), TaskMode::Scheduled),
        )
        .expect("save");
        drop(created);

        let path = db_path(dir.path());
        let gate = std::sync::Arc::new(std::sync::Barrier::new(2));
        let mut racers = Vec::new();
        for host in ["hostA", "hostB"] {
            let path = path.clone();
            let gate = std::sync::Arc::clone(&gate);
            racers.push(std::thread::spawn(move || {
                let c = Connection::open(&path).expect("second connection");
                // Both `UPDATE`s must genuinely execute against the row: the
                // WHERE clause, not the lock, is what has to exclude the loser.
                c.busy_timeout(std::time::Duration::from_secs(5))
                    .expect("busy timeout");
                gate.wait();
                claim_task(&c, "01R", host, 1_000, 60_000, None).expect("claim")
            }));
        }
        let claims: Vec<Option<i64>> = racers
            .into_iter()
            .map(|r| r.join().expect("racer"))
            .collect();

        assert_eq!(
            claims.iter().filter(|c| c.is_some()).count(),
            1,
            "the affected-row count is the arbiter, so exactly one host may hold the lease"
        );
        let reader = Connection::open(&path).expect("reader");
        let runs: i64 = reader
            .query_row(
                "SELECT COUNT(*) FROM task_runs WHERE task_id = '01R'",
                [],
                |r| r.get(0),
            )
            .expect("count");
        assert_eq!(runs, 1, "the losing claim must not open a run either");
    }

    #[test]
    fn a_dead_holders_lease_is_reclaimable_at_the_expiry_instant_and_not_before() {
        let c = conn();
        upsert_task(&c, &task("01L", Some("every 5m"), TaskMode::Scheduled)).expect("save");
        let abandoned = claim_task(&c, "01L", "hostA", 0, 1_000, None)
            .expect("claim")
            .expect("hostA claims a free task");

        // The expiry instant is a boundary, so it is asserted exactly.
        assert_eq!(
            claim_task(&c, "01L", "hostB", 999, 1_000, None).expect("claim"),
            None,
            "one millisecond before it expires the lease is still hostA's"
        );
        let taken = claim_task(&c, "01L", "hostB", 1_000, 1_000, None)
            .expect("claim")
            .expect("at the instant it expires the lease is reclaimable");

        let runs = task_runs(&c, "01L", 10).expect("runs");
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].id, taken);
        assert_eq!(runs[0].outcome, None, "the new run is still in flight");
        assert_eq!(runs[0].finished_ms, None);
        assert_eq!(runs[1].id, abandoned);
        assert_eq!(
            runs[1].outcome,
            Some(TaskOutcome::Abandoned),
            "a killed host's run is closed with the truth, not left open forever"
        );
        assert_eq!(runs[1].finished_ms, Some(1_000));
        let row = get_task(&c, "01L").expect("get").expect("row");
        assert_eq!(row.running_host.as_deref(), Some("hostB"));
        assert_eq!(row.lease_until_ms, Some(2_000));
    }

    #[test]
    fn a_disabled_task_cannot_be_claimed_at_all() {
        let c = conn();
        let mut off = task("01D", Some("every 5m"), TaskMode::Scheduled);
        off.enabled = false;
        upsert_task(&c, &off).expect("save");

        assert_eq!(
            claim_task(&c, "01D", "hostA", 0, 60_000, None).expect("claim"),
            None,
            "`enabled` decides whether the row is live, and a dead row runs for nobody"
        );
        assert!(
            task_runs(&c, "01D", 10).expect("runs").is_empty(),
            "a refused claim must not open a run"
        );
    }

    #[test]
    fn finishing_a_run_releases_the_lease_and_records_the_outcome_and_the_next_window() {
        let c = conn();
        upsert_task(&c, &task("01F", Some("every 5m"), TaskMode::Scheduled)).expect("save");
        let run = claim_task(&c, "01F", "hostA", 0, 60_000, None)
            .expect("claim")
            .expect("claimed");

        finish_task_run(
            &c,
            TaskRunClose {
                run_id: run,
                task_id: "01F",
                host: "hostA",
                finished_ms: 5_000,
                outcome: TaskOutcome::Failed,
                detail: Some("remote hung up"),
                next_due_ms: Some(300_000),
            },
        )
        .expect("finish");

        let row = get_task(&c, "01F").expect("get").expect("row");
        assert_eq!(row.running_host, None);
        assert_eq!(row.lease_until_ms, None);
        assert_eq!(row.next_due_ms, Some(300_000));
        assert_eq!(row.updated_ms, 5_000);
        let runs = task_runs(&c, "01F", 10).expect("runs");
        assert_eq!(runs.len(), 1);
        assert_eq!(runs[0].outcome, Some(TaskOutcome::Failed));
        assert_eq!(runs[0].finished_ms, Some(5_000));
        assert_eq!(runs[0].detail.as_deref(), Some("remote hung up"));
        assert_eq!(runs[0].host, "hostA");
        assert!(
            claim_task(&c, "01F", "hostB", 6_000, 60_000, None)
                .expect("claim")
                .is_some(),
            "releasing the lease and recording the result is one fact, so nothing is half-written"
        );
    }

    /// Three states, and the newest run is the one every surface calls "last":
    /// dropping a run this build cannot read would report an older attempt as
    /// the current one, which is worse than saying "a newer keeper recorded
    /// something here".
    #[test]
    fn an_unreadable_run_outcome_is_carried_not_dropped_and_a_null_one_is_in_flight() {
        let c = conn();
        upsert_task(&c, &task("01O", Some("every 5m"), TaskMode::Scheduled)).expect("save");
        let in_flight = claim_task(&c, "01O", "hostA", 0, 60_000, None)
            .expect("claim")
            .expect("claimed");
        c.execute(
            "INSERT INTO task_runs (task_id, started_ms, finished_ms, outcome, host)
             VALUES ('01O', 1, 2, 'teleported', 'hostZ')",
            [],
        )
        .expect("insert junk");

        let runs = task_runs(&c, "01O", 10).expect("runs");
        assert_eq!(runs.len(), 2, "neither row is dropped and nothing is fatal");
        assert_eq!(
            (runs[0].outcome, runs[0].unknown_outcome.as_deref()),
            (None, Some("teleported")),
            "the NEWEST row is the unreadable one, and it is carried rather than \
             dropped: dropping it would report the older attempt as the last run"
        );
        assert_eq!(runs[1].id, in_flight);
        assert_eq!(
            (runs[1].outcome, runs[1].unknown_outcome.as_deref()),
            (None, None),
            "a NULL outcome with no stored spelling is a run still going"
        );
    }

    #[test]
    fn the_task_runs_cap_trims_the_oldest_and_keeps_the_newest() {
        let c = conn();
        upsert_task(&c, &task("01C", Some("every 5m"), TaskMode::Scheduled)).expect("save");
        let mut ids = Vec::new();
        for n in 0..TASK_RUNS_CAP + 5 {
            let now = 1_000 * (n as i64 + 1);
            let run = claim_task(&c, "01C", "hostA", now, 60_000, None)
                .expect("claim")
                .expect("claimed");
            finish_task_run(
                &c,
                TaskRunClose {
                    run_id: run,
                    task_id: "01C",
                    host: "hostA",
                    finished_ms: now + 1,
                    outcome: TaskOutcome::Ok,
                    detail: None,
                    next_due_ms: Some(now + 60_000),
                },
            )
            .expect("finish");
            ids.push(run);
        }

        let runs = task_runs(&c, "01C", TASK_RUNS_CAP * 2).expect("runs");
        assert_eq!(runs.len(), TASK_RUNS_CAP, "the history is bounded");
        assert_eq!(
            runs[0].id,
            ids[TASK_RUNS_CAP + 4],
            "the newest run survives"
        );
        assert_eq!(
            runs[TASK_RUNS_CAP - 1].id,
            ids[5],
            "exactly the oldest 5 were dropped"
        );
    }

    #[test]
    fn deleting_a_task_takes_its_runs_with_it() {
        let c = conn();
        upsert_task(&c, &task("01G", Some("every 5m"), TaskMode::Scheduled)).expect("save");
        claim_task(&c, "01G", "hostA", 0, 60_000, None)
            .expect("claim")
            .expect("claimed");

        delete_task(&c, "01G").expect("delete");
        assert!(get_task(&c, "01G").expect("get").is_none());
        assert!(
            task_runs(&c, "01G", 10).expect("runs").is_empty(),
            "there is deliberately no foreign key to do this, so the function must"
        );
    }

    /// The lease alone cannot stop one window yielding two runs: two hosts whose
    /// ticks coincide both find it free, because the first host's release lands
    /// before the second host's claim. The window predicate in the same statement
    /// is what closes that.
    #[test]
    fn the_claim_refuses_a_window_that_is_not_open_yet() {
        let c = conn();
        upsert_task(&c, &task("01W", Some("every 5m"), TaskMode::Scheduled)).expect("save");
        arm_task(&c, "01W", Some(10_000), 0).expect("arm");

        assert_eq!(
            claim_task(&c, "01W", "hostA", 9_999, 60_000, Some(9_999)).expect("claim"),
            None,
            "one millisecond before the window opens there is nothing to claim"
        );
        assert!(
            claim_task(&c, "01W", "hostA", 10_000, 60_000, Some(10_000))
                .expect("claim")
                .is_some(),
            "and at the instant it opens the claim succeeds"
        );
        // The requested door passes `None` and does not care about the window,
        // which is what makes run-now work on a task that is not due.
        finish_task_run(
            &c,
            TaskRunClose {
                run_id: 1,
                task_id: "01W",
                host: "hostA",
                finished_ms: 11_000,
                outcome: TaskOutcome::Ok,
                detail: None,
                next_due_ms: Some(999_999),
            },
        )
        .expect("finish");
        assert!(
            claim_task(&c, "01W", "hostB", 12_000, 60_000, None)
                .expect("claim")
                .is_some(),
            "a request is not a window, so it claims a task that is not due"
        );
    }

    /// The sharpest edge in the whole design. A host whose pass outlives its
    /// lease finds the row already reclaimed; releasing it unconditionally freed
    /// the NEW holder's lease mid-run and rewound the window with a stale value,
    /// which turns one overrun into any number of concurrent passes over one git
    /// working tree.
    #[test]
    fn an_overrunning_host_cannot_free_the_lease_that_replaced_it() {
        let c = conn();
        upsert_task(&c, &task("01X", Some("every 5m"), TaskMode::Scheduled)).expect("save");
        arm_task(&c, "01X", Some(0), 0).expect("arm");
        let slow = claim_task(&c, "01X", "hostA", 0, 1_000, Some(0))
            .expect("claim")
            .expect("hostA claims it");
        let taken = claim_task(&c, "01X", "hostB", 2_000, 60_000, None)
            .expect("claim")
            .expect("hostB reclaims the expired lease");

        // hostA finally returns, long after it lost the row.
        finish_task_run(
            &c,
            TaskRunClose {
                run_id: slow,
                task_id: "01X",
                host: "hostA",
                finished_ms: 3_000,
                outcome: TaskOutcome::Ok,
                detail: Some("finished at last"),
                next_due_ms: Some(4_000),
            },
        )
        .expect("finish");

        let row = get_task(&c, "01X").expect("get").expect("row");
        assert_eq!(
            row.running_host.as_deref(),
            Some("hostB"),
            "the overrunning host must not free the lease it no longer holds"
        );
        assert_eq!(row.lease_until_ms, Some(62_000));
        assert_ne!(
            row.next_due_ms,
            Some(4_000),
            "nor rewind the window with the value it computed before it lost the row"
        );

        let runs = task_runs(&c, "01X", 10).expect("runs");
        assert_eq!(runs.len(), 2);
        assert_eq!(
            runs[0].id, taken,
            "hostB's run is the newest and still open"
        );
        assert_eq!(runs[0].outcome, None);
        assert_eq!(
            (runs[1].id, runs[1].outcome, runs[1].detail.as_deref()),
            (slow, Some(TaskOutcome::Ok), Some("finished at last")),
            "the run row still records the truth: it really did end, and later"
        );
    }

    /// The claim is the door the design calls the only arbiter, so `off` has to
    /// hold at the door — not only in whichever caller remembered to check.
    #[test]
    fn an_off_task_cannot_be_claimed_even_by_a_caller_that_did_not_check() {
        let c = conn();
        upsert_task(&c, &task("01Z", Some("every 5m"), TaskMode::Off)).expect("save");
        arm_task(&c, "01Z", Some(0), 0).expect("arm");

        assert_eq!(
            claim_task(&c, "01Z", "hostA", 1_000, 60_000, None).expect("claim"),
            None,
            "an `off` that still runs when asked is not off"
        );
        assert!(task_runs(&c, "01Z", 10).expect("runs").is_empty());
    }

    /// One row SQLite hands back with an unexpected type used to fail the whole
    /// listing, and `run_due_tasks` returns on that error — so a single corrupt
    /// row stopped every task on the host. That is the outcome NFR-43 forbids.
    #[test]
    fn a_row_whose_columns_will_not_read_is_listed_not_fatal() {
        let c = conn();
        upsert_task(&c, &task("01A", Some("every 5m"), TaskMode::Scheduled)).expect("save");
        // A BLOB, which TEXT affinity does not convert, in `kind` — so the
        // vocabulary column really does hand back a value this build's reader
        // refuses, and the tolerant read is what routes it to the unknown path
        // instead of failing the whole listing.
        c.execute(
            "INSERT INTO tasks (id, kind, mode, enabled, updated_ms)
             VALUES ('01T', x'00ff', 'scheduled', 1, 0)",
            [],
        )
        .expect("insert a typed-wrong row");
        // And one whose PRIMARY KEY will not read either. `id` is strict on
        // purpose — a row that cannot be named cannot be reported — so this
        // takes the per-row error arm rather than the tolerant one.
        c.execute(
            "INSERT INTO tasks (id, kind, mode, enabled, updated_ms)
             VALUES (x'dead', 'sync', 'scheduled', 1, 0)",
            [],
        )
        .expect("insert an unnameable row");

        let listing = list_tasks(&c).expect("the listing must survive both");
        assert_eq!(
            listing
                .tasks
                .iter()
                .map(|t| t.id.as_str())
                .collect::<Vec<_>>(),
            vec!["01A"],
            "the readable task is still there, which is the whole point: one \
             corrupt row must not stop every task on the host"
        );
        assert_eq!(listing.unknown.len(), 2, "and both are named as unknown");
        assert!(
            listing.unknown.iter().any(|u| u.id == "01T"),
            "the row whose kind will not read is reported by id"
        );
        assert!(
            listing
                .unknown
                .iter()
                .any(|u| u.id.is_empty() && u.reason.contains("unreadable task row")),
            "and the row whose id will not read is reported without one"
        );
    }

    /// A schedule change has to take effect on the next tick, not after the old
    /// schedule's window elapses — and an unrelated save must not move a window
    /// at all, which is what a settings form re-saving a whole row does.
    #[test]
    fn changing_the_schedule_rearms_and_an_unrelated_save_leaves_the_window_alone() {
        let c = conn();
        upsert_task(&c, &task("01S", Some("@daily"), TaskMode::Scheduled)).expect("save");
        arm_task(&c, "01S", Some(50_000), 0).expect("arm");

        let mut renamed = task("01S", Some("@daily"), TaskMode::Scheduled);
        renamed.profile_id = Some("elsewhere".to_owned());
        upsert_task(&c, &renamed).expect("save");
        assert_eq!(
            get_task(&c, "01S").expect("get").expect("row").next_due_ms,
            Some(50_000),
            "the schedule did not change, so neither does the window"
        );

        upsert_task(&c, &task("01S", Some("every 5m"), TaskMode::Scheduled)).expect("save");
        assert_eq!(
            get_task(&c, "01S").expect("get").expect("row").next_due_ms,
            None,
            "a new schedule is unarmed, so the next tick computes its first window"
        );
    }

    /// The id is a primary key, it is what a CLI reads from argv, and it is what
    /// `task_runs.task_id` joins on. Three spellings of one intended task is
    /// worse than a refusal.
    #[test]
    fn a_task_id_that_is_padded_with_whitespace_is_refused() {
        let c = conn();
        for id in [" 01A", "01A ", "\t01A"] {
            let padded = TaskRow {
                id: id.to_owned(),
                ..task("01A", Some("every 5m"), TaskMode::Scheduled)
            };
            assert!(
                matches!(upsert_task(&c, &padded), Err(SyncError::Config(_))),
                "{id:?} must be refused"
            );
        }
        assert!(list_tasks(&c).expect("list").tasks.is_empty());
    }

    /// The two-host design rests on a writer *waiting* rather than failing at
    /// once: `is_row_contention` is the last resort, not the normal path. Before
    /// Story 57.2 this file had no timeout at all, so every cross-process
    /// collision failed instantly — which for the release half of a task run
    /// means the outcome is recorded nowhere and the lease is held out.
    #[test]
    fn the_shared_database_waits_for_a_writer_rather_than_failing_at_once() {
        let dir = tempfile::tempdir().expect("tempdir");
        let c = open(dir.path()).expect("open");
        let timeout: i64 = c
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .expect("pragma");
        assert!(
            timeout >= 1_000,
            "a `sync.db` two processes write must wait for a writer, got {timeout} ms"
        );
    }

    /// First sight happens once. A decision computed from a listing read before
    /// the other host armed the row would otherwise rewind the window into the
    /// past and run the task again.
    #[test]
    fn arming_a_task_that_is_already_armed_changes_nothing() {
        let c = conn();
        upsert_task(&c, &task("01AA", Some("every 5m"), TaskMode::Scheduled)).expect("save");
        arm_task(&c, "01AA", Some(50_000), 1_000).expect("arm");

        arm_task(&c, "01AA", Some(10), 2_000).expect("a stale decision arrives");

        let row = get_task(&c, "01AA").expect("get").expect("row");
        assert_eq!(
            row.next_due_ms,
            Some(50_000),
            "a stale first-sight decision cannot rewind an armed window"
        );
        assert_eq!(row.updated_ms, 1_000, "and the row is not touched at all");
    }

    /// NFR-43's write half. `get_task` answers `None` for a row this build
    /// cannot read, so a create-if-absent caller walks straight into overwriting
    /// a newer keeper's task and rewriting its kind to one of ours.
    #[test]
    fn a_task_this_build_cannot_read_is_never_overwritten() {
        let c = conn();
        raw_task(&c, "01N", "teleport", "scheduled", Some("every 5m"));

        let mine = task("01N", Some("every 5m"), TaskMode::Scheduled);
        assert!(
            matches!(upsert_task(&c, &mine), Err(SyncError::Config(_))),
            "refusing to overwrite is the only honest answer"
        );
        let stored: String = c
            .query_row("SELECT kind FROM tasks WHERE id = '01N'", [], |r| r.get(0))
            .expect("kind");
        assert_eq!(
            stored, "teleport",
            "and the newer keeper's row is untouched"
        );
    }

    /// The run a claim has just opened must survive the trim in that same
    /// transaction: it is the row [`finish_task_run`] is about to close, and a
    /// trim that took it would leave a completed run recorded nowhere and
    /// `run_task_now` reporting a `Journal` error for work that succeeded.
    /// Reachable whenever the cap is already full, which for a frequent task is
    /// most of the time.
    #[test]
    fn the_trim_never_takes_the_run_the_claim_just_opened() {
        let c = conn();
        upsert_task(&c, &task("01I", Some("every 1m"), TaskMode::Scheduled)).expect("save");
        for n in 0..TASK_RUNS_CAP as i64 {
            let run = claim_task(&c, "01I", "hostA", n * 10, 1, None)
                .expect("claim")
                .expect("claimed");
            finish_task_run(
                &c,
                TaskRunClose {
                    run_id: run,
                    task_id: "01I",
                    host: "hostA",
                    finished_ms: n * 10 + 1,
                    outcome: TaskOutcome::Ok,
                    detail: None,
                    next_due_ms: None,
                },
            )
            .expect("finish");
        }
        assert_eq!(
            task_runs(&c, "01I", TASK_RUNS_CAP * 2).expect("runs").len(),
            TASK_RUNS_CAP,
            "the cap is full before the run under test is opened"
        );

        let opened = claim_task(&c, "01I", "hostB", 999_000, 60_000, None)
            .expect("claim")
            .expect("claimed");
        let runs = task_runs(&c, "01I", TASK_RUNS_CAP * 2).expect("runs");
        assert_eq!(runs.len(), TASK_RUNS_CAP, "and it still holds afterwards");
        assert_eq!(
            (runs[0].id, runs[0].finished_ms),
            (opened, None),
            "the newest row is the one still in flight, not a casualty of its own trim"
        );

        finish_task_run(
            &c,
            TaskRunClose {
                run_id: opened,
                task_id: "01I",
                host: "hostB",
                finished_ms: 999_500,
                outcome: TaskOutcome::Ok,
                detail: Some("done"),
                next_due_ms: None,
            },
        )
        .expect("finish");
        assert_eq!(
            task_runs(&c, "01I", 1).expect("runs")[0].detail.as_deref(),
            Some("done"),
            "so the outcome lands on a row that still exists"
        );
    }

    /// NFR-42 asks that a SIGTERM be a bounded finalize. Without this a
    /// `systemctl restart` mid-run left the lease held by a pid that no longer
    /// exists for the whole of the lease, and the restarted daemon cannot prove
    /// that pid is dead — the app shares the device row and may hold one.
    #[test]
    fn releasing_a_hosts_leases_closes_its_runs_and_frees_its_tasks() {
        let c = conn();
        for id in ["01P", "01Q"] {
            upsert_task(&c, &task(id, Some("every 5m"), TaskMode::Scheduled)).expect("save");
            claim_task(&c, id, "dying", 0, 3_600_000, None)
                .expect("claim")
                .expect("claimed");
        }
        upsert_task(&c, &task("01R2", Some("every 5m"), TaskMode::Scheduled)).expect("save");
        claim_task(&c, "01R2", "other", 0, 3_600_000, None)
            .expect("claim")
            .expect("claimed");

        assert_eq!(release_host_leases(&c, "dying", 9_000).expect("release"), 2);
        for id in ["01P", "01Q"] {
            let row = get_task(&c, id).expect("get").expect("row");
            assert_eq!(row.running_host, None, "{id} is claimable again at once");
            assert_eq!(row.lease_until_ms, None);
            let runs = task_runs(&c, id, 10).expect("runs");
            assert_eq!(
                (runs[0].outcome, runs[0].finished_ms),
                (Some(TaskOutcome::Abandoned), Some(9_000)),
                "{id}'s open run is closed with the truth rather than left open"
            );
        }
        assert_eq!(
            get_task(&c, "01R2")
                .expect("get")
                .expect("row")
                .running_host
                .as_deref(),
            Some("other"),
            "and another host's lease is none of this one's business"
        );
    }

    /// The `SQLITE_BUSY` mapping the two-host story rests on, exercised for
    /// real: a connection with no `busy_timeout` meeting a held write lock.
    /// `db::open` sets a timeout, so this is the last-resort path — and a design
    /// that reads a lock as "not mine" has to prove it does.
    #[test]
    fn a_claim_that_meets_a_held_write_lock_reads_as_not_mine_rather_than_as_an_error() {
        let dir = tempfile::tempdir().expect("tempdir");
        let created = open(dir.path()).expect("open");
        upsert_task(
            &created,
            &task("01B2", Some("every 5m"), TaskMode::Scheduled),
        )
        .expect("save");

        // A writer holding the lock, on its own connection.
        let holder = Connection::open(db_path(dir.path())).expect("holder");
        holder
            .execute_batch("BEGIN IMMEDIATE; UPDATE tasks SET updated_ms = 1 WHERE id = '01B2';")
            .expect("hold the write lock");

        // No `busy_timeout`, so the claim fails immediately rather than waiting.
        let contender = Connection::open(db_path(dir.path())).expect("contender");
        contender
            .busy_timeout(std::time::Duration::from_millis(0))
            .expect("no timeout");
        assert_eq!(
            claim_task(&contender, "01B2", "hostB", 0, 60_000, None)
                .expect("contention is an answer about the lease, not a fault to propagate"),
            None
        );

        holder.execute_batch("COMMIT;").expect("release");
        assert!(
            task_runs(&created, "01B2", 10).expect("runs").is_empty(),
            "and the refused claim opened no run"
        );
    }

    /// A task naming a folder that is gone fails permanently, on a schedule,
    /// forever — so the folder's removal has to take it.
    #[test]
    fn deleting_a_folder_takes_its_tasks_and_their_history() {
        let c = conn();
        upsert_profile(&c, &profile("p"), 0).expect("profile");
        let mut owned = task("01Y", Some("every 5m"), TaskMode::Scheduled);
        owned.profile_id = Some("p".to_owned());
        upsert_task(&c, &owned).expect("save");
        claim_task(&c, "01Y", "hostA", 0, 60_000, None)
            .expect("claim")
            .expect("claimed");
        let host_wide = TaskRow {
            profile_id: None,
            ..task("01HW", Some("every 5m"), TaskMode::Scheduled)
        };
        upsert_task(&c, &host_wide).expect("save");

        delete_profile(&c, "p").expect("delete");

        assert!(get_task(&c, "01Y").expect("get").is_none());
        assert!(
            task_runs(&c, "01Y", 10).expect("runs").is_empty(),
            "its history goes with it, since no foreign key does that here"
        );
        assert!(
            get_task(&c, "01HW").expect("get").is_some(),
            "a host-wide task belongs to the machine and survives"
        );
    }
}
