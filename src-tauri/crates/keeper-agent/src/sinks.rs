//! Every place a session's work goes is a sink (AD-391, story 92.6): an
//! answer, a status edit, a scope echo, a notice, a room and its invites, a
//! delegation, a reply, a drive write, a session file, a model round. Each
//! asks `check_sink` before it acts; a block sends and writes nothing, and
//! leaves an audit row naming the sink (R65) — beside the `tool_result`
//! line a tool call's reporter writes `refused`; a send of the host's own
//! has the row alone. An agent's tool call has exactly one row however it
//! ends, its tier in it (R90): its block is that row ([`CallAudit`]).
//!
//! A room is checked as it is at the send (R160, R168): its joined and
//! invited members, a known agent through its own audience and anyone else
//! as a person, whatever their power ([`room_audience`], [`RoomGate`]).
//!
//! A block is what a person could let through (FR-795): the request is
//! computed here — the blocked effect's digest, its destination, the sink,
//! the label's readers and each one's proxy DM. An agent's call whose host
//! has a decision source parks on it as a `declassify` approval, decided in
//! the approvers' proxy DMs, which lets exactly those bytes through once
//! ([`Lift`], R89); anything else is refused with [`NEEDS_APPROVAL`].

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use keeper_core::agents::home::{AgentConfig, AgentKind};
use keeper_core::agents::label::{
    check_sink, declassify_request, DeclassifyRequest, Destination, Label, Sink, SinkVerdict,
    NEEDS_APPROVAL,
};
use keeper_core::agents::matrix::{AgentClient, AgentMatrixError};
use keeper_core::agents::room::room_members;
use keeper_core::agents::seed::main_session_id;
use keeper_core::agents::session::parse_session_agent_toml;
use keeper_core::agents::tier::Classification;
use keeper_core::bots::audit::{self, AuditIntent, AuditOutcome};
use keeper_core::bots::grant::{Effect, GrantVerdict, ToolTarget};
use keeper_core::bots::tools::ToolOutcome;
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, UserId};
use serde_json::Value;

use crate::delegate::MembersFuture;
use crate::host::Approval;
use crate::matrix_sink::{EditPort, SendFuture};
use crate::rooms::Known;
use crate::sessions::verbs;
use crate::turn::now_ms;

/// What a room's status says, instead of the session's title and detail,
/// once the session's label no longer reaches everyone the room was opened
/// for (UX-DR135, R64); the detail goes to the requester's proxy DM.
pub const NARROWED_STATUS: &str = "This work continues where only some of you can read it.";

/// What a send says when its room's members cannot be read: an audience no
/// one can name is not one the label reaches (R168).
pub const MEMBERS_UNREAD: &str =
    "Who is in this room could not be read, so nothing this session read was sent into it.";

/// The audience of a room whose members — joined or invited, any power —
/// are `members` (R160): each agent `known` names through its own
/// audience, everyone else as a person; `own`, the agents the room's sends
/// pass between (the session's agent, and a delegated session's requester,
/// whose session joins the label a reply carries: R94), are left out.
pub fn room_audience(
    members: BTreeSet<OwnedUserId>,
    known: Option<&Known>,
    own: &[OwnedUserId],
) -> Sink {
    let members = members
        .into_iter()
        .filter(|member| !own.contains(member))
        .collect();
    crate::doorbell::room_sink(members, known.map_or(&[][..], |known| &known.agents))
}

/// One session room's outbound boundary (R168): every send that carries
/// what the session read asks it, at that send, whether the label as it is
/// then reaches the room as it is then. Once it finds the room narrowed
/// below the label it stays so for its life — a turn's — so nothing it
/// withheld is shown later in it; each kind of send it suppressed is
/// audited once (R65).
pub struct RoomGate {
    port: Arc<dyn EditPort>,
    known: Option<Arc<Known>>,
    own: Vec<OwnedUserId>,
    label: Mutex<Label>,
    narrowed: AtomicBool,
    /// Why the room was last found narrowed.
    reason: Mutex<Option<String>>,
    /// Where a suppression is audited, and the sends already audited.
    audit: Option<(Sinks, OwnedRoomId)>,
    audited: Mutex<BTreeSet<&'static str>>,
}

impl RoomGate {
    pub fn new(
        port: Arc<dyn EditPort>,
        known: Option<Arc<Known>>,
        own: Vec<OwnedUserId>,
        label: Label,
        audit: Option<(Sinks, OwnedRoomId)>,
    ) -> RoomGate {
        RoomGate {
            port,
            known,
            own,
            label: Mutex::new(label),
            narrowed: AtomicBool::new(false),
            reason: Mutex::new(None),
            audit,
            audited: Mutex::new(BTreeSet::new()),
        }
    }

    /// The session's label is `label` from now on.
    pub fn set_label(&self, label: Label) {
        *self.label.lock().unwrap_or_else(|p| p.into_inner()) = label;
    }

    /// The session's label as the gate last heard it.
    pub fn label(&self) -> Label {
        self.label.lock().unwrap_or_else(|p| p.into_inner()).clone()
    }

    /// The room as a sink now: its members read at this call;
    /// [`MEMBERS_UNREAD`] when they cannot be.
    pub async fn audience(&self) -> Result<Sink, String> {
        let members = self
            .port
            .members()
            .await
            .map_err(|_| MEMBERS_UNREAD.to_owned())?;
        Ok(room_audience(members, self.known.as_deref(), &self.own))
    }

    /// Whether what `label` covers may go into the room as its members are
    /// now; the block's sentence when not.
    pub async fn check(&self, label: &Label) -> Result<(), String> {
        match check_sink(label, &self.audience().await?) {
            SinkVerdict::Allow => Ok(()),
            SinkVerdict::Block { reason, .. } => Err(reason),
        }
    }

    /// [`Sinks::check`] of an effect of `tool`'s going into this room under
    /// the label now, with its audit row and declassification on a block;
    /// an unreadable room is refused and audited too.
    pub async fn admit(&self, tool: &str, effect: &[u8]) -> Result<(), String> {
        self.verdict(tool, effect).await.map_err(|blocked| {
            if let Some((sinks, _)) = &self.audit {
                sinks.refused(tool, &blocked.drive, &blocked.at, &blocked.sentence);
            }
            blocked.sentence
        })
    }

    /// [`RoomGate::admit`] without its row: the block, its declassification
    /// computed, for the classified row of the call that made it (R90).
    pub async fn verdict(&self, tool: &str, effect: &[u8]) -> Result<(), Blocked> {
        let label = self.label();
        let Some((sinks, room)) = &self.audit else {
            return self.check(&label).await.map_err(|sentence| Blocked {
                drive: String::new(),
                at: String::new(),
                sentence,
                flow: None,
            });
        };
        let destination = Destination::Room { room: room.clone() };
        match self.audience().await {
            Ok(sink) => sinks.verdict(tool, &destination, &label, &sink, effect, None),
            Err(unread) => Err(Blocked {
                drive: String::new(),
                at: room.to_string(),
                sentence: unread,
                flow: None,
            }),
        }
    }

    /// Whether the session's label is narrowed below the room now.
    pub async fn narrowed(&self) -> bool {
        if self.narrowed.load(Ordering::Relaxed) {
            return true;
        }
        let label = self.label.lock().unwrap_or_else(|p| p.into_inner()).clone();
        let Err(reason) = self.check(&label).await else {
            return false;
        };
        *self.reason.lock().unwrap_or_else(|p| p.into_inner()) = Some(reason);
        self.narrowed.store(true, Ordering::Relaxed);
        true
    }

    /// Whether this gate has found the room narrowed so far.
    pub fn was_narrowed(&self) -> bool {
        self.narrowed.load(Ordering::Relaxed)
    }

    /// `tool`'s send was suppressed or replaced because the room is
    /// narrowed: its audit row (R65), once per gate.
    pub fn suppressed(&self, tool: &'static str) {
        let Some((sinks, room)) = &self.audit else {
            return;
        };
        if !self
            .audited
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(tool)
        {
            return;
        }
        let reason = self
            .reason
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
            .unwrap_or_else(|| MEMBERS_UNREAD.to_owned());
        sinks.refused(tool, "", room.as_str(), &reason);
    }
}

/// A flow refused at a sink, its declassification computed and logged:
/// where it would have gone and the sentence said instead. Whoever made the
/// flow audits it — a send of the host's own as its R65 row
/// ([`Sinks::refused`]), an agent's call in its one classified row
/// ([`CallAudit::blocked`]), which may park on it instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blocked {
    pub drive: String,
    pub at: String,
    pub sentence: String,
    /// What a person would let through, when the label blocked it; `None`
    /// for a room whose members could not be read.
    pub flow: Option<Box<DeclassifyRequest>>,
}

/// What a blocked flow of an agent's call comes to (R89).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Lifted {
    /// A consumed `declassify` approval of this call names exactly these
    /// bytes, and the sink is within the readers it names: it goes
    /// through, under that approval.
    Released(ulid::Ulid),
    /// The call a consumed approval bound would now let through something
    /// that approval did not name: it is refused with this sentence on
    /// that approval, its one row closed — never parked again.
    Drift(ulid::Ulid, String),
    /// The call waits for a person on this new `declassify` record.
    Parked(ulid::Ulid),
    /// Nobody can let it through here: it is refused.
    Refused,
}

/// Who decides whether a blocked flow of call `call` parks, goes through
/// on an approval, or is refused: the turn's tools, which know whether a
/// decision source is installed and what a resumed call was approved for.
pub trait Lift {
    /// `flow`, blocked for call `call` (whose delegation is `delegation`,
    /// when it is one: its id is part of the bytes).
    fn lift(&self, call: &str, flow: &DeclassifyRequest, delegation: Option<&str>) -> Lifted;
    /// The delegation id a resumed call `call` was approved with: the same
    /// brief, byte for byte, needs the same id.
    fn delegation(&self, call: &str) -> Option<String>;
}

/// One session's sinks: the ids its audit rows name, and what the host
/// knows to route a declassification.
#[derive(Clone)]
pub struct Sinks {
    /// Where `keeper.db` lives.
    pub data_dir: PathBuf,
    pub provider_id: String,
    pub bot_id: String,
    /// The session's id, the audit row's conversation.
    pub session_id: String,
    /// The agents of the drives this host mounts, when it knows them.
    pub known: Option<Arc<Known>>,
    /// The agent's home drive and its sessions zone: where the proxy DMs
    /// this host can name live.
    pub home_drive: String,
    pub zone: PathBuf,
    /// The proxies this host runs, each through its own client (R169).
    pub doors: Option<Arc<dyn ProxyDoors>>,
}

impl Sinks {
    /// One audit row for `tool` refused at `at` — a room or a user, or a
    /// path — of `drive` (`""` for none): `Deny` with `reason`, closed
    /// `refused` (R65). A row that cannot be written is logged; the refusal
    /// stands either way, since nothing was sent.
    pub fn refused(&self, tool: &str, drive: &str, at: &str, reason: &str) {
        let verdict = GrantVerdict::Deny {
            reason: reason.to_owned(),
        };
        let target = ToolTarget {
            profile_id: drive.to_owned(),
            subpath: at.to_owned(),
        };
        let row = audit::append_intent(
            &self.data_dir,
            &AuditIntent {
                started_ms: now_ms(),
                provider_id: &self.provider_id,
                bot_id: Some(&self.bot_id),
                session_id: &self.session_id,
                message_id: None,
                tool,
                target: &target,
                effect: Effect::Write,
                verdict: &verdict,
                classified: None,
            },
        )
        .and_then(|id| {
            audit::complete(
                &self.data_dir,
                id,
                AuditOutcome::Refused,
                None,
                false,
                now_ms(),
            )
        });
        if let Err(error) = row {
            tracing::warn!(%error, tool, "agents: a refused sink's audit row could not be written");
        }
    }

    /// The one audit row of a helper's step refused as no read (R203):
    /// `Deny` with `reason`, closed `refused`, its message the outer
    /// `helper` call, classified where `tool` has a row of the tier table.
    /// Nothing ran, so a row that cannot be written is logged only.
    pub fn helper_refused(
        &self,
        tool: &str,
        (drive, at): (&str, &str),
        classification: Option<&Classification>,
        reason: &str,
        helper: &str,
    ) {
        let verdict = GrantVerdict::Deny {
            reason: reason.to_owned(),
        };
        let target = ToolTarget {
            profile_id: drive.to_owned(),
            subpath: at.to_owned(),
        };
        let row = audit::append_intent(
            &self.data_dir,
            &AuditIntent {
                started_ms: now_ms(),
                provider_id: &self.provider_id,
                bot_id: Some(&self.bot_id),
                session_id: &self.session_id,
                message_id: Some(helper),
                tool,
                target: &target,
                effect: Effect::Write,
                verdict: &verdict,
                classified: classification,
            },
        )
        .and_then(|id| {
            audit::complete(
                &self.data_dir,
                id,
                AuditOutcome::Refused,
                None,
                false,
                now_ms(),
            )
        });
        if let Err(error) = row {
            tracing::warn!(%error, tool, "agents: a helper's refused step's audit row could not be written");
        }
    }

    /// The audit row of an agent's call no grant answers (R90), written
    /// before any effect: `Allow` under `agent:<tool>`, or `Deny` with
    /// `refusal`, closed `refused` at once; either carries the call's
    /// tier. `Err` is the sentence the call is refused with when the row
    /// cannot be written: an unauditable effect is not performed (NFR-47).
    fn classified(
        &self,
        tool: &str,
        drive: &str,
        at: &str,
        effect: Effect,
        classification: &Classification,
        refusal: Option<&str>,
    ) -> Result<i64, String> {
        let verdict = match refusal {
            Some(reason) => GrantVerdict::Deny {
                reason: reason.to_owned(),
            },
            None => GrantVerdict::Allow {
                grant_id: format!("agent:{tool}"),
            },
        };
        let target = ToolTarget {
            profile_id: drive.to_owned(),
            subpath: at.to_owned(),
        };
        let row = audit::append_intent(
            &self.data_dir,
            &AuditIntent {
                started_ms: now_ms(),
                provider_id: &self.provider_id,
                bot_id: Some(&self.bot_id),
                session_id: &self.session_id,
                message_id: None,
                tool,
                target: &target,
                effect,
                verdict: &verdict,
                classified: Some(classification),
            },
        )
        .map_err(|error| {
            format!("keeper could not record this tool call, so it did not run: {error}")
        })?;
        if refusal.is_some() {
            self.close(row, AuditOutcome::Refused);
        }
        Ok(row)
    }

    /// Close a row [`Sinks::classified`] opened with what became of the call.
    fn close(&self, row: i64, outcome: AuditOutcome) {
        if let Err(error) = audit::complete(&self.data_dir, row, outcome, None, false, now_ms()) {
            tracing::warn!(%error, "agents: a tool call's audit row could not be closed");
        }
    }

    /// Whether the effect whose canonical bytes are `effect` (the file
    /// `artifact`, when it is one), labelled `label`, may go to `sink` for
    /// `tool` at `destination`. A block writes the audit row, computes the
    /// declassification and answers the sentence the caller says instead:
    /// the block's reason, then [`NEEDS_APPROVAL`].
    pub fn check(
        &self,
        tool: &str,
        destination: &Destination,
        label: &Label,
        sink: &Sink,
        effect: &[u8],
        artifact: Option<&str>,
    ) -> Result<(), String> {
        self.verdict(tool, destination, label, sink, effect, artifact)
            .map_err(|blocked| {
                self.refused(tool, &blocked.drive, &blocked.at, &blocked.sentence);
                blocked.sentence
            })
    }

    /// [`Sinks::check`] without its row: an agent's call audits its block in
    /// its own classified row ([`CallAudit::blocked`]).
    pub fn verdict(
        &self,
        tool: &str,
        destination: &Destination,
        label: &Label,
        sink: &Sink,
        effect: &[u8],
        artifact: Option<&str>,
    ) -> Result<(), Blocked> {
        let SinkVerdict::Block { reason, .. } = check_sink(label, sink) else {
            return Ok(());
        };
        let sentence = format!("{reason} {NEEDS_APPROVAL}");
        let request = declassify_request(effect, artifact, destination, sink, label, &|person| {
            self.proxy_dm(person)
        });
        tracing::info!(
            tool,
            session = %self.session_id,
            sha256 = %request.effect_sha256,
            destination = ?request.destination,
            artifact = ?request.artifact,
            route = ?request.route,
            "agents: a flow beyond the label was blocked; letting it through needs an approval"
        );
        let (drive, at) = destination.target();
        Err(Blocked {
            drive: drive.to_owned(),
            at: at.to_owned(),
            sentence,
            flow: Some(Box::new(request)),
        })
    }

    /// `person`'s proxy DM, by this host's own lookup: through the proxy
    /// this host runs for them, or the `main` session, in the home drive's
    /// zone, of a known proxy whose person they are.
    pub fn proxy_dm(&self, person: &UserId) -> Option<OwnedRoomId> {
        if let Some(dm) = self.doors.as_ref().and_then(|doors| doors.dm(person)) {
            return Some(dm);
        }
        let known = self.known.as_ref()?;
        known
            .agents
            .iter()
            .filter(|agent| {
                agent.kind == AgentKind::Proxy
                    && agent.drive == self.home_drive
                    && agent.human.as_deref() == Some(person)
            })
            .find_map(|proxy| main_dm(&self.zone, &proxy.drive, &proxy.id))
    }
}

/// What an audit row closes with for `outcome`.
pub fn audit_outcome(outcome: &ToolOutcome) -> AuditOutcome {
    match outcome {
        ToolOutcome::Refused { .. } => AuditOutcome::Refused,
        _ => AuditOutcome::Ok,
    }
}

/// Where a [`CallAudit`]'s row is.
#[derive(Debug, Clone, Copy)]
enum CallRow {
    Unwritten,
    Open(i64),
    Closed,
}

/// What a classified call comes to once its sinks have passed (R82).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Gated {
    /// It runs; on this consumed approval, when one let it.
    Run(Option<Approval>),
    /// It waits for a person on this approval record (R73).
    Park(ulid::Ulid),
    /// It does not run and says this: an integrity block, T5, or an
    /// approval nobody can give.
    Refuse(String),
}

impl Gated {
    /// The sentence it is refused with, and the record it parks on or was
    /// approved by.
    pub fn split(self) -> (Option<String>, Option<Approval>) {
        match self {
            Gated::Run(approval) => (None, approval),
            Gated::Park(approval) => (None, Some(Approval::Park(approval))),
            Gated::Refuse(reason) => (Some(reason), None),
        }
    }
}

/// What a call its audit did not admit answers instead of running.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Withheld {
    /// It is refused with this sentence.
    Refused(String),
    /// It waits for a person on this approval record.
    Parked(ulid::Ulid),
}

impl From<Withheld> for ToolOutcome {
    fn from(withheld: Withheld) -> ToolOutcome {
        match withheld {
            Withheld::Refused(reason) => ToolOutcome::Refused { reason },
            Withheld::Parked(approval) => ToolOutcome::Parked { approval },
        }
    }
}

/// One classified agent call's audit row (R90): exactly one, whichever
/// way the call ends. A sink that blocks it writes the row with the
/// block's sentence and destination ([`CallAudit::blocked`]); a call whose
/// sinks passed is admitted, its row written before any effect with what
/// the integrity rule and the tier answer then (R82, [`CallAudit::admit`]);
/// a call that ends before either has its row written as it ends
/// ([`CallAudit::finish`]) — it had no effect. A parked call's row waits,
/// pending and marked with its approval, for the run after approval, which
/// closes that same row (R172).
pub struct CallAudit<'s> {
    sinks: &'s Sinks,
    tool: &'s str,
    effect: Effect,
    classification: &'s Classification,
    /// What the call answers once its sinks have passed: an integrity
    /// block, T5, an approval nobody can give; `None` runs it.
    refusal: Option<String>,
    /// The record it parks on, or the consumed one it runs on — a
    /// `declassify` approval that released its flow included.
    approval: Mutex<Option<Approval>>,
    /// Where the row says the call went until the call names it.
    drive: String,
    at: String,
    row: Mutex<CallRow>,
    /// Who decides on a flow its sinks block, and the call's wire id.
    lift: Option<(&'s dyn Lift, &'s str)>,
    /// The delegation id the call's bytes carry, when it is one.
    delegation: Mutex<Option<String>>,
}

impl<'s> CallAudit<'s> {
    pub fn new(
        sinks: &'s Sinks,
        tool: &'s str,
        effect: Effect,
        classification: &'s Classification,
        gated: Gated,
        (drive, at): (&str, &str),
    ) -> CallAudit<'s> {
        let (refusal, approval) = gated.split();
        CallAudit {
            sinks,
            tool,
            effect,
            classification,
            refusal,
            approval: Mutex::new(approval),
            drive: drive.to_owned(),
            at: at.to_owned(),
            row: Mutex::new(CallRow::Unwritten),
            lift: None,
            delegation: Mutex::new(None),
        }
    }

    /// The same audit, a flow its sinks block decided by `lift` for the
    /// call `call` (R89).
    pub fn lifting(self, lift: &'s dyn Lift, call: &'s str) -> CallAudit<'s> {
        CallAudit {
            lift: Some((lift, call)),
            ..self
        }
    }

    /// The delegation id this call sends under: the one a resumed call was
    /// approved with, so its brief is the same bytes; else `fresh`.
    pub fn delegation_id(&self, fresh: String) -> String {
        let id = self
            .lift
            .and_then(|(lift, call)| lift.delegation(call))
            .unwrap_or(fresh);
        *self.delegation.lock().unwrap_or_else(|p| p.into_inner()) = Some(id.clone());
        id
    }

    fn row(&self) -> std::sync::MutexGuard<'_, CallRow> {
        self.row.lock().unwrap_or_else(|p| p.into_inner())
    }

    fn approval(&self) -> Option<Approval> {
        *self.approval.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// A sink blocked the call. A `declassify` approval consumed for exactly
    /// these bytes and this audience lets it go on (`Ok`), under that
    /// approval; an approved call whose flow moved since is refused on its
    /// approval, its parked row closed; a host with a decision source parks
    /// it on a new one, its row pending and marked (R89); else its row is
    /// `Deny` with the block's sentence at the block's destination, and the
    /// call says that sentence.
    pub fn blocked(&self, blocked: Blocked) -> Result<(), Withheld> {
        let lifted = match (self.lift, &blocked.flow) {
            (Some((lift, call)), Some(flow)) => {
                let delegation = self
                    .delegation
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .clone();
                lift.lift(call, flow, delegation.as_deref())
            }
            _ => Lifted::Refused,
        };
        let mut row = self.row();
        let sentence = match lifted {
            Lifted::Released(approval) => {
                *self.approval.lock().unwrap_or_else(|p| p.into_inner()) =
                    Some(Approval::Approved(approval));
                return Ok(());
            }
            Lifted::Drift(approval, sentence) => {
                *self.approval.lock().unwrap_or_else(|p| p.into_inner()) =
                    Some(Approval::Approved(approval));
                sentence
            }
            Lifted::Parked(approval) if matches!(*row, CallRow::Unwritten) => {
                *row = CallRow::Closed;
                let written = self
                    .sinks
                    .classified(
                        self.tool,
                        &blocked.drive,
                        &blocked.at,
                        self.effect,
                        self.classification,
                        None,
                    )
                    .and_then(|id| {
                        audit::mark_approval(&self.sinks.data_dir, id, &approval.to_string())
                            .map_err(|error| error.to_string())
                    });
                return match written {
                    Ok(_) => Err(Withheld::Parked(approval)),
                    // Unauditable, it does not wait either (NFR-47).
                    Err(error) => Err(Withheld::Refused(format!(
                        "keeper could not record this tool call, so it did not run: {error}"
                    ))),
                };
            }
            _ => blocked.sentence,
        };
        if matches!(*row, CallRow::Unwritten) {
            if let Err(error) = self.refused_row(&blocked.drive, &blocked.at, &sentence) {
                tracing::warn!(%error, tool = self.tool, "agents: a blocked call's audit row could not be written");
            }
            *row = CallRow::Closed;
        }
        Err(Withheld::Refused(sentence))
    }

    /// The row of a call refused before its sinks admitted it, closed
    /// refused: an approved call's is the one its park left pending here,
    /// or — on a host that took over — a new one carrying the approval, so
    /// the call keeps one row whichever way it ends (R172).
    fn refused_row(&self, drive: &str, at: &str, sentence: &str) -> Result<(), String> {
        let Some(Approval::Approved(approval)) = self.approval() else {
            return self
                .sinks
                .classified(
                    self.tool,
                    drive,
                    at,
                    self.effect,
                    self.classification,
                    Some(sentence),
                )
                .map(drop);
        };
        let approval = approval.to_string();
        if let Some(row) = audit::parked_row(&self.sinks.data_dir, &approval).unwrap_or(None) {
            self.sinks.close(row, AuditOutcome::Refused);
            return Ok(());
        }
        let row = self.sinks.classified(
            self.tool,
            drive,
            at,
            self.effect,
            self.classification,
            Some(sentence),
        )?;
        audit::mark_approval(&self.sinks.data_dir, row, &approval)
            .map(drop)
            .map_err(|error| error.to_string())
    }

    /// The call's sinks passed for the effect at `drive` and `at`: its row,
    /// before the effect — the one its park left pending on this machine
    /// when it runs approved, else a new one. `Err` is what the call
    /// answers instead: the integrity rule's or the tier's sentence, a row
    /// that could not be written, or its park.
    pub fn admit(&self, drive: &str, at: &str) -> Result<(), Withheld> {
        let mut row = self.row();
        *row = CallRow::Closed;
        let approval = self.approval();
        let parked = match approval {
            Some(Approval::Approved(id)) => {
                audit::parked_row(&self.sinks.data_dir, &id.to_string()).unwrap_or(None)
            }
            _ => None,
        };
        let id = match parked {
            Some(id) => id,
            None => self
                .sinks
                .classified(
                    self.tool,
                    drive,
                    at,
                    self.effect,
                    self.classification,
                    self.refusal.as_deref(),
                )
                .map_err(Withheld::Refused)?,
        };
        if let Some(sentence) = &self.refusal {
            return Err(Withheld::Refused(sentence.clone()));
        }
        if let Some(approval) = approval {
            audit::mark_approval(&self.sinks.data_dir, id, &approval.id()).map_err(|error| {
                Withheld::Refused(format!(
                    "keeper could not record this tool call, so it did not run: {error}"
                ))
            })?;
        }
        // A parked call's row waits, pending, for its run after approval.
        if let Some(Approval::Park(approval)) = approval {
            return Err(Withheld::Parked(approval));
        }
        *row = CallRow::Open(id);
        Ok(())
    }

    /// The call ended with `outcome`: its admitted row closed with it, or —
    /// a call that ended before its sinks were asked — its row written now.
    pub fn finish(&self, outcome: &ToolOutcome) {
        let mut row = self.row();
        match *row {
            CallRow::Open(id) => self.sinks.close(id, audit_outcome(outcome)),
            CallRow::Closed => {}
            CallRow::Unwritten => {
                let written = match outcome {
                    ToolOutcome::Refused { reason } => {
                        self.refused_row(&self.drive, &self.at, reason)
                    }
                    _ => self
                        .sinks
                        .classified(
                            self.tool,
                            &self.drive,
                            &self.at,
                            self.effect,
                            self.classification,
                            None,
                        )
                        .map(|id| self.sinks.close(id, audit_outcome(outcome))),
                };
                if let Err(error) = written {
                    tracing::warn!(%error, tool = self.tool, "agents: a call's audit row could not be written");
                }
            }
        }
        *row = CallRow::Closed;
    }

    /// Refuse the call with `reason` before its sinks were asked.
    pub fn refuse(&self, reason: String) -> ToolOutcome {
        let outcome = ToolOutcome::Refused { reason };
        self.finish(&outcome);
        outcome
    }
}

/// The room of the `main` session of agent `id` homed in `drive`, whose
/// sessions zone is `zone`, under its derived id.
pub(crate) fn main_dm(zone: &std::path::Path, drive: &str, id: &str) -> Option<OwnedRoomId> {
    let session = main_session_id(drive, id).to_string();
    let row = verbs::find(zone, &session)?;
    let text = crate::zone::read_text(&zone.join(&row.path), "agent.toml").ok()??;
    Some(parse_session_agent_toml(&text).ok()?.room)
}

/// The proxies this host runs, as a session of another agent reaches its
/// person through them (R169): the person's DM with their proxy, read and
/// sent into by the proxy's own client — the session's agent is not in it.
pub trait ProxyDoors: Send + Sync {
    /// `person`'s proxy DM, when this host runs their proxy and its `main`
    /// session names the room.
    fn dm(&self, person: &UserId) -> Option<OwnedRoomId>;
    /// Who is in `person`'s proxy DM now, as the proxy's client reads it,
    /// the proxy itself left out: it is the door the detail goes through,
    /// as the session's own agent is in its room.
    fn members<'a>(&'a self, person: &'a UserId) -> MembersFuture<'a>;
    /// Send `content` as an `event_type` event into `person`'s proxy DM, as
    /// the proxy: a notice, or an approval's request (R85).
    fn tell<'a>(
        &'a self,
        person: &'a UserId,
        event_type: &'a str,
        content: Value,
    ) -> SendFuture<'a>;
    /// The decisions this host waits for in its proxies' DMs (R89).
    fn forwards(&self) -> &crate::deciding::Forwards;
}

/// One proxy this host runs.
struct Door {
    client: AgentClient,
    proxy: OwnedUserId,
    person: OwnedUserId,
    drive: String,
    id: String,
    zone: PathBuf,
}

/// [`ProxyDoors`] over the proxies' own copies: each one's client and home.
#[derive(Default)]
pub struct ClientDoors {
    proxies: RwLock<Vec<Door>>,
    forwards: crate::deciding::Forwards,
}

impl ClientDoors {
    /// The copy of the agent `config`, signed in as `client`, whose sessions
    /// zone is `zone`: a door when it is a proxy.
    pub fn add(&self, client: &AgentClient, config: &AgentConfig, zone: &std::path::Path) {
        let (AgentKind::Proxy, Some(person)) = (config.kind, config.human.as_ref()) else {
            return;
        };
        self.proxies
            .write()
            .unwrap_or_else(|p| p.into_inner())
            .push(Door {
                client: client.clone(),
                proxy: config.matrix_user.clone(),
                person: person.clone(),
                drive: config.drive.clone(),
                id: config.id.clone(),
                zone: zone.to_owned(),
            });
    }

    /// `person`'s proxy: its client, its own id and its DM.
    fn door(&self, person: &UserId) -> Option<(AgentClient, OwnedUserId, OwnedRoomId)> {
        self.proxies
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .iter()
            .filter(|door| door.person == person)
            .find_map(|door| {
                let dm = main_dm(&door.zone, &door.drive, &door.id)?;
                Some((door.client.clone(), door.proxy.clone(), dm))
            })
    }
}

impl ProxyDoors for ClientDoors {
    fn dm(&self, person: &UserId) -> Option<OwnedRoomId> {
        self.door(person).map(|(.., dm)| dm)
    }

    fn members<'a>(&'a self, person: &'a UserId) -> MembersFuture<'a> {
        Box::pin(async move {
            let (client, proxy, dm) = self
                .door(person)
                .ok_or_else(|| "this host runs no proxy of theirs".to_owned())?;
            let room = client
                .client()
                .get_room(&dm)
                .ok_or_else(|| "the proxy is not in its DM".to_owned())?;
            let mut members = room_members(&room)
                .await
                .ok_or_else(|| "the DM's members could not be read".to_owned())?;
            members.remove(&proxy);
            Ok(members)
        })
    }

    fn tell<'a>(
        &'a self,
        person: &'a UserId,
        event_type: &'a str,
        content: Value,
    ) -> SendFuture<'a> {
        Box::pin(async move {
            let Some((client, _, dm)) = self.door(person) else {
                return Err(AgentMatrixError::Other(
                    "this host runs no proxy of theirs".to_owned(),
                ));
            };
            client.send(&dm, event_type, content, None).await
        })
    }

    fn forwards(&self) -> &crate::deciding::Forwards {
        &self.forwards
    }
}
