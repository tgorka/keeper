//! keeper's agents-server client against a real server (AD-418, AD-422).
//!
//! `#[ignore]`: it needs a running server that serves the contract in
//! `syncular/`, with the root routes `GET /databases`, `POST /pair/start`,
//! `POST /pair/redeem` and `POST /clients/{id}/revoke`, and a person who
//! belongs to the databases it names. Endpoints and secrets come from the
//! environment, never from this repository:
//!
//! ```sh
//! KEEPER_HQ_LIVE_URL=http://<host>:<port> \
//! KEEPER_HQ_LIVE_OPERATOR_TOKEN=<the operator's token> \
//! KEEPER_HQ_LIVE_ACTOR=<a person's actor id> \
//! KEEPER_HQ_LIVE_DATABASES=<the person's databases, comma-separated> \
//! KEEPER_HQ_LIVE_ONE_DB_ACTOR=<optional: a person in one database>:<that database> \
//! KEEPER_HQ_LIVE_AGENT=<an agent in the first database's directory, for the Agents window> \
//! cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --test hq_live -- --ignored --nocapture
//! ```
//!
//! The operator's token is used only for the person's first pairing code, as
//! an operator pairs a person's first device. Each device runs behind a local
//! TCP relay, so the test can cut one device off the server and see every
//! byte the other sends. Every log line keeper writes during the run is
//! captured and searched for the token.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use keeper_core::error::CoreError;
use keeper_core::hq::port::{Replica, RowOp, Subscription};
use keeper_core::hq::routes::HttpRoutes;
use keeper_core::hq::syncular::SyncularOpener;
use keeper_core::hq::vm::{HqDatabaseState, HqPhase, HqStatusVm};
use keeper_core::hq::Hq;
use keeper_core::platform::Platform;
use serde_json::{json, Map, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

struct Device {
    data_dir: PathBuf,
    keychain: Mutex<HashMap<String, String>>,
}

impl Platform for Device {
    fn data_dir(&self) -> Result<PathBuf, CoreError> {
        Ok(self.data_dir.clone())
    }
    fn keychain_set(&self, key: &str, value: &str) -> Result<(), CoreError> {
        self.keychain
            .lock()
            .expect("keychain")
            .insert(key.to_owned(), value.to_owned());
        Ok(())
    }
    fn keychain_get(&self, key: &str) -> Result<Option<String>, CoreError> {
        Ok(self.keychain.lock().expect("keychain").get(key).cloned())
    }
    fn keychain_delete(&self, key: &str) -> Result<(), CoreError> {
        self.keychain.lock().expect("keychain").remove(key);
        Ok(())
    }
    fn open_url(&self, _: &str) -> Result<(), CoreError> {
        Ok(())
    }
    fn notify(&self, _: &str, _: &str, _: &keeper_core::vm::NotifyTarget) -> Result<(), CoreError> {
        Ok(())
    }
    fn sidecar_path(&self, _: &str) -> Result<PathBuf, CoreError> {
        Err(CoreError::Unsupported("no sidecars".to_owned()))
    }
    fn exclude_from_backup(&self, _: &Path) -> Result<(), CoreError> {
        Ok(())
    }
    fn set_badge_count(&self, _: Option<u32>) -> Result<(), CoreError> {
        Ok(())
    }
}

/// A TCP relay in front of the server: it can be cut and restored, and it
/// counts the connections and the bytes a device sends, and whether they
/// named a path.
struct Relay {
    port: u16,
    up: Arc<AtomicBool>,
    connections: Arc<AtomicU64>,
    sent: Arc<Mutex<Vec<u8>>>,
    generation: Arc<AtomicU64>,
}

impl Relay {
    async fn start(upstream: String) -> Relay {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("binds");
        let port = listener.local_addr().expect("addr").port();
        let relay = Relay {
            port,
            up: Arc::new(AtomicBool::new(true)),
            connections: Arc::new(AtomicU64::new(0)),
            sent: Arc::new(Mutex::new(Vec::new())),
            generation: Arc::new(AtomicU64::new(0)),
        };
        let (up, connections, sent, generation) = (
            Arc::clone(&relay.up),
            Arc::clone(&relay.connections),
            Arc::clone(&relay.sent),
            Arc::clone(&relay.generation),
        );
        tokio::spawn(async move {
            loop {
                let Ok((client, _)) = listener.accept().await else {
                    return;
                };
                if !up.load(Ordering::SeqCst) {
                    drop(client);
                    continue;
                }
                connections.fetch_add(1, Ordering::SeqCst);
                let Ok(server) = TcpStream::connect(&upstream).await else {
                    continue;
                };
                let born = generation.load(Ordering::SeqCst);
                let (generation, sent) = (Arc::clone(&generation), Arc::clone(&sent));
                tokio::spawn(async move {
                    let (mut client_read, mut client_write) = client.into_split();
                    let (mut server_read, mut server_write) = server.into_split();
                    let alive = || generation.load(Ordering::SeqCst) == born;
                    let mut up_buf = [0u8; 16 * 1024];
                    let mut down_buf = [0u8; 16 * 1024];
                    loop {
                        tokio::select! {
                            read = client_read.read(&mut up_buf) => match read {
                                Ok(0) | Err(_) => return,
                                Ok(n) => {
                                    sent.lock().expect("sent").extend_from_slice(&up_buf[..n]);
                                    if server_write.write_all(&up_buf[..n]).await.is_err() { return; }
                                }
                            },
                            read = server_read.read(&mut down_buf) => match read {
                                Ok(0) | Err(_) => return,
                                Ok(n) => if client_write.write_all(&down_buf[..n]).await.is_err() { return; },
                            },
                            () = tokio::time::sleep(Duration::from_millis(100)) => {}
                        }
                        if !alive() {
                            return;
                        }
                    }
                });
            }
        });
        relay
    }

    fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    /// Refuse new connections and drop every open one.
    fn cut(&self) {
        self.up.store(false, Ordering::SeqCst);
        self.generation.fetch_add(1, Ordering::SeqCst);
    }

    fn restore(&self) {
        self.up.store(true, Ordering::SeqCst);
    }

    fn sent_contains(&self, needle: &str) -> bool {
        let sent = self.sent.lock().expect("sent");
        sent.windows(needle.len()).any(|w| w == needle.as_bytes())
    }
}

fn env(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

fn device(dir: &Path) -> Arc<Device> {
    std::fs::create_dir_all(dir).expect("mkdir");
    Arc::new(Device {
        data_dir: dir.to_owned(),
        keychain: Mutex::new(HashMap::new()),
    })
}

fn hq(device: &Arc<Device>, name: &str) -> Arc<Hq> {
    Hq::new(
        Arc::clone(device) as Arc<dyn Platform>,
        Arc::new(SyncularOpener),
        Arc::new(HttpRoutes::new()),
        name.to_owned(),
    )
}

async fn operator_code(server: &str, token: &str, actor: &str) -> String {
    let answer: Value = reqwest::Client::new()
        .post(format!("{server}/pair/start"))
        .bearer_auth(token)
        .json(&json!({ "actorId": actor }))
        .send()
        .await
        .expect("pair/start")
        .error_for_status()
        .expect("operator pair/start")
        .json()
        .await
        .expect("json");
    answer["code"].as_str().expect("code").to_owned()
}

async fn wait_status(
    hq: &Hq,
    what: &str,
    mut ok: impl FnMut(&HqStatusVm) -> bool,
    within: Duration,
) -> HqStatusVm {
    let deadline = Instant::now() + within;
    loop {
        let status = hq.status().borrow().clone();
        if ok(&status) {
            return status;
        }
        assert!(Instant::now() < deadline, "{what}: {status:#?}");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

async fn query(
    replica: &Arc<dyn Replica>,
    sql: &str,
    params: Vec<Value>,
) -> Vec<Map<String, Value>> {
    let replica = Arc::clone(replica);
    let sql = sql.to_owned();
    tokio::task::spawn_blocking(move || replica.query(&sql, &params).expect("reads"))
        .await
        .expect("joins")
}

async fn wait_rows(
    replica: &Arc<dyn Replica>,
    sql: &str,
    params: Vec<Value>,
    count: usize,
    within: Duration,
) -> Vec<Map<String, Value>> {
    let deadline = Instant::now() + within;
    loop {
        let rows = query(replica, sql, params.clone()).await;
        if rows.len() == count || Instant::now() >= deadline {
            assert_eq!(rows.len(), count, "{sql} {params:?}");
            return rows;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

fn object(value: Value) -> Map<String, Value> {
    match value {
        Value::Object(map) => map,
        _ => unreachable!("an object literal"),
    }
}

fn upsert(table: &str, values: Value) -> RowOp {
    RowOp::Upsert {
        table: table.to_owned(),
        values: object(values),
        base_version: None,
    }
}

async fn mutate(replica: &Arc<dyn Replica>, ops: Vec<RowOp>) -> String {
    let replica = Arc::clone(replica);
    tokio::task::spawn_blocking(move || replica.mutate(ops).expect("commits"))
        .await
        .expect("joins")
}

async fn subscribe(replica: &Arc<dyn Replica>, id: &str, table: &str, variable: &str, value: &str) {
    let replica = Arc::clone(replica);
    let subscription = Subscription {
        id: id.to_owned(),
        table: table.to_owned(),
        scopes: vec![(variable.to_owned(), vec![value.to_owned()])],
    };
    tokio::task::spawn_blocking(move || replica.subscribe(subscription).expect("subscribes"))
        .await
        .expect("joins");
}

struct Author {
    actor: String,
    client: String,
}

fn page_key(session: &str, at_ms: i64) -> String {
    let month = chrono::DateTime::from_timestamp_millis(at_ms)
        .expect("a time")
        .format("%Y-%m");
    format!("{session}/{month}")
}

fn by(author: &Author, mut values: Value) -> Value {
    values["author_actor"] = json!(author.actor);
    values["author_client"] = json!(author.client);
    values
}

/// Session `<client>.<number>` of `author`'s with one message, in one commit.
fn session_with_message(
    author: &Author,
    number: u32,
    agent: &str,
    message_id: &str,
    body: &str,
) -> (String, String, Vec<RowOp>) {
    let now = chrono::Utc::now().timestamp_millis();
    let id = format!("{}.{number}", author.client);
    let page = page_key(&id, now);
    let ops = vec![
        upsert(
            "sessions",
            by(
                author,
                json!({
                    "id": id, "title": "hq_live", "status": "open", "agent_actor": agent,
                    "created_by_actor": author.actor, "created_by_client": author.client,
                    "invite": "[]", "archived": false, "created_at_ms": now, "last_activity_ms": now,
                }),
            ),
        ),
        upsert(
            "session_members",
            by(
                author,
                json!({
                    "id": format!("{id}/{}", author.actor), "session_id": id, "actor_id": author.actor,
                    "role": "owner", "reads_trace": true, "added_by_actor": author.actor, "added_at_ms": now,
                }),
            ),
        ),
        message(author, &id, &page, message_id, body, now),
    ];
    (id, page, ops)
}

fn message(author: &Author, session: &str, page: &str, id: &str, body: &str, at_ms: i64) -> RowOp {
    upsert(
        "messages",
        by(
            author,
            json!({
                "id": id, "session_id": session, "page_key": page, "role": "person", "body": body,
                "created_at_ms": at_ms,
            }),
        ),
    )
}

/// Every log line of the run, at every level.
#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl std::io::Write for Captured {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        self.0.lock().expect("log").extend_from_slice(bytes);
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Captured {
    type Writer = Captured;
    fn make_writer(&'a self) -> Captured {
        self.clone()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs a running agents server; see the module docs"]
async fn pair_discover_replicate_go_offline_and_revoke() {
    let log = Captured::default();
    tracing::subscriber::set_global_default(
        tracing_subscriber::fmt()
            .with_max_level(tracing::Level::TRACE)
            .with_ansi(false)
            .with_writer(log.clone())
            .finish(),
    )
    .expect("the only subscriber");
    let server = env("KEEPER_HQ_LIVE_URL");
    let operator = env("KEEPER_HQ_LIVE_OPERATOR_TOKEN");
    let actor = env("KEEPER_HQ_LIVE_ACTOR");
    let expected: Vec<String> = env("KEEPER_HQ_LIVE_DATABASES")
        .split(',')
        .map(str::to_owned)
        .collect();
    assert!(
        expected.len() >= 2,
        "two databases, to prove they stay apart"
    );
    let upstream = server
        .trim_start_matches("http://")
        .trim_end_matches('/')
        .to_owned();
    let scratch = tempfile::tempdir().expect("tempdir");
    let within = Duration::from_secs(30);

    // Device A: paired by the operator's code, behind a relay that can be cut.
    let relay_a = Relay::start(upstream.clone()).await;
    let platform_a = device(&scratch.path().join("a"));
    let a = hq(&platform_a, "hq_live device a");
    a.start().await.expect("starts");
    assert_eq!(a.status().borrow().phase, HqPhase::NoServer);
    a.set_server(&relay_a.url())
        .await
        .expect("stores the address");
    assert_eq!(a.status().borrow().phase, HqPhase::NotPaired);
    assert_eq!(
        relay_a.connections.load(Ordering::SeqCst),
        0,
        "not paired, no request"
    );
    a.pair_redeem(&operator_code(&server, &operator, &actor).await)
        .await
        .expect("redeems");
    let status = wait_status(
        &a,
        "a connects every database",
        |s| {
            s.phase == HqPhase::Paired
                && s.databases.len() == expected.len()
                && s.databases
                    .iter()
                    .all(|db| db.state == HqDatabaseState::Connected)
        },
        within,
    )
    .await;
    let names: Vec<&str> = status.databases.iter().map(|db| db.name.as_str()).collect();
    assert_eq!(
        names,
        expected.iter().map(String::as_str).collect::<Vec<_>>()
    );
    let a_client = status.device.as_ref().expect("a device").client_id.clone();
    println!("PASS a paired as {a_client}, databases {names:?}");

    // Device B: types the code A shows, at its own relay.
    let relay_b = Relay::start(upstream.clone()).await;
    let platform_b = device(&scratch.path().join("b"));
    let b = hq(&platform_b, "hq_live device b");
    b.start().await.expect("starts");
    b.set_server(&relay_b.url()).await.expect("stores");
    let code = a.pair_start().await.expect("a issues a code").code;
    b.pair_redeem(&code).await.expect("b redeems a's code");
    let status_b = wait_status(
        &b,
        "b connects",
        |s| {
            s.databases.len() == expected.len()
                && s.databases
                    .iter()
                    .all(|db| db.state == HqDatabaseState::Connected)
        },
        within,
    )
    .await;
    let b_client = status_b
        .device
        .as_ref()
        .expect("b device")
        .client_id
        .clone();
    println!("PASS b paired by a's code as {b_client}");

    // The same session number in every database, each with its own words:
    // each database's rows reach B's replica of that database and no other.
    let author = Author {
        actor: actor.clone(),
        client: a_client.clone(),
    };
    let forger = Author {
        actor: actor.clone(),
        client: b_client.clone(),
    };
    let mut pages = Vec::new();
    for db in &expected {
        let ra = a.replica(db).expect("a holds it");
        let rb = b.replica(db).expect("b holds it");
        let agents = wait_rows(
            &ra,
            "SELECT id FROM actors WHERE kind = 'agent' ORDER BY id LIMIT 1",
            vec![],
            1,
            within,
        )
        .await;
        let agent = agents[0]["id"].as_str().expect("an agent").to_owned();
        let (session, page, ops) =
            session_with_message(&author, 1, &agent, "m-1", &format!("words of {db}"));
        mutate(&ra, ops).await;
        // A commit naming B as its author, sent by A: the server refuses it.
        let forged_page = page.clone();
        mutate(
            &ra,
            vec![message(
                &forger,
                &session,
                &forged_page,
                "m-forged",
                "forged",
                chrono::Utc::now().timestamp_millis(),
            )],
        )
        .await;
        subscribe(&rb, &format!("page-{db}"), "messages", "page", &page).await;
        rb.sync_now();
        let rows = wait_rows(
            &rb,
            "SELECT id, body, author_client FROM messages WHERE id = 'm-1'",
            vec![],
            1,
            within,
        )
        .await;
        assert_eq!(rows[0]["body"], format!("words of {db}"));
        assert_eq!(rows[0]["author_client"], a_client.as_str());
        pages.push((db.clone(), session, page, ra, rb));
    }
    // Long enough for the forged commit to have been answered.
    wait_status(
        &a,
        "a's outbox is empty",
        |s| s.databases.iter().all(|db| db.waiting == 0),
        within,
    )
    .await;
    for (db, _, _, ra, rb) in &pages {
        let bodies: Vec<String> = query(rb, "SELECT body FROM messages", vec![])
            .await
            .iter()
            .map(|r| r["body"].as_str().unwrap_or_default().to_owned())
            .collect();
        assert_eq!(
            bodies,
            [format!("words of {db}")],
            "{db}: b holds only its own database's row"
        );
        assert!(
            query(ra, "SELECT id FROM messages WHERE id = 'm-forged'", vec![])
                .await
                .is_empty(),
            "{db}: the forged row was not rolled back"
        );
        println!("PASS {db}: a's row reached b with a's client; the forged one was refused; nothing crossed databases");
    }

    // Both devices are listed on both, from the replicated directory.
    for (label, device) in [("a", &a), ("b", &b)] {
        wait_status(
            device,
            &format!("{label} lists both"),
            |s| {
                let ids: Vec<&str> = s.devices.iter().map(|d| d.client_id.as_str()).collect();
                ids.contains(&a_client.as_str()) && ids.contains(&b_client.as_str())
            },
            within,
        )
        .await;
    }
    println!("PASS both devices are listed on both");

    // A goes offline: writes wait in its outbox, reads answer, then drain.
    relay_a.cut();
    let now = chrono::Utc::now().timestamp_millis();
    for (db, session, page, ra, _) in &pages {
        mutate(
            ra,
            vec![message(
                &author,
                session,
                page,
                &format!("m-offline-{db}"),
                "written offline",
                now,
            )],
        )
        .await;
    }
    wait_status(
        &a,
        "a is offline with its writes waiting",
        |s| {
            s.databases
                .iter()
                .all(|db| db.state == HqDatabaseState::Offline && db.waiting >= 1)
        },
        Duration::from_secs(60),
    )
    .await;
    for (_, session, _, ra, _) in &pages {
        let started = Instant::now();
        assert_eq!(
            query(
                ra,
                "SELECT id FROM messages WHERE session_id = ?1",
                vec![json!(session)]
            )
            .await
            .len(),
            2
        );
        assert!(
            started.elapsed() < Duration::from_millis(500),
            "a read waited on the network"
        );
    }
    println!("PASS offline: outbox waits, reads answer from the replica");
    relay_a.restore();
    wait_status(
        &a,
        "a drains its outbox",
        |s| {
            s.databases
                .iter()
                .all(|db| db.state == HqDatabaseState::Connected && db.waiting == 0)
        },
        Duration::from_secs(120),
    )
    .await;
    for (db, _, _, _, rb) in &pages {
        wait_rows(
            rb,
            "SELECT id FROM messages WHERE id = ?1",
            vec![json!(format!("m-offline-{db}"))],
            1,
            within,
        )
        .await;
    }
    println!("PASS reconnected: the outbox drained and b has the offline writes");

    // A restart: the old service stops and is gone; a new one on the same
    // disk and keychain reconnects with no new code.
    drop(pages);
    a.shutdown().await;
    let gone = Arc::downgrade(&a);
    drop(a);
    tokio::time::sleep(Duration::from_millis(500)).await;
    assert!(
        gone.upgrade().is_none(),
        "the old service outlived its restart"
    );
    let before = relay_a.connections.load(Ordering::SeqCst);
    tokio::time::sleep(Duration::from_secs(6)).await;
    assert_eq!(
        relay_a.connections.load(Ordering::SeqCst),
        before,
        "a stopped service still connected"
    );
    let a = hq(&platform_a, "hq_live device a");
    a.start().await.expect("restarts");
    wait_status(
        &a,
        "a reconnects after a restart",
        |s| {
            s.phase == HqPhase::Paired
                && s.databases
                    .iter()
                    .all(|db| db.state == HqDatabaseState::Connected)
        },
        within,
    )
    .await;
    println!("PASS restart: the old service is gone; the new one is paired and connected with no new code");

    // A removes B: B is refused, says so, stops sending, stays readable.
    a.revoke(&b_client).await.expect("revokes");
    let removed = wait_status(
        &b,
        "b learns it was removed",
        |s| s.phase == HqPhase::Removed,
        Duration::from_secs(60),
    )
    .await;
    assert!(removed
        .databases
        .iter()
        .all(|db| db.state == HqDatabaseState::Removed));
    for db in &expected {
        let rb = b.replica(db).expect("still held");
        assert!(
            !query(&rb, "SELECT id FROM messages", vec![])
                .await
                .is_empty(),
            "{db} readable"
        );
    }
    wait_status(
        &a,
        "a lists b as removed",
        |s| {
            s.devices
                .iter()
                .any(|d| d.client_id == b_client && d.revoked)
        },
        within,
    )
    .await;
    tokio::time::sleep(Duration::from_secs(2)).await;
    let (connections, bytes) = (
        relay_b.connections.load(Ordering::SeqCst),
        relay_b.sent.lock().expect("sent").len(),
    );
    tokio::time::sleep(Duration::from_secs(15)).await;
    assert_eq!(
        relay_b.connections.load(Ordering::SeqCst),
        connections,
        "a removed device opened a connection"
    );
    assert_eq!(
        relay_b.sent.lock().expect("sent").len(),
        bytes,
        "a removed device sent bytes"
    );
    println!("PASS b removed: refused, readable, listed as removed on a, and silent for 15 s");

    // A person in one database never asks for another.
    if let Ok(one) = std::env::var("KEEPER_HQ_LIVE_ONE_DB_ACTOR") {
        let (one_actor, one_db) = one.split_once(':').expect("<actor>:<database>");
        let relay_m = Relay::start(upstream).await;
        let platform_m = device(&scratch.path().join("m"));
        let m = hq(&platform_m, "hq_live device m");
        m.start().await.expect("starts");
        m.set_server(&relay_m.url()).await.expect("stores");
        m.pair_redeem(&operator_code(&server, &operator, one_actor).await)
            .await
            .expect("redeems");
        let status = wait_status(
            &m,
            "m connects its one database",
            |s| s.databases.len() == 1 && s.databases[0].state == HqDatabaseState::Connected,
            within,
        )
        .await;
        assert_eq!(status.databases[0].name, one_db);
        tokio::time::sleep(Duration::from_secs(6)).await;
        for other in expected.iter().filter(|db| db.as_str() != one_db) {
            assert!(
                !relay_m.sent_contains(&format!("/{other}/")),
                "m requested /{other}"
            );
        }
        println!("PASS the one-database person holds {one_db} alone and never requested another");
    }

    // No token, A's or B's, in any file either device wrote or any log line.
    let log = String::from_utf8_lossy(&log.0.lock().expect("log")).into_owned();
    assert!(
        log.contains("keeper_core::hq"),
        "the capture saw keeper's lines"
    );
    for platform in [&platform_a, &platform_b] {
        let stored = platform
            .keychain
            .lock()
            .expect("keychain")
            .values()
            .next()
            .cloned()
            .expect("an item");
        let token: String = serde_json::from_str::<Value>(&stored).expect("json")["token"]
            .as_str()
            .expect("token")
            .to_owned();
        let needle = &token[..8];
        assert!(
            !log.contains(needle),
            "the token's first eight characters are in the log"
        );
        for directory in [&platform_a.data_dir, &platform_b.data_dir] {
            for entry in walk(directory) {
                let bytes = std::fs::read(&entry).expect("reads");
                assert!(
                    !bytes.windows(8).any(|w| w == needle.as_bytes()),
                    "the token is in {}",
                    entry.display()
                );
            }
        }
    }
    println!("PASS no token in any file or log line");
}

/// The pairing a device gets from the operator's code, connected in every
/// database it names.
async fn paired(
    platform: &Arc<Device>,
    server: &str,
    operator: &str,
    actor: &str,
    dbs: usize,
) -> Arc<Hq> {
    let device = hq(platform, "hq_live agents window");
    device.start().await.expect("starts");
    device.set_server(server).await.expect("stores the address");
    device
        .pair_redeem(&operator_code(server, operator, actor).await)
        .await
        .expect("redeems");
    wait_status(
        &device,
        "connects every database",
        |s| {
            s.databases.len() == dbs
                && s.databases
                    .iter()
                    .all(|db| db.state == HqDatabaseState::Connected)
        },
        Duration::from_secs(30),
    )
    .await;
    device
}

async fn count_messages(device: &Arc<Hq>, database: &str, session: &str) -> usize {
    let replica = device.replica(database).expect("a replica");
    let rows = query(
        &replica,
        "SELECT count(*) AS n FROM messages WHERE session_id = ?1",
        vec![json!(session)],
    )
    .await;
    rows[0]["n"].as_u64().unwrap_or(0) as usize
}

async fn wait_count(device: &Arc<Hq>, database: &str, session: &str, want: usize, what: &str) {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let n = count_messages(device, database, session).await;
        if n == want {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "{what}: {n} messages, want {want}"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
}

/// The Agents window against a real server: a session of 1,000 messages
/// written on one device lists and opens on another from its replica;
/// archiving it there evicts its messages; opened archived, its newest page
/// arrives through the registered `sessionPage` query within a second
/// (NFR-127, p95 over the runs); unarchiving brings the rows back.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs a running agents server; see the module docs"]
async fn agents_window_archive_and_an_archived_session_opens_within_a_second() {
    use keeper_core::hq::sessions::AgentSessionRef;
    use keeper_core::hq::timeline::PAGE_SIZE;

    const MESSAGES: usize = 1_000;
    const RUNS: usize = 20;
    let server = env("KEEPER_HQ_LIVE_URL");
    let operator = env("KEEPER_HQ_LIVE_OPERATOR_TOKEN");
    let actor = env("KEEPER_HQ_LIVE_ACTOR");
    let agent = env("KEEPER_HQ_LIVE_AGENT");
    let databases: Vec<String> = env("KEEPER_HQ_LIVE_DATABASES")
        .split(',')
        .map(str::to_owned)
        .collect();
    let database = databases[0].clone();
    let scratch = tempfile::tempdir().expect("tempdir");

    let a = paired(
        &device(&scratch.path().join("a")),
        &server,
        &operator,
        &actor,
        databases.len(),
    )
    .await;
    let key: AgentSessionRef = {
        let a = Arc::clone(&a);
        let (database, agent) = (database.clone(), agent.clone());
        tokio::task::spawn_blocking(move || {
            a.new_session(&database, &agent, "the first of a thousand")
        })
        .await
        .expect("joins")
        .expect("a new session")
    };
    let source = a
        .sources()
        .into_iter()
        .find(|s| s.database == database)
        .expect("the database");
    let author = Author {
        actor: source.actor.clone(),
        client: source.client.clone(),
    };
    let start = chrono::Utc::now().timestamp_millis() - MESSAGES as i64;
    let page = page_key(&key.session_id, start);
    let ops: Vec<RowOp> = (1..MESSAGES)
        .map(|i| {
            let id = ulid::Ulid::new().to_string();
            message(
                &author,
                &key.session_id,
                &page,
                &id,
                &format!("message {i}"),
                start + i as i64,
            )
        })
        .collect();
    for chunk in ops.chunks(100) {
        mutate(&source.replica, chunk.to_vec()).await;
    }
    source.replica.sync_now();
    wait_status(
        &a,
        "a's outbox drains",
        |s| s.databases.iter().all(|db| db.waiting == 0),
        Duration::from_secs(60),
    )
    .await;
    println!("PASS a wrote a session of {MESSAGES} messages in {database}");
    // Committed, a's own messages read sent — never a receipt, the server's word.
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let view = a.session_view(&key).expect("a's view");
        let states: Vec<_> = view.messages.iter().map(|m| m.state).collect();
        if view.complete
            && view.messages.len() == MESSAGES
            && states
                .iter()
                .all(|s| *s == Some(keeper_core::hq::timeline::AgentMessageStateVm::Sent))
        {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "a's messages never read sent: {} shown",
            view.messages.len()
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    println!("PASS a's {MESSAGES} messages read sent once the server took them");

    // Another device lists it from its replica, labelled, and holds its newest month.
    let b = paired(
        &device(&scratch.path().join("b")),
        &server,
        &operator,
        &actor,
        databases.len(),
    )
    .await;
    let deadline = Instant::now() + Duration::from_secs(60);
    let row = loop {
        if let Some(row) = b
            .sessions()
            .into_iter()
            .find(|r| r.session_id == key.session_id && r.database == database)
        {
            break row;
        }
        assert!(Instant::now() < deadline, "b never listed the session");
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    assert!(!row.archived);
    wait_count(
        &b,
        &database,
        &key.session_id,
        MESSAGES,
        "b holds the newest month",
    )
    .await;
    println!(
        "PASS b lists {}/{} and holds its {MESSAGES} messages",
        row.database, row.session_id
    );

    // Archiving on b evicts its messages there; the row stays listed.
    {
        let (b, key) = (Arc::clone(&b), key.clone());
        tokio::task::spawn_blocking(move || b.set_archived(&key, true))
            .await
            .expect("joins")
            .expect("archives");
    }
    wait_count(&b, &database, &key.session_id, 0, "archiving evicts").await;
    let deadline = Instant::now() + Duration::from_secs(30);
    while !b
        .sessions()
        .iter()
        .any(|r| r.session_id == key.session_id && r.archived)
    {
        assert!(Instant::now() < deadline, "b never listed it archived");
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    println!("PASS archived on b: 0 of its messages left in b's replica, still listed as archived");

    // NFR-127: opened archived, the newest page through the server.
    let mut took = Vec::new();
    for _ in 0..RUNS {
        let began = Instant::now();
        b.open_session(&key).expect("opens");
        let view = loop {
            let view = b.session_view(&key).expect("a view");
            if view.complete || view.note.is_some() {
                break view;
            }
            assert!(began.elapsed() < Duration::from_secs(10), "no page");
            tokio::time::sleep(Duration::from_millis(2)).await;
        };
        took.push(began.elapsed());
        b.close_session(&key);
        assert_eq!(view.note, None);
        assert!(view.read_only && view.has_earlier);
        assert_eq!(view.messages.len(), PAGE_SIZE);
        assert_eq!(
            view.messages.last().map(|m| m.body.as_str()),
            Some(format!("message {}", MESSAGES - 1).as_str()),
            "the newest page ends on the newest message"
        );
    }
    took.sort();
    let p95 = took[(took.len() * 95).div_ceil(100) - 1];
    println!(
        "NFR-127 {RUNS} opens of an archived {MESSAGES}-message session: min {:?}, median {:?}, p95 {:?}, max {:?}",
        took[0],
        took[took.len() / 2],
        p95,
        took[took.len() - 1]
    );
    assert!(p95 <= Duration::from_secs(1), "NFR-127: p95 {p95:?}");

    // Scrolling back reads the next page, still through the server.
    b.open_session(&key).expect("opens");
    let deadline = Instant::now() + Duration::from_secs(10);
    while !b.session_view(&key).expect("view").complete {
        assert!(Instant::now() < deadline);
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    b.page_back(&key).expect("pages back");
    loop {
        let view = b.session_view(&key).expect("view");
        if view.messages.len() == 2 * PAGE_SIZE {
            break;
        }
        assert!(Instant::now() < deadline, "the second page never came");
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    b.close_session(&key);
    println!("PASS scrolling back read the next {PAGE_SIZE} through sessionPage");

    // Unarchiving widens the window again.
    {
        let (b, key) = (Arc::clone(&b), key.clone());
        tokio::task::spawn_blocking(move || b.set_archived(&key, false))
            .await
            .expect("joins")
            .expect("unarchives");
    }
    wait_count(
        &b,
        &database,
        &key.session_id,
        MESSAGES,
        "unarchiving brings them back",
    )
    .await;
    println!("PASS unarchived on b: its {MESSAGES} messages are back");
    a.shutdown().await;
    b.shutdown().await;
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        let path = entry.path();
        if path.is_dir() {
            files.extend(walk(&path));
        } else {
            files.push(path);
        }
    }
    files
}
