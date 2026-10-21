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
//! KEEPER_HQ_LIVE_AGENT_2=<for many people: an agent in the second database's directory> \
//! KEEPER_HQ_LIVE_OTHER_DB_ACTOR=<for many people: a person of the second database alone>:<that database> \
//! KEEPER_HQ_LIVE_APPROVAL=<optional, many people: a message the first database's agent answers by asking for an approval> \
//! KEEPER_HQ_LIVE_MODEL_RECORD=<for the tool switches: a URL answering the model's requests as JSON> \
//! KEEPER_HQ_LIVE_KILL=<for the killed instance, run last: a URL that SIGKILLs the agent's instance> \
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

/// A live answer against a real agent instance: a person writes to the
/// agent in a new session; the answer grows in keeper-core's session view
/// as the instance streams it — chunks and the presence tail folded, only
/// ever growing, always a prefix of the answer — and a second device that
/// pairs and opens the session mid-answer sees at least everything the
/// first showed when it opened, then the same final row. Needs an instance
/// of `KEEPER_HQ_LIVE_AGENT` serving the first database and a model that
/// streams for several seconds.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs a running agents server and an agent instance; see the module docs"]
async fn a_live_answer_grows_token_by_token_and_a_late_opener_sees_it_so_far() {
    use keeper_core::hq::live::AgentViewPrefsPatch;
    use keeper_core::hq::sessions::AgentSessionRef;
    use keeper_core::hq::timeline::{AgentMessageRoleVm, AgentSessionViewVm};

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
    a.set_view_prefs(AgentViewPrefsPatch {
        show_reasoning: Some(true),
        show_details: Some(true),
    })
    .expect("prefs");
    let key: AgentSessionRef = {
        let a = Arc::clone(&a);
        let (database, agent) = (database.clone(), agent.clone());
        tokio::task::spawn_blocking(move || a.new_session(&database, &agent, "long: tell me"))
            .await
            .expect("joins")
            .expect("a new session")
    };
    a.open_session(&key).expect("opens");
    let view_of = |hq: &Arc<Hq>, key: &AgentSessionRef| {
        let (hq, key) = (Arc::clone(hq), key.clone());
        async move {
            tokio::task::spawn_blocking(move || hq.session_view(&key))
                .await
                .expect("joins")
                .expect("a view")
        }
    };
    let answer_of = |view: &AgentSessionViewVm| {
        view.messages
            .iter()
            .rev()
            .find(|m| m.role == AgentMessageRoleVm::Agent)
            .cloned()
    };

    // The second device opens the session once the first shows some text.
    // It reports the moment it opened: what a had shown by then is what b
    // must show at least.
    let (opening_tx, mut opening_rx) = tokio::sync::mpsc::unbounded_channel::<Instant>();
    let (first_tx, mut first_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
    let mut late = Some({
        let (server, operator, actor, key) =
            (server.clone(), operator.clone(), actor.clone(), key.clone());
        let dir = scratch.path().join("b");
        let dbs = databases.len();
        move || async move {
            let b = paired(&device(&dir), &server, &operator, &actor, dbs).await;
            let deadline = Instant::now() + Duration::from_secs(30);
            while b.session_view(&key).is_err() {
                assert!(Instant::now() < deadline, "b never held the session");
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
            opening_tx.send(Instant::now()).expect("main listens");
            b.open_session(&key).expect("b opens");
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                let view = b.session_view(&key).expect("b's view");
                if let Some(turn) = view.live.first() {
                    if !turn.text.is_empty() {
                        first_tx.send(turn.text.clone()).expect("main listens");
                        break;
                    }
                }
                assert!(Instant::now() < deadline, "b never showed the answer");
                tokio::time::sleep(Duration::from_millis(5)).await;
            }
            b
        }
    });

    let started = Instant::now();
    let mut changes = a.changes();
    let mut tails = a.live_changes();
    let mut shown: Vec<(u128, String)> = Vec::new();
    // When each length was first shown, by this host's clock: against the
    // model's emission times, the tail's delay from model to viewer.
    let mut received: Vec<(i64, usize)> = Vec::new();
    let mut reasoning_seen = false;
    let mut late_task = None;
    let mut a_at_open: Option<String> = None;
    let mut b_first: Option<(String, String)> = None;
    let final_view = loop {
        assert!(
            started.elapsed() < Duration::from_secs(180),
            "no answer in 3 minutes"
        );
        let view = view_of(&a, &key).await;
        if let Some(turn) = view.live.first() {
            let last = shown.last().map_or("", |(_, text)| text.as_str());
            assert!(
                turn.text.starts_with(last),
                "a showed {last:?}, then {:?}",
                turn.text
            );
            if turn.text != last {
                shown.push((started.elapsed().as_millis(), turn.text.clone()));
                received.push((chrono::Utc::now().timestamp_millis(), turn.text.len()));
            }
            reasoning_seen |= turn.reasoning.as_ref().is_some_and(|r| !r.is_empty());
        }
        if shown.len() >= 5 {
            if let Some(start) = late.take() {
                late_task = Some(tokio::spawn(start()));
            }
        }
        if let Ok(at) = opening_rx.try_recv() {
            let by = at.saturating_duration_since(started).as_millis();
            a_at_open = shown
                .iter()
                .take_while(|(when, _)| *when <= by)
                .last()
                .map(|(_, text)| text.clone());
        }
        if let Ok(text) = first_rx.try_recv() {
            let a_now = shown.last().map(|(_, t)| t.clone()).unwrap_or_default();
            b_first = Some((text, a_now));
        }
        if view.live.is_empty() && answer_of(&view).is_some() && !shown.is_empty() {
            break view;
        }
        tokio::select! {
            _ = changes.changed() => {}
            _ = tails.changed() => {}
            () = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
    };
    let answer = answer_of(&final_view).expect("the agent's answer");
    for (_, text) in &shown {
        assert!(
            answer.body.starts_with(text.as_str()),
            "{text:?} is not a prefix of the answer"
        );
    }
    let gaps: Vec<u128> = shown.windows(2).map(|w| w[1].0 - w[0].0).collect();
    println!(
        "PASS a showed the answer growing in {} steps over {} ms (median step {} ms), each a prefix of the final {} bytes; the last shown {} the final row",
        shown.len(),
        shown.last().map_or(0, |(at, _)| *at) - shown[0].0,
        {
            let mut sorted = gaps.clone();
            sorted.sort_unstable();
            sorted.get(sorted.len() / 2).copied().unwrap_or(0)
        },
        answer.body.len(),
        if shown.last().map(|(_, t)| t.as_str()) == Some(answer.body.as_str()) {
            "equals"
        } else {
            "is a prefix of"
        },
    );
    assert!(
        shown.len() >= 20,
        "token by token: only {} steps",
        shown.len()
    );
    println!("RECEIVED {}", json!(received));
    let details = answer.details.clone().expect("details, shown");
    assert!(details.model.is_some(), "{details:?}");
    println!("PASS the final row's details: {details:?}");
    assert!(
        reasoning_seen,
        "a reader showing the thinking saw it stream"
    );
    assert!(answer.reasoning.is_some(), "and kept it after the end");
    println!("PASS the thinking streamed beside the answer and stayed after it");

    let b = late_task.expect("b started").await.expect("b joins");
    let (b_text, a_then) = b_first.expect("b showed the answer while it was written");
    let a_open = a_at_open.unwrap_or_default();
    assert!(answer.body.starts_with(&b_text), "b's {b_text:?}");
    assert!(
        b_text.len() >= a_open.len(),
        "b opened when a showed {} bytes, and showed only {}",
        a_open.len(),
        b_text.len()
    );
    println!(
        "PASS b opened mid-answer when a showed {} bytes and at once showed {} (a then {}), of {}",
        a_open.len(),
        b_text.len(),
        a_then.len(),
        answer.body.len()
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    let b_answer = loop {
        let view = view_of(&b, &key).await;
        if let (true, Some(m)) = (view.live.is_empty(), answer_of(&view)) {
            break m;
        }
        assert!(Instant::now() < deadline, "b never held the final row");
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(b_answer.body, answer.body, "the same final row on both");
    println!("PASS b ends on the same final row, byte for byte");
    a.shutdown().await;
    b.shutdown().await;
}

/// A member who does not read the trace, and a session's tool switches,
/// against a real agent instance. The invited person sees the answer grow
/// but is shown no thinking and no tool step, and holds no reasoning or
/// tool-call row, even with *Show thinking* on; the owner, a reader, sees
/// the thinking. The tools the agent says it offers are listed on; one
/// switched off reaches the other member and is not offered to the model
/// on the next turn, and switched back on it is again. Needs
/// `KEEPER_HQ_LIVE_ONE_DB_ACTOR` (the invited person, in the first
/// database), an instance of the agent whose `agent_profiles.tools` names
/// at least one tool, and `KEEPER_HQ_LIVE_MODEL_RECORD`: a URL answering
/// the requests the model was sent as JSON `[{ "lastUser", "tools" }]`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs a running agents server and an agent instance; see the module docs"]
async fn a_non_reader_holds_no_thinking_and_a_switched_off_tool_is_not_offered() {
    use keeper_core::hq::live::AgentViewPrefsPatch;
    use keeper_core::hq::sessions::AgentSessionRef;
    use keeper_core::hq::timeline::{AgentMessageRoleVm, AgentMessageVm, AgentSessionViewVm};

    let server = env("KEEPER_HQ_LIVE_URL");
    let operator = env("KEEPER_HQ_LIVE_OPERATOR_TOKEN");
    let actor = env("KEEPER_HQ_LIVE_ACTOR");
    let agent = env("KEEPER_HQ_LIVE_AGENT");
    let record = env("KEEPER_HQ_LIVE_MODEL_RECORD");
    let databases = env("KEEPER_HQ_LIVE_DATABASES");
    let database = databases.split(',').next().expect("a database").to_owned();
    let guest_of = env("KEEPER_HQ_LIVE_ONE_DB_ACTOR");
    let (guest, guest_db) = guest_of.split_once(':').expect("<actor>:<database>");
    assert_eq!(guest_db, database, "the guest is in the first database");
    let scratch = tempfile::tempdir().expect("tempdir");
    let a = paired(
        &device(&scratch.path().join("a")),
        &server,
        &operator,
        &actor,
        databases.split(',').count(),
    )
    .await;
    let k = paired(
        &device(&scratch.path().join("k")),
        &server,
        &operator,
        guest,
        1,
    )
    .await;
    for hq in [&a, &k] {
        hq.set_view_prefs(AgentViewPrefsPatch {
            show_reasoning: Some(true),
            show_details: Some(true),
        })
        .expect("prefs");
    }
    let client = a
        .status()
        .borrow()
        .device
        .as_ref()
        .expect("a device")
        .client_id
        .clone();
    let author = Author {
        actor: actor.clone(),
        client,
    };
    let first = ulid::Ulid::new().to_string();
    let (id, _, mut ops) =
        session_with_message(&author, 1, &agent, &first, "long: think it through");
    if let RowOp::Upsert { values, .. } = &mut ops[0] {
        values.insert("invite".to_owned(), json!(json!([guest]).to_string()));
    }
    mutate(&a.replica(&database).expect("a holds it"), ops).await;
    let key = AgentSessionRef {
        database: database.clone(),
        session_id: id,
    };
    let view_of = |hq: &Arc<Hq>| {
        let (hq, key) = (Arc::clone(hq), key.clone());
        async move {
            tokio::task::spawn_blocking(move || hq.session_view(&key))
                .await
                .expect("joins")
        }
    };
    let answer_of = |view: &AgentSessionViewVm| -> Option<AgentMessageVm> {
        view.messages
            .iter()
            .rev()
            .find(|m| m.role == AgentMessageRoleVm::Agent)
            .cloned()
    };
    let deadline = Instant::now() + Duration::from_secs(30);
    while view_of(&k).await.is_err() {
        assert!(
            Instant::now() < deadline,
            "the guest never held the session"
        );
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    a.open_session(&key).expect("a opens");
    k.open_session(&key).expect("k opens");

    let (mut a_thinking, mut k_text) = (false, false);
    let mut changes = a.changes();
    let mut tails = a.live_changes();
    let started = Instant::now();
    let a_answer = loop {
        assert!(
            started.elapsed() < Duration::from_secs(180),
            "no answer in 3 minutes"
        );
        let (va, vk) = (
            view_of(&a).await.expect("a's view"),
            view_of(&k).await.expect("k's view"),
        );
        assert!(
            va.reads_trace && !vk.reads_trace,
            "the owner reads the trace, the guest does not"
        );
        a_thinking |= va.live.iter().any(|t| t.reasoning.is_some());
        k_text |= vk.live.iter().any(|t| !t.text.is_empty());
        assert!(
            vk.live
                .iter()
                .all(|t| t.reasoning.is_none() && t.steps.is_empty()),
            "the guest was shown {:?}",
            vk.live
        );
        if let (true, Some(answer)) = (va.live.is_empty() && a_thinking, answer_of(&va)) {
            break answer;
        }
        tokio::select! {
            _ = changes.changed() => {}
            _ = tails.changed() => {}
            () = tokio::time::sleep(Duration::from_millis(50)) => {}
        }
    };
    assert!(
        a_answer.reasoning.is_some(),
        "the owner keeps the thinking after the end"
    );
    assert!(k_text, "the guest saw the answer while it was written");
    let deadline = Instant::now() + Duration::from_secs(30);
    let k_answer = loop {
        let view = view_of(&k).await.expect("k's view");
        if let (true, Some(m)) = (view.live.is_empty(), answer_of(&view)) {
            break m;
        }
        assert!(
            Instant::now() < deadline,
            "the guest never held the final row"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    };
    assert_eq!(k_answer.body, a_answer.body, "the same final row on both");
    assert!(
        k_answer.reasoning.is_none() && k_answer.steps.is_none(),
        "{k_answer:?}"
    );
    let rk = k.replica(&database).expect("k holds it");
    for table in ["reasoning", "tool_calls"] {
        let rows = query(
            &rk,
            &format!("SELECT COUNT(*) AS n FROM {table} WHERE session_id = ?1"),
            vec![json!(key.session_id)],
        )
        .await;
        assert_eq!(rows[0]["n"], json!(0), "the guest holds {table} rows");
    }
    println!(
        "PASS the owner saw the thinking stream and keeps it; the guest saw the answer grow, was shown no thinking or tool step and holds no reasoning or tool_calls row"
    );

    let offered: Vec<(String, bool)> = view_of(&a)
        .await
        .expect("a's view")
        .tools
        .into_iter()
        .map(|tool| (tool.name, tool.on))
        .collect();
    assert!(
        offered.len() >= 2 && offered.iter().all(|(_, on)| *on),
        "{offered:?}"
    );
    let off = offered[0].0.clone();
    let kept = offered[1].0.clone();
    let tool_on_k = |want: bool| {
        let k = Arc::clone(&k);
        let off = off.clone();
        async move {
            let deadline = Instant::now() + Duration::from_secs(30);
            loop {
                let tools = view_of(&k).await.expect("k's view").tools;
                if tools.iter().any(|t| t.name == off && t.on == want) {
                    return;
                }
                assert!(
                    Instant::now() < deadline,
                    "the guest never showed {off} on = {want}: {tools:?}"
                );
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        }
    };
    let offered_for = |body: &'static str| {
        let record = record.clone();
        async move {
            let deadline = Instant::now() + Duration::from_secs(60);
            loop {
                let served: Value = reqwest::get(&record)
                    .await
                    .expect("the model's record")
                    .json()
                    .await
                    .expect("json");
                let asked = served.as_array().and_then(|all| {
                    all.iter()
                        .rev()
                        .find(|s| s["lastUser"] == json!(body))
                        .cloned()
                });
                if let Some(asked) = asked {
                    return asked["tools"]
                        .as_array()
                        .map(|tools| {
                            tools
                                .iter()
                                .filter_map(Value::as_str)
                                .map(str::to_owned)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_default();
                }
                assert!(
                    Instant::now() < deadline,
                    "the model was never asked {body:?}"
                );
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
        }
    };

    a.set_session_tool(&key, &off, false).expect("switches off");
    tool_on_k(false).await;
    a.send(&key, "tools: one off").expect("sends");
    let asked = offered_for("tools: one off").await;
    assert!(
        !asked.contains(&off) && asked.contains(&kept),
        "offered {asked:?} with {off} off"
    );
    println!("PASS {off} switched off on a reached the guest, and the next turn offered the model {asked:?}");

    a.set_session_tool(&key, &off, true).expect("switches on");
    tool_on_k(true).await;
    a.send(&key, "tools: all on").expect("sends");
    let asked = offered_for("tools: all on").await;
    assert!(
        asked.contains(&off) && asked.contains(&kept),
        "offered {asked:?} with {off} on"
    );
    println!("PASS {off} switched back on reached the guest, and the next turn offered it again: {asked:?}");
    let refused = a.set_session_tool(&key, "not_offered", false);
    assert!(
        refused.is_err(),
        "a tool the agent does not offer has no switch"
    );
    a.shutdown().await;
    k.shutdown().await;
}

/// An instance killed mid-answer: the viewer keeps what was committed and,
/// once the claim's lease lapses, says the answer stopped — no spinner
/// without an end. Needs `KEEPER_HQ_LIVE_KILL`: a URL that SIGKILLs the
/// agent's instance (and does not start it again). Run it last.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs a running agents server and an agent instance it may kill; see the module docs"]
async fn an_answer_whose_instance_was_killed_says_it_stopped() {
    let server = env("KEEPER_HQ_LIVE_URL");
    let operator = env("KEEPER_HQ_LIVE_OPERATOR_TOKEN");
    let actor = env("KEEPER_HQ_LIVE_ACTOR");
    let agent = env("KEEPER_HQ_LIVE_AGENT");
    let kill = env("KEEPER_HQ_LIVE_KILL");
    let databases = env("KEEPER_HQ_LIVE_DATABASES");
    let database = databases.split(',').next().expect("a database").to_owned();
    let scratch = tempfile::tempdir().expect("tempdir");
    let a = paired(
        &device(&scratch.path().join("a")),
        &server,
        &operator,
        &actor,
        databases.split(',').count(),
    )
    .await;
    let key = {
        let a = Arc::clone(&a);
        tokio::task::spawn_blocking(move || a.new_session(&database, &agent, "cut: a long one"))
            .await
            .expect("joins")
            .expect("a new session")
    };
    a.open_session(&key).expect("opens");
    let view_of = |hq: &Arc<Hq>| {
        let (hq, key) = (Arc::clone(hq), key.clone());
        async move {
            tokio::task::spawn_blocking(move || hq.session_view(&key))
                .await
                .expect("joins")
                .expect("a view")
        }
    };
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        let view = view_of(&a).await;
        if view.live.first().is_some_and(|turn| turn.text.len() >= 200) {
            break;
        }
        assert!(Instant::now() < deadline, "no answer to cut");
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    reqwest::get(&kill)
        .await
        .expect("kills")
        .error_for_status()
        .expect("killed");
    let killed = Instant::now();
    let at_kill = view_of(&a)
        .await
        .live
        .first()
        .map(|t| t.text.clone())
        .unwrap_or_default();
    let mut last = at_kill.clone();
    let stopped = loop {
        let view = view_of(&a).await;
        let turn = view
            .live
            .first()
            .cloned()
            .expect("the cut answer stays shown");
        assert!(
            turn.text.starts_with(&last),
            "{last:?} then {:?}",
            turn.text
        );
        assert!(!turn.text.is_empty(), "the committed part stays");
        last = turn.text.clone();
        if turn.stopped {
            break turn;
        }
        assert!(
            killed.elapsed() < keeper_core::hq::live::LEASE + Duration::from_secs(15),
            "the answer never said it stopped"
        );
        tokio::time::sleep(Duration::from_millis(100)).await;
    };
    println!(
        "PASS killed at {} bytes shown; {} ms later the answer said it stopped, keeping {} bytes",
        at_kill.len(),
        killed.elapsed().as_millis(),
        stopped.text.len()
    );
    a.shutdown().await;
}

/// Wait until `ready` answers, reading again whenever `hq` changes.
async fn until<T>(
    hq: &Arc<Hq>,
    what: &str,
    within: Duration,
    mut ready: impl FnMut() -> Option<T>,
) -> T {
    let deadline = Instant::now() + within;
    let mut changes = hq.changes();
    loop {
        if let Some(found) = ready() {
            return found;
        }
        assert!(Instant::now() < deadline, "{what}");
        tokio::select! {
            _ = changes.changed() => {}
            () = tokio::time::sleep(Duration::from_millis(20)) => {}
        }
    }
}

/// What became of `commit` once the server answered it.
async fn delivered(replica: &Arc<dyn Replica>, commit: &str) -> keeper_core::hq::port::Delivery {
    use keeper_core::hq::port::Delivery;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let delivery = replica.delivery(commit).expect("reads");
        if !matches!(delivery, Delivery::Pending) {
            return delivery;
        }
        assert!(Instant::now() < deadline, "{commit} was never answered");
        replica.sync_now();
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

async fn count(replica: &Arc<dyn Replica>, table: &str, session: &str) -> u64 {
    let rows = query(
        replica,
        &format!("SELECT count(*) AS n FROM {table} WHERE session_id = ?1"),
        vec![json!(session)],
    )
    .await;
    rows[0]["n"].as_u64().unwrap_or(0)
}

/// Many people in one session against the real server and an agent
/// instance in each database: the owner adds a person of the session's own
/// database and their device lists it after one sync with its newest page;
/// their message carries their name on the owner's; an author a client
/// claims for another is refused; the owner grants and withdraws the reader
/// flag and the thinking comes to and leaves that person's replica; the
/// owner removes them and the session leaves their device; a member leaving
/// is dropped from the invite; the directory of each database offers only
/// its own people, and the server refuses an invite across. Needs
/// `KEEPER_HQ_LIVE_DATABASES` naming two databases, `KEEPER_HQ_LIVE_AGENT`
/// serving the first and `KEEPER_HQ_LIVE_AGENT_2` the second,
/// `KEEPER_HQ_LIVE_ONE_DB_ACTOR` (a person of the first database alone) and
/// `KEEPER_HQ_LIVE_OTHER_DB_ACTOR` (a person of the second alone), and a
/// model that thinks before it answers a message starting `long`.
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs a running agents server and an agent instance in each database; see the module docs"]
async fn many_people_in_one_session_by_the_owner_and_the_server() {
    use keeper_core::hq::live::AgentViewPrefsPatch;
    use keeper_core::hq::port::Delivery;

    let server = env("KEEPER_HQ_LIVE_URL");
    let operator = env("KEEPER_HQ_LIVE_OPERATOR_TOKEN");
    let actor = env("KEEPER_HQ_LIVE_ACTOR");
    let databases = env("KEEPER_HQ_LIVE_DATABASES");
    let dbs: Vec<String> = databases.split(',').map(str::to_owned).collect();
    assert_eq!(dbs.len(), 2, "two databases, to prove they stay apart");
    let (first, second) = (dbs[0].clone(), dbs[1].clone());
    let agent = env("KEEPER_HQ_LIVE_AGENT");
    let agent_2 = env("KEEPER_HQ_LIVE_AGENT_2");
    let guest_of = env("KEEPER_HQ_LIVE_ONE_DB_ACTOR");
    let (guest, guest_db) = guest_of.split_once(':').expect("<actor>:<database>");
    assert_eq!(guest_db, first);
    let outsider_of = env("KEEPER_HQ_LIVE_OTHER_DB_ACTOR");
    let (outsider, outsider_db) = outsider_of.split_once(':').expect("<actor>:<database>");
    assert_eq!(outsider_db, second);
    let scratch = tempfile::tempdir().expect("tempdir");
    let a = paired(
        &device(&scratch.path().join("a")),
        &server,
        &operator,
        &actor,
        2,
    )
    .await;
    let k = paired(
        &device(&scratch.path().join("k")),
        &server,
        &operator,
        guest,
        1,
    )
    .await;
    k.set_view_prefs(AgentViewPrefsPatch {
        show_reasoning: Some(true),
        show_details: None,
    })
    .expect("prefs");
    let owner_client = a
        .status()
        .borrow()
        .device
        .as_ref()
        .expect("device")
        .client_id
        .clone();
    let guest_client = k
        .status()
        .borrow()
        .device
        .as_ref()
        .expect("device")
        .client_id
        .clone();
    let new_session = |db: String, agent: String, body: &'static str| {
        let a = Arc::clone(&a);
        async move {
            tokio::task::spawn_blocking(move || a.new_session(&db, &agent, body))
                .await
                .expect("joins")
                .expect("a new session")
        }
    };
    // Connected is not yet synced: wait until each directory names its agent.
    until(
        &a,
        "both directories name their agent",
        Duration::from_secs(30),
        || {
            let choices = a.choices().ok()?;
            let names = |agent: &str, db: &str| {
                choices
                    .iter()
                    .any(|c| c.agent_actor == agent && c.databases.iter().any(|d| d == db))
            };
            (names(&agent, &first) && names(&agent_2, &second)).then_some(())
        },
    )
    .await;
    let key = new_session(first.clone(), agent.clone(), "hello from the owner").await;
    let other = new_session(second.clone(), agent_2.clone(), "hello in the other").await;
    let ra = a.replica(&first).expect("a holds the first");
    let rk = k.replica(&first).expect("k holds the first");

    // 5: each directory offers only its own people; the server refuses an
    // invite across, and keeper never writes one.
    let sheet = until(&a, "a's sheet", Duration::from_secs(30), || {
        a.session_members(&key).ok()
    })
    .await;
    let offered: Vec<&str> = sheet.addable.iter().map(|p| p.actor_id.as_str()).collect();
    assert!(
        sheet.owner && offered.contains(&guest) && !offered.contains(&outsider),
        "{offered:?}"
    );
    let across = until(&a, "the other's sheet", Duration::from_secs(30), || {
        a.session_members(&other).ok()
    })
    .await;
    let offered_there: Vec<&str> = across.addable.iter().map(|p| p.actor_id.as_str()).collect();
    assert!(
        offered_there.contains(&outsider) && !offered_there.contains(&guest),
        "{offered_there:?}"
    );
    let refused = a
        .add_member(&key, outsider)
        .expect_err("not this database's");
    println!("PASS {first} offers {offered:?}, {second} offers {offered_there:?}; keeper refuses {outsider} in {first}: {refused}");
    let forged_invite = mutate(
        &ra,
        vec![RowOp::Patch {
            table: "sessions".to_owned(),
            values: object(json!({
                "id": key.session_id, "invite": json!([outsider]).to_string(),
                "author_actor": actor, "author_client": owner_client,
            })),
            base_version: None,
        }],
    )
    .await;
    match delivered(&ra, &forged_invite).await {
        Delivery::Refused(reason) => {
            assert_eq!(reason.code, "hq.v11.invite_outside_directory", "{reason}");
            println!("PASS the server refuses an invite naming {outsider} in {first}: {reason}");
        }
        other => panic!("an invite across databases was {other:?}"),
    }

    // 1: added, the guest's device lists the session after one sync and
    // shows its newest page; their message carries their name.
    let added = Instant::now();
    a.add_member(&key, guest).expect("adds");
    let listed = until(
        &k,
        "the guest lists the session",
        Duration::from_secs(30),
        || {
            k.sessions()
                .into_iter()
                .find(|row| row.database == key.database && row.session_id == key.session_id)
        },
    )
    .await;
    let listed_ms = added.elapsed().as_millis();
    k.open_session(&key).expect("k opens");
    let page = until(
        &k,
        "the guest's newest page",
        Duration::from_secs(30),
        || {
            k.session_view(&key).ok().filter(|v| {
                v.complete && v.messages.iter().any(|m| m.body == "hello from the owner")
            })
        },
    )
    .await;
    println!(
        "PASS the guest listed {:?} {listed_ms} ms after the add and showed its newest page ({} messages, the owner's by {:?})",
        listed.title,
        page.messages.len(),
        page.messages[0].author_name
    );
    let joined = until(
        &a,
        "the guest joins on a's sheet",
        Duration::from_secs(30),
        || {
            a.session_members(&key).ok().and_then(|s| {
                s.members
                    .into_iter()
                    .find(|m| m.actor_id == guest && !m.joining)
            })
        },
    )
    .await;
    assert!(
        !joined.reads_trace,
        "an added person does not read the trace"
    );
    k.send(&key, "hello from the guest").expect("k sends");
    let shown = until(
        &a,
        "the guest's message on a",
        Duration::from_secs(30),
        || {
            a.session_view(&key)
                .ok()?
                .messages
                .into_iter()
                .find(|m| m.body == "hello from the guest")
        },
    )
    .await;
    assert_eq!(shown.author_actor, guest);
    assert_eq!(shown.author_name, joined.name);
    println!(
        "PASS the guest's message shows on the owner's as {:?} ({})",
        shown.author_name, shown.author_actor
    );

    // 2: a row claiming another author is refused; nobody shows it.
    let now = chrono::Utc::now().timestamp_millis();
    let forged = mutate(
        &rk,
        vec![message(
            &Author {
                actor: actor.clone(),
                client: guest_client.clone(),
            },
            &key.session_id,
            &page_key(&key.session_id, now),
            &ulid::Ulid::new().to_string(),
            "forged as the owner",
            now,
        )],
    )
    .await;
    match delivered(&rk, &forged).await {
        Delivery::Refused(reason) => {
            assert_eq!(reason.code, "hq.v1.author_actor", "{reason}");
            println!("PASS a message claiming {actor} from {guest}'s device is refused: {reason}");
        }
        other => panic!("a forged author was {other:?}"),
    }
    // And a member who is not the owner changes nobody: refused here, and
    // an invite written anyway — naming one more person — is refused by the
    // server (rewriting the same invite changes nobody, so it is not).
    assert!(k.add_member(&key, &actor).is_err());
    let invite_by_guest = mutate(
        &rk,
        vec![RowOp::Patch {
            table: "sessions".to_owned(),
            values: object(json!({
                "id": key.session_id, "invite": json!([guest, actor]).to_string(),
                "author_actor": guest, "author_client": guest_client,
            })),
            base_version: None,
        }],
    )
    .await;
    match delivered(&rk, &invite_by_guest).await {
        Delivery::Refused(reason) => {
            assert_eq!(reason.code, "hq.v3.owner_only", "{reason}");
            println!(
                "PASS an invite written by a member who is not the owner is refused: {reason}"
            );
        }
        other => panic!("a member's invite change was {other:?}"),
    }

    // 3: the reader flag. Not a reader: no thinking reaches the guest.
    a.send(&key, "long: think about the plan").expect("a asks");
    // The owner reads the trace (and shows no thinking, so its window holds
    // none): the final answer ends the turn; the guest is never sent it.
    let answers = |hq: &Arc<Hq>| {
        hq.session_view(&key).map_or(0, |v| {
            v.messages
                .iter()
                .filter(|m| m.role == keeper_core::hq::timeline::AgentMessageRoleVm::Agent)
                .count()
        })
    };
    let before = answers(&a);
    until(
        &a,
        "the thinking answer ends",
        Duration::from_secs(120),
        || {
            let view = a.session_view(&key).ok()?;
            (view.live.is_empty() && answers(&a) > before).then_some(())
        },
    )
    .await;
    assert_eq!(count(&rk, "reasoning", &key.session_id).await, 0);
    assert_eq!(count(&rk, "tool_calls", &key.session_id).await, 0);
    let granting = {
        let (a, key, guest) = (Arc::clone(&a), key.clone(), guest.to_owned());
        tokio::task::spawn_blocking(move || a.set_member_reader(&key, &guest, true))
    };
    let granted_at = Instant::now();
    granting.await.expect("joins").expect("grants");
    until(
        &k,
        "the guest reads the trace",
        Duration::from_secs(30),
        || k.session_view(&key).ok().filter(|v| v.reads_trace),
    )
    .await;
    let deadline = Instant::now() + Duration::from_secs(30);
    let mut held = 0;
    while held == 0 {
        assert!(
            Instant::now() < deadline,
            "the thinking never reached the guest"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
        held = count(&rk, "reasoning", &key.session_id).await;
    }
    println!(
        "PASS granted: the guest reads the trace and holds {held} reasoning rows {} ms after the grant",
        granted_at.elapsed().as_millis()
    );
    // R1064K-01: with `KEEPER_HQ_LIVE_APPROVAL` naming a message the first
    // database's agent answers by asking for an approval, the guest — a
    // reader now — is shown that approval and holds its tool call before
    // the flag is withdrawn, so the withdrawal has one to take away.
    let approval = std::env::var("KEEPER_HQ_LIVE_APPROVAL").ok();
    if let Some(asking) = &approval {
        a.send(&key, asking).expect("a asks for a run");
        let shown = until(
            &k,
            "the guest is shown the agent's approval",
            Duration::from_secs(60),
            || {
                let view = k.session_view(&key).ok()?;
                (!view.approvals.is_empty()).then_some(view.approvals.len())
            },
        )
        .await;
        let calls = count(&rk, "tool_calls", &key.session_id).await;
        assert!(
            calls > 0,
            "the approval's tool call is on the guest's device"
        );
        println!(
            "PASS granted: the guest is shown {shown} approval(s) of the session and holds {calls} tool call(s)"
        );
    }
    let withdrawing = {
        let (a, key, guest) = (Arc::clone(&a), key.clone(), guest.to_owned());
        tokio::task::spawn_blocking(move || a.set_member_reader(&key, &guest, false))
    };
    withdrawing.await.expect("joins").expect("withdraws");
    // The moment the guest's device knows the flag is withdrawn, its view
    // shows no approval — whether or not the window has evicted the rows yet.
    let (approvals_held, approvals_shown) = until(
        &k,
        "the guest no longer reads the trace",
        Duration::from_secs(30),
        || {
            k.session_view(&key)
                .ok()
                .filter(|v| !v.reads_trace)
                .map(|v| {
                    let held = rk
                        .query(
                            "SELECT count(*) AS n FROM approvals WHERE session_id = ?1",
                            &[json!(key.session_id)],
                        )
                        .ok()
                        .and_then(|rows| rows.first().and_then(|r| r["n"].as_u64()))
                        .unwrap_or(0);
                    (held, v.approvals.len())
                })
        },
    )
    .await;
    assert_eq!(approvals_shown, 0, "a non-reader is shown no approval");
    assert!(
        k.approvals_waiting()
            .iter()
            .all(|waiting| waiting.session_id != key.session_id),
        "nothing of the session waits for the guest"
    );
    if approval.is_some() {
        println!(
            "PASS withdrawn: the guest's view showed no approval the moment it knew, with {approvals_held} approval row(s) still on the device"
        );
    }
    let deadline = Instant::now() + Duration::from_secs(30);
    while count(&rk, "reasoning", &key.session_id).await != 0 {
        assert!(
            Instant::now() < deadline,
            "the thinking stayed on the guest's device"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert_eq!(count(&rk, "tool_calls", &key.session_id).await, 0);
    assert_eq!(count(&rk, "approvals", &key.session_id).await, 0);
    let view = k.session_view(&key).expect("k's view");
    assert!(view.messages.iter().all(|m| m.reasoning.is_none()));
    println!("PASS withdrawn: the guest's replica holds no reasoning, tool call or approval of the session, and still the answers ({} messages)", view.messages.len());

    // 4: the owner removes the guest: the session leaves their device.
    let removing = {
        let (a, key, guest) = (Arc::clone(&a), key.clone(), guest.to_owned());
        tokio::task::spawn_blocking(move || a.remove_member(&key, &guest))
    };
    let removed_at = Instant::now();
    removing.await.expect("joins").expect("removes");
    until(
        &k,
        "the session leaves the guest's list",
        Duration::from_secs(30),
        || {
            (!k.sessions()
                .iter()
                .any(|r| r.database == key.database && r.session_id == key.session_id))
            .then_some(())
        },
    )
    .await;
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let left = [
            count(&rk, "messages", &key.session_id).await,
            count(&rk, "session_members", &key.session_id).await,
            count(&rk, "turn_chunks", &key.session_id).await,
        ];
        let row = query(
            &rk,
            "SELECT count(*) AS n FROM sessions WHERE id = ?1",
            vec![json!(key.session_id)],
        )
        .await[0]["n"]
            .as_u64()
            .unwrap_or(0);
        if left == [0, 0, 0] && row == 0 {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "the guest still holds {left:?} and the session row {row}"
        );
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let after = until(
        &a,
        "the guest leaves a's sheet",
        Duration::from_secs(30),
        || {
            a.session_members(&key)
                .ok()
                .filter(|s| !s.members.iter().any(|m| m.actor_id == guest))
        },
    )
    .await;
    assert!(
        after.addable.iter().any(|p| p.actor_id == guest),
        "removed, they can be added again"
    );
    println!("PASS removed by the owner: the session left the guest's list and replica ({} ms) and the owner's sheet", removed_at.elapsed().as_millis());
    k.close_session(&key);

    // A member leaving: added again, they leave; the server drops them from
    // the invite, so the next change does not bring them back.
    a.add_member(&key, guest).expect("adds again");
    until(
        &k,
        "the guest lists it again",
        Duration::from_secs(30),
        || k.session_members(&key).ok().filter(|s| s.can_leave),
    )
    .await;
    k.leave_session(&key).expect("leaves");
    until(
        &a,
        "the invite drops the guest",
        Duration::from_secs(30),
        || {
            let rows = ra
                .query(
                    "SELECT invite FROM sessions WHERE id = ?1",
                    &[json!(key.session_id)],
                )
                .ok()?;
            let held = rows.first()?.get("invite")?;
            let invite = held
                .as_str()
                .map_or_else(|| held.to_string(), str::to_owned);
            (!invite.contains(guest)
                && !a
                    .session_members(&key)
                    .ok()?
                    .members
                    .iter()
                    .any(|m| m.actor_id == guest))
            .then_some(())
        },
    )
    .await;
    assert!(!k
        .sessions()
        .iter()
        .any(|r| r.session_id == key.session_id && r.database == key.database));
    println!(
        "PASS the guest left: gone from their list, from the owner's sheet and from the invite"
    );
    a.shutdown().await;
    k.shutdown().await;
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
