//! A Linux host's run loop: rooms in, streamed edits out, the log written
//! (AD-370, AD-375; story 90.5).
//!
//! # Start, in order
//!
//! 1. `agentd.toml`'s providers become `keeper.db` rows.
//! 2. The engine opens behind the mount rule on the pins, and every drive is
//!    checked out once.
//! 3. Each zone is checked against its pin; a zone that differs hosts nothing.
//! 4. Every sessions zone resumes its interrupted plans, and its index is
//!    rebuilt from the files.
//! 5. Each hosted agent's copy is restored from its stored session.
//! 6. Each active session of a hosted agent whose room the copy has joined
//!    is served. Only an invite joins a room, as
//!    [`crate::rooms::invite_decision`] says: a session file naming a room
//!    is never a reason to join it.
//!
//! Then one 1 Hz tick (AD-62, the process's one clock) publishes the host's
//! status for `keeper-agentd status` when it changed; every [`SCAN`] it reads
//! the zones again, off the runtime, for sessions and homes that arrived
//! with a sync. Each served session has one worker, so its turns never
//! overlap. A worker first reads its room's timeline back to the newest
//! event its log has seen, so a message sent before it started is answered;
//! arrivals for a room with no worker yet are kept for it, a few per room.
//!
//! # Stop
//!
//! On shutdown every running turn is stopped: its answer so far gets its
//! final edit, "… (stopped: <host> is shutting down)", and its lines are
//! `fsync`ed. An arrival not yet started is left unlogged, for the next
//! start's timeline read. Then the engine finalises, bounded, and the
//! process exits. An engine that stops while the host runs stops the host
//! too, with the runtime's exit code, so its unit restarts it.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use keeper_core::agents::agentd::{AgentdConfig, DrivePin, TrustEntry};
use keeper_core::agents::drive::{self, DriveDecl};
use keeper_core::agents::events::{
    control_levels, ControlLevels, APPROVAL_DECISION, CONTROL_ROOM_TYPE, CONVERSATION_REQUEST,
    DELEGATE, DOORBELL, PRESENCE, SCOPE, SESSION_ROOM_TYPE, SURFACE_REQUEST, SURFACE_RESULT, TURN,
};
use keeper_core::agents::index::Index;
use keeper_core::agents::label::{Label, Readers};
use keeper_core::agents::log::{ClaimAction, HostSlug};
use keeper_core::agents::matrix::{self, AgentClient, RoomKind};
use keeper_core::agents::mount;
use keeper_core::agents::presence::Published;
use keeper_core::agents::room::sealed_by_sender;
use keeper_core::agents::session::{SessionAgent, SessionKind};
use keeper_core::agents::trust::PinState;
use keeper_core::auth::StoredSession;
use keeper_core::bots::chat::{self, CancelSignal};
use keeper_core::bots::store;
use keeper_sync::provenance::SyncSource;
use keeper_sync::xdg::XdgDirs;
use keeper_sync::SyncProfile;
use matrix_sdk::deserialized_responses::{EncryptionInfo, VerificationLevel, VerificationState};
use matrix_sdk::room::MessagesOptions;
use matrix_sdk::ruma::events::room::member::{MembershipState, StrippedRoomMemberEvent};
use matrix_sdk::ruma::events::AnySyncTimelineEvent;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{
    OwnedEventId, OwnedRoomId, OwnedTransactionId, OwnedUserId, RoomId, UInt, UserId,
};
use matrix_sdk::{LoopCtrl, Room, RoomMemberships, RoomState};
use serde_json::{json, Value};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::agent::{
    bot_for, reply_of, trail_of, AgentDeps, AgentProfiles, Arrived, ConversationPort, RoomFuture,
    ServedSession, SessionRef,
};
use crate::claims::Lease;
use crate::delegate::{BoolFuture, BriefRoomFuture, DelegationPort, EventsFuture, MembersFuture};
use crate::doorbell::{self as bells, Doorbell, DriveEngine, Ringer, RING_FINISH};
use crate::headless::{
    apply_providers, drive_path, open_engine, zone_verdicts, HeadlessError, HeadlessPlatform,
    HeadlessSyncPlatform, SecretMap,
};
use crate::hosts::{read_back_rooms, HostRuntime, Opening, PendingBriefs};
use crate::matrix_sink::{EditPort, RoomPort, SendFuture};
use crate::rooms::{
    self, Arrival, BriefEvent, BriefRoom, Invite, InviteDecision, Known, KnownAgent,
};
use crate::sinks::{ClientDoors, ProxyDoors};
use crate::surface::{self, PresenceFuture, SurfacePort};
use crate::turn::{DrivePorts, TurnEnv};
use crate::zone::{active_sessions, read_text, read_zone, AgentHome, FoundSession, ZoneRead};

/// The host's one clock (AD-62).
pub const TICK: Duration = Duration::from_secs(1);

/// How often the zones are read again: every session's `agent.toml` and
/// every home is file work, kept off the clock's every tick.
pub const SCAN: Duration = Duration::from_secs(5);

/// What `run` publishes, beside the secrets directory's parent: written when
/// it changes, and at least this often while nothing does.
pub const STATUS_FILE: &str = "status.json";
pub const STATUS_HEARTBEAT: Duration = Duration::from_secs(60);

/// How long the running turns get to send their final edits on shutdown.
pub const TURNS_FINISH: Duration = Duration::from_secs(15);

/// How long the engine gets to finalise on shutdown (syncd's bound).
pub const GRACEFUL_FINALIZE: Duration = Duration::from_secs(10);

/// What a room with no worker yet keeps for the worker to come: the newest
/// arrivals, in at most this many rooms. The worker's timeline read is what
/// makes nothing lost; these spare it a page.
pub const PENDING_PER_ROOM: usize = 16;
pub const PENDING_ROOMS: usize = 64;

/// How far back a starting worker reads its room's timeline for what its log
/// has not seen: pages of [`BACKLOG_PAGE`] events.
pub(crate) const BACKLOG_PAGES: usize = 20;
pub(crate) const BACKLOG_PAGE: u32 = 50;

/// One mounted drive, as this host reads it without the engine: the
/// checkout `open_engine` makes, its zones, and the pin's verdict.
#[derive(Debug, Clone)]
pub struct DriveView {
    pub id: String,
    pub profile: SyncProfile,
    /// The declaration that matched the pin, or why the zone hosts nothing.
    pub hosts: Result<DriveDecl, String>,
    pub zone: ZoneRead,
    pub sessions: Vec<FoundSession>,
}

/// What `status` and `agents list` read: the drives' checkouts as they are
/// on disk. Nothing is fetched and the engine is not opened, so a running
/// host's database is never touched by an inspection.
#[derive(Debug, Clone)]
pub struct Inspection {
    pub drives: Vec<DriveView>,
    /// The homes `[[agents]]` names that read cleanly.
    pub hosted: Vec<AgentHome>,
}

/// The profile `open_engine` gives drive `pin`: its checkout under
/// `drives/<id>/` with the agents and sessions zones at their defaults.
pub fn drive_profile(dirs: &XdgDirs, pin: &DrivePin) -> SyncProfile {
    let mut profile = SyncProfile::new(
        pin.id.clone(),
        pin.id.clone(),
        drive_path(dirs, &pin.id),
        pin.remote.clone(),
    );
    profile.sessions = Some(Default::default());
    profile.agents = Some(Default::default());
    profile
}

/// The pin's verdict on a checkout's `_drive.toml` (S-15). The declaration a
/// zone hosts under is the file's, with `local_only` taken from the pin as
/// well (review R34-03): a reader can edit `_drive.toml`, so the pin is what
/// keeps drive content away from a remote model.
pub fn pin_verdict(pin: &DrivePin, profile: &SyncProfile) -> Result<DriveDecl, String> {
    let zone = profile
        .agents_root()
        .ok_or_else(|| keeper_core::agents::zone::NO_DRIVE.to_owned())?;
    let text = read_text(&zone, drive::FILE_NAME)?
        .ok_or_else(|| keeper_core::agents::zone::NO_DRIVE.to_owned())?;
    let mut decl = drive::parse(&text).map_err(|refusal| refusal.sentence())?;
    mount::pin_matches(&decl, pin).map_err(|difference| difference.sentence())?;
    decl.local_only |= pin.local_only;
    Ok(decl)
}

/// Read every mounted drive's zones from disk.
pub fn inspect(config: &AgentdConfig, dirs: &XdgDirs) -> Inspection {
    let drives: Vec<DriveView> = config
        .drives
        .iter()
        .map(|pin| {
            let profile = drive_profile(dirs, pin);
            let hosts = pin_verdict(pin, &profile);
            view(&pin.id, profile, hosts)
        })
        .collect();
    let hosted = hosted_homes(config, &drives);
    Inspection { drives, hosted }
}

pub(crate) fn view(id: &str, profile: SyncProfile, hosts: Result<DriveDecl, String>) -> DriveView {
    let zone = read_zone(id, &profile, hosts.as_ref().ok());
    let sessions = if hosts.is_ok() {
        active_sessions(&profile)
    } else {
        Vec::new()
    };
    DriveView {
        id: id.to_owned(),
        profile,
        hosts,
        zone,
        sessions,
    }
}

/// The homes `[[agents]]` names, from zones that host.
pub fn hosted_homes(config: &AgentdConfig, drives: &[DriveView]) -> Vec<AgentHome> {
    let mut hosted = Vec::new();
    for entry in &config.agents {
        let Some(drive) = drives.iter().find(|drive| drive.id == entry.drive) else {
            continue;
        };
        for (folder, home) in &drive.zone.homes {
            if let (true, Ok(home)) = (entry.ids.contains(folder), home) {
                hosted.push(home.clone());
            }
        }
    }
    hosted
}

/// Check out every drive that has no checkout yet, through this host's own
/// engine behind the mount rule: what `login` and `init` need before `run`
/// has ever run. A drive already checked out is left alone, so a running
/// host's engine is never opened twice.
pub async fn check_out_missing(
    config: &AgentdConfig,
    dirs: &XdgDirs,
    secrets: &Arc<SecretMap>,
) -> Result<(), HeadlessError> {
    if config
        .drives
        .iter()
        .all(|pin| drive_path(dirs, &pin.id).exists())
    {
        return Ok(());
    }
    let sync_platform = Arc::new(HeadlessSyncPlatform::new(
        &dirs.data,
        config.host.clone(),
        Arc::clone(secrets),
    ));
    let agentd = open_engine(config, sync_platform)?;
    for mounted in &agentd.drives {
        if !mounted.local_path.exists() {
            agentd
                .engine
                .sync_once(&mounted.profile_id, SyncSource::Manual)
                .await?;
        }
    }
    agentd.engine.request_stop();
    Ok(())
}

/// What an invite is decided against: every agent of a drive this host
/// mounts, and the `[[trust]]` pins.
pub fn known(config: &AgentdConfig, drives: &[DriveView], hosted: &[AgentHome]) -> Known {
    known_with(config.trust.clone(), drives, hosted)
}

/// [`known`] over `trust` rather than a config's: the desktop has no
/// `[[trust]]` pins, so every other person's invite stays pending there.
pub(crate) fn known_with(
    trust: Vec<keeper_core::agents::agentd::TrustEntry>,
    drives: &[DriveView],
    hosted: &[AgentHome],
) -> Known {
    Known {
        agents: known_agents(drives.iter().map(|drive| &drive.zone), hosted),
        trust,
    }
}

/// Every agent `zones` home, each `hosted` when an entry of `hosted` is it.
pub(crate) fn known_agents<'a>(
    zones: impl IntoIterator<Item = &'a ZoneRead>,
    hosted: &[AgentHome],
) -> Vec<KnownAgent> {
    let mut agents = Vec::new();
    for zone in zones {
        for (_, home) in &zone.homes {
            let Ok(home) = home else { continue };
            let readers = Readers::Only(home.config.audience.clone());
            agents.push(KnownAgent {
                id: home.config.id.clone(),
                drive: home.config.drive.clone(),
                name: home.config.name.clone(),
                matrix_user: home.config.matrix_user.clone(),
                kind: home.config.kind,
                human: home.config.human.clone(),
                hosted: hosted.iter().any(|h| h.config.key() == home.config.key()),
                opening: Label {
                    readers: readers.clone(),
                    local_only: home.config.local_only,
                    ..Label::top()
                },
                home_readers: readers,
                drives: home.config.drives.clone(),
            });
        }
    }
    agents
}

/// Each drive's id and its engine's profile id: what a doorbell is mapped
/// by. agentd's ids are its pins'.
pub(crate) fn mounted(drives: &[DriveView]) -> Vec<(String, String)> {
    drives
        .iter()
        .map(|drive| (drive.id.clone(), drive.profile.id.clone()))
        .collect()
}

/// A copy's client, restored from its stored session; `None` when `login`
/// has not signed it in on this host.
pub async fn restore_copy(
    config: &AgentdConfig,
    platform: &HeadlessPlatform,
    data_dir: &Path,
    user: &OwnedUserId,
) -> Result<Option<AgentClient>, String> {
    open_copy(&config.homeserver.url.normalized, platform, data_dir, user).await
}

/// [`restore_copy`] over any host's secrets and homeserver: the session and
/// the store passphrase are read from `platform`'s keychain under the
/// copy's keys, `agents/<user>/…`.
pub async fn open_copy(
    homeserver: &str,
    platform: &dyn keeper_core::platform::Platform,
    data_dir: &Path,
    user: &OwnedUserId,
) -> Result<Option<AgentClient>, String> {
    let get = |key: String| {
        platform
            .keychain_get(&key)
            .map_err(|error| error.to_string())
    };
    let (Some(session), Some(passphrase)) = (
        get(matrix::session_key(user))?,
        get(matrix::passphrase_key(user))?,
    ) else {
        return Ok(None);
    };
    let client = AgentClient::open(homeserver, &matrix::store_dir(data_dir, user), &passphrase)
        .await
        .map_err(|error| error.to_string())?;
    let stored = StoredSession::from_json(&session).map_err(|error| error.to_string())?;
    client
        .restore(stored)
        .await
        .map_err(|error| error.to_string())?;
    Ok(Some(client))
}

/// Everything one hosted agent's turns run with, over this host's mounted
/// drives and provider rows.
pub fn agent_deps(
    platform: &Arc<HeadlessPlatform>,
    data_dir: &Path,
    host: &HostSlug,
    drives: &[DriveView],
    rows: &[keeper_core::bots::store::ProviderRow],
    home: &AgentHome,
) -> Result<AgentDeps, String> {
    deps_over(
        &TurnEnv::new(Arc::clone(platform) as Arc<dyn keeper_core::platform::Platform>),
        data_dir,
        host,
        drives,
        rows,
        home,
    )
}

/// [`agent_deps`] over `base`: its platform and account, with the drive
/// narrowed to the hosting drives, no notes-vault writer and nobody to
/// approve. No host's copies write a vault, so placement may treat every
/// host alike: a manifest has no word for "writes the person's vault".
pub(crate) fn deps_over(
    base: &TurnEnv,
    data_dir: &Path,
    host: &HostSlug,
    drives: &[DriveView],
    rows: &[keeper_core::bots::store::ProviderRow],
    home: &AgentHome,
) -> Result<AgentDeps, String> {
    let (row, bot) = bot_for(home, rows).ok_or_else(|| {
        format!(
            "no [[providers]] entry serves {}'s model {}; add one with kind = \"{}\" and base_url = \"{}\"",
            home.config.id,
            home.config.bot.target,
            home.config.bot.kind.as_registry_str(),
            home.config.bot.base
        )
    })?;
    let home_drive = drives
        .iter()
        .find(|d| d.id == home.config.drive)
        .ok_or_else(|| format!("{} is not a mounted drive", home.config.drive))?;
    let sessions_zone = home_drive
        .profile
        .sessions_root()
        .ok_or_else(|| format!("{} has no sessions zone", home.config.drive))?;
    let mounted: Vec<(String, SyncProfile)> = drives
        .iter()
        .filter(|drive| drive.hosts.is_ok())
        .map(|drive| (drive.id.clone(), drive.profile.clone()))
        .collect();
    let decls: BTreeMap<String, DriveDecl> = drives
        .iter()
        .filter_map(|drive| {
            drive
                .hosts
                .clone()
                .ok()
                .map(|decl| (drive.id.clone(), decl))
        })
        .collect();
    Ok(AgentDeps {
        env: TurnEnv {
            drive: Some(DrivePorts {
                profiles: Arc::new(AgentProfiles::new(mounted)),
                vault: None,
                approval: None,
            }),
            ..base.clone()
        },
        data_dir: data_dir.to_owned(),
        row,
        bot,
        home: home.clone(),
        host: host.clone(),
        drives: decls,
        sessions_zone,
        sessions_subfolder: home_drive
            .profile
            .sessions
            .as_ref()
            .map(|sessions| sessions.subfolder.clone())
            .unwrap_or_default(),
        lfs_threshold_bytes: home_drive.profile.lfs_threshold_bytes,
        decisions: None,
    })
}

/// Where one copy's arrivals go: the worker serving each room, and what
/// came for a room before its worker started.
#[derive(Default)]
pub struct Router {
    routes: Mutex<Routes>,
}

#[derive(Default)]
struct Routes {
    /// Per served room: the session folder, and its worker's channel.
    workers: HashMap<OwnedRoomId, (String, mpsc::UnboundedSender<Arrived>)>,
    pending: HashMap<OwnedRoomId, VecDeque<Arrived>>,
}

impl Router {
    fn routes(&self) -> std::sync::MutexGuard<'_, Routes> {
        self.routes.lock().unwrap_or_else(|p| p.into_inner())
    }

    /// Hand `arrived` to `room`'s worker, or keep it for the worker to come.
    pub fn route(&self, room: &RoomId, arrived: Arrived) {
        let mut routes = self.routes();
        let arrived = match routes.workers.get(room) {
            Some((_, worker)) => match worker.send(arrived) {
                Ok(()) => return,
                Err(closed) => closed.0,
            },
            None => arrived,
        };
        if routes.pending.len() >= PENDING_ROOMS && !routes.pending.contains_key(room) {
            return;
        }
        let kept = routes.pending.entry(room.to_owned()).or_default();
        if kept.len() == PENDING_PER_ROOM {
            kept.pop_front();
        }
        kept.push_back(arrived);
    }

    /// Run `serve` as `room`'s worker for `session`, its channel holding
    /// first what was kept for the room; `None` when a worker serves the room
    /// already. However the worker ends, the room is free again for the
    /// next one.
    pub fn spawn<F, W>(
        self: &Arc<Self>,
        room: &RoomId,
        session: &str,
        serve: F,
    ) -> Option<JoinHandle<()>>
    where
        F: FnOnce(mpsc::UnboundedReceiver<Arrived>) -> W,
        W: Future<Output = ()> + Send + 'static,
    {
        let receiver = {
            let mut routes = self.routes();
            if routes.workers.contains_key(room) {
                return None;
            }
            let (sender, receiver) = mpsc::unbounded_channel();
            for arrived in routes.pending.remove(room).unwrap_or_default() {
                let _ = sender.send(arrived);
            }
            routes
                .workers
                .insert(room.to_owned(), (session.to_owned(), sender));
            receiver
        };
        let freed = Detach {
            router: Arc::clone(self),
            room: room.to_owned(),
        };
        let work = serve(receiver);
        Some(tokio::spawn(async move {
            let _freed = freed;
            work.await;
        }))
    }

    /// The session folder each served room is served for.
    pub fn served(&self) -> BTreeMap<OwnedRoomId, String> {
        self.routes()
            .workers
            .iter()
            .map(|(room, (session, _))| (room.clone(), session.clone()))
            .collect()
    }

    /// Close every worker's channel.
    pub fn close(&self) {
        self.routes().workers.clear();
    }

    /// Drop what was kept for `room`: another host holds its claim and
    /// answers it (story 90.6).
    pub fn forget(&self, room: &RoomId) {
        self.routes().pending.remove(room);
    }

    /// Close `room`'s worker channel: its claim ended (story 90.6).
    pub fn close_room(&self, room: &RoomId) {
        self.routes().workers.remove(room);
    }

    /// Whether a worker serves `room`.
    pub fn serves(&self, room: &RoomId) -> bool {
        self.routes().workers.contains_key(room)
    }
}

/// Frees a worker's room when the worker ends, a panic included.
struct Detach {
    router: Arc<Router>,
    room: OwnedRoomId,
}

impl Drop for Detach {
    fn drop(&mut self) {
        self.router.routes().workers.remove(&self.room);
    }
}

/// One hosted agent's copy on this host while it runs.
pub(crate) struct Copy {
    pub(crate) deps: Arc<AgentDeps>,
    pub(crate) client: AgentClient,
    /// Read again with every scan, so a home that arrives by sync counts.
    pub(crate) known: Arc<RwLock<Arc<Known>>>,
    pub(crate) router: Arc<Router>,
    /// Counts the copy's completed `/sync` rounds: a taker's settle waits one.
    pub(crate) syncs: watch::Receiver<u64>,
    /// Rooms this copy's sessions delegated into, each with the room of the
    /// session that did: their joins and replies go to that session (R55).
    pub(crate) children: Arc<Mutex<HashMap<OwnedRoomId, OwnedRoomId>>>,
    /// The opening brief of each delegated room no session folder names
    /// yet, addressed to this agent: placement decides which host makes the
    /// session (R54). Bounded like the router's rooms.
    pub(crate) pending: Mutex<PendingBriefs>,
    /// The host's receiver: doorbells this copy hears are answered by it.
    pub(crate) doorbell: Arc<Doorbell>,
    /// What the harvest worker made of each closed session it was handed:
    /// the host's clock takes them (R61).
    pub(crate) harvest_acks: crate::agent::HarvestAcks,
    /// The proxies every copy of this host runs: a narrowed session's
    /// person is told through theirs (R169).
    pub(crate) doors: Arc<ClientDoors>,
}

/// The claim a worker writes under (story 90.6).
pub(crate) struct Claimed {
    pub(crate) lease: Arc<Lease>,
    /// The host the claim was taken from.
    pub(crate) from_host: Option<String>,
    /// The `claim` line the worker writes when it stops.
    pub(crate) ending: Arc<Mutex<ClaimAction>>,
    /// Busy while the worker works — starting included — and whether a
    /// call waits for a person (R177).
    pub(crate) activity: Arc<crate::agent::Activity>,
}

/// How the engine's supervisor ended.
enum Finalize {
    Done(Result<(), keeper_sync::SyncError>),
    Panicked(String),
    TimedOut,
}

/// `run`'s result from how the supervisor ended, and whether it stopped on
/// its own while the host ran. A panic, a timeout or an engine that stopped
/// by itself is a runtime failure (exit 1), which the unit restarts.
fn finalized(finalize: Finalize, stopped_by_itself: bool) -> Result<(), HeadlessError> {
    match finalize {
        Finalize::Done(Ok(())) if !stopped_by_itself => Ok(()),
        Finalize::Done(Ok(())) => Err(HeadlessError::Runtime(
            "the sync engine stopped while the host ran".to_owned(),
        )),
        Finalize::Done(Err(error)) => Err(error.into()),
        Finalize::Panicked(join) => Err(HeadlessError::Runtime(format!(
            "the sync supervisor panicked: {join}"
        ))),
        Finalize::TimedOut => Err(HeadlessError::Runtime(format!(
            "the sync supervisor did not finalize within {} s; its work stays journaled",
            GRACEFUL_FINALIZE.as_secs()
        ))),
    }
}

/// The zones read again: each drive's homes and active sessions, and what
/// an invite is decided against now.
fn rescan(
    config: &AgentdConfig,
    drives: &[DriveView],
    hosted: &[AgentHome],
) -> (Vec<DriveView>, Known) {
    let fresh: Vec<DriveView> = drives
        .iter()
        .map(|drive| view(&drive.id, drive.profile.clone(), drive.hosts.clone()))
        .collect();
    let known = known(config, &fresh, hosted);
    (fresh, known)
}

/// One agent's sessions, one per room.
pub(crate) struct RoomSessions<'a> {
    /// The session that serves each room: the first by path.
    pub(crate) served: Vec<(&'a FoundSession, &'a SessionAgent)>,
    /// Every other one, with the session it lost the room to.
    pub(crate) shadowed: Vec<(&'a FoundSession, String)>,
}

/// The sessions of `copy`'s agent among `found`.
pub(crate) fn sessions_of<'a>(copy: &Copy, found: &'a [FoundSession]) -> RoomSessions<'a> {
    let config = &copy.deps.home.config;
    let mut mine: Vec<(&FoundSession, &SessionAgent)> = found
        .iter()
        .filter_map(|session| session.agent.as_ref().ok().map(|agent| (session, agent)))
        .filter(|(_, agent)| agent.agent == config.id && agent.drive == config.drive)
        .collect();
    mine.sort_by(|a, b| a.0.path.cmp(&b.0.path));
    let mut first: HashMap<&OwnedRoomId, &str> = HashMap::new();
    let mut sessions = RoomSessions {
        served: Vec::new(),
        shadowed: Vec::new(),
    };
    for (session, agent) in mine {
        match first.get(&agent.room) {
            Some(winner) => sessions.shadowed.push((session, (*winner).to_owned())),
            None => {
                first.insert(&agent.room, &session.path);
                sessions.served.push((session, agent));
            }
        }
    }
    sessions
}

/// A restored copy, live: its invite and timeline handlers registered and
/// its sync loop running, which counts the rounds a taker's settle waits on.
pub(crate) fn start_copy(
    deps: Arc<AgentDeps>,
    client: AgentClient,
    known: Arc<RwLock<Arc<Known>>>,
    doorbell: Arc<Doorbell>,
    doors: Arc<ClientDoors>,
) -> (Arc<Copy>, JoinHandle<()>) {
    let (rounds, rounds_seen) = watch::channel(0u64);
    doors.add(&client, &deps.home.config, &deps.sessions_zone);
    let copy = Arc::new(Copy {
        deps,
        client,
        known,
        router: Arc::new(Router::default()),
        syncs: rounds_seen,
        children: Arc::default(),
        pending: Mutex::default(),
        doorbell,
        harvest_acks: Arc::default(),
        doors,
    });
    register_handlers(&copy);
    let sync_client = copy.client.client().clone();
    let sync = tokio::spawn(async move {
        let rounds = &rounds;
        let synced = sync_client
            .sync_with_callback(matrix::sync_settings(), |_| async move {
                rounds.send_modify(|count| *count += 1);
                LoopCtrl::Continue
            })
            .await;
        if let Err(error) = synced {
            tracing::error!(%error, "agents: a copy's sync loop ended");
        }
    });
    (copy, sync)
}

/// Run the host until `shutdown` turns `true`.
pub async fn run(
    config: AgentdConfig,
    dirs: XdgDirs,
    secrets: Arc<SecretMap>,
    mut shutdown: watch::Receiver<bool>,
) -> Result<(), HeadlessError> {
    let config = Arc::new(config);
    let host =
        HostSlug::new(&config.host).map_err(|error| HeadlessError::Config(error.to_string()))?;
    let platform = Arc::new(HeadlessPlatform::new(&dirs.data, Arc::clone(&secrets)));
    let sync_platform = Arc::new(HeadlessSyncPlatform::new(
        &dirs.data,
        config.host.clone(),
        Arc::clone(&secrets),
    ));
    apply_providers(&config, &dirs.data, &secrets)?;

    let agentd = open_engine(&config, sync_platform)?;
    for mounted in &agentd.drives {
        if let Err(error) = agentd
            .engine
            .sync_once(&mounted.profile_id, SyncSource::Manual)
            .await
        {
            tracing::warn!(drive = %mounted.id, %error, "agentd: the first checkout did not finish; serving what is on disk");
        }
    }
    let verdicts = zone_verdicts(&config, &agentd)?;
    let profiles = agentd.engine.list_profiles()?;
    let drives: Vec<DriveView> = agentd
        .drives
        .iter()
        .filter_map(|mounted| {
            let profile = profiles
                .iter()
                .find(|p| p.id == mounted.profile_id)?
                .clone();
            let hosts = verdicts
                .iter()
                .find(|v| v.drive == mounted.id)
                .map_or_else(|| Err("not checked".to_owned()), |v| v.hosts.clone());
            Some(view(&mounted.id, profile, hosts))
        })
        .collect();
    for drive in &drives {
        if let Err(sentence) = &drive.hosts {
            tracing::warn!(drive = %drive.id, %sentence, "agentd: this drive's zone hosts nothing");
        }
    }

    let sessions_zones: Vec<PathBuf> = drives
        .iter()
        .filter(|drive| drive.hosts.is_ok())
        .filter_map(|drive| drive.profile.sessions_root())
        .collect();
    for (zone, error) in crate::sessions::resume_all(sessions_zones.iter().map(PathBuf::as_path)) {
        tracing::warn!(zone = %zone.display(), %error, "agentd: an interrupted session plan could not be resumed");
    }
    for zone in &sessions_zones {
        match Index::open(zone).and_then(|mut index| index.rebuild()) {
            Ok(report) => {
                for problem in report.problems {
                    tracing::warn!(zone = %zone.display(), %problem, "agentd: index rebuild");
                }
            }
            Err(error) => {
                tracing::warn!(zone = %zone.display(), %error, "agentd: the agents index could not be rebuilt")
            }
        }
    }

    let hosted = Arc::new(hosted_homes(&config, &drives));
    let known = Arc::new(RwLock::new(Arc::new(known(&config, &drives, &hosted))));
    let rows = store::list_providers(&dirs.data)?.rows;

    let (stop_turns, stop_signal) = chat::cancellation();
    let doorbell = Arc::new(Doorbell::default());
    let doors = Arc::new(ClientDoors::default());
    doorbell.set_engine(Arc::clone(&agentd.engine) as Arc<dyn DriveEngine>);
    doorbell.set_drives(
        mounted(&drives),
        known
            .read()
            .unwrap_or_else(|p| p.into_inner())
            .agents
            .clone(),
    );
    let mut copies: Vec<Arc<Copy>> = Vec::new();
    let mut syncs: Vec<JoinHandle<()>> = Vec::new();
    for home in hosted.iter() {
        let user = home.config.matrix_user.clone();
        let client = match restore_copy(&config, &platform, &dirs.data, &user).await {
            Ok(Some(client)) => client,
            Ok(None) => {
                tracing::warn!(%user, "agentd: this copy is not signed in; run `keeper-agentd login {}/{}`", home.config.drive, home.config.id);
                continue;
            }
            Err(error) => {
                tracing::error!(%user, %error, "agentd: this copy could not be restored");
                continue;
            }
        };
        let deps = match agent_deps(&platform, &dirs.data, &host, &drives, &rows, home) {
            Ok(deps) => Arc::new(deps),
            Err(sentence) => {
                tracing::error!(agent = %home.config.id, %sentence, "agentd: this agent is not served");
                continue;
            }
        };
        let (copy, sync) = start_copy(
            deps,
            client,
            Arc::clone(&known),
            Arc::clone(&doorbell),
            Arc::clone(&doors),
        );
        syncs.push(sync);
        copies.push(copy);
    }

    let mut hosts = HostRuntime::agentd(
        &config,
        host.clone(),
        env!("CARGO_PKG_VERSION"),
        &drives,
        copies.clone(),
    );
    doorbell.set_principal_agents(hosts.principal_agents());
    // Each steward's triage and harvest sessions are made from the first
    // tick on, beside the lease clock (R66, R165).
    hosts.make_stewards(
        copies
            .iter()
            .filter(|copy| {
                copy.deps.home.config.kind == keeper_core::agents::home::AgentKind::Steward
            })
            .map(|copy| Arc::clone(copy) as Arc<dyn crate::hosts::CopyPort>)
            .collect(),
    );

    // Tapped before the supervisor runs, so its first push is seen.
    let mut ringer = Ringer::new(Arc::clone(&agentd.engine) as Arc<dyn DriveEngine>);
    let (engine_stop, engine_shutdown) = watch::channel(false);
    let engine = Arc::clone(&agentd.engine);
    let mut supervisor = tokio::spawn(async move { engine.run(engine_shutdown).await });

    let mut status = StatusFile::new(dirs.state.join(STATUS_FILE));
    let trust = Arc::new(Mutex::new(Vec::<Value>::new()));
    let trust_reader = tokio::spawn(read_trust(
        config.trust.clone(),
        copies.clone(),
        Arc::clone(&trust),
    ));
    let mut drives = Arc::new(drives);
    let mut shadowed: Vec<(String, Value)> = Vec::new();
    let mut reported: HashSet<String> = HashSet::new();
    let mut last_scan: Option<Instant> = None;
    let mut stopped_by_itself = false;
    let mut control_checked = false;
    let mut ticker = tokio::time::interval(TICK);
    ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
    loop {
        tokio::select! {
            _ = ticker.tick() => {}
            changed = shutdown.changed() => {
                if changed.is_err() || *shutdown.borrow() {
                    break;
                }
            }
        }
        if supervisor.is_finished() {
            tracing::error!(
                "agentd: the sync engine stopped while the host ran; stopping the host"
            );
            stopped_by_itself = true;
            break;
        }
        if last_scan.is_none_or(|at| at.elapsed() >= SCAN) {
            last_scan = Some(Instant::now());
            let (config, before, homes) = (
                Arc::clone(&config),
                Arc::clone(&drives),
                Arc::clone(&hosted),
            );
            match tokio::task::spawn_blocking(move || rescan(&config, &before, &homes)).await {
                Ok((fresh, now_known)) => {
                    drives = Arc::new(fresh);
                    doorbell.set_drives(mounted(&drives), now_known.agents.clone());
                    *known.write().unwrap_or_else(|p| p.into_inner()) = Arc::new(now_known);
                }
                Err(error) => tracing::error!(%error, "agentd: the zones could not be read again"),
            }
            shadowed.clear();
            for copy in &copies {
                let Some(home_drive) = drives.iter().find(|d| d.id == copy.deps.home.config.drive)
                else {
                    continue;
                };
                let RoomSessions {
                    served,
                    shadowed: lost,
                } = sessions_of(copy, &home_drive.sessions);
                // Each served session is placed and claimed by the host
                // runtime; only the claim's holder starts a worker.
                for (session, agent) in served {
                    hosts.offer(copy, session, agent);
                }
                for (session, winner) in lost {
                    if reported.insert(session.path.clone()) {
                        tracing::warn!(session = %session.path, served = %winner, "agentd: two sessions name one room; only the first is served");
                    }
                    shadowed.push((
                        copy.deps.home.config.matrix_user.to_string(),
                        json!({ "session": session.path, "room_served_for": winner }),
                    ));
                }
            }
            // A session the rescan no longer finds is handed back.
            hosts.scanned();
        }
        hosts.tick(&stop_signal).await;
        ringer.tick(&hosts.round(&drives), &doorbell);
        // Once a copy has synced, a control room made before presence or
        // the doorbell existed is brought up to date (R37, R59).
        if !control_checked && copies.iter().any(|copy| *copy.syncs.borrow() > 0) {
            control_checked = true;
            if let Some(room) = &config.homeserver.control_room {
                let mut settled = false;
                for copy in copies.iter().filter(|copy| *copy.syncs.borrow() > 0) {
                    let me = copy.deps.home.config.matrix_user.clone();
                    if update_control_room(&copy.client, room, &me).await {
                        settled = true;
                        break;
                    }
                }
                if !settled {
                    tracing::warn!(
                        %room,
                        "agentd: the control room does not let people publish their presence or a visiting agent ring, and no agent here may change it; its creator's host must, or surface calls find no device and a shared drive's doorbell is not heard"
                    );
                }
            }
        }
        let trust_lines = trust.lock().unwrap_or_else(|p| p.into_inner()).clone();
        status.publish(
            &config,
            &agentd.engine,
            &copies,
            &shadowed,
            &hosts,
            &trust_lines,
        );
    }
    trust_reader.abort();

    tracing::info!("agentd: stopping; running turns get their final edits");
    stop_turns.cancel();
    hosts.stop_workers(TURNS_FINISH).await;
    agentd.engine.request_stop();
    let _ = engine_stop.send(true);
    let finalize = match tokio::time::timeout(GRACEFUL_FINALIZE, &mut supervisor).await {
        Ok(Ok(done)) => Finalize::Done(done),
        Ok(Err(join)) => Finalize::Panicked(join.to_string()),
        Err(_) => {
            supervisor.abort();
            Finalize::TimedOut
        }
    };
    // What the last pushes published is rung before the copies go quiet.
    ringer
        .finish(&hosts.round(&drives), &doorbell, RING_FINISH)
        .await;
    // The log is pushed: now another host may take the sessions (AD-378).
    hosts.release_all().await;
    for sync in syncs {
        sync.abort();
    }
    finalized(finalize, stopped_by_itself)
}

/// Start serving `session` under `claimed` when the copy has joined its room
/// and no worker serves it yet. Joining is the invite rule's alone (F5): any
/// reader of the home drive can write a session file naming any room.
pub(crate) fn spawn_worker(
    copy: &Arc<Copy>,
    session: &FoundSession,
    agent: &SessionAgent,
    stop: &CancelSignal,
    claimed: Claimed,
) -> Option<JoinHandle<()>> {
    let joined = copy
        .client
        .client()
        .get_room(&agent.room)
        .is_some_and(|room| room.state() == RoomState::Joined);
    if !joined {
        return None;
    }
    let worker = Arc::clone(copy);
    let session = session.clone();
    let agent = agent.clone();
    let stop = stop.clone();
    copy.router.spawn(
        &agent.room.clone(),
        &session.path.clone(),
        move |arrivals| serve_session(worker, session, agent, arrivals, stop, claimed),
    )
}

async fn serve_session(
    copy: Arc<Copy>,
    session: FoundSession,
    agent: SessionAgent,
    mut arrivals: mpsc::UnboundedReceiver<Arrived>,
    stop: CancelSignal,
    claimed: Claimed,
) {
    // A worker that ends — or never starts — holds nothing busy.
    let _idle = Idle(Arc::clone(&claimed.activity));
    let room_id = agent.room.clone();
    let Some(room) = copy.client.client().get_room(&room_id) else {
        return;
    };
    let reference = SessionRef {
        drive: copy.deps.home.config.drive.clone(),
        path: session.path.clone(),
    };
    let (deps, dir) = (Arc::clone(&copy.deps), session.dir.clone());
    let lease = Arc::clone(&claimed.lease);
    let held = Some(Arc::clone(&lease));
    let opened = tokio::task::spawn_blocking(move || {
        ServedSession::open(&deps, &dir, reference, agent, held)
    })
    .await;
    let mut served = match opened {
        Ok(Ok(served)) => served,
        Ok(Err(error)) => {
            tracing::error!(session = %session.path, %error, "agentd: this session is not served");
            return;
        }
        Err(error) => {
            tracing::error!(session = %session.path, %error, "agentd: opening this session failed");
            return;
        }
    };
    // The first line under a claim says so, with its event and server time.
    let acquired = lease.line(ClaimAction::Acquired, claimed.from_host.clone());
    if let Err(error) = served
        .writer
        .write_claim(&mut served.context, acquired)
        .and_then(|_| served.writer.sync())
    {
        tracing::error!(session = %session.path, %error, "agentd: the claim line could not be written");
        return;
    }
    let deps = &copy.deps;
    let me = &deps.home.config.matrix_user;
    let rooms = Arc::new(ClientRooms {
        client: copy.client.clone(),
        known: Arc::clone(&copy.known),
        children: Arc::clone(&copy.children),
    });
    served.conversations = Some(Arc::clone(&rooms) as Arc<dyn ConversationPort>);
    served.delegations = Some(rooms);
    served.harvests = Some(Arc::clone(&copy.harvest_acks));
    served.doors = Some(Arc::clone(&copy.doors) as Arc<dyn ProxyDoors>);
    let (router, inbox_room) = (Arc::clone(&copy.router), room_id.clone());
    served.inbox = Some(Arc::new(move |arrived| router.route(&inbox_room, arrived)));
    served.surface = Some(Arc::new(ClientSurface {
        client: copy.client.clone(),
        room: room_id.clone(),
        known: Arc::clone(&copy.known),
    }));
    let port: Arc<dyn EditPort> = Arc::new(RoomPort::new(copy.client.clone(), room_id.clone()));
    served.approval_room = Some(Arc::new(ClientApprovals::new(
        copy.client.clone(),
        room_id,
        me.clone(),
        deps.host.as_str(),
    )));
    let (events, mut backlog) = read_back(&room, &mut served, me).await;
    // A taker's log may not hold the last holder's lines yet: what another
    // copy of this agent already answered is not asked again.
    let answered = answered_by(&events, me);
    backlog.retain(|arrived| !answered.contains(arrived.event_id.as_str()));
    // A harvest another copy started — its anchor is in the room — is not
    // started again, though its `peer` line never reached this log.
    served.context.started(answered.iter().map(String::as_str));
    let trail = trail_of(&events, me, served.context.unanswered);
    match served.recover(deps, Arc::clone(&port), &trail).await {
        Ok(true) => {
            tracing::info!(session = %session.path, "agentd: an interrupted turn was closed, not re-run")
        }
        Ok(false) => {}
        Err(error) => {
            tracing::error!(session = %session.path, %error, "agentd: the interrupted turn could not be closed")
        }
    }
    // What this session's delegations did while no worker served it.
    backlog.extend(served.resume_delegations(deps).await);
    served
        .serve_arrivals(deps, port, backlog, &mut arrivals, stop, &claimed.activity)
        .await;
    let ending = *claimed.ending.lock().unwrap_or_else(|p| p.into_inner());
    if let Err(error) = served
        .writer
        .write_claim(&mut served.context, lease.line(ending, None))
        .and_then(|_| served.writer.sync())
    {
        tracing::warn!(session = %session.path, %error, "agentd: the claim's last line could not be written");
    }
}

/// Clears a worker's busy flag when it ends, however it ends.
struct Idle(Arc<crate::agent::Activity>);

impl Drop for Idle {
    fn drop(&mut self) {
        self.0
            .busy
            .store(false, std::sync::atomic::Ordering::SeqCst);
    }
}

/// The session room's approval reads over the copy's own client (R75,
/// R86): `consumed` as unencrypted state, read forward in room order. A
/// live test drives it as a worker does.
pub struct ClientApprovals {
    client: AgentClient,
    room: OwnedRoomId,
    agent: OwnedUserId,
    host: String,
}

impl ClientApprovals {
    /// `client`'s approvals in `room`, its agent user `agent`, on `host`.
    pub fn new(client: AgentClient, room: OwnedRoomId, agent: OwnedUserId, host: &str) -> Self {
        ClientApprovals {
            client,
            room,
            agent,
            host: host.to_owned(),
        }
    }
}

impl crate::approvals::ApprovalRoom for ClientApprovals {
    fn upload(&self, bytes: Vec<u8>) -> crate::approvals::RoomFuture<'_, Value> {
        Box::pin(async move { self.client.upload_encrypted(&bytes).await })
    }

    fn consume(
        &self,
        content: keeper_core::agents::events::ConsumedContent,
    ) -> crate::approvals::RoomFuture<'_, OwnedEventId> {
        Box::pin(async move {
            let value = serde_json::to_value(&content).map_err(|err| {
                keeper_core::agents::matrix::AgentMatrixError::Other(err.to_string())
            })?;
            crate::claims::bounded(self.client.send_state(
                &self.room,
                keeper_core::agents::events::APPROVAL_CONSUMED,
                &content.id,
                &value,
            ))
            .await
        })
    }

    fn consumed<'a>(
        &'a self,
        id: &'a str,
        from: Option<&'a matrix_sdk::ruma::EventId>,
    ) -> crate::approvals::RoomFuture<'a, crate::approvals::ConsumedRead> {
        Box::pin(async move {
            let read = crate::claims::bounded(self.client.state_events_from(
                &self.room,
                from,
                keeper_core::agents::events::APPROVAL_CONSUMED,
                id,
            ))
            .await?;
            // Only the session's agent user counts: a person cannot send
            // state here, and another agent's would not be ours.
            Ok(crate::approvals::ConsumedRead {
                consumed: read
                    .found
                    .into_iter()
                    .filter(|state| state.sender == self.agent)
                    .filter_map(|state| {
                        Some(crate::approvals::Consumed {
                            event: state.event_id,
                            content: serde_json::from_value(state.content).ok()?,
                        })
                    })
                    .collect(),
                complete: read.complete,
            })
        })
    }

    fn holds(&self, epoch: u64) -> crate::approvals::RoomFuture<'_, bool> {
        Box::pin(async move {
            let state = crate::claims::bounded(self.client.server_state(
                &self.room,
                keeper_core::agents::events::CLAIM,
                "",
            ))
            .await?;
            Ok(state
                .and_then(|state| {
                    serde_json::from_value::<keeper_core::agents::events::ClaimContent>(
                        state.content,
                    )
                    .ok()
                })
                .is_some_and(|claim| {
                    claim.host == self.host && claim.epoch == epoch && !claim.released
                }))
        })
    }
}

/// The copy's own client, making the conversations a `main` session's
/// person asks for (R36).
struct ClientRooms {
    client: AgentClient,
    known: Arc<RwLock<Arc<Known>>>,
    children: Arc<Mutex<HashMap<OwnedRoomId, OwnedRoomId>>>,
}

impl ConversationPort for ClientRooms {
    fn create<'a>(&'a self, name: &'a str, person: &'a UserId) -> RoomFuture<'a> {
        Box::pin(self.client.create_room(
            RoomKind::Session(SessionKind::Conversation),
            name,
            vec![person.to_owned()],
            &[],
        ))
    }

    fn send<'a>(&'a self, room: &'a RoomId, event_type: &'a str, content: Value) -> SendFuture<'a> {
        Box::pin(self.client.send(room, event_type, content, None))
    }

    fn discard<'a>(
        &'a self,
        room: &'a RoomId,
        person: &'a UserId,
    ) -> Pin<Box<dyn Future<Output = ()> + Send + 'a>> {
        Box::pin(async move {
            let Some(joined) = self.client.client().get_room(room) else {
                return;
            };
            if let Err(error) = joined
                .kick_user(person, Some("This conversation could not be opened."))
                .await
            {
                tracing::warn!(%room, %error, "agentd: the invite to an unopened conversation could not be revoked");
            }
            if let Err(error) = joined.leave().await {
                tracing::warn!(%room, %error, "agentd: an unopened conversation could not be left");
                return;
            }
            if let Err(error) = joined.forget().await {
                tracing::warn!(%room, %error, "agentd: an unopened conversation could not be forgotten");
            }
        })
    }

    fn joined<'a>(
        &'a self,
        room: &'a RoomId,
        person: &'a UserId,
    ) -> Pin<Box<dyn Future<Output = bool> + Send + 'a>> {
        Box::pin(async move {
            let Some(room) = self.client.client().get_room(room) else {
                return false;
            };
            matches!(
                room.get_member_no_sync(person).await,
                Ok(Some(member)) if *member.membership() == MembershipState::Join
            )
        })
    }
}

impl DelegationPort for ClientRooms {
    fn known(&self) -> Arc<Known> {
        Arc::clone(&self.known.read().unwrap_or_else(|p| p.into_inner()))
    }

    fn create<'a>(
        &'a self,
        name: &'a str,
        invite: Vec<OwnedUserId>,
        agents: Vec<OwnedUserId>,
    ) -> RoomFuture<'a> {
        Box::pin(async move {
            self.client
                .create_room(
                    RoomKind::Session(SessionKind::Delegated),
                    name,
                    invite,
                    &agents,
                )
                .await
        })
    }

    fn send<'a>(
        &'a self,
        room: &'a RoomId,
        content: Value,
        txn: OwnedTransactionId,
    ) -> SendFuture<'a> {
        Box::pin(async move {
            self.client
                .send(room, "m.room.message", content, Some(&txn))
                .await
        })
    }

    fn joined<'a>(&'a self, room: &'a RoomId, user: &'a UserId) -> BoolFuture<'a> {
        ConversationPort::joined(self, room, user)
    }

    fn members<'a>(&'a self, room: &'a RoomId) -> MembersFuture<'a> {
        Box::pin(async move {
            let room = self
                .client
                .client()
                .get_room(room)
                .ok_or_else(|| "this copy is not in the room".to_owned())?;
            members_of(&room).await
        })
    }

    fn since_brief<'a>(&'a self, room: &'a RoomId, me: &'a UserId) -> EventsFuture<'a> {
        Box::pin(async move {
            let room = self
                .client
                .client()
                .get_room(room)
                .ok_or_else(|| "this copy is not in the room".to_owned())?;
            after_brief(|from| page_back(&room, from), me).await
        })
    }

    fn brief_room<'a>(&'a self, room: &'a RoomId) -> BriefRoomFuture<'a> {
        Box::pin(async move {
            let room = self.client.client().get_room(room)?;
            brief_room(&room).await
        })
    }

    fn watch(&self, child: &RoomId, parent: &RoomId) {
        self.children
            .lock()
            .unwrap_or_else(|p| p.into_inner())
            .insert(child.to_owned(), parent.to_owned());
    }
}

/// The copy's own client, carrying a session's surface calls (AD-383): the
/// presences of the principal's control room — one a known agent made — and
/// the session room the requests go into.
struct ClientSurface {
    client: AgentClient,
    room: OwnedRoomId,
    known: Arc<RwLock<Arc<Known>>>,
}

impl SurfacePort for ClientSurface {
    fn room(&self) -> &RoomId {
        &self.room
    }

    fn presences(&self) -> PresenceFuture<'_> {
        Box::pin(async move {
            let known = Arc::clone(&self.known.read().unwrap_or_else(|p| p.into_inner()));
            let controls: Vec<OwnedRoomId> = self
                .client
                .client()
                .joined_rooms()
                .into_iter()
                .filter(|room| {
                    room.room_type()
                        .is_some_and(|kind| kind.to_string() == CONTROL_ROOM_TYPE)
                        && room.creators().is_some_and(|creators| {
                            creators.iter().any(|creator| {
                                known
                                    .agents
                                    .iter()
                                    .any(|agent| agent.matrix_user == *creator)
                            })
                        })
                })
                .map(|room| room.room_id().to_owned())
                .collect();
            let mut presences = Vec::new();
            for room in controls {
                for (state_key, state) in self.client.cached_states(&room, PRESENCE).await {
                    presences.push(Published {
                        state_key,
                        sender: state.sender,
                        content: state.content,
                    });
                }
            }
            presences
        })
    }

    fn request(&self, content: Value) -> SendFuture<'_> {
        Box::pin(self.client.send(&self.room, SURFACE_REQUEST, content, None))
    }
}

/// Bring the control room `room` up to date with R37 and R59 — the person's
/// devices publish their presence at 0, a visiting agent rings at 0 — when
/// `me` may. Whether that is settled: `false` only when `me` may not change
/// the room's power levels.
async fn update_control_room(client: &AgentClient, room: &RoomId, me: &UserId) -> bool {
    let levels = match client.server_state(room, "m.room.power_levels", "").await {
        Ok(Some(state)) => state.content,
        Ok(None) => return true,
        Err(error) => {
            tracing::warn!(%room, %error, "agentd: the control room's power levels could not be read");
            return true;
        }
    };
    match control_levels(&levels, me) {
        ControlLevels::UpToDate => true,
        ControlLevels::Update(updated) => {
            match client
                .send_state(room, "m.room.power_levels", "", &updated)
                .await
            {
                Ok(_) => {
                    tracing::info!(%room, "agentd: the control room now takes the person's presence and a visitor's doorbell")
                }
                Err(error) => {
                    tracing::warn!(%room, %error, "agentd: the control room's power levels could not be updated")
                }
            }
            true
        }
        ControlLevels::NoPower => false,
    }
}

/// `room`'s timeline after the newest event `served`'s log has seen, oldest
/// first: every event, for the interrupted turn's [`crate::agent::Trail`],
/// and the arrivals among them from anyone but `me`.
async fn read_back(
    room: &Room,
    served: &mut ServedSession,
    me: &UserId,
) -> (Vec<Value>, Vec<Arrived>) {
    let mut unseen: Vec<(Value, Option<Arc<EncryptionInfo>>)> = Vec::new();
    let mut from: Option<String> = None;
    'pages: for _ in 0..BACKLOG_PAGES {
        let mut options = MessagesOptions::backward().from(from.as_deref());
        options.limit = UInt::from(BACKLOG_PAGE);
        let page = match room.messages(options).await {
            Ok(page) => page,
            Err(error) => {
                tracing::warn!(room = %room.room_id(), %error, "agentd: the room's timeline could not be read back");
                break;
            }
        };
        for event in &page.chunk {
            let Ok(value) = event.raw().deserialize_as::<Value>() else {
                continue;
            };
            let seen = value["event_id"]
                .as_str()
                .and_then(|id| OwnedEventId::try_from(id).ok())
                .is_some_and(|id| served.writer.seen(&id).unwrap_or(false));
            if seen {
                break 'pages;
            }
            unseen.push((value, event.encryption_info().cloned()));
        }
        match page.end {
            Some(end) if !page.chunk.is_empty() => from = Some(end),
            _ => break,
        }
    }
    unseen.reverse();
    let now = Instant::now();
    let arrivals = unseen
        .iter()
        .filter(|(value, _)| value["sender"].as_str() != Some(me.as_str()))
        .filter_map(|(value, encryption)| arrival_of(value, encryption.as_deref(), now))
        .map(|arrived| Arrived {
            replay: true,
            ..arrived
        })
        .collect();
    (
        unseen.into_iter().map(|(value, _)| value).collect(),
        arrivals,
    )
}

/// The anchor of the latest status `me` sent in `room`, read newest page
/// first: the anchor a host edits rather than posting a second one.
pub(crate) async fn latest_status(room: &Room, me: &UserId) -> Option<OwnedEventId> {
    let mut from: Option<String> = None;
    for _ in 0..BACKLOG_PAGES {
        let mut options = MessagesOptions::backward().from(from.as_deref());
        options.limit = UInt::from(BACKLOG_PAGE);
        let page = room.messages(options).await.ok()?;
        let mut events: Vec<Value> = page
            .chunk
            .iter()
            .filter_map(|event| event.raw().deserialize_as::<Value>().ok())
            .collect();
        events.reverse();
        if let Some((anchor, _)) = trail_of(&events, me, None).status {
            return Some(anchor);
        }
        match page.end {
            Some(end) if !page.chunk.is_empty() => from = Some(end),
            _ => return None,
        }
    }
    None
}

/// The questions among `events` (oldest first) that a copy of `me` already
/// started answering. An answer's anchor names its question; one that does
/// not answers the oldest question before it no anchor answered yet, as a
/// room's turns run in order. A question no person asked in the room — a
/// harvest's, a scheduled run's — is answered by the anchor naming it.
fn answered_by(events: &[Value], me: &UserId) -> HashSet<String> {
    let mut open: VecDeque<&str> = VecDeque::new();
    let mut answered = HashSet::new();
    for event in events {
        if event["sender"] == me.as_str() {
            if event["content"][TURN].is_object() {
                match event["content"][TURN]["question"].as_str() {
                    Some(question) => {
                        open.retain(|asked| *asked != question);
                        answered.insert(question.to_owned());
                    }
                    None => {
                        if let Some(question) = open.pop_front() {
                            answered.insert(question.to_owned());
                        }
                    }
                }
            }
            continue;
        }
        let edit = event["content"]["m.relates_to"]["rel_type"] == "m.replace";
        if event["type"] == "m.room.message" && !edit {
            if let Some(id) = event["event_id"].as_str() {
                open.push_back(id);
            }
        }
    }
    answered
}

/// The copy's invite, doorbell and timeline handlers.
fn register_handlers(copy: &Arc<Copy>) {
    let client = copy.client.client();
    bells::listen(client, Arc::clone(&copy.doorbell));
    let invites = Arc::clone(copy);
    client.add_event_handler(move |event: StrippedRoomMemberEvent, room: Room| {
        let copy = Arc::clone(&invites);
        async move {
            let me = &copy.deps.home.config.matrix_user;
            if event.state_key != *me || event.content.membership != MembershipState::Invite {
                return;
            }
            let invite = Invite {
                room_type: room.room_type().map(|kind| kind.to_string()),
                inviter: event.sender.clone(),
                invited: me.clone(),
            };
            let known = Arc::clone(&copy.known.read().unwrap_or_else(|p| p.into_inner()));
            match rooms::invite_decision(&invite, &known) {
                InviteDecision::Join => {
                    if let Err(error) = room.join().await {
                        tracing::warn!(room = %room.room_id(), %error, "agentd: could not join on invite");
                    }
                }
                InviteDecision::Pending => tracing::info!(
                    room = %room.room_id(), inviter = %event.sender,
                    "agentd: an invite stays pending"
                ),
            }
        }
    });
    let timeline = Arc::clone(copy);
    client.add_event_handler(
        move |event: Raw<AnySyncTimelineEvent>, room: Room, encryption: Option<EncryptionInfo>| {
            let copy = Arc::clone(&timeline);
            async move {
                let received_at = Instant::now();
                let Ok(value) = event.deserialize_as::<Value>() else {
                    return;
                };
                // A doorbell is the host's, never a session's: heard by the
                // state handler (`doorbell::listen`), which a timeline's
                // state event reaches as well, and never routed (R59).
                if value["type"] == DOORBELL {
                    return;
                }
                // A device's answer to a surface call goes to the call
                // waiting on it, never to the session's worker, which that
                // call is holding (R39).
                if value["type"] == SURFACE_RESULT {
                    let sender = value["sender"].as_str().and_then(|s| UserId::parse(s).ok());
                    let taken = sender.is_some_and(|sender| {
                        surface::deliver(
                            room.room_id(),
                            &sender,
                            owner_signed(encryption.as_ref()),
                            &value["content"],
                        )
                    });
                    if !taken {
                        tracing::debug!(room = %room.room_id(), "agentd: a surface result no call waits for is ignored");
                    }
                    return;
                }
                // A room one of this agent's sessions delegated into: its
                // target's join and reply go to that session, and nothing
                // else of it to anyone (R55).
                let parent = copy
                    .children
                    .lock()
                    .unwrap_or_else(|p| p.into_inner())
                    .get(room.room_id())
                    .cloned();
                if let Some(parent) = parent {
                    if let Some(arrived) =
                        child_arrival(&value, encryption.as_ref(), room.room_id(), received_at)
                    {
                        copy.router.route(&parent, arrived);
                    }
                    return;
                }
                let Some(arrived) = arrival_of(&value, encryption.as_ref(), received_at) else {
                    return;
                };
                // The agent's own events are never acted on (R30's first
                // rule): they are neither routed nor kept.
                if arrived.sender == copy.deps.home.config.matrix_user {
                    return;
                }
                if arrived.arrival == Arrival::Brief && !copy.router.serves(room.room_id()) {
                    hold_brief(&copy, &room, &value, sealed_by_sender(encryption.as_ref())).await;
                }
                copy.router.route(room.room_id(), arrived);
            }
        },
    );
}

/// Who is in `room` or invited to it.
async fn members_of(room: &Room) -> Result<BTreeSet<OwnedUserId>, String> {
    let members = room
        .members(RoomMemberships::JOIN | RoomMemberships::INVITE)
        .await
        .map_err(|error| error.to_string())?;
    Ok(members
        .iter()
        .map(|member| member.user_id().to_owned())
        .collect())
}

/// What a brief's admission reads of `room` now (R93); `None` when its
/// power levels or its members could not be read, and then no brief is
/// taken from it.
pub(crate) async fn brief_room(room: &Room) -> Option<BriefRoom> {
    let levels = room.power_levels().await.ok()?;
    let members = members_of(room).await.ok()?;
    Some(BriefRoom {
        room_type: room.room_type().map(|kind| kind.to_string()),
        creators: room.creators().unwrap_or_default(),
        levels: Some(levels),
        members,
    })
}

/// `event` (decrypted, `sealed` by its sender's device or not) as the
/// opening of a delegation to `me` in a room that is `room` now: what
/// [`rooms::admit_brief`] admits, with the event's id and server time.
pub(crate) fn admitted(
    room: &BriefRoom,
    event: &Value,
    sealed: bool,
    me: &UserId,
    known: &Known,
) -> Result<Opening, &'static str> {
    let sender = event["sender"]
        .as_str()
        .and_then(|sender| UserId::parse(sender).ok())
        .ok_or(rooms::NOT_A_BRIEF)?;
    let brief = rooms::admit_brief(
        room,
        &BriefEvent {
            event_type: event["type"].as_str().unwrap_or_default(),
            sender: &sender,
            content: &event["content"],
            sealed,
        },
        me,
        known,
    )?;
    Ok(Opening {
        event: event["event_id"]
            .as_str()
            .and_then(|id| OwnedEventId::try_from(id).ok())
            .ok_or(rooms::NOT_A_BRIEF)?,
        at: event["origin_server_ts"]
            .as_u64()
            .ok_or(rooms::NOT_A_BRIEF)?,
        brief: Arc::new(brief),
    })
}

/// A brief in a room no worker serves yet, admitted as every brief is
/// (R93), is held for placement (R54); the first held is the room's
/// opening, and a later round never replaces it. A room whose state could
/// not be read is read back on a later tick instead.
async fn hold_brief(copy: &Copy, room: &Room, event: &Value, sealed: bool) {
    let me = &copy.deps.home.config.matrix_user;
    let known = Arc::clone(&copy.known.read().unwrap_or_else(|p| p.into_inner()));
    let pending = || copy.pending.lock().unwrap_or_else(|p| p.into_inner());
    let Some(facts) = brief_room(room).await else {
        tracing::warn!(room = %room.room_id(), "agents: a brief's room could not be read; it is read back later");
        pending().unread(room.room_id());
        return;
    };
    match admitted(&facts, event, sealed, me, &known) {
        Ok(opening) => {
            pending().hold(room.room_id(), opening);
        }
        Err(note) => {
            tracing::info!(room = %room.room_id(), sender = event["sender"].as_str().unwrap_or_default(), note, "agents: a brief this agent may not take is ignored")
        }
    }
}

/// Every delegated room this copy joined that no session among `served`
/// names and no worker serves, not read back yet, is read back for its
/// opening brief (R54): after a restart, and for a room joined since. A
/// room whose read failed stays unread and is read again on a later tick.
pub(crate) async fn recover_briefs(copy: &Copy, served: HashSet<OwnedRoomId>) {
    if *copy.syncs.borrow() == 0 {
        return;
    }
    let me = copy.deps.home.config.matrix_user.clone();
    let known = Arc::clone(&copy.known.read().unwrap_or_else(|p| p.into_inner()));
    let rooms: Vec<Room> = copy
        .client
        .client()
        .joined_rooms()
        .into_iter()
        .filter(|room| {
            room.room_type()
                .is_some_and(|kind| kind.to_string() == SESSION_ROOM_TYPE)
                && !served.contains(room.room_id())
                && !copy.router.serves(room.room_id())
                && room
                    .creators()
                    .is_some_and(|creators| !creators.contains(&me))
        })
        .collect();
    let ids = rooms.iter().map(|room| room.room_id().to_owned()).collect();
    let zone = copy.deps.sessions_zone.clone();
    read_back_rooms(&copy.pending, ids, |id: OwnedRoomId| {
        let room = rooms.iter().find(|room| *room.room_id() == *id).cloned();
        let (me, known, zone) = (me.clone(), Arc::clone(&known), zone.clone());
        async move {
            let Some(room) = room else {
                return Ok(None);
            };
            let facts = brief_room(&room)
                .await
                .ok_or_else(|| "the room's state could not be read".to_owned())?;
            let Some(opening) =
                oldest_opening(|from| page_back(&room, from), &facts, &me, &known).await?
            else {
                return Ok(None);
            };
            let id = opening.brief.id.clone();
            let made = tokio::task::spawn_blocking(move || {
                crate::sessions::verbs::find(&zone, &id).is_some()
            })
            .await
            .map_err(|error| error.to_string())?;
            if made {
                return Ok(None);
            }
            tracing::info!(room = %room.room_id(), delegation = %opening.brief.id, "agents: a brief with no session yet was read back");
            Ok(Some(opening))
        }
    })
    .await;
}

/// One page of a room's timeline read backward: its events, newest first,
/// each with whether its sender's device sealed it, and where the next
/// page starts — `None` at the room's beginning.
pub(crate) struct Page {
    pub(crate) events: Vec<(Value, bool)>,
    pub(crate) end: Option<String>,
}

/// The page of `room` before `from` (the newest when `None`).
async fn page_back(room: &Room, from: Option<String>) -> Result<Page, String> {
    let mut options = MessagesOptions::backward().from(from.as_deref());
    options.limit = UInt::from(BACKLOG_PAGE);
    let page = room
        .messages(options)
        .await
        .map_err(|error| error.to_string())?;
    Ok(Page {
        events: page
            .chunk
            .iter()
            .filter_map(|event| {
                let value = event.raw().deserialize_as::<Value>().ok()?;
                Some((
                    value,
                    sealed_by_sender(event.encryption_info().map(|info| &**info)),
                ))
            })
            .collect(),
        end: page.end.filter(|_| !page.chunk.is_empty()),
    })
}

/// Read a timeline back, newest first, at most [`BACKLOG_PAGES`] pages of
/// `fetch`, handing every event to `visit` until it says stop; whether it
/// did. A page that could not be read is an error, never an empty room.
pub(crate) async fn walk_back<F, Fut>(
    mut fetch: F,
    mut visit: impl FnMut(&Value, bool) -> bool,
) -> Result<bool, String>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = Result<Page, String>>,
{
    let mut from = None;
    for _ in 0..BACKLOG_PAGES {
        let page = fetch(from.take()).await?;
        for (event, sealed) in &page.events {
            if visit(event, *sealed) {
                return Ok(true);
            }
        }
        match page.end {
            Some(end) => from = Some(end),
            None => return Ok(false),
        }
    }
    Ok(false)
}

/// The events its senders' devices sealed after the newest brief `me` sent
/// into a room, oldest first, read back as far as that brief however many
/// pages away it is (R55).
pub(crate) async fn after_brief<F, Fut>(fetch: F, me: &UserId) -> Result<Vec<Value>, String>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = Result<Page, String>>,
{
    let mut after = Vec::new();
    walk_back(fetch, |event, sealed| {
        let brief =
            event["sender"].as_str() == Some(me.as_str()) && event["content"][DELEGATE].is_object();
        if !brief && sealed {
            after.push(event.clone());
        }
        brief
    })
    .await?;
    after.reverse();
    Ok(after)
}

/// The opening of a delegation to `me` in a room that is `room` now: the
/// oldest event of its timeline [`admitted`] admits, so a later round never
/// stands for the brief that opened it (R93).
pub(crate) async fn oldest_opening<F, Fut>(
    fetch: F,
    room: &BriefRoom,
    me: &UserId,
    known: &Known,
) -> Result<Option<Opening>, String>
where
    F: FnMut(Option<String>) -> Fut,
    Fut: Future<Output = Result<Page, String>>,
{
    let mut oldest = None;
    walk_back(fetch, |event, sealed| {
        if let Ok(opening) = admitted(room, event, sealed, me, known) {
            oldest = Some(opening);
        }
        false
    })
    .await?;
    Ok(oldest)
}

/// An event of a room a session delegated into, as that session's arrival:
/// a member's join, or a reply its sender's device sealed. The session
/// checks the sender is its target.
fn child_arrival(
    value: &Value,
    encryption: Option<&EncryptionInfo>,
    room: &RoomId,
    received_at: Instant,
) -> Option<Arrived> {
    let sender = UserId::parse(value["sender"].as_str()?).ok()?;
    if value["type"] == "m.room.member" {
        if value["content"]["membership"] != "join"
            || value["state_key"].as_str() != Some(sender.as_str())
        {
            return None;
        }
        return Some(Arrived {
            event_id: OwnedEventId::try_from(value["event_id"].as_str()?).ok()?,
            sender,
            arrival: Arrival::Joined,
            text: String::new(),
            content: value["content"].clone(),
            received_at,
            replay: false,
            via: Some(room.to_owned()),
            device: None,
        });
    }
    if !sealed_by_sender(encryption) {
        return None;
    }
    reply_of(value, &sender, room, received_at)
}

/// Whether a decrypted event came from a device its sender's cross-signing
/// identity signed — verified by this host or not (R47): what a scope or a
/// request for a conversation needs. A device its owner never signed, one
/// keeper does not know, a session of another user's device, and an identity
/// that changed after it was verified are refused.
fn owner_signed(encryption: Option<&EncryptionInfo>) -> bool {
    encryption.is_some_and(|info| {
        matches!(
            info.verification_state,
            VerificationState::Verified
                | VerificationState::Unverified(VerificationLevel::UnverifiedIdentity)
        )
    })
}

/// A decrypted timeline event as an [`Arrived`], or `None` for an event no
/// session acts on (membership, state, reactions, attachments, notices).
///
/// Text is a person's `m.text` only, and only when the sender's own device
/// sealed it: a message in clear, or one whose sender does not match its
/// Megolm session, is never a turn.
pub fn arrival_of(
    value: &Value,
    encryption: Option<&EncryptionInfo>,
    received_at: Instant,
) -> Option<Arrived> {
    let event_type = value["type"].as_str()?;
    let content = value["content"].clone();
    let arrival = if event_type == "m.room.message" {
        if content["m.relates_to"]["rel_type"] == "m.replace" {
            Arrival::Edit
        } else if content["msgtype"] != "m.text" {
            return None;
        } else if !sealed_by_sender(encryption) {
            tracing::info!(
                sender = value["sender"].as_str().unwrap_or_default(),
                event = value["event_id"].as_str().unwrap_or_default(),
                "agentd: a message its sender's device did not seal is not a turn"
            );
            return None;
        } else if content[DELEGATE].is_object() {
            Arrival::Brief
        } else {
            Arrival::Text
        }
    } else if event_type == APPROVAL_DECISION {
        Arrival::Decision {
            sealed: sealed_by_sender(encryption),
        }
    } else if event_type == SCOPE {
        Arrival::Scope {
            owner_signed: owner_signed(encryption),
        }
    } else if event_type == CONVERSATION_REQUEST {
        Arrival::ConversationRequest {
            owner_signed: owner_signed(encryption),
        }
    } else if event_type.starts_with("dev.keeper.agent.") {
        Arrival::AgentEvent
    } else {
        return None;
    };
    Some(Arrived {
        event_id: OwnedEventId::try_from(value["event_id"].as_str()?).ok()?,
        sender: OwnedUserId::try_from(value["sender"].as_str()?).ok()?,
        arrival,
        text: content["body"].as_str().unwrap_or_default().to_owned(),
        content,
        received_at,
        replay: false,
        via: None,
        device: encryption.and_then(|info| info.sender_device.clone()),
    })
}

/// How often the running host asks for each `[[trust]]` person's master
/// key (R88).
const TRUST_READ: Duration = Duration::from_secs(300);

/// Ask, through the first copy that has synced, for each `[[trust]]`
/// person's published master key, every [`TRUST_READ`], and keep the
/// status file's trust lines. Nothing here writes `agentd.toml`: a pin is a
/// person's to write.
async fn read_trust(
    entries: Vec<TrustEntry>,
    copies: Vec<Arc<Copy>>,
    lines: Arc<Mutex<Vec<Value>>>,
) {
    if entries.is_empty() {
        return;
    }
    loop {
        let synced = copies.iter().find(|copy| *copy.syncs.borrow() > 0);
        let Some(copy) = synced else {
            tokio::time::sleep(TICK).await;
            continue;
        };
        let mut read = Vec::with_capacity(entries.len());
        for entry in &entries {
            let published = copy
                .client
                .published_master_key(&entry.user)
                .await
                .map_err(|error| error.to_string());
            read.push(trust_line(entry, published));
        }
        *lines.lock().unwrap_or_else(|p| p.into_inner()) = read;
        tokio::time::sleep(TRUST_READ).await;
    }
}

/// One `[[trust]]` person's line in the status file: the key published
/// now, the pinned one, and how they stand (R88).
fn trust_line(entry: &TrustEntry, published: Result<Option<String>, String>) -> Value {
    let pinned = entry.master_key.as_deref();
    let (published, state, error) = match &published {
        Ok(key) => (key.as_deref(), PinState::of(key.as_deref(), pinned), None),
        Err(error) => (None, PinState::Unknown, Some(error.as_str())),
    };
    let mut line = json!({
        "user": entry.user,
        "published": published,
        "pinned": pinned,
        "state": state.as_word(),
    });
    if let Some(error) = error {
        line["error"] = json!(error);
    }
    line
}

/// What `status` prints about the running host, written to [`STATUS_FILE`]
/// when it changes and every [`STATUS_HEARTBEAT`] while it does not.
struct StatusFile {
    path: PathBuf,
    /// The last body written, without its time, and when.
    last: Option<(String, Instant)>,
}

impl StatusFile {
    fn new(path: PathBuf) -> StatusFile {
        StatusFile { path, last: None }
    }

    /// Each drive's engine state, each copy, the sessions it serves and the
    /// ones it does not because another session names their room, and each
    /// `[[trust]]` person's pin against what their homeserver publishes
    /// (R88).
    fn publish(
        &mut self,
        config: &AgentdConfig,
        engine: &keeper_sync::engine::Engine,
        copies: &[Arc<Copy>],
        shadowed: &[(String, Value)],
        hosts: &HostRuntime,
        trust: &[Value],
    ) {
        let drives: Vec<Value> = match engine.statuses() {
            Ok(statuses) => statuses
                .into_iter()
                .map(|status| {
                    json!({
                        "drive": status.profile_name,
                        "state": format!("{:?}", status.state),
                        "phase": format!("{:?}", status.phase),
                        "pending": status.pending,
                    })
                })
                .collect(),
            Err(error) => vec![json!({ "error": error.to_string() })],
        };
        let copies: Vec<Value> = copies
            .iter()
            .map(|copy| {
                let user = copy.deps.home.config.matrix_user.to_string();
                let sessions: Vec<Value> = copy
                    .router
                    .served()
                    .into_iter()
                    .map(|(room, session)| json!({ "room": room, "session": session }))
                    .collect();
                let unserved: Vec<&Value> = shadowed
                    .iter()
                    .filter(|(owner, _)| *owner == user)
                    .map(|(_, entry)| entry)
                    .collect();
                json!({
                    "agent": format!("{}/{}", copy.deps.home.config.drive, copy.deps.home.config.id),
                    "user": user,
                    "device": copy.client.device_id(),
                    "sessions": sessions,
                    "unserved": unserved,
                })
            })
            .collect();
        let mut status = json!({
            "host": config.host,
            "drives": drives,
            "copies": copies,
            "claims": hosts.held(),
            "trust": trust,
        });
        let body = status.to_string();
        let fresh = self
            .last
            .as_ref()
            .is_some_and(|(last, at)| *last == body && at.elapsed() < STATUS_HEARTBEAT);
        if fresh {
            return;
        }
        status["updated_at"] = json!(chrono::Utc::now().to_rfc3339());
        let temp = self.path.with_extension("json.tmp");
        let written = std::fs::write(&temp, status.to_string())
            .and_then(|()| std::fs::rename(&temp, &self.path));
        match written {
            Ok(()) => self.last = Some((body, Instant::now())),
            Err(error) => tracing::debug!(%error, "agentd: the status file could not be written"),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;

    use matrix_sdk::deserialized_responses::AlgorithmInfo;

    use super::*;

    #[test]
    fn a_question_another_copy_answered_is_not_asked_again() {
        let me = OwnedUserId::try_from("@nixi:example.org").expect("user");
        let message = |id: &str, sender: &str, content: Value| json!({"event_id": id, "sender": sender, "type": "m.room.message", "content": content});
        let text = |body: &str| json!({"msgtype": "m.text", "body": body});
        let events = [
            message("$q1", "@tgorka:example.org", text("one")),
            message("$q2", "@tgorka:example.org", text("two")),
            // The last holder answered the first, then died.
            message(
                "$a1",
                me.as_str(),
                json!({"body": "…", TURN: {"session": "s", "line": "l"}}),
            ),
            message(
                "$e1",
                me.as_str(),
                json!({"m.relates_to": {"rel_type": "m.replace", "event_id": "$a1"}}),
            ),
            message("$q3", "@tgorka:example.org", text("three")),
            message(
                "$x",
                "@tgorka:example.org",
                json!({"m.relates_to": {"rel_type": "m.replace", "event_id": "$q3"}}),
            ),
        ];
        assert_eq!(answered_by(&events, &me), HashSet::from(["$q1".to_owned()]));
    }

    fn sealed(level: Option<VerificationLevel>) -> EncryptionInfo {
        EncryptionInfo {
            sender: OwnedUserId::try_from("@tgorka:example.org").expect("user"),
            sender_device: None,
            forwarder: None,
            algorithm_info: AlgorithmInfo::MegolmV1AesSha2 {
                curve25519_key: String::new(),
                sender_claimed_keys: BTreeMap::new(),
                session_id: None,
            },
            verification_state: match level {
                Some(level) => VerificationState::Unverified(level),
                None => VerificationState::Verified,
            },
        }
    }

    fn message(content: Value) -> Value {
        json!({
            "type": "m.room.message",
            "event_id": "$m:example.org",
            "sender": "@tgorka:example.org",
            "content": content,
        })
    }

    /// R30's envelope check and the text-only rule: a person's `m.text`
    /// sealed by their device is a turn; the same text in clear or from a
    /// mismatched Megolm sender, and any attachment or notice, is nothing.
    #[test]
    fn only_sealed_text_from_its_sender_is_a_turn() {
        let now = Instant::now();
        let text = message(json!({"msgtype": "m.text", "body": "hi"}));
        let unverified = sealed(Some(VerificationLevel::UnverifiedIdentity));
        let arrived = arrival_of(&text, Some(&unverified), now).expect("a turn");
        assert_eq!(arrived.arrival, Arrival::Text);
        assert_eq!(arrived.text, "hi");

        assert!(arrival_of(&text, None, now).is_none(), "a message in clear");
        let forged = sealed(Some(VerificationLevel::MismatchedSender));
        assert!(
            arrival_of(&text, Some(&forged), now).is_none(),
            "a mismatched sender"
        );

        for msgtype in ["m.image", "m.file", "m.notice", "m.audio"] {
            let other = message(json!({"msgtype": msgtype, "body": "photo.png"}));
            assert!(
                arrival_of(&other, Some(&unverified), now).is_none(),
                "{msgtype} is not a turn"
            );
        }
    }

    /// R47: a scope or a request for a conversation counts from a device its
    /// owner's identity signed, verified here or not; from an unsigned,
    /// unknown or mismatched device, or in clear, the person's own scope in
    /// their own DM is ignored.
    #[test]
    fn a_scope_counts_only_from_a_device_its_owner_signed() {
        use crate::rooms::{classify, Disposition, Served, UNSIGNED_DEVICE};
        use keeper_core::agents::home::AgentKind;
        use keeper_core::agents::session::SessionKind;
        use matrix_sdk::deserialized_responses::DeviceLinkProblem;

        let now = Instant::now();
        let tgorka = OwnedUserId::try_from("@tgorka:example.org").expect("user");
        let nixi = OwnedUserId::try_from("@nixi:example.org").expect("user");
        let readers = Readers::Only([tgorka.clone()].into_iter().collect());
        let dm = Served {
            agent_kind: AgentKind::Proxy,
            human: Some(&tgorka),
            session_kind: SessionKind::Main,
            agent_user: &nixi,
            requester: &tgorka,
            readers: &readers,
        };
        let event = |kind: &str, content: Value| json!({"type": kind, "event_id": "$s:example.org", "sender": tgorka.as_str(), "content": content});
        let scope = event(
            SCOPE,
            json!({"v": 1, "drives": [], "set_by": tgorka.as_str()}),
        );
        let ask = event(CONVERSATION_REQUEST, json!({"v": 1}));
        for (encryption, counts) in [
            (Some(sealed(None)), true),
            (
                Some(sealed(Some(VerificationLevel::UnverifiedIdentity))),
                true,
            ),
            (Some(sealed(Some(VerificationLevel::UnsignedDevice))), false),
            (
                Some(sealed(Some(VerificationLevel::VerificationViolation))),
                false,
            ),
            (
                Some(sealed(Some(VerificationLevel::MismatchedSender))),
                false,
            ),
            (
                Some(sealed(Some(VerificationLevel::None(
                    DeviceLinkProblem::MissingDevice,
                )))),
                false,
            ),
            (None, false),
        ] {
            let level = encryption
                .as_ref()
                .map(|info| info.verification_state.clone());
            let scoped = arrival_of(&scope, encryption.as_ref(), now).expect("a scope");
            assert_eq!(
                scoped.arrival,
                Arrival::Scope {
                    owner_signed: counts
                },
                "{level:?}"
            );
            let asked = arrival_of(&ask, encryption.as_ref(), now).expect("a request");
            let (scope_to, ask_to) = (
                classify(&dm, &tgorka, scoped.arrival),
                classify(&dm, &tgorka, asked.arrival),
            );
            if counts {
                assert_eq!(
                    (scope_to, ask_to),
                    (Disposition::Scope, Disposition::NewConversation)
                );
            } else {
                assert_eq!(scope_to, Disposition::Ignored(UNSIGNED_DEVICE), "{level:?}");
                assert_eq!(ask_to, Disposition::Ignored(UNSIGNED_DEVICE), "{level:?}");
            }
        }
    }

    /// A supervisor that panicked, hung or stopped by itself ends `run` as a
    /// runtime failure, exit 1, which the unit restarts (its
    /// `RestartPreventExitStatus=2 3` keeps only refusals down).
    #[test]
    fn a_failed_supervisor_is_a_runtime_failure_the_unit_restarts() {
        assert!(finalized(Finalize::Done(Ok(())), false).is_ok());
        for (finalize, by_itself) in [
            (Finalize::Done(Ok(())), true),
            (Finalize::Panicked("boom".to_owned()), false),
            (Finalize::TimedOut, false),
        ] {
            let error = finalized(finalize, by_itself).expect_err("a failure");
            assert_eq!(error.exit_code(), 1, "{error}");
        }
    }

    const NIXI: &str = "@nixi:example.org";
    const TOLA: &str = "@tola:example.org";

    fn user(id: &str) -> OwnedUserId {
        OwnedUserId::try_from(id).expect("user")
    }

    /// `timeline` (oldest first) as a homeserver pages it backward, 50 a
    /// page; the page starting `failing` events back fails.
    fn pages(
        timeline: &[(Value, bool)],
        failing: Option<usize>,
    ) -> impl FnMut(Option<String>) -> std::future::Ready<Result<Page, String>> + '_ {
        move |from| {
            let skip: usize = from.as_deref().map_or(0, |n| n.parse().unwrap_or(0));
            if failing == Some(skip) {
                return std::future::ready(Err("messages: 502".to_owned()));
            }
            let newest_first: Vec<(Value, bool)> =
                timeline.iter().rev().skip(skip).take(50).cloned().collect();
            let end = (skip + 50 < timeline.len()).then(|| (skip + 50).to_string());
            std::future::ready(Ok(Page {
                events: newest_first,
                end,
            }))
        }
    }

    fn event(n: usize, sender: &str, content: Value) -> Value {
        json!({
            "type": "m.room.message",
            "event_id": format!("$e{n}:example.org"),
            "sender": sender,
            "origin_server_ts": 1_759_570_000_000u64 + n as u64,
            "content": content,
        })
    }

    /// R55: a reply followed by more than a page of the child's other
    /// events while the parent's host was down is still found — the read
    /// goes back to the parent's newest brief and no further — and a page
    /// that cannot be read is an error, not an empty room.
    #[tokio::test]
    async fn a_reply_more_than_a_page_back_is_found() {
        let label = Label::top();
        let reply = crate::delegate::reply_content("Sorted.", Vec::new(), &label);
        let mut timeline = vec![
            (event(0, TOLA, reply.clone()), true),
            (
                event(
                    1,
                    NIXI,
                    json!({"msgtype": "m.text", "body": "b", DELEGATE: {}}),
                ),
                true,
            ),
            (event(2, TOLA, reply), true),
        ];
        for n in 3..70 {
            let status = json!({"type": "dev.keeper.agent.status", "event_id": format!("$e{n}:example.org"), "sender": TOLA, "content": {}});
            timeline.push((status, true));
        }
        let after = after_brief(pages(&timeline, None), &user(NIXI))
            .await
            .expect("read back");
        assert_eq!(
            after.len(),
            68,
            "everything after the brief, nothing before"
        );
        let room = OwnedRoomId::try_from("!child:example.org").expect("room");
        let replies: Vec<String> = after
            .iter()
            .filter_map(|event| reply_of(event, &user(TOLA), &room, Instant::now()))
            .map(|arrived| arrived.event_id.to_string())
            .collect();
        assert_eq!(replies, ["$e2:example.org"]);
        assert!(after_brief(pages(&timeline, Some(50)), &user(NIXI))
            .await
            .is_err());
    }

    /// R93: the live intake and the read-back after a restart take a brief
    /// by the one admission: an edit carrying a delegation, the delegation
    /// as a custom event, a notice and an unsealed message are refused by
    /// both; and the read-back pins the oldest admitted brief, never a
    /// later round.
    #[tokio::test]
    async fn the_live_intake_and_the_read_back_admit_the_same_briefs() {
        use keeper_core::agents::delegation::{
            brief_content, DelegateContent, DelegateFrom, DelegateLimits,
        };
        use keeper_core::agents::events::CONTENT_VERSION;
        use keeper_core::agents::home::AgentKind;
        use matrix_sdk::ruma::events::room::power_levels::{
            RoomPowerLevels, RoomPowerLevelsEventContent,
        };
        use matrix_sdk::ruma::room_version_rules::AuthorizationRules;

        let readers = Readers::Only(std::collections::BTreeSet::from([user(
            "@tgorka:example.org",
        )]));
        let agent = |id: &str, hosted: bool| KnownAgent {
            id: id.trim_start_matches('@').to_owned(),
            drive: "tgdrive".to_owned(),
            name: id.to_owned(),
            matrix_user: user(id),
            kind: AgentKind::Steward,
            human: None,
            hosted,
            home_readers: readers.clone(),
            opening: Label {
                readers: readers.clone(),
                ..Label::top()
            },
            drives: vec!["tgdrive".to_owned()],
        };
        let known = Known {
            agents: vec![agent(NIXI, false), agent(TOLA, true)],
            trust: Vec::new(),
        };
        let levels: RoomPowerLevelsEventContent =
            serde_json::from_value(keeper_core::agents::events::power_levels(
                SessionKind::Delegated,
                &user(NIXI),
                &[user(TOLA)],
            ))
            .expect("levels");
        let room = BriefRoom {
            room_type: Some(SESSION_ROOM_TYPE.to_owned()),
            creators: vec![user(NIXI)],
            levels: Some(RoomPowerLevels::new(
                levels.into(),
                &AuthorizationRules::V1,
                Vec::<OwnedUserId>::new(),
            )),
            members: std::collections::BTreeSet::from([user(NIXI), user(TOLA)]),
        };
        let delegation = |text: &str| DelegateContent {
            v: CONTENT_VERSION,
            id: "01J9ZZZZZZZZZZZZZZZZZZZZZZ".to_owned(),
            from: DelegateFrom {
                agent: user(NIXI),
                drive: "tgdrive".to_owned(),
                session: "active/2026-10-04-chat".to_owned(),
                room: OwnedRoomId::try_from("!parent:example.org").expect("room"),
            },
            to: user(TOLA),
            brief: text.to_owned(),
            drives: vec!["tgdrive".to_owned()],
            label: Label {
                readers: readers.clone(),
                ..Label::top()
            },
            hop: 1,
            limits: DelegateLimits {
                rounds_per_exchange: 3,
                tokens: 1000,
            },
            card: None,
            dispatch_chain: Vec::new(),
        };
        let opening = brief_content(&delegation("Sort the inbox."));
        let mut edit = opening.clone();
        edit["m.relates_to"] = json!({"rel_type": "m.replace", "event_id": "$e1:example.org"});
        let mut notice = opening.clone();
        notice["msgtype"] = json!("m.notice");
        let mut custom = event(4, NIXI, opening.clone());
        custom["type"] = json!(DELEGATE);
        let forged = [
            (event(2, NIXI, edit), true),
            (custom, true),
            (event(5, NIXI, notice), true),
            (event(6, NIXI, opening.clone()), false),
        ];
        let me = user(TOLA);
        for (forgery, sealed) in &forged {
            assert!(
                admitted(&room, forgery, *sealed, &me, &known).is_err(),
                "{forgery}"
            );
            let read = oldest_opening(
                pages(std::slice::from_ref(&(forgery.clone(), *sealed)), None),
                &room,
                &me,
                &known,
            )
            .await
            .expect("read back");
            assert!(read.is_none(), "{forgery}");
        }

        let genuine = event(1, NIXI, opening);
        let live = admitted(&room, &genuine, true, &me, &known).expect("admitted live");
        let mut timeline = vec![(genuine, true)];
        timeline.extend(forged.iter().cloned());
        timeline.push((
            event(7, NIXI, brief_content(&delegation("And Monday."))),
            true,
        ));
        let read = oldest_opening(pages(&timeline, None), &room, &me, &known)
            .await
            .expect("read back")
            .expect("an opening");
        assert_eq!((read.event, read.at), (live.event, live.at));
        assert_eq!(read.brief.brief, "Sort the inbox.");
    }
}
