//! The desktop host: the app's agents over the app's own drives, pins and
//! keychain (story 90.6; AD-374, AD-378, AD-379, S-15, DW-367).
//!
//! The app is a host like agentd, named by the account's device slug, for
//! the agents of the drives whose `_drive.toml` `principal` is the account's
//! login — only under the owner and readers the person pinned on this Mac.
//! It shares [`HostRuntime`] with agentd: manifests, placement and claims.
//! What differs is where the facts come from:
//!
//! - the drives are the app's flagged sync profiles, synced by the app's own
//!   engine; this host never opens one, and rings and answers doorbells over
//!   the app's once it is handed over ([`DesktopHost::attach_engine`]);
//! - each pin is `keeper.db`'s ([`keeper_core::agents::pins`]), and a zone
//!   whose `_drive.toml` differs from it hosts nothing;
//! - each copy's session and store passphrase are in the app's keychain
//!   under `agents/<user>/…`, signed in from Settings › Agents ([`sign_in`]);
//! - the control room is the principal's room of type
//!   `dev.keeper.agent.control` created by one of the principal's agents, as
//!   a copy finds it, joined or invited;
//! - it has no clock: the app's one interval calls [`DesktopHost::tick`]
//!   (AD-62).

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use keeper_core::agents::agentd::DrivePin;
use keeper_core::agents::copy::{self, AgentCopyVm};
use keeper_core::agents::drive::DriveDecl;
use keeper_core::agents::events::CONTROL_ROOM_TYPE;
use keeper_core::agents::home::AgentKind;
use keeper_core::agents::host::Materialized;
use keeper_core::agents::index::Index;
use keeper_core::agents::log::HostSlug;
use keeper_core::agents::mac_tables::{self, Heard, HeardTool, MacTables};
use keeper_core::agents::matrix::{self, AgentClient, AgentMatrixError};
use keeper_core::agents::mount::pin_matches;
use keeper_core::agents::proxy::ProxyFacts;
use keeper_core::agents::room::ScopeDriveVm;
use keeper_core::agents::run::SandboxTable;
use keeper_core::agents::soul;
use keeper_core::agents::trust::{Anchor, OwnAccount};
use keeper_core::bots::chat::{self, CancelHandle, CancelSignal};
use keeper_core::bots::store::{self, ProviderRow};
use keeper_core::platform::Platform;
use keeper_sync::SyncProfile;
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, UserId};
use matrix_sdk::RoomState;
use tokio::task::JoinHandle;

use crate::agent::{bot_for, AgentDeps};
use crate::deciding::ClientDecisions;
use crate::doorbell::{Doorbell, DriveEngine, Ringer, RING_FINISH};
use crate::hosts::{HostRuntime, RELEASE_BOUND};
use crate::mcp::McpServers;
use crate::rooms::{Known, KnownAgent};
use crate::runtime::{
    deps_over, known_agents, known_with, open_copy, sessions_of, start_copy, view, Copy, DriveView,
    RoomSessions, TURNS_FINISH,
};
use crate::sinks::ClientDoors;
use crate::surface::declared;
use crate::turn::TurnEnv;
use crate::zone::{read_text, read_zone, AgentHome, ZoneRead};

/// Why a pinnable drive hosts nothing before its first sign-in.
pub const UNPINNED: &str =
    "Nothing is pinned for this drive on this Mac yet. Signing an agent in pins its readers.";

/// Why a signed-in copy takes no session yet: the Mac has not found the
/// principal's control room, so it cannot see the other hosts or calibrate
/// its clock, and a claim would be taken blind.
pub const NO_CONTROL_ROOM: &str = "This Mac has not found your agents' control room yet, so it takes no conversation. It is found once an agent's copy has been invited to it.";

/// One host tick at a time: a tick that finds the last one still running
/// skips rather than queues.
#[derive(Debug, Default)]
pub struct TickGate(AtomicBool);

impl TickGate {
    /// The pass a tick runs under, or `None` while another tick holds one.
    pub fn enter(&self) -> Option<TickPass<'_>> {
        (!self.0.swap(true, Ordering::SeqCst)).then_some(TickPass(&self.0))
    }
}

/// Opens its [`TickGate`] again however the tick ends: a tick that panicked
/// would otherwise leave it shut, and the host would renew no manifest and
/// let every claim lapse with nothing in the log.
#[derive(Debug)]
pub struct TickPass<'a>(&'a AtomicBool);

impl Drop for TickPass<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            tracing::error!("agents: a host tick panicked; the next tick runs again");
        }
        self.0.store(false, Ordering::SeqCst);
    }
}

/// What one scan of the app hands the host.
#[derive(Debug, Clone, Default)]
pub struct DesktopFacts {
    /// The account's login: the principal this Mac hosts for.
    pub login: Option<String>,
    /// The account's name for this Mac: the host slug.
    pub device: Option<String>,
    /// Every sync profile the app holds.
    pub profiles: Vec<SyncProfile>,
    /// This Mac's pins, by sync profile id.
    pub pins: BTreeMap<String, DrivePin>,
    /// Each signed-in Matrix account's user id and homeserver URL: a copy
    /// reaches its homeserver through the person's account on its server.
    pub homeservers: Vec<(String, String)>,
    /// Each signed-in Matrix account as this app's device reads it: the
    /// Mac's trust anchor (R87). Only these people decide a run this Mac
    /// hosts, each while verified here; hosting a room trusts nobody.
    pub accounts: Vec<OwnAccount>,
    /// This Mac's own `[[mcp]]` servers and `[sandbox]` table, from
    /// Settings › Agents (96.2 #11, R213): a save is read at the next scan
    /// and the host is built again on it.
    pub tables: MacTables,
}

/// One flagged folder as this host reads it.
#[derive(Debug, Clone)]
pub struct DesktopDrive {
    pub profile_id: String,
    pub view: DriveView,
    pub materialized: Materialized,
    /// The zone as this Mac's pin reads it, whoever the drive's principal:
    /// what a doorbell for a drive mounted here but hosted elsewhere is
    /// heard by (R162). `None` unpinned, or a `_drive.toml` that differs
    /// from the pin.
    pub pinned: Option<ZoneRead>,
}

/// How much of `profile`'s content the app keeps on disk: a sparse cone or
/// a virtualisation policy leaves some of it away.
fn materialized(profile: &SyncProfile) -> Materialized {
    if profile.subpaths.is_empty()
        && profile.virtual_patterns.is_empty()
        && profile.virtual_over_bytes == 0
    {
        Materialized::Full
    } else {
        Materialized::Partial
    }
}

/// Whether a zone declaring `decl` hosts here, and under which
/// declaration: its principal is `login`, it is pinned, and the file matches
/// the pin. The pin's `local_only` binds as well (review R34-03).
fn hosting(
    decl: &Result<DriveDecl, String>,
    pin: Option<&DrivePin>,
    login: &str,
) -> Result<DriveDecl, String> {
    let mut decl = decl.clone()?;
    copy::hosts_principal(&decl, login)?;
    let pin = pin.ok_or_else(|| UNPINNED.to_owned())?;
    pin_matches(&decl, pin).map_err(|difference| difference.sentence())?;
    decl.local_only |= pin.local_only;
    Ok(decl)
}

/// Every folder flagged for agents, read: a zone hosts only when its
/// principal is `login` and its `_drive.toml` matches this Mac's pin.
pub fn desktop_drives(
    profiles: &[SyncProfile],
    pins: &BTreeMap<String, DrivePin>,
    login: &str,
) -> Vec<DesktopDrive> {
    profiles
        .iter()
        .filter(|profile| profile.agents.is_some())
        .map(|profile| {
            let decl = declared(profile);
            let hosts = hosting(&decl, pins.get(&profile.id), login);
            let id = decl
                .as_ref()
                .map_or_else(|_| profile.id.clone(), |decl| decl.id.clone());
            let view = view(&id, profile.clone(), hosts);
            let pinned = match (&view.hosts, &decl, pins.get(&profile.id)) {
                (Ok(_), _, _) => Some(view.zone.clone()),
                (Err(_), Ok(decl), Some(pin)) if pin_matches(decl, pin).is_ok() => {
                    Some(read_zone(&pin.id, profile, Some(decl)))
                }
                _ => None,
            };
            DesktopDrive {
                profile_id: profile.id.clone(),
                view,
                materialized: materialized(profile),
                pinned,
            }
        })
        .collect()
}

/// Every checkout this app syncs, flagged for agents or not, by its
/// folder's name and where it resolves now, sorted: what no sandbox grant
/// may reach (R148, R213, R228). A host's sandbox is built on these; when
/// one moves, comes or goes, the host is built again.
pub fn checkouts(profiles: &[SyncProfile]) -> Vec<(String, PathBuf)> {
    let mut roots: Vec<(String, PathBuf)> = profiles
        .iter()
        .map(|profile| {
            let root = profile
                .local_path
                .canonicalize()
                .unwrap_or_else(|_| profile.local_path.clone());
            (profile.name.clone(), root)
        })
        .collect();
    roots.sort();
    roots.dedup();
    roots
}

/// What the doorbell is set with (R162): each pinned drive by its pin's id
/// — never a `_drive.toml` the pin refused — and its profile, and every
/// agent the pinned zones home, `hosted` among them hosted here.
pub(crate) fn doorbell_roster(
    drives: &[DesktopDrive],
    hosted: &[AgentHome],
) -> (Vec<(String, String)>, Vec<KnownAgent>) {
    let pinned: Vec<(&DesktopDrive, &ZoneRead)> = drives
        .iter()
        .filter_map(|drive| drive.pinned.as_ref().map(|zone| (drive, zone)))
        .collect();
    let mounted = pinned
        .iter()
        .map(|(drive, zone)| (zone.drive.clone(), drive.profile_id.clone()))
        .collect();
    (
        mounted,
        known_agents(pinned.iter().map(|(_, zone)| *zone), hosted),
    )
}

/// The URL of the homeserver `user` is on, from a signed-in account on the
/// same server.
pub fn homeserver_for(user: &UserId, homeservers: &[(String, String)]) -> Option<String> {
    homeservers
        .iter()
        .find(|(account, _)| {
            <&UserId>::try_from(account.as_str())
                .is_ok_and(|account| account.server_name() == user.server_name())
        })
        .map(|(_, url)| url.clone())
}

/// What the running host found about each copy, for the rows.
pub type Problems = HashMap<OwnedUserId, String>;

/// Display names by Matrix user.
pub type Names = BTreeMap<OwnedUserId, String>;

/// The sentence for an agent whose server no signed-in account is on.
pub fn no_homeserver(user: &UserId) -> String {
    format!(
        "No Matrix account on {} is signed in to keeper, so this Mac cannot reach {user}'s homeserver. Add one under Accounts.",
        user.server_name()
    )
}

/// What Settings › Agents lists: one row per agent of a flagged folder of
/// this principal's (every flagged folder's without an account), with its
/// drive's pin and why it is not hosted here, if it is not. A flagged folder
/// whose `_drive.toml` does not read has one row of its own, with no pin
/// and the reason: it hosts nothing, and the person sees why (DW-367).
///
/// Whether each copy is signed in is read from `platform`'s keychain;
/// `names` are people's display names; `problems` what the running host
/// found about a copy.
pub fn listing(
    facts: &DesktopFacts,
    rows: &[ProviderRow],
    platform: &dyn Platform,
    names: &Names,
    problems: &Problems,
) -> Vec<AgentCopyVm> {
    let mut listed = Vec::new();
    for profile in facts.profiles.iter().filter(|p| p.agents.is_some()) {
        let decl = match declared(profile) {
            Ok(decl) => decl,
            Err(sentence) => {
                listed.push(AgentCopyVm {
                    profile_id: profile.id.clone(),
                    drive: profile.name.clone(),
                    agent: String::new(),
                    name: profile.name.clone(),
                    matrix_user: String::new(),
                    device: None,
                    host: facts.device.clone(),
                    signed_in: false,
                    pin: None,
                    problem: Some(sentence),
                });
                continue;
            }
        };
        if facts
            .login
            .as_deref()
            .is_some_and(|login| copy::hosts_principal(&decl, login).is_err())
        {
            continue;
        }
        let pin = facts.pins.get(&profile.id);
        let pin_vm = copy::pin_vm(&decl, pin, &|user: &UserId| names.get(user).cloned());
        let differs = pin.and_then(|pin| pin_matches(&decl, pin).err());
        let zone = read_zone(&decl.id, profile, Some(&decl));
        for (_, home) in &zone.homes {
            let Ok(home) = home else { continue };
            let user = &home.config.matrix_user;
            let device = signed_in_device(platform, user);
            let problem = if facts.login.is_none() {
                Some(copy::NO_ACCOUNT.to_owned())
            } else if facts.device.is_none() {
                Some(copy::NO_DEVICE.to_owned())
            } else if let Some(difference) = &differs {
                Some(difference.sentence())
            } else if bot_for(home, rows).is_none() {
                Some(format!(
                    "No bot provider on this Mac serves {}'s model {} at {}. Add one under Bots.",
                    home.config.name, home.config.bot.target, home.config.bot.base
                ))
            } else if homeserver_for(user, &facts.homeservers).is_none() {
                Some(no_homeserver(user))
            } else {
                problems.get(user).cloned()
            };
            listed.push(AgentCopyVm {
                profile_id: profile.id.clone(),
                drive: decl.id.clone(),
                agent: home.config.id.clone(),
                name: home.config.name.clone(),
                matrix_user: user.to_string(),
                signed_in: device.is_some(),
                device,
                host: facts.device.clone(),
                pin: Some(pin_vm.clone()),
                problem,
            });
        }
    }
    listed
}

/// Every agent of a flagged folder of this principal's, by Matrix user, with
/// its `SOUL.md` `icon` when its soul reads and names one: the agents this
/// Mac knows, whose briefs its rooms draw as briefs (R114), and the mark
/// each room's header draws (91.1 acceptance 6). keeper-core keeps only the
/// marks the bot identity draws.
pub fn zone_agents(facts: &DesktopFacts) -> BTreeMap<OwnedUserId, Option<String>> {
    let mut agents = BTreeMap::new();
    for profile in facts.profiles.iter().filter(|p| p.agents.is_some()) {
        let Ok(decl) = declared(profile) else {
            continue;
        };
        if facts
            .login
            .as_deref()
            .is_some_and(|login| copy::hosts_principal(&decl, login).is_err())
        {
            continue;
        }
        let zone = read_zone(&decl.id, profile, Some(&decl));
        for (_, home) in &zone.homes {
            let Ok(home) = home else { continue };
            let icon = read_text(&home.dir, soul::FILE_NAME)
                .ok()
                .flatten()
                .and_then(|text| soul::parse_soul(&text, &home.config.name).ok())
                .map(|soul| soul.icon)
                .filter(|icon| !icon.trim().is_empty());
            agents.insert(home.config.matrix_user.clone(), icon);
        }
    }
    agents
}

/// Each proxy of this principal's flagged folders, by Matrix user: its
/// person and its `[tools].drives` with their titles where their
/// `_drive.toml` is on this Mac — what the notes view's dock offers its
/// scope chip (91.2).
pub fn agent_proxies(facts: &DesktopFacts) -> BTreeMap<OwnedUserId, ProxyFacts> {
    let flagged: Vec<(&SyncProfile, DriveDecl)> = facts
        .profiles
        .iter()
        .filter(|p| p.agents.is_some())
        .filter_map(|profile| declared(profile).ok().map(|decl| (profile, decl)))
        .filter(|(_, decl)| {
            facts
                .login
                .as_deref()
                .is_none_or(|login| copy::hosts_principal(decl, login).is_ok())
        })
        .collect();
    let title = |id: &str| {
        flagged
            .iter()
            .find(|(_, decl)| decl.id == id)
            .map_or_else(|| id.to_owned(), |(_, decl)| decl.title.clone())
    };
    let mut proxies = BTreeMap::new();
    for (profile, decl) in &flagged {
        let zone = read_zone(&decl.id, profile, Some(decl));
        for (_, home) in &zone.homes {
            let Ok(home) = home else { continue };
            let config = &home.config;
            let (AgentKind::Proxy, Some(human)) = (config.kind, &config.human) else {
                continue;
            };
            let mut drives = vec![config.drive.clone()];
            drives.extend(
                config
                    .drives
                    .iter()
                    .filter(|d| **d != config.drive)
                    .cloned(),
            );
            proxies.insert(
                config.matrix_user.clone(),
                ProxyFacts {
                    human: human.clone(),
                    allowed: drives
                        .iter()
                        .map(|id| ScopeDriveVm {
                            id: id.clone(),
                            title: title(id),
                        })
                        .collect(),
                },
            );
        }
    }
    proxies
}

/// The flagged folder `profile_id` and its `_drive.toml`: what a re-pin
/// pins.
pub fn find_drive(
    profiles: &[SyncProfile],
    profile_id: &str,
) -> Result<(SyncProfile, DriveDecl), String> {
    let profile = profiles
        .iter()
        .find(|p| p.id == profile_id && p.agents.is_some())
        .ok_or_else(|| "That folder no longer keeps agents.".to_owned())?;
    Ok((profile.clone(), declared(profile)?))
}

/// The home `agent` of the flagged folder `profile_id`, read under its own
/// `_drive.toml`: what a sign-in signs in, and the declaration it pins.
pub fn find_home(
    profiles: &[SyncProfile],
    profile_id: &str,
    agent: &str,
) -> Result<(SyncProfile, DriveDecl, AgentHome), String> {
    let (profile, decl) = find_drive(profiles, profile_id)?;
    let zone = read_zone(&decl.id, &profile, Some(&decl));
    let home = zone
        .homes
        .into_iter()
        .find(|(folder, _)| folder == agent)
        .ok_or_else(|| format!("{} has no agent {agent}.", decl.id))?
        .1?;
    Ok((profile, decl, home))
}

/// A failed sign-in as the row says it.
pub fn sign_in_refusal(user: &UserId, error: &AgentMatrixError) -> String {
    match error {
        AgentMatrixError::Forbidden(_) => {
            format!("The homeserver did not accept that password for {user}.")
        }
        AgentMatrixError::Network(_) => {
            format!("{user}'s homeserver could not be reached. Try again when this Mac is online.")
        }
        other => format!("{user} could not be signed in on this Mac: {other}."),
    }
}

/// Sign `user`'s copy in on this Mac with `password`, displayed `display`
/// (`<agent>@<host>`): the session and the store passphrase go to
/// `platform`'s keychain, and a copy signed in before keeps its device id.
/// Answers the device id. No other client of this copy's store may be
/// open: the caller stops the host first.
pub async fn sign_in(
    platform: &dyn Platform,
    homeserver: &str,
    user: &OwnedUserId,
    password: &str,
    display: &str,
) -> Result<String, String> {
    let data_dir = platform.data_dir().map_err(|error| error.to_string())?;
    let get = |key: String| {
        platform
            .keychain_get(&key)
            .map_err(|error| error.to_string())
    };
    let device = get(matrix::session_key(user))?
        .as_deref()
        .and_then(matrix::device_of_session);
    let store = matrix::store_dir(&data_dir, user);
    let passphrase = matrix::sign_in_passphrase(get(matrix::passphrase_key(user))?, &store)
        .map_err(|error| error.to_string())?;
    let client = AgentClient::open(homeserver, &store, &passphrase)
        .await
        .map_err(|error| error.to_string())?;
    let session = client
        .login(user.as_str(), password, device.as_deref(), display)
        .await
        .map_err(|error| sign_in_refusal(user, &error))?;
    let json = session.to_json().map_err(|error| error.to_string())?;
    let set = |key: String, value: &str| {
        platform
            .keychain_set(&key, value)
            .map_err(|error| error.to_string())
    };
    set(matrix::passphrase_key(user), &passphrase)?;
    set(matrix::session_key(user), &json)?;
    Ok(matrix::device_of_session(&json).unwrap_or_default())
}

/// Everyone the rows' pins name, for a display-name lookup.
pub fn people(rows: &[AgentCopyVm]) -> Vec<OwnedUserId> {
    let mut people: Vec<OwnedUserId> = rows
        .iter()
        .filter_map(|row| row.pin.as_ref())
        .flat_map(|pin| {
            std::iter::once(&pin.owner)
                .chain(&pin.readers)
                .chain(&pin.pinned_owner)
                .chain(&pin.pinned_readers)
        })
        .filter_map(|person| OwnedUserId::try_from(person.matrix_id.as_str()).ok())
        .collect();
    people.sort();
    people.dedup();
    people
}

/// The device id of `user`'s copy when it is signed in on this Mac.
pub fn signed_in_device(platform: &dyn Platform, user: &UserId) -> Option<String> {
    platform
        .keychain_get(&matrix::session_key(user))
        .ok()
        .flatten()
        .as_deref()
        .and_then(matrix::device_of_session)
}

/// What a built host was built from; a scan that reads anything else
/// builds it again.
#[derive(Debug, Clone, PartialEq, Eq)]
struct BuildKey {
    host: String,
    principal: String,
    /// `(profile id, drive id)` of every zone that hosts.
    hosting: Vec<(String, String)>,
    /// The agents with a signed-in copy, by `(drive, agent)`.
    signed_in: Vec<(String, String)>,
    /// The trust anchor its decision sources judge under: a person who
    /// verifies this device, or whose identity moves, rebuilds the host, so
    /// no copy decides on what was true before (R87).
    trust: Vec<OwnAccount>,
    /// The Mac's tables, of one revision: a saved server, token or sandbox
    /// table builds it again.
    tables: MacTables,
    /// Every checkout its sandbox may never grant, as they resolved.
    checkouts: Vec<(String, PathBuf)>,
}

/// A running desktop host.
struct Built {
    key: BuildKey,
    drives: Vec<DriveView>,
    copies: Vec<Arc<Copy>>,
    syncs: Vec<JoinHandle<()>>,
    known: Arc<RwLock<Arc<Known>>>,
    runtime: HostRuntime,
    stop: CancelHandle,
    signal: CancelSignal,
    /// This Mac's MCP servers, offered while each answers (96.2 #3).
    mcp: Arc<McpServers>,
    /// What the sandbox's probe found: its status, or `unavailable — why`.
    sandbox: String,
}

/// The app's host: built from a scan's facts, rebuilt when they change,
/// ticked by the app's one interval.
pub struct DesktopHost {
    platform: Arc<dyn Platform>,
    /// The turns' platform and account.
    base: TurnEnv,
    data_dir: PathBuf,
    version: String,
    built: Option<Built>,
    problems: HashMap<OwnedUserId, String>,
    /// The receiver every copy answers doorbells with.
    doorbell: Arc<Doorbell>,
    /// The doorbells' work over the app's engine, once handed over
    /// ([`Self::attach_engine`]).
    engine: Option<Ringer>,
}

impl DesktopHost {
    pub fn new(base: TurnEnv, data_dir: PathBuf, version: &str) -> DesktopHost {
        DesktopHost {
            platform: Arc::clone(&base.platform),
            base,
            data_dir,
            version: version.to_owned(),
            built: None,
            problems: HashMap::new(),
            doorbell: Arc::default(),
            engine: None,
        }
    }

    /// Ring and answer doorbells over the app's sync engine, from now on.
    /// This host never opens an engine of its own; handing the same one
    /// again changes nothing.
    pub fn attach_engine(&mut self, engine: Arc<dyn DriveEngine>) {
        if self
            .engine
            .as_ref()
            .is_some_and(|ringer| Arc::ptr_eq(ringer.engine(), &engine))
        {
            return;
        }
        self.doorbell.set_engine(Arc::clone(&engine));
        self.engine = Some(Ringer::new(engine));
    }

    /// What the running host found about each copy, for the listing.
    pub fn problems(&self) -> &HashMap<OwnedUserId, String> {
        &self.problems
    }

    /// The claims this host holds.
    pub fn held(&self) -> Vec<serde_json::Value> {
        self.built
            .as_ref()
            .map(|built| built.runtime.held())
            .unwrap_or_default()
    }

    /// This Mac's MCP servers while it hosts, for Settings, and the
    /// revision of the tables they were built on.
    pub fn mcp(&self) -> Option<(i64, Arc<McpServers>)> {
        self.built
            .as_ref()
            .map(|built| (built.key.tables.revision, Arc::clone(&built.mcp)))
    }

    /// What this Mac's sandbox probe found while it hosts, for Settings,
    /// and the revision of the table it probed.
    pub fn sandbox_status(&self) -> Option<(i64, String)> {
        self.built
            .as_ref()
            .map(|built| (built.key.tables.revision, built.sandbox.clone()))
    }

    /// One tick of the app's interval. With `facts` (a scan), the host is
    /// built, rebuilt or stopped as they say, and the sessions it can see
    /// are offered to placement; then the runtime ticks. Until a copy has
    /// found the principal's control room the Mac takes no claim: without
    /// the other hosts' manifests and a server clock calibrated by its own
    /// manifest's read-back, a claim would be taken blind.
    pub async fn tick(&mut self, facts: Option<DesktopFacts>) {
        if let Some(facts) = facts {
            self.scan(facts).await;
        }
        let Some(built) = self.built.as_mut() else {
            return;
        };
        // Bells are answered whether or not the control room is found yet.
        if let Some(ringer) = self.engine.as_mut() {
            ringer.tick(&built.runtime.round(&built.drives), &self.doorbell);
        }
        let users = built
            .copies
            .iter()
            .map(|copy| copy.deps.home.config.matrix_user.clone());
        if built.runtime.control_room().is_none() {
            match find_control_room(&built.copies, built.runtime.principal_agents()) {
                Some(room) => {
                    tracing::info!(%room, "agents: this Mac found its principal's control room");
                    built.runtime.set_control_room(room);
                    self.problems
                        .retain(|_, problem| problem.as_str() != NO_CONTROL_ROOM);
                }
                None => {
                    for user in users {
                        self.problems
                            .entry(user)
                            .or_insert_with(|| NO_CONTROL_ROOM.to_owned());
                    }
                    return;
                }
            }
        }
        built.runtime.tick(&built.signal).await;
    }

    async fn scan(&mut self, facts: DesktopFacts) {
        let (Some(login), Some(device)) = (facts.login.clone(), facts.device.clone()) else {
            self.stop().await;
            return;
        };
        let Ok(host) = HostSlug::new(&device) else {
            tracing::warn!(%device, "agents: this Mac's device name is not a host slug; it hosts nothing");
            self.stop().await;
            return;
        };
        // The trust anchor before anything that can fail: a person signed
        // out, no longer verified here, or whose identity moved stops the
        // host now, so no decision is judged on the anchor that was — a
        // zone or provider read that fails below must not keep it (R87).
        if self
            .built
            .as_ref()
            .is_some_and(|built| built.key.trust != facts.accounts)
        {
            tracing::info!("agents: this Mac's verified accounts changed; its host stops before it is built again");
            self.stop().await;
        }
        // The person's servers and sandbox table, likewise: a server
        // removed, a tier raised, a grant taken back stops the host built
        // on what was before a provider read below can fail and return
        // (R228), so the old effect is never kept in use.
        if self
            .built
            .as_ref()
            .is_some_and(|built| built.key.tables != facts.tables)
        {
            tracing::info!("agents: this Mac's MCP servers or sandbox table changed; its host stops before it is built again");
            self.stop().await;
        }
        let platform = Arc::clone(&self.platform);
        let data_dir = self.data_dir.clone();
        let read = tokio::task::spawn_blocking(move || {
            let drives = desktop_drives(&facts.profiles, &facts.pins, &login);
            let checkouts = checkouts(&facts.profiles);
            let rows = store::list_providers(&data_dir).map(|listing| listing.rows);
            let homes: Vec<AgentHome> = drives
                .iter()
                .filter(|drive| drive.view.hosts.is_ok())
                .flat_map(|drive| drive.view.zone.homes.iter())
                .filter_map(|(_, home)| home.as_ref().ok().cloned())
                .collect();
            let signed: Vec<AgentHome> = homes
                .into_iter()
                .filter(|home| {
                    signed_in_device(platform.as_ref(), &home.config.matrix_user).is_some()
                })
                .collect();
            (facts, login, drives, checkouts, rows, signed)
        })
        .await;
        let Ok((facts, login, drives, checkouts, rows, signed)) = read else {
            tracing::error!("agents: the desktop's zones could not be read");
            return;
        };
        // A checkout moved, came or went: the sandbox built on the old set
        // could grant a folder that now holds one (R228).
        if self
            .built
            .as_ref()
            .is_some_and(|built| built.key.checkouts != checkouts)
        {
            tracing::info!("agents: this Mac's checkouts changed; its host stops before its sandbox is built again");
            self.stop().await;
        }
        let rows = match rows {
            Ok(rows) => rows,
            Err(error) => {
                tracing::warn!(%error, "agents: the provider rows could not be read");
                return;
            }
        };
        let mut hosting: Vec<(String, String)> = drives
            .iter()
            .filter(|drive| drive.view.hosts.is_ok())
            .map(|drive| (drive.profile_id.clone(), drive.view.id.clone()))
            .collect();
        hosting.sort();
        let mut signed_keys: Vec<(String, String)> = signed
            .iter()
            .map(|home| (home.config.drive.clone(), home.config.id.clone()))
            .collect();
        signed_keys.sort();
        let key = BuildKey {
            host: host.as_str().to_owned(),
            principal: login.clone(),
            hosting,
            signed_in: signed_keys,
            trust: facts.accounts.clone(),
            tables: facts.tables.clone(),
            checkouts,
        };

        let views: Vec<DriveView> = drives.iter().map(|drive| drive.view.clone()).collect();
        let (mounted, agents) = doorbell_roster(&drives, &signed);
        self.doorbell.set_drives(mounted, agents);
        if self.built.as_ref().is_some_and(|built| built.key == key) {
            if let Some(built) = self.built.as_mut() {
                *built.known.write().unwrap_or_else(|p| p.into_inner()) =
                    Arc::new(known_with(Vec::new(), &views, &signed));
                built.drives = views;
                offer_sessions(built);
            }
            return;
        }
        self.stop().await;
        if signed.is_empty() {
            return;
        }
        self.build(key, host, &login, drives, views, &rows, signed, &facts)
            .await;
    }

    #[allow(clippy::too_many_arguments)]
    async fn build(
        &mut self,
        key: BuildKey,
        host: HostSlug,
        login: &str,
        drives: Vec<DesktopDrive>,
        views: Vec<DriveView>,
        rows: &[ProviderRow],
        signed: Vec<AgentHome>,
        facts: &DesktopFacts,
    ) {
        let zones: Vec<PathBuf> = views
            .iter()
            .filter(|drive| drive.hosts.is_ok())
            .filter_map(|drive| drive.profile.sessions_root())
            .collect();
        let _ = tokio::task::spawn_blocking(move || rebuild_indexes(&zones)).await;

        self.problems.clear();
        let known = Arc::new(RwLock::new(Arc::new(known_with(
            Vec::new(),
            &views,
            &signed,
        ))));
        let mut copies = Vec::new();
        let mut syncs = Vec::new();
        let doors = Arc::new(ClientDoors::default());
        let probed = crate::agent::off_the_runtime(|| {
            desktop_sandbox(&key.checkouts, &self.data_dir, &host, &facts.tables.sandbox)
        });
        let sandbox_line = match &probed {
            Ok(sandbox) => sandbox.status.clone(),
            Err(reason) => format!("unavailable — {reason}"),
        };
        let sandbox = probed.ok().map(Arc::new);
        // The Mac's `[[mcp]]` servers, each with its token from the
        // keychain; connected and listed at the manifest's first renewal and
        // every one after (96.2 #3, #11).
        let mcp = Arc::new(crate::agent::off_the_runtime(|| {
            mac_servers(self.platform.as_ref(), &facts.tables)
        }));
        for home in &signed {
            let user = home.config.matrix_user.clone();
            let Some(homeserver) = homeserver_for(&user, &facts.homeservers) else {
                self.problems.insert(user.clone(), no_homeserver(&user));
                continue;
            };
            let client = match open_copy(&homeserver, self.platform.as_ref(), &self.data_dir, &user)
                .await
            {
                Ok(Some(client)) => client,
                Ok(None) => continue,
                Err(error) => {
                    tracing::error!(%user, %error, "agents: this copy could not be restored");
                    self.problems.insert(
                        user.clone(),
                        format!("This Mac's copy could not be opened: {error}. Sign it in again."),
                    );
                    continue;
                }
            };
            let deps = match deps_over(&self.base, &self.data_dir, &host, &views, rows, home) {
                // Every call that needs a person parks, decided under this
                // Mac's own verified accounts, never a pin (R87, R92).
                Ok(deps) => Arc::new(AgentDeps {
                    decisions: Some(Arc::new(ClientDecisions {
                        client: client.clone(),
                        anchor: Anchor::Desktop(key.trust.clone()),
                    })),
                    sandbox: sandbox.clone(),
                    mcp: Some(Arc::clone(&mcp)),
                    ..deps
                }),
                Err(sentence) => {
                    tracing::error!(agent = %home.config.id, %sentence, "agents: this agent is not served on this Mac");
                    self.problems.insert(user.clone(), sentence);
                    continue;
                }
            };
            let (copy, sync) = start_copy(
                deps,
                client,
                Arc::clone(&known),
                Arc::clone(&self.doorbell),
                Arc::clone(&doors),
            );
            copies.push(copy);
            syncs.push(sync);
        }
        if copies.is_empty() {
            return;
        }
        let manifest: Vec<(DriveView, Materialized)> = drives
            .into_iter()
            .map(|drive| (drive.view, drive.materialized))
            .collect();
        let runtime = HostRuntime::desktop(
            host,
            login,
            &self.version,
            &manifest,
            copies.clone(),
            sandbox.is_some(),
        )
        .with_mcp(Arc::clone(&mcp));
        self.doorbell
            .set_principal_agents(runtime.principal_agents());
        let (stop, signal) = chat::cancellation();
        tracing::info!(host = %key.host, copies = copies.len(), "agents: this Mac hosts its agents");
        let mut built = Built {
            key,
            drives: views,
            copies,
            syncs,
            known,
            runtime,
            stop,
            signal,
            mcp,
            sandbox: sandbox_line,
        };
        offer_sessions(&mut built);
        self.built = Some(built);
    }

    /// Stop hosting: every running turn is stopped and gets its final edit,
    /// the manifest is withdrawn and every claim released (AD-378), each
    /// step bounded. Nothing is hosted until the next scan builds again.
    pub async fn stop(&mut self) {
        self.stop_turns(TURNS_FINISH).await;
        self.release().await;
    }

    /// Quit, first half: every running turn is stopped and gets its final
    /// edit and its lines, within `within`. Hosting stops; the claims stay
    /// held until [`Self::release`], after the app has pushed the drives.
    pub async fn stop_turns(&mut self, within: std::time::Duration) {
        let Some(built) = self.built.as_mut() else {
            return;
        };
        tracing::info!("agents: this Mac stops hosting; running turns get their final edits");
        built.stop.cancel();
        built.runtime.stop_workers(within).await;
    }

    /// Quit, second half: the manifest is withdrawn and every claim written
    /// `released: true`, so another host takes over within two ticks.
    pub async fn release(&mut self) {
        let Some(mut built) = self.built.take() else {
            return;
        };
        built.stop.cancel();
        // What the last pushes published is rung before the copies go quiet.
        if let Some(ringer) = self.engine.as_mut() {
            ringer
                .finish(
                    &built.runtime.round(&built.drives),
                    &self.doorbell,
                    RING_FINISH,
                )
                .await;
        }
        if tokio::time::timeout(RELEASE_BOUND * 2, built.runtime.release_all())
            .await
            .is_err()
        {
            tracing::warn!("agents: the claims' release did not finish; they lapse");
        }
        for sync in built.syncs {
            sync.abort();
        }
    }
}

/// This Mac's sandbox (96.1 #11): `/usr/bin/sandbox-exec` over the Mac's
/// own `[sandbox]` table, checked by the check agentd's goes through
/// (R148, R213), with the developer folder `xcode-select -p` names added
/// read-and-execute (where `/usr/bin/git`'s tools live), probed. No grant
/// reaches any of `checkouts` — every checkout this app syncs, flagged for
/// agents or not (R228). `Err` — a table that does not check, a folder it
/// may not grant, off macOS, or a failed probe — is why `run` is not
/// offered here.
fn desktop_sandbox(
    checkouts: &[(String, PathBuf)],
    data_dir: &Path,
    host: &HostSlug,
    table: &Result<SandboxTable, String>,
) -> Result<crate::run::SandboxHost, String> {
    let mut table = table.clone()?;
    let developer = std::process::Command::new("/usr/bin/xcode-select")
        .arg("-p")
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|text| PathBuf::from(text.trim()))
        .filter(|path| path.is_absolute());
    table.read_exec.extend(developer);
    let forbidden = crate::run::Forbidden {
        drives: checkouts.to_vec(),
        secrets: vec![data_dir.to_path_buf()],
        home: std::env::var_os("HOME").map(PathBuf::from),
    };
    let probed = crate::run::SandboxHost::probe(
        crate::run::Kind::SandboxExec,
        host.as_str(),
        &table,
        &forbidden,
    );
    match &probed {
        Ok(sandbox) => tracing::info!(sandbox = %sandbox.status, "agents: this Mac's sandbox"),
        Err(reason) => {
            tracing::warn!(%reason, "agents: no sandbox on this Mac; agents that need `sandbox` wait for another host")
        }
    }
    probed
}

/// This Mac's `[[mcp]]` servers from its saved tables, each with its token
/// from the keychain (96.2 #3, #11): what the host installs in every copy's
/// `AgentDeps` and its manifest. A server whose token cannot be read is
/// left out, never connected without the token its row names (R228).
pub fn mac_servers(platform: &dyn Platform, tables: &MacTables) -> McpServers {
    McpServers::new(
        tables
            .mcp
            .iter()
            .filter_map(|entry| match mac_tables::bearer(platform, entry) {
                Ok(token) => Some((entry.clone(), token)),
                Err(sentence) => {
                    tracing::warn!(%sentence, "agents: a server of this Mac's is not connected");
                    None
                }
            })
            .collect(),
    )
}

/// What this Mac's servers last answered, for Settings: each that answered
/// with the program it was started as and every tool it listed — its exact
/// name only when it travels as a function name, and whatever else the
/// server wrote of it, name and refusal, as a [`crate::mcp::diagnostic`]:
/// redacted and bounded, as the host's status writes it (R225, R276) —
/// each that did not with why. A server not asked yet is absent.
pub fn heard(servers: &McpServers) -> BTreeMap<String, Heard> {
    servers
        .heard()
        .into_iter()
        .filter_map(|(name, heard)| {
            let heard = match heard? {
                Ok((started, listed)) => Heard::Answers {
                    started,
                    tools: listed
                        .into_iter()
                        .map(|(listed, hints)| HeardTool {
                            tool: match listed.wire {
                                Ok(_) => Ok(listed.tool.clone()),
                                Err(why) => Err(crate::mcp::diagnostic(&why)),
                            },
                            shown: crate::mcp::diagnostic(&listed.tool),
                            hints,
                        })
                        .collect(),
                },
                Err(why) => Heard::Silent(why),
            };
            Some((name, heard))
        })
        .collect()
}

/// Offer every active session of each copy's agent to placement.
fn offer_sessions(built: &mut Built) {
    for copy in &built.copies {
        let Some(home_drive) = built
            .drives
            .iter()
            .find(|d| d.id == copy.deps.home.config.drive)
        else {
            continue;
        };
        let RoomSessions { served, shadowed } = sessions_of(copy, &home_drive.sessions);
        for (session, agent) in served {
            built.runtime.offer(copy, session, agent);
        }
        for (session, winner) in shadowed {
            tracing::debug!(session = %session.path, served = %winner, "agents: two sessions name one room; only the first is served");
        }
    }
    built.runtime.scanned();
}

/// The agents index of each hosting sessions zone, rebuilt from its files:
/// a sync may have brought sessions this Mac has not indexed.
fn rebuild_indexes(zones: &[PathBuf]) {
    for zone in zones {
        match Index::open(zone).and_then(|mut index| index.rebuild()) {
            Ok(report) => {
                for problem in report.problems {
                    tracing::warn!(zone = %zone.display(), %problem, "agents: index rebuild");
                }
            }
            Err(error) => {
                tracing::warn!(zone = %zone.display(), %error, "agents: the agents index could not be rebuilt")
            }
        }
    }
}

/// The principal's control room as the copies see it: a room of type
/// `dev.keeper.agent.control`, joined or invited, created only by the
/// principal's agents. Of several, the lowest room id, so every desktop of
/// the principal picks the same one.
fn find_control_room(copies: &[Arc<Copy>], agents: &[OwnedUserId]) -> Option<OwnedRoomId> {
    let mut rooms: Vec<OwnedRoomId> = copies
        .iter()
        .flat_map(|copy| copy.client.client().rooms())
        .filter(|room| matches!(room.state(), RoomState::Joined | RoomState::Invited))
        .filter(|room| {
            room.room_type()
                .is_some_and(|kind| kind.to_string() == CONTROL_ROOM_TYPE)
        })
        .filter(|room| {
            room.creators().is_some_and(|creators| {
                !creators.is_empty() && creators.iter().all(|user| agents.contains(user))
            })
        })
        .map(|room| room.room_id().to_owned())
        .collect();
    rooms.sort();
    rooms.dedup();
    rooms.into_iter().next()
}

/// The app spawns the host's tick and awaits its stop and sign-in inside
/// its commands, all on a multi-threaded runtime: each future must be
/// `Send`, which only a build checks.
#[cfg(test)]
#[allow(dead_code)]
fn the_app_facing_futures_are_send(host: &mut DesktopHost, platform: &dyn Platform) {
    fn send<T: Send>(_: T) {}
    send(host.tick(None));
    send(host.stop());
    let user = OwnedUserId::try_from("@nixi:example.org").expect("user");
    send(sign_in(platform, "https://example.org", &user, "", ""));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    use keeper_core::agents::copy::{pin_from_shown, AgentPinReq, CHANGED_SINCE_SHOWN};
    use keeper_core::bots::{Provider, ProviderKind};
    use keeper_sync::xdg::SecretStore;

    use crate::agent::AgentProfiles;
    use crate::headless::{HeadlessPlatform, SecretMap, SECRET_ENV_PREFIX};
    use crate::ports::VaultWriter;
    use crate::turn::DrivePorts;

    fn write(root: &Path, rel: &str, text: &str) {
        let path = root.join(rel);
        std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
        std::fs::write(path, text).expect("write");
    }

    const TG: &str = "@tgorka:example.org";
    const MARTA: &str = "@marta:example.org";
    const EVE: &str = "@eve:example.org";
    const NIXI_BOT: &str = "http://127.0.0.1:9";

    fn drive_toml(owner: &str, readers: &[&str], local_only: bool) -> String {
        let readers: Vec<String> = readers.iter().map(|r| format!("\"{r}\"")).collect();
        format!(
            "version = 1\nid = \"tgdrive\"\nprincipal = \"tgorka\"\nowner = \"{owner}\"\nreaders = [{}]\nlocal_only = {local_only}\n",
            readers.join(", ")
        )
    }

    /// A drive of `tgorka`'s with one agent, Nixi, on a local model, so the
    /// drive may be `local_only` or not.
    fn nixi_drive(root: &Path) {
        write(
            root,
            "80-agents/_drive.toml",
            &drive_toml(TG, &[TG, MARTA], false),
        );
        write(
            root,
            "80-agents/nixi/agent.toml",
            &format!(
                "version = 1\nid = \"nixi\"\nname = \"Nixi\"\nkind = \"proxy\"\nmatrix_user = \"@nixi:example.org\"\nhuman = \"{TG}\"\n\n[model]\nbot = \"bot:ollama:{NIXI_BOT}#model\"\n"
            ),
        );
    }

    fn profile(root: &Path) -> SyncProfile {
        let mut profile = SyncProfile::new("p1", "tgdrive", root, "git@forge:tgorka/tgdrive.git");
        profile.agents = Some(Default::default());
        profile.sessions = Some(Default::default());
        profile
    }

    fn platform(root: &Path) -> HeadlessPlatform {
        HeadlessPlatform::new(
            root.join("data"),
            Arc::new(SecretMap::new(SecretStore::new(
                SECRET_ENV_PREFIX,
                root.join("secrets"),
            ))),
        )
    }

    fn shown(readers: &[&str], local_only: bool) -> AgentPinReq {
        AgentPinReq {
            owner: TG.to_owned(),
            readers: readers.iter().map(|r| (*r).to_owned()).collect(),
            local_only,
        }
    }

    fn pin_now(profile: &SyncProfile, shown: &AgentPinReq) -> BTreeMap<String, DrivePin> {
        let decl = declared(profile).expect("decl");
        BTreeMap::from([(
            profile.id.clone(),
            pin_from_shown(&decl, &profile.remote_url, shown).expect("pin"),
        )])
    }

    /// What the one drive hosts: its homes' agent ids, or the sentence.
    fn hosted(
        profile: &SyncProfile,
        pins: &BTreeMap<String, DrivePin>,
    ) -> Result<Vec<String>, String> {
        let drives = desktop_drives(std::slice::from_ref(profile), pins, "tgorka");
        let drive = &drives[0];
        drive.view.hosts.as_ref().map_err(Clone::clone)?;
        Ok(drive
            .view
            .zone
            .homes
            .iter()
            .filter_map(|(_, home)| home.as_ref().ok().map(|h| h.config.id.clone()))
            .collect())
    }

    /// S-15 on the desktop: no pin hosts nothing; the pin hosts; a reader
    /// added to `_drive.toml`, another owner or `local_only` turned off hosts
    /// nothing and is named; re-pinning what the person was shown hosts
    /// again, and a re-pin from a stale view of `local_only` is refused.
    #[test]
    fn the_desktop_hosts_a_drive_only_under_its_pin() {
        let root = tempfile::tempdir().expect("tempdir");
        nixi_drive(root.path());
        let drive = |owner: &str, readers: &[&str], local_only: bool| {
            write(
                root.path(),
                "80-agents/_drive.toml",
                &drive_toml(owner, readers, local_only),
            );
        };
        let profile = profile(root.path());

        assert_eq!(hosted(&profile, &BTreeMap::new()), Err(UNPINNED.to_owned()));

        let pins = pin_now(&profile, &shown(&[TG, MARTA], false));
        assert_eq!(hosted(&profile, &pins), Ok(vec!["nixi".to_owned()]));

        drive(TG, &[TG, MARTA, EVE], false);
        let refused = hosted(&profile, &pins).expect_err("a reader was added");
        assert!(refused.contains(EVE), "{refused}");

        drive(MARTA, &[TG, MARTA], false);
        let refused = hosted(&profile, &pins).expect_err("another owner");
        assert!(
            refused.contains(&format!("the owner {MARTA}; this host pinned {TG}")),
            "{refused}"
        );

        drive(TG, &[TG, MARTA, EVE], false);
        let repinned = pin_now(&profile, &shown(&[TG, MARTA, EVE], false));
        assert_eq!(hosted(&profile, &repinned), Ok(vec!["nixi".to_owned()]));

        // A local-only pin: the file turning it off hosts nothing, and only
        // a re-pin of what the row showed — local-only off — hosts again.
        drive(TG, &[TG, MARTA, EVE], true);
        let local = pin_now(&profile, &shown(&[TG, MARTA, EVE], true));
        assert_eq!(hosted(&profile, &local), Ok(vec!["nixi".to_owned()]));
        drive(TG, &[TG, MARTA, EVE], false);
        let refused = hosted(&profile, &local).expect_err("local_only was turned off");
        assert!(refused.contains("local_only = false"), "{refused}");
        let decl = declared(&profile).expect("decl");
        assert_eq!(
            pin_from_shown(&decl, &profile.remote_url, &shown(&[TG, MARTA, EVE], true)),
            Err(CHANGED_SINCE_SHOWN.to_owned())
        );
        let lowered = pin_now(&profile, &shown(&[TG, MARTA, EVE], false));
        assert_eq!(hosted(&profile, &lowered), Ok(vec!["nixi".to_owned()]));

        // Another principal's drive is never hosted here, pinned or not.
        let others = desktop_drives(std::slice::from_ref(&profile), &lowered, "marta");
        assert!(others[0].view.hosts.is_err());
    }

    /// A flagged folder whose `_drive.toml` does not read is not silently
    /// absent: it has one row, with no pin and the parser's sentence.
    #[test]
    fn a_flagged_folder_whose_drive_file_does_not_read_says_why() {
        let root = tempfile::tempdir().expect("tempdir");
        nixi_drive(root.path());
        write(
            root.path(),
            "80-agents/_drive.toml",
            "version = 1\nid = \"tgdrive\"\nlocal_only = \"yes\"\n",
        );
        let profile = profile(root.path());
        let sentence = declared(&profile).expect_err("does not read");
        let facts = DesktopFacts {
            login: Some("tgorka".to_owned()),
            device: Some("hesperia".to_owned()),
            profiles: vec![profile],
            ..DesktopFacts::default()
        };
        let rows = listing(
            &facts,
            &[],
            &platform(root.path()),
            &Names::new(),
            &Problems::new(),
        );
        assert_eq!(rows.len(), 1, "{rows:?}");
        assert_eq!(rows[0].profile_id, "p1");
        assert_eq!(rows[0].agent, "");
        assert_eq!(rows[0].pin, None);
        assert_eq!(rows[0].problem.as_deref(), Some(sentence.as_str()));
    }

    /// Each agent of the principal's flagged folders, by Matrix user, with
    /// its soul's icon; an agent with no soul, or a soul naming no icon, is
    /// still one this Mac knows, with none.
    #[test]
    fn each_hosted_agent_is_known_with_its_souls_icon() {
        let root = tempfile::tempdir().expect("tempdir");
        nixi_drive(root.path());
        let soul = |icon: &str| {
            format!(
                "---\nname: \"Nixi\"\ntitle: \"Proxy\"\nicon: \"{icon}\"\nrole: \"r\"\nidentity: \"i\"\ncommunication_style: \"c\"\n---\nBody.\n"
            )
        };
        let facts = |login: &str| DesktopFacts {
            login: Some(login.to_owned()),
            profiles: vec![profile(root.path())],
            ..DesktopFacts::default()
        };
        let nixi = OwnedUserId::try_from("@nixi:example.org").expect("user");
        let nixi_with =
            |icon: Option<&str>| BTreeMap::from([(nixi.clone(), icon.map(str::to_owned))]);
        assert_eq!(
            zone_agents(&facts("tgorka")),
            nixi_with(None),
            "no soul yet"
        );
        write(root.path(), "80-agents/nixi/SOUL.md", &soul("N"));
        assert_eq!(zone_agents(&facts("tgorka")), nixi_with(Some("N")));
        // Another principal's folder holds no agent of this Mac's.
        assert!(zone_agents(&facts("marta")).is_empty());
        write(root.path(), "80-agents/nixi/SOUL.md", &soul(""));
        assert_eq!(
            zone_agents(&facts("tgorka")),
            nixi_with(None),
            "no icon named"
        );
    }

    /// A first sign-in that fails after its store was created must not
    /// strand that store: the next attempt opens it again and gets as far as
    /// the homeserver, rather than "could not build the client" for ever.
    #[tokio::test]
    async fn a_failed_first_sign_in_leaves_the_next_one_possible() {
        let root = tempfile::tempdir().expect("tempdir");
        let platform = platform(root.path());
        let user = OwnedUserId::try_from("@nixi:example.org").expect("user");
        // Nothing listens on port 1: the login fails once the store is open.
        let homeserver = "http://127.0.0.1:1";
        let first = sign_in(&platform, homeserver, &user, "pw", "nixi@hesperia")
            .await
            .expect_err("no homeserver");
        assert!(
            matrix::store_dir(&root.path().join("data"), &user).exists(),
            "the first attempt created the store"
        );
        let again = sign_in(&platform, homeserver, &user, "pw", "nixi@hesperia")
            .await
            .expect_err("still no homeserver");
        assert_eq!(again, first);
        assert!(!again.contains("could not build the client"), "{again}");
    }

    /// A tick that panics opens the gate again, so the next tick runs.
    #[test]
    fn a_tick_that_panics_does_not_freeze_the_host() {
        let gate = TickGate::default();
        let pass = gate.enter().expect("open");
        assert!(gate.enter().is_none(), "one tick at a time");
        drop(pass);
        let panicked = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _pass = gate.enter().expect("open again");
            panic!("a tick panics");
        }));
        assert!(panicked.is_err());
        assert!(gate.enter().is_some(), "the gate is open after the panic");
    }

    /// A host built under `trust` and `tables` over no checkout, hosting
    /// nothing: what a scan finds running.
    fn built_under(trust: Vec<OwnAccount>, tables: MacTables) -> Built {
        let host = HostSlug::new("hesperia").expect("slug");
        let (stop, signal) = chat::cancellation();
        Built {
            key: BuildKey {
                host: host.as_str().to_owned(),
                principal: "tgorka".to_owned(),
                hosting: Vec::new(),
                signed_in: Vec::new(),
                trust,
                tables,
                checkouts: Vec::new(),
            },
            drives: Vec::new(),
            copies: Vec::new(),
            syncs: Vec::new(),
            known: Arc::default(),
            runtime: HostRuntime::desktop(host, "tgorka", "test", &[], Vec::new(), false),
            stop,
            signal,
            mcp: Arc::new(McpServers::new(Vec::new())),
            sandbox: String::new(),
        }
    }

    /// Tables of `revision` whose sandbox grants `read_exec`.
    fn saved(revision: i64, read_exec: &[&str]) -> MacTables {
        MacTables {
            sandbox: Ok(SandboxTable {
                read_exec: read_exec.iter().map(PathBuf::from).collect(),
                env: Vec::new(),
            }),
            revision,
            ..MacTables::default()
        }
    }

    /// 96.2 #11, R213: a scan that reads tables of another revision — a
    /// token rotated with nothing else changed included — stops the host
    /// built on the old ones, so no copy keeps serving the servers or the
    /// sandbox that were; a scan of the same tables keeps it. That the host
    /// built again uses what was saved is
    /// `a_saved_server_is_used_with_its_saved_token`'s.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_saved_table_stops_the_host_built_on_the_old_one() {
        for (tables, kept, what) in [
            (saved(1, &["/opt/bin"]), true, "the same tables"),
            (
                saved(2, &["/opt/bin"]),
                false,
                "a save of the same rows, a token replaced",
            ),
            (saved(1, &["/opt/other"]), false, "another sandbox table"),
        ] {
            let root = tempfile::tempdir().expect("tempdir");
            let data_dir = root.path().join("data");
            let mut host = DesktopHost::new(
                TurnEnv::new(Arc::new(platform(root.path()))),
                data_dir,
                "test",
            );
            host.built = Some(built_under(Vec::new(), saved(1, &["/opt/bin"])));
            host.scan(DesktopFacts {
                login: Some("tgorka".to_owned()),
                device: Some("hesperia".to_owned()),
                tables,
                ..DesktopFacts::default()
            })
            .await;
            assert_eq!(host.built.is_some(), kept, "{what}");
        }
    }

    /// R96MM-06: a server removed, a tier raised or a grant taken back
    /// stops the host built on the old tables even when the provider rows
    /// cannot be read in the same scan, and so does a checkout that moved;
    /// the same failing read with nothing changed keeps the host.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_revoked_policy_stops_the_host_however_the_scan_fails() {
        let root = tempfile::tempdir().expect("tempdir");
        let moved = root.path().join("moved");
        std::fs::create_dir_all(&moved).expect("moved");
        let notes = SyncProfile::new("p2", "notes", &moved, "git@forge:tgorka/notes.git");
        for (tables, profiles, kept, what) in [
            (saved(1, &["/opt/bin"]), Vec::new(), true, "unchanged"),
            (
                saved(2, &["/opt/bin"]),
                Vec::new(),
                false,
                "a server removed",
            ),
            (saved(1, &[]), Vec::new(), false, "a grant taken back"),
            (
                saved(1, &["/opt/bin"]),
                vec![notes.clone()],
                false,
                "a checkout that came",
            ),
        ] {
            // `keeper.db` cannot be opened under a file: the provider rows
            // do not read, and the scan returns there.
            let data_dir = root.path().join("not-a-folder");
            std::fs::write(&data_dir, "").expect("a file");
            let mut host = DesktopHost::new(
                TurnEnv::new(Arc::new(platform(root.path()))),
                data_dir.clone(),
                "test",
            );
            assert!(store::list_providers(&data_dir).is_err(), "{what}");
            host.built = Some(built_under(Vec::new(), saved(1, &["/opt/bin"])));
            host.scan(DesktopFacts {
                login: Some("tgorka".to_owned()),
                device: Some("hesperia".to_owned()),
                profiles,
                tables,
                ..DesktopFacts::default()
            })
            .await;
            assert_eq!(host.built.is_some(), kept, "{what}");
            assert_eq!(host.mcp().is_some(), kept, "{what}");
        }
    }

    /// R96MM-02: no sandbox grant reaches a checkout this app syncs,
    /// flagged for agents or not. A toolchain folder holding an unflagged
    /// checkout is refused; a flagged checkout that moved beneath an
    /// already granted folder is refused once the host is built on where it
    /// is now, and the move itself builds the host again; a toolchain
    /// folder holding no checkout passes the grant check.
    #[tokio::test(flavor = "multi_thread")]
    async fn every_checkout_is_never_granted() {
        let root = tempfile::tempdir().expect("tempdir");
        let path = |rel: &str| {
            let path = root.path().join(rel);
            std::fs::create_dir_all(&path).expect("mkdir");
            path
        };
        let (tools, toolchain) = (path("tools"), path("toolchain/bin"));
        let data = path("data");
        let host = HostSlug::new("hesperia").expect("slug");
        let table = |dir: &Path| {
            Ok(SandboxTable {
                read_exec: vec![dir.to_path_buf()],
                env: Vec::new(),
            })
        };
        let granted = |checkouts: &[(String, PathBuf)], dir: &Path| {
            desktop_sandbox(checkouts, &data, &host, &table(dir))
                .err()
                .filter(|why| why.contains("cannot grant it"))
        };

        // An unflagged checkout under a toolchain folder.
        let unflagged = SyncProfile::new(
            "p2",
            "notes",
            path("tools/notes"),
            "git@forge:tgorka/notes.git",
        );
        assert!(unflagged.agents.is_none());
        let refused = granted(&checkouts(std::slice::from_ref(&unflagged)), &tools)
            .expect("a folder holding a checkout is refused");
        assert!(refused.contains("notes"), "{refused}");
        assert_eq!(
            granted(&checkouts(std::slice::from_ref(&unflagged)), &toolchain),
            None,
            "a toolchain folder holding no checkout"
        );

        // A flagged checkout moves beneath the granted folder.
        let mut tgdrive = profile(&path("drives/tgdrive"));
        let before = checkouts(std::slice::from_ref(&tgdrive));
        assert_eq!(granted(&before, &tools), None, "before the move");
        tgdrive.local_path = path("tools/tgdrive");
        let after = checkouts(std::slice::from_ref(&tgdrive));
        assert!(granted(&after, &tools).is_some(), "after the move");

        let mut built = built_under(Vec::new(), saved(1, &[]));
        built.key.checkouts = before;
        let mut desktop = DesktopHost::new(
            TurnEnv::new(Arc::new(platform(root.path()))),
            data.clone(),
            "test",
        );
        desktop.built = Some(built);
        desktop
            .scan(DesktopFacts {
                login: Some("tgorka".to_owned()),
                device: Some("hesperia".to_owned()),
                profiles: vec![tgdrive],
                tables: saved(1, &[]),
                ..DesktopFacts::default()
            })
            .await;
        assert!(desktop.built.is_none(), "the move builds the host again");
    }

    /// What a fixture HTTP endpoint was asked: each request's path and its
    /// `Authorization` header.
    type Asked = Arc<std::sync::Mutex<Vec<(String, Option<String>)>>>;

    /// An endpoint that records every request and answers 401: its base
    /// URL and what it was asked.
    async fn recording_endpoint() -> (String, Asked) {
        use axum::http::{header::AUTHORIZATION, HeaderMap, StatusCode, Uri};
        let asked = Asked::default();
        let log = Arc::clone(&asked);
        let router = axum::Router::new().fallback(move |uri: Uri, headers: HeaderMap| {
            let log = Arc::clone(&log);
            async move {
                let bearer = headers
                    .get(AUTHORIZATION)
                    .and_then(|value| value.to_str().ok())
                    .map(str::to_owned);
                log.lock()
                    .expect("log")
                    .push((uri.path().to_owned(), bearer));
                StatusCode::UNAUTHORIZED
            }
        });
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind");
        let address = listener.local_addr().expect("address");
        tokio::spawn(async move {
            let _ = axum::serve(listener, router).await;
        });
        (format!("http://{address}"), asked)
    }

    /// Build this Mac's servers from `tables` as the host does, ask each
    /// once, and say where each request went and with which bearer.
    async fn used(
        platform: &dyn Platform,
        tables: &MacTables,
        asked: &Asked,
    ) -> Vec<(String, Option<String>)> {
        asked.lock().expect("asked").clear();
        mac_servers(platform, tables).refresh().await;
        let mut seen = asked.lock().expect("asked").clone();
        seen.sort();
        seen.dedup();
        seen
    }

    /// R96MM-12, R96MM2-01: from the store to the server. A server saved in
    /// Settings is asked at the URL saved, with the token saved; a rotated
    /// token makes a new revision, which stops the host built on the old
    /// one, and the servers built again send the new token; a moved URL is
    /// asked where it moved; a removed server is asked nothing. Servers
    /// built from a read taken before a save never send that save's token:
    /// not after a rotation, and not to the old URL after a move with a new
    /// token — they send nothing. The full host — copies signed in to a
    /// homeserver — is the Mac device run's.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_saved_server_is_used_with_its_saved_token() {
        use keeper_core::agents::mac_tables::AgentMcpServerReq;
        let root = tempfile::tempdir().expect("tempdir");
        let platform = platform(root.path());
        let data = root.path().join("data");
        let (base, asked) = recording_endpoint().await;
        let save = |path: &str, token: Option<&str>| {
            mac_tables::save_server(
                &data,
                &platform,
                AgentMcpServerReq {
                    name: "notes".to_owned(),
                    url: Some(format!("{base}{path}")),
                    command: Vec::new(),
                    role: None,
                    fingerprint: None,
                    readers: Vec::new(),
                    trust_annotations: false,
                    rows: Vec::new(),
                    token: token.map(str::to_owned),
                    forget_token: false,
                },
            )
            .expect("saved");
        };
        let sent =
            |path: &str, token: &str| vec![(path.to_owned(), Some(format!("Bearer {token}")))];

        save("/one", Some("tok-1"));
        let first = mac_tables::read(&data);
        assert_eq!(used(&platform, &first, &asked).await, sent("/one", "tok-1"));

        save("/one", Some("tok-2"));
        let rotated = mac_tables::read(&data);
        assert!(
            used(&platform, &first, &asked).await.is_empty(),
            "the read before the rotation never sends its token"
        );
        assert_ne!(rotated.mcp, first.mcp, "a new token is a new entry");
        let mut host = DesktopHost::new(
            TurnEnv::new(Arc::new(self::platform(root.path()))),
            data.clone(),
            "test",
        );
        host.built = Some(built_under(Vec::new(), first.clone()));
        host.scan(DesktopFacts {
            login: Some("tgorka".to_owned()),
            device: Some("hesperia".to_owned()),
            tables: rotated.clone(),
            ..DesktopFacts::default()
        })
        .await;
        assert!(host.built.is_none(), "the rotation stops the old host");
        assert_eq!(
            used(&platform, &rotated, &asked).await,
            sent("/one", "tok-2")
        );

        save("/two", None);
        let moved = mac_tables::read(&data);
        assert_eq!(used(&platform, &moved, &asked).await, sent("/two", "tok-2"));

        save("/three", Some("tok-3"));
        let again = mac_tables::read(&data);
        assert_eq!(
            used(&platform, &again, &asked).await,
            sent("/three", "tok-3")
        );
        assert!(
            used(&platform, &moved, &asked).await.is_empty(),
            "the old URL never gets the new token"
        );

        mac_tables::remove_server(&data, &platform, "notes").expect("removed");
        let removed = mac_tables::read(&data);
        assert!(used(&platform, &removed, &asked).await.is_empty());
    }

    /// The argument that makes this test binary [`mcp_child_server`].
    const MCP_CHILD: &str = "keeper-desktop-mcp-child";
    /// `<this><file>`: the child appends a line to `file` for every call.
    const MCP_CHILD_CALLS: &str = "keeper-desktop-mcp-calls=";
    /// The child also lists a tool named with a secret, one named a secret
    /// alone — a name that travels — and one too long.
    const MCP_CHILD_ODD: &str = "keeper-desktop-mcp-odd";
    /// The secret [`MCP_CHILD_ODD`]'s tools are named with.
    const ODD_SECRET: &str = "ghp_AbCdEfGhIjKlMnOpQrStUvWxYz0123456789";

    /// This test binary as an MCP server over its stdio, when it was
    /// started with [`MCP_CHILD`]: one tool, `echo`, and with
    /// [`MCP_CHILD_ODD`] three more — [`ODD_SECRET`], and two whose names
    /// cannot travel; with [`MCP_CHILD_CALLS`] each call recorded.
    /// Otherwise nothing.
    #[test]
    fn mcp_child_server() {
        use rmcp::model::{
            CallToolRequestParams, CallToolResult, ContentBlock, ListToolsResult,
            PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
        };
        use rmcp::service::RequestContext;
        use rmcp::ServiceExt;
        #[derive(Clone)]
        struct Child {
            calls: Option<PathBuf>,
            odd: bool,
        }
        impl rmcp::ServerHandler for Child {
            fn get_info(&self) -> ServerConfig {
                ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            }
            async fn list_tools(
                &self,
                _request: Option<PaginatedRequestParams>,
                _context: RequestContext<rmcp::RoleServer>,
            ) -> Result<ListToolsResult, rmcp::ErrorData> {
                let mut names = vec!["echo".to_owned()];
                if self.odd {
                    names.push(format!("read {ODD_SECRET}"));
                    names.push(ODD_SECRET.to_owned());
                    names.push("x".repeat(4096));
                }
                Ok(ListToolsResult::with_all_items(
                    names
                        .into_iter()
                        .map(|name| Tool::new(name, "Echoes.", Arc::new(serde_json::Map::new())))
                        .collect(),
                ))
            }

            #[allow(deprecated)]
            async fn call_tool(
                &self,
                _request: CallToolRequestParams,
                _context: RequestContext<rmcp::RoleServer>,
            ) -> Result<rmcp::model::CallToolResponse, rmcp::ErrorData> {
                if let Some(calls) = &self.calls {
                    use std::io::Write;
                    let mut file = std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(calls)
                        .expect("calls");
                    writeln!(file, "call").expect("recorded");
                }
                Ok(CallToolResult::success(vec![ContentBlock::text("echoed")]).into())
            }
        }
        if !std::env::args().any(|arg| arg == MCP_CHILD) {
            return;
        }
        let child = Child {
            calls: std::env::args()
                .find_map(|arg| arg.strip_prefix(MCP_CHILD_CALLS).map(PathBuf::from)),
            odd: std::env::args().any(|arg| arg == MCP_CHILD_ODD),
        };
        let runtime = tokio::runtime::Runtime::new().expect("runtime");
        runtime.block_on(async {
            let served = child.serve(rmcp::transport::stdio()).await.expect("served");
            let _ = served.waiting().await;
        });
        std::process::exit(0);
    }

    /// Save `kid`, this test binary at `program` as [`mcp_child_server`]
    /// with `extra` arguments, in Settings: the argv saved.
    fn save_kid(
        data: &Path,
        platform: &dyn Platform,
        program: &Path,
        extra: &[String],
    ) -> Vec<String> {
        use keeper_core::agents::mac_tables::AgentMcpServerReq;
        let mut argv = vec![
            program.to_string_lossy().into_owned(),
            "desktop::tests::mcp_child_server".to_owned(),
            "--exact".to_owned(),
            "--nocapture".to_owned(),
            "--quiet".to_owned(),
            "--test-threads".to_owned(),
            "1".to_owned(),
            MCP_CHILD.to_owned(),
        ];
        argv.extend_from_slice(extra);
        mac_tables::save_server(
            data,
            platform,
            AgentMcpServerReq {
                name: "kid".to_owned(),
                url: None,
                command: argv.clone(),
                role: None,
                fingerprint: None,
                readers: Vec::new(),
                trust_annotations: false,
                rows: Vec::new(),
                token: None,
                forget_token: false,
            },
        )
        .expect("saved");
        argv
    }

    /// Q10, R225: a program saved on this Mac is known the way agentd's
    /// are — its argv's program resolved to an absolute path and hashed as
    /// it is started — and Settings lists it as that very program, the one
    /// an approval of its tools binds: saved through a link, it is listed
    /// by the file the link resolves to and that file's SHA-256.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_saved_program_is_listed_as_it_was_started() {
        use keeper_core::agents::mac_tables::{AgentMcpStartedVm, Hosted};
        let root = tempfile::tempdir().expect("tempdir");
        let platform = platform(root.path());
        let data = root.path().join("data");
        let real = root.path().join("bin").join("kid");
        std::fs::create_dir_all(real.parent().expect("bin")).expect("bin");
        std::fs::copy(std::env::current_exe().expect("this test binary"), &real).expect("copy");
        let real = std::fs::canonicalize(real).expect("real");
        let link = root.path().join("kid-link");
        std::os::unix::fs::symlink(&real, &link).expect("link");
        let sha256 =
            keeper_core::agents::approval::sha256_hex(&std::fs::read(&real).expect("bytes"));
        let argv = save_kid(&data, &platform, &link, &[]);
        let tables = mac_tables::read(&data);
        let servers = mac_servers(&platform, &tables);
        servers.refresh().await;

        let binding = servers
            .listed(&["kid".to_owned()])
            .into_iter()
            .find(|listed| listed.tool == "echo")
            .and_then(|listed| listed.binding())
            .expect("offered");
        assert_eq!(binding["program"], real.to_string_lossy().as_ref());
        assert_eq!(binding["program_sha256"], sha256.as_str());
        let heard = heard(&servers);
        let listed = mac_tables::listing(
            &data,
            &platform,
            Some(Hosted {
                revision: tables.revision,
                heard: &heard,
            }),
            &|_| None,
        )
        .expect("listing");
        let kid = &listed.servers[0];
        assert!(kid.answers, "{kid:?}");
        assert_eq!(kid.command, argv, "the argv as saved");
        assert_eq!(
            kid.started,
            Some(AgentMcpStartedVm {
                path: real.to_string_lossy().into_owned(),
                sha256,
            }),
            "listed as the program the approval binds"
        );
    }

    /// R258: the Mac's calls go through rung 2's admission like agentd's.
    /// A save builds the host's servers again; the tool the servers built
    /// before it listed — the same program, tool, definition and tier — is
    /// refused by the servers built after: no child is called at all. Their
    /// own listing of it is sent, and called exactly once.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_tool_listed_before_a_rebuild_is_never_sent_after_it() {
        let root = tempfile::tempdir().expect("tempdir");
        let platform = platform(root.path());
        let data = root.path().join("data");
        let calls = root.path().join("calls");
        save_kid(
            &data,
            &platform,
            &std::env::current_exe().expect("this test binary"),
            &[format!("{MCP_CHILD_CALLS}{}", calls.display())],
        );
        let called = || {
            std::fs::read_to_string(&calls)
                .unwrap_or_default()
                .lines()
                .count()
        };
        let tables = mac_tables::read(&data);
        let echo = |servers: &McpServers| {
            servers
                .listed(&["kid".to_owned()])
                .into_iter()
                .find(|listed| listed.tool == "echo")
                .expect("offered")
        };
        let before = mac_servers(&platform, &tables);
        before.refresh().await;
        let after = mac_servers(&platform, &tables);
        after.refresh().await;
        let (stale, own) = (echo(&before), echo(&after));
        assert_eq!(
            stale.binding(),
            own.binding(),
            "only the connection differs"
        );

        let (_keep, signal) = keeper_core::bots::chat::cancellation();
        after
            .call(&stale, serde_json::Map::new(), signal)
            .await
            .expect_err("listed over another connection");
        assert_eq!(called(), 0, "the stale listing reached no child");
        let (_keep, signal) = keeper_core::bots::chat::cancellation();
        after
            .call(&own, serde_json::Map::new(), signal)
            .await
            .expect("its own connection");
        assert_eq!(called(), 1, "its own listing is called once");
    }

    /// R225, R96MM2-03, R96MM3-01: what a server writes of its tools
    /// reaches Settings as the host's status writes it. A live listing with
    /// a tool named with a secret, one named that secret alone — a name
    /// that travels — and one of 4 KiB. In the sheet of the entry with a
    /// row on the secret-named tool, of it made a role server, and of it
    /// read with no host built on its tables, neither the secret nor the
    /// long name is in a name, refusal or conflict Settings shows, nor in
    /// the serialized listing; the secret-named tool's exact name still
    /// picks its row, and the untravelled ones carry none; `echo` reads as
    /// listed.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_listed_tool_reaches_settings_redacted_and_bounded() {
        use keeper_core::agents::mac_tables::{AgentMcpRowVm, AgentMcpServerReq, Hosted};
        let root = tempfile::tempdir().expect("tempdir");
        let platform = platform(root.path());
        let data = root.path().join("data");
        let argv = save_kid(
            &data,
            &platform,
            &std::env::current_exe().expect("this test binary"),
            &[MCP_CHILD_ODD.to_owned()],
        );
        let tables = mac_tables::read(&data);
        let servers = mac_servers(&platform, &tables);
        servers.refresh().await;
        let heard = heard(&servers);
        let hosted = Hosted {
            revision: tables.revision,
            heard: &heard,
        };
        let listed =
            mac_tables::listing(&data, &platform, Some(hosted), &|_| None).expect("listing");
        assert!(listed.servers[0].answers, "{:?}", listed.servers[0]);
        let sheet = |hosted: Option<Hosted<'_>>, role: Option<&str>| {
            let req = AgentMcpServerReq {
                name: "kid".to_owned(),
                url: None,
                command: argv.clone(),
                role: role.map(str::to_owned),
                fingerprint: None,
                readers: Vec::new(),
                trust_annotations: false,
                rows: vec![AgentMcpRowVm {
                    tool: ODD_SECRET.to_owned(),
                    tier: "T1".to_owned(),
                }],
                token: None,
                forget_token: false,
            };
            mac_tables::draft(&data, hosted, req, &crate::mcp::diagnostic).expect("draft")
        };

        let ordinary = sheet(Some(hosted), None);
        assert_eq!(ordinary.tools.len(), 4, "{ordinary:?}");
        assert_eq!(ordinary.tools[0].tool.as_deref(), Some("echo"));
        assert_eq!(ordinary.tools[0].shown, "echo");
        assert!(ordinary.tools[0].word.is_some());
        assert_eq!(
            ordinary.tools[2].tool.as_deref(),
            Some(ODD_SECRET),
            "its row's"
        );
        assert!(ordinary.tools[2].word.is_some(), "{ordinary:?}");
        for refused in [&ordinary.tools[1], &ordinary.tools[3]] {
            assert_eq!(refused.tool, None);
            assert!(refused.refusal.is_some());
        }
        let as_role = sheet(Some(hosted), Some("paseo"));
        assert_eq!(as_role.tools[2].tool.as_deref(), Some(ODD_SECRET));
        assert!(as_role.tools[2].refusal.is_some(), "{as_role:?}");
        assert_eq!(as_role.conflicts.len(), 1, "{as_role:?}");
        assert_eq!(as_role.conflicts[0].tool, ODD_SECRET, "what Drop matches");
        let unhosted = sheet(None, None);
        assert_eq!(unhosted.tools.len(), 1, "{unhosted:?}");
        assert_eq!(unhosted.tools[0].tool.as_deref(), Some(ODD_SECRET));

        let long = "x".repeat(600);
        for sheet in [&ordinary, &as_role, &unhosted] {
            let shown = sheet
                .tools
                .iter()
                .flat_map(|tool| [Some(tool.shown.as_str()), tool.refusal.as_deref()])
                .flatten()
                .chain(sheet.conflicts.iter().map(|row| row.shown.as_str()));
            for text in shown {
                assert!(!text.contains(ODD_SECRET), "{text}");
                assert!(!text.contains(&long), "unbounded: {} bytes", text.len());
            }
        }
        let json = serde_json::to_string(&listed).expect("json");
        assert!(!json.contains(ODD_SECRET), "{json}");
        assert!(!json.contains(&long), "unbounded: {} bytes", json.len());
    }

    /// R87, R194: the trust anchor is judged before anything a scan reads
    /// can fail. tgorka losing this Mac's verification, or signing out,
    /// stops the host even when the provider rows cannot be read in the
    /// same scan — no decision is judged on the anchor that was; the same
    /// failing read with the anchor unchanged keeps the host as it is.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_revoked_anchor_stops_the_host_however_the_scan_fails() {
        let tgorka = |verified: bool| OwnAccount {
            user: OwnedUserId::try_from(TG).expect("user"),
            device_id: "MAC".to_owned(),
            own_identity_verified: verified,
            master_key: Some("ed25519:tgorkas".to_owned()),
        };
        for (accounts, kept, what) in [
            (vec![tgorka(false)], false, "no longer verified here"),
            (Vec::new(), false, "signed out"),
            (vec![tgorka(true)], true, "unchanged"),
        ] {
            let root = tempfile::tempdir().expect("tempdir");
            // `keeper.db` cannot be opened under a file: the provider rows
            // do not read, and the scan returns there.
            let data_dir = root.path().join("not-a-folder");
            std::fs::write(&data_dir, "").expect("a file");
            let mut host = DesktopHost::new(
                TurnEnv::new(Arc::new(platform(root.path()))),
                data_dir.clone(),
                "test",
            );
            assert!(store::list_providers(&data_dir).is_err(), "{what}");
            host.built = Some(built_under(vec![tgorka(true)], MacTables::default()));
            host.scan(DesktopFacts {
                login: Some("tgorka".to_owned()),
                device: Some("hesperia".to_owned()),
                accounts,
                ..DesktopFacts::default()
            })
            .await;
            assert_eq!(host.built.is_some(), kept, "{what}");
        }
    }

    struct Vault;

    impl VaultWriter for Vault {
        fn subfolder(&self, _: &str) -> Option<String> {
            Some("vault".to_owned())
        }

        fn write(&self, _: &str, _: &str, _: &str, _: &str) -> Result<(), String> {
            Ok(())
        }

        fn amend(
            &self,
            _: &str,
            _: &str,
            _: &dyn Fn(&str) -> Option<String>,
        ) -> Result<bool, String> {
            Ok(false)
        }
    }

    /// The app's turn environment carries its notes-vault writer; a copy
    /// hosted on the Mac gets none, like agentd's, since no manifest says
    /// that one host writes the person's vault.
    #[test]
    fn the_mac_s_copies_write_no_notes_vault() {
        let root = tempfile::tempdir().expect("tempdir");
        nixi_drive(root.path());
        let profile = profile(root.path());
        let pins = pin_now(&profile, &shown(&[TG, MARTA], false));
        let drives = desktop_drives(std::slice::from_ref(&profile), &pins, "tgorka");
        let views: Vec<DriveView> = drives.into_iter().map(|drive| drive.view).collect();
        let home = views[0].zone.homes[0].1.clone().expect("nixi reads");

        let data = root.path().join("data");
        store::insert_provider(
            &data,
            &Provider {
                id: "ollama".to_owned(),
                kind: ProviderKind::Ollama,
                name: "ollama".to_owned(),
                base_url: NIXI_BOT.to_owned(),
                created_ms: 1,
            },
        )
        .expect("provider");
        let rows = store::list_providers(&data).expect("rows").rows;

        let platform: Arc<dyn Platform> = Arc::new(platform(root.path()));
        let base = TurnEnv {
            drive: Some(DrivePorts {
                profiles: Arc::new(AgentProfiles::new(Vec::new())),
                vault: Some(Arc::new(Vault)),
                approval: None,
            }),
            ..TurnEnv::new(platform)
        };
        let host = HostSlug::new("hesperia").expect("slug");
        let deps = deps_over(&base, &data, &host, &views, &rows, &home).expect("served");
        let ports = deps.env.drive.expect("the hosting drives");
        assert!(ports.vault.is_none());
    }

    /// R162 (review DB-04, DB-05): the doorbell hears by this Mac's pins.
    /// tgorka's Mac mounts tgdrive (its own, hosted) and neuradrive
    /// (another principal's, pinned, never hosted here): Lucyna, homed in
    /// neuradrive and not hosted here, is heard for it. A third folder whose
    /// `_drive.toml` now claims `tgdrive` against its pin maps nothing, so
    /// tgdrive's bell still fetches tgdrive's folder.
    #[test]
    fn the_doorbell_hears_by_the_pins() {
        use crate::doorbell::fake::FakeEngine;
        use crate::doorbell::Answer;

        let (tg, neura, third) = (
            tempfile::tempdir().expect("tg"),
            tempfile::tempdir().expect("neura"),
            tempfile::tempdir().expect("third"),
        );
        nixi_drive(tg.path());
        let flagged = |id: &str, name: &str, root: &Path| {
            let mut profile =
                SyncProfile::new(id, name, root, format!("git@forge:tgorka/{name}.git"));
            profile.agents = Some(Default::default());
            profile.sessions = Some(Default::default());
            profile
        };
        let drive_file = |id: &str, principal: &str| {
            format!(
                "version = 1\nid = \"{id}\"\nprincipal = \"{principal}\"\nowner = \"{TG}\"\nreaders = [\"{TG}\", \"{MARTA}\"]\nlocal_only = false\n"
            )
        };
        write(
            neura.path(),
            "80-agents/_drive.toml",
            &drive_file("neuradrive", "neuraffica"),
        );
        write(
            neura.path(),
            "80-agents/lucyna-novak/agent.toml",
            &format!(
                "version = 1\nid = \"lucyna-novak\"\nname = \"Lucyna\"\nkind = \"proxy\"\nmatrix_user = \"@lucyna-novak:example.org\"\nhuman = \"{MARTA}\"\n\n[model]\nbot = \"bot:ollama:{NIXI_BOT}#model\"\n"
            ),
        );
        write(
            third.path(),
            "80-agents/_drive.toml",
            &drive_file("shared", "tgorka"),
        );
        let profiles = [
            profile(tg.path()),
            flagged("p-neura", "neuradrive", neura.path()),
            flagged("p3", "shared", third.path()),
        ];
        let mut pins = BTreeMap::new();
        for profile in &profiles {
            pins.extend(pin_now(profile, &shown(&[TG, MARTA], false)));
        }
        // The third folder's file changes after its pin.
        write(
            third.path(),
            "80-agents/_drive.toml",
            &drive_file("tgdrive", "tgorka"),
        );

        let drives = desktop_drives(&profiles, &pins, "tgorka");
        assert!(
            drives[1].view.hosts.is_err(),
            "neuradrive is hosted elsewhere"
        );
        let (mut mounted, agents) = doorbell_roster(&drives, &[]);
        mounted.sort();
        assert_eq!(
            mounted,
            vec![
                ("neuradrive".to_owned(), "p-neura".to_owned()),
                ("tgdrive".to_owned(), "p1".to_owned()),
            ]
        );

        let engine = Arc::new(FakeEngine::default());
        let doorbell = Doorbell::default();
        doorbell.set_engine(Arc::clone(&engine) as Arc<dyn DriveEngine>);
        doorbell.set_drives(mounted, agents);
        doorbell.set_principal_agents(&[OwnedUserId::try_from("@nixi:example.org").expect("nixi")]);
        let bell = |drive: &str, commit: &str| serde_json::json!({"v": 1, "drive": drive, "commit": commit, "reason": "memory"});
        let (a, b) = ("a".repeat(40), "b".repeat(40));
        let lucyna = OwnedUserId::try_from("@lucyna-novak:example.org").expect("lucyna");
        let nixi = OwnedUserId::try_from("@nixi:example.org").expect("nixi");
        assert_eq!(
            doorbell.hear(&lucyna, "neuradrive", &bell("neuradrive", &a)),
            Answer::Heard
        );
        assert_eq!(
            doorbell.hear(&nixi, "tgdrive", &bell("tgdrive", &b)),
            Answer::Heard
        );
        doorbell.deliver(std::time::Instant::now(), 4);
        let mut pulls = engine.pulls.lock().expect("lock").clone();
        pulls.sort();
        assert_eq!(pulls, vec![("p-neura".to_owned(), a), ("p1".to_owned(), b)]);
    }
}
