//! The agents' database (AD-418, AD-421, AD-422; FR-845, FR-846, FR-871):
//! keeper as a client of the syncular server the person runs.
//!
//! keeper ships no server address (D-4). The person sets `agents.server_url`
//! to the server's base URL; a device whose person is signed in to the
//! account gets its credential by signing in ([`signin`], 109.6), and any
//! other device pairs by redeeming a code another of their devices shows,
//! typed or opened from its QR ([`pairing`]); the token it receives lives in
//! the platform keychain and nowhere else — never in `keeper.db`, a settings
//! file, a view model or a log line. `GET /databases` says which databases
//! the token opens, and the device holds one replica of each ([`databases`]),
//! each with its own socket at `<base>/<database>/…` and its own owner thread
//! ([`syncular`]). With no address, or no credential, everything written
//! waits in a local replica's outbox and keeper makes no request to any
//! agents server.
//!
//! One lifecycle owns every request: [`Hq`] follows the effective
//! `agents.server_url` ([`Hq::reconcile`], called whenever a setting or a
//! settings layer changes), and a change closes every replica's network
//! before it is published. [`Hq::contacting`] names each server a request may
//! still reach, so the egress list never drops a host keeper is talking to.
//! A replica links only once this run's discovery says its database is open;
//! a refused replica is retried on a growing backoff that outlives the
//! replica; a device the server removed stays removed across restarts until
//! it is paired again, and stays readable until the person removes it.
//!
//! The Agents window reads only the replicas (AD-424, AD-433; [`window`]):
//! the sessions of every held database in one list, each labelled with its
//! database ([`sessions`]); an opened session from its windowed months, or,
//! archived, through the server's registered `sessionPage` ([`timeline`]);
//! messages and decisions written as rows into the session's own database
//! ([`send`], [`approvals`]); who is in a session, from that database's
//! directory alone ([`members`]); which instance of an agent answers and
//! who is online, and the person's choice of it ([`instances`]).
//!
//! A phone registers where the server's push reaction posts for it, and
//! reads, shows and decides the one item a push names ([`push`], AD-432,
//! D-35): the notification extension with the same item read and the same
//! table, the app with the same decision row.
//!
//! The shell's `hq_ipc` is a call site and decides nothing. Tracing targets
//! are `keeper_core::hq::*` and carry no row content.

pub mod approvals;
pub mod attachments;
pub mod blob_cache;
pub mod commands;
pub mod context;
pub mod databases;
pub mod default_agent;
pub mod drives;
pub mod grants;
pub mod identity;
pub mod instances;
pub mod live;
pub mod members;
pub mod pairing;
pub mod port;
pub mod push;
pub mod routes;
pub mod send;
pub mod server;
pub mod sessions;
pub mod settings;
pub mod signin;
pub mod spoken;
pub mod syncular;
pub mod thumbnails;
pub mod timeline;
pub mod vm;
pub mod vocabulary;
pub mod window;

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError, Weak};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::watch;
use tokio::task::JoinHandle;
use tokio::time::Instant;

use crate::error::CoreError;
use crate::hq::databases::Authorization;
use crate::hq::pairing::PairingCode;
use crate::hq::port::{Link, LinkState, Opener, Replica, ReplicaSpec, ReplicaState, Subscription};
use crate::hq::routes::{RouteError, Routes};
use crate::hq::server::ServerUrl;
use crate::hq::vm::{
    database_vm, not_connected_sentence, phase_sentence, HqDeviceVm, HqPairCodeVm, HqPhase,
    HqSignInState, HqSignInVm, HqStatusVm, HqThisDeviceVm,
};
use crate::platform::{exclude_from_backup_best_effort, Platform};

/// The keychain item of a server's pairing: this prefix and the base URL.
const KEYCHAIN_PREFIX: &str = "agents-server/";

/// The directory subscriptions every linked replica holds: the database's
/// actors and clients (the person's devices among them), by `dir`. Beside
/// them each holds the person's memberships (`memberships`), from which the
/// Agents window's windows follow ([`window`]), and the person's choice of
/// which instance answers each agent (`preferences`, [`instances`]).
const DIRECTORY: [(&str, &str); 3] = [
    ("directoryActors", "actors"),
    ("directoryClients", "clients"),
    ("directoryAgentProfiles", "agent_profiles"),
];

/// The longest wait before a refused database is tried again.
const MAX_REFUSAL_BACKOFF: Duration = Duration::from_secs(600);

/// The wait before a database refused `streak` times in a row is linked again.
fn refusal_backoff(streak: u32) -> Duration {
    let exponent = streak.saturating_sub(1).min(10);
    Duration::from_secs(1u64 << exponent).min(MAX_REFUSAL_BACKOFF)
}

/// Why an hq call failed. `Sentence` is shown to the person as it is.
#[derive(Debug, thiserror::Error)]
pub enum HqError {
    #[error("{0}")]
    Sentence(String),
    /// What was asked for is not on this device yet, and may still arrive:
    /// the replicas are still opening, or the database syncs (or the server
    /// has yet to say whether it does). The sentence is shown while the
    /// caller waits for the list or the status to move and asks again.
    #[error("{0}")]
    Arriving(String),
    #[error(transparent)]
    Core(#[from] CoreError),
    #[error("the agents' replicas on disk: {0}")]
    Io(#[from] std::io::Error),
}

/// What a device keeps of its pairing with one server, in the keychain.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Pairing {
    token: String,
    actor_id: String,
    client_id: String,
    name: String,
    /// Set when the credential came from a sign-in (109.6): when it was
    /// last attested. A pairing's has none, and a sign-in adopts it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bound: Option<signin::Bound>,
}

impl std::fmt::Debug for Pairing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pairing")
            .field("token", &"<redacted>")
            .field("client_id", &self.client_id)
            .field("bound", &self.bound)
            .finish()
    }
}

/// The servers a request may still reach, each held by whatever is
/// reaching it.
#[derive(Default)]
struct Contacts {
    next: AtomicU64,
    live: Mutex<Vec<(u64, String)>>,
}

/// One holder of a server in [`Contacts`]; released when dropped.
struct Contact {
    contacts: Arc<Contacts>,
    id: u64,
}

impl Contacts {
    fn hold(self: &Arc<Self>, server: &ServerUrl) -> Contact {
        let id = self.next.fetch_add(1, Ordering::SeqCst);
        lock(&self.live).push((id, server.as_str().to_owned()));
        Contact {
            contacts: Arc::clone(self),
            id,
        }
    }
}

impl Drop for Contact {
    fn drop(&mut self) {
        lock(&self.contacts.live).retain(|(id, _)| *id != self.id);
    }
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(PoisonError::into_inner)
}

struct Db {
    name: String,
    replica: Arc<dyn Replica>,
    /// Whether `replica` was opened with a link.
    linked: bool,
    /// This replica's refusal has been counted.
    refusal_counted: bool,
}

/// A database's refusals in a row, kept across the replicas that replace
/// each other, and when it may be linked again.
#[derive(Debug, Clone, Copy, Default)]
struct Refusal {
    streak: u32,
    until: Option<Instant>,
}

/// Where this run's opening of the replicas on disk stands. Until it
/// settles, what is not found may simply not be open yet.
#[derive(Debug, Default)]
enum LocalOpen {
    /// Not started, or a restart is opening the replicas now.
    #[default]
    Opening,
    /// Every replica this configuration holds is open.
    Open,
    /// The last restart failed; its sentence.
    Failed(String),
}

#[derive(Default)]
struct Inner {
    server: Option<ServerUrl>,
    /// `agents.server_url` holds text the grammar refuses.
    address_refused: bool,
    /// The pairing whose replicas are held: the configured server's, or —
    /// with no address set — the one of the server this device last synced
    /// with ([`databases::remembered_server`]), whose replicas stay listed
    /// and written to and never link.
    pairing: Option<Pairing>,
    authorization: Authorization,
    /// Whether the replicas on disk are open yet ([`Hq::restart`]).
    local_open: LocalOpen,
    local: Option<Arc<dyn Replica>>,
    dbs: Vec<Db>,
    /// Bumped on every reconfiguration, so work of an older one lands nowhere.
    generation: u64,
    tasks: Vec<JoinHandle<()>>,
    /// A discovery is pending or running.
    discovering: bool,
    refusals: HashMap<String, Refusal>,
    /// A pairing link opened here, waiting for the person to confirm it.
    offer: Option<(ServerUrl, PairingCode)>,
    /// Held while any replica of `server` is linked.
    linked_contact: Option<Contact>,
    /// The push target this device wants the server to post to (106.6):
    /// `None` until the shell has said, `Some(None)` for none.
    push_target: Option<Option<push::PushTarget>>,
    /// What the server last took: its base URL, this device's client id and
    /// the target.
    push_told: Option<(String, String, Option<push::PushTarget>)>,
    /// Sign-in enrolment on this configuration ([`signin`]).
    sign_in: signin::SignInMemo,
}

/// What a reconfiguration takes out of service.
struct Retired {
    replicas: Vec<Arc<dyn Replica>>,
    contact: Option<Contact>,
}

/// The agents server, this device's pairing with it, and every replica.
pub struct Hq {
    me: Weak<Hq>,
    platform: Arc<dyn Platform>,
    opener: Arc<dyn Opener>,
    routes: Arc<dyn Routes>,
    /// The account's sign-in, which enrols this device ([`signin`]).
    sign_in: Arc<dyn signin::SignIn>,
    /// How far tests moved the attestation timers' wall clock on.
    #[cfg(test)]
    wall_offset: std::sync::atomic::AtomicI64,
    device_name: String,
    device_host: Box<dyn Fn() -> String + Send + Sync>,
    /// Serialises every change of configuration. Never held across a request
    /// to the server; reads never take it.
    lifecycle: tokio::sync::Mutex<()>,
    /// Short sections only, never across an await.
    inner: Mutex<Inner>,
    contacts: Arc<Contacts>,
    status: watch::Sender<HqStatusVm>,
    /// Ticks on every change a view of the Agents window reads again on.
    changes: watch::Sender<u64>,
    /// The sessions open on screen, and their pages.
    views: Mutex<HashMap<sessions::AgentSessionRef, timeline::Opened>>,
    /// The units each replica's windows were last set to.
    windows: Mutex<HashMap<(String, &'static str), Vec<String>>>,
    /// When each database's revoked grants units were last asked for again
    /// ([`grants::reask_revoked`]); emptied when the replicas restart.
    grants_asked: Mutex<HashMap<String, std::time::Instant>>,
    /// The files the composers staged, not sent yet ([`attachments::Staging`]).
    staging: attachments::Staging,
    window_task: Mutex<Option<JoinHandle<()>>>,
    /// Decisions whose outcome the server still owes ([`approvals::Decisions`]).
    decisions: std::sync::OnceLock<approvals::Decisions>,
    /// What this viewer chose to see ([`live::AgentViewPrefsVm`]), once read.
    prefs: Mutex<Option<live::AgentViewPrefsVm>>,
    /// Ticks when only a presence document moved: the open sessions read
    /// again on it, nothing else does.
    live: watch::Sender<u64>,
    /// One push target request at a time, so the last wish is the one the
    /// server keeps.
    push_turn: tokio::sync::Mutex<()>,
}

fn no_server() -> HqStatusVm {
    HqStatusVm {
        server: None,
        phase: HqPhase::NoServer,
        sentence: phase_sentence(HqPhase::NoServer, None, 0),
        device: None,
        databases: Vec::new(),
        waiting_here: 0,
        devices: Vec::new(),
        offer: None,
        sign_in: HqSignInVm::unavailable(),
    }
}

fn keychain_key(server: &ServerUrl) -> String {
    format!("{KEYCHAIN_PREFIX}{}", server.as_str())
}

fn route_sentence(server: &ServerUrl, error: &RouteError) -> String {
    match error {
        RouteError::Unauthorized => "The server does not know this device.".to_owned(),
        RouteError::Forbidden(code) | RouteError::Refused { code, .. } => {
            format!("{} refused: {code}.", server.host())
        }
        RouteError::Unreachable(why) => format!("keeper could not reach {}: {why}", server.host()),
        RouteError::Unexpected(why) => {
            format!(
                "{} answered something keeper cannot read: {why}",
                server.host()
            )
        }
    }
}

fn sentence(error: impl std::fmt::Display) -> HqError {
    HqError::Sentence(error.to_string())
}

/// Close every replica's network at once, then wait for every owner to stop.
async fn retire(retired: Retired) {
    for replica in &retired.replicas {
        replica.close();
    }
    let replicas = retired.replicas;
    let joined = tokio::task::spawn_blocking(move || {
        for replica in &replicas {
            replica.join();
        }
    })
    .await;
    if let Err(error) = joined {
        tracing::warn!(%error, "an agents replica did not stop cleanly");
    }
    // Only now is the server no longer contacted.
    drop(retired.contact);
}

impl Hq {
    /// A service with nothing open yet; [`Hq::start`] reads the setting.
    /// `sign_in` is the account whose sign-in enrols this device.
    /// `device_name` is how the person's other devices will list this one.
    /// `device_host` reads the OS host at redemption and at enrolment, after
    /// any late platform setup.
    pub fn new(
        platform: Arc<dyn Platform>,
        opener: Arc<dyn Opener>,
        routes: Arc<dyn Routes>,
        sign_in: Arc<dyn signin::SignIn>,
        device_name: String,
        device_host: impl Fn() -> String + Send + Sync + 'static,
    ) -> Arc<Hq> {
        let (status, _) = watch::channel(no_server());
        let (changes, _) = watch::channel(0);
        Arc::new_cyclic(|me| Hq {
            me: me.clone(),
            platform,
            opener,
            routes,
            sign_in,
            #[cfg(test)]
            wall_offset: std::sync::atomic::AtomicI64::new(0),
            device_name,
            device_host: Box::new(device_host),
            lifecycle: tokio::sync::Mutex::new(()),
            inner: Mutex::new(Inner::default()),
            contacts: Arc::default(),
            status,
            changes,
            live: watch::channel(0).0,
            views: Mutex::default(),
            windows: Mutex::default(),
            grants_asked: Mutex::default(),
            staging: attachments::Staging::default(),
            window_task: Mutex::default(),
            decisions: std::sync::OnceLock::new(),
            prefs: Mutex::default(),
            push_turn: tokio::sync::Mutex::new(()),
        })
    }

    /// The status, now and at every change.
    pub fn status(&self) -> watch::Receiver<HqStatusVm> {
        self.status.subscribe()
    }

    /// Every agents server a request may still reach: the configured one
    /// while it is linked or asked, and an old one until its last request
    /// has ended. The egress list adds these to the setting.
    pub fn contacting(&self) -> Vec<String> {
        let mut servers: Vec<String> = Vec::new();
        for (_, server) in lock(&self.contacts.live).iter() {
            if !servers.contains(server) {
                servers.push(server.clone());
            }
        }
        servers
    }

    /// The replica of `database`, when this device holds one.
    pub fn replica(&self, database: &str) -> Option<Arc<dyn Replica>> {
        lock(&self.inner)
            .dbs
            .iter()
            .find(|db| db.name == database)
            .map(|db| Arc::clone(&db.replica))
    }

    /// The replica of what is written while no server is paired.
    pub fn local(&self) -> Option<Arc<dyn Replica>> {
        lock(&self.inner).local.clone()
    }

    fn data_dir(&self) -> Result<PathBuf, HqError> {
        Ok(self.platform.data_dir()?)
    }

    fn read_pairing(&self, server: &ServerUrl) -> Result<Option<Pairing>, HqError> {
        let Some(stored) = self.platform.keychain_get(&keychain_key(server))? else {
            return Ok(None);
        };
        match serde_json::from_str::<Pairing>(&stored) {
            Ok(pairing) => Ok(Some(pairing)),
            Err(error) => {
                tracing::warn!(%error, "the agents server pairing in the keychain is unreadable; treating this device as not paired");
                Ok(None)
            }
        }
    }

    /// Open replicas off the async workers; never touches the network.
    async fn open_all(&self, specs: Vec<ReplicaSpec>) -> Result<Vec<Arc<dyn Replica>>, HqError> {
        for spec in &specs {
            if let Some(parent) = spec.path.parent() {
                std::fs::create_dir_all(parent)?;
            }
        }
        let opener = Arc::clone(&self.opener);
        tokio::task::spawn_blocking(move || {
            let mut opened: Vec<Arc<dyn Replica>> = Vec::new();
            for spec in specs {
                match opener.open(spec) {
                    Ok(replica) => opened.push(replica),
                    Err(error) => {
                        for replica in &opened {
                            replica.close();
                        }
                        return Err(sentence(error));
                    }
                }
            }
            Ok(opened)
        })
        .await
        .map_err(sentence)?
    }

    fn spec(
        data_dir: &std::path::Path,
        server: &ServerUrl,
        pairing: &Pairing,
        database: &str,
        linked: bool,
    ) -> ReplicaSpec {
        ReplicaSpec {
            database: database.to_owned(),
            path: databases::replica(data_dir, server, database, &pairing.actor_id),
            client_id: Some(pairing.client_id.clone()),
            link: linked.then(|| Link {
                base_url: server.mount(database),
                token: pairing.token.clone(),
            }),
            subscriptions: if linked {
                DIRECTORY
                    .iter()
                    .map(|(id, table)| Subscription {
                        id: (*id).to_owned(),
                        table: (*table).to_owned(),
                        scopes: vec![("dir".to_owned(), vec![database.to_owned()])],
                    })
                    .chain(std::iter::once(Subscription {
                        id: "memberships".to_owned(),
                        table: "session_members".to_owned(),
                        scopes: vec![("member".to_owned(), vec![pairing.actor_id.clone()])],
                    }))
                    .chain(std::iter::once(Subscription {
                        id: "preferences".to_owned(),
                        table: "instance_prefs".to_owned(),
                        scopes: vec![("actor".to_owned(), vec![pairing.actor_id.clone()])],
                    }))
                    .collect()
            } else {
                Vec::new()
            },
            watch_presence: linked.then(|| instances::dir_key(database)),
        }
    }

    /// Read `agents.server_url` and the pairing and open every replica the
    /// device holds — local-only, so every read answers before the server
    /// does — then ask the server which of them sync.
    pub async fn start(&self) -> Result<(), HqError> {
        let _lifecycle = self.lifecycle.lock().await;
        self.restart().await?;
        self.follow_windows();
        Ok(())
    }

    /// Follow the effective settings: the view preferences (a settings file
    /// or another device may have switched them) and the server — a no-op
    /// when it still names what this service runs, a restart otherwise.
    pub async fn reconcile(&self) -> Result<(), HqError> {
        let _lifecycle = self.lifecycle.lock().await;
        self.reread_view_prefs();
        let stored = crate::registry::get_agents_server_url(&self.data_dir()?)?;
        let wanted = stored.as_deref().map(ServerUrl::parse);
        let unchanged = {
            let inner = lock(&self.inner);
            match &wanted {
                None | Some(Ok(None)) => inner.server.is_none() && !inner.address_refused,
                Some(Ok(Some(server))) => inner.server.as_ref() == Some(server),
                Some(Err(_)) => inner.address_refused,
            }
        };
        if unchanged {
            // An account signed in or out, or another account set up: a
            // device that can sign in now does.
            self.consider_sign_in();
            return Ok(());
        }
        tracing::info!("the agents server setting changed; following it");
        self.restart().await
    }

    /// Stop everything: no request after this returns. For a process
    /// ending, or a test's restart.
    pub async fn shutdown(&self) {
        let _lifecycle = self.lifecycle.lock().await;
        let retired = {
            let mut inner = lock(&self.inner);
            inner.generation += 1;
            Self::retire_all(&mut inner)
        };
        retire(retired).await;
    }

    fn retire_all(inner: &mut Inner) -> Retired {
        for task in inner.tasks.drain(..) {
            task.abort();
        }
        // The sign-in attempt of the configuration retired here sends
        // nothing more (R1096-02); one already sent keeps what it is answered.
        inner.sign_in.cancel();
        inner.discovering = false;
        let mut replicas: Vec<Arc<dyn Replica>> =
            inner.dbs.drain(..).map(|db| db.replica).collect();
        replicas.extend(inner.local.take());
        Retired {
            replicas,
            contact: inner.linked_contact.take(),
        }
    }

    /// The lifecycle lock is held. While the replicas on disk reopen, a
    /// session not found is still arriving ([`LocalOpen`]); once they are
    /// open, or the reopen failed, every open waiting on them is told to ask
    /// again through the status — even when it reads the same.
    async fn restart(&self) -> Result<(), HqError> {
        lock(&self.inner).local_open = LocalOpen::Opening;
        // A new link asks again for every grants unit the server refused.
        lock(&self.grants_asked).clear();
        let reopened = self.reopen().await;
        if let Err(error) = &reopened {
            lock(&self.inner).local_open = LocalOpen::Failed(error.to_string());
        }
        self.publish();
        self.status.send_modify(|_| {});
        if reopened.is_ok() {
            self.consider_sign_in();
        }
        reopened
    }

    /// [`Hq::restart`]'s work: settles [`LocalOpen::Open`] with the replicas
    /// it installs.
    async fn reopen(&self) -> Result<(), HqError> {
        let data_dir = self.data_dir()?;
        let stored = crate::registry::get_agents_server_url(&data_dir)?;
        let (server, address_refused) = match stored.as_deref().map(ServerUrl::parse) {
            None | Some(Ok(None)) => (None, false),
            Some(Ok(Some(server))) => (Some(server), false),
            Some(Err(_)) => (None, true),
        };
        // With no address, the server this device last synced with keeps its
        // replicas listed and written to; nothing of it links (106.1 AC 2).
        let holding = match &server {
            Some(server) => Some(server.clone()),
            None => databases::remembered_server(&data_dir),
        };
        let pairing = match &holding {
            Some(holding) => self.read_pairing(holding)?,
            None => None,
        };
        if let (Some(server), Some(_)) = (&server, &pairing) {
            if let Err(error) = databases::remember_server(&data_dir, server) {
                tracing::warn!(%error, "which agents server this device syncs with could not be kept");
            }
        }
        let authorization = match (&holding, &pairing) {
            (Some(holding), Some(_)) => Authorization::load(&data_dir, holding),
            _ => Authorization::default(),
        };
        let (retired, generation) = {
            let mut inner = lock(&self.inner);
            inner.generation += 1;
            let retired = Self::retire_all(&mut inner);
            if inner.server != server {
                inner.refusals.clear();
            }
            inner.server = server.clone();
            inner.address_refused = address_refused;
            inner.pairing = pairing.clone();
            inner.authorization = authorization.clone();
            inner.sign_in = signin::SignInMemo::default();
            (retired, inner.generation)
        };
        // Every old request ends before the new configuration is published.
        retire(retired).await;

        let root = databases::root(&data_dir);
        std::fs::create_dir_all(&root)?;
        exclude_from_backup_best_effort(self.platform.as_ref(), &root);
        let local = databases::local_replica(&data_dir);
        let mut specs = Vec::new();
        let open_local = pairing.is_none() || local.is_file();
        if open_local {
            specs.push(ReplicaSpec {
                database: "local".to_owned(),
                path: local,
                client_id: None,
                link: None,
                subscriptions: Vec::new(),
                watch_presence: None,
            });
        }
        let held = match (&holding, &pairing) {
            (Some(holding), Some(pairing)) => {
                databases::held(&data_dir, holding, &pairing.actor_id)
            }
            _ => Vec::new(),
        };
        if let (Some(holding), Some(pairing)) = (&holding, &pairing) {
            for name in &held {
                specs.push(Self::spec(&data_dir, holding, pairing, name, false));
            }
        }
        let mut opened = self.open_all(specs).await?.into_iter();
        {
            let mut inner = lock(&self.inner);
            if open_local {
                inner.local = opened.next();
            }
            for (name, replica) in held.into_iter().zip(opened) {
                inner.dbs.push(Db {
                    name,
                    replica,
                    linked: false,
                    refusal_counted: false,
                });
            }
            let watched: Vec<(Option<String>, Arc<dyn Replica>)> = inner
                .local
                .iter()
                .map(|local| (None, Arc::clone(local)))
                .chain(
                    inner
                        .dbs
                        .iter()
                        .map(|db| (Some(db.name.clone()), Arc::clone(&db.replica))),
                )
                .collect();
            for (name, replica) in watched {
                let task = self.watch_replica(name, &replica, generation);
                inner.tasks.push(task);
            }
            // A removed device asks nothing until it is paired again, and
            // with no address nothing is asked at all.
            if server.is_some() && pairing.is_some() && !authorization.removed {
                self.schedule_discovery(&mut inner, Duration::ZERO);
            }
            // A credential that came from a sign-in stays attested.
            if let (Some(_), Some(bound), false) = (
                &server,
                pairing.as_ref().and_then(|pairing| pairing.bound),
                authorization.removed,
            ) {
                self.schedule_attest(&mut inner, bound);
            }
            inner.local_open = LocalOpen::Open;
        }
        Ok(())
    }

    /// Recompute the status whenever `replica` publishes; count a refusal of
    /// `database` once per replica.
    fn watch_replica(
        &self,
        database: Option<String>,
        replica: &Arc<dyn Replica>,
        generation: u64,
    ) -> JoinHandle<()> {
        let mut states = replica.watch();
        let me = self.me.clone();
        let watched = Arc::downgrade(replica);
        tokio::spawn(async move {
            // Nothing is taken as seen before the first change: one landing
            // before this task first runs is still published.
            let mut last: Option<ReplicaState> = None;
            while states.changed().await.is_ok() {
                let state = *states.borrow_and_update();
                let Some(hq) = me.upgrade() else { return };
                // A tail moving is the open sessions' alone: nothing the
                // status or the list reads changed.
                let presence_only = last.is_some_and(|last| {
                    ReplicaState {
                        presence: last.presence,
                        ..state
                    } == last
                });
                last = Some(state);
                if presence_only {
                    hq.live.send_modify(|tick| *tick = tick.wrapping_add(1));
                    continue;
                }
                if let Some(database) = &database {
                    hq.on_link(database, &watched, state.link, generation);
                }
                hq.publish();
            }
        })
    }

    fn on_link(
        &self,
        database: &str,
        replica: &Weak<dyn Replica>,
        link: LinkState,
        generation: u64,
    ) {
        let mut inner = lock(&self.inner);
        if inner.generation != generation {
            return;
        }
        let Some(db) = inner.dbs.iter_mut().find(|db| {
            db.name == database
                && replica
                    .upgrade()
                    .is_some_and(|replica| Arc::ptr_eq(&replica, &db.replica))
        }) else {
            return;
        };
        match link {
            LinkState::Connected => {
                inner.refusals.remove(database);
            }
            LinkState::Refused if !db.refusal_counted => {
                db.refusal_counted = true;
                let refusal = inner.refusals.entry(database.to_owned()).or_default();
                refusal.streak = refusal.streak.saturating_add(1);
                let wait = refusal_backoff(refusal.streak);
                refusal.until = Some(Instant::now() + wait);
                tracing::info!(
                    streak = refusal.streak,
                    retry_in_ms = wait.as_millis() as u64,
                    "an agents database refused this device; asking the server again later"
                );
                // A refused mount is often a removed device: ask at once whether
                // it still is one; the database itself waits for its backoff.
                if !inner.authorization.removed {
                    self.schedule_discovery(&mut inner, Duration::ZERO);
                }
            }
            _ => {}
        }
    }

    /// `GET /databases` after `delay`, unless one is already pending.
    fn schedule_discovery(&self, inner: &mut Inner, delay: Duration) {
        if inner.discovering {
            return;
        }
        let (Some(server), Some(pairing)) = (inner.server.clone(), inner.pairing.clone()) else {
            return;
        };
        inner.discovering = true;
        let generation = inner.generation;
        let me = self.me.clone();
        let routes = Arc::clone(&self.routes);
        let contacts = Arc::clone(&self.contacts);
        inner.tasks.push(tokio::spawn(async move {
            tokio::time::sleep(delay).await;
            let mut failures: u32 = 0;
            loop {
                let answer = {
                    let _contact = contacts.hold(&server);
                    routes.discover(&server, &pairing.token).await
                };
                match answer {
                    Err(RouteError::Unreachable(why)) | Err(RouteError::Unexpected(why)) => {
                        failures = failures.saturating_add(1);
                        let wait = syncular::backoff(failures);
                        tracing::info!(%why, retry_in_ms = wait.as_millis() as u64, "agents server discovery failed");
                        tokio::time::sleep(wait).await;
                    }
                    other => {
                        let Some(hq) = me.upgrade() else { return };
                        if let Err(error) = hq.apply_discovery(generation, other).await {
                            tracing::warn!(%error, "agents server discovery could not be applied");
                        }
                        return;
                    }
                }
            }
        }));
    }

    /// Link what the answer opens (and whose refusal backoff is over), stop
    /// linking what it does not, remember what it said. A replica being
    /// replaced stays readable until its successor is in place.
    async fn apply_discovery(
        &self,
        generation: u64,
        answer: Result<routes::Discovery, RouteError>,
    ) -> Result<(), HqError> {
        let _lifecycle = self.lifecycle.lock().await;
        let data_dir = self.data_dir()?;
        let (server, pairing, authorization) = {
            let mut inner = lock(&self.inner);
            if inner.generation != generation {
                return Ok(());
            }
            inner.discovering = false;
            let (Some(server), Some(pairing)) = (inner.server.clone(), inner.pairing.clone())
            else {
                return Ok(());
            };
            let authorization = match answer {
                Ok(found) => Authorization {
                    removed: false,
                    open: found.databases,
                },
                Err(RouteError::Unauthorized) => {
                    tracing::warn!("the agents server no longer knows this device; every replica stops syncing");
                    Authorization {
                        removed: true,
                        open: inner.authorization.open.clone(),
                    }
                }
                Err(error) => {
                    tracing::warn!(%error, "agents server discovery refused");
                    return Ok(());
                }
            };
            inner.authorization = authorization.clone();
            (server, pairing, authorization)
        };
        if let Err(error) = authorization.save(&data_dir, &server) {
            tracing::warn!(%error, "what the agents server said about this device could not be kept");
        }

        let now = Instant::now();
        // What changes, decided under the lock; done outside it.
        let (reopen, retired, next_retry) = {
            let inner = lock(&self.inner);
            let held: Vec<String> = inner.dbs.iter().map(|db| db.name.clone()).collect();
            let mut reopen: Vec<(String, bool)> = Vec::new();
            let mut retired: Vec<Arc<dyn Replica>> = Vec::new();
            let mut next_retry: Option<Duration> = None;
            for (name, open) in databases::plan(&held, &authorization.open) {
                let ready = inner
                    .refusals
                    .get(&name)
                    .and_then(|refusal| refusal.until)
                    .is_none_or(|until| now >= until);
                if !ready && open && !authorization.removed {
                    let wait = inner
                        .refusals
                        .get(&name)
                        .and_then(|refusal| refusal.until)
                        .map_or(Duration::ZERO, |until| until.saturating_duration_since(now));
                    next_retry = Some(next_retry.map_or(wait, |w: Duration| w.min(wait)));
                }
                let link = open && !authorization.removed && ready;
                let current = inner.dbs.iter().position(|db| db.name == name);
                let keep = current.is_some_and(|at| {
                    let db = &inner.dbs[at];
                    db.linked == link && !(link && db.replica.state().link == LinkState::Refused)
                }) || current.is_some_and(|at| {
                    // A refused replica waits for its backoff rather than reopening.
                    let db = &inner.dbs[at];
                    db.linked && !ready && open && !authorization.removed
                });
                if keep {
                    continue;
                }
                // The old replica stays listed, and readable, until its
                // successor is installed.
                if let Some(at) = current {
                    retired.push(Arc::clone(&inner.dbs[at].replica));
                }
                reopen.push((name, link));
            }
            (reopen, retired, next_retry)
        };
        retire(Retired {
            replicas: retired,
            contact: None,
        })
        .await;
        let specs = reopen
            .iter()
            .map(|(name, link)| Self::spec(&data_dir, &server, &pairing, name, *link))
            .collect();
        let opened = self.open_all(specs).await?;
        let stale_contact = {
            let mut inner = lock(&self.inner);
            if inner.generation != generation {
                for replica in &opened {
                    replica.close();
                }
                return Ok(());
            }
            for ((name, linked), replica) in reopen.into_iter().zip(opened) {
                inner.dbs.retain(|db| db.name != name);
                let task = self.watch_replica(Some(name.clone()), &replica, generation);
                inner.tasks.push(task);
                inner.dbs.push(Db {
                    name,
                    replica,
                    linked,
                    refusal_counted: false,
                });
            }
            let order = databases::plan(
                &inner
                    .dbs
                    .iter()
                    .map(|db| db.name.clone())
                    .collect::<Vec<_>>(),
                &authorization.open,
            );
            inner
                .dbs
                .sort_by_key(|db| order.iter().position(|(name, _)| *name == db.name));
            let any_linked = inner.dbs.iter().any(|db| db.linked);
            let stale = if any_linked {
                if inner.linked_contact.is_none() {
                    inner.linked_contact = Some(self.contacts.hold(&server));
                }
                None
            } else {
                inner.linked_contact.take()
            };
            if let Some(wait) = next_retry {
                self.schedule_discovery(&mut inner, wait);
            }
            stale
        };
        drop(stale_contact);
        self.publish();
        if authorization.removed {
            // A lapsed or suspended sign-in is refused like a removed device:
            // signed in, the device signs in again, keeping its client id.
            self.consider_sign_in();
        } else {
            self.tell_push_target();
        }
        Ok(())
    }

    /// Recompute the status from the replicas' states and their directory.
    /// Reads replica files only.
    fn publish(&self) {
        let (snapshot, replicas, actor, me) = {
            let inner = lock(&self.inner);
            // With no address, what waits in the last server's replicas waits
            // here too: nothing takes it until an address is set again.
            let waiting_here = inner
                .local
                .iter()
                .chain(
                    inner
                        .dbs
                        .iter()
                        .filter(|_| inner.server.is_none())
                        .map(|db| &db.replica),
                )
                .map(|replica| u32::try_from(replica.state().outbox).unwrap_or(u32::MAX))
                .fold(0u32, u32::saturating_add);
            let host = inner.server.as_ref().map(ServerUrl::host);
            let removed = inner.authorization.removed;
            let phase = if inner.address_refused {
                HqPhase::Refused
            } else if inner.server.is_none() {
                HqPhase::NoServer
            } else if inner.pairing.is_none() {
                HqPhase::NotPaired
            } else if removed {
                HqPhase::Removed
            } else {
                HqPhase::Paired
            };
            // This device as a server knows it: nothing while no address is set,
            // even with the last server's replicas held.
            let paired = inner.server.as_ref().and(inner.pairing.as_ref());
            let linkable = inner.server.is_some();
            let sign_in = Self::sign_in_vm(&inner);
            let sentence = match (phase, sign_in.state, host) {
                // No code to type: signing in connects it.
                (
                    HqPhase::NotPaired,
                    HqSignInState::SignedOut | HqSignInState::Enrolling,
                    Some(host),
                ) => not_connected_sentence(host, waiting_here),
                _ => phase_sentence(phase, host, waiting_here),
            };
            let status = HqStatusVm {
                server: inner.server.as_ref().map(|s| s.as_str().to_owned()),
                phase,
                sentence,
                device: paired.map(|pairing| HqThisDeviceVm {
                    client_id: pairing.client_id.clone(),
                    name: pairing.name.clone(),
                }),
                databases: inner
                    .dbs
                    .iter()
                    .map(|db| {
                        let state = db.replica.state();
                        let open = inner.authorization.open.contains(&db.name);
                        database_vm(&db.name, state.link, state.outbox, linkable, open, removed)
                    })
                    .collect(),
                waiting_here,
                devices: Vec::new(),
                offer: inner
                    .offer
                    .as_ref()
                    .map(|(server, _)| server.host().to_owned()),
                sign_in,
            };
            let replicas: Vec<Arc<dyn Replica>> =
                inner.dbs.iter().map(|db| Arc::clone(&db.replica)).collect();
            let actor = paired.map(|p| p.actor_id.clone());
            let me = paired.map(|p| p.client_id.clone());
            (status, replicas, actor, me)
        };
        let mut status = snapshot;
        if let (Some(actor), Some(me)) = (actor, me) {
            status.devices = devices(&replicas, &actor, &me);
        }
        self.status.send_if_modified(|current| {
            let changed = *current != status;
            *current = status;
            changed
        });
        // A replica's rows or the replicas themselves changed.
        self.touch();
    }

    /// Set `agents.server_url` and follow it. A refused address is returned
    /// as its sentence and nothing is stored; blank clears it.
    pub async fn set_server(&self, raw: &str) -> Result<HqStatusVm, HqError> {
        let server = ServerUrl::parse(raw).map_err(sentence)?;
        let _lifecycle = self.lifecycle.lock().await;
        crate::registry::set_agents_server_url(&self.data_dir()?, server.as_ref())?;
        self.restart().await?;
        Ok(self.status.borrow().clone())
    }

    /// Redeem a code (or the link a QR carries) typed on this device. The
    /// token goes into the keychain and nowhere else.
    pub async fn pair_redeem(&self, raw: &str) -> Result<HqStatusVm, HqError> {
        let read = pairing::parse(raw).map_err(sentence)?;
        let server = match (read.server, lock(&self.inner).server.clone()) {
            (Some(linked), _) => linked,
            (None, Some(stored)) => stored,
            (None, None) => {
                return Err(HqError::Sentence(
                    "Set your agents server's address first; the code is for that server."
                        .to_owned(),
                ))
            }
        };
        self.redeem(server, read.code).await
    }

    async fn redeem(&self, server: ServerUrl, code: PairingCode) -> Result<HqStatusVm, HqError> {
        let generation = lock(&self.inner).generation;
        let host = (self.device_host)();
        let redeemed = {
            let _contact = self.contacts.hold(&server);
            self.routes
                .redeem(&server, &code, &self.device_name, &host)
                .await
                .map_err(|error| match error {
                    RouteError::Unauthorized => HqError::Sentence(
                        "That code is not valid any more: it was used, or ten minutes passed. \
                         Ask for a new one."
                            .to_owned(),
                    ),
                    other => HqError::Sentence(route_sentence(&server, &other)),
                })?
        };
        let _lifecycle = self.lifecycle.lock().await;
        if lock(&self.inner).generation != generation {
            return Err(HqError::Sentence(
                "The agents server changed while this device was pairing, so nothing was \
                 saved. Pair again."
                    .to_owned(),
            ));
        }
        let data_dir = self.data_dir()?;
        let record = Pairing {
            token: redeemed.token,
            actor_id: redeemed.discovery.actor_id,
            client_id: redeemed.discovery.client_id,
            name: self.device_name.clone(),
            bound: None,
        };
        let stored = serde_json::to_string(&record).map_err(sentence)?;
        self.platform
            .keychain_set(&keychain_key(&server), &stored)?;
        Authorization {
            removed: false,
            open: redeemed.discovery.databases.clone(),
        }
        .save(&data_dir, &server)?;
        crate::registry::set_agents_server_url(&data_dir, Some(&server))?;
        {
            let mut inner = lock(&self.inner);
            inner.refusals.clear();
            inner.offer = None;
        }
        tracing::info!(
            databases = redeemed.discovery.databases.len(),
            "this device is paired with its agents server"
        );
        self.restart().await?;
        Ok(self.status.borrow().clone())
    }

    /// A pairing link opened on this device (a scanned QR): checked here,
    /// kept, and shown for the person to confirm. Nothing is sent yet.
    pub fn offer_link(&self, raw: &str) -> Result<(), HqError> {
        let read = pairing::parse(raw).map_err(sentence)?;
        let Some(server) = read.server else {
            return Err(HqError::Sentence(
                pairing::CodeRefusal::NotACode.to_string(),
            ));
        };
        lock(&self.inner).offer = Some((server, read.code));
        self.publish();
        Ok(())
    }

    /// Pair with the offered link's server.
    pub async fn accept_offer(&self) -> Result<HqStatusVm, HqError> {
        let offer = lock(&self.inner).offer.take();
        self.publish();
        let Some((server, code)) = offer else {
            return Err(HqError::Sentence(
                "There is no pairing to confirm.".to_owned(),
            ));
        };
        self.redeem(server, code).await
    }

    /// Forget the offered link.
    pub fn dismiss_offer(&self) {
        lock(&self.inner).offer = None;
        self.publish();
    }

    /// A code another device of this person can redeem, with its QR.
    pub async fn pair_start(&self) -> Result<HqPairCodeVm, HqError> {
        let (server, pairing) = self.paired()?;
        let issued = {
            let _contact = self.contacts.hold(&server);
            self.routes
                .start(&server, &pairing.token)
                .await
                .map_err(|error| HqError::Sentence(route_sentence(&server, &error)))?
        };
        let link = pairing::pairing_link(&server, &issued.code);
        Ok(HqPairCodeVm {
            code: issued.code.grouped(),
            qr_svg: crate::bridges::login::qr_svg(&link),
            expires_at_ms: issued.expires_at_ms,
        })
    }

    fn paired(&self) -> Result<(ServerUrl, Pairing), HqError> {
        let inner = lock(&self.inner);
        match (&inner.server, &inner.pairing, inner.authorization.removed) {
            (Some(server), Some(pairing), false) => Ok((server.clone(), pairing.clone())),
            (Some(server), Some(_), true) => Err(HqError::Sentence(phase_sentence(
                HqPhase::Removed,
                Some(server.host()),
                0,
            ))),
            _ => Err(HqError::Sentence(
                "This device is not paired with an agents server.".to_owned(),
            )),
        }
    }

    /// Remove one of the person's devices from the server, on every
    /// database. The Devices list follows when the directory replicates.
    pub async fn revoke(&self, client_id: &str) -> Result<HqStatusVm, HqError> {
        let (server, pairing) = self.paired()?;
        {
            let _contact = self.contacts.hold(&server);
            self.routes
                .revoke(&server, &pairing.token, client_id)
                .await
                .map_err(|error| HqError::Sentence(route_sentence(&server, &error)))?;
        }
        tracing::info!(
            this_device = client_id == pairing.client_id,
            "a device was removed from the agents server"
        );
        let replicas: Vec<Arc<dyn Replica>> = lock(&self.inner)
            .dbs
            .iter()
            .map(|db| Arc::clone(&db.replica))
            .collect();
        for replica in replicas {
            replica.sync_now();
        }
        Ok(self.status.borrow().clone())
    }

    /// Forget this server: its replicas leave the disk, then its pairing
    /// leaves the keychain. What waits in the local replica stays. A copy
    /// that cannot be deleted is said, and the pairing is kept with it.
    pub async fn forget(&self) -> Result<HqStatusVm, HqError> {
        let _lifecycle = self.lifecycle.lock().await;
        let data_dir = self.data_dir()?;
        let (server, retired) = {
            let mut inner = lock(&self.inner);
            inner.generation += 1;
            (inner.server.clone(), Self::retire_all(&mut inner))
        };
        retire(retired).await;
        let Some(server) = server else {
            self.restart().await?;
            return Ok(self.status.borrow().clone());
        };
        let dir = databases::server_dir(&data_dir, &server);
        let removed = match std::fs::remove_dir_all(&dir) {
            Err(error) if error.kind() != std::io::ErrorKind::NotFound => Err(error),
            _ => Ok(()),
        };
        if let Err(error) = removed {
            tracing::warn!(%error, "the agents server's copies could not be deleted");
            self.restart().await?;
            return Err(HqError::Sentence(format!(
                "keeper could not delete this server's copies in {}: {error}. They are still \
                 on this device, and so is the pairing.",
                dir.display()
            )));
        }
        self.platform.keychain_delete(&keychain_key(&server))?;
        if let Err(error) = databases::forget_server(&data_dir, &server) {
            tracing::warn!(%error, "keeper could not forget which agents server it synced with");
        }
        lock(&self.inner).refusals.clear();
        self.restart().await?;
        Ok(self.status.borrow().clone())
    }
}

/// The person's devices from every replica's `clients` rows.
fn devices(replicas: &[Arc<dyn Replica>], actor: &str, me: &str) -> Vec<HqDeviceVm> {
    let mut devices: Vec<HqDeviceVm> = Vec::new();
    for replica in replicas {
        let rows = match replica.query(
            "SELECT id, name, created_at_ms, revoked_at_ms FROM clients \
             WHERE actor_id = ?1 AND kind = 'device' ORDER BY created_at_ms, id",
            &[Value::from(actor)],
        ) {
            Ok(rows) => rows,
            Err(error) => {
                tracing::debug!(%error, "an agents replica's directory could not be read");
                continue;
            }
        };
        for row in rows {
            let Some(id) = row.get("id").and_then(Value::as_str) else {
                continue;
            };
            let revoked = row.get("revoked_at_ms").is_some_and(|v| !v.is_null());
            if let Some(known) = devices.iter_mut().find(|d| d.client_id == id) {
                known.revoked |= revoked;
                continue;
            }
            devices.push(HqDeviceVm {
                client_id: id.to_owned(),
                name: row
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or(id)
                    .to_owned(),
                added_at_ms: row.get("created_at_ms").and_then(Value::as_i64),
                this_device: id == me,
                revoked,
            });
        }
    }
    devices
}

#[cfg(test)]
mod signin_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod window_tests;
