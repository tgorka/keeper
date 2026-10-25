//! Sign-in enrolment against a loopback server shaped like keeper-hq's 109.5
//! routes: what keeper sends, to whom, when, and what it keeps.

use std::sync::atomic::{AtomicUsize, Ordering};

use super::*;
use crate::forges::testing::{serve, Fake, FakePlatform, Reply, Seen};
use crate::hq::port::fake::FakeOpener;
use crate::hq::signin::{AccountSignIn, AccountState, Bound, SignIn, TokenAnswer, TokenError};
use crate::hq::vm::HqSignInState;

const ISSUER: &str = "https://id.example.org";
const CLIENT: &str = "keeper-app";
const SCOPES: [&str; 2] = ["openid", "urn:zitadel:iam:org:project:id:42:aud"];
const PAIRED_TOKEN: &str = "0123456789abcdef";

/// The account: signed in or not, and every token it minted.
struct FakeSignIn {
    signed_in: Mutex<bool>,
    minted: AtomicUsize,
}

impl FakeSignIn {
    fn new(signed_in: bool) -> Arc<FakeSignIn> {
        Arc::new(FakeSignIn {
            signed_in: Mutex::new(signed_in),
            minted: AtomicUsize::new(0),
        })
    }
}

fn account() -> AccountSignIn {
    AccountSignIn {
        issuer: ISSUER.to_owned(),
        client_id: CLIENT.to_owned(),
        scopes: SCOPES.iter().map(|s| (*s).to_owned()).collect(),
    }
}

impl SignIn for FakeSignIn {
    fn account(&self) -> AccountState {
        if *lock(&self.signed_in) {
            AccountState::SignedIn(account())
        } else {
            AccountState::SignedOut(account())
        }
    }
    fn fresh_token(&self) -> TokenAnswer<'_> {
        let n = self.minted.fetch_add(1, Ordering::SeqCst) + 1;
        let signed_in = *lock(&self.signed_in);
        Box::pin(async move {
            if signed_in {
                Ok(format!("signin-token-{n}"))
            } else {
                Err(TokenError::NeedsSignIn("signed out".to_owned()))
            }
        })
    }
}

#[derive(Debug, Clone)]
struct Device {
    token: String,
    client: String,
    bound: bool,
    lapsed: bool,
}

/// keeper-hq as 109.5 serves it, for one person `ana` in database `alpha`.
#[derive(Default)]
struct Server {
    issuer: String,
    devices: Vec<Device>,
    spent: Vec<String>,
    minted: usize,
}

fn discovery(client: &str) -> String {
    format!(r#"{{"actorId":"ana","clientId":"{client}","databases":[{{"name":"alpha"}}]}}"#)
}

fn bearer(seen: &Seen) -> String {
    seen.header("authorization")
        .and_then(|h| h.strip_prefix("Bearer "))
        .unwrap_or_default()
        .to_owned()
}

fn hq_server(state: Arc<Mutex<Server>>) -> Fake {
    serve(move |seen| {
        let mut server = lock(&state);
        match (seen.method.as_str(), seen.path.as_str()) {
            ("GET", "/auth/config") => Reply::json(
                200,
                &serde_json::json!({
                    "issuer": server.issuer,
                    "clientId": CLIENT,
                    "projectId": "42",
                    "role": "user",
                    "scopes": SCOPES,
                })
                .to_string(),
            ),
            ("POST", "/devices/enroll") => {
                let sign_in = bearer(seen);
                if server.spent.contains(&sign_in) {
                    return Reply::json(401, r#"{"error":"hq.signin.replayed"}"#);
                }
                server.spent.push(sign_in);
                server.minted += 1;
                let token = format!("device-token-{}", server.minted);
                let body = seen.json();
                if body["host"].as_str().unwrap_or("").is_empty() {
                    return Reply::json(400, r#"{"error":"hq.device_host"}"#);
                }
                let (client, adopted) = match body["deviceToken"].as_str() {
                    Some(old) => {
                        let Some(device) = server.devices.iter_mut().find(|d| d.token == old)
                        else {
                            return Reply::json(401, r#"{"error":"hq.device_unknown"}"#);
                        };
                        device.token = token.clone();
                        device.bound = true;
                        device.lapsed = false;
                        (device.client.clone(), true)
                    }
                    None => {
                        let client = format!("ana.new{}", server.minted);
                        server.devices.push(Device {
                            token: token.clone(),
                            client: client.clone(),
                            bound: true,
                            lapsed: false,
                        });
                        (client, false)
                    }
                };
                Reply::json(
                    200,
                    &serde_json::json!({
                        "token": token,
                        "clientId": client,
                        "actorId": "ana",
                        "databases": [{ "name": "alpha", "mount": "/alpha" }],
                        "adopted": adopted,
                    })
                    .to_string(),
                )
            }
            ("POST", "/clients/self/attest") => {
                let token = seen.json()["deviceToken"].as_str().unwrap_or("").to_owned();
                match server.devices.iter().find(|d| d.token == token) {
                    Some(d) if d.bound => Reply::json(
                        200,
                        r#"{"clientId":"x","attestedAtMs":1000,"lapsesAtMs":604801000}"#,
                    ),
                    Some(_) => Reply::json(409, r#"{"error":"hq.attest.not_enrolled"}"#),
                    None => Reply::json(401, r#"{"error":"hq.device_unknown"}"#),
                }
            }
            ("GET", "/databases") => {
                let token = bearer(seen);
                match server.devices.iter().find(|d| d.token == token) {
                    Some(d) if !d.lapsed => Reply::json(200, &discovery(&d.client)),
                    _ => Reply::json(401, r#"{"error":"sync.auth_required"}"#),
                }
            }
            ("GET", "/clients/self/enrolments") => Reply::json(
                200,
                r#"{"enrolments":[
                    {"clientId":"ana.k","name":"keeper","host":"kalypso","kind":"enrolled","atMs":2,"self":false},
                    {"clientId":"ana.1","name":"keeper","host":"hesperia","kind":"adopted","atMs":1,"self":true}]}"#,
            ),
            _ => Reply::json(404, r#"{"error":"not found"}"#),
        }
    })
}

struct World {
    platform: Arc<FakePlatform>,
    state: Arc<Mutex<Server>>,
    fake: Fake,
    sign_in: Arc<FakeSignIn>,
    host: Arc<Mutex<String>>,
}

impl World {
    fn new(signed_in: bool) -> World {
        let state = Arc::new(Mutex::new(Server {
            issuer: ISSUER.to_owned(),
            ..Server::default()
        }));
        World {
            platform: Arc::new(FakePlatform::default()),
            fake: hq_server(Arc::clone(&state)),
            state,
            sign_in: FakeSignIn::new(signed_in),
            host: Arc::new(Mutex::new("hesperia".to_owned())),
        }
    }

    fn hq(&self) -> Arc<Hq> {
        let host = Arc::clone(&self.host);
        Hq::new(
            Arc::clone(&self.platform) as Arc<dyn Platform>,
            Arc::new(FakeOpener::default()),
            Arc::new(routes::HttpRoutes::new()),
            Arc::clone(&self.sign_in) as Arc<dyn SignIn>,
            "keeper".to_owned(),
            move || lock(&host).clone(),
        )
    }

    fn server(&self) -> ServerUrl {
        ServerUrl::parse(&self.fake.base)
            .expect("parses")
            .expect("an address")
    }

    /// This device holds `token` for `client`: a pairing (`bound` none) or a
    /// sign-in's, and the server knows it so.
    fn holds(&self, token: &str, client: &str, bound: Option<Bound>, lapsed: bool) {
        let server = self.server();
        crate::registry::set_agents_server_url(
            &self.platform.data_dir().expect("dir"),
            Some(&server),
        )
        .expect("stores");
        let pairing = Pairing {
            token: token.to_owned(),
            actor_id: "ana".to_owned(),
            client_id: client.to_owned(),
            name: "keeper".to_owned(),
            bound,
        };
        self.platform
            .keychain_set(
                &keychain_key(&server),
                &serde_json::to_string(&pairing).expect("json"),
            )
            .expect("keychain");
        lock(&self.state).devices.push(Device {
            token: token.to_owned(),
            client: client.to_owned(),
            bound: bound.is_some(),
            lapsed,
        });
    }

    fn kept(&self) -> Option<Pairing> {
        self.platform
            .keychain_get(&keychain_key(&self.server()))
            .expect("keychain")
            .map(|raw| serde_json::from_str(&raw).expect("a pairing"))
    }

    fn asked(&self, path: &str) -> Vec<Seen> {
        self.fake
            .requests()
            .into_iter()
            .filter(|seen| seen.path == path)
            .collect()
    }
}

async fn status_until(hq: &Hq, what: &str, ok: impl Fn(&HqStatusVm) -> bool) -> HqStatusVm {
    let mut watch = hq.status();
    let reached = tokio::time::timeout(Duration::from_secs(5), watch.wait_for(|s| ok(s))).await;
    let status = hq.status().borrow().clone();
    assert!(reached.is_ok(), "{what}: {status:#?}");
    status
}

fn enrolled(status: &HqStatusVm) -> bool {
    status.phase == HqPhase::Paired && status.sign_in.state == HqSignInState::Enrolled
}

#[tokio::test]
async fn a_signed_in_device_enrols_with_the_host_of_the_moment_and_keeps_the_token_in_the_keychain()
{
    let world = World::new(true);
    let hq = world.hq();
    // The OS host is known only after construction (109.1): enrolment reads it then.
    *lock(&world.host) = "kalypso".to_owned();
    hq.start().await.expect("starts");
    hq.set_server(&world.fake.base).await.expect("sets");
    let status = status_until(&hq, "enrolled", enrolled).await;

    let enrol = world.asked("/devices/enroll");
    assert_eq!(enrol.len(), 1);
    assert_eq!(bearer(&enrol[0]), "signin-token-1");
    assert_eq!(enrol[0].json()["host"], "kalypso");
    assert_eq!(enrol[0].json()["name"], "keeper");
    assert!(enrol[0].json().get("deviceToken").is_none(), "a new device");
    let kept = world.kept().expect("kept");
    assert_eq!(kept.token, "device-token-1");
    assert_eq!(kept.client_id, "ana.new1");
    assert!(kept.bound.is_some());
    assert_eq!(
        status.device.as_ref().expect("device").client_id,
        "ana.new1"
    );
    // Signed in just now: nothing to attest yet, and nothing in the view of a token.
    assert!(world.asked("/clients/self/attest").is_empty());
    let shown = serde_json::to_string(&status).expect("json");
    assert!(!shown.contains("device-token") && !shown.contains("signin-token"));
}

#[tokio::test]
async fn no_token_leaves_for_a_server_that_trusts_another_sign_in_and_a_code_stays_the_way_in() {
    let world = World::new(true);
    lock(&world.state).issuer = "https://id.example.org/".to_owned();
    let hq = world.hq();
    hq.start().await.expect("starts");
    hq.set_server(&world.fake.base).await.expect("sets");
    let status = status_until(&hq, "refused", |s| {
        s.sign_in.state == HqSignInState::Refused
    })
    .await;
    assert_eq!(status.phase, HqPhase::NotPaired);
    assert!(
        status.sentence.contains("Type the code"),
        "{}",
        status.sentence
    );
    assert_eq!(
        world.sign_in.minted.load(Ordering::SeqCst),
        0,
        "no token minted"
    );
    assert!(world.asked("/devices/enroll").is_empty());
    // Reconciling again tries nothing new against the same account.
    hq.reconcile().await.expect("reconciles");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(world.asked("/auth/config").len(), 1);
}

#[tokio::test]
async fn signed_out_nothing_is_minted_and_signing_in_connects_without_a_code() {
    let world = World::new(false);
    let hq = world.hq();
    hq.start().await.expect("starts");
    hq.set_server(&world.fake.base).await.expect("sets");
    let status = status_until(&hq, "signed out", |s| {
        s.sign_in.state == HqSignInState::SignedOut
    })
    .await;
    assert!(
        status.sentence.contains("not connected") && !status.sentence.contains("code"),
        "{}",
        status.sentence
    );
    assert!(world.asked("/devices/enroll").is_empty());
    assert_eq!(
        world.sign_in.minted.load(Ordering::SeqCst),
        0,
        "signed out, no token is asked for"
    );
    *lock(&world.sign_in.signed_in) = true;
    hq.reconcile().await.expect("reconciles");
    status_until(&hq, "enrolled", enrolled).await;
    assert_eq!(world.asked("/devices/enroll").len(), 1);
}

#[tokio::test]
async fn a_paired_device_is_adopted_under_its_client_id_with_its_token_in_the_body() {
    let world = World::new(true);
    world.holds(PAIRED_TOKEN, "ana.1", None, false);
    let hq = world.hq();
    hq.start().await.expect("starts");
    let status = status_until(&hq, "adopted", enrolled).await;

    let enrol = world.asked("/devices/enroll");
    assert_eq!(enrol.len(), 1);
    assert_eq!(enrol[0].json()["deviceToken"], PAIRED_TOKEN);
    assert!(enrol[0].query.is_empty(), "never a token in the URL");
    assert_eq!(bearer(&enrol[0]), "signin-token-1");
    let kept = world.kept().expect("kept");
    assert_eq!(kept.client_id, "ana.1", "the client id stays");
    assert_eq!(kept.token, "device-token-1", "the token is rotated");
    assert_eq!(status.device.expect("device").client_id, "ana.1");
}

#[tokio::test]
async fn an_attestation_is_made_once_due_and_not_before() {
    let day = 24 * 60 * 60 * 1000;
    // Attested five days ago: not due.
    let world = World::new(true);
    let recent = Bound {
        attested_at_ms: sessions::now_ms() - 5 * day,
        window_ms: 7 * day,
    };
    world.holds("device-token-a", "ana.1", Some(recent), false);
    let hq = world.hq();
    hq.start().await.expect("starts");
    status_until(&hq, "enrolled", enrolled).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(world.asked("/clients/self/attest").is_empty());
    assert!(world.asked("/devices/enroll").is_empty());
    hq.shutdown().await;

    // Attested six and a half days ago: due, with a token minted for it.
    let world = World::new(true);
    let due = Bound {
        attested_at_ms: sessions::now_ms() - 13 * day / 2,
        window_ms: 7 * day,
    };
    world.holds("device-token-b", "ana.1", Some(due), false);
    let hq = world.hq();
    hq.start().await.expect("starts");
    for _ in 0..50 {
        if world
            .kept()
            .and_then(|kept| kept.bound)
            .is_some_and(|bound| bound.attested_at_ms > due.attested_at_ms)
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let attest = world.asked("/clients/self/attest");
    assert_eq!(attest.len(), 1);
    assert_eq!(bearer(&attest[0]), "signin-token-1");
    assert_eq!(attest[0].json()["deviceToken"], "device-token-b");
    let kept = world.kept().expect("kept").bound.expect("bound");
    assert!(
        kept.attested_at_ms > due.attested_at_ms,
        "the new attestation is kept"
    );
    assert_eq!(kept.window_ms, 604_800_000, "the server's window");
}

#[tokio::test]
async fn a_lapsed_sign_in_signs_in_again_silently_keeping_its_client_id() {
    let world = World::new(true);
    let old = Bound {
        attested_at_ms: sessions::now_ms(),
        window_ms: 7 * 24 * 60 * 60 * 1000,
    };
    // The server suspended it (S-ID-9): the token no longer opens anything.
    world.holds("device-token-old", "ana.1", Some(old), true);
    let hq = world.hq();
    hq.start().await.expect("starts");
    let status = status_until(&hq, "signed in again", |s| {
        enrolled(s) && world.asked("/devices/enroll").len() == 1
    })
    .await;
    let enrol = world.asked("/devices/enroll");
    assert_eq!(enrol[0].json()["deviceToken"], "device-token-old");
    assert_eq!(world.kept().expect("kept").client_id, "ana.1");
    assert_eq!(status.device.expect("device").client_id, "ana.1");
    assert_ne!(world.kept().expect("kept").token, "device-token-old");
}

#[tokio::test]
async fn neither_token_is_ever_logged() {
    use tracing_subscriber::fmt::MakeWriter;

    #[derive(Clone, Default)]
    struct Captured(Arc<Mutex<Vec<u8>>>);
    impl std::io::Write for Captured {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            lock(&self.0).extend_from_slice(bytes);
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    impl<'a> MakeWriter<'a> for Captured {
        type Writer = Captured;
        fn make_writer(&'a self) -> Captured {
            self.clone()
        }
    }
    let captured = Captured::default();
    let subscriber = tracing_subscriber::fmt()
        .with_ansi(false)
        .with_max_level(tracing::Level::TRACE)
        .with_writer(captured.clone())
        .finish();
    let _logging = tracing::subscriber::set_default(subscriber);

    // An adoption (both tokens in play) and a lapse refused once on the way.
    let world = World::new(true);
    world.holds(PAIRED_TOKEN, "ana.1", None, false);
    let hq = world.hq();
    hq.start().await.expect("starts");
    status_until(&hq, "adopted", enrolled).await;
    let notices = hq.enrolments().await.expect("notices");
    assert_eq!(notices.len(), 1, "this device's own notice is not shown");
    hq.shutdown().await;

    let logged = String::from_utf8(lock(&captured.0).clone()).expect("utf-8");
    assert!(
        logged.contains("signed in to its agents server"),
        "{logged}"
    );
    for secret in [PAIRED_TOKEN, "signin-token", "device-token"] {
        assert!(!logged.contains(secret), "{secret} logged:\n{logged}");
    }
}
