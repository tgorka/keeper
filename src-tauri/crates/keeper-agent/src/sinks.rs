//! Every place a session's work goes is a sink (AD-391, story 92.6): an
//! answer, a status edit, a scope echo, a notice, a room and its invites, a
//! delegation, a reply, a drive write, a session file, a model round. Each
//! asks `check_sink` before it acts; a block sends and writes nothing, and
//! leaves an audit row naming the sink (R65) — beside the `tool_result`
//! line a tool call's reporter writes `refused`; a send of the host's own
//! has the row alone.
//!
//! A room is checked as it is at the send (R160, R168): its joined and
//! invited members, a known agent through its own audience and anyone else
//! as a person, whatever their power ([`room_audience`], [`RoomGate`]).
//!
//! A block is what a person could let through (FR-795): the request is
//! computed here — the blocked effect's digest, its destination, the sink,
//! the label's readers and each one's proxy DM — and, until approvals exist
//! (epic 93), refused with [`NEEDS_APPROVAL`].

use std::collections::BTreeSet;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, RwLock};

use keeper_core::agents::home::{AgentConfig, AgentKind};
use keeper_core::agents::label::{
    check_sink, declassify_request, Destination, Label, Sink, SinkVerdict, NEEDS_APPROVAL,
};
use keeper_core::agents::matrix::{AgentClient, AgentMatrixError};
use keeper_core::agents::room::room_members;
use keeper_core::agents::seed::main_session_id;
use keeper_core::agents::session::parse_session_agent_toml;
use keeper_core::bots::audit::{self, AuditIntent, AuditOutcome};
use keeper_core::bots::grant::{Effect, GrantVerdict, ToolTarget};
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, UserId};
use serde_json::Value;

use crate::delegate::MembersFuture;
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
        let label = self.label();
        let Some((sinks, room)) = &self.audit else {
            return self.check(&label).await;
        };
        let destination = Destination::Room { room: room.clone() };
        match self.audience().await {
            Ok(sink) => sinks.check(tool, &destination, &label, &sink, effect, None),
            Err(unread) => {
                sinks.refused(tool, "", room.as_str(), &unread);
                Err(unread)
            }
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
        let SinkVerdict::Block { reason, .. } = check_sink(label, sink) else {
            return Ok(());
        };
        self.block(tool, destination, label, sink, effect, artifact, &reason)
    }

    /// A flow to `destination` already found beyond `label` for `reason`:
    /// its audit row and declassification, and the sentence said instead.
    #[allow(clippy::too_many_arguments)]
    pub fn block(
        &self,
        tool: &str,
        destination: &Destination,
        label: &Label,
        sink: &Sink,
        effect: &[u8],
        artifact: Option<&str>,
        reason: &str,
    ) -> Result<(), String> {
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
            "agents: a flow beyond the label was refused; letting it through needs an approval"
        );
        let (drive, at) = destination.target();
        self.refused(tool, drive, at, &sentence);
        Err(sentence)
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

/// The room of the `main` session of agent `id` homed in `drive`, whose
/// sessions zone is `zone`, under its derived id.
fn main_dm(zone: &std::path::Path, drive: &str, id: &str) -> Option<OwnedRoomId> {
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
    /// Send `content` as an `m.room.message` into `person`'s proxy DM, as
    /// the proxy.
    fn tell<'a>(&'a self, person: &'a UserId, content: Value) -> SendFuture<'a>;
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

    fn tell<'a>(&'a self, person: &'a UserId, content: Value) -> SendFuture<'a> {
        Box::pin(async move {
            let Some((client, _, dm)) = self.door(person) else {
                return Err(AgentMatrixError::Other(
                    "this host runs no proxy of theirs".to_owned(),
                ));
            };
            client.send(&dm, "m.room.message", content, None).await
        })
    }
}
