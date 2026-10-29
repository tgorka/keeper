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
    /// Every push target request: the server, the token and the body (`None`
    /// for a `DELETE`).
    push_targets: Mutex<Vec<(String, String, Option<Value>)>>,
    /// Refuse every push target request.
    push_target_refused: std::sync::atomic::AtomicBool,
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
    fn set_push_target<'a>(
        &'a self,
        server: &'a ServerUrl,
        token: &'a str,
        target: Option<&'a push::PushTarget>,
    ) -> Answer<'a, ()> {
        lock(&self.push_targets).push((
            server.as_str().to_owned(),
            token.to_owned(),
            target.map(push::PushTarget::body),
        ));
        let refused = self.push_target_refused.load(Ordering::SeqCst);
        Box::pin(async move {
            if refused {
                Err(RouteError::Unreachable("down".to_owned()))
            } else {
                Ok(())
            }
        })
    }
    fn push_item<'a>(
        &'a self,
        _: &'a ServerUrl,
        _: &'a str,
        _: &'a push::PushedItem,
    ) -> Answer<'a, Vec<u8>> {
        Box::pin(async { Err(RouteError::Unreachable("unused".to_owned())) })
    }
    fn auth_config<'a>(&'a self, _: &'a ServerUrl) -> Answer<'a, routes::AuthConfig> {
        Box::pin(async { Err(RouteError::Unreachable("unused".to_owned())) })
    }
    fn enroll<'a>(
        &'a self,
        _: &'a ServerUrl,
        _: &'a str,
        _: &'a str,
        _: &'a str,
        _: Option<&'a str>,
    ) -> Answer<'a, routes::Enrolled> {
        Box::pin(async { Err(RouteError::Unreachable("unused".to_owned())) })
    }
    fn attest<'a>(
        &'a self,
        _: &'a ServerUrl,
        _: &'a str,
        _: &'a str,
    ) -> Answer<'a, routes::Attested> {
        Box::pin(async { Err(RouteError::Unreachable("unused".to_owned())) })
    }
    fn enrolments<'a>(
        &'a self,
        _: &'a ServerUrl,
        _: &'a str,
    ) -> Answer<'a, Vec<routes::Enrolment>> {
        Box::pin(async { Err(RouteError::Unreachable("unused".to_owned())) })
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
            Arc::new(signin::NoAccount),
            "keeper on test".to_owned(),
            || DeviceHosts::same("test"),
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
            bound: None,
            host: None,
            enrolled_host: None,
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

#[tokio::test]
async fn pairing_registers_the_host_not_the_display_name() {
    use crate::forges::testing::{serve, Reply};
    for (host, offered) in [("hesperia", false), ("kalypso", true)] {
        let server = serve(move |request| {
            if request.path == "/pair/redeem" {
                if request.json()["host"] != host || request.json()["name"] != "My keeper" {
                    return Reply::json(400, r#"{"error":"hq.device_host"}"#);
                }
                Reply::json(
                    200,
                    r#"{"token":"device-token","actorId":"ana","clientId":"ana.1","databases":[]}"#,
                )
            } else {
                Reply::json(
                    200,
                    r#"{"actorId":"ana","clientId":"ana.1","databases":[]}"#,
                )
            }
        });
        let world = World::new();
        let live_host = Arc::new(Mutex::new(String::new()));
        let provided_host = Arc::clone(&live_host);
        let hq = Hq::new(
            Arc::clone(&world.platform) as Arc<dyn Platform>,
            Arc::clone(&world.opener) as Arc<dyn Opener>,
            Arc::new(routes::HttpRoutes::new()),
            Arc::new(signin::NoAccount),
            "My keeper".to_owned(),
            // A name set in Settings never names a new credential: the
            // pairing is created under the system's (R-HQ-94 amendment 2 (4)).
            move || DeviceHosts {
                system: lock(&provided_host).clone(),
                name: "studio".to_owned(),
            },
        );
        hq.start().await.expect("starts");
        hq.set_server(&server.base).await.expect("sets server");
        assert!(matches!(
            hq.pair_redeem("0123456789ABCDEFGHJKMNPQ").await,
            Err(HqError::Sentence(_))
        ));
        assert!(lock(&hq.inner).pairing.is_none());
        *lock(&live_host) = host.to_owned();
        if offered {
            let link = format!(
                "keeper://agents/pair?server={}&code=0123456789ABCDEFGHJKMNPQ",
                server.base
            );
            hq.offer_link(&link).expect("offers pairing");
            hq.accept_offer()
                .await
                .expect("pairs from link with the OS host");
        } else {
            hq.pair_redeem("0123456789ABCDEFGHJKMNPQ")
                .await
                .expect("pairs with the OS host");
        }
        assert_eq!(
            lock(&hq.inner)
                .pairing
                .as_ref()
                .expect("paired credential")
                .client_id,
            "ana.1"
        );
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

/// The view mode is a person's setting: switched in a settings file or
/// synced from another device, it reaches the open views on the next
/// reconcile.
#[tokio::test]
async fn a_view_preference_switched_elsewhere_is_followed() {
    let world = World::new();
    let hq = world.hq();
    hq.start().await.expect("starts");
    assert_eq!(hq.view_prefs().view_mode, live::AgentViewModeVm::Normal);
    let mut changes = hq.changes();
    changes.borrow_and_update();
    crate::registry::set_agents_view_mode(&world.data_dir(), live::AgentViewModeVm::Thinking)
        .expect("stores");
    hq.reconcile().await.expect("reconciles");
    assert_eq!(hq.view_prefs().view_mode, live::AgentViewModeVm::Thinking);
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

/// R-HQ-94 and its amendments: what a rename is checked against is read from
/// the replicated directory — the hosts of the person's other live devices
/// (never this one's, a removed one's, a blank one or someone else's) and
/// every live agent instance with its host — and only once it is complete:
/// every database the server opens to this device held here, with its
/// client list synced (it lists this device). With no credential there is
/// nothing to protect.
///
/// Probe A (Rev607r2, R607-04): alpha synced, beta held but not synced, is
/// not read — beta may list `nixi@hesperia` — and the refusal names beta.
#[tokio::test]
async fn a_rename_is_checked_against_the_complete_replicated_directory() {
    use crate::device_name::{Directory, Unread};
    let world = World::new();
    let unpaired = world.hq();
    unpaired.start().await.expect("starts");
    assert_eq!(unpaired.device_directory(), Ok(Directory::default()));
    unpaired.shutdown().await;

    let server = world.paired("https://hq.example.org", &["alpha", "beta"]);
    let opens = |open: &[&str]| {
        crate::hq::databases::Authorization {
            removed: false,
            open: open.iter().map(|db| (*db).to_owned()).collect(),
        }
        .save(&world.data_dir(), &server)
        .expect("saved");
    };
    opens(&["alpha", "beta"]);
    let hq = world.hq();
    hq.start().await.expect("starts");
    let unread = |database: &str| {
        Err(Unread {
            database: Some(database.to_owned()),
        })
    };
    let row = |id: &str, actor: &str, kind: &str, host: &str, revoked: Option<i64>| {
        let Value::Object(row) = serde_json::json!({
            "id": id, "actor_id": actor, "kind": kind, "host": host, "created_at_ms": 1,
            "revoked_at_ms": revoked,
        }) else {
            unreachable!("an object literal")
        };
        row
    };
    let (alpha, beta) = (
        world.live("alpha")[0].clone(),
        world.live("beta")[0].clone(),
    );
    alpha.replicate(vec![row("ana.2", "ana", "device", "kalypso", None)]);
    assert_eq!(
        hq.device_directory(),
        unread("alpha"),
        "this device is not listed yet"
    );

    alpha.replicate(vec![
        row("ana.1", "ana", "device", "test", None),
        row("ana.2", "ana", "device", "kalypso", None),
        row("ana.3", "ana", "device", "electra", Some(5)),
        row("ana.4", "ana", "device", "  ", None),
        row("ana.5", "ana", "device", "kalypso", None),
        row("bo.1", "bo", "device", "attic", None),
        row("dixi.electra", "dixi", "instance", "electra", None),
        row("dixi.old", "dixi", "instance", "den", Some(9)),
    ]);
    // Probe A: beta has not synced its client list.
    assert_eq!(hq.device_directory(), unread("beta"));

    beta.replicate(vec![
        row("ana.1", "ana", "device", "test", None),
        row("nixi.hesperia", "nixi", "instance", "hesperia", None),
    ]);
    let directory = hq.device_directory().expect("read");
    assert_eq!(directory.taken, ["kalypso"]);
    assert_eq!(
        directory.instances,
        [
            ("dixi".to_owned(), "electra".to_owned()),
            ("nixi".to_owned(), "hesperia".to_owned()),
        ]
    );
    // Kept before hosts were recorded: backfilled with the system's name.
    assert_eq!(directory.bound.as_deref(), Some("test"));
    assert_eq!(directory.enrolled.as_deref(), Some("test"));
    // This world has no account: nothing can adopt the pairing under a new
    // name, and a new pairing would be a new device.
    assert_eq!(directory.pinned.as_deref(), Some("hq.example.org"));
    hq.shutdown().await;

    // A database the server opens that this device does not hold.
    opens(&["alpha", "beta", "gamma"]);
    let again = world.hq();
    again.start().await.expect("starts");
    for database in ["alpha", "beta"] {
        world.live(database)[0].replicate(vec![row("ana.1", "ana", "device", "test", None)]);
    }
    assert_eq!(again.device_directory(), unread("gamma"));
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
    fn commit_with_blobs(
        &self,
        b: Vec<crate::hq::port::BlobBody>,
        ops: Vec<RowOp>,
    ) -> Result<String, crate::hq::port::ReplicaError> {
        self.inner.commit_with_blobs(b, ops)
    }
    fn fetch_blob(&self, b: &str) -> Result<Vec<u8>, crate::hq::port::ReplicaError> {
        self.inner.fetch_blob(b)
    }
    fn cached_blob(&self, b: &str) -> Result<Vec<u8>, crate::hq::port::ReplicaError> {
        self.inner.cached_blob(b)
    }
    fn delivery(
        &self,
        commit: &str,
    ) -> Result<crate::hq::port::Delivery, crate::hq::port::ReplicaError> {
        self.inner.delivery(commit)
    }
    fn row_writes(
        &self,
        table: &str,
        row_id: &str,
        field: Option<&str>,
    ) -> Result<crate::hq::port::RowWrites, crate::hq::port::ReplicaError> {
        self.inner.row_writes(table, row_id, field)
    }
    fn revoked_units(&self, table: &str) -> Result<Vec<String>, crate::hq::port::ReplicaError> {
        self.inner.revoked_units(table)
    }
    fn granted(
        &self,
        table: &str,
        variable: &str,
    ) -> Result<Vec<String>, crate::hq::port::ReplicaError> {
        self.inner.granted(table, variable)
    }
    fn refused_commits(
        &self,
        table: &str,
    ) -> Result<Vec<crate::hq::port::RefusedCommit>, crate::hq::port::ReplicaError> {
        self.inner.refused_commits(table)
    }
    fn upload_refusals(&self) -> Vec<crate::hq::port::UploadRefusal> {
        self.inner.upload_refusals()
    }
    fn blob_cache_bytes(&self) -> Result<u64, crate::hq::port::ReplicaError> {
        self.inner.blob_cache_bytes()
    }
    fn mutate_superseding(
        &self,
        ops: Vec<RowOp>,
        earlier: Vec<String>,
    ) -> Result<String, crate::hq::port::ReplicaError> {
        self.inner.mutate_superseding(ops, earlier)
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
    ) -> Result<(), crate::hq::port::ReplicaError> {
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
        Arc::new(signin::NoAccount),
        "keeper on test".to_owned(),
        || DeviceHosts::same("test"),
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

/// R1063-09: a settings file that sets the view mode keeps it. Asking for
/// another — verbose where the file says normal — answers what holds,
/// keeps it and draws by it.
#[tokio::test]
async fn a_view_preference_a_settings_file_sets_stays_as_the_file_says() {
    let layers = crate::config::install_for_test(crate::config::layers_from(&[(
        "agents.view_mode",
        "normal",
        crate::config::LayerTier::UserGlobal,
    )]));
    let world = World::new();
    let (hq, staged, key) = staged_session(&world, true, &[""]).await;
    *lock(&staged.others) = Vec::new();
    let held = live::AgentViewPrefsVm {
        view_mode: live::AgentViewModeVm::Normal,
    };
    let answered = hq
        .set_view_prefs(live::AgentViewPrefsPatch {
            view_mode: Some(live::AgentViewModeVm::Verbose),
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
    assert!(
        view.folds.iter().all(|fold| fold.reasoning.is_none()),
        "no thinking the file keeps off"
    );
    // Without the file the written choice holds.
    drop(layers);
    hq.reconcile().await.expect("reconciles");
    assert_eq!(
        hq.view_prefs(),
        live::AgentViewPrefsVm {
            view_mode: live::AgentViewModeVm::Verbose,
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

/// What the server was asked about this device's push target, in order.
async fn push_targets_until(world: &World, count: usize) -> Vec<(String, String, Option<Value>)> {
    for _ in 0..100 {
        if lock(&world.routes.push_targets).len() >= count {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    // Anything more would have been sent by now.
    tokio::time::sleep(Duration::from_millis(50)).await;
    lock(&world.routes.push_targets).clone()
}

fn apns(gateway: &str) -> Option<push::PushTarget> {
    push::PushTarget::apns(
        Some(gateway),
        crate::agents::push::ApnsEnvironment::Production,
        &[0xAB, 0xCD],
    )
}

/// 106.6: the phone's push target goes to the server it is paired with,
/// once per wish, with its own token; clearing the gateway removes it; a
/// wish made before the device is paired, or while the server is down, is
/// sent at the next discovery; an unpaired or removed device sends nothing.
#[tokio::test]
async fn the_push_target_is_registered_once_per_wish_and_cleared_with_the_gateway() {
    let world = World::new();
    let hq = world.hq();
    hq.start().await.expect("starts");
    // Not paired: nothing to tell, and nothing told.
    hq.set_push_target(apns("https://push.example.org"));
    assert!(push_targets_until(&world, 1).await.is_empty());

    // Paired: the wish made earlier goes at the first discovery.
    let server = world.paired("https://hq.example.org", &["alpha"]);
    *lock(&world.routes.discovery) = Some(Ok(found(&["alpha"])));
    hq.reconcile().await.expect("follows the setting");
    let sent = push_targets_until(&world, 1).await;
    let body = apns("https://push.example.org").map(|t| t.body());
    assert_eq!(
        sent,
        [(
            server.as_str().to_owned(),
            "0123456789abcdef".to_owned(),
            body.clone()
        )]
    );
    assert_eq!(
        body.as_ref().map(|b| b["gateway_url"].clone()),
        Some(serde_json::json!(
            "https://push.example.org/_matrix/push/v1/notify"
        ))
    );

    // The same wish again sends nothing; a new gateway sends it; clearing
    // the gateway removes the target.
    hq.set_push_target(apns("https://push.example.org"));
    assert_eq!(push_targets_until(&world, 2).await.len(), 1);
    hq.set_push_target(apns("https://other.example.org"));
    hq.set_push_target(None);
    let sent = push_targets_until(&world, 3).await;
    // Two quick wishes may fold into one request; the last is always sent.
    assert!(matches!(sent.len(), 2 | 3), "{sent:?}");
    assert_eq!(
        sent.last().map(|s| s.2.clone()),
        Some(None),
        "the last wish is the one the server keeps"
    );

    // A refused request is sent again at the next discovery.
    let before = sent.len();
    world
        .routes
        .push_target_refused
        .store(true, Ordering::SeqCst);
    hq.set_push_target(apns("https://push.example.org"));
    assert_eq!(
        push_targets_until(&world, before + 1).await.len(),
        before + 1
    );
    world
        .routes
        .push_target_refused
        .store(false, Ordering::SeqCst);
    hq.shutdown().await;
    let again = world.hq();
    again.set_push_target(apns("https://push.example.org"));
    again.start().await.expect("restarts");
    let sent = push_targets_until(&world, before + 2).await;
    assert_eq!(sent.len(), before + 2, "{sent:?}");
    assert_eq!(sent.last().map(|s| s.2.clone()), Some(body));
}

#[tokio::test]
async fn a_removed_device_registers_no_push_target() {
    let world = World::new();
    world.paired("https://hq.example.org", &["alpha"]);
    *lock(&world.routes.discovery) = Some(Err(RouteError::Unauthorized));
    let hq = world.hq();
    hq.set_push_target(apns("https://push.example.org"));
    hq.start().await.expect("starts");
    status_until(&hq, "removed", |s| s.phase == HqPhase::Removed).await;
    hq.set_push_target(None);
    assert!(push_targets_until(&world, 1).await.is_empty());
}

/// A device paired with `hq.example.org` holding a real replica of alpha
/// whose directory names the agent nixie.
fn paired_holding_alpha(world: &World) {
    world.paired("https://hq.example.org", &[]);
    let alpha = databases::replica(
        &world.data_dir(),
        &ServerUrl::parse("https://hq.example.org")
            .expect("parses")
            .expect("an address"),
        "alpha",
        "ana",
    );
    std::fs::create_dir_all(alpha.parent().expect("parent")).expect("mkdir");
    let seed = syncular::SyncularReplica::open(ReplicaSpec {
        database: "alpha".to_owned(),
        path: alpha,
        client_id: Some("ana.1".to_owned()),
        link: None,
        subscriptions: Vec::new(),
        watch_presence: None,
    })
    .expect("opens");
    seed.mutate(vec![RowOp::Upsert {
        table: "actors".to_owned(),
        values: sessions::object(serde_json::json!({
            "id": "nixie", "kind": "agent", "display_name": "Nixie", "family": null,
            "dir": "alpha", "created_at_ms": 1, "author_actor": "server",
            "author_client": "server"})),
        base_version: None,
    }])
    .expect("seeds");
    seed.close();
    seed.join();
}

/// A service over `world` opening real replicas through `opener`.
fn real_hq(world: &World, opener: Arc<dyn Opener>) -> Arc<Hq> {
    Hq::new(
        Arc::clone(&world.platform) as Arc<dyn Platform>,
        opener,
        Arc::clone(&world.routes) as Arc<dyn Routes>,
        Arc::new(signin::NoAccount),
        "keeper on test".to_owned(),
        || DeviceHosts::same("test"),
    )
}

/// 106.1 AC 2 and invariant 2 through the calls 106.2's composer makes
/// (A106-01): a device paired with a server whose address is then cleared
/// keeps that server's databases listed and written to. A new session and a
/// message wait in that database's own outbox, the count shows, nothing is
/// asked of any server, and a relaunch keeps all of it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn with_the_address_cleared_the_composer_writes_into_the_held_database_and_it_waits() {
    let world = World::new();
    paired_holding_alpha(&world);
    let service = || real_hq(&world, Arc::new(syncular::SyncularOpener));
    let hq = service();
    hq.start().await.expect("starts");

    let status = hq.set_server("").await.expect("clears");
    assert_eq!(status.phase, HqPhase::NoServer);
    assert_eq!(status.device, None);
    let asked = world.routes.discovered.load(Ordering::SeqCst);
    // What waits so far is the directory row seeded above.
    let before = 1;
    let held = status_until(&hq, "alpha held with no server", |s| {
        s.databases.iter().any(|d| {
            d.name == "alpha" && d.state == HqDatabaseState::NoServer && d.waiting == before
        })
    })
    .await;
    assert_eq!(held.waiting_here, before);

    // New session offers the agent of the held database, and writes there.
    let choices = hq.choices().expect("choices");
    assert_eq!(
        choices
            .iter()
            .map(|c| (c.agent_actor.as_str(), c.database.as_deref()))
            .collect::<Vec<_>>(),
        [("nixie", Some("alpha"))]
    );
    let key = hq
        .new_session("alpha", "nixie", "plan the week", None, None)
        .expect("a new session with no server");
    assert_eq!(key.database, "alpha");
    hq.send(&key, "and then the weekend", &[], None)
        .expect("a message");
    let status = status_until(&hq, "both writes wait", |s| s.waiting_here == before + 2).await;
    assert_eq!(status.databases[0].waiting, before + 2);
    assert!(
        status
            .sentence
            .ends_with(&format!("{} changes wait on this device.", before + 2)),
        "{}",
        status.sentence
    );
    assert!(hq
        .sessions()
        .iter()
        .any(|row| row.database == "alpha" && row.session_id == key.session_id));
    assert_eq!(world.routes.discovered.load(Ordering::SeqCst), asked);
    assert!(lock(&world.routes.push_targets).is_empty());
    assert!(hq.contacting().is_empty());

    // A relaunch with no address keeps the session and what waits.
    hq.shutdown().await;
    let again = service();
    again.start().await.expect("restarts");
    let status = status_until(&again, "still waiting after a relaunch", |s| {
        s.waiting_here == before + 2
    })
    .await;
    assert_eq!(status.phase, HqPhase::NoServer);
    assert!(again
        .sessions()
        .iter()
        .any(|row| row.database == "alpha" && row.session_id == key.session_id));
    assert_eq!(world.routes.discovered.load(Ordering::SeqCst), asked);
    assert!(again.contacting().is_empty());

    // Forgetting that server takes its replicas out of the list for good.
    again
        .set_server("https://hq.example.org")
        .await
        .expect("stores");
    let removed = again.forget().await.expect("forgets");
    assert!(removed.databases.is_empty());
    again.set_server("").await.expect("clears");
    assert!(again.sessions().is_empty());
}

/// A device never paired has no directory: New session has no agent to
/// address and nothing is invented (A106-01, case b).
#[tokio::test]
async fn a_device_never_paired_offers_no_agent_and_asks_nothing() {
    let world = World::new();
    let hq = world.hq();
    hq.start().await.expect("starts");
    assert!(hq.choices().expect("choices").is_empty());
    assert!(matches!(
        hq.new_session("local", "nixie", "hello", None, None),
        Err(HqError::Sentence(_))
    ));
    assert_eq!(world.routes.discovered.load(Ordering::SeqCst), 0);
}

/// A notification may name a session this device does not hold yet
/// (A106-03): opening it is `Arriving` while a round may still bring it, and
/// a sentence once nothing will.
#[tokio::test]
async fn a_session_not_held_yet_is_arriving_until_nothing_can_bring_it() {
    let world = World::new();
    world.paired("https://hq.example.org", &["alpha"]);
    *lock(&world.routes.discovery) = Some(Ok(found(&["alpha"])));
    let hq = world.hq();
    let key = |database: &str| sessions::AgentSessionRef {
        database: database.to_owned(),
        session_id: "ben.2.9".to_owned(),
    };
    // Before the copies open (a cold launch from the tap).
    assert!(matches!(
        hq.open_session(&key("alpha")),
        Err(HqError::Arriving(_))
    ));
    hq.start().await.expect("starts");
    status_until(&hq, "alpha linked", |_| {
        world.live("alpha").iter().any(|r| r.spec.link.is_some())
    })
    .await;
    // alpha syncs: its session may still come.
    assert!(matches!(
        hq.open_session(&key("alpha")),
        Err(HqError::Arriving(_))
    ));
    // gamma is not open to this device: nothing brings it.
    assert!(matches!(
        hq.open_session(&key("gamma")),
        Err(HqError::Sentence(s)) if s.contains("gamma is not open to this device")
    ));
    // Removed: nothing syncs any more.
    hq.apply_discovery(generation(&hq), Err(RouteError::Unauthorized))
        .await
        .expect("applies");
    assert!(matches!(
        hq.open_session(&key("alpha")),
        Err(HqError::Sentence(s)) if s.starts_with("This device was removed")
    ));
    // No address: nothing syncs.
    hq.set_server("").await.expect("clears");
    assert!(matches!(
        hq.open_session(&key("alpha")),
        Err(HqError::Sentence(s)) if s == "This conversation is not on this device."
    ));
}

/// Opens real replicas, holding `database`'s open until released: says when
/// that open has begun, then waits on the release.
struct HeldOpener {
    database: &'static str,
    entered: Arc<tokio::sync::Notify>,
    release: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

impl Opener for HeldOpener {
    fn open(&self, spec: ReplicaSpec) -> Result<Arc<dyn Replica>, crate::hq::port::ReplicaError> {
        if spec.database == self.database {
            let release = lock(&self.release).take();
            if let Some(release) = release {
                self.entered.notify_one();
                let _ = release.recv();
            }
        }
        syncular::SyncularOpener.open(spec)
    }
}

/// The audit's round-2 case (A106-03): paired, the address cleared, a
/// session written and kept, keeper relaunched — and a tapped notification
/// opens that session while the held replica is still opening. That is
/// arriving, never "not on this device"; once the replica is open, the same
/// session opens.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_held_session_opened_while_its_replica_reopens_is_arriving_then_opens() {
    let world = World::new();
    paired_holding_alpha(&world);
    let first = real_hq(&world, Arc::new(syncular::SyncularOpener));
    first.start().await.expect("starts");
    first.set_server("").await.expect("clears");
    status_until(&first, "alpha held with no server", |s| {
        s.databases.iter().any(|d| d.name == "alpha")
    })
    .await;
    let key = first
        .new_session("alpha", "nixie", "plan the week", None, None)
        .expect("a session");
    first.shutdown().await;

    let entered = Arc::new(tokio::sync::Notify::new());
    let (release, held) = std::sync::mpsc::channel();
    let opener = Arc::new(HeldOpener {
        database: "alpha",
        entered: Arc::clone(&entered),
        release: Mutex::new(Some(held)),
    });
    let again = real_hq(&world, opener);
    let starting = {
        let again = Arc::clone(&again);
        tokio::spawn(async move { again.start().await })
    };
    tokio::time::timeout(Duration::from_secs(10), entered.notified())
        .await
        .expect("alpha's open begins");
    let early = again.open_session(&key);
    release.send(()).expect("releases");
    starting.await.expect("joins").expect("starts");
    assert!(
        matches!(&early, Err(HqError::Arriving(_))),
        "opened while its replica was opening: {early:?}"
    );
    again
        .open_session(&key)
        .expect("opens once the replica is open");
    // Opened, and nothing absent is said to be on its way any more.
    assert!(matches!(
        again.open_session(&sessions::AgentSessionRef {
            database: "alpha".to_owned(),
            session_id: "ben.2.9".to_owned(),
        }),
        Err(HqError::Sentence(s)) if s == "This conversation is not on this device."
    ));
}

/// Opens every replica as a real local syncular replica (no server), so a
/// write goes through syncular's own column codec.
#[derive(Default)]
struct LocalOpener(
    Mutex<HashMap<String, Arc<dyn Replica>>>,
    Mutex<HashMap<String, std::path::PathBuf>>,
);

impl Opener for LocalOpener {
    fn open(&self, spec: ReplicaSpec) -> Result<Arc<dyn Replica>, crate::hq::port::ReplicaError> {
        let database = spec.database.clone();
        lock(&self.1).insert(database.clone(), spec.path.clone());
        let replica: Arc<dyn Replica> =
            Arc::new(crate::hq::syncular::SyncularReplica::open(ReplicaSpec {
                link: None,
                ..spec
            })?);
        lock(&self.0).insert(database, Arc::clone(&replica));
        Ok(replica)
    }
}

impl LocalOpener {
    fn replica(&self, database: &str) -> Arc<dyn Replica> {
        lock(&self.0).get(database).cloned().expect("opened")
    }

    fn path(&self, database: &str) -> std::path::PathBuf {
        lock(&self.1).get(database).cloned().expect("opened")
    }
}

/// A started service over real local replicas of `databases`, each of
/// whose directories names the agent `nixie`.
async fn spoken_world(world: &World, databases: &[&str]) -> (Arc<Hq>, Arc<LocalOpener>) {
    world.paired("https://hq.example.org", databases);
    let opener = Arc::new(LocalOpener::default());
    let hq = Hq::new(
        Arc::clone(&world.platform) as Arc<dyn Platform>,
        Arc::clone(&opener) as Arc<dyn Opener>,
        Arc::clone(&world.routes) as Arc<dyn Routes>,
        Arc::new(signin::NoAccount),
        "keeper on test".to_owned(),
        || DeviceHosts::same("test"),
    );
    hq.start().await.expect("starts");
    for database in databases {
        opener
            .replica(database)
            .mutate(vec![RowOp::Upsert {
                table: "actors".to_owned(),
                values: sessions::object(serde_json::json!({
                    "id": "nixie", "kind": "agent", "display_name": "Nixie", "family": null,
                    "dir": database, "created_at_ms": 1, "author_actor": "server",
                    "author_client": "server"})),
                base_version: None,
            }])
            .expect("directory");
    }
    (hq, opener)
}

fn rows(replica: &Arc<dyn Replica>, sql: &str) -> Vec<serde_json::Map<String, Value>> {
    replica.query(sql, &[]).expect("reads")
}

/// R1082-01: the heard mark goes through syncular's own mutation into the
/// `messages.metadata` json column, and reads back as the mark.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_heard_mark_is_stored_as_the_mark_the_runtime_reads() {
    let world = World::new();
    let (hq, opener) = spoken_world(&world, &["alpha"]).await;
    let ask = hq
        .spoken_send("nixie", "What is the weather?")
        .expect("asked");
    let mut mark = spoken::HeardMark::new(ask.key.clone(), ask.question.clone());
    mark.anchored();
    mark.handed(13);
    mark.handed(33);
    mark.spoken();
    let metadata = mark
        .metadata(crate::agents::log::HeardReason::Stop)
        .expect("a mark");
    hq.spoken_heard(&ask.key, metadata).expect("written");

    let alpha = opener.replica("alpha");
    let stored = rows(
        &alpha,
        "SELECT body, metadata FROM messages WHERE role = 'person' AND metadata IS NOT NULL",
    );
    assert_eq!(stored.len(), 1, "{stored:?}");
    let text = stored[0]["metadata"].as_str().expect("JSON text");
    let read: Value = serde_json::from_str(text).expect("parses");
    assert_eq!(
        read,
        serde_json::json!({"kind": "heard", "message": ask.question, "heard_until": 13,
                           "sentence": 1, "reason": "stop"})
    );
    assert_eq!(stored[0]["body"], Value::from(""));
    // The question itself carries no metadata: SQL null, not the text "null".
    let question = rows(
        &alpha,
        &format!(
            "SELECT metadata FROM messages WHERE id = '{}'",
            ask.question
        ),
    );
    assert_eq!(question[0]["metadata"], Value::Null);
}

/// R1082-02: an agent shared by two databases, with no conversation and no
/// database chosen for it, is not given one by name: nothing is written and
/// the person is told where to choose.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_spoken_question_to_an_agent_of_two_databases_waits_for_a_choice() {
    let world = World::new();
    let (hq, opener) = spoken_world(&world, &["neuradrive", "tgdrive"]).await;
    let refused = hq
        .spoken_send("nixie", "Remind me about the dentist")
        .expect_err("no database chosen");
    assert_eq!(
        refused.to_string(),
        spoken::unchosen_sentence("Nixie", &["neuradrive".to_owned(), "tgdrive".to_owned()])
    );
    for database in ["neuradrive", "tgdrive"] {
        let replica = opener.replica(database);
        assert!(
            rows(&replica, "SELECT id FROM sessions").is_empty(),
            "{database}"
        );
        assert!(
            rows(&replica, "SELECT id FROM messages").is_empty(),
            "{database}"
        );
    }

    // Chosen in Agents (the database the person last opened a session with
    // it in): the question goes there.
    sessions::remember_choice(&world.data_dir(), "nixie", "tgdrive");
    let ask = hq
        .spoken_send("nixie", "Remind me about the dentist")
        .expect("asked");
    assert_eq!(ask.key.database, "tgdrive");
    assert_eq!(
        rows(&opener.replica("tgdrive"), "SELECT body FROM messages"),
        vec![sessions::object(
            serde_json::json!({"body": "Remind me about the dentist"})
        )]
    );
    assert!(rows(&opener.replica("neuradrive"), "SELECT id FROM messages").is_empty());
}

/// The only database that names the agent needs no choice.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_spoken_question_to_an_agent_of_one_database_opens_its_conversation_there() {
    let world = World::new();
    let (hq, opener) = spoken_world(&world, &["tgdrive"]).await;
    let ask = hq.spoken_send("nixie", "Hello").expect("asked");
    assert_eq!(
        (ask.key.database.as_str(), ask.agent_name.as_str()),
        ("tgdrive", "Nixie")
    );
    let sessions = rows(
        &opener.replica("tgdrive"),
        "SELECT id, agent_actor FROM sessions",
    );
    assert_eq!(sessions.len(), 1);
    assert_eq!(sessions[0]["agent_actor"], Value::from("nixie"));
    assert_eq!(sessions[0]["id"], Value::from(ask.key.session_id.clone()));
}

/// After #570 a relaunch opens the replicas on disk before anything is
/// listed. A spoken turn meanwhile is told keeper is still opening them —
/// retriable — never that its agent is gone (choose again) nor written
/// anywhere; once open, the same question goes.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_spoken_question_while_the_replicas_open_is_arriving_then_goes() {
    let world = World::new();
    paired_holding_alpha(&world);
    let entered = Arc::new(tokio::sync::Notify::new());
    let (release, held) = std::sync::mpsc::channel();
    let opener = Arc::new(HeldOpener {
        database: "alpha",
        entered: Arc::clone(&entered),
        release: Mutex::new(Some(held)),
    });
    let hq = real_hq(&world, opener);
    let starting = {
        let hq = Arc::clone(&hq);
        tokio::spawn(async move { hq.start().await })
    };
    tokio::time::timeout(Duration::from_secs(10), entered.notified())
        .await
        .expect("alpha's open begins");
    let agents = hq.spoken_agents();
    let early = hq.spoken_send("nixie", "What is on today?");
    release.send(()).expect("releases");
    starting.await.expect("joins").expect("starts");
    assert!(matches!(&agents, Err(HqError::Arriving(_))), "{agents:?}");
    assert!(matches!(&early, Err(HqError::Arriving(_))), "{early:?}");

    assert_eq!(hq.spoken_agents().expect("listed"), ["nixie"]);
    let ask = hq.spoken_send("nixie", "What is on today?").expect("asked");
    assert_eq!(ask.key.database, "alpha");
    let source = hq.source("alpha").expect("alpha");
    let written = source
        .replica
        .query("SELECT body FROM messages", &[])
        .expect("reads");
    assert_eq!(
        written.len(),
        1,
        "the question once, after the open: {written:?}"
    );
}

/// Nixie's instance in `database`, offering `drives` to read.
fn offers(opener: &LocalOpener, database: &str, drives: &[&str]) {
    opener
        .replica(database)
        .mutate(vec![RowOp::Upsert {
            table: "agent_profiles".to_owned(),
            values: sessions::object(serde_json::json!({
                "id": format!("nixie.{database}"), "actor_id": "nixie", "name": "nixie",
                "description": "", "avatar_blob": null, "definition_hash": "h",
                "image_digest": "d", "started_at_ms": 1, "dir": database,
                "drives": Value::from(drives.to_vec()).to_string(),
                "author_actor": "nixie", "author_client": format!("nixie.{database}")})),
            base_version: None,
        }])
        .expect("profile");
}

/// The ticked drives of every session of `database`, by session id.
fn ticks(opener: &LocalOpener, database: &str) -> Vec<(String, String)> {
    rows(
        &opener.replica(database),
        "SELECT session_id, drives FROM session_drives ORDER BY session_id",
    )
    .iter()
    .map(|row| {
        (
            sessions::text(row, "session_id").to_owned(),
            sessions::text(row, "drives").to_owned(),
        )
    })
    .collect()
}

/// The ticked drives the session `key` shows as not yet seen here: what
/// draws the warning beside the chip, wherever the session is shown (the
/// dock or the Agents window alike).
fn unseen_in(hq: &Hq, key: &sessions::AgentSessionRef) -> Vec<String> {
    unseen_ticks_in(hq, key)
        .into_iter()
        .map(|tick| tick.drive)
        .collect()
}

fn unseen_ticks_in(hq: &Hq, key: &sessions::AgentSessionRef) -> Vec<drives::AgentUnseenTickVm> {
    hq.session_view(key)
        .expect("a view")
        .drives
        .map(|drives| drives.unseen)
        .unwrap_or_default()
}

/// OK on the warning `key` shows for `drive`, naming the tick it shows.
fn ok(hq: &Hq, key: &sessions::AgentSessionRef, drive: &str) -> Result<(), HqError> {
    let tick = unseen_ticks_in(hq, key)
        .into_iter()
        .find(|tick| tick.drive == drive)
        .and_then(|tick| tick.tick);
    hq.note_tick_seen(key, drive, tick.as_deref())
}

fn chat(session: &str, database: &str) -> sessions::AgentSessionRef {
    sessions::AgentSessionRef {
        database: database.to_owned(),
        session_id: session.to_owned(),
    }
}

/// 109.28 (R-HQ-86) AC 1: opened on a tgdrive note, the dock gets the
/// default agent's chat in the tgdrive database — started there, since it
/// has none — with tgdrive ticked as the person. Opened again, it is the
/// same chat, already ticked: nothing is ticked twice. A neuradrive note
/// gets neuradrive's own chat.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_notes_assistant_opens_the_notes_drive_chat_and_ticks_it_once() {
    let world = World::new();
    let (hq, opener) = spoken_world(&world, &["neuradrive", "tgdrive"]).await;
    for database in ["neuradrive", "tgdrive"] {
        offers(&opener, database, &["neuradrive", "tgdrive"]);
    }
    crate::registry::set_agents_default(&world.data_dir(), Some("@nixie:hq.example.org"))
        .expect("default agent");
    // The agent's newest chat is elsewhere: the note's drive decides.
    let elsewhere = hq
        .new_session("neuradrive", "nixie", "groceries", None, None)
        .expect("a neuradrive chat");

    let first = hq
        .session_for_note(Some("tgdrive"), None)
        .expect("answers")
        .expect("a default agent");
    let session = first.target.session_id.clone().expect("a chat");
    assert_eq!(
        (
            first.target.database.as_deref(),
            first.drive.as_deref(),
            first.reason.as_deref(),
        ),
        (Some("tgdrive"), Some("tgdrive"), None)
    );
    let started = rows(
        &opener.replica("tgdrive"),
        "SELECT id, agent_actor FROM sessions",
    );
    assert_eq!(started.len(), 1, "{started:?}");
    assert_eq!(started[0]["id"], Value::from(session.clone()));
    assert_eq!(
        ticks(&opener, "tgdrive"),
        [(session.clone(), r#"["tgdrive"]"#.to_owned())]
    );

    let again = hq
        .session_for_note(Some("tgdrive"), None)
        .expect("answers")
        .expect("a default agent");
    assert_eq!(again.target.session_id.as_deref(), Some(session.as_str()));
    assert_eq!(again.reason, None);
    assert_eq!(
        ticks(&opener, "tgdrive"),
        [(session.clone(), r#"["tgdrive"]"#.to_owned())]
    );
    assert_eq!(
        rows(&opener.replica("tgdrive"), "SELECT id FROM sessions").len(),
        1,
        "the same chat, not a second one"
    );

    let other = hq
        .session_for_note(Some("neuradrive"), None)
        .expect("answers")
        .expect("a default agent");
    assert_eq!(
        (
            other.target.database.as_deref(),
            other.target.session_id.as_deref()
        ),
        (Some("neuradrive"), Some(elsewhere.session_id.as_str()))
    );
    assert_eq!(
        ticks(&opener, "neuradrive"),
        [(elsewhere.session_id.clone(), r#"["neuradrive"]"#.to_owned())]
    );
}

/// Round 5 ruling (R605-02/-05/-06/-07/-08): keeper never takes a dock tick
/// back. Every one is marked unseen, and the session shows it — the chip and
/// the warning read from the same selection — wherever it is opened: an
/// answer the dock dropped (closed, moved on) leaves a ticked drive whose
/// warning the Agents window draws. OK clears the mark; a person's own
/// untick clears it with the tick, and their own tick acknowledges it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_dock_tick_stays_unseen_wherever_its_chat_is_shown_until_a_person_answers_it() {
    let world = World::new();
    let (hq, opener) = spoken_world(&world, &["tgdrive"]).await;
    offers(&opener, "tgdrive", &["tgdrive"]);
    crate::registry::set_agents_default(&world.data_dir(), Some("@nixie:hq.example.org"))
        .expect("default agent");

    // An answer nobody saw: the dock closed before it came.
    let dropped = hq
        .session_for_note(Some("tgdrive"), None)
        .expect("answers")
        .expect("a default agent");
    let key = chat(
        dropped.target.session_id.as_deref().expect("a chat"),
        "tgdrive",
    );
    // The Agents window opens that chat: the drive is ticked, and its
    // warning stands beside the chip.
    assert_eq!(unseen_in(&hq, &key), ["tgdrive"]);
    // A second answer, the reopened dock's: nothing changes, nothing is
    // taken back.
    hq.session_for_note(Some("tgdrive"), None).expect("answers");
    assert_eq!(unseen_in(&hq, &key), ["tgdrive"]);
    assert_eq!(
        ticks(&opener, "tgdrive"),
        [(key.session_id.clone(), r#"["tgdrive"]"#.to_owned())]
    );

    // OK: seen, and the drive stays ticked.
    ok(&hq, &key, "tgdrive").expect("seen");
    assert!(unseen_in(&hq, &key).is_empty());
    assert_eq!(
        ticks(&opener, "tgdrive"),
        [(key.session_id.clone(), r#"["tgdrive"]"#.to_owned())]
    );

    // The person unticks; the dock ticks again, unseen again.
    hq.set_session_drive(&key, "tgdrive", false)
        .expect("unticked");
    hq.session_for_note(Some("tgdrive"), None).expect("answers");
    assert_eq!(unseen_in(&hq, &key), ["tgdrive"]);
    // Untick from the warning: tick and mark go together.
    hq.set_session_drive(&key, "tgdrive", false)
        .expect("unticked");
    assert!(unseen_in(&hq, &key).is_empty());
    assert_eq!(
        ticks(&opener, "tgdrive"),
        [(key.session_id.clone(), "[]".to_owned())]
    );
    // A person's own tick is theirs: no warning of the dock's.
    hq.set_session_drive(&key, "tgdrive", true).expect("ticked");
    assert!(unseen_in(&hq, &key).is_empty());
    // A mark left over with no tick draws nothing.
    hq.set_session_drive(&key, "tgdrive", false)
        .expect("unticked");
    hq.session_for_note(Some("tgdrive"), None).expect("answers");
    hq.set_session_drive(&key, "tgdrive", true)
        .expect("ticked over the dock's");
    assert!(
        unseen_in(&hq, &key).is_empty(),
        "the person's tick answers it"
    );
}

/// The seen record's file under `world`'s data directory.
fn seen_file(world: &World) -> std::path::PathBuf {
    crate::hq::databases::root(&world.data_dir()).join("drive-ticks-seen.json")
}

/// A started service for the Notes Assistant on tgdrive, its chat ticked by
/// the dock: the service, the opener and the chat.
async fn dock_ticked(world: &World) -> (Arc<Hq>, Arc<LocalOpener>, sessions::AgentSessionRef) {
    let (hq, opener) = spoken_world(world, &["tgdrive"]).await;
    offers(&opener, "tgdrive", &["tgdrive"]);
    crate::registry::set_agents_default(&world.data_dir(), Some("@nixie:hq.example.org"))
        .expect("default agent");
    let first = hq
        .session_for_note(Some("tgdrive"), None)
        .expect("answers")
        .expect("a default agent");
    let key = chat(
        first.target.session_id.as_deref().expect("a chat"),
        "tgdrive",
    );
    (hq, opener, key)
}

/// Round 6 ruling: the person's own tick in the drive menu, where the
/// warning stands beside every drive, is seen when it is made; the dock's
/// is not, and warns.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_person_s_own_tick_is_seen_and_the_dock_s_warns() {
    let world = World::new();
    let (hq, _opener, dock) = dock_ticked(&world).await;
    assert_eq!(unseen_in(&hq, &dock), ["tgdrive"], "the dock's tick warns");
    let mine = hq
        .new_session("tgdrive", "nixie", "my own", None, None)
        .expect("a chat");
    hq.set_session_drive(&mine, "tgdrive", true)
        .expect("ticked");
    assert!(
        unseen_in(&hq, &mine).is_empty(),
        "the person's own tick is seen"
    );
    assert_eq!(
        unseen_in(&hq, &dock),
        ["tgdrive"],
        "another chat's seen tick of the same drive is not this one's"
    );
}

/// R605-11: a record that is missing, does not read, is an older version's,
/// or is another device's is no record: a tick answered before warns again
/// rather than pass as seen.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_lost_or_foreign_seen_record_fails_toward_the_warning() {
    let world = World::new();
    let (hq, opener, key) = dock_ticked(&world).await;
    let answered = || {
        ok(&hq, &key, "tgdrive").expect("seen");
        assert!(unseen_in(&hq, &key).is_empty(), "answered");
    };
    answered();
    std::fs::remove_file(seen_file(&world)).expect("removed");
    assert_eq!(unseen_in(&hq, &key), ["tgdrive"], "missing");
    answered();
    std::fs::write(seen_file(&world), "[[\"tgdrive\",").expect("truncated");
    assert_eq!(unseen_in(&hq, &key), ["tgdrive"], "unreadable");
    answered();
    // Round 5's shape: a list of "database\nsession\ndrive" strings.
    std::fs::write(
        seen_file(&world),
        serde_json::to_string(&[format!("tgdrive\n{}\ntgdrive", key.session_id)]).expect("json"),
    )
    .expect("legacy");
    assert_eq!(unseen_in(&hq, &key), ["tgdrive"], "an older version's");
    answered();
    // Round 6's shape: (database, session, drive) triples, with no tick.
    std::fs::write(
        seen_file(&world),
        serde_json::to_string(&[["tgdrive", key.session_id.as_str(), "tgdrive"]]).expect("json"),
    )
    .expect("round 6");
    assert_eq!(unseen_in(&hq, &key), ["tgdrive"], "round 6's");
    answered();

    // Another device, holding the same rows, has seen nothing: it warns.
    let other = World::new();
    other.paired("https://hq.example.org", &["tgdrive"]);
    let there = Hq::new(
        Arc::clone(&other.platform) as Arc<dyn Platform>,
        Arc::new(Shared(Arc::clone(&opener))) as Arc<dyn Opener>,
        Arc::clone(&other.routes) as Arc<dyn Routes>,
        Arc::new(signin::NoAccount),
        "keeper on another device".to_owned(),
        || DeviceHosts::same("other"),
    );
    there.start().await.expect("starts");
    assert_eq!(unseen_in(&there, &key), ["tgdrive"], "another device's");
    assert!(unseen_in(&hq, &key).is_empty(), "still seen here");
}

/// Opens every database as the replica `0` already opened: a second device's
/// service reading the same rows.
struct Shared(Arc<LocalOpener>);

impl Opener for Shared {
    fn open(&self, spec: ReplicaSpec) -> Result<Arc<dyn Replica>, crate::hq::port::ReplicaError> {
        Ok(self.0.replica(&spec.database))
    }
}

/// R605-12: the record holds only current ticks. Each view drops an entry
/// whose session is gone or whose drive is no longer ticked there — by the
/// person or by a selection that arrived from elsewhere — and a tick made
/// again after that warns again.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_seen_record_drops_gone_sessions_and_unticked_drives() {
    let world = World::new();
    let (hq, opener, first) = dock_ticked(&world).await;
    ok(&hq, &first, "tgdrive").expect("seen");
    let second = hq
        .new_session("tgdrive", "nixie", "another", None, None)
        .expect("a chat");
    hq.set_session_drive(&second, "tgdrive", true)
        .expect("ticked");
    let record = || std::fs::read_to_string(seen_file(&world)).expect("the record");
    assert!(record().contains(&first.session_id) && record().contains(&second.session_id));

    // The first session is deleted; a view of the second prunes it.
    opener
        .replica("tgdrive")
        .mutate(vec![
            RowOp::Delete {
                table: "session_drives".to_owned(),
                row_id: first.session_id.clone(),
                base_version: None,
            },
            RowOp::Delete {
                table: "sessions".to_owned(),
                row_id: first.session_id.clone(),
                base_version: None,
            },
        ])
        .expect("deleted");
    assert!(unseen_in(&hq, &second).is_empty());
    assert!(!record().contains(&first.session_id), "{}", record());
    assert!(record().contains(&second.session_id));

    // The second's drive is unticked by a selection from elsewhere.
    opener
        .replica("tgdrive")
        .mutate(vec![RowOp::Upsert {
            table: "session_drives".to_owned(),
            values: sessions::object(serde_json::json!({
                "session_id": second.session_id, "drives": "[]", "updated_at_ms": 9,
                "author_actor": "ana", "author_client": "ana.2"})),
            base_version: None,
        }])
        .expect("unticked elsewhere");
    assert!(unseen_in(&hq, &second).is_empty());
    assert_eq!(record(), "[]");
    // Ticked again from elsewhere: it warns, the earlier answer gone with it.
    opener
        .replica("tgdrive")
        .mutate(vec![RowOp::Upsert {
            table: "session_drives".to_owned(),
            values: sessions::object(serde_json::json!({
                "session_id": second.session_id, "drives": "[\"tgdrive\"]", "updated_at_ms": 10,
                "author_actor": "ana", "author_client": "ana.2"})),
            base_version: None,
        }])
        .expect("ticked elsewhere");
    assert_eq!(unseen_in(&hq, &second), ["tgdrive"]);
}

/// R605-13: OK and Untick tell every open view, as every other change of a
/// session does: the native stream wakes with nothing else moving.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ok_and_untick_wake_the_session_stream() {
    let world = World::new();
    let (hq, _opener, key) = dock_ticked(&world).await;
    let mut changes = hq.changes();
    assert_eq!(unseen_in(&hq, &key), ["tgdrive"]);
    // Let the dock tick's own writes settle: the stream is idle before OK.
    loop {
        changes.borrow_and_update();
        tokio::time::sleep(Duration::from_millis(300)).await;
        if !changes.has_changed().expect("open") {
            break;
        }
    }
    ok(&hq, &key, "tgdrive").expect("seen");
    assert!(changes.has_changed().expect("open"), "OK wakes the stream");
    changes.borrow_and_update();
    assert!(unseen_in(&hq, &key).is_empty());
    hq.set_session_drive(&key, "tgdrive", false)
        .expect("unticked");
    assert!(
        changes.has_changed().expect("open"),
        "Untick wakes the stream"
    );
}

/// A record that cannot be written only ever warns: the person's own tick
/// is then said once more, and OK says it could not note it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_seen_record_that_cannot_be_written_keeps_the_warning() {
    let world = World::new();
    let (hq, _opener, key) = dock_ticked(&world).await;
    std::fs::create_dir_all(seen_file(&world).with_extension("json.new")).expect("blocked");
    let refused = ok(&hq, &key, "tgdrive").expect_err("not noted");
    assert!(
        refused
            .to_string()
            .starts_with("keeper could not note that"),
        "{refused}"
    );
    assert_eq!(unseen_in(&hq, &key), ["tgdrive"]);
    let mine = hq
        .new_session("tgdrive", "nixie", "my own", None, None)
        .expect("a chat");
    hq.set_session_drive(&mine, "tgdrive", true)
        .expect("ticked");
    assert_eq!(
        unseen_in(&hq, &mine),
        ["tgdrive"],
        "unrecorded: said once more"
    );
}

/// R605-09: the person's untick and the dock's tick go through one lock.
/// However they interleave, a drive left ticked by the dock warns — never a
/// silent tick.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_person_s_untick_racing_the_dock_s_tick_never_leaves_a_silent_tick() {
    let world = World::new();
    let (hq, opener) = spoken_world(&world, &["tgdrive"]).await;
    offers(&opener, "tgdrive", &["tgdrive"]);
    crate::registry::set_agents_default(&world.data_dir(), Some("@nixie:hq.example.org"))
        .expect("default agent");
    let first = hq
        .session_for_note(Some("tgdrive"), None)
        .expect("answers")
        .expect("a default agent");
    let key = chat(
        first.target.session_id.as_deref().expect("a chat"),
        "tgdrive",
    );
    let marked = || unseen_in(&hq, &key).contains(&"tgdrive".to_owned());
    for round in 0..60 {
        std::thread::scope(|scope| {
            scope.spawn(|| {
                hq.set_session_drive(&key, "tgdrive", false)
                    .expect("unticked");
            });
            scope.spawn(|| {
                hq.session_for_note(Some("tgdrive"), None).expect("answers");
            });
        });
        let ticked = ticks(&opener, "tgdrive")
            .iter()
            .any(|(_, drives)| drives.contains("tgdrive"));
        assert!(
            !ticked || marked(),
            "round {round}: ticked by the dock, unmarked"
        );
    }
}
/// 109.28 AC 1: a note whose drive the agent does not offer opens the
/// drive's chat with nothing ticked, and says why; a drive with no database
/// here, or a note on no drive, opens the agent's main conversation and says
/// why nothing was ticked.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn the_notes_assistant_says_why_it_did_not_tick_the_notes_drive() {
    let world = World::new();
    let (hq, opener) = spoken_world(&world, &["tgdrive"]).await;
    offers(&opener, "tgdrive", &["neuradrive"]);
    // Ana reads tgdrive, so keeper knows the drive and judges it.
    opener
        .replica("tgdrive")
        .mutate(vec![RowOp::Upsert {
            table: "actors".to_owned(),
            values: sessions::object(serde_json::json!({
                "id": "ana", "kind": "person", "display_name": "Ana", "family": null,
                "dir": "tgdrive", "created_at_ms": 1, "author_actor": "server",
                "author_client": "server"})),
            base_version: None,
        }])
        .expect("directory");
    crate::registry::set_agents_default(&world.data_dir(), Some("@nixie:hq.example.org"))
        .expect("default agent");

    let unoffered = hq
        .session_for_note(Some("tgdrive"), None)
        .expect("answers")
        .expect("a default agent");
    assert_eq!(unoffered.target.database.as_deref(), Some("tgdrive"));
    assert!(unoffered.target.session_id.is_some());
    assert_eq!(
        unoffered.reason.as_deref(),
        Some("Nixie does not offer tgdrive.")
    );
    assert!(ticks(&opener, "tgdrive").is_empty());

    let main = hq.default_session().expect("reads").expect("Nixie");
    let absent = hq
        .session_for_note(Some("neuradrive"), None)
        .expect("answers")
        .expect("a default agent");
    assert_eq!(absent.target, main);
    assert_eq!(
        (absent.drive.as_deref(), absent.reason.as_deref()),
        (
            Some("neuradrive"),
            Some("This device holds no neuradrive database with Nixie in it, so neuradrive is not ticked.")
        )
    );
    let none = hq
        .session_for_note(None, None)
        .expect("answers")
        .expect("a default agent");
    assert_eq!(
        (none.target, none.drive, none.reason.as_deref()),
        (main, None, Some(drives::NO_DRIVE))
    );
    assert_eq!(
        rows(&opener.replica("tgdrive"), "SELECT id FROM sessions").len(),
        1,
        "no drive, no new chat"
    );
}

/// Directory rows for `database`: each `(id, kind)` as an actor of it.
fn directory(opener: &LocalOpener, database: &str, actors: &[(&str, &str)]) {
    let ops = actors
        .iter()
        .map(|(id, kind)| RowOp::Upsert {
            table: "actors".to_owned(),
            values: sessions::object(serde_json::json!({
                "id": id, "kind": kind, "display_name": id, "family": null, "dir": database,
                "created_at_ms": 1, "author_actor": "server", "author_client": "server"})),
            base_version: None,
        })
        .collect();
    opener.replica(database).mutate(ops).expect("directory");
}

/// The `metadata` of `database`'s message whose body is `body`, parsed;
/// `Null` when it has none.
fn metadata_of(opener: &LocalOpener, database: &str, body: &str) -> Value {
    let stored = rows(
        &opener.replica(database),
        "SELECT body, metadata FROM messages",
    );
    let row = stored
        .iter()
        .find(|row| row["body"] == body)
        .unwrap_or_else(|| panic!("{body:?} in {database}: {stored:?}"));
    match &row["metadata"] {
        Value::String(text) => serde_json::from_str::<Value>(text).expect("JSON"),
        other => other.clone(),
    }
}

fn inbox_focus(drive: &str) -> send::AgentFocus {
    send::AgentFocus::new(
        drive,
        "10-notes/inbox.md",
        "# Plans\n\nintro\n## Q3\nship it",
        5,
    )
    .expect("in bounds")
}

/// 109.28 AC 2 (contract §9.1, binding): every message the Notes Assistant
/// writes with the note in front — an ordinary line, a captionless ask
/// answer through `send`, the first message of a chat it starts, and the
/// first message of an `@` thread (R605-03) — carries `metadata.focus`:
/// drive, path, the heading trail of the caret's line and the line, as the
/// replica stores it for the runtime. A message sent without one carries no
/// metadata at all.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_sent_from_the_notes_assistant_carries_the_note_as_focus() {
    let world = World::new();
    let (hq, opener) = spoken_world(&world, &["tgdrive"]).await;
    directory(&opener, "tgdrive", &[("tola", "agent")]);
    let focus = inbox_focus("tgdrive");
    let key = hq
        .new_session(
            "tgdrive",
            "nixie",
            "a new chat from the note",
            None,
            Some(&focus),
        )
        .expect("a chat");
    hq.send(&key, "what am I looking at?", &[], Some(&focus))
        .expect("sent");
    hq.send_line(&key, "and this?", &[], Some(&focus))
        .expect("sent");
    hq.send_line(&key, "@tola what is this note?", &[], Some(&focus))
        .expect("a thread");
    hq.send(&key, "from the Agents window", &[], None)
        .expect("sent");

    let expected = serde_json::json!({"focus": {
        "drive": "tgdrive", "path": "10-notes/inbox.md", "heading": "Plans › Q3", "line": 5}});
    for body in [
        "a new chat from the note",
        "what am I looking at?",
        "and this?",
        "@tola what is this note?",
    ] {
        assert_eq!(metadata_of(&opener, "tgdrive", body), expected, "{body}");
    }
    assert_eq!(
        metadata_of(&opener, "tgdrive", "from the Agents window"),
        Value::Null
    );
}

/// R605-01 (D-34): a note's path and heading are its drive's, so a message
/// carries them only into a conversation everyone of whose readers reads
/// that drive. A database read by someone the drive's readers do not
/// include, or one whose readers keeper cannot compare (the drive's
/// database is not held here), gets the message without its focus — on a
/// line, an `@` thread, and a chat started from the note alike.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_note_s_focus_never_reaches_a_wider_audience_than_its_drive() {
    let world = World::new();
    let (hq, opener) = spoken_world(&world, &["tgdrive", "beta", "solo"]).await;
    directory(&opener, "tgdrive", &[("ana", "person"), ("ben", "person")]);
    directory(
        &opener,
        "beta",
        &[
            ("ana", "person"),
            ("ben", "person"),
            ("cy", "person"),
            ("tola", "agent"),
        ],
    );
    directory(&opener, "solo", &[("ana", "person")]);
    let tg = inbox_focus("tgdrive");
    let elsewhere = inbox_focus("neuradrive");

    // beta is also read by Cy, who does not read tgdrive.
    let wide = hq
        .new_session("beta", "nixie", "started in beta", None, Some(&tg))
        .expect("a beta chat");
    hq.send(&wide, "sent to beta", &[], Some(&tg))
        .expect("sent");
    hq.send_line(&wide, "a line to beta", &[], Some(&tg))
        .expect("sent");
    hq.send_line(&wide, "@tola in beta", &[], Some(&tg))
        .expect("a thread");
    for body in [
        "started in beta",
        "sent to beta",
        "a line to beta",
        "@tola in beta",
    ] {
        assert_eq!(metadata_of(&opener, "beta", body), Value::Null, "{body}");
    }

    // solo is read by Ana alone, who reads tgdrive: the focus goes.
    let narrow = hq
        .new_session("solo", "nixie", "started in solo", None, Some(&tg))
        .expect("a solo chat");
    hq.send(&narrow, "tgdrive's note to solo", &[], Some(&tg))
        .expect("sent");
    assert_eq!(
        metadata_of(&opener, "solo", "tgdrive's note to solo")["focus"]["drive"],
        Value::from("tgdrive")
    );
    assert_eq!(
        metadata_of(&opener, "solo", "started in solo")["focus"]["drive"],
        Value::from("tgdrive")
    );
    // neuradrive's readers are unknown here: no focus.
    hq.send(&narrow, "neuradrive's note to solo", &[], Some(&elsewhere))
        .expect("sent");
    assert_eq!(
        metadata_of(&opener, "solo", "neuradrive's note to solo"),
        Value::Null
    );
}

/// Record, as a round would, the server refusing commit `commit` with
/// `code`; it wrote `ops` (`table`, `op`, values).
fn journal_refused(path: &std::path::Path, commit: &str, code: &str, ops: &[(&str, &str, Value)]) {
    use syncular_client::api::{CommitOperation, CommitOperationOutcome, RejectionRecord};
    let operations: Vec<CommitOperation> = ops
        .iter()
        .map(|(table, op, values)| CommitOperation {
            table: (*table).to_owned(),
            row_id: values["id"].as_str().unwrap_or_default().to_owned(),
            op: (*op).to_owned(),
            base_version: None,
            values: values.as_object().cloned(),
        })
        .collect();
    let results = vec![CommitOperationOutcome::Error {
        rejection: RejectionRecord {
            client_commit_id: commit.to_owned(),
            op_index: 0,
            code: code.to_owned(),
            message: code.to_owned(),
            retryable: false,
            details: None,
            operation: None,
        },
    }];
    let conn = rusqlite::Connection::open(path).expect("opens");
    conn.busy_timeout(Duration::from_secs(5)).expect("timeout");
    conn.execute(
        "INSERT INTO _syncular_commit_outcomes(client_commit_id, status, recorded_at_ms, \
         results_json, operations_json) VALUES (?1, 'rejected', 1, ?2, ?3)",
        rusqlite::params![
            commit,
            serde_json::to_string(&results).expect("results"),
            serde_json::to_string(&operations).expect("operations")
        ],
    )
    .expect("records");
}

/// Contract § 9.1a (R605-04): the server, not this device, decides who may
/// read a note's focus. A message it refuses for that (`hq.focus.audience`),
/// or for a focus that is not well-formed (`hq.focus.malformed`, here one
/// whose line is `42.0`), is written again once, the same message without
/// the focus — never
/// dropped — and the person reads why under it. Another refusal is left as
/// it stands, and a second pass writes nothing more.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_message_the_server_refuses_for_its_note_goes_again_without_it() {
    let world = World::new();
    let (hq, opener) = spoken_world(&world, &["beta"]).await;
    let key = hq
        .new_session("beta", "nixie", "hello", None, None)
        .expect("a chat");
    let source = hq.source("beta").expect("beta");
    let at = sessions::now_ms();
    let focused = |id: &str, body: &str| match sessions::person_message(
        &source,
        &key.session_id,
        id,
        body,
        Some(inbox_focus("tgdrive").metadata()),
        at,
    ) {
        RowOp::Upsert { values, .. } => Value::Object(values),
        _ => unreachable!("a message is an upsert"),
    };
    let activity = serde_json::json!({"id": key.session_id, "last_activity_ms": at,
        "author_actor": "ana", "author_client": "ana.1"});
    let path = opener.path("beta");
    journal_refused(
        &path,
        "c-focus",
        "hq.focus.audience",
        &[
            (
                "messages",
                "upsert",
                focused("m-focus", "what is this note?"),
            ),
            // A patch is journaled as an upsert of the values it wrote.
            ("sessions", "upsert", activity.clone()),
        ],
    );
    let mut broken = focused("m-broken", "and this one?");
    broken["metadata"] = Value::from(
        serde_json::json!({"focus": {"drive": "tgdrive", "path": "a.md", "heading": null,
                                     "line": 42.0}})
        .to_string(),
    );
    journal_refused(
        &path,
        "c-broken",
        "hq.focus.malformed",
        &[("messages", "upsert", broken)],
    );
    journal_refused(
        &path,
        "c-other",
        "hq.v5.page",
        &[(
            "messages",
            "upsert",
            focused("m-other", "refused otherwise"),
        )],
    );

    hq.resend_withheld("beta");
    hq.resend_withheld("beta");

    let stored = rows(
        &opener.replica("beta"),
        "SELECT id, body, metadata FROM messages WHERE id IN ('m-focus', 'm-broken', \
         'm-other') ORDER BY id",
    );
    let ids: Vec<&Value> = stored.iter().map(|row| &row["id"]).collect();
    assert_eq!(
        ids,
        [&Value::from("m-broken"), &Value::from("m-focus")],
        "once each, and only the note's refusals: {stored:?}"
    );
    for body in ["what is this note?", "and this one?"] {
        assert_eq!(metadata_of(&opener, "beta", body), Value::Null, "{body}");
    }
    let refused: Vec<String> = source
        .replica
        .refused_commits("messages")
        .expect("reads")
        .into_iter()
        .map(|commit| commit.commit_id)
        .collect();
    assert_eq!(refused, ["c-other"], "the note's refusals are set aside");

    let view = hq.session_view(&key).expect("a view");
    let said: Vec<(&str, Option<&str>)> = view
        .messages
        .iter()
        .map(|message| (message.body.as_str(), message.note.as_deref()))
        .collect();
    assert!(
        said.contains(&(
            "what is this note?",
            Some("This chat's readers can't all read tgdrive, so the note you're in wasn't sent.")
        )),
        "{said:?}"
    );
    assert!(
        said.contains(&(
            "and this one?",
            Some("The note you're in couldn't be sent with this message.")
        )),
        "{said:?}"
    );
    assert!(said.contains(&("hello", None)), "{said:?}");
}

/// A `session_drives` write from another device of the same person: `drives`
/// ticked, with `tick_ids` as given (`None`: a schema-4 client's write, with
/// no ids).
fn selection_from_elsewhere(
    opener: &LocalOpener,
    key: &sessions::AgentSessionRef,
    drives: &[&str],
    tick_ids: Option<Value>,
    at: i64,
) {
    let mut values = serde_json::json!({
        "session_id": key.session_id, "drives": Value::from(drives.to_vec()).to_string(),
        "updated_at_ms": at, "author_actor": "ana", "author_client": "ana.2"});
    if let Some(ids) = tick_ids {
        values["tick_ids"] = Value::from(ids.to_string());
    }
    opener
        .replica(&key.database)
        .mutate(vec![RowOp::Upsert {
            table: "session_drives".to_owned(),
            values: sessions::object(values),
            base_version: None,
        }])
        .expect("written elsewhere");
}

/// R605-14, the reviewer's P1: the person unticks while the seen record
/// cannot be written, and the disk comes back before the dock ticks again.
/// The new tick has a new id, so the old answer acknowledges nothing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn p1_a_lost_untick_record_never_acknowledges_the_next_tick() {
    let world = World::new();
    let (hq, opener, key) = dock_ticked(&world).await;
    ok(&hq, &key, "tgdrive").expect("seen");
    let blocked = seen_file(&world).with_extension("json.new");
    std::fs::create_dir_all(&blocked).expect("blocked");
    hq.set_session_drive(&key, "tgdrive", false)
        .expect("unticked");
    assert!(
        unseen_in(&hq, &key).is_empty(),
        "unticked: nothing to warn of"
    );
    std::fs::remove_dir_all(&blocked).expect("unblocked");
    hq.session_for_note(Some("tgdrive"), None).expect("answers");
    assert_eq!(ticks(&opener, "tgdrive")[0].1, r#"["tgdrive"]"#);
    assert_eq!(unseen_in(&hq, &key), ["tgdrive"]);
}

/// R605-14, P2: another device unticks, and this device's dock ticks again
/// before any view is built here.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn p2_an_untick_elsewhere_never_seen_here_leaves_the_dock_s_re_tick_warning() {
    let world = World::new();
    let (hq, opener, key) = dock_ticked(&world).await;
    ok(&hq, &key, "tgdrive").expect("seen");
    selection_from_elsewhere(
        &opener,
        &key,
        &[],
        Some(serde_json::json!({"at": 5, "drives": {}})),
        5,
    );
    hq.session_for_note(Some("tgdrive"), None).expect("answers");
    assert_eq!(unseen_in(&hq, &key), ["tgdrive"]);
}

/// R605-14, P3: another device unticks and ticks again — or only the final
/// row arrives — and this device builds no view in between. Whether that
/// device wrote a fresh id (schema 5) or none (schema 4), it warns here.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn p3_an_untick_and_re_tick_elsewhere_warns_here() {
    let world = World::new();
    let (hq, opener, key) = dock_ticked(&world).await;
    ok(&hq, &key, "tgdrive").expect("seen");
    selection_from_elsewhere(
        &opener,
        &key,
        &[],
        Some(serde_json::json!({"at": 5, "drives": {}})),
        5,
    );
    selection_from_elsewhere(
        &opener,
        &key,
        &["tgdrive"],
        Some(serde_json::json!({"at": 6, "drives": {"tgdrive": "01JELSEWHERE0000000000000"}})),
        6,
    );
    assert_eq!(
        unseen_in(&hq, &key),
        ["tgdrive"],
        "a fresh id from elsewhere"
    );
    ok(&hq, &key, "tgdrive").expect("seen");
    assert!(unseen_in(&hq, &key).is_empty());
    // A schema-4 device re-ticks: it leaves the old map behind and moves
    // `updated_at_ms`, so the map no longer counts and the tick has no id.
    selection_from_elsewhere(
        &opener,
        &key,
        &["tgdrive"],
        Some(serde_json::json!({"at": 6, "drives": {"tgdrive": "01JELSEWHERE0000000000000"}})),
        7,
    );
    assert_eq!(
        unseen_in(&hq, &key),
        ["tgdrive"],
        "an older device's re-tick"
    );
}

/// A tick with no id — a schema-4 client's or the server's — warns, and OK
/// gives it one, as the person, so that this one tick is acknowledged.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_tick_without_an_id_warns_and_ok_gives_it_one() {
    let world = World::new();
    let (hq, opener, key) = dock_ticked(&world).await;
    selection_from_elsewhere(&opener, &key, &["tgdrive"], None, 8);
    let shown = unseen_ticks_in(&hq, &key);
    assert_eq!(
        shown,
        [drives::AgentUnseenTickVm {
            drive: "tgdrive".to_owned(),
            tick: None
        }]
    );
    ok(&hq, &key, "tgdrive").expect("seen");
    assert!(unseen_in(&hq, &key).is_empty());
    let source = hq.source("tgdrive").expect("held");
    assert!(drives::tick_ids(&source, &key.session_id).contains_key("tgdrive"));
}

/// OK names the tick its warning showed: shown for a tick that has since
/// been replaced, it acknowledges nothing, and the new one still warns.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ok_for_a_replaced_tick_acknowledges_nothing() {
    let world = World::new();
    let (hq, opener, key) = dock_ticked(&world).await;
    let shown = unseen_ticks_in(&hq, &key)[0].tick.clone().expect("an id");
    selection_from_elsewhere(
        &opener,
        &key,
        &["tgdrive"],
        Some(serde_json::json!({"at": 9, "drives": {"tgdrive": "01JREPLACED000000000000000"}})),
        9,
    );
    hq.note_tick_seen(&key, "tgdrive", Some(&shown))
        .expect("asked");
    assert_eq!(unseen_in(&hq, &key), ["tgdrive"]);
}

/// The record's file, as entries.
fn seen_entries(world: &World) -> Vec<Value> {
    serde_json::from_str(&std::fs::read_to_string(seen_file(world)).expect("the record"))
        .expect("entries")
}

fn write_seen(world: &World, entries: &[Value]) {
    std::fs::write(seen_file(world), Value::from(entries.to_vec()).to_string()).expect("written");
}

/// R605-15: an answer belongs to one server and one person. Another
/// server's or another person's entry for the same database, session, drive
/// and even tick id acknowledges nothing here — and pruning leaves it be.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn an_answer_on_another_server_or_by_another_person_is_not_this_one() {
    let world = World::new();
    let (hq, _opener, key) = dock_ticked(&world).await;
    let tick = unseen_ticks_in(&hq, &key)[0].tick.clone().expect("an id");
    let entry = |server: &str, person: &str| {
        serde_json::json!({"server": server, "person": person, "database": "tgdrive",
                           "session": key.session_id, "drive": "tgdrive", "tick": tick})
    };
    let elsewhere = [
        entry("https://other.example.org", "ana"),
        entry("https://hq.example.org", "ben"),
    ];
    write_seen(&world, &elsewhere);
    assert_eq!(unseen_in(&hq, &key), ["tgdrive"]);
    assert_eq!(seen_entries(&world).len(), 2, "left alone");
    ok(&hq, &key, "tgdrive").expect("seen");
    assert!(unseen_in(&hq, &key).is_empty());
    assert_eq!(seen_entries(&world).len(), 3);
}

/// R605-16: pruning never touches an entry of a database this device does
/// not hold; it drops this server's and person's stale entry of one it does.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn pruning_leaves_databases_not_held_alone() {
    let world = World::new();
    let (hq, _opener, key) = dock_ticked(&world).await;
    let foreign = serde_json::json!({"server": "https://hq.example.org", "person": "ana",
        "database": "neuradrive", "session": "ana.1.1", "drive": "neuradrive", "tick": "01JFOREIGN"});
    let stale = serde_json::json!({"server": "https://hq.example.org", "person": "ana",
        "database": "tgdrive", "session": key.session_id, "drive": "tgdrive", "tick": "01JSTALE"});
    write_seen(&world, &[foreign.clone(), stale]);
    assert_eq!(
        unseen_in(&hq, &key),
        ["tgdrive"],
        "a stale entry acknowledges nothing"
    );
    assert_eq!(
        seen_entries(&world),
        [foreign],
        "the stale one pruned, the foreign one kept"
    );
}

/// `/clean` creates new grants, not acknowledgements inherited from the old
/// conversation, even when every carried drive was already seen here.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn clean_carries_a_fresh_tick_that_warns_until_its_own_ok() {
    let world = World::new();
    let (hq, _opener, key) = dock_ticked(&world).await;
    let old_tick = unseen_ticks_in(&hq, &key)[0].tick.clone().expect("an id");
    ok(&hq, &key, "tgdrive").expect("old grant seen");
    let result = hq.command(&key, "/clean", None, &[]).expect("clean");
    let fresh = result.session.expect("the new conversation");
    let carried = unseen_ticks_in(&hq, &fresh);
    assert_eq!(carried.len(), 1);
    assert_eq!(carried[0].drive, "tgdrive");
    let new_tick = carried[0].tick.as_deref().expect("a carried tick id");
    assert_ne!(new_tick, old_tick, "a new chat has a new grant");
    assert!(ulid::Ulid::from_string(new_tick).is_ok());
    assert!(unseen_in(&hq, &key).is_empty(), "the old answer holds");
    ok(&hq, &fresh, "tgdrive").expect("new grant seen");
    assert!(unseen_in(&hq, &fresh).is_empty());
}
