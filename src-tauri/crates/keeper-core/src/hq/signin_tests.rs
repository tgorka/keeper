//! Sign-in enrolment against a loopback server shaped like keeper-hq's 109.5
//! routes: what keeper sends, to whom, when, and what it keeps — including
//! while the account, the sign-in or the server changes under an attempt.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;

use super::*;
use crate::forges::testing::{serve, Fake, FakePlatform, Reply, Seen};
use crate::hq::port::fake::FakeOpener;
use crate::hq::signin::{AccountSignIn, AccountState, Bound, SignIn, TokenAnswer, TokenError};
use crate::hq::vm::HqSignInState;

const ISSUER: &str = "https://id.example.org";
const OTHER_ISSUER: &str = "https://id.elsewhere.org";
const CLIENT: &str = "keeper-app";
const SCOPES: [&str; 2] = ["openid", "urn:zitadel:iam:org:project:id:42:aud"];
const PAIRED_TOKEN: &str = "0123456789abcdef";

/// The account as the fake sign-in has it now.
#[derive(Clone)]
struct Account {
    id: &'static str,
    issuer: &'static str,
    signed_in: bool,
    /// keeper's sign-in marker: a new interactive sign-in moves it.
    sign_in: u32,
}

/// The account: which one, signed in or not, and every token it minted. A
/// token is minted only for the account asked for, as `OrgAccount` does.
struct FakeSignIn {
    now: Mutex<Account>,
    minted: AtomicUsize,
    /// The next mint answers that the sign-in is no longer good.
    refuse_next: AtomicBool,
    /// Every mint moves the account's sign-in marker: an account state that
    /// never holds still (R1096-05's provider, whatever the cause).
    moves_on_mint: AtomicBool,
    /// The next mint waits, after its account check, until notified.
    hold: Mutex<Option<Arc<tokio::sync::Notify>>>,
    /// A held mint was dropped before it finished: its attempt was cancelled.
    dropped: Arc<AtomicBool>,
}

impl FakeSignIn {
    fn new(signed_in: bool) -> Arc<FakeSignIn> {
        Arc::new(FakeSignIn {
            now: Mutex::new(Account {
                id: "acme",
                issuer: ISSUER,
                signed_in,
                sign_in: 1,
            }),
            minted: AtomicUsize::new(0),
            refuse_next: AtomicBool::new(false),
            moves_on_mint: AtomicBool::new(false),
            hold: Mutex::new(None),
            dropped: Arc::new(AtomicBool::new(false)),
        })
    }

    fn set(&self, change: impl FnOnce(&mut Account)) {
        change(&mut lock(&self.now));
    }

    /// Whether the held mint is dropped within a second: an aborted task's
    /// future is dropped when the runtime next schedules it.
    async fn cancelled(&self) -> bool {
        for _ in 0..50 {
            if self.dropped.load(Ordering::SeqCst) {
                return true;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        false
    }

    /// Hold the next mint; notify the returned handle to let it finish.
    fn hold_next(&self) -> Arc<tokio::sync::Notify> {
        let notify = Arc::new(tokio::sync::Notify::new());
        *lock(&self.hold) = Some(Arc::clone(&notify));
        notify
    }
}

fn snapshot(account: &Account) -> AccountSignIn {
    AccountSignIn {
        account: account.id.to_owned(),
        issuer: account.issuer.to_owned(),
        client_id: CLIENT.to_owned(),
        scopes: SCOPES.iter().map(|s| (*s).to_owned()).collect(),
        sign_in: account.signed_in.then(|| account.sign_in.to_string()),
    }
}

/// Sets its flag when dropped unless disarmed: a future cancelled mid-way.
struct DropGuard(Arc<AtomicBool>, bool);

impl Drop for DropGuard {
    fn drop(&mut self) {
        if self.1 {
            self.0.store(true, Ordering::SeqCst);
        }
    }
}

impl SignIn for FakeSignIn {
    fn account(&self) -> AccountState {
        let now = lock(&self.now).clone();
        if now.signed_in {
            AccountState::SignedIn(snapshot(&now))
        } else {
            AccountState::SignedOut(snapshot(&now))
        }
    }
    fn fresh_token<'a>(&'a self, asked: &'a AccountSignIn) -> TokenAnswer<'a> {
        let now = lock(&self.now).clone();
        let hold = lock(&self.hold).take();
        Box::pin(async move {
            if snapshot(&now).account != asked.account || now.issuer != asked.issuer {
                return Err(TokenError::Changed);
            }
            if let Some(hold) = hold {
                let mut guard = DropGuard(Arc::clone(&self.dropped), true);
                hold.notified().await;
                guard.1 = false;
            }
            if !now.signed_in || self.refuse_next.swap(false, Ordering::SeqCst) {
                return Err(TokenError::NeedsSignIn("sign in again".to_owned()));
            }
            let n = self.minted.fetch_add(1, Ordering::SeqCst) + 1;
            if self.moves_on_mint.load(Ordering::SeqCst) {
                lock(&self.now).sign_in += 1;
            }
            Ok(format!("signin-token-{n}"))
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
    /// What `/clients/self/attest` answers first, in order: the status and
    /// the body as sent, or a 200 for the ordinary answer.
    attest_refusals: Vec<(u16, String)>,
    /// The attestation window the server states; a week unless set.
    attest_window_ms: Option<i64>,
    /// Every revocation: the client revoked and the bearer that asked.
    revoked: Vec<(String, String)>,
    /// Held requests: `"before <path>"` waits before answering anything,
    /// `"after <path>"` after the server acted on it. A held request goes
    /// on when its sender sends or is dropped.
    holds: HashMap<String, mpsc::Receiver<()>>,
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
        let held = |when: &str| lock(&state).holds.remove(&format!("{when} {}", seen.path));
        if let Some(hold) = held("before") {
            let _ = hold.recv();
        }
        let reply = answer(&mut lock(&state), seen);
        if let Some(hold) = held("after") {
            let _ = hold.recv();
        }
        reply
    })
}

fn answer(server: &mut Server, seen: &Seen) -> Reply {
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
                    let Some(device) = server.devices.iter_mut().find(|d| d.token == old) else {
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
            if !server.attest_refusals.is_empty() {
                let (status, body) = server.attest_refusals.remove(0);
                if status != 200 {
                    return Reply::json(status, &body);
                }
            }
            let token = seen.json()["deviceToken"].as_str().unwrap_or("").to_owned();
            let window = server.attest_window_ms.unwrap_or(604_800_000);
            match server.devices.iter().find(|d| d.token == token) {
                Some(d) if d.bound => Reply::json(
                    200,
                    &format!(
                        r#"{{"clientId":"x","attestedAtMs":1000,"lapsesAtMs":{}}}"#,
                        1000 + window
                    ),
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
        ("POST", path) if path.starts_with("/clients/") && path.ends_with("/revoke") => {
            let client = path
                .trim_start_matches("/clients/")
                .trim_end_matches("/revoke")
                .to_owned();
            let token = bearer(seen);
            if !server.devices.iter().any(|d| d.token == token) {
                return Reply::json(401, r#"{"error":"sync.auth_required"}"#);
            }
            server.revoked.push((client.clone(), token));
            server.devices.retain(|d| d.client != client);
            Reply::json(200, &format!(r#"{{"clientId":"{client}"}}"#))
        }
        _ => Reply::json(404, r#"{"error":"not found"}"#),
    }
}

/// keeper-hq's refusal body for `code`.
fn refusal(status: u16, code: &str) -> (u16, String) {
    (status, format!(r#"{{"error":"{code}"}}"#))
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

    /// Hold the server's answer to `path`: `when` is `"before"` or `"after"`.
    fn hold(&self, when: &str, path: &str) -> mpsc::Sender<()> {
        let (release, held) = mpsc::channel();
        lock(&self.state)
            .holds
            .insert(format!("{when} {path}"), held);
        release
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

    /// Wait (in real time) until the server has seen `count` requests to `path`.
    async fn until_asked(&self, path: &str, count: usize) {
        for _ in 0..100 {
            if self.asked(path).len() >= count {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("{path} asked {} times, not {count}", self.asked(path).len());
    }
}

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// A sign-in's credential attested six and a half days ago: due.
fn due() -> Bound {
    Bound {
        attested_at_ms: sessions::now_ms() - 13 * DAY_MS / 2,
        window_ms: 7 * DAY_MS,
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
    world.sign_in.set(|account| account.signed_in = true);
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

// ---- R1096-01: the account checked is the account whose token is sent ----

#[tokio::test]
async fn an_account_switched_while_its_server_is_asked_gets_nothing_sent() {
    let world = World::new(true);
    let release = world.hold("before", "/auth/config");
    let hq = world.hq();
    hq.start().await.expect("starts");
    hq.set_server(&world.fake.base).await.expect("sets");
    world.until_asked("/auth/config", 1).await;
    // Another account, with another issuer, is signed in meanwhile.
    world.sign_in.set(|account| {
        account.id = "other";
        account.issuer = OTHER_ISSUER;
    });
    hq.reconcile().await.expect("reconciles");
    drop(release);
    status_until(&hq, "the other account is refused", |s| {
        s.sign_in.state == HqSignInState::Refused
    })
    .await;
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(world.asked("/devices/enroll").is_empty(), "nothing sent");
    assert_eq!(
        world.sign_in.minted.load(Ordering::SeqCst),
        0,
        "nothing minted"
    );
}

#[tokio::test]
async fn a_token_minted_while_the_account_changes_is_not_sent() {
    let world = World::new(true);
    let mint = world.sign_in.hold_next();
    let hq = world.hq();
    hq.start().await.expect("starts");
    hq.set_server(&world.fake.base).await.expect("sets");
    world.until_asked("/auth/config", 1).await;
    // No reconcile: nothing tells the service the account moved on.
    world.sign_in.set(|account| {
        account.id = "other";
        account.issuer = OTHER_ISSUER;
    });
    mint.notify_one();
    status_until(&hq, "the other account is refused", |s| {
        s.sign_in.state == HqSignInState::Refused
    })
    .await;
    assert!(world.asked("/devices/enroll").is_empty(), "nothing sent");
}

#[tokio::test]
async fn an_attestation_whose_account_changes_while_minting_is_not_sent() {
    let world = World::new(true);
    world.holds("device-token-a", "ana.1", Some(due()), false);
    let mint = world.sign_in.hold_next();
    let hq = world.hq();
    hq.start().await.expect("starts");
    world.until_asked("/auth/config", 1).await;
    world.sign_in.set(|account| account.id = "other");
    mint.notify_one();
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert!(
        world.asked("/clients/self/attest").is_empty(),
        "nothing sent"
    );
}

#[tokio::test]
async fn an_account_change_cancels_the_attempt_of_the_old_one() {
    let world = World::new(true);
    let mint = world.sign_in.hold_next();
    let hq = world.hq();
    hq.start().await.expect("starts");
    hq.set_server(&world.fake.base).await.expect("sets");
    world.until_asked("/auth/config", 1).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    world.sign_in.set(|account| {
        account.id = "other";
        account.issuer = OTHER_ISSUER;
    });
    hq.reconcile().await.expect("reconciles");
    assert!(
        world.sign_in.cancelled().await,
        "the old account's attempt was cancelled, not left to finish"
    );
    mint.notify_one();
    status_until(&hq, "the other account is refused", |s| {
        s.sign_in.state == HqSignInState::Refused
    })
    .await;
    assert!(world.asked("/devices/enroll").is_empty());
}

// ---- R1096-02: a superseded configuration's attempt sends nothing --------

#[tokio::test]
async fn clearing_the_server_cancels_an_unsent_sign_in_and_issues_no_device() {
    let world = World::new(true);
    let mint = world.sign_in.hold_next();
    let hq = world.hq();
    hq.start().await.expect("starts");
    hq.set_server(&world.fake.base).await.expect("sets");
    world.until_asked("/auth/config", 1).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    hq.set_server("").await.expect("clears");
    assert_eq!(hq.status().borrow().phase, HqPhase::NoServer);
    assert!(
        world.sign_in.cancelled().await,
        "the attempt was cancelled, not left to finish"
    );
    mint.notify_one();
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert!(world.asked("/devices/enroll").is_empty());
    assert!(lock(&world.state).devices.is_empty(), "no orphan device");
}

#[tokio::test]
async fn switching_the_server_sends_the_sign_in_to_the_new_one_only() {
    let world = World::new(true);
    let other = hq_server(Arc::new(Mutex::new(Server {
        issuer: ISSUER.to_owned(),
        ..Server::default()
    })));
    let mint = world.sign_in.hold_next();
    let hq = world.hq();
    hq.start().await.expect("starts");
    hq.set_server(&world.fake.base).await.expect("sets");
    world.until_asked("/auth/config", 1).await;
    tokio::time::sleep(Duration::from_millis(50)).await;
    hq.set_server(&other.base).await.expect("switches");
    mint.notify_one();
    status_until(&hq, "enrolled on the new server", enrolled).await;
    assert!(world.asked("/devices/enroll").is_empty(), "the old server");
    assert_eq!(
        other
            .requests()
            .iter()
            .filter(|seen| seen.path == "/devices/enroll")
            .count(),
        1
    );
}

#[tokio::test]
async fn an_adoption_already_sent_keeps_the_rotated_token_when_the_server_is_cleared() {
    let world = World::new(true);
    world.holds(PAIRED_TOKEN, "ana.1", None, false);
    // The server rotates the old token, then its answer is held.
    let release = world.hold("after", "/devices/enroll");
    let hq = world.hq();
    hq.start().await.expect("starts");
    world.until_asked("/devices/enroll", 1).await;
    hq.set_server("").await.expect("clears");
    drop(release);
    let mut kept = world.kept();
    for _ in 0..100 {
        if kept.as_ref().is_some_and(|k| k.token != PAIRED_TOKEN) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
        kept = world.kept();
    }
    let kept = kept.expect("kept");
    assert_eq!(
        kept.token, "device-token-1",
        "the token the server now knows"
    );
    assert_eq!(kept.client_id, "ana.1");
    assert!(kept.bound.is_some());
    assert_eq!(
        hq.status().borrow().phase,
        HqPhase::NoServer,
        "not restarted onto it"
    );
}

// ---- R1096-03: a new sign-in of the same account tries again --------------

#[tokio::test]
async fn a_new_sign_in_after_a_refused_refresh_enrols() {
    let world = World::new(true);
    world.sign_in.refuse_next.store(true, Ordering::SeqCst);
    let hq = world.hq();
    hq.start().await.expect("starts");
    hq.set_server(&world.fake.base).await.expect("sets");
    status_until(&hq, "asked to sign in", |s| {
        s.sign_in.state == HqSignInState::SignedOut
    })
    .await;
    // The same sign-in again: nothing new to try.
    hq.reconcile().await.expect("reconciles");
    tokio::time::sleep(Duration::from_millis(200)).await;
    assert_eq!(world.asked("/auth/config").len(), 1);
    // The person signs in again: same account, a new sign-in.
    world.sign_in.set(|account| account.sign_in = 2);
    hq.reconcile().await.expect("reconciles");
    status_until(&hq, "enrolled", enrolled).await;
    assert_eq!(world.asked("/devices/enroll").len(), 1);
}

// ---- R1096-04: a refused sign-in proof is retried; a gone device is not ---

/// `wall` passes on the wall clock and `awake` on tokio's — less when the
/// computer slept — then the test goes on in real time.
async fn pass(hq: &Hq, wall: Duration, awake: Duration) {
    hq.advance_wall(wall);
    tokio::time::pause();
    tokio::time::advance(awake).await;
    tokio::time::resume();
}

/// Let the ten-minute retry pass, awake.
async fn past_the_retry(hq: &Hq) {
    let ten = Duration::from_secs(601);
    pass(hq, ten, ten).await;
}

/// Start `world`'s service with the first attestation's answer held until
/// the start-up discovery has been applied, so the rediscovery a refusal
/// asks for is never folded into the start-up one (R1096-10).
async fn start_after_discovery(world: &World) -> Arc<Hq> {
    let release = world.hold("before", "/clients/self/attest");
    let hq = world.hq();
    hq.start().await.expect("starts");
    status_until(&hq, "the start-up discovery applied", |s| {
        !s.databases.is_empty()
    })
    .await;
    drop(release);
    hq
}

#[tokio::test]
async fn an_attestation_refused_for_its_sign_in_token_is_tried_again() {
    let world = World::new(true);
    world.holds("device-token-a", "ana.1", Some(due()), false);
    lock(&world.state).attest_refusals = vec![refusal(401, "hq.signin.stale")];
    let hq = start_after_discovery(&world).await;
    world.until_asked("/clients/self/attest", 1).await;
    // The device itself is asked about again, and is still known.
    world.until_asked("/databases", 2).await;
    past_the_retry(&hq).await;
    world.until_asked("/clients/self/attest", 2).await;
    for _ in 0..100 {
        if world
            .kept()
            .and_then(|kept| kept.bound)
            .is_some_and(|bound| {
                bound.window_ms == 604_800_000 && bound.attested_at_ms > due().attested_at_ms
            })
        {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let bound = world.kept().and_then(|kept| kept.bound).expect("bound");
    assert!(
        bound.attested_at_ms > due().attested_at_ms,
        "the second one was kept"
    );
    assert_eq!(hq.status().borrow().phase, HqPhase::Paired);
}

#[tokio::test]
async fn an_attestation_answered_device_unknown_stops() {
    let world = World::new(true);
    world.holds("device-token-a", "ana.1", Some(due()), false);
    lock(&world.state).attest_refusals = vec![refusal(401, "hq.device_unknown")];
    let hq = start_after_discovery(&world).await;
    world.until_asked("/clients/self/attest", 1).await;
    world.until_asked("/databases", 2).await;
    past_the_retry(&hq).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(world.asked("/clients/self/attest").len(), 1);
}

// ---- R1096-05: an account state that keeps moving is no hot loop ----------

#[tokio::test]
async fn an_account_state_that_moves_with_every_mint_is_tried_a_bounded_number_of_times() {
    let world = World::new(true);
    world.sign_in.moves_on_mint.store(true, Ordering::SeqCst);
    let hq = world.hq();
    hq.start().await.expect("starts");
    hq.set_server(&world.fake.base).await.expect("sets");
    let status = status_until(&hq, "it stops trying", |s| {
        s.sign_in.state == HqSignInState::Refused
    })
    .await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    assert!(
        status
            .sign_in
            .sentence
            .as_deref()
            .is_some_and(|s| s.contains("kept changing")),
        "{status:#?}"
    );
    assert_eq!(
        world.sign_in.minted.load(Ordering::SeqCst),
        3,
        "three attempts, then a wait"
    );
    assert_eq!(world.asked("/auth/config").len(), 3);
    assert!(world.asked("/devices/enroll").is_empty());
}

// ---- R1096-07: a late answer never undoes what came after it --------------

#[tokio::test]
async fn an_adoption_answered_after_forget_is_discarded_and_its_device_revoked() {
    let world = World::new(true);
    world.holds(PAIRED_TOKEN, "ana.1", None, false);
    let release = world.hold("after", "/devices/enroll");
    let hq = world.hq();
    hq.start().await.expect("starts");
    world.until_asked("/devices/enroll", 1).await;
    // Forget: the pairing goes, and the device signs in afresh.
    hq.forget().await.expect("forgets");
    let status = status_until(&hq, "a new device", enrolled).await;
    let fresh = status.device.expect("device").client_id;
    assert_eq!(fresh, "ana.new2");
    // The adoption's answer arrives late.
    drop(release);
    let mut revoked = Vec::new();
    for _ in 0..100 {
        revoked = lock(&world.state).revoked.clone();
        if !revoked.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        revoked,
        [("ana.1".to_owned(), "device-token-1".to_owned())],
        "the orphan revoked with its own token"
    );
    let kept = world.kept().expect("kept");
    assert_eq!(kept.client_id, "ana.new2", "what came after stays");
    assert_eq!(kept.token, "device-token-2");
    assert_eq!(
        hq.status()
            .borrow()
            .device
            .as_ref()
            .expect("device")
            .client_id,
        "ana.new2"
    );
}

#[tokio::test]
async fn an_enrolment_answered_after_a_newer_one_kept_its_own_is_discarded() {
    let world = World::new(true);
    let release = world.hold("after", "/devices/enroll");
    let hq = world.hq();
    hq.start().await.expect("starts");
    hq.set_server(&world.fake.base).await.expect("sets");
    world.until_asked("/devices/enroll", 1).await;
    // The same server set again: a newer attempt enrols and keeps its own.
    hq.set_server("").await.expect("clears");
    hq.set_server(&world.fake.base).await.expect("sets again");
    status_until(&hq, "the newer device", enrolled).await;
    drop(release);
    let mut revoked = Vec::new();
    for _ in 0..100 {
        revoked = lock(&world.state).revoked.clone();
        if !revoked.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert_eq!(
        revoked,
        [("ana.new1".to_owned(), "device-token-1".to_owned())]
    );
    assert_eq!(world.kept().expect("kept").client_id, "ana.new2");
}

// ---- R1096-08: only the device's own refusal stops attesting --------------

fn sentence(hq: &Hq) -> String {
    hq.status()
        .borrow()
        .sign_in
        .sentence
        .clone()
        .unwrap_or_default()
}

#[tokio::test]
async fn a_403_that_is_not_about_the_device_is_retried_on_a_growing_wait_and_said() {
    let world = World::new(true);
    world.holds("device-token-a", "ana.1", Some(due()), false);
    lock(&world.state).attest_refusals = vec![
        // A reverse proxy's page, not keeper-hq's answer.
        (403, "<html>Forbidden</html>".to_owned()),
        // A role an administrator can restore.
        refusal(403, "hq.signin.role"),
    ];
    let hq = world.hq();
    hq.start().await.expect("starts");
    status_until(&hq, "the proxy's refusal said", |s| {
        s.sign_in
            .sentence
            .as_deref()
            .is_some_and(|s| s.contains("(http-403)"))
    })
    .await;
    assert_eq!(hq.status().borrow().sign_in.state, HqSignInState::Enrolled);
    // Ten minutes on: asked again.
    past_the_retry(&hq).await;
    status_until(&hq, "the role refusal said", |s| {
        s.sign_in
            .sentence
            .as_deref()
            .is_some_and(|s| s.contains("hq.signin.role"))
    })
    .await;
    assert_eq!(world.asked("/clients/self/attest").len(), 2);
    // Twenty minutes now: ten more is not enough.
    past_the_retry(&hq).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(
        world.asked("/clients/self/attest").len(),
        2,
        "the wait grew"
    );
    past_the_retry(&hq).await;
    status_until(&hq, "renewed", |s| {
        !s.sign_in
            .sentence
            .as_deref()
            .unwrap_or("")
            .contains("refused")
    })
    .await;
    assert_eq!(world.asked("/clients/self/attest").len(), 3);
    assert_eq!(
        sentence(&hq),
        format!(
            "Signed in: {} knows this device by your account.",
            world.server().host()
        )
    );
}

#[tokio::test]
async fn a_403_naming_another_persons_device_stops_attesting_and_is_said() {
    let world = World::new(true);
    world.holds("device-token-a", "ana.1", Some(due()), false);
    lock(&world.state).attest_refusals = vec![refusal(403, "hq.attest.other_person")];
    let hq = world.hq();
    hq.start().await.expect("starts");
    status_until(&hq, "said", |s| {
        s.sign_in
            .sentence
            .as_deref()
            .is_some_and(|s| s.contains("signed in by someone else"))
    })
    .await;
    past_the_retry(&hq).await;
    past_the_retry(&hq).await;
    tokio::time::sleep(Duration::from_millis(300)).await;
    assert_eq!(world.asked("/clients/self/attest").len(), 1);
}

// ---- R1096-09: the backoff is a schedule on the wall clock ----------------

/// Wait for the `n`th attestation, refused with `code`, to be said.
async fn refused_attestation(world: &World, hq: &Hq, n: usize, code: &str) {
    world.until_asked("/clients/self/attest", n).await;
    status_until(hq, code, |s| {
        s.sign_in
            .sentence
            .as_deref()
            .is_some_and(|s| s.contains(code))
    })
    .await;
}

#[tokio::test]
async fn a_long_retry_is_not_slept_through_while_the_computer_sleeps() {
    let world = World::new(true);
    world.holds("device-token-a", "ana.1", Some(due()), false);
    // Consecutive codes differ, so each refusal is seen on its own.
    lock(&world.state).attest_refusals = [
        "hq.signin.role",
        "hq.signin.client",
        "hq.not_a_person",
        "hq.signin.role",
    ]
    .into_iter()
    .map(|code| refusal(403, code))
    .collect();
    let hq = world.hq();
    hq.start().await.expect("starts");
    refused_attestation(&world, &hq, 1, "hq.signin.role").await;
    let minutes = |m: u64| Duration::from_secs(m * 60 + 1);
    pass(&hq, minutes(10), minutes(10)).await;
    refused_attestation(&world, &hq, 2, "hq.signin.client").await;
    pass(&hq, minutes(20), minutes(20)).await;
    refused_attestation(&world, &hq, 3, "hq.not_a_person").await;
    pass(&hq, minutes(40), minutes(40)).await;
    refused_attestation(&world, &hq, 4, "hq.signin.role").await;
    // The next wait is 80 minutes. The computer sleeps through them, awake
    // for only one hour of it: the attestation still goes at the next wake.
    pass(&hq, minutes(80), minutes(60)).await;
    world.until_asked("/clients/self/attest", 5).await;
}

#[tokio::test]
async fn a_success_starts_the_retry_schedule_over() {
    let world = World::new(true);
    world.holds("device-token-a", "ana.1", Some(due()), false);
    {
        let mut server = lock(&world.state);
        // Refused, renewed, refused again.
        server.attest_refusals = vec![
            refusal(403, "hq.signin.role"),
            (200, String::new()),
            refusal(403, "hq.signin.client"),
        ];
        // A window of 0.7 s: the renewal falls due again at once.
        server.attest_window_ms = Some(700);
    }
    let hq = world.hq();
    hq.start().await.expect("starts");
    refused_attestation(&world, &hq, 1, "hq.signin.role").await;
    past_the_retry(&hq).await;
    world.until_asked("/clients/self/attest", 2).await;
    refused_attestation(&world, &hq, 3, "hq.signin.client").await;
    // Ten minutes again, not the twenty that came after the first refusal.
    past_the_retry(&hq).await;
    world.until_asked("/clients/self/attest", 4).await;
}
