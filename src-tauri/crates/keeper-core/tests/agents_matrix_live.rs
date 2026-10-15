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
        Arc::new(keeper_core::agents::approval_card::HostedRooms::default()),
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

/// 97.3 #9 (UX-DR143): the person's own mark of where they stopped listening
/// rides beside the stream of a real encrypted agent room — sealed by their
/// device, split in UTF-16 units of the answer's latest body — while the
/// agent's mark of the same answer and the mark event itself are no item.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn the_persons_heard_mark_is_drawn_beside_the_stream() {
    use keeper_core::agents::heard::HeardMarkVm;
    use keeper_core::agents::room::{AgentIcons, AgentKinds};
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
    nixi.send(
        &room,
        "m.room.message",
        events::edit_content(&answer, "Hi 😀. Two three! Four."),
        None,
    )
    .await
    .expect("answer edit");
    let mark = |sentence: u32| {
        json!({
            "v": 1, "anchor": answer, "heard_until": 0, "sentence": sentence, "reason": "stop",
        })
    };
    // The agent's mark is no one's: it comes first and is ignored.
    nixi.send(&room, events::HEARD, mark(2), None)
        .await
        .expect("the agent's mark");
    person
        .send(&room, events::HEARD, mark(1), None)
        .await
        .expect("the person's mark");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let events = cached_events(&person, &room).await;
        let marks = events.iter().filter(|e| e["type"] == events::HEARD).count();
        if marks == 2 {
            break;
        }
        assert!(Instant::now() < deadline, "the marks never arrived");
        person.sync_once().await.expect("sync");
    }

    let open = timeline::open_timeline(person.client(), &room, "acct")
        .await
        .expect("timeline");
    let batches: Arc<Mutex<Vec<TimelineBatch>>> = Arc::default();
    let sink = Arc::clone(&batches);
    let producer = tokio::spawn(timeline::forward_timeline(
        open,
        room.clone(),
        Box::new(move |batch| {
            sink.lock().expect("lock").push(batch);
            true
        }),
        Arc::new(AgentKinds::default()),
        Arc::new(AgentIcons::default()),
        Arc::new(keeper_core::agents::approval_card::HostedRooms::default()),
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
    let answer_key = items
        .iter()
        .find_map(|item| match item {
            TimelineItemVm::Message { key, body, .. } if body == "Hi 😀. Two three! Four." => {
                Some(key.clone())
            }
            _ => None,
        })
        .expect("the answer's item");
    let messages = items
        .iter()
        .filter(|item| matches!(item, TimelineItemVm::Message { .. }))
        .count();
    assert_eq!(messages, 1, "a mark is no item: {items:?}");
    assert_eq!(
        first.heard,
        Some(vec![HeardMarkVm {
            key: answer_key,
            split_utf16: 6,
        }]),
        "the person's mark, one sentence heard, the emoji two units"
    );
}

/// 98.1 #7: edits do not push, on a real homeserver's push rules. The person
/// has an `event_id_only` pusher; the agent streams an answer — the anchor,
/// three edits, a final edit — and asks for an approval. What the server
/// recorded as notifying for the person (`GET /notifications`) is the
/// anchor and the request, and none of the edits (`.m.rule.suppress_edits`
/// reads `m.relates_to`, which an encrypted edit carries in the clear).
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn synapse_suppresses_edit_pushes() {
    let smoke = Smoke::setup().await;
    let root = TempDir::new();
    let (nixi, person, room) = joined_pair(&smoke, SessionKind::Main, root.path()).await;
    let token = person.client().access_token().expect("signed in");
    let pushers = format!("{}/_matrix/client/v3/pushers/set", smoke.homeserver);
    let pusher = |kind: Value| {
        json!({
            "kind": kind, "app_id": "dev.tgorka.keeper.ios.dev", "pushkey": "keeper-smoke",
            "app_display_name": "keeper", "device_display_name": "smoke", "lang": "en",
            "data": {
                "url": "https://push.invalid/_matrix/push/v1/notify",
                "format": "event_id_only",
                "default_payload": {"aps": {"mutable-content": 1}, "keeper_account": "smoke"},
            },
        })
    };
    let set = smoke
        .http
        .post(&pushers)
        .bearer_auth(&token)
        .json(&pusher(json!("http")))
        .send()
        .await
        .expect("set the pusher");
    assert!(set.status().is_success(), "{}", set.status());

    let anchor = nixi
        .send(
            &room,
            "m.room.message",
            json!({"msgtype": "m.text", "body": events::PLACEHOLDER,
                events::TURN: {"session": "s", "line": "l"}}),
            None,
        )
        .await
        .expect("anchor");
    let mut edits = Vec::new();
    for text in [
        "Looking",
        "Looking at",
        "Looking at the plan",
        "Done: the plan is written.",
    ] {
        edits.push(
            nixi.send(
                &room,
                "m.room.message",
                events::edit_content(&anchor, text),
                None,
            )
            .await
            .expect("edit"),
        );
    }
    let request = nixi
        .send(
            &room,
            events::APPROVAL_REQUEST,
            json!({"v": 1, "id": "01SMOKE", "tier": 3, "summary": "a smoke approval"}),
            None,
        )
        .await
        .expect("approval request");

    let notified = || async {
        smoke
            .http
            .get(format!(
                "{}/_matrix/client/v3/notifications?limit=50",
                smoke.homeserver
            ))
            .bearer_auth(&token)
            .send()
            .await
            .expect("notifications")
            .json::<Value>()
            .await
            .expect("notifications json")["notifications"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|n| n["room_id"] == room.as_str())
            .filter_map(|n| n["event"]["event_id"].as_str().map(str::to_owned))
            .collect::<Vec<String>>()
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut ids = notified().await;
    while !(ids.contains(&anchor.to_string()) && ids.contains(&request.to_string())) {
        assert!(Instant::now() < deadline, "notified: {ids:?}");
        tokio::time::sleep(Duration::from_millis(500)).await;
        ids = notified().await;
    }
    for edit in &edits {
        assert!(
            !ids.contains(&edit.to_string()),
            "an edit was pushed: {ids:?}"
        );
    }

    let removed = smoke
        .http
        .post(&pushers)
        .bearer_auth(&token)
        .json(&pusher(Value::Null))
        .send()
        .await
        .expect("remove the pusher");
    assert!(removed.status().is_success(), "{}", removed.status());
}

/// 98.1 #3: the pusher keeper registers is the one the homeserver keeps. The
/// person holds an agent session room, so `push::register` sets this phone's
/// pusher: `http`, `event_id_only`, the gateway as `Gateway::parse` reads it
/// and a `default_payload` naming the local account; registering with no
/// gateway removes it, and the person has no pusher of this phone's left.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn the_homeserver_keeps_the_pusher_keeper_registers() {
    use keeper_core::agents::push::{self, ApnsEnvironment, PusherSpec, Registered};

    let smoke = Smoke::setup().await;
    let root = TempDir::new();
    let (_nixi, person, _room) = joined_pair(&smoke, SessionKind::Main, root.path()).await;
    let token = [0x6b, 0x65, 0x65, 0x70, 0x65, 0x72];
    let account = "01J9ZK0000000000000000ACCT";
    let spec = PusherSpec::of_setting(
        Some("https://push.invalid"),
        ApnsEnvironment::Development,
        &token,
        account,
        "smoke",
    )
    .expect("a gateway keeper registers");
    let pushkey = "a2VlcGVy";
    let mine = || async {
        smoke
            .http
            .get(format!("{}/_matrix/client/v3/pushers", smoke.homeserver))
            .bearer_auth(person.client().access_token().expect("signed in"))
            .send()
            .await
            .expect("pushers")
            .json::<Value>()
            .await
            .expect("pushers json")["pushers"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .into_iter()
            .filter(|p| p["pushkey"] == pushkey && p["app_id"] == "dev.tgorka.keeper.ios.dev")
            .collect::<Vec<Value>>()
    };

    let set = push::register(person.client(), spec.ids(), Some(&spec))
        .await
        .expect("set the pusher");
    assert_eq!(set, Registered::Set);
    let kept = mine().await;
    assert_eq!(kept.len(), 1, "{kept:?}");
    assert_eq!(kept[0]["kind"], "http");
    assert_eq!(
        kept[0]["data"]["url"],
        "https://push.invalid/_matrix/push/v1/notify"
    );
    assert_eq!(kept[0]["data"]["format"], "event_id_only");
    assert_eq!(
        kept[0]["data"]["default_payload"]["keeper_account"],
        account
    );
    assert_eq!(
        kept[0]["data"]["default_payload"]["aps"]["mutable-content"],
        1
    );

    let removed = push::register(person.client(), spec.ids(), None)
        .await
        .expect("remove the pusher");
    assert_eq!(removed, Registered::Removed);
    assert_eq!(mine().await, Vec::<Value>::new());
}

/// Story 100.4 #3 (R384): the host makes the home's picture the agent's
/// profile avatar once — not again at the next scan, after a restart or from
/// a second host while the file is unchanged — and again when it changes; a
/// person's device in the agent's room fetches that avatar whole, typed as
/// the picture it is.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn the_agents_picture_is_its_profile_avatar_once_until_the_file_changes() {
    use keeper_core::agents::avatar::{self, AvatarPort, Candidate, Published};
    let smoke = Smoke::setup().await;
    let root = TempDir::new();
    let stamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let svg = |n: u8| {
        format!("<svg xmlns=\"http://www.w3.org/2000/svg\"><!-- {stamp}-{n} --></svg>").into_bytes()
    };
    let picture = |bytes: Vec<u8>| {
        avatar::scan(&[], move |file| {
            if file == "avatar.svg" {
                Candidate::Bytes(bytes.clone())
            } else {
                Candidate::Absent
            }
        })
        .avatar
        .expect("a picture")
    };

    let (nixi, person, room) = joined_pair(&smoke, SessionKind::Main, root.path()).await;
    let first = picture(svg(1));
    let Published::Uploaded(mxc) = avatar::publish(nixi.client(), &first)
        .await
        .expect("publish")
    else {
        panic!("a new file is uploaded");
    };
    let profile = nixi
        .client()
        .account()
        .get_avatar_url()
        .await
        .expect("profile");
    assert_eq!(profile.as_deref(), Some(&*mxc));

    // The next scan, and a second host's copy, find the record: nothing is
    // uploaded and the profile keeps its avatar.
    assert_eq!(
        avatar::publish(nixi.client(), &first).await,
        Ok(Published::Unchanged)
    );
    let (second, _) = smoke
        .copy(
            &root.path().join("second"),
            "nixi-smoke",
            "NIXI_SMOKE_PASSWORD",
            "nixi@second",
        )
        .await;
    assert_eq!(
        avatar::publish(second.client(), &first).await,
        Ok(Published::Unchanged)
    );
    let profile = nixi
        .client()
        .account()
        .get_avatar_url()
        .await
        .expect("profile");
    assert_eq!(profile.as_deref(), Some(&*mxc));

    // A changed file is a new avatar.
    let changed = picture(svg(2));
    let Published::Uploaded(newer) = avatar::publish(second.client(), &changed)
        .await
        .expect("publish")
    else {
        panic!("a changed file is uploaded");
    };
    assert_ne!(newer, mxc);

    // The person's device reads the agent's new avatar from the room and
    // fetches the original bytes, typed as an SVG.
    let joined = person.client().get_room(&room).expect("the room");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        person.sync_once().await.expect("sync");
        let held = joined
            .get_member_no_sync(&smoke.user("nixi-smoke"))
            .await
            .expect("member")
            .and_then(|member| member.avatar_url().map(ToOwned::to_owned));
        if held.as_deref() == Some(&*newer) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the room never held the new avatar: {held:?}"
        );
    }
    let fetched =
        keeper_core::media::fetch_agent_avatar(person.client(), &joined, &smoke.user("nixi-smoke"))
            .await
            .expect("the avatar");
    assert_eq!(fetched.mimetype, "image/svg+xml");
    assert_eq!(fetched.bytes, svg(2));
    // The person holds no agent's power: their own avatar is never served.
    assert!(keeper_core::media::fetch_agent_avatar(
        person.client(),
        &joined,
        &smoke.user("tgorka-smoke"),
    )
    .await
    .is_err());

    // R399 #1: a profile avatar past 256 KiB — any client may set one — is
    // never served: the device stops reading at the bound and draws the
    // letter.
    let mut huge = b"\x89PNG\r\n\x1a\n".to_vec();
    huge.resize(300 * 1024, 0);
    let huge = AvatarPort::upload(nixi.client(), "image/png", huge)
        .await
        .expect("the large upload");
    AvatarPort::set_profile(nixi.client(), &huge)
        .await
        .expect("the large avatar");
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        person.sync_once().await.expect("sync");
        let held = joined
            .get_member_no_sync(&smoke.user("nixi-smoke"))
            .await
            .expect("member")
            .and_then(|member| member.avatar_url().map(ToOwned::to_owned));
        if held.as_deref() == Some(&*huge) {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the room never held the large avatar: {held:?}"
        );
    }
    assert!(matches!(
        keeper_core::media::fetch_agent_avatar(person.client(), &joined, &smoke.user("nixi-smoke"))
            .await,
        Err(keeper_core::error::MediaError::NotFound)
    ));
}

/// The proxy facts of a Mac whose zone says `person` is `agent`'s human.
fn proxies_of(
    agent: &OwnedUserId,
    person: &OwnedUserId,
) -> keeper_core::agents::proxy::AgentProxies {
    use keeper_core::agents::proxy::{AgentProxies, ProxyFacts};
    let proxies = AgentProxies::default();
    proxies.replace(
        [(
            agent.clone(),
            ProxyFacts {
                human: person.clone(),
                allowed: Vec::new(),
            },
        )]
        .into_iter()
        .collect(),
    );
    proxies
}

/// 101.1 #1, #2, #8 (R416): a DM whose status is buried under more than
/// 200 claim renewals, read by a device whose cache holds nothing of it, is
/// still the person's DM by the seat an earlier read kept in `keeper.db`;
/// without that seat it is not, and the page-back that ran out of its budget
/// is not taken for "this room has no status".
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn a_dm_buried_under_claims_is_admitted_by_its_kept_seat() {
    use keeper_core::account::admit_proxy_room;
    use keeper_core::agents::room::{AgentKinds, HeaderReader};

    let smoke = Smoke::setup().await;
    let root = TempDir::new();
    let keeper_dir = root.path().join("keeper");
    let agent = smoke.user("nixi-smoke");
    let person_user = smoke.user("tgorka-smoke");
    smoke.set_ratelimit(&agent, 0, 0).await;
    let (nixi, person, room) = joined_pair(&smoke, SessionKind::Main, root.path()).await;
    person
        .client()
        .event_cache()
        .subscribe()
        .expect("event cache");
    nixi.send(
        &room,
        events::STATUS,
        json!({
            "v": 1, "session": "sessions/nixi/main", "kind": "main", "title": "main",
            "agent": agent, "host": "electra", "epoch": 1, "run": "idle",
        }),
        None,
    )
    .await
    .expect("status");
    let deadline = Instant::now() + Duration::from_secs(60);
    while !cached_events(&person, &room)
        .await
        .iter()
        .any(|e| e["type"] == events::STATUS)
    {
        assert!(Instant::now() < deadline, "the status never arrived");
        person.sync_once().await.expect("sync");
    }
    let proxies = proxies_of(&agent, &person_user);
    let joined = person.client().get_room(&room).expect("room");
    let seen = AgentKinds::kept(&keeper_dir);
    let (kind, admitted) = admit_proxy_room(&joined, &seen, &proxies)
        .await
        .expect("the DM, read from its status");
    assert_eq!((kind, admitted), (SessionKind::Main, agent.clone()));

    // A host renews its claim, over and over, while nobody speaks.
    let start = chrono::Utc::now();
    for n in 0..220_i64 {
        let at = (start + chrono::Duration::seconds(n)).to_rfc3339();
        nixi.send_state(
            &room,
            events::CLAIM,
            "",
            &json!({
                "v": 1, "host": "electra", "device": "ELECTRA", "agent": agent, "epoch": 1,
                "acquired_at": start.to_rfc3339(), "renewed_at": at, "expires_at": at,
                "released": false,
            }),
        )
        .await
        .expect("a renewal");
    }

    // A device with nothing of the room in its cache — keeper restarted
    // with an empty cache, a new device — reads the room.
    let (fresh, _) = smoke
        .copy(
            &root.path().join("fresh"),
            "tgorka-smoke",
            "TGORKA_SMOKE_PASSWORD",
            "tgorka@fresh",
        )
        .await;
    fresh
        .client()
        .event_cache()
        .subscribe()
        .expect("event cache");
    fresh.sync_once().await.expect("sync");
    let buried = fresh.client().get_room(&room).expect("the room, joined");
    let unseated = AgentKinds::default();
    assert!(
        admit_proxy_room(&buried, &unseated, &proxies)
            .await
            .is_err(),
        "without a seat the status is out of reach"
    );
    let header = HeaderReader::open(buried.clone(), &unseated).await;
    assert!(!header.state().has_status());
    assert!(
        !unseated.searched_empty(&person_user, &room),
        "a page-back that ran out of its budget searched nothing to its end"
    );
    let restarted = AgentKinds::kept(&keeper_dir);
    let (kind, admitted) = admit_proxy_room(&buried, &restarted, &proxies)
        .await
        .expect("the DM, by the seat kept in keeper.db");
    assert_eq!((kind, admitted), (SessionKind::Main, agent));
}

/// 101.1 #3, #8 (R416): the person's own agent's new conversation and DM
/// arrive as invites and are joined without the person sending first — the
/// DM as direct; a room that is not a session room waits.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn an_own_agents_invites_are_joined_and_the_dm_is_direct() {
    use keeper_core::account::join_agent_invite;
    use std::collections::BTreeSet;

    let smoke = Smoke::setup().await;
    let root = TempDir::new();
    let agent = smoke.user("nixi-smoke");
    let (nixi, _) = smoke
        .copy(
            &root.path().join("nixi"),
            "nixi-smoke",
            "NIXI_SMOKE_PASSWORD",
            "nixi@smoke",
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
    let invite = |kind: RoomKind| {
        let nixi = &nixi;
        let person_user = smoke.user("tgorka-smoke");
        async move {
            nixi.create_room(kind, "Nixi", vec![person_user], &[])
                .await
                .expect("room")
        }
    };
    let conversation = invite(RoomKind::Session(SessionKind::Conversation)).await;
    let dm = invite(RoomKind::Session(SessionKind::Main)).await;
    let control = invite(RoomKind::Control).await;
    sync_until(&person, || {
        [&conversation, &dm, &control]
            .iter()
            .all(|room| person.client().get_room(room).is_some())
    })
    .await;
    let known = BTreeSet::from([agent.clone()]);
    let room = |id: &OwnedRoomId| person.client().get_room(id).expect("room");
    assert!(join_agent_invite(&room(&conversation), &known).await);
    assert!(join_agent_invite(&room(&dm), &known).await);
    assert!(
        !join_agent_invite(&room(&control), &known).await,
        "not a session room"
    );
    assert!(
        !join_agent_invite(&room(&conversation), &BTreeSet::new()).await,
        "already joined, and nobody known"
    );
    person.sync_once().await.expect("sync");
    assert_eq!(room(&conversation).state(), matrix_sdk::RoomState::Joined);
    assert_eq!(room(&control).state(), matrix_sdk::RoomState::Invited);
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let direct = room(&dm)
            .direct_targets()
            .iter()
            .any(|target| target.as_user_id() == Some(&agent));
        if direct {
            break;
        }
        assert!(Instant::now() < deadline, "the DM was not marked direct");
        person.sync_once().await.expect("sync");
    }
}

/// A status for `agent`'s session of `kind` titled `title`.
fn status_of(agent: &OwnedUserId, kind: &str, title: &str) -> Value {
    json!({
        "v": 1, "session": "sessions/nixi/x", "kind": kind, "title": title,
        "agent": agent, "host": "electra", "epoch": 1, "run": "idle",
    })
}

/// R450 (review-101rooms-1 03, 06): the person's DM already exists, its
/// status buried under more than 200 claim renewals, and this device kept no
/// seat for it — an upgraded keeper. keeper asks the agent's host; the host
/// answers with the existing room, sending no new status; keeper searches
/// that one room back page after page, admits it and opens it, within the
/// one bound of the ask.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn an_existing_buried_dm_opens_on_a_device_that_kept_no_seat() {
    use keeper_core::account::{admit_proxy_room, open_main_dm};
    use keeper_core::agents::main_dm::MAIN_DM_WAIT;
    use keeper_core::agents::room::AgentKinds;

    let smoke = Smoke::setup().await;
    let root = TempDir::new();
    let agent = smoke.user("nixi-smoke");
    let person_user = smoke.user("tgorka-smoke");
    smoke.set_ratelimit(&agent, 0, 0).await;
    smoke.set_ratelimit(&person_user, 0, 0).await;
    // Earlier runs leave the person in many agent rooms and invites, each one
    // a whole listing reads; they stay: the request goes out before any of
    // them is read (review-101rooms-2 06).
    let (nixi, person, room) = joined_pair(&smoke, SessionKind::Main, root.path()).await;
    let control = nixi
        .create_room(
            RoomKind::Control,
            "tgorka's agents",
            vec![person_user.clone()],
            &[],
        )
        .await
        .expect("control room");
    sync_until(&person, || person.client().get_room(&control).is_some()).await;
    person.join(&control).await.expect("join the control room");
    nixi.sync_once().await.expect("sync");
    nixi.send(
        &room,
        events::STATUS,
        status_of(&agent, "main", "main"),
        None,
    )
    .await
    .expect("status");
    // The person's device is away while the host renews, over and over.
    let start = chrono::Utc::now();
    for n in 0..220_i64 {
        let at = (start + chrono::Duration::seconds(n)).to_rfc3339();
        nixi.send_state(
            &room,
            events::CLAIM,
            "",
            &json!({
                "v": 1, "host": "electra", "device": "ELECTRA", "agent": agent, "epoch": 1,
                "acquired_at": start.to_rfc3339(), "renewed_at": at, "expires_at": at,
                "released": false,
            }),
        )
        .await
        .expect("a renewal");
    }
    person
        .client()
        .event_cache()
        .subscribe()
        .expect("event cache");
    person.sync_once().await.expect("sync");
    let proxies = proxies_of(&agent, &person_user);
    let keeper_dir = root.path().join("keeper");
    let kinds = AgentKinds::kept(&keeper_dir);
    let joined = person.client().get_room(&room).expect("room");
    assert!(
        admit_proxy_room(&joined, &kinds, &proxies).await.is_err(),
        "the status is out of the row's reach and no seat was kept"
    );

    // The agent's host: it answers the ask with the DM that exists, and
    // sends no status. Earlier runs left the agent and the person in older
    // control rooms of the agent's, with their own old requests: the host
    // answers the request that is new, in whichever of them keeper chose.
    let control_rooms = |nixi: &AgentClient| {
        nixi.client()
            .joined_rooms()
            .into_iter()
            .filter(|room| {
                room.room_type().map(|kind| kind.to_string()).as_deref() == Some(CONTROL_ROOM_TYPE)
            })
            .map(|room| room.room_id().to_owned())
            .collect::<Vec<_>>()
    };
    let mut old_requests = std::collections::BTreeSet::new();
    for asked_in in control_rooms(&nixi) {
        for (key, state) in nixi.cached_states(&asked_in, events::MAIN_DM_REQUEST).await {
            if key == person_user.as_str() {
                old_requests.insert(state.event_id);
            }
        }
    }
    assert!(
        control_rooms(&nixi).contains(&control),
        "this run's control room"
    );
    let host = {
        let (nixi, person_user, room) = (nixi.clone(), person_user.clone(), room.clone());
        tokio::spawn(async move {
            loop {
                nixi.sync_once().await.expect("sync");
                let mut asked = None;
                for asked_in in control_rooms(&nixi) {
                    if let Some((_, state)) = nixi
                        .cached_states(&asked_in, events::MAIN_DM_REQUEST)
                        .await
                        .into_iter()
                        .find(|(key, state)| {
                            key == person_user.as_str() && !old_requests.contains(&state.event_id)
                        })
                    {
                        asked = Some((asked_in, state));
                        break;
                    }
                }
                if let Some((control, state)) = asked {
                    nixi.send(
                        &control,
                        events::MAIN_DM_ANSWER,
                        json!({"v": 1, "request": state.event_id, "agent": state.content["agent"], "room": room}),
                        None,
                    )
                    .await
                    .expect("answer");
                    return;
                }
            }
        })
    };
    let syncing = {
        let person = person.clone();
        tokio::spawn(async move {
            loop {
                let _ = person.sync_once().await;
            }
        })
    };
    let asked = tokio::time::Instant::now();
    let opened = open_main_dm(
        person.client(),
        "acct",
        &agent,
        &kinds,
        &proxies,
        asked + MAIN_DM_WAIT,
    )
    .await;
    syncing.abort();
    host.abort();
    let opened = opened.expect("the DM opened");
    assert_eq!(opened.room_id, room.as_str());
    assert!(asked.elapsed() < MAIN_DM_WAIT);
    assert!(
        admit_proxy_room(&joined, &kinds, &proxies).await.is_ok(),
        "and its seat is kept now"
    );
}

/// R450 (review-101rooms-1 09): a DM the person left and forgot loses its
/// direct mark, and the host's re-invite carries none. An answer that names
/// it before the invite arrives settles nothing yet; once the invite is
/// here — and when the answer comes while it is — keeper joins it and marks
/// it direct with the agent.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn a_dm_left_and_reinvited_is_joined_and_direct_again() {
    use keeper_core::account::settle_answered_dm;

    let smoke = Smoke::setup().await;
    let root = TempDir::new();
    let agent = smoke.user("nixi-smoke");
    let person_user = smoke.user("tgorka-smoke");
    let (nixi, person, room) = joined_pair(&smoke, SessionKind::Main, root.path()).await;
    let direct = |person: &AgentClient| {
        person.client().get_room(&room).is_some_and(|joined| {
            joined
                .direct_targets()
                .iter()
                .any(|target| target.as_user_id() == Some(&agent))
        })
    };
    for answered_before_invite in [true, false] {
        let joined = person.client().get_room(&room).expect("room");
        joined.leave().await.expect("leave");
        joined.forget().await.expect("forget");
        person.sync_once().await.expect("sync");
        nixi.sync_once().await.expect("sync");
        if answered_before_invite {
            assert!(
                !settle_answered_dm(person.client(), &room, &agent).await,
                "the invite has not arrived"
            );
        }
        nixi.client()
            .get_room(&room)
            .expect("the agent's room")
            .invite_user_by_id(&person_user)
            .await
            .expect("invite again");
        sync_until(&person, || {
            person
                .client()
                .get_room(&room)
                .is_some_and(|r| r.state() == matrix_sdk::RoomState::Invited)
        })
        .await;
        assert!(settle_answered_dm(person.client(), &room, &agent).await);
        let deadline = Instant::now() + Duration::from_secs(30);
        while !direct(&person) {
            assert!(Instant::now() < deadline, "not marked direct");
            person.sync_once().await.expect("sync");
        }
        assert_eq!(
            person.client().get_room(&room).expect("room").state(),
            matrix_sdk::RoomState::Joined
        );
    }
}

/// R451 (review-101rooms-1 05): a room whose seat this device kept follows
/// a newer status without being opened: the row's read folds the cached
/// status into the seat (and the account-level fold does the same from the
/// event it is handed), its kind and title are the newer status's, and the
/// rows are told to redraw.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn an_unopened_rooms_row_follows_a_newer_status() {
    use keeper_core::agents::room::{fold_status, room_kind_vm, AgentKinds, AgentRoomKindVm};

    let smoke = Smoke::setup().await;
    let root = TempDir::new();
    let agent = smoke.user("nixi-smoke");
    let person_user = smoke.user("tgorka-smoke");
    let (nixi, person, room) = joined_pair(&smoke, SessionKind::Conversation, root.path()).await;
    person
        .client()
        .event_cache()
        .subscribe()
        .expect("event cache");
    let seen = record(&person);
    nixi.send(
        &room,
        events::STATUS,
        status_of(&agent, "conversation", "draft"),
        None,
    )
    .await
    .expect("status");
    sync_until(&person, || {
        seen.lock()
            .expect("lock")
            .iter()
            .any(|e| e["type"] == events::STATUS && e["content"]["title"] == "draft")
    })
    .await;
    let kinds = AgentKinds::default();
    let joined = person.client().get_room(&room).expect("room");
    assert_eq!(
        room_kind_vm(&joined, &kinds).await,
        Some(AgentRoomKindVm::of(
            keeper_core::agents::room::AgentRoomKind::Session,
            Some(SessionKind::Conversation)
        ))
    );
    let mut rows = kinds.subscribe();
    rows.borrow_and_update();

    nixi.send(
        &room,
        events::STATUS,
        status_of(&agent, "delegated", "test"),
        None,
    )
    .await
    .expect("a newer status");
    sync_until(&person, || {
        seen.lock()
            .expect("lock")
            .iter()
            .any(|e| e["type"] == events::STATUS && e["content"]["title"] == "test")
    })
    .await;
    assert_eq!(
        room_kind_vm(&joined, &kinds).await,
        Some(AgentRoomKindVm::of(
            keeper_core::agents::room::AgentRoomKind::Session,
            Some(SessionKind::Delegated)
        )),
        "the row follows the newer status"
    );
    assert_eq!(
        kinds.seat(&person_user, &room).map(|seat| seat.title),
        Some("test".to_owned())
    );
    assert!(rows.has_changed().expect("open"), "the rows redraw");

    // The account-level fold, from the event alone.
    let folded = AgentKinds::default();
    let newest = seen
        .lock()
        .expect("lock")
        .iter()
        .rev()
        .find(|e| e["type"] == events::STATUS)
        .cloned()
        .expect("a status");
    fold_status(&joined, &newest, &folded).await;
    assert_eq!(
        folded
            .seat(&person_user, &room)
            .map(|seat| (seat.kind, seat.title)),
        Some((SessionKind::Delegated, "test".to_owned()))
    );
}

/// Story 101.3 (R430, R431): a question for the person survives a restart of
/// their device — the restored cache holds only the room's newest events, and
/// the panel's reader reads the room back to the ask through the event cache
/// — and the proxy's sealed answer, sent from its own device, ends the wait.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn a_waiting_question_survives_a_restart_and_ends_on_the_proxys_answer() {
    use keeper_core::agents::ask::{answer_content, ask_content, AskContent};
    use keeper_core::agents::label::Label;
    use keeper_core::agents::waiting::ASK_READ_BACK;

    const ASK: &str = "01J8ASKAAAAAAAAAAAAAAAAAAA";
    let smoke = Smoke::setup().await;
    let root = TempDir::new();
    let (tola, _) = smoke
        .copy(
            &root.path().join("tola"),
            "nixi-paced",
            "NIXI_PACED_PASSWORD",
            "tola@smoke",
        )
        .await;
    let (proxy, _) = smoke
        .copy(
            &root.path().join("nixi"),
            "nixi-smoke",
            "NIXI_SMOKE_PASSWORD",
            "nixi@smoke",
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
    smoke.set_ratelimit(&smoke.user("nixi-paced"), 0, 0).await;
    person
        .client()
        .event_cache()
        .subscribe()
        .expect("event cache");
    let (me, via) = (smoke.user("tgorka-smoke"), smoke.user("nixi-smoke"));
    let room = tola
        .create_room(
            RoomKind::Session(SessionKind::Delegated),
            "tola 2026-10-15",
            vec![me.clone(), via.clone()],
            &[],
        )
        .await
        .expect("create room");
    sync_until(&person, || person.client().get_room(&room).is_some()).await;
    person.join(&room).await.expect("join");
    sync_until(&proxy, || proxy.client().get_room(&room).is_some()).await;
    proxy.join(&room).await.expect("join");
    tola.sync_once().await.expect("sync");

    let ask = AskContent {
        v: events::CONTENT_VERSION,
        id: ASK.to_owned(),
        question: "Which trip?".to_owned(),
        choices: vec!["Tatry".to_owned(), "Bieszczady".to_owned()],
        default: None,
        to: me.clone(),
        via: via.clone(),
        label: Label::top(),
    };
    let asked = tola
        .send(&room, "m.room.message", ask_content(&ask), None)
        .await
        .expect("ask");
    // More than a restored cache holds, fewer than the panel reads back.
    let fillers = usize::from(ASK_READ_BACK) / 3;
    for n in 0..fillers {
        tola.send(
            &room,
            "m.room.message",
            json!({"msgtype": "m.text", "body": format!("working {n}")}),
            None,
        )
        .await
        .expect("filler");
    }
    let last = format!("working {}", fillers - 1);
    let deadline = Instant::now() + Duration::from_secs(120);
    while !cached_events(&person, &room)
        .await
        .iter()
        .any(|e| e["content"]["body"] == last.as_str())
    {
        assert!(Instant::now() < deadline, "the person never read the room");
        person.sync_once().await.expect("sync");
    }
    let session = StoredSession::from_client(person.client())
        .expect("session")
        .to_json()
        .expect("json");
    drop(person);

    let restored = AgentClient::open(
        &smoke.homeserver,
        &root.path().join("tgorka"),
        "smoke-passphrase",
    )
    .await
    .expect("reopen");
    restored
        .restore(StoredSession::from_json(&session).expect("session"))
        .await
        .expect("restore");
    restored
        .client()
        .event_cache()
        .subscribe()
        .expect("event cache");
    restored.sync_once().await.expect("sync");
    let loaded = cached_events(&restored, &room).await;
    assert!(
        !loaded.iter().any(|e| e["event_id"] == asked.as_str()),
        "the restored cache already held the ask ({} events): nothing to read back",
        loaded.len()
    );

    let ids = waiting_ids(&restored, &room, &me, &via).await;
    assert_eq!(ids, vec![ASK.to_owned()], "the question after a restart");

    let answered = proxy
        .send(&room, "m.room.message", answer_content("Tatry", ASK), None)
        .await
        .expect("answer");
    let deadline = Instant::now() + Duration::from_secs(60);
    while !cached_events(&restored, &room)
        .await
        .iter()
        .any(|e| e["event_id"] == answered.as_str())
    {
        assert!(Instant::now() < deadline, "the answer never arrived");
        restored.sync_once().await.expect("sync");
    }
    let ids = waiting_ids(&restored, &room, &me, &via).await;
    assert!(ids.is_empty(), "the proxy's answer ends the wait: {ids:?}");
}

/// Story 101.6 (R453, review 04): a card waiting for the person survives a
/// restart of their device in the room's state — the restored cache holds
/// only the room's newest chunk, a newer status among it, and the card
/// behind it; the row's reading reads the room back as 101.3's reader does
/// and still says *Needs you*, where the restored chunk alone would not.
#[tokio::test(flavor = "multi_thread")]
#[ignore = "live: Synapse on delectra"]
async fn a_card_behind_a_restarts_chunk_still_needs_the_person() {
    use keeper_core::agents::approval::{binding_digest, summary_of};
    use keeper_core::agents::attention::{attention, of_room, AgentAttentionVm, Reader, RoomEvent};
    use keeper_core::agents::room::holds_agent_power;
    use keeper_core::agents::tier::AgentTool;
    use keeper_core::agents::waiting::ASK_READ_BACK;

    let smoke = Smoke::setup().await;
    let root = TempDir::new();
    let (tola, _) = smoke
        .copy(
            &root.path().join("tola"),
            "nixi-paced",
            "NIXI_PACED_PASSWORD",
            "tola@smoke",
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
    smoke.set_ratelimit(&smoke.user("nixi-paced"), 0, 0).await;
    person
        .client()
        .event_cache()
        .subscribe()
        .expect("event cache");
    let (me, agent) = (smoke.user("tgorka-smoke"), smoke.user("nixi-paced"));
    let room = tola
        .create_room(
            RoomKind::Session(SessionKind::Delegated),
            "tola 2026-10-15",
            vec![me.clone()],
            &[],
        )
        .await
        .expect("create room");
    sync_until(&person, || person.client().get_room(&room).is_some()).await;
    person.join(&room).await.expect("join");
    tola.sync_once().await.expect("sync");

    let status = |run: &str| {
        json!({
            "v": CONTENT_VERSION, "session": "01J9ZQ3W4K5M6N7P8Q9R0SESSN", "kind": "delegated",
            "title": "Tola", "agent": agent.as_str(), "host": "delectra", "epoch": 1, "run": run,
        })
    };
    tola.send(&room, events::STATUS, status("running"), None)
        .await
        .expect("status");
    // A T2 card for a drive write, bound as a host binds it (R185), that
    // the person may decide for an hour.
    let args = json!({"profile": "tgdrive", "path": "notes/plan.md", "content": "hello"});
    let preconditions = json!({
        "files": [{"drive": "tgdrive", "path": "notes/plan.md",
            "landing": "notes/plan.md", "sha256": null}],
        "workspace": null, "screen": null, "max_staleness_s": null,
    });
    let (record, session) = (
        "01J9ZQ3W4K5M6N7P8Q9R0APPRV",
        "60-sessions/active/2026-10-15-tola",
    );
    let digest = binding_digest(
        record,
        session,
        "tola",
        "drive_write",
        &args,
        &Value::Null,
        "c0ffee",
        &preconditions,
    )
    .expect("digest");
    let expires = chrono::Utc::now() + chrono::Duration::hours(1);
    let card = tola
        .send(
            &room,
            events::APPROVAL_REQUEST,
            json!({
                "v": CONTENT_VERSION, "id": record, "session": session,
                "room": room.as_str(), "agent": "tola", "tier": 2,
                "summary": summary_of(AgentTool::DriveWrite, &args, &Value::Null),
                "action": {"tool": "drive_write", "args": args, "exec_binding": null},
                "checkpoint_sha256": "c0ffee",
                "preconditions": preconditions,
                "binding_digest": digest, "scopes": ["once"],
                "expires_at": expires.to_rfc3339(),
                "approvers": [me.as_str()], "dispatch_chain": [me.as_str(), agent.as_str()],
            }),
            None,
        )
        .await
        .expect("card");
    // More than a restored cache holds, fewer than the reading reads back;
    // then a newer status, inside the chunk a restart keeps.
    let fillers = usize::from(ASK_READ_BACK) / 3;
    for n in 0..fillers {
        tola.send(
            &room,
            "m.room.message",
            json!({"msgtype": "m.text", "body": format!("working {n}")}),
            None,
        )
        .await
        .expect("filler");
    }
    let newest = tola
        .send(&room, events::STATUS, status("running"), None)
        .await
        .expect("newer status");
    let deadline = Instant::now() + Duration::from_secs(120);
    while !cached_events(&person, &room)
        .await
        .iter()
        .any(|e| e["event_id"] == newest.as_str())
    {
        assert!(Instant::now() < deadline, "the person never read the room");
        person.sync_once().await.expect("sync");
    }
    let session = StoredSession::from_client(person.client())
        .expect("session")
        .to_json()
        .expect("json");
    drop(person);

    let restored = AgentClient::open(
        &smoke.homeserver,
        &root.path().join("tgorka"),
        "smoke-passphrase",
    )
    .await
    .expect("reopen");
    restored
        .restore(StoredSession::from_json(&session).expect("session"))
        .await
        .expect("restore");
    restored
        .client()
        .event_cache()
        .subscribe()
        .expect("event cache");
    restored.sync_once().await.expect("sync");
    let loaded = cached_events(&restored, &room).await;
    assert!(
        !loaded.iter().any(|e| e["event_id"] == card.as_str()),
        "the restored cache already held the card ({} events): nothing to read back",
        loaded.len()
    );
    assert!(
        loaded.iter().any(|e| e["event_id"] == newest.as_str()),
        "the restored chunk holds the newer status"
    );
    let joined = restored.client().get_room(&room).expect("room");
    let levels = joined.power_levels().await.expect("power levels");
    let is_agent = |user: &matrix_sdk::ruma::UserId| holds_agent_power(Some(&levels), user);
    let owners = [agent.clone()];
    let chunk_alone = attention(
        loaded.iter().map(|event| RoomEvent {
            event,
            sealed: true,
        }),
        &Reader {
            own: &me,
            agent: &is_agent,
            owners: &owners,
            marker: None,
            whole: true,
        },
        None,
        chrono::Utc::now(),
    );
    assert_ne!(
        chunk_alone.state,
        Some(AgentAttentionVm::NeedsYou),
        "the chunk alone holds no card"
    );

    let read = of_room(&joined, None).await;
    assert_eq!(
        read.state,
        Some(AgentAttentionVm::NeedsYou),
        "the card behind the restored chunk"
    );
    assert_eq!(
        read.recheck_at.map(|at| at.timestamp()),
        Some(expires.timestamp())
    );
}

/// The ids of the asks waiting for `me` through `via` in `room`, as the
/// decisions panel's reader reads them: the room read back through its event
/// cache, then folded.
async fn waiting_ids(
    client: &AgentClient,
    room: &RoomId,
    me: &OwnedUserId,
    via: &OwnedUserId,
) -> Vec<String> {
    use keeper_core::agents::room::holds_agent_power;
    use keeper_core::agents::waiting::{fold, read_room};
    let room = client.client().get_room(room).expect("room");
    let (seen, _) = read_room(&room).await.expect("read back");
    let levels = room.power_levels().await.expect("power levels");
    fold(seen, me, via, &|user| {
        holds_agent_power(Some(&levels), user)
    })
    .waiting
    .into_iter()
    .map(|open| open.ask.id)
    .collect()
}
