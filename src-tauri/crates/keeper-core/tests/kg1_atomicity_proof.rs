//! 110.2 disposable contract proof. Real persistent native replicas; no query engine.
//! Run the ignored case against the matching exported contract with
//! KEEPER_HQ_LIVE_URL, KEEPER_HQ_LIVE_DATABASE, KEEPER_HQ_LIVE_CLIENT_1/2,
//! KEEPER_HQ_LIVE_TOKEN_1/2. Credentials are transient environment inputs.
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use keeper_core::hq::port::{
    CommandResult, Link, Replica, ReplicaError, ReplicaSpec, Subscription,
};
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

/// Independently canonicalize native values with the audited RFC 8785
/// serializer, rather than trusting JSON key order or Rust number formatting.
fn delta_digest(deltas: &[String]) -> Result<String, String> {
    let values: Vec<Value> = deltas
        .iter()
        .map(|encoded| serde_json::from_str(encoded))
        .collect::<Result<_, _>>()
        .map_err(|error| error.to_string())?;
    let bytes = serde_jcs::to_vec(&values).map_err(|error| error.to_string())?;
    Ok(hex::encode(Sha256::digest(bytes)))
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
        if delta_digest(&deltas)? != text(header, "delta_digest")? {
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
        headers.push(json!({"id":id, "graph_seq":(index+1).to_string(), "op_count":1, "delta_digest":delta_digest(std::slice::from_ref(&delta)).expect("fixture JCS")}).as_object().expect("header").clone());
        operations.push(
            json!({"event_id":id,"ordinal":0,"delta":delta})
                .as_object()
                .expect("operation")
                .clone(),
        );
    }
    (headers, operations)
}

/// The shared corpus is consumed unchanged by the HQ server suite. Native
/// parsing plus serde_jcs must produce each independently stated JCS text.
#[test]
fn shared_jcs_vectors_match_native_canonical_bytes() {
    let corpus: Value = serde_json::from_str(include_str!(
        "../../../../syncular/knowledge-graph.vectors.json"
    ))
    .expect("vector corpus");
    let vectors = corpus["vectors"].as_array().expect("vectors");
    assert!(vectors.len() >= 8);
    for vector in vectors {
        let id = vector["id"].as_str().expect("id");
        let parsed: Value = serde_json::from_str(vector["json"].as_str().expect("json")).expect(id);
        let bytes = serde_jcs::to_vec(&parsed).expect(id);
        let text = String::from_utf8(bytes.clone()).expect(id);
        assert_eq!(
            text,
            vector["canonical"].as_str().expect("canonical"),
            "{id}"
        );
        assert_eq!(
            hex::encode(Sha256::digest(&bytes)),
            vector["sha256"].as_str().expect("sha"),
            "{id}"
        );
    }
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

/// What a native graph caller may conclude from one command attempt and the
/// scoped receipt its replica currently holds. Transport success alone never
/// means accepted; only `idempotency_mismatch` and graph backstop codes are
/// terminal refusals. Everything else keeps the same request pending.
#[derive(Debug, Clone, PartialEq, Eq)]
enum GraphOutcome {
    Accepted {
        graph_seq: String,
        commit_seq: Option<i64>,
    },
    Conflict,
    Rejected(String),
    PendingVisibility,
    Pending(String),
    TechnicalRefusal(String),
    Mismatch,
}

fn classify(attempt: &Result<CommandResult, ReplicaError>, receipt: Option<&Row>) -> GraphOutcome {
    match attempt {
        Ok(result) => match receipt {
            None => GraphOutcome::PendingVisibility,
            Some(row) => {
                let detail: Value = row
                    .get("detail")
                    .and_then(Value::as_str)
                    .and_then(|text| serde_json::from_str(text).ok())
                    .unwrap_or(Value::Null);
                match row.get("outcome").and_then(Value::as_str) {
                    Some("accepted") => GraphOutcome::Accepted {
                        graph_seq: detail["graph_seq"].as_str().unwrap_or_default().to_owned(),
                        commit_seq: result.commit_seq,
                    },
                    Some("conflict") => GraphOutcome::Conflict,
                    Some("rejected") => GraphOutcome::Rejected(
                        detail["code"].as_str().unwrap_or("rejected").to_owned(),
                    ),
                    _ => GraphOutcome::Pending("unknown receipt".to_owned()),
                }
            }
        },
        Err(ReplicaError::Remote { message, .. }) if message == "idempotency_mismatch" => {
            GraphOutcome::Mismatch
        }
        Err(ReplicaError::Remote { code, message, .. })
            if code.starts_with("kg.") || message.starts_with("kg.") =>
        {
            GraphOutcome::TechnicalRefusal(if code.starts_with("kg.") {
                code.clone()
            } else {
                message.clone()
            })
        }
        Err(error) => GraphOutcome::Pending(error.to_string()),
    }
}

fn receipt_now(replica: &dyn Replica, request: &str) -> Option<Row> {
    replica
        .query(
            "SELECT * FROM kg_receipts WHERE request_id = ?",
            &[request.into()],
        )
        .ok()
        .and_then(|rows| rows.into_iter().next())
}

#[test]
fn outcome_table_distinguishes_transport_from_graph_acceptance() {
    let applied = Ok(CommandResult {
        request_id: "r".into(),
        commit_seq: Some(9),
        cached: false,
    });
    let receipt = |outcome: &str, detail: Value| {
        json!({"outcome":outcome,"detail":detail.to_string()})
            .as_object()
            .expect("row")
            .clone()
    };
    assert_eq!(
        classify(
            &applied,
            Some(&receipt("accepted", json!({"graph_seq":"4"})))
        ),
        GraphOutcome::Accepted {
            graph_seq: "4".into(),
            commit_seq: Some(9)
        }
    );
    assert_eq!(
        classify(&applied, Some(&receipt("conflict", json!({})))),
        GraphOutcome::Conflict
    );
    assert_eq!(
        classify(
            &applied,
            Some(&receipt("rejected", json!({"code":"approval_used"})))
        ),
        GraphOutcome::Rejected("approval_used".into())
    );
    assert_eq!(classify(&applied, None), GraphOutcome::PendingVisibility);
    let remote = |code: &str, message: &str| -> Result<CommandResult, ReplicaError> {
        Err(ReplicaError::Remote {
            code: code.into(),
            message: message.into(),
            retryable: false,
        })
    };
    assert_eq!(
        classify(&remote("operation.forbidden", "idempotency_mismatch"), None),
        GraphOutcome::Mismatch
    );
    assert_eq!(
        classify(&remote("kg.event_group", "kg.event_group"), None),
        GraphOutcome::TechnicalRefusal("kg.event_group".into())
    );
    // Disclosure/barrier/unresolved refusals never close the request.
    for message in [
        "graph_unavailable",
        "context_revoked",
        "outcome_unresolved",
        "technical_unresolved",
    ] {
        assert!(matches!(
            classify(&remote("operation.forbidden", message), None),
            GraphOutcome::Pending(_)
        ));
    }
    assert!(matches!(
        classify(
            &Err(ReplicaError::Unheard {
                code: "x".into(),
                message: "y".into(),
                retryable: true
            }),
            None
        ),
        GraphOutcome::Pending(_)
    ));
}

struct Live {
    url: String,
    domain: String,
    actor: String,
}

impl Live {
    fn from_env() -> Live {
        Live {
            url: std::env::var("KEEPER_HQ_LIVE_URL").expect("server URL"),
            domain: std::env::var("KEEPER_HQ_LIVE_DATABASE").expect("domain"),
            actor: std::env::var("KEEPER_HQ_LIVE_ACTOR").expect("actor"),
        }
    }
    fn open(
        &self,
        index: u8,
        base: &str,
        path: &std::path::Path,
        tables: &[&str],
    ) -> SyncularReplica {
        let client = std::env::var(format!("KEEPER_HQ_LIVE_CLIENT_{index}")).expect("client");
        let token = std::env::var(format!("KEEPER_HQ_LIVE_TOKEN_{index}")).expect("token");
        let scope = format!(
            "{}:{}{}:{client}",
            self.actor.len(),
            self.actor,
            client.len()
        );
        let mut subscriptions: Vec<Subscription> = tables
            .iter()
            .map(|table| Subscription {
                id: (*table).into(),
                table: (*table).into(),
                scopes: vec![("kg_graph".into(), vec![self.domain.clone()])],
            })
            .collect();
        subscriptions.push(Subscription {
            id: "kg_receipts".into(),
            table: "kg_receipts".into(),
            scopes: vec![("kg_client".into(), vec![scope])],
        });
        SyncularReplica::open(ReplicaSpec {
            database: self.domain.clone(),
            path: path.to_path_buf(),
            client_id: Some(client),
            link: Some(Link {
                base_url: format!("{base}/{}", self.domain),
                token,
            }),
            subscriptions,
            watch_presence: None,
        })
        .expect("open real replica")
    }
}

#[test]
#[ignore = "requires the disposable 110.2 server, a dropping relay and issued native credentials"]
fn native_frames_resolve_through_the_outcome_table() {
    let live = Live::from_env();
    let relay = std::env::var("KEEPER_HQ_LIVE_DROP_URL").expect("dropping relay");
    let technical = std::env::var("KEEPER_HQ_LIVE_TECHNICAL_ID").expect("technical payload id");
    let scratch = tempfile::tempdir().expect("stores");
    let all = [
        "kg_head",
        "kg_events",
        "kg_event_ops",
        "kg_nodes",
        "kg_edges",
    ];
    let own = live.open(1, &live.url, &scratch.path().join("own.sqlite"), &all);
    let peer = live.open(2, &live.url, &scratch.path().join("peer.sqlite"), &all);
    let dropping = live.open(1, &relay, &scratch.path().join("relay.sqlite"), &[]);
    let apply = |replica: &dyn Replica, request: &str, id: &str, expected: &str| {
        replica.remote_command(
            "kg.proof.apply",
            request,
            json!({"version":1,"context_id":null,"payload":{"id":id,"expected":expected}}),
        )
    };
    let mut seen = Vec::new();
    // Applied frame, receipt not visible in a replica without this client's receipt scope.
    let accepted = ulid::Ulid::new().to_string();
    let attempt = apply(&own, &accepted, &accepted, "0");
    let invisible = classify(&attempt, receipt_now(&peer, &accepted).as_ref());
    assert_eq!(invisible, GraphOutcome::PendingVisibility);
    let resolved = classify(&attempt, Some(&await_receipt(&own, &accepted)));
    assert!(matches!(resolved, GraphOutcome::Accepted { .. }));
    seen.push(("applied+receipt-not-visible", format!("{invisible:?}")));
    seen.push(("applied+accepted", format!("{resolved:?}")));
    // Applied frame carrying a business conflict.
    let conflict = ulid::Ulid::new().to_string();
    let attempt = apply(&own, &conflict, &accepted, "0");
    let outcome = classify(&attempt, Some(&await_receipt(&own, &conflict)));
    assert_eq!(outcome, GraphOutcome::Conflict);
    seen.push(("applied+conflict", format!("{outcome:?}")));
    // Changed payload under an existing request identity.
    let outcome = classify(&apply(&own, &accepted, &accepted, "1"), None);
    assert_eq!(outcome, GraphOutcome::Mismatch);
    seen.push(("binding+changed-payload", format!("{outcome:?}")));
    // Backstop validator refusal is terminal and stays terminal on retry.
    let refused = ulid::Ulid::new().to_string();
    for _ in 0..2 {
        let outcome = classify(&apply(&own, &refused, &technical, "0"), None);
        assert_eq!(
            outcome,
            GraphOutcome::TechnicalRefusal("kg.event_group".into())
        );
        seen.push(("rejected/technical", format!("{outcome:?}")));
    }
    // Response lost after the server committed: pending with the same id, then retry resolves.
    let lost = ulid::Ulid::new().to_string();
    let id = ulid::Ulid::new().to_string();
    let outcome = classify(&apply(&dropping, &lost, &id, "0"), None);
    assert!(matches!(outcome, GraphOutcome::Pending(_)), "{outcome:?}");
    seen.push(("disconnect", format!("{outcome:?}")));
    let retry = apply(&own, &lost, &id, "0");
    assert!(
        retry.as_ref().is_ok_and(|result| result.cached),
        "{retry:?}"
    );
    let outcome = classify(&retry, Some(&await_receipt(&own, &lost)));
    assert!(matches!(outcome, GraphOutcome::Accepted { .. }));
    seen.push(("cached-after-disconnect", format!("{outcome:?}")));
    for (row, outcome) in &seen {
        tracing::info!(row, outcome, "frame");
        println!("KG1_NATIVE_FRAME {row} => {outcome}");
    }
    for replica in [own, peer, dropping] {
        replica.close();
        replica.join();
    }
}

fn state_digest(state: &BTreeMap<String, Value>) -> String {
    hex::encode(Sha256::digest(serde_jcs::to_vec(state).expect("state JCS")))
}

fn manifest(db: &Connection) -> u64 {
    db.query_row("SELECT graph_seq FROM manifest WHERE id=1", [], |row| {
        row.get(0)
    })
    .unwrap_or(0)
}

/// Two persistent replicas, each missing different tables at first, publish
/// only complete prefixes. A parent process kills and restarts this one.
#[test]
#[ignore = "driven by the HQ prefix proof, which kills and restarts it"]
fn native_replicas_publish_only_complete_prefixes() {
    use std::io::Write;
    let live = Live::from_env();
    let dir = std::path::PathBuf::from(std::env::var("KEEPER_HQ_LIVE_STATE_DIR").expect("state"));
    let target: u64 = std::env::var("KEEPER_HQ_LIVE_TARGET")
        .expect("target")
        .parse()
        .expect("n");
    let delay: u32 = std::env::var("KEEPER_HQ_LIVE_DELAY")
        .expect("delay")
        .parse()
        .expect("rounds");
    let staged: [&[&str]; 2] = [
        &["kg_head", "kg_events", "kg_nodes", "kg_edges"],
        &["kg_events", "kg_event_ops", "kg_nodes", "kg_edges"],
    ];
    let all = [
        "kg_head",
        "kg_events",
        "kg_event_ops",
        "kg_nodes",
        "kg_edges",
    ];
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("published.jsonl"))
        .expect("log");
    let full = |index: usize| dir.join(format!("full-{index}"));
    let mut replicas: Vec<SyncularReplica> = (0..2)
        .map(|index| {
            let tables: &[&str] = if full(index).exists() {
                &all
            } else {
                staged[index]
            };
            live.open(
                index as u8 + 1,
                &live.url,
                &dir.join(format!("replica-{index}.sqlite")),
                tables,
            )
        })
        .collect();
    let mut projections: Vec<Connection> = (0..2)
        .map(|index| {
            Connection::open(dir.join(format!("projection-{index}.sqlite"))).expect("projection")
        })
        .collect();
    for db in &projections {
        db.execute_batch("CREATE TABLE IF NOT EXISTS entities(id TEXT PRIMARY KEY, value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS manifest(id INTEGER PRIMARY KEY, graph_seq INTEGER NOT NULL)").expect("schema");
    }
    let deadline = Instant::now() + Duration::from_secs(150);
    let mut round = 0u32;
    loop {
        round += 1;
        let mut done = true;
        for index in 0..2 {
            let replica = &replicas[index];
            replica.sync_now();
            let rows = |table: &str| {
                replica
                    .query(&format!("SELECT * FROM {table}"), &[])
                    .expect(table)
            };
            let head = rows("kg_head")
                .first()
                .map(|row| number(row, "graph_seq").expect("head"))
                .unwrap_or(0);
            let (at, state) = prefix(head, &rows("kg_events"), &rows("kg_event_ops"))
                .expect("observed prefix is never corrupt");
            let published = manifest(&projections[index]);
            if at > published {
                publish(&mut projections[index], at, &state, false);
                writeln!(
                    log,
                    "{}",
                    json!({"replica":index,"n":at,"digest":state_digest(&state)})
                )
                .expect("log");
            } else if head > at || (head == 0 && !full(index).exists()) {
                writeln!(
                    log,
                    "{}",
                    json!({"replica":index,"stale":true,"head":head,"complete":at})
                )
                .expect("log");
            }
            done &= manifest(&projections[index]) >= target;
            if round == delay && !full(index).exists() {
                std::fs::write(full(index), b"1").expect("marker");
            }
        }
        if round == delay {
            // Delayed bootstrap of the missing tables: reopen the same persistent files.
            for replica in replicas.drain(..) {
                replica.close();
                replica.join();
            }
            replicas = (0..2)
                .map(|index| {
                    live.open(
                        index as u8 + 1,
                        &live.url,
                        &dir.join(format!("replica-{index}.sqlite")),
                        &all,
                    )
                })
                .collect();
        }
        if done {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "replicas did not publish the target prefix"
        );
        std::thread::sleep(Duration::from_millis(40));
    }
    projections.clear();
    for replica in replicas {
        replica.close();
        replica.join();
    }
}
