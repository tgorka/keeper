//! This device's graph intents in one domain (112.1): the changes a person
//! asked for that the server has not resolved yet, kept outside accepted
//! state so that they survive a restart without ever being read as graph
//! data.
//!
//! One file per server, domain and actor ([`crate::hq::databases::graph_queue`]),
//! beside the replica and never synced: no Syncular table, outbox or
//! migration touches it. It is SQLite on the workspace's one `rusqlite`, in
//! WAL mode with `synchronous=FULL`, and every transition is one
//! transaction, so an interruption leaves each entry in its last persisted
//! state with the same identity and bytes.
//!
//! An entry holds the whole identity tuple (server URL, domain, actor,
//! client, descriptor, contract version, request id, context id), the
//! request's canonical JCS envelope and its digest. A restored entry is
//! re-read from those bytes, never re-minted: the digest is re-derived and a
//! mismatch is corruption (refused, the row kept); a tuple that differs from
//! what the currently opened identity mints is [`State::Suspended`], never
//! sent and never rebound.
//!
//! A terminal outcome is persisted first, with its receipt summary and the
//! request binding; only then, in a separate resumable step, is the payload
//! deleted. A crash between the two leaves a terminal row that still holds
//! its payload, and the next open deletes it before anything else.
//!
//! The file also holds the domain's revocation fence (`graph_fence`, written
//! before any cleanup, [`super::lifecycle`]) and the last successful read
//! authority (`graph_authority`, written only from a successful
//! `read_status`). While a fence exists nothing is enqueued, restored or
//! listed.
//!
//! What a transport answer or a receipt means for an entry is not decided
//! here: callers move entries through [`GraphQueue::mark`],
//! [`GraphQueue::retry`] and [`GraphQueue::resolve`], and the table of
//! allowed transitions refuses everything else.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;

use rusqlite::{params, Connection, OpenFlags, OptionalExtension, TransactionBehavior};
use serde_json::json;

use super::client::{GraphClient, Prepared};
use super::outcome::{GraphOutcome, PendingCause};
use super::protocol::request::{self, Request};
use super::protocol::scope::client_scope;
use super::protocol::value::canonical;
use super::protocol::wire::Authority;
use super::protocol::CONTRACT_VERSION;

/// How long a contended writer waits for the write lock.
const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

/// The file's schema version (`PRAGMA user_version`). Version 2 adds the
/// agent approval binding (`approval_id`, `tool_call_id`, `bound_args`).
const SCHEMA_VERSION: i64 = 2;

/// Where an entry stands. The open states are the ones a send may come
/// from or lead to; the outcome states and the three local terminal states
/// end it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum State {
    /// An agent's `graph_apply`, bound to its approval and waiting for a
    /// person's decision; never sent yet.
    AwaitingApproval,
    /// Persisted, not sent yet.
    Queued,
    /// A send was started; the server may have it.
    Submitting,
    /// Applied by the transport; its receipt is not visible yet.
    PendingVisibility,
    /// The server could not say what became of it (`outcome_unresolved`):
    /// only a same-id replay or `graph.outcome` resolves it.
    Unresolved,
    /// Its persisted identity is not what the opened identity mints: never
    /// sent, never rebound.
    Suspended,
    /// Locked by a revocation: kept, never sent again.
    Disabled,
    Accepted,
    Proposed,
    Conflict,
    Rejected,
    TechnicalRefusal,
    IdempotencyMismatch,
    Unsupported,
    /// Discarded by its caller before any outcome; never sent again.
    Withdrawn,
    /// Ended on this device and never sent: its approval was denied or
    /// expired, or its agent context ended before the send.
    RefusedLocally,
}

const STATES: [State; 16] = [
    State::AwaitingApproval,
    State::Queued,
    State::Submitting,
    State::PendingVisibility,
    State::Unresolved,
    State::Suspended,
    State::Disabled,
    State::Accepted,
    State::Proposed,
    State::Conflict,
    State::Rejected,
    State::TechnicalRefusal,
    State::IdempotencyMismatch,
    State::Unsupported,
    State::Withdrawn,
    State::RefusedLocally,
];

impl State {
    /// The stored name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingApproval => "awaiting_approval",
            Self::Queued => "queued",
            Self::Submitting => "submitting",
            Self::PendingVisibility => "pending_visibility",
            Self::Unresolved => "unresolved",
            Self::Suspended => "suspended",
            Self::Disabled => "disabled",
            Self::Accepted => "accepted",
            Self::Proposed => "proposed",
            Self::Conflict => "conflict",
            Self::Rejected => "rejected",
            Self::TechnicalRefusal => "technical_refusal",
            Self::IdempotencyMismatch => "idempotency_mismatch",
            Self::Unsupported => "unsupported",
            Self::Withdrawn => "withdrawn",
            Self::RefusedLocally => "refused_locally",
        }
    }

    fn parse(name: &str) -> Option<Self> {
        STATES.into_iter().find(|state| state.as_str() == name)
    }

    /// Whether the entry is over: its payload is deleted once this state
    /// is persisted.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Accepted
                | Self::Proposed
                | Self::Conflict
                | Self::Rejected
                | Self::TechnicalRefusal
                | Self::IdempotencyMismatch
                | Self::Unsupported
                | Self::Withdrawn
                | Self::RefusedLocally
        )
    }

    /// A server outcome (every terminal state but the local ones).
    fn is_outcome(self) -> bool {
        self.is_terminal() && !matches!(self, Self::Withdrawn | Self::RefusedLocally)
    }

    /// The states a revocation locks: everything still open.
    fn is_open(self) -> bool {
        !self.is_terminal() && self != Self::Disabled
    }

    /// Never sent: nothing of it can have reached the server.
    #[must_use]
    pub fn never_sent(self) -> bool {
        matches!(self, Self::AwaitingApproval | Self::Queued)
    }

    /// The storage transitions. A revocation locks every open entry and an
    /// identity change suspends it; a locked one is only resolved by its own
    /// receipt or withdrawn; a suspended one is never sent. Only an entry
    /// never sent may end refused on this device.
    fn may_become(self, to: Self) -> bool {
        use State::{
            AwaitingApproval, Disabled, PendingVisibility, Queued, RefusedLocally, Submitting,
            Suspended, Unresolved, Withdrawn,
        };
        if self.is_open() && matches!(to, Disabled | Suspended) && self != to {
            return true;
        }
        match self {
            AwaitingApproval => matches!(to, Submitting | RefusedLocally),
            Queued => matches!(to, Submitting | Withdrawn | RefusedLocally),
            Submitting => matches!(to, PendingVisibility | Unresolved) || to.is_outcome(),
            PendingVisibility => to == Unresolved || to.is_outcome(),
            Unresolved => matches!(to, Submitting | PendingVisibility) || to.is_outcome(),
            Suspended => to == Withdrawn,
            Disabled => to == Withdrawn || to.is_outcome(),
            _ => false,
        }
    }
}

impl std::fmt::Display for State {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Who this file and its entries belong to: the server's canonical URL,
/// the domain (database), and this device's actor and client in it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Binding {
    pub server: String,
    pub domain: String,
    pub actor: String,
    pub client: String,
}

impl Binding {
    /// The identity a replica `source` of `server` was opened under.
    #[must_use]
    pub fn of(server: &crate::hq::server::ServerUrl, source: &crate::hq::sessions::Source) -> Self {
        Self {
            server: server.as_str().to_owned(),
            domain: source.database.clone(),
            actor: source.actor.clone(),
            client: source.client.clone(),
        }
    }
}

/// Why the queue refused.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
pub enum QueueError {
    /// The file could not be read or written; nothing was persisted.
    #[error("storage_unavailable: {0}")]
    Storage(String),
    /// The file belongs to another server, domain or actor.
    #[error("this queue file belongs to another server, domain or actor")]
    ForeignFile,
    /// The request was minted under another domain or client.
    #[error("this request belongs to another domain or client")]
    ForeignRequest,
    /// The domain is fenced after a revocation.
    #[error("forbidden")]
    Fenced,
    #[error("no queued request {0}")]
    Unknown(String),
    /// The entry is not in the state the caller moved it from.
    #[error("{request_id} is {actual}, not {expected}")]
    Moved {
        request_id: String,
        expected: State,
        actual: State,
    },
    #[error("{from} cannot become {to}")]
    Transition { from: State, to: State },
    /// Only a terminal entry's record may be acknowledged.
    #[error("{0} is not resolved yet")]
    Open(String),
    /// An agent entry whose request does not name its approval, carries no
    /// context, or is not a `graph.apply`.
    #[error("approval_binding")]
    Unbound,
    /// A stored value this build cannot read.
    #[error("unreadable queue entry: {0}")]
    Unreadable(String),
}

impl QueueError {
    /// The stable code.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Storage(_) => "storage_unavailable",
            Self::Fenced => "forbidden",
            Self::ForeignFile | Self::ForeignRequest => "foreign_request",
            Self::Unknown(_) => "unknown_request",
            Self::Moved { .. } | Self::Transition { .. } | Self::Open(_) => "invalid_transition",
            Self::Unbound => "approval_binding",
            Self::Unreadable(_) => "unreadable",
        }
    }
}

fn storage(error: rusqlite::Error) -> QueueError {
    QueueError::Storage(error.to_string())
}

/// What a terminal outcome leaves behind: its state and the receipt
/// summary (ids and a code, never receipt detail).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Resolution {
    pub event_id: Option<String>,
    pub graph_seq: Option<String>,
    pub code: Option<String>,
}

/// The terminal state and receipt summary a definite outcome persists, or
/// `None` while the request is not resolved (`pending_visibility`, or
/// `pending`, which leaves the entry where it is under its own id). A
/// receipt's detail is never kept.
#[must_use]
pub fn terminal(outcome: &super::outcome::GraphOutcome) -> Option<(State, Resolution)> {
    let only = |code: Option<String>| Resolution {
        code,
        ..Resolution::default()
    };
    Some(match outcome {
        GraphOutcome::Accepted {
            event_id,
            graph_seq,
            ..
        } => (
            State::Accepted,
            Resolution {
                event_id: Some(event_id.clone()),
                graph_seq: Some(graph_seq.clone()),
                code: None,
            },
        ),
        GraphOutcome::Proposed { .. } => (State::Proposed, only(None)),
        GraphOutcome::Conflict { .. } => (State::Conflict, only(None)),
        // A server refusal keeps its stable code (`forbidden` names the
        // refusal in its detail), never the detail itself.
        GraphOutcome::Rejected { reason, detail } => (
            State::Rejected,
            only(Some(
                detail
                    .get("code")
                    .and_then(serde_json::Value::as_str)
                    .filter(|_| reason == "forbidden")
                    .unwrap_or(reason)
                    .to_owned(),
            )),
        ),
        GraphOutcome::TechnicalRefusal { code } => {
            (State::TechnicalRefusal, only(Some(code.clone())))
        }
        GraphOutcome::IdempotencyMismatch => (State::IdempotencyMismatch, only(None)),
        GraphOutcome::Unsupported => (State::Unsupported, only(None)),
        GraphOutcome::PendingVisibility { .. } | GraphOutcome::Pending { .. } => return None,
    })
}

/// What the queue does with an entry in `from` given `outcome` (112.1
/// CC-5): the one policy, keyed on the outcome and its typed
/// [`PendingCause`], never on text.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Next {
    /// Persist this terminal outcome. When `revokes`, the domain is fenced
    /// too ([`super::outcome::GraphOutcome::revokes`]).
    Resolve {
        to: State,
        resolution: Resolution,
        revokes: bool,
    },
    /// Move to this open state (or stay, when already there).
    Mark(State),
    /// End it on this device: it was never sent.
    RefuseLocally,
    /// A revocation signal: fence the domain, which locks every open entry.
    Revoke,
    /// Keep it; retry the identical request under its own id after a
    /// back-off.
    Retry(PendingCause),
    /// Nothing is known yet: look its receipt up (and send it only if it is
    /// sendable).
    Lookup,
}

/// [`Next`] for an entry in `from` that `outcome` describes.
///
/// - a definite outcome resolves it; only a read-level refusal fences
///   ([`GraphOutcome::revokes`]): a `forbidden` receipt or a command
///   refused at send ends that request alone;
/// - `pending_visibility` waits for the receipt, never sent again;
/// - `outcome_unresolved` is `unresolved`: same-id replay or
///   `graph.outcome` only, never a new id (DF-110-8);
/// - an ended agent context refuses an entry never sent and leaves one
///   that may have been sent `unresolved`, never refused;
/// - a refused lookup revokes; a lookup the server refused as invalid
///   leaves the entry as it is (nothing resent); a transport failure, a
///   transient server error or `graph_unavailable` retries the identical
///   request; a foreign key suspends.
#[must_use]
pub fn next(from: State, outcome: &GraphOutcome) -> Next {
    if let Some((to, resolution)) = terminal(outcome) {
        return Next::Resolve {
            to,
            resolution,
            revokes: outcome.revokes(),
        };
    }
    match outcome {
        GraphOutcome::PendingVisibility { .. } => Next::Mark(State::PendingVisibility),
        GraphOutcome::Pending { cause } => match cause {
            PendingCause::Transport | PendingCause::Unavailable | PendingCause::Invalid => {
                Next::Retry(*cause)
            }
            PendingCause::Unresolved => Next::Mark(State::Unresolved),
            PendingCause::ContextEnded if from.never_sent() => Next::RefuseLocally,
            PendingCause::ContextEnded => Next::Mark(State::Unresolved),
            PendingCause::Refused => Next::Revoke,
            PendingCause::NoAttempt => Next::Lookup,
            PendingCause::Foreign => Next::Mark(State::Suspended),
        },
        _ => Next::Lookup,
    }
}

/// Whether a resume sends the identical request of an entry that was in
/// `from`, whose receipt lookup answered `looked` and which [`next`] left
/// in `settled`: only when nothing is known of it (no receipt, or a lookup
/// the transport lost) and it is still a person's to send, queued or
/// already submitting under its own id. Never after `outcome_unresolved`
/// or for an entry in [`State::Unresolved`] (only `graph.outcome` settles
/// those), never on `graph_unavailable` (it waits its back-off, which a
/// resume does not keep), and never an agent's request that may have been
/// sent (it is sent once, on its approval). `agent`: the request names an
/// agent context.
#[must_use]
pub fn resume_sends(from: State, looked: &GraphOutcome, settled: State, agent: bool) -> bool {
    let unknown = matches!(
        next(from, looked),
        Next::Lookup | Next::Retry(PendingCause::Transport)
    );
    unknown
        && match settled {
            State::Queued => true,
            State::Submitting => !agent,
            _ => false,
        }
}

/// The `graph_apply` call arguments that bind `prepared`: `{domain,
/// request}`, the request as its version-1 envelope, as canonical JCS text.
/// The queue stores these bytes and the runtime writes exactly them into
/// the call's row.
pub fn bound_args(domain: &str, prepared: &Prepared) -> Result<String, QueueError> {
    let request = prepared.request();
    jcs(&json!({
        "domain": domain,
        "request": {
            "version": CONTRACT_VERSION,
            "request_id": request.request_id,
            "context_id": request.context_id,
            "payload": request.payload,
        },
    }))
}

/// An agent's `graph_apply` approval binding, as the queue keeps it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Bound {
    pub approval_id: String,
    pub tool_call_id: String,
    /// [`bound_args`]' bytes (absent once the entry is terminal).
    pub bound_args: Option<String>,
}

/// What an approved agent entry is now.
#[derive(Clone, Debug, PartialEq)]
pub enum Approved {
    /// Its stored bytes match the frozen call: [`State::Submitting`], to be
    /// sent exactly as `prepared`.
    Send(Prepared),
    /// The frozen call's arguments are not the stored ones, or the stored
    /// request no longer matches its digest: [`State::Suspended`], never
    /// sent.
    Suspended,
}

/// One entry as a listing shows it: domain-safe metadata only, never the
/// payload.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub request_id: String,
    pub state: State,
    pub attempts: u32,
    pub last_cause: Option<String>,
    pub enqueued_at_ms: i64,
    pub updated_at_ms: i64,
}

/// What a restore found for one open entry.
#[derive(Clone, Debug, PartialEq)]
pub enum Restored {
    /// Its stored bytes and identity are exactly what the opened identity
    /// mints: sendable (or resolvable) under its own id, in `state`.
    Ready { prepared: Prepared, state: State },
    /// Its identity is not the opened one's; now [`State::Suspended`].
    Suspended { request_id: String },
    /// Its stored bytes do not match its digest: refused, the row kept as
    /// it was.
    Corrupt { request_id: String },
}

/// The fence's persisted state.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FenceState {
    /// Revocation learned: everything is locked, cleanup may be pending.
    Revoked,
    /// The replica's scope purge was observed; still locked.
    Purged,
}

impl FenceState {
    fn as_str(self) -> &'static str {
        match self {
            Self::Revoked => "revoked",
            Self::Purged => "purged",
        }
    }
}

/// The last read authority a successful online `read_status` answered.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AuthorityRow {
    pub authority: Authority,
    /// Device wall clock, UTC ms, when the answer returned.
    pub observed_at_ms: i64,
}

/// One server, domain and actor's queue file, open.
pub struct GraphQueue {
    conn: Connection,
    binding: Binding,
}

const SCHEMA: &str = "\
CREATE TABLE IF NOT EXISTS graph_queue_scope(\
    only INTEGER PRIMARY KEY CHECK (only = 1), \
    server TEXT NOT NULL, \
    domain TEXT NOT NULL, \
    actor TEXT NOT NULL\
);\
CREATE TABLE IF NOT EXISTS graph_intents(\
    request_id TEXT PRIMARY KEY, \
    server TEXT NOT NULL, \
    domain TEXT NOT NULL, \
    actor TEXT NOT NULL, \
    client TEXT NOT NULL, \
    descriptor TEXT NOT NULL, \
    version INTEGER NOT NULL, \
    context_id TEXT, \
    request_jcs TEXT, \
    request_digest TEXT NOT NULL, \
    state TEXT NOT NULL, \
    attempts INTEGER NOT NULL DEFAULT 0, \
    last_cause TEXT, \
    event_id TEXT, \
    graph_seq TEXT, \
    outcome_code TEXT, \
    approval_id TEXT, \
    tool_call_id TEXT, \
    bound_args TEXT, \
    enqueued_at INTEGER NOT NULL, \
    updated_at INTEGER NOT NULL\
);\
CREATE TABLE IF NOT EXISTS graph_fence(\
    only INTEGER PRIMARY KEY CHECK (only = 1), \
    state TEXT NOT NULL CHECK (state IN ('revoked', 'purged')), \
    fenced_at INTEGER NOT NULL, \
    updated_at INTEGER NOT NULL\
);\
CREATE TABLE IF NOT EXISTS graph_authority(\
    only INTEGER PRIMARY KEY CHECK (only = 1), \
    epoch TEXT NOT NULL, \
    digest TEXT NOT NULL, \
    observed_at INTEGER NOT NULL\
);";

/// The columns version 2 added to `graph_intents` (the agent approval
/// binding).
const V2_COLUMNS: [&str; 3] = ["approval_id", "tool_call_id", "bound_args"];

/// Bring the file to [`SCHEMA_VERSION`] in one transaction with its
/// version bump: an interruption leaves the file at the version it had.
/// A column already there (a file an earlier build left half-migrated, its
/// `ALTER`s committed one by one) is not added again.
fn migrate(conn: &mut Connection) -> Result<(), QueueError> {
    let tx = conn
        .transaction_with_behavior(TransactionBehavior::Immediate)
        .map_err(storage)?;
    let version: i64 = tx
        .pragma_query_value(None, "user_version", |row| row.get(0))
        .map_err(storage)?;
    if version > SCHEMA_VERSION {
        return Err(QueueError::Unreadable(format!(
            "queue schema {version} is newer than this build's {SCHEMA_VERSION}"
        )));
    }
    if version == SCHEMA_VERSION {
        return Ok(());
    }
    // `CREATE … IF NOT EXISTS` leaves a table an older build made as it was,
    // so the version 2 columns are checked whatever the version said.
    tx.execute_batch(SCHEMA).map_err(storage)?;
    let present = {
        let mut statement = tx
            .prepare("SELECT name FROM pragma_table_info('graph_intents')")
            .map_err(storage)?;
        let names = statement
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(storage)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(storage)?;
        names
    };
    for column in V2_COLUMNS {
        if !present.iter().any(|name| name == column) {
            tx.execute_batch(&format!(
                "ALTER TABLE graph_intents ADD COLUMN {column} TEXT;"
            ))
            .map_err(storage)?;
        }
    }
    tx.pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(storage)?;
    tx.commit().map_err(storage)
}

/// The SQL list of terminal state names.
fn terminal_names() -> String {
    STATES
        .into_iter()
        .filter(|state| state.is_terminal())
        .map(|state| format!("'{}'", state.as_str()))
        .collect::<Vec<_>>()
        .join(", ")
}

impl GraphQueue {
    /// Open (creating) the queue file at `path` for `binding`. A file
    /// written for another server, domain or actor refuses. Payloads of
    /// entries that reached a terminal state before a crash are deleted
    /// first.
    pub fn open(path: &Path, binding: Binding) -> Result<Self, QueueError> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|e| QueueError::Storage(e.to_string()))?;
        }
        let mut conn = Connection::open(path).map_err(storage)?;
        conn.busy_timeout(BUSY_TIMEOUT).map_err(storage)?;
        conn.pragma_update(None, "journal_mode", "WAL")
            .map_err(storage)?;
        conn.pragma_update(None, "synchronous", "FULL")
            .map_err(storage)?;
        migrate(&mut conn)?;
        conn.execute(
            "INSERT OR IGNORE INTO graph_queue_scope(only, server, domain, actor) \
             VALUES (1, ?1, ?2, ?3)",
            params![binding.server, binding.domain, binding.actor],
        )
        .map_err(storage)?;
        let scope: (String, String, String) = conn
            .query_row(
                "SELECT server, domain, actor FROM graph_queue_scope WHERE only = 1",
                [],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .map_err(storage)?;
        if scope
            != (
                binding.server.clone(),
                binding.domain.clone(),
                binding.actor.clone(),
            )
        {
            return Err(QueueError::ForeignFile);
        }
        let queue = Self { conn, binding };
        queue.clean_payloads()?;
        Ok(queue)
    }

    /// The identity this queue was opened under.
    #[must_use]
    pub fn binding(&self) -> &Binding {
        &self.binding
    }

    fn refuse_fenced(&self) -> Result<(), QueueError> {
        if self.fence()?.is_some() {
            return Err(QueueError::Fenced);
        }
        Ok(())
    }

    /// Persist a person's `prepared` as [`State::Queued`]. It is queued
    /// only once this returns `Ok`: the commit happened. Its client scope
    /// and domain must be this queue's; a request naming an agent context
    /// is only ever enqueued bound to its approval
    /// ([`GraphQueue::enqueue_for_approval`]).
    pub fn enqueue(&mut self, prepared: &Prepared, now_ms: i64) -> Result<(), QueueError> {
        self.refuse_fenced()?;
        if prepared.request().context_id.is_some() {
            return Err(QueueError::Unbound);
        }
        self.insert(prepared, State::Queued, None, now_ms)
    }

    /// One new entry in `state`, with an agent's approval binding
    /// `(approval_id, tool_call_id, bound_args)` when it has one.
    fn insert(
        &mut self,
        prepared: &Prepared,
        state: State,
        approval: Option<(&str, &str, &str)>,
        now_ms: i64,
    ) -> Result<(), QueueError> {
        let key = prepared.key();
        if key.domain() != self.binding.domain
            || key.client_scope() != client_scope(&self.binding.actor, &self.binding.client)
        {
            return Err(QueueError::ForeignRequest);
        }
        let request = prepared.request();
        let jcs = envelope_jcs(request)?;
        let (approval_id, tool_call_id, bound) = match approval {
            Some((approval_id, tool_call_id, bound)) => {
                (Some(approval_id), Some(tool_call_id), Some(bound))
            }
            None => (None, None, None),
        };
        self.conn
            .execute(
                "INSERT INTO graph_intents(request_id, server, domain, actor, client, \
                 descriptor, version, context_id, request_jcs, request_digest, state, \
                 enqueued_at, updated_at, approval_id, tool_call_id, bound_args) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?12, ?13, ?14, ?15)",
                params![
                    request.request_id,
                    self.binding.server,
                    self.binding.domain,
                    self.binding.actor,
                    self.binding.client,
                    key.descriptor(),
                    i64::from(CONTRACT_VERSION),
                    request.context_id,
                    jcs,
                    key.request_digest(),
                    state.as_str(),
                    now_ms,
                    approval_id,
                    tool_call_id,
                    bound,
                ],
            )
            .map_err(storage)?;
        Ok(())
    }

    fn state_of(&self, request_id: &str) -> Result<State, QueueError> {
        let name: Option<String> = self
            .conn
            .query_row(
                "SELECT state FROM graph_intents WHERE request_id = ?1",
                [request_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage)?;
        let name = name.ok_or_else(|| QueueError::Unknown(request_id.to_owned()))?;
        State::parse(&name).ok_or(QueueError::Unreadable(name))
    }

    /// Move `request_id` from `from` to `to` in one transaction, if the
    /// table allows it and the entry is still in `from`.
    fn advance(
        &mut self,
        request_id: &str,
        from: State,
        to: State,
        now_ms: i64,
        update: &str,
        extra: &[&dyn rusqlite::ToSql],
    ) -> Result<(), QueueError> {
        if !from.may_become(to) {
            return Err(QueueError::Transition { from, to });
        }
        let (to_name, from_name) = (to.as_str(), from.as_str());
        let mut values: Vec<&dyn rusqlite::ToSql> =
            vec![&to_name, &now_ms, &request_id, &from_name];
        values.extend_from_slice(extra);
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let changed = tx
            .execute(
                &format!(
                    "UPDATE graph_intents SET state = ?1, updated_at = ?2{update} \
                     WHERE request_id = ?3 AND state = ?4"
                ),
                values.as_slice(),
            )
            .map_err(storage)?;
        if changed == 0 {
            drop(tx);
            let actual = self.state_of(request_id)?;
            return Err(QueueError::Moved {
                request_id: request_id.to_owned(),
                expected: from,
                actual,
            });
        }
        tx.commit().map_err(storage)
    }

    /// A non-terminal transition (or [`State::Withdrawn`]'s, through
    /// [`GraphQueue::discard`]), with the cause that drove it.
    pub fn mark(
        &mut self,
        request_id: &str,
        from: State,
        to: State,
        cause: Option<&str>,
        now_ms: i64,
    ) -> Result<(), QueueError> {
        if to.is_terminal() {
            return Err(QueueError::Transition { from, to });
        }
        let attempts = i64::from(to == State::Submitting);
        self.advance(
            request_id,
            from,
            to,
            now_ms,
            ", last_cause = ?5, attempts = attempts + ?6",
            &[&cause, &attempts],
        )
    }

    /// Record another send of an entry already [`State::Submitting`] (an
    /// identical retry under the same id).
    pub fn retry(&mut self, request_id: &str, cause: &str, now_ms: i64) -> Result<(), QueueError> {
        let changed = self
            .conn
            .execute(
                "UPDATE graph_intents SET attempts = attempts + 1, last_cause = ?1, \
                 updated_at = ?2 WHERE request_id = ?3 AND state = ?4",
                params![cause, now_ms, request_id, State::Submitting.as_str()],
            )
            .map_err(storage)?;
        if changed == 0 {
            return Err(QueueError::Moved {
                request_id: request_id.to_owned(),
                expected: State::Submitting,
                actual: self.state_of(request_id)?,
            });
        }
        Ok(())
    }

    /// Persist a terminal outcome: the state and the receipt summary in one
    /// transaction, then (separately, resumably) delete the payload. A
    /// failed persist leaves the entry where it was, replayable.
    pub fn resolve(
        &mut self,
        request_id: &str,
        from: State,
        to: State,
        resolution: &Resolution,
        now_ms: i64,
    ) -> Result<(), QueueError> {
        self.persist_terminal(request_id, from, to, resolution, now_ms)?;
        self.clean_payloads()
    }

    fn persist_terminal(
        &mut self,
        request_id: &str,
        from: State,
        to: State,
        resolution: &Resolution,
        now_ms: i64,
    ) -> Result<(), QueueError> {
        if !to.is_outcome() {
            return Err(QueueError::Transition { from, to });
        }
        self.advance(
            request_id,
            from,
            to,
            now_ms,
            ", event_id = ?5, graph_seq = ?6, outcome_code = ?7",
            &[
                &resolution.event_id,
                &resolution.graph_seq,
                &resolution.code,
            ],
        )
    }

    /// Withdraw an entry that was never sent, is suspended or is locked:
    /// it is never sent again, and its payload is deleted.
    pub fn discard(&mut self, request_id: &str, now_ms: i64) -> Result<(), QueueError> {
        let from = self.state_of(request_id)?;
        self.advance(
            request_id,
            from,
            State::Withdrawn,
            now_ms,
            ", last_cause = ?5",
            &[&"discarded"],
        )?;
        self.clean_payloads()
    }

    /// Delete a terminal entry's record once its caller has read it.
    pub fn acknowledge(&mut self, request_id: &str) -> Result<(), QueueError> {
        let state = self.state_of(request_id)?;
        if !state.is_terminal() {
            return Err(QueueError::Open(request_id.to_owned()));
        }
        self.conn
            .execute(
                "DELETE FROM graph_intents WHERE request_id = ?1",
                [request_id],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// The resumable cleanup: delete the payload (the request and an
    /// agent's bound call arguments) of every terminal entry.
    fn clean_payloads(&self) -> Result<(), QueueError> {
        self.conn
            .execute(
                &format!(
                    "UPDATE graph_intents SET request_jcs = NULL, bound_args = NULL \
                     WHERE (request_jcs IS NOT NULL OR bound_args IS NOT NULL) \
                     AND state IN ({})",
                    terminal_names()
                ),
                [],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// End an entry never sent on this device ([`State::RefusedLocally`]):
    /// its approval was denied or expired, or its context ended before the
    /// send. An entry that may have been sent refuses the move.
    pub fn refuse_locally(
        &mut self,
        request_id: &str,
        cause: &str,
        now_ms: i64,
    ) -> Result<(), QueueError> {
        let from = self.state_of(request_id)?;
        self.advance(
            request_id,
            from,
            State::RefusedLocally,
            now_ms,
            ", last_cause = ?5",
            &[&cause],
        )?;
        self.clean_payloads()
    }

    /// Persist an agent's `graph_apply` request, bound to its approval
    /// before anything is shown to a person: [`State::AwaitingApproval`],
    /// with the approval id, the call's preallocated row id and the bound
    /// call arguments ([`bound_args`]), in one transaction. The request must
    /// carry its agent context, be a `graph.apply` and name `approval_id`.
    /// Returns the bound arguments the call's row must carry, byte for byte.
    pub fn enqueue_for_approval(
        &mut self,
        prepared: &Prepared,
        approval_id: &str,
        tool_call_id: &str,
        now_ms: i64,
    ) -> Result<String, QueueError> {
        self.refuse_fenced()?;
        let request = prepared.request();
        let named = request
            .payload
            .get("approval_id")
            .and_then(serde_json::Value::as_str);
        if request.context_id.is_none()
            || prepared.key().descriptor() != super::protocol::change::DESCRIPTOR
            || named != Some(approval_id)
            || tool_call_id.is_empty()
        {
            return Err(QueueError::Unbound);
        }
        let args = bound_args(&self.binding.domain, prepared)?;
        self.insert(
            prepared,
            State::AwaitingApproval,
            Some((approval_id, tool_call_id, &args)),
            now_ms,
        )?;
        Ok(args)
    }

    /// The approval binding of `request_id`, if it is an agent's. Refused
    /// while fenced: a fenced queue discloses no frozen request (112.1
    /// CC-9), whoever opens it.
    pub fn bound(&self, request_id: &str) -> Result<Option<Bound>, QueueError> {
        self.refuse_fenced()?;
        let row: Option<(Option<String>, Option<String>, Option<String>)> = self
            .conn
            .query_row(
                "SELECT approval_id, tool_call_id, bound_args FROM graph_intents \
                 WHERE request_id = ?1",
                [request_id],
                |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
            )
            .optional()
            .map_err(storage)?;
        match row {
            None => Err(QueueError::Unknown(request_id.to_owned())),
            Some((Some(approval_id), Some(tool_call_id), bound_args)) => Ok(Some(Bound {
                approval_id,
                tool_call_id,
                bound_args,
            })),
            Some(_) => Ok(None),
        }
    }

    /// A person approved `request_id` and the runtime rechecked its
    /// authority: compare the call's frozen arguments (`frozen_args`, as its
    /// row holds them) with the bound ones, re-derive the request from the
    /// stored bytes under `client`, and move it to [`State::Submitting`] to
    /// be sent exactly once as stored. Any difference suspends it, never
    /// sent. Refused while fenced.
    pub fn approve(
        &mut self,
        client: &GraphClient,
        request_id: &str,
        frozen_args: &str,
        now_ms: i64,
    ) -> Result<Approved, QueueError> {
        self.refuse_fenced()?;
        let actual = self.state_of(request_id)?;
        let Some(row) = self
            .open_rows()?
            .into_iter()
            .find(|row| row.request_id == request_id && row.state == State::AwaitingApproval)
        else {
            return Err(QueueError::Moved {
                request_id: request_id.to_owned(),
                expected: State::AwaitingApproval,
                actual,
            });
        };
        let frozen = super::protocol::value::parse(frozen_args.as_bytes())
            .ok()
            .and_then(|value| jcs(&value).ok());
        let stored = self.bound(request_id)?.and_then(|bound| bound.bound_args);
        let prepared = match self.check(client, &row) {
            Check::Ready(prepared)
                if frozen.is_some()
                    && frozen == stored
                    && bound_args(&self.binding.domain, &prepared).ok() == stored =>
            {
                prepared
            }
            _ => {
                self.mark(
                    request_id,
                    State::AwaitingApproval,
                    State::Suspended,
                    Some("approval_binding"),
                    now_ms,
                )?;
                return Ok(Approved::Suspended);
            }
        };
        self.mark(
            request_id,
            State::AwaitingApproval,
            State::Submitting,
            Some("approved"),
            now_ms,
        )?;
        Ok(Approved::Send(prepared))
    }

    /// After an agent runtime restarts, its contexts are gone (they are
    /// bound to the process incarnation). Every agent entry never sent
    /// ends [`State::RefusedLocally`]; every one that may have been sent
    /// (`submitting`, `pending_visibility`) becomes [`State::Unresolved`],
    /// never refused and never resent until `graph.outcome` resolves it.
    /// Persons' entries (no context) are untouched. One transaction; the
    /// counts (refused, unresolved).
    pub fn incarnation_ended(&mut self, now_ms: i64) -> Result<(usize, usize), QueueError> {
        let tx = self
            .conn
            .transaction_with_behavior(TransactionBehavior::Immediate)
            .map_err(storage)?;
        let refused = tx
            .execute(
                "UPDATE graph_intents SET state = ?1, last_cause = 'incarnation_ended', \
                 updated_at = ?2 WHERE context_id IS NOT NULL AND state IN (?3, ?4)",
                params![
                    State::RefusedLocally.as_str(),
                    now_ms,
                    State::AwaitingApproval.as_str(),
                    State::Queued.as_str(),
                ],
            )
            .map_err(storage)?;
        let unresolved = tx
            .execute(
                "UPDATE graph_intents SET state = ?1, last_cause = 'incarnation_ended', \
                 updated_at = ?2 WHERE context_id IS NOT NULL AND state IN (?3, ?4)",
                params![
                    State::Unresolved.as_str(),
                    now_ms,
                    State::Submitting.as_str(),
                    State::PendingVisibility.as_str(),
                ],
            )
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
        self.clean_payloads()?;
        Ok((refused, unresolved))
    }

    /// Every entry, metadata only, oldest first. Refused while fenced.
    pub fn entries(&self) -> Result<Vec<Entry>, QueueError> {
        self.refuse_fenced()?;
        let mut statement = self
            .conn
            .prepare(
                "SELECT request_id, state, attempts, last_cause, enqueued_at, updated_at \
                 FROM graph_intents ORDER BY enqueued_at, request_id",
            )
            .map_err(storage)?;
        let rows = statement
            .query_map([], |row| {
                Ok((
                    row.get::<_, String>(0)?,
                    row.get::<_, String>(1)?,
                    row.get::<_, i64>(2)?,
                    row.get::<_, Option<String>>(3)?,
                    row.get::<_, i64>(4)?,
                    row.get::<_, i64>(5)?,
                ))
            })
            .map_err(storage)?;
        let mut entries = Vec::new();
        for row in rows {
            let (request_id, state, attempts, last_cause, enqueued_at_ms, updated_at_ms) =
                row.map_err(storage)?;
            entries.push(Entry {
                request_id,
                state: State::parse(&state).ok_or(QueueError::Unreadable(state))?,
                attempts: u32::try_from(attempts).unwrap_or(u32::MAX),
                last_cause,
                enqueued_at_ms,
                updated_at_ms,
            });
        }
        Ok(entries)
    }

    /// How many entries are in each state (domain-safe; allowed while
    /// fenced).
    pub fn counts(&self) -> Result<BTreeMap<State, usize>, QueueError> {
        count_states(&self.conn)
    }

    /// Restore every open entry that may be sent or resolved (queued,
    /// submitting, pending visibility, unresolved) under `client`, the
    /// graph of the identity this queue was opened under. Each is rebuilt
    /// from its stored bytes with [`GraphClient::prepared`], never
    /// re-minted; a digest that does not match those bytes is corruption
    /// (refused, kept); any other difference from what `client` mints, or
    /// from this queue's binding, suspends it. Refused while fenced.
    pub fn restore(
        &mut self,
        client: &GraphClient,
        now_ms: i64,
    ) -> Result<Vec<Restored>, QueueError> {
        self.refuse_fenced()?;
        let stored = self.open_rows()?;
        let mut restored = Vec::with_capacity(stored.len());
        for row in stored {
            match self.check(client, &row) {
                Check::Ready(prepared) => restored.push(Restored::Ready {
                    prepared,
                    state: row.state,
                }),
                Check::Corrupt => restored.push(Restored::Corrupt {
                    request_id: row.request_id,
                }),
                Check::Differs => {
                    self.mark(
                        &row.request_id,
                        row.state,
                        State::Suspended,
                        Some("identity_changed"),
                        now_ms,
                    )?;
                    restored.push(Restored::Suspended {
                        request_id: row.request_id,
                    });
                }
            }
        }
        Ok(restored)
    }

    fn open_rows(&self) -> Result<Vec<StoredRow>, QueueError> {
        let mut statement = self
            .conn
            .prepare(
                "SELECT request_id, server, domain, actor, client, descriptor, version, \
                 context_id, request_jcs, request_digest, state FROM graph_intents \
                 WHERE state IN (?1, ?2, ?3, ?4, ?5) ORDER BY enqueued_at, request_id",
            )
            .map_err(storage)?;
        let rows = statement
            .query_map(
                params![
                    State::AwaitingApproval.as_str(),
                    State::Queued.as_str(),
                    State::Submitting.as_str(),
                    State::PendingVisibility.as_str(),
                    State::Unresolved.as_str(),
                ],
                |row| {
                    Ok((
                        StoredRow {
                            request_id: row.get(0)?,
                            binding: Binding {
                                server: row.get(1)?,
                                domain: row.get(2)?,
                                actor: row.get(3)?,
                                client: row.get(4)?,
                            },
                            descriptor: row.get(5)?,
                            version: row.get(6)?,
                            context_id: row.get(7)?,
                            jcs: row.get(8)?,
                            digest: row.get(9)?,
                            state: State::Queued,
                        },
                        row.get::<_, String>(10)?,
                    ))
                },
            )
            .map_err(storage)?;
        let mut stored = Vec::new();
        for row in rows {
            let (mut row, state) = row.map_err(storage)?;
            row.state = State::parse(&state).ok_or(QueueError::Unreadable(state))?;
            stored.push(row);
        }
        Ok(stored)
    }

    /// Whether `row` is exactly what `client` mints from its own bytes.
    fn check(&self, client: &GraphClient, row: &StoredRow) -> Check {
        let Some(request) = row
            .jcs
            .as_deref()
            .and_then(|jcs| request::parse(jcs.as_bytes()).ok())
        else {
            return Check::Corrupt;
        };
        // The stored bytes must still be canonical and carry the stored
        // digest; anything else is a damaged row, not another identity.
        let canonical_again = envelope_jcs(&request).ok();
        let digest = request::digest(&row.descriptor, &request).ok();
        if canonical_again.as_deref() != row.jcs.as_deref()
            || digest.as_deref() != Some(row.digest.as_str())
            || request.request_id != row.request_id
        {
            return Check::Corrupt;
        }
        if row.binding != self.binding
            || row.version != i64::from(CONTRACT_VERSION)
            || request.context_id != row.context_id
        {
            return Check::Differs;
        }
        let Ok(prepared) = client.prepared(&row.descriptor, request) else {
            return Check::Differs;
        };
        let key = prepared.key();
        let same = key.domain() == row.binding.domain
            && key.descriptor() == row.descriptor
            && key.request_id() == row.request_id
            && key.request_digest() == row.digest
            && key.client_scope() == client_scope(&row.binding.actor, &row.binding.client)
            && prepared.request().context_id == row.context_id;
        if same {
            Check::Ready(prepared)
        } else {
            Check::Differs
        }
    }

    /// The fence, if one was persisted.
    pub fn fence(&self) -> Result<Option<FenceState>, QueueError> {
        let state: Option<String> = self
            .conn
            .query_row("SELECT state FROM graph_fence WHERE only = 1", [], |row| {
                row.get(0)
            })
            .optional()
            .map_err(storage)?;
        Ok(match state.as_deref() {
            None => None,
            Some("purged") => Some(FenceState::Purged),
            // Anything else written there is read as the stricter state.
            Some(_) => Some(FenceState::Revoked),
        })
    }

    /// When the standing fence was set (the caller's clock at the first
    /// revocation signal), if one stands.
    pub(crate) fn fenced_at(&self) -> Result<Option<i64>, QueueError> {
        self.conn
            .query_row(
                "SELECT fenced_at FROM graph_fence WHERE only = 1",
                [],
                |row| row.get(0),
            )
            .optional()
            .map_err(storage)
    }

    /// Persist the fence ([`FenceState::Revoked`]), committed on its own
    /// before anything else of the revocation. Idempotent: an existing
    /// fence stays as it is.
    pub(crate) fn persist_fence(&self, now_ms: i64) -> Result<(), QueueError> {
        self.conn
            .execute(
                "INSERT OR IGNORE INTO graph_fence(only, state, fenced_at, updated_at) \
                 VALUES (1, 'revoked', ?1, ?1)",
                [now_ms],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// Lock every open entry ([`State::Disabled`]), identity and payload
    /// kept; how many were locked now. Idempotent.
    pub(crate) fn disable_open(&self, now_ms: i64) -> Result<usize, QueueError> {
        let open: Vec<String> = STATES
            .into_iter()
            .filter(|state| state.is_open())
            .map(|state| format!("'{}'", state.as_str()))
            .collect();
        self.conn
            .execute(
                &format!(
                    "UPDATE graph_intents SET state = 'disabled', last_cause = 'revoked', \
                     updated_at = ?1 WHERE state IN ({})",
                    open.join(", ")
                ),
                [now_ms],
            )
            .map_err(storage)
    }

    /// Record that the replica's scope purge was observed.
    pub(crate) fn mark_purged(&self, now_ms: i64) -> Result<(), QueueError> {
        self.conn
            .execute(
                "UPDATE graph_fence SET state = ?1, updated_at = ?2 WHERE only = 1",
                params![FenceState::Purged.as_str(), now_ms],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// Remove the fence (a regrant, after fresh authority).
    pub(crate) fn clear_fence(&self) -> Result<(), QueueError> {
        self.conn
            .execute("DELETE FROM graph_fence", [])
            .map_err(storage)?;
        Ok(())
    }

    /// Persist the authority a successful online `read_status` answered at
    /// `observed_at_ms`, replacing the previous one. Written from nothing
    /// else.
    pub fn record_authority(
        &self,
        authority: &Authority,
        observed_at_ms: i64,
    ) -> Result<(), QueueError> {
        self.conn
            .execute(
                "INSERT INTO graph_authority(only, epoch, digest, observed_at) \
                 VALUES (1, ?1, ?2, ?3) ON CONFLICT(only) DO UPDATE SET \
                 epoch = excluded.epoch, digest = excluded.digest, \
                 observed_at = excluded.observed_at",
                params![authority.epoch, authority.digest, observed_at_ms],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// Drop the kept authority observed at `observed_at_ms`, found to be
    /// from the future: once this delete commits, the row never authorises
    /// again, across restarts, and only the next successful online
    /// `read_status` ([`GraphQueue::record_authority`]) keeps one again. If
    /// it does not commit, only the in-memory distrust
    /// ([`super::lifecycle::Lifecycle::trusts`]) holds, and a restart trusts
    /// the row again once the clock passes it (owner decision 112.1 G3,
    /// 2026-11-11: an accepted exception). A newer row written meanwhile
    /// stays.
    pub fn invalidate_authority(&self, observed_at_ms: i64) -> Result<(), QueueError> {
        self.conn
            .execute(
                "DELETE FROM graph_authority WHERE only = 1 AND observed_at = ?1",
                [observed_at_ms],
            )
            .map_err(storage)?;
        Ok(())
    }

    /// The last persisted read authority.
    pub fn authority(&self) -> Result<Option<AuthorityRow>, QueueError> {
        self.conn
            .query_row(
                "SELECT epoch, digest, observed_at FROM graph_authority WHERE only = 1",
                [],
                |row| {
                    Ok(AuthorityRow {
                        authority: Authority {
                            epoch: row.get(0)?,
                            digest: row.get(1)?,
                        },
                        observed_at_ms: row.get(2)?,
                    })
                },
            )
            .optional()
            .map_err(storage)
    }
}

struct StoredRow {
    request_id: String,
    binding: Binding,
    descriptor: String,
    version: i64,
    context_id: Option<String>,
    jcs: Option<String>,
    digest: String,
    state: State,
}

enum Check {
    Ready(Prepared),
    Corrupt,
    Differs,
}

/// The canonical JCS text of `value`.
fn jcs(value: &serde_json::Value) -> Result<String, QueueError> {
    let bytes =
        canonical(value).map_err(|refusal| QueueError::Unreadable(refusal.code().to_owned()))?;
    String::from_utf8(bytes).map_err(|_| QueueError::Unreadable("envelope.utf8".to_owned()))
}

/// The canonical JCS text of `request`'s envelope.
fn envelope_jcs(request: &Request) -> Result<String, QueueError> {
    jcs(&json!({
        "version": CONTRACT_VERSION,
        "request_id": request.request_id,
        "context_id": request.context_id,
        "payload": request.payload,
    }))
}

fn count_states(conn: &Connection) -> Result<BTreeMap<State, usize>, QueueError> {
    let mut statement = conn
        .prepare("SELECT state, COUNT(*) FROM graph_intents GROUP BY state")
        .map_err(storage)?;
    let rows = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })
        .map_err(storage)?;
    let mut counts = BTreeMap::new();
    for row in rows {
        let (state, count) = row.map_err(storage)?;
        let state = State::parse(&state).ok_or(QueueError::Unreadable(state))?;
        counts.insert(state, usize::try_from(count).unwrap_or(usize::MAX));
    }
    Ok(counts)
}

/// How many entries are in each state across every queue file under
/// `dir` (a server's directory), read-only, for a log before the
/// directory is deleted. A file that cannot be read is skipped and counted.
#[must_use]
pub fn counts_under(dir: &Path) -> (BTreeMap<State, usize>, usize) {
    let mut totals = BTreeMap::new();
    let mut unreadable = 0;
    let Ok(databases) = std::fs::read_dir(dir) else {
        return (totals, unreadable);
    };
    for database in databases.flatten() {
        let Ok(files) = std::fs::read_dir(database.path()) else {
            continue;
        };
        for file in files.flatten() {
            let path = file.path();
            if path.extension().and_then(|e| e.to_str())
                != Some(crate::hq::databases::GRAPH_QUEUE_EXTENSION)
            {
                continue;
            }
            let counts = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                .map_err(storage)
                .and_then(|conn| count_states(&conn));
            match counts {
                Ok(counts) => {
                    for (state, count) in counts {
                        *totals.entry(state).or_insert(0) += count;
                    }
                }
                Err(_) => unreadable += 1,
            }
        }
    }
    (totals, unreadable)
}

#[cfg(test)]
#[path = "queue_tests.rs"]
mod tests;
