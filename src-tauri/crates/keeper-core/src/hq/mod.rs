//! The agents' database (AD-418, AD-421, AD-422; FR-845, FR-846, FR-871):
//! keeper as a client of the syncular server the person runs.
//!
//! keeper ships no server address (D-4). The person sets `agents.server_url`
//! to the server's base URL; a device pairs by redeeming a code another of
//! their devices shows ([`pairing`]); the token it receives lives in the
//! platform keychain and nowhere else — never in `keeper.db`, a settings
//! file, a view model or a log line. `GET /databases` says which databases the
//! token opens, and the device holds one replica of each ([`databases`]),
//! each with its own socket at `<base>/<database>/…` and its own owner thread
//! ([`syncular`]). With no address, or no pairing, everything written waits in
//! a local replica's outbox and keeper makes no request to any agents server.
//!
//! A revoked device's next round is refused; its replicas stop syncing, say
//! so, send nothing more, and stay readable until the person removes them.
//!
//! [`Hq`] owns all of it; the shell's `hq_ipc` is a call site and decides
//! nothing. Its tracing targets are `keeper_core::hq::*` and carry no row
//! content.

pub mod databases;
pub mod pairing;
pub mod port;
pub mod routes;
pub mod server;
pub mod syncular;
pub mod vm;

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::{watch, Mutex};

use crate::error::CoreError;
use crate::hq::port::{Link, LinkState, Opener, Replica, ReplicaSpec};
use crate::hq::routes::RouteError;
use crate::hq::server::ServerUrl;
use crate::hq::vm::{
    database_vm, phase_sentence, HqDeviceVm, HqPairCodeVm, HqPhase, HqStatusVm, HqThisDeviceVm,
};
use crate::platform::{exclude_from_backup_best_effort, Platform};

/// The keychain item of a server's pairing: this prefix and the base URL.
const KEYCHAIN_PREFIX: &str = "agents-server/";

/// The directory subscriptions every replica holds: the database's actors
/// and clients (the person's devices among them), by `dir` (`syncular.json`).
const DIRECTORY: [(&str, &str); 2] = [
    ("directoryActors", "actors"),
    ("directoryClients", "clients"),
];

/// The longest wait between two discoveries of a server that cannot be
/// reached.
const MAX_DISCOVERY_BACKOFF: Duration = Duration::from_secs(60);

/// Why an hq call failed. `Sentence` is shown to the person as it is.
#[derive(Debug, thiserror::Error)]
pub enum HqError {
    #[error("{0}")]
    Sentence(String),
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
}

impl std::fmt::Debug for Pairing {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Pairing")
            .field("token", &"<redacted>")
            .field("actor_id", &self.actor_id)
            .field("client_id", &self.client_id)
            .finish()
    }
}

struct Db {
    name: String,
    replica: Arc<dyn Replica>,
    /// The server's last `GET /databases` listed it (or has not answered).
    open: bool,
    /// Whether `replica` syncs (it is reopened local-only when not).
    linked: bool,
}

#[derive(Default)]
struct Inner {
    server: Option<ServerUrl>,
    /// `agents.server_url` holds text the grammar refuses.
    address_refused: bool,
    pairing: Option<Pairing>,
    removed: bool,
    local: Option<Arc<dyn Replica>>,
    databases: Vec<Db>,
    /// Bumped on every (re)start, so a discovery of an older start lands
    /// nowhere.
    generation: u64,
    discovering: bool,
}

impl Inner {
    fn close_all(&mut self) {
        for db in self.databases.drain(..) {
            db.replica.close();
        }
        if let Some(local) = self.local.take() {
            local.close();
        }
    }
}

/// The agents server, this device's pairing with it, and every replica.
pub struct Hq {
    platform: Arc<dyn Platform>,
    opener: Arc<dyn Opener>,
    http: reqwest::Client,
    device_name: String,
    inner: Mutex<Inner>,
    status: watch::Sender<HqStatusVm>,
}

fn no_server() -> HqStatusVm {
    HqStatusVm {
        server: None,
        phase: HqPhase::NoServer,
        sentence: phase_sentence(HqPhase::NoServer, None, 0),
        device: None,
        databases: Vec::new(),
        waiting_here: 0,
    }
}

fn keychain_key(server: &ServerUrl) -> String {
    format!("{KEYCHAIN_PREFIX}{}", server.as_str())
}

fn route_sentence(server: &ServerUrl, error: &RouteError) -> String {
    match error {
        RouteError::Unauthorized => "The server does not know this device.".to_owned(),
        RouteError::Forbidden(code) => format!("{} refused: {code}.", server.host()),
        RouteError::Unreachable(why) => format!("keeper could not reach {}: {why}", server.host()),
        RouteError::Unexpected(why) => {
            format!(
                "{} answered something keeper cannot read: {why}",
                server.host()
            )
        }
    }
}

impl Hq {
    /// A service with nothing open yet; [`Hq::start`] reads the setting.
    /// `device_name` is how the person's other devices will list this one.
    pub fn new(
        platform: Arc<dyn Platform>,
        opener: Arc<dyn Opener>,
        device_name: String,
    ) -> Arc<Hq> {
        let (status, _) = watch::channel(no_server());
        Arc::new(Hq {
            platform,
            opener,
            http: routes::client(),
            device_name,
            inner: Mutex::new(Inner::default()),
            status,
        })
    }

    /// The status, now and at every change.
    pub fn status(&self) -> watch::Receiver<HqStatusVm> {
        self.status.subscribe()
    }

    /// The replica of `database`, when this device holds one.
    pub async fn replica(&self, database: &str) -> Option<Arc<dyn Replica>> {
        let inner = self.inner.lock().await;
        inner
            .databases
            .iter()
            .find(|db| db.name == database)
            .map(|db| Arc::clone(&db.replica))
    }

    /// The replica of what is written while no server is paired.
    pub async fn local(&self) -> Option<Arc<dyn Replica>> {
        self.inner.lock().await.local.clone()
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

    async fn open(&self, spec: ReplicaSpec) -> Result<Arc<dyn Replica>, HqError> {
        if let Some(parent) = spec.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let opener = Arc::clone(&self.opener);
        let database = spec.database.clone();
        tokio::task::spawn_blocking(move || opener.open(spec))
            .await
            .map_err(|error| HqError::Sentence(format!("opening {database}: {error}")))?
            .map_err(|error| HqError::Sentence(format!("opening {database}: {error}")))
    }

    fn spec(
        &self,
        data_dir: &Path,
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
        }
    }

    /// Read `agents.server_url` and the pairing, and open every replica the
    /// device holds: the ones on disk first, so reads answer before the
    /// server does, then whatever `GET /databases` adds.
    pub async fn start(self: &Arc<Self>) -> Result<(), HqError> {
        let data_dir = self.data_dir()?;
        let stored = crate::registry::get_agents_server_url(&data_dir)?;
        let mut inner = self.inner.lock().await;
        inner.close_all();
        inner.generation += 1;
        inner.removed = false;
        inner.discovering = false;
        (inner.server, inner.address_refused) = match stored.as_deref().map(ServerUrl::parse) {
            None | Some(Ok(None)) => (None, false),
            Some(Ok(Some(server))) => (Some(server), false),
            Some(Err(_)) => (None, true),
        };
        inner.pairing = match &inner.server {
            Some(server) => self.read_pairing(server)?,
            None => None,
        };
        let root = databases::root(&data_dir);
        std::fs::create_dir_all(&root)?;
        exclude_from_backup_best_effort(self.platform.as_ref(), &root);

        let local = databases::local_replica(&data_dir);
        if inner.pairing.is_none() || local.is_file() {
            inner.local = Some(
                self.open(ReplicaSpec {
                    database: "local".to_owned(),
                    path: local,
                    client_id: None,
                    link: None,
                })
                .await?,
            );
        }
        if let (Some(server), Some(pairing)) = (inner.server.clone(), inner.pairing.clone()) {
            for name in databases::held(&data_dir, &server, &pairing.actor_id) {
                let replica = self
                    .open(self.spec(&data_dir, &server, &pairing, &name, true))
                    .await?;
                self.watch_replica(&replica, inner.generation);
                inner.databases.push(Db {
                    name,
                    replica,
                    open: true,
                    linked: true,
                });
            }
            inner.discovering = true;
            let hq = Arc::clone(self);
            let generation = inner.generation;
            tokio::spawn(async move { hq.discover(generation).await });
        }
        if let Some(local) = &inner.local {
            self.watch_replica(local, inner.generation);
        }
        self.publish(&inner);
        Ok(())
    }

    /// Recompute the status whenever `replica` publishes, and ask the server
    /// about a refusal once.
    fn watch_replica(self: &Arc<Self>, replica: &Arc<dyn Replica>, generation: u64) {
        let mut states = replica.watch();
        let hq = Arc::clone(self);
        tokio::spawn(async move {
            while states.changed().await.is_ok() {
                let refused = states.borrow_and_update().link == LinkState::Refused;
                let mut inner = hq.inner.lock().await;
                if inner.generation != generation {
                    return;
                }
                hq.publish(&inner);
                if refused && !inner.discovering && !inner.removed {
                    inner.discovering = true;
                    let again = Arc::clone(&hq);
                    tokio::spawn(async move { again.discover(generation).await });
                }
            }
        });
    }

    /// `GET /databases` until it answers or a newer start replaces this one,
    /// then open what it lists and stop syncing what it no longer does.
    async fn discover(self: Arc<Self>, generation: u64) {
        let mut failures: u32 = 0;
        loop {
            let (server, pairing) = {
                let inner = self.inner.lock().await;
                if inner.generation != generation {
                    return;
                }
                match (inner.server.clone(), inner.pairing.clone()) {
                    (Some(server), Some(pairing)) => (server, pairing),
                    _ => return,
                }
            };
            let answer = routes::discover(&self.http, &server, &pairing.token).await;
            match answer {
                Err(RouteError::Unreachable(why)) | Err(RouteError::Unexpected(why)) => {
                    failures = failures.saturating_add(1);
                    let wait = syncular::backoff(failures).min(MAX_DISCOVERY_BACKOFF);
                    tracing::info!(%why, retry_in_ms = wait.as_millis() as u64, "agents server discovery failed");
                    tokio::time::sleep(wait).await;
                }
                other => {
                    if let Err(error) = self.apply_discovery(generation, other).await {
                        tracing::warn!(%error, "agents server discovery could not be applied");
                    }
                    return;
                }
            }
        }
    }

    async fn apply_discovery(
        self: &Arc<Self>,
        generation: u64,
        answer: Result<routes::Discovery, RouteError>,
    ) -> Result<(), HqError> {
        let data_dir = self.data_dir()?;
        let mut inner = self.inner.lock().await;
        if inner.generation != generation {
            return Ok(());
        }
        inner.discovering = false;
        let (Some(server), Some(pairing)) = (inner.server.clone(), inner.pairing.clone()) else {
            return Ok(());
        };
        let open: Vec<String> = match answer {
            Ok(found) => {
                tracing::info!(
                    databases = found.databases.len(),
                    "agents server discovered"
                );
                inner.removed = false;
                found.databases
            }
            Err(RouteError::Unauthorized) => {
                tracing::warn!(
                    "the agents server no longer knows this device; every replica stops syncing"
                );
                inner.removed = true;
                Vec::new()
            }
            Err(error) => {
                tracing::warn!(%error, "agents server discovery refused");
                return Ok(());
            }
        };
        let held: Vec<String> = inner.databases.iter().map(|db| db.name.clone()).collect();
        let mut next: Vec<Db> = Vec::new();
        for (name, is_open) in databases::plan(&held, &open) {
            let existing = inner
                .databases
                .iter()
                .position(|db| db.name == name)
                .map(|at| inner.databases.remove(at));
            let db = match existing {
                Some(db) if db.linked == is_open => Db {
                    open: is_open,
                    ..db
                },
                Some(db) => {
                    db.replica.close();
                    self.reopen(&data_dir, &server, &pairing, name, is_open, generation)
                        .await?
                }
                None => {
                    self.reopen(&data_dir, &server, &pairing, name, is_open, generation)
                        .await?
                }
            };
            if db.linked && db.replica.state().link == LinkState::Refused {
                // Listed again after a refusal: try once more from a fresh owner.
                db.replica.close();
                next.push(
                    self.reopen(&data_dir, &server, &pairing, db.name, true, generation)
                        .await?,
                );
            } else {
                next.push(db);
            }
        }
        inner.databases = next;
        self.publish(&inner);
        Ok(())
    }

    async fn reopen(
        self: &Arc<Self>,
        data_dir: &Path,
        server: &ServerUrl,
        pairing: &Pairing,
        name: String,
        linked: bool,
        generation: u64,
    ) -> Result<Db, HqError> {
        let replica = self
            .open(self.spec(data_dir, server, pairing, &name, linked))
            .await?;
        if linked {
            let subscriber = Arc::clone(&replica);
            let database = name.clone();
            tokio::task::spawn_blocking(move || {
                for (id, table) in DIRECTORY {
                    subscriber.subscribe(
                        id,
                        table,
                        vec![("dir".to_owned(), vec![database.clone()])],
                    )?;
                }
                Ok::<(), port::ReplicaError>(())
            })
            .await
            .map_err(|error| HqError::Sentence(error.to_string()))?
            .map_err(|error| HqError::Sentence(error.to_string()))?;
        }
        self.watch_replica(&replica, generation);
        Ok(Db {
            name,
            replica,
            open: linked,
            linked,
        })
    }

    fn publish(&self, inner: &Inner) {
        let waiting_here = inner.local.as_ref().map_or(0, |local| {
            u32::try_from(local.state().outbox).unwrap_or(u32::MAX)
        });
        let host = inner.server.as_ref().map(ServerUrl::host);
        let phase = if inner.address_refused {
            HqPhase::Refused
        } else if inner.server.is_none() {
            HqPhase::NoServer
        } else if inner.pairing.is_none() {
            HqPhase::NotPaired
        } else if inner.removed {
            HqPhase::Removed
        } else {
            HqPhase::Paired
        };
        let status = HqStatusVm {
            server: inner.server.as_ref().map(|s| s.as_str().to_owned()),
            phase,
            sentence: phase_sentence(phase, host, waiting_here),
            device: inner.pairing.as_ref().map(|pairing| HqThisDeviceVm {
                client_id: pairing.client_id.clone(),
                name: pairing.name.clone(),
            }),
            databases: inner
                .databases
                .iter()
                .map(|db| {
                    let state = db.replica.state();
                    database_vm(&db.name, state.link, state.outbox, db.open, inner.removed)
                })
                .collect(),
            waiting_here,
        };
        self.status.send_if_modified(|current| {
            let changed = *current != status;
            *current = status;
            changed
        });
    }

    /// Set `agents.server_url` and start over on it. A refused address is
    /// returned as its sentence and nothing is stored; blank clears it.
    pub async fn set_server(self: &Arc<Self>, raw: &str) -> Result<HqStatusVm, HqError> {
        let server = ServerUrl::parse(raw).map_err(|why| HqError::Sentence(why.to_string()))?;
        crate::registry::set_agents_server_url(&self.data_dir()?, server.as_ref())?;
        self.start().await?;
        Ok(self.status.borrow().clone())
    }

    /// Redeem a code (or the link a QR carries) typed on this device. The
    /// token goes into the keychain and nowhere else.
    pub async fn pair_redeem(self: &Arc<Self>, raw: &str) -> Result<HqStatusVm, HqError> {
        let read = pairing::parse(raw).map_err(|why| HqError::Sentence(why.to_string()))?;
        let data_dir = self.data_dir()?;
        let server = match (&read.server, self.inner.lock().await.server.clone()) {
            (Some(linked), _) => linked.clone(),
            (None, Some(stored)) => stored,
            (None, None) => {
                return Err(HqError::Sentence(
                    "Set your agents server's address first; the code is for that server."
                        .to_owned(),
                ))
            }
        };
        let redeemed = routes::redeem(&self.http, &server, &read.code, &self.device_name)
            .await
            .map_err(|error| match error {
                RouteError::Unauthorized => HqError::Sentence(
                    "That code is not valid any more: it was used, or ten minutes passed. Ask \
                     for a new one."
                        .to_owned(),
                ),
                other => HqError::Sentence(route_sentence(&server, &other)),
            })?;
        let record = Pairing {
            token: redeemed.token,
            actor_id: redeemed.discovery.actor_id,
            client_id: redeemed.discovery.client_id,
            name: self.device_name.clone(),
        };
        let stored =
            serde_json::to_string(&record).map_err(|error| HqError::Sentence(error.to_string()))?;
        self.platform
            .keychain_set(&keychain_key(&server), &stored)?;
        crate::registry::set_agents_server_url(&data_dir, Some(&server))?;
        tracing::info!(
            databases = redeemed.discovery.databases.len(),
            "this device is paired with its agents server"
        );
        self.start().await?;
        Ok(self.status.borrow().clone())
    }

    /// A code another device of this person can redeem, with its QR.
    pub async fn pair_start(&self) -> Result<HqPairCodeVm, HqError> {
        let (server, pairing) = self.paired().await?;
        let issued = routes::start(&self.http, &server, &pairing.token)
            .await
            .map_err(|error| HqError::Sentence(route_sentence(&server, &error)))?;
        let link = pairing::pairing_link(&server, &issued.code);
        Ok(HqPairCodeVm {
            code: issued.code.grouped(),
            qr_svg: crate::bridges::login::qr_svg(&link),
            expires_at_ms: issued.expires_at_ms,
        })
    }

    async fn paired(&self) -> Result<(ServerUrl, Pairing), HqError> {
        let inner = self.inner.lock().await;
        match (&inner.server, &inner.pairing, inner.removed) {
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

    /// The person's devices, from every database's `clients` rows.
    pub async fn devices(&self) -> Result<Vec<HqDeviceVm>, HqError> {
        let (replicas, pairing) = {
            let inner = self.inner.lock().await;
            let Some(pairing) = inner.pairing.clone() else {
                return Ok(Vec::new());
            };
            let replicas: Vec<Arc<dyn Replica>> = inner
                .databases
                .iter()
                .map(|db| Arc::clone(&db.replica))
                .collect();
            (replicas, pairing)
        };
        tokio::task::spawn_blocking(move || {
            let mut devices: Vec<HqDeviceVm> = Vec::new();
            for replica in replicas {
                let rows = replica
                    .query(
                        "SELECT id, name, created_at_ms, revoked_at_ms FROM clients \
                         WHERE actor_id = ?1 AND kind = 'device' ORDER BY created_at_ms, id",
                        &[Value::from(pairing.actor_id.clone())],
                    )
                    .map_err(|error| HqError::Sentence(error.to_string()))?;
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
                        this_device: id == pairing.client_id,
                        revoked,
                    });
                }
            }
            Ok(devices)
        })
        .await
        .map_err(|error| HqError::Sentence(error.to_string()))?
    }

    /// Remove one of the person's devices from the server, on every database.
    pub async fn revoke(&self, client_id: &str) -> Result<Vec<HqDeviceVm>, HqError> {
        let (server, pairing) = self.paired().await?;
        routes::revoke(&self.http, &server, &pairing.token, client_id)
            .await
            .map_err(|error| HqError::Sentence(route_sentence(&server, &error)))?;
        tracing::info!(
            this_device = client_id == pairing.client_id,
            "a device was removed from the agents server"
        );
        {
            let inner = self.inner.lock().await;
            for db in &inner.databases {
                db.replica.sync_now();
            }
        }
        self.devices().await
    }

    /// Forget this server: its pairing leaves the keychain and its replicas
    /// leave the disk. What waits in the local replica stays.
    pub async fn forget(self: &Arc<Self>) -> Result<HqStatusVm, HqError> {
        let data_dir = self.data_dir()?;
        let server = {
            let mut inner = self.inner.lock().await;
            inner.close_all();
            inner.server.clone()
        };
        if let Some(server) = server {
            self.platform.keychain_delete(&keychain_key(&server))?;
            let dir = databases::server_dir(&data_dir, &server);
            // The owner threads close their files as they stop.
            for _ in 0..50 {
                match std::fs::remove_dir_all(&dir) {
                    Ok(()) => break,
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => break,
                    Err(_) => tokio::time::sleep(Duration::from_millis(20)).await,
                }
            }
        }
        self.start().await?;
        Ok(self.status.borrow().clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::forges::testing::FakePlatform;
    use crate::hq::port::fake::FakeOpener;
    use crate::hq::port::RowOp;

    fn hq(platform: &Arc<FakePlatform>, opener: &Arc<FakeOpener>) -> Arc<Hq> {
        Hq::new(
            Arc::clone(platform) as Arc<dyn Platform>,
            Arc::clone(opener) as Arc<dyn Opener>,
            "keeper on test".to_owned(),
        )
    }

    #[tokio::test]
    async fn with_no_address_writes_wait_in_a_local_replica_and_nothing_is_linked() {
        let platform = Arc::new(FakePlatform::default());
        let opener = Arc::new(FakeOpener::default());
        let hq = hq(&platform, &opener);
        hq.start().await.expect("starts");
        let status = hq.status().borrow().clone();
        assert_eq!(status.phase, HqPhase::NoServer);
        assert!(status.databases.is_empty());
        let opened = opener.opened.lock().expect("opened").clone();
        assert_eq!(opened.len(), 1);
        assert!(opened[0].spec.link.is_none(), "no replica is linked");

        let local = hq.local().await.expect("a local replica");
        local
            .mutate(vec![RowOp::Upsert {
                table: "messages".to_owned(),
                values: serde_json::Map::new(),
                base_version: None,
            }])
            .expect("commits");
        let mut status = hq.status();
        let waited = tokio::time::timeout(
            Duration::from_secs(2),
            status.wait_for(|status| status.waiting_here == 1),
        )
        .await;
        assert!(waited.is_ok(), "the outbox count reaches the status");
        assert!(hq
            .status()
            .borrow()
            .sentence
            .ends_with("1 change waits on this device."));
    }

    #[tokio::test]
    async fn a_refused_address_is_not_stored_and_a_good_one_without_a_pairing_contacts_nothing() {
        let platform = Arc::new(FakePlatform::default());
        let opener = Arc::new(FakeOpener::default());
        let hq = hq(&platform, &opener);
        hq.start().await.expect("starts");
        let refused = hq.set_server("agents.example.org").await;
        assert!(matches!(refused, Err(HqError::Sentence(_))));
        let data_dir = platform.data_dir().expect("data dir");
        assert_eq!(
            crate::registry::get_agents_server_url(&data_dir).expect("reads"),
            None
        );

        let status = hq
            .set_server("https://agents.example.org/")
            .await
            .expect("stores");
        assert_eq!(status.phase, HqPhase::NotPaired);
        assert_eq!(status.server.as_deref(), Some("https://agents.example.org"));
        assert_eq!(
            crate::registry::get_agents_server_url(&data_dir)
                .expect("reads")
                .as_deref(),
            Some("https://agents.example.org")
        );
        assert!(opener
            .opened
            .lock()
            .expect("opened")
            .iter()
            .all(|replica| replica.spec.link.is_none()));

        let cleared = hq.set_server(" ").await.expect("clears");
        assert_eq!(cleared.phase, HqPhase::NoServer);
    }

    #[tokio::test]
    async fn a_paired_device_opens_the_replicas_it_holds_before_the_server_answers() {
        let platform = Arc::new(FakePlatform::default());
        let opener = Arc::new(FakeOpener::default());
        let data_dir = platform.data_dir().expect("data dir");
        // A port nothing listens on: discovery cannot answer.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("binds");
        let server = ServerUrl::parse(&format!(
            "http://127.0.0.1:{}",
            listener.local_addr().expect("addr").port()
        ))
        .expect("parses")
        .expect("an address");
        drop(listener);
        crate::registry::set_agents_server_url(&data_dir, Some(&server)).expect("stores");
        let pairing = Pairing {
            token: "0123456789abcdef".to_owned(),
            actor_id: "tgorka".to_owned(),
            client_id: "tgorka.0a1b".to_owned(),
            name: "keeper on test".to_owned(),
        };
        platform
            .keychain_set(
                &keychain_key(&server),
                &serde_json::to_string(&pairing).expect("json"),
            )
            .expect("keychain");
        for db in ["tgdrive", "neuradrive"] {
            let path = databases::replica(&data_dir, &server, db, "tgorka");
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, b"").expect("write");
        }
        let hq = hq(&platform, &opener);
        hq.start().await.expect("starts");
        let status = hq.status().borrow().clone();
        assert_eq!(status.phase, HqPhase::Paired);
        assert_eq!(
            status
                .databases
                .iter()
                .map(|db| db.name.as_str())
                .collect::<Vec<_>>(),
            ["neuradrive", "tgdrive"]
        );
        assert_eq!(
            status.device.as_ref().map(|d| d.client_id.as_str()),
            Some("tgorka.0a1b")
        );
        let opened = opener.opened.lock().expect("opened").clone();
        let linked: Vec<_> = opened.iter().filter_map(|r| r.spec.link.clone()).collect();
        assert_eq!(linked.len(), 2);
        assert_eq!(linked[0].base_url, server.mount("neuradrive"));
        // The token reached the replica's link and no view model.
        assert!(!serde_json::to_string(&status)
            .expect("json")
            .contains("01234567"));
        // Not even a refusal stops the replicas answering reads.
        opened[0].set(LinkState::Offline);
        let mut watch = hq.status();
        let offline = tokio::time::timeout(
            Duration::from_secs(2),
            watch.wait_for(|s| {
                s.databases
                    .iter()
                    .any(|db| db.state == vm::HqDatabaseState::Offline)
            }),
        )
        .await;
        assert!(offline.is_ok());
        assert!(hq.replica("neuradrive").await.is_some());
    }

    #[tokio::test]
    async fn a_revoked_device_stops_syncing_and_keeps_its_replicas_readable() {
        let platform = Arc::new(FakePlatform::default());
        let opener = Arc::new(FakeOpener::default());
        let data_dir = platform.data_dir().expect("data dir");
        let server = ServerUrl::parse("http://127.0.0.1:9")
            .expect("parses")
            .expect("an address");
        let pairing = Pairing {
            token: "t".to_owned(),
            actor_id: "tgorka".to_owned(),
            client_id: "tgorka.1".to_owned(),
            name: "n".to_owned(),
        };
        crate::registry::set_agents_server_url(&data_dir, Some(&server)).expect("stores");
        platform
            .keychain_set(
                &keychain_key(&server),
                &serde_json::to_string(&pairing).expect("json"),
            )
            .expect("keychain");
        let hq = hq(&platform, &opener);
        {
            let mut inner = hq.inner.lock().await;
            inner.server = Some(server.clone());
            inner.pairing = Some(pairing.clone());
            inner.generation = 7;
        }
        hq.apply_discovery(
            7,
            Ok(routes::Discovery {
                actor_id: "tgorka".to_owned(),
                client_id: "tgorka.1".to_owned(),
                databases: vec!["tgdrive".to_owned(), "neuradrive".to_owned()],
            }),
        )
        .await
        .expect("applies");
        assert_eq!(hq.status().borrow().databases.len(), 2);

        // The server drops neuradrive: it stops syncing, and is still shown.
        hq.apply_discovery(
            7,
            Ok(routes::Discovery {
                actor_id: "tgorka".to_owned(),
                client_id: "tgorka.1".to_owned(),
                databases: vec!["tgdrive".to_owned()],
            }),
        )
        .await
        .expect("applies");
        let status = hq.status().borrow().clone();
        assert_eq!(status.databases[1].name, "neuradrive");
        assert_eq!(status.databases[1].state, vm::HqDatabaseState::NotOpen);
        let neuradrive = hq.replica("neuradrive").await.expect("held");
        assert_eq!(
            neuradrive.state().link,
            LinkState::Local,
            "reopened without a link"
        );

        // Then the device is removed.
        hq.apply_discovery(7, Err(RouteError::Unauthorized))
            .await
            .expect("applies");
        let status = hq.status().borrow().clone();
        assert_eq!(status.phase, HqPhase::Removed);
        assert!(status
            .databases
            .iter()
            .all(|db| db.state == vm::HqDatabaseState::Removed));
        let opened = opener.opened.lock().expect("opened").clone();
        let live: Vec<_> = opened
            .iter()
            .filter(|r| !*r.closed.lock().expect("closed"))
            .collect();
        assert_eq!(live.len(), 2);
        assert!(live.iter().all(|r| r.spec.link.is_none()), "nothing syncs");
        assert!(matches!(hq.pair_start().await, Err(HqError::Sentence(_))));
    }
}
