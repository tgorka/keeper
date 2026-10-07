//! The doorbell (story 92.4, AD-388; rulings R59, R60, R160–R162): a host
//! that pushed agent work rings the rooms of the hosts that should fetch
//! it, and a host that hears a doorbell for a drive it mounts fetches now
//! instead of at its paced poll.
//!
//! - **The ringer**, for the pushes of the engine's push tap, coalesced per
//!   drive: the range's paths ([`DriveEngine::changed_paths`]) sorted by
//!   [`keeper_core::agents::doorbell::rings`]; a session's work rings the
//!   session's room, the agents zone (`memory`) rings the principal's control
//!   room and every other control room a copy here is in whose hosts'
//!   manifests list the drive (R29 F19). A room is rung only when the
//!   drive's readers may reach every member of it (R160, [`room_sink`]).
//! - **The receiver**, for a `dev.keeper.agent.doorbell` state event —
//!   from the timeline, the sync's state section, or the cached state read
//!   again once the host is ready — from one of the principal's agents or an
//!   agent homed in the named drive, for a drive this host mounts: one
//!   queued bell per drive, the last commit winning ([`Doorbell::hear`]);
//!   each tick answers a few with one [`DriveEngine::pull_now`] each
//!   ([`Doorbell::deliver`]). Anything else is ignored.
//! - **The [`Ringer`]** services both from the host's one tick without ever
//!   holding it across a network wait (R161), and rings what the last
//!   pushes published when the host stops.
//!
//! keeper-agent does no path arithmetic on a range: the engine says what
//! changed, keeper-core says what it rings, `zone::read_text` reads a
//! session's room through keeper-sync's containment.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, RwLock};
use std::time::{Duration, Instant};

use keeper_core::agents::doorbell::rings;
use keeper_core::agents::events::{
    DoorbellContent, DoorbellReason, CONTENT_VERSION, DOORBELL, HOST,
};
use keeper_core::agents::host::HostManifest;
use keeper_core::agents::label::{check_sink, Integrity, Label, Sink, SinkVerdict};
use keeper_core::agents::session::{parse_session_agent_toml, FILE_NAME as SESSION_FILE};
use keeper_sync::engine::{Engine, PullNow, Pushed};
use keeper_sync::SyncError;
use matrix_sdk::ruma::events::AnySyncStateEvent;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, UserId};
use matrix_sdk::Room;
use tokio::sync::broadcast;
use tokio::task::JoinHandle;

use crate::claims::REQUEST_TIMEOUT;
use crate::hosts::CopyPort;
use crate::rooms::KnownAgent;
use crate::runtime::DriveView;
use crate::zone::read_text;

/// How long a sender's last fetch of a drive has to bring its commit before
/// that sender's next, different commit for the drive is fetched (R161): a
/// sender naming commits that never arrive costs at most one fetch of the
/// drive a minute.
pub const PACE: Duration = Duration::from_secs(60);

/// How many drives' bells one tick answers; the rest wait for the next.
const ANSWERED_PER_TICK: usize = 4;

/// How long a stopping host gives the doorbells of its last pushes.
pub(crate) const RING_FINISH: Duration = Duration::from_secs(5);

/// What the doorbell uses of a sync engine. [`Engine`] is the real one.
pub trait DriveEngine: Send + Sync {
    /// Every push that reaches a remote.
    fn push_tap(&self) -> broadcast::Receiver<Pushed>;
    fn has_commit(&self, profile_id: &str, commit: &str) -> Result<bool, SyncError>;
    fn pull_now(&self, profile_id: &str, commit: &str) -> Result<PullNow, SyncError>;
    fn changed_paths(
        &self,
        profile_id: &str,
        from: Option<&str>,
        to: &str,
    ) -> Result<Vec<String>, SyncError>;
}

impl DriveEngine for Engine {
    fn push_tap(&self) -> broadcast::Receiver<Pushed> {
        Engine::push_tap(self)
    }

    fn has_commit(&self, profile_id: &str, commit: &str) -> Result<bool, SyncError> {
        Engine::has_commit(self, profile_id, commit)
    }

    fn pull_now(&self, profile_id: &str, commit: &str) -> Result<PullNow, SyncError> {
        Engine::pull_now(self, profile_id, commit)
    }

    fn changed_paths(
        &self,
        profile_id: &str,
        from: Option<&str>,
        to: &str,
    ) -> Result<Vec<String>, SyncError> {
        Engine::changed_paths(self, profile_id, from, to)
    }
}

/// What a doorbell was answered with.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Answer {
    /// Queued for the next tick, in place of the drive's earlier bell.
    Heard,
    /// One `Pull` asked of the engine.
    Pulled,
    /// The commit is here already.
    Present,
    /// The engine had this commit asked for already, or a parked pull
    /// waits for a person, or the profile does not pull.
    Held(PullNow),
    /// The sender's last fetch of this drive, less than [`PACE`] ago, has
    /// not brought its commit: kept for a later tick.
    Paced,
    /// Not a doorbell this version reads.
    Unreadable,
    /// A drive this host does not mount.
    UnknownDrive,
    /// From neither one of the principal's agents nor an agent homed in the
    /// drive it names.
    Untrusted,
    /// The engine could not answer.
    Failed,
}

/// Who a bell is admitted by and where it goes.
#[derive(Default)]
struct Roster {
    /// Drive id → the engine's profile id.
    profiles: HashMap<String, String>,
    /// Every agent homed in a drive mounted here, hosted here or not.
    agents: Vec<KnownAgent>,
}

/// One heard bell.
struct Bell {
    sender: OwnedUserId,
    commit: String,
}

#[derive(Default)]
struct Queue {
    /// The last bell heard per drive and not answered yet.
    pending: BTreeMap<String, Bell>,
    /// Per sender and drive: when its bell last caused a fetch, and of
    /// which commit.
    fetched: HashMap<(OwnedUserId, String), (Instant, String)>,
}

/// One host's receiver: its engine, its mounted drives by id, the agents
/// they home and the principal's agent users, and the bells heard and not
/// answered yet. Shared by every copy of the host and kept up to date by the
/// host's rescans.
#[derive(Default)]
pub struct Doorbell {
    engine: RwLock<Option<Arc<dyn DriveEngine>>>,
    roster: RwLock<Roster>,
    principal: RwLock<Vec<OwnedUserId>>,
    queue: Mutex<Queue>,
    /// Changes whenever what admits a bell does, so the copies' cached bells
    /// are heard again: a bell refused before the host was ready is not lost.
    generation: AtomicU64,
}

impl Doorbell {
    pub fn set_engine(&self, engine: Arc<dyn DriveEngine>) {
        *self.engine.write().unwrap_or_else(|p| p.into_inner()) = Some(engine);
        self.generation.fetch_add(1, Ordering::Relaxed);
    }

    /// The drives this host mounts, as `(id, profile id)` with the id their
    /// device-local pin gives them, and every agent the pinned zones home,
    /// hosted here or not (R162). An id two profiles claim maps neither.
    pub fn set_drives(
        &self,
        drives: impl IntoIterator<Item = (String, String)>,
        agents: Vec<KnownAgent>,
    ) {
        let mut profiles: HashMap<String, String> = HashMap::new();
        let mut ambiguous = BTreeSet::new();
        for (id, profile) in drives {
            if profiles
                .insert(id.clone(), profile.clone())
                .is_some_and(|held| held != profile)
            {
                ambiguous.insert(id);
            }
        }
        for id in ambiguous {
            tracing::warn!(drive = %id, "agents: two folders name one drive; its doorbell is not answered");
            profiles.remove(&id);
        }
        let mut roster = self.roster.write().unwrap_or_else(|p| p.into_inner());
        let users = |agents: &[KnownAgent]| -> Vec<(OwnedUserId, String)> {
            agents
                .iter()
                .map(|agent| (agent.matrix_user.clone(), agent.drive.clone()))
                .collect()
        };
        let same = roster.profiles == profiles && users(&roster.agents) == users(&agents);
        *roster = Roster { profiles, agents };
        if !same {
            self.generation.fetch_add(1, Ordering::Relaxed);
        }
    }

    pub fn set_principal_agents(&self, agents: &[OwnedUserId]) {
        let mut principal = self.principal.write().unwrap_or_else(|p| p.into_inner());
        if principal.as_slice() != agents {
            *principal = agents.to_vec();
            self.generation.fetch_add(1, Ordering::Relaxed);
        }
    }

    /// Every agent homed in a drive mounted here: whose audience a room
    /// member is checked by.
    pub fn agents(&self) -> Vec<KnownAgent> {
        self.roster
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .agents
            .clone()
    }

    fn generation(&self) -> u64 {
        self.generation.load(Ordering::Relaxed)
    }

    fn waiting(&self) -> bool {
        !self
            .queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pending
            .is_empty()
    }

    /// Hear the doorbell `content`, state key `state_key`, rung by
    /// `sender`: queued as its drive's bell when it is admitted. Never
    /// blocks; [`Self::deliver`] answers it.
    pub fn hear(&self, sender: &UserId, state_key: &str, content: &serde_json::Value) -> Answer {
        let Some(bell) = DoorbellContent::accept(state_key, content) else {
            return Answer::Unreadable;
        };
        {
            let roster = self.roster.read().unwrap_or_else(|p| p.into_inner());
            if !roster.profiles.contains_key(&bell.drive) {
                return Answer::UnknownDrive;
            }
            let principal = self
                .principal
                .read()
                .unwrap_or_else(|p| p.into_inner())
                .iter()
                .any(|agent| agent == sender);
            let homed = roster
                .agents
                .iter()
                .any(|agent| agent.matrix_user == sender && agent.drive == bell.drive);
            if !principal && !homed {
                return Answer::Untrusted;
            }
        }
        self.queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .pending
            .insert(
                bell.drive,
                Bell {
                    sender: sender.to_owned(),
                    commit: bell.commit,
                },
            );
        Answer::Heard
    }

    /// Answer the bells of at most `at_most` drives, at `now`. Blocking: the
    /// engine reads the repository and the journal. Without an engine the
    /// bells wait.
    pub fn deliver(&self, now: Instant, at_most: usize) -> Vec<(String, Answer)> {
        let Some(engine) = self
            .engine
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .clone()
        else {
            return Vec::new();
        };
        let batch: Vec<(String, Bell)> = {
            let mut queue = self.queue.lock().unwrap_or_else(|p| p.into_inner());
            let drives: Vec<String> = queue.pending.keys().take(at_most).cloned().collect();
            drives
                .into_iter()
                .filter_map(|drive| queue.pending.remove(&drive).map(|bell| (drive, bell)))
                .collect()
        };
        batch
            .into_iter()
            .map(|(drive, bell)| {
                let answer = self.answer(engine.as_ref(), &drive, bell, now);
                (drive, answer)
            })
            .collect()
    }

    fn answer(&self, engine: &dyn DriveEngine, drive: &str, bell: Bell, now: Instant) -> Answer {
        let Some(profile) = self
            .roster
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .profiles
            .get(drive)
            .cloned()
        else {
            return Answer::UnknownDrive;
        };
        match engine.has_commit(&profile, &bell.commit) {
            Ok(true) => return Answer::Present,
            Ok(false) => {}
            Err(error) => {
                tracing::warn!(%drive, %error, "agents: a doorbell's commit could not be looked for");
                return Answer::Failed;
            }
        }
        let key = (bell.sender.clone(), drive.to_owned());
        let last = self
            .queue
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .fetched
            .get(&key)
            .cloned();
        if let Some((at, asked)) = last {
            if asked != bell.commit
                && now.saturating_duration_since(at) < PACE
                && !engine.has_commit(&profile, &asked).unwrap_or(false)
            {
                self.queue
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .pending
                    .entry(drive.to_owned())
                    .or_insert(bell);
                return Answer::Paced;
            }
        }
        match engine.pull_now(&profile, &bell.commit) {
            Ok(PullNow::Queued) => {
                self.queue
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .fetched
                    .insert(key, (now, bell.commit));
                Answer::Pulled
            }
            Ok(held) => Answer::Held(held),
            Err(error) => {
                tracing::warn!(%drive, %error, "agents: a doorbell's pull could not be queued");
                Answer::Failed
            }
        }
    }
}

/// Hear every doorbell `client` syncs through `doorbell`: a state event, so
/// the state handler gets it whether the sync carries it in a room's
/// timeline or in its state section (a first sync, a room just joined, a
/// gappy sync).
pub fn listen(client: &matrix_sdk::Client, doorbell: Arc<Doorbell>) {
    client.add_event_handler(move |event: Raw<AnySyncStateEvent>, room: Room| {
        let doorbell = Arc::clone(&doorbell);
        async move {
            if event.get_field::<&str>("type").ok().flatten() != Some(DOORBELL) {
                return;
            }
            let Ok(value) = event.deserialize_as::<serde_json::Value>() else {
                return;
            };
            let (Some(key), Some(sender)) = (
                value["state_key"].as_str(),
                value["sender"].as_str().and_then(|s| UserId::parse(s).ok()),
            ) else {
                return;
            };
            let answer = doorbell.hear(&sender, key, &value["content"]);
            tracing::debug!(room = %room.room_id(), %sender, ?answer, "agents: a doorbell was heard");
        }
    });
}

/// A drive as the ringer needs it.
#[derive(Clone)]
pub(crate) struct RingDrive {
    pub(crate) id: String,
    pub(crate) profile_id: String,
    pub(crate) sessions_root: Option<PathBuf>,
    sessions: Option<String>,
    agents: Option<String>,
    /// The drive's readers, at the owner's integrity.
    label: Label,
}

impl RingDrive {
    /// `view`, when its zone hosts (a zone that hosts nothing rings nothing).
    pub(crate) fn of(view: &DriveView) -> Option<RingDrive> {
        let decl = view.hosts.as_ref().ok()?;
        Some(RingDrive {
            id: view.id.clone(),
            profile_id: view.profile.id.clone(),
            sessions_root: view.profile.sessions_root(),
            sessions: view.profile.sessions.as_ref().map(|s| s.subfolder.clone()),
            agents: view.profile.agents.as_ref().map(|a| a.subfolder.clone()),
            label: Label::opening(decl, Integrity::Owner),
        })
    }
}

/// What a ringing needs of its host, owned: it runs off the host's tick.
#[derive(Clone)]
pub(crate) struct Round {
    pub(crate) drives: Vec<RingDrive>,
    pub(crate) copies: Vec<Arc<dyn CopyPort>>,
    pub(crate) control: Option<OwnedRoomId>,
}

impl Round {
    /// Ring for `pushed` when it is one of this round's drives'.
    pub(crate) async fn ring(
        &self,
        pushed: &Pushed,
        engine: &Arc<dyn DriveEngine>,
        agents: &[KnownAgent],
    ) -> Vec<(OwnedRoomId, DoorbellReason)> {
        let Some(drive) = self
            .drives
            .iter()
            .find(|drive| drive.profile_id == pushed.profile_id)
        else {
            return Vec::new();
        };
        ring(
            pushed,
            drive,
            engine,
            &self.copies,
            self.control.as_ref(),
            agents,
        )
        .await
    }
}

/// The audience of a room whose members — joined or invited — are
/// `members` (R160): each one of `agents` through its own audience, every
/// other member as a person, whatever power the room gives them. An account
/// this host cannot name as an agent is a person, so it must be a reader.
pub fn room_sink(members: BTreeSet<OwnedUserId>, agents: &[KnownAgent]) -> Sink {
    let mut humans = BTreeSet::new();
    let mut agent_audiences = Vec::new();
    for member in members {
        match agents.iter().find(|agent| agent.matrix_user == member) {
            Some(agent) => agent_audiences.push(agent.home_readers.clone()),
            None => {
                humans.insert(member);
            }
        }
    }
    Sink::Room {
        humans,
        agent_audiences,
    }
}

/// Ring for `pushed` of `drive`: each room it rings, with its reason.
pub(crate) async fn ring(
    pushed: &Pushed,
    drive: &RingDrive,
    engine: &Arc<dyn DriveEngine>,
    copies: &[Arc<dyn CopyPort>],
    control: Option<&OwnedRoomId>,
    agents: &[KnownAgent],
) -> Vec<(OwnedRoomId, DoorbellReason)> {
    // The range and the sessions' rooms are read off the async runtime:
    // the repository and the drive's files.
    let (read_engine, read_drive, read_pushed) =
        (Arc::clone(engine), drive.clone(), pushed.clone());
    let read = tokio::task::spawn_blocking(move || {
        rung_rooms(&read_pushed, &read_drive, read_engine.as_ref())
    })
    .await;
    let Ok(Some((mut targets, memory))) = read else {
        return Vec::new();
    };
    if memory {
        for room in control_rooms(&drive.id, copies, control).await {
            targets.push((room, DoorbellReason::Memory));
        }
    }
    let content = serde_json::to_value(DoorbellContent {
        v: CONTENT_VERSION,
        drive: drive.id.clone(),
        commit: pushed.to.clone(),
        reason: DoorbellReason::Memory,
    });
    let Ok(mut content) = content else {
        return Vec::new();
    };
    let mut sent = Vec::new();
    for (room, reason) in targets {
        let Some(copy) = copies.iter().find(|copy| copy.joined(&room)) else {
            continue;
        };
        let Some(members) = copy.members(&room).await else {
            tracing::debug!(%room, "agents: a room whose members could not be read is not rung");
            continue;
        };
        if let SinkVerdict::Block { .. } = check_sink(&drive.label, &room_sink(members, agents)) {
            tracing::info!(%room, drive = %drive.id, "agents: a doorbell is not rung where someone outside the drive's readers would see it");
            continue;
        }
        content["reason"] = serde_json::json!(reason);
        match tokio::time::timeout(
            REQUEST_TIMEOUT,
            copy.send_state(&room, DOORBELL, &drive.id, &content),
        )
        .await
        {
            Ok(Ok(_)) => sent.push((room, reason)),
            Ok(Err(error)) => tracing::warn!(%room, %error, "agents: a doorbell could not be rung"),
            Err(_) => tracing::warn!(%room, "agents: a doorbell's send did not finish in time"),
        }
    }
    sent
}

/// The session rooms `pushed` rings, and whether it rings `memory`.
/// Blocking.
fn rung_rooms(
    pushed: &Pushed,
    drive: &RingDrive,
    engine: &dyn DriveEngine,
) -> Option<(Vec<(OwnedRoomId, DoorbellReason)>, bool)> {
    let paths = match engine.changed_paths(&drive.profile_id, pushed.from.as_deref(), &pushed.to) {
        Ok(paths) => paths,
        Err(error) => {
            tracing::warn!(drive = %drive.id, %error, "agents: a push's range could not be read; nothing is rung");
            return None;
        }
    };
    let rung = rings(&paths, drive.sessions.as_deref(), drive.agents.as_deref());
    let sessions = rung
        .sessions
        .iter()
        .filter_map(|(session, reason)| session_room(drive, session).map(|room| (room, *reason)))
        .collect();
    Some((sessions, rung.memory))
}

/// The room of the session folder `session` (`active/<name>`) of `drive`,
/// read through the zone's containment: a link out of the sessions zone,
/// or an `agent.toml` that is not a file, names no room. Blocking.
fn session_room(drive: &RingDrive, session: &str) -> Option<OwnedRoomId> {
    let root = drive.sessions_root.as_ref()?;
    match read_text(root, &format!("{session}/{SESSION_FILE}")) {
        Ok(Some(text)) => parse_session_agent_toml(&text).ok().map(|agent| agent.room),
        Ok(None) => None,
        Err(refusal) => {
            tracing::info!(drive = %drive.id, %refusal, "agents: a session's room is not read");
            None
        }
    }
}

/// The control rooms a `memory` doorbell of `drive` goes to: the
/// principal's, and every other control room a copy here is in whose
/// hosts' manifests list the drive.
async fn control_rooms(
    drive: &str,
    copies: &[Arc<dyn CopyPort>],
    control: Option<&OwnedRoomId>,
) -> Vec<OwnedRoomId> {
    let mut rooms: BTreeSet<OwnedRoomId> = control.cloned().into_iter().collect();
    for copy in copies {
        for room in copy.control_rooms() {
            if rooms.contains(&room) {
                continue;
            }
            let lists = copy
                .cached_states(&room, HOST)
                .await
                .into_iter()
                .any(|(_, state)| {
                    serde_json::from_value::<HostManifest>(state.content)
                        .is_ok_and(|manifest| manifest.drives.iter().any(|d| d.id == drive))
                });
            if lists {
                rooms.insert(room);
            }
        }
    }
    rooms.into_iter().collect()
}

/// A host's doorbell work, serviced by its one tick and never waited on by
/// it (R161): the pushes not rung yet, one per profile (a later push widens
/// the range); at most one ringing and one answering in flight; the copies'
/// cached bells heard again whenever what admits a bell changes or another
/// copy has synced.
pub(crate) struct Ringer {
    engine: Arc<dyn DriveEngine>,
    pushes: broadcast::Receiver<Pushed>,
    pending: BTreeMap<String, Pushed>,
    ringing: Option<JoinHandle<()>>,
    answering: Option<JoinHandle<()>>,
    /// The doorbell's generation and the synced copies the cached bells
    /// were last heard at.
    recovered: Option<(u64, usize)>,
}

impl Ringer {
    /// Over `engine`'s push tap, from now on.
    pub(crate) fn new(engine: Arc<dyn DriveEngine>) -> Ringer {
        let pushes = engine.push_tap();
        Ringer::over(engine, pushes)
    }

    pub(crate) fn over(
        engine: Arc<dyn DriveEngine>,
        pushes: broadcast::Receiver<Pushed>,
    ) -> Ringer {
        Ringer {
            engine,
            pushes,
            pending: BTreeMap::new(),
            ringing: None,
            answering: None,
            recovered: None,
        }
    }

    pub(crate) fn engine(&self) -> &Arc<dyn DriveEngine> {
        &self.engine
    }

    /// The pushes the tap holds, coalesced into `pending`.
    fn take_pushes(&mut self) {
        use tokio::sync::broadcast::error::TryRecvError;
        loop {
            match self.pushes.try_recv() {
                Ok(pushed) => match self.pending.get_mut(&pushed.profile_id) {
                    Some(held) => held.to = pushed.to,
                    None => {
                        self.pending.insert(pushed.profile_id.clone(), pushed);
                    }
                },
                Err(TryRecvError::Lagged(missed)) => {
                    tracing::warn!(
                        missed,
                        "agents: pushes went by unrung; their hosts fetch at their poll"
                    );
                }
                Err(TryRecvError::Empty | TryRecvError::Closed) => return,
            }
        }
    }

    /// One tick: start ringing what was pushed when no ringing is in
    /// flight, and answering what was heard when no answering is. Returns
    /// at once.
    pub(crate) fn tick(&mut self, round: &Round, doorbell: &Arc<Doorbell>) {
        self.take_pushes();
        if !self.pending.is_empty() && self.ringing.as_ref().is_none_or(JoinHandle::is_finished) {
            let batch = std::mem::take(&mut self.pending);
            self.ringing = Some(tokio::spawn(ring_all(
                batch,
                round.clone(),
                Arc::clone(&self.engine),
                doorbell.agents(),
            )));
        }
        if self
            .answering
            .as_ref()
            .is_some_and(|answering| !answering.is_finished())
        {
            return;
        }
        let synced = round.copies.iter().filter(|copy| copy.synced()).count();
        let mark = (doorbell.generation(), synced);
        let recover = self.recovered != Some(mark);
        if !recover && !doorbell.waiting() {
            return;
        }
        self.recovered = Some(mark);
        let copies = if recover {
            round.copies.clone()
        } else {
            Vec::new()
        };
        self.answering = Some(tokio::spawn(answer_all(Arc::clone(doorbell), copies)));
    }

    /// The host stops: what its last pushes published is rung — after any
    /// ringing in flight — within `within`; answering stops.
    pub(crate) async fn finish(&mut self, round: &Round, doorbell: &Doorbell, within: Duration) {
        if let Some(answering) = self.answering.take() {
            answering.abort();
        }
        self.take_pushes();
        let batch = std::mem::take(&mut self.pending);
        let in_flight = self.ringing.take();
        let in_flight_abort = in_flight.as_ref().map(JoinHandle::abort_handle);
        let (round, engine, agents) = (round.clone(), Arc::clone(&self.engine), doorbell.agents());
        let last = tokio::spawn(async move {
            if let Some(in_flight) = in_flight {
                let _ = in_flight.await;
            }
            ring_all(batch, round, engine, agents).await;
        });
        let last_abort = last.abort_handle();
        if tokio::time::timeout(within, last).await.is_err() {
            last_abort.abort();
            if let Some(in_flight) = in_flight_abort {
                in_flight.abort();
            }
            tracing::warn!("agents: the last pushes' doorbells did not all ring; their hosts fetch at their poll");
        }
    }
}

async fn ring_all(
    batch: BTreeMap<String, Pushed>,
    round: Round,
    engine: Arc<dyn DriveEngine>,
    agents: Vec<KnownAgent>,
) {
    for pushed in batch.into_values() {
        for (room, reason) in round.ring(&pushed, &engine, &agents).await {
            tracing::info!(%room, ?reason, profile = %pushed.profile_id, "agents: rang a doorbell");
        }
    }
}

/// Hear again the bells `recover` cached, then answer a tick's worth.
async fn answer_all(doorbell: Arc<Doorbell>, recover: Vec<Arc<dyn CopyPort>>) {
    for copy in &recover {
        for (sender, key, content) in copy.doorbells().await {
            doorbell.hear(&sender, &key, &content);
        }
    }
    let answered =
        tokio::task::spawn_blocking(move || doorbell.deliver(Instant::now(), ANSWERED_PER_TICK))
            .await;
    match answered {
        Ok(answers) => {
            for (drive, answer) in answers {
                tracing::debug!(%drive, ?answer, "agents: a doorbell was answered");
            }
        }
        Err(error) => tracing::warn!(%error, "agents: the doorbells' answers did not finish"),
    }
}

/// A [`DriveEngine`] that records what it is asked: its range is a real
/// repository's when it has one.
#[cfg(test)]
pub(crate) mod fake {
    use std::collections::HashSet;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use super::*;

    #[derive(Default)]
    pub(crate) struct FakeEngine {
        pub(crate) repo: Option<PathBuf>,
        pub(crate) present: Mutex<HashSet<String>>,
        pub(crate) pulls: Mutex<Vec<(String, String)>>,
    }

    impl DriveEngine for FakeEngine {
        fn push_tap(&self) -> broadcast::Receiver<Pushed> {
            broadcast::channel(1).1
        }

        fn has_commit(&self, _profile_id: &str, commit: &str) -> Result<bool, SyncError> {
            Ok(self.present.lock().expect("lock").contains(commit))
        }

        fn pull_now(&self, profile_id: &str, commit: &str) -> Result<PullNow, SyncError> {
            self.pulls
                .lock()
                .expect("lock")
                .push((profile_id.to_owned(), commit.to_owned()));
            Ok(PullNow::Queued)
        }

        fn changed_paths(
            &self,
            _profile_id: &str,
            from: Option<&str>,
            to: &str,
        ) -> Result<Vec<String>, SyncError> {
            let repo = self
                .repo
                .as_ref()
                .ok_or_else(|| SyncError::Config("no repository".to_owned()))?;
            keeper_sync::git::history::changed_between(repo, from, to)
        }
    }
}

#[cfg(test)]
mod tests {
    use keeper_core::agents::home::AgentKind;
    use keeper_core::agents::label::Readers;
    use serde_json::json;

    use super::fake::FakeEngine;
    use super::*;

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user")
    }

    fn homed(id: &str, drive: &str) -> KnownAgent {
        KnownAgent {
            id: id.to_owned(),
            drive: drive.to_owned(),
            name: id.to_owned(),
            matrix_user: user(&format!("@{id}:example.org")),
            kind: AgentKind::Steward,
            human: None,
            hosted: false,
            home_readers: Readers::Anyone,
            opening: Label::top(),
            drives: vec![drive.to_owned()],
        }
    }

    fn bell(drive: &str, commit: &str) -> serde_json::Value {
        json!({"v": 1, "drive": drive, "commit": commit, "reason": "memory"})
    }

    fn receiver(engine: &Arc<FakeEngine>) -> Doorbell {
        let doorbell = Doorbell::default();
        doorbell.set_engine(Arc::clone(engine) as Arc<dyn DriveEngine>);
        doorbell.set_drives(
            [("neuradrive".to_owned(), "p-neura".to_owned())],
            vec![
                homed("lucyna-novak", "neuradrive"),
                homed("tola-grey", "tgdrive"),
            ],
        );
        doorbell.set_principal_agents(&[user("@nixi:example.org")]);
        doorbell
    }

    /// 92.4 acceptance 5: who is heard, and what a heard bell asks.
    #[test]
    fn a_doorbell_pulls_its_drive_once() {
        let engine = Arc::new(FakeEngine::default());
        let doorbell = receiver(&engine);
        let absent = "a".repeat(40);
        let here = "b".repeat(40);
        engine.present.lock().expect("lock").insert(here.clone());
        let lucyna = user("@lucyna-novak:example.org");
        let now = Instant::now();

        // An agent homed in the named drive, a commit not here: one pull.
        assert_eq!(
            doorbell.hear(&lucyna, "neuradrive", &bell("neuradrive", &absent)),
            Answer::Heard
        );
        assert_eq!(
            doorbell.deliver(now, ANSWERED_PER_TICK),
            vec![("neuradrive".to_owned(), Answer::Pulled)]
        );
        // One of the principal's own agents is heard for any mounted drive.
        let later = "c".repeat(40);
        doorbell.hear(
            &user("@nixi:example.org"),
            "neuradrive",
            &bell("neuradrive", &later),
        );
        assert_eq!(
            doorbell.deliver(now, ANSWERED_PER_TICK),
            vec![("neuradrive".to_owned(), Answer::Pulled)]
        );
        // A commit already here asks nothing.
        doorbell.hear(&lucyna, "neuradrive", &bell("neuradrive", &here));
        assert_eq!(
            doorbell.deliver(now, ANSWERED_PER_TICK),
            vec![("neuradrive".to_owned(), Answer::Present)]
        );
        // A drive this host does not mount.
        assert_eq!(
            doorbell.hear(&lucyna, "tgdrive", &bell("tgdrive", &absent)),
            Answer::UnknownDrive
        );
        // Neither the principal's agent nor homed in the drive it names: an
        // agent of another drive, and a person.
        for sender in ["@tola-grey:example.org", "@tgorka:example.org"] {
            assert_eq!(
                doorbell.hear(&user(sender), "neuradrive", &bell("neuradrive", &absent)),
                Answer::Untrusted,
                "{sender}"
            );
        }
        // A doorbell keyed by another drive than it names.
        assert_eq!(
            doorbell.hear(&lucyna, "tgdrive", &bell("neuradrive", &absent)),
            Answer::Unreadable
        );
        assert!(doorbell.deliver(now, ANSWERED_PER_TICK).is_empty());
        assert_eq!(
            *engine.pulls.lock().expect("lock"),
            vec![
                ("p-neura".to_owned(), absent),
                ("p-neura".to_owned(), later),
            ],
            "each heard doorbell asked the engine once; nothing else did"
        );
    }

    /// R161 (review DB-07): an admitted sender naming commits that never
    /// arrive — distinct, or alternating — costs at most one fetch of the
    /// drive per [`PACE`]; its latest bell is kept, not dropped, and a sender
    /// whose last fetch brought its commit is not held back.
    #[test]
    fn a_sender_whose_commits_never_arrive_is_paced() {
        let engine = Arc::new(FakeEngine::default());
        let doorbell = receiver(&engine);
        let lucyna = user("@lucyna-novak:example.org");
        let start = Instant::now();
        let commit = |n: u32| format!("{n:040x}");
        for n in 0..30u32 {
            let ring = commit(if n % 2 == 0 { n } else { 1 });
            doorbell.hear(&lucyna, "neuradrive", &bell("neuradrive", &ring));
            doorbell.deliver(start + Duration::from_secs(u64::from(n)), ANSWERED_PER_TICK);
        }
        assert_eq!(
            engine.pulls.lock().expect("lock").len(),
            1,
            "thirty bells within a minute, one fetch"
        );
        let latest = commit(29);
        doorbell.hear(&lucyna, "neuradrive", &bell("neuradrive", &latest));
        assert_eq!(
            doorbell.deliver(start + PACE, ANSWERED_PER_TICK),
            vec![("neuradrive".to_owned(), Answer::Pulled)],
            "a minute on, the latest bell is fetched"
        );
        // Its commit arrived: the next bell is fetched at once.
        engine.present.lock().expect("lock").insert(latest);
        let next = commit(31);
        doorbell.hear(&lucyna, "neuradrive", &bell("neuradrive", &next));
        assert_eq!(
            doorbell.deliver(start + PACE + Duration::from_secs(1), ANSWERED_PER_TICK),
            vec![("neuradrive".to_owned(), Answer::Pulled)]
        );
        assert_eq!(engine.pulls.lock().expect("lock").len(), 3);
    }

    /// R162 (review DB-05): an id two folders claim maps neither, so a
    /// bell for it fetches no folder.
    #[test]
    fn a_drive_two_folders_claim_is_not_answered() {
        let engine = Arc::new(FakeEngine::default());
        let doorbell = receiver(&engine);
        doorbell.set_drives(
            [
                ("tgdrive".to_owned(), "p1".to_owned()),
                ("tgdrive".to_owned(), "p2".to_owned()),
                ("neuradrive".to_owned(), "p-neura".to_owned()),
            ],
            vec![homed("tola-grey", "tgdrive")],
        );
        assert_eq!(
            doorbell.hear(
                &user("@nixi:example.org"),
                "tgdrive",
                &bell("tgdrive", &"a".repeat(40))
            ),
            Answer::UnknownDrive
        );
    }

    /// R162 (review DB-09): a session's room is read only inside the
    /// sessions zone: an `agent.toml` linked out of it, or a session folder
    /// linked out of it, names no room.
    #[test]
    fn a_session_linked_out_of_the_zone_names_no_room() {
        use keeper_core::agents::session::{compose_session_agent_toml, SessionAgent, SessionKind};
        let root = tempfile::tempdir().expect("root");
        let outside = tempfile::tempdir().expect("outside");
        let zone = root.path().join("60-sessions");
        let room = OwnedRoomId::try_from("!stolen:example.org").expect("room");
        let toml = compose_session_agent_toml(&SessionAgent {
            id: ulid::Ulid::new(),
            agent: "nixi".to_owned(),
            drive: "tgdrive".to_owned(),
            kind: SessionKind::Conversation,
            title: "work".to_owned(),
            requested_by: user("@tgorka:example.org"),
            parent: None,
            reply: None,
            room: room.clone(),
            drives: vec!["tgdrive".to_owned()],
            label: Label::top(),
            needs: None,
            pin: None,
            hop: 0,
            dispatch_chain: Vec::new(),
            limits: None,
            workflow: None,
            checkpoints: None,
            outputs: Vec::new(),
            created_at: chrono::Utc::now(),
        });
        std::fs::create_dir_all(outside.path().join("elsewhere")).expect("dir");
        std::fs::write(outside.path().join(SESSION_FILE), &toml).expect("write");
        std::fs::write(outside.path().join("elsewhere").join(SESSION_FILE), &toml).expect("write");
        std::fs::create_dir_all(zone.join("active/a")).expect("dir");
        std::os::unix::fs::symlink(
            outside.path().join(SESSION_FILE),
            zone.join("active/a").join(SESSION_FILE),
        )
        .expect("link");
        std::os::unix::fs::symlink(outside.path().join("elsewhere"), zone.join("active/b"))
            .expect("link");
        std::fs::create_dir_all(zone.join("active/c")).expect("dir");
        std::fs::write(zone.join("active/c").join(SESSION_FILE), &toml).expect("write");
        let drive = RingDrive {
            id: "tgdrive".to_owned(),
            profile_id: "tgdrive".to_owned(),
            sessions_root: Some(zone),
            sessions: Some("60-sessions".to_owned()),
            agents: None,
            label: Label::top(),
        };
        assert_eq!(
            session_room(&drive, "active/a"),
            None,
            "a linked agent.toml"
        );
        assert_eq!(
            session_room(&drive, "active/b"),
            None,
            "a linked session folder"
        );
        // The same file inside the zone is read: the refusals are the links'.
        assert_eq!(session_room(&drive, "active/c"), Some(room));
    }
}
