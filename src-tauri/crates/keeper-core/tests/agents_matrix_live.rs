//! The agents' Matrix client against a real homeserver (story 90.4).
//!
//! Every test is `#[ignore]`: it needs the Synapse test homeserver and its
//! users (the operator's steps in the epic, 90.4 acceptance 9). Endpoints and
//! secrets come from the environment, never from this repository:
//!
//! ```sh
//! KEEPER_AGENTS_SMOKE_HOMESERVER=http://100.101.101.23:8008 \
//! KEEPER_AGENTS_SMOKE_SECRETS=$HOME/.config/keeper-smoke/synapse.env \
//! cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --test agents_matrix_live -- --ignored --nocapture --test-threads=1
//! ```
//!
//! The secrets file (mode `0600`) holds `KEY=value` lines: `SERVER_NAME`,
//! `ADMIN_TOKEN`, `NIXI_SMOKE_PASSWORD`, `NIXI_PACED_PASSWORD` and
//! `TGORKA_SMOKE_PASSWORD`. The admin token is used only to clear the test
//! users' devices and to set `nixi-paced`'s rate limit through Synapse's
//! `override_ratelimit` (the coordinator's ruling on the codemap's
//! disagreement D2, not the sessions digest D2) — the server's configuration
//! is never changed. Every test starts by removing the three users' devices.

// matrix-sdk's sync future is deep enough to need it, as in the library.
#![recursion_limit = "256"]

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use keeper_core::agents::events::{
    self, ClaimContent, CONTENT_VERSION, CONTROL_ROOM_TYPE, HOST, SESSION_ROOM_TYPE,
};
use keeper_core::agents::matrix::{AgentClient, AgentMatrixError, RoomKind};
use keeper_core::agents::session::SessionKind;
use keeper_core::auth::StoredSession;
use matrix_sdk::ruma::events::AnySyncTimelineEvent;
use matrix_sdk::ruma::serde::Raw;
use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId, OwnedUserId, RoomId};
use serde_json::{json, Value};

/// A directory removed when dropped (keeper-core carries no `tempfile`).
struct TempDir(std::path::PathBuf);

impl TempDir {
    fn new() -> TempDir {
        let dir = std::env::temp_dir().join(format!(
            "keeper-agents-live-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        TempDir(dir)
    }

    fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// The test homeserver's users these tests sign in as.
const SMOKE_USERS: [&str; 3] = ["nixi-smoke", "nixi-paced", "tgorka-smoke"];

struct Smoke {
    homeserver: String,
    server_name: String,
    secrets: HashMap<String, String>,
    http: reqwest::Client,
}

impl Smoke {
    fn from_env() -> Smoke {
        let homeserver = std::env::var("KEEPER_AGENTS_SMOKE_HOMESERVER")
            .expect("KEEPER_AGENTS_SMOKE_HOMESERVER names the test homeserver");
        let path = std::env::var("KEEPER_AGENTS_SMOKE_SECRETS")
            .expect("KEEPER_AGENTS_SMOKE_SECRETS names the secrets file");
        let text = std::fs::read_to_string(&path).expect("the secrets file is readable");
        let secrets: HashMap<String, String> = text
            .lines()
            .filter_map(|line| line.split_once('='))
            .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
            .collect();
        let server_name = secrets
            .get("SERVER_NAME")
            .cloned()
            .expect("SERVER_NAME in the secrets file");
        Smoke {
            homeserver: homeserver.trim_end_matches('/').to_owned(),
            server_name,
            secrets,
            http: reqwest::Client::new(),
        }
    }

    /// Every test's start: the smoke users' devices from earlier tests and
    /// runs removed, so a run leaves at most one test's devices behind and
    /// E2EE key sharing never grows with the number of runs.
    async fn setup() -> Smoke {
        let smoke = Smoke::from_env();
        for localpart in SMOKE_USERS {
            smoke.clear_devices(&smoke.user(localpart)).await;
        }
        smoke
    }

    fn user(&self, localpart: &str) -> OwnedUserId {
        OwnedUserId::try_from(format!("@{localpart}:{}", self.server_name)).expect("user id")
    }

    fn secret(&self, key: &str) -> &str {
        self.secrets
            .get(key)
            .map(String::as_str)
            .unwrap_or_else(|| panic!("{key} in the secrets file"))
    }

    async fn admin(&self, method: reqwest::Method, path: &str, body: Option<Value>) -> Value {
        let mut request = self
            .http
            .request(method, format!("{}{path}", self.homeserver))
            .bearer_auth(self.secret("ADMIN_TOKEN"));
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request.send().await.expect("admin call");
        let status = response.status();
        let value = response.json::<Value>().await.unwrap_or(Value::Null);
        assert!(status.is_success(), "admin {path}: {status} {value}");
        value
    }

    /// Remove every device of `user`.
    async fn clear_devices(&self, user: &OwnedUserId) {
        let listed = self
            .admin(
                reqwest::Method::GET,
                &format!("/_synapse/admin/v2/users/{user}/devices"),
                None,
            )
            .await;
        let devices: Vec<Value> = listed["devices"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .map(|d| d["device_id"].clone())
            .collect();
        if !devices.is_empty() {
            self.admin(
                reqwest::Method::POST,
                &format!("/_synapse/admin/v2/users/{user}/delete_devices"),
                Some(json!({ "devices": devices })),
            )
            .await;
        }
    }

    async fn devices(&self, user: &OwnedUserId) -> Vec<Value> {
        self.admin(
            reqwest::Method::GET,
            &format!("/_synapse/admin/v2/users/{user}/devices"),
            None,
        )
        .await["devices"]
            .as_array()
            .cloned()
            .unwrap_or_default()
    }

    async fn set_ratelimit(&self, user: &OwnedUserId, per_second: u32, burst: u32) {
        self.admin(
            reqwest::Method::POST,
            &format!("/_synapse/admin/v1/users/{user}/override_ratelimit"),
            Some(json!({ "messages_per_second": per_second, "burst_count": burst })),
        )
        .await;
    }

    async fn copy(
        &self,
        dir: &std::path::Path,
        localpart: &str,
        password_key: &str,
        display: &str,
    ) -> (AgentClient, StoredSession) {
        let client = AgentClient::open(&self.homeserver, dir, "smoke-passphrase")
            .await
            .expect("client");
        let session = client
            .login(localpart, self.secret(password_key), None, display)
            .await
            .expect("login");
        client.sync_once().await.expect("first sync");
        (client, session)
    }
}

/// Every timeline event `client` is handed, decrypted, as JSON.
fn record(client: &AgentClient) -> Arc<Mutex<Vec<Value>>> {
    let seen = Arc::new(Mutex::new(Vec::new()));
    let sink = Arc::clone(&seen);
    client
        .client()
        .add_event_handler(move |event: Raw<AnySyncTimelineEvent>| {
            let sink = Arc::clone(&sink);
            async move {
                if let Ok(value) = event.deserialize_as::<Value>() {
                    sink.lock().expect("lock").push(value);
                }
            }
        });
    seen
}

async fn sync_until(client: &AgentClient, mut done: impl FnMut() -> bool) {
    let deadline = Instant::now() + Duration::from_secs(60);
    while !done() {
        assert!(Instant::now() < deadline, "timed out waiting for sync");
        client.sync_once().await.expect("sync");
    }
}

async fn joined_pair(
    smoke: &Smoke,
    kind: SessionKind,
    root: &std::path::Path,
) -> (AgentClient, AgentClient, OwnedRoomId) {
    let (nixi, _) = smoke
        .copy(
            &root.join("nixi"),
            "nixi-smoke",
            "NIXI_SMOKE_PASSWORD",
            "nixi@smoke",
        )
        .await;
    let (person, _) = smoke
        .copy(
            &root.join("tgorka"),
            "tgorka-smoke",
            "TGORKA_SMOKE_PASSWORD",
            "tgorka@smoke",
        )
        .await;
    let room = nixi
        .create_room(
            RoomKind::Session(kind),
            "nixi 2026-10-02",
            vec![smoke.user("tgorka-smoke")],
            &[],
        )
        .await
        .expect("create room");
    sync_until(&person, || person.client().get_room(&room).is_some()).await;
    person.join(&room).await.expect("join");
    nixi.sync_once().await.expect("sync");
    person.sync_once().await.expect("sync");
    (nixi, person, room)
}

async fn state_of(smoke: &Smoke, client: &AgentClient, room: &RoomId) -> Vec<Value> {
    let token = client.client().access_token().expect("signed in");
    smoke
        .http
        .get(format!(
            "{}/_matrix/client/v3/rooms/{room}/state",
            smoke.homeserver
        ))
        .bearer_auth(token)
        .send()
        .await
        .expect("state")
        .json::<Vec<Value>>()
        .await
        .expect("state json")
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn login_twice_keeps_one_device() {
    let smoke = Smoke::setup().await;
    let nixi = smoke.user("nixi-smoke");
    let root = TempDir::new();

    let (first, session) = smoke
        .copy(
            root.path(),
            "nixi-smoke",
            "NIXI_SMOKE_PASSWORD",
            "nixi@smoke",
        )
        .await;
    let device = first.device_id().expect("device");
    let stored = session.to_json().expect("store the session");
    drop(first);

    let restored = AgentClient::open(&smoke.homeserver, root.path(), "smoke-passphrase")
        .await
        .expect("reopen");
    restored
        .restore(StoredSession::from_json(&stored).expect("read back"))
        .await
        .expect("restore");
    assert_eq!(restored.device_id().as_deref(), Some(device.as_str()));
    drop(restored);

    let other = TempDir::new();
    let again = AgentClient::open(&smoke.homeserver, other.path(), "smoke-passphrase")
        .await
        .expect("client");
    again
        .login(
            "nixi-smoke",
            smoke.secret("NIXI_SMOKE_PASSWORD"),
            Some(&device),
            "nixi@smoke",
        )
        .await
        .expect("login with the stored device id");

    let devices = smoke.devices(&nixi).await;
    assert_eq!(devices.len(), 1, "{devices:?}");
    assert_eq!(devices[0]["device_id"], device.as_str());
    assert_eq!(devices[0]["display_name"], "nixi@smoke");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn a_session_room_is_typed_encrypted_and_has_its_power_levels() {
    let smoke = Smoke::setup().await;
    let root = TempDir::new();
    for kind in [SessionKind::Main, SessionKind::Delegated] {
        let (nixi, _person, room) =
            joined_pair(&smoke, kind, &root.path().join(kind.as_str())).await;
        let state = state_of(&smoke, &nixi, &room).await;
        let of = |t: &str| {
            state
                .iter()
                .find(|e| e["type"] == t)
                .unwrap_or_else(|| panic!("{t} in the room state"))
                .clone()
        };
        assert_eq!(of("m.room.create")["content"]["type"], SESSION_ROOM_TYPE);
        assert_eq!(
            of("m.room.encryption")["content"]["algorithm"],
            "m.megolm.v1.aes-sha2"
        );
        let levels = &of("m.room.power_levels")["content"];
        let expected = events::power_levels(kind, &smoke.user("nixi-smoke"), &[]);
        assert_eq!(levels["events_default"], expected["events_default"]);
        assert_eq!(levels["events"], expected["events"], "{kind}");
        assert_eq!(levels["users"][smoke.user("nixi-smoke").as_str()], 100);
    }
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn a_person_talks_and_decides_and_writes_no_state() {
    let smoke = Smoke::setup().await;
    let root = TempDir::new();

    let (_nixi, person, main) =
        joined_pair(&smoke, SessionKind::Main, &root.path().join("m")).await;
    person
        .send(
            &main,
            "m.room.message",
            json!({"msgtype": "m.text", "body": "hi"}),
            None,
        )
        .await
        .expect("the person talks in the main room");
    person
        .send(
            &main,
            events::SCOPE,
            json!({"drives": [], "set_by": "x"}),
            None,
        )
        .await
        .expect("the person sets the scope in the main room");

    // R30: in a delegated room the server cannot refuse the person's free
    // text — it sees only `m.room.encrypted` — so a decision must pass; the
    // host keeps the free text out of the turns (90.5).
    let (_nixi, person, delegated) =
        joined_pair(&smoke, SessionKind::Delegated, &root.path().join("d")).await;
    person
        .send(
            &delegated,
            events::APPROVAL_DECISION,
            json!({"id": "x", "decision": "deny"}),
            None,
        )
        .await
        .expect("a decision is accepted");
    let refused = person
        .client()
        .get_room(&delegated)
        .expect("room")
        .send_state_event_raw(events::CLAIM, "", json!({"v": 1}))
        .await
        .expect_err("a person writes no state in a session room");
    assert!(refused.to_string().contains("M_FORBIDDEN"), "{refused}");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn send_edit_custom_and_state_reach_a_second_client() {
    let smoke = Smoke::setup().await;
    let root = TempDir::new();
    let (nixi, person, room) = joined_pair(&smoke, SessionKind::Main, root.path()).await;
    let seen = record(&person);

    let anchor = nixi
        .send(
            &room,
            "m.room.message",
            json!({"msgtype": "m.text", "body": "…"}),
            None,
        )
        .await
        .expect("anchor");
    nixi.send(
        &room,
        "m.room.message",
        events::edit_content(&anchor, "the whole answer"),
        None,
    )
    .await
    .expect("edit");
    nixi.send(
        &room,
        events::STATUS,
        json!({"v": 1, "run": "running"}),
        None,
    )
    .await
    .expect("status");
    nixi.send_state(&room, events::CLAIM, "", &json!({"v": 1, "epoch": 1}))
        .await
        .expect("state");

    let has = |pred: &dyn Fn(&Value) -> bool| seen.lock().expect("lock").iter().any(pred);
    sync_until(&person, || {
        has(&|e| e["event_id"] == anchor.as_str() && e["type"] == "m.room.message")
            && has(&|e| e["content"]["m.relates_to"]["rel_type"] == "m.replace")
            && has(&|e| e["type"] == events::STATUS)
            && has(&|e| e["type"] == events::CLAIM)
    })
    .await;
    assert!(has(
        &|e| e["content"]["m.new_content"]["body"] == "the whole answer"
    ));
    assert!(
        !has(&|e| e["type"] == "m.room.encrypted"),
        "every event decrypted"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn the_server_read_back_names_the_event_the_writer_sent() {
    let smoke = Smoke::setup().await;
    let root = TempDir::new();
    let (nixi, _person, room) = joined_pair(&smoke, SessionKind::Delegated, root.path()).await;
    let claim = ClaimContent {
        v: CONTENT_VERSION,
        host: "smoke".to_owned(),
        device: nixi.device_id().expect("device"),
        agent: smoke.user("nixi-smoke"),
        epoch: 1,
        acquired_at: "2026-10-02T12:00:00Z".to_owned(),
        renewed_at: "2026-10-02T12:00:00Z".to_owned(),
        expires_at: "2026-10-02T12:03:00Z".to_owned(),
        released: false,
        window: None,
    };
    let sent: OwnedEventId = nixi
        .send_state(
            &room,
            events::CLAIM,
            "",
            &serde_json::to_value(&claim).expect("json"),
        )
        .await
        .expect("send the claim");
    let back = nixi
        .server_state(&room, events::CLAIM, "")
        .await
        .expect("read back")
        .expect("the claim is there");
    assert_eq!(back.event_id, sent);
    assert!(u64::from(back.origin_server_ts.get()) > 0);
    assert_eq!(
        serde_json::from_value::<ClaimContent>(back.content).expect("content"),
        claim
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn an_event_handled_before_a_restart_is_not_handed_out_again() {
    let smoke = Smoke::setup().await;
    let root = TempDir::new();
    let (nixi, person, room) = joined_pair(&smoke, SessionKind::Main, root.path()).await;
    let person_session = StoredSession::from_client(person.client())
        .expect("session")
        .to_json()
        .expect("json");
    let seen = record(&person);
    let sent = nixi
        .send(
            &room,
            "m.room.message",
            json!({"msgtype": "m.text", "body": "once"}),
            None,
        )
        .await
        .expect("send");
    sync_until(&person, || {
        seen.lock()
            .expect("lock")
            .iter()
            .any(|e| e["event_id"] == sent.as_str())
    })
    .await;
    drop(person);

    let restored = AgentClient::open(
        &smoke.homeserver,
        &root.path().join("tgorka"),
        "smoke-passphrase",
    )
    .await
    .expect("reopen");
    restored
        .restore(StoredSession::from_json(&person_session).expect("session"))
        .await
        .expect("restore");
    let again = record(&restored);
    restored.sync_once().await.expect("sync");
    restored.sync_once().await.expect("sync");
    assert!(
        !again
            .lock()
            .expect("lock")
            .iter()
            .any(|e| e["event_id"] == sent.as_str()),
        "the event was handed out again after the restart"
    );
}

/// The event-cache events of `room` on `client`, as JSON, decrypted where
/// the cache holds them decrypted.
async fn cached_events(client: &AgentClient, room: &RoomId) -> Vec<Value> {
    let room = client.client().get_room(room).expect("room known");
    let (cache, _handles) = room.event_cache().await.expect("event cache");
    cache
        .events()
        .await
        .expect("events")
        .iter()
        .filter_map(|event| event.raw().deserialize_as::<Value>().ok())
        .collect()
}

/// Ruling R33's measurement: a person's device reads an agent room's header
/// beside the timeline from the event cache, so the cache must hold the
/// agent's status decrypted after the app restarts, before any sync.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn the_event_cache_holds_a_decrypted_status_after_a_restart() {
    let smoke = Smoke::setup().await;
    let root = TempDir::new();
    let (nixi, person, room) = joined_pair(&smoke, SessionKind::Main, root.path()).await;
    person
        .client()
        .event_cache()
        .subscribe()
        .expect("event cache");
    let person_session = StoredSession::from_client(person.client())
        .expect("session")
        .to_json()
        .expect("json");
    let status = nixi
        .send(
            &room,
            events::STATUS,
            json!({
                "v": 1, "session": "sessions/nixi/main", "kind": "main", "title": "Nixi",
                "agent": smoke.user("nixi-smoke"), "host": "electra", "epoch": 1, "run": "idle",
            }),
            None,
        )
        .await
        .expect("status");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let events = cached_events(&person, &room).await;
        if events.iter().any(|e| e["event_id"] == status.as_str()) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the live cache never held the status"
        );
        person.sync_once().await.expect("sync");
    }
    drop(person);

    let restored = AgentClient::open(
        &smoke.homeserver,
        &root.path().join("tgorka"),
        "smoke-passphrase",
    )
    .await
    .expect("reopen");
    restored
        .restore(StoredSession::from_json(&person_session).expect("session"))
        .await
        .expect("restore");
    restored
        .client()
        .event_cache()
        .subscribe()
        .expect("event cache");
    let events = cached_events(&restored, &room).await;
    let held = events
        .iter()
        .find(|e| e["event_id"] == status.as_str())
        .unwrap_or_else(|| panic!("no status in the restored cache: {} events", events.len()));
    assert_eq!(held["type"], events::STATUS, "held decrypted: {held}");
    assert_eq!(held["content"]["kind"], "main");
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn a_429_reaches_the_caller_with_its_retry_after() {
    let smoke = Smoke::setup().await;
    let paced = smoke.user("nixi-paced");
    // One message a second, a burst of five, set for this user alone: a
    // deterministic stand-in for Synapse's default `rc_message` (the
    // coordinator's ruling on the codemap's disagreement D2; epic 90,
    // 90.4 acceptance 6).
    smoke.set_ratelimit(&paced, 1, 5).await;
    let root = TempDir::new();
    let (client, _) = smoke
        .copy(
            root.path(),
            "nixi-paced",
            "NIXI_PACED_PASSWORD",
            "nixi@paced",
        )
        .await;
    let room = client
        .create_room(
            RoomKind::Session(SessionKind::Delegated),
            "paced",
            vec![],
            &[],
        )
        .await
        .expect("room");
    let mut limited = Vec::new();
    for n in 0..15 {
        match client
            .send(
                &room,
                "m.room.message",
                json!({"msgtype": "m.text", "body": n.to_string()}),
                None,
            )
            .await
        {
            Ok(_) => {}
            Err(AgentMatrixError::RateLimited { retry_after_ms }) => limited.push(retry_after_ms),
            Err(other) => panic!("unexpected: {other}"),
        }
    }
    println!("429s in a burst of 15: {limited:?}");
    assert!(
        limited.iter().any(Option::is_some),
        "no 429 with a retry_after reached the caller: {limited:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn p95_delivery_between_two_copies() {
    let smoke = Smoke::setup().await;
    let nixi = smoke.user("nixi-smoke");
    smoke.set_ratelimit(&nixi, 0, 0).await;
    let root = TempDir::new();
    let (a, _) = smoke
        .copy(
            &root.path().join("a"),
            "nixi-smoke",
            "NIXI_SMOKE_PASSWORD",
            "nixi@a",
        )
        .await;
    let (b, _) = smoke
        .copy(
            &root.path().join("b"),
            "nixi-smoke",
            "NIXI_SMOKE_PASSWORD",
            "nixi@b",
        )
        .await;
    let room = a
        .create_room(RoomKind::Control, "latency", vec![], &[])
        .await
        .expect("room");
    sync_until(&b, || b.client().get_room(&room).is_some()).await;
    // Copy A learns copy B's device before its first send, so the room key
    // is shared with it.
    a.sync_once().await.expect("sync");
    a.sync_once().await.expect("sync");

    let arrived: Arc<Mutex<HashMap<String, Instant>>> = Arc::default();
    let sink = Arc::clone(&arrived);
    b.client()
        .add_event_handler(move |event: Raw<AnySyncTimelineEvent>| {
            let sink = Arc::clone(&sink);
            async move {
                if let Ok(value) = event.deserialize_as::<Value>() {
                    if let Some(n) = value["content"]["n"].as_str() {
                        sink.lock()
                            .expect("lock")
                            .insert(n.to_owned(), Instant::now());
                    } else if value["type"] == "m.room.encrypted" {
                        // Handed out before its room key arrived.
                        sink.lock()
                            .expect("lock")
                            .insert(format!("utd:{}", value["event_id"]), Instant::now());
                    }
                }
            }
        });
    let syncing = b.client().clone();
    let sync_task = tokio::spawn(async move {
        let _ = syncing
            .sync(keeper_core::agents::matrix::sync_settings().timeout(Duration::from_secs(30)))
            .await;
    });

    const EVENTS: usize = 1000;
    let mut sent_at = HashMap::new();
    for n in 0..EVENTS {
        let key = n.to_string();
        sent_at.insert(key.clone(), Instant::now());
        a.send(&room, events::STATUS, json!({"n": key}), None)
            .await
            .expect("send");
        // Paced at 20 a second: a burst lands in one sync whose timeline is
        // cut to its last events (`limited`), and those cut never reach a
        // handler at all — a sync gap, not a delivery delay.
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let deadline = Instant::now() + Duration::from_secs(120);
    while arrived
        .lock()
        .expect("lock")
        .keys()
        .filter(|k| !k.starts_with("utd:"))
        .count()
        < EVENTS
        && Instant::now() < deadline
    {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    sync_task.abort();
    let arrived = arrived.lock().expect("lock");
    let mut delays: Vec<Duration> = sent_at
        .iter()
        .filter_map(|(n, at)| arrived.get(n).map(|got| got.duration_since(*at)))
        .collect();
    delays.sort();
    let undecrypted = arrived.keys().filter(|k| k.starts_with("utd:")).count();
    println!(
        "arrived {} of {EVENTS} decrypted, {undecrypted} handed out undecrypted",
        delays.len()
    );
    assert_eq!(delays.len(), EVENTS, "every event arrived");
    let pct = |p: usize| delays[(delays.len() * p / 100).min(delays.len() - 1)];
    println!(
        "delivery over {EVENTS} events: p50 {:?}, p95 {:?}, p99 {:?}",
        pct(50),
        pct(95),
        pct(99)
    );
}

/// AD-374 on Synapse: in a principal's control room a second agent user of
/// the principal writes its own host's manifest, and a person writes none.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn a_second_agent_of_the_principal_writes_its_manifest_in_the_control_room() {
    let smoke = Smoke::setup().await;
    let root = TempDir::new();
    let (nixi, _) = smoke
        .copy(
            &root.path().join("nixi"),
            "nixi-smoke",
            "NIXI_SMOKE_PASSWORD",
            "nixi@smoke",
        )
        .await;
    let (paced, _) = smoke
        .copy(
            &root.path().join("paced"),
            "nixi-paced",
            "NIXI_PACED_PASSWORD",
            "paced@smoke",
        )
        .await;
    let (person, _) = smoke
        .copy(
            &root.path().join("tgorka"),
            "tgorka-smoke",
            "TGORKA_SMOKE_PASSWORD",
            "tgorka@smoke",
        )
        .await;
    let second = smoke.user("nixi-paced");
    let room = nixi
        .create_room(
            RoomKind::Control,
            "tgorka's agents",
            vec![second.clone(), smoke.user("tgorka-smoke")],
            std::slice::from_ref(&second),
        )
        .await
        .expect("control room");
    for client in [&paced, &person] {
        sync_until(client, || client.client().get_room(&room).is_some()).await;
        client.join(&room).await.expect("join");
        client.sync_once().await.expect("sync");
    }
    let state = state_of(&smoke, &nixi, &room).await;
    let create = state
        .iter()
        .find(|e| e["type"] == "m.room.create")
        .expect("create");
    assert_eq!(create["content"]["type"], CONTROL_ROOM_TYPE);

    let manifest = |host: &str| json!({"v": CONTENT_VERSION, "host": host});
    let written = paced
        .send_state(&room, HOST, "paced-host", &manifest("paced-host"))
        .await
        .expect("the second agent writes its manifest");
    let read = nixi
        .server_state(&room, HOST, "paced-host")
        .await
        .expect("read")
        .expect("the manifest");
    assert_eq!(read.event_id, written);
    assert_eq!(read.sender, second);
    match person
        .send_state(&room, HOST, "laptop", &manifest("laptop"))
        .await
    {
        Err(AgentMatrixError::Forbidden(_)) => {}
        other => panic!("a person writes no manifest: {other:?}"),
    }
}

/// 91.1 acceptance 7: an agent's room as a person's keeper reads it — one
/// header line from a status anchor and five updates and a scope, one answer
/// message grown by twelve edits and not marked edited, and the room in the
/// Agents window; the principal's control room in none.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn an_agent_room_reads_as_one_header_and_one_growing_answer() {
    use keeper_core::agents::room::{
        room_kind_vm, AgentIcons, AgentKinds, AgentRoomKindVm, AgentRunVm,
    };
    use keeper_core::timeline;
    use keeper_core::vm::{TimelineBatch, TimelineItemVm, TimelineOp};

    let smoke = Smoke::setup().await;
    let root = TempDir::new();
    let (nixi, person, room) = joined_pair(&smoke, SessionKind::Main, root.path()).await;
    person
        .client()
        .event_cache()
        .subscribe()
        .expect("event cache");
    let agent = smoke.user("nixi-smoke");
    let status = |run: &str, detail: &str, anchor: Option<&OwnedEventId>| {
        let mut content = json!({
            "v": 1, "session": "sessions/nixi/main", "kind": "main", "title": "Nixi",
            "agent": agent, "host": "electra", "epoch": 1, "run": run, "detail": detail,
        });
        if let Some(anchor) = anchor {
            content["anchor"] = json!(anchor);
        }
        content
    };
    let anchor = nixi
        .send(&room, events::STATUS, status("idle", "0", None), None)
        .await
        .expect("status anchor");
    for n in 1..=5 {
        let run = if n == 5 { "running" } else { "blocked" };
        nixi.send(
            &room,
            events::STATUS,
            status(run, &format!("{n} notes read"), Some(&anchor)),
            None,
        )
        .await
        .expect("status update");
    }
    nixi.send(
        &room,
        events::SCOPE,
        json!({
            "v": 1, "set_by": agent,
            "drives": [{"id": "tgdrive", "title": "tgdrive"}, {"id": "neura", "title": "Neura"}],
            "label": {"readers": [smoke.user("tgorka-smoke")], "integrity": "owner"},
        }),
        None,
    )
    .await
    .expect("scope");
    let answer = nixi
        .send(
            &room,
            "m.room.message",
            json!({
                "msgtype": "m.text", "body": "…",
                "dev.keeper.agent.turn": {"session": "sessions/nixi/main", "line": "01J"},
            }),
            None,
        )
        .await
        .expect("answer anchor");
    // The person writes the marker too, and edits it: a person's message,
    // marked edited, and never the caret's (R30).
    let forged = person
        .send(
            &room,
            "m.room.message",
            json!({
                "msgtype": "m.text", "body": "not an answer",
                "dev.keeper.agent.turn": {"session": "sessions/nixi/main", "line": "02J"},
            }),
            None,
        )
        .await
        .expect("a person's marked message");
    person
        .send(
            &room,
            "m.room.message",
            events::edit_content(&forged, "still not an answer"),
            None,
        )
        .await
        .expect("its edit");
    for n in 1..=12 {
        nixi.send(
            &room,
            "m.room.message",
            events::edit_content(&answer, &format!("the answer, {n} of 12")),
            None,
        )
        .await
        .expect("answer edit");
    }
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let events = cached_events(&person, &room).await;
        if events
            .iter()
            .any(|e| e["content"]["m.new_content"]["body"] == "the answer, 12 of 12")
        {
            break;
        }
        assert!(Instant::now() < deadline, "the answer never arrived");
        person.sync_once().await.expect("sync");
    }

    let open = timeline::open_timeline(person.client(), &room, "acct")
        .await
        .expect("timeline");
    let batches: Arc<Mutex<Vec<TimelineBatch>>> = Arc::default();
    let sink = Arc::clone(&batches);
    let kinds = Arc::new(AgentKinds::default());
    let producer = tokio::spawn(timeline::forward_timeline(
        open,
        room.clone(),
        Box::new(move |batch| {
            sink.lock().expect("lock").push(batch);
            true
        }),
        Arc::clone(&kinds),
        Arc::new(AgentIcons::default()),
    ));
    let deadline = Instant::now() + Duration::from_secs(30);
    while batches.lock().expect("lock").is_empty() {
        assert!(Instant::now() < deadline, "no first batch");
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    producer.abort();
    let first = batches.lock().expect("lock")[0].clone();
    let TimelineOp::Reset { items } = &first.ops[0] else {
        panic!("the first batch is a reset");
    };
    let answers: Vec<(&String, &String, bool)> = items
        .iter()
        .filter_map(|item| match item {
            TimelineItemVm::Message {
                key,
                sender,
                body,
                is_edited,
                ..
            } if sender == agent.as_str() => Some((key, body, *is_edited)),
            _ => None,
        })
        .collect();
    assert_eq!(
        answers.len(),
        1,
        "one message item from the agent: {answers:?}"
    );
    let (answer_key, body, is_edited) = answers[0];
    assert_eq!(body, "the answer, 12 of 12");
    assert!(!is_edited, "a streamed answer is not marked edited");
    let forged_edited = items.iter().find_map(|item| match item {
        TimelineItemVm::Message {
            body, is_edited, ..
        } if body == "still not an answer" => Some(*is_edited),
        _ => None,
    });
    assert_eq!(
        forged_edited,
        Some(true),
        "a person's marked message is edited"
    );
    let header = first.header.expect("a header on the first batch");
    let line = header.status.expect("a status");
    assert_eq!(line.run, AgentRunVm::Running);
    assert_eq!(line.detail.as_deref(), Some("5 notes read"));
    assert_eq!(line.handle, "nixi-smoke@electra");
    let drives: Vec<String> = header
        .scope
        .expect("scope")
        .into_iter()
        .map(|d| d.id)
        .collect();
    assert_eq!(drives, ["tgdrive", "neura"]);
    assert_eq!(header.label.expect("label").integrity, "owner");
    assert_eq!(header.caret_key.as_ref(), Some(answer_key));

    let joined = person.client().get_room(&room).expect("room");
    assert_eq!(
        room_kind_vm(&joined, &AgentKinds::default()).await,
        Some(AgentRoomKindVm::Proxy)
    );

    let control = nixi
        .create_room(
            RoomKind::Control,
            "nixi control",
            vec![smoke.user("tgorka-smoke")],
            &[],
        )
        .await
        .expect("control room");
    sync_until(&person, || person.client().get_room(&control).is_some()).await;
    person.join(&control).await.expect("join control");
    person.sync_once().await.expect("sync");
    let control_room = person.client().get_room(&control).expect("control");
    assert_eq!(
        room_kind_vm(&control_room, &kinds).await,
        Some(AgentRoomKindVm::Control)
    );
}
