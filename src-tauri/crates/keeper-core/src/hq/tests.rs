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
