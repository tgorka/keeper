//! E1 against a real keeper-hq (story 110.5, AC1/AC4/AC5): two of a
//! person's devices with different client identities replicate the same
//! accepted graph, build their own projections with the shipped worker and
//! answer the shared `queries[]`-style corpus identically. keeper-hq's
//! harness provisions the server and runs each ignored case inside the
//! admitted runtime (the agent image), with:
//!
//! - `KEEPER_HQ_LIVE_URL`, `KEEPER_HQ_LIVE_DATABASE`, `KEEPER_HQ_LIVE_ACTOR`,
//!   `KEEPER_HQ_LIVE_CLIENT_{1,2}`, `KEEPER_HQ_LIVE_TOKEN_{1,2}`,
//!   `KEEPER_HQ_LIVE_STATE_DIR` (as `keeper-core/tests/graph_live.rs`);
//! - `KEEPER_GRAPH_LIVE_WORKER` (absolute path of the shipped worker) and
//!   `KEEPER_GRAPH_LIVE_WORKER_SHA256`;
//! - `KEEPER_GRAPH_LIVE_LEDGER` (the runtime ledger file; Linux);
//! - `KEEPER_HQ_LIVE_FORBIDDEN_DATABASE` (one the person may not read).
//!
//! Each case prints `KG1_QUERY_LIVE <case> <json>` lines for the harness.
//! Credentials are never printed.

#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use keeper_core::graph::admission::{Admission, Process};
use keeper_core::graph::client::{subscriptions, GraphClient};
use keeper_core::graph::outcome::GraphOutcome;
use keeper_core::graph::protocol::change::{ChangeOp, NodeFields};
use keeper_core::graph::query::{Freshness, GraphReads, HumanReader, QueryError};
use keeper_core::graph::runtime::{Runtime, VerifiedWorker};
use keeper_core::graph::source::{GraphSource, ScopeState};
use keeper_core::graph::supervisor::Cancellation;
use keeper_core::hq::port::{Link, ReplicaSpec};
use keeper_core::hq::sessions::Source;
use keeper_core::hq::syncular::SyncularReplica;
use serde_json::{json, Map, Value};

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("{key} is required"))
}

fn device(index: u8, database: &str) -> Source {
    let actor = env("KEEPER_HQ_LIVE_ACTOR");
    let client = env(&format!("KEEPER_HQ_LIVE_CLIENT_{index}"));
    let dir = PathBuf::from(env("KEEPER_HQ_LIVE_STATE_DIR"));
    let replica = SyncularReplica::open(ReplicaSpec {
        database: database.to_owned(),
        path: dir.join(format!("query-{database}-{index}.sqlite")),
        client_id: Some(client.clone()),
        link: Some(Link {
            base_url: format!("{}/{database}", env("KEEPER_HQ_LIVE_URL")),
            token: env(&format!("KEEPER_HQ_LIVE_TOKEN_{index}")),
        }),
        subscriptions: subscriptions(database, &actor, &client),
        watch_presence: None,
    })
    .expect("a real replica");
    Source {
        database: database.to_owned(),
        replica: Arc::new(replica),
        actor,
        client,
    }
}

fn runtime() -> Arc<Runtime> {
    let worker = VerifiedWorker::open(
        &PathBuf::from(env("KEEPER_GRAPH_LIVE_WORKER")),
        Some(&env("KEEPER_GRAPH_LIVE_WORKER_SHA256")),
    )
    .expect("the shipped worker");
    #[cfg(target_os = "macos")]
    return Arc::new(Runtime::macos_evidence(worker).expect("evidence runtime"));
    #[cfg(target_os = "linux")]
    {
        let ledger = std::fs::read_to_string(env("KEEPER_GRAPH_LIVE_LEDGER")).expect("ledger");
        Arc::new(Runtime::admit(worker, Some(&ledger)).expect("the admitted runtime class"))
    }
}

fn reader(source: &Source, index: u8, admission: Arc<Admission>, runtime: Arc<Runtime>) -> HumanReader {
    let root = PathBuf::from(env("KEEPER_HQ_LIVE_STATE_DIR"))
        .join(format!("query-{}-{index}.projection", source.database));
    GraphReads::open(Arc::new(source.clone()) as Arc<dyn GraphSource>, &root, admission, runtime)
        .expect("reads")
        .human(source)
        .expect("this device's reader")
}

fn report(case: &str, value: Value) {
    println!("KG1_QUERY_LIVE {case} {value}");
}

async fn caught_up(source: &Source, head: u64) {
    let started = Instant::now();
    loop {
        let batch = source.prefix_batch(0, 0).expect("head");
        let scope = source.checkpoint().expect("checkpoint").scope;
        if scope == ScopeState::Granted && batch.head.unwrap_or(0) >= head {
            return;
        }
        assert!(started.elapsed() < Duration::from_secs(60), "replica did not catch up to {head}");
        tokio::time::sleep(Duration::from_millis(200)).await;
    }
}

fn ulid(n: u32) -> String {
    format!("01JH{n:022}")
}

/// The fixture: custom kinds and properties, tags, parallel edges, an
/// archived endpoint and a property removal, applied by device 1.
fn apply_fixture(client: &GraphClient) -> u64 {
    let props = |v: Value| v.as_object().expect("props").clone();
    let mut head = 0;
    let mut send = |ops: Vec<ChangeOp>| {
        let change = client.change(ops, None).expect("change");
        let applied = client.apply(&change).expect("applied");
        match applied.outcome.expect("outcome") {
            GraphOutcome::Accepted { graph_seq, .. } => head = graph_seq.parse().expect("seq"),
            other => panic!("{other:?}"),
        }
    };
    send(vec![
        ChangeOp::CreateNode { id: ulid(1), kind: "Concept".into(), title: "Alpha".into(), description: None, properties: props(json!({"score": 1, "gone": true})) },
        ChangeOp::CreateNode { id: ulid(2), kind: "work:Task".into(), title: "Beta".into(), description: None, properties: props(json!({"score": 2.5})) },
        ChangeOp::CreateNode { id: ulid(3), kind: "Concept".into(), title: "Gamma".into(), description: None, properties: props(json!({})) },
        ChangeOp::AddTag { node_id: ulid(1), tag: "#Project".into(), expected_revision: None },
        ChangeOp::AddTag { node_id: ulid(2), tag: "#Project".into(), expected_revision: None },
    ]);
    send(vec![
        ChangeOp::CreateEdge { id: ulid(10), from_id: ulid(1), to_id: ulid(2), edge_type: "work:depends_on".into(), properties: props(json!({"weight": 1})), from_lifecycle_revision: Some("1".into()), to_lifecycle_revision: Some("1".into()) },
        ChangeOp::CreateEdge { id: ulid(11), from_id: ulid(1), to_id: ulid(2), edge_type: "work:depends_on".into(), properties: props(json!({"weight": 2})), from_lifecycle_revision: Some("1".into()), to_lifecycle_revision: Some("1".into()) },
        ChangeOp::CreateEdge { id: ulid(12), from_id: ulid(1), to_id: ulid(3), edge_type: "contains".into(), properties: props(json!({})), from_lifecycle_revision: Some("1".into()), to_lifecycle_revision: Some("1".into()) },
    ]);
    send(vec![ChangeOp::UpdateNode {
        id: ulid(1),
        expected_revision: "2".into(),
        set: NodeFields { title: Some("Alpha 2".into()), description: None },
        set_properties: Map::new(),
        remove_properties: vec!["gone".into()],
    }]);
    send(vec![ChangeOp::ArchiveNode { id: ulid(3), expected_revision: "1".into() }]);
    head
}

const CORPUS: [(&str, &str); 7] = [
    ("MATCH (n) RETURN n.id AS id, n.title AS title, n.kind AS kind ORDER BY id LIMIT 100", "{}"),
    ("MATCH (n)-[:HAS_TAG]->(t:Tag) RETURN n.id AS id, t.text AS tag LIMIT 100", "{}"),
    ("MATCH (a)-[e]->(b) RETURN e.id AS id, e.type AS type LIMIT 100", "{}"),
    ("MATCH (n) WHERE n.P_676f6e65 IS NOT NULL RETURN n.id AS id LIMIT 10", "{}"),
    ("MATCH (n) WHERE n.P_73636f7265 >= $s RETURN n.id AS id, n.P_73636f7265 AS score ORDER BY score DESC LIMIT 10", r#"{"s":1}"#),
    ("MATCH (a)-[:contains]->(b) RETURN b.id AS id LIMIT 10", "{}"),
    ("MATCH (n) RETURN count(*) AS n LIMIT 1", "{}"),
];

/// AC1/AC5: create → accept → replicate → query → inspect, two devices,
/// equal answers at the same N.
#[tokio::test(flavor = "current_thread")]
#[ignore = "needs a real keeper-hq and the admitted runtime; run by keeper-hq's harness"]
async fn graph_query_live_two_devices_answer_the_same() {
    let database = env("KEEPER_HQ_LIVE_DATABASE");
    let (one, two) = (device(1, &database), device(2, &database));
    let client = GraphClient::new(one.clone());
    caught_up(&one, 0).await;
    let head = tokio::task::spawn_blocking(move || apply_fixture(&client)).await.expect("applied");
    caught_up(&one, head).await;
    caught_up(&two, head).await;
    let (admission, runtime) = (Admission::new(Process::Headless), runtime());
    let readers = [reader(&one, 1, admission.clone(), runtime.clone()), reader(&two, 2, admission, runtime)];
    let mut answers = Vec::new();
    for r in &readers {
        let refreshed = r.refresh(&Cancellation::default()).await.expect("refresh");
        assert_eq!(refreshed.graph_seq, head);
        let mut rows = Vec::new();
        for (source, params) in CORPUS {
            let result = r.query(source, params.as_bytes(), Freshness::Current, &Cancellation::default()).await.expect(source);
            assert_eq!(result.graph_seq, head);
            rows.push(json!({"rows": result.rows, "checkpoint_known": !result.checkpoint.is_empty(),
                             "engine": result.engine, "projection": result.projection_version}));
        }
        answers.push(rows);
    }
    report("two_devices", json!({"head": head, "answers": answers[0]}));
    assert_eq!(answers[0], answers[1], "two devices answer the same accepted graph identically");
    let gone = &answers[0][3]["rows"];
    assert_eq!(gone, &json!([]), "a removed property is gone");
}

/// AC4: a domain this person may not read gives nothing but `forbidden`.
#[tokio::test(flavor = "current_thread")]
#[ignore = "needs a real keeper-hq and the admitted runtime; run by keeper-hq's harness"]
async fn graph_query_live_a_forbidden_domain_reads_nothing() {
    let database = env("KEEPER_HQ_LIVE_FORBIDDEN_DATABASE");
    let source = device(1, &database);
    let r = reader(&source, 1, Admission::new(Process::Headless), runtime());
    let refused = r.refresh(&Cancellation::default()).await.err();
    report("forbidden", json!({"refresh": refused.as_ref().map(QueryError::code)}));
    assert!(matches!(refused, Some(QueryError::Forbidden | QueryError::SyncPending)));
    let queried = r.query("MATCH (n) RETURN n.id AS id LIMIT 1", b"{}", Freshness::AllowStale, &Cancellation::default()).await;
    assert!(queried.is_err());
}
