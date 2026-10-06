//! The `delegate` and `reply` tools: an agent hands work to another agent
//! in a session the target owns (AD-385, story 92.1).
//!
//! Like the surface tools they are an agent's alone, served from the
//! agent's own host through [`ToolHost::run_named`]; a ⌘9 bot never has them
//! (R38). `delegate` is offered when `[tools].allow` names it, `reply` in
//! every delegated session (R48) and, while a question another agent asked
//! its person waits for their answer, in a proxy's `main` or
//! `conversation` as the relay of that answer (R100, [`crate::ask`]).
//!
//! # Handing on
//!
//! A `delegate` call checks, before anything is sent: the bounds
//! ([`Limits::check`]), the drives against the target's `[tools].drives`,
//! the label against the target's audience and the room's members
//! (`check_sink(Delegation)`, `check_sink(Room)`), then the integrity rule
//! and its tier (AD-392, R82): a card with a schedule or a workflow is T3,
//! which needs a person and so is refused until a decision source is
//! installed. A block or a refusal is the call's one audit row (R90).
//! Then it makes the room — the target and the label's readers invited — and
//! returns at once with a `delegate opened` line. The brief is not sent yet:
//! an invited device may be outside the room's key, so the host sends it once
//! the target has joined ([`content_for`], R29 F5) and writes `delegate sent`.
//! With `session` naming an open delegation, the call sends the next round of
//! its exchange instead (R49), at most `rounds_per_exchange` before a reply.
//!
//! # Replying
//!
//! `reply` sends the delegated session's answer into its room, carrying
//! `dev.keeper.agent.artifacts` and the session's label as it is then
//! (`dev.keeper.agent.label`, R94), and sets its card's `run: review`. With
//! `ask`, in a proxy's own session, it relays its person's answer into the
//! room the question came from instead ([`crate::ask::relay`]).
//!
//! Every send — the brief once the target joined, each later round, and the
//! reply — is checked against the label at that moment and the room's people
//! as they are then; a block sends nothing and writes `delegate refused`.
//!
//! [`ToolHost::run_named`]: keeper_core::bots::tools::ToolHost::run_named

use std::collections::BTreeSet;
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex};

use keeper_core::agents::approval::sha256_hex;
use keeper_core::agents::card::Run;
use keeper_core::agents::delegation::{
    self, brief_content, child_label, observers, room_invites, session_title, BoundReached,
    DelegateCard, DelegateContent, DelegateFrom, DelegateLimits, Limits, CARD_FILE,
};
use keeper_core::agents::events::{ARTIFACTS, CONTENT_VERSION, REPLY_LABEL};
use keeper_core::agents::home;
use keeper_core::agents::label::{
    approved_label, check_sink, Destination, Label, Readers, Sink, SinkVerdict,
};
use keeper_core::agents::log::{DelegateBody, DelegateState, LineBody, RunBody, RunState};
use keeper_core::agents::matrix::AgentMatrixError;
use keeper_core::agents::proxy::ScopeRequest;
use keeper_core::agents::session::SessionKind;
use keeper_core::bots::chat::{ToolCall as WireToolCall, ToolSpec};
use keeper_core::bots::tools::ToolOutcome;
use keeper_core::sessions::model::ARTIFACTS_DIR;
use keeper_sync::browse;
use matrix_sdk::ruma::{
    OwnedRoomId, OwnedTransactionId, OwnedUserId, RoomId, TransactionId, UserId,
};
use serde::Deserialize;
use serde_json::{json, Value};
use ulid::Ulid;

use crate::agent::RoomFuture;
use crate::matrix_sink::{EditPort, SendFuture};
use crate::rooms::{BriefRoom, Known, KnownAgent};
use crate::sessions::verbs::VerbError;
use crate::sinks::{room_audience, Blocked, CallAudit, Sinks, Withheld};

/// The tool that hands work on.
pub const DELEGATE: &str = "delegate";
/// The tool a delegated session answers with.
pub const REPLY: &str = "reply";

/// What `reply` says outside a delegated session.
pub const NOT_DELEGATED: &str =
    "reply answers a delegation, and this session was not delegated to you.";

/// Which `reply` a session is offered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReplyOffer {
    /// None: neither delegated nor relaying.
    None,
    /// A delegated session's answer to its requester (R48).
    Delegated,
    /// A proxy's relay of its person's answer to an open ask (R100).
    Relay,
}

/// What a call says on a host with no room to make one in.
pub const NO_ROOMS: &str = "This host cannot open a room for a delegation.";

/// Whether `name` is `delegate` or `reply`.
pub fn is_delegation(name: &str) -> bool {
    name == DELEGATE || name == REPLY
}

/// The specs a turn is offered: `delegate` when `[tools].allow` names it,
/// `reply` as `reply` says (R48, R100).
pub fn specs(delegate: bool, reply: ReplyOffer) -> Vec<ToolSpec> {
    let mut specs = Vec::new();
    if delegate {
        specs.push(ToolSpec {
            name: DELEGATE.to_owned(),
            description: "Hand work to another agent: it works in a session of its own and replies here. Name it as <drive>/<id> (an id alone when only one agent has it). The brief is all it is told. With session set to an open delegation's id, send it the next message of that exchange instead.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "agent": {"type": "string", "description": "The agent: <drive>/<id>."},
                    "brief": {"type": "string", "description": "What to do, whole: the agent reads nothing else of this session."},
                    "drives": {"type": "array", "items": {"type": "string"}, "description": "Drives it works in; its home drive always."},
                    "card": {
                        "type": "object",
                        "description": "The card the session opens with.",
                        "properties": {
                            "title": {"type": "string"},
                            "schedule": {"type": "string"},
                            "workflow": {"type": "string"},
                            "integrity": {"type": "string", "enum": ["untrusted"], "description": "untrusted when the card was made from outside content."}
                        },
                        "required": ["title"],
                        "additionalProperties": false
                    },
                    "session": {"type": "string", "description": "An open delegation's id, to say more in its exchange."},
                    "source": {"type": "string", "description": "The card whose work this hands on: one of this session's, e.g. answer-x.md, whose run shows the delegation's progress, or another session's of this drive as <session id>:<card>. Handing a card on again names the delegation it went to."}
                },
                "required": ["agent", "brief"],
                "additionalProperties": false
            }),
        });
    }
    match reply {
        ReplyOffer::None => {}
        ReplyOffer::Delegated => specs.push(ToolSpec {
            name: REPLY.to_owned(),
            description: "Reply to the agent that delegated this session: your answer, and the files under artifacts/ it should read. This closes the exchange.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string"},
                    "artifacts": {"type": "array", "items": {"type": "string"}, "description": "Session-relative paths under artifacts/."}
                },
                "required": ["text"],
                "additionalProperties": false
            }),
        }),
        ReplyOffer::Relay => specs.push(ToolSpec {
            name: REPLY.to_owned(),
            description: "Relay your person's answer to a question another agent asked them through you, once they have answered it here. What goes to the asking agent is your person's own message, as they wrote it — never your words: name the question's id as ask, and, when they said more than one thing since the question, quote the message that answers it as text.".to_owned(),
            parameters: json!({
                "type": "object",
                "properties": {
                    "text": {"type": "string", "description": "The message of your person's that answers, exactly as they wrote it; without it, their first message since the question."},
                    "ask": {"type": "string", "description": "The question's id, as its message names it."}
                },
                "required": ["ask"],
                "additionalProperties": false
            }),
        }),
    }
    specs
}

/// A boxed future answering yes or no.
pub type BoolFuture<'a> = Pin<Box<dyn Future<Output = bool> + Send + 'a>>;
/// A boxed future of decrypted timeline events, oldest first, or why they
/// could not all be read.
pub type EventsFuture<'a> = Pin<Box<dyn Future<Output = Result<Vec<Value>, String>> + Send + 'a>>;
/// A boxed future of a room change, or the homeserver's answer refusing it.
pub type UnitFuture<'a> = Pin<Box<dyn Future<Output = Result<(), AgentMatrixError>> + Send + 'a>>;
/// A boxed future of a room's members, joined or invited.
pub type MembersFuture<'a> =
    Pin<Box<dyn Future<Output = Result<BTreeSet<OwnedUserId>, String>> + Send + 'a>>;
/// A boxed future of what a host reads of a room a brief arrived in.
pub type BriefRoomFuture<'a> = Pin<Box<dyn Future<Output = Option<BriefRoom>> + Send + 'a>>;

/// The rooms a delegation goes through.
pub trait DelegationPort: Send + Sync {
    /// Every agent of a drive this host mounts.
    fn known(&self) -> Arc<Known>;
    /// A session room of `kind` — a delegation's, or a workflow's run
    /// (R104) — named `name`, made by the agent, `invite` invited and
    /// `agents` at power 50.
    fn create<'a>(
        &'a self,
        kind: SessionKind,
        name: &'a str,
        invite: Vec<OwnedUserId>,
        agents: Vec<OwnedUserId>,
    ) -> RoomFuture<'a>;
    /// Send one `m.room.message` into `room`, once.
    fn send<'a>(
        &'a self,
        room: &'a RoomId,
        content: Value,
        txn: OwnedTransactionId,
    ) -> SendFuture<'a>;
    /// Whether `user` has joined `room`, as this copy last synced it.
    fn joined<'a>(&'a self, room: &'a RoomId, user: &'a UserId) -> BoolFuture<'a>;
    /// Who is in `room` or invited to it now.
    fn members<'a>(&'a self, room: &'a RoomId) -> MembersFuture<'a>;
    /// `room`'s events after the newest brief `me` sent into it, oldest
    /// first: paged back as far as that brief, so a reply behind any number
    /// of later events is found (R55).
    fn since_brief<'a>(&'a self, room: &'a RoomId, me: &'a UserId) -> EventsFuture<'a>;
    /// What `room` is now, as a brief's admission reads it (R93).
    fn brief_room<'a>(&'a self, room: &'a RoomId) -> BriefRoomFuture<'a>;
    /// Route `child`'s joins and replies to `parent`'s worker from now on;
    /// `kind` is the parent session's: an ask in `child` goes to it only
    /// when it is the proxy's own conversation (R199).
    fn watch(&self, child: &RoomId, parent: &RoomId, kind: SessionKind);
    /// Invite `user` into `room` when what the room holds, labelled
    /// `label`, may reach them — through `audience` when they are an agent
    /// this host knows or a pinned proxy (AD-391): the proxy an ask goes
    /// through (R101, R199).
    fn invite<'a>(
        &'a self,
        room: &'a RoomId,
        user: &'a UserId,
        label: &'a Label,
        audience: Option<Readers>,
    ) -> UnitFuture<'a>;
    /// A proxy relayed an answer into `room` (R100, S-27): its host reads
    /// the room back and leaves it once no ask there waits for a relay of
    /// this proxy's and none of its sessions delegated into it — tried
    /// again until it has left.
    fn depart(&self, room: &RoomId);
}

/// One delegation a session made, as its log says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delegation {
    /// Its id: the child session's.
    pub id: String,
    pub to: OwnedUserId,
    pub room: OwnedRoomId,
    /// The `delegate` call's arguments, verbatim.
    pub args: Option<String>,
    /// Whether the brief went in.
    pub sent: bool,
    /// Whether the target replied to the latest round.
    pub replied: bool,
    /// Rounds sent since the last reply.
    pub rounds: u32,
    /// A workflow's run a scheduled card started: that card and its window.
    pub window: Option<keeper_core::agents::log::CardWindow>,
}

/// The session a turn runs in, as its `delegate` and `reply` read it.
#[derive(Debug, Clone)]
pub struct Delegator {
    pub user: OwnedUserId,
    /// The home drive.
    pub drive: String,
    /// The session's id.
    pub id: String,
    /// The session, zone-relative.
    pub session: String,
    pub room: OwnedRoomId,
    pub kind: SessionKind,
    /// Who asked for the session: a delegated one's delegating agent.
    pub requester: OwnedUserId,
    pub hop: u8,
    pub limits: home::Limits,
    /// The home drive's sessions zone, and its folder inside the drive.
    pub zone: PathBuf,
    pub subfolder: String,
    /// The delegating session's dispatch chain (R76).
    pub chain: Vec<OwnedUserId>,
}

/// What a running turn's tools read of its session: the label now, which a
/// read earlier in the turn may have narrowed, and its delegations.
pub trait TurnView: Sync {
    fn label(&self) -> Label;
    fn delegation(&self, id: &str) -> Option<Delegation>;
    /// The delegation the card `source` of this session was handed to.
    fn handed(&self, source: &str) -> Option<Delegation>;
    /// Whether the session's claim lets this host write now (NFR-120):
    /// asked by every file writer right before its effect (R120).
    fn may_write(&self) -> bool;
    /// The question another agent asked this proxy's person, `id`, while it
    /// waits for its answer to be relayed (R100).
    fn relay(&self, id: &str) -> Option<crate::ask::Relay>;
    /// Whether this session is a workflow's run that has ended — replied,
    /// or failed: it takes no more effects (R202).
    fn ended(&self) -> bool;
    /// The tokens the turn under way has spent so far: its rounds' and its
    /// helpers' (R111).
    fn turn_spend(&self) -> u64;
    /// A delegated session's or a workflow's run's own token budget, when
    /// it has one: what the session has spent — the round under way
    /// included — and its limit. A helper stops at it as the run's next
    /// round would (Q12, R111, R214).
    fn session_budget(&self) -> Option<(u64, u64)>;
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct CardArgs {
    title: String,
    #[serde(default)]
    schedule: Option<String>,
    #[serde(default)]
    workflow: Option<String>,
    #[serde(default)]
    integrity: Option<String>,
}

/// A `delegate` call's arguments.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct DelegateArgs {
    agent: String,
    brief: String,
    #[serde(default)]
    drives: Vec<String>,
    #[serde(default)]
    card: Option<CardArgs>,
    #[serde(default)]
    session: Option<String>,
    #[serde(default)]
    source: Option<String>,
}

fn parse_args(raw: &str) -> Result<DelegateArgs, String> {
    serde_json::from_str(raw).map_err(|error| format!("delegate's arguments do not read: {error}"))
}

/// The card a delegation's `delegate` call `args` handed on: its path in
/// the delegating session, as the call named it, or `<session>:<card>`
/// for a card of another session ([`handed_from`]).
pub fn source_of(args: Option<&str>) -> Option<String> {
    parse_args(args?)
        .ok()?
        .source
        .map(|source| source.trim().to_owned())
        .filter(|source| !source.is_empty())
}

/// A `source` naming a card of another session of this drive — the session
/// a dispatch run was given — as `<session id>:<card>`: that session's id
/// and the card (R202). `None` for a card of the calling session.
pub fn handed_from(source: &str) -> Option<(&str, &str)> {
    let (session, card) = source.split_once(':')?;
    session.parse::<Ulid>().ok()?;
    Some((session, card))
}

/// What a card handed on already answers: its delegation, and how to say
/// more in that exchange.
fn handed_already(source: &str, id: &str) -> Option<ToolOutcome> {
    Some(ToolOutcome::Answered {
        text: format!(
            "{source} was handed on already, as delegation {id}. To say more in that exchange, call delegate with session = {id}."
        ),
    })
}

/// The delegation the card `card` of the session at `session` (zone
/// relative, its id `session_id`) was handed on as, read from the disk:
/// by that session itself — a `delegate` call naming the card as its
/// `source` — or, as [`keeper_core::agents::workflow::handoff_id`] names
/// it, by any workflow run `me` opened in `zone` (R202). The host's
/// binding: whichever run asks, a card is handed on once.
fn handed_on(
    zone: &Path,
    me: &UserId,
    session: &str,
    session_id: &str,
    card: &str,
) -> Option<String> {
    use keeper_core::agents::log::reader::read_session;
    let handoff = keeper_core::agents::workflow::handoff_id(session_id, card).to_string();
    let mut calls: std::collections::HashMap<Ulid, String> = std::collections::HashMap::new();
    for line in read_session(&zone.join(session)).lines {
        match &line.body {
            LineBody::ToolCall(call) if call.tool == DELEGATE => {
                calls.insert(line.id, call.args.clone());
            }
            LineBody::Delegate(body) if body.state == DelegateState::Opened => {
                let named = line
                    .parent
                    .and_then(|parent| calls.get(&parent))
                    .and_then(|args| source_of(Some(args)));
                if named.as_deref() == Some(card) {
                    return Some(body.id.clone());
                }
            }
            _ => {}
        }
    }
    for rel in crate::sessions::scan::session_dirs(zone) {
        let dir = zone.join(&rel);
        let run = std::fs::read_to_string(dir.join(keeper_core::agents::session::FILE_NAME))
            .ok()
            .and_then(|text| keeper_core::agents::session::parse_session_agent_toml(&text).ok());
        if !run.is_some_and(|run| run.kind == SessionKind::Workflow && run.requested_by == me) {
            continue;
        }
        let opened = read_session(&dir).lines.into_iter().any(|line| {
            matches!(&line.body, LineBody::Delegate(body)
                if body.state == DelegateState::Opened && body.id == handoff)
        });
        if opened {
            return Some(handoff);
        }
    }
    None
}

/// The agent `name` names (R67): `<drive>/<id>`, or a bare id only when
/// exactly one known agent has it.
pub fn resolve<'k>(known: &'k Known, name: &str) -> Result<&'k KnownAgent, String> {
    let name = name.trim();
    let found: Vec<&KnownAgent> = match name.split_once('/') {
        Some((drive, id)) => known
            .agents
            .iter()
            .filter(|agent| agent.drive == drive && agent.id == id)
            .collect(),
        None => known
            .agents
            .iter()
            .filter(|agent| agent.id == name)
            .collect(),
    };
    match found.as_slice() {
        [one] => Ok(one),
        [] => Err(format!("No agent named {name} is known on this host.")),
        many => Err(format!(
            "{name} names more than one agent: {}. Name one as <drive>/<id>.",
            many.iter()
                .map(|agent| agent.name_in_drive())
                .collect::<Vec<_>>()
                .join(", ")
        )),
    }
}

/// The delegation `args` (a `delegate` call's) describes from `from` to
/// `target` under the session's `label`, its id `id`: the drives checked
/// against the target's `[tools].drives`, the label lowered for a card of
/// outside content (Q17).
fn compose(
    args: &DelegateArgs,
    from: &Delegator,
    target: &KnownAgent,
    label: &Label,
    id: &str,
) -> Result<DelegateContent, String> {
    let drives = ScopeRequest(args.drives.clone())
        .check(&target.drives, &target.drive)
        .map_err(|refusal| refusal.to_string())?;
    let untrusted = args
        .card
        .as_ref()
        .and_then(|card| card.integrity.as_deref())
        .is_some_and(|word| word.trim() == "untrusted");
    Ok(DelegateContent {
        v: CONTENT_VERSION,
        id: id.to_owned(),
        from: DelegateFrom {
            agent: from.user.clone(),
            drive: from.drive.clone(),
            session: from.session.clone(),
            room: from.room.clone(),
        },
        to: target.matrix_user.clone(),
        brief: args.brief.clone(),
        drives,
        label: child_label(label, untrusted),
        hop: from.hop.saturating_add(1),
        limits: DelegateLimits {
            rounds_per_exchange: from.limits.rounds_per_exchange,
            tokens: from.limits.tokens_per_delegation,
        },
        card: args.card.as_ref().map(|card| DelegateCard {
            title: card.title.clone(),
            schedule: card.schedule.clone(),
            workflow: card.workflow.clone(),
        }),
        dispatch_chain: delegation::child_chain(&from.chain, &from.user),
    })
}

/// The brief to send for `delegation` now that its target has joined,
/// under the session's label now: from the `delegate` call it was opened
/// by, so a host that restarted between the two sends the same brief.
pub fn content_for(
    delegation: &Delegation,
    from: &Delegator,
    label: &Label,
    known: &Known,
) -> Result<DelegateContent, String> {
    let args = parse_args(delegation.args.as_deref().unwrap_or_default())?;
    let target = known
        .agents
        .iter()
        .find(|agent| agent.matrix_user == delegation.to)
        .ok_or_else(|| format!("{} is no longer known on this host.", delegation.to))?;
    compose(&args, from, target, label, &delegation.id)
}

/// A reply's `m.room.message` content: the text, the files handed over, and
/// the replying session's label now, which the delegating session joins.
pub fn reply_content(text: &str, artifacts: Vec<Value>, label: &Label) -> Value {
    json!({
        "msgtype": "m.text",
        "body": text,
        ARTIFACTS: artifacts,
        REPLY_LABEL: label,
    })
}

/// The label a reply's content carries; `None` when it carries none this
/// build reads, and such a reply is not taken.
pub fn reply_label(content: &Value) -> Option<Label> {
    serde_json::from_value(content.get(REPLY_LABEL)?.clone()).ok()
}

/// The audience of the known agent `user`.
pub fn audience_of(known: &Known, user: &UserId) -> Option<Readers> {
    known
        .agents
        .iter()
        .find(|agent| agent.matrix_user.as_str() == user.as_str())
        .map(|agent| agent.home_readers.clone())
}

/// A delegation's room as a sink (R94, R160): its members — joined or
/// invited, any power — but `agents`, the two agents of the delegation;
/// each other agent `known` names through its own audience, everyone else
/// as a person; and `audiences`, the delegation's agents' own, beyond them.
pub fn room_of(
    members: BTreeSet<OwnedUserId>,
    known: &Known,
    agents: [&UserId; 2],
    audiences: Vec<Readers>,
) -> Sink {
    let own: Vec<OwnedUserId> = agents.iter().map(|agent| (*agent).to_owned()).collect();
    match room_audience(members, Some(known), &own) {
        Sink::Room {
            humans,
            mut agent_audiences,
        } => {
            agent_audiences.extend(audiences);
            Sink::Room {
                humans,
                agent_audiences,
            }
        }
        other => other,
    }
}

/// Whether what `label` covers may go into a room whose members are
/// `members` ([`room_of`]). `Err` is the sentence a refusal says.
pub fn check_room(
    label: &Label,
    members: BTreeSet<OwnedUserId>,
    known: &Known,
    agents: [&UserId; 2],
    audiences: Vec<Readers>,
) -> Result<(), String> {
    match check_sink(label, &room_of(members, known, agents, audiences)) {
        SinkVerdict::Allow => Ok(()),
        SinkVerdict::Block { reason, .. } => Err(reason),
    }
}

/// `room` as [`room_of`] reads it now; `Err` when its members cannot be.
pub async fn room_now(
    port: &dyn DelegationPort,
    room: &RoomId,
    agents: [&UserId; 2],
    audiences: Vec<Readers>,
) -> Result<Sink, String> {
    let members = port.members(room).await.map_err(|error| {
        format!("Who is in the delegation's room could not be read, so nothing was sent: {error}")
    })?;
    Ok(room_of(members, &port.known(), agents, audiences))
}

/// Whether a reply `content` labelled `label` may go into the delegated
/// session's `room` now (R94, R168): to its people, everyone but the two
/// `agents` of the delegation — the requester's session joins the label
/// the reply carries. A block, or a room that cannot be read, is audited
/// (R65) with what the reply's declassification would name.
pub async fn admit_reply(
    port: &dyn DelegationPort,
    sinks: &Sinks,
    room: &RoomId,
    agents: [&UserId; 2],
    label: &Label,
    content: &Value,
) -> Result<(), String> {
    reply_verdict(port, sinks, room, agents, label, content)
        .await
        .map_err(|blocked| {
            sinks.refused(REPLY, &blocked.drive, &blocked.at, &blocked.sentence);
            blocked.sentence
        })
}

/// [`admit_reply`] without its row: the model's `reply` audits its block in
/// its one classified row (R90).
async fn reply_verdict(
    port: &dyn DelegationPort,
    sinks: &Sinks,
    room: &RoomId,
    agents: [&UserId; 2],
    label: &Label,
    content: &Value,
) -> Result<(), Blocked> {
    let sink = room_now(port, room, agents, Vec::new())
        .await
        .map_err(|unread| Blocked {
            drive: String::new(),
            at: room.to_string(),
            sentence: unread,
            flow: None,
        })?;
    sinks.verdict(
        REPLY,
        &Destination::Room {
            room: room.to_owned(),
        },
        label,
        &sink,
        content.to_string().as_bytes(),
        None,
    )
}

/// Whether `rel`, session-relative, names a file under the session's
/// `artifacts/` — through keeper-sync's containment (AD-65), so `..`, a
/// missing file and a link that leads out of `artifacts/` are refused alike.
pub fn artifact_in(zone: &Path, session: &str, rel: &str) -> Result<(), String> {
    let refused = || format!("{rel} is not a file under {ARTIFACTS_DIR}/ in this session.");
    let segments = browse::plain_segments(rel).map_err(|refusal| refusal.to_string())?;
    if segments.len() < 2 || *segments[0] != *ARTIFACTS_DIR {
        return Err(refused());
    }
    let root = browse::lexical_join(zone, session).map_err(|refusal| refusal.to_string())?;
    let artifacts = browse::resolve(&root, ARTIFACTS_DIR)
        .map_err(|refusal| refusal.to_string())?
        .ok_or_else(refused)?;
    match browse::resolve(&root, rel) {
        Ok(Some(file)) if file.starts_with(&artifacts) && file.is_file() => Ok(()),
        Ok(_) => Err(refused()),
        Err(refusal) => Err(refusal.to_string()),
    }
}

/// Whether `rel`, session-relative, names a file of the session at
/// `session` — through keeper-sync's containment (AD-65), so a path or a
/// link that leads out of it is refused.
fn card_in(zone: &Path, session: &str, rel: &str) -> Result<(), String> {
    let refused = || format!("{rel} is not a card in this session.");
    let root = browse::lexical_join(zone, session).map_err(|refusal| refusal.to_string())?;
    let here = root.canonicalize().map_err(|_| refused())?;
    match browse::resolve(&root, rel) {
        Ok(Some(file)) if file.starts_with(&here) && file.is_file() => Ok(()),
        Ok(_) => Err(refused()),
        Err(refusal) => Err(refusal.to_string()),
    }
}

/// Set the card of the session at `session` (zone-relative) to `run`,
/// through the host's run writer, on a transition only: whether it wrote.
/// A session without its card writes nothing; neither does a host whose
/// claim `may_write` denies.
pub fn set_card_run(
    zone: &Path,
    session: &str,
    run: Run,
    may_write: &dyn Fn() -> bool,
) -> Result<bool, VerbError> {
    let card = zone.join(session).join(CARD_FILE);
    if !card.is_file() {
        return Ok(false);
    }
    crate::cards::write_run(zone, session, CARD_FILE, run, None, may_write)
}

/// A delegation's canonical bytes for its declassification: the whole
/// brief event — the card and the drives with it, not the brief's text
/// alone — so one brief with two cards is two effects (FR-795).
pub(crate) fn brief_effect(content: &DelegateContent) -> Vec<u8> {
    brief_content(content).to_string().into_bytes()
}

/// The brief a person lets through when they let `readers` read what only
/// `label`'s readers may (R193): composed by `compose` under the label they
/// approve — the session's own never changes — and the SHA-256 of its
/// event, the bytes their approval binds. The one place a declassified
/// brief is composed, whether it opens a delegation, is sent at the
/// target's join, or is a later round of the exchange.
pub(crate) fn approved_brief(
    label: &Label,
    readers: &BTreeSet<OwnedUserId>,
    compose: impl FnOnce(&Label) -> Result<DelegateContent, String>,
) -> Option<(DelegateContent, String)> {
    let content = compose(&approved_label(label, readers)).ok()?;
    let sha = sha256_hex(&brief_effect(&content));
    Some((content, sha))
}

pub(crate) fn block_on<F: Future>(fut: F) -> F::Output {
    tokio::task::block_in_place(|| tokio::runtime::Handle::current().block_on(fut))
}

/// One turn's `delegate` and `reply`.
pub struct DelegateTools<'t> {
    pub from: Delegator,
    pub port: Option<Arc<dyn DelegationPort>>,
    /// This session's own room, where a reply goes.
    pub room: Arc<dyn EditPort>,
    pub view: &'t dyn TurnView,
    pub offer_delegate: bool,
    pub offer_reply: ReplyOffer,
    /// Where a send the label refuses is audited (R65).
    pub sinks: &'t Sinks,
    /// A workflow's run: the outputs it declared, session-relative, each
    /// checked when it replies (R107).
    pub outputs: Vec<String>,
    lines: Mutex<Vec<LineBody>>,
}

fn refused(reason: impl Into<String>) -> Option<ToolOutcome> {
    Some(ToolOutcome::Refused {
        reason: reason.into(),
    })
}

impl<'t> DelegateTools<'t> {
    pub fn new(
        from: Delegator,
        port: Option<Arc<dyn DelegationPort>>,
        room: Arc<dyn EditPort>,
        view: &'t dyn TurnView,
        offer_delegate: bool,
        offer_reply: ReplyOffer,
        sinks: &'t Sinks,
    ) -> DelegateTools<'t> {
        DelegateTools {
            offer_reply,
            from,
            port,
            room,
            view,
            offer_delegate,
            sinks,
            outputs: Vec::new(),
            lines: Mutex::new(Vec::new()),
        }
    }

    /// The outputs a workflow's run declared, checked at its reply.
    pub fn with_outputs(self, outputs: Vec<String>) -> DelegateTools<'t> {
        DelegateTools { outputs, ..self }
    }

    /// The `delegate` and `run` lines written since the last take: the
    /// call's reporter writes them under its `tool_call` line.
    pub fn take_lines(&self) -> Vec<LineBody> {
        std::mem::take(&mut *self.lines.lock().unwrap_or_else(|p| p.into_inner()))
    }

    fn line(&self, body: LineBody) {
        self.lines
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .push(body);
    }

    fn refuse(
        &self,
        id: &str,
        to: &str,
        room: Option<OwnedRoomId>,
        reason: String,
    ) -> Option<ToolOutcome> {
        self.line(LineBody::Delegate(DelegateBody {
            id: id.to_owned(),
            to: to.to_owned(),
            room,
            child: None,
            state: DelegateState::Refused,
            reason: Some(reason.clone()),
            reply: None,
            window: None,
        }));
        refused(reason)
    }

    /// What a call its audit did not admit answers: a refusal is the
    /// delegation's, its line written; a park waits, with no line yet.
    fn withheld(
        &self,
        id: &str,
        to: &str,
        room: Option<OwnedRoomId>,
        withheld: Withheld,
    ) -> Option<ToolOutcome> {
        match withheld {
            Withheld::Refused(reason) => self.refuse(id, to, room, reason),
            Withheld::Parked(approval) => Some(ToolOutcome::Parked { approval }),
        }
    }

    /// Run `wire` when it is `delegate` or `reply`; `None` for any other name.
    /// `audit` is the call's one row (R90): a sink's block is written in it,
    /// and once the sinks pass it is admitted — the integrity rule and the
    /// tier answer only then, before any room is made or sent into (R82).
    pub fn run(&self, wire: &WireToolCall, audit: &CallAudit<'_>) -> Option<ToolOutcome> {
        match wire.name.as_str() {
            DELEGATE if !self.offer_delegate => {
                refused(format!("{DELEGATE} is not one of this agent's tools."))
            }
            DELEGATE => self.delegate(&wire.arguments_raw, audit),
            REPLY => match (self.offer_reply, Self::ask_of(wire)) {
                (ReplyOffer::Relay, Some(id)) => self.relay(wire, &id, audit),
                (ReplyOffer::Relay, None) => {
                    refused("reply relays your person's answer: name the question's id as ask.")
                }
                (ReplyOffer::Delegated, None) => self.reply(wire.arguments.as_ref(), audit),
                (_, Some(_)) => refused(crate::ask::NO_RELAY),
                (ReplyOffer::None, None) => refused(NOT_DELEGATED),
            },
            _ => None,
        }
    }

    /// The question a `reply` call `wire` relays the answer to.
    fn ask_of(wire: &WireToolCall) -> Option<String> {
        wire.arguments.as_ref()?["ask"]
            .as_str()
            .map(|id| id.trim().to_owned())
    }

    /// A proxy's `reply(ask, text?)`: its person's own message relayed into
    /// the room the question came from (R100, R199); `text` only picks which.
    fn relay(&self, wire: &WireToolCall, id: &str, audit: &CallAudit<'_>) -> Option<ToolOutcome> {
        let picked = wire
            .arguments
            .as_ref()
            .and_then(|args| args["text"].as_str());
        let Some(relay) = self.view.relay(id) else {
            return refused(format!(
                "No question with the id {id} waits for your person's answer here."
            ));
        };
        let Some(port) = self.port.clone() else {
            return refused(NO_ROOMS);
        };
        crate::ask::relay(
            port.as_ref(),
            self.sinks,
            &self.from.user,
            &relay,
            picked,
            audit,
            &|line| self.line(line),
        )
    }

    /// Where a call `wire` goes, as its audit row names it before the call
    /// says: a `reply` into this session's own room, a `delegate` to the
    /// agent [`DelegateTools::recipient`] resolves (its home drive, when
    /// known).
    pub fn destination(&self, wire: &WireToolCall) -> (String, String) {
        if wire.name == REPLY {
            let relayed = Self::ask_of(wire).and_then(|id| self.view.relay(&id));
            return match relayed {
                Some(relay) => (String::new(), relay.room.to_string()),
                None => (String::new(), self.from.room.to_string()),
            };
        }
        let Some((name, _)) = self.recipient(wire) else {
            return Default::default();
        };
        let Some(known) = self.port.as_ref().map(|port| port.known()) else {
            return (String::new(), name);
        };
        // A next round names its delegation's agent by user, a new one by
        // the name the call gave.
        let target = known
            .agents
            .iter()
            .find(|agent| agent.matrix_user.as_str() == name)
            .or_else(|| resolve(&known, &name).ok());
        match target {
            Some(target) => (target.drive.clone(), target.matrix_user.to_string()),
            None => (String::new(), name),
        }
    }

    /// The agent a `delegate` call `wire` sends to, as the host resolves it
    /// (R167): an open delegation's own target for a next round — whatever
    /// `agent` says, which [`DelegateTools::run`] refuses when it differs —
    /// or the agent `agent` names, with its audience when it is known.
    /// `None` for a call that names no one: a `reply`, which goes to the
    /// session's own requester, or arguments that do not read.
    pub fn recipient(&self, wire: &WireToolCall) -> Option<(String, Option<Readers>)> {
        if wire.name != DELEGATE {
            return None;
        }
        let args = parse_args(&wire.arguments_raw).ok()?;
        let known = self.port.as_ref()?.known();
        if let Some(id) = &args.session {
            let delegation = self.view.delegation(id)?;
            let audience = audience_of(&known, &delegation.to);
            return Some((delegation.to.to_string(), audience));
        }
        let audience = resolve(&known, &args.agent)
            .ok()
            .map(|target| target.home_readers.clone());
        Some((args.agent, audience))
    }

    fn delegate(&self, raw: &str, audit: &CallAudit<'_>) -> Option<ToolOutcome> {
        let args = match parse_args(raw) {
            Ok(args) => args,
            Err(sentence) => return refused(sentence),
        };
        let Some(port) = self.port.clone() else {
            return refused(NO_ROOMS);
        };
        if let Some(id) = &args.session {
            return self.next_round(port.as_ref(), id, &args.agent, &args.brief, audit);
        }
        let source = args
            .source
            .as_deref()
            .map(str::trim)
            .filter(|source| !source.is_empty());
        // A card of another session — a dispatch run's input — is handed on
        // as the one delegation its binding names, however many runs ask.
        let mut handoff = None;
        if let Some(source) = source {
            // A card handed on already names its delegation, whichever day
            // asks again: its exchange goes on there.
            if let Some(open) = self.view.handed(source) {
                return handed_already(source, &open.id);
            }
            match handed_from(source) {
                Some((session, card)) => {
                    let Some(row) = crate::sessions::verbs::find(&self.from.zone, session) else {
                        return refused(format!("{session} is no session of this drive."));
                    };
                    if let Err(sentence) = card_in(&self.from.zone, &row.path, card) {
                        return refused(sentence.replace("this session", "that session"));
                    }
                    if let Some(id) =
                        handed_on(&self.from.zone, &self.from.user, &row.path, session, card)
                    {
                        return handed_already(source, &id);
                    }
                    handoff =
                        Some(keeper_core::agents::workflow::handoff_id(session, card).to_string());
                }
                None => {
                    if let Err(sentence) = card_in(&self.from.zone, &self.from.session, source) {
                        return refused(sentence);
                    }
                }
            }
        }
        // A resumed call approved for these bytes sends under the same id.
        let id = audit.delegation_id(handoff.unwrap_or_else(|| Ulid::new().to_string()));
        let known = port.known();
        let target = match resolve(&known, &args.agent) {
            Ok(target) => target,
            Err(sentence) => return self.refuse(&id, args.agent.trim(), None, sentence),
        };
        let to = target.matrix_user.to_string();
        if target.matrix_user == self.from.user {
            return self.refuse(
                &id,
                &to,
                None,
                "An agent does not delegate to itself.".to_owned(),
            );
        }
        if let Err(bound) = Limits::of(&self.from.limits).check(self.from.hop, 0, 0) {
            return self.refuse(&id, &to, None, bound.sentence());
        }
        let label = self.view.label();
        let content = match compose(&args, &self.from, target, &label, &id) {
            Ok(content) => content,
            Err(sentence) => return self.refuse(&id, &to, None, sentence),
        };
        // The room's observers are the session's readers; a declassified
        // brief's wider readers read it through the target's own sessions.
        let invites = room_invites(&target.matrix_user, &self.from.user, &label);
        let members = observers(&invites, &target.matrix_user);
        let effect = brief_effect(&content);
        let destination = Destination::Agent {
            drive: target.drive.clone(),
            agent: target.matrix_user.clone(),
            room: None,
        };
        for sink in [
            Sink::Delegation {
                target_audience: target.home_readers.clone(),
                room_members: members.clone(),
            },
            Sink::Room {
                humans: members.clone(),
                agent_audiences: vec![target.home_readers.clone()],
            },
        ] {
            if let Err(mut blocked) =
                self.sinks
                    .verdict(DELEGATE, &destination, &content.label, &sink, &effect, None)
            {
                // What a person would let through is the brief under the
                // label they would approve: those are its bytes (R193).
                if let Some(flow) = blocked.flow.as_mut() {
                    let widened = approved_brief(&label, &flow.readers, |approved| {
                        compose(&args, &self.from, target, approved, &id)
                    });
                    if let Some((_, sha)) = widened {
                        flow.effect_sha256 = sha;
                    }
                }
                if let Err(withheld) = audit.blocked(blocked) {
                    return self.withheld(&id, &to, None, withheld);
                }
            }
        }
        // The label's say first, then the integrity rule's and the tier's: a
        // card with a schedule or a workflow is a person's to allow (S-21,
        // T3).
        if let Err(withheld) = audit.admit(&target.drive, &to) {
            return self.withheld(&id, &to, None, withheld);
        }
        let name = session_title(&target.id, chrono::Utc::now());
        let room = match block_on(port.create(
            SessionKind::Delegated,
            &name,
            invites,
            vec![target.matrix_user.clone()],
        )) {
            Ok(room) => room,
            Err(error) => {
                return self.refuse(
                    &id,
                    &to,
                    None,
                    format!("The delegation's room could not be made: {error}"),
                )
            }
        };
        port.watch(&room, &self.from.room, self.from.kind);
        self.line(LineBody::Delegate(DelegateBody {
            id: id.clone(),
            to,
            room: Some(room),
            child: None,
            state: DelegateState::Opened,
            reason: None,
            reply: None,
            window: None,
        }));
        // The card it hands on says so, and is not handed on again; another
        // session's card is that session's to write.
        if let Some(source) = source.filter(|source| handed_from(source).is_none()) {
            let (zone, session) = (&self.from.zone, &self.from.session);
            if let Err(error) =
                crate::cards::write_run(zone, session, source, Run::Running, None, &|| {
                    self.view.may_write()
                })
            {
                tracing::warn!(%session, card = source, %error, "agents: a handed-on card could not be marked");
            }
        }
        // The id is what a later round names: the result says it, and a
        // replay of the session says it again.
        Some(ToolOutcome::Answered {
            text: format!(
                "Handed to {} as delegation {id}; waiting for it to join. Its reply will come to this session. To say more in this exchange, call delegate with session = {id}.",
                target.name
            ),
        })
    }

    /// The next round of an open delegation's exchange (R49), to the agent
    /// it was opened with: a call naming another `agent` is refused.
    fn next_round(
        &self,
        port: &dyn DelegationPort,
        id: &str,
        agent: &str,
        brief: &str,
        audit: &CallAudit<'_>,
    ) -> Option<ToolOutcome> {
        let Some(delegation) = self.view.delegation(id) else {
            return refused(format!("No delegation of this session has the id {id}."));
        };
        let to = delegation.to.to_string();
        let known = port.known();
        let Some(target) = known
            .agents
            .iter()
            .find(|known| known.matrix_user == delegation.to)
        else {
            return refused(format!("{to} is no longer known on this host."));
        };
        if resolve(&known, agent).map(|named| &named.matrix_user) != Ok(&delegation.to) {
            return self.refuse(
                id,
                &to,
                Some(delegation.room.clone()),
                format!(
                    "Delegation {id} is an exchange with {}, not {}; name it to send it more.",
                    target.name_in_drive(),
                    agent.trim()
                ),
            );
        }
        if !delegation.sent {
            return refused(format!(
                "{to} has not joined yet; the brief goes in once it does."
            ));
        }
        let limit = self.from.limits.rounds_per_exchange;
        if delegation.rounds >= limit {
            let bound = BoundReached::Rounds { limit };
            return self.refuse(id, &to, Some(delegation.room.clone()), bound.sentence());
        }
        let label = self.view.label();
        let round = |label: &Label| {
            content_for(&delegation, &self.from, label, &known).map(|mut content| {
                content.brief = brief.to_owned();
                content.card = None;
                content
            })
        };
        let content = match round(&label) {
            Ok(content) => content,
            Err(sentence) => return refused(sentence),
        };
        // A round a person lets through goes as the brief under the label
        // they approve, as the opening brief does (R193).
        let mut approved = None;
        // Its answer comes back here whatever this copy was told before.
        port.watch(&delegation.room, &self.from.room, self.from.kind);
        let checked = block_on(room_now(
            port,
            &delegation.room,
            [&self.from.user, &delegation.to],
            vec![target.home_readers.clone()],
        ))
        .map_err(|unread| Blocked {
            drive: target.drive.clone(),
            at: to.clone(),
            sentence: unread,
            flow: None,
        })
        .and_then(|sink| {
            self.sinks.verdict(
                DELEGATE,
                &Destination::Agent {
                    drive: target.drive.clone(),
                    agent: delegation.to.clone(),
                    room: Some(delegation.room.clone()),
                },
                &content.label,
                &sink,
                &brief_effect(&content),
                None,
            )
        })
        .or_else(|mut blocked| {
            if let Some(flow) = blocked.flow.as_mut() {
                if let Some((widened, sha)) = approved_brief(&label, &flow.readers, round) {
                    flow.effect_sha256 = sha;
                    approved = Some(widened);
                }
            }
            audit.blocked(blocked)
        })
        .and_then(|()| audit.admit(&target.drive, &to));
        if let Err(withheld) = checked {
            return self.withheld(id, &to, Some(delegation.room.clone()), withheld);
        }
        // Released, the approved bytes go; nothing blocked, the round's own.
        let content = approved.unwrap_or(content);
        let txn = TransactionId::new();
        if let Err(error) = block_on(port.send(&delegation.room, brief_content(&content), txn)) {
            return refused(format!("The message could not be sent: {error}"));
        }
        self.line(LineBody::Delegate(DelegateBody {
            id: id.to_owned(),
            to: to.clone(),
            room: Some(delegation.room),
            child: None,
            state: DelegateState::Sent,
            reason: None,
            reply: None,
            window: None,
        }));
        Some(ToolOutcome::Answered {
            text: format!("Sent to {to}."),
        })
    }

    fn reply(&self, args: Option<&Value>, audit: &CallAudit<'_>) -> Option<ToolOutcome> {
        let Some(text) = args.and_then(|args| args["text"].as_str()) else {
            return refused("reply needs a \"text\" argument.");
        };
        let mut handed = Vec::new();
        for rel in args
            .and_then(|args| args["artifacts"].as_array())
            .into_iter()
            .flatten()
        {
            let Some(rel) = rel.as_str() else {
                return refused("reply's artifacts are session-relative paths.");
            };
            if let Err(sentence) = artifact_in(&self.from.zone, &self.from.session, rel) {
                return refused(sentence);
            }
            handed.push(json!({
                "drive": self.from.drive,
                "path": format!("{}/{}/{rel}", self.from.subfolder, self.from.session),
            }));
        }
        let Some(port) = self.port.clone() else {
            return refused(NO_ROOMS);
        };
        // A workflow's run closes at its reply: each output it declared and
        // did not write is named in the reply and on its `run` line (R107).
        let missing: Vec<String> = self
            .outputs
            .iter()
            .filter(|path| artifact_in(&self.from.zone, &self.from.session, path).is_err())
            .map(|path| keeper_core::agents::workflow::missing_output(path))
            .collect();
        let text = if missing.is_empty() {
            text.to_owned()
        } else {
            format!("{text}\n\n{}.", missing.join(".\n"))
        };
        let (me, requester) = (&self.from.user, &self.from.requester);
        let label = self.view.label();
        let content = reply_content(&text, handed, &label);
        let admitted = block_on(reply_verdict(
            port.as_ref(),
            self.sinks,
            &self.from.room,
            [me, requester],
            &label,
            &content,
        ))
        .or_else(|blocked| audit.blocked(blocked))
        .and_then(|()| audit.admit("", self.from.room.as_str()));
        if let Err(withheld) = admitted {
            return self.withheld(
                &self.from.id,
                requester.as_str(),
                Some(self.from.room.clone()),
                withheld,
            );
        }
        if let Err(error) = block_on(self.room.send(
            "m.room.message",
            content,
            TransactionId::new(),
        )) {
            return refused(format!("The reply could not be sent: {error}"));
        }
        // The exchange closes on this line alone: a refused or failed reply
        // leaves its rounds counted.
        self.line(LineBody::Delegate(DelegateBody {
            id: self.from.id.clone(),
            to: requester.to_string(),
            room: Some(self.from.room.clone()),
            child: None,
            state: DelegateState::Replied,
            reason: None,
            reply: None,
            window: None,
        }));
        // A workflow's run ends here, whatever becomes of its card: the line
        // is the log's own word (R202).
        let (zone, session) = (self.from.zone.clone(), self.from.session.clone());
        let carded = set_card_run(&zone, &session, Run::Review, &|| self.view.may_write());
        if let Err(error) = &carded {
            tracing::warn!(%session, %error, "agents: a reply's card could not be set to review");
        }
        if carded.is_ok() || self.from.kind == SessionKind::Workflow {
            self.line(LineBody::Run(RunBody {
                state: RunState::Review,
                detail: (!missing.is_empty()).then(|| missing.join("; ")),
                step: None,
            }));
        }
        Some(ToolOutcome::Answered {
            text: "Replied.".to_owned(),
        })
    }
}

#[cfg(test)]
mod tests {
    use keeper_core::agents::home::AgentKind;
    use keeper_core::agents::label::{Integrity, Readers};

    use super::*;

    fn agent(drive: &str, id: &str) -> KnownAgent {
        let readers = Readers::Only(Default::default());
        KnownAgent {
            id: id.to_owned(),
            drive: drive.to_owned(),
            name: id.to_owned(),
            matrix_user: OwnedUserId::try_from(format!("@{drive}-{id}:h")).expect("user"),
            kind: AgentKind::Specialist,
            human: None,
            hosted: false,
            home_readers: readers.clone(),
            opening: Label {
                readers,
                integrity: Integrity::Owner,
                local_only: false,
            },
            drives: vec![drive.to_owned()],
        }
    }

    /// R67: `<drive>/<id>` names one agent; a bare id only when it is
    /// unique, otherwise the refusal names the candidates.
    #[test]
    fn a_target_is_named_by_drive_and_id() {
        let known = Known {
            agents: vec![
                agent("tgdrive", "amelia"),
                agent("neuradrive", "amelia"),
                agent("tgdrive", "tola-grey"),
            ],
            trust: Vec::new(),
        };
        assert_eq!(
            resolve(&known, "neuradrive/amelia").expect("one").drive,
            "neuradrive"
        );
        assert_eq!(resolve(&known, "tola-grey").expect("one").id, "tola-grey");
        let both = resolve(&known, "amelia").expect_err("two");
        assert!(
            both.contains("tgdrive/amelia") && both.contains("neuradrive/amelia"),
            "{both}"
        );
        assert!(resolve(&known, "tgdrive/winston").is_err());
    }

    /// FR-795: a declassification names the whole brief event, so
    /// the same brief to the same agent with two different cards is two
    /// effects with two digests; the same one twice is one.
    #[test]
    fn one_brief_with_two_cards_is_two_effects() {
        use keeper_core::agents::delegation::{DelegateCard, DelegateFrom, DelegateLimits};
        use keeper_core::agents::label::{declassify_request, Destination, Sink};
        let tgorka = OwnedUserId::try_from("@tgorka:h").expect("user");
        let with_card = |title: &str| DelegateContent {
            v: 1,
            id: "01J00000000000000000000000".to_owned(),
            from: DelegateFrom {
                agent: OwnedUserId::try_from("@nixi:h").expect("user"),
                drive: "tgdrive".to_owned(),
                session: "active/s".to_owned(),
                room: OwnedRoomId::try_from("!s:h").expect("room"),
            },
            to: OwnedUserId::try_from("@lucyna:h").expect("user"),
            brief: "Sort the inbox.".to_owned(),
            drives: vec!["neuradrive".to_owned()],
            label: Label {
                readers: Readers::Only([tgorka.clone()].into()),
                integrity: Integrity::Owner,
                local_only: false,
            },
            hop: 1,
            limits: DelegateLimits {
                rounds_per_exchange: 3,
                tokens: 1000,
            },
            card: Some(DelegateCard {
                title: title.to_owned(),
                schedule: None,
                workflow: None,
            }),
            dispatch_chain: Vec::new(),
        };
        let destination = Destination::Agent {
            drive: "neuradrive".to_owned(),
            agent: OwnedUserId::try_from("@lucyna:h").expect("user"),
            room: None,
        };
        let sink = Sink::Delegation {
            target_audience: Readers::Anyone,
            room_members: Default::default(),
        };
        let digest = |content: &DelegateContent| {
            declassify_request(
                &brief_effect(content),
                None,
                &destination,
                &sink,
                &content.label,
                &|_| None,
            )
            .effect_sha256
        };
        let inbox = with_card("Inbox");
        assert_eq!(digest(&inbox), digest(&with_card("Inbox")));
        assert_ne!(digest(&inbox), digest(&with_card("Payroll")));
    }

    /// A reply hands over only files under its session's `artifacts/`:
    /// `..`, a path elsewhere in the session, a missing file, and a link out
    /// of `artifacts/` — to another session's file or the session's own
    /// `agent.toml` — are refused.
    #[cfg(unix)]
    #[test]
    fn a_reply_hands_over_only_its_own_artifacts() {
        let zone = tempfile::tempdir().expect("zone");
        let session = "active/2026-10-04-tola";
        let dir = zone.path().join(session);
        std::fs::create_dir_all(dir.join("artifacts")).expect("artifacts");
        std::fs::write(dir.join("artifacts/report.md"), "# Inbox\n").expect("report");
        std::fs::write(dir.join("agent.toml"), "version = 1\n").expect("agent.toml");
        let other = zone.path().join("active/2026-10-04-other");
        std::fs::create_dir_all(&other).expect("other");
        std::fs::write(other.join("secret.md"), "theirs\n").expect("secret");
        std::os::unix::fs::symlink(other.join("secret.md"), dir.join("artifacts/out.md"))
            .expect("link out");
        std::os::unix::fs::symlink(dir.join("agent.toml"), dir.join("artifacts/own.md"))
            .expect("link in");

        assert_eq!(
            artifact_in(zone.path(), session, "artifacts/report.md"),
            Ok(())
        );
        for rel in [
            "artifacts/../agent.toml",
            "agent.toml",
            "artifacts/missing.md",
            "artifacts/out.md",
            "artifacts/own.md",
            "artifacts",
        ] {
            assert!(artifact_in(zone.path(), session, rel).is_err(), "{rel}");
        }
    }
}
