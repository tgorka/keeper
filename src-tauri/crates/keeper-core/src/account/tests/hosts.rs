//! Story 102.3 #1 (R466, R484): the hosts a person's `agent.toml` save is
//! checked against are the server's room state, read at every save; the
//! store and a recent server read serve only what a header or a Settings
//! row shows. A control room the server does not answer for refuses the
//! save instead of passing it.
//!
//! A real `matrix_sdk::Client` over a loopback fake homeserver: one sync puts
//! the control room in the store, with or without host manifests in its
//! state; the fake's `GET /rooms/{id}/state` answers what the server holds.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use matrix_sdk::config::SyncSettings;
use matrix_sdk::ruma::{OwnedUserId, UserId};
use matrix_sdk::Client;
use serde_json::{json, Value};

use crate::account::{control_manifests, HostReads, HostsFor};
use crate::agents::events::{CONTROL_ROOM_TYPE, HOST};
use crate::agents::model::{self, ServingHost, HOSTS_UNREAD};
use crate::forges::testing::{self, Reply, Seen};

const ME: &str = "@tgorka:example.org";
const NIXI: &str = "@nixi:example.org";
const CONTROL: &str = "!control:example.org";
/// 2025-10-15T12:00:00Z.
const NOW: u64 = 1_760_529_600_000;
const LIVE: &str = "2099-01-01T00:00:00Z";
const GONE: &str = "2020-01-01T00:00:00Z";

/// What the fake's `/state` answers: the room's state events, or a failure.
#[derive(Clone)]
enum Server {
    State(Vec<Value>),
    Down,
}

struct Device {
    client: Client,
    fake: testing::Fake,
    server: Arc<Mutex<Server>>,
}

fn event(kind: &str, key: &str, sender: &str, content: Value, n: u64) -> Value {
    json!({
        "type": kind, "state_key": key, "sender": sender, "content": content,
        "event_id": format!("$s{n}:h"), "origin_server_ts": 1_000 + n,
    })
}

/// `host`'s manifest on keeper `version`, live until `expires`, serving
/// Nixi, naming an engine as `web_search` says.
fn manifest(host: &str, version: &str, expires: &str, web_search: Option<bool>) -> Value {
    let mut content = json!({
        "v": 1, "host": host, "principal": "tgorka", "version": version,
        "always_on": true, "tools": ["sandbox"], "drives": [], "bots": [],
        "agents": ["tgdrive/nixi"],
        "renewed_at": "2025-10-15T11:59:00Z", "expires_at": expires,
    });
    if let Some(searches) = web_search {
        content["web_search"] = json!(searches);
    }
    let n = if host == "electra" { 9 } else { 10 };
    event(HOST, host, NIXI, content, n)
}

fn control_state(extra: Vec<Value>) -> Vec<Value> {
    let mut state = vec![
        event(
            "m.room.create",
            "",
            NIXI,
            json!({ "creator": NIXI, "room_version": "11", "type": CONTROL_ROOM_TYPE }),
            1,
        ),
        event(
            "m.room.member",
            NIXI,
            NIXI,
            json!({ "membership": "join" }),
            2,
        ),
        event("m.room.member", ME, ME, json!({ "membership": "join" }), 3),
    ];
    state.extend(extra);
    state
}

impl Device {
    /// A device whose store holds the control room with `stored` in its
    /// state, over a server whose `/state` answers `server`.
    async fn new(stored: Vec<Value>, server: Server) -> Device {
        let server = Arc::new(Mutex::new(server));
        let answering = Arc::clone(&server);
        let sync = json!({
            "next_batch": "s1",
            "rooms": { "join": { CONTROL: {
                "state": { "events": control_state(stored) },
                "timeline": { "events": [], "limited": false },
            } } },
        });
        let synced = Arc::new(Mutex::new(Some(sync)));
        let fake = testing::serve(move |seen: &Seen| {
            let path = seen.path.as_str();
            if path.ends_with("/sync") {
                let body = synced
                    .lock()
                    .expect("sync")
                    .take()
                    .unwrap_or_else(|| json!({ "next_batch": "idle" }));
                return Reply::json(200, &body.to_string());
            }
            if path.ends_with("/state") {
                return match &*answering.lock().expect("server") {
                    Server::State(events) => {
                        Reply::json(200, &Value::Array(events.clone()).to_string())
                    }
                    Server::Down => {
                        Reply::json(502, r#"{"errcode":"M_UNKNOWN","error":"bad gateway"}"#)
                    }
                };
            }
            if path.ends_with("/versions") {
                return Reply::json(200, r#"{"versions":["v1.1","v1.11","v1.12"]}"#);
            }
            if path.ends_with("/keys/upload") {
                return Reply::json(200, r#"{"one_time_key_counts":{"signed_curve25519":50}}"#);
            }
            if path.ends_with("/keys/query") {
                return Reply::json(200, r#"{"device_keys":{},"failures":{}}"#);
            }
            Reply::json(200, "{}")
        });
        let client = Client::builder()
            .homeserver_url(&fake.base)
            .build()
            .await
            .expect("client");
        let session: matrix_sdk::authentication::matrix::MatrixSession = serde_json::from_value(
            json!({ "user_id": ME, "device_id": "MAC", "access_token": "t" }),
        )
        .expect("session");
        client.restore_session(session).await.expect("restore");
        client
            .sync_once(SyncSettings::default().timeout(Duration::ZERO))
            .await
            .expect("sync");
        Device {
            client,
            fake,
            server,
        }
    }

    fn state_reads(&self) -> usize {
        self.fake
            .requests()
            .iter()
            .filter(|seen| seen.path.ends_with("/state"))
            .count()
    }

    /// The serving hosts other than `me`, read for `purpose` now.
    async fn serving(
        &self,
        reads: &HostReads,
        purpose: HostsFor,
        me: Option<&str>,
    ) -> Result<Vec<ServingHost>, String> {
        let manifests = control_manifests(&self.client, NOW, reads, purpose).await?;
        let agents: Vec<OwnedUserId> = vec![UserId::parse(NIXI).expect("nixi")];
        Ok(model::serving_from(
            manifests
                .iter()
                .map(|(key, sender, content)| (key.as_str(), &**sender, content)),
            &agents,
            NOW,
            me,
        ))
    }
}

fn nixi_saving(extra: &str) -> crate::agents::home::AgentConfig {
    crate::agents::home::parse_agent_toml(
        &format!("version = 1\nid = \"nixi\"\nname = \"nixi\"\nkind = \"steward\"\nmatrix_user = \"@nixi:example.org\"\n\n[model]\nbot = \"bot:openai:https://provider.example:8452#claude-opus\"\n{extra}"),
        "nixi",
        &crate::agents::drive::parse(
            "version = 1\nid = \"tgdrive\"\ntitle = \"tgdrive\"\nprincipal = \"tgorka\"\nowner = \"@tgorka:example.org\"\nreaders = [\"@tgorka:example.org\"]\n",
        )
        .expect("drive"),
    )
    .expect("home")
}

const SAVE: HostsFor = HostsFor::Save;
const SHOWING: HostsFor = HostsFor::Showing(Duration::from_secs(60));

fn electra(version: &str, web_search: Option<bool>) -> ServingHost {
    ServingHost {
        host: "electra".to_owned(),
        version: version.to_owned(),
        agents: vec!["tgdrive/nixi".to_owned()],
        web_search,
        sandbox: true,
    }
}

/// R466: a store holding no manifest at all — what the sliding sync leaves
/// — reads electra's from the server, so a 0.9.0 host refuses a save of
/// `[web_search]` it would refuse to load.
#[tokio::test]
async fn an_empty_store_reads_the_hosts_from_the_server() {
    let device = Device::new(
        Vec::new(),
        Server::State(control_state(vec![manifest(
            "electra", "0.9.0", LIVE, None,
        )])),
    )
    .await;
    let read = device.serving(&HostReads::default(), SAVE, None).await;
    assert_eq!(read, Ok(vec![electra("0.9.0", None)]));
    assert_eq!(device.state_reads(), 1);

    let config = nixi_saving("\n[web_search]\nask = false\n");
    let hosts = read.expect("read");
    let refused = model::older_hosts(&config, "tgdrive/nixi", Ok(&hosts)).expect_err("0.9.0");
    assert!(
        refused.starts_with("electra runs keeper 0.9.0"),
        "{refused}"
    );
}

/// R484 (review R102T1-01): a store whose every manifest is live proves
/// only that those are live. This device's own new-build manifest is in
/// the store while electra's 0.9.0 one is only on the server: a save reads
/// the server and is refused naming electra, and with the server down it
/// is refused as unread — never let through on the store's word.
#[tokio::test]
async fn a_save_reads_the_server_even_when_the_store_holds_live_manifests() {
    let both = control_state(vec![
        manifest("electra", "0.9.0", LIVE, None),
        manifest("hesperia", "0.10.6", LIVE, Some(true)),
    ]);
    let device = Device::new(
        vec![manifest("hesperia", "0.10.6", LIVE, Some(true))],
        Server::State(both),
    )
    .await;
    let config = nixi_saving("\n[web_search]\nask = false\n");
    let read = device
        .serving(&HostReads::default(), SAVE, Some("hesperia"))
        .await
        .expect("read");
    assert_eq!(read, vec![electra("0.9.0", None)]);
    assert_eq!(device.state_reads(), 1);
    let refused = model::older_hosts(&config, "tgdrive/nixi", Ok(&read)).expect_err("0.9.0");
    assert!(
        refused.starts_with("electra runs keeper 0.9.0"),
        "{refused}"
    );

    *device.server.lock().expect("server") = Server::Down;
    let unread = device
        .serving(&HostReads::default(), SAVE, Some("hesperia"))
        .await
        .expect_err("the server is down");
    assert_eq!(unread, HOSTS_UNREAD);
    let refused = model::older_hosts(&config, "tgdrive/nixi", Err(&unread)).expect_err("unread");
    assert!(
        refused.starts_with("keeper could not read which hosts serve Nixi"),
        "{refused}"
    );
}

/// R466, R484: what a header shows takes the store while every manifest
/// it holds is live, without asking the server; a stored manifest that
/// expired is not believed over the server's live one.
#[tokio::test]
async fn a_header_reads_a_live_store_and_a_stale_one_again_from_the_server() {
    let stale = Device::new(
        vec![manifest("electra", "0.10.6", GONE, Some(true))],
        Server::State(control_state(vec![manifest(
            "electra", "0.9.0", LIVE, None,
        )])),
    )
    .await;
    assert_eq!(
        stale.serving(&HostReads::default(), SHOWING, None).await,
        Ok(vec![electra("0.9.0", None)])
    );
    assert_eq!(stale.state_reads(), 1);

    let live = Device::new(
        vec![manifest("electra", "0.10.6", LIVE, Some(false))],
        Server::Down,
    )
    .await;
    assert_eq!(
        live.serving(&HostReads::default(), SHOWING, None).await,
        Ok(vec![electra("0.10.6", Some(false))])
    );
    assert_eq!(live.state_reads(), 0);
}

/// R466: a control room the server does not answer for is an unread
/// inventory — the save naming the agent is refused — never an empty one
/// that lets it through.
#[tokio::test]
async fn a_control_room_the_server_does_not_answer_for_refuses_the_save() {
    let device = Device::new(Vec::new(), Server::Down).await;
    let read = device.serving(&HostReads::default(), SAVE, None).await;
    assert_eq!(read, Err(HOSTS_UNREAD.to_owned()));
    let config = nixi_saving("effort = \"high\"\n");
    let unread = read.expect_err("unread");
    let refused = model::older_hosts(&config, "tgdrive/nixi", Err(&unread)).expect_err("unread");
    assert!(
        refused.starts_with("keeper could not read which hosts serve Nixi"),
        "{refused}"
    );
}

/// The headers' refresh asks the server for a room at most once while its
/// last answer is younger than the window; a save asks every time.
#[tokio::test]
async fn the_headers_reuse_a_recent_server_read_and_a_save_does_not() {
    let device = Device::new(
        Vec::new(),
        Server::State(control_state(vec![manifest(
            "electra",
            "0.10.6",
            LIVE,
            Some(true),
        )])),
    )
    .await;
    let reads = HostReads::default();
    for _ in 0..3 {
        assert_eq!(
            device.serving(&reads, SHOWING, None).await,
            Ok(vec![electra("0.10.6", Some(true))])
        );
    }
    assert_eq!(device.state_reads(), 1);
    *device.server.lock().expect("server") = Server::Down;
    assert_eq!(
        device.serving(&reads, SHOWING, None).await,
        Ok(vec![electra("0.10.6", Some(true))]),
        "a recent read is reused"
    );
    assert_eq!(
        device.serving(&reads, SAVE, None).await,
        Err(HOSTS_UNREAD.to_owned()),
        "a save asks again"
    );
}
