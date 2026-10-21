use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::forges::testing::FakePlatform;
use crate::hq::port::fake::{FakeOpener, FakeReplica};
use crate::hq::port::RowOp;
use crate::hq::routes::{Answer, Discovery, Issued, Redeemed};
use crate::hq::vm::HqDatabaseState;

/// Root routes that answer what the test says, counting what was asked.
#[derive(Default)]
struct FakeRoutes {
    discovered: AtomicUsize,
    discovery: Mutex<Option<Result<Discovery, RouteError>>>,
    redeemed: Mutex<Vec<String>>,
    /// The next redeem waits until this is notified.
    redeem_gate: Mutex<Option<Arc<tokio::sync::Notify>>>,
}

fn found(databases: &[&str]) -> Discovery {
    Discovery {
        actor_id: "ana".to_owned(),
        client_id: "ana.1".to_owned(),
        databases: databases.iter().map(|d| (*d).to_owned()).collect(),
    }
}

impl Routes for FakeRoutes {
    fn discover<'a>(&'a self, _: &'a ServerUrl, _: &'a str) -> Answer<'a, Discovery> {
        self.discovered.fetch_add(1, Ordering::SeqCst);
        let answer = lock(&self.discovery)
            .clone()
            .unwrap_or_else(|| Err(RouteError::Unreachable("no answer".to_owned())));
        Box::pin(async move { answer })
    }
    fn redeem<'a>(
        &'a self,
        server: &'a ServerUrl,
        _: &'a PairingCode,
        _: &'a str,
    ) -> Answer<'a, Redeemed> {
        lock(&self.redeemed).push(server.as_str().to_owned());
        let gate = lock(&self.redeem_gate).take();
        Box::pin(async move {
            if let Some(gate) = gate {
                gate.notified().await;
            }
            Ok(Redeemed {
                token: "0123456789abcdef".to_owned(),
                discovery: found(&["alpha", "beta"]),
            })
        })
    }
    fn start<'a>(&'a self, _: &'a ServerUrl, _: &'a str) -> Answer<'a, Issued> {
        Box::pin(async { Err(RouteError::Unreachable("unused".to_owned())) })
    }
    fn revoke<'a>(&'a self, _: &'a ServerUrl, _: &'a str, _: &'a str) -> Answer<'a, ()> {
        Box::pin(async { Ok(()) })
    }
}

struct World {
    platform: Arc<FakePlatform>,
    opener: Arc<FakeOpener>,
    routes: Arc<FakeRoutes>,
}

impl World {
    fn new() -> World {
        World {
            platform: Arc::new(FakePlatform::default()),
            opener: Arc::new(FakeOpener::default()),
            routes: Arc::new(FakeRoutes::default()),
        }
    }

    fn hq(&self) -> Arc<Hq> {
        Hq::new(
            Arc::clone(&self.platform) as Arc<dyn Platform>,
            Arc::clone(&self.opener) as Arc<dyn Opener>,
            Arc::clone(&self.routes) as Arc<dyn Routes>,
            "keeper on test".to_owned(),
        )
    }

    fn data_dir(&self) -> PathBuf {
        self.platform.data_dir().expect("data dir")
    }

    /// A paired device holding replicas of `databases` on `raw`.
    fn paired(&self, raw: &str, databases: &[&str]) -> ServerUrl {
        let server = ServerUrl::parse(raw).expect("parses").expect("an address");
        crate::registry::set_agents_server_url(&self.data_dir(), Some(&server)).expect("stores");
        let pairing = Pairing {
            token: "0123456789abcdef".to_owned(),
            actor_id: "ana".to_owned(),
            client_id: "ana.1".to_owned(),
            name: "keeper on test".to_owned(),
        };
        self.platform
            .keychain_set(
                &keychain_key(&server),
                &serde_json::to_string(&pairing).expect("json"),
            )
            .expect("keychain");
        for db in databases {
            let path = databases::replica(&self.data_dir(), &server, db, "ana");
            std::fs::create_dir_all(path.parent().expect("parent")).expect("mkdir");
            std::fs::write(&path, b"").expect("write");
        }
        server
    }

    fn live(&self, database: &str) -> Vec<Arc<FakeReplica>> {
        self.opener
            .live()
            .into_iter()
            .filter(|r| r.spec.database == database)
            .collect()
    }

    fn opened(&self, database: &str) -> usize {
        self.opener
            .all()
            .iter()
            .filter(|r| r.spec.database == database)
            .count()
    }
}

async fn status_until(hq: &Hq, what: &str, ok: impl Fn(&HqStatusVm) -> bool) -> HqStatusVm {
    let mut watch = hq.status();
    let reached = tokio::time::timeout(Duration::from_secs(3), watch.wait_for(|s| ok(s))).await;
    let status = hq.status().borrow().clone();
    assert!(reached.is_ok(), "{what}: {status:#?}");
    status
}

fn generation(hq: &Hq) -> u64 {
    lock(&hq.inner).generation
}

#[tokio::test]
async fn with_no_address_writes_wait_in_a_local_replica_and_nothing_is_asked() {
    let world = World::new();
    let hq = world.hq();
    hq.start().await.expect("starts");
    assert_eq!(hq.status().borrow().phase, HqPhase::NoServer);
    let opened = world.opener.all();
    assert_eq!(opened.len(), 1);
    assert!(opened[0].spec.link.is_none());
    hq.local()
        .expect("a local replica")
        .mutate(vec![RowOp::Delete {
            table: "messages".to_owned(),
            row_id: "m".to_owned(),
            base_version: None,
        }])
        .expect("commits");
    let status = status_until(&hq, "the outbox count", |s| s.waiting_here == 1).await;
    assert!(status.sentence.ends_with("1 change waits on this device."));
    assert_eq!(world.routes.discovered.load(Ordering::SeqCst), 0);
    assert!(hq.contacting().is_empty());
}

#[tokio::test]
async fn a_refused_address_is_not_stored_and_an_unpaired_one_contacts_nothing() {
    let world = World::new();
    let hq = world.hq();
    hq.start().await.expect("starts");
    assert!(matches!(
        hq.set_server("agents.example.org").await,
        Err(HqError::Sentence(_))
    ));
    assert_eq!(
        crate::registry::get_agents_server_url(&world.data_dir()).expect("reads"),
        None
    );
    let status = hq
        .set_server("https://agents.example.org/")
        .await
        .expect("stores");
    assert_eq!(status.phase, HqPhase::NotPaired);
    assert!(world.opener.all().iter().all(|r| r.spec.link.is_none()));
    assert_eq!(world.routes.discovered.load(Ordering::SeqCst), 0);
    assert!(hq.contacting().is_empty());
}

#[tokio::test]
async fn held_replicas_open_local_and_link_only_what_discovery_opens() {
    let world = World::new();
    let server = world.paired("https://hq.example.org", &["alpha", "beta"]);
    *lock(&world.routes.discovery) = Some(Ok(found(&["alpha"])));
    let hq = world.hq();
    hq.start().await.expect("starts");
    // Before any answer: both open, both local, and neither shown open.
    assert!(world.opener.all().iter().all(|r| r.spec.link.is_none()));
    let mut linked = false;
    for _ in 0..50 {
        linked = world.live("alpha").iter().any(|r| r.spec.link.is_some());
        if linked {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(linked, "alpha links");
    let status = status_until(&hq, "beta not open", |s| {
        s.databases
            .iter()
            .any(|d| d.name == "beta" && d.state == HqDatabaseState::NotOpen)
    })
    .await;
    assert_eq!(status.phase, HqPhase::Paired);
    let alpha = world.live("alpha");
    assert_eq!(alpha.len(), 1);
    assert_eq!(
        alpha[0].spec.link.as_ref().map(|l| l.base_url.clone()),
        Some(server.mount("alpha"))
    );
    assert!(world.live("beta")[0].spec.link.is_none());
    assert_eq!(hq.contacting(), [server.as_str().to_owned()]);
    assert!(!serde_json::to_string(&status)
        .expect("json")
        .contains("01234567"));
}

#[tokio::test]
async fn a_removed_device_stays_removed_across_a_restart_and_asks_nothing() {
    let world = World::new();
    world.paired("https://hq.example.org", &["alpha", "beta"]);
    *lock(&world.routes.discovery) = Some(Err(RouteError::Unauthorized));
    let hq = world.hq();
    hq.start().await.expect("starts");
    status_until(&hq, "removed", |s| s.phase == HqPhase::Removed).await;
    hq.shutdown().await;
    let asked = world.routes.discovered.load(Ordering::SeqCst);
    assert!(world.opener.live().is_empty(), "shutdown closed everything");

    let again = world.hq();
    again.start().await.expect("restarts");
    let status = again.status().borrow().clone();
    assert_eq!(status.phase, HqPhase::Removed);
    assert!(status
        .databases
        .iter()
        .all(|d| d.state == HqDatabaseState::Removed));
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(
        world.routes.discovered.load(Ordering::SeqCst),
        asked,
        "a removed device asked"
    );
    assert!(world.opener.live().iter().all(|r| r.spec.link.is_none()));
    assert!(again.contacting().is_empty());
}

#[tokio::test]
async fn a_refused_database_is_retried_on_a_backoff_that_outlives_its_replica() {
    let world = World::new();
    world.paired("https://hq.example.org", &["alpha"]);
    *lock(&world.routes.discovery) = Some(Ok(found(&["alpha"])));
    let hq = world.hq();
    hq.start().await.expect("starts");
    status_until(&hq, "linked", |_| {
        world.live("alpha").iter().any(|r| r.spec.link.is_some())
    })
    .await;
    let gen = generation(&hq);
    for streak in 1..=2u32 {
        let opens = world.opened("alpha");
        world.live("alpha")[0].set(LinkState::Refused);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(lock(&hq.inner).refusals["alpha"].streak, streak);
        // The server still lists it, yet nothing reopens before the backoff.
        hq.apply_discovery(gen, Ok(found(&["alpha"])))
            .await
            .expect("applies");
        hq.apply_discovery(gen, Ok(found(&["alpha"])))
            .await
            .expect("applies");
        assert_eq!(world.opened("alpha"), opens, "reopened during the backoff");
        // After it, the scheduled discovery links it once more.
        tokio::time::sleep(refusal_backoff(streak) + Duration::from_millis(400)).await;
        assert_eq!(world.opened("alpha"), opens + 1, "streak {streak}");
        assert!(world.live("alpha")[0].spec.link.is_some());
    }
    // An answered round ends the streak.
    world.live("alpha")[0].set(LinkState::Connected);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!lock(&hq.inner).refusals.contains_key("alpha"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_held_database_reads_while_another_one_opens() {
    let world = World::new();
    world.paired("https://hq.example.org", &["alpha"]);
    let hq = world.hq();
    hq.start().await.expect("starts");
    let gen = generation(&hq);
    // alpha is linked; then the server opens beta too, and beta's open stalls.
    hq.apply_discovery(gen, Ok(found(&["alpha"])))
        .await
        .expect("applies");
    let (release, held) = std::sync::mpsc::channel();
    lock(&world.opener.hold).insert("beta".to_owned(), held);
    let applying = {
        let hq = Arc::clone(&hq);
        tokio::spawn(async move { hq.apply_discovery(gen, Ok(found(&["alpha", "beta"]))).await })
    };
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(!applying.is_finished(), "beta's open is held");
    let read = tokio::time::timeout(Duration::from_millis(200), async {
        let alpha = hq.replica("alpha").expect("held");
        tokio::task::spawn_blocking(move || alpha.query("SELECT 1", &[])).await
    })
    .await;
    assert!(
        matches!(read, Ok(Ok(Ok(_)))),
        "a read waited on beta's open"
    );
    drop(release);
    applying.await.expect("joins").expect("applies");
    assert!(hq.replica("beta").is_some());
}

#[tokio::test]
async fn clearing_the_address_stops_every_replica_before_it_is_published() {
    let world = World::new();
    let server = world.paired("https://hq.example.org", &["alpha"]);
    *lock(&world.routes.discovery) = Some(Ok(found(&["alpha"])));
    let hq = world.hq();
    hq.start().await.expect("starts");
    status_until(&hq, "linked", |_| {
        world.live("alpha").iter().any(|r| r.spec.link.is_some())
    })
    .await;
    assert_eq!(hq.contacting(), [server.as_str().to_owned()]);
    let before = world.opener.all();
    let status = hq.set_server("").await.expect("clears");
    assert_eq!(status.phase, HqPhase::NoServer);
    for replica in &before {
        assert!(
            replica.closed() && replica.joined.load(Ordering::SeqCst),
            "{}",
            replica.spec.database
        );
    }
    assert!(hq.contacting().is_empty());
}

#[tokio::test]
async fn a_changed_setting_is_followed_and_an_unchanged_one_restarts_nothing() {
    let world = World::new();
    let hq = world.hq();
    hq.start().await.expect("starts");
    let gen = generation(&hq);
    hq.reconcile().await.expect("reconciles");
    assert_eq!(generation(&hq), gen, "nothing changed, nothing restarted");
    // An account layer or a pulled settings file changes the effective value.
    let server = ServerUrl::parse("https://hq.example.org")
        .expect("ok")
        .expect("some");
    crate::registry::set_agents_server_url(&world.data_dir(), Some(&server)).expect("stores");
    hq.reconcile().await.expect("reconciles");
    assert_eq!(
        hq.status().borrow().server.as_deref(),
        Some("https://hq.example.org")
    );
}

/// The view preferences are a person's settings: switched in a settings
/// file or synced from another device, they reach the open views on the
/// next reconcile.
#[tokio::test]
async fn a_view_preference_switched_elsewhere_is_followed() {
    let world = World::new();
    let hq = world.hq();
    hq.start().await.expect("starts");
    assert!(!hq.view_prefs().show_reasoning);
    let mut changes = hq.changes();
    changes.borrow_and_update();
    crate::registry::set_agents_show_reasoning(&world.data_dir(), true).expect("stores");
    hq.reconcile().await.expect("reconciles");
    assert!(hq.view_prefs().show_reasoning);
    assert!(changes.has_changed().expect("open"), "the views read again");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_redemption_overtaken_by_a_new_address_saves_nothing() {
    let world = World::new();
    let hq = world.hq();
    hq.start().await.expect("starts");
    hq.set_server("https://hq.example.org")
        .await
        .expect("stores");
    let gate = Arc::new(tokio::sync::Notify::new());
    *lock(&world.routes.redeem_gate) = Some(Arc::clone(&gate));
    let redeeming = {
        let hq = Arc::clone(&hq);
        tokio::spawn(async move { hq.pair_redeem("0123456789ABCDEFGHJKMNPQ").await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    hq.set_server("").await.expect("clears");
    gate.notify_one();
    let outcome = redeeming.await.expect("joins");
    assert!(matches!(outcome, Err(HqError::Sentence(_))), "{outcome:?}");
    assert_eq!(
        crate::registry::get_agents_server_url(&world.data_dir()).expect("reads"),
        None
    );
    assert!(world
        .platform
        .keychain_get(&format!("{KEYCHAIN_PREFIX}https://hq.example.org"))
        .expect("reads")
        .is_none());
    assert_eq!(hq.status().borrow().phase, HqPhase::NoServer);
}

#[tokio::test]
async fn a_scanned_link_is_offered_and_pairs_only_when_confirmed() {
    let world = World::new();
    let hq = world.hq();
    hq.start().await.expect("starts");
    let link = format!(
        "keeper://agents/pair?server={}&code=0123456789ABCDEFGHJKMNPQ",
        "https%3A%2F%2Fhq.example.org"
    );
    hq.offer_link(&link).expect("offers");
    assert_eq!(
        hq.status().borrow().offer.as_deref(),
        Some("hq.example.org")
    );
    assert!(
        lock(&world.routes.redeemed).is_empty(),
        "nothing is sent before the person confirms"
    );
    assert!(hq
        .offer_link("keeper://agents/pair?code=0123456789ABCDEFGHJKMNPQ")
        .is_err());
    let status = hq.accept_offer().await.expect("pairs");
    assert_eq!(*lock(&world.routes.redeemed), ["https://hq.example.org"]);
    assert_eq!(status.phase, HqPhase::Paired);
    assert_eq!(status.offer, None);
}

#[tokio::test]
async fn the_devices_follow_the_replicated_directory() {
    let world = World::new();
    world.paired("https://hq.example.org", &["alpha"]);
    let hq = world.hq();
    hq.start().await.expect("starts");
    let row = |id: &str, revoked: Option<i64>| {
        let Value::Object(row) = serde_json::json!({
            "id": id, "name": format!("keeper on {id}"), "created_at_ms": 1, "revoked_at_ms": revoked,
        }) else {
            unreachable!("an object literal")
        };
        row
    };
    let alpha = world.live("alpha")[0].clone();
    alpha.replicate(vec![row("ana.1", None), row("ana.2", None)]);
    let status = status_until(&hq, "two devices", |s| s.devices.len() == 2).await;
    assert!(status.devices[0].this_device && !status.devices[1].this_device);
    alpha.replicate(vec![row("ana.1", None), row("ana.2", Some(5))]);
    status_until(&hq, "ana.2 revoked", |s| {
        s.devices
            .iter()
            .any(|d| d.client_id == "ana.2" && d.revoked)
    })
    .await;
}

#[cfg(unix)]
#[tokio::test]
async fn copies_that_cannot_be_deleted_are_reported_and_the_pairing_kept() {
    use std::os::unix::fs::PermissionsExt;
    let world = World::new();
    let server = world.paired("https://hq.example.org", &["alpha"]);
    let hq = world.hq();
    hq.start().await.expect("starts");
    let dir = databases::server_dir(&world.data_dir(), &server).join("alpha");
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o500)).expect("chmod");
    let outcome = hq.forget().await;
    std::fs::set_permissions(&dir, std::fs::Permissions::from_mode(0o700)).expect("chmod back");
    assert!(
        matches!(&outcome, Err(HqError::Sentence(s)) if s.contains("could not delete")),
        "{outcome:?}"
    );
    assert!(world
        .platform
        .keychain_get(&keychain_key(&server))
        .expect("reads")
        .is_some());
    assert_eq!(hq.status().borrow().phase, HqPhase::Paired);
    // With the copies deletable, it forgets.
    let status = hq.forget().await.expect("forgets");
    assert_eq!(status.phase, HqPhase::NotPaired);
    assert!(!databases::server_dir(&world.data_dir(), &server).exists());
    assert!(world
        .platform
        .keychain_get(&keychain_key(&server))
        .expect("reads")
        .is_none());
}

/// A real local replica whose `session:` presence answers by the reading
/// thread, so a test can stage two streams of one session reading it at once.
struct Staged {
    inner: Arc<dyn Replica>,
    /// The stream on thread `late` says it is reading the presence...
    entered: Mutex<Option<std::sync::mpsc::Sender<()>>>,
    /// ...and reads on once the stream on thread `early` is done, or after a
    /// while if that stream cannot get in.
    early_done: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
    /// The stream on thread `older` says it is drawing its view (reading the
    /// messages, its tails already kept) and draws on once released.
    projecting: Mutex<Option<Gate>>,
    /// The People sheet read on thread `people` says it has read the
    /// members, and returns them once released.
    people: Mutex<Option<Gate>>,
    /// What every thread the staging does not name reads.
    others: Mutex<Vec<crate::hq::port::Peer>>,
}

/// One side says it got there; the other lets it go on.
type Gate = (std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>);

fn tail(from_seq: i64, text: &str) -> Vec<crate::hq::port::Peer> {
    vec![crate::hq::port::Peer {
        actor_id: "nixie".to_owned(),
        client_id: "nixie.box".to_owned(),
        doc: serde_json::json!({"turn": "t1", "from_seq": from_seq, "text": text}),
    }]
}

impl Replica for Staged {
    fn mutate(&self, ops: Vec<RowOp>) -> Result<String, crate::hq::port::ReplicaError> {
        self.inner.mutate(ops)
    }
    fn query(
        &self,
        sql: &str,
        params: &[Value],
    ) -> Result<Vec<serde_json::Map<String, Value>>, crate::hq::port::ReplicaError> {
        let rows = self.inner.query(sql, params)?;
        if std::thread::current().name() == Some("people")
            && sql.contains("reads_trace FROM session_members WHERE session_id")
        {
            if let Some((entered, release)) = lock(&self.people).take() {
                let _ = entered.send(());
                let _ = release.recv_timeout(Duration::from_secs(10));
            }
        }
        Ok(rows)
    }
    fn query_covered(
        &self,
        sql: &str,
        params: &[Value],
        base: &crate::hq::port::WindowBase,
        units: &[String],
    ) -> Result<crate::hq::port::Covered, crate::hq::port::ReplicaError> {
        if std::thread::current().name() == Some("older") {
            if let Some((entered, release)) = lock(&self.projecting).take() {
                let _ = entered.send(());
                let _ = release.recv_timeout(Duration::from_secs(10));
            }
        }
        self.inner.query_covered(sql, params, base, units)
    }
    fn subscribe(&self, s: Subscription) -> Result<(), crate::hq::port::ReplicaError> {
        self.inner.subscribe(s)
    }
    fn set_window(
        &self,
        base: &crate::hq::port::WindowBase,
        units: &[String],
    ) -> Result<(), crate::hq::port::ReplicaError> {
        self.inner.set_window(base, units)
    }
    fn window_state(
        &self,
        base: &crate::hq::port::WindowBase,
    ) -> Result<crate::hq::port::WindowState, crate::hq::port::ReplicaError> {
        self.inner.window_state(base)
    }
    fn set_presence(
        &self,
        key: &str,
        doc: Option<Value>,
    ) -> Result<(), crate::hq::port::ReplicaError> {
        self.inner.set_presence(key, doc)
    }
    fn presence(
        &self,
        key: &str,
    ) -> Result<Vec<crate::hq::port::Peer>, crate::hq::port::ReplicaError> {
        if !key.starts_with("session:") {
            return Ok(Vec::new());
        }
        // The writer flushed between the two reads: `early` saw the tail at
        // 5, everyone after sees chunk 5's successor, still empty.
        match std::thread::current().name() {
            Some("early") => Ok(tail(5, "abc")),
            Some("late") => {
                if let Some(entered) = lock(&self.entered).take() {
                    let _ = entered.send(());
                }
                if let Some(done) = lock(&self.early_done).take() {
                    let _ = done.recv_timeout(Duration::from_millis(300));
                }
                Ok(tail(6, ""))
            }
            // Before the answer began; then its first tail, from seq 0.
            Some("older") => Ok(Vec::new()),
            Some("newer") => Ok(tail(0, "hel")),
            _ => Ok(lock(&self.others).clone()),
        }
    }
    fn upload_blob(
        &self,
        b: Vec<u8>,
        m: Option<String>,
    ) -> Result<Value, crate::hq::port::ReplicaError> {
        self.inner.upload_blob(b, m)
    }
    fn fetch_blob(&self, b: &str) -> Result<Vec<u8>, crate::hq::port::ReplicaError> {
        self.inner.fetch_blob(b)
    }
    fn delivery(
        &self,
        commit: &str,
    ) -> Result<crate::hq::port::Delivery, crate::hq::port::ReplicaError> {
        self.inner.delivery(commit)
    }
    fn remote_query(
        &self,
        id: &str,
        params: Value,
    ) -> Result<Vec<serde_json::Map<String, Value>>, crate::hq::port::ReplicaError> {
        self.inner.remote_query(id, params)
    }
    fn remote_command(
        &self,
        id: &str,
        request: &str,
        params: Value,
    ) -> Result<crate::hq::port::Answered, crate::hq::port::ReplicaError> {
        self.inner.remote_command(id, request, params)
    }
    fn sync_now(&self) {
        self.inner.sync_now();
    }
    fn state(&self) -> ReplicaState {
        self.inner.state()
    }
    fn watch(&self) -> watch::Receiver<ReplicaState> {
        self.inner.watch()
    }
    fn close(&self) {
        self.inner.close();
    }
    fn join(&self) {
        self.inner.join();
    }
}

/// Opens the one staged replica, on this device alone.
struct StagedOpener(Mutex<Option<Arc<Staged>>>);

impl Opener for StagedOpener {
    fn open(&self, spec: ReplicaSpec) -> Result<Arc<dyn Replica>, crate::hq::port::ReplicaError> {
        let inner = crate::hq::syncular::SyncularReplica::open(ReplicaSpec { link: None, ..spec })?;
        let (entered, _) = std::sync::mpsc::channel();
        let (_, early_done) = std::sync::mpsc::channel();
        let staged = Arc::new(Staged {
            inner: Arc::new(inner),
            entered: Mutex::new(Some(entered)),
            early_done: Mutex::new(Some(early_done)),
            projecting: Mutex::new(None),
            people: Mutex::new(None),
            others: Mutex::new(tail(6, "")),
        });
        *lock(&self.0) = Some(Arc::clone(&staged));
        Ok(staged)
    }
}

/// A started service on one staged replica holding `ana`'s session
/// `ana.1.1` with agent `nixie`, the trace read or not, and `t1`'s chunks
/// `chunks` from seq 0, opened by two streams.
async fn staged_session(
    world: &World,
    reads_trace: bool,
    chunks: &[&str],
) -> (Arc<Hq>, Arc<Staged>, sessions::AgentSessionRef) {
    world.paired("https://hq.example.org", &["alpha"]);
    let opener = Arc::new(StagedOpener(Mutex::new(None)));
    let hq = Hq::new(
        Arc::clone(&world.platform) as Arc<dyn Platform>,
        Arc::clone(&opener) as Arc<dyn Opener>,
        Arc::clone(&world.routes) as Arc<dyn Routes>,
        "keeper on test".to_owned(),
    );
    hq.start().await.expect("starts");
    let staged = lock(&opener.0).clone().expect("alpha opened");
    let at = sessions::now_ms() - 60_000;
    let upsert = |table: &str, values: Value| RowOp::Upsert {
        table: table.to_owned(),
        values: sessions::object(values),
        base_version: None,
    };
    let mut seed = vec![
        upsert(
            "actors",
            serde_json::json!({"id": "nixie", "kind": "agent", "display_name": "Nixie",
                "family": null, "dir": "alpha", "created_at_ms": 1, "author_actor": "server",
                "author_client": "server"}),
        ),
        upsert(
            "sessions",
            serde_json::json!({"id": "ana.1.1", "title": "t", "status": "open",
                "agent_actor": "nixie", "created_by_actor": "ana", "created_by_client": "ana.1",
                "preferred_client": null, "parent_session": null, "invite": "[]",
                "archived": false, "heard_until": null, "created_at_ms": at,
                "last_activity_ms": at, "trust": null, "author_actor": "ana",
                "author_client": "ana.1"}),
        ),
        upsert(
            "session_members",
            serde_json::json!({"id": "ana.1.1/ana", "session_id": "ana.1.1", "actor_id": "ana",
                "role": "person", "reads_trace": reads_trace, "added_by_actor": "ana",
                "added_at_ms": at, "read_until": null, "author_actor": "ana",
                "author_client": "ana.1"}),
        ),
    ];
    for (seq, text) in chunks.iter().enumerate() {
        seed.push(upsert(
            "turn_chunks",
            serde_json::json!({"id": format!("k{seq}"), "session_id": "ana.1.1",
                "turn_id": "t1", "seq": seq, "text": text, "created_at_ms": at,
                "author_actor": "nixie", "author_client": "nixie.box"}),
        ));
    }
    if reads_trace {
        seed.push(upsert(
            "reasoning",
            serde_json::json!({"id": "r1", "session_id": "ana.1.1",
                "page_key": sessions::page_key("ana.1.1", &sessions::month(at)),
                "turn_id": "t1", "seq": 1, "compacted": false, "text": "thinking",
                "created_at_ms": at, "author_actor": "nixie", "author_client": "nixie.box"}),
        ));
    }
    staged.mutate(seed).expect("seeded");
    let key = sessions::AgentSessionRef {
        database: "alpha".to_owned(),
        session_id: "ana.1.1".to_owned(),
    };
    hq.open_session(&key).expect("first stream");
    hq.open_session(&key).expect("second stream");
    (hq, staged, key)
}

fn live_text(view: &timeline::AgentSessionViewVm) -> Option<String> {
    view.live.first().map(|turn| turn.text.clone())
}

/// R1063-01: two streams of one open session (a remount opens the next
/// before the last has closed) read its tails at once. Whichever finishes
/// last, the tail the other one kept is not lost: the answer only grows.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn two_streams_of_one_session_never_drop_each_others_tail() {
    let world = World::new();
    let (hq, staged, key) = staged_session(&world, false, &["", "1", "2", "3", "4"]).await;
    let (entered_tx, entered) = std::sync::mpsc::channel();
    let (done, early_done) = std::sync::mpsc::channel();
    *lock(&staged.entered) = Some(entered_tx);
    *lock(&staged.early_done) = Some(early_done);
    // `late` reads first and holds inside its presence read until `early`
    // is done (or cannot be, because the read excludes it).
    let late = {
        let hq = Arc::clone(&hq);
        let key = key.clone();
        std::thread::Builder::new()
            .name("late".to_owned())
            .spawn(move || hq.session_view(&key).expect("late view"))
            .expect("spawns")
    };
    entered
        .recv_timeout(Duration::from_secs(5))
        .expect("late reads the presence");
    let early = {
        let hq = Arc::clone(&hq);
        let key = key.clone();
        std::thread::Builder::new()
            .name("early".to_owned())
            .spawn(move || {
                let view = hq.session_view(&key).expect("early view");
                let _ = done.send(());
                view
            })
            .expect("spawns")
    };
    let early = early.join().expect("early joins");
    let _late = late.join().expect("late joins");
    assert_eq!(
        live_text(&early).as_deref(),
        Some("1234abc"),
        "early saw the tail"
    );
    // Chunk 5 has not landed: the kept tail is all that shows its text.
    let next = hq.session_view(&key).expect("the next read");
    assert_eq!(
        live_text(&next).as_deref(),
        Some("1234abc"),
        "the answer shrank: a stream wrote back a set without the other's tail"
    );
}

/// R1063-08: a view drawn from a snapshot taken before an answer began
/// settles after a newer stream of the session showed that answer's first
/// tail. The older view must not forget a turn it never saw: once the
/// writer moves on to its next, still empty tail before the chunk lands
/// here, the answer still shows what it showed.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_older_view_never_forgets_a_turn_a_newer_stream_saw() {
    let world = World::new();
    let (hq, staged, key) = staged_session(&world, false, &[]).await;
    let (entered_tx, entered) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    *lock(&staged.projecting) = Some((entered_tx, released));
    // After both views: chunk 0 is written but not here yet, and the
    // writer's next tail is still empty.
    *lock(&staged.others) = tail(1, "");
    let older = {
        let hq = Arc::clone(&hq);
        let key = key.clone();
        std::thread::Builder::new()
            .name("older".to_owned())
            .spawn(move || hq.session_view(&key).expect("older view"))
            .expect("spawns")
    };
    entered
        .recv_timeout(Duration::from_secs(5))
        .expect("the older view is drawing from its snapshot");
    let newer = {
        let hq = Arc::clone(&hq);
        let key = key.clone();
        std::thread::Builder::new()
            .name("newer".to_owned())
            .spawn(move || hq.session_view(&key).expect("newer view"))
            .expect("spawns")
    }
    .join()
    .expect("newer joins");
    assert_eq!(
        live_text(&newer).as_deref(),
        Some("hel"),
        "the newer stream saw the tail"
    );
    release.send(()).expect("the older view waits");
    let older = older.join().expect("older joins");
    assert!(older.live.is_empty(), "drawn before the answer began");
    let next = hq.session_view(&key).expect("the next read");
    assert_eq!(
        live_text(&next).as_deref(),
        Some("hel"),
        "the answer shrank: an older view forgot a turn it never saw"
    );
}

/// R1063-09: a settings file that sets a view preference keeps it. Asking
/// for the other value — thinking on where the file says off, details off
/// where it says on — answers what holds, keeps it and draws by it.
#[tokio::test]
async fn a_view_preference_a_settings_file_sets_stays_as_the_file_says() {
    let layers = crate::config::install_for_test(crate::config::layers_from(&[
        (
            "agents.show_reasoning",
            "0",
            crate::config::LayerTier::UserGlobal,
        ),
        (
            "agents.show_details",
            "1",
            crate::config::LayerTier::UserGlobal,
        ),
    ]));
    let world = World::new();
    let (hq, staged, key) = staged_session(&world, true, &[""]).await;
    *lock(&staged.others) = Vec::new();
    let held = live::AgentViewPrefsVm {
        show_reasoning: false,
        show_details: true,
    };
    let answered = hq
        .set_view_prefs(live::AgentViewPrefsPatch {
            show_reasoning: Some(true),
            show_details: Some(false),
        })
        .expect("writes");
    assert_eq!(
        answered, held,
        "the caller learns what holds, not what it asked"
    );
    assert_eq!(hq.view_prefs(), held, "and that is what is kept");
    let view = hq.session_view(&key).expect("view");
    assert_eq!(view.prefs, held, "the switches show what holds");
    assert!(view.reads_trace);
    assert_eq!(
        view.live[0].reasoning, None,
        "no thinking the file keeps off"
    );
    // Without the file the written choice holds.
    drop(layers);
    hq.reconcile().await.expect("reconciles");
    assert_eq!(
        hq.view_prefs(),
        live::AgentViewPrefsVm {
            show_reasoning: true,
            show_details: false,
        }
    );
}

/// R1064K-03: a change to a session's people that lands while its sheet is
/// first read — after the members were read, before the stream follows the
/// feed — is still reported by the feed the stream follows, so the stream
/// reads again instead of keeping the stale sheet.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_people_change_while_the_sheet_is_first_read_is_not_missed() {
    let world = World::new();
    let (hq, staged, key) = staged_session(&world, true, &[]).await;
    let (entered_tx, entered) = std::sync::mpsc::channel();
    let (release, released) = std::sync::mpsc::channel();
    *lock(&staged.people) = Some((entered_tx, released));
    let watching = {
        let hq = Arc::clone(&hq);
        let key = key.clone();
        std::thread::Builder::new()
            .name("people".to_owned())
            .spawn(move || hq.session_members_watched(&key).expect("the sheet"))
            .expect("spawns")
    };
    entered
        .recv_timeout(Duration::from_secs(5))
        .expect("the members are read");
    // Ben's membership lands now, and the service says so.
    staged
        .mutate(vec![RowOp::Upsert {
            table: "session_members".to_owned(),
            values: sessions::object(serde_json::json!({"id": "ana.1.1/ben",
                "session_id": "ana.1.1", "actor_id": "ben", "role": "person",
                "reads_trace": false, "added_by_actor": "ana", "added_at_ms": 1,
                "read_until": null, "author_actor": "server", "author_client": "server"})),
            base_version: None,
        }])
        .expect("ben arrives");
    hq.touch();
    release.send(()).expect("releases");
    let (sheet, changes) = watching.join().expect("joins");
    assert!(
        sheet.members.iter().all(|m| m.actor_id != "ben"),
        "the first sheet was read before Ben arrived"
    );
    assert!(
        changes.has_changed().expect("the service is alive"),
        "the feed reports the change, so the stream reads the sheet again"
    );
    assert!(hq
        .session_members(&key)
        .expect("again")
        .members
        .iter()
        .any(|m| m.actor_id == "ben"));
}
