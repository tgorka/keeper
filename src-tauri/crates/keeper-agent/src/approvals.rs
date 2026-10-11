//! A run that waits for a person, and its resume (AD-393, AD-394; FR-797,
//! NFR-117; rulings R73–R80, R84, R86).
//!
//! # Park
//!
//! A call that needs a person — its tier, or the integrity rule — parks
//! when a [`DecisionSource`] is installed (R77): the shared tool loop
//! returns right after it with the round's later calls unrun (R73), and the
//! session's worker, outside any thread a wait could hold, `fsync`s the
//! chunk, takes the checkpoint, writes `approvals/<ulid>.json` create-new
//! and `fsync`s it and its folder, sends the request into the room, logs
//! `approval requested` and says in the status what it waits for. Nothing
//! waits: the turn ends, `busy` clears, a hand-back is possible. With no
//! source the call is refused with [`crate::host::UNATTENDED_REFUSAL`] and
//! nothing is written.
//!
//! # Resume, exactly once
//!
//! On a decision, at serve start, and when a record expires, the worker
//! reads the room first — any `consumed` event for the approval, from any
//! copy, means it is spent (R75) — then re-reads the record and recomputes
//! its digest, checks every precondition, checks that it holds the
//! session's claim, and consumes: it sends the `consumed` state event, waits
//! for the server's id, reads the room forward from the request and goes on
//! only when its own event is the first. Then the local mirror line is
//! `fsync`ed, the call runs, and the turn continues from the checkpoint with
//! the round's remaining calls in order. A send that fails runs nothing and
//! the run stays parked; a crash after the `consumed` event leaves the
//! effect unknown, which the model is told — never a second execution.

use std::fs::{self, File, OpenOptions};
use std::future::Future;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use keeper_core::agents::approval::{
    self, parse_decision, parse_record, ApprovalRecord, CallRef, Checkpoint, Decision,
    DecisionRecord, FilePin, Parking as RecordParking, Preconditions, WrittenBy,
};
use keeper_core::agents::events::{
    ApprovalDecisionContent, ApprovalRequestContent, ConsumedContent, RequestAction, RunState,
    APPROVAL_REQUEST, CONTENT_VERSION,
};
use keeper_core::agents::label::{check_sink, Readers, Sink, SinkVerdict};
use keeper_core::agents::log::{ApprovalBody, ApprovalState, LineBody, PeerBody};
use keeper_core::agents::matrix::AgentMatrixError;
use keeper_core::agents::tier::{AgentTool, Classification, APPROVALS_DIR};
use keeper_core::agents::trust::{Anchor, Published};
use keeper_core::bots::audit::{self, AuditIntent, AuditOutcome};
use keeper_core::bots::chat::{self, CancelSignal};
use keeper_core::bots::grant::{Effect, GrantVerdict, ToolTarget};
use keeper_core::bots::tools::{render_result, ToolOutcome};
use keeper_sync::SyncProfile;
use matrix_sdk::ruma::{DeviceId, EventId, OwnedEventId, OwnedRoomId, OwnedUserId, UserId};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::time::Instant;
use ulid::Ulid;

use crate::agent::{AgentDeps, Arrived, Outcome, ServeError, ServedSession};
use crate::matrix_sink::EditPort;
use crate::turn::now_ms;

/// What the model is told of a call a person declined.
pub const DENIED: &str = "A person declined this, so keeper did not do it. Nothing was changed.";
/// What the model is told of a call whose approval expired undecided.
pub const EXPIRED: &str =
    "Nobody decided on this in time, so keeper did not do it. Nothing was changed.";
/// Why a pending approval ends when its person writes again in their own
/// conversation (R74).
pub const SUPERSEDED: &str = "superseded by your message";
/// What the model is told of a call its person's new message superseded.
pub const SUPERSEDED_RESULT: &str =
    "The person wrote again before deciding on this, so keeper did not do it. Nothing was changed.";
/// What a call later in a round is told when the call before it did not
/// run.
pub const NOT_RUN: &str =
    "This was not run: a call before it in the same round was not approved. Nothing was changed.";
/// What the status says while a session waits for a person.
pub const WAITING_FOR: &str = "Waiting for a decision on:";
/// Why a decision arrival is ignored when no approval of this session
/// waits for it.
pub const NO_SUCH_APPROVAL: &str = "no approval of this session waits for this decision";
/// Why a decision arrival is ignored on a host that does not hold the
/// session's claim: it writes no decision (R176).
pub const NOT_HOLDER: &str = "this host does not hold the session's claim";
/// Why a decision arrival is ignored when its file could not be written.
pub const DECISION_UNWRITTEN: &str = "the decision could not be written";
/// Why a decision whose content keeper cannot read is ignored.
pub const UNREADABLE_DECISION: &str = "this decision could not be read";
/// Why a decision after the one already taken is ignored.
pub const ALREADY_DECIDED: &str = "this approval was decided already";
/// Why a decision on an approval that ended already — run, denied,
/// expired, superseded — is ignored (R80).
pub const APPROVAL_ENDED: &str = "this approval has ended; a later decision changes nothing";
/// What a call of a parked round is told when a stop cut its round's
/// continuation while it may have been running.
pub const INTERRUPTED: &str = "keeper stopped while this ran, so it does not know whether it took effect, and it will not run it again. Check, and ask again if it is still needed.";
/// What a later call of a parked round is told when what it asked is not
/// in the round's file: it never runs from the log's redacted copy.
pub const UNBOUND: &str =
    "keeper could not read back exactly what this asked, so it did not run it. Nothing was changed.";
/// What the model is told of a call whose request was never sent because
/// its arguments could not be attached: nobody was asked (R180).
pub const UNATTACHED: &str = "keeper could not attach exactly what this asks to its request, so nobody was asked and it was not done. Nothing was changed.";
/// What the model is told of a call approved here whose approval expired
/// before keeper could use it: it was never consumed, so never run (R179).
pub const APPROVED_UNUSED: &str = "This was approved, but keeper could not use the approval before it expired, so it did not do it. Nothing was changed.";
/// Why an approval ends when the room's history could not be read far
/// enough to know whether it was used (R179).
pub const HISTORY_UNREAD: &str =
    "the room's history could not be read far enough to know whether it was used";
/// What the model is told of such a call: never run again, its effect
/// unknown.
pub const HISTORY_UNREAD_RESULT: &str = "keeper could not read the room far enough back to know whether this approval was used, so it does not know whether it took effect, and it will not run it. Check, and propose it again if it is still needed.";
/// Why an approved call does not run when what it would do now is not the
/// action its approval names.
pub const CHANGED: &str = "the action changed after it was approved";
/// What the model is told of a call that needs a person when none of the
/// people who could approve it can be asked from this host: it does not
/// wait, and no request is left behind (R194; a relay through another host
/// is DW-485).
pub const NOBODY_TO_ASK: &str = "This needs a person's approval, but nobody who can approve this can be asked from this host — none of their proxies runs here — so keeper did not do it. Nothing was changed.";

/// What the model is told of an approved call that did not run because
/// `reason` moved after its approval.
pub fn not_done(reason: &str) -> String {
    format!("This was not done: {reason}. Nothing was changed.")
}

/// Who may decide an approval and from which device (R77; 93.3's trust
/// adapter, [`crate::deciding::ClientDecisions`]). Installed, every call
/// that needs a person parks; absent, it is refused as before Epic 93.
/// Both production hosts install one (R92).
pub trait DecisionSource: Send + Sync {
    /// Where this host's trusted master keys come from.
    fn anchor(&self) -> &Anchor;
    /// What `user`'s homeserver publishes now of their `device` and their
    /// cross-signing identity.
    fn published<'a>(&'a self, user: &'a UserId, device: &'a DeviceId)
        -> RoomFuture<'a, Published>;
}

/// A boxed room request.
pub type RoomFuture<'a, T> = Pin<Box<dyn Future<Output = Result<T, AgentMatrixError>> + Send + 'a>>;

/// One `dev.keeper.agent.approval.consumed` event in the room.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Consumed {
    pub event: OwnedEventId,
    pub content: ConsumedContent,
}

/// What a forward read of the room found for one approval (R75, R179).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ConsumedRead {
    /// Its `consumed` events, in room order from where the read began: the
    /// first of them is the first, complete or not.
    pub consumed: Vec<Consumed>,
    /// `false`: the read stopped at its bound with history left — finding
    /// none is then no evidence that nothing was consumed.
    pub complete: bool,
}

/// What the worker asks of the session's room beyond its edits (R75, R86).
pub trait ApprovalRoom: Send + Sync {
    /// Upload `bytes` as an encrypted file: its `EncryptedFile` JSON.
    fn upload(&self, bytes: Vec<u8>) -> RoomFuture<'_, Value>;
    /// Send the `consumed` state event (key = its id); the server's id.
    fn consume(&self, content: ConsumedContent) -> RoomFuture<'_, OwnedEventId>;
    /// Every `consumed` event for `id` the session's agent user sent, in
    /// room order, read forward from `from` — the request, or the status
    /// sent before a request that went to the approvers' DMs — or from the
    /// room's start when it is unknown; bounded, and saying so.
    fn consumed<'a>(
        &'a self,
        id: &'a str,
        from: Option<&'a EventId>,
    ) -> RoomFuture<'a, ConsumedRead>;
    /// Whether this host holds the session's claim at `epoch`, read from the
    /// server now.
    fn holds(&self, epoch: u64) -> RoomFuture<'_, bool>;
}

/// What one copy's attempt to consume an approval came to (R75).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Consumption {
    /// Its own `consumed` is the first in the room's order: it runs here.
    Won(OwnedEventId),
    /// Another `consumed` came first: spent, and not here.
    Lost(Consumed),
    /// Not known yet — the send or the read failed, or the read stopped at
    /// its bound before it found this copy's own: the event this copy sent,
    /// when the server took it. Nothing runs; it is tried again.
    Unknown(Option<OwnedEventId>),
}

/// Consume `content.id` once in `room`: send the `consumed` state event —
/// unless `sent` is the one this copy's server took already — then read the
/// room forward from `from` and win only when this copy's event is the
/// first. Every copy of an agent decides by this, and only a winner runs.
pub async fn consume_once(
    room: &dyn ApprovalRoom,
    content: ConsumedContent,
    from: Option<&EventId>,
    sent: Option<OwnedEventId>,
) -> Consumption {
    let id = content.id.clone();
    let ours = match sent {
        Some(ours) => ours,
        None => match room.consume(content).await {
            Ok(ours) => ours,
            Err(error) => {
                tracing::warn!(%error, approval = %id, "agents: the consumed event was not accepted; the run stays parked");
                return Consumption::Unknown(None);
            }
        },
    };
    match room.consumed(&id, from).await {
        Ok(read) => match read.consumed.into_iter().next() {
            Some(first) if first.event == ours => Consumption::Won(ours),
            Some(first) => Consumption::Lost(first),
            None => {
                tracing::warn!(approval = %id, "agents: the room's history was not read as far as this copy's consumed event; the run stays parked");
                Consumption::Unknown(Some(ours))
            }
        },
        Err(error) => {
            tracing::warn!(%error, approval = %id, "agents: the room could not be read after consuming; the run stays parked");
            Consumption::Unknown(Some(ours))
        }
    }
}

/// The request a person's card is drawn from for `record`, as written —
/// its large payload already moved out ([`ApprovalRecord::externalise_args`])
/// and uploaded as `file` (its `EncryptedFile` JSON) — in the session room
/// `room`, decided by `approvers` (empty: anyone reading). Every field its
/// digest is over is the record's own.
pub(crate) fn request_content(
    record: &ApprovalRecord,
    room: &str,
    approvers: Vec<String>,
    file: Option<Value>,
) -> ApprovalRequestContent {
    ApprovalRequestContent {
        v: CONTENT_VERSION,
        id: record.id.clone(),
        session: record.session.clone(),
        room: room.to_owned(),
        agent: record.agent.clone(),
        tier: record.risk.tier,
        summary: record.action.summary.clone(),
        action: RequestAction {
            tool: record.action.tool.clone(),
            args: record.action.args.clone(),
            exec_binding: record.action.exec_binding.clone(),
        },
        file_sha256: file.as_ref().and(record.action.args_blob.clone()),
        file,
        checkpoint_sha256: record.checkpoint.sha256.clone(),
        preconditions: record.preconditions.clone(),
        binding_digest: record.binding_digest.clone(),
        scopes: record
            .scopes
            .iter()
            .map(|scope| scope.as_word().to_owned())
            .collect(),
        expires_at: record.expires_at.clone(),
        approvers,
        dispatch_chain: record.dispatch_chain.clone(),
    }
}

/// The call a turn's host parked: what the record needs beyond the wire.
#[derive(Debug, Clone)]
pub(crate) struct Parking {
    pub approval: Ulid,
    pub call_id: String,
    pub classification: Classification,
    /// The files it relies on, `(drive, drive-relative path)`.
    pub pins: Vec<(String, String)>,
    /// A flow its sinks blocked: the `declassify` action's arguments, the
    /// call itself added once its wire is known (R89).
    pub declassify: Option<Value>,
    /// A `run`'s `exec_binding` (AD-393); `null` for every other call.
    pub exec_binding: Value,
    /// A networked `run`'s workspace set, its `preconditions.workspace`
    /// (S-03).
    pub workspace: Option<Value>,
}

/// A parked turn, handed from the tool loop to the worker.
#[derive(Debug, Clone)]
pub(crate) struct ParkedTurn {
    pub parking: Parking,
    pub call_line: Ulid,
    pub args: Value,
    pub preconditions: Preconditions,
    /// The round's later calls, their `tool_call` lines written, as the
    /// model sent them.
    pub rest: Vec<(Ulid, chat::ToolCall)>,
}

/// What a consumed `declassify` approval lets through (R89): exactly the
/// effect whose canonical bytes have this SHA-256, for the call it bound,
/// while that call runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Released {
    pub sha256: String,
    /// The delegation id the blocked brief carried, sent under again.
    pub delegation: Option<String>,
    /// Who it let read: the readers the approved label adds (R193).
    pub readers: std::collections::BTreeSet<OwnedUserId>,
}

/// What a consumed `run` approval was checked against (R144, R213): its
/// `exec_binding` and the workspace set it releases. The execution it lets
/// go prepares again and runs only when both are still these — one fact set
/// from the check before `consumed` to the program's start.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ApprovedRun {
    pub exec_binding: Value,
    pub workspace: Option<Value>,
}

/// What became of a parked call when it was settled.
#[derive(Debug, Clone)]
pub(crate) enum Settled {
    /// Its approval was consumed here: it runs as its record bound it —
    /// the tool and the exact arguments, never the log's redacted copy
    /// (R174) — with the flow a `declassify` approval releases, and a
    /// `run`'s checked facts.
    Run(chat::ToolCall, Option<Released>, Option<Box<ApprovedRun>>),
    /// It does not run; the model is told this.
    Refuse(String),
}

/// A turn resuming at a parked call.
#[derive(Debug, Clone)]
pub(crate) struct Resume {
    pub approval: Ulid,
    pub settled: Settled,
    /// The parked call's `tool_call` line and wire call, while it has no
    /// result; `None` once it has one and only the round's rest is owed.
    pub call: Option<(Ulid, chat::ToolCall)>,
    /// The round's calls after it with no result yet, their lines written,
    /// their arguments as the model sent them (the round file's).
    pub rest: Vec<(Ulid, chat::ToolCall)>,
    /// Whether the rest runs: the parked call ran here.
    pub run_rest: bool,
    /// What the first of the rest is told when a stop cut the
    /// continuation while it may have run: never run again (R176).
    pub uncertain: Option<String>,
    /// The rest's call ids the round file does not hold: refused.
    pub unbound: Vec<String>,
    /// A person's note with a decision, as a `peer` line after the results.
    pub note: Option<PeerBody>,
    /// The event the continuation's anchor names.
    pub question: OwnedEventId,
}

/// An approval of this session whose round is not whole yet: its call, or a
/// call after it in its round, has no result (R176).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pending {
    pub id: String,
    pub call_id: String,
    pub call_line: Ulid,
    /// The request's event, when it reached the room.
    pub request_event: Option<OwnedEventId>,
    /// Its terminal line's state, once it has one: the call never runs
    /// again, only its result is owed.
    pub ended: Option<ApprovalState>,
    /// The round's call ids, the parked call first, that have no result.
    pub round: Vec<String>,
    /// Whether its `approval requested` line is written; a record whose
    /// line a stop lost is announced again at serve start (R176).
    pub announced: bool,
    /// Whether its call was consumed here and run: the rest of its round
    /// runs too.
    pub ran: bool,
}

impl Pending {
    /// Whether the parked call itself still has no result.
    pub fn call_open(&self) -> bool {
        self.round.contains(&self.call_id)
    }
}

/// One call of a parked round as the model sent it, before the log's
/// redaction: `approvals/<ulid>.round.json` keeps the round's later calls,
/// written before the record, so what runs after approval is what was
/// asked (R174, R176).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoundCall {
    line: String,
    call_id: String,
    tool: String,
    args: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct RoundFile {
    v: u32,
    id: String,
    calls: Vec<RoundCall>,
    /// The scheduled card whose run parked, session-relative: the
    /// continuation ends that run on the card and in the log (R178).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    scheduled: Option<String>,
}

/// One approver a request that its room could not carry reached, and the
/// proxy DM it went into.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AskedDm {
    person: OwnedUserId,
    dm: OwnedRoomId,
}

/// `approvals/<ulid>.asked.json`: who a request sent to proxy DMs reached
/// and where (R85, R89), kept apart from the room's read cursor — the
/// return route a restart rebuilds for every approval still waiting.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AskedFile {
    v: u32,
    id: String,
    asked: Vec<AskedDm>,
}

/// `<session>/approvals`, a real folder of the session — made when `make` —
/// whose link to anywhere else is refused, never followed (the log's own
/// discipline, R180): a record or a blob holds the exact action.
fn approvals_dir(deps: &AgentDeps, session: &str, make: bool) -> std::io::Result<PathBuf> {
    contained(&deps.sessions_zone.join(session).join(APPROVALS_DIR), make)
}

/// `dir` when it is a real folder (made when `make`); a link is refused.
fn contained(dir: &Path, make: bool) -> std::io::Result<PathBuf> {
    let checked = if make {
        keeper_core::agents::log::writer::real_dir(dir)
    } else {
        keeper_core::agents::log::writer::refuse_symlink(dir).map(drop)
    };
    checked.map_err(|error| std::io::Error::other(error.to_string()))?;
    Ok(dir.to_path_buf())
}

/// The bytes of `name` in the store's folder `dir`; a link is not followed.
fn read_in(dir: &Path, name: &str) -> std::io::Result<Vec<u8>> {
    let path = dir.join(name);
    if fs::symlink_metadata(&path)?.file_type().is_symlink() {
        return Err(std::io::Error::other(format!(
            "{} is a link, which keeper does not follow",
            path.display()
        )));
    }
    fs::read(path)
}

/// [`read_in`], as text.
fn read_text_in(dir: &Path, name: &str) -> std::io::Result<String> {
    String::from_utf8(read_in(dir, name)?).map_err(std::io::Error::other)
}

/// The record `id` in the approvals store of the session at `session_dir`,
/// read strictly with its arguments: a store, record or blob that is a link
/// is refused, never followed, and a record naming another id than its file
/// is refused. The one reader of every store — the worker's and the
/// consolidator's.
pub(crate) fn read_stored_record(
    session_dir: &Path,
    id: &str,
) -> Result<(ApprovalRecord, Value), String> {
    let (record, whole) = read_stored_whole(session_dir, id)?;
    Ok((record, whole.action.args))
}

/// [`read_stored_record`]'s record `id` as it is stored — what its request
/// is announced from, its large payload an attachment — and as its digest
/// is over it, with what [`ApprovalRecord::externalise_args`] moved out read
/// back.
pub(crate) fn read_stored_whole(
    session_dir: &Path,
    id: &str,
) -> Result<(ApprovalRecord, ApprovalRecord), String> {
    let unread = |error: std::io::Error| format!("the approval record could not be read: {error}");
    let dir = contained(&session_dir.join(APPROVALS_DIR), false).map_err(unread)?;
    let text = read_text_in(&dir, &format!("{id}.json")).map_err(unread)?;
    let record = parse_record(&text).map_err(|refusal| refusal.to_string())?;
    if record.id != id {
        return Err("the approval record names another id than its file".to_owned());
    }
    let blob = match &record.action.args_blob {
        Some(sha) => Some(
            contained(&dir.join("blobs"), false)
                .and_then(|blobs| read_text_in(&blobs, &format!("{sha}.json")))
                .map_err(|error| format!("the approval's arguments could not be read: {error}"))?,
        ),
        None => None,
    };
    let whole = record
        .whole(blob.as_deref())
        .ok_or_else(|| "the approval's arguments do not match their digest".to_owned())?;
    Ok((record, whole))
}

/// The decision written beside `id`'s record in the store of the session at
/// `session_dir`, when one reads — through no link.
pub(crate) fn read_stored_decision(session_dir: &Path, id: &str) -> Option<DecisionRecord> {
    let dir = contained(&session_dir.join(APPROVALS_DIR), false).ok()?;
    let text = read_text_in(&dir, &format!("{id}.decision.json")).ok()?;
    parse_decision(&text)
        .ok()
        .filter(|decision| decision.id == id)
}

/// The ids of the records in the store of the session at `session_dir`:
/// regular files only, a linked store none.
pub(crate) fn stored_ids(session_dir: &Path) -> Vec<String> {
    let Ok(dir) = contained(&session_dir.join(APPROVALS_DIR), false) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut ids: Vec<String> = entries
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_file()))
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let id = name.strip_suffix(".json")?;
            Ulid::from_string(id).ok().map(|_| id.to_owned())
        })
        .collect();
    ids.sort();
    ids
}

/// Write `bytes` as `dir/name` (a folder [`contained`] made), once and
/// whole: into a create-new file beside it, `fsync`ed, then hard-linked to
/// `name` — which fails when anything, a link included, is there already
/// (`rename` would overwrite) — then the folder `fsync`ed. A write that
/// fails leaves nothing at `name`, so a later try is not refused by a
/// partial file.
fn write_once(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    let part = dir.join(format!(".{name}.{}.part", Ulid::new()));
    let written = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::hard_link(&part, dir.join(name))
    })();
    let _ = fs::remove_file(&part);
    written?;
    File::open(dir)?.sync_all()
}

/// Write `bytes` as `dir/name` whole, replacing what is there: into a
/// create-new file beside it, `fsync`ed, renamed over `name`, then the
/// folder `fsync`ed.
fn write_replacing(dir: &Path, name: &str, bytes: &[u8]) -> std::io::Result<()> {
    let part = dir.join(format!(".{name}.{}.part", Ulid::new()));
    let written = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&part)?;
        file.write_all(bytes)?;
        file.sync_all()?;
        fs::rename(&part, dir.join(name))
    })();
    if written.is_err() {
        let _ = fs::remove_file(&part);
    }
    written?;
    File::open(dir)?.sync_all()
}

/// The SHA-256 of the first `through` bytes of the chunk `chunk` of the
/// session at `session_dir`.
fn chunk_sha256(session_dir: &Path, chunk: &str, through: u64) -> std::io::Result<String> {
    let bytes = fs::read(
        session_dir
            .join(keeper_core::agents::log::LOG_DIR)
            .join(chunk),
    )?;
    let end = usize::try_from(through)
        .unwrap_or(usize::MAX)
        .min(bytes.len());
    Ok(approval::sha256_hex(&bytes[..end]))
}

/// Whether `checkpoint` still names the session's log as it was: the
/// SHA-256 of its chunk through the end of the line it names (R174). The
/// calls a resume trusts are in those bytes; a log edited since is drift.
fn checkpoint_holds(session_dir: &Path, checkpoint: &Checkpoint) -> bool {
    let Ok(bytes) = fs::read(session_dir.join(&checkpoint.chunk)) else {
        return false;
    };
    let mut end = 0;
    for line in bytes.split_inclusive(|byte| *byte == b'\n') {
        end += line.len();
        let named = serde_json::from_slice::<Value>(line)
            .ok()
            .is_some_and(|value| value["id"].as_str() == Some(checkpoint.through.as_str()));
        if named {
            return approval::sha256_hex(&bytes[..end]) == checkpoint.sha256;
        }
    }
    false
}

/// Each pinned file where it lands now — keeper-sync's canonical
/// drive-relative landing, through every link — and its SHA-256, `None`
/// where it does not exist (never path arithmetic here).
pub(crate) fn pin_files(profiles: &[SyncProfile], pins: &[(String, String)]) -> Vec<FilePin> {
    pins.iter()
        .map(|(drive, path)| {
            let root = profiles
                .iter()
                .find(|profile| &profile.id == drive)
                .map(|profile| &profile.local_path);
            FilePin {
                drive: drive.clone(),
                path: path.clone(),
                landing: root
                    .and_then(|root| keeper_sync::browse::landing(root, path).ok())
                    .map(|names| names.join("/")),
                sha256: root
                    .and_then(|root| keeper_sync::browse::resolve(root, path).ok().flatten())
                    .and_then(|file| fs::read(file).ok())
                    .map(|bytes| approval::sha256_hex(&bytes)),
            }
        })
        .collect()
}

fn line(id: &str, state: ApprovalState) -> ApprovalBody {
    ApprovalBody {
        id: id.to_owned(),
        state,
        decision: None,
        by: None,
        result: None,
        reason: None,
        scope: None,
    }
}

fn question_of(id: &str) -> OwnedEventId {
    OwnedEventId::try_from(format!(
        "$approval-{}:keeper.invalid",
        id.to_ascii_lowercase()
    ))
    .unwrap_or_else(|_| matrix_sdk::ruma::event_id!("$approval:keeper.invalid").to_owned())
}

impl ServedSession {
    /// The session's approvals folder, a real one (made when `make`).
    fn approvals(&self, deps: &AgentDeps, make: bool) -> std::io::Result<PathBuf> {
        approvals_dir(deps, &self.context.session.path, make)
    }

    /// The record `id`, read strictly, as its digest is over it (its
    /// attached payload read back), with its arguments.
    fn read_record(&self, deps: &AgentDeps, id: &str) -> Result<(ApprovalRecord, Value), String> {
        let (_, whole) = self.read_stored(deps, id)?;
        let args = whole.action.args.clone();
        Ok((whole, args))
    }

    /// The record `id` as it is stored and as its digest is over it
    /// ([`read_stored_whole`]).
    fn read_stored(
        &self,
        deps: &AgentDeps,
        id: &str,
    ) -> Result<(ApprovalRecord, ApprovalRecord), String> {
        read_stored_whole(&deps.sessions_zone.join(&self.context.session.path), id)
    }

    /// The round file of `id`: the parked round's later calls as the model
    /// sent them; `None` when it cannot be read.
    fn read_round(&self, deps: &AgentDeps, id: &str) -> Option<RoundFile> {
        let dir = self.approvals(deps, false).ok()?;
        let text = read_text_in(&dir, &format!("{id}.round.json")).ok()?;
        serde_json::from_str::<RoundFile>(&text)
            .ok()
            .filter(|round| round.v == approval::RECORD_VERSION && round.id == id)
    }

    /// The decision written beside `id`'s record, when one reads.
    fn read_decision(&self, deps: &AgentDeps, id: &str) -> Option<DecisionRecord> {
        read_stored_decision(&deps.sessions_zone.join(&self.context.session.path), id)
    }

    /// The proxy DMs the request of `id` reached, by approver; none when its
    /// request went into the room, or its asked file does not read.
    pub(crate) fn asked(&self, deps: &AgentDeps, id: &str) -> Vec<(OwnedUserId, OwnedRoomId)> {
        let Ok(dir) = self.approvals(deps, false) else {
            return Vec::new();
        };
        read_text_in(&dir, &format!("{id}.asked.json"))
            .ok()
            .and_then(|text| serde_json::from_str::<AskedFile>(&text).ok())
            .filter(|file| file.v == approval::RECORD_VERSION && file.id == id)
            .map(|file| {
                file.asked
                    .into_iter()
                    .map(|asked| (asked.person, asked.dm))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Keep who the request of `id` reached, and in which DM, beside its
    /// record: whole, replacing what an earlier announcement of it kept.
    fn keep_asked(
        &self,
        deps: &AgentDeps,
        id: &str,
        asked: &[(OwnedUserId, OwnedRoomId)],
    ) -> std::io::Result<()> {
        let dir = self.approvals(deps, false)?;
        let text = serde_json::to_string_pretty(&AskedFile {
            v: approval::RECORD_VERSION,
            id: id.to_owned(),
            asked: asked
                .iter()
                .map(|(person, dm)| AskedDm {
                    person: person.clone(),
                    dm: dm.clone(),
                })
                .collect(),
        })
        .map_err(std::io::Error::other)?;
        crate::agent::off_the_runtime(|| {
            write_replacing(&dir, &format!("{id}.asked.json"), text.as_bytes())
        })
    }

    /// Leave nothing of the park `id` nobody could be asked about: its
    /// record, round and asked files, and its timer — no pending record
    /// stays to be announced again or expired (R194).
    fn forget(&mut self, deps: &AgentDeps, id: &str) {
        self.due.remove(id);
        let Ok(dir) = self.approvals(deps, false) else {
            return;
        };
        for name in [
            format!("{id}.json"),
            format!("{id}.round.json"),
            format!("{id}.asked.json"),
        ] {
            match fs::remove_file(dir.join(&name)) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    tracing::warn!(%error, approval = id, file = %name, "agents: a park nobody could be asked about left a file");
                }
                _ => {}
            }
        }
    }

    /// What a person let the brief of delegation `open` carry (R191a,
    /// R193): the readers its approved label adds and the SHA-256 of the
    /// bytes they approved, from the `declassify` record of this session
    /// that opened it. The brief goes in at the target's join, after the
    /// approval was spent, so the binding is checked again here: the record
    /// is this session's and agent's under its own id, it binds this
    /// delegation's very `delegate` call, the decision beside it approves
    /// the digest the record's arguments recompute to, and that approval
    /// was consumed here. Any drift lets nothing through: the brief goes
    /// under the session's own label, which the room's check then refuses.
    pub(crate) fn declassified(
        &self,
        deps: &AgentDeps,
        open: &crate::delegate::Delegation,
    ) -> Option<Released> {
        let session = format!("{}/{}", deps.sessions_subfolder, self.context.session.path);
        let entries = self.approvals(deps, false).and_then(fs::read_dir).ok()?;
        entries.filter_map(Result::ok).find_map(|entry| {
            let name = entry.file_name().to_string_lossy().into_owned();
            let id = name
                .strip_suffix(".json")
                .filter(|id| Ulid::from_string(id).is_ok())?;
            let (record, args) = self.read_record(deps, id).ok()?;
            if record.action.tool != AgentTool::Declassify.as_wire()
                || args["delegation"].as_str() != Some(open.id.as_str())
            {
                return None;
            }
            let digest = record.recomputed_digest(&args).ok();
            let held = record.id == id
                && record.session == session
                && record.agent == deps.home.config.id
                && args["call"]["tool"].as_str() == Some(crate::delegate::DELEGATE)
                && args["call"]["arguments"].as_str() == open.args.as_deref()
                && self.context.consumed.contains(id)
                && self.read_decision(deps, id).is_some_and(|decision| {
                    decision.id == id
                        && decision.decision == Decision::Approve
                        && Some(&decision.binding_digest) == digest.as_ref()
                });
            if !held {
                tracing::warn!(approval = id, delegation = %open.id, "agents: a declassified brief's approval no longer binds it; it is not let through");
                return None;
            }
            bound_call(&record, args).1
        })
    }

    /// Park `parked`, the call its turn ended on (93.2 AC1): a scheduled
    /// run's card says `run: blocked` first (R178), then the checkpoint,
    /// the round file, the record, the request, the line and the status, in
    /// that order — a stop after the record leaves it to be announced again
    /// at serve start (R176). `Err` names what could not be done; the caller
    /// then refuses the call.
    pub(crate) async fn park(
        &mut self,
        deps: &AgentDeps,
        port: &Arc<dyn EditPort>,
        parked: &ParkedTurn,
    ) -> Result<(), String> {
        fn failed(error: impl std::fmt::Display) -> String {
            error.to_string()
        }
        let preconditions = self.block_scheduled(deps, parked);
        crate::agent::off_the_runtime(|| self.writer.sync()).map_err(failed)?;
        let place = self
            .writer
            .last_place()
            .cloned()
            .ok_or_else(|| "the session's log has no line to check against".to_owned())?;
        let session_dir = deps.sessions_zone.join(&self.context.session.path);
        let sha = chunk_sha256(&session_dir, &place.chunk, place.through_bytes).map_err(failed)?;
        let id = parked.parking.approval.to_string();
        let session = format!("{}/{}", deps.sessions_subfolder, self.context.session.path);
        let agent = &self.context.agent;
        let mut record = ApprovalRecord::new(RecordParking {
            id: &id,
            created_at: Utc::now(),
            session: &session,
            session_kind: agent.kind,
            agent: &deps.home.config.id,
            drive: &deps.home.config.drive,
            host: deps.host.as_str(),
            epoch: self.context.epoch,
            call: CallRef {
                line: parked.call_line.to_string(),
                call_id: parked.parking.call_id.clone(),
            },
            dispatch_chain: agent
                .dispatch_chain
                .iter()
                .map(|user| user.to_string())
                .collect(),
            checkpoint: Checkpoint {
                chunk: format!("{}/{}", keeper_core::agents::log::LOG_DIR, place.chunk),
                through: place.line.to_string(),
                sha256: sha,
            },
            args: &parked.args,
            exec_binding: parked.parking.exec_binding.clone(),
            classification: &parked.parking.classification,
            label: &self.context.label,
            preconditions,
        })
        .map_err(failed)?;
        let dir = self.approvals(deps, true).map_err(failed)?;
        let blob = record.externalise_args();
        let text = serde_json::to_string_pretty(&record).map_err(failed)?;
        let round = serde_json::to_string_pretty(&RoundFile {
            v: approval::RECORD_VERSION,
            id: id.clone(),
            calls: parked
                .rest
                .iter()
                .map(|(line, wire)| RoundCall {
                    line: line.to_string(),
                    call_id: wire.id.clone(),
                    tool: wire.name.clone(),
                    args: wire.arguments_raw.clone(),
                })
                .collect(),
            scheduled: self.scheduled_card.clone(),
        })
        .map_err(failed)?;
        crate::agent::off_the_runtime(|| {
            write_once(&dir, &format!("{id}.round.json"), round.as_bytes())?;
            if let Some((sha, bytes)) = &blob {
                let blobs = contained(&dir.join("blobs"), true)?;
                match write_once(&blobs, &format!("{sha}.json"), bytes.as_bytes()) {
                    Err(error) if error.kind() != std::io::ErrorKind::AlreadyExists => {
                        return Err(error)
                    }
                    _ => {}
                }
            }
            write_once(&dir, &format!("{id}.json"), text.as_bytes())
        })
        .map_err(failed)?;
        self.due.insert(id.clone(), record.expires());
        self.announce(deps, port, &record, parked.call_line).await
    }

    /// A scheduled run's card says `run: blocked` before its park pins
    /// anything for good (R178): the host's own write is no drift. A pin of
    /// the card is taken again only when that write explains its change
    /// exactly — its bytes before were the pinned ones and its bytes now
    /// are what the write made — so a change by anyone else stays drift.
    fn block_scheduled(&mut self, deps: &AgentDeps, parked: &ParkedTurn) -> Preconditions {
        let mut preconditions = parked.preconditions.clone();
        let Some(card) = self.scheduled_card.clone() else {
            return preconditions;
        };
        let (zone, session) = (
            deps.sessions_zone.clone(),
            self.context.session.path.clone(),
        );
        let lease = self.writer.lease();
        let may_write = move || lease.as_ref().is_none_or(|lease| lease.may_write());
        let written = crate::agent::off_the_runtime(|| {
            crate::cards::block_run(&zone, &session, &card, &may_write)
        });
        let (before, after) = match written {
            Ok(Some(seen)) => seen,
            Ok(None) => return preconditions,
            Err(error) => {
                tracing::warn!(%session, %card, %error, "agents: a parked run's card could not be set blocked");
                return preconditions;
            }
        };
        let (was, now) = (
            approval::sha256_hex(before.as_bytes()),
            approval::sha256_hex(after.as_bytes()),
        );
        let profiles = deps
            .env
            .drive
            .as_ref()
            .map(|drive| drive.profiles.profiles())
            .unwrap_or_default();
        for (pin, fresh) in preconditions
            .files
            .iter_mut()
            .zip(pin_files(&profiles, &parked.parking.pins))
        {
            if pin.sha256.as_deref() == Some(was.as_str())
                && fresh.sha256.as_deref() == Some(now.as_str())
                && fresh.landing == pin.landing
            {
                pin.sha256 = fresh.sha256;
            }
        }
        preconditions
    }

    /// Ask for a decision on `record`, durable on disk, whose call is the
    /// `tool_call` line `call_line`: the request, its large arguments an
    /// encrypted file (R86); then the `approval requested` line and the
    /// status. Every attempt to send it into the room asks the room's gate
    /// as the room is then; a room that cannot carry it — at the first
    /// attempt or at a retry — never gets it, and it goes to the approvers'
    /// proxy DMs instead (R85, R175), the room's status sent first so its
    /// event is where the room is read forward from (R179). A request whose
    /// arguments could not be attached is never sent: one without them
    /// could be approved unseen (`Err` is [`UNATTACHED`], R180).
    async fn announce(
        &mut self,
        deps: &AgentDeps,
        port: &Arc<dyn EditPort>,
        record: &ApprovalRecord,
        call_line: Ulid,
    ) -> Result<(), String> {
        fn failed(error: impl std::fmt::Display) -> String {
            error.to_string()
        }
        let file = match &record.action.args_blob {
            Some(sha) => {
                let bytes = self
                    .approvals(deps, false)
                    .and_then(|dir| contained(&dir.join("blobs"), false))
                    .and_then(|blobs| read_in(&blobs, &format!("{sha}.json")))
                    .map_err(failed)?;
                let uploaded = match &self.approval_room {
                    Some(room) => room.upload(bytes).await,
                    None => Err(AgentMatrixError::Other("no room to upload to".to_owned())),
                };
                match uploaded {
                    Ok(file) => Some(file),
                    Err(error) => {
                        tracing::warn!(%error, approval = %record.id, "agents: an approval's arguments could not be uploaded; its request is not sent");
                        return Err(UNATTACHED.to_owned());
                    }
                }
            }
            None => None,
        };
        // The consolidator's record is decided by the approvers its digest
        // binds, while they are still who they should be (R206); a parked
        // call asks the session's readers.
        let bound;
        let readers = if keeper_core::agents::consolidate::is_host_action(record) {
            let (_, args) = self.read_record(deps, &record.id)?;
            bound = Readers::Only(self.host_action_approvers(deps, record, &args)?);
            &bound
        } else {
            &self.context.label.readers
        };
        let approvers = match readers {
            Readers::Only(set) => set.iter().map(|user| user.to_string()).collect(),
            Readers::Anyone => Vec::new(),
        };
        let content = request_content(record, self.context.agent.room.as_ref(), approvers, file);
        let value = serde_json::to_value(&content).map_err(failed)?;
        let gate = self.gate(deps, port);
        // A declassification is decided in its approvers' proxy DMs, never
        // in the session's room (AD-391, R89).
        let admitted = if record.action.tool == AgentTool::Declassify.as_wire() {
            Err(String::new())
        } else {
            gate.admit(APPROVAL_REQUEST, value.to_string().as_bytes())
                .await
        };
        let request_event = match &admitted {
            Ok(()) => crate::matrix_sink::deliver_unless_narrowed(
                port.as_ref(),
                APPROVAL_REQUEST,
                &gate,
                &|narrowed| (!narrowed).then(|| value.clone()),
                Instant::now(),
            )
            .await
            .map(|(sent, _)| sent),
            Err(_) => None,
        };
        let detail = format!("{WAITING_FOR} {}", record.action.summary);
        let (request_event, said) = match request_event {
            Some(event) => (event, false),
            None => {
                // The room is wider than the label (R85), or became so while
                // a send was retried: the request goes to each approver's
                // proxy DM, and the room's status says only R64's fixed
                // sentence. That status is sent first: a decision comes only
                // after the request, so any `consumed` of it is after this
                // event in the room — the read forward begins here, never at
                // the room's start (R179). Where no approver's proxy runs on
                // this host nobody can be asked, and nothing waits (R194).
                if self.askable().is_empty() {
                    self.forget(deps, &record.id);
                    return Err(NOBODY_TO_ASK.to_owned());
                }
                if admitted.is_ok() {
                    gate.suppressed(APPROVAL_REQUEST);
                }
                tracing::info!(approval = %record.id, "agents: an approval's request may not go into its room; it goes to its approvers' DMs");
                let cursor = self
                    .send_status(deps, port, RunState::Blocked, Some(&detail))
                    .await;
                let asked = self.request_by_doors(deps, &value).await;
                // Nobody it reached is nobody asked: it is not announced.
                if asked.is_empty() {
                    self.forget(deps, &record.id);
                    return Err(NOBODY_TO_ASK.to_owned());
                }
                // Where each decision comes home from, kept apart from the
                // room's read cursor so a restart hears those DMs again.
                if let Err(error) = self.keep_asked(deps, &record.id, &asked) {
                    self.forget(deps, &record.id);
                    return Err(error.to_string());
                }
                (cursor, true)
            }
        };
        self.writer
            .write(
                &mut self.context,
                Some(call_line),
                Some(request_event),
                LineBody::Approval(line(&record.id, ApprovalState::Requested)),
            )
            .map_err(failed)?;
        crate::agent::off_the_runtime(|| self.writer.sync()).map_err(failed)?;
        if !said {
            self.send_status(deps, port, RunState::Blocked, Some(&detail))
                .await;
        }
        Ok(())
    }

    /// Every record under `approvals/` whose call is still open in the log
    /// but which has no `approval requested` line — a stop came between the
    /// record and its line — waits as a park not yet announced (R176):
    /// [`Self::resume_approvals`] announces it again, or expires it.
    pub(crate) fn adopt_records(&mut self, deps: &AgentDeps) {
        let Ok(entries) = self.approvals(deps, false).and_then(fs::read_dir) else {
            return;
        };
        let ids: Vec<String> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let name = entry.file_name().into_string().ok()?;
                let id = name.strip_suffix(".json")?;
                Ulid::from_string(id).ok().map(|_| id.to_owned())
            })
            .filter(|id| !self.context.parked.contains_key(id))
            .collect();
        for id in ids {
            let Ok((record, args)) = self.read_record(deps, &id) else {
                continue;
            };
            let Ok(call_line) = Ulid::from_string(&record.call.line) else {
                continue;
            };
            if keeper_core::agents::consolidate::is_host_action(&record) {
                self.adopt_host_action(deps, id, &record, &args, call_line);
                continue;
            }
            let round = self.context.calls_from(call_line);
            if round.first().map(|(_, wire)| &wire.id) != Some(&record.call.call_id) {
                continue;
            }
            self.context.parked.insert(
                id.clone(),
                Pending {
                    id,
                    call_id: record.call.call_id.clone(),
                    call_line,
                    request_event: None,
                    ended: None,
                    round: round.into_iter().map(|(_, wire)| wire.id).collect(),
                    announced: false,
                    ran: false,
                },
            );
        }
    }

    /// Who may decide `record`, one of the consolidator's host actions
    /// bound to `args`: the approvers its digest binds, while they are
    /// exactly who the drive's owner and readers and the source sessions'
    /// requesters make them now ([`ApplyArgs::approvers_now`]) — checked
    /// when it is adopted, announced and decided (R206). Never its label:
    /// that is the data's.
    pub(crate) fn host_action_approvers(
        &self,
        deps: &AgentDeps,
        record: &ApprovalRecord,
        args: &Value,
    ) -> Result<std::collections::BTreeSet<OwnedUserId>, String> {
        use keeper_core::agents::consolidate::{ApplyArgs, APPROVERS_MOVED};
        let args: ApplyArgs =
            serde_json::from_value(args.clone()).map_err(|_| APPROVERS_MOVED.to_owned())?;
        let decl = deps
            .drives
            .get(&record.drive)
            .ok_or_else(|| APPROVERS_MOVED.to_owned())?;
        let facts = |path: &str| {
            crate::zone::session_facts(&deps.drive_root, &deps.sessions_subfolder, path)
        };
        args.approvers_now(&decl.owner, &decl.readers, &facts)
    }

    /// A record the consolidator wrote into this review session (R128): its
    /// action parks no model call, so the log holds no `tool_call` for it.
    /// It waits from serve start on — announced already when the log says
    /// so, carried by its own lines after a restart — until it ends; the
    /// worker asks its approvers and writes their decision, consumes it once
    /// as every approval is consumed, and the consolidator carries out only
    /// a consumed one. One whose approvers are no longer who they should be
    /// is refused here, for good.
    fn adopt_host_action(
        &mut self,
        deps: &AgentDeps,
        id: String,
        record: &ApprovalRecord,
        args: &Value,
        call_line: Ulid,
    ) {
        let session_dir = deps.sessions_zone.join(&self.context.session.path);
        let mut request_event = None;
        let mut announced = false;
        for line in keeper_core::agents::log::reader::read_session(&session_dir).lines {
            let LineBody::Approval(body) = &line.body else {
                continue;
            };
            if body.id != id {
                continue;
            }
            if body.is_terminal() {
                return;
            }
            if body.state == ApprovalState::Requested {
                announced = true;
                request_event = line.matrix_event.clone();
            }
        }
        let mut pending = Pending {
            id: id.clone(),
            call_id: record.call.call_id.clone(),
            call_line,
            request_event,
            ended: None,
            round: vec![record.call.call_id.clone()],
            announced,
            ran: false,
        };
        if let Err(reason) = self.host_action_approvers(deps, record, args) {
            tracing::warn!(approval = %id, %reason, "agents: a memory review's approvers no longer hold; it is refused");
            let mut body = line(&id, ApprovalState::Refused);
            body.reason = Some(reason);
            if !self.end(&pending, body) {
                return;
            }
            pending.ended = Some(ApprovalState::Refused);
        }
        self.context.parked.insert(id, pending);
    }

    /// The earliest moment one of this session's pending approvals expires.
    pub(crate) fn next_expiry(&self) -> Option<DateTime<Utc>> {
        self.context
            .parked
            .values()
            .filter(|pending| pending.ended.is_none())
            .filter_map(|pending| self.due.get(&pending.id))
            .min()
            .copied()
    }

    /// Whether a call of this session waits for a person now.
    pub fn waiting(&self) -> bool {
        self.context
            .parked
            .values()
            .any(|pending| pending.ended.is_none())
    }

    /// At serve start, after a restart or a takeover (93.2 AC5): every
    /// approval whose round is not whole is settled as far as it can be —
    /// a record whose line a stop lost is announced again or expires, a
    /// continuation a stop cut gets the rest of its results, one already
    /// ended gets its result and the turn goes on, one with a decision
    /// written beside it resumes, one past its time expires — and the rest
    /// stay parked, their expiry timed (R84, R176).
    pub async fn resume_approvals(
        &mut self,
        deps: &AgentDeps,
        port: Arc<dyn EditPort>,
        stop: CancelSignal,
    ) {
        self.adopt_records(deps);
        let pending: Vec<Pending> = self.context.parked.values().cloned().collect();
        for pending in pending {
            let record = self.read_stored(deps, &pending.id).ok();
            if let Some((record, _)) = &record {
                self.due.insert(pending.id.clone(), record.expires());
            }
            if !pending.call_open() {
                // A stop cut the round's continuation: the rest is owed.
                self.continue_parked(deps, &port, &pending, None, None, stop.clone())
                    .await;
                continue;
            }
            if !pending.announced {
                if let Some(settled) = self.expire_if_due(deps, &pending).await {
                    self.continue_parked(deps, &port, &pending, Some(settled), None, stop.clone())
                        .await;
                } else if let Some((record, _)) = &record {
                    if let Err(error) = self.announce(deps, &port, record, pending.call_line).await
                    {
                        tracing::error!(approval = %pending.id, %error, "agents: a parked call's request could not be announced again");
                        if error == UNATTACHED
                            || error == NOBODY_TO_ASK
                            || error == keeper_core::agents::consolidate::APPROVERS_MOVED
                        {
                            // Nobody can be asked — without what it asks, or
                            // with no approver's proxy here: refused, and
                            // its turn goes on (R180, R194).
                            let mut body = line(&pending.id, ApprovalState::Refused);
                            body.reason = Some(error.clone());
                            if self.end(&pending, body) {
                                let settled = Settled::Refuse(error);
                                self.continue_parked(
                                    deps,
                                    &port,
                                    &pending,
                                    Some(settled),
                                    None,
                                    stop.clone(),
                                )
                                .await;
                            }
                        }
                    }
                }
                continue;
            }
            let decision = self.read_decision(deps, &pending.id);
            let settled = match (pending.ended, &decision) {
                (Some(ApprovalState::Consumed), _) => {
                    Some(Settled::Refuse(effect_unknown(deps.host.as_str())))
                }
                (Some(ApprovalState::Expired), _) => Some(Settled::Refuse(EXPIRED.to_owned())),
                (Some(_), _) => Some(Settled::Refuse(DENIED.to_owned())),
                (None, Some(decision)) => self.settle(deps, &pending, decision).await,
                (None, None) => self.expire_if_due(deps, &pending).await,
            };
            if let Some(settled) = settled {
                let note = decision.as_ref().and_then(note_of);
                self.continue_parked(deps, &port, &pending, Some(settled), note, stop.clone())
                    .await;
            }
        }
    }

    /// `pending` ended when its time has passed (R179), the room read first:
    /// a `consumed` of it from any copy means it was used, its effect
    /// unknown — never "nothing was changed"; a read that stopped at its
    /// bound ends it refused, its effect unknown too; a read that failed
    /// leaves it for a later try. Only a room read whole and showing no
    /// `consumed` expires it: undecided, or approved and never used.
    async fn expire_if_due(&mut self, deps: &AgentDeps, pending: &Pending) -> Option<Settled> {
        let due = self.due.get(&pending.id).copied()?;
        let now = Utc::now();
        if now < due {
            return None;
        }
        if let Some(room) = self.approval_room.clone() {
            match room
                .consumed(&pending.id, pending.request_event.as_deref())
                .await
            {
                Ok(read) => {
                    if let Some(first) = read.consumed.first() {
                        return self.spent(pending, first);
                    }
                    if !read.complete {
                        let mut body = line(&pending.id, ApprovalState::Refused);
                        body.reason = Some(HISTORY_UNREAD.to_owned());
                        return self
                            .end(pending, body)
                            .then(|| Settled::Refuse(HISTORY_UNREAD_RESULT.to_owned()));
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, approval = %pending.id, "agents: the room could not be read at expiry; it is tried again");
                    let again = chrono::Duration::from_std(crate::runtime::TICK)
                        .unwrap_or_else(|_| chrono::Duration::seconds(1));
                    self.due.insert(pending.id.clone(), now + again);
                    return None;
                }
            }
        }
        let approved = self
            .read_decision(deps, &pending.id)
            .is_some_and(|decision| decision.decision == Decision::Approve);
        let said = if approved { APPROVED_UNUSED } else { EXPIRED };
        self.settling.remove(&pending.id);
        self.end(pending, line(&pending.id, ApprovalState::Expired))
            .then(|| Settled::Refuse(said.to_owned()))
    }

    /// `pending` was consumed in the room, first by `first`: mirrored as
    /// consumed with its result unknown, and told so — never run here.
    fn spent(&mut self, pending: &Pending, first: &Consumed) -> Option<Settled> {
        let mut body = line(&pending.id, ApprovalState::Consumed);
        body.result = Some("unknown".to_owned());
        body.by = Some(first.content.host.clone());
        let mirrored = self
            .writer
            .write(
                &mut self.context,
                Some(pending.call_line),
                Some(first.event.clone()),
                LineBody::Approval(body),
            )
            .and_then(|_| crate::agent::off_the_runtime(|| self.writer.sync()));
        if mirrored.is_err() {
            return None;
        }
        self.settling.remove(&pending.id);
        Some(Settled::Refuse(effect_unknown(&first.content.host)))
    }

    /// Every pending approval past its time expires, and its turn goes on
    /// (R84: the worker's timer, before any held arrival is served).
    pub(crate) async fn expire_due(
        &mut self,
        deps: &AgentDeps,
        port: &Arc<dyn EditPort>,
        stop: CancelSignal,
    ) {
        let pending: Vec<Pending> = self
            .context
            .parked
            .values()
            .filter(|pending| pending.ended.is_none())
            .cloned()
            .collect();
        for pending in pending {
            if let Some(settled) = self.expire_if_due(deps, &pending).await {
                self.continue_parked(deps, port, &pending, Some(settled), None, stop.clone())
                    .await;
            }
        }
    }

    /// Whether an approve decision is written here whose settlement did not
    /// finish — a send or a read failed — and waits for the worker's next
    /// try (R179).
    pub(crate) fn settling(&self) -> bool {
        !self.settling.is_empty()
    }

    /// Try again each settlement that did not finish, on the same worker,
    /// with no restart: the room first, then the checks, and a `consumed`
    /// this copy's server took already is not sent twice (R179).
    pub(crate) async fn retry_settlements(
        &mut self,
        deps: &AgentDeps,
        port: &Arc<dyn EditPort>,
        stop: CancelSignal,
    ) {
        let ids: Vec<String> = self.settling.keys().cloned().collect();
        for id in ids {
            let pending = self
                .context
                .parked
                .get(&id)
                .filter(|pending| pending.ended.is_none())
                .cloned();
            let (Some(pending), Some(decision)) = (pending, self.read_decision(deps, &id)) else {
                self.settling.remove(&id);
                continue;
            };
            if let Some(settled) = self.settle(deps, &pending, &decision).await {
                let note = note_of(&decision);
                self.continue_parked(deps, port, &pending, Some(settled), note, stop.clone())
                    .await;
            }
        }
    }

    /// Write `body` for `pending`, `fsync`ed; whether it was.
    fn end(&mut self, pending: &Pending, body: ApprovalBody) -> bool {
        let written = self
            .writer
            .write(
                &mut self.context,
                Some(pending.call_line),
                None,
                LineBody::Approval(body),
            )
            .and_then(|_| crate::agent::off_the_runtime(|| self.writer.sync()));
        if let Err(error) = &written {
            tracing::error!(%error, approval = %pending.id, "agents: an approval's line could not be written");
        }
        written.is_ok()
    }

    /// A person's decision arrived (R77): with a source installed, the
    /// holder of the claim — and only it — takes it when it counts
    /// (93.3: [`crate::deciding`]'s trust, then the record's
    /// admissibility), writes it as `<ulid>.decision.json` and the parked
    /// run resumes; one that does not count is logged `decided` with why,
    /// and nothing moves. One on an approval of this session's that ended
    /// already — run, denied, expired — is logged too (R80), and changes
    /// nothing.
    pub(crate) async fn decided(
        &mut self,
        deps: &AgentDeps,
        port: Arc<dyn EditPort>,
        arrived: Arrived,
        stop: CancelSignal,
    ) -> Result<Outcome, ServeError> {
        let Some(source) = deps.decisions.clone() else {
            return Ok(Outcome::Ignored(NO_SUCH_APPROVAL));
        };
        let Some(id) = arrived.content["id"]
            .as_str()
            .filter(|id| Ulid::from_string(id).is_ok())
            .map(str::to_owned)
        else {
            return Ok(Outcome::Ignored(NO_SUCH_APPROVAL));
        };
        let known = self.context.parked.get(&id).cloned();
        let pending = known.clone().filter(|pending| pending.ended.is_none());
        if pending.is_none() && known.is_none() && self.read_record(deps, &id).is_err() {
            return Ok(Outcome::Ignored(NO_SUCH_APPROVAL));
        }
        // A host that does not hold the claim writes nothing, not even why.
        if !self.holds_claim().await {
            tracing::info!(approval = %id, "agents: this host does not hold the claim; a decision is not taken here");
            return Ok(Outcome::Ignored(NOT_HOLDER));
        }
        let ignored = |reason: String| {
            let mut body = line(&id, ApprovalState::Decided);
            body.by = Some(arrived.sender.to_string());
            body.reason = Some(reason);
            LineBody::Approval(body)
        };
        let Some(pending) = pending else {
            tracing::info!(approval = %id, "agents: a decision on an approval that ended is ignored");
            self.writer.write(
                &mut self.context,
                known.map(|known| known.call_line),
                Some(arrived.event_id.clone()),
                ignored(APPROVAL_ENDED.to_owned()),
            )?;
            crate::agent::off_the_runtime(|| self.writer.sync())?;
            return Ok(Outcome::Duplicate);
        };
        let decision =
            match serde_json::from_value::<ApprovalDecisionContent>(arrived.content.clone()) {
                // Never the parser's words: they quote what the sender wrote.
                Err(_) => Err(UNREADABLE_DECISION.to_owned()),
                Ok(content) => match self.read_record(deps, &id) {
                    Err(reason) => Err(reason),
                    Ok((record, _)) => {
                        match self
                            .decided_by(deps, source.as_ref(), &record, &arrived)
                            .await
                        {
                            Err(reason) => Err(reason),
                            Ok(decided_by) => {
                                let decision = DecisionRecord {
                                    v: approval::RECORD_VERSION,
                                    id: content.id.clone(),
                                    decision: content.decision,
                                    scope: content.scope,
                                    note: content.note.clone(),
                                    binding_digest: content.binding_digest.clone(),
                                    decided_by,
                                    decided_at: approval::stamp(Utc::now()),
                                    matrix_event: Some(arrived.event_id.to_string()),
                                    written_by: WrittenBy {
                                        host: deps.host.as_str().to_owned(),
                                        epoch: self.context.epoch,
                                    },
                                };
                                decision
                                    .admissible(&record, Utc::now())
                                    .map(|()| decision)
                                    .map_err(|refusal| refusal.to_string())
                            }
                        }
                    }
                },
            };
        let decision = match decision {
            Ok(decision) => decision,
            Err(reason) => {
                tracing::info!(approval = %id, %reason, "agents: a decision is ignored");
                self.writer.write(
                    &mut self.context,
                    Some(pending.call_line),
                    Some(arrived.event_id.clone()),
                    ignored(reason),
                )?;
                crate::agent::off_the_runtime(|| self.writer.sync())?;
                return Ok(Outcome::Decided);
            }
        };
        // Only the holder of the session's claim publishes a decision, a
        // deny as an approve (R176) — read again now that the trust lookup
        // has returned, since the claim may have moved while it was asked
        // (R183): a host that lost it writes nothing.
        if !self.holds_claim().await {
            tracing::info!(approval = %id, "agents: this host does not hold the claim; it writes no decision");
            return Ok(Outcome::Ignored(NOT_HOLDER));
        }
        // Written once: a second decision finds the file, is logged, and
        // changes nothing.
        let text = serde_json::to_string_pretty(&decision).unwrap_or_default();
        let dir = match self.approvals(deps, false) {
            Ok(dir) => dir,
            Err(error) => {
                tracing::error!(approval = %id, %error, "agents: the approvals folder is not the session's own; no decision is written");
                return Ok(Outcome::Ignored(DECISION_UNWRITTEN));
            }
        };
        let name = format!("{id}.decision.json");
        // The lease, asked last with nothing awaited after it: the claim
        // read above awaited the server.
        if !self.writer.may_write() {
            tracing::info!(approval = %id, "agents: this host's lease lapsed; it writes no decision");
            return Ok(Outcome::Ignored(NOT_HOLDER));
        }
        match crate::agent::off_the_runtime(|| write_once(&dir, &name, text.as_bytes())) {
            Ok(()) => {}
            // Only a whole decision there is one taken already; anything
            // else at that name is a fault, and the event stays unseen so
            // its redelivery is taken once the fault clears.
            Err(error)
                if error.kind() == std::io::ErrorKind::AlreadyExists
                    && self.read_decision(deps, &id).is_some() =>
            {
                tracing::info!(approval = %id, "agents: a decision was written already");
                self.writer.write(
                    &mut self.context,
                    Some(pending.call_line),
                    Some(arrived.event_id.clone()),
                    ignored(ALREADY_DECIDED.to_owned()),
                )?;
                crate::agent::off_the_runtime(|| self.writer.sync())?;
                return Ok(Outcome::Duplicate);
            }
            Err(error) => {
                tracing::error!(approval = %id, error = ?error.kind(), "agents: a decision could not be written; the run stays parked");
                return Ok(Outcome::Ignored(DECISION_UNWRITTEN));
            }
        }
        let mut body = line(&id, ApprovalState::Decided);
        body.decision = Some(decision.decision.as_word().to_owned());
        body.by = Some(decision.decided_by.user.clone());
        body.scope = Some(decision.scope.as_word().to_owned());
        self.writer.write(
            &mut self.context,
            Some(pending.call_line),
            Some(arrived.event_id.clone()),
            LineBody::Approval(body),
        )?;
        crate::agent::off_the_runtime(|| self.writer.sync())?;
        let settled = match decision.decision {
            Decision::Deny => Some(Settled::Refuse(DENIED.to_owned())),
            Decision::Approve => self.settle(deps, &pending, &decision).await,
        };
        if let Some(settled) = settled {
            let note = note_of(&decision);
            self.continue_parked(deps, &port, &pending, Some(settled), note, stop)
                .await;
        }
        Ok(Outcome::Decided)
    }

    /// An approve decision on `pending`, in NFR-117's order: room →
    /// record → digest → preconditions → claim → `consumed` accepted and
    /// first ([`consume_once`]) → mirror line. `None`: it stays parked — a
    /// send or a read failed or stopped at its bound, or another host holds
    /// the claim — and the worker tries it again on its clock (R179).
    async fn settle(
        &mut self,
        deps: &AgentDeps,
        pending: &Pending,
        decision: &DecisionRecord,
    ) -> Option<Settled> {
        if decision.decision == Decision::Deny {
            return Some(Settled::Refuse(DENIED.to_owned()));
        }
        let room = self.approval_room.clone()?;
        let id = pending.id.as_str();
        // What this copy's server took already, on an earlier try.
        let sent = self.settling.get(id).cloned().flatten();
        let unknown = |this: &mut Self, sent: Option<OwnedEventId>| {
            this.settling.insert(id.to_owned(), sent);
            None
        };
        // The room first: any copy's `consumed` but this one's own means it
        // is spent; a read that stopped at its bound proves nothing yet.
        let read = match room.consumed(id, pending.request_event.as_deref()).await {
            Ok(read) => read,
            Err(error) => {
                tracing::warn!(%error, approval = id, "agents: the room could not be read; the run stays parked");
                return unknown(self, sent);
            }
        };
        match read.consumed.first() {
            Some(first) if Some(&first.event) != sent.as_ref() => {
                return self.spent(pending, first);
            }
            None if !read.complete => return unknown(self, sent),
            _ => {}
        }
        type Checked = (
            chat::ToolCall,
            Option<Released>,
            Option<Box<ApprovedRun>>,
            Option<keeper_core::agents::run::RunAllowance>,
        );
        let checked: Result<Checked, (String, ApprovalState)> = match self.read_record(deps, id) {
            Err(reason) => Err((reason, ApprovalState::Refused)),
            Ok((record, args)) => self
                .preconditions(deps, pending, &record, &args, decision)
                .map(|()| {
                    let run = approved_run(&record);
                    let allowance = run_allowance(&record, decision);
                    let (call, released) = bound_call(&record, args);
                    (call, released, run, allowance)
                }),
        };
        let call = match checked {
            Ok(call) => call,
            Err((reason, state)) => {
                let mut body = line(id, state);
                if state == ApprovalState::Refused {
                    body.reason = Some(reason.clone());
                }
                if !self.end(pending, body) {
                    return None;
                }
                self.settling.remove(id);
                let said = if state == ApprovalState::Expired {
                    reason
                } else {
                    not_done(&reason)
                };
                return Some(Settled::Refuse(said));
            }
        };
        // Holding the claim is checked, not bound (R25).
        if !self.holds_claim().await {
            tracing::info!(
                approval = id,
                "agents: this host does not hold the claim; it does not consume"
            );
            return unknown(self, sent);
        }
        let content = ConsumedContent {
            v: CONTENT_VERSION,
            id: id.to_owned(),
            epoch: self.context.epoch,
            host: deps.host.as_str().to_owned(),
        };
        match consume_once(
            room.as_ref(),
            content,
            pending.request_event.as_deref(),
            sent,
        )
        .await
        {
            Consumption::Won(ours) => {
                let mut body = line(id, ApprovalState::Consumed);
                body.by = Some(deps.host.as_str().to_owned());
                let mirrored = self
                    .writer
                    .write(
                        &mut self.context,
                        Some(pending.call_line),
                        Some(ours.clone()),
                        LineBody::Approval(body),
                    )
                    .and_then(|_| crate::agent::off_the_runtime(|| self.writer.sync()));
                match mirrored {
                    Ok(()) => {
                        self.settling.remove(id);
                        // A `session` approval of a T2 run lets its kin go
                        // on this host until the session closes (R146).
                        self.context.run_allowances.extend(call.3);
                        Some(Settled::Run(call.0, call.1, call.2))
                    }
                    Err(_) => unknown(self, Some(ours)),
                }
            }
            Consumption::Lost(first) => {
                // Another copy's came first: spent, and not here.
                let mut body = line(id, ApprovalState::Refused);
                body.reason = Some(format!("spent: consumed first by {}", first.content.host));
                let ended = self.end(pending, body);
                if ended {
                    self.settling.remove(id);
                }
                ended.then(|| Settled::Refuse(effect_unknown(&first.content.host)))
            }
            Consumption::Unknown(sent) => unknown(self, sent),
        }
    }

    /// Whether everything `record` relied on still holds (93.2 AC4,
    /// R174): not expired, the record's call the one parked here, the
    /// digest the decision named, the log through the checkpoint unchanged,
    /// not stale, every pinned file where it landed and with the bytes it
    /// had, and the label still letting a drive write reach its sink.
    fn preconditions(
        &self,
        deps: &AgentDeps,
        pending: &Pending,
        record: &ApprovalRecord,
        args: &Value,
        decision: &DecisionRecord,
    ) -> Result<(), (String, ApprovalState)> {
        let drift = |reason: String| Err((reason, ApprovalState::Refused));
        let now = Utc::now();
        if now >= record.expires() {
            return Err((APPROVED_UNUSED.to_owned(), ApprovalState::Expired));
        }
        if record.call.call_id != pending.call_id
            || record.call.line != pending.call_line.to_string()
        {
            return drift("the approval names another call than the one that waits".to_owned());
        }
        if record.recomputed_digest(args).ok().as_deref() != Some(decision.binding_digest.as_str())
        {
            return drift(CHANGED.to_owned());
        }
        let session_dir = deps.sessions_zone.join(&self.context.session.path);
        // The consolidator's action parked no model call: no log to check.
        if !keeper_core::agents::consolidate::is_host_action(record)
            && !checkpoint_holds(&session_dir, &record.checkpoint)
        {
            return drift("the session's log changed after it was approved".to_owned());
        }
        if record.stale(now) {
            return drift(format!(
                "more than {} s passed since what it relied on was read",
                record.preconditions.max_staleness_s.unwrap_or(0)
            ));
        }
        let profiles = deps
            .env
            .drive
            .as_ref()
            .map(|drive| drive.profiles.profiles())
            .unwrap_or_default();
        if let Some(moved) = moved_file(record, &profiles) {
            return drift(format!("{moved} changed since it was approved"));
        }
        if let Some(reason) = self.sink_now_blocks(deps, record, args) {
            return drift(reason);
        }
        if let Some(reason) = self.run_moved(deps, record, args) {
            return drift(reason);
        }
        Ok(())
    }

    /// What a `run` record relied on beyond its files, read again before
    /// its approval is consumed (R144, R213): the binding recomputed whole
    /// here — the host that resolves it (digested, so a record moved to or
    /// edited for another host drifts), the folder `cwd` resolves to, the
    /// programs' paths and SHA-256 and every operand's — over the drives
    /// the agent's grants let it read now, and, with network, the workspace
    /// set the card showed (S-03).
    fn run_moved(&self, deps: &AgentDeps, record: &ApprovalRecord, args: &Value) -> Option<String> {
        if record.action.tool != AgentTool::Run.as_wire() {
            return None;
        }
        let Some(sandbox) = &deps.sandbox else {
            return Some("this host no longer offers a sandbox".to_owned());
        };
        let drives = crate::agent::run_drives(&self.context, deps);
        let session = crate::run::Session {
            drive: &deps.drive_root,
            zone: &deps.sessions_subfolder,
            path: &self.context.session.path,
        };
        let now = match crate::agent::off_the_runtime(|| sandbox.prepare(args, session, &drives)) {
            Ok(now) => now,
            Err(reason) => return Some(reason),
        };
        if now.exec_binding != record.action.exec_binding {
            tracing::warn!(approval = %record.id, "agents: a run's host, folder, program or the code it runs changed after its approval");
            return Some(
                "where it runs, its program, or the code it runs changed after it was approved"
                    .to_owned(),
            );
        }
        if now.workspace_set != record.preconditions.workspace {
            tracing::warn!(approval = %record.id, "agents: a networked run's workspace changed after its approval");
            return Some("its workspace changed after it was approved".to_owned());
        }
        None
    }

    /// Whether this host holds the session's claim now: its lease lets it
    /// write and the room, read now, says so (R25, R176). Nothing of the
    /// session is held across the read.
    fn holds_claim(&self) -> impl Future<Output = bool> + Send + 'static {
        let room = self.approval_room.clone();
        let may_write = self.writer.may_write();
        let epoch = self.context.epoch;
        async move {
            match room {
                Some(room) if may_write => matches!(room.holds(epoch).await, Ok(true)),
                _ => false,
            }
        }
    }

    /// Close the one audit row of the call parked on `id` with `outcome`
    /// (R172, R176): the row its park left pending on this machine; none
    /// when this machine already has a row of it; else — a host that took
    /// over — a row of its own, carrying the approval, from the record.
    fn close_row(&self, deps: &AgentDeps, id: &str, outcome: AuditOutcome) {
        let data_dir = &deps.data_dir;
        let closed = match audit::parked_row(data_dir, id) {
            Ok(Some(row)) => {
                audit::complete(data_dir, row, outcome, None, false, now_ms()).map(drop)
            }
            Ok(None) => match audit::approval_rowed(data_dir, id) {
                Ok(true) => Ok(()),
                Ok(false) => self.taker_row(deps, id, outcome),
                Err(error) => Err(error),
            },
            Err(error) => Err(error),
        };
        if let Err(error) = closed {
            tracing::warn!(%error, approval = id, "agents: a parked call's audit row could not be closed");
        }
    }

    /// The one row of a call parked on `id` that this machine never wrote,
    /// closed `outcome` at once.
    fn taker_row(
        &self,
        deps: &AgentDeps,
        id: &str,
        outcome: AuditOutcome,
    ) -> Result<(), keeper_core::error::CoreError> {
        let unreadable = |reason: String| keeper_core::error::CoreError::Internal(reason);
        let (record, _) = self.read_record(deps, id).map_err(unreadable)?;
        let classification = record.classification().ok_or_else(|| {
            unreadable("the approval's classification is not one this keeper knows".to_owned())
        })?;
        let (drive, at) = record.preconditions.files.first().map_or_else(
            || (record.drive.clone(), record.session.clone()),
            |pin| (pin.drive.clone(), pin.path.clone()),
        );
        let sinks = self.sinks(deps);
        let row = audit::append_intent(
            &deps.data_dir,
            &AuditIntent {
                started_ms: now_ms(),
                provider_id: &sinks.provider_id,
                bot_id: Some(&sinks.bot_id),
                session_id: &sinks.session_id,
                message_id: None,
                tool: &record.action.tool,
                target: &ToolTarget {
                    profile_id: drive,
                    subpath: at,
                },
                effect: Effect::Write,
                verdict: &GrantVerdict::Allow {
                    grant_id: format!("agent:{}", record.action.tool),
                },
                classified: Some(&classification),
            },
        )?;
        audit::mark_approval(&deps.data_dir, row, id)?;
        audit::complete(&deps.data_dir, row, outcome, None, false, now_ms()).map(drop)
    }

    /// The sentence refusing a drive write whose sink the label now blocks.
    fn sink_now_blocks(
        &self,
        deps: &AgentDeps,
        record: &ApprovalRecord,
        args: &Value,
    ) -> Option<String> {
        let tool = AgentTool::from_wire(&record.action.tool)?;
        if !matches!(tool, AgentTool::DriveWrite | AgentTool::DriveEdit) {
            return None;
        }
        let drive = args["profile"].as_str()?;
        let drive_readers = deps
            .drives
            .get(drive)
            .map_or(Readers::Anyone, |decl| Readers::Only(decl.readers.clone()));
        match check_sink(&self.context.label, &Sink::DriveWrite { drive_readers }) {
            SinkVerdict::Allow => None,
            SinkVerdict::Block { reason, .. } => Some(reason),
        }
    }

    /// The person's own new message in their `main` or `conversation`
    /// session denies what waits (R74, a fail-safe decision that needs no
    /// device): logged, the parked call and the round's rest answered
    /// refused, so the person's turn reads a whole transcript.
    pub(crate) fn supersede(&mut self, deps: &AgentDeps, by: &OwnedUserId) {
        let pending: Vec<Pending> = self
            .context
            .parked
            .values()
            .filter(|pending| pending.ended.is_none())
            .cloned()
            .collect();
        for pending in pending {
            let mut body = line(&pending.id, ApprovalState::Decided);
            body.decision = Some(Decision::Deny.as_word().to_owned());
            body.by = Some(by.to_string());
            body.reason = Some(SUPERSEDED.to_owned());
            if self.end(&pending, body) {
                self.answer_unrun(deps, &pending, SUPERSEDED_RESULT);
            }
        }
    }

    /// A call that could not be parked (its record not written, or nobody
    /// who could approve it askable from here): refused as when nobody can
    /// be asked — saying so, when that is why — the round's rest not run,
    /// and the turn of `user_line` closed with why.
    pub(crate) fn refuse_parked(
        &mut self,
        deps: &AgentDeps,
        parked: &ParkedTurn,
        user_line: Ulid,
        error: &str,
    ) -> Result<(), ServeError> {
        let pending = Pending {
            id: parked.parking.approval.to_string(),
            call_id: parked.parking.call_id.clone(),
            call_line: parked.call_line,
            request_event: None,
            ended: None,
            round: self
                .context
                .calls_from(parked.call_line)
                .into_iter()
                .map(|(_, wire)| wire.id)
                .collect(),
            announced: false,
            ran: false,
        };
        let said = if error == NOBODY_TO_ASK {
            NOBODY_TO_ASK
        } else {
            crate::host::UNATTENDED_REFUSAL
        };
        self.answer_unrun(deps, &pending, said);
        self.writer.write(
            &mut self.context,
            Some(user_line),
            None,
            LineBody::Error(keeper_core::agents::log::ErrorBody {
                sentence: format!("This could not wait for a person: {error}"),
                code: "approval".to_owned(),
            }),
        )?;
        crate::agent::off_the_runtime(|| self.writer.sync())?;
        Ok(())
    }

    /// Answer `pending`'s call with `reason` — its one audit row closed
    /// refused first — and every call of its round still open with
    /// [`NOT_RUN`], without a turn.
    fn answer_unrun(&mut self, deps: &AgentDeps, pending: &Pending, reason: &str) {
        if pending.call_open() {
            self.close_row(deps, &pending.id, AuditOutcome::Refused);
        }
        for (line_id, wire) in self.context.calls_of(&pending.round) {
            let said = if wire.id == pending.call_id {
                reason
            } else {
                NOT_RUN
            };
            let body = LineBody::ToolResult(keeper_core::agents::log::ToolResultBody {
                call_id: wire.id.clone(),
                outcome: keeper_core::agents::log::ToolOutcomeWord::Refused,
                content: render_result(&ToolOutcome::Refused {
                    reason: said.to_owned(),
                }),
                truncated: None,
                label: self.context.label.clone(),
            });
            if let Err(error) = self
                .writer
                .write(&mut self.context, Some(line_id), None, body)
            {
                tracing::error!(%error, approval = %pending.id, "agents: an unrun call's result could not be written");
                return;
            }
        }
        if let Err(error) = crate::agent::off_the_runtime(|| self.writer.sync()) {
            tracing::error!(%error, "agents: the session's log could not be synced");
        }
    }

    /// The round's later calls `open` as the model sent them, from the
    /// round file; the ids of those it does not hold, which are refused
    /// rather than run from the log's redacted copy (R174).
    fn bound_rest(
        &self,
        deps: &AgentDeps,
        id: &str,
        open: Vec<(Ulid, chat::ToolCall)>,
    ) -> (Vec<(Ulid, chat::ToolCall)>, Vec<String>) {
        let round = self.read_round(deps, id);
        let mut unbound = Vec::new();
        let rest = open
            .into_iter()
            .map(|(line, wire)| {
                let sent = round.as_ref().and_then(|round| {
                    round.calls.iter().find(|call| {
                        call.call_id == wire.id
                            && call.tool == wire.name
                            && call.line == line.to_string()
                    })
                });
                match sent {
                    Some(sent) => (
                        line,
                        chat::ToolCall {
                            arguments: serde_json::from_str(&sent.args).ok(),
                            arguments_raw: sent.args.clone(),
                            ..wire
                        },
                    ),
                    None => {
                        unbound.push(wire.id.clone());
                        (line, wire)
                    }
                }
            })
            .collect();
        (rest, unbound)
    }

    /// Go on with the turn `pending` parked: its call answered as
    /// `settled` says, while it has no result, then every call of its round
    /// still open — run when the call ran here, refused otherwise — and
    /// the model. A continuation a stop cut (`settled` is then `None`)
    /// tells the first open call it may have run and never runs it again,
    /// so every `tool_call` of the round gets its result (R176).
    async fn continue_parked(
        &mut self,
        deps: &AgentDeps,
        port: &Arc<dyn EditPort>,
        pending: &Pending,
        settled: Option<Settled>,
        note: Option<PeerBody>,
        stop: CancelSignal,
    ) {
        let Ok(approval) = Ulid::from_string(&pending.id) else {
            return;
        };
        let mut open = self.context.calls_of(&pending.round);
        let call = match open.first() {
            Some((_, wire)) if pending.call_open() && wire.id == pending.call_id => {
                Some(open.remove(0))
            }
            _ => None,
        };
        let (settled, run_rest, uncertain) = match (&call, settled) {
            (Some(_), Some(settled)) => {
                let ran = matches!(settled, Settled::Run(..));
                (settled, ran, None)
            }
            (Some(_), None) => return,
            (None, _) => (
                Settled::Refuse(String::new()),
                pending.ran,
                pending.ran.then(|| INTERRUPTED.to_owned()),
            ),
        };
        if open.is_empty() && call.is_none() {
            return;
        }
        if let (Some(_), Settled::Refuse(_)) = (&call, &settled) {
            // Refused here, it never runs: its one row closes now (R172);
            // consumed here and cut before its result, its effect is
            // unknown.
            let outcome = if pending.ran {
                AuditOutcome::Failed
            } else {
                AuditOutcome::Refused
            };
            self.close_row(deps, &pending.id, outcome);
        }
        let (rest, unbound) = if run_rest {
            self.bound_rest(deps, &pending.id, open)
        } else {
            (open, Vec::new())
        };
        let ran = call.is_some() && run_rest;
        let resume = Resume {
            approval,
            settled,
            call,
            rest,
            run_rest,
            uncertain,
            unbound,
            note,
            question: pending
                .request_event
                .clone()
                .unwrap_or_else(|| question_of(&pending.id)),
        };
        // A scheduled run's continuation ends that run, on its card and in
        // the log, as the run itself would have (R178).
        self.scheduled_card = self
            .read_round(deps, &pending.id)
            .and_then(|round| round.scheduled);
        let went_on = self.resume_turn(deps, Arc::clone(port), resume, stop).await;
        match &went_on {
            Ok(report) => {
                tracing::info!(approval = %pending.id, ending = ?report.ending, "agents: a parked run went on")
            }
            Err(error) => {
                tracing::error!(approval = %pending.id, %error, "agents: a parked run could not go on")
            }
        }
        if let Some(card) = self.scheduled_card.take() {
            let ending = went_on.as_ref().ok().map(|report| report.ending);
            if let Err(error) = self.finish_scheduled(deps, &card, ending) {
                tracing::warn!(approval = %pending.id, %error, "agents: a parked scheduled run's end could not be logged");
            }
        }
        if ran {
            // A call refused before it reached its row leaves that row
            // here; it did not run (R172).
            self.close_row(deps, &pending.id, AuditOutcome::Refused);
        }
    }
}

/// What a consumed `run` record was checked against; `None` for every
/// other tool.
fn approved_run(record: &ApprovalRecord) -> Option<Box<ApprovedRun>> {
    (record.action.tool == AgentTool::Run.as_wire()).then(|| {
        Box::new(ApprovedRun {
            exec_binding: record.action.exec_binding.clone(),
            workspace: record.preconditions.workspace.clone(),
        })
    })
}

/// The allowance a `session` decision on a T2 `run` record gives (R146):
/// what its binding covers, until 24 hours after the decision; `None` for
/// any other decision, tool or tier.
fn run_allowance(
    record: &ApprovalRecord,
    decision: &DecisionRecord,
) -> Option<keeper_core::agents::run::RunAllowance> {
    let decided = chrono::DateTime::parse_from_rfc3339(&decision.decided_at).ok()?;
    (record.action.tool == AgentTool::Run.as_wire()
        && decision.scope == approval::Scope::Session
        && record.risk.tier == 2)
        .then(|| keeper_core::agents::run::RunAllowance {
            approval: record.id.clone(),
            key: keeper_core::agents::run::allowance_key(&record.action.exec_binding),
            ends: approval::session_scope_ends(decided.with_timezone(&Utc), None),
        })
}

/// The call `record` binds, as it runs after approval: its call id, its
/// tool and its arguments — never the log's redacted copy (R174). A
/// `declassify` record binds the blocked call as the model sent it, byte
/// for byte, and releases exactly the bytes it names (R89).
fn bound_call(record: &ApprovalRecord, args: Value) -> (chat::ToolCall, Option<Released>) {
    if record.action.tool == AgentTool::Declassify.as_wire() {
        let raw = args["call"]["arguments"].as_str().unwrap_or("").to_owned();
        let call = chat::ToolCall {
            id: record.call.call_id.clone(),
            name: args["call"]["tool"].as_str().unwrap_or("").to_owned(),
            arguments: serde_json::from_str(&raw).ok(),
            arguments_raw: raw,
        };
        let released = Released {
            sha256: args["sha256"].as_str().unwrap_or("").to_owned(),
            delegation: args["delegation"].as_str().map(str::to_owned),
            readers: args["readers"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|user| OwnedUserId::try_from(user.as_str()?).ok())
                .collect(),
        };
        return (call, Some(released));
    }
    let call = chat::ToolCall {
        id: record.call.call_id.clone(),
        name: record.action.tool.clone(),
        arguments_raw: args.to_string(),
        arguments: Some(args),
    };
    (call, None)
}

/// What the model is told of an approval consumed on `host` whose call has
/// no result: whether it took effect is unknown, and it is never run again.
pub fn effect_unknown(host: &str) -> String {
    format!("This was approved and used on {host}, but keeper does not know whether it took effect, and it will not run it again. Check, and propose it again if it is still needed.")
}

/// A decision's note, as the `peer` line the model reads after the result.
fn note_of(decision: &DecisionRecord) -> Option<PeerBody> {
    let note = decision.note.as_ref()?.trim();
    if note.is_empty() {
        return None;
    }
    Some(PeerBody {
        sender: OwnedUserId::try_from(decision.decided_by.user.as_str()).ok()?,
        text: note.to_owned(),
        ask: None,
        answers: None,
        artifacts: None,
    })
}

/// The first pinned file of `record` that does not land where it landed,
/// or whose bytes are not what they were (R174).
fn moved_file(record: &ApprovalRecord, profiles: &[SyncProfile]) -> Option<String> {
    let pins: Vec<(String, String)> = record
        .preconditions
        .files
        .iter()
        .map(|pin| (pin.drive.clone(), pin.path.clone()))
        .collect();
    record
        .preconditions
        .files
        .iter()
        .zip(pin_files(profiles, &pins))
        .find(|(then, now)| then.landing != now.landing || then.sha256 != now.sha256)
        .map(|(then, _)| format!("{}/{}", then.drive, then.path))
}

/// The content of a person's decision on `record`, as a device sends it.
pub fn decision_content(record: &ApprovalRecord, decision: Decision, note: Option<&str>) -> Value {
    serde_json::to_value(ApprovalDecisionContent {
        id: record.id.clone(),
        binding_digest: record.binding_digest.clone(),
        decision,
        scope: approval::Scope::Once,
        note: note.map(str::to_owned),
    })
    .unwrap_or(Value::Null)
}
