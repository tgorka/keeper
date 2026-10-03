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
//!   engine; this host never fetches or opens an engine;
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
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};

use keeper_core::agents::agentd::DrivePin;
use keeper_core::agents::copy::{self, AgentCopyVm};
use keeper_core::agents::drive::{self, DriveDecl};
use keeper_core::agents::events::CONTROL_ROOM_TYPE;
use keeper_core::agents::host::Materialized;
use keeper_core::agents::index::Index;
use keeper_core::agents::log::HostSlug;
use keeper_core::agents::matrix::{self, AgentClient, AgentMatrixError};
use keeper_core::agents::mount::pin_matches;
use keeper_core::bots::chat::{self, CancelHandle, CancelSignal};
use keeper_core::bots::store::{self, ProviderRow};
use keeper_core::platform::Platform;
use keeper_sync::SyncProfile;
use matrix_sdk::ruma::{OwnedRoomId, OwnedUserId, UserId};
use matrix_sdk::RoomState;
use tokio::task::JoinHandle;

use crate::agent::bot_for;
use crate::hosts::{HostRuntime, RELEASE_BOUND};
use crate::rooms::Known;
use crate::runtime::{
    deps_over, known_with, open_copy, sessions_of, start_copy, view, Copy, DriveView, RoomSessions,
    TURNS_FINISH,
};
use crate::turn::TurnEnv;
use crate::zone::{read_text, read_zone, AgentHome};

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
}

/// One flagged folder as this host reads it.
#[derive(Debug, Clone)]
pub struct DesktopDrive {
    pub profile_id: String,
    pub view: DriveView,
    pub materialized: Materialized,
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

/// A flagged folder's `_drive.toml`, as the file says it.
fn declared(profile: &SyncProfile) -> Result<DriveDecl, String> {
    let zone = profile
        .agents_root()
        .ok_or_else(|| keeper_core::agents::zone::NO_DRIVE.to_owned())?;
    let text = read_text(&zone, drive::FILE_NAME)?
        .ok_or_else(|| keeper_core::agents::zone::NO_DRIVE.to_owned())?;
    drive::parse(&text).map_err(|refusal| refusal.sentence())
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
            DesktopDrive {
                profile_id: profile.id.clone(),
                view: view(&id, profile.clone(), hosts),
                materialized: materialized(profile),
            }
        })
        .collect()
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
        }
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
        let platform = Arc::clone(&self.platform);
        let data_dir = self.data_dir.clone();
        let read = tokio::task::spawn_blocking(move || {
            let drives = desktop_drives(&facts.profiles, &facts.pins, &login);
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
            (facts, login, drives, rows, signed)
        })
        .await;
        let Ok((facts, login, drives, rows, signed)) = read else {
            tracing::error!("agents: the desktop's zones could not be read");
            return;
        };
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
        };

        let views: Vec<DriveView> = drives.iter().map(|drive| drive.view.clone()).collect();
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
                Ok(deps) => Arc::new(deps),
                Err(sentence) => {
                    tracing::error!(agent = %home.config.id, %sentence, "agents: this agent is not served on this Mac");
                    self.problems.insert(user.clone(), sentence);
                    continue;
                }
            };
            let (copy, sync) = start_copy(deps, client, Arc::clone(&known));
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
        let runtime = HostRuntime::desktop(host, login, &self.version, &manifest, copies.clone());
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

    struct Vault;

    impl VaultWriter for Vault {
        fn subfolder(&self, _: &str) -> Option<String> {
            Some("vault".to_owned())
        }

        fn write(&self, _: &str, _: &str, _: &str) -> Result<(), String> {
            Ok(())
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
}
