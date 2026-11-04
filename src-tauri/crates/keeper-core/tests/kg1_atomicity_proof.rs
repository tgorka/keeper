//! 110.2 disposable contract proof. Real persistent native replicas; no query engine.
//! Run the ignored case against the matching exported contract with
//! KEEPER_HQ_LIVE_URL, KEEPER_HQ_LIVE_DATABASE, KEEPER_HQ_LIVE_CLIENT_1/2,
//! KEEPER_HQ_LIVE_TOKEN_1/2. Credentials are transient environment inputs.
use std::collections::BTreeMap;
use std::time::{Duration, Instant};

use keeper_core::hq::port::{
    CommandResult, Delivery, Link, Replica, ReplicaError, ReplicaSpec, RowOp, Subscription,
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

/// How a publication ends after its entity writes.
enum Stop<'a> {
    /// Write the manifest and commit.
    Commit,
    /// Drop the transaction before the manifest: the in-process control.
    DropBeforeManifest,
    /// Announce `marker`, then hold the open transaction until the parent
    /// SIGKILLs this process between replay and manifest.
    PauseBeforeManifest(&'a std::path::Path),
}

/// State and manifest share one checked SQLite transaction.
fn publish(db: &mut Connection, at: u64, state: &BTreeMap<String, Value>, stop: Stop) {
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
    match stop {
        Stop::Commit => {}
        Stop::DropBeforeManifest => return,
        Stop::PauseBeforeManifest(marker) => {
            std::fs::write(marker, json!({"n": at}).to_string()).expect("pause marker");
            loop {
                std::thread::sleep(Duration::from_secs(1));
            }
        }
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
    publish(&mut db, one, &first, Stop::Commit);
    publish(&mut db, two, &second, Stop::DropBeforeManifest);
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
    /// A private proposal receipt: no graph head increment, nothing accepted.
    Proposed(String),
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
                    Some("proposed") => GraphOutcome::Proposed(
                        detail["proposal"].as_str().unwrap_or_default().to_owned(),
                    ),
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
            Some(&receipt(
                "proposed",
                json!({"proposal":"p1","target":"tgdrive"})
            ))
        ),
        GraphOutcome::Proposed("p1".into())
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
    // Applied frame carrying a private proposal receipt: no graph head increment.
    let heads_before = await_rows(&own, "kg_head", 1);
    let proposal = ulid::Ulid::new().to_string();
    let attempt = own.remote_command(
        "kg.proof.apply",
        &proposal,
        json!({"version":1,"context_id":null,"payload":{"id":ulid::Ulid::new().to_string(),"expected":"0","propose":true}}),
    );
    let outcome = classify(&attempt, Some(&await_receipt(&own, &proposal)));
    assert_eq!(outcome, GraphOutcome::Proposed(proposal.clone()));
    own.sync_now();
    assert_eq!(await_rows(&own, "kg_head", 1), heads_before);
    seen.push(("applied+proposed", format!("{outcome:?}")));
    // Applied frame carrying a business refusal (invalid reference).
    let invalid = ulid::Ulid::new().to_string();
    let attempt = own.remote_command(
        "kg.proof.apply",
        &invalid,
        json!({"version":1,"context_id":null,"payload":{"id":ulid::Ulid::new().to_string(),"expected":"0","invalid_ref":"01K1MISSING0000000000000000"}}),
    );
    let outcome = classify(&attempt, Some(&await_receipt(&own, &invalid)));
    assert_eq!(outcome, GraphOutcome::Rejected("invalid_reference".into()));
    seen.push(("applied+rejected", format!("{outcome:?}")));
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

/// The published projection on disk: its manifest and the digest of its entities.
fn on_disk(db: &Connection) -> (u64, String) {
    let mut statement = db
        .prepare("SELECT id, value FROM entities")
        .expect("entities");
    let state: BTreeMap<String, Value> = statement
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .expect("rows")
        .map(|row| {
            let (id, value) = row.expect("row");
            (id, serde_json::from_str(&value).expect("value"))
        })
        .collect();
    (manifest(db), state_digest(&state))
}

const PROOF_TABLES: [&str; 6] = [
    "kg_head",
    "kg_events",
    "kg_event_ops",
    "kg_nodes",
    "kg_edges",
    "kg_node_tags",
];

/// Two persistent replicas whose tables the parent admits one at a time, per
/// replica and in different orders, while multi-table commits continue: each
/// `gate-<replica>-<table>` file subscribes that table (the replica is reopened
/// on its same file). They publish only complete prefixes. The parent may ask
/// replica 0 to pause inside its `KEEPER_HQ_LIVE_PAUSE_AT`-th publication, between
/// replay and manifest, and SIGKILLs it there.
#[test]
#[ignore = "driven by the HQ prefix proof, which gates, kills and restarts it"]
fn native_replicas_publish_only_complete_prefixes() {
    use std::io::Write;
    let live = Live::from_env();
    let dir = std::path::PathBuf::from(std::env::var("KEEPER_HQ_LIVE_STATE_DIR").expect("state"));
    let target: u64 = std::env::var("KEEPER_HQ_LIVE_TARGET")
        .expect("target")
        .parse()
        .expect("n");
    let pause_at: Option<u32> = std::env::var("KEEPER_HQ_LIVE_PAUSE_AT")
        .ok()
        .map(|n| n.parse().expect("pause"));
    let mut log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(dir.join("published.jsonl"))
        .expect("log");
    let gated = |index: usize| -> Vec<&'static str> {
        PROOF_TABLES
            .iter()
            .copied()
            .filter(|table| dir.join(format!("gate-{index}-{table}")).exists())
            .collect()
    };
    let open = |index: usize, tables: &[&str]| {
        live.open(
            index as u8 + 1,
            &live.url,
            &dir.join(format!("replica-{index}.sqlite")),
            tables,
        )
    };
    let mut subscribed: Vec<Vec<&str>> = (0..2).map(gated).collect();
    let mut replicas: Vec<SyncularReplica> = (0..2).map(|i| open(i, &subscribed[i])).collect();
    let mut projections: Vec<Connection> = (0..2)
        .map(|index| {
            let db = Connection::open(dir.join(format!("projection-{index}.sqlite")))
                .expect("projection");
            db.execute_batch("CREATE TABLE IF NOT EXISTS entities(id TEXT PRIMARY KEY, value TEXT NOT NULL); CREATE TABLE IF NOT EXISTS manifest(id INTEGER PRIMARY KEY, graph_seq INTEGER NOT NULL)").expect("schema");
            db
        })
        .collect();
    // What survived the previous process, read before anything new is published.
    for (index, db) in projections.iter().enumerate() {
        let (n, digest) = on_disk(db);
        writeln!(
            log,
            "{}",
            json!({"replica":index,"recovered":n,"digest":digest})
        )
        .expect("log");
    }
    let mut publications = [0u32; 2];
    let deadline = Instant::now() + Duration::from_secs(240);
    loop {
        let mut done = true;
        for index in 0..2 {
            let now = gated(index);
            if now != subscribed[index] {
                // A newly admitted table: the same persistent file, reopened.
                let old = std::mem::replace(&mut replicas[index], open(index, &now));
                old.close();
                old.join();
                subscribed[index] = now;
            }
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
                publications[index] += 1;
                let pause = index == 0 && pause_at == Some(publications[index]);
                let marker = dir.join("paused-0");
                publish(
                    &mut projections[index],
                    at,
                    &state,
                    if pause {
                        Stop::PauseBeforeManifest(&marker)
                    } else {
                        Stop::Commit
                    },
                );
                writeln!(
                    log,
                    "{}",
                    json!({"replica":index,"n":at,"digest":state_digest(&state)})
                )
                .expect("log");
            } else if head > at || subscribed[index].len() < PROOF_TABLES.len() {
                writeln!(
                    log,
                    "{}",
                    json!({"replica":index,"stale":true,"head":head,"complete":at,"tables":subscribed[index]})
                )
                .expect("log");
            }
            done &= manifest(&projections[index]) >= target;
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

/// T2: the caller records the request durably before sending; the parent
/// SIGKILLs it after the server answered and before the outcome is persisted.
/// On restart the same recorded identity is retried, never a new one.
#[test]
#[ignore = "driven by the HQ native crash proof, which SIGKILLs it after the response"]
fn native_caller_resumes_after_crash_before_outcome_persistence() {
    use std::io::Write;
    let live = Live::from_env();
    let dir = std::path::PathBuf::from(std::env::var("KEEPER_HQ_LIVE_STATE_DIR").expect("state"));
    let phase = std::env::var("KEEPER_HQ_LIVE_PHASE").expect("phase");
    let replica = live.open(1, &live.url, &dir.join("caller.sqlite"), &[]);
    let outbox = Connection::open(dir.join("outbox.sqlite")).expect("outbox");
    outbox
        .execute_batch("CREATE TABLE IF NOT EXISTS requests(request TEXT PRIMARY KEY, payload TEXT NOT NULL, outcome TEXT)")
        .expect("outbox schema");
    if phase == "send" {
        let request = ulid::Ulid::new().to_string();
        let payload = json!({"version":1,"context_id":null,"payload":{"id":ulid::Ulid::new().to_string(),"expected":"0"}});
        // Durable before the network: an autocommitted row.
        outbox
            .execute(
                "INSERT INTO requests VALUES (?1, ?2, NULL)",
                params![request, payload.to_string()],
            )
            .expect("record request");
        let answer = replica
            .remote_command("kg.proof.apply", &request, payload)
            .expect("answered");
        println!(
            "KG1_RESPONDED {}",
            json!({"request":request,"commit_seq":answer.commit_seq,"cached":answer.cached})
        );
        std::io::stdout().flush().expect("flush");
        // The outcome is never written: the parent kills this process here.
        loop {
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    let total: i64 = outbox
        .query_row("SELECT count(*) FROM requests", [], |r| r.get(0))
        .expect("count");
    let (request, payload): (String, String) = outbox
        .query_row(
            "SELECT request, payload FROM requests WHERE outcome IS NULL",
            [],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .expect("one unresolved request");
    assert_eq!(total, 1, "no replacement request was minted");
    let retry = replica.remote_command(
        "kg.proof.apply",
        &request,
        serde_json::from_str(&payload).expect("payload"),
    );
    let outcome = classify(&retry, Some(&await_receipt(&replica, &request)));
    let GraphOutcome::Accepted {
        graph_seq,
        commit_seq,
    } = outcome
    else {
        panic!("resumed request not accepted: {outcome:?}");
    };
    outbox
        .execute(
            "UPDATE requests SET outcome = ?2 WHERE request = ?1",
            params![request, graph_seq],
        )
        .expect("persist outcome");
    println!(
        "KG1_RESUMED {}",
        json!({"request":request,"graph_seq":graph_seq,"commit_seq":commit_seq,"cached":retry.as_ref().is_ok_and(|r| r.cached),"requests":total})
    );
    replica.close();
    replica.join();
}

// ─── S02-AC5: one runtime instance, its per-domain replicas, two sessions ───

/// The runtime instance: one replica per domain, shared by all its sessions.
struct Runtime {
    client: String,
    month: String,
    replicas: BTreeMap<String, SyncularReplica>,
}

impl Runtime {
    fn open(dir: &std::path::Path) -> Runtime {
        let url = std::env::var("KEEPER_HQ_LIVE_URL").expect("server URL");
        let client = std::env::var("KEEPER_HQ_LIVE_AGENT_CLIENT").expect("agent client");
        let token = std::env::var("KEEPER_HQ_LIVE_AGENT_TOKEN").expect("agent token");
        let month = std::env::var("KEEPER_HQ_LIVE_MONTH").expect("month");
        let replicas = ["tgdrive", "neuradrive"]
            .into_iter()
            .map(|domain| {
                let replica = SyncularReplica::open(ReplicaSpec {
                    database: domain.into(),
                    path: dir.join(format!("runtime-{domain}.sqlite")),
                    client_id: Some(client.clone()),
                    link: Some(Link {
                        base_url: format!("{url}/{domain}"),
                        token: token.clone(),
                    }),
                    subscriptions: vec![Subscription {
                        id: "kg_nodes".into(),
                        table: "kg_nodes".into(),
                        scopes: vec![("kg_graph".into(), vec![domain.into()])],
                    }],
                    watch_presence: None,
                })
                .expect("runtime replica");
                (domain.to_owned(), replica)
            })
            .collect();
        Runtime {
            client,
            month,
            replicas,
        }
    }
    fn at(&self, domain: &str) -> &SyncularReplica {
        self.replicas.get(domain).expect("domain replica")
    }
    /// The server's answer, or its refusal message (`source_audience`, ...).
    fn context(&self, domain: &str, params: Value) -> Result<Row, String> {
        self.at(domain)
            .remote_query("kg.proof.context", params)
            .map_err(|error| match error {
                ReplicaError::Remote { message, .. } => message,
                other => other.to_string(),
            })
            .map(|rows| rows.into_iter().next().unwrap_or_default())
    }
    /// Commit `ops` through the agent's own replica of `domain` and wait for the server.
    fn write(&self, domain: &str, ops: Vec<RowOp>) -> Result<(), String> {
        let replica = self.at(domain);
        let commit = replica.mutate(ops).map_err(|e| e.to_string())?;
        let deadline = Instant::now() + Duration::from_secs(30);
        loop {
            replica.sync_now();
            match replica.delivery(&commit).expect("delivery") {
                Delivery::Applied => return Ok(()),
                Delivery::Refused(rejection) => return Err(rejection.code),
                Delivery::Pending | Delivery::Unknown => {}
            }
            assert!(Instant::now() < deadline, "write not delivered");
            std::thread::sleep(Duration::from_millis(30));
        }
    }
    fn stamp(&self, mut values: Map<String, Value>) -> Map<String, Value> {
        values.insert("author_actor".into(), "nixi".into());
        values.insert("author_client".into(), self.client.clone().into());
        values
    }
    fn upsert(&self, table: &str, values: Value) -> RowOp {
        RowOp::Upsert {
            table: table.into(),
            values: self.stamp(values.as_object().expect("row").clone()),
            base_version: None,
        }
    }
    /// Every ordinary output destination of `session`, as the agent writes it.
    fn sinks(&self, session: &str) -> BTreeMap<&'static str, Vec<RowOp>> {
        let at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_millis() as i64;
        let page = format!("{session}/{}", self.month);
        let id = || ulid::Ulid::new().to_string();
        let turn = format!("turn-{}", id());
        let child = format!("{}.{}", self.client, at);
        BTreeMap::from([
            ("message", vec![self.upsert("messages", json!({"id":id(),"session_id":session,"page_key":page,"turn_id":turn,"role":"agent","body":"labelled answer","body_blob":null,"metadata":null,"created_at_ms":at}))]),
            ("focus", vec![self.upsert("messages", json!({"id":id(),"session_id":session,"page_key":page,"turn_id":turn,"role":"agent","body":"in this note","body_blob":null,"metadata":json!({"focus":{"drive":"tgdrive","path":"notes/kg1.md"}}).to_string(),"created_at_ms":at}))]),
            ("chunk", vec![self.upsert("turn_chunks", json!({"id":id(),"session_id":session,"turn_id":turn,"seq":1,"text":"partial","created_at_ms":at}))]),
            ("reasoning", vec![self.upsert("reasoning", json!({"id":id(),"session_id":session,"page_key":page,"turn_id":turn,"seq":1,"compacted":false,"text":"thinking","created_at_ms":at}))]),
            ("trace", vec![self.upsert("tool_calls", json!({"id":id(),"session_id":session,"page_key":page,"turn_id":turn,"name":"run","args":"{}","status":"started","result":null,"result_blob":null,"started_at_ms":at,"ended_at_ms":null,"created_at_ms":at}))]),
            ("memory", vec![self.upsert("agent_memory", json!({"id":id(),"agent_actor":"nixi","family":"nixi","kind":"journal","text":"labelled memory","source_session":session,"created_at_ms":at}))]),
            ("delegation", vec![
                self.upsert("sessions", json!({"id":child,"title":"delegated","status":"open","agent_actor":"nixi","created_by_actor":"nixi","created_by_client":self.client,"preferred_client":null,"parent_session":session,"invite":"[]","archived":false,"heard_until":null,"created_at_ms":at,"last_activity_ms":at})),
                self.upsert("session_members", json!({"id":format!("{child}/nixi"),"session_id":child,"actor_id":"nixi","role":"agent","reads_trace":true,"added_by_actor":"nixi","added_at_ms":at,"read_until":null})),
            ]),
        ])
    }
    fn close(self) {
        for replica in self.replicas.into_values() {
            replica.close();
            replica.join();
        }
    }
}

/// Test-only sealed handle, bound before exposure to one server-issued context:
/// its origin database and canonical session. A result carries its provenance.
struct SealedHandle {
    context: String,
    origin: String,
    session: String,
    admitted: std::collections::BTreeSet<String>,
}

struct Sealed {
    origin: String,
    session: String,
    rows: Vec<Row>,
}

impl SealedHandle {
    fn open(rt: &Runtime, origin: &str, session: &str, permitted: &[&str]) -> SealedHandle {
        let row = rt
            .context(
                origin,
                json!({"op":"open","session":session,"permitted":permitted}),
            )
            .expect("context opened");
        SealedHandle {
            context: text(&row, "context").expect("context id").to_owned(),
            origin: origin.into(),
            session: session.into(),
            admitted: Default::default(),
        }
    }
    fn admit(&mut self, rt: &Runtime, domain: &str) -> Result<(), String> {
        rt.context(
            &self.origin,
            json!({"op":"admit","context":self.context,"domain":domain}),
        )?;
        self.admitted.insert(domain.into());
        Ok(())
    }
    /// Model-visible graph data, read from the runtime's own replica of `domain`.
    fn read(
        &self,
        rt: &Runtime,
        caller: (&str, &str),
        domain: &str,
    ) -> Result<Sealed, &'static str> {
        if caller != (self.origin.as_str(), self.session.as_str()) {
            return Err("handle belongs to another session");
        }
        if !self.admitted.contains(domain) {
            return Err("domain not admitted to this context");
        }
        let replica = rt.at(domain);
        replica.sync_now();
        Ok(Sealed {
            origin: self.origin.clone(),
            session: self.session.clone(),
            rows: replica.query("SELECT * FROM kg_nodes", &[]).expect("nodes"),
        })
    }
}

/// The runtime's output path: a result goes only to the session that read it.
fn emit(rt: &Runtime, origin: &str, session: &str, result: &Sealed) -> Result<(), String> {
    if (result.origin.as_str(), result.session.as_str()) != (origin, session) {
        return Err("result belongs to another session".into());
    }
    let mut ops = rt.sinks(session);
    rt.write(origin, ops.remove("message").expect("message"))
}

fn contains(rows: &[Row], needle: &str) -> bool {
    rows.iter().any(|row| {
        row.get("value")
            .and_then(Value::as_str)
            .is_some_and(|v| v.contains(needle))
    })
}

#[test]
#[ignore = "driven by the HQ context proof with one runtime instance and its two sessions"]
fn one_runtime_instance_isolates_handles_results_and_every_output_sink() {
    let dir = std::path::PathBuf::from(std::env::var("KEEPER_HQ_LIVE_STATE_DIR").expect("state"));
    let personal = std::env::var("KEEPER_HQ_LIVE_PERSONAL").expect("personal session");
    let shared = std::env::var("KEEPER_HQ_LIVE_SHARED").expect("shared session");
    let plain = std::env::var("KEEPER_HQ_LIVE_PLAIN").expect("never-graph session");
    let secret = std::env::var("KEEPER_HQ_LIVE_SECRET").expect("personal datum");
    let public = std::env::var("KEEPER_HQ_LIVE_PUBLIC").expect("shared datum");
    let phase = std::env::var("KEEPER_HQ_LIVE_PHASE").expect("phase");
    let rt = Runtime::open(&dir);
    let propose = |domain: &str, context: &str| {
        let request = ulid::Ulid::new().to_string();
        let attempt = rt.at(domain).remote_command(
            "kg.proof.apply",
            &request,
            json!({"version":1,"context_id":context,"payload":{"id":ulid::Ulid::new().to_string(),"expected":"0","propose":true}}),
        );
        match attempt {
            Ok(_) => Ok(request),
            Err(ReplicaError::Remote { message, .. }) => Err(message),
            Err(other) => Err(other.to_string()),
        }
    };
    if phase == "handles" {
        let mut mine = SealedHandle::open(&rt, "neuradrive", &personal, &["tgdrive", "neuradrive"]);
        mine.admit(&rt, "neuradrive").expect("personal domain");
        mine.admit(&rt, "tgdrive").expect("shared domain");
        let deadline = Instant::now() + Duration::from_secs(30);
        let (private, both) = loop {
            let private = mine
                .read(&rt, ("neuradrive", &personal), "neuradrive")
                .expect("read");
            let both = mine
                .read(&rt, ("neuradrive", &personal), "tgdrive")
                .expect("read");
            if contains(&private.rows, &secret) && contains(&both.rows, &public) {
                break (private, both);
            }
            assert!(Instant::now() < deadline, "replicas did not catch up");
            std::thread::sleep(Duration::from_millis(50));
        };
        let mut theirs = SealedHandle::open(&rt, "tgdrive", &shared, &["tgdrive", "neuradrive"]);
        theirs.admit(&rt, "tgdrive").expect("shared domain");
        let refused_admit = theirs
            .admit(&rt, "neuradrive")
            .expect_err("personal domain refused");
        let refused_read = theirs
            .read(&rt, ("tgdrive", &shared), "neuradrive")
            .err()
            .expect("not admitted");
        let borrowed = mine
            .read(&rt, ("tgdrive", &shared), "neuradrive")
            .err()
            .expect("borrow refused");
        let reused = emit(&rt, "tgdrive", &shared, &private).expect_err("result reuse refused");
        emit(&rt, "neuradrive", &personal, &private).expect("own session receives its result");
        let shared_reads_shared = theirs
            .read(&rt, ("tgdrive", &shared), "tgdrive")
            .expect("shared read");
        assert!(!contains(&shared_reads_shared.rows, &secret));
        let proposal_into_shared = propose("tgdrive", &mine.context).expect_err("wider graph");
        let proposal_own = propose("neuradrive", &mine.context).expect("own graph proposal");
        let receipt = await_receipt(rt.at("neuradrive"), &proposal_own);
        assert_eq!(
            receipt.get("outcome").and_then(Value::as_str),
            Some("proposed")
        );
        std::fs::write(dir.join("personal-context"), &mine.context).expect("context");
        assert_eq!(refused_admit, "source_audience");
        assert_eq!(proposal_into_shared, "source_audience");
        println!(
            "KG1_RUNTIME_HANDLES {}",
            json!({
                "personalReadsPersonal": contains(&private.rows, &secret),
                "personalReadsShared": contains(&both.rows, &public),
                "sharedAdmitPersonal": refused_admit,
                "sharedReadUnadmitted": refused_read,
                "borrowedHandle": borrowed,
                "resultReuse": reused,
                "sharedSeesSecret": contains(&shared_reads_shared.rows, &secret),
                "proposalIntoSharedGraph": proposal_into_shared,
                "proposalIntoOwnGraph": "proposed",
            })
        );
    } else {
        // The registry narrowed the personal database's audience after the label
        // was recorded: every ordinary sink of the labelled session refuses, the
        // never-graph session's identical writes still apply.
        let mut outcomes = BTreeMap::new();
        for (name, session) in [("labelled", &personal), ("never_graph", &plain)] {
            let mut seen = BTreeMap::new();
            for (sink, ops) in rt.sinks(session) {
                seen.insert(
                    sink,
                    rt.write("neuradrive", ops)
                        .err()
                        .unwrap_or_else(|| "applied".into()),
                );
            }
            outcomes.insert(name, seen);
        }
        let context = std::fs::read_to_string(dir.join("personal-context")).expect("context");
        let proposal = propose("neuradrive", &context)
            .err()
            .unwrap_or_else(|| "proposed".into());
        let mut plain_handle = SealedHandle::open(&rt, "neuradrive", &plain, &["neuradrive"]);
        plain_handle.admit(&rt, "neuradrive").expect("plain admit");
        let plain_proposal = propose("neuradrive", &plain_handle.context)
            .err()
            .unwrap_or_else(|| "proposed".into());
        for outcome in outcomes["labelled"].values() {
            assert_eq!(outcome, "kg.source_audience");
        }
        for outcome in outcomes["never_graph"].values() {
            assert_eq!(outcome, "applied");
        }
        assert_eq!(proposal, "source_audience");
        assert_eq!(plain_proposal, "proposed");
        println!(
            "KG1_RUNTIME_SINKS {}",
            json!({"labelled":outcomes["labelled"],"neverGraph":outcomes["never_graph"],"labelledProposal":proposal,"neverGraphProposal":plain_proposal})
        );
    }
    rt.close();
}
