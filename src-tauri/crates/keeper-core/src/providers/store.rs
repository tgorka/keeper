//! Provider rows in `keeper.db` (Story 61.1, FR-369, AD-139; Story 108.3a).
//!
//! One table beside the account registry, in the same database and under the
//! same rules as [`crate::registry`]: WAL, a busy timeout because every caller
//! opens its own short-lived connection, `CREATE TABLE IF NOT EXISTS` on every
//! open so a cold install and an upgrade take the same path, and **no JSON blob
//! in a row** (AD-139) — a health snapshot is three scalar columns, so every
//! one of them is a column SQL can filter and a migration can add.
//!
//! The table keeps its name, `bot_providers`, after the surface it was written
//! for left keeper (Story 108.3a, DW-E108-6): every device that still runs an
//! older build opens this same file.
//!
//! Synchronous throughout, for [`crate::registry`]'s reason: a rusqlite
//! `Connection` is never held across an `.await`. Callers open, operate and
//! drop within one synchronous scope.
//!
//! **No credential column exists here, and none can be added.** A provider's
//! token lives behind the secret port ([`crate::providers::provider_token_key`])
//! and the base-URL grammar refuses userinfo, so there is no shape in which
//! `keeper.db` holds one (FR-370, and the same posture the account registry
//! takes for access tokens).

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::{Provider, ProviderHealth, ProviderHealthState, ProviderKind};
use crate::error::{CoreError, PlatformError};

/// Resolve the `keeper.db` path under a data directory — the same file
/// [`crate::registry`] uses, because this is the same registry.
fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("keeper.db")
}

/// How long a contended writer waits for the write lock. Mirrors
/// `registry`'s timeout: WAL serializes writers, and a health write landing
/// while another row is rewritten must wait rather than return `SQLITE_BUSY`
/// to a surface that has nothing useful to say about it.
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Open `keeper.db` in WAL mode, ensuring the provider table exists. Every
/// call is idempotent, so the first call on a cold install and the
/// ten-thousandth on an upgraded one do the same thing.
pub(crate) fn open(data_dir: &Path) -> Result<Connection, CoreError> {
    std::fs::create_dir_all(data_dir).map_err(|e| {
        CoreError::Platform(PlatformError::DirUnavailable(format!(
            "could not create data dir: {e}"
        )))
    })?;
    let conn = Connection::open(db_path(data_dir))
        .map_err(|e| CoreError::Internal(format!("could not open keeper.db: {e}")))?;
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(|e| CoreError::Internal(format!("could not set busy timeout: {e}")))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| CoreError::Internal(format!("could not set WAL mode: {e}")))?;
    // One row per configured AI provider (Story 61.1, FR-369). The health
    // snapshot is three scalars rather than a blob (AD-139): `health_state` is
    // the only thing a listing filters on, and a JSON column would make it
    // unfilterable and unmigratable at once. `read_timeout_ms` is the
    // per-provider SILENCE bound (Story 61.2), nullable because the default is
    // the policy and an override is the exception.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS bot_providers(\
            id TEXT PRIMARY KEY, \
            kind TEXT NOT NULL, \
            name TEXT NOT NULL, \
            base_url TEXT NOT NULL, \
            created_ms INTEGER NOT NULL, \
            health_state TEXT NOT NULL, \
            health_checked_ms INTEGER, \
            health_detail TEXT, \
            read_timeout_ms INTEGER\
        )",
        [],
    )
    .map_err(|e| CoreError::Internal(format!("could not ensure bot_providers schema: {e}")))?;
    Ok(conn)
}

/// A provider row as stored: what the user typed, plus what keeper has learned
/// about it (Story 61.1).
///
/// Split from [`Provider`] because the two have different lifetimes. The
/// `Provider` is identity — it changes only when the user edits it — while the
/// health snapshot is rewritten by every probe. Keeping them one struct would
/// mean a probe result and a rename could not be written independently, and one
/// of them would end up clobbering the other.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderRow {
    /// The identity fields.
    pub provider: Provider,
    /// The last thing keeper learned by talking to it.
    pub health: ProviderHealth,
    /// The per-provider silence bound in milliseconds, or `None` to use the
    /// policy default (Story 61.2 owns the default; this only stores an
    /// override).
    pub read_timeout_ms: Option<i64>,
}

impl ProviderRow {
    /// How long a request to this provider may deliver nothing before it is
    /// a failure: the row's override, else the policy default
    /// ([`super::http::READ_TIMEOUT`]). An override of zero or less is no
    /// override.
    pub fn read_timeout(&self) -> std::time::Duration {
        match self.read_timeout_ms {
            Some(ms) if ms > 0 => std::time::Duration::from_millis(ms.unsigned_abs()),
            _ => super::http::READ_TIMEOUT,
        }
    }
}

/// A provider row keeper could read but cannot speak to (Story 61.1).
///
/// Produced when `kind` holds a value this build does not know — a row written
/// by a newer build, or hand-edited. Surfaced rather than skipped, for the
/// reason [`crate::tasks::UnknownTaskVm`] exists: a provider that silently
/// vanished from the list is a provider the user will add a second time, and
/// then wonder why the first one's egress row is still there.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownProviderRow {
    /// The row's id, which is readable even when its kind is not.
    pub id: String,
    /// The row's display name.
    pub name: String,
    /// The `kind` string exactly as stored, so the sentence can name it.
    pub kind: String,
}

/// Everything the `bot_providers` table holds, partitioned by whether this
/// build can speak to it (Story 61.1).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProviderListing {
    /// The readable rows, in the deterministic order [`list_providers`]
    /// documents.
    pub rows: Vec<ProviderRow>,
    /// The rows whose `kind` this build does not know, in the same order.
    pub unknown: Vec<UnknownProviderRow>,
}

/// The `SELECT` column list every provider read shares, so the column order the
/// mapper assumes is written exactly once.
const PROVIDER_COLUMNS: &str = "id, kind, name, base_url, created_ms, health_state, \
     health_checked_ms, health_detail, read_timeout_ms";

/// Insert one provider row (Story 61.1, FR-369).
///
/// Fails if `provider.id` already exists (PRIMARY KEY), exactly as
/// `registry::insert_account` does: an id collision means the caller minted a
/// duplicate, and overwriting would silently retarget the existing provider's
/// bots and credential.
///
/// The row starts at [`ProviderHealth::unknown`] — the honest state before any
/// probe — and with no timeout override.
pub fn insert_provider(data_dir: &Path, provider: &Provider) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    conn.execute(
        "INSERT INTO bot_providers(id, kind, name, base_url, created_ms, health_state) \
         VALUES(?1, ?2, ?3, ?4, ?5, ?6)",
        rusqlite::params![
            provider.id,
            provider.kind.as_registry_str(),
            provider.name,
            provider.base_url,
            provider.created_ms,
            ProviderHealth::unknown().state.as_registry_str(),
        ],
    )
    .map_err(|e| CoreError::Internal(format!("could not insert provider: {e}")))?;
    Ok(())
}

/// Rewrite a provider's editable identity — kind, name and base URL — by id
/// (Story 61.1, FR-379).
///
/// Returns whether a row matched, rather than succeeding silently on a missing
/// id: an edit form that saves into nothing and reports success is the kind of
/// affordance AD-27 exists to forbid.
///
/// `created_ms` is deliberately not writable, and neither is the health
/// snapshot: an edit changes where keeper will call next, so the previous
/// probe's verdict is about an endpoint that may no longer be this one. The
/// caller re-probes and writes the result through [`set_provider_health`],
/// which is one call it cannot forget because the stale verdict would otherwise
/// still be on screen.
pub fn update_provider(data_dir: &Path, provider: &Provider) -> Result<bool, CoreError> {
    let conn = open(data_dir)?;
    let changed = conn
        .execute(
            "UPDATE bot_providers SET kind = ?2, name = ?3, base_url = ?4 WHERE id = ?1",
            rusqlite::params![
                provider.id,
                provider.kind.as_registry_str(),
                provider.name,
                provider.base_url,
            ],
        )
        .map_err(|e| CoreError::Internal(format!("could not update provider: {e}")))?;
    Ok(changed > 0)
}

/// Write a provider's health snapshot (Story 61.1).
///
/// Returns whether a row matched. The three columns are written together
/// because they are one observation: a state without its timestamp is a verdict
/// with no age, and this is the app that refuses to print a number it did not
/// measure.
pub fn set_provider_health(
    data_dir: &Path,
    provider_id: &str,
    health: &ProviderHealth,
) -> Result<bool, CoreError> {
    let conn = open(data_dir)?;
    let changed = conn
        .execute(
            "UPDATE bot_providers \
             SET health_state = ?2, health_checked_ms = ?3, health_detail = ?4 \
             WHERE id = ?1",
            rusqlite::params![
                provider_id,
                health.state.as_registry_str(),
                health.checked_ms,
                health.detail,
            ],
        )
        .map_err(|e| CoreError::Internal(format!("could not write provider health: {e}")))?;
    Ok(changed > 0)
}

/// Set (or clear, with `None`) a provider's silence-bound override in
/// milliseconds (Story 61.1).
///
/// Returns whether a row matched. Clearing puts the policy default back in
/// force, which is why "unset" and "never set" are one state — the same
/// convention the recording settings follow.
pub fn set_provider_read_timeout(
    data_dir: &Path,
    provider_id: &str,
    read_timeout_ms: Option<i64>,
) -> Result<bool, CoreError> {
    let conn = open(data_dir)?;
    let changed = conn
        .execute(
            "UPDATE bot_providers SET read_timeout_ms = ?2 WHERE id = ?1",
            rusqlite::params![provider_id, read_timeout_ms],
        )
        .map_err(|e| CoreError::Internal(format!("could not write provider timeout: {e}")))?;
    Ok(changed > 0)
}

/// Read one provider row by id, or `None` when there is none — or when its
/// `kind` is one this build cannot speak (Story 61.1).
pub fn get_provider(data_dir: &Path, provider_id: &str) -> Result<Option<ProviderRow>, CoreError> {
    let conn = open(data_dir)?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {PROVIDER_COLUMNS} FROM bot_providers WHERE id = ?1"
        ))
        .map_err(|e| CoreError::Internal(format!("could not prepare provider query: {e}")))?;
    let mut rows = stmt
        .query(rusqlite::params![provider_id])
        .map_err(|e| CoreError::Internal(format!("could not query provider: {e}")))?;
    let Some(row) = rows
        .next()
        .map_err(|e| CoreError::Internal(format!("could not read provider row: {e}")))?
    else {
        return Ok(None);
    };
    Ok(map_provider_row(row)
        .map_err(|e| CoreError::Internal(format!("could not map provider row: {e}")))?
        .ok())
}

/// Every provider row, partitioned into what this build can speak to and what
/// it cannot (Story 61.1, FR-369).
///
/// Ordered `created_ms, id` — insertion order, with the id as the tiebreaker so
/// two providers added inside one millisecond still list identically on every
/// read. A bare `SELECT` has unspecified row order in SQLite, and this list
/// drives a picker whose keyboard order must not shuffle between opens.
pub fn list_providers(data_dir: &Path) -> Result<ProviderListing, CoreError> {
    let conn = open(data_dir)?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {PROVIDER_COLUMNS} FROM bot_providers ORDER BY created_ms ASC, id ASC"
        ))
        .map_err(|e| CoreError::Internal(format!("could not prepare provider list: {e}")))?;
    let mapped = stmt
        .query_map([], map_provider_row)
        .map_err(|e| CoreError::Internal(format!("could not query providers: {e}")))?;
    let mut listing = ProviderListing::default();
    for entry in mapped {
        match entry.map_err(|e| CoreError::Internal(format!("could not read provider: {e}")))? {
            Ok(row) => listing.rows.push(row),
            Err(unknown) => listing.unknown.push(unknown),
        }
    }
    Ok(listing)
}

/// Every readable provider's base URL, in list order — the input
/// `crate::egress::compute_egress` derives its provider disclosure from (Story
/// 61.1, FR-371).
///
/// A projection rather than a second query, so "which providers are disclosed"
/// and "which providers exist" cannot answer differently. An unreadable row
/// contributes nothing: keeper cannot say what it would contact, and a
/// fabricated destination is the one thing the honesty surface must never
/// carry.
pub fn provider_base_urls(data_dir: &Path) -> Result<Vec<String>, CoreError> {
    Ok(list_providers(data_dir)?
        .rows
        .into_iter()
        .map(|row| row.provider.base_url)
        .collect())
}

/// Delete a provider row (Story 61.1, FR-379).
///
/// Idempotent — deleting a missing provider is not an error, so this is safe on
/// the rollback path of a half-finished add.
///
/// **The credential is not deleted here.** It lives behind the secret port,
/// which this crate reaches only through [`crate::platform::Platform`] — so the
/// caller pairs this with [`crate::providers::delete_provider_token`]. Splitting
/// it is deliberate: a database function that quietly needed a platform port
/// would make every test of this table need one too.
///
/// **A `bots` row naming the provider goes with it**, in the same transaction:
/// that table is the agent runtime's until it leaves keeper (Story 108.3c),
/// and its foreign key on this one is enforced, so a row an older build wrote
/// after the upgrade dropped the table must not make a provider impossible to
/// remove. Grants cascade by their own key.
pub fn delete_provider(data_dir: &Path, provider_id: &str) -> Result<(), CoreError> {
    let mut conn = open(data_dir)?;
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| CoreError::Internal(format!("could not begin provider delete: {e}")))?;
    let bots: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = 'bots')",
            [],
            |r| r.get(0),
        )
        .map_err(|e| CoreError::Internal(format!("could not read the schema: {e}")))?;
    if bots {
        tx.execute(
            "DELETE FROM bots WHERE provider_id = ?1",
            rusqlite::params![provider_id],
        )
        .map_err(|e| CoreError::Internal(format!("could not delete provider's bots: {e}")))?;
    }
    tx.execute(
        "DELETE FROM bot_providers WHERE id = ?1",
        rusqlite::params![provider_id],
    )
    .map_err(|e| CoreError::Internal(format!("could not delete provider: {e}")))?;
    tx.commit()
        .map_err(|e| CoreError::Internal(format!("could not commit provider delete: {e}")))
}

/// Map a `SELECT PROVIDER_COLUMNS` row into a [`ProviderRow`], or into an
/// [`UnknownProviderRow`] when the stored `kind` is not one this build knows.
///
/// The `Result` inside the `Ok` is the partition, not a failure: `Err` here
/// means "readable, but not speakable", which is a row the listing still shows.
/// A genuine SQLite error is the outer `Err`.
#[allow(clippy::type_complexity)]
fn map_provider_row(
    r: &rusqlite::Row<'_>,
) -> rusqlite::Result<Result<ProviderRow, UnknownProviderRow>> {
    let id: String = r.get(0)?;
    let kind_text: String = r.get(1)?;
    let name: String = r.get(2)?;
    let Some(kind) = ProviderKind::from_registry_str(&kind_text) else {
        return Ok(Err(UnknownProviderRow {
            id,
            name,
            kind: kind_text,
        }));
    };
    let health_state: String = r.get(5)?;
    Ok(Ok(ProviderRow {
        provider: Provider {
            id,
            kind,
            name,
            base_url: r.get(3)?,
            created_ms: r.get(4)?,
        },
        health: ProviderHealth {
            state: ProviderHealthState::from_registry_str(&health_state),
            checked_ms: r.get(6)?,
            detail: r.get(7)?,
        },
        read_timeout_ms: r.get(8)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scratch directory no other test can land in — the pid, a nanosecond
    /// stamp AND a process-wide counter, because two threads asking inside one
    /// clock tick otherwise open the same SQLite file and fail on whichever
    /// collision they reach first (`registry.rs:2096-2104` records both
    /// observations).
    fn temp_dir() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "keeper-providers-test-{}-{}-{n}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        dir
    }

    fn provider(id: &str, kind: ProviderKind, created_ms: i64) -> Provider {
        Provider {
            id: id.to_owned(),
            kind,
            name: format!("provider {id}"),
            base_url: "http://localhost:11434".to_owned(),
            created_ms,
        }
    }

    #[test]
    fn a_provider_round_trips_and_starts_with_no_health_it_did_not_measure() {
        let dir = temp_dir();
        let inserted = provider("p1", ProviderKind::Hermes, 1_700_000_000_000);
        insert_provider(&dir, &inserted).expect("insert should succeed");

        let row = get_provider(&dir, "p1")
            .expect("read should succeed")
            .expect("row should exist");
        assert_eq!(row.provider, inserted);
        assert_eq!(
            row.health.state,
            ProviderHealthState::Unknown,
            "a provider nobody has called yet is unknown, never reachable"
        );
        assert_eq!(
            row.health.checked_ms, None,
            "a timestamp keeper did not measure must not be invented"
        );
        assert_eq!(row.health.detail, None);
        assert_eq!(row.read_timeout_ms, None);

        assert!(
            get_provider(&dir, "nope")
                .expect("read should succeed")
                .is_none(),
            "an absent id reads as None, not as an error"
        );
    }

    #[test]
    fn a_second_insert_of_one_id_is_refused() {
        let dir = temp_dir();
        let row = provider("p1", ProviderKind::Ollama, 1);
        insert_provider(&dir, &row).expect("first insert");
        assert!(
            insert_provider(&dir, &row).is_err(),
            "an id collision must not silently retarget the existing provider"
        );
    }

    #[test]
    fn an_edit_reports_whether_it_landed() {
        let dir = temp_dir();
        insert_provider(&dir, &provider("p1", ProviderKind::Ollama, 1)).expect("insert");

        let edited = Provider {
            name: "home ollama".to_owned(),
            base_url: "http://10.0.0.7:11434".to_owned(),
            kind: ProviderKind::Ollama,
            ..provider("p1", ProviderKind::Ollama, 1)
        };
        assert!(update_provider(&dir, &edited).expect("update"));
        let row = get_provider(&dir, "p1").expect("read").expect("row");
        assert_eq!(row.provider.name, "home ollama");
        assert_eq!(row.provider.base_url, "http://10.0.0.7:11434");

        let missing = provider("nope", ProviderKind::Ollama, 1);
        assert!(
            !update_provider(&dir, &missing).expect("update"),
            "an edit that matched no row must say so rather than report success"
        );
    }

    #[test]
    fn health_and_the_timeout_override_are_written_and_read_as_scalars() {
        let dir = temp_dir();
        insert_provider(&dir, &provider("p1", ProviderKind::Hermes, 1)).expect("insert");

        let health = ProviderHealth {
            state: ProviderHealthState::Unauthorized,
            checked_ms: Some(1_700_000_123_456),
            detail: Some("this profile needs its own key".to_owned()),
        };
        assert!(set_provider_health(&dir, "p1", &health).expect("write health"));
        assert!(set_provider_read_timeout(&dir, "p1", Some(90_000)).expect("write timeout"));

        let row = get_provider(&dir, "p1").expect("read").expect("row");
        assert_eq!(row.health, health);
        assert_eq!(row.read_timeout_ms, Some(90_000));

        // Clearing the override puts the policy default back in force.
        assert!(set_provider_read_timeout(&dir, "p1", None).expect("clear timeout"));
        let row = get_provider(&dir, "p1").expect("read").expect("row");
        assert_eq!(row.read_timeout_ms, None);

        assert!(
            !set_provider_health(&dir, "nope", &health).expect("write health"),
            "a health write against no row must report that it landed nowhere"
        );
    }

    #[test]
    fn providers_list_in_a_deterministic_order() {
        let dir = temp_dir();
        // Inserted out of order, and two share a creation millisecond so the id
        // tiebreaker is what decides.
        insert_provider(&dir, &provider("p3", ProviderKind::Ollama, 30)).expect("insert");
        insert_provider(&dir, &provider("p1", ProviderKind::Hermes, 10)).expect("insert");
        insert_provider(&dir, &provider("p2", ProviderKind::Ollama, 10)).expect("insert");

        let ids: Vec<String> = list_providers(&dir)
            .expect("list")
            .rows
            .into_iter()
            .map(|row| row.provider.id)
            .collect();
        assert_eq!(ids, vec!["p1", "p2", "p3"]);
    }

    /// A row this build cannot speak to is SHOWN, not skipped: a provider that
    /// silently vanished is one the user adds a second time.
    #[test]
    fn a_provider_of_an_unknown_kind_is_surfaced_rather_than_dropped() {
        let dir = temp_dir();
        insert_provider(&dir, &provider("p1", ProviderKind::Ollama, 10)).expect("insert");
        // Written the way a newer build would write it.
        {
            let conn = open(&dir).expect("open");
            conn.execute(
                "INSERT INTO bot_providers(id, kind, name, base_url, created_ms, health_state) \
                 VALUES('p2', 'omp', 'future', 'https://omp.example.org', 20, 'unknown')",
                [],
            )
            .expect("insert future row");
        }

        let listing = list_providers(&dir).expect("list");
        assert_eq!(listing.rows.len(), 1);
        assert_eq!(listing.rows[0].provider.id, "p1");
        assert_eq!(listing.unknown.len(), 1);
        assert_eq!(listing.unknown[0].id, "p2");
        assert_eq!(listing.unknown[0].kind, "omp");
        assert_eq!(listing.unknown[0].name, "future");

        assert!(
            get_provider(&dir, "p2").expect("read").is_none(),
            "a kind this build cannot speak has no ProviderRow to hand back"
        );
        assert_eq!(
            provider_base_urls(&dir).expect("base urls"),
            vec!["http://localhost:11434".to_owned()],
            "an unreadable row must not contribute a disclosed destination"
        );
    }

    #[test]
    fn deleting_a_provider_removes_its_row_and_leaves_every_other() {
        let dir = temp_dir();
        insert_provider(&dir, &provider("p1", ProviderKind::Ollama, 1)).expect("insert");
        insert_provider(&dir, &provider("p2", ProviderKind::Hermes, 2)).expect("insert");
        delete_provider(&dir, "p1").expect("delete");
        delete_provider(&dir, "p1").expect("a second delete is not an error");
        let ids: Vec<String> = list_providers(&dir)
            .expect("list")
            .rows
            .into_iter()
            .map(|row| row.provider.id)
            .collect();
        assert_eq!(ids, vec!["p2"]);
    }
}
