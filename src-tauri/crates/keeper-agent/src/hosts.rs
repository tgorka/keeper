//! `HostRuntime`: what a host does each tick about the sessions it can see
//! (AD-374, AD-378, AD-379; story 90.6).
//!
//! Each tick it renews its manifest in the principal's control room (every
//! 60 s, live for 180 s), places every active session of the agents it hosts
//! over the live manifests, and acts on [`claims::step`]: acquire what it
//! wins, renew what it holds, stop writing a claim it cannot confirm, and
//! hand a session back at an idle moment when placement prefers another live
//! host (C10). A session nobody can serve shows `run: waiting` with what it
//! waits for; a conflicted session is served by no host and says so once.
//!
//! Only the claim's holder has a worker for the session's room, so a message
//! both copies receive is answered once. The tick never waits for a worker:
//! a claim handed back is released once its worker has finished, at a later
//! tick, so every other claim is renewed on time meanwhile.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keeper_core::agents::agentd::AgentdConfig;
use keeper_core::agents::claim::{self, Claimant, ServerClaim, RENEW_EVERY};
use keeper_core::agents::events::{RunState, StatusContent, CLAIM, CONTENT_VERSION, HOST, STATUS};
use keeper_core::agents::home::AgentConfig;
use keeper_core::agents::host::{accept, bot_id, HostDrive, HostManifest, Materialized};
use keeper_core::agents::log::reader::ClaimConflict;
use keeper_core::agents::log::{ClaimAction, HostSlug};
use keeper_core::agents::matrix::{AgentMatrixError, ServerState};
use keeper_core::agents::placement::{place, Ask, Placement};
use keeper_core::agents::session::SessionAgent;
use keeper_core::bots::chat::CancelSignal;
use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId, OwnedUserId, RoomId, TransactionId};
use matrix_sdk::RoomState;
use serde_json::{json, Value};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::claims::{
    self, acquire, blocked_status, bounded, conflict_of, release, renew, wall_ms, Acquired,
    ClaimFuture, ClaimPort, Lease, Moment, RoomClaims, Rtt, ServerClock, Step, REQUEST_TIMEOUT,
};
use crate::matrix_sink::{EditPort, RoomPort};
use crate::runtime::{self, spawn_worker, Claimed, Copy, DriveView};

use crate::zone::FoundSession;

/// What a holder that lost its claim parks the session's status with.
const LOST_DETAIL: &str = "This host lost its claim on the session; another host may continue it.";

/// How long a host is in the control room before it says a session waits.
const ANNOUNCE_AFTER: Duration = Duration::from_secs(10);

/// How long after a worker ended by itself (a log it could not open) its
/// session is claimed again; how often a conflicted session's log is read
/// again.
const RETRY_AFTER: Duration = Duration::from_secs(30);

/// How long a clean shutdown waits for each claim's release.
pub const RELEASE_BOUND: Duration = Duration::from_secs(10);

/// What the host runtime uses of one copy: its agent, its client in the
/// control and session rooms, its router and its workers. [`Copy`] is the
/// real one.
pub(crate) trait CopyPort: Send + Sync {
    /// The agent's `agent.toml`.
    fn config(&self) -> &AgentConfig;
    /// The principal the declaration of `drive` this host mounts names.
    fn principal_of(&self, drive: &str) -> Option<String>;
    /// The home drive's sessions folder inside the drive.
    fn sessions_subfolder(&self) -> &str;
    /// This copy's device id.
    fn device(&self) -> String;
    fn joined(&self, room: &RoomId) -> bool;
    /// Join `room` when this copy is invited to it.
    fn join_invited<'a>(&'a self, room: &'a RoomId) -> ClaimFuture<'a, ()>;
    /// Every `event_type` state event of `room` as the last sync left it.
    fn cached_states<'a>(
        &'a self,
        room: &'a RoomId,
        event_type: &'a str,
    ) -> ClaimFuture<'a, Vec<(String, ServerState)>>;
    /// Send a state event, once.
    fn send_state<'a>(
        &'a self,
        room: &'a RoomId,
        event_type: &'a str,
        state_key: &'a str,
        content: &'a Value,
    ) -> ClaimFuture<'a, Result<OwnedEventId, AgentMatrixError>>;
    /// A state event as the server holds it now.
    fn server_state<'a>(
        &'a self,
        room: &'a RoomId,
        event_type: &'a str,
        state_key: &'a str,
    ) -> ClaimFuture<'a, Result<Option<ServerState>, AgentMatrixError>>;
    /// The claim of the session room `room`.
    fn claims<'a>(&'a self, room: &'a OwnedRoomId) -> Box<dyn ClaimPort + 'a>;
    /// Send a session status into `room`.
    fn send_status<'a>(
        &'a self,
        room: &'a RoomId,
        status: Value,
    ) -> ClaimFuture<'a, Result<OwnedEventId, AgentMatrixError>>;
    /// The anchor of the latest status this copy's agent sent in `room`.
    fn latest_status<'a>(&'a self, room: &'a RoomId) -> ClaimFuture<'a, Option<OwnedEventId>>;
    /// Drop what was kept for `room`'s worker.
    fn forget(&self, room: &RoomId);
    /// Close `room`'s worker channel.
    fn close_room(&self, room: &RoomId);
    /// Start `session`'s worker under `claimed`; `None` when it cannot start.
    fn spawn_worker(
        self: Arc<Self>,
        session: &FoundSession,
        agent: &SessionAgent,
        stop: &CancelSignal,
        claimed: Claimed,
    ) -> Option<JoinHandle<()>>;
}

impl CopyPort for Copy {
    fn config(&self) -> &AgentConfig {
        &self.deps.home.config
    }

    fn principal_of(&self, drive: &str) -> Option<String> {
        self.deps
            .drives
            .get(drive)
            .map(|decl| decl.principal.clone())
    }

    fn sessions_subfolder(&self) -> &str {
        &self.deps.sessions_subfolder
    }

    fn device(&self) -> String {
        self.client.device_id().unwrap_or_default()
    }

    fn joined(&self, room: &RoomId) -> bool {
        self.client
            .client()
            .get_room(room)
            .is_some_and(|room| room.state() == RoomState::Joined)
    }

    fn join_invited<'a>(&'a self, room: &'a RoomId) -> ClaimFuture<'a, ()> {
        Box::pin(async move {
            let Some(invited) = self
                .client
                .client()
                .get_room(room)
                .filter(|r| r.state() == RoomState::Invited)
            else {
                return;
            };
            if let Err(error) = invited.join().await {
                tracing::warn!(%room, %error, "agents: could not join the control room");
            }
        })
    }

    fn cached_states<'a>(
        &'a self,
        room: &'a RoomId,
        event_type: &'a str,
    ) -> ClaimFuture<'a, Vec<(String, ServerState)>> {
        Box::pin(self.client.cached_states(room, event_type))
    }

    fn send_state<'a>(
        &'a self,
        room: &'a RoomId,
        event_type: &'a str,
        state_key: &'a str,
        content: &'a Value,
    ) -> ClaimFuture<'a, Result<OwnedEventId, AgentMatrixError>> {
        Box::pin(self.client.send_state(room, event_type, state_key, content))
    }

    fn server_state<'a>(
        &'a self,
        room: &'a RoomId,
        event_type: &'a str,
        state_key: &'a str,
    ) -> ClaimFuture<'a, Result<Option<ServerState>, AgentMatrixError>> {
        Box::pin(self.client.server_state(room, event_type, state_key))
    }

    fn claims<'a>(&'a self, room: &'a OwnedRoomId) -> Box<dyn ClaimPort + 'a> {
        Box::new(RoomClaims::new(
            self.client.clone(),
            room.clone(),
            self.syncs.clone(),
        ))
    }

    fn send_status<'a>(
        &'a self,
        room: &'a RoomId,
        status: Value,
    ) -> ClaimFuture<'a, Result<OwnedEventId, AgentMatrixError>> {
        Box::pin(async move {
            let port = RoomPort::new(self.client.clone(), room.to_owned());
            port.send(STATUS, status, TransactionId::new()).await
        })
    }

    fn latest_status<'a>(&'a self, room: &'a RoomId) -> ClaimFuture<'a, Option<OwnedEventId>> {
        Box::pin(async move {
            let room = self.client.client().get_room(room)?;
            let me = &self.deps.home.config.matrix_user;
            tokio::time::timeout(REQUEST_TIMEOUT, runtime::latest_status(&room, me))
                .await
                .ok()
                .flatten()
        })
    }

    fn forget(&self, room: &RoomId) {
        self.router.forget(room);
    }

    fn close_room(&self, room: &RoomId) {
        self.router.close_room(room);
    }

    fn spawn_worker(
        self: Arc<Self>,
        session: &FoundSession,
        agent: &SessionAgent,
        stop: &CancelSignal,
        claimed: Claimed,
    ) -> Option<JoinHandle<()>> {
        spawn_worker(&self, session, agent, stop, claimed)
    }
}

/// One session this host sees, and what it holds of it.
struct Slot {
    copy: Arc<dyn CopyPort>,
    session: FoundSession,
    agent: SessionAgent,
    claim: Option<Held>,
    /// A worker that is finishing after its claim ended.
    draining: Option<JoinHandle<()>>,
    /// A claim handed back: released once `draining` has finished.
    release_after: Option<Arc<Lease>>,
    /// What the session's status last said about waiting or a conflict.
    shown: Option<String>,
    /// The claim's `(host, epoch)` the last tick saw: when it changes, what
    /// was shown is said again.
    seen_claim: Option<(String, u64)>,
    status_anchor: Option<OwnedEventId>,
    /// Whether the room was read for the session's status anchor since this
    /// host last served it.
    anchor_read: bool,
    conflict: Option<ClaimConflict>,
    /// When the log was last read for a conflict.
    conflict_read: Option<Instant>,
    /// A worker that ended by itself is not started again before this.
    retry_at: Option<Instant>,
    /// The rescan under way offered this session.
    offered: bool,
    /// The last rescan did not find it: its claim goes back, then the slot.
    gone: bool,
}

/// A claim this host holds and the worker writing under it.
struct Held {
    lease: Arc<Lease>,
    worker: JoinHandle<()>,
    ending: Arc<Mutex<ClaimAction>>,
    busy: Arc<AtomicBool>,
}

/// A host's placement and claims over its copies.
pub struct HostRuntime {
    host: HostSlug,
    principal: String,
    always_on: bool,
    version: String,
    control_room: Option<OwnedRoomId>,
    tools: Vec<String>,
    drives: Vec<HostDrive>,
    /// The agent users whose manifests are believed (AD-374).
    principal_agents: Vec<OwnedUserId>,
    copies: Vec<Arc<dyn CopyPort>>,
    slots: HashMap<OwnedRoomId, Slot>,
    clock: ServerClock,
    rtt: Rtt,
    manifest_sent: Option<Instant>,
    /// When this host's manifest first reached the control room.
    first_published: Option<Instant>,
}

impl HostRuntime {
    /// The runtime of an agentd host over its mounted `drives` and `copies`.
    pub(crate) fn agentd(
        config: &AgentdConfig,
        host: HostSlug,
        version: &str,
        drives: &[DriveView],
        copies: Vec<Arc<Copy>>,
    ) -> HostRuntime {
        let tools = config
            .mcp
            .iter()
            .map(|mcp| format!("mcp:{}", mcp.name))
            .chain(config.kvm.iter().map(|kvm| format!("kvm:{}", kvm.id)))
            .collect();
        let manifest_drives = drives
            .iter()
            .map(|drive| HostDrive {
                id: drive.id.clone(),
                present: drive.hosts.is_ok() && drive.profile.local_path.exists(),
                // agentd checks out whole clones.
                materialized: Materialized::Full,
            })
            .collect();
        let mut principal_agents: Vec<OwnedUserId> = drives
            .iter()
            .filter(|drive| {
                drive
                    .hosts
                    .as_ref()
                    .is_ok_and(|decl| decl.principal == config.principal)
            })
            .flat_map(|drive| drive.zone.homes.iter())
            .filter_map(|(_, home)| home.as_ref().ok().map(|h| h.config.matrix_user.clone()))
            .collect();
        principal_agents.sort();
        principal_agents.dedup();
        HostRuntime {
            host,
            principal: config.principal.clone(),
            always_on: config.always_on,
            version: version.to_owned(),
            control_room: config.homeserver.control_room.clone(),
            tools,
            drives: manifest_drives,
            principal_agents,
            copies: copies
                .into_iter()
                .map(|copy| copy as Arc<dyn CopyPort>)
                .collect(),
            slots: HashMap::new(),
            clock: ServerClock::default(),
            rtt: Rtt::default(),
            manifest_sent: None,
            first_published: None,
        }
    }

    /// This host's manifest at `server_now`; `live: false` withdraws it.
    fn manifest(&self, server_now: u64, live: bool) -> HostManifest {
        let mut bots: Vec<String> = self
            .copies
            .iter()
            .map(|copy| bot_id(&copy.config().bot))
            .collect();
        bots.sort();
        bots.dedup();
        HostManifest {
            v: CONTENT_VERSION,
            host: self.host.as_str().to_owned(),
            principal: self.principal.clone(),
            version: self.version.clone(),
            always_on: self.always_on,
            tools: self.tools.clone(),
            drives: self.drives.clone(),
            bots,
            agents: self
                .copies
                .iter()
                .map(|copy| agent_name(copy.config()))
                .collect(),
            renewed_at: claim::rfc3339(server_now),
            expires_at: claim::rfc3339(if live {
                server_now.saturating_add(claim::TTL.as_millis() as u64)
            } else {
                server_now
            }),
        }
    }

    /// The copy that writes the manifest: the first one in the control room.
    fn publisher(&self) -> Option<(&Arc<dyn CopyPort>, &OwnedRoomId)> {
        let room = self.control_room.as_ref()?;
        self.copies
            .iter()
            .find(|copy| copy.joined(room))
            .map(|copy| (copy, room))
    }

    /// Write this host's manifest and read it back, which also sets the
    /// server clock; `false` when no copy is in the control room yet. Each
    /// request is bounded, so a stalled homeserver delays the tick by at
    /// most [`REQUEST_TIMEOUT`] each.
    async fn publish(&mut self, live: bool) -> Result<bool, AgentMatrixError> {
        let Some((copy, room)) = self.publisher() else {
            return Ok(false);
        };
        let content =
            serde_json::to_value(self.manifest(self.clock.now(), live)).unwrap_or(Value::Null);
        let sent = wall_ms();
        let event = bounded(copy.send_state(room, HOST, self.host.as_str(), &content)).await?;
        let answered = wall_ms();
        self.rtt
            .record(Duration::from_millis(answered.saturating_sub(sent)));
        if let Some(state) = bounded(copy.server_state(room, HOST, self.host.as_str()))
            .await?
            .filter(|state| state.event_id == event)
        {
            self.clock
                .observe(u64::from(state.origin_server_ts.get()), sent, answered);
        }
        Ok(true)
    }

    /// The live manifests this host believes, its own as it is now.
    async fn manifests(&self) -> Vec<HostManifest> {
        let mut hosts = Vec::new();
        if let Some((copy, room)) = self.publisher() {
            for (key, state) in copy.cached_states(room, HOST).await {
                if key == self.host.as_str() {
                    continue;
                }
                match accept(&key, &state.sender, &state.content, &self.principal_agents) {
                    Ok(manifest) => hosts.push(manifest),
                    Err(rejected) => {
                        tracing::debug!(host = %key, %rejected, "agents: a manifest is not believed")
                    }
                }
            }
        }
        hosts.push(self.manifest(self.clock.now(), true));
        hosts
    }

    /// Join the control room `agentd.toml` names with every copy invited to
    /// it: its id is the operator's, so the invite needs no other rule.
    async fn join_control_room(&self) {
        let Some(room) = &self.control_room else {
            return;
        };
        for copy in &self.copies {
            copy.join_invited(room).await;
        }
    }

    /// One tick: the manifest, then every session this host can see.
    pub async fn tick(&mut self, stop: &CancelSignal) {
        self.join_control_room().await;
        if self
            .manifest_sent
            .is_none_or(|at| at.elapsed() >= RENEW_EVERY)
        {
            match self.publish(true).await {
                Ok(true) => {
                    let now = Instant::now();
                    self.manifest_sent = Some(now);
                    self.first_published.get_or_insert(now);
                }
                // No copy has the control room yet: try again next tick.
                Ok(false) => {}
                Err(error) => tracing::warn!(%error, "agents: the host manifest was not renewed"),
            }
        }
        let hosts = self.manifests().await;
        let rooms: Vec<OwnedRoomId> = self.slots.keys().cloned().collect();
        for room in rooms {
            self.tick_session(&room, &hosts, stop).await;
        }
    }

    /// A session of `copy`'s agent that its rescan found serving a room:
    /// from now on it is placed every tick, with the session's `agent.toml`
    /// as this rescan read it. A room the copy has not joined is left to the
    /// invite rule (F5).
    pub(crate) fn offer<C: CopyPort + 'static>(
        &mut self,
        copy: &Arc<C>,
        session: &FoundSession,
        agent: &SessionAgent,
    ) {
        if let Some(slot) = self.slots.get_mut(&agent.room) {
            slot.session = session.clone();
            slot.agent = agent.clone();
            slot.offered = true;
            slot.gone = false;
            return;
        }
        if !copy.joined(&agent.room) {
            return;
        }
        self.slots.insert(
            agent.room.clone(),
            Slot {
                copy: Arc::clone(copy) as Arc<dyn CopyPort>,
                agent: agent.clone(),
                session: session.clone(),
                claim: None,
                draining: None,
                release_after: None,
                shown: None,
                seen_claim: None,
                status_anchor: None,
                anchor_read: false,
                conflict: None,
                conflict_read: None,
                retry_at: None,
                offered: true,
                gone: false,
            },
        );
    }

    /// The rescan offered every session it found: a session it did not
    /// offer (moved out of `active/`, its drive no longer hosting) is gone —
    /// its claim is handed back and its slot dropped.
    pub(crate) fn scanned(&mut self) {
        for slot in self.slots.values_mut() {
            slot.gone = !slot.offered;
            slot.offered = false;
        }
    }

    async fn tick_session(
        &mut self,
        room: &OwnedRoomId,
        hosts: &[HostManifest],
        stop: &CancelSignal,
    ) {
        let me = self.host.as_str().to_owned();
        let now = Instant::now();
        let Some(slot) = self.slots.get_mut(room) else {
            return;
        };
        let copy = Arc::clone(&slot.copy);
        if slot.draining.as_ref().is_some_and(JoinHandle::is_finished) {
            slot.draining = None;
            if let Some(lease) = slot.release_after.take() {
                self.release_lease(&copy, room, &lease).await;
            }
        }
        let Some(slot) = self.slots.get_mut(room) else {
            return;
        };
        if slot
            .claim
            .as_ref()
            .is_some_and(|held| held.worker.is_finished())
        {
            // The worker could not open the session (a conflict, a refused
            // home): the claim goes back, and the session says why.
            let lease = slot.claim.take().map(|held| held.lease);
            slot.retry_at = Some(now + RETRY_AFTER);
            slot.conflict = conflict_in(slot.session.dir.clone()).await;
            slot.conflict_read = Some(now);
            if let Some(lease) = lease {
                self.release_lease(&copy, room, &lease).await;
            }
        }
        let Some(slot) = self.slots.get_mut(room) else {
            return;
        };
        if slot.gone {
            if slot.claim.is_some() {
                tracing::info!(%room, "agents: the session is no longer active here; handing its claim back");
                self.end(room, ClaimAction::Released, true);
            } else if slot.draining.is_none() && slot.release_after.is_none() {
                self.slots.remove(room);
            }
            return;
        }
        if slot.conflict.is_some() {
            if slot.conflict_read.is_none_or(|at| now >= at + RETRY_AFTER) {
                slot.conflict = conflict_in(slot.session.dir.clone()).await;
                slot.conflict_read = Some(now);
            }
            if let Some(conflict) = slot.conflict.clone() {
                self.show_conflict(room, &conflict).await;
                return;
            }
        }

        let cached = copy
            .cached_states(room, CLAIM)
            .await
            .into_iter()
            .find(|(key, _)| key.is_empty())
            .and_then(|(_, state)| ServerClaim::read(&state).ok());
        let server_now = self.clock.now();
        let config = copy.config();
        let principal = copy
            .principal_of(&config.drive)
            .unwrap_or_else(|| self.principal.clone());
        let Some(slot) = self.slots.get_mut(room) else {
            return;
        };
        let needs = slot
            .agent
            .needs
            .clone()
            .unwrap_or_else(|| config.host.needs.clone());
        let pin = slot
            .agent
            .pin
            .clone()
            .unwrap_or_else(|| config.host.pin.clone());
        let (bot, agent) = (bot_id(&config.bot), agent_name(config));
        let placement = place(
            &Ask {
                needs: &needs,
                pin: (!pin.is_empty()).then_some(pin.as_str()),
                drives: &slot.agent.drives,
                agent: &agent,
                bot: &bot,
                principal: &principal,
                prefer_always_on: config.host.prefer_always_on,
                holder: cached.as_ref().map(|c| c.content.host.as_str()),
            },
            hosts,
            server_now,
        );
        // Another host's status may have replaced what this one said: once
        // the claim changes hands or the session is placed, a waiting
        // session is said again.
        let seen = cached
            .as_ref()
            .map(|c| (c.content.host.clone(), c.content.epoch));
        if slot.seen_claim != seen || matches!(placement, Placement::Host(_)) {
            slot.seen_claim = seen;
            slot.shown = None;
        }
        let held = slot.claim.as_ref().map(|held| held.lease.as_ref());
        let holding = held.is_some();
        let busy = slot
            .claim
            .as_ref()
            .is_some_and(|held| held.busy.load(Ordering::Relaxed));
        // With a control room the clock is calibrated by the manifest's
        // read-back before any claim is taken; without one, `acquire` leaves
        // another host's claim alone until its own read-back has.
        let calibrated = self.control_room.is_none() || self.clock.calibrated();
        let may = calibrated
            && slot.draining.is_none()
            && slot.retry_at.is_none_or(|at| now >= at)
            && claim::may_acquire(cached.as_ref(), server_now);
        if cached
            .as_ref()
            .is_some_and(|c| c.content.host != me && !claim::may_acquire(Some(c), server_now))
        {
            // Another live host holds it and answers what arrives.
            copy.forget(room);
        }
        let action = claims::step(held, &placement, &me, busy, may, Moment::now());
        // A host that has just started may not have seen the others'
        // manifests yet: it says nothing until it has been in the control
        // room a while, so a waiting session is said by one host.
        let settled = self.control_room.is_none()
            || self
                .first_published
                .is_some_and(|at| at.elapsed() >= ANNOUNCE_AFTER);
        let announces =
            settled && announcer(hosts, &principal, server_now).as_deref() == Some(me.as_str());
        let epoch = cached.as_ref().map_or(0, |c| c.content.epoch);

        match action {
            Step::Acquire => self.acquire(room, stop).await,
            Step::Renew => self.renew(room).await,
            Step::Stop => {
                tracing::warn!(%room, "agents: no renewal was confirmed in time; this host stops writing the session");
                self.end(room, ClaimAction::Lost, false);
                self.show(
                    room,
                    RunState::Blocked,
                    epoch,
                    None,
                    Some(LOST_DETAIL.to_owned()),
                )
                .await;
            }
            Step::HandBack => {
                tracing::info!(%room, ?placement, "agents: another live host is preferred; handing the session back");
                self.end(room, ClaimAction::Released, true);
            }
            Step::Nothing => {
                if let (Some(text), true, false) = (placement.waiting_text(), announces, holding) {
                    self.show(room, RunState::Waiting, epoch, Some(text), None)
                        .await;
                }
            }
        }
    }

    async fn acquire(&mut self, room: &OwnedRoomId, stop: &CancelSignal) {
        let Some(slot) = self.slots.get(room) else {
            return;
        };
        let (copy, session, agent) = (
            Arc::clone(&slot.copy),
            slot.session.clone(),
            slot.agent.clone(),
        );
        if let Some(conflict) = conflict_in(session.dir.clone()).await {
            self.park_conflicted(room, conflict).await;
            return;
        }
        let me = claimant(&self.host, copy.as_ref());
        let port = copy.claims(room);
        match acquire(port.as_ref(), &me, &self.clock, &self.rtt, None).await {
            Ok(Acquired::Won { lease, from_host }) => {
                tracing::info!(
                    session = %session.path, %room, epoch = lease.epoch,
                    claim_event = %lease.claim_event, server_ts = %claim::rfc3339(lease.server_ts),
                    from_host = from_host.as_deref().unwrap_or(""),
                    "agents: claim acquired"
                );
                let ending = Arc::new(Mutex::new(ClaimAction::Released));
                let busy = Arc::new(AtomicBool::new(false));
                let worker = Arc::clone(&copy).spawn_worker(
                    &session,
                    &agent,
                    stop,
                    Claimed {
                        lease: Arc::clone(&lease),
                        from_host,
                        ending: Arc::clone(&ending),
                        busy: Arc::clone(&busy),
                    },
                );
                if let Some(slot) = self.slots.get_mut(room) {
                    // The worker may post a status of its own: the next one
                    // this host says edits whichever is latest.
                    slot.shown = None;
                    slot.anchor_read = false;
                    if let Some(worker) = worker {
                        slot.claim = Some(Held {
                            lease,
                            worker,
                            ending,
                            busy,
                        });
                        return;
                    }
                }
                let _ = release(port.as_ref(), &me, &lease, &self.clock, &self.rtt).await;
            }
            Ok(Acquired::HeldElsewhere) => {}
            Ok(Acquired::Yielded) => tracing::info!(
                session = %session.path, %room,
                "agents: claim yielded; another host's write won the race"
            ),
            Err(error) => tracing::warn!(%room, %error, "agents: the claim could not be acquired"),
        }
    }

    async fn renew(&mut self, room: &OwnedRoomId) {
        let Some(slot) = self.slots.get(room) else {
            return;
        };
        let Some(held) = &slot.claim else { return };
        let (copy, lease, dir) = (
            Arc::clone(&slot.copy),
            Arc::clone(&held.lease),
            slot.session.dir.clone(),
        );
        // A conflict found while holding stops service as well (S-05).
        if let Some(conflict) = conflict_in(dir).await {
            self.end(room, ClaimAction::Released, true);
            self.park_conflicted(room, conflict).await;
            return;
        }
        let me = claimant(&self.host, copy.as_ref());
        let renewed = renew(
            copy.claims(room).as_ref(),
            &me,
            &lease,
            &self.clock,
            &self.rtt,
        )
        .await;
        match renewed {
            Ok(true) => {}
            Ok(false) => {
                tracing::warn!(%room, "agents: another host holds the session's claim now; claim lost");
                self.end(room, ClaimAction::Lost, false);
            }
            Err(error) => tracing::warn!(%room, %error, "agents: the claim was not renewed"),
        }
    }

    /// Remember `conflict` for `room` and say so.
    async fn park_conflicted(&mut self, room: &OwnedRoomId, conflict: ClaimConflict) {
        if let Some(slot) = self.slots.get_mut(room) {
            slot.conflict = Some(conflict.clone());
            slot.conflict_read = Some(Instant::now());
        }
        self.show_conflict(room, &conflict).await;
    }

    /// End this host's claim on `room` without waiting for anything: its
    /// worker writes `ending` and stops at its next idle moment; a hand-back
    /// releases the claim on the server once that worker has finished.
    fn end(&mut self, room: &OwnedRoomId, ending: ClaimAction, hand_back: bool) {
        let Some(slot) = self.slots.get_mut(room) else {
            return;
        };
        let Some(held) = slot.claim.take() else {
            return;
        };
        *held.ending.lock().unwrap_or_else(|p| p.into_inner()) = ending;
        if ending == ClaimAction::Lost {
            held.lease.lose();
        }
        slot.copy.close_room(room);
        slot.draining = Some(held.worker);
        if hand_back {
            slot.release_after = Some(held.lease);
        }
    }

    /// Release `lease` on `room`'s claim, saying how it went.
    async fn release_lease(&self, copy: &Arc<dyn CopyPort>, room: &OwnedRoomId, lease: &Lease) {
        let me = claimant(&self.host, copy.as_ref());
        match release(
            copy.claims(room).as_ref(),
            &me,
            lease,
            &self.clock,
            &self.rtt,
        )
        .await
        {
            Ok(true) => tracing::info!(%room, epoch = lease.epoch, "agents: claim released"),
            Ok(false) => {}
            Err(error) => tracing::warn!(%room, %error, "agents: the claim could not be released"),
        }
    }

    /// Send the session's status when it says something new, as an edit of
    /// the session's own status anchor when the room has one.
    async fn show(
        &mut self,
        room: &OwnedRoomId,
        run: RunState,
        epoch: u64,
        waiting: Option<String>,
        detail: Option<String>,
    ) {
        let me = self.host.as_str().to_owned();
        let Some(slot) = self.slots.get_mut(room) else {
            return;
        };
        let said = format!("{run:?} {waiting:?} {detail:?}");
        if slot.shown.as_deref() == Some(said.as_str()) {
            return;
        }
        let copy = Arc::clone(&slot.copy);
        if !slot.anchor_read {
            // The worker adopts the room's latest status anchor too, so both
            // edit one.
            if let Some(anchor) = copy.latest_status(room).await {
                slot.status_anchor = Some(anchor);
            }
            slot.anchor_read = true;
        }
        let status = StatusContent {
            v: CONTENT_VERSION,
            session: format!("{}/{}", copy.sessions_subfolder(), slot.session.path),
            kind: slot.agent.kind,
            title: slot.agent.title.clone(),
            agent: copy.config().matrix_user.clone(),
            host: me,
            epoch,
            run,
            detail,
            waiting,
            anchor: slot.status_anchor.clone(),
        };
        match copy
            .send_status(room, serde_json::to_value(&status).unwrap_or(Value::Null))
            .await
        {
            Ok(event) => {
                slot.status_anchor.get_or_insert(event);
                slot.shown = Some(said);
            }
            Err(error) => {
                tracing::warn!(%room, %error, "agents: the session's status was not sent")
            }
        }
    }

    async fn show_conflict(&mut self, room: &OwnedRoomId, conflict: &ClaimConflict) {
        let Some(slot) = self.slots.get(room) else {
            return;
        };
        let base = StatusContent {
            v: CONTENT_VERSION,
            session: String::new(),
            kind: slot.agent.kind,
            title: String::new(),
            agent: slot.copy.config().matrix_user.clone(),
            host: String::new(),
            epoch: 0,
            run: RunState::Blocked,
            detail: None,
            waiting: None,
            anchor: None,
        };
        let blocked = blocked_status(base, conflict);
        tracing::warn!(%room, "agents: {}", claims::conflict_line(conflict));
        self.show(room, blocked.run, blocked.epoch, None, blocked.detail)
            .await;
    }

    /// The claims this host holds, for `status`.
    pub fn held(&self) -> Vec<Value> {
        self.slots
            .iter()
            .filter_map(|(room, slot)| {
                let held = slot.claim.as_ref()?;
                Some(json!({
                    "room": room,
                    "session": slot.session.path,
                    "epoch": held.lease.epoch,
                    "claim_event": held.lease.claim_event,
                }))
            })
            .collect()
    }

    /// Shutdown, first half: every worker writes `released` and stops; the
    /// running turns were cancelled by the caller. Bounded by `within`.
    pub async fn stop_workers(&mut self, within: Duration) {
        let mut workers = Vec::new();
        for (room, slot) in &mut self.slots {
            if let Some(held) = &slot.claim {
                *held.ending.lock().unwrap_or_else(|p| p.into_inner()) = ClaimAction::Released;
            }
            slot.copy.close_room(room);
            if let Some(draining) = slot.draining.take() {
                workers.push(draining);
            }
        }
        let held: Vec<&mut Held> = self
            .slots
            .values_mut()
            .filter_map(|slot| slot.claim.as_mut())
            .collect();
        let finish = async {
            for held in held {
                let _ = (&mut held.worker).await;
            }
            for worker in workers {
                let _ = worker.await;
            }
        };
        if tokio::time::timeout(within, finish).await.is_err() {
            tracing::warn!("agents: a turn did not finish within the bound");
        }
    }

    /// Shutdown, second half, after the drives were pushed: the manifest is
    /// withdrawn and every claim written `released: true` (AD-378), a claim
    /// handed back and not yet released among them.
    pub async fn release_all(&mut self) {
        // The manifest first: a taker that sees a released claim must not
        // still place the session on this host.
        if !matches!(
            tokio::time::timeout(RELEASE_BOUND, self.publish(false)).await,
            Ok(Ok(_))
        ) {
            tracing::warn!("agents: the host manifest was not withdrawn; it lapses");
        }
        let mut leases: Vec<(OwnedRoomId, Arc<dyn CopyPort>, Arc<Lease>)> = Vec::new();
        for (room, slot) in &mut self.slots {
            let held = slot.claim.take().map(|held| held.lease);
            for lease in held.into_iter().chain(slot.release_after.take()) {
                leases.push((room.clone(), Arc::clone(&slot.copy), lease));
            }
        }
        for (room, copy, lease) in leases {
            let me = claimant(&self.host, copy.as_ref());
            let port = copy.claims(&room);
            let released = tokio::time::timeout(
                RELEASE_BOUND,
                release(port.as_ref(), &me, &lease, &self.clock, &self.rtt),
            )
            .await;
            match released {
                Ok(Ok(true)) => {
                    tracing::info!(%room, epoch = lease.epoch, "agents: claim released")
                }
                Ok(Ok(false)) => {}
                Ok(Err(error)) => {
                    tracing::warn!(%room, %error, "agents: the claim could not be released; it lapses")
                }
                Err(_) => {
                    tracing::warn!(%room, "agents: the claim's release did not finish; it lapses")
                }
            }
        }
    }
}

/// `<drive>/<agent>`: how a manifest and placement name an agent.
fn agent_name(config: &AgentConfig) -> String {
    format!("{}/{}", config.drive, config.id)
}

/// Who `copy` claims as on `host`.
fn claimant(host: &HostSlug, copy: &dyn CopyPort) -> Claimant {
    Claimant {
        host: host.as_str().to_owned(),
        device: copy.device(),
        agent: copy.config().matrix_user.clone(),
    }
}

/// The first conflict in the session log at `dir`, read off the runtime.
async fn conflict_in(dir: PathBuf) -> Option<ClaimConflict> {
    tokio::task::spawn_blocking(move || conflict_of(&dir))
        .await
        .ok()
        .flatten()
}

/// The host that says a session waits: the principal's first live host,
/// always-on first, then by slug — one host, so the status is said once.
fn announcer(hosts: &[HostManifest], principal: &str, server_now: u64) -> Option<String> {
    hosts
        .iter()
        .filter(|host| host.principal == principal && host.is_live(server_now))
        .min_by_key(|host| (!host.always_on, host.host.clone()))
        .map(|host| host.host.clone())
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::path::Path;
    use std::sync::atomic::AtomicU64;

    use keeper_core::agents::home::parse_agent_toml;
    use keeper_core::agents::label::{Integrity, Label};
    use keeper_core::agents::log::writer::{rotate_at, ChunkWriter};
    use keeper_core::agents::log::{ClaimBody, LineBody, LogLine, LINE_VERSION};
    use keeper_core::agents::session::SessionKind;
    use keeper_core::bots::chat::{cancellation, CancelHandle};
    use matrix_sdk::ruma::{MilliSecondsSinceUnixEpoch, UInt};
    use tokio::sync::Notify;

    use super::*;

    const ME: &str = "hesperia";
    const OTHER: &str = "electra";
    const PERSON: &str = "@tgorka:example.org";
    const ANCHOR: &str = "$anchor:example.org";

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user")
    }

    fn room(n: u32) -> OwnedRoomId {
        OwnedRoomId::try_from(format!("!s{n}:example.org")).expect("room")
    }

    fn control() -> OwnedRoomId {
        OwnedRoomId::try_from("!control:example.org").expect("room")
    }

    fn drive_toml() -> String {
        format!("version = 1\nid = \"tgdrive\"\ntitle = \"tgdrive\"\nprincipal = \"tgorka\"\nowner = \"{PERSON}\"\nreaders = [\"{PERSON}\"]\n")
    }

    fn config() -> AgentConfig {
        let decl = keeper_core::agents::drive::parse(&drive_toml()).expect("decl");
        parse_agent_toml(
            &format!("version = 1\nid = \"nixi\"\nname = \"Nixi\"\nkind = \"proxy\"\nmatrix_user = \"@nixi:example.org\"\nhuman = \"{PERSON}\"\n\n[model]\nbot = \"bot:openai:http://127.0.0.1:9#model\"\n\n[host]\nneeds = []\n"),
            "nixi",
            &decl,
        )
        .expect("agent.toml")
    }

    /// The homeserver: every room's state as one copy's sync sees it, and
    /// every send in order.
    #[derive(Default)]
    struct Server {
        states: Mutex<HashMap<(OwnedRoomId, String, String), ServerState>>,
        /// `<type> <key> <live|released|withdrawn>` for every state send.
        sends: Mutex<Vec<String>>,
        statuses: Mutex<Vec<Value>>,
        events: AtomicU64,
        /// A manifest send never answers.
        stall_manifests: AtomicBool,
        /// A claim send fails.
        fail_claims: AtomicBool,
    }

    impl Server {
        fn event(&self) -> OwnedEventId {
            let n = self.events.fetch_add(1, Ordering::Relaxed);
            OwnedEventId::try_from(format!("$e{n}:example.org")).expect("event")
        }

        fn put(
            &self,
            room: &RoomId,
            kind: &str,
            key: &str,
            sender: &OwnedUserId,
            content: Value,
        ) -> OwnedEventId {
            let event = self.event();
            let said = if content["released"] == true {
                "released"
            } else if kind == HOST && content["expires_at"] == content["renewed_at"] {
                "withdrawn"
            } else {
                "live"
            };
            self.sends
                .lock()
                .expect("lock")
                .push(format!("{kind} {key} {said}"));
            self.states.lock().expect("lock").insert(
                (room.to_owned(), kind.to_owned(), key.to_owned()),
                ServerState {
                    event_id: event.clone(),
                    sender: sender.clone(),
                    origin_server_ts: MilliSecondsSinceUnixEpoch(UInt::new(wall_ms()).expect("ts")),
                    content,
                },
            );
            event
        }

        fn get(&self, room: &RoomId, kind: &str, key: &str) -> Option<ServerState> {
            self.states
                .lock()
                .expect("lock")
                .get(&(room.to_owned(), kind.to_owned(), key.to_owned()))
                .cloned()
        }

        fn claim(&self, room: &RoomId) -> Option<ServerClaim> {
            self.get(room, CLAIM, "")
                .map(|state| ServerClaim::read(&state).expect("claim"))
        }

        fn statuses(&self) -> Vec<Value> {
            self.statuses.lock().expect("lock").clone()
        }
    }

    struct FakeClaims<'a> {
        server: &'a Server,
        room: OwnedRoomId,
        sender: OwnedUserId,
    }

    impl ClaimPort for FakeClaims<'_> {
        fn send(&self, content: Value) -> ClaimFuture<'_, Result<OwnedEventId, AgentMatrixError>> {
            Box::pin(async move {
                if self.server.fail_claims.load(Ordering::Relaxed) {
                    return Err(AgentMatrixError::Network("unreachable".to_owned()));
                }
                Ok(self
                    .server
                    .put(&self.room, CLAIM, "", &self.sender, content))
            })
        }

        fn read(&self) -> ClaimFuture<'_, Result<Option<ServerState>, AgentMatrixError>> {
            Box::pin(async move { Ok(self.server.get(&self.room, CLAIM, "")) })
        }

        fn next_sync(&self) -> ClaimFuture<'_, Duration> {
            Box::pin(async { Duration::from_millis(1) })
        }
    }

    /// One copy over [`Server`]. Its workers serve until their room is
    /// closed, then take `worker_takes` to finish — a turn, or a backlog.
    struct FakeCopy {
        config: AgentConfig,
        server: Arc<Server>,
        joined: Mutex<HashSet<OwnedRoomId>>,
        latest_status: Option<OwnedEventId>,
        worker_takes: Duration,
        /// A worker ends at once, as one that could not open its log.
        workers_fail: bool,
        workers: Mutex<HashMap<OwnedRoomId, Arc<Notify>>>,
        spawned: Mutex<Vec<OwnedRoomId>>,
        closed: Mutex<Vec<OwnedRoomId>>,
    }

    impl CopyPort for FakeCopy {
        fn config(&self) -> &AgentConfig {
            &self.config
        }

        fn principal_of(&self, _drive: &str) -> Option<String> {
            Some("tgorka".to_owned())
        }

        fn sessions_subfolder(&self) -> &str {
            "60-sessions"
        }

        fn device(&self) -> String {
            "HESPERIA1".to_owned()
        }

        fn joined(&self, room: &RoomId) -> bool {
            self.joined.lock().expect("lock").contains(room)
        }

        fn join_invited<'a>(&'a self, _room: &'a RoomId) -> ClaimFuture<'a, ()> {
            Box::pin(async {})
        }

        fn cached_states<'a>(
            &'a self,
            room: &'a RoomId,
            event_type: &'a str,
        ) -> ClaimFuture<'a, Vec<(String, ServerState)>> {
            Box::pin(async move {
                self.server
                    .states
                    .lock()
                    .expect("lock")
                    .iter()
                    .filter(|((r, kind, _), _)| r == room && kind == event_type)
                    .map(|((_, _, key), state)| (key.clone(), state.clone()))
                    .collect()
            })
        }

        fn send_state<'a>(
            &'a self,
            room: &'a RoomId,
            event_type: &'a str,
            state_key: &'a str,
            content: &'a Value,
        ) -> ClaimFuture<'a, Result<OwnedEventId, AgentMatrixError>> {
            Box::pin(async move {
                if self.server.stall_manifests.load(Ordering::Relaxed) {
                    std::future::pending::<()>().await;
                }
                Ok(self.server.put(
                    room,
                    event_type,
                    state_key,
                    &self.config.matrix_user,
                    content.clone(),
                ))
            })
        }

        fn server_state<'a>(
            &'a self,
            room: &'a RoomId,
            event_type: &'a str,
            state_key: &'a str,
        ) -> ClaimFuture<'a, Result<Option<ServerState>, AgentMatrixError>> {
            Box::pin(async move { Ok(self.server.get(room, event_type, state_key)) })
        }

        fn claims<'a>(&'a self, room: &'a OwnedRoomId) -> Box<dyn ClaimPort + 'a> {
            Box::new(FakeClaims {
                server: &self.server,
                room: room.clone(),
                sender: self.config.matrix_user.clone(),
            })
        }

        fn send_status<'a>(
            &'a self,
            _room: &'a RoomId,
            status: Value,
        ) -> ClaimFuture<'a, Result<OwnedEventId, AgentMatrixError>> {
            Box::pin(async move {
                self.server.statuses.lock().expect("lock").push(status);
                Ok(self.server.event())
            })
        }

        fn latest_status<'a>(&'a self, _room: &'a RoomId) -> ClaimFuture<'a, Option<OwnedEventId>> {
            Box::pin(async move { self.latest_status.clone() })
        }

        fn forget(&self, _room: &RoomId) {}

        fn close_room(&self, room: &RoomId) {
            if let Some(worker) = self.workers.lock().expect("lock").get(room) {
                worker.notify_one();
            }
            self.closed.lock().expect("lock").push(room.to_owned());
        }

        fn spawn_worker(
            self: Arc<Self>,
            _session: &FoundSession,
            agent: &SessionAgent,
            _stop: &CancelSignal,
            claimed: Claimed,
        ) -> Option<JoinHandle<()>> {
            let closed = Arc::new(Notify::new());
            self.workers
                .lock()
                .expect("lock")
                .insert(agent.room.clone(), Arc::clone(&closed));
            self.spawned.lock().expect("lock").push(agent.room.clone());
            let (fails, takes) = (self.workers_fail, self.worker_takes);
            Some(tokio::spawn(async move {
                let _claimed = claimed;
                if fails {
                    return;
                }
                closed.notified().await;
                tokio::time::sleep(takes).await;
            }))
        }
    }

    struct World {
        root: tempfile::TempDir,
        copy: Arc<FakeCopy>,
        rt: HostRuntime,
        stop: CancelSignal,
        _cancel: CancelHandle,
    }

    /// This host, `hesperia`, not always on, in the control room unless
    /// `in_control` is false, its copy's workers taking `worker_takes`.
    fn world(worker_takes: Duration, in_control: bool) -> World {
        let copy = Arc::new(FakeCopy {
            worker_takes,
            ..fake_copy()
        });
        world_over(copy, in_control)
    }

    fn world_over(copy: Arc<FakeCopy>, in_control: bool) -> World {
        if in_control {
            copy.joined.lock().expect("lock").insert(control());
        }
        let rt = HostRuntime {
            host: HostSlug::new(ME).expect("slug"),
            principal: "tgorka".to_owned(),
            always_on: false,
            version: "0.90.0".to_owned(),
            control_room: Some(control()),
            tools: Vec::new(),
            drives: vec![HostDrive {
                id: "tgdrive".to_owned(),
                present: true,
                materialized: Materialized::Full,
            }],
            principal_agents: vec![copy.config.matrix_user.clone()],
            copies: vec![Arc::clone(&copy) as Arc<dyn CopyPort>],
            slots: HashMap::new(),
            clock: ServerClock::default(),
            rtt: Rtt::default(),
            manifest_sent: None,
            first_published: None,
        };
        let (cancel, stop) = cancellation();
        World {
            root: tempfile::tempdir().expect("tempdir"),
            copy,
            rt,
            stop,
            _cancel: cancel,
        }
    }

    impl World {
        fn server(&self) -> &Server {
            &self.copy.server
        }

        fn dir(&self, room: &OwnedRoomId) -> PathBuf {
            self.root.path().join(room.as_str())
        }

        /// The rescan finds the session of `room`, pinned to `pin`.
        fn offer(&mut self, room: &OwnedRoomId, pin: Option<&str>) {
            self.copy.joined.lock().expect("lock").insert(room.clone());
            let decl = keeper_core::agents::drive::parse(&drive_toml()).expect("decl");
            let agent = SessionAgent {
                id: ulid::Ulid::new(),
                agent: "nixi".to_owned(),
                drive: "tgdrive".to_owned(),
                kind: SessionKind::Main,
                title: "main".to_owned(),
                requested_by: user(PERSON),
                parent: None,
                room: room.clone(),
                drives: vec!["tgdrive".to_owned()],
                label: Label::opening(&decl, Integrity::Owner),
                needs: None,
                pin: pin.map(str::to_owned),
                hop: 0,
                limits: None,
                workflow: None,
                created_at: chrono::Utc::now(),
            };
            let dir = self.dir(room);
            std::fs::create_dir_all(&dir).expect("dir");
            let session = FoundSession {
                path: format!("active/{room}"),
                dir,
                agent: Ok(agent.clone()),
            };
            self.rt.offer(&self.copy, &session, &agent);
        }

        async fn tick(&mut self) {
            self.rt.tick(&self.stop).await;
        }

        /// `electra`, always on, with everything this host has: live, or
        /// lapsed.
        fn electra(&self, live: bool) {
            let now = wall_ms();
            let mut manifest = self.rt.manifest(now, true);
            manifest.host = OTHER.to_owned();
            manifest.always_on = true;
            if !live {
                manifest.expires_at = claim::rfc3339(now - 1_000);
            }
            self.server().put(
                &control(),
                HOST,
                OTHER,
                &self.copy.config.matrix_user,
                serde_json::to_value(manifest).expect("manifest"),
            );
        }

        fn held_by_me(&self, room: &OwnedRoomId) -> bool {
            self.server()
                .claim(room)
                .is_some_and(|c| c.content.host == ME && !c.content.released)
        }

        fn released(&self, room: &OwnedRoomId) -> bool {
            self.server()
                .claim(room)
                .is_some_and(|c| c.content.released)
        }
    }

    /// A hand-back closes the room and lets its worker finish its turn — or
    /// its whole backlog — without the tick waiting for it: every other
    /// claim is renewed meanwhile, and the claim is released once the
    /// worker has finished.
    #[tokio::test(start_paused = true)]
    async fn a_hand_back_never_waits_for_the_worker() {
        let mut w = world(Duration::from_secs(5), true);
        let (a, b) = (room(1), room(2));
        w.offer(&a, None);
        w.offer(&b, Some(ME));
        w.tick().await;
        assert!(w.held_by_me(&a) && w.held_by_me(&b));

        // electra comes back: a is placed there, b stays pinned here, and
        // b's renewal is due on the same tick.
        w.electra(true);
        tokio::time::advance(RENEW_EVERY).await;
        let renewed = w.server().claim(&b).expect("b").event_id;
        tokio::time::timeout(Duration::from_secs(1), w.rt.tick(&w.stop))
            .await
            .expect("the tick does not wait for a's worker");
        assert_ne!(
            w.server().claim(&b).expect("b").event_id,
            renewed,
            "b was renewed"
        );
        assert!(w.copy.closed.lock().expect("lock").contains(&a));
        assert!(w.held_by_me(&a), "released only once the worker finished");

        // The worker takes its five seconds; the next tick releases.
        tokio::time::sleep(Duration::from_secs(6)).await;
        w.tick().await;
        assert!(w.released(&a));
        assert!(w.held_by_me(&b));
    }

    /// A waiting session is said again after another host served it and
    /// went away, and after its claim changed hands; between, it is said
    /// once. Every status edits the session's own status anchor.
    #[tokio::test(start_paused = true)]
    async fn a_repeated_outage_is_announced_again_on_the_sessions_anchor() {
        let copy = Arc::new(FakeCopy {
            latest_status: Some(OwnedEventId::try_from(ANCHOR).expect("anchor")),
            ..fake_copy()
        });
        let mut w = world_over(copy, true);
        let a = room(1);
        w.offer(&a, Some(OTHER));
        w.tick().await;
        assert!(
            w.server().statuses().is_empty(),
            "not before ANNOUNCE_AFTER"
        );
        tokio::time::advance(ANNOUNCE_AFTER).await;
        w.tick().await;
        w.tick().await;
        let waiting = |w: &World| {
            w.server()
                .statuses()
                .iter()
                .filter(|s| s["run"] == "waiting")
                .count()
        };
        assert_eq!(waiting(&w), 1, "said once");
        assert_eq!(w.server().statuses()[0]["waiting"], "electra — a live host");

        // electra serves it, then goes away again.
        w.electra(true);
        w.tick().await;
        assert_eq!(waiting(&w), 1);
        w.electra(false);
        w.tick().await;
        assert_eq!(waiting(&w), 2, "the outage is said again");
        w.tick().await;
        assert_eq!(waiting(&w), 2);

        // Another host's claim replaced this host's word on the session.
        let other = Claimant {
            host: OTHER.to_owned(),
            device: "ELECTRA1".to_owned(),
            agent: w.copy.config.matrix_user.clone(),
        };
        let now = w.rt.clock.now();
        w.server().put(
            &a,
            CLAIM,
            "",
            &w.copy.config.matrix_user,
            serde_json::to_value(other.content(5, now, now, false, None)).expect("claim"),
        );
        w.tick().await;
        assert_eq!(waiting(&w), 3, "said again once the claim changed");
        for status in w.server().statuses() {
            assert_eq!(status["anchor"], ANCHOR, "{status}");
        }
    }

    fn fake_copy() -> FakeCopy {
        FakeCopy {
            config: config(),
            server: Arc::default(),
            joined: Mutex::new(HashSet::new()),
            latest_status: None,
            worker_takes: Duration::ZERO,
            workers_fail: false,
            workers: Mutex::default(),
            spawned: Mutex::default(),
            closed: Mutex::default(),
        }
    }

    /// A pin written into a served session's `agent.toml` reaches placement
    /// at the next rescan; a session the rescan no longer finds is handed
    /// back and dropped.
    #[tokio::test(start_paused = true)]
    async fn a_session_edit_is_seen_and_a_vanished_session_is_dropped() {
        let mut w = world(Duration::ZERO, true);
        let (a, b) = (room(1), room(2));
        w.offer(&a, None);
        w.rt.scanned();
        w.tick().await;
        assert!(w.held_by_me(&a));

        w.offer(&a, Some(OTHER));
        w.rt.scanned();
        w.tick().await;
        assert!(
            w.copy.closed.lock().expect("lock").contains(&a),
            "pinned elsewhere: handed back"
        );
        tokio::time::sleep(Duration::from_millis(10)).await;
        w.tick().await;
        assert!(w.released(&a));

        w.offer(&a, Some(OTHER));
        w.offer(&b, None);
        w.rt.scanned();
        w.tick().await;
        assert!(w.held_by_me(&b));
        // b moved out of active/ (and a too).
        w.rt.scanned();
        w.tick().await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        w.tick().await;
        assert!(w.released(&b));
        assert!(w.rt.slots.is_empty(), "both slots dropped");
    }

    /// With a control room, no claim is taken on the host's own wall clock:
    /// only once the manifest's read-back has calibrated it.
    #[tokio::test(start_paused = true)]
    async fn no_claim_is_taken_before_the_clock_is_calibrated() {
        let mut w = world(Duration::ZERO, false);
        let a = room(1);
        w.offer(&a, None);
        w.tick().await;
        assert!(w.server().claim(&a).is_none(), "no manifest read back yet");
        w.copy.joined.lock().expect("lock").insert(control());
        w.tick().await;
        assert!(w.held_by_me(&a));
    }

    /// A homeserver that stops answering delays a tick by the request
    /// bound, never indefinitely.
    #[tokio::test(start_paused = true)]
    async fn a_stalled_homeserver_does_not_hold_the_tick() {
        let mut w = world(Duration::ZERO, true);
        w.server().stall_manifests.store(true, Ordering::Relaxed);
        w.offer(&room(1), None);
        tokio::time::timeout(REQUEST_TIMEOUT + Duration::from_secs(5), w.rt.tick(&w.stop))
            .await
            .expect("bounded");
    }

    /// NFR-120: renewals fail; at 120 s the holder stops, closes the room
    /// and parks the session blocked with why.
    #[tokio::test(start_paused = true)]
    async fn a_holder_that_cannot_renew_parks_the_session_blocked() {
        let mut w = world(Duration::ZERO, true);
        let a = room(1);
        w.offer(&a, None);
        w.tick().await;
        assert!(w.held_by_me(&a));
        w.server().fail_claims.store(true, Ordering::Relaxed);
        tokio::time::advance(RENEW_EVERY).await;
        w.tick().await;
        assert!(w.server().statuses().is_empty());
        tokio::time::advance(RENEW_EVERY).await;
        w.tick().await;
        let statuses = w.server().statuses();
        assert_eq!(statuses.len(), 1, "{statuses:?}");
        assert_eq!(statuses[0]["run"], "blocked");
        assert_eq!(statuses[0]["detail"], LOST_DETAIL);
        assert!(w.copy.closed.lock().expect("lock").contains(&a));
        assert!(w.rt.held().is_empty());
    }

    /// A worker that ends by itself gives its claim back at once and is not
    /// started again before RETRY_AFTER.
    #[tokio::test(start_paused = true)]
    async fn a_worker_that_ends_by_itself_releases_and_waits_to_retry() {
        let copy = Arc::new(FakeCopy {
            workers_fail: true,
            ..fake_copy()
        });
        let mut w = world_over(copy, true);
        let a = room(1);
        w.offer(&a, None);
        w.tick().await;
        tokio::time::advance(Duration::from_secs(1)).await;
        w.tick().await;
        assert!(w.released(&a));
        w.tick().await;
        assert_eq!(w.copy.spawned.lock().expect("lock").len(), 1);
        tokio::time::advance(RETRY_AFTER).await;
        w.tick().await;
        assert_eq!(w.copy.spawned.lock().expect("lock").len(), 2);
    }

    /// AD-378's order on a clean shutdown: the manifest is withdrawn before
    /// any claim is released, so a taker never places a released session
    /// back on this host.
    #[tokio::test(start_paused = true)]
    async fn a_clean_shutdown_withdraws_the_manifest_before_it_releases_the_claims() {
        let mut w = world(Duration::ZERO, true);
        let a = room(1);
        w.offer(&a, None);
        w.tick().await;
        w.rt.stop_workers(Duration::from_secs(1)).await;
        w.rt.release_all().await;
        let sends = w.server().sends.lock().expect("lock").clone();
        let at = |what: &str| sends.iter().position(|s| s == what);
        let withdrawn = at(&format!("{HOST} {ME} withdrawn")).expect("withdrawn");
        let released = at(&format!("{CLAIM}  released")).expect("released");
        assert!(withdrawn < released, "{sends:?}");
    }

    fn conflicted(dir: &Path) {
        for (host, event) in [
            ("electra", "$a:example.org"),
            ("hesperia", "$b:example.org"),
        ] {
            let slug = HostSlug::new(host).expect("slug");
            let mut chunks = ChunkWriter::open(
                dir,
                &slug,
                rotate_at(1 << 20),
                chrono::Utc::now().date_naive(),
            )
            .expect("chunk");
            let now = chrono::Utc::now();
            chunks
                .append(&LogLine {
                    v: LINE_VERSION,
                    id: ulid::Ulid::new(),
                    parent: None,
                    ts: chrono::DateTime::from_timestamp_millis(now.timestamp_millis())
                        .expect("ts"),
                    host: slug,
                    epoch: 2,
                    claim: Some(event.to_owned()),
                    matrix_event: None,
                    body: LineBody::Claim(ClaimBody {
                        epoch: 2,
                        action: ClaimAction::Acquired,
                        from_host: None,
                        claim_event: event.to_owned(),
                        server_ts: "2026-10-03T12:00:00.000Z".to_owned(),
                    }),
                })
                .expect("append");
            std::thread::sleep(Duration::from_millis(3));
        }
    }

    /// S-05: a conflicted session is served by no host and says so once;
    /// its log is read again every RETRY_AFTER, not every tick, and once it
    /// is clean the session is claimed again.
    #[tokio::test(start_paused = true)]
    async fn a_conflicted_session_is_said_once_and_read_again_every_retry() {
        let mut w = world(Duration::ZERO, true);
        let a = room(1);
        w.offer(&a, None);
        conflicted(&w.dir(&a));
        w.tick().await;
        w.tick().await;
        let statuses = w.server().statuses();
        assert_eq!(statuses.len(), 1, "{statuses:?}");
        assert_eq!(statuses[0]["run"], "blocked");
        assert!(w.server().claim(&a).is_none());

        std::fs::remove_dir_all(w.dir(&a).join("log")).expect("resolved");
        w.tick().await;
        assert!(w.server().claim(&a).is_none(), "not read again yet");
        tokio::time::advance(RETRY_AFTER).await;
        w.tick().await;
        assert!(w.held_by_me(&a));
    }
}
