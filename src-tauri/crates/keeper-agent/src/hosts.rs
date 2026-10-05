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
//!
//! A `kind = scheduled` session's card (92.3) is placed by its `host:` pin
//! and run by the session's holder on the same tick, never a second clock:
//! a due window is named in the claim before the worker writes
//! `run: running` (R56); a window the previous holder named is settled, not
//! run again (R58); and when no host can run it, the announcing host takes
//! the claim, says so on the card and hands it back (Q8).

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::future::Future;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use keeper_core::agents::agentd::AgentdConfig;
use keeper_core::agents::claim::{self, Claimant, ServerClaim, RENEW_EVERY};
use keeper_core::agents::delegation::{session_title, DelegateContent};
use keeper_core::agents::events::{
    RunState, StatusContent, CLAIM, CONTENT_VERSION, CONTROL_ROOM_TYPE, DOORBELL, HOST, STATUS,
    STEWARD_ROOM,
};
use keeper_core::agents::home::AgentConfig;
use keeper_core::agents::host::{accept, bot_id, HostDrive, HostManifest, Materialized};
use keeper_core::agents::label::{check_sink, Label, SinkVerdict};
use keeper_core::agents::log::reader::ClaimConflict;
use keeper_core::agents::log::{ClaimAction, HostSlug, LineBody};
use keeper_core::agents::matrix::{AgentMatrixError, ServerState};
use keeper_core::agents::placement::{place, Ask, Placement};
use keeper_core::agents::room::room_members;
use keeper_core::agents::session::{SessionAgent, SessionKind};
use keeper_core::bots::chat::CancelSignal;
use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId, OwnedUserId, RoomId, TransactionId};
use matrix_sdk::RoomState;
use serde_json::{json, Value};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::cards::{self, Due, Scheduled};
use crate::claims::{
    self, acquire, blocked_status, bounded, conflict_of, release, renew, wall_ms, Acquired,
    ClaimFuture, ClaimPort, Lease, Moment, RoomClaims, Rtt, ServerClock, Step, REQUEST_TIMEOUT,
};
use crate::doorbell::{RingDrive, Round};
use crate::matrix_sink::{EditPort, RoomPort};
use crate::runtime::{self, spawn_worker, Claimed, Copy, DriveView, PENDING_ROOMS};

use crate::sinks::{room_audience, ProxyDoors, Sinks, MEMBERS_UNREAD, NARROWED_STATUS};
use crate::stewards::{self, Closed, Duty, Harvester};
use crate::zone::FoundSession;

/// The session at `dir`'s label as its log says now: its last `label`
/// line's, else `opening` (`agent.toml`'s).
fn logged_label(dir: &std::path::Path, opening: &Label) -> Label {
    keeper_core::agents::log::reader::read_session(dir)
        .lines
        .iter()
        .rev()
        .find_map(|line| match &line.body {
            LineBody::Label(body) => Some(body.label()),
            _ => None,
        })
        .unwrap_or_else(|| opening.clone())
}

/// The time a card's schedule is read at, epoch ms, and the machine's UTC
/// offset then in minutes, given the server's time now.
pub(crate) type Calendar = Arc<dyn Fn(u64) -> (i64, i32) + Send + Sync>;

/// The server's time — the one every host's claims agree on — at this
/// machine's offset, which keeper-sync's dialect reads a wall clock by.
fn server_calendar() -> Calendar {
    Arc::new(|server_now| {
        (
            i64::try_from(server_now).unwrap_or(i64::MAX),
            chrono::Local::now().offset().local_minus_utc() / 60,
        )
    })
}

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

/// The opening brief of a delegated room no session folder names yet, as a
/// copy holds it for placement (R54, R93).
#[derive(Debug, Clone)]
pub(crate) struct Opening {
    /// The brief's event: the first one admitted in its room.
    pub(crate) event: OwnedEventId,
    /// Its server time, ms: the session's creation time on whichever host
    /// makes it, and the key of the claim that host takes first (R95).
    pub(crate) at: u64,
    pub(crate) brief: Arc<DelegateContent>,
}

/// What a copy holds for placement: each delegated room's opening — at
/// most [`PENDING_ROOMS`] rooms, as the router keeps — and which rooms were
/// read back from the server. A room past the bound is not forgotten: it
/// stays unread, and is read back once there is space.
#[derive(Debug, Default)]
pub(crate) struct PendingBriefs {
    held: BTreeMap<OwnedRoomId, Opening>,
    read: HashSet<OwnedRoomId>,
}

impl PendingBriefs {
    /// A brief admitted live: held unless its room holds one already — the
    /// first admitted is the opening, and a later round never replaces it —
    /// or the bound is reached, when the room is left for its read-back.
    pub(crate) fn hold(&mut self, room: &RoomId, opening: Opening) {
        if self.held.contains_key(room) {
            return;
        }
        if self.held.len() >= PENDING_ROOMS {
            self.read.remove(room);
            return;
        }
        self.held.insert(room.to_owned(), opening);
    }

    /// `room` read back: the opening its timeline holds replaces whatever a
    /// live hold took — a later round, when the opening came while this host
    /// was down; none leaves a live hold be. `false` when the bound left no
    /// space for it, and the room stays unread.
    pub(crate) fn read_back(&mut self, room: &RoomId, opening: Option<Opening>) -> bool {
        if let Some(opening) = opening {
            if !self.held.contains_key(room) && self.held.len() >= PENDING_ROOMS {
                return false;
            }
            self.held.insert(room.to_owned(), opening);
        }
        self.read.insert(room.to_owned());
        true
    }

    /// Whether `room` was read back.
    pub(crate) fn is_read(&self, room: &RoomId) -> bool {
        self.read.contains(room)
    }

    /// Read `room` back again: its state could not be read when a brief
    /// arrived.
    pub(crate) fn unread(&mut self, room: &RoomId) {
        self.read.remove(room);
    }

    /// The openings of the rooms read back: what placement acts on.
    pub(crate) fn ready(&self) -> Vec<(OwnedRoomId, Opening)> {
        self.held
            .iter()
            .filter(|(room, _)| self.read.contains(*room))
            .map(|(room, opening)| (room.clone(), opening.clone()))
            .collect()
    }

    /// Forget `room`'s opening: its session exists.
    pub(crate) fn forget(&mut self, room: &RoomId) {
        self.held.remove(room);
    }
}

/// Read back every room of `rooms` that is not yet with `read`, keeping
/// what it finds in `pending`. A read that fails — a `/messages` error, a
/// room whose state does not read — leaves the room unread, to be read on a
/// later tick, never taken as a room with no brief.
pub(crate) async fn read_back_rooms<F, Fut>(
    pending: &Mutex<PendingBriefs>,
    rooms: Vec<OwnedRoomId>,
    mut read: F,
) where
    F: FnMut(OwnedRoomId) -> Fut,
    Fut: Future<Output = Result<Option<Opening>, String>>,
{
    let lock = || pending.lock().unwrap_or_else(|p| p.into_inner());
    for room in rooms {
        if lock().is_read(&room) {
            continue;
        }
        match read(room.clone()).await {
            Ok(opening) => {
                if !lock().read_back(&room, opening) {
                    tracing::info!(%room, "agents: too many delegations wait here; this one is read back later");
                }
            }
            Err(error) => {
                tracing::warn!(%room, %error, "agents: a delegated room could not be read back; it is read again on the next tick")
            }
        }
    }
}

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
    /// The claim under the state key `key` of `room`: a steward duty's
    /// creation claim in the control room (R165).
    fn keyed_claims<'a>(&'a self, room: &'a OwnedRoomId, key: &'a str) -> Box<dyn ClaimPort + 'a>;
    /// Send a session status into `room`.
    fn send_status<'a>(
        &'a self,
        room: &'a RoomId,
        status: Value,
    ) -> ClaimFuture<'a, Result<OwnedEventId, AgentMatrixError>>;
    /// The sinks of `session`, as its worker holds them: its audit ids and
    /// the known agents now (R65, R160).
    fn sinks(&self, session: &SessionAgent) -> Sinks;
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
    /// The opening briefs to this agent of rooms no session folder names
    /// yet, each room read back (R54).
    fn pending_delegates(&self) -> Vec<(OwnedRoomId, Opening)>;
    /// Forget `room`'s brief: its session exists.
    fn drop_pending(&self, room: &RoomId);
    /// Make the session `opening` opens in `room`, idempotently on its id.
    fn create_delegated<'a>(
        &'a self,
        room: &'a OwnedRoomId,
        opening: &'a Opening,
    ) -> ClaimFuture<'a, Result<(), String>>;
    /// Once this copy has synced: read back, for its opening brief, every
    /// delegated room it joined that no session among `served` names and
    /// that was not read back yet (R54).
    fn recover_briefs<'a>(&'a self, served: HashSet<OwnedRoomId>) -> ClaimFuture<'a, ()>;
    /// The control rooms (`dev.keeper.agent.control`) this copy is in.
    fn control_rooms(&self) -> Vec<OwnedRoomId>;
    /// Everyone in `room` or invited to it, whatever their power, as the
    /// server lists them now; `None` when they cannot be read.
    fn members<'a>(&'a self, room: &'a RoomId) -> ClaimFuture<'a, Option<BTreeSet<OwnedUserId>>>;
    /// Whether this copy has synced at least once.
    fn synced(&self) -> bool;
    /// Every doorbell in the state of the rooms this copy is in, as the
    /// last sync left it: `(sender, state key, content)`.
    fn doorbells(&self) -> ClaimFuture<'_, Vec<(OwnedUserId, String, Value)>>;
    /// Hand `scheduled` to `room`'s worker as a scheduled arrival.
    fn route_scheduled(&self, room: &RoomId, scheduled: Scheduled);
    /// Hand `closed` to `room`'s worker, a steward's harvest session (R61).
    fn route_harvest(&self, room: &RoomId, closed: Closed);
    /// What this copy's harvest worker made of the closed sessions it was
    /// handed since the last call, by id: `true` once it is settled.
    fn harvest_acks(&self) -> Vec<(String, bool)>;
    /// Whether this copy's sessions zone holds its steward's `duty` session.
    fn steward_found(&self, duty: Duty) -> ClaimFuture<'_, Result<bool, String>>;
    /// A new room for its steward's `duty` session, the drive's readers
    /// invited to watch.
    fn steward_room(&self, duty: Duty) -> ClaimFuture<'_, Result<OwnedRoomId, String>>;
    /// Its steward's `duty` session folder, naming `room`, with its files:
    /// whether this call made it.
    fn steward_folder<'a>(
        &'a self,
        duty: Duty,
        room: &'a RoomId,
    ) -> ClaimFuture<'a, Result<bool, String>>;
    /// Leave every room this copy is in that was made for its steward's
    /// `duty` session, but `keep`, revoking the drive's readers' invites.
    fn steward_orphans<'a>(&'a self, duty: Duty, keep: Option<&'a RoomId>) -> ClaimFuture<'a, ()>;
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

    fn keyed_claims<'a>(&'a self, room: &'a OwnedRoomId, key: &'a str) -> Box<dyn ClaimPort + 'a> {
        Box::new(RoomClaims::keyed(
            self.client.clone(),
            room.clone(),
            key,
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

    fn sinks(&self, session: &SessionAgent) -> Sinks {
        let deps = &self.deps;
        Sinks {
            data_dir: deps.data_dir.clone(),
            provider_id: deps.row.provider.id.clone(),
            bot_id: deps.bot.id.clone(),
            session_id: session.id.to_string(),
            known: Some(Arc::clone(
                &self.known.read().unwrap_or_else(|p| p.into_inner()),
            )),
            home_drive: deps.home.config.drive.clone(),
            zone: deps.sessions_zone.clone(),
            doors: Some(Arc::clone(&self.doors) as Arc<dyn ProxyDoors>),
        }
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

    fn pending_delegates(&self) -> Vec<(OwnedRoomId, Opening)> {
        self.pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .ready()
    }

    fn drop_pending(&self, room: &RoomId) {
        self.pending
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .forget(room);
    }

    fn create_delegated<'a>(
        &'a self,
        room: &'a OwnedRoomId,
        opening: &'a Opening,
    ) -> ClaimFuture<'a, Result<(), String>> {
        let (zone, config) = (
            self.deps.sessions_zone.clone(),
            self.deps.home.config.clone(),
        );
        let (room, opening) = (room.clone(), opening.clone());
        Box::pin(async move {
            tokio::task::spawn_blocking(move || {
                create_delegated(&zone, &config, &room, &opening.brief, made_at(&opening))
            })
            .await
            .map_err(|error| error.to_string())?
        })
    }

    fn recover_briefs<'a>(&'a self, served: HashSet<OwnedRoomId>) -> ClaimFuture<'a, ()> {
        Box::pin(runtime::recover_briefs(self, served))
    }

    fn control_rooms(&self) -> Vec<OwnedRoomId> {
        self.client
            .client()
            .joined_rooms()
            .into_iter()
            .filter(|room| {
                room.room_type()
                    .is_some_and(|kind| kind.to_string() == CONTROL_ROOM_TYPE)
            })
            .map(|room| room.room_id().to_owned())
            .collect()
    }

    fn members<'a>(&'a self, room: &'a RoomId) -> ClaimFuture<'a, Option<BTreeSet<OwnedUserId>>> {
        Box::pin(async move {
            let room = self.client.client().get_room(room)?;
            tokio::time::timeout(REQUEST_TIMEOUT, room_members(&room))
                .await
                .ok()
                .flatten()
        })
    }

    fn synced(&self) -> bool {
        *self.syncs.borrow() > 0
    }

    fn doorbells(&self) -> ClaimFuture<'_, Vec<(OwnedUserId, String, Value)>> {
        Box::pin(async move {
            let mut bells = Vec::new();
            for room in self.client.client().joined_rooms() {
                for (key, state) in self.client.cached_states(room.room_id(), DOORBELL).await {
                    bells.push((state.sender, key, state.content));
                }
            }
            bells
        })
    }

    fn route_harvest(&self, room: &RoomId, closed: Closed) {
        match crate::agent::harvest_arrival(&self.deps.home.config.matrix_user, &closed) {
            Some(arrived) => self.router.route(room, arrived),
            None => {
                tracing::warn!(%room, session = %closed.id, "agents: a closed session's id makes no harvest arrival");
                self.harvest_acks
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .push((closed.id, false));
            }
        }
    }

    fn harvest_acks(&self) -> Vec<(String, bool)> {
        std::mem::take(&mut *self.harvest_acks.lock().unwrap_or_else(|p| p.into_inner()))
    }

    fn steward_found(&self, duty: Duty) -> ClaimFuture<'_, Result<bool, String>> {
        Box::pin(async move {
            let zone = self.deps.sessions_zone.clone();
            let id = stewards::session_id(self.config(), duty).to_string();
            tokio::task::spawn_blocking(move || {
                zone.is_dir() && crate::sessions::verbs::find(&zone, &id).is_some()
            })
            .await
            .map_err(|error| error.to_string())
        })
    }

    fn steward_room(&self, duty: Duty) -> ClaimFuture<'_, Result<OwnedRoomId, String>> {
        Box::pin(async move {
            let config = self.config();
            let readers = self.deps.home.drive.readers.iter().cloned().collect();
            bounded(self.client.create_room(
                keeper_core::agents::matrix::RoomKind::Session(SessionKind::Scheduled),
                &stewards::room_name(config, duty),
                readers,
                &[],
            ))
            .await
            .map_err(|error| error.to_string())
        })
    }

    fn steward_folder<'a>(
        &'a self,
        duty: Duty,
        room: &'a RoomId,
    ) -> ClaimFuture<'a, Result<bool, String>> {
        Box::pin(async move {
            let (config, decl) = (self.config().clone(), self.deps.home.drive.clone());
            let (zone, room) = (self.deps.sessions_zone.clone(), room.to_owned());
            tokio::task::spawn_blocking(move || {
                let now = chrono::Local::now();
                let files = stewards::folder_files(&config, &decl, duty, &zone)?;
                let agent = stewards::session(&config, &decl, duty, &room, now);
                crate::seed::make_folder(&zone, &agent, files, false, now)
                    .map(|settled| settled.folder_made)
            })
            .await
            .map_err(|error| error.to_string())
            .and_then(|made| made)
        })
    }

    fn steward_orphans<'a>(&'a self, duty: Duty, keep: Option<&'a RoomId>) -> ClaimFuture<'a, ()> {
        Box::pin(async move {
            let name = stewards::room_name(self.config(), duty);
            let invited: Vec<OwnedUserId> = self.deps.home.drive.readers.iter().cloned().collect();
            let orphans: Vec<OwnedRoomId> = self
                .client
                .client()
                .joined_rooms()
                .into_iter()
                .filter(|room| {
                    room.room_type().map(|kind| kind.to_string()).as_deref()
                        == Some(keeper_core::agents::events::SESSION_ROOM_TYPE)
                        && room.name().as_deref() == Some(name.as_str())
                        && keep != Some(room.room_id())
                })
                .map(|room| room.room_id().to_owned())
                .collect();
            for orphan in orphans {
                tracing::info!(room = %orphan, duty = duty.name(), "agents: a steward's room no folder will name is left");
                crate::seed::discard(&self.client, &orphan, &invited).await;
            }
        })
    }

    fn route_scheduled(&self, room: &RoomId, scheduled: Scheduled) {
        match crate::agent::scheduled_arrival(&self.deps.home.config.matrix_user, &scheduled) {
            Some(arrived) => self.router.route(room, arrived),
            None => {
                tracing::warn!(%room, ?scheduled, "agents: a scheduled card's arrival could not be made")
            }
        }
    }
}

/// When a delegated session was made: its opening brief's server time, the
/// same on every host that might make it.
pub(crate) fn made_at(opening: &Opening) -> chrono::DateTime<chrono::Utc> {
    chrono::DateTime::from_timestamp_millis(i64::try_from(opening.at).unwrap_or(i64::MAX))
        .unwrap_or_else(chrono::Utc::now)
}

/// Make the delegated session `brief` opens in `room` for `config`'s agent
/// in the sessions zone `zone`, as made at `at`: its `agent.toml` and its
/// card in one journaled plan, nothing when its id names a session already.
pub fn create_delegated(
    zone: &std::path::Path,
    config: &AgentConfig,
    room: &OwnedRoomId,
    brief: &DelegateContent,
    at: chrono::DateTime<chrono::Utc>,
) -> Result<(), String> {
    use keeper_core::agents::delegation::{child_card, child_session, CARD_FILE};
    let agent = child_session(brief, &config.id, &config.drive, room, at)
        .ok_or_else(|| "the brief's id is not a ULID".to_owned())?;
    crate::sessions::verbs::create_carded_session(
        zone,
        &agent,
        vec![(CARD_FILE.to_owned(), child_card(brief, &config.id))],
        at.with_timezone(&chrono::Local),
    )
    .map(|_| ())
    .map_err(|error| error.to_string())
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
    /// The host and window of the claim this host took a scheduled session
    /// from, until the card reads settled against them (Q18, R163, R164).
    taken: Option<(String, Option<String>)>,
    /// The window of the scheduled card last handed to the worker.
    sent: Option<String>,
    /// The window the scheduled card was last said to wait in.
    waited: Option<String>,
    /// Why the session's scheduled card does not run here, as last said.
    refused: Option<String>,
}

/// A claim this host holds and the worker writing under it.
struct Held {
    lease: Arc<Lease>,
    worker: JoinHandle<()>,
    ending: Arc<Mutex<ClaimAction>>,
    activity: Arc<crate::agent::Activity>,
}

/// Whether `held`'s worker is at work, or its session's run waits for a
/// person: either way no window of its scheduled card is begun or named —
/// a run blocked on an approval is not due (R84, R177).
fn busy_or_parked(held: &Held) -> bool {
    held.activity.busy.load(Ordering::SeqCst) || held.activity.parked.load(Ordering::SeqCst)
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
    clock: Arc<ServerClock>,
    rtt: Arc<Rtt>,
    manifest_sent: Option<Instant>,
    /// When this host's manifest first reached the control room.
    first_published: Option<Instant>,
    /// What each pending delegation's room was last told it waits for.
    pending_shown: HashMap<OwnedRoomId, String>,
    calendar: Calendar,
    /// Each held harvest session's view of the archive (R61).
    harvesters: HashMap<OwnedRoomId, Harvester>,
    stewards: Stewarding,
}

/// The steward duty sessions this host still has to see made (R66, R165),
/// and the one bootstrap task working on them beside the lease clock.
#[derive(Default)]
struct Stewarding {
    due: Vec<(Arc<dyn CopyPort>, Duty)>,
    task: Option<(JoinHandle<()>, tokio::sync::oneshot::Receiver<StewardsLeft>)>,
    next: Option<Instant>,
    /// The missing control room was said.
    no_control_said: bool,
}

/// The duties a bootstrap task did not see made.
type StewardsLeft = Vec<(Arc<dyn CopyPort>, Duty)>;

/// How long one steward duty's bootstrap may take before it is given up
/// for this round.
pub(crate) const STEWARD_DEADLINE: Duration = Duration::from_secs(60);
/// How long after one bootstrap round the next starts.
pub(crate) const STEWARD_AGAIN: Duration = Duration::from_secs(5);

/// Make `copy`'s steward's `duty` session once across every host (R165):
/// `true` once its folder is in this checkout. The claim keyed by the
/// session's id in the control room decides who makes it, and its holder
/// names the room it made in the room record ([`STEWARD_ROOM`], same key)
/// before it writes the folder. A released claim beside a record means the
/// session was made, and its folder is on its way. A winner adopts the room
/// a record names — a maker that stopped between the room and the folder —
/// or makes one; once the folder is written, the claim is handed back. Any
/// other room made for the duty is left, its invites revoked.
async fn make_steward(
    copy: &dyn CopyPort,
    control: &OwnedRoomId,
    host: &HostSlug,
    clock: &ServerClock,
    rtt: &Rtt,
    duty: Duty,
) -> Result<bool, String> {
    if copy.steward_found(duty).await? {
        return Ok(true);
    }
    let key = stewards::session_id(copy.config(), duty).to_string();
    let me = claimant(host, copy);
    let port = copy.keyed_claims(control, &key);
    let recorded = || async {
        bounded(copy.server_state(control, STEWARD_ROOM, &key))
            .await
            .map(|state| {
                state.and_then(|state| {
                    state.content["room"]
                        .as_str()
                        .and_then(|room| OwnedRoomId::try_from(room).ok())
                })
            })
            .map_err(|error| error.to_string())
    };
    let released = bounded(port.read())
        .await
        .map_err(|error| error.to_string())?
        .as_ref()
        .and_then(|state| ServerClaim::read(state).ok())
        .is_some_and(|claim| claim.content.released);
    if released {
        if let Some(room) = recorded().await? {
            copy.steward_orphans(duty, Some(&room)).await;
            return Ok(false);
        }
    }
    let lease = match acquire(port.as_ref(), &me, clock, rtt, None)
        .await
        .map_err(|error| error.to_string())?
    {
        Acquired::Won { lease, .. } => lease,
        Acquired::HeldElsewhere | Acquired::Yielded => return Ok(false),
    };
    let adopted = recorded().await?;
    copy.steward_orphans(duty, adopted.as_deref()).await;
    let room = match adopted {
        Some(room) => room,
        None => {
            // A room made and never recorded is left by the next winner.
            let room = copy.steward_room(duty).await?;
            if !renew(port.as_ref(), &me, &lease, clock, rtt)
                .await
                .map_err(|error| error.to_string())?
            {
                return Err("the creation claim was lost before the room was recorded".to_owned());
            }
            let record = json!({ "v": CONTENT_VERSION, "room": room });
            bounded(copy.send_state(control, STEWARD_ROOM, &key, &record))
                .await
                .map_err(|error| error.to_string())?;
            room
        }
    };
    // A folder that could not be written leaves the claim to lapse beside
    // the record: the next winner adopts the room.
    copy.steward_folder(duty, &room).await?;
    if let Err(error) = release(port.as_ref(), &me, &lease, clock, rtt).await {
        tracing::warn!(%room, %error, "agents: a steward's creation claim could not be handed back");
    }
    Ok(true)
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
        HostRuntime {
            host,
            principal: config.principal.clone(),
            always_on: config.always_on,
            version: version.to_owned(),
            control_room: config.homeserver.control_room.clone(),
            tools,
            drives: manifest_drives,
            principal_agents: principal_agents(&config.principal, drives),
            copies: copies
                .into_iter()
                .map(|copy| copy as Arc<dyn CopyPort>)
                .collect(),
            slots: HashMap::new(),
            clock: Arc::default(),
            rtt: Arc::default(),
            manifest_sent: None,
            first_published: None,
            pending_shown: HashMap::new(),
            calendar: server_calendar(),
            harvesters: HashMap::new(),
            stewards: Stewarding::default(),
        }
    }

    /// The runtime of a desktop host: never always on, no tool of its own
    /// yet, its drives as the app syncs them, and no control room until one
    /// of its copies is found in the principal's ([`Self::set_control_room`]).
    pub(crate) fn desktop(
        host: HostSlug,
        principal: &str,
        version: &str,
        drives: &[(DriveView, Materialized)],
        copies: Vec<Arc<Copy>>,
    ) -> HostRuntime {
        HostRuntime {
            host,
            principal: principal.to_owned(),
            always_on: false,
            version: version.to_owned(),
            control_room: None,
            tools: Vec::new(),
            drives: drives
                .iter()
                .map(|(view, materialized)| HostDrive {
                    id: view.id.clone(),
                    present: view.hosts.is_ok() && view.profile.local_path.exists(),
                    materialized: *materialized,
                })
                .collect(),
            principal_agents: principal_agents(principal, drives.iter().map(|(view, _)| view)),
            copies: copies
                .into_iter()
                .map(|copy| copy as Arc<dyn CopyPort>)
                .collect(),
            slots: HashMap::new(),
            clock: Arc::default(),
            rtt: Arc::default(),
            manifest_sent: None,
            first_published: None,
            pending_shown: HashMap::new(),
            calendar: server_calendar(),
            harvesters: HashMap::new(),
            stewards: Stewarding::default(),
        }
    }

    /// The principal's control room, once known.
    pub(crate) fn control_room(&self) -> Option<&OwnedRoomId> {
        self.control_room.as_ref()
    }

    /// Use `room` as the principal's control room from now on.
    pub(crate) fn set_control_room(&mut self, room: OwnedRoomId) {
        self.control_room = Some(room);
    }

    /// The agent users whose manifests this host believes.
    pub(crate) fn principal_agents(&self) -> &[OwnedUserId] {
        &self.principal_agents
    }

    /// What a ringing over `drives` needs of this host: the drives whose
    /// zones host, the copies, the principal's control room.
    pub(crate) fn round(&self, drives: &[DriveView]) -> Round {
        Round {
            drives: drives.iter().filter_map(RingDrive::of).collect(),
            copies: self.copies.clone(),
            control: self.control_room.clone(),
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
        self.tick_stewards();
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
        self.tick_harvest().await;
        self.recover_briefs().await;
        self.tick_pending(&hosts).await;
    }

    /// Make `copies`' steward duty sessions from now on (R66): only the
    /// duties whose card her menu can make are tried; the rest are said once.
    pub(crate) fn make_stewards(&mut self, copies: Vec<Arc<dyn CopyPort>>) {
        for copy in copies {
            for duty in Duty::ALL {
                let config = copy.config();
                match stewards::prompts(config, duty) {
                    Ok(_) => self.stewards.due.push((Arc::clone(&copy), duty)),
                    Err(sentence) => {
                        tracing::warn!(agent = %config.id, %sentence, "agents: a steward's session is not made")
                    }
                }
            }
        }
    }

    /// The steward bootstrap (R66, R165), beside the lease clock and never
    /// ahead of it: a finished round's leftovers are taken, and a
    /// new round starts as one task, each duty bounded by
    /// [`STEWARD_DEADLINE`], [`STEWARD_AGAIN`] after the last.
    fn tick_stewards(&mut self) {
        let now = Instant::now();
        if let Some((_, done)) = &mut self.stewards.task {
            match done.try_recv() {
                Ok(left) => self.stewards.due = left,
                Err(tokio::sync::oneshot::error::TryRecvError::Empty) => return,
                Err(tokio::sync::oneshot::error::TryRecvError::Closed) => {}
            }
            self.stewards.task = None;
            self.stewards.next = Some(now + STEWARD_AGAIN);
        }
        if self.stewards.due.is_empty() || self.stewards.next.is_some_and(|at| now < at) {
            return;
        }
        let Some(control) = self.control_room.clone() else {
            // Without the room, no host can claim a duty, and a session made
            // anyway would be made again on every other host.
            if !self.stewards.no_control_said {
                self.stewards.no_control_said = true;
                tracing::warn!("agents: this host has no control room, so no steward's triage or harvest session is made here until it has one");
            }
            return;
        };
        let (host, clock, rtt) = (
            self.host.clone(),
            Arc::clone(&self.clock),
            Arc::clone(&self.rtt),
        );
        let due = self.stewards.due.clone();
        let (tell, told) = tokio::sync::oneshot::channel();
        let task = tokio::spawn(async move {
            let mut left = Vec::new();
            for (copy, duty) in due {
                let agent = copy.config().id.clone();
                let made = tokio::time::timeout(
                    STEWARD_DEADLINE,
                    make_steward(copy.as_ref(), &control, &host, &clock, &rtt, duty),
                )
                .await;
                match made {
                    Ok(Ok(true)) => {
                        tracing::info!(%agent, duty = duty.name(), "agents: a steward's session is in this checkout");
                        continue;
                    }
                    Ok(Ok(false)) => {}
                    Ok(Err(sentence)) => {
                        tracing::warn!(%agent, duty = duty.name(), %sentence, "agents: a steward's session could not be made; trying again")
                    }
                    Err(_) => {
                        tracing::warn!(%agent, duty = duty.name(), "agents: a steward's session was not made in time; trying again")
                    }
                }
                left.push((copy, duty));
            }
            let _ = tell.send(left);
        });
        self.stewards.task = Some((task, told));
    }

    /// Each held harvest session's worker is handed the closed sessions its
    /// [`Harvester`]'s bounded step found (R61), and what the worker said of
    /// the ones it was handed is taken first. A session handed back drops
    /// its harvester; the next holder starts from the archive again.
    async fn tick_harvest(&mut self) {
        let slots = &self.slots;
        self.harvesters.retain(|room, _| {
            slots
                .get(room)
                .is_some_and(|slot| slot.claim.is_some() && slot.draining.is_none())
        });
        let held: Vec<OwnedRoomId> = self
            .slots
            .iter()
            .filter(|(_, slot)| {
                slot.claim.is_some() && slot.draining.is_none() && stewards::is_harvest(&slot.agent)
            })
            .map(|(room, _)| room.clone())
            .collect();
        for room in held {
            let Some(slot) = self.slots.get(&room) else {
                continue;
            };
            let (copy, session, agent) = (
                Arc::clone(&slot.copy),
                slot.session.clone(),
                slot.agent.clone(),
            );
            let mut harvester = self.harvesters.remove(&room).unwrap_or_default();
            let now = Instant::now();
            for (id, done) in copy.harvest_acks() {
                harvester.acknowledged(&id, done, now);
            }
            let Some(zone) = zone_of(&session) else {
                continue;
            };
            let stepped = tokio::task::spawn_blocking(move || {
                let handed = harvester.step(&zone, &session.path, &agent, now);
                (harvester, handed)
            })
            .await;
            let Ok((harvester, handed)) = stepped else {
                continue;
            };
            for closed in handed {
                copy.route_harvest(&room, closed);
            }
            self.harvesters.insert(room, harvester);
        }
    }

    /// Each copy, once it has synced, reads back the delegated rooms it
    /// joined that no session names yet and that it has not read, for their
    /// opening briefs (R54) — every tick, so a read that failed is tried
    /// again without a restart.
    async fn recover_briefs(&mut self) {
        for copy in self.copies.clone() {
            let served: HashSet<OwnedRoomId> = self.slots.keys().cloned().collect();
            copy.recover_briefs(served).await;
        }
    }

    /// Make the session `opening` opens in `room` through `copy`, placed on
    /// this host, when the room's claim says no other host does (R95): the
    /// two hosts' checkouts do not see each other's folders, so the room's
    /// claim — keyed by the opening's server time, which only it has there —
    /// is taken before the folder is made and handed back once it is. A
    /// handed-back claim under that key means the session was made, here or
    /// elsewhere, and its folder is on its way; a live one, that another
    /// host is making it now.
    async fn make_delegated(
        &mut self,
        copy: &Arc<dyn CopyPort>,
        room: &OwnedRoomId,
        opening: &Opening,
    ) {
        let key = claim::rfc3339(opening.at);
        let me = claimant(&self.host, copy.as_ref());
        let port = copy.claims(room);
        let made = match bounded(port.read()).await {
            Ok(state) => state
                .as_ref()
                .and_then(|state| ServerClaim::read(state).ok())
                .is_some_and(|claim| {
                    claim.content.released && claim.content.window.as_deref() == Some(key.as_str())
                }),
            Err(error) => {
                tracing::warn!(%room, %error, "agents: a delegated room's claim could not be read");
                return;
            }
        };
        if made {
            return;
        }
        let lease = match acquire(port.as_ref(), &me, &self.clock, &self.rtt, Some(key)).await {
            Ok(Acquired::Won { lease, .. }) => lease,
            Ok(Acquired::HeldElsewhere | Acquired::Yielded) => return,
            Err(error) => {
                tracing::warn!(%room, %error, "agents: a delegated room's claim could not be taken");
                return;
            }
        };
        match copy.create_delegated(room, opening).await {
            Ok(()) => {
                tracing::info!(%room, delegation = %opening.brief.id, brief = %opening.event, "agents: a delegated session was made here");
                if let Err(error) =
                    release(port.as_ref(), &me, &lease, &self.clock, &self.rtt).await
                {
                    tracing::warn!(%room, %error, "agents: a made delegation's claim could not be handed back");
                }
                copy.drop_pending(room);
                self.pending_shown.remove(room);
            }
            // The claim lapses unreleased: the session counts as unmade,
            // and is made again once it has.
            Err(error) => {
                tracing::warn!(%room, %error, "agents: a delegated session could not be made")
            }
        }
    }

    /// Every brief this host's copies hold for a room no session names yet
    /// is placed (AD-379, R54): the placed host makes the session, which its
    /// next rescan offers and its claim then serves; a brief nobody can
    /// serve says in its room what it waits for, by the one announcing host.
    async fn tick_pending(&mut self, hosts: &[HostManifest]) {
        let me = self.host.as_str().to_owned();
        let server_now = self.clock.now();
        let settled = self.control_room.is_none()
            || self
                .first_published
                .is_some_and(|at| at.elapsed() >= ANNOUNCE_AFTER);
        for copy in self.copies.clone() {
            for (room, opening) in copy.pending_delegates() {
                if self.slots.contains_key(&room) {
                    copy.drop_pending(&room);
                    self.pending_shown.remove(&room);
                    continue;
                }
                // A host that has just started may not know the others yet:
                // two hosts that both thought they won would each make the
                // folder in their own checkout.
                if !settled {
                    continue;
                }
                let config = copy.config();
                let principal = copy
                    .principal_of(&config.drive)
                    .unwrap_or_else(|| self.principal.clone());
                let (bot, agent) = (bot_id(&config.bot), agent_name(config));
                let placement = place(
                    &Ask {
                        needs: &config.host.needs,
                        pin: (!config.host.pin.is_empty()).then_some(config.host.pin.as_str()),
                        drives: &opening.brief.drives,
                        agent: &agent,
                        bot: &bot,
                        principal: &principal,
                        prefer_always_on: config.host.prefer_always_on,
                        holder: None,
                    },
                    hosts,
                    server_now,
                );
                match &placement {
                    Placement::Host(host) if *host == me => {
                        self.make_delegated(&copy, &room, &opening).await;
                    }
                    Placement::Host(_) => {}
                    Placement::Waiting { .. } => {
                        let Some(text) = placement.waiting_text() else {
                            continue;
                        };
                        let announces = announcer(hosts, &principal, server_now).as_deref()
                            == Some(me.as_str());
                        if !announces || self.pending_shown.get(&room) == Some(&text) {
                            continue;
                        }
                        let status = StatusContent {
                            v: CONTENT_VERSION,
                            session: String::new(),
                            kind: SessionKind::Delegated,
                            title: session_title(&config.id, chrono::Utc::now()),
                            agent: config.matrix_user.clone(),
                            host: me.clone(),
                            epoch: 0,
                            run: RunState::Waiting,
                            detail: None,
                            waiting: Some(text.clone()),
                            anchor: None,
                        };
                        match copy
                            .send_status(
                                &room,
                                serde_json::to_value(&status).unwrap_or(Value::Null),
                            )
                            .await
                        {
                            Ok(_) => {
                                self.pending_shown.insert(room, text);
                            }
                            Err(error) => {
                                tracing::warn!(%room, %error, "agents: a waiting delegation's status was not sent")
                            }
                        }
                    }
                }
            }
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
                taken: None,
                sent: None,
                waited: None,
                refused: None,
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
        // A scheduled card's `host:` is its session's pin (R57).
        let card_pin = cards::session_schedule(Some(&slot.agent), &slot.session.scheduled)
            .ok()
            .flatten()
            .and_then(|card| match &card.card.host {
                Some(keeper_core::agents::card::Field::Read(slug)) => {
                    Some(slug.as_str().to_owned())
                }
                _ => None,
            });
        let pin = card_pin
            .or_else(|| slot.agent.pin.clone())
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
            .is_some_and(|held| held.activity.busy.load(Ordering::Relaxed));
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
            Step::Renew => {
                self.renew(room).await;
            }
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
                return;
            }
            Step::HandBack => {
                tracing::info!(%room, ?placement, "agents: another live host is preferred; handing the session back");
                self.end(room, ClaimAction::Released, true);
                return;
            }
            Step::Nothing => {
                if let (Some(text), true, false) = (placement.waiting_text(), announces, holding) {
                    self.show(room, RunState::Waiting, epoch, Some(text), None)
                        .await;
                }
            }
        }
        self.tick_schedule(room, &placement, may, announces, epoch, stop)
            .await;
    }

    /// The session's scheduled card on this tick (92.3). Its holder first
    /// settles what the claim it took named and a run the card still says
    /// is running, every tick until the card reads settled (R163, R164);
    /// then, placed here — never while placement waits — it begins a due
    /// window: the card read now, the claim renewed naming the window, and
    /// the worker, which reads the card again under the claim before it
    /// writes `run: running` (R56, R58, [`cards::begin`]). While placement
    /// waits, a holder has the card say `run: waiting`; with no holder, the
    /// announcing host takes the claim to say it and hands it back (Q8).
    /// While the worker works or its run waits for a person nothing of
    /// this is done: the claim keeps naming the parked run's window, and the
    /// worker's own timer expires the approval before a window is judged
    /// again (R84, R177).
    async fn tick_schedule(
        &mut self,
        room: &OwnedRoomId,
        placement: &Placement,
        may: bool,
        announces: bool,
        epoch: u64,
        stop: &CancelSignal,
    ) {
        let server_now = self.clock.now();
        let me = self.host.as_str().to_owned();
        let Some(slot) = self.slots.get_mut(room) else {
            return;
        };
        let card = match cards::session_schedule(Some(&slot.agent), &slot.session.scheduled) {
            Ok(Some(card)) => card.clone(),
            Ok(None) => return,
            Err(sentence) => {
                if slot.claim.is_some() && slot.refused.as_deref() != Some(sentence.as_str()) {
                    tracing::info!(%room, %sentence, "agents: the session's scheduled card does not run here");
                    slot.refused = Some(sentence.clone());
                    self.show(room, RunState::Blocked, epoch, None, Some(sentence))
                        .await;
                }
                return;
            }
        };
        let copy = Arc::clone(&slot.copy);
        let (now_ms, offset) = (self.calendar)(server_now);
        let due_in =
            |card: &keeper_core::agents::card::CardAgent| match cards::due(card, now_ms, offset) {
                Due::Due { window_ms } => {
                    Some(claim::rfc3339(u64::try_from(window_ms).unwrap_or(0)))
                }
                _ => None,
            };
        if let Some(held) = &slot.claim {
            if busy_or_parked(held) || slot.draining.is_some() {
                return;
            }
            let lease = Arc::clone(&held.lease);
            let (dir, rel) = (slot.session.dir.clone(), card.rel.clone());
            let fresh = match tokio::task::spawn_blocking(move || cards::read_scheduled(&dir, &rel))
                .await
            {
                Ok(Ok(Some(fresh))) => fresh,
                Ok(Ok(None)) | Err(_) => return,
                Ok(Err(error)) => {
                    tracing::warn!(%room, %error, "agents: the scheduled card could not be read");
                    return;
                }
            };
            // A turn begun while the card was read is left to finish.
            let Some(slot) = self.slots.get_mut(room).filter(|slot| {
                slot.claim
                    .as_ref()
                    .is_some_and(|held| !busy_or_parked(held))
            }) else {
                return;
            };
            if let Some((host, window)) = slot.taken.clone() {
                if cards::unsettled(&fresh, window.as_deref()) {
                    copy.route_scheduled(
                        room,
                        Scheduled::TakenOver {
                            card: card.rel,
                            window,
                            host,
                        },
                    );
                    return;
                }
                slot.taken = None;
            }
            let window = due_in(&fresh);
            if !matches!(placement, Placement::Host(host) if *host == me) {
                // Holding the session for its messages is no leave to
                // begin a window (R164).
                if let (Some(window), Some(waiting)) = (window, placement.waiting_text()) {
                    if slot.waited.as_ref() != Some(&window) {
                        slot.waited = Some(window);
                        copy.route_scheduled(
                            room,
                            Scheduled::Wait {
                                card: card.rel,
                                waiting,
                            },
                        );
                    }
                }
                return;
            }
            let Some(window) = window.filter(|window| slot.sent.as_ref() != Some(window)) else {
                return;
            };
            lease.set_window(Some(window.clone()));
            if !self.renew(room).await {
                return;
            }
            if let Some(slot) = self.slots.get_mut(room).filter(|slot| slot.claim.is_some()) {
                copy.route_scheduled(
                    room,
                    Scheduled::Run {
                        card: card.rel,
                        window: window.clone(),
                        now_ms,
                        utc_offset_minutes: offset,
                    },
                );
                slot.sent = Some(window);
            }
            return;
        }
        let window = due_in(&card.card);
        let (Some(window), Some(waiting)) = (window, placement.waiting_text()) else {
            return;
        };
        if !announces || !may || slot.draining.is_some() || slot.waited.as_ref() == Some(&window) {
            return;
        }
        self.acquire(room, stop).await;
        let Some(slot) = self.slots.get_mut(room).filter(|slot| slot.claim.is_some()) else {
            return;
        };
        slot.taken = None;
        slot.waited = Some(window);
        copy.route_scheduled(
            room,
            Scheduled::Wait {
                card: card.rel,
                waiting,
            },
        );
        self.end(room, ClaimAction::Released, true);
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
        let scheduled = agent.kind == SessionKind::Scheduled;
        let acquired = if scheduled {
            claims::acquire_carrying(port.as_ref(), &me, &self.clock, &self.rtt).await
        } else {
            acquire(port.as_ref(), &me, &self.clock, &self.rtt, None).await
        };
        match acquired {
            Ok(Acquired::Won {
                lease,
                from_host,
                from_window,
            }) => {
                tracing::info!(
                    session = %session.path, %room, epoch = lease.epoch,
                    claim_event = %lease.claim_event, server_ts = %claim::rfc3339(lease.server_ts),
                    from_host = from_host.as_deref().unwrap_or(""),
                    from_window = from_window.as_deref().unwrap_or(""),
                    "agents: claim acquired"
                );
                // Every holder of a scheduled session settles what the claim
                // it took named, and a run its card still says is running,
                // before it begins a window (R163, R164) — its own earlier
                // claim's too, after a restart.
                let taken = scheduled.then(|| {
                    (
                        from_host.clone().unwrap_or_else(|| me.host.clone()),
                        from_window,
                    )
                });
                let ending = Arc::new(Mutex::new(ClaimAction::Released));
                let activity = Arc::new(crate::agent::Activity::starting());
                let worker = Arc::clone(&copy).spawn_worker(
                    &session,
                    &agent,
                    stop,
                    Claimed {
                        lease: Arc::clone(&lease),
                        from_host,
                        ending: Arc::clone(&ending),
                        activity: Arc::clone(&activity),
                    },
                );
                if let Some(slot) = self.slots.get_mut(room) {
                    // The worker may post a status of its own: the next one
                    // this host says edits whichever is latest.
                    slot.shown = None;
                    slot.anchor_read = false;
                    slot.taken = taken;
                    slot.refused = None;
                    if let Some(worker) = worker {
                        slot.claim = Some(Held {
                            lease,
                            worker,
                            ending,
                            activity,
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

    /// Renew `room`'s claim; whether the server confirmed it.
    async fn renew(&mut self, room: &OwnedRoomId) -> bool {
        let Some(slot) = self.slots.get(room) else {
            return false;
        };
        let Some(held) = &slot.claim else {
            return false;
        };
        let (copy, lease, dir) = (
            Arc::clone(&slot.copy),
            Arc::clone(&held.lease),
            slot.session.dir.clone(),
        );
        // A conflict found while holding stops service as well (S-05).
        if let Some(conflict) = conflict_in(dir).await {
            self.end(room, ClaimAction::Released, true);
            self.park_conflicted(room, conflict).await;
            return false;
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
            Ok(true) => true,
            Ok(false) => {
                tracing::warn!(%room, "agents: another host holds the session's claim now; claim lost");
                self.end(room, ClaimAction::Lost, false);
                false
            }
            Err(error) => {
                tracing::warn!(%room, %error, "agents: the claim was not renewed");
                false
            }
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
    /// the session's own status anchor when the room has one. Like every
    /// status a worker sends (R64, R160, R169): when the session's label, as
    /// its log says now, does not reach the room's members now — a known
    /// agent through its audience, anyone else as a person — or they cannot
    /// be read, it keeps `session` and says the fixed sentence with no
    /// detail, and each such status sent is audited (R65).
    async fn show(
        &mut self,
        room: &OwnedRoomId,
        run: RunState,
        epoch: u64,
        waiting: Option<String>,
        detail: Option<String>,
    ) {
        let me = self.host.as_str().to_owned();
        let Some(slot) = self.slots.get(room) else {
            return;
        };
        let copy = Arc::clone(&slot.copy);
        let label = logged_label(&slot.session.dir, &slot.agent.label);
        let mut own = vec![copy.config().matrix_user.clone()];
        if slot.agent.kind == SessionKind::Delegated {
            own.push(slot.agent.requested_by.clone());
        }
        let sinks = copy.sinks(&slot.agent);
        let suppressed = match copy.members(room).await {
            Some(members) => {
                match check_sink(
                    &label,
                    &room_audience(members, sinks.known.as_deref(), &own),
                ) {
                    SinkVerdict::Allow => None,
                    SinkVerdict::Block { reason, .. } => Some(reason),
                }
            }
            None => Some(MEMBERS_UNREAD.to_owned()),
        };
        let narrowed = suppressed.is_some();
        let Some(slot) = self.slots.get_mut(room) else {
            return;
        };
        let said = format!("{run:?} {waiting:?} {detail:?} {narrowed}");
        if slot.shown.as_deref() == Some(said.as_str()) {
            return;
        }
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
            title: if narrowed {
                NARROWED_STATUS.to_owned()
            } else {
                slot.agent.title.clone()
            },
            agent: copy.config().matrix_user.clone(),
            host: me,
            epoch,
            run,
            detail: detail.filter(|_| !narrowed),
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
                if let Some(reason) = &suppressed {
                    sinks.refused("status", "", room.as_str(), reason);
                }
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
        // A steward's room or folder half made stays named by its claim,
        // which the next start's winner adopts.
        if let Some((task, _)) = self.stewards.task.take() {
            task.abort();
        }
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

/// The agent users of `principal`'s drives among `drives`, whose manifests
/// a host believes (AD-374).
fn principal_agents<'a>(
    principal: &str,
    drives: impl IntoIterator<Item = &'a DriveView>,
) -> Vec<OwnedUserId> {
    let mut agents: Vec<OwnedUserId> = drives
        .into_iter()
        .filter(|drive| {
            drive
                .hosts
                .as_ref()
                .is_ok_and(|decl| decl.principal == principal)
        })
        .flat_map(|drive| drive.zone.homes.iter())
        .filter_map(|(_, home)| home.as_ref().ok().map(|h| h.config.matrix_user.clone()))
        .collect();
    agents.sort();
    agents.dedup();
    agents
}

/// The sessions zone `session` was found in: its folder less its
/// zone-relative path.
fn zone_of(session: &FoundSession) -> Option<PathBuf> {
    let depth = std::path::Path::new(&session.path).components().count();
    session
        .dir
        .ancestors()
        .nth(depth)
        .map(std::path::Path::to_path_buf)
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
    use std::collections::{HashSet, VecDeque};
    use std::path::Path;
    use std::sync::atomic::{AtomicBool, AtomicU64};

    use keeper_core::agents::home::parse_agent_toml;
    use keeper_core::agents::label::{Integrity, Label};
    use keeper_core::agents::log::writer::{rotate_at, ChunkWriter};
    use keeper_core::agents::log::{ClaimBody, LineBody, LogLine, LINE_VERSION};
    use keeper_core::agents::session::SessionKind;
    use keeper_core::bots::chat::{cancellation, CancelHandle};
    use matrix_sdk::ruma::{MilliSecondsSinceUnixEpoch, UInt};
    use tokio::sync::Notify;

    use super::*;
    use crate::doorbell::fake::FakeEngine;
    use crate::doorbell::{Answer, Doorbell, DriveEngine, Ringer, RING_FINISH};
    use crate::rooms::KnownAgent;
    use keeper_core::agents::events::DoorbellReason;
    use keeper_core::agents::label::Readers;

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
        config_pinned("")
    }

    /// Nixi's `agent.toml` with `[host].pin = pin`.
    fn config_pinned(pin: &str) -> AgentConfig {
        let decl = keeper_core::agents::drive::parse(&drive_toml()).expect("decl");
        parse_agent_toml(
            &format!("version = 1\nid = \"nixi\"\nname = \"Nixi\"\nkind = \"proxy\"\nmatrix_user = \"@nixi:example.org\"\nhuman = \"{PERSON}\"\n\n[model]\nbot = \"bot:openai:http://127.0.0.1:9#model\"\n\n[host]\nneeds = []\npin = \"{pin}\"\n"),
            "nixi",
            &decl,
        )
        .expect("agent.toml")
    }

    /// A brief to Nixi in `drives`, handed on by Dr Tola Grey.
    fn brief_to_nixi(drives: &[&str]) -> DelegateContent {
        use keeper_core::agents::delegation::{DelegateFrom, DelegateLimits};
        let decl = keeper_core::agents::drive::parse(&drive_toml()).expect("decl");
        DelegateContent {
            v: CONTENT_VERSION,
            id: ulid::Ulid::new().to_string(),
            from: DelegateFrom {
                agent: user("@tola:example.org"),
                drive: "tgdrive".to_owned(),
                session: "active/2026-10-04-triage".to_owned(),
                room: room(90),
            },
            to: user("@nixi:example.org"),
            brief: "Ask tgorka about Friday.".to_owned(),
            drives: drives.iter().map(|d| (*d).to_owned()).collect(),
            label: Label::opening(&decl, Integrity::Owner),
            hop: 1,
            limits: DelegateLimits {
                rounds_per_exchange: 3,
                tokens: 200_000,
            },
            card: None,
            dispatch_chain: Vec::new(),
        }
    }

    /// The session folders under `zone`'s `active/`.
    fn folders(zone: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(zone.join("active"))
            .map(|entries| {
                entries
                    .filter_map(Result::ok)
                    .map(|entry| entry.path())
                    .filter(|path| path.join("agent.toml").is_file())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// `brief`'s opening event, the `n`th, at a fixed server time.
    fn opening(brief: &DelegateContent, n: u64) -> Opening {
        Opening {
            event: OwnedEventId::try_from(format!("$brief{n}:example.org")).expect("event"),
            at: 1_759_570_000_000 + n,
            brief: Arc::new(brief.clone()),
        }
    }

    /// `room` read back by `copy` with `opening` in it, as its scan would.
    fn read_back(copy: &FakeCopy, room: &OwnedRoomId, opening: Option<Opening>) {
        copy.pending.lock().expect("lock").read_back(room, opening);
    }

    /// The folders under `zone`'s `active/`, zone-relative, with the bytes of
    /// their `agent.toml` and card.
    fn made(zone: &Path) -> Vec<(PathBuf, String, String)> {
        folders(zone)
            .into_iter()
            .map(|dir| {
                let read = |name: &str| std::fs::read_to_string(dir.join(name)).expect("file");
                (
                    dir.strip_prefix(zone).expect("in zone").to_owned(),
                    read("agent.toml"),
                    read(keeper_core::agents::delegation::CARD_FILE),
                )
            })
            .collect()
    }

    /// 92.1 acceptance 3 and R95: two hosts of the principal, each with its
    /// own checkout, each placing the delegation on itself (their pins say
    /// so), each holding the brief twice (a replay), tick at once: the room's
    /// claim, keyed by the opening, lets one make the session, and the other
    /// never does — though it cannot see that folder — however often it ticks.
    /// What either would make is the same bytes at the same path: the
    /// opening's server time, not the host's clock, dates it.
    #[tokio::test(start_paused = true)]
    async fn a_replayed_delegate_event_makes_one_session() {
        let server = Arc::new(Server::default());
        let (zone_here, zone_there) = (
            tempfile::tempdir().expect("zone"),
            tempfile::tempdir().expect("zone"),
        );
        let copy = |pin: &str, zone: &Path| {
            Arc::new(FakeCopy {
                config: config_pinned(pin),
                server: Arc::clone(&server),
                zone: Some(zone.to_owned()),
                ..fake_copy()
            })
        };
        let mut here = world_over(copy(ME, zone_here.path()), true);
        let mut there = world_over(copy(OTHER, zone_there.path()), true);
        there.rt.host = HostSlug::new(OTHER).expect("slug");
        let (child, brief) = (room(7), brief_to_nixi(&["tgdrive"]));
        for w in [&here, &there] {
            let held = || opening(&brief, 1);
            w.copy.pending.lock().expect("lock").hold(&child, held());
            w.copy.pending.lock().expect("lock").hold(&child, held());
            read_back(&w.copy, &child, Some(held()));
        }
        tokio::join!(here.tick(), there.tick());
        assert!(made(zone_here.path()).is_empty() && made(zone_there.path()).is_empty());
        tokio::time::advance(ANNOUNCE_AFTER).await;
        for _ in 0..3 {
            tokio::join!(here.tick(), there.tick());
        }

        let (mine, theirs) = (made(zone_here.path()), made(zone_there.path()));
        assert_eq!(mine.len() + theirs.len(), 1, "{mine:?} {theirs:?}");
        let created = here.copy.created.lock().expect("lock").len()
            + there.copy.created.lock().expect("lock").len();
        assert_eq!(created, 1);
        let claim = here.server().claim(&child).expect("the room's claim");
        assert!(claim.content.released);
        assert_eq!(
            claim.content.window.as_deref(),
            Some(claim::rfc3339(opening(&brief, 1).at).as_str())
        );

        // The other checkout, made regardless of the claim, holds the same.
        let (first, other) = if mine.is_empty() {
            (&theirs[0], zone_here.path())
        } else {
            (&mine[0], zone_there.path())
        };
        create_delegated(
            other,
            &config(),
            &child,
            &brief,
            made_at(&opening(&brief, 1)),
        )
        .expect("made");
        assert_eq!(made(other)[0], *first);
        assert!(first.1.contains(&brief.id), "{}", first.1);
    }

    /// R93: the first brief held for a room is its opening, and rounds that
    /// arrive before the first tick never replace it; a later round held
    /// live before the room was read back gives way to the opening the
    /// read-back finds. Each session's card is its opening's.
    #[tokio::test(start_paused = true)]
    async fn later_rounds_never_replace_the_opening_brief() {
        let zone = tempfile::tempdir().expect("zone");
        let copy = Arc::new(FakeCopy {
            zone: Some(zone.path().to_owned()),
            ..fake_copy()
        });
        let mut w = world_over(copy, true);
        let opened = || DelegateContent {
            card: Some(keeper_core::agents::delegation::DelegateCard {
                title: "Friday".to_owned(),
                schedule: None,
                workflow: None,
            }),
            ..brief_to_nixi(&["tgdrive"])
        };
        let round = |first: &DelegateContent, n: u64, text: &str| {
            opening(
                &DelegateContent {
                    brief: text.to_owned(),
                    card: None,
                    ..first.clone()
                },
                n,
            )
        };
        let (live, restarted) = (room(7), room(8));
        let (one, two) = (opened(), opened());
        {
            let mut pending = w.copy.pending.lock().expect("lock");
            pending.hold(&live, opening(&one, 1));
            pending.hold(&live, round(&one, 2, "And Monday."));
            pending.hold(&live, round(&one, 3, "And Tuesday."));
            pending.read_back(&live, None);
            pending.hold(&restarted, round(&two, 5, "And Monday."));
            pending.read_back(&restarted, Some(opening(&two, 4)));
        }
        w.tick().await;
        tokio::time::advance(ANNOUNCE_AFTER).await;
        w.tick().await;

        assert_eq!(*w.copy.created.lock().expect("lock"), vec![live, restarted]);
        let made = made(zone.path());
        assert_eq!(made.len(), 2, "{made:?}");
        for (_, _, card) in &made {
            let (fields, _) = keeper_core::notes::frontmatter::Frontmatter::parse(card);
            assert_eq!(fields.as_string("title"), Some("Friday"), "{card}");
            assert!(card.ends_with("\nAsk tgorka about Friday.\n"), "{card}");
        }
    }

    /// A room whose read-back failed is not taken for a room with no brief:
    /// it stays unread, its live hold waits, and the next tick reads it
    /// again and makes the session — no restart needed.
    #[tokio::test(start_paused = true)]
    async fn a_failed_read_back_is_read_again_on_the_next_tick() {
        let zone = tempfile::tempdir().expect("zone");
        let copy = Arc::new(FakeCopy {
            zone: Some(zone.path().to_owned()),
            ..fake_copy()
        });
        let mut w = world_over(copy, true);
        let (child, brief) = (room(7), brief_to_nixi(&["tgdrive"]));
        w.copy.reads.lock().expect("lock").insert(
            child.clone(),
            VecDeque::from([
                Err("messages: 502".to_owned()),
                Ok(Some(opening(&brief, 1))),
            ]),
        );
        // The failed read: nothing held, the room still unread.
        w.tick().await;
        assert!(!w.copy.pending.lock().expect("lock").is_read(&child));
        assert!(folders(zone.path()).is_empty());
        tokio::time::advance(ANNOUNCE_AFTER).await;
        w.tick().await;
        assert_eq!(*w.copy.created.lock().expect("lock"), vec![child]);
        assert_eq!(folders(zone.path()).len(), 1);
    }

    /// The held briefs are bounded like the router's rooms: with the target
    /// pinned to a host that is not live, a room past the bound — read back
    /// or arriving live — is left unread rather than kept or forgotten, and
    /// is read back once a held room's session exists.
    #[tokio::test(start_paused = true)]
    async fn held_briefs_are_bounded_and_the_rest_read_back_later() {
        let copy = Arc::new(FakeCopy {
            config: config_pinned(OTHER),
            ..fake_copy()
        });
        let mut w = world_over(copy, true);
        let rooms: Vec<OwnedRoomId> = (0..=PENDING_ROOMS as u32).map(|n| room(100 + n)).collect();
        {
            let mut reads = w.copy.reads.lock().expect("lock");
            for (n, child) in rooms.iter().enumerate() {
                let brief = brief_to_nixi(&["tgdrive"]);
                reads.insert(
                    child.clone(),
                    VecDeque::from([Ok(Some(opening(&brief, n as u64)))]),
                );
            }
        }
        w.tick().await;
        let held = w.copy.pending_delegates();
        assert_eq!(held.len(), PENDING_ROOMS);
        let left: Vec<&OwnedRoomId> = rooms
            .iter()
            .filter(|room| !w.copy.pending.lock().expect("lock").is_read(room))
            .collect();
        assert_eq!(left.len(), 1, "{left:?}");
        // A brief admitted live past the bound is left for its read-back too.
        let live = room(500);
        {
            let mut pending = w.copy.pending.lock().expect("lock");
            pending.hold(&live, opening(&brief_to_nixi(&["tgdrive"]), 500));
            assert!(!pending.held.contains_key(&live) && !pending.is_read(&live));
        }
        let last = left[0].clone();
        // The server answers its read every tick it is read again.
        let brief = brief_to_nixi(&["tgdrive"]);
        w.copy.reads.lock().expect("lock").insert(
            last.clone(),
            VecDeque::from([
                Ok(Some(opening(&brief, 999))),
                Ok(Some(opening(&brief, 999))),
            ]),
        );

        // electra made one held room's session; the rescan here found it.
        w.offer(&held[0].0, None);
        w.tick().await;
        w.tick().await;
        let now: Vec<OwnedRoomId> = w
            .copy
            .pending_delegates()
            .into_iter()
            .map(|(room, _)| room)
            .collect();
        assert_eq!(
            now.len(),
            PENDING_ROOMS,
            "last {last} held[0] {}",
            held[0].0
        );
        assert!(now.contains(&last) && !now.contains(&held[0].0));
    }

    /// 92.1 acceptance 7, the target's half: with the agent pinned to a
    /// host that is not live, the joined room's status says `waiting:
    /// <host> — <need>` once, from the announcing host, and nothing is made.
    #[tokio::test(start_paused = true)]
    async fn a_delegation_nobody_can_serve_says_what_it_waits_for() {
        let zone = tempfile::tempdir().expect("zone");
        let copy = Arc::new(FakeCopy {
            config: config_pinned(OTHER),
            zone: Some(zone.path().to_owned()),
            ..fake_copy()
        });
        let mut w = world_over(copy, true);
        let child = room(8);
        read_back(
            &w.copy,
            &child,
            Some(opening(&brief_to_nixi(&["tgdrive"]), 1)),
        );
        w.tick().await;
        tokio::time::advance(ANNOUNCE_AFTER).await;
        w.tick().await;
        w.tick().await;
        let statuses = w.server().statuses();
        assert_eq!(statuses.len(), 1, "{statuses:?}");
        assert_eq!(statuses[0]["run"], "waiting");
        assert_eq!(statuses[0]["waiting"], "electra — a live host");
        assert_eq!(statuses[0]["kind"], "delegated");
        assert!(folders(zone.path()).is_empty());
        assert_eq!(w.copy.pending_delegates().len(), 1, "still waiting");
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
        /// A doorbell send never answers.
        stall_doorbells: AtomicBool,
        /// A claim send fails.
        fail_claims: AtomicBool,
        /// Every room the steward's user made for a duty, as every copy of
        /// her sees it, and the ones a copy left.
        steward_rooms: Mutex<Vec<OwnedRoomId>>,
        left: Mutex<Vec<OwnedRoomId>>,
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
        key: String,
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
                    .put(&self.room, CLAIM, &self.key, &self.sender, content))
            })
        }

        fn read(&self) -> ClaimFuture<'_, Result<Option<ServerState>, AgentMatrixError>> {
            Box::pin(async move { Ok(self.server.get(&self.room, CLAIM, &self.key)) })
        }

        fn next_sync(&self) -> ClaimFuture<'_, Duration> {
            Box::pin(async { Duration::from_millis(1) })
        }
    }

    /// One read-back of a room: a failure, or the opening found.
    type ReadBack = Result<Option<Opening>, String>;

    /// A served room's session path, agent and lease.
    type Served = (String, SessionAgent, Arc<Lease>);

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
        /// What the copy holds for placement, as [`Copy`] holds it.
        pending: Mutex<PendingBriefs>,
        /// Each joined delegated room's read-backs to come, in order: a
        /// failure, or the opening found (none once they run out).
        reads: Mutex<BTreeMap<OwnedRoomId, VecDeque<ReadBack>>>,
        /// The sessions zone delegated sessions are made in.
        zone: Option<PathBuf>,
        /// Every room a delegated session was made for, in order.
        created: Mutex<Vec<OwnedRoomId>>,
        /// The control rooms this copy is in, beside its principal's.
        controls: Mutex<Vec<OwnedRoomId>>,
        /// Each room's members, as the server would list them.
        members: Mutex<HashMap<OwnedRoomId, BTreeSet<OwnedUserId>>>,
        /// Each served room's session path, agent and lease, as its worker
        /// holds them.
        served: Mutex<HashMap<OwnedRoomId, Served>>,
        /// What the host's clock routed, and what the worker's
        /// [`cards::begin`] made of it in the zone, in order.
        routed: Mutex<Vec<(Scheduled, Result<cards::Begun, String>)>>,
        /// How many routed asks from now on fail as a refused guarded write
        /// does, the card left as it is.
        refuse_writes: std::sync::atomic::AtomicUsize,
        /// A turn that begins never ends: the host dies in it, its card
        /// left `run: running`.
        turns_die: AtomicBool,
        /// Each closed session handed to a harvest session's worker.
        harvests: Mutex<Vec<(OwnedRoomId, String)>>,
        /// What the harvest worker said, as [`Copy`]'s acks hold it.
        acks: Mutex<Vec<(String, bool)>>,
        /// A steward room's creation never answers.
        stall_steward_rooms: bool,
        /// A steward folder cannot be written: the host stops between its
        /// room and its folder.
        fail_steward_folders: bool,
        /// The agents this host knows, as [`Copy`] reads them now.
        known: Option<Arc<crate::rooms::Known>>,
        /// Where its sessions' audit rows are written.
        data_dir: tempfile::TempDir,
        /// A run that begins parks on a person, as a worker with a decision
        /// source does: its card says `run: blocked` and the worker that it
        /// waits.
        parks: AtomicBool,
        /// A worker that starts finds a run parked in the session's
        /// `approvals/`, as `resume_approvals` does at serve start.
        parked_at_start: AtomicBool,
        /// Each served room's worker activity, as its host reads it.
        activities: Mutex<HashMap<OwnedRoomId, Arc<crate::agent::Activity>>>,
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
                let stall = if event_type == DOORBELL {
                    &self.server.stall_doorbells
                } else {
                    &self.server.stall_manifests
                };
                if stall.load(Ordering::Relaxed) {
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
            self.keyed_claims(room, "")
        }

        fn keyed_claims<'a>(
            &'a self,
            room: &'a OwnedRoomId,
            key: &'a str,
        ) -> Box<dyn ClaimPort + 'a> {
            Box::new(FakeClaims {
                server: &self.server,
                room: room.clone(),
                key: key.to_owned(),
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

        fn sinks(&self, session: &SessionAgent) -> Sinks {
            Sinks {
                data_dir: self.data_dir.path().to_path_buf(),
                provider_id: "provider".to_owned(),
                bot_id: "bot".to_owned(),
                session_id: session.id.to_string(),
                known: self.known.clone(),
                home_drive: "tgdrive".to_owned(),
                zone: PathBuf::new(),
                doors: None,
            }
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
            session: &FoundSession,
            agent: &SessionAgent,
            _stop: &CancelSignal,
            claimed: Claimed,
        ) -> Option<JoinHandle<()>> {
            self.served.lock().expect("lock").insert(
                agent.room.clone(),
                (
                    session.path.clone(),
                    agent.clone(),
                    Arc::clone(&claimed.lease),
                ),
            );
            let closed = Arc::new(Notify::new());
            self.workers
                .lock()
                .expect("lock")
                .insert(agent.room.clone(), Arc::clone(&closed));
            self.spawned.lock().expect("lock").push(agent.room.clone());
            // The worker's start, as `serve_arrivals`'s: what waited is
            // settled — a parked run found — before busy clears.
            claimed.activity.parked.store(
                self.parked_at_start.load(Ordering::SeqCst),
                Ordering::SeqCst,
            );
            claimed.activity.busy.store(false, Ordering::SeqCst);
            self.activities
                .lock()
                .expect("lock")
                .insert(agent.room.clone(), Arc::clone(&claimed.activity));
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

        fn pending_delegates(&self) -> Vec<(OwnedRoomId, Opening)> {
            self.pending.lock().expect("lock").ready()
        }

        fn drop_pending(&self, room: &RoomId) {
            self.pending.lock().expect("lock").forget(room);
        }

        fn create_delegated<'a>(
            &'a self,
            room: &'a OwnedRoomId,
            opening: &'a Opening,
        ) -> ClaimFuture<'a, Result<(), String>> {
            Box::pin(async move {
                let zone = self.zone.as_ref().ok_or("no zone")?;
                create_delegated(zone, &self.config, room, &opening.brief, made_at(opening))?;
                self.created.lock().expect("lock").push(room.clone());
                Ok(())
            })
        }

        fn recover_briefs<'a>(&'a self, served: HashSet<OwnedRoomId>) -> ClaimFuture<'a, ()> {
            Box::pin(async move {
                let rooms: Vec<OwnedRoomId> = self
                    .reads
                    .lock()
                    .expect("lock")
                    .keys()
                    .filter(|room| !served.contains(*room))
                    .cloned()
                    .collect();
                read_back_rooms(&self.pending, rooms, |room| {
                    let next = self
                        .reads
                        .lock()
                        .expect("lock")
                        .get_mut(&room)
                        .and_then(VecDeque::pop_front)
                        .unwrap_or(Ok(None));
                    std::future::ready(next)
                })
                .await;
            })
        }

        fn control_rooms(&self) -> Vec<OwnedRoomId> {
            self.controls.lock().expect("lock").clone()
        }

        fn members<'a>(
            &'a self,
            room: &'a RoomId,
        ) -> ClaimFuture<'a, Option<BTreeSet<OwnedUserId>>> {
            Box::pin(async move { self.members.lock().expect("lock").get(room).cloned() })
        }

        fn synced(&self) -> bool {
            true
        }

        fn doorbells(&self) -> ClaimFuture<'_, Vec<(OwnedUserId, String, Value)>> {
            Box::pin(async move {
                let joined = self.joined.lock().expect("lock").clone();
                self.server
                    .states
                    .lock()
                    .expect("lock")
                    .iter()
                    .filter(|((room, kind, _), _)| kind == DOORBELL && joined.contains(room))
                    .map(|((_, _, key), state)| {
                        (state.sender.clone(), key.clone(), state.content.clone())
                    })
                    .collect()
            })
        }

        /// The worker's half, as `ServedSession::scheduled` does it: the
        /// card read and written under the claim, and a run's turn ending
        /// in `review`.
        fn route_scheduled(&self, room: &RoomId, scheduled: Scheduled) {
            // One refusal is spent per routed ask while any are left (a
            // compare-and-swap loop: `fetch_update` is deprecated from
            // Rust 1.99, and its replacement does not exist before it).
            let mut left = self.refuse_writes.load(Ordering::Relaxed);
            let refused = loop {
                if left == 0 {
                    break false;
                }
                match self.refuse_writes.compare_exchange_weak(
                    left,
                    left - 1,
                    Ordering::Relaxed,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => break true,
                    Err(now) => left = now,
                }
            };
            let host = self.server.claim(room).map(|claim| claim.content.host);
            let begun = match (
                &self.zone,
                self.served.lock().expect("lock").get(room),
                host,
            ) {
                _ if refused => Err("the card changed under the write".to_owned()),
                (Some(zone), Some((session, agent, lease)), Some(host)) => {
                    let names = match &scheduled {
                        Scheduled::Run { window, .. } => Some(window.clone()),
                        _ => None,
                    };
                    let may_write = || {
                        lease.may_write()
                            && names
                                .as_ref()
                                .is_none_or(|w| lease.window().as_ref() == Some(w))
                    };
                    let holder = cards::Holder { agent, host: &host };
                    let begun = cards::begin(zone, session, &holder, &scheduled, &may_write)
                        .map_err(|error| error.to_string());
                    if let Ok(cards::Begun::Run { .. }) = &begun {
                        if self.parks.load(Ordering::SeqCst) {
                            cards::write_run(
                                zone,
                                session,
                                scheduled.card(),
                                keeper_core::agents::card::Run::Blocked,
                                None,
                                &|| lease.may_write(),
                            )
                            .expect("the park's card");
                            if let Some(activity) = self.activities.lock().expect("lock").get(room)
                            {
                                activity.parked.store(true, Ordering::SeqCst);
                            }
                        } else if !self.turns_die.load(Ordering::Relaxed) {
                            cards::write_run(
                                zone,
                                session,
                                scheduled.card(),
                                keeper_core::agents::card::Run::Review,
                                None,
                                &|| lease.may_write(),
                            )
                            .expect("the turn's end");
                        }
                    }
                    begun
                }
                _ => Err("no worker serves the room".to_owned()),
            };
            self.routed.lock().expect("lock").push((scheduled, begun));
        }

        fn route_harvest(&self, room: &RoomId, closed: Closed) {
            self.harvests
                .lock()
                .expect("lock")
                .push((room.to_owned(), closed.id));
        }

        fn harvest_acks(&self) -> Vec<(String, bool)> {
            std::mem::take(&mut *self.acks.lock().expect("lock"))
        }

        fn steward_found(&self, duty: Duty) -> ClaimFuture<'_, Result<bool, String>> {
            Box::pin(async move {
                let id = stewards::session_id(&self.config, duty).to_string();
                Ok(self
                    .zone
                    .as_ref()
                    .is_some_and(|zone| crate::sessions::verbs::find(zone, &id).is_some()))
            })
        }

        fn steward_room(&self, _duty: Duty) -> ClaimFuture<'_, Result<OwnedRoomId, String>> {
            Box::pin(async move {
                if self.stall_steward_rooms {
                    std::future::pending::<()>().await;
                }
                let mut rooms = self.server.steward_rooms.lock().expect("lock");
                let room = OwnedRoomId::try_from(format!("!duty{}:example.org", rooms.len()))
                    .expect("room");
                rooms.push(room.clone());
                Ok(room)
            })
        }

        fn steward_folder<'a>(
            &'a self,
            duty: Duty,
            room: &'a RoomId,
        ) -> ClaimFuture<'a, Result<bool, String>> {
            Box::pin(async move {
                if self.fail_steward_folders {
                    return Err("the disk is full".to_owned());
                }
                let zone = self.zone.as_ref().ok_or("no zone")?;
                let decl = keeper_core::agents::drive::parse(&drive_toml()).expect("decl");
                let now = chrono::Local::now();
                let agent = stewards::session(&self.config, &decl, duty, room, now);
                crate::seed::make_folder(
                    zone,
                    &agent,
                    vec![(duty.card_file(), "card\n".to_owned())],
                    false,
                    now,
                )
                .map(|settled| settled.folder_made)
            })
        }

        fn steward_orphans<'a>(
            &'a self,
            _duty: Duty,
            keep: Option<&'a RoomId>,
        ) -> ClaimFuture<'a, ()> {
            Box::pin(async move {
                let made = self.server.steward_rooms.lock().expect("lock").clone();
                let mut left = self.server.left.lock().expect("lock");
                for room in made {
                    if keep != Some(room.as_ref()) && !left.contains(&room) {
                        left.push(room);
                    }
                }
            })
        }
    }

    struct World {
        root: tempfile::TempDir,
        copy: Arc<FakeCopy>,
        rt: HostRuntime,
        stop: CancelSignal,
        _cancel: CancelHandle,
        /// What the host's calendar reads, epoch ms (UTC).
        now: Arc<std::sync::atomic::AtomicI64>,
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
            clock: Arc::default(),
            rtt: Arc::default(),
            manifest_sent: None,
            first_published: None,
            pending_shown: HashMap::new(),
            calendar: server_calendar(),
            harvesters: HashMap::new(),
            stewards: Stewarding::default(),
        };
        let (cancel, stop) = cancellation();
        let now = Arc::new(std::sync::atomic::AtomicI64::new(wall_ms() as i64));
        let mut w = World {
            root: tempfile::tempdir().expect("tempdir"),
            copy,
            rt,
            stop,
            _cancel: cancel,
            now: Arc::clone(&now),
        };
        w.rt.calendar = Arc::new(move |_| (now.load(Ordering::Relaxed), 0));
        w
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
            self.copy
                .members
                .lock()
                .expect("lock")
                .entry(room.clone())
                .or_insert_with(|| [user(PERSON)].into());
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
                dispatch_chain: Vec::new(),
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
                scheduled: cards::ScheduledScan::default(),
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

    impl World {
        /// The rescan finds Nixi's harvest session serving `room`, its
        /// folder in the zone `zone` with an empty baseline.
        fn offer_harvest(&mut self, room: &OwnedRoomId, zone: &Path) {
            self.copy.joined.lock().expect("lock").insert(room.clone());
            let decl = keeper_core::agents::drive::parse(&drive_toml()).expect("decl");
            let agent = stewards::session(
                &self.copy.config,
                &decl,
                Duty::Harvest,
                room,
                chrono::Local::now(),
            );
            let path = "active/2026-10-05-harvest".to_owned();
            let dir = zone.join(&path);
            std::fs::create_dir_all(&dir).expect("dir");
            std::fs::write(dir.join(stewards::BASELINE), "\n").expect("baseline");
            let session = FoundSession {
                path,
                dir,
                agent: Ok(agent.clone()),
                scheduled: cards::ScheduledScan::default(),
            };
            self.rt.offer(&self.copy, &session, &agent);
        }

        fn handed(&self) -> Vec<String> {
            self.copy
                .harvests
                .lock()
                .expect("lock")
                .iter()
                .map(|(_, id)| id.clone())
                .collect()
        }
    }

    /// A person's session `id`, archived in `zone`.
    fn archived(zone: &Path, id: &str) {
        let dir = zone.join(format!("archive/2026/2026-10-06-{id}"));
        std::fs::create_dir_all(&dir).expect("dir");
        std::fs::write(
            dir.join("README.md"),
            format!("---\nid: {id}\n---\n\n# {id}\n"),
        )
        .expect("record");
    }

    /// R61 with acknowledgement: a closed session is handed to the held
    /// harvest session's worker once; when the worker says it failed before
    /// its turn began (an index or log error) it is handed again after
    /// [`stewards::RETRY`], while the same worker and claim stay; once it
    /// says the harvest is settled it is never handed again.
    #[tokio::test(start_paused = true)]
    async fn a_harvest_that_failed_before_it_began_is_handed_again() {
        let zone = tempfile::tempdir().expect("zone");
        let mut w = world(Duration::ZERO, true);
        let mine = room(1);
        w.offer_harvest(&mine, zone.path());
        archived(zone.path(), "taxes");
        w.tick().await;
        assert!(w.held_by_me(&mine));
        w.tick().await;
        assert_eq!(w.handed(), ["taxes"]);

        w.copy
            .acks
            .lock()
            .expect("lock")
            .push(("taxes".to_owned(), false));
        w.tick().await;
        assert_eq!(w.handed(), ["taxes"], "not before the retry");
        tokio::time::advance(stewards::RETRY).await;
        w.tick().await;
        assert_eq!(w.handed(), ["taxes", "taxes"]);

        w.copy
            .acks
            .lock()
            .expect("lock")
            .push(("taxes".to_owned(), true));
        for _ in 0..3 {
            tokio::time::advance(stewards::RETRY).await;
            w.tick().await;
        }
        assert_eq!(w.handed(), ["taxes", "taxes"]);
    }

    /// Two hosts of the steward, each with its own checkout, `fake` making
    /// what each copy does.
    fn steward_hosts(
        server: &Arc<Server>,
        zones: [&Path; 2],
        fake: impl Fn(FakeCopy) -> FakeCopy,
    ) -> [World; 2] {
        zones.map(|zone| {
            let copy = Arc::new(fake(FakeCopy {
                server: Arc::clone(server),
                zone: Some(zone.to_owned()),
                ..fake_copy()
            }));
            let mut w = world_over(Arc::clone(&copy), true);
            w.rt.stewards
                .due
                .push((copy as Arc<dyn CopyPort>, Duty::Triage));
            w
        })
    }

    /// The creation claim of Nixi's triage session, in the control room,
    /// and the room its record names.
    fn duty_claim(server: &Server) -> Option<(ServerClaim, Option<String>)> {
        let key = stewards::session_id(&config(), Duty::Triage).to_string();
        let record = server
            .get(&control(), STEWARD_ROOM, &key)
            .and_then(|state| state.content["room"].as_str().map(str::to_owned));
        server
            .get(&control(), CLAIM, &key)
            .map(|state| (ServerClaim::read(&state).expect("claim"), record))
    }

    /// Ticks both hosts at once, `rounds` bootstrap rounds.
    async fn tick_both(hosts: &mut [World; 2], rounds: usize) {
        for _ in 0..rounds {
            let [a, b] = hosts;
            tokio::join!(a.tick(), b.tick());
            tokio::time::advance(STEWARD_AGAIN).await;
            tokio::task::yield_now().await;
        }
    }

    /// R165: two hosts of a steward, each with its own checkout and
    /// neither seeing the other's folder, start together. The claim keyed
    /// by her triage session's id in the control room lets one make the
    /// room and the folder; the other makes nothing, however often it
    /// tries — one room, one folder, the claim handed back, the room named.
    #[tokio::test(start_paused = true)]
    async fn a_stewards_session_is_made_once_across_two_hosts() {
        let server = Arc::new(Server::default());
        let (here, there) = (
            tempfile::tempdir().expect("zone"),
            tempfile::tempdir().expect("zone"),
        );
        let mut hosts = steward_hosts(&server, [here.path(), there.path()], |copy| copy);
        hosts[1].rt.host = HostSlug::new(OTHER).expect("slug");
        tick_both(&mut hosts, 4).await;

        let rooms = server.steward_rooms.lock().expect("lock").clone();
        assert_eq!(rooms.len(), 1, "{rooms:?}");
        let made = [here.path(), there.path()]
            .iter()
            .filter(|zone| !folders(zone).is_empty())
            .count();
        assert_eq!(made, 1);
        let (claim, record) = duty_claim(&server).expect("the creation claim");
        assert!(claim.content.released);
        assert_eq!(record.as_deref(), Some(rooms[0].as_str()));
        assert!(server.left.lock().expect("lock").is_empty());
    }

    /// R165: a host with no control room has nowhere to claim a duty, so it
    /// makes no room and no folder however long it runs, and keeps the duty
    /// due; once it has the room, the session is made.
    #[tokio::test(start_paused = true)]
    async fn a_host_with_no_control_room_makes_no_stewards_session_until_it_has_one() {
        let server = Arc::new(Server::default());
        let zone = tempfile::tempdir().expect("zone");
        let [mut w, _] = steward_hosts(&server, [zone.path(), zone.path()], |copy| copy);
        w.rt.control_room = None;
        for _ in 0..4 {
            w.tick().await;
            tokio::time::advance(STEWARD_AGAIN).await;
            tokio::task::yield_now().await;
        }
        assert!(server.steward_rooms.lock().expect("lock").is_empty());
        assert!(folders(zone.path()).is_empty());
        assert_eq!(w.rt.stewards.due.len(), 1, "still due");

        w.rt.set_control_room(control());
        for _ in 0..4 {
            w.tick().await;
            tokio::time::advance(STEWARD_AGAIN).await;
            tokio::task::yield_now().await;
        }
        assert_eq!(server.steward_rooms.lock().expect("lock").len(), 1);
        assert_eq!(folders(zone.path()).len(), 1);
    }

    /// R165, a crash: electra made the room, recorded it and stopped
    /// before the folder; an earlier attempt's room was never recorded.
    /// Once electra's claim lapsed, hesperia adopts the recorded room — its
    /// folder names it — leaves the other, and makes no third.
    #[tokio::test(start_paused = true)]
    async fn a_room_made_before_a_crash_is_adopted_and_an_unnamed_one_left() {
        let server = Arc::new(Server::default());
        let zone = tempfile::tempdir().expect("zone");
        let (named, unnamed) = (
            OwnedRoomId::try_from("!named:example.org").expect("room"),
            OwnedRoomId::try_from("!unnamed:example.org").expect("room"),
        );
        server
            .steward_rooms
            .lock()
            .expect("lock")
            .extend([unnamed.clone(), named.clone()]);
        let key = stewards::session_id(&config(), Duty::Triage).to_string();
        let lapsed = wall_ms() - 600_000;
        let electra = Claimant {
            host: OTHER.to_owned(),
            device: "ELECTRA1".to_owned(),
            agent: user("@nixi:example.org"),
        };
        server.put(
            &control(),
            CLAIM,
            &key,
            &user("@nixi:example.org"),
            serde_json::to_value(electra.content(1, lapsed, lapsed, false, None)).expect("claim"),
        );
        server.put(
            &control(),
            STEWARD_ROOM,
            &key,
            &user("@nixi:example.org"),
            json!({ "v": 1, "room": named }),
        );
        server
            .states
            .lock()
            .expect("lock")
            .get_mut(&(control(), CLAIM.to_owned(), key))
            .expect("the claim")
            .origin_server_ts = MilliSecondsSinceUnixEpoch(UInt::new(lapsed).expect("ts"));
        let [mut w, _] = steward_hosts(&server, [zone.path(), zone.path()], |copy| copy);
        for _ in 0..3 {
            w.tick().await;
            tokio::time::advance(STEWARD_AGAIN).await;
            tokio::task::yield_now().await;
        }

        let toml = std::fs::read_to_string(folders(zone.path())[0].join("agent.toml"))
            .expect("the folder");
        let agent = keeper_core::agents::session::parse_session_agent_toml(&toml).expect("parse");
        assert_eq!(agent.room, named);
        assert_eq!(*server.left.lock().expect("lock"), [unnamed]);
        assert_eq!(server.steward_rooms.lock().expect("lock").len(), 2);
        let (claim, _) = duty_claim(&server).expect("claim");
        assert!(claim.content.released && claim.content.host == ME);
    }

    /// A steward's room whose creation never answers holds neither the
    /// tick nor the claims it renews: the bootstrap runs beside them, and
    /// a shutdown does not wait for it.
    #[tokio::test(start_paused = true)]
    async fn a_stalled_steward_bootstrap_does_not_hold_the_lease_clock() {
        let server = Arc::new(Server::default());
        let zone = tempfile::tempdir().expect("zone");
        let [mut w, _] = steward_hosts(&server, [zone.path(), zone.path()], |copy| FakeCopy {
            stall_steward_rooms: true,
            ..copy
        });
        let a = room(1);
        w.offer(&a, None);
        w.tick().await;
        tokio::time::advance(STEWARD_AGAIN).await;
        for _ in 0..4 {
            let started = Instant::now();
            w.tick().await;
            assert!(
                started.elapsed() < Duration::from_secs(1),
                "the tick waited"
            );
            tokio::time::advance(RENEW_EVERY).await;
        }
        assert!(w.held_by_me(&a));
        assert!(
            w.rt.stewards.task.is_some(),
            "the bootstrap is still waiting"
        );
        tokio::time::timeout(Duration::from_secs(1), w.rt.release_all())
            .await
            .expect("the shutdown does not wait for the bootstrap");
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
            pending: Mutex::default(),
            reads: Mutex::default(),
            zone: None,
            created: Mutex::default(),
            controls: Mutex::default(),
            members: Mutex::default(),
            served: Mutex::default(),
            routed: Mutex::default(),
            refuse_writes: Default::default(),
            turns_die: AtomicBool::new(false),
            parks: AtomicBool::new(false),
            parked_at_start: AtomicBool::new(false),
            activities: Mutex::default(),
            harvests: Mutex::default(),
            acks: Mutex::default(),
            stall_steward_rooms: false,
            fail_steward_folders: false,
            known: None,
            data_dir: tempfile::tempdir().expect("data directory"),
        }
    }

    /// A drive's checkout at `root`, its zones at their defaults, read by
    /// `readers`.
    fn drive_view(root: &Path, id: &str, readers: &[&str]) -> DriveView {
        let mut profile = keeper_sync::SyncProfile::new(
            id.to_owned(),
            id.to_owned(),
            root.to_path_buf(),
            String::new(),
        );
        profile.sessions = Some(Default::default());
        profile.agents = Some(Default::default());
        let readers: Vec<String> = readers.iter().map(|r| format!("\"{r}\"")).collect();
        let decl = keeper_core::agents::drive::parse(&format!(
            "version = 1\nid = \"{id}\"\ntitle = \"{id}\"\nprincipal = \"tgorka\"\nowner = \"{PERSON}\"\nreaders = [{}]\n",
            readers.join(", ")
        ))
        .expect("decl");
        runtime::view(id, profile, Ok(decl))
    }

    /// A git repository at `dir`, committed by hand: `commit` writes files
    /// and answers the new commit's id. `None` with no `git`.
    struct Repo<'a> {
        dir: &'a Path,
    }

    impl Repo<'_> {
        fn git(&self, args: &[&str]) -> Option<String> {
            let out = std::process::Command::new("git")
                .current_dir(self.dir)
                .env("GIT_CONFIG_NOSYSTEM", "1")
                .env("GIT_CONFIG_GLOBAL", "/dev/null")
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@example.invalid")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@example.invalid")
                .args(args)
                .output()
                .ok()?;
            out.status
                .success()
                .then(|| String::from_utf8_lossy(&out.stdout).trim().to_owned())
        }

        fn init(dir: &Path) -> Option<Repo<'_>> {
            let repo = Repo { dir };
            repo.git(&["init", "-q", "-b", "main"])?;
            Some(repo)
        }

        fn commit(&self, files: &[(&str, &str)]) -> String {
            for (rel, text) in files {
                let path = self.dir.join(rel);
                std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
                std::fs::write(path, text).expect("write");
            }
            self.git(&["add", "-A"]).expect("add");
            self.git(&["commit", "-q", "-m", "c"]).expect("commit");
            self.git(&["rev-parse", "HEAD"]).expect("head")
        }
    }

    /// A session `agent.toml` naming `room`.
    fn session_toml(room: &OwnedRoomId) -> String {
        let decl = keeper_core::agents::drive::parse(&drive_toml()).expect("decl");
        keeper_core::agents::session::compose_session_agent_toml(&SessionAgent {
            id: ulid::Ulid::new(),
            agent: "nixi".to_owned(),
            drive: "tgdrive".to_owned(),
            kind: SessionKind::Conversation,
            title: "work".to_owned(),
            requested_by: user(PERSON),
            parent: None,
            room: room.clone(),
            drives: vec!["tgdrive".to_owned()],
            label: Label::opening(&decl, Integrity::Owner),
            needs: None,
            pin: None,
            hop: 0,
            dispatch_chain: Vec::new(),
            limits: None,
            workflow: None,
            created_at: chrono::Utc::now(),
        })
    }

    fn ringing_runtime(copy: &Arc<FakeCopy>, control: OwnedRoomId) -> HostRuntime {
        let mut w = world_over(Arc::clone(copy), false);
        w.rt.control_room = Some(control);
        w.rt
    }

    fn doorbells(server: &Server) -> usize {
        server
            .sends
            .lock()
            .expect("lock")
            .iter()
            .filter(|send| send.starts_with(DOORBELL))
            .count()
    }

    /// 92.4 acceptance 4: who rings, and where.
    #[tokio::test]
    async fn a_push_rings_only_for_agent_work() {
        let root = tempfile::tempdir().expect("root");
        let Some(repo) = Repo::init(root.path()) else {
            return;
        };
        let (session, shared) = (room(1), room(2));
        let copy = Arc::new(fake_copy());
        for (room, people) in [
            (&session, vec![PERSON]),
            (&shared, vec![PERSON, "@marta:example.org"]),
            (&control(), vec![PERSON]),
        ] {
            copy.joined.lock().expect("lock").insert(room.clone());
            copy.members
                .lock()
                .expect("lock")
                .insert(room.clone(), people.into_iter().map(user).collect());
        }
        let rt = ringing_runtime(&copy, control());
        let drives = [drive_view(root.path(), "tgdrive", &[PERSON])];
        let engine: Arc<dyn DriveEngine> = Arc::new(FakeEngine {
            repo: Some(root.path().to_path_buf()),
            ..FakeEngine::default()
        });
        let mut from = repo.commit(&[("README.md", "seed")]);
        let toml = session_toml(&session);
        let shared_toml = session_toml(&shared);
        // What one push writes, and the rooms it must ring.
        type Step<'a> = (&'a [(&'a str, &'a str)], Vec<(OwnedRoomId, DoorbellReason)>);
        let steps: [Step; 6] = [
            (
                &[
                    ("60-sessions/active/a/agent.toml", toml.as_str()),
                    ("60-sessions/active/a/brief.md", "card"),
                ],
                vec![(session.clone(), DoorbellReason::Session)],
            ),
            (
                &[("60-sessions/active/a/artifacts/out.md", "out")],
                vec![(session.clone(), DoorbellReason::Artifact)],
            ),
            (
                &[("60-sessions/active/a/brief.md", "card, moved")],
                vec![(session.clone(), DoorbellReason::Card)],
            ),
            (
                &[("80-agents/nixi/MEMORY.md", "remembered")],
                vec![(control(), DoorbellReason::Memory)],
            ),
            (&[("10-notes/today.md", "a note")], Vec::new()),
            // A session whose room holds Marta, who does not read tgdrive.
            (
                &[
                    ("60-sessions/active/b/agent.toml", shared_toml.as_str()),
                    ("60-sessions/active/b/brief.md", "card"),
                ],
                Vec::new(),
            ),
        ];
        for (files, expected) in steps {
            let to = repo.commit(files);
            let pushed = keeper_sync::engine::Pushed {
                profile_id: "tgdrive".to_owned(),
                from: Some(from.clone()),
                to: to.clone(),
            };
            assert_eq!(
                rt.round(&drives).ring(&pushed, &engine, &[]).await,
                expected,
                "{files:?}"
            );
            for (room, reason) in &expected {
                let rung = copy.server.get(room, DOORBELL, "tgdrive").expect("rung");
                assert_eq!(
                    rung.content,
                    json!({"v": 1, "drive": "tgdrive", "commit": to, "reason": reason})
                );
            }
            from = to;
        }
        assert!(
            copy.server.get(&shared, DOORBELL, "tgdrive").is_none(),
            "nothing reached the room with a person outside the readers"
        );
    }

    fn lucyna(readers: &[&str]) -> KnownAgent {
        KnownAgent {
            id: "lucyna-novak".to_owned(),
            drive: "neuradrive".to_owned(),
            name: "Dr Lucyna Novak".to_owned(),
            matrix_user: user("@lucyna-novak:example.org"),
            kind: keeper_core::agents::home::AgentKind::Steward,
            human: None,
            hosted: false,
            home_readers: Readers::Only(readers.iter().map(|r| user(r)).collect()),
            opening: Label::top(),
            drives: vec!["neuradrive".to_owned()],
        }
    }

    /// A host manifest of `host` listing `drive`.
    fn listing(host: &str, drive: &str) -> Value {
        serde_json::to_value(HostManifest {
            v: 1,
            host: host.to_owned(),
            principal: "tgorka".to_owned(),
            version: "0.92.0".to_owned(),
            always_on: true,
            tools: Vec::new(),
            drives: vec![HostDrive {
                id: drive.to_owned(),
                present: true,
                materialized: Materialized::Full,
            }],
            bots: Vec::new(),
            agents: Vec::new(),
            renewed_at: "2026-10-05T00:00:00Z".to_owned(),
            expires_at: "2026-10-05T00:03:00Z".to_owned(),
        })
        .expect("manifest")
    }

    /// `copy` in the control room `room` with `members`, a manifest there
    /// listing `drive`.
    fn in_control_room(copy: &FakeCopy, room: &OwnedRoomId, members: &[&str], drive: &str) {
        copy.joined.lock().expect("lock").insert(room.clone());
        copy.controls.lock().expect("lock").push(room.clone());
        copy.members
            .lock()
            .expect("lock")
            .insert(room.clone(), members.iter().map(|m| user(m)).collect());
        copy.server.put(
            room,
            HOST,
            "electra",
            &user("@nixi:example.org"),
            listing("electra", drive),
        );
    }

    /// A pushed memory change of neuradrive in a fresh repository, with
    /// an engine reading it; `None` with no `git`.
    fn neuradrive_push(root: &Path) -> Option<(Arc<dyn DriveEngine>, keeper_sync::engine::Pushed)> {
        let repo = Repo::init(root)?;
        let from = repo.commit(&[("README.md", "seed")]);
        let to = repo.commit(&[("80-agents/lucyna-novak/MEMORY.md", "consolidated")]);
        let engine: Arc<dyn DriveEngine> = Arc::new(FakeEngine {
            repo: Some(root.to_path_buf()),
            ..FakeEngine::default()
        });
        let pushed = keeper_sync::engine::Pushed {
            profile_id: "neuradrive".to_owned(),
            from: Some(from),
            to,
        };
        Some((engine, pushed))
    }

    /// 92.4 acceptance 9 (the routing; the server's half is the live test),
    /// R160 (review DB-01): Dr Lucyna Novak's host pushes a memory change
    /// to neuradrive. Its own control room and agentd-tgorka's, which she
    /// visits at power 0 and whose manifests list neuradrive, each get one
    /// doorbell — the visiting steward counts through her audience, not as
    /// a person — and no other room does; the tgorka host's receiver hears
    /// it and pulls once.
    #[tokio::test]
    async fn a_shared_drive_rings_every_control_room_that_lists_it() {
        let root = tempfile::tempdir().expect("root");
        let Some((engine, pushed)) = neuradrive_push(root.path()) else {
            return;
        };
        let marta = "@marta:example.org";
        let visitor = "@lucyna-novak:example.org";
        let room_of =
            |name: &str| OwnedRoomId::try_from(format!("!{name}:example.org")).expect("room");
        let (own, visited, other) = (room_of("neuraffica"), room_of("tgorka"), room_of("other"));
        let copy = Arc::new(fake_copy());
        in_control_room(&copy, &own, &[marta, visitor], "neuradrive");
        in_control_room(&copy, &visited, &[PERSON, visitor], "neuradrive");
        in_control_room(&copy, &other, &[PERSON, visitor], "tgdrive");
        let rt = ringing_runtime(&copy, own.clone());
        let drives = [drive_view(root.path(), "neuradrive", &[PERSON, marta])];
        let agents = [lucyna(&[PERSON, marta])];
        let mut rung = rt.round(&drives).ring(&pushed, &engine, &agents).await;
        rung.sort();
        let mut expected = vec![
            (own.clone(), DoorbellReason::Memory),
            (visited.clone(), DoorbellReason::Memory),
        ];
        expected.sort();
        assert_eq!(rung, expected);
        assert_eq!(
            doorbells(&copy.server),
            2,
            "one doorbell per room, none elsewhere"
        );

        // agentd-tgorka hears it in its control room: one pull.
        let heard = copy
            .server
            .get(&visited, DOORBELL, "neuradrive")
            .expect("rung");
        let tgorka_engine = Arc::new(FakeEngine::default());
        let receiver = Doorbell::default();
        receiver.set_engine(Arc::clone(&tgorka_engine) as Arc<dyn DriveEngine>);
        receiver.set_drives(
            [("neuradrive".to_owned(), "neuradrive".to_owned())],
            agents.to_vec(),
        );
        receiver.set_principal_agents(&[user("@nixi:example.org")]);
        assert_eq!(
            receiver.hear(&heard.sender, "neuradrive", &heard.content),
            Answer::Heard
        );
        assert_eq!(
            receiver.deliver(std::time::Instant::now(), 4),
            vec![("neuradrive".to_owned(), Answer::Pulled)]
        );
        assert_eq!(
            *tgorka_engine.pulls.lock().expect("lock"),
            vec![("neuradrive".to_owned(), pushed.to)]
        );
    }

    /// R160 (review DB-02): a room listing the drive is not rung when any
    /// member is outside its readers, whatever power the room gives them —
    /// an outsider, an account this host cannot name as an agent, or an
    /// agent whose own audience is wider than the readers.
    #[tokio::test]
    async fn a_doorbell_is_not_rung_past_the_drive_s_readers() {
        let root = tempfile::tempdir().expect("root");
        let Some((engine, pushed)) = neuradrive_push(root.path()) else {
            return;
        };
        let marta = "@marta:example.org";
        let visitor = "@lucyna-novak:example.org";
        let wide = "@tola-grey:example.org";
        let copy = Arc::new(fake_copy());
        let (outsider, unnamed, wider, fine) = (room(1), room(2), room(3), room(4));
        in_control_room(
            &copy,
            &outsider,
            &[PERSON, visitor, "@mallory:example.org"],
            "neuradrive",
        );
        in_control_room(
            &copy,
            &unnamed,
            &[PERSON, visitor, "@bot:example.org"],
            "neuradrive",
        );
        in_control_room(&copy, &wider, &[PERSON, visitor, wide], "neuradrive");
        in_control_room(&copy, &fine, &[PERSON, marta, visitor], "neuradrive");
        let mut tola = lucyna(&[PERSON, "@eve:example.org"]);
        tola.matrix_user = user(wide);
        tola.drive = "tgdrive".to_owned();
        let rt = ringing_runtime(&copy, fine.clone());
        let drives = [drive_view(root.path(), "neuradrive", &[PERSON, marta])];
        let rung = rt
            .round(&drives)
            .ring(&pushed, &engine, &[lucyna(&[PERSON, marta]), tola])
            .await;
        assert_eq!(rung, vec![(fine, DoorbellReason::Memory)]);
        assert_eq!(doorbells(&copy.server), 1);
    }

    /// R161 (review DB-03): a bell the copy's sync left in a room's state
    /// while the host could not yet admit it — no drives read — is heard
    /// again from the cached state once it can, and answered with a pull.
    #[tokio::test]
    async fn a_bell_cached_before_the_host_was_ready_is_answered_once_it_is() {
        let copy = Arc::new(fake_copy());
        let visited = room(7);
        in_control_room(&copy, &visited, &[PERSON], "neuradrive");
        let commit = "d".repeat(40);
        let bell = json!({"v": 1, "drive": "neuradrive", "commit": commit, "reason": "memory"});
        copy.server.put(
            &visited,
            DOORBELL,
            "neuradrive",
            &user("@lucyna-novak:example.org"),
            bell.clone(),
        );
        let engine = Arc::new(FakeEngine::default());
        let doorbell = Arc::new(Doorbell::default());
        doorbell.set_engine(Arc::clone(&engine) as Arc<dyn DriveEngine>);
        assert_eq!(
            doorbell.hear(&user("@lucyna-novak:example.org"), "neuradrive", &bell),
            Answer::UnknownDrive,
            "heard before the drives were read"
        );
        let rt = ringing_runtime(&copy, control());
        let round = rt.round(&[]);
        let (_tx, rx) = tokio::sync::broadcast::channel(4);
        let mut ringer = Ringer::over(Arc::clone(&engine) as Arc<dyn DriveEngine>, rx);
        ringer.tick(&round, &doorbell);
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert!(engine.pulls.lock().expect("lock").is_empty());

        doorbell.set_drives(
            [("neuradrive".to_owned(), "p-neura".to_owned())],
            vec![lucyna(&[PERSON])],
        );
        for _ in 0..100 {
            ringer.tick(&round, &doorbell);
            if !engine.pulls.lock().expect("lock").is_empty() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        assert_eq!(
            *engine.pulls.lock().expect("lock"),
            vec![("p-neura".to_owned(), commit)]
        );
    }

    /// R161 (review DB-08): a doorbell send that never answers, under a
    /// stream of pushes, never holds the host's tick: each tick returns at
    /// once, the pushes wait coalesced, and stopping is bounded.
    #[tokio::test]
    async fn a_stalled_doorbell_never_holds_the_tick() {
        let root = tempfile::tempdir().expect("root");
        let Some((engine, pushed)) = neuradrive_push(root.path()) else {
            return;
        };
        let copy = Arc::new(fake_copy());
        copy.server.stall_doorbells.store(true, Ordering::Relaxed);
        let own = control();
        in_control_room(&copy, &own, &[PERSON], "neuradrive");
        let rt = ringing_runtime(&copy, own);
        let drives = [drive_view(root.path(), "neuradrive", &[PERSON])];
        let round = rt.round(&drives);
        let doorbell = Arc::new(Doorbell::default());
        let (tx, rx) = tokio::sync::broadcast::channel(64);
        let mut ringer = Ringer::over(engine, rx);
        for _ in 0..20 {
            for _ in 0..3 {
                tx.send(pushed.clone()).expect("send");
            }
            tokio::time::timeout(Duration::from_millis(20), async {
                ringer.tick(&round, &doorbell);
            })
            .await
            .expect("a tick returns at once while a send stalls");
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
        tokio::time::timeout(
            Duration::from_secs(2),
            ringer.finish(&round, &doorbell, Duration::from_millis(200)),
        )
        .await
        .expect("stopping is bounded");
        assert_eq!(doorbells(&copy.server), 0);
    }

    /// R161 (review DB-10): a push that completes after the host's last
    /// tick is rung when the host stops.
    #[tokio::test]
    async fn the_last_push_is_rung_when_the_host_stops() {
        let root = tempfile::tempdir().expect("root");
        let Some((engine, pushed)) = neuradrive_push(root.path()) else {
            return;
        };
        let copy = Arc::new(fake_copy());
        let own = control();
        in_control_room(&copy, &own, &[PERSON], "neuradrive");
        let rt = ringing_runtime(&copy, own.clone());
        let drives = [drive_view(root.path(), "neuradrive", &[PERSON])];
        let round = rt.round(&drives);
        let doorbell = Arc::new(Doorbell::default());
        let (tx, rx) = tokio::sync::broadcast::channel(4);
        let mut ringer = Ringer::over(engine, rx);
        ringer.tick(&round, &doorbell);
        tx.send(pushed.clone()).expect("the final push");
        ringer.finish(&round, &doorbell, RING_FINISH).await;
        let rung = copy.server.get(&own, DOORBELL, "neuradrive").expect("rung");
        assert_eq!(rung.content["commit"], pushed.to);
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

    /// R160 and R65 through placement: a known agent whose audience is
    /// within the label counts through it — the blocked status keeps its
    /// title and detail, and nothing is audited — while a known agent with
    /// a wider audience, or an account no one knows, narrows it; each
    /// narrowed status sent leaves one `status` row for its session and
    /// room, and a status not sent again leaves none.
    #[tokio::test(start_paused = true)]
    async fn a_placement_status_counts_known_agents_and_audits_its_suppression() {
        use keeper_core::bots::audit::{list_audit, AuditOutcome, AuditVerdict};
        let within = lucyna(&[PERSON]);
        let wider = KnownAgent {
            matrix_user: user("@wide:example.org"),
            ..lucyna(&[PERSON, "@eve:example.org"])
        };
        let copy = Arc::new(FakeCopy {
            known: Some(Arc::new(crate::rooms::Known {
                agents: vec![within.clone(), wider.clone()],
                trust: Vec::new(),
            })),
            ..fake_copy()
        });
        let mut w = world_over(copy, true);
        let rooms = [
            (room(1), within.matrix_user.clone()),
            (room(2), wider.matrix_user.clone()),
            (room(3), user("@stranger:example.org")),
        ];
        for (room, other) in &rooms {
            w.copy
                .members
                .lock()
                .expect("lock")
                .insert(room.clone(), [user(PERSON), other.clone()].into());
            w.offer(room, None);
        }
        w.tick().await;
        w.server().fail_claims.store(true, Ordering::Relaxed);
        for _ in 0..4 {
            tokio::time::advance(RENEW_EVERY).await;
            w.tick().await;
        }
        let statuses = w.server().statuses();
        let rows = list_audit(w.copy.data_dir.path(), None, None).expect("audit");
        for (n, (room, _)) in rooms.iter().enumerate() {
            let session = format!("60-sessions/active/{room}");
            let of_room: Vec<&Value> = statuses
                .iter()
                .filter(|s| s["session"] == session)
                .collect();
            assert!(
                of_room.iter().any(|s| s["run"] == "blocked"),
                "{statuses:?}"
            );
            let narrowed = of_room
                .iter()
                .filter(|s| s["title"] == NARROWED_STATUS)
                .count();
            let audited: Vec<_> = rows.iter().filter(|r| r.subpath == room.as_str()).collect();
            if n == 0 {
                assert_eq!(narrowed, 0, "{of_room:?}");
                assert!(of_room
                    .iter()
                    .all(|s| s["detail"] == LOST_DETAIL || s["run"] != "blocked"));
            } else {
                assert!(narrowed > 0, "{of_room:?}");
                assert!(of_room
                    .iter()
                    .all(|s| s.get("detail").is_none_or(Value::is_null)));
            }
            assert_eq!(audited.len(), narrowed, "{room}: {audited:?}");
            let id = w.copy.served.lock().expect("lock")[room].1.id.to_string();
            for row in audited {
                assert_eq!(row.tool, "status");
                assert_eq!(row.session_id, id);
                assert_eq!(row.verdict, Some(AuditVerdict::Deny));
                assert_eq!(row.outcome, AuditOutcome::Refused);
            }
        }
    }

    /// R169 (R64 for placement): the same blocked status, in a room the
    /// session's label does not reach now — someone outside the drive's
    /// readers joined — keeps `session`, says the fixed sentence and no
    /// detail; a room whose members cannot be read is treated so too.
    #[tokio::test(start_paused = true)]
    async fn a_placement_status_follows_the_label_too() {
        let mut w = world(Duration::ZERO, true);
        let a = room(1);
        w.copy
            .members
            .lock()
            .expect("lock")
            .insert(a.clone(), [user(PERSON), user("@stranger:h")].into());
        w.offer(&a, None);
        w.tick().await;
        w.server().fail_claims.store(true, Ordering::Relaxed);
        tokio::time::advance(RENEW_EVERY).await;
        w.tick().await;
        tokio::time::advance(RENEW_EVERY).await;
        w.tick().await;
        let statuses = w.server().statuses();
        assert_eq!(statuses.len(), 1, "{statuses:?}");
        assert_eq!(statuses[0]["run"], "blocked");
        assert_eq!(statuses[0]["title"], NARROWED_STATUS);
        assert!(statuses[0].get("detail").is_none_or(Value::is_null));
        assert_eq!(statuses[0]["session"], format!("60-sessions/active/{a}"));
    }

    /// R64 through placement (R169): a session whose own log narrowed its
    /// label below the room — a read its worker made here — keeps the fixed
    /// title and no detail on every status placement says after: handed
    /// back to another host, waiting for it, and again once another host's
    /// claim took it over. Its `session` stays, and so does the anchor.
    #[tokio::test(start_paused = true)]
    async fn a_narrowed_session_stays_fixed_through_a_hand_back_a_wait_and_a_takeover() {
        use keeper_core::agents::label::{LabelBody, LabelCause, LabelCauseKind, Readers};
        let copy = Arc::new(FakeCopy {
            latest_status: Some(OwnedEventId::try_from(ANCHOR).expect("anchor")),
            ..fake_copy()
        });
        let mut w = world_over(copy, true);
        let a = room(1);
        w.offer(&a, None);
        w.rt.scanned();
        w.tick().await;
        assert!(w.held_by_me(&a), "its worker serves it here");

        // The worker read a file the room's person may not read.
        let slug = HostSlug::new(ME).expect("slug");
        let mut chunks = ChunkWriter::open(
            &w.dir(&a),
            &slug,
            rotate_at(1 << 20),
            chrono::Utc::now().date_naive(),
        )
        .expect("chunk");
        let narrowed = Label {
            readers: Readers::Only(Default::default()),
            integrity: Integrity::Owner,
            local_only: false,
        };
        let now = chrono::Utc::now();
        chunks
            .append(&LogLine {
                v: LINE_VERSION,
                id: ulid::Ulid::new(),
                parent: None,
                ts: chrono::DateTime::from_timestamp_millis(now.timestamp_millis()).expect("ts"),
                host: slug,
                epoch: 0,
                claim: None,
                matrix_event: None,
                body: LineBody::Label(LabelBody::new(
                    &narrowed,
                    LabelCause {
                        kind: LabelCauseKind::DriveRead,
                        reference: "otherdrive/plan.md".to_owned(),
                    },
                )),
            })
            .expect("append");
        drop(chunks);

        // Pinned to electra, which is not live: handed back, then waiting.
        w.offer(&a, Some(OTHER));
        w.rt.scanned();
        w.tick().await;
        tokio::time::sleep(Duration::from_millis(10)).await;
        w.tick().await;
        assert!(w.released(&a));
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
        assert_eq!(waiting(&w), 1, "{:?}", w.server().statuses());

        // Another host's claim took it over: said again, still fixed.
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
        assert_eq!(waiting(&w), 2);
        for status in w.server().statuses() {
            assert_eq!(status["title"], NARROWED_STATUS, "{status}");
            assert!(status.get("detail").is_none_or(Value::is_null), "{status}");
            assert_eq!(status["session"], format!("60-sessions/active/{a}"));
            assert_eq!(status["anchor"], ANCHOR, "{status}");
        }
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

    /// The scheduled session's folder, zone-relative.
    const SCHEDULED: &str = "active/2026-10-05-sort";

    /// Nixi's `@hourly` card, with `keys`.
    fn hourly(keys: &str) -> String {
        format!("---\ntags: [task]\ntitle: Sort the inbox\nstatus: todo\nassignee: nixi\nschedule: \"@hourly\"\n{keys}---\n\nSort what came in.\n")
    }

    fn put_card(zone: &Path, text: &str) {
        let dir = zone.join(SCHEDULED);
        std::fs::create_dir_all(&dir).expect("session");
        std::fs::write(dir.join("card.md"), text).expect("card");
    }

    fn card_text_in(zone: &Path) -> String {
        std::fs::read_to_string(zone.join(SCHEDULED).join("card.md")).expect("card")
    }

    fn card_in(zone: &Path) -> keeper_core::agents::card::CardAgent {
        keeper_core::agents::card::CardAgent::of_text(&card_text_in(zone)).expect("agent keys")
    }

    fn ms(text: &str) -> i64 {
        chrono::DateTime::parse_from_rfc3339(text)
            .expect("an instant")
            .timestamp_millis()
    }

    /// `text` as a claim names a window.
    fn named(text: &str) -> String {
        claim::rfc3339(ms(text) as u64)
    }

    fn ran_in(card: &keeper_core::agents::card::CardAgent, text: &str) -> bool {
        matches!(&card.last_run, Some(keeper_core::agents::card::Field::Read(at)) if at.timestamp_millis() == ms(text))
    }

    fn run_of(
        card: &keeper_core::agents::card::CardAgent,
    ) -> Option<keeper_core::agents::card::Run> {
        match &card.run {
            Some(keeper_core::agents::card::Field::Read(run)) => Some(*run),
            _ => None,
        }
    }

    impl FakeCopy {
        fn routed(&self) -> Vec<(Scheduled, Result<cards::Begun, String>)> {
            self.routed.lock().expect("lock").clone()
        }

        /// The turns the worker ran.
        fn runs(&self) -> usize {
            self.routed()
                .iter()
                .filter(|(_, begun)| matches!(begun, Ok(cards::Begun::Run { .. })))
                .count()
        }
    }

    impl World {
        /// The rescan finds Nixi's scheduled session of `room` in the copy's
        /// zone, with the cards its folder holds now.
        fn offer_scheduled(&mut self, room: &OwnedRoomId) {
            self.offer_scheduled_needing(room, None);
        }

        /// [`World::offer_scheduled`], the session needing `needs`.
        fn offer_scheduled_needing(&mut self, room: &OwnedRoomId, needs: Option<Vec<String>>) {
            self.copy.joined.lock().expect("lock").insert(room.clone());
            self.copy
                .members
                .lock()
                .expect("lock")
                .entry(room.clone())
                .or_insert_with(|| [user(PERSON)].into());
            let zone = self.copy.zone.clone().expect("a zone");
            let decl = keeper_core::agents::drive::parse(&drive_toml()).expect("decl");
            let agent = SessionAgent {
                id: ulid::Ulid::new(),
                agent: "nixi".to_owned(),
                drive: "tgdrive".to_owned(),
                kind: SessionKind::Scheduled,
                title: "sort".to_owned(),
                requested_by: user(PERSON),
                parent: None,
                room: room.clone(),
                drives: vec!["tgdrive".to_owned()],
                label: Label::opening(&decl, Integrity::Owner),
                needs,
                pin: None,
                hop: 0,
                dispatch_chain: Vec::new(),
                limits: None,
                workflow: None,
                created_at: chrono::Utc::now(),
            };
            let dir = zone.join(SCHEDULED);
            let session = FoundSession {
                path: SCHEDULED.to_owned(),
                dir: dir.clone(),
                agent: Ok(agent.clone()),
                scheduled: cards::scheduled_cards(SCHEDULED, &dir),
            };
            self.rt.offer(&self.copy, &session, &agent);
        }

        /// The calendar reads `text`.
        fn at(&self, text: &str) {
            self.now.store(ms(text), Ordering::Relaxed);
        }

        /// `host`'s manifest, as this world would publish it, lapsed: the
        /// host is gone.
        fn lapse_manifest(&self, host: &str) {
            let now = wall_ms();
            let mut manifest = self.rt.manifest(now, true);
            manifest.host = host.to_owned();
            manifest.expires_at = claim::rfc3339(now - 1_000);
            self.server().put(
                &control(),
                HOST,
                host,
                &self.copy.config.matrix_user,
                serde_json::to_value(manifest).expect("manifest"),
            );
        }
    }

    /// A copy over `server` whose worker reads and writes cards in `zone`.
    fn copy_over(server: &Arc<Server>, zone: &Path) -> Arc<FakeCopy> {
        Arc::new(FakeCopy {
            server: Arc::clone(server),
            zone: Some(zone.to_owned()),
            ..fake_copy()
        })
    }

    /// 92.3 AC2 (R56): two hosts see one card due over one drive and one
    /// homeserver; one holds the session's claim, names the window in it and
    /// runs it — one `run: running`, one `last_run`, one turn — and the other
    /// writes nothing, however often both tick.
    #[tokio::test(start_paused = true)]
    async fn a_due_card_runs_once_across_two_hosts() {
        let (server, zone) = (
            Arc::new(Server::default()),
            tempfile::tempdir().expect("zone"),
        );
        put_card(zone.path(), &hourly("last_run: \"2026-10-05T08:00:00Z\"\n"));
        let mut here = world_over(copy_over(&server, zone.path()), true);
        let mut there = world_over(copy_over(&server, zone.path()), true);
        there.rt.host = HostSlug::new(OTHER).expect("slug");
        let a = room(1);
        for w in [&mut here, &mut there] {
            w.at("2026-10-05T09:30:00Z");
            w.offer_scheduled(&a);
        }
        for _ in 0..6 {
            tokio::join!(here.tick(), there.tick());
            tokio::time::advance(Duration::from_secs(1)).await;
        }

        let (mine, theirs) = (here.copy.routed(), there.copy.routed());
        assert_eq!(
            here.copy.runs() + there.copy.runs(),
            1,
            "{mine:?} {theirs:?}"
        );
        assert_eq!(
            mine.len() + theirs.len(),
            1,
            "one window, routed once: {mine:?} {theirs:?}"
        );
        let claim = server.claim(&a).expect("the session's claim");
        assert_eq!(claim.content.window, Some(named("2026-10-05T09:00:00Z")));
        let card = card_in(zone.path());
        assert!(ran_in(&card, "2026-10-05T09:00:00Z"), "{card:?}");
        assert_eq!(run_of(&card), Some(keeper_core::agents::card::Run::Review));
    }

    /// 92.3 AC3 (Q8, R57): a card pinned to hesperia while hesperia is not
    /// live is said to wait by electra, the announcing host — `run: waiting`
    /// and its line naming what it waits for — which then hands the claim
    /// back; once hesperia's manifest is live, hesperia runs it once and
    /// electra does nothing more.
    #[tokio::test(start_paused = true)]
    async fn a_card_pinned_to_an_absent_host_waits_named() {
        let (server, zone) = (
            Arc::new(Server::default()),
            tempfile::tempdir().expect("zone"),
        );
        put_card(
            zone.path(),
            &hourly("host: hesperia\nlast_run: \"2026-10-05T08:00:00Z\"\n"),
        );
        let a = room(1);
        let mut electra = world_over(copy_over(&server, zone.path()), true);
        electra.rt.host = HostSlug::new(OTHER).expect("slug");
        electra.rt.always_on = true;
        electra.at("2026-10-05T09:30:00Z");
        electra.offer_scheduled(&a);
        electra.tick().await;
        assert!(
            electra.copy.routed().is_empty(),
            "not before ANNOUNCE_AFTER"
        );
        tokio::time::advance(ANNOUNCE_AFTER).await;
        for _ in 0..3 {
            electra.tick().await;
            tokio::time::advance(Duration::from_secs(1)).await;
        }
        let waited = electra.copy.routed();
        assert_eq!(waited.len(), 1, "{waited:?}");
        let waiting = "hesperia — a live host".to_owned();
        assert_eq!(
            waited[0],
            (
                Scheduled::Wait {
                    card: "card.md".to_owned(),
                    waiting: waiting.clone()
                },
                Ok(cards::Begun::Said(keeper_core::agents::log::RunBody {
                    state: keeper_core::agents::log::RunState::Waiting,
                    detail: Some(waiting)
                }))
            )
        );
        assert_eq!(
            run_of(&card_in(zone.path())),
            Some(keeper_core::agents::card::Run::Waiting)
        );
        assert!(electra.released(&a), "electra hands the claim back");

        let mut hesperia = world_over(copy_over(&server, zone.path()), true);
        hesperia.at("2026-10-05T09:30:00Z");
        hesperia.offer_scheduled(&a);
        for _ in 0..4 {
            tokio::join!(electra.tick(), hesperia.tick());
            tokio::time::advance(Duration::from_secs(1)).await;
        }
        assert_eq!(hesperia.copy.runs(), 1, "{:?}", hesperia.copy.routed());
        assert_eq!(electra.copy.routed().len(), 1, "electra did nothing more");
        let card = card_in(zone.path());
        assert!(ran_in(&card, "2026-10-05T09:00:00Z"), "{card:?}");
        assert_eq!(run_of(&card), Some(keeper_core::agents::card::Run::Review));
    }

    /// 92.3 AC9 (S-21): a card due by its schedule but carrying
    /// `scheduled_by` names no window and runs nothing across a day of
    /// ticks; once a person's *Allow* removes the mark, the next due window
    /// runs once.
    #[tokio::test(start_paused = true)]
    async fn an_agent_scheduled_card_never_runs_before_a_persons_tick() {
        let (server, zone) = (
            Arc::new(Server::default()),
            tempfile::tempdir().expect("zone"),
        );
        let marked = hourly("scheduled_by: \"@tola:example.org\"\n");
        put_card(zone.path(), &marked);
        let mut w = world_over(copy_over(&server, zone.path()), true);
        let a = room(1);
        w.at("2026-10-05T00:00:30Z");
        w.offer_scheduled(&a);
        for _ in 0..24 * 60 {
            w.tick().await;
            tokio::time::advance(Duration::from_secs(60)).await;
            w.now.fetch_add(60_000, Ordering::Relaxed);
        }
        assert!(w.held_by_me(&a), "the session is served all along");
        assert!(w.copy.routed().is_empty(), "{:?}", w.copy.routed());
        assert_eq!(server.claim(&a).expect("claim").content.window, None);
        assert_eq!(card_text_in(zone.path()), marked);

        let plan = keeper_core::sessions::tasks::compile_allow_schedule(
            SCHEDULED, "card.md", &marked, PERSON,
        )
        .expect("a person's tick");
        crate::sessions::exec::run(zone.path(), plan).expect("allowed");
        w.offer_scheduled(&a);
        for _ in 0..3 {
            w.tick().await;
            tokio::time::advance(Duration::from_secs(1)).await;
        }
        assert_eq!(w.copy.runs(), 1, "{:?}", w.copy.routed());
        assert!(ran_in(&card_in(zone.path()), "2026-10-06T00:00:00Z"));
        assert_eq!(
            server.claim(&a).expect("claim").content.window,
            Some(named("2026-10-06T00:00:00Z"))
        );
    }

    /// `room`'s claim, as the server holds it now, ten minutes old by the
    /// server's clock: lapsed.
    fn lapse(server: &Server, room: &OwnedRoomId) {
        let lapsed = wall_ms() - 600_000;
        server
            .states
            .lock()
            .expect("lock")
            .get_mut(&(room.clone(), CLAIM.to_owned(), String::new()))
            .expect("the claim")
            .origin_server_ts = MilliSecondsSinceUnixEpoch(UInt::new(lapsed).expect("ts"));
    }

    /// Electra's lapsed claim on `room` at epoch 4, naming `window`.
    fn electra_lapsed(server: &Server, room: &OwnedRoomId, window: Option<String>) {
        let lapsed = wall_ms() - 600_000;
        let electra = Claimant {
            host: OTHER.to_owned(),
            device: "ELECTRA1".to_owned(),
            agent: user("@nixi:example.org"),
        };
        server.put(
            room,
            CLAIM,
            "",
            &user("@nixi:example.org"),
            serde_json::to_value(electra.content(4, lapsed, lapsed, false, window)).expect("claim"),
        );
        lapse(server, room);
    }

    /// 92.3 AC10 (S-25, R58): electra claimed the session naming window W,
    /// wrote its card locally and never pushed; hesperia takes the session
    /// over within W, finds the claim's W and the card's older `last_run`,
    /// and settles W as "ran on electra, effect unknown" — `last_run` = W,
    /// `run: review`, no turn — then runs the next window once.
    #[tokio::test(start_paused = true)]
    async fn a_takeover_mid_window_does_not_run_the_window_again() {
        let (server, zone) = (
            Arc::new(Server::default()),
            tempfile::tempdir().expect("zone"),
        );
        put_card(zone.path(), &hourly("last_run: \"2026-10-05T08:00:00Z\"\n"));
        let a = room(1);
        let w_9 = named("2026-10-05T09:00:00Z");
        electra_lapsed(&server, &a, Some(w_9.clone()));
        let mut w = world_over(copy_over(&server, zone.path()), true);
        w.at("2026-10-05T09:10:00Z");
        w.offer_scheduled(&a);
        // One tick takes the session and settles W; the rescan then reads
        // the card as written, and a renewal is due.
        w.tick().await;
        w.offer_scheduled(&a);
        tokio::time::advance(RENEW_EVERY).await;
        w.tick().await;
        let routed = w.copy.routed();
        assert_eq!(
            routed[0],
            (
                Scheduled::TakenOver {
                    card: "card.md".to_owned(),
                    window: Some(w_9.clone()),
                    host: OTHER.to_owned()
                },
                Ok(cards::Begun::Said(keeper_core::agents::log::RunBody {
                    state: keeper_core::agents::log::RunState::Review,
                    detail: Some("ran on electra, effect unknown".to_owned())
                }))
            ),
            "{routed:?}"
        );
        assert_eq!(w.copy.runs(), 0, "{routed:?}");
        let card = card_in(zone.path());
        assert!(ran_in(&card, "2026-10-05T09:00:00Z"), "{card:?}");
        assert_eq!(run_of(&card), Some(keeper_core::agents::card::Run::Review));
        let claim = server.claim(&a).expect("claim");
        assert!(w.held_by_me(&a) && claim.content.epoch == 5);
        assert_eq!(
            claim.content.window,
            Some(w_9),
            "the renewal keeps W named: a later taker finds it too"
        );
        assert_eq!(routed.len(), 1, "{routed:?}");

        w.at("2026-10-05T10:05:00Z");
        for _ in 0..3 {
            w.tick().await;
            tokio::time::advance(Duration::from_secs(1)).await;
        }
        assert_eq!(w.copy.runs(), 1, "{:?}", w.copy.routed());
        assert!(ran_in(&card_in(zone.path()), "2026-10-05T10:00:00Z"));
    }

    /// R163: the claim's window survives a taker that dies before it
    /// settles it. Electra named W and never pushed; hesperia takes the
    /// session — its very first claim write names W — and dies before its
    /// settlement lands; kalypso, over a checkout as stale as hesperia's,
    /// takes it from hesperia, settles W, and never runs it.
    #[tokio::test(start_paused = true)]
    async fn a_window_stays_named_through_a_taker_that_dies() {
        let server = Arc::new(Server::default());
        let (zone_b, zone_c) = (
            tempfile::tempdir().expect("hesperia's checkout"),
            tempfile::tempdir().expect("kalypso's checkout"),
        );
        for zone in [&zone_b, &zone_c] {
            put_card(zone.path(), &hourly("last_run: \"2026-10-05T08:00:00Z\"\n"));
        }
        let a = room(1);
        let w_9 = named("2026-10-05T09:00:00Z");
        electra_lapsed(&server, &a, Some(w_9.clone()));

        let mut hesperia = world_over(copy_over(&server, zone_b.path()), true);
        hesperia
            .copy
            .refuse_writes
            .store(usize::MAX, Ordering::Relaxed);
        hesperia.at("2026-10-05T09:10:00Z");
        hesperia.offer_scheduled(&a);
        hesperia.tick().await;
        assert!(hesperia.held_by_me(&a));
        assert_eq!(
            server.claim(&a).expect("claim").content.window,
            Some(w_9.clone()),
            "the taker's acquisition names W"
        );
        hesperia.lapse_manifest(ME);
        lapse(&server, &a);

        let mut kalypso = world_over(copy_over(&server, zone_c.path()), true);
        kalypso.rt.host = HostSlug::new("kalypso").expect("slug");
        kalypso.at("2026-10-05T09:20:00Z");
        kalypso.offer_scheduled(&a);
        for _ in 0..3 {
            kalypso.tick().await;
            tokio::time::advance(Duration::from_secs(1)).await;
        }
        let routed = kalypso.copy.routed();
        assert_eq!(kalypso.copy.runs(), 0, "{routed:?}");
        assert_eq!(
            routed.first().map(|(scheduled, _)| scheduled.clone()),
            Some(Scheduled::TakenOver {
                card: "card.md".to_owned(),
                window: Some(w_9),
                host: ME.to_owned()
            }),
            "{routed:?}"
        );
        let card = card_in(zone_c.path());
        assert!(ran_in(&card, "2026-10-05T09:00:00Z"), "{card:?}");
        assert_eq!(run_of(&card), Some(keeper_core::agents::card::Run::Review));
    }

    /// R163: a window is decided by the card as it reads now, never by the
    /// rescan's copy. A never-run `every 2h` card runs its 09:30 window at
    /// 09:30:58 and nothing at 09:31:00 though no rescan saw its
    /// `last_run`; a clone that never saw it run, taking the session from
    /// electra whose claim named 09:30, settles 09:30 and runs nothing at
    /// 09:31 either.
    #[tokio::test(start_paused = true)]
    async fn a_window_is_decided_by_the_card_as_it_reads_now() {
        let every = hourly("").replace("\"@hourly\"", "every 2h");
        let (server, zone) = (
            Arc::new(Server::default()),
            tempfile::tempdir().expect("zone"),
        );
        put_card(zone.path(), &every);
        let mut w = world_over(copy_over(&server, zone.path()), true);
        let a = room(1);
        w.at("2026-10-05T09:30:58Z");
        w.offer_scheduled(&a);
        w.tick().await;
        assert_eq!(w.copy.runs(), 1, "{:?}", w.copy.routed());
        w.at("2026-10-05T09:31:00Z");
        for _ in 0..3 {
            w.tick().await;
            tokio::time::advance(Duration::from_secs(1)).await;
        }
        assert_eq!(w.copy.runs(), 1, "{:?}", w.copy.routed());
        assert!(ran_in(&card_in(zone.path()), "2026-10-05T09:30:00Z"));

        let (server, zone) = (
            Arc::new(Server::default()),
            tempfile::tempdir().expect("zone"),
        );
        put_card(zone.path(), &every);
        electra_lapsed(&server, &a, Some(named("2026-10-05T09:30:00Z")));
        let mut w = world_over(copy_over(&server, zone.path()), true);
        w.at("2026-10-05T09:30:40Z");
        w.offer_scheduled(&a);
        w.tick().await;
        w.at("2026-10-05T09:31:00Z");
        for _ in 0..3 {
            w.tick().await;
            tokio::time::advance(Duration::from_secs(1)).await;
        }
        let routed = w.copy.routed();
        assert_eq!(w.copy.runs(), 0, "{routed:?}");
        assert!(
            matches!(&routed[..], [(Scheduled::TakenOver { .. }, Ok(_))]),
            "{routed:?}"
        );
        assert!(ran_in(&card_in(zone.path()), "2026-10-05T09:30:00Z"));
    }

    /// R163: a settlement whose write fails is not forgotten. The worker's
    /// guarded write is refused once; the next tick settles W again rather
    /// than run it, and no later tick runs W.
    #[tokio::test(start_paused = true)]
    async fn a_failed_settlement_is_tried_until_it_lands() {
        let (server, zone) = (
            Arc::new(Server::default()),
            tempfile::tempdir().expect("zone"),
        );
        put_card(zone.path(), &hourly("last_run: \"2026-10-05T08:00:00Z\"\n"));
        let a = room(1);
        let w_9 = named("2026-10-05T09:00:00Z");
        electra_lapsed(&server, &a, Some(w_9.clone()));
        let mut w = world_over(copy_over(&server, zone.path()), true);
        w.copy.refuse_writes.store(1, Ordering::Relaxed);
        w.at("2026-10-05T09:10:00Z");
        w.offer_scheduled(&a);
        for _ in 0..6 {
            w.tick().await;
            tokio::time::advance(Duration::from_secs(1)).await;
        }
        let routed = w.copy.routed();
        assert_eq!(w.copy.runs(), 0, "{routed:?}");
        assert!(
            routed
                .iter()
                .all(|(scheduled, _)| matches!(scheduled, Scheduled::TakenOver { window, .. } if *window == Some(w_9.clone()))),
            "{routed:?}"
        );
        assert!(
            matches!(&routed[..], [(_, Err(_)), (_, Ok(cards::Begun::Said(_)))]),
            "{routed:?}"
        );
        let card = card_in(zone.path());
        assert!(ran_in(&card, "2026-10-05T09:00:00Z"), "{card:?}");
        assert_eq!(run_of(&card), Some(keeper_core::agents::card::Run::Review));
    }

    /// R164: holding the session is no leave to begin a window. Hesperia
    /// runs 09:00; the session then needs `screen:mac`, which no live host
    /// has, and placement waits — hesperia keeps the claim for its
    /// messages, has the card say it waits, and begins no 10:00 window.
    #[tokio::test(start_paused = true)]
    async fn a_holder_placement_no_longer_picks_begins_no_window() {
        let (server, zone) = (
            Arc::new(Server::default()),
            tempfile::tempdir().expect("zone"),
        );
        put_card(zone.path(), &hourly("last_run: \"2026-10-05T08:00:00Z\"\n"));
        let mut w = world_over(copy_over(&server, zone.path()), true);
        let a = room(1);
        w.at("2026-10-05T09:30:00Z");
        w.offer_scheduled(&a);
        w.tick().await;
        assert_eq!(w.copy.runs(), 1, "{:?}", w.copy.routed());

        w.at("2026-10-05T10:05:00Z");
        w.offer_scheduled_needing(&a, Some(vec!["screen:mac".to_owned()]));
        for _ in 0..3 {
            w.tick().await;
            tokio::time::advance(Duration::from_secs(1)).await;
        }
        let routed = w.copy.routed();
        assert_eq!(w.copy.runs(), 1, "{routed:?}");
        assert!(w.held_by_me(&a), "the holder keeps the session");
        assert!(
            matches!(routed.last(), Some((Scheduled::Wait { waiting, .. }, Ok(_))) if waiting.contains("screen:mac")),
            "{routed:?}"
        );
        let card = card_in(zone.path());
        assert!(ran_in(&card, "2026-10-05T09:00:00Z"), "{card:?}");
        assert_eq!(run_of(&card), Some(keeper_core::agents::card::Run::Waiting));
    }

    /// R164: a run its host died in — after the card said `running`, before
    /// the turn wrote how it ended — is settled by the next holder, the
    /// same host restarted included: `run: review`, "ran on hesperia,
    /// effect unknown", `last_run` left at its window, and no turn.
    #[tokio::test(start_paused = true)]
    async fn a_run_left_running_by_a_dead_host_is_settled() {
        let (server, zone) = (
            Arc::new(Server::default()),
            tempfile::tempdir().expect("zone"),
        );
        put_card(zone.path(), &hourly("last_run: \"2026-10-05T08:00:00Z\"\n"));
        let mut dead = world_over(copy_over(&server, zone.path()), true);
        dead.copy.turns_die.store(true, Ordering::Relaxed);
        let a = room(1);
        dead.at("2026-10-05T09:30:00Z");
        dead.offer_scheduled(&a);
        dead.tick().await;
        assert_eq!(dead.copy.runs(), 1, "{:?}", dead.copy.routed());
        assert_eq!(
            run_of(&card_in(zone.path())),
            Some(keeper_core::agents::card::Run::Running)
        );
        lapse(&server, &a);

        let mut again = world_over(copy_over(&server, zone.path()), true);
        again.at("2026-10-05T09:40:00Z");
        again.offer_scheduled(&a);
        for _ in 0..3 {
            again.tick().await;
            tokio::time::advance(Duration::from_secs(1)).await;
        }
        let routed = again.copy.routed();
        assert_eq!(again.copy.runs(), 0, "{routed:?}");
        assert_eq!(
            routed.first().map(|(_, begun)| begun.clone()),
            Some(Ok(cards::Begun::Said(keeper_core::agents::log::RunBody {
                state: keeper_core::agents::log::RunState::Review,
                detail: Some("ran on hesperia, effect unknown".to_owned())
            }))),
            "{routed:?}"
        );
        let card = card_in(zone.path());
        assert!(ran_in(&card, "2026-10-05T09:00:00Z"), "{card:?}");
        assert_eq!(run_of(&card), Some(keeper_core::agents::card::Run::Review));
    }

    /// R84, R177: a run parked on a person keeps its window. Across three
    /// more due windows its holder names none and routes none while the
    /// worker says the run waits; a host that takes the session over while
    /// it waits — its worker finding the parked run at serve start — names
    /// only the parked window and begins none either.
    #[tokio::test(start_paused = true)]
    async fn a_parked_run_names_no_window_while_it_waits() {
        let (server, zone) = (
            Arc::new(Server::default()),
            tempfile::tempdir().expect("zone"),
        );
        put_card(zone.path(), &hourly("last_run: \"2026-10-05T08:00:00Z\"\n"));
        let mut w = world_over(copy_over(&server, zone.path()), true);
        w.copy.parks.store(true, Ordering::SeqCst);
        let a = room(1);
        w.at("2026-10-05T09:30:00Z");
        w.offer_scheduled(&a);
        for _ in 0..3 {
            w.tick().await;
            tokio::time::advance(Duration::from_secs(1)).await;
        }
        assert_eq!(w.copy.runs(), 1, "{:?}", w.copy.routed());
        assert_eq!(
            run_of(&card_in(zone.path())),
            Some(keeper_core::agents::card::Run::Blocked)
        );
        let w_9 = named("2026-10-05T09:00:00Z");
        for at in [
            "2026-10-05T10:05:00Z",
            "2026-10-05T11:05:00Z",
            "2026-10-05T12:05:00Z",
        ] {
            w.at(at);
            w.offer_scheduled(&a);
            for _ in 0..3 {
                w.tick().await;
                tokio::time::advance(RENEW_EVERY).await;
            }
        }
        assert_eq!(w.copy.routed().len(), 1, "{:?}", w.copy.routed());
        assert_eq!(
            server.claim(&a).expect("claim").content.window,
            Some(w_9.clone())
        );
        assert!(ran_in(&card_in(zone.path()), "2026-10-05T09:00:00Z"));

        lapse(&server, &a);
        let mut taker = world_over(copy_over(&server, zone.path()), true);
        taker.rt.host = HostSlug::new(OTHER).expect("slug");
        taker.copy.parked_at_start.store(true, Ordering::SeqCst);
        // Hesperia is gone: its claim lapsed and so did its manifest.
        taker.lapse_manifest(ME);
        taker.at("2026-10-05T12:05:00Z");
        taker.offer_scheduled(&a);
        for _ in 0..4 {
            taker.tick().await;
            tokio::time::advance(RENEW_EVERY).await;
        }
        let claim = server.claim(&a).expect("claim");
        assert_eq!(claim.content.host, OTHER, "the taker holds it");
        assert_eq!(claim.content.window, Some(w_9));
        assert!(taker.copy.routed().is_empty(), "{:?}", taker.copy.routed());
        assert!(ran_in(&card_in(zone.path()), "2026-10-05T09:00:00Z"));
    }
}
