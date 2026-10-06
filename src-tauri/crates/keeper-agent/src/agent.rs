//! An agent's turn in a session (C2, F2, S-04; story 90.5).
//!
//! # The context is held, not re-read
//!
//! [`SessionContext`] is what a served session's turns read: the
//! conversation, the frozen core memory, the label, the scope. It is loaded
//! once per (session, claim) — when this host opens the session, takes it
//! over or restarts — from the log's lines, and every line the
//! [`SessionWriter`] writes afterwards is pushed into it the way a replay
//! would put it there. A turn never opens a chunk (AD-365, NFR-116).
//!
//! # The model is a sink
//!
//! Before every request — each round of the tool loop — the turn asks the
//! session's label whether this agent's model may see it (S-04). A drive read
//! during the turn joins the label, so a read of a `local_only` drive stops
//! the very next round from leaving for a model that does not run locally.
//!
//! # What a turn writes
//!
//! A `user` line for the person's message, one `assistant` line per round
//! that called tools (its `tool_call` lines point at it, its `tool_result`
//! lines at them), a `label` line whenever a read narrows the label, and the
//! final `assistant` line naming the anchor its answer edited — or an `error`
//! line when the turn could not finish.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use chrono::{DateTime, FixedOffset};
use keeper_core::agents::card::Run;
use keeper_core::agents::delegation::{read_brief, BoundReached, Limits as DelegationLimits};
use keeper_core::agents::drive::DriveDecl;
use keeper_core::agents::events::{
    edit_content, ConversationRequestContent, Focus, RunState, ScopeContent, ScopeDrive,
    StatusContent, ARTIFACTS, CONTENT_VERSION, SCOPE, STATUS, TURN,
};
use keeper_core::agents::focus::FOCUS_TTL;
use keeper_core::agents::home::{serves_local_models, MenuItem};
use keeper_core::agents::label::{
    check_call, check_sink, label_drive_read, label_person_message, okf_label_facts, Author,
    CallVerdict, Destination, Integrity, Label, LabelBody, LabelCause, LabelCauseKind, ReadFacts,
    Readers, Recipient, Sink, SinkVerdict, NEEDS_APPROVAL,
};
use keeper_core::agents::log::reader::{hydrate_blob, read_session};
use keeper_core::agents::log::replay::{message_for, ReplayRefusal};
use keeper_core::agents::log::{
    ApprovalBody, ApprovalState, AssistantBody, ChildSession, DelegateBody, DelegateReply,
    DelegateState, ErrorBody, HostSlug, LineBody, LogLine, OpenBody, PeerBody, RunBody, ScopeBody,
    ToldBody, ToolCallBody, ToolOutcomeWord, ToolResultBody, Truncated, Usage, UserBody,
};
use keeper_core::agents::matrix::AgentMatrixError;
use keeper_core::agents::memory::{self, MemorySnapshot};
use keeper_core::agents::prompt::{self, ComposedPrompt, PromptInput, RenderedFact, SessionFrame};
use keeper_core::agents::proxy::{conversation_session_id, ScopeRequest, NEW_CONVERSATION_TITLE};
use keeper_core::agents::redact::redact_secrets;
use keeper_core::agents::session::{SessionAgent, SessionKind};
use keeper_core::agents::skills::SkillsIndex;
use keeper_core::agents::soul::{self, Fact, Soul};
use keeper_core::agents::tier::{
    self, AgentTool, CallFacts, Classification, Context, Gate, GrantWord, Place, Tier, FORBIDDEN,
};
use keeper_core::bots::chat::{self, CancelSignal, ChatEvent, ChatMessage, ChatOptions, Role};
use keeper_core::bots::context_files::ContextBundle;
use keeper_core::bots::error::BotsError;
use keeper_core::bots::grant::Effect;
use keeper_core::bots::store::ProviderRow;
use keeper_core::bots::tools::{
    self, ToolCall, ToolCallRecord, ToolHost, ToolLoop, ToolLoopEvent, ToolLoopOptions, ToolName,
    ToolOutcome,
};
use keeper_core::bots::{http, Bot};
use keeper_core::error::CoreError;
use keeper_sync::{browse, names, SyncProfile};
use matrix_sdk::ruma::{EventId, OwnedEventId, OwnedRoomId, OwnedUserId, RoomId, UserId};
use serde_json::{json, Value};
use tokio::sync::mpsc;
use tokio::time::Instant;
use ulid::Ulid;

use crate::approvals::Parking;
use crate::cards::{self, Begun, Scheduled};
use crate::claims::Lease;
use crate::delegate::{self, DelegateTools, Delegation, DelegationPort, Delegator, TurnView};
use crate::drive::finish_word;
use crate::grants::AgentGrants;
use crate::host::{AgentDrive, Approval, Classified, HostIds, UNATTENDED_REFUSAL};
use crate::matrix_sink::{
    anchor_content, cut, cut_to_log, deliver, deliver_gated, deliver_unless_narrowed,
    notice_content, status_content, EditPort, MatrixSink, SendFuture, StatusBoard, ToolProgress,
};
use crate::ports::ProfileSource;
use crate::rooms::{self, Arrival, BriefEvent, Disposition, Served};
use crate::sessions::verbs::{self, CreateOutcome};
use crate::sessions::write::session_write;
use crate::sinks::{
    room_audience, CallAudit, Gated, ProxyDoors, RoomGate, Sinks, MEMBERS_UNREAD, NARROWED_STATUS,
};
use crate::turn::{arm_turn_probing, endpoint_of, read_timeout_of, TurnEnv, TurnOrigin};
use crate::writer::{SessionWriter, WriterError};
use crate::zone::{read_text, AgentHome};

/// A session, by its drive and its zone-relative folder.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct SessionRef {
    /// The drive whose sessions zone holds it.
    pub drive: String,
    /// Zone-relative: `active/2026-10-02-release`.
    pub path: String,
}

/// What the final edit says when the label stopped the model (S-04).
pub const LOCAL_ONLY_REFUSAL: &str = "This conversation has read something that may go only to a model on your own machines, and this agent's model is not one. Nothing more was sent.";

/// What the room is told when the model or the host failed mid-answer; the
/// reason itself is in the session's log.
pub const TURN_FAILED: &str =
    "I could not finish this answer. The reason is in this session's log.";

/// What the room is told instead of an answer when the session's label no
/// longer reaches everyone the session's `agent.toml` names (S-16): the
/// answer drew on something narrower than the room. The log keeps the whole
/// answer, with an `error` line whose code is [`LABEL_CODE`].
pub const NARROWER_THAN_ROOM: &str = "This answer drew on something not everyone in this room may read, so it is not shown here. It is in this session's log.";

/// The code of the `error` line of an answer withheld from its room.
const LABEL_CODE: &str = "label";

/// The `finish` of an `assistant` line written for a round that called
/// tools: the turn goes on after it, so it answers nothing yet.
const ROUND_FINISH: &str = "tool_calls";

/// The message for a turn cut off by a restart (C6): it is not re-run,
/// because its tool calls may already have had effects.
pub fn cut_off_sentence(host: &str) -> String {
    format!("My answer was cut off when {host} restarted. Ask again if you still need it.")
}

/// The words a turn stopped by shutdown ends with.
pub fn shutdown_suffix(host: &str) -> String {
    format!(" … (stopped: {host} is shutting down)")
}

/// Why a session's context could not be loaded; the session is not served.
#[derive(Debug, thiserror::Error)]
pub enum LoadRefusal {
    #[error(transparent)]
    Replay(#[from] ReplayRefusal),
    #[error("{0}")]
    Home(String),
}

/// The docked note's focus as the host holds it: the note, and when its
/// scope event reached this host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HeldFocus {
    pub focus: Focus,
    pub heard: Instant,
}

/// What a served session's turns read (F2).
pub struct SessionContext {
    pub session: SessionRef,
    /// The session's `agent.toml`.
    pub agent: SessionAgent,
    /// The conversation as the model is sent it, system prompt aside.
    pub messages: Vec<ChatMessage>,
    /// Per message: the line that made it, and that line's position.
    placed: Vec<(Ulid, usize)>,
    /// Every line pushed, in order.
    lines: Vec<Ulid>,
    /// Core memory, read once when the context loaded (AD-364).
    pub memory_snapshot: MemorySnapshot,
    soul: Soul,
    facts: Vec<RenderedFact>,
    skills: SkillsIndex,
    menu: Vec<MenuItem>,
    /// The session's label after every join so far.
    pub label: Label,
    /// The drives in scope.
    pub scope: Vec<String>,
    /// Who set the scope last: the session's requester until a `scope` line.
    pub scope_set_by: OwnedUserId,
    /// The note the person's docked notes view shows (R41): from their
    /// newest scope event, held in memory and never logged, and stated
    /// only while it was heard within [`FOCUS_TTL`] — the dock says it again
    /// while it stays open, so one that was never cleared (a quit, a crash)
    /// stops being stated.
    pub focus: Option<HeldFocus>,
    pub epoch: u64,
    pub claim: Option<String>,
    /// The last `open` line.
    pub open: Option<OpenBody>,
    /// The frame's time: the last `open` line's, or when the context loaded
    /// while there is none. It moves only when a new `open` line is written,
    /// so a session's prompt changes only when what it says changes, and
    /// `status --session` recomposes exactly what the model was told.
    pub frame_time: DateTime<FixedOffset>,
    /// A `user` or `peer` line no `assistant` or `error` line answered yet.
    pub unanswered: Option<Ulid>,
    /// The session's status anchor in its room, once there is one.
    pub status_anchor: Option<OwnedEventId>,
    /// The delegations this session made, by id (R55).
    pub delegations: BTreeMap<String, Delegation>,
    /// Each `delegate` call's arguments, by its `tool_call` line, until its
    /// `delegate opened` line names it.
    delegate_calls: HashMap<Ulid, String>,
    /// Tokens every `assistant` line reports, summed: what a delegated
    /// session has spent of its budget.
    pub tokens_spent: u64,
    /// Messages from the delegating agent since this session's last reply:
    /// the rounds of the exchange under way (Q12). Only a reply that went
    /// out — this session's own `delegate replied` line — closes it.
    pub exchange_rounds: u32,
    /// Whether this delegated session logged `delegate accepted`.
    pub accepted: bool,
    /// The last `run` line's state.
    pub run: Option<keeper_core::agents::log::RunState>,
    /// A delegation's reply whose receipt is logged and whose `peer` line
    /// is not: the receipt's line, its event, and the receipt.
    reply_unpeered: Option<(Ulid, Option<OwnedEventId>, DelegateBody)>,
    /// The harvests this session began, by arrival id: from its log's
    /// `peer` lines, every host's, and from the anchors another copy left
    /// in the room ([`Self::started`]). Never a second turn (R61).
    harvested: HashSet<String>,
    /// Whether the session's person was told, in their proxy's DM, that
    /// its work went on where only some of its room may read it (R169).
    pub told: bool,
    /// Whether an answer of the session was withheld from its room as the
    /// room then was — an `error` line coded [`LABEL_CODE`] (R168).
    pub withheld: bool,
    /// The approvals whose call has no result yet, by id (93.2): a turn
    /// parked on one is waiting, not unanswered, and `recover` leaves it.
    pub parked: BTreeMap<String, crate::approvals::Pending>,
    /// The approvals consumed here and run, by id, from their `consumed`
    /// lines with no result — never one mirrored from another copy.
    pub(crate) consumed: HashSet<String>,
    /// Every `tool_call` line with no result yet, in order: a parked
    /// round's call and the calls after it.
    open_calls: Vec<(Ulid, chat::ToolCall)>,
}

impl SessionContext {
    /// Load the context of the session at `dir` from its log and its
    /// agent's home: the cold path, once per (session, claim).
    ///
    /// The log is read with `read_session` and each line, its blob hydrated,
    /// is pushed exactly as the writer pushes it later, so the loaded
    /// context and a fresh `replay` agree by construction. A conflicted log
    /// (two hosts at one epoch) loads nothing.
    #[allow(clippy::too_many_arguments)]
    pub fn load(
        home: &AgentHome,
        dir: &Path,
        session: SessionRef,
        agent: SessionAgent,
        epoch: u64,
        claim: Option<String>,
        opened_at: DateTime<FixedOffset>,
    ) -> Result<SessionContext, LoadRefusal> {
        let log = read_session(dir);
        if log.conflicted() {
            return Err(ReplayRefusal::Conflicted.into());
        }
        let frozen = Frozen::read(home)?;
        let mut context = SessionContext {
            session,
            label: agent.label.clone(),
            scope: agent.drives.clone(),
            scope_set_by: agent.requested_by.clone(),
            focus: None,
            agent,
            messages: Vec::new(),
            placed: Vec::new(),
            lines: Vec::new(),
            memory_snapshot: frozen.memory,
            soul: frozen.soul,
            facts: frozen.facts,
            skills: frozen.skills,
            menu: home.config.menu.clone(),
            epoch,
            claim,
            open: None,
            frame_time: opened_at,
            unanswered: None,
            status_anchor: None,
            delegations: BTreeMap::new(),
            delegate_calls: HashMap::new(),
            tokens_spent: 0,
            exchange_rounds: 0,
            accepted: false,
            run: None,
            reply_unpeered: None,
            harvested: HashSet::new(),
            told: false,
            withheld: false,
            parked: BTreeMap::new(),
            consumed: HashSet::new(),
            open_calls: Vec::new(),
        };
        for stored in &log.lines {
            match &stored.body {
                LineBody::Blob(blob) => {
                    let value =
                        hydrate_blob(dir, &blob.sha256).map_err(|source| ReplayRefusal::Blob {
                            sha256: blob.sha256.clone(),
                            source,
                        })?;
                    let body = LineBody::decode(blob.kind, value).map_err(|error| {
                        ReplayRefusal::BlobShape {
                            sha256: blob.sha256.clone(),
                            kind: blob.kind.as_str(),
                            detail: error.to_string(),
                        }
                    })?;
                    context.push(&LogLine {
                        body,
                        ..stored.clone()
                    });
                }
                _ => context.push(stored),
            }
        }
        Ok(context)
    }

    /// Take one written (or read) line into the context, as `replay` would.
    pub fn push(&mut self, line: &LogLine) {
        let position = self.lines.len();
        self.lines.push(line.id);
        self.keep(line);
        self.track(line);
        match &line.body {
            // A label line holds the label after its join.
            LineBody::Label(body) => self.label = body.label(),
            LineBody::Scope(body) => {
                self.scope = body.drives.clone();
                self.scope_set_by = body.set_by.clone();
            }
            LineBody::Open(body) => {
                self.open = Some(body.clone());
                self.frame_time = line.ts.with_timezone(&chrono::Local).fixed_offset();
            }
            LineBody::User(_) | LineBody::Peer(_) => self.unanswered = Some(line.id),
            // A round that called tools is the middle of a turn (C6): a crash
            // after it still leaves the person's question unanswered.
            LineBody::Assistant(body) if body.finish == ROUND_FINISH => {}
            LineBody::Assistant(_) | LineBody::Error(_) => self.unanswered = None,
            LineBody::Compact(compact) => {
                let through = self
                    .lines
                    .iter()
                    .position(|id| *id == compact.replaces_through)
                    .unwrap_or(position);
                let mut kept = Vec::with_capacity(self.messages.len());
                let mut kept_placed = Vec::with_capacity(self.placed.len());
                for (message, placed) in self.messages.drain(..).zip(self.placed.drain(..)) {
                    if placed.1 > through {
                        kept.push(message);
                        kept_placed.push(placed);
                    }
                }
                self.messages = kept;
                self.placed = kept_placed;
                if let Some(message) = message_for(line) {
                    self.messages.insert(0, message);
                    self.placed.insert(0, (line.id, through));
                }
                return;
            }
            LineBody::ToolCall(_) => {
                if let Some(message) = message_for(line) {
                    let owner = line.parent.and_then(|parent| {
                        self.placed
                            .iter()
                            .position(|(id, _)| *id == parent)
                            .filter(|at| self.messages[*at].role == Role::Assistant)
                    });
                    match owner {
                        Some(at) => self.messages[at].tool_calls.extend(message.tool_calls),
                        None => {
                            self.messages.push(message);
                            self.placed.push((line.id, position));
                        }
                    }
                }
                return;
            }
            _ => {}
        }
        if let Some(message) = message_for(line) {
            self.messages.push(message);
            self.placed.push((line.id, position));
        }
    }

    /// The calls a parked round left open, and its approvals.
    fn track(&mut self, line: &LogLine) {
        match &line.body {
            LineBody::ToolCall(call) => self.open_calls.push((
                line.id,
                chat::ToolCall {
                    id: call.call_id.clone(),
                    name: call.tool.clone(),
                    arguments_raw: call.args.clone(),
                    arguments: serde_json::from_str(&call.args).ok(),
                },
            )),
            LineBody::ToolResult(result) => {
                self.open_calls
                    .retain(|(_, wire)| wire.id != result.call_id);
                // An approval stays until every call of its round has its
                // result, so a continuation a stop cut is resumed (R176).
                for pending in self.parked.values_mut() {
                    pending.round.retain(|id| *id != result.call_id);
                }
                self.parked.retain(|_, pending| !pending.round.is_empty());
            }
            LineBody::Approval(body) => match body.state {
                ApprovalState::Requested => {
                    let round = line
                        .parent
                        .map(|parent| self.calls_from(parent))
                        .unwrap_or_default();
                    if let Some((call_line, call)) = round.first() {
                        self.parked.insert(
                            body.id.clone(),
                            crate::approvals::Pending {
                                id: body.id.clone(),
                                call_id: call.id.clone(),
                                call_line: *call_line,
                                request_event: line.matrix_event.clone(),
                                ended: None,
                                round: round.iter().map(|(_, wire)| wire.id.clone()).collect(),
                                announced: true,
                                ran: false,
                            },
                        );
                    }
                }
                _ if body.is_terminal() => {
                    // Consumed here, not mirrored from another copy: it
                    // runs here, and the rest of its round with it.
                    let ran = body.state == ApprovalState::Consumed && body.result.is_none();
                    if ran {
                        self.consumed.insert(body.id.clone());
                    }
                    if let Some(pending) = self.parked.get_mut(&body.id) {
                        pending.ended.get_or_insert(body.state);
                        pending.ran |= ran;
                    }
                }
                _ => {}
            },
            _ => {}
        }
    }

    /// The open calls whose ids are `ids`, in the log's order.
    pub(crate) fn calls_of(&self, ids: &[String]) -> Vec<(Ulid, chat::ToolCall)> {
        self.open_calls
            .iter()
            .filter(|(_, wire)| ids.contains(&wire.id))
            .cloned()
            .collect()
    }

    /// The open calls from the `tool_call` line `call_line` on, in order.
    pub(crate) fn calls_from(&self, call_line: Ulid) -> Vec<(Ulid, chat::ToolCall)> {
        self.open_calls
            .iter()
            .skip_while(|(id, _)| *id != call_line)
            .cloned()
            .collect()
    }

    /// What a line changes beyond the conversation: the budget, the
    /// exchange, the delegations, the run.
    fn keep(&mut self, line: &LogLine) {
        match &line.body {
            LineBody::Assistant(body) => {
                let used = |n: Option<u32>| u64::from(n.unwrap_or(0));
                self.tokens_spent += used(body.usage.prompt) + used(body.usage.completion);
            }
            LineBody::ToolCall(call) if call.tool == delegate::DELEGATE => {
                self.delegate_calls.insert(line.id, call.args.clone());
            }
            LineBody::Peer(peer) => {
                if peer.sender == self.agent.requested_by {
                    self.exchange_rounds += 1;
                }
                self.reply_unpeered = None;
                if let Some(event) = line
                    .matrix_event
                    .as_ref()
                    .filter(|event| is_harvest_event(event.as_str()))
                {
                    self.harvested.insert(event.to_string());
                }
            }
            LineBody::Run(run) => self.run = Some(run.state),
            LineBody::Told(_) => self.told = true,
            LineBody::Error(error) if error.code == LABEL_CODE => self.withheld = true,
            LineBody::Delegate(body) => match body.state {
                DelegateState::Opened => {
                    let (Some(room), Ok(to)) =
                        (&body.room, OwnedUserId::try_from(body.to.as_str()))
                    else {
                        return;
                    };
                    let args = line
                        .parent
                        .and_then(|call| self.delegate_calls.remove(&call));
                    self.delegations.insert(
                        body.id.clone(),
                        Delegation {
                            id: body.id.clone(),
                            to,
                            room: room.clone(),
                            args,
                            sent: false,
                            replied: false,
                            rounds: 0,
                        },
                    );
                }
                DelegateState::Sent => {
                    if let Some(open) = self.delegations.get_mut(&body.id) {
                        open.sent = true;
                        open.replied = false;
                        open.rounds += 1;
                    }
                }
                // This session's own reply went out: its exchange is closed.
                DelegateState::Replied if body.id == self.agent.id.to_string() => {
                    self.exchange_rounds = 0;
                }
                DelegateState::Replied => {
                    if let Some(open) = self.delegations.get_mut(&body.id) {
                        open.replied = true;
                        open.rounds = 0;
                    }
                    if body.reply.is_some() {
                        self.reply_unpeered =
                            Some((line.id, line.matrix_event.clone(), body.clone()));
                    }
                }
                DelegateState::Accepted => self.accepted = true,
                // A brief its label no longer lets in is never sent: the
                // delegation is over before it began.
                DelegateState::Refused => {
                    if self
                        .delegations
                        .get(&body.id)
                        .is_some_and(|open| !open.sent)
                    {
                        self.delegations.remove(&body.id);
                    }
                }
            },
            _ => {}
        }
    }

    /// The harvests among `answered` — the questions another copy of this
    /// agent already answered in the room — count as begun.
    pub fn started<'a>(&mut self, answered: impl Iterator<Item = &'a str>) {
        self.harvested.extend(
            answered
                .filter(|event| is_harvest_event(event))
                .map(str::to_owned),
        );
    }

    /// Whether the harvest `event` began in this session, here or elsewhere.
    pub fn harvest_began(&self, event: &EventId) -> bool {
        self.harvested.contains(event.as_str())
    }

    /// The delegation this session handed its card `source` to.
    pub fn handed(&self, source: &str) -> Option<&Delegation> {
        let source = source.trim();
        self.delegations
            .values()
            .find(|open| delegate::source_of(open.args.as_deref()).as_deref() == Some(source))
    }

    /// Why a brief arriving now is not a turn: this delegated session's
    /// exchange has had its rounds, or its budget is spent (Q12).
    pub fn exchange_closed(&self) -> Option<&'static str> {
        let limits = self.agent.limits.as_ref()?;
        if self.token_bound().is_some() {
            Some(BUDGET_SPENT)
        } else if self.exchange_rounds >= limits.rounds_per_exchange {
            Some(ROUNDS_SPENT)
        } else {
            None
        }
    }

    /// The bound a delegated session's budget reached, if it did.
    pub fn token_bound(&self) -> Option<BoundReached> {
        let limits = self.agent.limits.as_ref()?;
        match DelegationLimits::of_session(limits).check(0, 0, self.tokens_spent) {
            Err(bound @ BoundReached::Tokens { .. }) => Some(bound),
            _ => None,
        }
    }

    /// The delegation this session made into `room`.
    pub fn delegation_in(&self, room: &RoomId) -> Option<&Delegation> {
        self.delegations.values().find(|open| open.room == room)
    }

    /// The agents this session's room sends pass between, left out of its
    /// audience (R94): its own agent `me`, and a delegated session's
    /// requester, whose session joins the label a reply carries.
    pub fn room_own(&self, me: &UserId) -> Vec<OwnedUserId> {
        let mut own = vec![me.to_owned()];
        if self.agent.kind == SessionKind::Delegated {
            own.push(self.agent.requested_by.clone());
        }
        own
    }

    /// Whether its person is owed word, once, in their proxy's DM, that the
    /// session goes on where only some of its room may read it (R64,
    /// R169): its label no longer reaches everyone it was opened for
    /// (`agent.toml`'s label), or an answer was withheld from its room as
    /// the room is now — the label unchanged and the room wider.
    pub fn narrowed(&self) -> bool {
        self.withheld || !self.label.may_reach(&self.agent.label.readers)
    }

    /// A person's message joins the label (`label_person_message`); the new
    /// label when it changed. In a `main` session a line of its person's
    /// resets the integrity to that line's instead (R28 S-09): each of
    /// their turns starts at their own word, while the readers stay
    /// cumulative. Every other session only narrows.
    pub fn on_user_line(
        &self,
        sender: &OwnedUserId,
        person: &OwnedUserId,
        readers: &Label,
    ) -> Option<Label> {
        let room = match &readers.readers {
            Readers::Only(set) => set.clone(),
            Readers::Anyone => Default::default(),
        };
        let said = label_person_message(sender, person, &room);
        let joined = if self.agent.kind == SessionKind::Main && sender == person {
            self.label.at_persons_turn(&said)
        } else {
            self.label.join(&said)
        };
        (joined != self.label).then_some(joined)
    }

    /// The system message for one turn: the frozen soul, memory, skills and
    /// menu, the frame over the current label and scope, and the context
    /// files the turn's arming loaded.
    pub fn compose(&self, deps: &AgentDeps, context: Option<&ContextBundle>) -> ComposedPrompt {
        let frame = SessionFrame {
            agent: deps.home.config.id.clone(),
            host: deps.host.as_str().to_owned(),
            session_path: format!("{}/{}", deps.sessions_subfolder, self.session.path),
            session_kind: self.agent.kind.as_str().to_owned(),
            drives: self
                .scope
                .iter()
                .map(|drive| {
                    let title = deps
                        .drives
                        .get(drive)
                        .map_or_else(|| drive.clone(), |decl| decl.title.clone());
                    (drive.clone(), title)
                })
                .collect(),
            audience_sentence: self.label.sentence(&|user| user.to_string()),
            now: self.frame_time,
            // A note in a drive outside the scope is not named to the model,
            // nor one not heard again within the TTL.
            focus: self
                .focus
                .as_ref()
                .filter(|held| held.heard.elapsed() < FOCUS_TTL)
                .map(|held| held.focus.clone())
                .filter(|focus| self.scope.contains(&focus.drive)),
        };
        prompt::compose(&PromptInput {
            soul: &self.soul,
            facts: &self.facts,
            memory: &self.memory_snapshot,
            skills: &self.skills,
            menu: &self.menu,
            frame: &frame,
            context,
        })
    }
}

/// What a context reads from the agent's home once, and keeps.
struct Frozen {
    soul: Soul,
    facts: Vec<RenderedFact>,
    skills: SkillsIndex,
    memory: MemorySnapshot,
}

impl Frozen {
    fn read(home: &AgentHome) -> Result<Frozen, LoadRefusal> {
        let text = |rel: &str| read_text(&home.dir, rel).map_err(LoadRefusal::Home);
        let soul_text = text(soul::FILE_NAME)?.ok_or_else(|| {
            LoadRefusal::Home(format!(
                "{}/ has no {}, so the agent has no voice to answer in.",
                home.config.id,
                soul::FILE_NAME
            ))
        })?;
        let soul = soul::parse_soul(&soul_text, &home.config.name)
            .map_err(|refusal| LoadRefusal::Home(refusal.to_string()))?;
        let mut facts = Vec::with_capacity(soul.persistent_facts.len());
        for fact in &soul.persistent_facts {
            match fact {
                Fact::Text(sentence) => facts.push(RenderedFact::Text(sentence.clone())),
                Fact::File(path) => match text(path)? {
                    Some(body) => facts.push(RenderedFact::File {
                        path: path.clone(),
                        text: body,
                    }),
                    None => {
                        tracing::warn!(%path, agent = %home.config.id, "agents: a persistent fact names a file the home does not hold");
                    }
                },
            }
        }
        let memory = memory::snapshot(text("USER.md")?.as_deref(), text("MEMORY.md")?.as_deref());
        Ok(Frozen {
            soul,
            facts,
            skills: crate::zone::skills_of(home),
            memory,
        })
    }
}

/// The profiles an agent's tools name, each named by its drive id.
pub struct AgentProfiles {
    profiles: Vec<SyncProfile>,
}

impl AgentProfiles {
    /// `mounted` pairs each drive id with its checkout's profile.
    pub fn new(mounted: impl IntoIterator<Item = (String, SyncProfile)>) -> AgentProfiles {
        AgentProfiles {
            profiles: mounted
                .into_iter()
                .map(|(drive, mut profile)| {
                    profile.id = drive;
                    profile
                })
                .collect(),
        }
    }
}

impl ProfileSource for AgentProfiles {
    fn profiles(&self) -> Vec<SyncProfile> {
        self.profiles.clone()
    }
}

/// Everything one hosted agent's turns run with.
pub struct AgentDeps {
    /// The platform, and the drive ports with [`AgentProfiles`] and no
    /// approver: every ask is refused (C4).
    pub env: TurnEnv,
    /// Where `keeper.db` lives.
    pub data_dir: PathBuf,
    pub row: ProviderRow,
    pub bot: Bot,
    pub home: AgentHome,
    pub host: HostSlug,
    /// The declarations of the drives this host mounts, by id.
    pub drives: BTreeMap<String, DriveDecl>,
    /// The home drive's sessions zone.
    pub sessions_zone: PathBuf,
    /// Its folder inside the drive, for the frame's drive-relative path.
    pub sessions_subfolder: String,
    pub lfs_threshold_bytes: u64,
    /// Who may decide an approval (R77): installed — agentd over its
    /// `[[trust]]` pins, the desktop over its verified accounts (R92) — a
    /// call that needs a person parks and waits for a decision; `None`, as
    /// a test world or agentd's prompt preview has it, refuses it with
    /// [`UNATTENDED_REFUSAL`].
    pub decisions: Option<Arc<dyn crate::approvals::DecisionSource>>,
}

impl AgentDeps {
    /// Whether this agent's model runs on a machine its readers control.
    pub fn model_is_local(&self) -> bool {
        serves_local_models(self.row.provider.kind)
    }
}

/// A tool host that refuses any tool outside `[tools].allow`, and serves the
/// agent's surface, `delegate`, `reply`, `card_update` and `session_write`
/// tools itself (R38, R50: no ⌘9 host has them). Every call is classified
/// (AD-392) on where it lands and audited in exactly one row with its tier
/// (R90), before any effect; one that writes or sends is checked first
/// against the label at its sink, then against the integrity rule over its
/// tier, then its tier decides whether it runs (R82's precedence).
struct AllowedTools<'t> {
    inner: Box<dyn AgentDrive>,
    allow: Vec<String>,
    surface: Option<crate::surface::SurfaceTools>,
    delegation: DelegateTools<'t>,
    cards: crate::cards::CardTools<'t>,
    view: &'t dyn TurnView,
    sinks: &'t Sinks,
    /// The declarations of the drives this host mounts: a write's audience.
    drives: &'t BTreeMap<String, DriveDecl>,
    /// The agent's home drive: where its session tools write.
    home: &'t DriveDecl,
    /// The session's folder, drive-relative: where its session tools land.
    session_dir: String,
    /// The same folder as it lands on the disk, by name.
    session_landed: Vec<String>,
    /// The mounted drives, where a write's landing is read.
    profiles: Vec<SyncProfile>,
    /// The session's `agent.toml`: whose work this is (R83).
    agent: SessionAgent,
    /// Each classified call's tier by call id, for its `tool_call` line.
    tiers: Mutex<HashMap<String, Tier>>,
    /// Whether a call that needs a person parks: a decision source is
    /// installed (R77).
    parks: bool,
    /// The consumed approval the call it bound runs on, handed to that one
    /// execution and cleared as it ends ([`AllowedTools::run_bound`]):
    /// nothing else in the turn, whatever its wire id, runs on it.
    bound: Mutex<Option<Bound>>,
    /// The call this turn parked on.
    parked: Mutex<Option<Parking>>,
}

/// A consumed approval as the one execution of its call holds it: the
/// call's wire id and tool, the record, and the flow a `declassify`
/// approval releases (R89).
struct Bound {
    call: String,
    tool: String,
    approval: Ulid,
    released: Option<crate::approvals::Released>,
}

/// Why a bound call that would now reach someone its approval did not name
/// does not run.
const AUDIENCE_MOVED: &str = "who it would reach changed after it was approved";

/// Where `subpath` of the drive checked out at `profile` lands, as
/// drive-relative names: keeper-sync's landing, every link followed. The
/// requested names where it cannot be landed — a drive this host does not
/// hold, or a path its executor refuses whatever its tier.
fn landed_names(profile: Option<&SyncProfile>, subpath: &str) -> Vec<String> {
    profile
        .and_then(|profile| browse::landing(&profile.local_path, subpath).ok())
        .unwrap_or_else(|| {
            subpath
                .split('/')
                .filter(|part| !part.is_empty())
                .map(str::to_owned)
                .collect()
        })
}

impl AllowedTools<'_> {
    /// Classify call `id` in this session now, and keep its tier for the
    /// call's line.
    fn classify(
        &self,
        id: &str,
        tool: AgentTool,
        facts: &CallFacts,
        grant: Option<GrantWord>,
    ) -> Classification {
        let context = Context {
            grant,
            ..Context::of_session(&self.agent, self.view.label().integrity)
        };
        let classification = tier::classify(tool, facts, &context);
        self.tiers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(id.to_owned(), classification.tier);
        classification
    }

    /// The tier call `id`'s line says; T0 for a call never classified — a
    /// name with no row, or arguments that do not read.
    fn tier_of(&self, id: &str) -> u8 {
        self.tiers
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .remove(id)
            .map_or(0, Tier::as_u8)
    }

    /// The facts of a write to `subpath` of `drive`, from where it lands —
    /// names compared as keeper-sync's fence compares them.
    fn landed_facts(&self, drive: &str, subpath: &str) -> CallFacts {
        let profile = self.profiles.iter().find(|profile| profile.id == drive);
        let place = Place {
            home_drive: &self.home.id,
            session_dir: &self.session_landed,
        };
        tier::landed_facts(
            drive,
            &landed_names(profile, subpath),
            &place,
            &names::same_entry_folded,
        )
    }

    /// Run `run`, the one execution of the call a consumed approval bound
    /// — `call`, exactly as its record holds it — with that approval handed
    /// to it, then take it back, whatever became of the call: no later
    /// call of the turn, even one under the same wire id, runs on it.
    fn run_bound<R>(
        &self,
        call: &chat::ToolCall,
        approval: Ulid,
        released: Option<crate::approvals::Released>,
        run: impl FnOnce() -> R,
    ) -> R {
        *self.bound.lock().unwrap_or_else(|p| p.into_inner()) = Some(Bound {
            call: call.id.clone(),
            tool: call.name.clone(),
            approval,
            released,
        });
        let ran = run();
        *self.bound.lock().unwrap_or_else(|p| p.into_inner()) = None;
        ran
    }

    /// The consumed approval call `id` of `tool` runs on, while it is the
    /// bound call's execution.
    fn bound_approval(&self, id: &str, tool: &str) -> Option<Ulid> {
        self.bound
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .filter(|bound| bound.call == id && bound.tool == tool)
            .map(|bound| bound.approval)
    }

    /// What classified call `id` to `recipients` comes to once its sinks
    /// have passed (R82): the integrity rule's block; else, at T5,
    /// [`FORBIDDEN`] — never an approval's to give; else a call that needs
    /// a person, by the integrity rule or by its tier, runs when a consumed
    /// approval names it, parks when a decision source is installed (R77),
    /// and is refused otherwise, as when nobody can be asked. `pins` are
    /// the files a park relies on.
    fn gated(
        &self,
        id: &str,
        tool: &str,
        classification: &Classification,
        recipients: &[Recipient],
        pins: Vec<(String, String)>,
    ) -> Gated {
        let refusal = match check_call(&self.view.label(), tool, classification.tier, recipients) {
            CallVerdict::Block { reason } => return Gated::Refuse(reason),
            _ if classification.gate() == Gate::Refuse => {
                return Gated::Refuse(FORBIDDEN.to_owned())
            }
            CallVerdict::NeedsApproval => NEEDS_APPROVAL,
            CallVerdict::Allow if classification.gate() == Gate::Run => return Gated::Run(None),
            CallVerdict::Allow => UNATTENDED_REFUSAL,
        };
        if let Some(approval) = self.bound_approval(id, tool) {
            return Gated::Run(Some(Approval::Approved(approval)));
        }
        if !self.parks {
            return Gated::Refuse(refusal.to_owned());
        }
        let approval = Ulid::new();
        *self.parked.lock().unwrap_or_else(|p| p.into_inner()) = Some(Parking {
            approval,
            call_id: id.to_owned(),
            classification: classification.clone(),
            pins,
            declassify: None,
        });
        Gated::Park(approval)
    }

    /// Where a card tool's call lands, as it runs it: `card_update`'s card,
    /// `session_write`'s path, in this session.
    fn card_at(&self, tool: AgentTool, args: &Value) -> String {
        let key = if tool == AgentTool::CardUpdate {
            "card"
        } else {
            "path"
        };
        format!("{}/{}", self.session_dir, args[key].as_str().unwrap_or(""))
    }

    /// A call later in a parked round, by the table's tier for its line:
    /// its facts read as [`ToolHost::run`] and [`ToolHost::run_named`] read
    /// them when it runs.
    fn table_tier(&self, wire: &chat::ToolCall, default_profile_id: &str) -> u8 {
        let Some(tool) = AgentTool::from_wire(&wire.name) else {
            return 0;
        };
        let args = wire.arguments.as_ref().unwrap_or(&Value::Null);
        let facts = match tools::parse_call(default_profile_id, wire) {
            Ok(call) if call.name.effect() == Effect::Write => {
                self.landed_facts(&call.target.profile_id, &call.target.subpath)
            }
            Ok(_) => CallFacts::default(),
            Err(_) if crate::cards::is_card_tool(&wire.name) => {
                let landed = self.landed_facts(&self.home.id, &self.card_at(tool, args));
                tier::named_facts(tool, args, landed)
            }
            Err(_) => tier::named_facts(tool, args, CallFacts::default()),
        };
        let context = Context::of_session(&self.agent, self.view.label().integrity);
        tier::classify(tool, &facts, &context).tier.as_u8()
    }
}

impl crate::sinks::Lift for AllowedTools<'_> {
    /// A flow of call `call` the label blocks (R89). The call a consumed
    /// approval bound never parks again: its flow goes through when that
    /// approval is a `declassify` naming these very bytes and the sink, as
    /// it is now, is within the session's label widened only by the
    /// readers it names (R193); anything else — other bytes, an audience
    /// grown or unknown, an approval of another kind — is drift, and the
    /// call ends refused on that same approval. Any other call, with a
    /// decision source installed and someone who could let it through,
    /// parks on a new `declassify` record at that action's tier, the file
    /// it writes pinned; else it is refused.
    fn lift(
        &self,
        call: &str,
        flow: &keeper_core::agents::label::DeclassifyRequest,
        delegation: Option<&str>,
    ) -> crate::sinks::Lifted {
        let bound = self
            .bound
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .filter(|bound| bound.call == call)
            .map(|bound| (bound.approval, bound.released.clone()));
        if let Some((approval, released)) = bound {
            let moved = match released {
                Some(released) if released.sha256 != flow.effect_sha256 => {
                    crate::approvals::CHANGED
                }
                Some(released) => {
                    let approved = keeper_core::agents::label::approved_label(
                        &self.view.label(),
                        &released.readers,
                    );
                    if check_sink(&approved, &flow.sink) == SinkVerdict::Allow {
                        return crate::sinks::Lifted::Released(approval);
                    }
                    AUDIENCE_MOVED
                }
                None => AUDIENCE_MOVED,
            };
            return crate::sinks::Lifted::Drift(approval, crate::approvals::not_done(moved));
        }
        let Some(mut args) = flow.args().filter(|_| self.parks) else {
            return crate::sinks::Lifted::Refused;
        };
        if let Some(delegation) = delegation {
            args["delegation"] = Value::from(delegation);
        }
        let context = Context::of_session(&self.agent, self.view.label().integrity);
        let classification = tier::classify(AgentTool::Declassify, &CallFacts::default(), &context);
        let pins = match &flow.destination {
            Destination::Drive { drive, path } => vec![(drive.clone(), path.clone())],
            _ => Vec::new(),
        };
        let approval = Ulid::new();
        *self.parked.lock().unwrap_or_else(|p| p.into_inner()) = Some(Parking {
            approval,
            call_id: call.to_owned(),
            classification,
            pins,
            declassify: Some(args),
        });
        crate::sinks::Lifted::Parked(approval)
    }

    fn delegation(&self, call: &str) -> Option<String> {
        self.bound
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .as_ref()
            .filter(|bound| bound.call == call)
            .and_then(|bound| bound.released.as_ref()?.delegation.clone())
    }
}

fn refusal(reason: String) -> ToolOutcome {
    ToolOutcome::Refused { reason }
}

/// A drive write's or edit's canonical bytes for its declassification: the
/// drive and path it lands at with the whole new file, or with the edit's
/// change, so equal text bound for two files is two effects (FR-795).
fn write_effect(call: &ToolCall) -> String {
    json!({
        "drive": call.target.profile_id,
        "path": call.target.subpath,
        "content": call.args.content,
        "old_text": call.args.old_text,
        "new_text": call.args.new_text,
    })
    .to_string()
}

impl ToolHost for AllowedTools<'_> {
    fn run(&self, call: &ToolCall) -> Result<ToolOutcome, BotsError> {
        let name = call.name.as_wire();
        let Some(tool) = AgentTool::from_wire(name) else {
            return Ok(refusal(format!("{name} is not one of this agent's tools.")));
        };
        let drive = call.target.profile_id.as_str();
        let path = call.target.subpath.as_str();
        let effect = call.name.effect();
        let facts = if effect == Effect::Write {
            self.landed_facts(drive, path)
        } else {
            CallFacts::default()
        };
        // The table's tier before the grant answers: what the line and the
        // row say of a call refused before it reaches the drive.
        let table = self.classify(&call.id, tool, &facts, None);
        // An approved call refused here closes the row its park left (R172).
        let approved = self.bound_approval(&call.id, name).map(Approval::Approved);
        let audit = CallAudit::new(
            self.sinks,
            name,
            effect,
            &table,
            Gated::Run(approved),
            (drive, path),
        )
        .lifting(self, &call.id);
        if !self.allow.iter().any(|allowed| allowed == name) {
            return Ok(audit.refuse(format!("{name} is not one of this agent's tools.")));
        }
        if effect == Effect::Write {
            // A drive keeper holds no declaration for is read by anyone:
            // the lower bound, never the higher.
            let drive_readers = self
                .drives
                .get(drive)
                .map_or(Readers::Anyone, |decl| Readers::Only(decl.readers.clone()));
            if let Err(blocked) = self.sinks.verdict(
                name,
                &Destination::Drive {
                    drive: drive.to_owned(),
                    path: path.to_owned(),
                },
                &self.view.label(),
                &Sink::DriveWrite { drive_readers },
                write_effect(call).as_bytes(),
                Some(path),
            ) {
                // Released, it goes on to its grant under the approval.
                if let Err(withheld) = audit.blocked(blocked) {
                    return Ok(withheld.into());
                }
            }
        }
        // Its sinks passed: the drive host writes the row, on the grant's
        // answer; a park relies on the file it writes.
        let pins = if effect == Effect::Write {
            vec![(drive.to_owned(), path.to_owned())]
        } else {
            Vec::new()
        };
        self.inner.run_classified(call, &|verdict| {
            let classification = self.classify(&call.id, tool, &facts, Some(verdict.into()));
            let (refusal, approval) = self
                .gated(&call.id, name, &classification, &[], pins.clone())
                .split();
            Classified {
                classification,
                refusal,
                approval,
            }
        })
    }

    fn run_named(&self, wire: &chat::ToolCall) -> Option<ToolOutcome> {
        let args = wire.arguments.as_ref().unwrap_or(&Value::Null);
        if delegate::is_delegation(&wire.name) {
            let tool = AgentTool::from_wire(&wire.name)?;
            let facts = tier::named_facts(tool, args, CallFacts::default());
            let classification = self.classify(&wire.id, tool, &facts, None);
            let recipient = self.delegation.recipient(wire);
            let recipients: Vec<Recipient> = recipient
                .iter()
                .map(|(name, audience)| Recipient {
                    name,
                    audience: audience.as_ref(),
                })
                .collect();
            // Decided now, said only once the delegation's own sinks have
            // passed (R82).
            let gated = self.gated(
                &wire.id,
                &wire.name,
                &classification,
                &recipients,
                Vec::new(),
            );
            let (drive, at) = self.delegation.destination(wire);
            let audit = CallAudit::new(
                self.sinks,
                &wire.name,
                Effect::Write,
                &classification,
                gated,
                (&drive, &at),
            )
            .lifting(self, &wire.id);
            let outcome = self.delegation.run(wire, &audit);
            if let Some(outcome) = &outcome {
                audit.finish(outcome);
            }
            return outcome;
        }
        if crate::cards::is_card_tool(&wire.name) {
            let tool = AgentTool::from_wire(&wire.name)?;
            let at = self.card_at(tool, args);
            let landed = self.landed_facts(&self.home.id, &at);
            let facts = tier::named_facts(tool, args, landed);
            let classification = self.classify(&wire.id, tool, &facts, None);
            let gated = self.gated(
                &wire.id,
                &wire.name,
                &classification,
                &[],
                vec![(self.home.id.clone(), at.clone())],
            );
            let audit = CallAudit::new(
                self.sinks,
                &wire.name,
                Effect::Write,
                &classification,
                gated,
                (&self.home.id, &at),
            )
            .lifting(self, &wire.id);
            // A tool the agent was not given is refused by the card tools.
            let outcome = if !self.allow.contains(&wire.name) {
                self.cards.run(wire)
            } else {
                let admitted = self
                    .sinks
                    .verdict(
                        &wire.name,
                        &Destination::Drive {
                            drive: self.home.id.clone(),
                            path: at.clone(),
                        },
                        &self.view.label(),
                        &Sink::DriveWrite {
                            drive_readers: Readers::Only(self.home.readers.clone()),
                        },
                        wire.arguments_raw.as_bytes(),
                        None,
                    )
                    .or_else(|blocked| audit.blocked(blocked))
                    .and_then(|()| audit.admit(&self.home.id, &at));
                match admitted {
                    Ok(()) => self.cards.run(wire),
                    Err(withheld) => Some(withheld.into()),
                }
            };
            if let Some(outcome) = &outcome {
                audit.finish(outcome);
            }
            return outcome;
        }
        if !crate::surface::is_surface(&wire.name) {
            return None;
        }
        let tool = AgentTool::from_wire(&wire.name)?;
        let facts = tier::named_facts(tool, args, CallFacts::default());
        let classification = self.classify(&wire.id, tool, &facts, None);
        let gated = self.gated(&wire.id, &wire.name, &classification, &[], Vec::new());
        // A request the label blocks is refused, never parked: each send
        // mints its own id and expiry, so no approval could name its bytes
        // again (DW-521).
        let audit = CallAudit::new(
            self.sinks,
            &wire.name,
            Effect::Read,
            &classification,
            gated,
            (
                args["drive"].as_str().unwrap_or(""),
                args["path"].as_str().unwrap_or(""),
            ),
        );
        let outcome = match &self.surface {
            // The request is checked against the room it goes into, at its
            // send, and its row written then.
            Some(surface) => surface.run(wire, &|verdict, (drive, path)| {
                verdict
                    .or_else(|blocked| audit.blocked(blocked))
                    .and_then(|()| audit.admit(drive, path))
            }),
            None => Some(ToolOutcome::Refused {
                reason: format!("{} is not one of this agent's tools.", wire.name),
            }),
        };
        if let Some(outcome) = &outcome {
            audit.finish(outcome);
        }
        outcome
    }
}

/// What a turn's own tools reach beyond the drive: the person's device,
/// the rooms delegations go through, this session's room, and its sinks.
struct TurnTools {
    surface: Option<Arc<dyn crate::surface::SurfacePort>>,
    delegations: Option<Arc<dyn DelegationPort>>,
    room: Arc<dyn EditPort>,
    from: Delegator,
    sinks: Sinks,
    /// The session room's boundary, which the surface's requests pass too.
    gate: Arc<RoomGate>,
}

/// One served session: its context and its writer.
pub struct ServedSession {
    pub context: SessionContext,
    pub writer: SessionWriter,
    /// Where a `main` session makes the conversations its person asks for
    /// (R36); `None` serves no such request.
    pub conversations: Option<Arc<dyn ConversationPort>>,
    /// Where the session's surface calls go (AD-383); `None`: every one is
    /// `unavailable`.
    pub surface: Option<Arc<dyn crate::surface::SurfacePort>>,
    /// Where this session's delegations go (92.1); `None`: `delegate` is
    /// refused, a joined target is not told and a brief is not taken.
    pub delegations: Option<Arc<dyn DelegationPort>>,
    /// Where the worker says what became of each closed session it was
    /// handed (R61); `None`: nobody asks.
    pub harvests: Option<HarvestAcks>,
    /// The proxies this host runs, through which a narrowed session's
    /// person is told (R169); `None`: no one is.
    pub doors: Option<Arc<dyn ProxyDoors>>,
    /// This session's way in through the host's router, for a decision
    /// made in a proxy's DM (R89); `None`: none is forwarded here.
    pub inbox: Option<crate::deciding::Inbox>,
    /// Work on this session's delegations that failed and is tried again on
    /// the host's clock while the worker runs: briefs a joined target has
    /// not been sent, child rooms whose replies could not be read back, a
    /// person not yet told their work narrowed.
    retry: Retry,
    /// The room's `consumed` events, its claim and its uploads (R75, R86);
    /// `None`: an approval is never consumed here and a run stays parked.
    pub approval_room: Option<Arc<dyn crate::approvals::ApprovalRoom>>,
    /// When each pending approval expires, by id (R84's timer).
    pub(crate) due: BTreeMap<String, chrono::DateTime<chrono::Utc>>,
    /// Arrivals held while a call waits for a person (R74).
    held: Vec<Arrived>,
    /// Whether the status said, since the last hold began, that arrivals
    /// wait.
    held_said: bool,
    /// Each approval whose approve decision is written here and whose
    /// settlement did not finish, tried again on the worker's clock: the
    /// `consumed` event this copy's server took already, if any (R179).
    pub(crate) settling: BTreeMap<String, Option<OwnedEventId>>,
    /// The scheduled card whose run the turn under way is — its own or a
    /// parked run's continuation — so a park records it and the run's end
    /// is written on it (R178).
    pub(crate) scheduled_card: Option<String>,
}

/// What a session's worker tells its host while it holds the claim.
#[derive(Debug, Default)]
pub struct Activity {
    /// Set while the worker does anything — starting, a turn, a parked
    /// run's continuation, an expiry, a settlement: a hand-back waits for
    /// it to clear and no window is begun (R177).
    pub busy: AtomicBool,
    /// Set while a call of the session waits for a person: its scheduled
    /// card is not due, so no window is named (R84, R177). It is idle — a
    /// hand-back may go on.
    pub parked: AtomicBool,
}

impl Activity {
    /// A worker just spawned: busy until it has settled what waited for it.
    pub fn starting() -> Activity {
        Activity {
            busy: AtomicBool::new(true),
            parked: AtomicBool::new(false),
        }
    }
}

/// What a harvest worker made of each closed session it was handed, by id:
/// `true` once it is settled — a turn ran or began, it was one already, or
/// it was refused — and `false` when it failed before it began.
pub type HarvestAcks = Arc<std::sync::Mutex<Vec<(String, bool)>>>;

/// What a worker tries again every [`crate::runtime::TICK`].
#[derive(Debug, Default)]
struct Retry {
    /// Delegations whose joined target's brief could not be sent.
    briefs: std::collections::BTreeSet<String>,
    /// Delegations whose room could not be read back for a reply.
    replies: std::collections::BTreeSet<String>,
    /// The session's person was not told yet that its work narrowed.
    tell: bool,
}

impl Retry {
    fn is_empty(&self) -> bool {
        self.briefs.is_empty() && self.replies.is_empty() && !self.tell
    }
}

/// A boxed room-creating future.
pub type RoomFuture<'a> =
    Pin<Box<dyn Future<Output = Result<OwnedRoomId, AgentMatrixError>> + Send + 'a>>;

/// The agent's own rooms beyond the one it serves: what a `main` session
/// needs to open a proxy conversation (R36).
pub trait ConversationPort: Send + Sync {
    /// A `conversation` session room named `name`, made by the agent, with
    /// `person` invited. The name is room state, which the homeserver
    /// reads: a conversation's title travels in its encrypted status.
    fn create<'a>(&'a self, name: &'a str, person: &'a UserId) -> RoomFuture<'a>;
    /// Send one event into `room`.
    fn send<'a>(&'a self, room: &'a RoomId, event_type: &'a str, content: Value) -> SendFuture<'a>;
    /// Leave a room no session names, the person's invite revoked first.
    fn discard<'a>(
        &'a self,
        room: &'a RoomId,
        person: &'a UserId,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>>;
    /// Whether `person` has joined `room`, as this copy last synced it.
    fn joined<'a>(
        &'a self,
        room: &'a RoomId,
        person: &'a UserId,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>>;
}

/// How long the host watches a new conversation for its person's join, and
/// how often it looks.
pub const JOIN_WATCH: std::time::Duration = std::time::Duration::from_secs(10 * 60);
pub const JOIN_POLL: std::time::Duration = std::time::Duration::from_secs(2);

/// Say a new conversation's status again once its person has joined: the
/// anchor went out with the person only invited, encrypted to the devices
/// known then, so a device of theirs that was not could never read it — and
/// a room whose status is unread is never in that device's dock. Sent after
/// the join, it is shared with every device the person has.
fn restate_once_joined(
    rooms: Arc<dyn ConversationPort>,
    room: OwnedRoomId,
    person: OwnedUserId,
    status: Value,
) {
    tokio::spawn(async move {
        let until = Instant::now() + JOIN_WATCH;
        while Instant::now() < until {
            tokio::time::sleep(JOIN_POLL).await;
            if rooms.joined(&room, &person).await {
                if let Err(error) = rooms.send(&room, STATUS, status).await {
                    tracing::warn!(%room, %error, "agents: a new conversation's status could not be said again");
                }
                return;
            }
        }
    });
}

/// What the DM is told when a conversation could not be opened; why is in
/// the host's log.
pub const CONVERSATION_FAILED: &str =
    "I could not open a new conversation. Ask me again in a moment.";

/// A scope request keeper cannot read.
pub const UNREADABLE_SCOPE_REQUEST: &str = "a scope this host cannot read is not applied";
/// A request for a conversation keeper cannot read, or one this host cannot
/// serve here.
pub const UNSERVED_CONVERSATION: &str = "a request for a conversation this host cannot serve here";

/// An event that arrived in a served session's room.
#[derive(Debug, Clone)]
pub struct Arrived {
    pub event_id: OwnedEventId,
    pub sender: OwnedUserId,
    pub arrival: Arrival,
    /// The message's `body`, for text.
    pub text: String,
    /// The event's content, for a decision.
    pub content: Value,
    /// When this host received it.
    pub received_at: Instant,
    /// Read back from the room when the worker started, not delivered live:
    /// an event served before may not have left a line (a focus, an
    /// unchanged scope), so it is served again on every start.
    pub replay: bool,
    /// For an arrival the host routed here from another room — a target's
    /// join or reply in a room this session delegated into — that room.
    pub via: Option<OwnedRoomId>,
    /// The device that sent it, as its encryption names it: whose trust a
    /// decision is judged on (93.3).
    pub device: Option<matrix_sdk::ruma::OwnedDeviceId>,
}

/// A brief whose delegation is not this session's.
pub const NOT_THIS_DELEGATION: &str = "a brief for another delegation is not a turn here";
/// A join or a reply no delegation of this session waits for.
pub const NOT_A_DELEGATION: &str = "no delegation of this session waits for this";
/// A brief that arrived after the exchange's last round, before a reply.
pub const ROUNDS_SPENT: &str =
    "this exchange has had its rounds; a brief is not a turn here until this session replies";
/// A brief that arrived after the session spent its token budget.
pub const BUDGET_SPENT: &str = "this delegation spent its token budget; a brief is not a turn here";
/// A brief in a room this host could not read the state of.
pub const ROOM_UNREAD: &str = "the delegated room could not be read, so the brief is not taken";
/// A target's join whose brief could not be sent: sent again on the clock.
pub const BRIEF_UNSENT: &str = "the brief could not be sent; it is sent again on the host's clock";
/// A target's join whose brief the label no longer lets into the room.
pub const BRIEF_REFUSED: &str = "the brief was not sent: the label no longer lets it into the room";

/// `event` (decrypted) as the reply of `to` in the room `room` a session
/// delegated into: an `m.room.message` from `to`, not an edit, carrying
/// `dev.keeper.agent.artifacts` and the replying session's label (R55, R94).
/// A reply without a label this build reads is not taken.
pub fn reply_of(
    event: &Value,
    to: &UserId,
    room: &RoomId,
    received_at: Instant,
) -> Option<Arrived> {
    let content = &event["content"];
    let reply = event["type"] == "m.room.message"
        && event["sender"].as_str() == Some(to.as_str())
        && content["m.relates_to"]["rel_type"] != "m.replace"
        && content[ARTIFACTS].is_array()
        && delegate::reply_label(content).is_some();
    if !reply {
        return None;
    }
    Some(Arrived {
        event_id: OwnedEventId::try_from(event["event_id"].as_str()?).ok()?,
        sender: to.to_owned(),
        arrival: Arrival::Replied,
        text: content["body"].as_str().unwrap_or_default().to_owned(),
        content: content.clone(),
        received_at,
        replay: false,
        via: Some(room.to_owned()),
        device: None,
    })
}

/// A scheduled arrival whose content is not one this build reads.
pub const NOT_SCHEDULED: &str = "the host's clock sent nothing this host reads";
/// A scheduled card that could not be read or written.
pub const CARD_UNWRITTEN: &str =
    "the scheduled card could not be read or written; it is tried at its next window";

/// The arrival the host's clock routes to a scheduled session's worker
/// (92.3), in the agent's own name. A window's run has an id derived from
/// its window — however often it is routed, the session's dedupe keeps it
/// to one turn; a settlement or a wait is a fresh event each time, routed
/// again until the card reads settled (R163). Never a room's event.
pub fn scheduled_arrival(agent: &UserId, scheduled: &Scheduled) -> Option<Arrived> {
    let fresh = || Ulid::new().to_string().to_ascii_lowercase();
    let key = match scheduled {
        Scheduled::Run { window, .. } => format!(
            "run-{}",
            DateTime::parse_from_rfc3339(window)
                .map(|at| at.timestamp_millis().to_string())
                .unwrap_or_else(|_| fresh())
        ),
        Scheduled::TakenOver { .. } => format!("taken-{}", fresh()),
        Scheduled::Wait { .. } => format!("wait-{}", fresh()),
    };
    Some(Arrived {
        event_id: OwnedEventId::try_from(format!("$scheduled-{key}:keeper.invalid")).ok()?,
        sender: agent.to_owned(),
        arrival: Arrival::Scheduled,
        text: String::new(),
        content: serde_json::to_value(scheduled).ok()?,
        received_at: Instant::now(),
        replay: false,
        via: None,
        device: None,
    })
}

/// A harvest arrival whose content is not a closed session.
pub const NOT_A_HARVEST: &str = "the host sent no closed session this host reads";
/// A harvest arrival outside the agent's harvest session, or for an agent
/// whose menu has no `HV` prompt.
pub const NO_HARVEST: &str = "only a steward's harvest session with an HV prompt harvests";
/// A closed session the harvest session's label does not let in (R166).
pub const HARVEST_REFUSED: &str = "a closed session's label keeps it out of this harvest";
/// The tool a harvest's audit row names when a closed session's label keeps
/// it out of the harvest room (R65, R166).
pub const HARVEST: &str = "harvest";

/// The opaque part every harvest arrival's id starts with.
const HARVEST_EVENT: &str = "$harvest-";

/// The question a harvest's anchor names in a room its label no longer
/// reaches: no closed session's id, and no person's message an answer of
/// another copy would take for answered.
fn unnamed_harvest() -> &'static EventId {
    matrix_sdk::ruma::event_id!("$harvest:keeper.invalid")
}

/// Whether `event` is a harvest arrival's id.
fn is_harvest_event(event: &str) -> bool {
    event.starts_with(HARVEST_EVENT)
}

/// The arrival a steward's harvest session's holder routes for `closed`
/// (R61), in the agent's own name. Its id is the closed session's id,
/// hex-encoded so any id is an event id's opaque part: routed again, after a
/// restart or an index rebuild, it is the event the session logged already.
pub fn harvest_arrival(agent: &UserId, closed: &crate::stewards::Closed) -> Option<Arrived> {
    let key: String = closed.id.bytes().map(|b| format!("{b:02x}")).collect();
    Some(Arrived {
        event_id: OwnedEventId::try_from(format!("{HARVEST_EVENT}{key}:keeper.invalid")).ok()?,
        sender: agent.to_owned(),
        arrival: Arrival::Harvest,
        text: String::new(),
        content: serde_json::to_value(closed).ok()?,
        received_at: Instant::now(),
        replay: false,
        via: None,
        device: None,
    })
}

/// What became of an arrival.
#[derive(Debug)]
pub enum Outcome {
    /// Already logged: nothing done.
    Duplicate,
    /// Neither a turn nor a decision; nothing logged.
    Ignored(&'static str),
    /// A reader's decision, logged.
    Decided,
    /// A turn ran.
    Answered(TurnReport),
    /// The person's scope: the drives now in scope, a `scope` line written
    /// when they changed, and the focus taken.
    Scoped(Vec<String>),
    /// The person asked for drives the agent may not use: refused, named.
    ScopeRefused(String),
    /// A focus alone: held for the next turn, nothing logged.
    Focused,
    /// A conversation session at this path: made now, or made before for
    /// the same request.
    Conversation { path: String, made: bool },
    /// The brief of the delegation with this id went in, its target joined.
    BriefSent(String),
    /// The scheduled card says this `run` line's state now; no turn ran.
    Scheduled(keeper_core::agents::log::RunState),
    /// A call of this session waits for a person: the arrival is held,
    /// and served once the approval ends (R74).
    Held,
    /// A decision on a request another session sent into this DM, handed
    /// to that session (R89).
    Forwarded,
}

/// How a turn ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TurnEnding {
    /// The model finished.
    Complete,
    /// Shutdown stopped it; the answer so far was kept.
    Stopped,
    /// The label stopped the model (S-04).
    LocalOnly,
    /// A delegated session reached its token budget (92.1).
    Bounded,
    /// A call waits for a person's decision: the turn holds nothing and
    /// goes on when it is decided (93.2).
    Parked,
    /// The model or the log failed.
    Failed,
}

/// What one turn did, for the host's log and the NFR-113 harness.
#[derive(Debug, Clone)]
pub struct TurnReport {
    pub user_line: Ulid,
    pub anchor: OwnedEventId,
    /// When the request reached this host.
    pub received_at: Instant,
    /// When the homeserver accepted the anchor.
    pub anchor_at: Instant,
    /// When the model's stream ended.
    pub stream_end: Instant,
    /// When the homeserver accepted the final edit.
    pub final_at: Instant,
    pub edits: usize,
    pub ending: TurnEnding,
    /// The digest of the system message the turn was sent, when it was sent one.
    pub prompt_sha256: Option<String>,
    /// The whole answer, as the log holds it.
    pub answer: String,
}

/// Why a turn could not be served at all.
#[derive(Debug, thiserror::Error)]
pub enum ServeError {
    #[error(transparent)]
    Writer(#[from] WriterError),
    /// A reply's turn opens on its logged receipt, and there was none.
    #[error("a delegation's reply has no logged receipt to open its turn")]
    NoReceipt,
    /// A harvest's turn opens on the closed session it carries.
    #[error("a harvest arrival carries no closed session")]
    NotAHarvest,
    /// A harvest's room's members could not be read: it is handed again.
    #[error("the harvest room's members could not be read")]
    MembersUnread,
}

/// `<drive>/<path>` of each file a reply's content hands over.
fn handed_over(content: &Value) -> Vec<String> {
    content[ARTIFACTS]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|file| {
            Some(format!(
                "{}/{}",
                file["drive"].as_str()?,
                file["path"].as_str()?
            ))
        })
        .collect()
}

/// What a room shows of this session's last turn, read from its timeline
/// after a restart: the anchor of the unanswered turn, and the session's
/// status anchor with whether it was left `running`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Trail {
    pub anchor: Option<OwnedEventId>,
    pub status: Option<(OwnedEventId, bool)>,
}

/// The [`Trail`] in `events` (decrypted, oldest first): `agent`'s anchor for
/// `user_line` and its latest status.
pub fn trail_of(events: &[Value], agent: &UserId, user_line: Option<Ulid>) -> Trail {
    let mut trail = Trail::default();
    let line = user_line.map(|line| line.to_string());
    for event in events {
        if event["sender"].as_str() != Some(agent.as_str()) {
            continue;
        }
        let Some(id) = event["event_id"]
            .as_str()
            .and_then(|id| OwnedEventId::try_from(id).ok())
        else {
            continue;
        };
        let content = &event["content"];
        if line.is_some() && content[TURN]["line"].as_str() == line.as_deref() {
            trail.anchor = Some(id);
        } else if event["type"] == STATUS {
            let anchor = content["anchor"]
                .as_str()
                .and_then(|anchor| OwnedEventId::try_from(anchor).ok())
                .unwrap_or(id);
            trail.status = Some((anchor, content["run"] == "running"));
        }
    }
    trail
}

/// Run blocking work (an `fsync`, a zone write) where it does not stall the
/// runtime's other tasks: in place on a multi-thread runtime's worker.
pub(crate) fn off_the_runtime<T>(work: impl FnOnce() -> T) -> T {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) if handle.runtime_flavor() == tokio::runtime::RuntimeFlavor::MultiThread => {
            tokio::task::block_in_place(work)
        }
        _ => work(),
    }
}

impl ServedSession {
    /// Open a served session under `lease` (none before claims): its
    /// writer, then its context.
    pub fn open(
        deps: &AgentDeps,
        dir: &Path,
        session: SessionRef,
        agent: SessionAgent,
        lease: Option<Arc<Lease>>,
    ) -> Result<ServedSession, ServeOpenError> {
        let (epoch, claim) = lease.as_ref().map_or((0, None), |lease| {
            (lease.epoch, Some(lease.claim_event.to_string()))
        });
        let writer = SessionWriter::open(
            &deps.sessions_zone,
            &session.path,
            &agent,
            &deps.host,
            deps.lfs_threshold_bytes,
            lease,
        )?;
        let context = SessionContext::load(
            &deps.home,
            dir,
            session,
            agent,
            epoch,
            claim,
            chrono::Local::now().fixed_offset(),
        )?;
        Ok(ServedSession {
            context,
            writer,
            conversations: None,
            surface: None,
            delegations: None,
            harvests: None,
            doors: None,
            inbox: None,
            retry: Retry::default(),
            approval_room: None,
            due: BTreeMap::new(),
            held: Vec::new(),
            held_said: false,
            settling: BTreeMap::new(),
            scheduled_card: None,
        })
    }

    /// After a restart: a turn whose `user` line has no answer is not run
    /// again (C6). An `error` line closes it and the room is told — by an
    /// edit of the turn's anchor when `trail` found it, else by a message —
    /// and a status left `running` is set `idle`; `true` when there was one.
    /// A delegation's reply whose receipt was logged but not its `peer` line
    /// gets that line first, from the receipt, and is closed the same way.
    pub async fn recover(
        &mut self,
        deps: &AgentDeps,
        port: Arc<dyn EditPort>,
        trail: &Trail,
    ) -> Result<bool, ServeError> {
        if let Some((status, _)) = &trail.status {
            self.context.status_anchor = Some(status.clone());
        }
        self.peer_the_reply()?;
        // A record whose `approval requested` line a stop lost still waits
        // (R176): it is announced again, never cut off.
        self.adopt_records(deps);
        // A turn parked on an approval waits; it was not cut off (R74).
        if !self.context.parked.is_empty() {
            return Ok(false);
        }
        let Some(user_line) = self.context.unanswered else {
            return Ok(false);
        };
        let sentence = cut_off_sentence(deps.host.as_str());
        self.writer.write(
            &mut self.context,
            Some(user_line),
            None,
            LineBody::Error(ErrorBody {
                sentence: sentence.clone(),
                code: "interrupted".to_owned(),
            }),
        )?;
        off_the_runtime(|| self.writer.sync())?;
        let told = match &trail.anchor {
            Some(anchor) => edit_content(anchor, &sentence),
            None => notice_content(&sentence),
        };
        deliver(port.as_ref(), "m.room.message", told, Instant::now()).await;
        if let Some((status, true)) = &trail.status {
            self.context.status_anchor = Some(status.clone());
            self.send_status(deps, &port, RunState::Idle, None).await;
        }
        Ok(true)
    }

    /// The status every edit of this session's status anchor carries; a
    /// send under a narrowed room makes its title the fixed sentence and
    /// drops its detail ([`status_content`], R64).
    fn status_base(&self, deps: &AgentDeps) -> StatusContent {
        StatusContent {
            v: CONTENT_VERSION,
            session: format!("{}/{}", deps.sessions_subfolder, self.context.session.path),
            kind: self.context.agent.kind,
            title: self.context.agent.title.clone(),
            agent: deps.home.config.matrix_user.clone(),
            host: deps.host.as_str().to_owned(),
            epoch: self.context.epoch,
            run: RunState::Running,
            detail: None,
            waiting: None,
            anchor: None,
        }
    }

    /// This session's sinks: its audit ids, and what routes a declassification.
    pub(crate) fn sinks(&self, deps: &AgentDeps) -> Sinks {
        Sinks {
            data_dir: deps.data_dir.clone(),
            provider_id: deps.row.provider.id.clone(),
            bot_id: deps.bot.id.clone(),
            session_id: self.context.agent.id.to_string(),
            known: self.delegations.as_ref().map(|rooms| rooms.known()),
            home_drive: deps.home.config.drive.clone(),
            zone: deps.sessions_zone.clone(),
            doors: self.doors.clone(),
        }
    }

    /// Serve `backlog`, then every arrival until the channel closes or `stop`
    /// fires. A stop is checked before each arrival, so a queued arrival is
    /// never started on shutdown: it stays unlogged, and the next start reads
    /// it from the room's timeline. While a delegation's brief or reply
    /// read failed, it is tried again every [`crate::runtime::TICK`]; so is
    /// an approval's settlement that did not finish (R179). `activity` is
    /// busy through all work — what waited at start, every arrival, every
    /// expiry and settlement — and says, before busy clears, whether a call
    /// waits for a person (R177).
    pub async fn serve_arrivals(
        &mut self,
        deps: &AgentDeps,
        port: Arc<dyn EditPort>,
        backlog: Vec<Arrived>,
        arrivals: &mut mpsc::UnboundedReceiver<Arrived>,
        mut stop: CancelSignal,
        activity: &Activity,
    ) {
        let mut backlog: std::collections::VecDeque<Arrived> = backlog.into();
        activity.busy.store(true, Ordering::SeqCst);
        // A person not told before a restart is told now.
        self.tell_the_requester(deps).await;
        // What waited for a person while no worker served it is settled
        // first: an approval decided or expired meanwhile (93.2 AC5, R84).
        self.resume_approvals(deps, Arc::clone(&port), stop.clone())
            .await;
        // What still waits, with its request in a proxy's DM, is heard
        // there again (R89).
        self.expect_decisions(deps);
        self.idle(activity);
        loop {
            if stop.is_cancelled() {
                return;
            }
            // Once nothing waits, what was held meanwhile is served (R74).
            if !self.waiting() && !self.held.is_empty() {
                backlog.extend(self.held.drain(..));
                self.held_said = false;
            }
            let expiry = self.next_expiry().map(|at| {
                (at - chrono::Utc::now())
                    .to_std()
                    .unwrap_or(std::time::Duration::ZERO)
            });
            let arrived = match backlog.pop_front() {
                Some(arrived) => arrived,
                None => tokio::select! {
                    biased;
                    () = stop.cancelled() => return,
                    () = tokio::time::sleep(expiry.unwrap_or_default()), if expiry.is_some() => {
                        activity.busy.store(true, Ordering::SeqCst);
                        self.expire_due(deps, &port, stop.clone()).await;
                        self.idle(activity);
                        continue;
                    }
                    arrived = arrivals.recv() => match arrived {
                        Some(arrived) => arrived,
                        None => return,
                    },
                    () = tokio::time::sleep(crate::runtime::TICK), if self.settling() => {
                        activity.busy.store(true, Ordering::SeqCst);
                        self.retry_settlements(deps, &port, stop.clone()).await;
                        self.idle(activity);
                        continue;
                    }
                    () = tokio::time::sleep(crate::runtime::TICK), if !self.retry.is_empty() => {
                        backlog.extend(self.retry_delegations(deps).await);
                        continue;
                    }
                },
            };
            let session = self.context.session.path.clone();
            let harvest = (arrived.arrival == Arrival::Harvest).then(|| {
                let id = arrived.content["id"]
                    .as_str()
                    .unwrap_or_default()
                    .to_owned();
                (id, arrived.event_id.clone())
            });
            activity.busy.store(true, Ordering::SeqCst);
            let outcome = self
                .serve(deps, Arc::clone(&port), arrived, stop.clone())
                .await;
            self.idle(activity);
            // The host hands a harvest again only when it failed before it
            // began: one that began and then failed is the interrupted
            // turn's, never run twice.
            if let (Some((id, event)), Some(acks)) = (harvest, &self.harvests) {
                let done = outcome.is_ok() || self.context.harvest_began(&event);
                acks.lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push((id, done));
            }
            match outcome {
                Ok(Outcome::Answered(report)) => tracing::info!(
                    %session,
                    anchor_ms = report.anchor_at.duration_since(report.received_at).as_millis() as u64,
                    final_ms = report.final_at.duration_since(report.stream_end).as_millis() as u64,
                    edits = report.edits,
                    ending = ?report.ending,
                    "agentd: a turn was answered"
                ),
                Ok(outcome) => tracing::debug!(%session, ?outcome, "agentd: an event"),
                Err(error) => {
                    tracing::error!(%session, %error, "agentd: a turn could not be served")
                }
            }
        }
    }

    /// The work is done: say first whether a call waits for a person, then
    /// clear busy — the host never sees an idle worker it may begin a
    /// window beside while a run is parked (R177).
    fn idle(&self, activity: &Activity) {
        activity.parked.store(self.waiting(), Ordering::SeqCst);
        activity.busy.store(false, Ordering::SeqCst);
    }

    /// Serve one arrival: ignore it, log a decision, or run a turn.
    pub async fn serve(
        &mut self,
        deps: &AgentDeps,
        port: Arc<dyn EditPort>,
        arrived: Arrived,
        stop: CancelSignal,
    ) -> Result<Outcome, ServeError> {
        if self.writer.seen(&arrived.event_id)? {
            return Ok(Outcome::Duplicate);
        }
        let config = &deps.home.config;
        let disposition = rooms::classify(
            &Served {
                agent_kind: config.kind,
                human: config.human.as_deref(),
                session_kind: self.context.agent.kind,
                agent_user: &config.matrix_user,
                requester: &self.context.agent.requested_by,
                readers: &self.context.label.readers,
            },
            &arrived.sender,
            arrived.arrival,
        );
        match disposition {
            Disposition::Ignored(note) => {
                tracing::info!(session = %self.context.session.path, sender = %arrived.sender, note, "agents: an observer's event is not a turn");
                Ok(Outcome::Ignored(note))
            }
            // A decision on a request another session sent into this DM
            // goes home to that session (R89).
            Disposition::Decision if self.forward(&arrived) => Ok(Outcome::Forwarded),
            Disposition::Decision if deps.decisions.is_some() => {
                self.decided(deps, port, arrived, stop).await
            }
            // Nothing parks without a source, so nothing waits for this: it
            // is logged ignored, with why (R80).
            Disposition::Decision => {
                let id = arrived.content["id"].as_str().map(str::to_owned);
                self.writer.write(
                    &mut self.context,
                    None,
                    Some(arrived.event_id.clone()),
                    LineBody::Approval(ApprovalBody {
                        id: id.unwrap_or_else(|| arrived.event_id.to_string()),
                        state: ApprovalState::Decided,
                        decision: None,
                        by: Some(arrived.sender.to_string()),
                        result: None,
                        reason: Some(crate::approvals::NO_SUCH_APPROVAL.to_owned()),
                        scope: None,
                    }),
                )?;
                off_the_runtime(|| self.writer.sync())?;
                Ok(Outcome::Decided)
            }
            // While a call waits for a person (R74): in the person's own
            // `main` or `conversation` session their new message denies it
            // and then runs; anywhere else what would start a turn is held
            // until the approval ends, and the status says once what waits.
            _ if self.waiting() => {
                let persons_word = matches!(
                    self.context.agent.kind,
                    SessionKind::Main | SessionKind::Conversation
                ) && disposition == Disposition::Turn
                    && arrived.arrival == Arrival::Text;
                if persons_word {
                    self.supersede(deps, &arrived.sender);
                    return self
                        .turn(deps, port, arrived, stop)
                        .await
                        .map(Outcome::Answered);
                }
                if !self.held_said {
                    self.held_said = true;
                    let detail = format!(
                        "{} (new messages wait until it is decided)",
                        crate::approvals::WAITING_FOR.trim_end_matches(':')
                    );
                    self.send_status(deps, &port, RunState::Blocked, Some(&detail))
                        .await;
                }
                self.held.push(arrived);
                Ok(Outcome::Held)
            }
            Disposition::Turn if arrived.arrival == Arrival::Brief => {
                self.take_brief(deps, port, arrived, stop).await
            }
            Disposition::Turn => self
                .turn(deps, port, arrived, stop)
                .await
                .map(Outcome::Answered),
            Disposition::Scope => self.scope(deps, &port, arrived).await,
            Disposition::NewConversation => self.new_conversation(deps, &port, arrived).await,
            Disposition::Delegation => self.delegation_moved(deps, port, arrived, stop).await,
            Disposition::Scheduled => self.scheduled(deps, port, arrived, stop).await,
            Disposition::Harvest => self.harvest(deps, port, arrived, stop).await,
        }
    }

    /// A closed session of the drive wakes the steward's harvest session
    /// (R61): one turn whose brief is her `HV` prompt naming the closed
    /// session, opened by its label's join and a `peer` line in her own name
    /// carrying the arrival's id — the closed session's — so it is never a
    /// second turn, here or after another copy began it. A closed session
    /// whose readers do not reach the harvest room's, that may go only to a
    /// model of its readers' while hers is not, or whose label, joined, does
    /// not reach the room as it is now — its members, a known agent through
    /// its audience — is refused before anything of it is logged or sent
    /// (R166, R168), and the refusal audited (R65). A room whose members
    /// cannot be read refuses it too, and the host hands it again.
    async fn harvest(
        &mut self,
        deps: &AgentDeps,
        port: Arc<dyn EditPort>,
        mut arrived: Arrived,
        stop: CancelSignal,
    ) -> Result<Outcome, ServeError> {
        let Ok(closed) = serde_json::from_value::<crate::stewards::Closed>(arrived.content.clone())
        else {
            return Ok(Outcome::Ignored(NOT_A_HARVEST));
        };
        if !crate::stewards::is_harvest(&self.context.agent) {
            return Ok(Outcome::Ignored(NO_HARVEST));
        }
        if self.context.harvest_began(&arrived.event_id) {
            return Ok(Outcome::Duplicate);
        }
        let room = self.context.agent.room.clone();
        if let Some(refusal) = crate::stewards::refusal(
            &closed,
            &self.context.agent.label.readers,
            deps.model_is_local(),
        ) {
            let reason = refusal.reason();
            tracing::warn!(session = %self.context.session.path, closed = %closed.id, reason, "agents: a closed session is not harvested");
            let sinks = self.sinks(deps);
            match refusal {
                crate::stewards::Refusal::Readers => {
                    sinks.refused(HARVEST, "", room.as_str(), reason)
                }
                crate::stewards::Refusal::Model => {
                    sinks.refused("model", "", &deps.bot.target, reason)
                }
            }
            return Ok(Outcome::Ignored(HARVEST_REFUSED));
        }
        // The turn's anchor names the arrival: the room as it is now hears
        // of it only when the joined label reaches it.
        let gate = self.gate(deps, &port);
        gate.set_label(self.context.label.join(&closed.label));
        if let Err(reason) = gate
            .admit(HARVEST, arrived.event_id.as_str().as_bytes())
            .await
        {
            tracing::warn!(session = %self.context.session.path, closed = %closed.id, %reason, "agents: a closed session is not harvested into this room");
            if reason == MEMBERS_UNREAD {
                return Err(ServeError::MembersUnread);
            }
            return Ok(Outcome::Ignored(HARVEST_REFUSED));
        }
        let Some(brief) =
            crate::stewards::harvest_brief(&deps.home.config, &deps.sessions_subfolder, &closed)
        else {
            return Ok(Outcome::Ignored(NO_HARVEST));
        };
        arrived.text = brief;
        self.turn(deps, port, arrived, stop)
            .await
            .map(Outcome::Answered)
    }

    /// The host's clock asked about the session's scheduled card (92.3).
    /// [`cards::begin`] reads the card again under the claim and writes its
    /// keys; a run is a turn whose brief is the card's body, opened by a
    /// `run: running` line and a `peer` line in the agent's own name, and the
    /// card then says how the turn ended: `review`, `failed` or `blocked`.
    /// An action in it that needs a person is refused as in every agent
    /// turn before Epic 93 (`UNATTENDED_REFUSAL`).
    async fn scheduled(
        &mut self,
        deps: &AgentDeps,
        port: Arc<dyn EditPort>,
        mut arrived: Arrived,
        stop: CancelSignal,
    ) -> Result<Outcome, ServeError> {
        use keeper_core::agents::log::RunState as LogRun;
        let Ok(scheduled) = serde_json::from_value::<Scheduled>(arrived.content.clone()) else {
            return Ok(Outcome::Ignored(NOT_SCHEDULED));
        };
        let session = self.context.session.path.clone();
        let lease = self.writer.lease();
        // A window begins only while the claim names it (R56): a run routed
        // before the host named a later window never begins.
        let names = match &scheduled {
            Scheduled::Run { window, .. } => Some(window.clone()),
            _ => None,
        };
        let may_begin = move || {
            lease.as_ref().is_none_or(|lease| {
                lease.may_write()
                    && names
                        .as_ref()
                        .is_none_or(|w| lease.window().as_ref() == Some(w))
            })
        };
        let zone = deps.sessions_zone.clone();
        let holder = cards::Holder {
            agent: &self.context.agent,
            host: deps.host.as_str(),
        };
        let begun = match off_the_runtime(|| {
            cards::begin(&zone, &session, &holder, &scheduled, &may_begin)
        }) {
            Ok(begun) => begun,
            Err(error) => {
                tracing::warn!(%session, %error, "agents: a scheduled card could not be read or written");
                return Ok(Outcome::Ignored(CARD_UNWRITTEN));
            }
        };
        let (brief, untrusted) = match begun {
            Begun::Nothing(note) => {
                tracing::info!(%session, card = scheduled.card(), note, "agents: the scheduled card is left as it is");
                return Ok(Outcome::Ignored(note));
            }
            Begun::Said(body) => {
                let state = body.state;
                self.writer.write(
                    &mut self.context,
                    None,
                    Some(arrived.event_id.clone()),
                    LineBody::Run(body),
                )?;
                off_the_runtime(|| self.writer.sync())?;
                return Ok(Outcome::Scheduled(state));
            }
            Begun::Run { brief, untrusted } => (brief, untrusted),
        };
        self.writer.write(
            &mut self.context,
            None,
            None,
            LineBody::Run(RunBody {
                state: LogRun::Running,
                detail: None,
            }),
        )?;
        // The card is read as the drive read of it would be (S-02): one
        // made from outside content lowers the session's integrity.
        let joined = if untrusted {
            self.context.label.join(&Label {
                integrity: Integrity::Untrusted,
                ..Label::top()
            })
        } else {
            self.context.label.clone()
        };
        if joined != self.context.label {
            self.writer.write(
                &mut self.context,
                None,
                None,
                LineBody::Label(LabelBody::new(
                    &joined,
                    LabelCause {
                        kind: LabelCauseKind::DriveRead,
                        reference: scheduled.card().to_owned(),
                    },
                )),
            )?;
        }
        arrived.text = brief;
        let card = scheduled.card().to_owned();
        self.scheduled_card = Some(card.clone());
        let ran = self.turn(deps, port, arrived, stop).await;
        self.scheduled_card = None;
        self.finish_scheduled(deps, &card, ran.as_ref().ok().map(|report| report.ending))?;
        ran.map(Outcome::Answered)
    }

    /// End the run of the scheduled card `card` as its turn — the run's own
    /// or a parked run's continuation — ended (`None`: it failed), on the
    /// card and as a `run` line: `review`, `failed`, or `blocked` — a run
    /// parked on a person reads `blocked` until its approval ends (93.2 AC1,
    /// R84, R178); one that could not wait for anyone failed.
    pub(crate) fn finish_scheduled(
        &mut self,
        deps: &AgentDeps,
        card: &str,
        ending: Option<TurnEnding>,
    ) -> Result<(), ServeError> {
        use keeper_core::agents::log::RunState as LogRun;
        let (run, state) = match ending {
            Some(TurnEnding::Complete) => (Run::Review, LogRun::Review),
            Some(TurnEnding::Parked) if self.waiting() => (Run::Blocked, LogRun::Blocked),
            Some(TurnEnding::Stopped | TurnEnding::LocalOnly | TurnEnding::Bounded) => {
                (Run::Blocked, LogRun::Blocked)
            }
            Some(TurnEnding::Parked | TurnEnding::Failed) | None => (Run::Failed, LogRun::Failed),
        };
        let (zone, session) = (
            deps.sessions_zone.clone(),
            self.context.session.path.clone(),
        );
        let lease = self.writer.lease();
        let may_write = move || lease.as_ref().is_none_or(|lease| lease.may_write());
        match off_the_runtime(|| cards::write_run(&zone, &session, card, run, None, &may_write)) {
            Ok(_) => {
                self.writer.write(
                    &mut self.context,
                    None,
                    None,
                    LineBody::Run(RunBody {
                        state,
                        detail: None,
                    }),
                )?;
                off_the_runtime(|| self.writer.sync())?;
            }
            Err(error) => {
                tracing::warn!(%session, %card, %error, "agents: the scheduled card's end could not be written")
            }
        }
        Ok(())
    }

    /// The session as its `delegate` and `reply` tools and its briefs name it.
    fn delegator(&self, deps: &AgentDeps) -> Delegator {
        let config = &deps.home.config;
        Delegator {
            user: config.matrix_user.clone(),
            drive: config.drive.clone(),
            id: self.context.agent.id.to_string(),
            session: self.context.session.path.clone(),
            room: self.context.agent.room.clone(),
            kind: self.context.agent.kind,
            requester: self.context.agent.requested_by.clone(),
            hop: self.context.agent.hop,
            limits: config.limits,
            zone: deps.sessions_zone.clone(),
            subfolder: deps.sessions_subfolder.clone(),
            chain: keeper_core::agents::delegation::session_chain(
                &self.context.agent,
                &config.matrix_user,
            ),
        }
    }

    /// A brief from the agent that delegated this session: a turn only when
    /// it passes the admission the live intake and the read-back after a
    /// restart use (R93), is this session's own delegation from its parent's
    /// room, and arrives while its exchange is open (Q12).
    async fn take_brief(
        &mut self,
        deps: &AgentDeps,
        port: Arc<dyn EditPort>,
        arrived: Arrived,
        stop: CancelSignal,
    ) -> Result<Outcome, ServeError> {
        let session = self.context.session.path.clone();
        let Some(delegations) = self.delegations.clone() else {
            return Ok(Outcome::Ignored(ROOM_UNREAD));
        };
        let Some(room) = delegations.brief_room(&self.context.agent.room).await else {
            tracing::warn!(%session, "agents: the delegated room could not be read for a brief");
            return Ok(Outcome::Ignored(ROOM_UNREAD));
        };
        // `arrival_of` makes a brief only of an `m.room.message` its sender's
        // device sealed; the content says the rest.
        let event = BriefEvent {
            event_type: "m.room.message",
            sender: &arrived.sender,
            content: &arrived.content,
            sealed: true,
        };
        let brief = match rooms::admit_brief(
            &room,
            &event,
            &deps.home.config.matrix_user,
            &delegations.known(),
        ) {
            Ok(brief) => brief,
            Err(note) => {
                tracing::info!(%session, sender = %arrived.sender, note, "agents: a brief is not taken");
                return Ok(Outcome::Ignored(note));
            }
        };
        let parent = self
            .context
            .agent
            .parent
            .as_ref()
            .map(|parent| &parent.room);
        if brief.id != self.context.agent.id.to_string() || parent != Some(&brief.from.room) {
            return Ok(Outcome::Ignored(NOT_THIS_DELEGATION));
        }
        if let Some(note) = self.context.exchange_closed() {
            tracing::info!(%session, note, "agents: a brief is not taken");
            return Ok(Outcome::Ignored(note));
        }
        self.turn(deps, port, arrived, stop)
            .await
            .map(Outcome::Answered)
    }

    /// A delegation this session made moved (R55): its target joined — the
    /// brief goes in now — or replied, which is logged with what it said and
    /// answered by a turn of this session's agent. Anyone else's event from
    /// that room, and one for a delegation in another state, changes nothing.
    async fn delegation_moved(
        &mut self,
        deps: &AgentDeps,
        port: Arc<dyn EditPort>,
        arrived: Arrived,
        stop: CancelSignal,
    ) -> Result<Outcome, ServeError> {
        let Some(open) = arrived
            .via
            .as_deref()
            .and_then(|room| self.context.delegation_in(room))
            .filter(|open| open.to == arrived.sender)
            .cloned()
        else {
            return Ok(Outcome::Ignored(NOT_A_DELEGATION));
        };
        match arrived.arrival {
            Arrival::Joined if !open.sent => {
                self.send_brief(deps, &open, Some(arrived.event_id)).await
            }
            Arrival::Replied if open.sent && !open.replied => {
                let Some(label) = delegate::reply_label(&arrived.content) else {
                    return Ok(Outcome::Ignored(NOT_A_DELEGATION));
                };
                // The receipt carries the reply: a crash before its `peer`
                // line loses nothing (`peer_the_reply`).
                self.writer.write(
                    &mut self.context,
                    None,
                    Some(arrived.event_id.clone()),
                    LineBody::Delegate(DelegateBody {
                        id: open.id.clone(),
                        to: open.to.to_string(),
                        room: Some(open.room.clone()),
                        child: None,
                        state: DelegateState::Replied,
                        reason: None,
                        reply: Some(DelegateReply {
                            text: arrived.text.clone(),
                            artifacts: handed_over(&arrived.content),
                            label,
                        }),
                    }),
                )?;
                // The card it was handed on for goes to review with it.
                if let Some(source) = delegate::source_of(open.args.as_deref()) {
                    let lease = self.writer.lease();
                    let may_write = move || lease.as_ref().is_none_or(|lease| lease.may_write());
                    let (zone, session) = (&deps.sessions_zone, &self.context.session.path);
                    if let Err(error) = off_the_runtime(|| {
                        cards::write_run(zone, session, &source, Run::Review, None, &may_write)
                    }) {
                        tracing::warn!(%session, card = %source, %error, "agents: a handed-on card could not be set to review");
                    }
                }
                self.turn(deps, port, arrived, stop)
                    .await
                    .map(Outcome::Answered)
            }
            _ => Ok(Outcome::Ignored(NOT_A_DELEGATION)),
        }
    }

    /// The `peer` line of a reply whose receipt is logged and whose line is
    /// not, written now: the reply's label joined into the session's —
    /// readers narrowed, `local_only` and a lower integrity kept, another
    /// agent's words at most `agent` (R94) — then its words and its files.
    fn peer_the_reply(&mut self) -> Result<Option<LogLine>, ServeError> {
        let Some((receipt, event, body)) = self.context.reply_unpeered.clone() else {
            return Ok(None);
        };
        let (Some(reply), Ok(sender)) = (body.reply, OwnedUserId::try_from(body.to.as_str()))
        else {
            return Ok(None);
        };
        let agent = Label {
            integrity: Integrity::Agent,
            ..Label::top()
        };
        let joined = self.context.label.join(&reply.label).join(&agent);
        if joined != self.context.label {
            let reference = event.map_or_else(|| receipt.to_string(), |event| event.to_string());
            self.writer.write(
                &mut self.context,
                None,
                None,
                LineBody::Label(LabelBody::new(
                    &joined,
                    LabelCause {
                        kind: LabelCauseKind::AgentMessage,
                        reference,
                    },
                )),
            )?;
        }
        let peer = self.writer.write(
            &mut self.context,
            Some(receipt),
            None,
            LineBody::Peer(PeerBody {
                sender,
                text: reply.text,
                ask: None,
                artifacts: (!reply.artifacts.is_empty()).then_some(reply.artifacts),
            }),
        )?;
        Ok(Some(peer))
    }

    /// Send `open`'s brief, its target joined, and log `delegate sent`.
    ///
    /// The label as it is now must still let the brief into the room as it
    /// is now (R94): a block is logged `delegate refused` and sends nothing,
    /// ending the delegation. A send that fails is tried again on the
    /// host's clock under the same transaction id.
    async fn send_brief(
        &mut self,
        deps: &AgentDeps,
        open: &Delegation,
        join: Option<OwnedEventId>,
    ) -> Result<Outcome, ServeError> {
        let Some(rooms) = self.delegations.clone() else {
            return Ok(Outcome::Ignored(NOT_A_DELEGATION));
        };
        let session = self.context.session.path.clone();
        let known = rooms.known();
        let content = match delegate::content_for(
            open,
            &self.delegator(deps),
            &self.context.label,
            &known,
        ) {
            Ok(content) => content,
            Err(sentence) => {
                tracing::warn!(%session, delegation = %open.id, %sentence, "agents: a brief could not be composed");
                return Ok(Outcome::Ignored(NOT_A_DELEGATION));
            }
        };
        // A brief a person let through carries the label they approved, and
        // goes in only as those very bytes (R191a, R193).
        let content = self
            .declassified(deps, open)
            .and_then(|released| {
                delegate::approved_brief(&self.context.label, &released.readers, |approved| {
                    delegate::content_for(open, &self.delegator(deps), approved, &known)
                })
                .filter(|(_, sha)| *sha == released.sha256)
            })
            .map_or(content, |(widened, _)| widened);
        let members = match rooms.members(&open.room).await {
            Ok(members) => members,
            Err(error) => {
                tracing::warn!(%session, delegation = %open.id, %error, "agents: a delegation's room could not be read; its brief waits");
                self.retry.briefs.insert(open.id.clone());
                return Ok(Outcome::Ignored(BRIEF_UNSENT));
            }
        };
        let me = deps.home.config.matrix_user.clone();
        let effect = keeper_core::agents::delegation::brief_content(&content).to_string();
        let sinks = self.sinks(deps);
        let checked = match known
            .agents
            .iter()
            .find(|agent| agent.matrix_user == open.to)
        {
            Some(target) => sinks
                .verdict(
                    delegate::DELEGATE,
                    &Destination::Agent {
                        drive: target.drive.clone(),
                        agent: open.to.clone(),
                        room: Some(open.room.clone()),
                    },
                    &content.label,
                    &delegate::room_of(
                        members,
                        &known,
                        [&me, &open.to],
                        vec![target.home_readers.clone()],
                    ),
                    effect.as_bytes(),
                    None,
                )
                .map_err(|blocked| {
                    sinks.refused(
                        delegate::DELEGATE,
                        &blocked.drive,
                        &blocked.at,
                        &blocked.sentence,
                    );
                    blocked.sentence
                }),
            None => Err(format!("{} is no longer known on this host.", open.to)),
        };
        if let Err(reason) = checked {
            self.retry.briefs.remove(&open.id);
            self.writer.write(
                &mut self.context,
                None,
                join,
                LineBody::Delegate(DelegateBody {
                    id: open.id.clone(),
                    to: open.to.to_string(),
                    room: Some(open.room.clone()),
                    child: None,
                    state: DelegateState::Refused,
                    reason: Some(reason),
                    reply: None,
                }),
            )?;
            off_the_runtime(|| self.writer.sync())?;
            return Ok(Outcome::Ignored(BRIEF_REFUSED));
        }
        // The delegation's id is the send's transaction: a host that sends
        // it again — on the clock, or after a restart — sends one event.
        let txn = matrix_sdk::ruma::OwnedTransactionId::from(open.id.as_str());
        if let Err(error) = rooms
            .send(
                &open.room,
                keeper_core::agents::delegation::brief_content(&content),
                txn,
            )
            .await
        {
            tracing::warn!(%session, delegation = %open.id, %error, "agents: a brief could not be sent; it is sent again on the clock");
            self.retry.briefs.insert(open.id.clone());
            return Ok(Outcome::Ignored(BRIEF_UNSENT));
        }
        self.retry.briefs.remove(&open.id);
        self.writer.write(
            &mut self.context,
            None,
            join,
            LineBody::Delegate(DelegateBody {
                id: open.id.clone(),
                to: open.to.to_string(),
                room: Some(open.room.clone()),
                child: None,
                state: DelegateState::Sent,
                reason: None,
                reply: None,
            }),
        )?;
        off_the_runtime(|| self.writer.sync())?;
        Ok(Outcome::BriefSent(open.id.clone()))
    }

    /// The replies `open`'s target sent since its latest round that this
    /// session has not logged, read back from the room; a room that could
    /// not be read is tried again on the clock.
    async fn read_replies(&mut self, open: &Delegation, me: &UserId) -> Vec<Arrived> {
        let Some(rooms) = self.delegations.clone() else {
            return Vec::new();
        };
        let events = match rooms.since_brief(&open.room, me).await {
            Ok(events) => events,
            Err(error) => {
                tracing::warn!(delegation = %open.id, %error, "agents: a delegation's room could not be read back; it is read again on the clock");
                self.retry.replies.insert(open.id.clone());
                return Vec::new();
            }
        };
        self.retry.replies.remove(&open.id);
        let now = Instant::now();
        events
            .iter()
            .filter_map(|event| reply_of(event, &open.to, &open.room, now))
            .filter(|arrived| !self.writer.seen(&arrived.event_id).unwrap_or(true))
            .collect()
    }

    /// On a worker's start: every delegation's room is watched again —
    /// replied ones too, since a later round's reply must come back here; a
    /// brief whose target joined while this host was down is sent now; a
    /// reply that came meanwhile is returned, to be served like one that
    /// arrives (R55).
    pub async fn resume_delegations(&mut self, deps: &AgentDeps) -> Vec<Arrived> {
        let Some(rooms) = self.delegations.clone() else {
            return Vec::new();
        };
        let me = deps.home.config.matrix_user.clone();
        let all: Vec<Delegation> = self.context.delegations.values().cloned().collect();
        let mut replies = Vec::new();
        for open in all {
            rooms.watch(&open.room, &self.context.agent.room);
            if !open.sent {
                if rooms.joined(&open.room, &open.to).await {
                    if let Err(error) = self.send_brief(deps, &open, None).await {
                        tracing::warn!(delegation = %open.id, %error, "agents: a brief could not be logged");
                    }
                }
            } else if !open.replied {
                replies.extend(self.read_replies(&open, &me).await);
            }
        }
        replies
    }

    /// What failed on the delegations and waits for the clock, tried again:
    /// unsent briefs are sent, unread rooms read back; the replies found.
    async fn retry_delegations(&mut self, deps: &AgentDeps) -> Vec<Arrived> {
        let me = deps.home.config.matrix_user.clone();
        let mut replies = Vec::new();
        for id in std::mem::take(&mut self.retry.briefs) {
            let Some(open) = self.context.delegations.get(&id).filter(|open| !open.sent) else {
                continue;
            };
            let open = open.clone();
            if let Err(error) = self.send_brief(deps, &open, None).await {
                tracing::warn!(delegation = %open.id, %error, "agents: a brief could not be logged");
            }
        }
        for id in std::mem::take(&mut self.retry.replies) {
            let Some(open) = self
                .context
                .delegations
                .get(&id)
                .filter(|open| open.sent && !open.replied)
            else {
                continue;
            };
            let open = open.clone();
            replies.extend(self.read_replies(&open, &me).await);
        }
        if self.retry.tell {
            self.tell_the_requester(deps).await;
        }
        replies
    }

    /// The person's scope event (AD-382, R41): its focus replaces the held
    /// one; its drives, checked against `[tools].drives` with the home kept,
    /// become a `scope` line when they change the scope, and the accepted
    /// scope is echoed for the room's chips. A refused scope changes nothing
    /// and is named in the status's detail.
    async fn scope(
        &mut self,
        deps: &AgentDeps,
        port: &Arc<dyn EditPort>,
        arrived: Arrived,
    ) -> Result<Outcome, ServeError> {
        let request = match serde_json::from_value::<ScopeContent>(arrived.content) {
            Ok(request) if request.v == CONTENT_VERSION => request,
            _ => {
                tracing::info!(session = %self.context.session.path, note = UNREADABLE_SCOPE_REQUEST, "agents: a scope was not applied");
                return Ok(Outcome::Ignored(UNREADABLE_SCOPE_REQUEST));
            }
        };
        self.context.focus = request.focus.map(|focus| HeldFocus {
            focus,
            heard: arrived.received_at,
        });
        let Some(drives) = request.drives else {
            return Ok(Outcome::Focused);
        };
        let config = &deps.home.config;
        let asked = ScopeRequest(drives.into_iter().map(|drive| drive.id).collect());
        match asked.check(&config.drives, &config.drive) {
            Ok(scope) => {
                let changed = scope != self.context.scope;
                if changed {
                    self.writer.write(
                        &mut self.context,
                        None,
                        Some(arrived.event_id.clone()),
                        LineBody::Scope(ScopeBody {
                            drives: scope.clone(),
                            set_by: arrived.sender.clone(),
                        }),
                    )?;
                    off_the_runtime(|| self.writer.sync())?;
                }
                // A scope read back on start that changed nothing was echoed
                // when it first arrived; echoing it again on every restart
                // would only make the chips flicker through old answers.
                // Under a narrowed room no scope is echoed: it names drives
                // and the label to everyone in the room (R64).
                if changed || !arrived.replay {
                    self.echo_scope(deps, port).await;
                }
                Ok(Outcome::Scoped(scope))
            }
            Err(refusal) => {
                let sentence = refusal.to_string();
                let sent = self
                    .send_status(deps, port, RunState::Idle, Some(&sentence))
                    .await;
                if self.context.status_anchor.is_none() {
                    self.context.status_anchor = Some(sent);
                }
                Ok(Outcome::ScopeRefused(sentence))
            }
        }
    }

    /// The scope and the label as the room's chips read them: the owning
    /// host's own scope event (R30 shows only the agent's).
    fn scope_echo(&self, deps: &AgentDeps) -> Value {
        let echo = ScopeContent {
            v: CONTENT_VERSION,
            drives: Some(
                self.context
                    .scope
                    .iter()
                    .map(|id| ScopeDrive {
                        id: id.clone(),
                        title: deps
                            .drives
                            .get(id)
                            .map_or_else(|| id.clone(), |decl| decl.title.clone()),
                    })
                    .collect(),
            ),
            label: Some(self.context.label.clone()),
            focus: None,
            set_by: self.context.scope_set_by.clone(),
        };
        serde_json::to_value(echo).unwrap_or(Value::Null)
    }

    /// The person asked in the DM for a new conversation (R36). Only the
    /// claim holder serves this session, so only it acts: it makes the room
    /// with the person invited, then the `conversation` session folder
    /// naming it under an id derived from the request, then the room's
    /// status anchor, and tells the DM. The same request served again finds
    /// its folder and makes nothing.
    async fn new_conversation(
        &mut self,
        deps: &AgentDeps,
        port: &Arc<dyn EditPort>,
        arrived: Arrived,
    ) -> Result<Outcome, ServeError> {
        let (Some(rooms), Ok(request)) = (
            self.conversations.clone(),
            serde_json::from_value::<ConversationRequestContent>(arrived.content),
        ) else {
            return Ok(Outcome::Ignored(UNSERVED_CONVERSATION));
        };
        if request.v != CONTENT_VERSION {
            return Ok(Outcome::Ignored(UNSERVED_CONVERSATION));
        }
        let config = &deps.home.config;
        let title = request
            .title
            .map(|title| title.trim().to_owned())
            .filter(|title| !title.is_empty())
            .unwrap_or_else(|| NEW_CONVERSATION_TITLE.to_owned());
        let id = conversation_session_id(&config.drive, &config.id, &arrived.event_id);
        let zone = deps.sessions_zone.clone();
        if let Some(row) = off_the_runtime(|| verbs::find(&zone, &id.to_string())) {
            return Ok(Outcome::Conversation {
                path: row.path,
                made: false,
            });
        }
        let person = arrived.sender;
        // The new room carries the conversation's opening label to its
        // person: the one door a conversation's invite takes (AD-391).
        if let Err(sentence) = self.sinks(deps).check(
            "conversation",
            &Destination::Person {
                user: person.clone(),
            },
            &self.context.agent.label,
            &Sink::Room {
                humans: [person.clone()].into(),
                agent_audiences: Vec::new(),
            },
            title.as_bytes(),
            None,
        ) {
            tracing::info!(session = %self.context.session.path, %sentence, "agents: a conversation was not opened");
            return Ok(Outcome::Ignored(UNSERVED_CONVERSATION));
        }
        // The room is named after the proxy: its name is clear state, and
        // the title the person typed goes only in the encrypted status.
        let room = match rooms.create(&config.name, &person).await {
            Ok(room) => room,
            Err(error) => {
                tracing::warn!(session = %self.context.session.path, %error, "agents: a conversation's room could not be made");
                deliver(
                    port.as_ref(),
                    "m.room.message",
                    notice_content(CONVERSATION_FAILED),
                    Instant::now(),
                )
                .await;
                return Ok(Outcome::Ignored(UNSERVED_CONVERSATION));
            }
        };
        let agent = SessionAgent {
            id,
            agent: config.id.clone(),
            drive: config.drive.clone(),
            kind: SessionKind::Conversation,
            title: title.clone(),
            requested_by: person.clone(),
            parent: None,
            room: room.clone(),
            drives: self.context.scope.clone(),
            label: self.context.agent.label.clone(),
            needs: None,
            pin: None,
            hop: 0,
            dispatch_chain: vec![person.clone(), config.matrix_user.clone()],
            limits: None,
            workflow: None,
            created_at: chrono::Utc::now(),
        };
        let path = match off_the_runtime(|| {
            verbs::create_agent_session(&zone, &agent, chrono::Local::now())
        }) {
            Ok(CreateOutcome::Created { path, .. }) => path,
            Ok(CreateOutcome::Existed { path, .. }) => {
                rooms.discard(&room, &person).await;
                return Ok(Outcome::Conversation { path, made: false });
            }
            Err(error) => {
                tracing::warn!(session = %self.context.session.path, %error, "agents: a conversation's session could not be made");
                rooms.discard(&room, &person).await;
                deliver(
                    port.as_ref(),
                    "m.room.message",
                    notice_content(CONVERSATION_FAILED),
                    Instant::now(),
                )
                .await;
                return Ok(Outcome::Ignored(UNSERVED_CONVERSATION));
            }
        };
        let anchor = StatusContent {
            v: CONTENT_VERSION,
            session: format!("{}/{path}", deps.sessions_subfolder),
            kind: SessionKind::Conversation,
            title: title.clone(),
            agent: config.matrix_user.clone(),
            host: deps.host.as_str().to_owned(),
            epoch: 0,
            run: RunState::Idle,
            detail: None,
            waiting: None,
            anchor: None,
        };
        let content = serde_json::to_value(&anchor).unwrap_or(Value::Null);
        match rooms.send(&room, STATUS, content).await {
            Ok(sent) => {
                let again = StatusContent {
                    anchor: Some(sent),
                    ..anchor
                };
                let again = serde_json::to_value(again).unwrap_or(Value::Null);
                restate_once_joined(Arc::clone(&rooms), room.clone(), person.clone(), again);
            }
            Err(error) => {
                tracing::warn!(%room, %error, "agents: a new conversation's status anchor could not be sent");
            }
        }
        // The title is the person's words, said here: the DM hears it back
        // unless the label no longer reaches the DM as it is at the send
        // (R64, R168), which is audited (R65).
        let gate = self.gate(deps, port);
        let told = format!("I opened a new conversation, “{title}”.");
        deliver_gated(
            port.as_ref(),
            "m.room.message",
            Some(&gate),
            &|narrowed| {
                if narrowed {
                    gate.suppressed("notice");
                    notice_content("I opened a new conversation.")
                } else {
                    notice_content(&told)
                }
            },
            Instant::now(),
        )
        .await;
        Ok(Outcome::Conversation { path, made: true })
    }

    /// After a turn of a delegated session: a token budget that stopped it
    /// — at the gate before a request, or crossed by the turn's last
    /// completion — is told to the requester as the reply (the bound and
    /// what was spent), and the card goes `run: blocked` with the bound's
    /// word; so does an exchange whose last round passed without a reply,
    /// since no more message can come in it (Q12). Once blocked, nothing
    /// more is said or written.
    async fn after_delegated_turn(
        &mut self,
        deps: &AgentDeps,
        port: &Arc<dyn EditPort>,
        bound: Option<BoundReached>,
    ) -> Result<(), ServeError> {
        if self.context.agent.kind != SessionKind::Delegated
            || self.context.run == Some(keeper_core::agents::log::RunState::Blocked)
        {
            return Ok(());
        }
        let detail = match (bound, &self.context.agent.limits) {
            (Some(bound), _) => {
                // The host's own sentence, no word the session read; it
                // carries the label so the delegating session takes it, and
                // so it passes the reply's own guard first (R94, R169): a
                // room the label no longer reaches hears nothing, and the
                // refusal is audited (R65).
                self.reply_bound(deps, port, &bound).await;
                bound.word()
            }
            (None, Some(limits)) if self.context.exchange_rounds >= limits.rounds_per_exchange => {
                "rounds"
            }
            _ => return Ok(()),
        };
        let (zone, path) = (
            deps.sessions_zone.clone(),
            self.context.session.path.clone(),
        );
        let lease = self.writer.lease();
        let may_write = move || lease.as_ref().is_none_or(|lease| lease.may_write());
        if let Err(error) =
            off_the_runtime(|| delegate::set_card_run(&zone, &path, Run::Blocked, &may_write))
        {
            tracing::warn!(session = %path, %error, "agents: a delegated card could not be set blocked");
        }
        self.writer.write(
            &mut self.context,
            None,
            None,
            LineBody::Run(RunBody {
                state: keeper_core::agents::log::RunState::Blocked,
                detail: Some(detail.to_owned()),
            }),
        )?;
        off_the_runtime(|| self.writer.sync())?;
        Ok(())
    }

    /// While a delegation this session opened waits for its target to join,
    /// the session's status says so (AD-385, R29 F5).
    async fn say_waiting(&mut self, port: &Arc<dyn EditPort>, deps: &AgentDeps) {
        let waiting: Vec<&Delegation> = self
            .context
            .delegations
            .values()
            .filter(|open| !open.sent)
            .collect();
        if waiting.is_empty() {
            return;
        }
        let known = self.delegations.as_ref().map(|rooms| rooms.known());
        let names: Vec<String> = waiting
            .iter()
            .map(|open| {
                known
                    .as_ref()
                    .and_then(|known| {
                        known
                            .agents
                            .iter()
                            .find(|agent| agent.matrix_user == open.to)
                    })
                    .map_or_else(|| open.to.to_string(), |agent| agent.name.clone())
            })
            .collect();
        let detail = format!("waiting for {} to join", names.join(", "));
        let sent = self
            .send_status(deps, port, RunState::Idle, Some(&detail))
            .await;
        self.context.status_anchor.get_or_insert(sent);
    }

    /// A delegated session's budget reply: the bound's sentence, sent
    /// through the reply's guard over the room as it is now.
    async fn reply_bound(
        &mut self,
        deps: &AgentDeps,
        port: &Arc<dyn EditPort>,
        bound: &BoundReached,
    ) {
        let label = &self.context.label;
        let told = delegate::reply_content(&bound.sentence(), Vec::new(), label);
        let me = &deps.home.config.matrix_user;
        let sinks = self.sinks(deps);
        let admitted = match &self.delegations {
            Some(rooms) => {
                delegate::admit_reply(
                    rooms.as_ref(),
                    &sinks,
                    &self.context.agent.room,
                    [me, &self.context.agent.requested_by],
                    label,
                    &told,
                )
                .await
            }
            None => {
                sinks.refused(
                    delegate::REPLY,
                    "",
                    self.context.agent.room.as_str(),
                    delegate::NO_ROOMS,
                );
                Err(delegate::NO_ROOMS.to_owned())
            }
        };
        match admitted {
            Ok(()) => {
                deliver(port.as_ref(), "m.room.message", told, Instant::now()).await;
            }
            Err(reason) => {
                tracing::info!(session = %self.context.session.path, %reason, "agents: a budget reply was not sent");
            }
        }
    }

    /// The lines that open a turn, and the one its answer answers: for a
    /// person's message, its label join and a `user` line; for a brief, the
    /// session's `delegate accepted` (once), the delegation's label and a
    /// `peer` line; for a delegation's reply, the `peer` line its receipt
    /// stands for ([`Self::peer_the_reply`]).
    fn open_turn(&mut self, deps: &AgentDeps, arrived: &Arrived) -> Result<LogLine, ServeError> {
        if arrived.arrival == Arrival::Replied {
            return self.peer_the_reply()?.ok_or(ServeError::NoReceipt);
        }
        let (joined, cause, opening) = match arrived.arrival {
            Arrival::Brief => {
                let brief = read_brief(&arrived.content);
                if !self.context.accepted {
                    let accepted = LineBody::Delegate(DelegateBody {
                        id: self.context.agent.id.to_string(),
                        to: deps.home.config.matrix_user.to_string(),
                        room: Some(self.context.agent.room.clone()),
                        child: Some(ChildSession {
                            drive: deps.home.config.drive.clone(),
                            session: self.context.session.path.clone(),
                        }),
                        state: DelegateState::Accepted,
                        reason: None,
                        reply: None,
                    });
                    self.writer.write(&mut self.context, None, None, accepted)?;
                }
                let label = brief.as_ref().map_or_else(
                    || self.context.label.clone(),
                    |brief| self.context.label.join(&brief.label),
                );
                let text = brief.map_or_else(|| arrived.text.clone(), |brief| brief.brief);
                (
                    label,
                    LabelCauseKind::Delegation,
                    LineBody::Peer(PeerBody {
                        sender: arrived.sender.clone(),
                        text,
                        ask: None,
                        artifacts: None,
                    }),
                )
            }
            // The card's body in the agent's own name: no person spoke.
            Arrival::Scheduled => (
                self.context.label.clone(),
                LabelCauseKind::AgentMessage,
                LineBody::Peer(PeerBody {
                    sender: arrived.sender.clone(),
                    text: arrived.text.clone(),
                    ask: None,
                    artifacts: None,
                }),
            ),
            // The harvest brief names a session of the drive: its label is
            // joined before the model reads a word of it (R166).
            Arrival::Harvest => {
                let closed =
                    serde_json::from_value::<crate::stewards::Closed>(arrived.content.clone())
                        .map_err(|_| ServeError::NotAHarvest)?;
                (
                    self.context.label.join(&closed.label),
                    LabelCauseKind::DriveRead,
                    LineBody::Peer(PeerBody {
                        sender: arrived.sender.clone(),
                        text: arrived.text.clone(),
                        ask: None,
                        artifacts: None,
                    }),
                )
            }
            _ => {
                let person = deps
                    .home
                    .config
                    .human
                    .clone()
                    .unwrap_or_else(|| arrived.sender.clone());
                let readers = Label {
                    readers: keeper_core::agents::label::Readers::Only(
                        deps.home.config.audience.clone(),
                    ),
                    ..Label::top()
                };
                let joined = self
                    .context
                    .on_user_line(&arrived.sender, &person, &readers)
                    .unwrap_or_else(|| self.context.label.clone());
                (
                    joined,
                    LabelCauseKind::PersonMessage,
                    LineBody::User(UserBody {
                        sender: arrived.sender.clone(),
                        text: arrived.text.clone(),
                        attachments: Vec::new(),
                    }),
                )
            }
        };
        if joined != self.context.label {
            self.writer.write(
                &mut self.context,
                None,
                None,
                LineBody::Label(LabelBody::new(
                    &joined,
                    LabelCause {
                        kind: cause,
                        reference: arrived.event_id.to_string(),
                    },
                )),
            )?;
        }
        Ok(self.writer.write(
            &mut self.context,
            None,
            Some(arrived.event_id.clone()),
            opening,
        )?)
    }

    async fn turn(
        &mut self,
        deps: &AgentDeps,
        port: Arc<dyn EditPort>,
        arrived: Arrived,
        stop: CancelSignal,
    ) -> Result<TurnReport, ServeError> {
        self.run_turn(deps, port, Opening::Arrived(arrived), stop)
            .await
    }

    /// Go on with the turn a parked call ended, that call settled (93.2):
    /// its result, then the round's remaining calls in order, then the
    /// model, under a new anchor answering the same question.
    pub(crate) async fn resume_turn(
        &mut self,
        deps: &AgentDeps,
        port: Arc<dyn EditPort>,
        resume: crate::approvals::Resume,
        stop: CancelSignal,
    ) -> Result<TurnReport, ServeError> {
        self.run_turn(deps, port, Opening::Resumed(Box::new(resume)), stop)
            .await
    }

    async fn run_turn(
        &mut self,
        deps: &AgentDeps,
        port: Arc<dyn EditPort>,
        opening: Opening,
        stop: CancelSignal,
    ) -> Result<TurnReport, ServeError> {
        let label_before = self.context.label.clone();
        let sinks = self.sinks(deps);
        let (user_id, question, received_at, harvest, resume) = match opening {
            Opening::Arrived(arrived) => {
                let user = self.open_turn(deps, &arrived)?;
                // A harvest's anchor names its arrival, the closed session:
                // each attempt asks the room as it is then, and a room its
                // label no longer reaches hears only that a harvest ran
                // (R166).
                let harvest = arrived.arrival == Arrival::Harvest;
                (
                    user.id,
                    arrived.event_id,
                    arrived.received_at,
                    harvest,
                    None,
                )
            }
            Opening::Resumed(resume) => (
                self.context.unanswered.unwrap_or_else(|| {
                    resume
                        .call
                        .as_ref()
                        .or(resume.rest.first())
                        .map_or_else(Ulid::new, |(line, _)| *line)
                }),
                resume.question.clone(),
                Instant::now(),
                false,
                Some(*resume),
            ),
        };
        // Every send of the turn into the room asks this gate, against the
        // room's members at that send (R168).
        let gate = self.gate(deps, &port);
        let (session, line) = (&self.context.session.path, user_id.to_string());
        let (anchor, anchor_at, _) = deliver_gated(
            port.as_ref(),
            "m.room.message",
            harvest.then_some(gate.as_ref()),
            &|narrowed| {
                let question = if narrowed {
                    unnamed_harvest()
                } else {
                    &question
                };
                anchor_content(session, &line, question)
            },
            Instant::now(),
        )
        .await;
        let sink = MatrixSink::start(
            Arc::clone(&port),
            anchor.clone(),
            anchor_at,
            Some(Arc::clone(&gate)),
        );
        let board = StatusBoard::start(
            Arc::clone(&port),
            self.context.status_anchor.clone(),
            self.status_base(deps),
            Some(Arc::clone(&gate)),
        );

        let tools = TurnTools {
            surface: self.surface.clone(),
            delegations: self.delegations.clone(),
            room: Arc::clone(&port),
            from: self.delegator(deps),
            sinks: sinks.clone(),
            gate: Arc::clone(&gate),
        };
        let ran = run_agent_turn(
            &mut self.context,
            &mut self.writer,
            deps,
            &sink,
            &board,
            stop,
            tools,
            resume,
        )
        .await;
        let stream_end = Instant::now();

        // S-16's room half: once the label no longer reaches the room as it
        // is now, the room gets one fixed sentence and the log the whole
        // answer (D-31); the suppression is audited (R65).
        let shown = sink.text();
        gate.set_label(self.context.label.clone());
        let withheld = !shown.trim().is_empty() && gate.narrowed().await;
        if withheld {
            gate.suppressed("answer");
        }
        let visible = if withheld {
            NARROWER_THAN_ROOM.to_owned()
        } else {
            shown.clone()
        };
        let (final_text, answer) = match &ran.ending {
            TurnEnding::Complete => (visible, shown.clone()),
            TurnEnding::Stopped => {
                let suffix = shutdown_suffix(deps.host.as_str());
                (format!("{visible}{suffix}"), format!("{shown}{suffix}"))
            }
            TurnEnding::LocalOnly => (join_note(&visible, LOCAL_ONLY_REFUSAL), shown.clone()),
            TurnEnding::Bounded => (
                join_note(
                    &visible,
                    &ran.bound.map(|b| b.sentence()).unwrap_or_default(),
                ),
                shown.clone(),
            ),
            TurnEnding::Failed => (join_note(&visible, TURN_FAILED), shown.clone()),
            TurnEnding::Parked => {
                let summary = ran.parked.as_ref().map_or_else(String::new, |parked| {
                    keeper_core::agents::approval::summary_of(
                        parked.parking.classification.tool,
                        &parked.args,
                    )
                });
                let note = format!("{} {summary}", crate::approvals::WAITING_FOR);
                (join_note(&visible, &note), shown.clone())
            }
        };
        // The room gets the log's redaction (S-17), and the artifact too, so
        // it equals the log's line.
        let final_text = redact_secrets(&final_text).text;
        let artifact = format!("artifacts/answer-{user_id}.md");
        let message = match cut(&final_text, &artifact) {
            Some(message) => {
                let path = self.context.session.path.clone();
                let lease = self.writer.lease();
                let may_write = move || lease.as_ref().is_none_or(|lease| lease.may_write());
                let home = &deps.home.drive;
                // The file it would be, drive-relative, and its bytes.
                let in_drive = format!("{}/{path}/{artifact}", deps.sessions_subfolder);
                let written = sinks
                    .check(
                        "session_write",
                        &Destination::Drive {
                            drive: home.id.clone(),
                            path: in_drive.clone(),
                        },
                        &self.context.label,
                        &Sink::DriveWrite {
                            drive_readers: Readers::Only(home.readers.clone()),
                        },
                        final_text.as_bytes(),
                        Some(&in_drive),
                    )
                    .and_then(|()| {
                        off_the_runtime(|| {
                            session_write(
                                &deps.sessions_zone,
                                &path,
                                &artifact,
                                &final_text,
                                &may_write,
                            )
                        })
                        .map_err(|error| error.to_string())
                    });
                match written {
                    Ok(_) => message,
                    Err(error) => {
                        tracing::warn!(%error, session = %path, "agents: the long answer's artifact could not be written; the room is pointed at the log");
                        cut_to_log(&final_text)
                    }
                }
            }
            None => final_text.clone(),
        };
        // A room that narrows while the final edit is retried gets the
        // fixed sentence at that attempt; a turn that streamed nothing has
        // only the host's own note to say.
        let narrowed_text = if shown.trim().is_empty() {
            message.as_str()
        } else {
            NARROWER_THAN_ROOM
        };
        let delivered = sink.finish(&message, narrowed_text).await;
        let withheld = withheld || (delivered.narrowed && !shown.trim().is_empty());
        if withheld {
            gate.suppressed("answer");
        }
        // Only now is the status `idle`: a device following the answer takes
        // that as the answer being whole (AD-384).
        if let Some(anchor) = board.finish(&delivered).await {
            self.context.status_anchor = Some(anchor);
        }

        let closing = match ran.ending {
            // A parked turn is not over: nothing closes it yet.
            TurnEnding::Parked => None,
            TurnEnding::Complete | TurnEnding::Stopped => {
                let outcome = ran.outcome.as_ref();
                let usage = outcome.and_then(|o| o.usage.as_ref());
                let line = self.writer.write(
                    &mut self.context,
                    ran.parent,
                    Some(delivered.final_event.clone()),
                    LineBody::Assistant(AssistantBody {
                        text: if ran.ending == TurnEnding::Stopped {
                            format!("{}{}", ran.round_text, shutdown_suffix(deps.host.as_str()))
                        } else {
                            ran.round_text.clone()
                        },
                        model: outcome
                            .and_then(|o| o.model.clone())
                            .unwrap_or_else(|| deps.bot.target.clone()),
                        finish: outcome
                            .map_or_else(|| "stop".to_owned(), |o| finish_word(&o.finish_reason)),
                        usage: Usage {
                            prompt: usage.and_then(|u| u.prompt_tokens),
                            completion: usage.and_then(|u| u.completion_tokens),
                        },
                        ttft_ms: outcome.and_then(|o| o.first_token_ms),
                        duration_ms: outcome.map_or(0, |o| o.total_ms),
                        anchor_event: Some(anchor.to_string()),
                    }),
                )?;
                Some(line)
            }
            TurnEnding::LocalOnly | TurnEnding::Bounded | TurnEnding::Failed => {
                let mut parent = ran.parent.or(Some(user_id));
                // The prose the room already saw of the round that failed:
                // the next turn's model must read what the person read.
                if !ran.round_logged && !ran.round_text.is_empty() {
                    let partial = self.writer.write(
                        &mut self.context,
                        parent,
                        None,
                        LineBody::Assistant(AssistantBody {
                            text: ran.round_text.clone(),
                            model: deps.bot.target.clone(),
                            finish: "failed".to_owned(),
                            usage: Usage::default(),
                            ttft_ms: None,
                            duration_ms: 0,
                            anchor_event: Some(anchor.to_string()),
                        }),
                    )?;
                    parent = Some(partial.id);
                }
                let (sentence, code) = match (ran.ending, ran.bound) {
                    (TurnEnding::LocalOnly, _) => (LOCAL_ONLY_REFUSAL.to_owned(), "local_only"),
                    (TurnEnding::Bounded, Some(bound)) => (bound.sentence(), bound.word()),
                    _ => (
                        ran.error.clone().unwrap_or_else(|| TURN_FAILED.to_owned()),
                        "turn_failed",
                    ),
                };
                Some(self.writer.write(
                    &mut self.context,
                    parent,
                    Some(delivered.final_event.clone()),
                    LineBody::Error(ErrorBody {
                        sentence,
                        code: code.to_owned(),
                    }),
                )?)
            }
        };
        if withheld {
            self.writer.write(
                &mut self.context,
                closing.as_ref().map(|line| line.id),
                None,
                LineBody::Error(ErrorBody {
                    sentence: NARROWER_THAN_ROOM.to_owned(),
                    code: LABEL_CODE.to_owned(),
                }),
            )?;
        }
        off_the_runtime(|| self.writer.sync())?;
        if let Some(parked) = &ran.parked {
            if let Err(error) = self.park(deps, &port, parked).await {
                // It cannot wait for anyone: refused as if no one could be
                // asked, and the turn closes.
                tracing::error!(session = %self.context.session.path, %error, "agents: a call could not be parked");
                self.refuse_parked(deps, parked, user_id, &error)?;
            }
        }
        // A `label` line changes the label chip: the room is told, as after
        // a `scope` line — unless the label no longer reaches the room, whose
        // members the chip's drives and readers would then reach (R64).
        if self.context.label != label_before {
            self.echo_scope(deps, &port).await;
        }
        self.tell_the_requester(deps).await;
        // The closing line counts the last completion's tokens too: a budget
        // it crossed parks the session as one the gate stopped would.
        let bound = ran.bound.or_else(|| self.context.token_bound());
        self.after_delegated_turn(deps, &port, bound).await?;
        self.say_waiting(&port, deps).await;

        Ok(TurnReport {
            user_line: user_id,
            anchor,
            received_at,
            anchor_at,
            stream_end,
            final_at: delivered.accepted_at,
            edits: delivered.edits,
            ending: ran.ending,
            prompt_sha256: ran.prompt_sha256,
            answer,
        })
    }

    /// This session's room gate over `port` (R168): the room's members at
    /// each send, the known agents through their audiences, the session's
    /// own agents left out, under the label now; a suppression audited.
    pub(crate) fn gate(&self, deps: &AgentDeps, port: &Arc<dyn EditPort>) -> Arc<RoomGate> {
        Arc::new(RoomGate::new(
            Arc::clone(port),
            self.delegations.as_ref().map(|rooms| rooms.known()),
            self.context.room_own(&deps.home.config.matrix_user),
            self.context.label.clone(),
            Some((self.sinks(deps), self.context.agent.room.clone())),
        ))
    }

    /// Echo the scope into the room, unless the label no longer reaches it
    /// now: the echo names drives and the label (R64), and its suppression
    /// is audited (R65). Asked again at every attempt.
    async fn echo_scope(&mut self, deps: &AgentDeps, port: &Arc<dyn EditPort>) {
        let gate = self.gate(deps, port);
        let echo = self.scope_echo(deps);
        let sent = deliver_unless_narrowed(
            port.as_ref(),
            SCOPE,
            &gate,
            &|narrowed| (!narrowed).then(|| echo.clone()),
            Instant::now(),
        )
        .await;
        if sent.is_none() {
            gate.suppressed("scope");
        }
    }

    /// Send this session's status — `run`, `detail` — into the room as an
    /// edit of its anchor, each attempt under the room as it is then (R64,
    /// R168): narrowed, the fixed title and no detail, audited (R65).
    pub(crate) async fn send_status(
        &mut self,
        deps: &AgentDeps,
        port: &Arc<dyn EditPort>,
        run: RunState,
        detail: Option<&str>,
    ) -> OwnedEventId {
        let gate = self.gate(deps, port);
        let base = self.status_base(deps);
        let anchor = self.context.status_anchor.clone();
        let (sent, ..) = deliver_gated(
            port.as_ref(),
            STATUS,
            Some(&gate),
            &|narrowed| {
                if narrowed {
                    gate.suppressed("status");
                }
                status_content(&base, anchor.as_ref(), run, detail, narrowed)
            },
            Instant::now(),
        )
        .await;
        sent
    }

    /// The person this session's work is for: the head of its dispatch
    /// chain, else whoever asked for it.
    fn person(&self) -> &OwnedUserId {
        let agent = &self.context.agent;
        agent.dispatch_chain.first().unwrap_or(&agent.requested_by)
    }

    /// Once the label no longer reaches everyone the session was opened
    /// for, or an answer was withheld from a room grown wider than the
    /// label ([`SessionContext::narrowed`]), its person is told — once,
    /// through their proxy's own client, in their proxy's DM — which work
    /// it is and where its answers are
    /// (R64, R169). A `told` line marks it done; a send that failed is
    /// tried again on the host's clock and after a restart. A DM the label
    /// does not reach is refused and audited (R65), and not tried again.
    async fn tell_the_requester(&mut self, deps: &AgentDeps) {
        if self.context.told || !self.context.narrowed() {
            self.retry.tell = false;
            return;
        }
        let person = self.person().clone();
        let sinks = self.sinks(deps);
        // A host that runs no proxy of theirs has no door to them: the next
        // turn, or a host that does, tells them (DW-460).
        let Some(doors) = self.doors.clone() else {
            tracing::info!(session = %self.context.session.path, "agents: a narrowed session's person has no proxy on this host");
            self.retry.tell = false;
            return;
        };
        let Some(dm) = doors.dm(&person) else {
            tracing::info!(session = %self.context.session.path, "agents: a narrowed session's person has no DM this host can name");
            self.retry.tell = false;
            return;
        };
        let agent = &self.context.agent;
        let detail = format!(
            "“{}” ({}/{}) read something not everyone in its room may read. It goes on there under “{NARROWED_STATUS}”, and its answers are in its log.",
            agent.title, deps.sessions_subfolder, self.context.session.path
        );
        let members = match doors.members(&person).await {
            Ok(members) => members,
            Err(error) => {
                tracing::warn!(%dm, %error, "agents: a narrowed session's person's DM could not be read; telling them waits");
                self.retry.tell = true;
                return;
            }
        };
        // The proxy is its person's door: its audience is theirs.
        let known = self.delegations.as_ref().map(|rooms| rooms.known());
        let refused = sinks.check(
            "status",
            &Destination::Room { room: dm.clone() },
            &self.context.label,
            &room_audience(members, known.as_deref(), &[]),
            detail.as_bytes(),
            None,
        );
        if refused.is_err() {
            self.retry.tell = false;
            return;
        }
        if let Err(error) = doors
            .tell(&person, "m.room.message", notice_content(&detail))
            .await
        {
            tracing::warn!(%dm, %error, "agents: a narrowed session's detail could not reach its person's DM; it is sent again");
            self.retry.tell = true;
            return;
        }
        self.retry.tell = false;
        let told = self.writer.write(
            &mut self.context,
            None,
            None,
            LineBody::Told(ToldBody { person, room: dm }),
        );
        if let Err(error) = told.and_then(|_| off_the_runtime(|| self.writer.sync())) {
            tracing::warn!(session = %self.context.session.path, %error, "agents: a told line could not be written");
        }
    }

    /// The approvers of this session — each reader of its label — whom this
    /// host can ask: those whose proxy it runs, with the DM it names.
    pub(crate) fn askable(&self) -> Vec<(OwnedUserId, OwnedRoomId)> {
        let (Some(doors), Readers::Only(approvers)) = (&self.doors, &self.context.label.readers)
        else {
            return Vec::new();
        };
        approvers
            .iter()
            .filter_map(|person| Some((person.clone(), doors.dm(person)?)))
            .collect()
    }

    /// An approval's request `content` that its room may not carry (R85):
    /// to each approver this host can ask ([`Self::askable`]) through
    /// their proxy's DM, as the proxy, each DM checked against the label as
    /// it is now and a refusal audited (R65). An approver no door on this
    /// host reaches is not asked from here (DW-485); the room's status
    /// says only R64's fixed sentence. Who it reached, and in which DM.
    pub(crate) async fn request_by_doors(
        &mut self,
        deps: &AgentDeps,
        content: &Value,
    ) -> Vec<(OwnedUserId, OwnedRoomId)> {
        let event_type = keeper_core::agents::events::APPROVAL_REQUEST;
        let Some(doors) = self.doors.clone() else {
            return Vec::new();
        };
        let sinks = self.sinks(deps);
        let known = self.delegations.as_ref().map(|rooms| rooms.known());
        let effect = content.to_string();
        let mut asked = Vec::new();
        for (person, dm) in self.askable() {
            let members = match doors.members(&person).await {
                Ok(members) => members,
                Err(error) => {
                    tracing::warn!(%dm, %error, "agents: an approver's DM could not be read; the request is not sent there");
                    continue;
                }
            };
            // The proxy is its person's door: its audience is theirs.
            let refused = sinks.check(
                event_type,
                &Destination::Room { room: dm.clone() },
                &self.context.label,
                &room_audience(members, known.as_deref(), &[]),
                effect.as_bytes(),
                None,
            );
            if refused.is_err() {
                continue;
            }
            match doors.tell(&person, event_type, content.clone()).await {
                // Its decision comes home from that DM (R89).
                Ok(_) => {
                    if let (Some(id), Some(inbox)) = (content["id"].as_str(), &self.inbox) {
                        let home = &self.context.agent.room;
                        doors.forwards().expect(&dm, id, home, Arc::clone(inbox));
                    }
                    asked.push((person, dm));
                }
                Err(error) => {
                    tracing::warn!(%dm, %error, "agents: an approval's request could not reach its approver's DM");
                }
            }
        }
        asked
    }
}

/// Why a session could not be opened for serving.
#[derive(Debug, thiserror::Error)]
pub enum ServeOpenError {
    #[error(transparent)]
    Writer(#[from] WriterError),
    #[error(transparent)]
    Load(#[from] LoadRefusal),
}

fn join_note(shown: &str, note: &str) -> String {
    if shown.trim().is_empty() {
        note.to_owned()
    } else {
        format!("{shown}\n\n{note}")
    }
}

/// What the tool loop left for the close.
struct Ran {
    ending: TurnEnding,
    outcome: Option<chat::ChatOutcome>,
    /// The last round's prose: the final `assistant` line's text.
    round_text: String,
    /// Whether that prose is already in the log, on its round's line.
    round_logged: bool,
    /// The line the closing line answers: the last tool result, or none.
    parent: Option<Ulid>,
    prompt_sha256: Option<String>,
    error: Option<String>,
    /// The bound that stopped a delegated session's run.
    bound: Option<BoundReached>,
    /// The call the turn parked on, when it did.
    parked: Option<crate::approvals::ParkedTurn>,
}

/// How a turn opens: on an arrival, or at a call that waited for a person.
enum Opening {
    Arrived(Arrived),
    Resumed(Box<crate::approvals::Resume>),
}

/// The lines a running turn writes, behind one lock: the tool loop's event
/// sink, its reporter and its round gate all reach them.
struct TurnLog<'a> {
    context: &'a mut SessionContext,
    writer: &'a mut SessionWriter,
    model: String,
    round_text: String,
    round_line: Option<Ulid>,
    last_line: Option<Ulid>,
    progress: ToolProgress,
    failure: Option<WriterError>,
    local_only: bool,
    /// The delegated session's budget stopped the run before this round.
    bound: Option<BoundReached>,
    /// The usage the endpoint reported for the round under way (R69).
    round_usage: Usage,
    /// Why the model's stream broke, when it did: the partial answer is
    /// returned as an outcome, the cause only to the event sink.
    broken: Option<String>,
    /// The parked call's `tool_call` line and wire call (R73).
    parked: Option<(Option<Ulid>, chat::ToolCall)>,
}

impl TurnView for Mutex<TurnLog<'_>> {
    fn label(&self) -> Label {
        self.lock()
            .unwrap_or_else(|p| p.into_inner())
            .context
            .label
            .clone()
    }

    fn may_write(&self) -> bool {
        self.lock()
            .unwrap_or_else(|p| p.into_inner())
            .writer
            .may_write()
    }

    fn delegation(&self, id: &str) -> Option<Delegation> {
        self.lock()
            .unwrap_or_else(|p| p.into_inner())
            .context
            .delegations
            .get(id)
            .cloned()
    }

    fn handed(&self, source: &str) -> Option<Delegation> {
        self.lock()
            .unwrap_or_else(|p| p.into_inner())
            .context
            .handed(source)
            .cloned()
    }
}

impl TurnLog<'_> {
    fn write(&mut self, parent: Option<Ulid>, body: LineBody) -> Option<Ulid> {
        if self.failure.is_some() {
            return None;
        }
        match self.writer.write(self.context, parent, None, body) {
            Ok(line) => Some(line.id),
            Err(error) => {
                self.failure = Some(error);
                None
            }
        }
    }
}

/// Whether arming may ask the provider which tools its model supports.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Probe {
    /// A turn: ask, as every turn does.
    Ask,
    /// A read-only verb: compose as if the answer were unknown, and reach
    /// nothing.
    Skip,
}

/// Arm one turn of `context`'s agent: its own grants, its history, and only
/// the tools in `[tools].allow`. The system message is not in it yet: it is
/// [`SessionContext::compose`] over the returned context bundle.
pub async fn arm_agent(
    context: &SessionContext,
    deps: &AgentDeps,
    probe: Probe,
) -> crate::turn::Armed {
    let config = &deps.home.config;
    let grants = Arc::new(AgentGrants::new(
        &deps.row.provider.id,
        &deps.bot.id,
        &config.drives,
        &context.scope,
        &config.allow,
    ));
    let origin = TurnOrigin::Agent {
        session: context.session.clone(),
    };
    let mut armed = arm_turn_probing(
        &deps.env,
        &deps.data_dir,
        &deps.row,
        &deps.bot,
        &deps.bot.target,
        context.messages.clone(),
        grants,
        &move |_| origin.clone(),
        probe == Probe::Ask,
    )
    .await;
    // Whether this model is offered tools at all: the surface tools ride on
    // the same offer, and a model that cannot call tools is told of none.
    let tools_offered = !armed.request.tools.is_empty();
    armed
        .request
        .tools
        .retain(|spec| config.allow.contains(&spec.name));
    // The surface, `delegate` and `reply` tools are the agent's own, never
    // a drive verb's spec: `delegate` as `[tools].allow` says, `reply` in a
    // delegated session whatever it says (R48).
    if tools_offered {
        armed
            .request
            .tools
            .extend(crate::surface::specs(&crate::surface::offered(config)));
        armed.request.tools.extend(delegate::specs(
            config.allow.iter().any(|name| name == delegate::DELEGATE),
            context.agent.kind == SessionKind::Delegated,
        ));
        armed
            .request
            .tools
            .extend(crate::cards::specs(&config.allow));
    }
    armed
}

/// The `open` line for `composed`.
fn open_body(context: &SessionContext, deps: &AgentDeps, composed: &ComposedPrompt) -> OpenBody {
    let config = &deps.home.config;
    OpenBody {
        agent: config.id.clone(),
        drive: config.drive.clone(),
        kind: context.agent.kind,
        title: context.agent.title.clone(),
        requested_by: context.agent.requested_by.clone(),
        label: context.label.clone(),
        drives: context.scope.clone(),
        model: format!(
            "bot:{}:{}#{}",
            config.bot.kind.as_registry_str(),
            config.bot.base,
            config.bot.target
        ),
        prompt_sha256: composed.prompt_sha256.clone(),
        memory_sha256: composed.memory_sha256.clone(),
    }
}

/// Run one turn of `context`'s agent: arm, compose, and drive the tool loop
/// into `sink`, writing every round, call, result and label to the log.
#[allow(clippy::too_many_arguments)]
async fn run_agent_turn(
    context: &mut SessionContext,
    writer: &mut SessionWriter,
    deps: &AgentDeps,
    sink: &MatrixSink,
    board: &StatusBoard,
    stop: CancelSignal,
    tools: TurnTools,
    resume: Option<crate::approvals::Resume>,
) -> Ran {
    let config = &deps.home.config;
    let local = deps.model_is_local();
    let mut armed = arm_agent(context, deps, Probe::Ask).await;
    let failed = |error: String| Ran {
        ending: TurnEnding::Failed,
        outcome: None,
        round_text: String::new(),
        round_logged: false,
        parent: None,
        prompt_sha256: None,
        error: Some(error),
        bound: None,
        parked: None,
    };

    // What the prompt carries beyond the conversation — the home's frozen
    // files and the drives' context files — joins the label before it is
    // composed, so the first round's gate sees it (R168).
    if let Err(error) = join_prompt_sources(context, writer, deps, &mut armed) {
        return failed(error.to_string());
    }
    board.relabel(&context.label);
    let mut composed = context.compose(deps, armed.context.as_ref());
    if context.open.as_ref().map(|open| &open.prompt_sha256) != Some(&composed.prompt_sha256) {
        // What the model is told changed (or was never recorded): the frame
        // takes the new `open` line's time, and the line records the digest
        // of exactly that composition.
        let ts = writer.next_ts();
        context.frame_time = ts.with_timezone(&chrono::Local).fixed_offset();
        composed = context.compose(deps, armed.context.as_ref());
        let open = open_body(context, deps, &composed);
        if let Err(error) = writer.write_at(context, ts, None, None, LineBody::Open(open)) {
            return failed(error.to_string());
        }
    }
    armed
        .request
        .messages
        .insert(0, ChatMessage::text(Role::System, composed.text.clone()));

    let endpoint = match endpoint_of(&deps.env, &deps.row, Some(&deps.bot.target)).await {
        Ok(endpoint) => endpoint,
        Err(error) => return failed(error.to_string()),
    };
    let read_timeout = read_timeout_of(&deps.row);
    let client = match http::client(read_timeout) {
        Ok(client) => client,
        Err(error) => return failed(error.to_string()),
    };
    let offered = crate::surface::offered(config);
    let room_gate = Arc::clone(&tools.gate);
    let surface = crate::surface::person(config)
        .filter(|_| !offered.is_empty())
        .map(|person| crate::surface::SurfaceTools {
            port: tools.surface,
            person: person.clone(),
            offered,
            profiles: armed.profiles.clone(),
            scope: context.scope.clone(),
            stop: stop.clone(),
            wait: keeper_core::agents::events::SURFACE_WAIT,
            lines: Mutex::new(Vec::new()),
            admit: Some(Arc::new(move |tool: &str, effect: &[u8]| {
                tokio::task::block_in_place(|| {
                    tokio::runtime::Handle::current().block_on(room_gate.verdict(tool, effect))
                })
            })),
        });
    // A read's label is read from the files it named (R119).
    let read_profiles = armed.profiles.clone();
    // A write is classified where it lands on these drives.
    let profiles = armed.profiles.clone();
    // An agent writes its sessions through its session tools only (R51),
    // and every drive call is classified before its audit row.
    let drive_host = armed.drive.agent_host(
        HostIds {
            data_dir: deps.data_dir.clone(),
            provider_id: deps.row.provider.id.clone(),
            bot_id: deps.bot.id.clone(),
            session_id: context.agent.id.to_string(),
            message_id: None,
        },
        armed.profiles,
        stop.clone(),
    );

    let session_dir = format!("{}/{}", deps.sessions_subfolder, context.session.path);
    let session_landed = landed_names(
        profiles
            .iter()
            .find(|profile| profile.id == deps.home.drive.id),
        &session_dir,
    );
    let agent = context.agent.clone();
    let log = Mutex::new(TurnLog {
        context,
        writer,
        model: deps.bot.target.clone(),
        round_text: String::new(),
        round_line: None,
        last_line: None,
        progress: ToolProgress::default(),
        failure: None,
        local_only: false,
        bound: None,
        round_usage: Usage::default(),
        broken: None,
        parked: None,
    });
    let host = AllowedTools {
        inner: drive_host,
        allow: config.allow.clone(),
        surface,
        cards: crate::cards::CardTools {
            from: tools.from.clone(),
            drive_readers: Readers::Only(deps.home.drive.readers.clone()),
            view: &log,
            allow: &config.allow,
        },
        delegation: DelegateTools::new(
            tools.from,
            tools.delegations,
            tools.room,
            &log,
            config.allow.iter().any(|name| name == delegate::DELEGATE),
            &tools.sinks,
        ),
        view: &log,
        sinks: &tools.sinks,
        drives: &deps.drives,
        home: &deps.home.drive,
        session_dir,
        session_landed,
        profiles,
        agent,
        tiers: Mutex::new(HashMap::new()),
        parks: deps.decisions.is_some(),
        bound: Mutex::new(None),
        parked: Mutex::new(None),
    };
    let tool_loop = ToolLoop {
        client: &client,
        endpoint: &endpoint,
        host: &host,
        default_profile_id: &armed.default_profile_id,
    };
    let lock = || log.lock().unwrap_or_else(|p| p.into_inner());

    let mut events = |event: ToolLoopEvent| match event {
        ToolLoopEvent::RoundStarted { round, .. } => {
            let mut log = lock();
            log.round_text.clear();
            log.round_line = None;
            log.round_usage = Usage::default();
            let shown = sink.text();
            if round > 0 && !shown.is_empty() && !shown.ends_with('\n') {
                sink.push("\n\n");
            }
        }
        ToolLoopEvent::Chat(ChatEvent::Usage(usage)) => {
            lock().round_usage = Usage {
                prompt: usage.prompt_tokens,
                completion: usage.completion_tokens,
            };
        }
        ToolLoopEvent::Chat(ChatEvent::ContentDelta(text)) => {
            lock().round_text.push_str(&text);
            // Each paced edit asks the room's gate first: once the label no
            // longer reaches the room, no more of the answer is streamed
            // into it and its status says only the fixed sentence (S-16).
            sink.push(&text);
        }
        ToolLoopEvent::Chat(ChatEvent::Failed { error }) => {
            lock().broken = Some(error.to_string());
        }
        _ => {}
    };
    // A call's result and what follows it, under its `tool_call` line.
    let finish = |log: &mut TurnLog<'_>,
                  call_line: Option<Ulid>,
                  record: &ToolCallRecord,
                  wire: &chat::ToolCall,
                  outcome: &ToolOutcome| {
        let read = read_label(deps, &read_profiles, record, outcome);
        let result_label = read
            .as_ref()
            .map_or_else(|| log.context.label.clone(), |(label, _)| label.clone());
        let (word, truncated) = match outcome {
            ToolOutcome::Refused { .. } => (ToolOutcomeWord::Refused, None),
            ToolOutcome::Text {
                truncated_at: Some(shown),
                of_bytes: Some(total),
                ..
            } => (
                ToolOutcomeWord::Ok,
                Some(Truncated {
                    shown: *shown,
                    total: *total,
                }),
            ),
            _ => (ToolOutcomeWord::Ok, None),
        };
        log.last_line = log.write(
            call_line,
            LineBody::ToolResult(ToolResultBody {
                call_id: wire.id.clone(),
                outcome: word,
                content: tools::render_result(outcome),
                truncated,
                label: result_label,
            }),
        );
        let surfaced = host
            .surface
            .as_ref()
            .map(crate::surface::SurfaceTools::take_lines)
            .unwrap_or_default();
        for line in surfaced {
            log.write(call_line, LineBody::Surface(line));
        }
        for line in host.delegation.take_lines() {
            log.write(call_line, line);
        }
        if let Some((label, path)) = read {
            let joined = log.context.label.join(&label);
            if joined != log.context.label {
                log.write(
                    None,
                    LineBody::Label(LabelBody::new(
                        &joined,
                        LabelCause {
                            kind: LabelCauseKind::DriveRead,
                            reference: path,
                        },
                    )),
                );
            }
            log.progress.reads += 1;
        }
        log.progress.calls += 1;
        // The room's gate checks the label as it is now from the next send.
        board.relabel(&log.context.label);
        board.update(log.progress);
    };
    let mut report = |record: &ToolCallRecord, wire: &chat::ToolCall, outcome: &ToolOutcome| {
        let mut log = lock();
        if log.round_line.is_none() {
            let body = LineBody::Assistant(AssistantBody {
                text: log.round_text.clone(),
                model: log.model.clone(),
                finish: ROUND_FINISH.to_owned(),
                usage: log.round_usage,
                ttft_ms: None,
                duration_ms: 0,
                anchor_event: None,
            });
            log.round_line = log.write(None, body);
        }
        let parent = log.round_line;
        let call_line = log.write(
            parent,
            LineBody::ToolCall(ToolCallBody {
                call_id: wire.id.clone(),
                tool: wire.name.clone(),
                args: wire.arguments_raw.clone(),
                tier: host.tier_of(&wire.id),
                grant_id: None,
            }),
        );
        // A parked call has no result until a person decided (R73).
        if let ToolOutcome::Parked { .. } = outcome {
            log.parked = Some((call_line, wire.clone()));
            return;
        }
        finish(&mut log, call_line, record, wire, outcome);
    };

    // A turn resuming at a parked call (93.2 AC6): that call's result —
    // run, its approval consumed, or refused — then the round's later calls
    // in order, each run once, then the model reads them all.
    let mut reparked = false;
    // The round's calls after a parked call, as the model sent them.
    let mut parked_rest: Vec<(Ulid, chat::ToolCall)> = Vec::new();
    if let Some(resume) = resume {
        let default_profile = armed.default_profile_id.clone();
        let refused = |wire: &chat::ToolCall, reason: &str| {
            (
                ToolCallRecord {
                    id: wire.id.clone(),
                    requested_name: wire.name.clone(),
                    name: ToolName::from_wire(&wire.name),
                    display_path: None,
                    refusal: Some(reason.to_owned()),
                    grant_denied: false,
                },
                ToolOutcome::Refused {
                    reason: reason.to_owned(),
                },
            )
        };
        if let Some((call_line, logged)) = resume.call {
            let (call, (record, outcome)) = match resume.settled {
                // What runs is what the record bound, not the log's
                // redacted copy (R174).
                crate::approvals::Settled::Run(bound, released) => {
                    // The approval — and the flow a `declassify` one lets
                    // through — is this one execution's alone (R89).
                    let ran = host.run_bound(&bound, resume.approval, released, || {
                        tools::run_call(&host, &default_profile, &bound, &mut events)
                    });
                    let ran = match ran {
                        // A bound call never parks again (its drift is
                        // refused); were it to, it would not wait either.
                        (_, ToolOutcome::Parked { .. }) => refused(&bound, UNATTENDED_REFUSAL),
                        ran => ran,
                    };
                    (bound, ran)
                }
                crate::approvals::Settled::Refuse(reason) => {
                    let ran = refused(&logged, &reason);
                    (logged, ran)
                }
            };
            finish(&mut lock(), Some(call_line), &record, &call, &outcome);
        }
        let mut uncertain = resume.uncertain;
        let mut rest = resume.rest.into_iter();
        while let Some((line, wire)) = rest.next() {
            let (record, outcome) = if let Some(reason) = uncertain.take() {
                refused(&wire, &reason)
            } else if !resume.run_rest {
                refused(&wire, crate::approvals::NOT_RUN)
            } else if resume.unbound.contains(&wire.id) {
                refused(&wire, crate::approvals::UNBOUND)
            } else {
                tools::run_call(&host, &default_profile, &wire, &mut events)
            };
            if let ToolOutcome::Parked { .. } = outcome {
                lock().parked = Some((Some(line), wire));
                parked_rest = rest.collect();
                reparked = true;
                break;
            }
            finish(&mut lock(), Some(line), &record, &wire, &outcome);
        }
        if let (Some(note), false) = (resume.note, reparked) {
            let mut log = lock();
            log.last_line = log.write(None, LineBody::Peer(note));
        }
        // The model reads the conversation as it is now.
        let mut messages = vec![ChatMessage::text(Role::System, composed.text.clone())];
        messages.extend(lock().context.messages.iter().cloned());
        armed.request.messages = messages;
    }
    let mut gate = |_round: usize| {
        let mut log = lock();
        if log.failure.is_some() {
            return Err(BotsError::Tool {
                detail: "the session's log could not be written".to_owned(),
            });
        }
        if let SinkVerdict::Block { reason, .. } =
            check_sink(&log.context.label, &Sink::Model { local })
        {
            log.local_only = true;
            tools.sinks.refused("model", "", &deps.bot.target, &reason);
            return Err(BotsError::Tool {
                detail: LOCAL_ONLY_REFUSAL.to_owned(),
            });
        }
        if let Some(bound) = log.context.token_bound() {
            log.bound = Some(bound);
            return Err(BotsError::Tool {
                detail: bound.sentence(),
            });
        }
        Ok(())
    };

    let result = if reparked {
        None
    } else {
        Some(
            tools::run_tool_loop_gated(
                &tool_loop,
                &armed.request,
                &ChatOptions {
                    read_timeout,
                    ..ChatOptions::default()
                },
                &ToolLoopOptions {
                    max_rounds: usize::try_from(config.limits.rounds_per_turn)
                        .unwrap_or(tools::MAX_TOOL_ROUNDS)
                        .max(1),
                    ..ToolLoopOptions::default()
                },
                stop.clone(),
                &mut events,
                &mut report,
                &mut gate,
            )
            .await,
        )
    };
    // The calls a parked round leaves unrun keep their `tool_call` lines,
    // with the table's tier, until it resumes (93.2).
    if let Some(Ok(done)) = &result {
        if let Some((_, rest)) = &done.parked {
            // The tiers first: the table reads the label, which the log's
            // lock guards.
            let tiers: Vec<u8> = rest
                .iter()
                .map(|wire| host.table_tier(wire, &armed.default_profile_id))
                .collect();
            let mut log = lock();
            let parent = log.round_line;
            for (wire, tier) in rest.iter().zip(tiers) {
                let line = log.write(
                    parent,
                    LineBody::ToolCall(ToolCallBody {
                        call_id: wire.id.clone(),
                        tool: wire.name.clone(),
                        args: wire.arguments_raw.clone(),
                        tier,
                        grant_id: None,
                    }),
                );
                if let Some(line) = line {
                    parked_rest.push((line, wire.clone()));
                }
            }
        }
    }
    let parking = host.parked.lock().unwrap_or_else(|p| p.into_inner()).take();
    drop(host);
    let log = log.into_inner().unwrap_or_else(|p| p.into_inner());
    let parked = match (parking, log.parked) {
        (Some(mut parking), Some((Some(call_line), wire))) => {
            let files = crate::approvals::pin_files(&read_profiles, &parking.pins);
            // A declassification binds the blocked call as the model sent
            // it, beside what it would let through (R89).
            let args = match parking.declassify.take() {
                Some(mut args) => {
                    args["call"] = json!({"tool": wire.name, "arguments": wire.arguments_raw});
                    args
                }
                None => wire.arguments.unwrap_or(Value::Null),
            };
            Some(crate::approvals::ParkedTurn {
                parking,
                call_line,
                args,
                preconditions: keeper_core::agents::approval::Preconditions {
                    files,
                    ..Default::default()
                },
                rest: parked_rest,
            })
        }
        _ => None,
    };
    let parent = log.last_line;
    let round_logged = log.round_line.is_some();
    let round_text = log.round_text;
    let prompt_sha256 = Some(composed.prompt_sha256);
    let bound = log.bound;
    let waits = parked.is_some();
    let ran =
        move |ending: TurnEnding, outcome: Option<chat::ChatOutcome>, error: Option<String>| Ran {
            ending,
            outcome,
            round_text,
            round_logged,
            parent,
            prompt_sha256,
            error,
            bound,
            parked,
        };
    if let Some(error) = log.failure {
        return ran(TurnEnding::Failed, None, Some(error.to_string()));
    }
    if waits {
        return ran(TurnEnding::Parked, None, None);
    }
    match result {
        None => ran(
            TurnEnding::Failed,
            None,
            Some("a parked call was lost".to_owned()),
        ),
        Some(Ok(done)) => {
            if stop.is_cancelled() {
                ran(TurnEnding::Stopped, Some(done.final_outcome), None)
            } else if done.final_outcome.finish_reason == chat::FinishReason::Failed {
                // A stream that broke hands back what had arrived; the turn
                // still failed.
                let error = log.broken.unwrap_or_else(|| TURN_FAILED.to_owned());
                ran(TurnEnding::Failed, Some(done.final_outcome), Some(error))
            } else {
                ran(TurnEnding::Complete, Some(done.final_outcome), None)
            }
        }
        Some(Err(_)) if log.local_only => ran(TurnEnding::LocalOnly, None, None),
        Some(Err(_)) if bound.is_some() => ran(TurnEnding::Bounded, None, None),
        Some(Err(error)) => ran(TurnEnding::Failed, None, Some(error.to_string())),
    }
}

/// The label a drive call's result carries, with the path it read, when the
/// call read a drive: 89.4's `label_drive_read` over the drive's declaration,
/// an author this host cannot name (DW-431) and the file's own facts — its
/// OKF keys and its `integrity:` mark, read from the head of the file on the
/// disk, so a ranged read past the frontmatter labels as a whole read does
/// (R119). A search joins the label of every file it returned a line of.
fn read_label(
    deps: &AgentDeps,
    profiles: &[keeper_sync::SyncProfile],
    record: &ToolCallRecord,
    outcome: &ToolOutcome,
) -> Option<(Label, String)> {
    let read = matches!(
        outcome,
        ToolOutcome::Text { .. }
            | ToolOutcome::Entries { .. }
            | ToolOutcome::NotMaterialized { .. }
    );
    if !read || record.name.map(ToolName::effect) != Some(keeper_core::bots::grant::Effect::Read) {
        return None;
    }
    let display = record.display_path.as_deref()?;
    let (drive, path) = display.split_once('/').unwrap_or((display, ""));
    let decl = deps.drives.get(drive)?;
    let label = drive_read_label(decl, profiles, drive, path, record.name, outcome);
    Some((label, display.to_owned()))
}

/// [`read_label`] once the drive is known: what the call at `path` of
/// `drive` returned is labelled by the files it came from.
fn drive_read_label(
    decl: &DriveDecl,
    profiles: &[keeper_sync::SyncProfile],
    drive: &str,
    path: &str,
    tool: Option<ToolName>,
    outcome: &ToolOutcome,
) -> Label {
    let label_of = |path: &str, text: &str| {
        let okf = okf_label_facts(text);
        label_drive_read(
            decl,
            &ReadFacts {
                path: path.to_owned(),
                last_author: Author::Unknown,
                okf_human_reviewed: okf.human_reviewed,
                okf_external_source: okf.external_source,
                card_untrusted: keeper_core::agents::card::marked_untrusted(text),
            },
        )
    };
    match (tool, outcome) {
        (Some(ToolName::Grep), ToolOutcome::Text { body, .. }) => grep_sources(body)
            .into_iter()
            .filter_map(|candidates| {
                candidates
                    .into_iter()
                    .find_map(|hit| file_head(profiles, drive, hit).map(|head| (hit, head)))
            })
            .fold(label_of(path, ""), |label, (hit, head)| {
                label.join(&label_of(hit, &head))
            }),
        (_, ToolOutcome::Text { body, .. }) => label_of(
            path,
            &file_head(profiles, drive, path).unwrap_or_else(|| body.clone()),
        ),
        _ => label_of(path, ""),
    }
}

/// Join what the prompt carries beyond the conversation into the label,
/// writing a `label` line for each source that changed it (R168):
/// the home's frozen files — soul, facts, memory — under the home drive's
/// declaration, and each context file arming loaded, under its drive's
/// declaration and its own facts, as a read of it would be. A context file
/// of a drive this host holds no declaration for is left out of the prompt.
fn join_prompt_sources(
    context: &mut SessionContext,
    writer: &mut SessionWriter,
    deps: &AgentDeps,
    armed: &mut crate::turn::Armed,
) -> Result<(), WriterError> {
    let home = &deps.home.drive;
    let mut sources = vec![(
        Label {
            readers: Readers::Only(home.readers.clone()),
            integrity: Integrity::Owner,
            local_only: home.local_only,
        },
        format!("{}/{}", deps.home.config.drive, deps.home.config.id),
    )];
    if let Some(bundle) = armed.context.as_mut() {
        let profiles = &armed.profiles;
        bundle.files.retain(|file| {
            let (drive, path) = file.subpath.split_once('/').unwrap_or((&file.subpath, ""));
            let Some(decl) = deps.drives.get(drive) else {
                tracing::info!(file = %file.subpath, "agents: a context file of a drive with no declaration is left out");
                return false;
            };
            let head = file_head(profiles, drive, path).unwrap_or_else(|| file.text.clone());
            let okf = okf_label_facts(&head);
            let label = label_drive_read(
                decl,
                &ReadFacts {
                    path: path.to_owned(),
                    last_author: Author::Unknown,
                    okf_human_reviewed: okf.human_reviewed,
                    okf_external_source: okf.external_source,
                    card_untrusted: keeper_core::agents::card::marked_untrusted(&head),
                },
            );
            sources.push((label, file.subpath.clone()));
            true
        });
        bundle.total_bytes = bundle
            .files
            .iter()
            .map(|file| usize::try_from(file.bytes).unwrap_or(usize::MAX))
            .sum();
    }
    for (label, reference) in sources {
        let joined = context.label.join(&label);
        if joined != context.label {
            writer.write(
                context,
                None,
                None,
                LineBody::Label(LabelBody::new(
                    &joined,
                    LabelCause {
                        kind: LabelCauseKind::DriveRead,
                        reference,
                    },
                )),
            )?;
        }
    }
    Ok(())
}

/// How much of a file its read's label is read from: where its frontmatter is.
const LABEL_HEAD_BYTES: u64 = 64 * 1024;

/// The first [`LABEL_HEAD_BYTES`] of `path` in drive `drive`, resolved by
/// keeper-sync, or `None` when it is not a file there.
fn file_head(profiles: &[keeper_sync::SyncProfile], drive: &str, path: &str) -> Option<String> {
    use std::io::Read;
    let profile = profiles.iter().find(|profile| profile.id == drive)?;
    let file = keeper_sync::browse::resolve(&profile.local_path, path).ok()??;
    let mut head = Vec::new();
    std::fs::File::open(file)
        .ok()?
        .take(LABEL_HEAD_BYTES)
        .read_to_end(&mut head)
        .ok()?;
    Some(String::from_utf8_lossy(&head).into_owned())
}

/// The files a `drive_grep` result names, one entry per hit line: every
/// prefix of the line that `<line number>: ` follows, since a file name may
/// itself hold `:<digits>: `. The first that is a file is the hit's file.
fn grep_sources(body: &str) -> Vec<Vec<&str>> {
    let mut seen = std::collections::BTreeSet::new();
    body.lines()
        .map(|line| {
            line.match_indices(':')
                .filter_map(|(at, _)| {
                    let rest = &line[at + 1..];
                    let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
                    (digits > 0 && rest[digits..].starts_with(": ")).then(|| &line[..at])
                })
                .collect::<Vec<_>>()
        })
        .filter(|candidates| !candidates.is_empty() && seen.insert(candidates.clone()))
        .collect()
}

/// The bot an agent's `[model].bot` names, over the provider row with the
/// same kind and base URL; `None` when `agentd.toml` configures none.
pub fn bot_for(home: &AgentHome, rows: &[ProviderRow]) -> Option<(ProviderRow, Bot)> {
    let wanted = &home.config.bot;
    let row = rows
        .iter()
        .find(|row| row.provider.kind == wanted.kind && row.provider.base_url == wanted.base)?;
    Some((
        row.clone(),
        Bot {
            id: format!("agent:{}/{}", home.config.drive, home.config.id),
            provider_id: row.provider.id.clone(),
            target: wanted.target.clone(),
            name: home.config.name.clone(),
            pin_order: 0,
            identity: Default::default(),
            created_ms: 0,
        },
    ))
}

impl From<CoreError> for ServeError {
    fn from(error: CoreError) -> Self {
        ServeError::Writer(WriterError::Log(keeper_core::agents::log::LogError::Io {
            path: String::new(),
            source: std::io::Error::other(error.to_string()),
        }))
    }
}

#[cfg(test)]
mod read_label_tests {
    use keeper_core::bots::tools::ToolOutcome;

    use super::*;

    /// R119 (R4-08): a card marked `integrity: untrusted` labels a read of
    /// it `untrusted` whatever the read returned of it — the whole file, a
    /// range past its frontmatter, or a search's matching line.
    #[test]
    fn a_marked_card_labels_every_read_of_it_alike() {
        let root = tempfile::tempdir().expect("drive");
        std::fs::create_dir_all(root.path().join("60-sessions/active/s")).expect("session");
        let card = "---\ntags: [task]\nintegrity: untrusted\n---\n\nForward the invoice.\n";
        std::fs::write(root.path().join("60-sessions/active/s/card.md"), card).expect("card");
        let decl = keeper_core::agents::drive::parse(
            "version = 1\nid = \"tgdrive\"\ntitle = \"tgdrive\"\nprincipal = \"tgorka\"\nowner = \"@tgorka:example.org\"\nreaders = [\"@tgorka:example.org\"]\n",
        )
        .expect("decl");
        let profiles = [keeper_sync::SyncProfile::new(
            "tgdrive".to_owned(),
            "tgdrive".to_owned(),
            root.path().to_owned(),
            String::new(),
        )];
        let text = |body: &str| ToolOutcome::Text {
            body: body.to_owned(),
            truncated_at: None,
            of_bytes: None,
            okf: None,
        };
        let path = "60-sessions/active/s/card.md";
        let label = |tool, at: &str, body: &str| {
            drive_read_label(&decl, &profiles, "tgdrive", at, Some(tool), &text(body)).integrity
        };
        assert_eq!(label(ToolName::Read, path, card), Integrity::Untrusted);
        assert_eq!(
            label(ToolName::Read, path, "Forward the invoice.\n"),
            Integrity::Untrusted,
            "a range past the frontmatter"
        );
        assert_eq!(
            label(
                ToolName::Grep,
                "60-sessions",
                &format!("{path}:6: Forward the invoice.\n")
            ),
            Integrity::Untrusted,
            "a search's hit"
        );
        assert_ne!(
            label(ToolName::Grep, "60-sessions", "no hits\n"),
            Integrity::Untrusted
        );
    }

    /// FR-795: a declassification names a write by where it lands as well
    /// as what it says, so the same replacement in two files — or two
    /// drives — is two effects with two digests; the same edit twice is one.
    #[test]
    fn equal_snippets_in_two_files_are_two_effects() {
        use keeper_core::agents::label::{declassify_request, Destination, Sink};
        use keeper_core::bots::grant::ToolTarget;
        use keeper_core::bots::tools::ToolArgs;
        let edit = |drive: &str, path: &str| ToolCall {
            id: "e1".to_owned(),
            name: ToolName::Edit,
            target: ToolTarget {
                profile_id: drive.to_owned(),
                subpath: path.to_owned(),
            },
            args: ToolArgs {
                old_text: Some("draft".to_owned()),
                new_text: Some("sell in March".to_owned()),
                ..ToolArgs::default()
            },
        };
        let digest = |call: &ToolCall| {
            declassify_request(
                write_effect(call).as_bytes(),
                Some(&call.target.subpath),
                &Destination::Drive {
                    drive: call.target.profile_id.clone(),
                    path: call.target.subpath.clone(),
                },
                &Sink::DriveWrite {
                    drive_readers: Readers::Anyone,
                },
                &Label::top(),
                &|_| None,
            )
            .effect_sha256
        };
        let plan = digest(&edit("tgdrive", "notes/plan.md"));
        assert_eq!(plan, digest(&edit("tgdrive", "notes/plan.md")));
        assert_ne!(plan, digest(&edit("tgdrive", "notes/other.md")));
        assert_ne!(plan, digest(&edit("neuradrive", "notes/plan.md")));
    }
}
