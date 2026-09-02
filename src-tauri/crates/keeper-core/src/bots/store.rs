//! Provider and bot rows in `keeper.db` (Story 61.1, FR-369, AD-139).
//!
//! Two tables beside the account registry, in the same database and under the
//! same rules as [`crate::registry`]: WAL, a busy timeout because every caller
//! opens its own short-lived connection, `CREATE TABLE IF NOT EXISTS` on every
//! open so a cold install and an upgrade take the same path, and **no JSON blob
//! in a row** (AD-139) — a health snapshot is three scalar columns and an
//! identity is three more, so every one of them is a column SQL can filter and
//! a migration can add.
//!
//! Synchronous throughout, for [`crate::registry`]'s reason: a rusqlite
//! `Connection` is never held across an `.await`. Callers open, operate and
//! drop within one synchronous scope.
//!
//! **Why this module opens the database itself.** `registry::open` is private
//! and owns the `accounts` schema; two idempotent openers on one file is
//! exactly what SQLite's WAL is for, and the alternative — making the registry
//! aware of every later feature's tables — is the coupling that turns one
//! module into the place every story edits.
//!
//! **No credential column exists here, and none can be added.** A provider's
//! token lives behind the secret port ([`crate::bots::provider_token_key`]) and
//! the base-URL grammar refuses userinfo, so there is no shape in which
//! `keeper.db` holds one (FR-370, and the same posture the account registry
//! takes for access tokens).

use std::path::{Path, PathBuf};

use rusqlite::Connection;

use super::grant::{Grant, GrantMode, GrantScope};
use super::{Bot, BotIdentity, Provider, ProviderHealth, ProviderKind};
use crate::error::{CoreError, PlatformError};

/// Resolve the `keeper.db` path under a data directory — the same file
/// [`crate::registry`] uses, because this is the same registry.
fn db_path(data_dir: &Path) -> PathBuf {
    data_dir.join("keeper.db")
}

/// How long a contended writer waits for the write lock. Mirrors
/// `registry`'s timeout: WAL serializes writers, and a health write landing
/// while the pin order is being rewritten must wait rather than return
/// `SQLITE_BUSY` to a surface that has nothing useful to say about it.
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// Open `keeper.db` in WAL mode, ensuring the bots schema exists. Every call is
/// idempotent, so the first call on a cold install and the ten-thousandth on an
/// upgraded one do the same thing.
///
/// The `bots.provider_id` foreign key is **declared and enforced**, and the
/// sentence that used to stand here — "rusqlite leaves `PRAGMA foreign_keys`
/// off" — was measured false on this build (2026-09-02, story 61.10):
/// `libsqlite3-sys`' bundled SQLite is compiled with
/// `SQLITE_DEFAULT_FOREIGN_KEYS=1`, which the test
/// `grant_a_grant_cannot_name_a_provider_that_does_not_exist` pins. Anyone
/// adding a table here must therefore either declare `ON DELETE CASCADE` or
/// delete children first: [`delete_provider`] does the latter explicitly inside
/// one transaction, for the reason `keeper-sync`'s `delete_profile` gives — a
/// cascade decides the order and the atomicity of a cleanup somewhere no test
/// can read — and `bot_grants` does the former, because a permission must not
/// outlive the endpoint it was about nor block its removal.
fn open(data_dir: &Path) -> Result<Connection, CoreError> {
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
    // One row per pinned bot (Story 61.1, FR-383). `target` is the Hermes
    // profile name or the Ollama model tag — one column because it is one
    // concept, and two nullable ones would let a row name neither. The identity
    // columns are nullable until Story 61.7 gives them a picker: an assigned
    // colour would have to be un-assigned later, and `DESIGN.md:172` requires a
    // colour to be paired with a shape.
    //
    // `UNIQUE(provider_id, target)` because pinning the same bot of the same
    // provider twice is not two bots; it is one bot and a duplicate row that
    // every list, order and grant would then have to disambiguate.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS bots(\
            id TEXT PRIMARY KEY, \
            provider_id TEXT NOT NULL REFERENCES bot_providers(id), \
            target TEXT NOT NULL, \
            name TEXT NOT NULL, \
            pin_order INTEGER NOT NULL, \
            identity_shape TEXT, \
            identity_colour TEXT, \
            identity_mark TEXT, \
            created_ms INTEGER NOT NULL, \
            UNIQUE(provider_id, target)\
        )",
        [],
    )
    .map_err(|e| CoreError::Internal(format!("could not ensure bots schema: {e}")))?;
    // One row per grant (Story 61.10, FR-386, AD-158). Every part of the scope
    // is its own scalar — `scope_kind` plus a nullable `profile_id` and a
    // nullable `subtree` — rather than one serialized scope (AD-139): "which
    // grants touch this profile" is a question Settings asks, a revocation
    // sweep asks, and a migration will ask, and none of them can ask it of a
    // blob.
    //
    // **A revocation sets `revoked_ms` and never deletes the row.** The audit
    // log records the `grant_id` a tool call ran under, and a log whose
    // authority column dangles cannot answer the one question it exists for.
    // Every read that decides anything filters on `revoked_ms IS NULL`.
    //
    // **These two foreign keys are enforced, and they cascade.** The comment
    // on [`open`] above inherited "rusqlite leaves `PRAGMA foreign_keys` off"
    // from the account registry, and on this build that is not true:
    // `libsqlite3-sys`'s bundled SQLite is compiled with
    // `SQLITE_DEFAULT_FOREIGN_KEYS=1`, so an insert naming an absent provider
    // is refused by the database (proved in `tests/bots_grant.rs`). Given that,
    // `ON DELETE CASCADE` is the only spelling that keeps
    // [`delete_provider`] and [`delete_bot`] working — and it is also the
    // behaviour the feature wants: a permission for a provider the user
    // removed must not survive it, and must not block its removal. The
    // "revoked rows stay" rule is about revocation, which is a state on a live
    // provider's grant; deleting the provider deletes the thing the grant was
    // about. The audit log stays readable either way, because it stores the
    // provider id and the path as text rather than only a `grant_id`.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS bot_grants(\
            id TEXT PRIMARY KEY, \
            provider_id TEXT NOT NULL REFERENCES bot_providers(id) ON DELETE CASCADE, \
            bot_id TEXT REFERENCES bots(id) ON DELETE CASCADE, \
            scope_kind TEXT NOT NULL, \
            profile_id TEXT, \
            subtree TEXT, \
            mode TEXT NOT NULL, \
            created_ms INTEGER NOT NULL, \
            revoked_ms INTEGER\
        )",
        [],
    )
    .map_err(|e| CoreError::Internal(format!("could not ensure bot_grants schema: {e}")))?;
    // The index the per-call check reads through. Every tool call runs
    // `list_grants_for_bot`, so this is the one query in the module whose cost
    // is paid per model action rather than per user action.
    conn.execute(
        "CREATE INDEX IF NOT EXISTS bot_grants_provider \
         ON bot_grants(provider_id, revoked_ms)",
        [],
    )
    .map_err(|e| CoreError::Internal(format!("could not ensure bot_grants index: {e}")))?;
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

/// Delete a provider and every bot that belongs to it, atomically (Story 61.1,
/// FR-379).
///
/// Idempotent — deleting a missing provider is not an error, so this is safe on
/// the rollback path of a half-finished add.
///
/// One `BEGIN IMMEDIATE` transaction, not two statements: a failure between
/// them would leave bots whose provider is gone, and every one of those rows
/// would then be a bot the user can select and keeper cannot call.
///
/// **The credential is not deleted here.** It lives behind the secret port,
/// which this crate reaches only through [`crate::platform::Platform`] — so the
/// caller pairs this with [`crate::bots::delete_provider_token`]. Splitting it
/// is deliberate: a database function that quietly needed a platform port would
/// make every test of this table need one too.
pub fn delete_provider(data_dir: &Path, provider_id: &str) -> Result<(), CoreError> {
    let mut conn = open(data_dir)?;
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| CoreError::Internal(format!("could not begin provider delete: {e}")))?;
    tx.execute(
        "DELETE FROM bots WHERE provider_id = ?1",
        rusqlite::params![provider_id],
    )
    .map_err(|e| CoreError::Internal(format!("could not delete provider's bots: {e}")))?;
    tx.execute(
        "DELETE FROM bot_providers WHERE id = ?1",
        rusqlite::params![provider_id],
    )
    .map_err(|e| CoreError::Internal(format!("could not delete provider: {e}")))?;
    tx.commit()
        .map_err(|e| CoreError::Internal(format!("could not commit provider delete: {e}")))?;
    Ok(())
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
            state: super::BotHealthState::from_registry_str(&health_state),
            checked_ms: r.get(6)?,
            detail: r.get(7)?,
        },
        read_timeout_ms: r.get(8)?,
    }))
}

/// The `SELECT` column list every bot read shares.
const BOT_COLUMNS: &str = "id, provider_id, target, name, pin_order, identity_shape, \
     identity_colour, identity_mark, created_ms";

/// The last line that keeps an unvalidated target out of a request URL (Story
/// 61.1, FR-376).
///
/// The user-facing refusal is [`super::parse_bot_target`], called by the
/// surface so the person typing gets the sentence that names what was wrong.
/// This guard exists because that is a *convention* and this is a *door*: for a
/// Hermes bot the target becomes a path segment in every request
/// (`/p/{target}`), so a `/` or a `?` that reaches the table would retarget
/// every call made through it — and the row would outlive whichever caller
/// forgot to validate. `Internal` is the right variant precisely because
/// reaching here with a bad target is an invariant violation and not user
/// input: the input was refused two layers up.
fn ensure_valid_target(target: &str) -> Result<(), CoreError> {
    super::parse_bot_target(target).map(|_| ()).map_err(|e| {
        CoreError::Internal(format!(
            "a bot target that never passed the grammar reached the store: {e}"
        ))
    })
}

/// Insert one bot row (Story 61.1, FR-383).
///
/// Fails when `id` already exists, and when `(provider_id, target)` is already
/// pinned — the second is the useful one: pinning the same bot twice is one bot
/// and a duplicate, and the UNIQUE index is what makes the duplicate
/// impossible rather than merely unlikely. Also fails when `target` never
/// passed the grammar — see [`ensure_valid_target`].
pub fn insert_bot(data_dir: &Path, bot: &Bot) -> Result<(), CoreError> {
    ensure_valid_target(&bot.target)?;
    let conn = open(data_dir)?;
    conn.execute(
        "INSERT INTO bots(id, provider_id, target, name, pin_order, identity_shape, \
             identity_colour, identity_mark, created_ms) \
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
        rusqlite::params![
            bot.id,
            bot.provider_id,
            bot.target,
            bot.name,
            bot.pin_order,
            bot.identity.shape,
            bot.identity.colour,
            bot.identity.mark,
            bot.created_ms,
        ],
    )
    .map_err(|e| CoreError::Internal(format!("could not insert bot: {e}")))?;
    Ok(())
}

/// Rewrite a bot's target and display name by id (Story 61.1). Returns whether
/// a row matched, for [`update_provider`]'s reason, and refuses a target that
/// never passed the grammar, for [`ensure_valid_target`]'s reason — an edit is
/// the other door into the same column.
pub fn update_bot(
    data_dir: &Path,
    bot_id: &str,
    target: &str,
    name: &str,
) -> Result<bool, CoreError> {
    ensure_valid_target(target)?;
    let conn = open(data_dir)?;
    let changed = conn
        .execute(
            "UPDATE bots SET target = ?2, name = ?3 WHERE id = ?1",
            rusqlite::params![bot_id, target, name],
        )
        .map_err(|e| CoreError::Internal(format!("could not update bot: {e}")))?;
    Ok(changed > 0)
}

/// Write a bot's chosen identity (Story 61.1, FR-383). Returns whether a row
/// matched.
///
/// All three columns every time, including the `None`s: a picker that clears a
/// colour must be able to clear it, and a setter that only ever wrote `Some`
/// would make "no colour" unreachable once a colour had been chosen.
pub fn set_bot_identity(
    data_dir: &Path,
    bot_id: &str,
    identity: &BotIdentity,
) -> Result<bool, CoreError> {
    let conn = open(data_dir)?;
    let changed = conn
        .execute(
            "UPDATE bots SET identity_shape = ?2, identity_colour = ?3, identity_mark = ?4 \
             WHERE id = ?1",
            rusqlite::params![bot_id, identity.shape, identity.colour, identity.mark],
        )
        .map_err(|e| CoreError::Internal(format!("could not write bot identity: {e}")))?;
    Ok(changed > 0)
}

/// Rewrite the whole pin order to exactly `order` — `order[i]` gets `pin_order`
/// `i` — in ONE connection and ONE `BEGIN IMMEDIATE` transaction (Story 61.1,
/// FR-383).
///
/// Carried verbatim from `registry::reorder_pins`, including why: N independent
/// writes each commit on their own, so a failure (or a process kill) partway
/// through leaves the persisted sequence half-rewritten — duplicated or gapped
/// orders that no longer describe any order the user asked for. Here the
/// rewrite commits as a unit or rolls back entirely.
///
/// An id that names no bot is skipped rather than inserted: unlike a pin, a bot
/// row carries a provider, a target and a creation time that a reorder has no
/// way to invent.
pub fn reorder_bots(data_dir: &Path, order: &[String]) -> Result<(), CoreError> {
    let mut conn = open(data_dir)?;
    let tx = conn
        .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
        .map_err(|e| CoreError::Internal(format!("could not begin bot reorder: {e}")))?;
    {
        let mut stmt = tx
            .prepare("UPDATE bots SET pin_order = ?2 WHERE id = ?1")
            .map_err(|e| CoreError::Internal(format!("could not prepare bot reorder: {e}")))?;
        for (index, bot_id) in order.iter().enumerate() {
            stmt.execute(rusqlite::params![bot_id, index as i64])
                .map_err(|e| CoreError::Internal(format!("could not reorder bot: {e}")))?;
        }
    }
    tx.commit()
        .map_err(|e| CoreError::Internal(format!("could not commit bot reorder: {e}")))?;
    Ok(())
}

/// Delete one bot by id (Story 61.1). Idempotent — deleting an absent bot is
/// not an error.
///
/// The bot's own credential, when it has one, is the caller's to delete through
/// [`crate::bots::delete_bot_token`], for [`delete_provider`]'s reason.
pub fn delete_bot(data_dir: &Path, bot_id: &str) -> Result<(), CoreError> {
    let conn = open(data_dir)?;
    conn.execute("DELETE FROM bots WHERE id = ?1", rusqlite::params![bot_id])
        .map_err(|e| CoreError::Internal(format!("could not delete bot: {e}")))?;
    Ok(())
}

/// Read one bot by id, or `None` when there is none (Story 61.1).
pub fn get_bot(data_dir: &Path, bot_id: &str) -> Result<Option<Bot>, CoreError> {
    let conn = open(data_dir)?;
    let mut stmt = conn
        .prepare(&format!("SELECT {BOT_COLUMNS} FROM bots WHERE id = ?1"))
        .map_err(|e| CoreError::Internal(format!("could not prepare bot query: {e}")))?;
    let mut rows = stmt
        .query(rusqlite::params![bot_id])
        .map_err(|e| CoreError::Internal(format!("could not query bot: {e}")))?;
    let Some(row) = rows
        .next()
        .map_err(|e| CoreError::Internal(format!("could not read bot row: {e}")))?
    else {
        return Ok(None);
    };
    Ok(Some(map_bot_row(row).map_err(|e| {
        CoreError::Internal(format!("could not map bot row: {e}"))
    })?))
}

/// Every bot, in the user's hand-set order (Story 61.1, FR-383).
///
/// `ORDER BY pin_order, created_ms, id`: the hand order first, then insertion,
/// then the id — so two bots that share a `pin_order` (possible after a partial
/// hand-edit of the database) still list identically on every read rather than
/// swapping places between opens.
///
/// Global across providers, exactly as `registry::get_pins` is global across
/// accounts: the pinned strip is one user-controlled sequence, not one per
/// tenant.
pub fn list_bots(data_dir: &Path) -> Result<Vec<Bot>, CoreError> {
    let conn = open(data_dir)?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {BOT_COLUMNS} FROM bots ORDER BY pin_order ASC, created_ms ASC, id ASC"
        ))
        .map_err(|e| CoreError::Internal(format!("could not prepare bot list: {e}")))?;
    let mapped = stmt
        .query_map([], map_bot_row)
        .map_err(|e| CoreError::Internal(format!("could not query bots: {e}")))?;
    let mut bots = Vec::new();
    for bot in mapped {
        bots.push(bot.map_err(|e| CoreError::Internal(format!("could not read bot: {e}")))?);
    }
    Ok(bots)
}

/// Every bot of one provider, in the same order [`list_bots`] uses (Story
/// 61.1).
pub fn list_bots_for_provider(data_dir: &Path, provider_id: &str) -> Result<Vec<Bot>, CoreError> {
    let conn = open(data_dir)?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {BOT_COLUMNS} FROM bots WHERE provider_id = ?1 \
             ORDER BY pin_order ASC, created_ms ASC, id ASC"
        ))
        .map_err(|e| CoreError::Internal(format!("could not prepare bot list: {e}")))?;
    let mapped = stmt
        .query_map(rusqlite::params![provider_id], map_bot_row)
        .map_err(|e| CoreError::Internal(format!("could not query bots: {e}")))?;
    let mut bots = Vec::new();
    for bot in mapped {
        bots.push(bot.map_err(|e| CoreError::Internal(format!("could not read bot: {e}")))?);
    }
    Ok(bots)
}

/// Map a `SELECT BOT_COLUMNS` row into a [`Bot`]. Shared by every bot query, so
/// the column order the mapper assumes is written exactly once.
fn map_bot_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Bot> {
    Ok(Bot {
        id: r.get(0)?,
        provider_id: r.get(1)?,
        target: r.get(2)?,
        name: r.get(3)?,
        pin_order: r.get(4)?,
        identity: BotIdentity {
            shape: r.get(5)?,
            colour: r.get(6)?,
            mark: r.get(7)?,
        },
        created_ms: r.get(8)?,
    })
}

/// The `SELECT` column list every grant read shares, so the column order the
/// mapper assumes is written exactly once.
const GRANT_COLUMNS: &str = "id, provider_id, bot_id, scope_kind, profile_id, subtree, \
     mode, created_ms, revoked_ms";

/// A grant row as stored: the grant, plus whether it has been revoked (Story
/// 61.10, FR-386).
///
/// Split for [`ProviderRow`]'s reason and one stronger one: a revoked grant
/// grants nothing, so it must not *be* a [`Grant`] — the type is what stops a
/// forgotten `revoked_ms` filter from handing a dead permission to
/// [`super::grant::decide`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantRow {
    /// The grant itself.
    pub grant: Grant,
    /// When it was revoked, or `None` while it is live.
    pub revoked_ms: Option<i64>,
}

/// A grant row keeper could read but will not act on (Story 61.10).
///
/// Produced when `scope_kind` or `mode` holds a value this build does not know
/// — a row written by a newer build, or hand-edited. Surfaced rather than
/// skipped, for [`UnknownProviderRow`]'s reason plus the one specific to a
/// permission: a grant the user can see in Settings and keeper silently
/// ignores is a permission they believe they have.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownGrantRow {
    /// The row's id, so the surface can offer to revoke it.
    pub id: String,
    /// The provider it names.
    pub provider_id: String,
    /// The stored `scope_kind`, verbatim.
    pub scope_kind: String,
    /// The stored `mode`, verbatim.
    pub mode: String,
}

/// Everything the `bot_grants` table holds, partitioned by whether this build
/// can act on it (Story 61.10).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GrantListing {
    /// The readable rows, live and revoked alike, in the deterministic order
    /// [`list_grants`] documents.
    pub rows: Vec<GrantRow>,
    /// The rows this build cannot act on, in the same order.
    pub unknown: Vec<UnknownGrantRow>,
}

/// The live and revoked grants speaking for one `(provider, bot)` — exactly
/// what [`super::grant::check`] needs and nothing else (Story 61.10, AD-158).
///
/// Both halves, because "no grant covers this" and "the grant covering this was
/// revoked" are different sentences and a person needs the second one.
/// Unreadable rows appear in neither: they grant nothing, and the surface that
/// discloses them is [`list_grants`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BotGrants {
    /// The grants in force right now.
    pub live: Vec<Grant>,
    /// The grants that were in force and are not.
    pub revoked: Vec<Grant>,
}

/// Reject a grant whose subtree never passed the grammar (Story 61.10).
///
/// [`ensure_valid_target`]'s reason, applied to the other user-typed path in
/// this module: a `..` inside `bot_grants.subtree` would make the stored scope
/// unmatchable against a target — and a scope that cannot be matched is a
/// permission whose boundary nobody can state. `Internal` because the surface
/// refuses the input with [`super::grant::SubpathError`]'s own sentence two
/// layers up.
fn ensure_valid_scope(scope: &GrantScope) -> Result<(), CoreError> {
    let Some(subpath) = scope.subpath() else {
        return Ok(());
    };
    super::grant::parse_subpath(subpath)
        .map_err(|e| {
            CoreError::Internal(format!(
                "a grant subtree that never passed the grammar reached the store: {e}"
            ))
        })
        .and_then(|normalized| {
            if normalized == subpath {
                Ok(())
            } else {
                Err(CoreError::Internal(format!(
                    "a grant subtree reached the store unnormalized: {subpath:?}"
                )))
            }
        })
}

/// Write a grant, creating it or replacing the one with that id (Story 61.10,
/// FR-386).
///
/// Idempotent by id, unlike [`insert_provider`]: the Settings surface edits a
/// grant in place — the mode changes, the subtree is widened — and a person who
/// changes read to write has not created a second permission. An id collision
/// here is therefore the intended edit and not a caller's duplicate.
///
/// **Saving clears `revoked_ms`.** Granting again, on purpose, is what the UI
/// just did; leaving the timestamp would store a grant that is simultaneously
/// present in the list and dead on every check, which is the affordance that
/// lies in its most literal form.
pub fn save_grant(data_dir: &Path, grant: &Grant) -> Result<(), CoreError> {
    ensure_valid_scope(&grant.scope)?;
    let conn = open(data_dir)?;
    conn.execute(
        "INSERT INTO bot_grants(id, provider_id, bot_id, scope_kind, profile_id, subtree, \
             mode, created_ms, revoked_ms) \
         VALUES(?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, NULL) \
         ON CONFLICT(id) DO UPDATE SET \
             provider_id = excluded.provider_id, \
             bot_id = excluded.bot_id, \
             scope_kind = excluded.scope_kind, \
             profile_id = excluded.profile_id, \
             subtree = excluded.subtree, \
             mode = excluded.mode, \
             revoked_ms = NULL",
        rusqlite::params![
            grant.id,
            grant.provider_id,
            grant.bot_id,
            grant.scope.kind_registry_str(),
            grant.scope.profile_id(),
            grant.scope.subpath(),
            grant.mode.as_registry_str(),
            grant.created_ms,
        ],
    )
    .map_err(|e| CoreError::Internal(format!("could not save grant: {e}")))?;
    Ok(())
}

/// Revoke a grant in one act (Story 61.10, FR-386).
///
/// Returns whether a live row matched, so a surface that revokes into nothing
/// says so rather than reporting success (AD-27) — and so a second revocation
/// of the same grant cannot silently move `revoked_ms` forward and misdate the
/// audit log's referent.
///
/// The row survives. `revoked_ms` is set and nothing is deleted, because every
/// audit row that ran under this grant names its id.
pub fn revoke_grant(data_dir: &Path, grant_id: &str, revoked_ms: i64) -> Result<bool, CoreError> {
    let conn = open(data_dir)?;
    let changed = conn
        .execute(
            "UPDATE bot_grants SET revoked_ms = ?2 WHERE id = ?1 AND revoked_ms IS NULL",
            rusqlite::params![grant_id, revoked_ms],
        )
        .map_err(|e| CoreError::Internal(format!("could not revoke grant: {e}")))?;
    Ok(changed > 0)
}

/// Read one grant row by id, or `None` when there is none — or when this build
/// cannot act on it (Story 61.10).
pub fn get_grant(data_dir: &Path, grant_id: &str) -> Result<Option<GrantRow>, CoreError> {
    let conn = open(data_dir)?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {GRANT_COLUMNS} FROM bot_grants WHERE id = ?1"
        ))
        .map_err(|e| CoreError::Internal(format!("could not prepare grant query: {e}")))?;
    let mut rows = stmt
        .query(rusqlite::params![grant_id])
        .map_err(|e| CoreError::Internal(format!("could not query grant: {e}")))?;
    let Some(row) = rows
        .next()
        .map_err(|e| CoreError::Internal(format!("could not read grant row: {e}")))?
    else {
        return Ok(None);
    };
    Ok(map_grant_row(row)
        .map_err(|e| CoreError::Internal(format!("could not map grant row: {e}")))?
        .ok())
}

/// Every grant row, live and revoked, partitioned into what this build can act
/// on and what it cannot (Story 61.10, FR-386).
///
/// This is the list that answers "what can it change?" in one place, so it
/// includes revoked rows: the answer to that question is a list of grants and
/// their state, never a history of clicks.
///
/// Ordered `created_ms, id` — [`list_providers`]'s order and its reason.
pub fn list_grants(data_dir: &Path) -> Result<GrantListing, CoreError> {
    let conn = open(data_dir)?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {GRANT_COLUMNS} FROM bot_grants ORDER BY created_ms, id"
        ))
        .map_err(|e| CoreError::Internal(format!("could not prepare grant list: {e}")))?;
    let mapped = stmt
        .query_map([], map_grant_row)
        .map_err(|e| CoreError::Internal(format!("could not query grants: {e}")))?;
    let mut listing = GrantListing::default();
    for row in mapped {
        match row.map_err(|e| CoreError::Internal(format!("could not read grant row: {e}")))? {
            Ok(readable) => listing.rows.push(readable),
            Err(unknown) => listing.unknown.push(unknown),
        }
    }
    Ok(listing)
}

/// The grants speaking for one `(provider, bot)`, partitioned into live and
/// revoked (Story 61.10, AD-158).
///
/// **The query is the narrowing.** `bot_id IS NULL OR bot_id = ?2` is
/// [`super::grant::Grant::applies_to`] in SQL: a provider-wide grant speaks for
/// every bot, a bot-scoped one only for its own. Doing it here rather than in
/// the caller is what makes it impossible for a tool call to be evaluated
/// against another bot's permissions.
///
/// Called once per tool call, which is the point (`super::grant::check`).
pub fn list_grants_for_bot(
    data_dir: &Path,
    provider_id: &str,
    bot_id: Option<&str>,
) -> Result<BotGrants, CoreError> {
    let conn = open(data_dir)?;
    let mut stmt = conn
        .prepare(&format!(
            "SELECT {GRANT_COLUMNS} FROM bot_grants \
             WHERE provider_id = ?1 AND (bot_id IS NULL OR bot_id = ?2) \
             ORDER BY created_ms, id"
        ))
        .map_err(|e| CoreError::Internal(format!("could not prepare bot grant list: {e}")))?;
    let mapped = stmt
        .query_map(rusqlite::params![provider_id, bot_id], map_grant_row)
        .map_err(|e| CoreError::Internal(format!("could not query bot grants: {e}")))?;
    let mut grants = BotGrants::default();
    for row in mapped {
        let Ok(readable) =
            row.map_err(|e| CoreError::Internal(format!("could not read grant row: {e}")))?
        else {
            // Unreadable rows grant nothing. Disclosed by `list_grants`.
            continue;
        };
        if readable.revoked_ms.is_some() {
            grants.revoked.push(readable.grant);
        } else {
            grants.live.push(readable.grant);
        }
    }
    Ok(grants)
}

/// Map a `SELECT GRANT_COLUMNS` row into a [`GrantRow`], or into an
/// [`UnknownGrantRow`] when the stored `scope_kind` or `mode` is not one this
/// build knows.
///
/// The `Result` inside the `Ok` is the partition, not a failure —
/// [`map_provider_row`]'s convention.
#[allow(clippy::type_complexity)]
fn map_grant_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Result<GrantRow, UnknownGrantRow>> {
    let id: String = r.get(0)?;
    let provider_id: String = r.get(1)?;
    let scope_kind: String = r.get(3)?;
    let mode_text: String = r.get(6)?;
    let scope = GrantScope::from_registry(&scope_kind, r.get(4)?, r.get(5)?);
    let mode = GrantMode::from_registry_str(&mode_text);
    let (Some(scope), Some(mode)) = (scope, mode) else {
        return Ok(Err(UnknownGrantRow {
            id,
            provider_id,
            scope_kind,
            mode: mode_text,
        }));
    };
    Ok(Ok(GrantRow {
        grant: Grant {
            id,
            provider_id,
            bot_id: r.get(2)?,
            scope,
            mode,
            created_ms: r.get(7)?,
        },
        revoked_ms: r.get(8)?,
    }))
}

#[cfg(test)]
mod tests {
    use super::super::BotHealthState;
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
            "keeper-bots-test-{}-{}-{n}",
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

    fn bot(id: &str, provider_id: &str, target: &str, pin_order: i64) -> Bot {
        Bot {
            id: id.to_owned(),
            provider_id: provider_id.to_owned(),
            target: target.to_owned(),
            name: format!("bot {id}"),
            pin_order,
            identity: BotIdentity::default(),
            created_ms: 1_700_000_000_000,
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
            BotHealthState::Unknown,
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
            state: BotHealthState::Unauthorized,
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
    fn a_bot_round_trips_its_chosen_identity_and_can_clear_it() {
        let dir = temp_dir();
        insert_provider(&dir, &provider("p1", ProviderKind::Hermes, 1)).expect("insert");
        let inserted = bot("b1", "p1", "grokbot", 0);
        insert_bot(&dir, &inserted).expect("insert bot");

        let read = get_bot(&dir, "b1").expect("read").expect("row");
        assert_eq!(read, inserted);
        assert!(
            read.identity.is_empty(),
            "a bot has no identity until somebody chooses one"
        );

        let identity = BotIdentity {
            shape: Some("hollow".to_owned()),
            colour: Some("clay".to_owned()),
            mark: Some("flask-conical".to_owned()),
        };
        assert!(set_bot_identity(&dir, "b1", &identity).expect("write identity"));
        assert_eq!(
            get_bot(&dir, "b1").expect("read").expect("row").identity,
            identity
        );

        // And a chosen colour can be un-chosen — a picker that cannot clear is
        // a picker that lies about being optional.
        assert!(set_bot_identity(&dir, "b1", &BotIdentity::default()).expect("clear identity"));
        assert!(get_bot(&dir, "b1")
            .expect("read")
            .expect("row")
            .identity
            .is_empty());
    }

    #[test]
    fn pinning_one_bot_of_one_provider_twice_is_refused() {
        let dir = temp_dir();
        insert_provider(&dir, &provider("p1", ProviderKind::Ollama, 1)).expect("insert");
        insert_provider(&dir, &provider("p2", ProviderKind::Ollama, 2)).expect("insert");
        insert_bot(&dir, &bot("b1", "p1", "llama3.1:8b", 0)).expect("insert bot");

        assert!(
            insert_bot(&dir, &bot("b2", "p1", "llama3.1:8b", 1)).is_err(),
            "the same target on the same provider is one bot, not two"
        );
        // The same target on a DIFFERENT provider is a different bot: two
        // Ollama servers may both hold `llama3.1:8b`.
        insert_bot(&dir, &bot("b3", "p2", "llama3.1:8b", 1)).expect("insert on other provider");
    }

    #[test]
    fn the_hand_order_is_rewritten_as_a_unit_and_lists_back_in_it() {
        let dir = temp_dir();
        insert_provider(&dir, &provider("p1", ProviderKind::Ollama, 1)).expect("insert");
        for (index, target) in ["a", "b", "c"].iter().enumerate() {
            insert_bot(
                &dir,
                &bot(&format!("b{target}"), "p1", target, index as i64),
            )
            .expect("insert bot");
        }

        reorder_bots(&dir, &["bc".to_owned(), "ba".to_owned(), "bb".to_owned()]).expect("reorder");

        let ids: Vec<String> = list_bots(&dir)
            .expect("list")
            .into_iter()
            .map(|b| b.id)
            .collect();
        assert_eq!(ids, vec!["bc", "ba", "bb"]);

        // An id that names no bot is skipped, never inserted: a reorder cannot
        // invent a provider, a target or a creation time.
        reorder_bots(&dir, &["nope".to_owned()]).expect("reorder with a stale id");
        assert_eq!(list_bots(&dir).expect("list").len(), 3);
    }

    /// Story 61.7: the write is guarded by [`crate::bots::identity::plan_reorder`],
    /// and this is the pair working together — a refused plan reaches the
    /// transaction not at all, so the stored order is exactly what it was.
    ///
    /// This is the observable half of the atomicity contract. That the write
    /// commits as a unit under a mid-loop failure is a property of the
    /// `BEGIN IMMEDIATE` transaction and cannot be provoked from a test without
    /// a fault-injecting VFS; what CAN be pinned is that nothing partial is
    /// ever submitted to it, and that a reorder which is submitted lands whole.
    #[test]
    fn a_refused_reorder_plan_never_reaches_the_write() {
        let dir = temp_dir();
        insert_provider(&dir, &provider("p1", ProviderKind::Ollama, 1)).expect("insert");
        for (index, target) in ["a", "b", "c"].iter().enumerate() {
            insert_bot(
                &dir,
                &bot(&format!("b{target}"), "p1", target, index as i64),
            )
            .expect("insert bot");
        }
        let known: Vec<String> = list_bots(&dir)
            .expect("list")
            .into_iter()
            .map(|b| b.id)
            .collect();

        // A partial order — the pins strip's own defect class, submitted while
        // a filter hid half the set.
        let refused = crate::bots::identity::plan_reorder(&known, &["bc".to_owned()]);
        assert!(refused.is_err(), "a partial order is refused");

        let after: Vec<String> = list_bots(&dir)
            .expect("list")
            .into_iter()
            .map(|b| b.id)
            .collect();
        assert_eq!(after, known, "a refused reorder writes nothing at all");

        // And the accepted one lands whole: each id at its named position, with
        // a contiguous sequence and no duplicate. The pairs matter rather than
        // the bare positions — a rewrite that rolled back would still list
        // `[0, 1, 2]`, just attached to the bots that had them before, which is
        // exactly the "committed nothing" failure this pair of tests is for.
        let plan = crate::bots::identity::plan_reorder(
            &known,
            &["bc".to_owned(), "bb".to_owned(), "ba".to_owned()],
        )
        .expect("a permutation");
        reorder_bots(&dir, &plan).expect("reorder");
        let orders: Vec<(String, i64)> = list_bots(&dir)
            .expect("list")
            .into_iter()
            .map(|b| (b.id, b.pin_order))
            .collect();
        assert_eq!(
            orders,
            vec![
                ("bc".to_owned(), 0),
                ("bb".to_owned(), 1),
                ("ba".to_owned(), 2)
            ]
        );
    }

    /// The transaction's own contract, provoked deterministically —
    /// `registry::a_reorder_that_fails_partway_leaves_no_position_rewritten`'s
    /// technique, because this rewrite is that one carried over and it deserves
    /// the same proof rather than the same comment.
    #[test]
    fn a_reorder_that_fails_partway_leaves_no_pin_order_rewritten() {
        let dir = temp_dir();
        insert_provider(&dir, &provider("p1", ProviderKind::Ollama, 1)).expect("insert");
        for (index, target) in ["a", "b", "c"].iter().enumerate() {
            insert_bot(
                &dir,
                &bot(&format!("b{target}"), "p1", target, index as i64),
            )
            .expect("insert bot");
        }

        // Abort the rewrite on its LAST row, standing in for the disk error or
        // the kill this transaction exists for: by then the first two rows have
        // been written, so a sequence that committed row-by-row would leave
        // `bc` at 0 beside `ba` still at 0 — a duplicated order describing
        // nothing anybody asked for.
        let conn = open(&dir).expect("open to arm the failure");
        conn.execute_batch(
            "CREATE TRIGGER reorder_fails BEFORE UPDATE ON bots \
             WHEN NEW.pin_order = 2 \
             BEGIN SELECT RAISE(ABORT, 'injected reorder failure'); END",
        )
        .expect("arm the failure");
        drop(conn);

        let outcome = reorder_bots(&dir, &["bc".to_owned(), "bb".to_owned(), "ba".to_owned()]);
        assert!(
            outcome.is_err(),
            "a rewrite that could not complete must be reported, never swallowed"
        );
        let orders: Vec<(String, i64)> = list_bots(&dir)
            .expect("list after the failed reorder")
            .into_iter()
            .map(|b| (b.id, b.pin_order))
            .collect();
        assert_eq!(
            orders,
            vec![
                ("ba".to_owned(), 0),
                ("bb".to_owned(), 1),
                ("bc".to_owned(), 2)
            ],
            "the previous order must survive whole — no half-applied positions"
        );
    }

    #[test]
    fn deleting_a_provider_takes_its_bots_and_leaves_every_other_row() {
        let dir = temp_dir();
        insert_provider(&dir, &provider("p1", ProviderKind::Hermes, 1)).expect("insert");
        insert_provider(&dir, &provider("p2", ProviderKind::Ollama, 2)).expect("insert");
        insert_bot(&dir, &bot("b1", "p1", "grokbot", 0)).expect("insert bot");
        insert_bot(&dir, &bot("b2", "p1", "hermes", 1)).expect("insert bot");
        insert_bot(&dir, &bot("b3", "p2", "llama3.1:8b", 2)).expect("insert bot");

        delete_provider(&dir, "p1").expect("delete");

        assert!(get_provider(&dir, "p1").expect("read").is_none());
        assert!(
            get_provider(&dir, "p2").expect("read").is_some(),
            "deleting one provider must not touch another"
        );
        let remaining: Vec<String> = list_bots(&dir)
            .expect("list")
            .into_iter()
            .map(|b| b.id)
            .collect();
        assert_eq!(
            remaining,
            vec!["b3"],
            "a bot whose provider is gone is a bot keeper could never call"
        );

        // Idempotent: the rollback path of a half-finished add calls this on a
        // provider that may never have been written.
        delete_provider(&dir, "p1").expect("second delete is not an error");
        delete_bot(&dir, "b1").expect("deleting an absent bot is not an error");
    }

    #[test]
    fn bots_can_be_listed_per_provider() {
        let dir = temp_dir();
        insert_provider(&dir, &provider("p1", ProviderKind::Hermes, 1)).expect("insert");
        insert_provider(&dir, &provider("p2", ProviderKind::Ollama, 2)).expect("insert");
        insert_bot(&dir, &bot("b1", "p1", "grokbot", 1)).expect("insert bot");
        insert_bot(&dir, &bot("b2", "p2", "llama3.1:8b", 0)).expect("insert bot");

        let ids: Vec<String> = list_bots_for_provider(&dir, "p1")
            .expect("list")
            .into_iter()
            .map(|b| b.id)
            .collect();
        assert_eq!(ids, vec!["b1"]);
    }

    #[test]
    fn a_bot_edit_reports_whether_it_landed() {
        let dir = temp_dir();
        insert_provider(&dir, &provider("p1", ProviderKind::Ollama, 1)).expect("insert");
        insert_bot(&dir, &bot("b1", "p1", "llama3.1:8b", 0)).expect("insert bot");

        assert!(update_bot(&dir, "b1", "qwen2.5:7b", "coder").expect("update"));
        let read = get_bot(&dir, "b1").expect("read").expect("row");
        assert_eq!(read.target, "qwen2.5:7b");
        assert_eq!(read.name, "coder");

        assert!(!update_bot(&dir, "nope", "x", "y").expect("update"));
    }

    /// The store is the last line, not the first: a target that never passed
    /// the grammar cannot be written by EITHER door, so no row can exist whose
    /// target would retarget a Hermes request URL.
    #[test]
    fn a_target_that_never_passed_the_grammar_cannot_be_stored() {
        let dir = temp_dir();
        insert_provider(&dir, &provider("p1", ProviderKind::Hermes, 1)).expect("insert");

        for hostile in ["../v1/models", "team/bot", "bot?x=1", "", "   "] {
            assert!(
                insert_bot(&dir, &bot("bx", "p1", hostile, 0)).is_err(),
                "insert_bot must refuse the target {hostile:?}"
            );
        }
        assert!(
            list_bots(&dir).expect("list").is_empty(),
            "a refused insert must leave no row behind"
        );

        insert_bot(&dir, &bot("b1", "p1", "grokbot", 0)).expect("a legal target inserts");
        assert!(
            update_bot(&dir, "b1", "../v1/models", "sneaky").is_err(),
            "an edit is the other door into the same column"
        );
        assert_eq!(
            get_bot(&dir, "b1").expect("read").expect("row").target,
            "grokbot",
            "a refused edit must not have changed the stored target"
        );
    }

    /// Reopening the same data directory must not migrate, duplicate or drop
    /// anything: every `CREATE TABLE IF NOT EXISTS` runs again on each call, so
    /// the tenth open of a populated database is the first open plus nothing.
    #[test]
    fn reopening_the_database_is_idempotent_and_keeps_every_row() {
        let dir = temp_dir();
        insert_provider(&dir, &provider("p1", ProviderKind::Hermes, 1)).expect("insert");
        insert_bot(&dir, &bot("b1", "p1", "grokbot", 0)).expect("insert bot");
        set_provider_health(
            &dir,
            "p1",
            &ProviderHealth {
                state: BotHealthState::Reachable,
                checked_ms: Some(42),
                detail: None,
            },
        )
        .expect("write health");

        let before = (
            list_providers(&dir).expect("list"),
            list_bots(&dir).expect("list"),
        );
        for _ in 0..5 {
            drop(open(&dir).expect("reopen"));
        }
        let after = (
            list_providers(&dir).expect("list"),
            list_bots(&dir).expect("list"),
        );
        assert_eq!(before, after);
        assert_eq!(after.0.rows.len(), 1);
        assert_eq!(after.0.rows[0].health.state, BotHealthState::Reachable);
        assert_eq!(after.1.len(), 1);
    }

    /// The tables live in the same file as the account registry, and neither
    /// opener disturbs the other's schema — the whole reason two idempotent
    /// openers are allowed.
    #[test]
    fn the_bots_tables_share_keeper_db_with_the_account_registry() {
        let dir = temp_dir();
        crate::registry::set_setting(&dir, "incognito.global", "1").expect("registry write");
        insert_provider(&dir, &provider("p1", ProviderKind::Hermes, 1)).expect("insert");

        assert_eq!(
            crate::registry::get_setting(&dir, "incognito.global").expect("registry read"),
            Some("1".to_owned()),
            "opening the bots schema must not disturb the registry's own tables"
        );
        assert_eq!(list_providers(&dir).expect("list").rows.len(), 1);
        assert!(
            dir.join("keeper.db").exists(),
            "both openers must be talking to keeper.db and not to a second file"
        );
    }
}
