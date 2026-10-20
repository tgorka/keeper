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
//! cargo test --manifest-path src-tauri/Cargo.toml -p keeper-core --test hq_live -- --ignored --nocapture
//! ```
//!
//! The operator's token is used only for the person's first pairing code, as
//! an operator pairs a person's first device. Each device runs behind a local
//! TCP relay, so the test can cut one device off the server and see every
//! byte the other sends.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use keeper_core::error::CoreError;
use keeper_core::hq::port::{Replica, RowOp};
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
    let (replica, id, table) = (Arc::clone(replica), id.to_owned(), table.to_owned());
    let scope = vec![(variable.to_owned(), vec![value.to_owned()])];
    tokio::task::spawn_blocking(move || replica.subscribe(&id, &table, scope).expect("subscribes"))
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

/// A session of `author`'s with one message, in one commit.
fn session_with_message(
    author: &Author,
    number: u32,
    agent: &str,
    body: &str,
) -> (String, String, Vec<RowOp>) {
    let now = chrono::Utc::now().timestamp_millis();
    let id = format!("{}.{number}", author.client);
    let page = page_key(&id, now);
    let by = json!({ "author_actor": author.actor, "author_client": author.client });
    let with = |mut values: Value| {
        for (k, v) in by.as_object().expect("object") {
            values[k] = v.clone();
        }
        values
    };
    let ops = vec![
        upsert(
            "sessions",
            with(json!({
                "id": id, "title": "hq_live", "status": "open", "agent_actor": agent,
                "created_by_actor": author.actor, "created_by_client": author.client,
                "invite": "[]", "archived": false, "created_at_ms": now, "last_activity_ms": now,
            })),
        ),
        upsert(
            "session_members",
            with(json!({
                "id": format!("{id}/{}", author.actor), "session_id": id, "actor_id": author.actor,
                "role": "owner", "reads_trace": true, "added_by_actor": author.actor, "added_at_ms": now,
            })),
        ),
        message(author, &id, &page, &format!("m-{number}-0"), body, now),
    ];
    (id, page, ops)
}

fn message(author: &Author, session: &str, page: &str, id: &str, body: &str, at_ms: i64) -> RowOp {
    upsert(
        "messages",
        json!({
            "id": id, "session_id": session, "page_key": page, "role": "person", "body": body,
            "created_at_ms": at_ms, "author_actor": author.actor, "author_client": author.client,
        }),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "needs a running agents server; see the module docs"]
async fn pair_discover_replicate_go_offline_and_revoke() {
    let server = env("KEEPER_HQ_LIVE_URL");
    let operator = env("KEEPER_HQ_LIVE_OPERATOR_TOKEN");
    let actor = env("KEEPER_HQ_LIVE_ACTOR");
    let expected: Vec<String> = env("KEEPER_HQ_LIVE_DATABASES")
        .split(',')
        .map(str::to_owned)
        .collect();
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
    assert_eq!(
        relay_a.connections.load(Ordering::SeqCst),
        0,
        "no server, no request"
    );
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

    // The token is in the keychain and nowhere on disk.
    let stored = platform_a
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
    for entry in walk(&platform_a.data_dir) {
        let bytes = std::fs::read(&entry).expect("reads");
        assert!(
            !bytes.windows(8).any(|w| w == needle.as_bytes()),
            "the token's first eight characters are in {}",
            entry.display()
        );
    }
    println!("PASS the token is in the keychain only");

    // Device B: types the code A shows, straight at its own relay.
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

    // A commit on A in each database reaches B with the server-stamped author.
    let author = Author {
        actor: actor.clone(),
        client: a_client.clone(),
    };
    let mut pages = Vec::new();
    for (n, db) in expected.iter().enumerate() {
        let ra = a.replica(db).await.expect("a holds it");
        let rb = b.replica(db).await.expect("b holds it");
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
            session_with_message(&author, (n + 1) as u32, &agent, &format!("hello {db}"));
        mutate(&ra, ops).await;
        subscribe(&rb, &format!("page-{db}"), "messages", "page", &page).await;
        rb.sync_now();
        let rows = wait_rows(
            &rb,
            "SELECT id, body, author_actor, author_client FROM messages WHERE session_id = ?1",
            vec![json!(session)],
            1,
            within,
        )
        .await;
        assert_eq!(rows[0]["body"], format!("hello {db}"));
        assert_eq!(rows[0]["author_actor"], actor.as_str());
        assert_eq!(rows[0]["author_client"], a_client.as_str());
        pages.push((db.clone(), session, page, ra, rb));
        println!("PASS {db}: a's message is in b's replica, authored {actor}/{a_client}");
    }

    // Both devices are listed under Devices on both.
    for (label, device) in [("a", &a), ("b", &b)] {
        let deadline = Instant::now() + within;
        loop {
            let listed = device.devices().await.expect("lists");
            let ids: Vec<&str> = listed.iter().map(|d| d.client_id.as_str()).collect();
            if ids.contains(&a_client.as_str()) && ids.contains(&b_client.as_str()) {
                break;
            }
            assert!(Instant::now() < deadline, "{label} lists {ids:?}");
            tokio::time::sleep(Duration::from_millis(200)).await;
        }
    }
    println!("PASS both devices are listed on both");

    // A goes offline: writes wait in its outbox, reads answer, B sees nothing.
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

    // A restart keeps the replicas and the pairing; no new code.
    drop(pages);
    drop(a);
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
    println!("PASS restart: paired and connected with no new code");

    // A removes B: B is refused on every database, says so and stops sending.
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
        let rb = b.replica(db).await.expect("still held");
        assert!(
            !query(&rb, "SELECT id FROM messages", vec![])
                .await
                .is_empty(),
            "{db} readable"
        );
    }
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
    println!("PASS b removed: refused, readable, and silent for 15 s");

    // A person in one database never asks for the other.
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
        println!("PASS {one_actor} holds {one_db} alone and never requested another");
    }
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
