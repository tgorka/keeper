//! 110.2 disposable contract proof. Real persistent native replicas; no query engine.
//! Run the ignored case against the matching exported contract with
//! KEEPER_HQ_LIVE_URL, KEEPER_HQ_LIVE_DATABASE, KEEPER_HQ_LIVE_CLIENT_1/2,
//! KEEPER_HQ_LIVE_TOKEN_1/2. Credentials are transient environment inputs.
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use keeper_core::hq::port::{Link, Replica, ReplicaSpec, Subscription};
use keeper_core::hq::syncular::SyncularReplica;
use rusqlite::{params, Connection};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};

type Row = Map<String, Value>;

fn text<'a>(row: &'a Row, key: &str) -> Result<&'a str, String> {
    row.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing {key}"))
}

fn number(row: &Row, key: &str) -> Result<u64, String> {
    text(row, key)?
        .parse()
        .map_err(|_| format!("invalid {key}"))
}

/// The event delta column retains the server's canonical JSON text. Hash that
/// exact ordered array; never reserialize its numbers with a different formatter.
fn delta_digest(deltas: &[String]) -> String {
    hex::encode(Sha256::digest(format!("[{}]", deltas.join(",")).as_bytes()))
}

fn prefix(
    head: u64,
    headers: &[Row],
    operations: &[Row],
) -> Result<(u64, BTreeMap<String, Value>), String> {
    let mut state = BTreeMap::new();
    let mut at = 0;
    let mut ordered = BTreeMap::new();
    for header in headers {
        let sequence = number(header, "graph_seq")?;
        if ordered.insert(sequence, header).is_some() {
            return Err("duplicate event position".into());
        }
    }
    for sequence in 1..=head {
        let Some(header) = ordered.get(&sequence) else {
            break;
        };
        let id = text(header, "id")?;
        let count = header
            .get("op_count")
            .and_then(Value::as_u64)
            .ok_or("invalid count")?;
        let mut group = BTreeMap::new();
        for row in operations
            .iter()
            .filter(|row| row.get("event_id").and_then(Value::as_str) == Some(id))
        {
            let ordinal = row
                .get("ordinal")
                .and_then(Value::as_u64)
                .ok_or("invalid ordinal")?;
            if group
                .insert(ordinal, text(row, "delta")?.to_owned())
                .is_some()
            {
                return Err("duplicate ordinal".into());
            }
        }
        if group.len() as u64 != count || group.keys().copied().ne(0..count) {
            break;
        }
        let deltas: Vec<_> = group.into_values().collect();
        if delta_digest(&deltas) != text(header, "delta_digest")? {
            return Err("delta digest mismatch".into());
        }
        for (ordinal, encoded) in deltas.iter().enumerate() {
            let delta: Value = serde_json::from_str(encoded).map_err(|e| e.to_string())?;
            if delta["ordinal"].as_u64() != Some(ordinal as u64) {
                return Err("delta ordinal mismatch".into());
            }
            let kind = delta["entity_type"].as_str().ok_or("missing entity type")?;
            let entity = delta["entity_id"].as_str().ok_or("missing entity id")?;
            let key = format!("{kind}:{entity}");
            let before = delta.get("before").ok_or("missing before")?;
            if state.get(&key).unwrap_or(&Value::Null) != before {
                return Err("before image mismatch".into());
            }
            let after = delta.get("after").ok_or("missing after")?;
            if after.is_null() {
                state.remove(&key);
            } else {
                state.insert(key, after.clone());
            }
        }
        at = sequence;
    }
    Ok((at, state))
}

/// State and manifest share one checked SQLite transaction. Dropping the
/// transaction after writes is the crash-before-publication control.
fn publish(db: &mut Connection, at: u64, state: &BTreeMap<String, Value>, crash: bool) {
    db.execute_batch("CREATE TABLE IF NOT EXISTS entities(id TEXT PRIMARY KEY, value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS manifest(id INTEGER PRIMARY KEY, graph_seq INTEGER NOT NULL)").expect("projection schema");
    let tx = db.transaction().expect("publication transaction");
    tx.execute("DELETE FROM entities", [])
        .expect("replace generation");
    for (id, value) in state {
        tx.execute(
            "INSERT INTO entities VALUES (?1, ?2)",
            params![id, value.to_string()],
        )
        .expect("after image");
    }
    if crash {
        return;
    }
    tx.execute("INSERT INTO manifest VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET graph_seq=excluded.graph_seq", [at]).expect("manifest");
    tx.commit().expect("publish");
}

fn fixture() -> (Vec<Row>, Vec<Row>) {
    let first = r#"{"after":{"id":"a","removed":"old","revision":"1"},"before":null,"entity_id":"a","entity_type":"node","ordinal":0}"#.to_owned();
    let second = r#"{"after":{"id":"a","revision":"2"},"before":{"id":"a","removed":"old","revision":"1"},"entity_id":"a","entity_type":"node","ordinal":0}"#.to_owned();
    let mut headers = Vec::new();
    let mut operations = Vec::new();
    for (index, delta) in [first, second].into_iter().enumerate() {
        let id = format!("event-{index}");
        headers.push(json!({"id":id, "graph_seq":(index+1).to_string(), "op_count":1, "delta_digest":delta_digest(std::slice::from_ref(&delta))}).as_object().expect("header").clone());
        operations.push(
            json!({"event_id":id,"ordinal":0,"delta":delta})
                .as_object()
                .expect("operation")
                .clone(),
        );
    }
    (headers, operations)
}

#[test]
fn missing_corrupt_duplicate_and_reordered_groups_never_publish_partial_state() {
    let (headers, operations) = fixture();
    let (at, state) = prefix(2, &headers, &operations).expect("complete");
    assert_eq!(at, 2);
    assert_eq!(state["node:a"], json!({"id":"a","revision":"2"}));
    let (at, old) = prefix(2, &headers, &operations[..1]).expect("stale complete prefix");
    assert_eq!(at, 1);
    assert_eq!(old["node:a"]["removed"], "old");
    assert_eq!(
        prefix(2, &headers[1..], &operations)
            .expect("missing genesis")
            .0,
        0
    );
    let mut reversed = operations.clone();
    reversed.reverse();
    assert_eq!(
        prefix(2, &headers, &reversed)
            .expect("ordered by ordinal")
            .1,
        state
    );
    let mut duplicate = operations.clone();
    duplicate.push(operations[0].clone());
    assert!(prefix(2, &headers, &duplicate).is_err());
    let mut corrupt = operations;
    corrupt[1].insert("delta".into(), Value::String("{}".into()));
    assert!(prefix(2, &headers, &corrupt).is_err());
}

#[test]
fn restart_between_replay_and_manifest_retains_previous_generation() {
    let directory = tempfile::tempdir().expect("scratch");
    let path = directory.path().join("projection.sqlite");
    let (headers, operations) = fixture();
    let (one, first) = prefix(1, &headers, &operations).expect("one");
    let (two, second) = prefix(2, &headers, &operations).expect("two");
    let mut db = Connection::open(&path).expect("open");
    publish(&mut db, one, &first, false);
    publish(&mut db, two, &second, true);
    drop(db);
    let db = Connection::open(path).expect("restart");
    assert_eq!(
        db.query_row("SELECT graph_seq FROM manifest", [], |r| r.get::<_, u64>(0))
            .expect("head"),
        1
    );
    let value: String = db
        .query_row("SELECT value FROM entities WHERE id='node:a'", [], |r| {
            r.get(0)
        })
        .expect("state");
    assert_eq!(
        serde_json::from_str::<Value>(&value).expect("json"),
        first["node:a"]
    );
}

fn await_rows(replica: &dyn Replica, table: &str, count: usize) -> Vec<Row> {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let rows = replica
            .query(&format!("SELECT * FROM {table}"), &[])
            .expect("replica query");
        if rows.len() >= count {
            return rows;
        }
        assert!(Instant::now() < deadline, "{table} did not catch up");
        replica.sync_now();
        std::thread::sleep(Duration::from_millis(30));
    }
}

fn await_receipt(replica: &dyn Replica, request: &str) -> Row {
    let deadline = Instant::now() + Duration::from_secs(30);
    loop {
        let rows = replica
            .query(
                "SELECT * FROM kg_receipts WHERE request_id = ?",
                &[request.into()],
            )
            .expect("own receipt");
        if let Some(row) = rows.into_iter().next() {
            return row;
        }
        assert!(Instant::now() < deadline, "receipt not visible");
        replica.sync_now();
        std::thread::sleep(Duration::from_millis(30));
    }
}

#[test]
#[ignore = "requires the disposable 110.2 server and two issued native credentials"]
fn two_native_replicas_observe_command_receipts_and_complete_prefixes() {
    let url = std::env::var("KEEPER_HQ_LIVE_URL").expect("server URL");
    let domain = std::env::var("KEEPER_HQ_LIVE_DATABASE").expect("domain");
    let actor = std::env::var("KEEPER_HQ_LIVE_ACTOR").expect("actor");
    let scratch = tempfile::tempdir().expect("persistent stores");
    let mut replicas = Vec::new();
    for index in 1..=2 {
        let client = std::env::var(format!("KEEPER_HQ_LIVE_CLIENT_{index}")).expect("client");
        let token = std::env::var(format!("KEEPER_HQ_LIVE_TOKEN_{index}")).expect("token");
        let client_scope = format!("{}:{actor}{}:{client}", actor.len(), client.len());
        let spec = ReplicaSpec {
            database: domain.clone(),
            path: scratch.path().join(format!("{index}.sqlite")),
            client_id: Some(client),
            link: Some(Link {
                base_url: format!("{url}/{domain}"),
                token,
            }),
            subscriptions: [
                "kg_head",
                "kg_events",
                "kg_event_ops",
                "kg_nodes",
                "kg_edges",
            ]
            .into_iter()
            .map(|table| Subscription {
                id: table.into(),
                table: table.into(),
                scopes: vec![("kg_graph".into(), vec![domain.clone()])],
            })
            .chain(std::iter::once(Subscription {
                id: "kg_receipts".into(),
                table: "kg_receipts".into(),
                scopes: vec![("kg_client".into(), vec![client_scope])],
            }))
            .collect(),
            watch_presence: None,
        };
        replicas.push(SyncularReplica::open(spec).expect("open real replica"));
    }
    let request = ulid::Ulid::new().to_string();
    let params = json!({"version":1,"context_id":null,"payload":{"id":request,"expected":"0"}});
    let first = replicas[0]
        .remote_command("kg.proof.apply", &request, params.clone())
        .expect("command");
    assert!(first.commit_seq.is_some_and(|seq| seq > 0));
    let replay = replicas[0]
        .remote_command("kg.proof.apply", &request, params.clone())
        .expect("replay");
    assert_eq!(first.commit_seq, replay.commit_seq);
    assert!(replay.cached);
    let receipt = await_receipt(&replicas[0], &request);
    assert_eq!(receipt["outcome"], "accepted");
    let detail: Value = serde_json::from_str(text(&receipt, "detail").expect("receipt detail"))
        .expect("receipt JSON");
    assert_eq!(detail["event_id"], request);
    let accepted_sequence: u64 = detail["graph_seq"]
        .as_str()
        .expect("accepted sequence")
        .parse()
        .expect("sequence integer");
    let conflict_id = ulid::Ulid::new().to_string();
    let transported = replicas[0]
        .remote_command("kg.proof.apply", &conflict_id, params)
        .expect("conflict transported");
    assert!(transported.commit_seq > first.commit_seq);
    assert_eq!(
        await_receipt(&replicas[0], &conflict_id)["outcome"],
        "conflict"
    );
    let mut states = Vec::new();
    for replica in &replicas {
        let deadline = Instant::now() + Duration::from_secs(30);
        let at = loop {
            let heads = await_rows(replica, "kg_head", 1);
            let at = number(&heads[0], "graph_seq").expect("head sequence");
            if at >= accepted_sequence {
                break at;
            }
            assert!(Instant::now() < deadline, "accepted head did not catch up");
            replica.sync_now();
            std::thread::sleep(Duration::from_millis(30));
        };
        let headers = await_rows(replica, "kg_events", at as usize);
        let operations = await_rows(
            replica,
            "kg_event_ops",
            headers
                .iter()
                .map(|h| h["op_count"].as_u64().expect("count") as usize)
                .sum(),
        );
        let (through, state) = prefix(at, &headers, &operations).expect("prefix");
        assert_eq!(through, at);
        assert!(state.contains_key(&format!("node:{request}")));
        states.push(state);
    }
    assert_eq!(states[0], states[1]);
    assert!(replicas[1]
        .query(
            "SELECT * FROM kg_receipts WHERE request_id = ?",
            &[request.into()]
        )
        .expect("other device receipt scope")
        .is_empty());
    for replica in replicas {
        replica.close();
        replica.join();
    }
}
