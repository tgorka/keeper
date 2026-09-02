//! The tool-call audit log (Story 61.10, FR-388, NFR-47, AD-158).
//!
//! One row per tool call that a grant was consulted about — allowed, asked or
//! refused — in `keeper.db` beside the grants themselves.
//!
//! # The one property this module is built around
//!
//! **The row is written and committed BEFORE the effect** (NFR-47). It is
//! appended as an *intent* with `outcome = 'pending'` and no `finished_ms`,
//! the tool body runs, and then [`complete`] writes what happened. So a crash
//! between the two leaves a pending row that says a write was about to happen
//! to a named path under a named grant — which is the only version of this log
//! worth keeping. The other order produces a log that is complete about every
//! call except the one that broke the machine.
//!
//! This is why [`append_intent`] returns the row id and why the two halves are
//! two functions: a single `record(call, outcome)` cannot be written before its
//! own outcome exists, and every implementation that tries ends up writing
//! after the effect.
//!
//! Durability is `keeper.db`'s: WAL, one connection per call, opened and
//! dropped inside one synchronous scope, with the same busy timeout the rest of
//! the registry uses (`registry.rs:33,84`). `append_intent` returns after its
//! statement has committed, so "the row exists" is true at the moment the tool
//! body starts.
//!
//! # Its reader is a human
//!
//! So the row carries **the path as text**, not a profile id plus a
//! profile-relative subpath the reader has to join, and not an internal
//! identifier at all where a name exists. This is the mistake Anthropic's own
//! telemetry documents in the opposite direction: Claude Code redacts
//! `file_path` unless `OTEL_LOG_TOOL_DETAILS=1`, so the default audit trail
//! records that a file was edited without recording which (R6 §8.1). A log
//! that cannot answer "what did it touch" is not an audit trail. The
//! profile id and the subpath are kept *as well*, because a filter needs them.
//!
//! # What this log is not
//!
//! It is not a rollback. Claude Code's checkpointing is the fully specified
//! implementation of that idea, and its own documented limits — bash-modified
//! files untracked, subagent edits unrestorable, symlinks skipped (R6 §8.2) —
//! are why keeper does not claim one here: the drive's history is git, which
//! the sync engine already keeps, and a second half-truth about recoverability
//! would be worse than none.

use std::path::Path;

use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

use super::grant::{Effect, GrantVerdict, ToolTarget};
use crate::error::{CoreError, PlatformError};

/// How long a contended writer waits for the write lock — [`super::store`]'s
/// timeout, for its reason.
const BUSY_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

/// The most rows one read of the log returns.
///
/// A cap rather than a page, because the surface this feeds is "what has it
/// touched lately" and not a general query tool; a conversation that made ten
/// thousand tool calls is a conversation whose log is read newest-first.
pub const MAX_AUDIT_ROWS: u32 = 500;

/// Open `keeper.db` in WAL mode, ensuring the audit schema exists (Story
/// 61.10).
///
/// Its own opener beside [`super::store`]'s, for the reason that module states:
/// two idempotent openers on one file is exactly what WAL is for, and the
/// alternative is one function every later story edits.
fn open(data_dir: &Path) -> Result<Connection, CoreError> {
    std::fs::create_dir_all(data_dir).map_err(|e| {
        CoreError::Platform(PlatformError::DirUnavailable(format!(
            "could not create data dir: {e}"
        )))
    })?;
    let conn = Connection::open(data_dir.join("keeper.db"))
        .map_err(|e| CoreError::Internal(format!("could not open keeper.db: {e}")))?;
    conn.busy_timeout(BUSY_TIMEOUT)
        .map_err(|e| CoreError::Internal(format!("could not set busy timeout: {e}")))?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|e| CoreError::Internal(format!("could not set WAL mode: {e}")))?;
    // One row per tool call (Story 61.10, FR-388, NFR-47). Every column is a
    // scalar (AD-139): the reader filters by session, by tool, by verdict and
    // by outcome, and a serialized call record would make all four
    // unfilterable at once.
    //
    // `display_path` is the human's column and `profile_id` + `subpath` are the
    // filter's. Both, deliberately: joining two ids to read a log entry is how
    // a log stops being read.
    //
    // `outcome` starts at `pending` and `finished_ms` starts NULL — that pair
    // IS the crash evidence, so neither has a non-null default.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS bot_audit(\
            id INTEGER PRIMARY KEY AUTOINCREMENT, \
            started_ms INTEGER NOT NULL, \
            finished_ms INTEGER, \
            provider_id TEXT NOT NULL, \
            bot_id TEXT, \
            session_id TEXT NOT NULL, \
            message_id TEXT, \
            tool TEXT NOT NULL, \
            profile_id TEXT NOT NULL, \
            subpath TEXT NOT NULL, \
            display_path TEXT NOT NULL, \
            effect TEXT NOT NULL, \
            verdict TEXT NOT NULL, \
            reason TEXT, \
            grant_id TEXT, \
            outcome TEXT NOT NULL, \
            bytes INTEGER, \
            truncated INTEGER NOT NULL\
        )",
        [],
    )
    .map_err(|e| CoreError::Internal(format!("could not ensure bot_audit schema: {e}")))?;
    // Newest-first within one conversation is the only order this log is read
    // in, and `id DESC` is that order — the rowid is monotonic, so it sorts
    // correctly even for rows sharing a millisecond.
    conn.execute(
        "CREATE INDEX IF NOT EXISTS bot_audit_session ON bot_audit(session_id, id DESC)",
        [],
    )
    .map_err(|e| CoreError::Internal(format!("could not ensure bot_audit index: {e}")))?;
    Ok(conn)
}

/// What the grant check concluded, as the log stores it (Story 61.10, FR-388).
///
/// Three arms mirroring [`GrantVerdict`], without its payloads: the grant id
/// and the sentence are their own columns, so a reader can count refusals
/// without parsing prose.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AuditVerdict {
    /// A grant permitted it outright.
    Allow,
    /// A person was asked.
    Ask,
    /// Refused.
    Deny,
}

impl AuditVerdict {
    /// The verdict the log records for a decision (Story 61.10).
    pub fn of(verdict: &GrantVerdict) -> Self {
        match verdict {
            GrantVerdict::Allow { .. } => AuditVerdict::Allow,
            GrantVerdict::Ask { .. } => AuditVerdict::Ask,
            GrantVerdict::Deny { .. } => AuditVerdict::Deny,
        }
    }

    /// The string stored in `bot_audit.verdict` and sent over IPC.
    pub fn as_registry_str(&self) -> &'static str {
        match self {
            AuditVerdict::Allow => "allow",
            AuditVerdict::Ask => "ask",
            AuditVerdict::Deny => "deny",
        }
    }

    /// Parse a stored `verdict`, or `None` for a value this build cannot read.
    ///
    /// `None` rather than a default: a log line whose verdict keeper invented
    /// is worse than one it admits it cannot read.
    pub fn from_registry_str(value: &str) -> Option<Self> {
        match value {
            "allow" => Some(AuditVerdict::Allow),
            "ask" => Some(AuditVerdict::Ask),
            "deny" => Some(AuditVerdict::Deny),
            _ => None,
        }
    }
}

/// What became of a tool call (Story 61.10, FR-388, NFR-47).
///
/// [`AuditOutcome::Pending`] is the state every row is born in and the one that
/// carries the information: a pending row that never completed is a call that
/// was about to happen when the process stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub enum AuditOutcome {
    /// The intent was recorded and no outcome has been written — either the
    /// call is running, or the process stopped while it was.
    Pending,
    /// The tool ran and did what it said.
    Ok,
    /// The tool did not run: the grant refused it, or a person declined.
    Refused,
    /// The tool ran and failed.
    Failed,
}

impl AuditOutcome {
    /// The string stored in `bot_audit.outcome` and sent over IPC.
    pub fn as_registry_str(&self) -> &'static str {
        match self {
            AuditOutcome::Pending => "pending",
            AuditOutcome::Ok => "ok",
            AuditOutcome::Refused => "refused",
            AuditOutcome::Failed => "failed",
        }
    }

    /// Parse a stored `outcome`, defaulting to [`AuditOutcome::Pending`].
    ///
    /// Total, unlike [`AuditVerdict::from_registry_str`]: a row whose outcome
    /// keeper cannot read is a row whose outcome keeper does not know, and
    /// `pending` is exactly that statement. It is also the safe direction —
    /// the honest reading of an unreadable outcome is "this may have been left
    /// half-done", never "this finished".
    pub fn from_registry_str(value: &str) -> Self {
        match value {
            "ok" => AuditOutcome::Ok,
            "refused" => AuditOutcome::Refused,
            "failed" => AuditOutcome::Failed,
            _ => AuditOutcome::Pending,
        }
    }
}

/// Everything known about a tool call **before** it runs (Story 61.10,
/// NFR-47).
///
/// Assembled from the call and the verdict, and that is all there is: no field
/// here can be filled in only afterwards, which is what makes writing the row
/// first possible.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditIntent<'a> {
    /// When the call was about to start, ms since the Unix epoch (UTC).
    pub started_ms: i64,
    /// Which provider.
    pub provider_id: &'a str,
    /// Which bot, or `None` where the call was not made on a bot's behalf.
    pub bot_id: Option<&'a str>,
    /// Which conversation.
    pub session_id: &'a str,
    /// Which assistant message asked for it, where one is known yet.
    pub message_id: Option<&'a str>,
    /// The tool's name, as the model called it.
    pub tool: &'a str,
    /// What it wanted to touch.
    pub target: &'a ToolTarget,
    /// Read or write.
    pub effect: Effect,
    /// The verdict the grant check reached.
    pub verdict: &'a GrantVerdict,
}

/// Append the intent row and commit it, returning its id (Story 61.10,
/// NFR-47).
///
/// **Call this before the effect.** The row lands with
/// [`AuditOutcome::Pending`] and no `finished_ms`; [`complete`] closes it. The
/// returned id is the handle for that second write and is the reason this
/// returns anything at all.
///
/// The grant id and the refusal sentence come off the verdict rather than being
/// passed separately, so the log cannot claim an authority the decision did not
/// name.
pub fn append_intent(data_dir: &Path, intent: &AuditIntent<'_>) -> Result<i64, CoreError> {
    let (grant_id, reason) = match intent.verdict {
        GrantVerdict::Allow { grant_id } => (Some(grant_id.as_str()), None),
        GrantVerdict::Ask { grant_id, reason } => (Some(grant_id.as_str()), Some(*reason)),
        GrantVerdict::Deny { reason } => (None, Some(reason.as_str())),
    };
    let conn = open(data_dir)?;
    conn.execute(
        "INSERT INTO bot_audit(started_ms, finished_ms, provider_id, bot_id, session_id, \
             message_id, tool, profile_id, subpath, display_path, effect, verdict, reason, \
             grant_id, outcome, bytes, truncated) \
         VALUES(?1, NULL, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, NULL, 0)",
        rusqlite::params![
            intent.started_ms,
            intent.provider_id,
            intent.bot_id,
            intent.session_id,
            intent.message_id,
            intent.tool,
            intent.target.profile_id,
            intent.target.subpath,
            intent.target.display_path(),
            intent.effect.as_registry_str(),
            AuditVerdict::of(intent.verdict).as_registry_str(),
            reason,
            grant_id,
            AuditOutcome::Pending.as_registry_str(),
        ],
    )
    .map_err(|e| CoreError::Internal(format!("could not append audit intent: {e}")))?;
    Ok(conn.last_insert_rowid())
}

/// Close an audit row with what happened (Story 61.10, FR-388).
///
/// Returns whether a row matched, so a caller closing a row that is not there
/// finds out; a silently discarded outcome would leave a pending row that
/// looks like a crash and was not.
///
/// `bytes` is `None` where nothing was counted — an absent number stays absent
/// rather than becoming a zero, which is this app's standing rule about numbers
/// it did not measure.
pub fn complete(
    data_dir: &Path,
    audit_id: i64,
    outcome: AuditOutcome,
    bytes: Option<i64>,
    truncated: bool,
    finished_ms: i64,
) -> Result<bool, CoreError> {
    let conn = open(data_dir)?;
    let changed = conn
        .execute(
            "UPDATE bot_audit \
             SET outcome = ?2, bytes = ?3, truncated = ?4, finished_ms = ?5 \
             WHERE id = ?1",
            rusqlite::params![
                audit_id,
                outcome.as_registry_str(),
                bytes,
                i64::from(truncated),
                finished_ms,
            ],
        )
        .map_err(|e| CoreError::Internal(format!("could not complete audit row: {e}")))?;
    Ok(changed > 0)
}

/// One row of the audit log, as stored (Story 61.10, FR-388).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditRow {
    /// The row's id — the handle [`complete`] uses.
    pub id: i64,
    /// When the call was about to start.
    pub started_ms: i64,
    /// When it finished, or `None` while it is pending.
    pub finished_ms: Option<i64>,
    /// Which provider.
    pub provider_id: String,
    /// Which bot, where one was named.
    pub bot_id: Option<String>,
    /// Which conversation.
    pub session_id: String,
    /// Which assistant message asked, where one was known.
    pub message_id: Option<String>,
    /// The tool's name.
    pub tool: String,
    /// The profile — the filter's column.
    pub profile_id: String,
    /// The profile-relative path — the filter's column.
    pub subpath: String,
    /// The path as a person reads it, `profile/sub/path` — this log's own
    /// column, because its reader is a human.
    pub display_path: String,
    /// Read or write. `None` for a stored value this build cannot read.
    pub effect: Option<Effect>,
    /// What the grant check concluded. `None` for a stored value this build
    /// cannot read.
    pub verdict: Option<AuditVerdict>,
    /// The sentence shown to the user, where one was, quoted verbatim from
    /// [`super::grant`]'s consts.
    pub reason: Option<String>,
    /// The grant it ran under, where one permitted it.
    pub grant_id: Option<String>,
    /// What became of it.
    pub outcome: AuditOutcome,
    /// Bytes read or written, where they were counted.
    pub bytes: Option<i64>,
    /// Whether the result was cut short at a cap.
    pub truncated: bool,
}

/// The `SELECT` column list every audit read shares.
const AUDIT_COLUMNS: &str = "id, started_ms, finished_ms, provider_id, bot_id, session_id, \
     message_id, tool, profile_id, subpath, display_path, effect, verdict, reason, grant_id, \
     outcome, bytes, truncated";

/// Read the audit log newest-first, optionally for one conversation (Story
/// 61.10, FR-388).
///
/// `limit` is clamped to [`MAX_AUDIT_ROWS`]; `None` takes the cap.
pub fn list_audit(
    data_dir: &Path,
    session_id: Option<&str>,
    limit: Option<u32>,
) -> Result<Vec<AuditRow>, CoreError> {
    let limit = limit.unwrap_or(MAX_AUDIT_ROWS).min(MAX_AUDIT_ROWS);
    let conn = open(data_dir)?;
    let sql = match session_id {
        Some(_) => format!(
            "SELECT {AUDIT_COLUMNS} FROM bot_audit WHERE session_id = ?1 \
             ORDER BY id DESC LIMIT ?2"
        ),
        None => format!("SELECT {AUDIT_COLUMNS} FROM bot_audit ORDER BY id DESC LIMIT ?1"),
    };
    let mut stmt = conn
        .prepare(&sql)
        .map_err(|e| CoreError::Internal(format!("could not prepare audit query: {e}")))?;
    let mapped = match session_id {
        Some(session_id) => stmt.query_map(rusqlite::params![session_id, limit], map_audit_row),
        None => stmt.query_map(rusqlite::params![limit], map_audit_row),
    }
    .map_err(|e| CoreError::Internal(format!("could not query audit log: {e}")))?;
    let mut rows = Vec::new();
    for row in mapped {
        rows.push(row.map_err(|e| CoreError::Internal(format!("could not read audit row: {e}")))?);
    }
    Ok(rows)
}

/// Map a `SELECT AUDIT_COLUMNS` row into an [`AuditRow`].
///
/// Unreadable `effect` and `verdict` values become `None` rather than
/// disqualifying the row: an audit line keeper can only partly read is still
/// evidence that a tool touched a named path at a named time, and dropping it
/// would lose exactly the row a newer build wrote.
fn map_audit_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<AuditRow> {
    let effect: String = r.get(11)?;
    let verdict: String = r.get(12)?;
    let outcome: String = r.get(15)?;
    let truncated: i64 = r.get(17)?;
    Ok(AuditRow {
        id: r.get(0)?,
        started_ms: r.get(1)?,
        finished_ms: r.get(2)?,
        provider_id: r.get(3)?,
        bot_id: r.get(4)?,
        session_id: r.get(5)?,
        message_id: r.get(6)?,
        tool: r.get(7)?,
        profile_id: r.get(8)?,
        subpath: r.get(9)?,
        display_path: r.get(10)?,
        effect: Effect::from_registry_str(&effect),
        verdict: AuditVerdict::from_registry_str(&verdict),
        reason: r.get(13)?,
        grant_id: r.get(14)?,
        outcome: AuditOutcome::from_registry_str(&outcome),
        bytes: r.get(16)?,
        truncated: truncated != 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_verdict_round_trips_through_its_stored_string() {
        for verdict in [AuditVerdict::Allow, AuditVerdict::Ask, AuditVerdict::Deny] {
            assert_eq!(
                AuditVerdict::from_registry_str(verdict.as_registry_str()),
                Some(verdict)
            );
        }
        assert_eq!(AuditVerdict::from_registry_str("maybe"), None);
    }

    #[test]
    fn an_unreadable_outcome_reads_as_pending_and_never_as_finished() {
        for outcome in [
            AuditOutcome::Pending,
            AuditOutcome::Ok,
            AuditOutcome::Refused,
            AuditOutcome::Failed,
        ] {
            assert_eq!(
                AuditOutcome::from_registry_str(outcome.as_registry_str()),
                outcome
            );
        }
        assert_eq!(
            AuditOutcome::from_registry_str("rolled-back"),
            AuditOutcome::Pending,
            "an outcome keeper cannot read is an outcome keeper does not know"
        );
    }
}
