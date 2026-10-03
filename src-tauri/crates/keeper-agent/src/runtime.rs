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

use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::future::Future;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use keeper_core::agents::agentd::{AgentdConfig, DrivePin};
use keeper_core::agents::drive::{self, DriveDecl};
use keeper_core::agents::events::APPROVAL_DECISION;
use keeper_core::agents::index::Index;
use keeper_core::agents::label::{Label, Readers};
use keeper_core::agents::log::HostSlug;
use keeper_core::agents::matrix::{self, AgentClient};
use keeper_core::agents::mount;
use keeper_core::agents::session::SessionAgent;
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
use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId, OwnedUserId, RoomId, UInt, UserId};
use matrix_sdk::{Room, RoomState};
use serde_json::{json, Value};
use tokio::sync::{mpsc, watch};
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::agent::{
    bot_for, trail_of, AgentDeps, AgentProfiles, Arrived, ServedSession, SessionRef,
};
use crate::headless::{
    apply_providers, drive_path, open_engine, zone_verdicts, HeadlessError, HeadlessPlatform,
    HeadlessSyncPlatform, SecretMap,
};
use crate::matrix_sink::{EditPort, RoomPort};
use crate::rooms::{self, Arrival, Invite, InviteDecision, Known, KnownAgent};
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
const BACKLOG_PAGES: usize = 20;
const BACKLOG_PAGE: u32 = 50;

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

fn view(id: &str, profile: SyncProfile, hosts: Result<DriveDecl, String>) -> DriveView {
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
    let mut agents = Vec::new();
    for drive in drives {
        for (_, home) in &drive.zone.homes {
            let Ok(home) = home else { continue };
            let readers = Readers::Only(home.config.audience.clone());
            agents.push(KnownAgent {
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
            });
        }
    }
    Known {
        agents,
        trust: config.trust.clone(),
    }
}

/// A copy's client, restored from its stored session; `None` when `login`
/// has not signed it in on this host.
pub async fn restore_copy(
    config: &AgentdConfig,
    platform: &HeadlessPlatform,
    data_dir: &Path,
    user: &OwnedUserId,
) -> Result<Option<AgentClient>, String> {
    let secrets = platform.secrets();
    let get = |key: String| secrets.get(&key).map_err(|error| error.to_string());
    let (Some(session), Some(passphrase)) = (
        get(matrix::session_key(user))?,
        get(matrix::passphrase_key(user))?,
    ) else {
        return Ok(None);
    };
    let client = AgentClient::open(
        &config.homeserver.url.normalized,
        &matrix::store_dir(data_dir, user),
        &passphrase,
    )
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
            ..TurnEnv::new(Arc::clone(platform) as Arc<dyn keeper_core::platform::Platform>)
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
struct Copy {
    deps: Arc<AgentDeps>,
    client: AgentClient,
    /// Read again with every scan, so a home that arrives by sync counts.
    known: Arc<RwLock<Arc<Known>>>,
    router: Arc<Router>,
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
struct RoomSessions<'a> {
    /// The session that serves each room: the first by path.
    served: Vec<(&'a FoundSession, &'a SessionAgent)>,
    /// Every other one, with the session it lost the room to.
    shadowed: Vec<(&'a FoundSession, String)>,
}

/// The sessions of `copy`'s agent among `found`.
fn sessions_of<'a>(copy: &Copy, found: &'a [FoundSession]) -> RoomSessions<'a> {
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
    let mut copies: Vec<Arc<Copy>> = Vec::new();
    let mut syncs: Vec<JoinHandle<()>> = Vec::new();
    let mut workers: Vec<JoinHandle<()>> = Vec::new();
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
        let copy = Arc::new(Copy {
            deps,
            client,
            known: Arc::clone(&known),
            router: Arc::new(Router::default()),
        });
        register_handlers(&copy);
        let sync_client = copy.client.client().clone();
        syncs.push(tokio::spawn(async move {
            if let Err(error) = sync_client.sync(matrix::sync_settings()).await {
                tracing::error!(%error, "agentd: a copy's sync loop ended");
            }
        }));
        copies.push(copy);
    }

    let (engine_stop, engine_shutdown) = watch::channel(false);
    let engine = Arc::clone(&agentd.engine);
    let mut supervisor = tokio::spawn(async move { engine.run(engine_shutdown).await });

    let mut status = StatusFile::new(dirs.state.join(STATUS_FILE));
    let mut drives = Arc::new(drives);
    let mut shadowed: Vec<(String, Value)> = Vec::new();
    let mut reported: HashSet<String> = HashSet::new();
    let mut last_scan: Option<Instant> = None;
    let mut stopped_by_itself = false;
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
                for (session, agent) in served {
                    if let Some(worker) = spawn_worker(copy, session, agent, &stop_signal) {
                        workers.push(worker);
                    }
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
            workers.retain(|worker| !worker.is_finished());
        }
        status.publish(&config, &agentd.engine, &copies, &shadowed);
    }

    tracing::info!("agentd: stopping; running turns get their final edits");
    stop_turns.cancel();
    for copy in &copies {
        copy.router.close();
    }
    let finish = futures_join(workers);
    if tokio::time::timeout(TURNS_FINISH, finish).await.is_err() {
        tracing::warn!("agentd: a turn did not finish within the bound");
    }
    for sync in syncs {
        sync.abort();
    }
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
    finalized(finalize, stopped_by_itself)
}

async fn futures_join(handles: Vec<JoinHandle<()>>) {
    for handle in handles {
        let _ = handle.await;
    }
}

/// Start serving `session` when the copy has joined its room and no worker
/// serves it yet. Joining is the invite rule's alone (F5): any reader of the
/// home drive can write a session file naming any room.
fn spawn_worker(
    copy: &Arc<Copy>,
    session: &FoundSession,
    agent: &SessionAgent,
    stop: &CancelSignal,
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
        move |arrivals| serve_session(worker, session, agent, arrivals, stop),
    )
}

async fn serve_session(
    copy: Arc<Copy>,
    session: FoundSession,
    agent: SessionAgent,
    mut arrivals: mpsc::UnboundedReceiver<Arrived>,
    stop: CancelSignal,
) {
    let room_id = agent.room.clone();
    let Some(room) = copy.client.client().get_room(&room_id) else {
        return;
    };
    let reference = SessionRef {
        drive: copy.deps.home.config.drive.clone(),
        path: session.path.clone(),
    };
    let (deps, dir) = (Arc::clone(&copy.deps), session.dir.clone());
    let opened = tokio::task::spawn_blocking(move || {
        ServedSession::open(&deps, &dir, reference, agent, 0, None)
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
    let deps = &copy.deps;
    let me = &deps.home.config.matrix_user;
    let port: Arc<dyn EditPort> = Arc::new(RoomPort::new(copy.client.clone(), room_id));
    let (events, backlog) = read_back(&room, &mut served, me).await;
    let trail = trail_of(&events, me, served.context.unanswered);
    match served.recover(deps, port.as_ref(), &trail).await {
        Ok(true) => {
            tracing::info!(session = %session.path, "agentd: an interrupted turn was closed, not re-run")
        }
        Ok(false) => {}
        Err(error) => {
            tracing::error!(session = %session.path, %error, "agentd: the interrupted turn could not be closed")
        }
    }
    served
        .serve_arrivals(deps, port, backlog, &mut arrivals, stop)
        .await;
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
        .collect();
    (
        unseen.into_iter().map(|(value, _)| value).collect(),
        arrivals,
    )
}

/// The copy's invite and timeline handlers.
fn register_handlers(copy: &Arc<Copy>) {
    let client = copy.client.client();
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
                let Some(arrived) = arrival_of(&value, encryption.as_ref(), received_at) else {
                    return;
                };
                // The agent's own events are never acted on (R30's first
                // rule): they are neither routed nor kept.
                if arrived.sender == copy.deps.home.config.matrix_user {
                    return;
                }
                copy.router.route(room.room_id(), arrived);
            }
        },
    );
}

/// Whether a decrypted event's sender is who its envelope says: an event
/// that came in clear, or whose Megolm session belongs to another user's
/// device (`MismatchedSender`), may be the server's forgery.
fn sealed_by_sender(encryption: Option<&EncryptionInfo>) -> bool {
    encryption.is_some_and(|info| {
        !matches!(
            info.verification_state,
            VerificationState::Unverified(VerificationLevel::MismatchedSender)
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
        } else {
            Arrival::Text
        }
    } else if event_type == APPROVAL_DECISION {
        Arrival::Decision {
            verified: encryption
                .is_some_and(|info| matches!(info.verification_state, VerificationState::Verified)),
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
    })
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
    /// ones it does not because another session names their room.
    fn publish(
        &mut self,
        config: &AgentdConfig,
        engine: &keeper_sync::engine::Engine,
        copies: &[Arc<Copy>],
        shadowed: &[(String, Value)],
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
}
