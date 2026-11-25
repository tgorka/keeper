//! Domain capacity history vectors: reproduce every history in
//! `syncular/graph-capacity.vectors.json` through the real `Exporter` and the
//! real `keeper-graph-query` worker, and generate the histories from the real
//! protocol algebra.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use crate::admission::{Admission, Process};
use crate::projection::{Exporter, Store};
use crate::protocol::candidate::{self, State, Verdict};
use crate::protocol::change::{Absorbed, Change, ChangeOp, MergeEdge, NodeFields};
use crate::protocol::delta::{self, Op};
use crate::protocol::scope::event_op_id;
use crate::protocol::vocabulary::{EntityType, Operation};
use crate::protocol::wire::Authority;
use crate::protocol::{inverse, review};
use crate::runtime::{Runtime, VerifiedWorker};
use crate::source::{Checkpoint, GraphSource, PrefixBatch, ScopeState, SourceError};
use crate::supervisor::{self, Cancellation, Stops, BUILD_DEADLINE};
use keeper_graph_protocol::tag::normalise;
use keeper_ports::{Replica, ReplicaSpec, RowOp, Source};
use keeper_replica::SyncularReplica;

const DB: &str = "work";

static WORKER: LazyLock<PathBuf> = LazyLock::new(|| {
    let manifest = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".to_owned());
    let status = std::process::Command::new(cargo)
        .args([
            "build",
            "--quiet",
            "--profile",
            "graph-worker",
            "-p",
            "keeper-graph-query",
            "--bins",
            "--manifest-path",
        ])
        .arg(&manifest)
        .status()
        .expect("cargo runs");
    assert!(status.success(), "the graph worker builds");
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/graph-worker/keeper-graph-query")
        .canonicalize()
        .expect("worker built")
});

fn worker() -> Arc<Runtime> {
    let path: &Path = &WORKER;
    let digest = {
        use sha2::{Digest, Sha256};
        let bytes = std::fs::read(path).expect("worker bytes");
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    #[cfg(target_os = "linux")]
    rustix::thread::set_no_new_privs(true).expect("no_new_privs");
    Arc::new(Runtime::for_tests(
        VerifiedWorker::open(path, Some(&digest)).expect("verified worker"),
    ))
}

fn ulid(n: u32) -> String {
    format!("01JG{n:022}")
}

#[derive(Debug, Deserialize)]
struct VectorFile {
    histories: Vec<HistoryVector>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HistoryVector {
    id: String,
    events: Vec<HistoryEvent>,
    steps: Vec<HistoryStep>,
    worker_entities: u64,
    consumer_entities: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HistoryEvent {
    event_id: String,
    delta_digest: String,
    descriptor: String,
    ops: Vec<Op>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct HistoryStep {
    graph_seq: u64,
    creates: u64,
    review_creates: u64,
    removes: u64,
    export_bytes: u64,
}

struct ObservedHistory {
    steps: Vec<HistoryStep>,
    worker_entities: u64,
    consumer_entities: u64,
}

struct Replicated {
    source: Source,
    scope: Mutex<Checkpoint>,
}

impl GraphSource for Replicated {
    fn domain(&self) -> &str {
        &self.source.database
    }
    fn prefix_batch(&self, after: u64, limit: usize) -> Result<PrefixBatch, SourceError> {
        self.source.prefix_batch(after, limit)
    }
    fn checkpoint(&self) -> Result<Checkpoint, SourceError> {
        Ok(*self.scope.lock().expect("scope"))
    }
}

fn granted() -> Checkpoint {
    Checkpoint {
        scope: ScopeState::Granted,
        cursor: Some(42),
    }
}

fn authority(epoch: &str) -> Authority {
    Authority {
        epoch: epoch.to_owned(),
        digest: "ab".repeat(32),
    }
}

fn upsert(table: &str, values: Value) -> RowOp {
    RowOp::Upsert {
        table: table.to_owned(),
        values: values.as_object().expect("row").clone(),
        base_version: None,
    }
}

fn write_event_to_replica(replica: &SyncularReplica, seq: u64, event: &HistoryEvent) {
    let text = |v: &Option<Value>| v.as_ref().map(|v| Value::from(v.to_string()));
    let mut rows = Vec::with_capacity(event.ops.len() + 2);
    rows.push(upsert(
        "kg_events",
        json!({
            "id": event.event_id,
            "graph": DB,
            "graph_seq": seq.to_string(),
            "descriptor": event.descriptor,
            "request_id": ulid(2_000_000 + u32::try_from(seq).expect("seq")),
            "request_digest": "0".repeat(64),
            "actor_id": "ana",
            "client_id": "dev1",
            "origin_class": "human",
            "origin_actor": null,
            "origin_chain": null,
            "context_id": null,
            "approval_basis": "{\"basis\":\"direct\"}",
            "reverses_event_id": null,
            "event_time_ms": null,
            "recorded_at_ms": 1,
            "op_count": event.ops.len(),
            "delta_digest": event.delta_digest,
            "author_actor": "server",
            "author_client": "server",
        }),
    ));
    for op in &event.ops {
        rows.push(upsert(
            "kg_event_ops",
            json!({
                "id": event_op_id(&event.event_id, op.ordinal),
                "graph": DB,
                "event_id": event.event_id,
                "ordinal": op.ordinal,
                "entity_type": op.entity_type,
                "entity_id": op.entity_id,
                "operation": op.operation,
                "before_revision": op.before_revision,
                "after_revision": op.after_revision,
                "before_value": text(&op.before_value),
                "after_value": text(&op.after_value),
                "author_actor": "server",
                "author_client": "server",
            }),
        ));
    }
    rows.push(upsert(
        "kg_head",
        json!({
            "id": DB,
            "graph": DB,
            "graph_seq": seq.to_string(),
            "event_id": event.event_id,
            "author_actor": "server",
            "author_client": "server",
        }),
    ));
    replica.mutate(rows).expect("replicated");
}

/// Replay a history's events one by one through a real `SyncularReplica`,
/// `Exporter`, and `keeper-graph-query` build worker.
async fn replay_history(
    history_id: &str,
    events: &[HistoryEvent],
    runtime: &Arc<Runtime>,
) -> ObservedHistory {
    let dir = tempfile::tempdir().expect("tempdir");
    let replica = Arc::new(
        SyncularReplica::open(ReplicaSpec {
            database: DB.to_owned(),
            path: dir.path().join("replica.sqlite"),
            client_id: Some("dev1".to_owned()),
            link: None,
            units: Vec::new(),
            watch_presence: None,
        })
        .expect("replica"),
    );
    let replicated = Replicated {
        source: Source {
            database: DB.to_owned(),
            replica: replica.clone(),
            actor: "ana".to_owned(),
            client: "dev1".to_owned(),
        },
        scope: Mutex::new(granted()),
    };
    let store = Store::open(&dir.path().join("projection")).expect("projection store");
    let mut exporter = Exporter::genesis(DB, &store.export_path()).expect("exporter genesis");

    let mut creates = 0u64;
    let mut review_creates = 0u64;
    let mut removes = 0u64;
    let mut steps = Vec::with_capacity(events.len());

    for (idx, event) in events.iter().enumerate() {
        let seq = u64::try_from(idx + 1).expect("graph_seq");
        let computed_digest = delta::digest(&event.ops).expect("delta digest");
        assert_eq!(
            computed_digest, event.delta_digest,
            "history {history_id} graph_seq {seq}: delta_digest mismatch"
        );

        write_event_to_replica(&replica, seq, event);
        let extended = exporter.extend(&replicated).unwrap_or_else(|err| {
            panic!("history {history_id} graph_seq {seq}: exporter.extend failed: {err}")
        });
        assert!(
            extended.complete && extended.head == seq && exporter.n == seq,
            "history {history_id} graph_seq {seq}: incomplete extend (head={}, n={}, complete={})",
            extended.head,
            exporter.n,
            extended.complete
        );

        for op in &event.ops {
            match op.operation {
                Operation::Create => {
                    creates += 1;
                    if op.entity_type == EntityType::Review {
                        review_creates += 1;
                    }
                }
                Operation::Remove => {
                    removes += 1;
                }
                Operation::Update | Operation::Archive | Operation::Unarchive => {}
            }
        }

        assert_eq!(
            exporter.images() as u64,
            creates - removes,
            "history {history_id} graph_seq {seq}: exporter.images() != creates - removes"
        );

        steps.push(HistoryStep {
            graph_seq: seq,
            creates,
            review_creates,
            removes,
            export_bytes: exporter.bytes,
        });
    }

    let consumer_entities = exporter.images() as u64;

    let auth = authority("1");
    let manifest = exporter.manifest(&auth, 42);
    let admission = Admission::new(Process::Agentd);
    let _builder = admission.build().expect("build permit");
    let staged = store.stage().expect("staged generation");
    let mut input = manifest.encode().expect("manifest encode");
    let export = tokio::fs::File::open(exporter.path())
        .await
        .expect("open export");
    let stream = tokio::io::AsyncReadExt::chain(
        std::io::Cursor::new(std::mem::take(&mut input)),
        tokio::io::AsyncReadExt::take(export, exporter.bytes),
    );
    let cancel = Cancellation::default();
    let reader = Cancellation::default();
    let (_revoked_tx, revoked_rx) = tokio::sync::watch::channel(false);
    let stops = Stops::new(&cancel, &reader, revoked_rx);
    let response = supervisor::run(
        runtime,
        &admission,
        "build",
        staged.path().expect("staged path"),
        stream,
        BUILD_DEADLINE,
        stops,
    )
    .await
    .unwrap_or_else(|err| panic!("history {history_id}: worker build failed: {err}"));
    assert_eq!(
        response.manifest, manifest,
        "history {history_id}: worker manifest mismatch"
    );
    assert!(
        response.rows.is_empty(),
        "history {history_id}: build response returned unexpected rows"
    );

    let conn = rusqlite::Connection::open(staged.path().expect("staged path"))
        .expect("open staged sqlite");
    let count: i64 = conn
        .query_row("SELECT count(*) FROM _kg_entities", [], |row| row.get(0))
        .expect("count _kg_entities");
    let worker_entities = u64::try_from(count).expect("non-negative entity count");

    ObservedHistory {
        steps,
        worker_entities,
        consumer_entities,
    }
}

#[derive(Default)]
struct Graph {
    nodes: BTreeMap<String, Map<String, Value>>,
    edges: BTreeMap<String, Map<String, Value>>,
    tags: BTreeMap<String, Map<String, Value>>,
    memberships: BTreeMap<String, Map<String, Value>>,
    contents: BTreeMap<String, Map<String, Value>>,
}

impl State for Graph {
    fn node(&self, id: &str) -> Option<Map<String, Value>> {
        self.nodes.get(id).cloned()
    }
    fn edge(&self, id: &str) -> Option<Map<String, Value>> {
        self.edges.get(id).cloned()
    }
    fn tag(&self, text: &str) -> Option<Map<String, Value>> {
        self.tags.values().find(|t| t["text"] == text).cloned()
    }
    fn membership(&self, node: &str, tag: &str) -> Option<Map<String, Value>> {
        self.memberships
            .values()
            .find(|m| m["node_id"] == node && m["tag_id"] == tag)
            .cloned()
    }
    fn contains_into(&self, to: &[String], limit: usize) -> Vec<Map<String, Value>> {
        self.edges
            .values()
            .filter(|e| e["type"] == "contains" && e["lifecycle"] == "active")
            .filter(|e| to.iter().any(|t| e["to_id"] == t.as_str()))
            .take(limit)
            .cloned()
            .collect()
    }
    fn content(&self, id: &str) -> Option<Map<String, Value>> {
        self.contents.get(id).cloned()
    }
    fn contents_of(&self, owner: &str, limit: usize) -> Vec<Map<String, Value>> {
        self.contents
            .values()
            .filter(|c| c["owner_node_id"] == owner)
            .take(limit)
            .cloned()
            .collect()
    }
    fn sources_from(&self, from: &str, limit: usize) -> Vec<Map<String, Value>> {
        self.edges
            .values()
            .filter(|e| e["type"] == "sources" && e["lifecycle"] == "active")
            .filter(|e| e["from_id"] == from)
            .take(limit)
            .cloned()
            .collect()
    }
    fn incident_active(&self, ids: &[String], limit: usize) -> Vec<Map<String, Value>> {
        self.edges
            .values()
            .filter(|e| e["lifecycle"] == "active")
            .filter(|e| {
                ids.iter()
                    .any(|i| e["from_id"] == i.as_str() || e["to_id"] == i.as_str())
            })
            .take(limit)
            .cloned()
            .collect()
    }
    fn merged_into(&self, ids: &[String], limit: usize) -> Vec<Map<String, Value>> {
        self.nodes
            .values()
            .filter(|n| ids.iter().any(|i| n["merged_into_id"] == i.as_str()))
            .take(limit)
            .cloned()
            .collect()
    }
    fn file_ref(&self, _: &str) -> Option<Map<String, Value>> {
        None
    }
}

impl candidate::Retained for Graph {
    fn written_at(&self, _: &str) -> Option<u64> {
        Some(0)
    }
    fn referrers(&self, _: &str, _: usize) -> Vec<String> {
        Vec::new()
    }
}

struct BuilderWorld {
    graph: Graph,
    events: Vec<HistoryEvent>,
    minted: u32,
    event_base: u32,
}

impl BuilderWorld {
    fn new(event_base: u32, minted_base: u32) -> Self {
        Self {
            graph: Graph::default(),
            events: Vec::new(),
            minted: minted_base,
            event_base,
        }
    }

    fn next_event_id(&self) -> String {
        let seq = u32::try_from(self.events.len() + 1).expect("seq");
        ulid(self.event_base + seq)
    }

    fn apply_ops_to_graph(&mut self, ops: &[Op]) {
        for op in ops {
            let map = match op.entity_type {
                EntityType::Node => &mut self.graph.nodes,
                EntityType::Edge => &mut self.graph.edges,
                EntityType::Tag => &mut self.graph.tags,
                EntityType::Content => &mut self.graph.contents,
                EntityType::Review => continue,
                EntityType::NodeTag => &mut self.graph.memberships,
            };
            match &op.after_value {
                Some(Value::Object(after)) => {
                    map.insert(op.entity_id.clone(), after.clone());
                }
                _ => {
                    map.remove(&op.entity_id);
                }
            }
        }
    }

    fn accept(&mut self, ops: Vec<ChangeOp>) {
        let change = Change::new(ops, None, None, normalise).expect("change");
        let mut minted = self.minted;
        let verdict = candidate::apply(
            &self.graph,
            &change,
            &candidate::Stamp {
                time_ms: 1,
                actor_id: "ana",
            },
            &mut || {
                minted += 1;
                ulid(minted)
            },
        );
        self.minted = minted;
        let Verdict::Accepted(effects) = verdict else {
            panic!("unexpected verdict: {verdict:?}");
        };
        self.push("graph.apply", effects.ops);
    }

    fn push(&mut self, descriptor: &str, ops: Vec<Op>) {
        self.apply_ops_to_graph(&ops);
        let delta_digest = delta::digest(&ops).expect("digest");
        let event_id = self.next_event_id();
        self.events.push(HistoryEvent {
            event_id,
            delta_digest,
            descriptor: descriptor.to_owned(),
            ops,
        });
    }

    fn retained(&self, seq: u64) -> crate::protocol::wire::Event {
        let ev = &self.events[usize::try_from(seq - 1).expect("seq")];
        serde_json::from_value(json!({
            "event": {
                "id": ev.event_id,
                "graph_seq": seq.to_string(),
                "descriptor": ev.descriptor,
                "request_id": ulid(2_000_000 + u32::try_from(seq).expect("seq")),
                "request_digest": "0".repeat(64),
                "actor_id": "ana",
                "client_id": "dev1",
                "origin_class": "human",
                "origin_actor": null,
                "origin_chain": null,
                "context_id": null,
                "approval_basis": {"basis": "direct"},
                "reverses_event_id": null,
                "event_time_ms": null,
                "recorded_at_ms": 1,
                "op_count": ev.ops.len(),
                "delta_digest": ev.delta_digest,
            },
            "ops": ev.ops,
        }))
        .expect("retained event")
    }

    fn rev(&self, id: &str) -> String {
        self.graph.nodes[id]["revision"]
            .as_str()
            .expect("rev")
            .to_owned()
    }

    fn life(&self, id: &str) -> Option<String> {
        self.graph.nodes[id]["lifecycle_revision"]
            .as_str()
            .map(str::to_owned)
    }
}

fn create_node(id: &str, kind: &str, title: &str, properties: Value) -> ChangeOp {
    ChangeOp::CreateNode {
        id: id.to_owned(),
        kind: kind.to_owned(),
        title: title.to_owned(),
        description: None,
        properties: properties.as_object().expect("props").clone(),
    }
}

fn create_edge(world: &BuilderWorld, id: &str, from: &str, to: &str, edge_type: &str) -> ChangeOp {
    ChangeOp::CreateEdge {
        id: id.to_owned(),
        from_id: from.to_owned(),
        to_id: to.to_owned(),
        edge_type: edge_type.to_owned(),
        properties: Map::new(),
        from_lifecycle_revision: world.life(from),
        to_lifecycle_revision: world.life(to),
    }
}

fn build_core_lifecycle() -> Vec<HistoryEvent> {
    let mut w = BuilderWorld::new(1_100_000, 10_000);
    let (n1, n2, n3) = (ulid(101), ulid(102), ulid(103));
    let (e1, c1) = (ulid(110), ulid(120));

    w.accept(vec![
        create_node(&n1, "Concept", "Alpha", json!({"domain": "core"})),
        create_node(&n2, "Concept", "Beta", json!({})),
        create_node(&n3, "Concept", "Gamma", json!({})),
    ]);
    let edge_op = create_edge(&w, &e1, &n1, &n2, "ex:relates");
    w.accept(vec![
        edge_op,
        ChangeOp::CreateContent {
            id: c1,
            owner_node_id: n1.clone(),
            body: json!({"inline_text": "Alpha summary"}),
        },
    ]);
    let n1_rev = w.rev(&n1);
    w.accept(vec![ChangeOp::UpdateNode {
        id: n1,
        expected_revision: n1_rev,
        set: NodeFields {
            title: Some("Alpha Revised".to_owned()),
            description: Some(Some("Updated description".to_owned())),
            description_source: None,
        },
        set_properties: json!({"rank": 1}).as_object().expect("props").clone(),
        remove_properties: Vec::new(),
    }]);
    let n3_rev = w.rev(&n3);
    w.accept(vec![ChangeOp::ArchiveNode {
        id: n3,
        expected_revision: n3_rev,
    }]);
    w.events
}

fn build_tag_membership_churn() -> Vec<HistoryEvent> {
    let mut w = BuilderWorld::new(1_200_000, 20_000);
    let n1 = ulid(201);

    w.accept(vec![
        create_node(&n1, "Concept", "Tagged Node", json!({})),
        ChangeOp::AddTag {
            node_id: n1.clone(),
            tag: "Priority".to_owned(),
            expected_revision: None,
        },
    ]);
    let n1_rev = w.rev(&n1);
    w.accept(vec![ChangeOp::RemoveTag {
        node_id: n1.clone(),
        tag: "priority".to_owned(),
        expected_revision: Some(n1_rev),
    }]);
    let n1_rev = w.rev(&n1);
    w.accept(vec![ChangeOp::AddTag {
        node_id: n1,
        tag: "priority".to_owned(),
        expected_revision: Some(n1_rev),
    }]);
    w.events
}

fn build_acknowledge_and_reverse() -> Vec<HistoryEvent> {
    let mut w = BuilderWorld::new(1_300_000, 30_000);
    let (target, x, y, e1) = (ulid(301), ulid(302), ulid(303), ulid(310));

    w.accept(vec![
        create_node(&target, "Concept", "Target", json!({})),
        create_node(&x, "Concept", "X", json!({})),
        create_node(&y, "Concept", "Y", json!({})),
    ]);
    let edge_op = create_edge(&w, &e1, &x, &y, "ex:rel");
    w.accept(vec![edge_op]);
    let (x_rev, t_rev) = (w.rev(&x), w.rev(&target));
    w.accept(vec![ChangeOp::MergeNodes {
        target_id: target,
        target_revision: t_rev,
        absorbed: vec![Absorbed {
            id: x,
            expected_revision: x_rev,
        }],
        edges: vec![MergeEdge {
            id: e1,
            expected_revision: "1".to_owned(),
            from_lifecycle_revision: "1".to_owned(),
            to_lifecycle_revision: "1".to_owned(),
        }],
    }]);
    let merge = w.retained(3);
    let target_of =
        review::Target::new(&merge.event.id, &merge.event.delta_digest).expect("review target");
    let ack = review::acknowledgement(&ulid(320), &target_of, "reviewer").expect("ack");
    w.push("graph.acknowledge", ack.ops);

    let plan = inverse::plan(&merge).expect("inverse plan");
    let stamp = candidate::Stamp {
        time_ms: 2,
        actor_id: "ana",
    };
    let Verdict::Accepted(reversal) =
        candidate::apply_inverse(&w.graph, &plan, &stamp, &mut || ulid(321))
    else {
        panic!("reversal refused");
    };
    w.push("graph.reverse", reversal.ops);
    w.events
}

#[test]
#[ignore]
fn write_capacity_histories() {
    let out_path =
        std::env::var("CAPACITY_HISTORIES_OUT").expect("CAPACITY_HISTORIES_OUT env var set");
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");

    rt.block_on(async {
        let runtime = worker();
        let raw_histories = vec![
            ("core-lifecycle", build_core_lifecycle()),
            ("tag-membership-churn", build_tag_membership_churn()),
            ("acknowledge-and-reverse", build_acknowledge_and_reverse()),
        ];

        let mut histories = Vec::with_capacity(raw_histories.len());
        for (id, events) in raw_histories {
            let observed = replay_history(id, &events, &runtime).await;
            let last = observed.steps.last().expect("non-empty steps");
            assert_eq!(
                observed.worker_entities,
                last.creates - last.review_creates,
                "history {id}: worker_entities invariant"
            );
            assert_eq!(
                observed.consumer_entities,
                last.creates - last.removes,
                "history {history_id}: consumer_entities invariant",
                history_id = id
            );
            histories.push(HistoryVector {
                id: id.to_owned(),
                events,
                steps: observed.steps,
                worker_entities: observed.worker_entities,
                consumer_entities: observed.consumer_entities,
            });
        }

        let mut json = serde_json::to_string_pretty(&histories).expect("serialize histories");
        json.push('\n');
        std::fs::write(&out_path, json).expect("write histories.json");
    });
}

#[tokio::test]
async fn histories_reproduce_through_the_exporter_and_the_worker() {
    let raw = include_str!("../../../../syncular/graph-capacity.vectors.json");
    let vectors: VectorFile = serde_json::from_str(raw).expect("parse graph-capacity.vectors.json");
    assert!(
        !vectors.histories.is_empty(),
        "graph-capacity.vectors.json must contain at least one history"
    );

    let runtime = worker();
    for history in &vectors.histories {
        let observed = replay_history(&history.id, &history.events, &runtime).await;
        assert_eq!(
            observed.steps.len(),
            history.steps.len(),
            "history {}: step count mismatch",
            history.id
        );
        for (obs, exp) in observed.steps.iter().zip(&history.steps) {
            assert_eq!(
                obs.graph_seq, exp.graph_seq,
                "history {} graph_seq {}: graph_seq mismatch",
                history.id, exp.graph_seq
            );
            assert_eq!(
                obs.creates, exp.creates,
                "history {} graph_seq {}: creates mismatch",
                history.id, exp.graph_seq
            );
            assert_eq!(
                obs.review_creates, exp.review_creates,
                "history {} graph_seq {}: review_creates mismatch",
                history.id, exp.graph_seq
            );
            assert_eq!(
                obs.removes, exp.removes,
                "history {} graph_seq {}: removes mismatch",
                history.id, exp.graph_seq
            );
            assert_eq!(
                obs.export_bytes, exp.export_bytes,
                "history {} graph_seq {}: export_bytes mismatch",
                history.id, exp.graph_seq
            );
        }
        assert_eq!(
            observed.consumer_entities, history.consumer_entities,
            "history {}: consumer_entities mismatch",
            history.id
        );
        assert_eq!(
            observed.worker_entities, history.worker_entities,
            "history {}: worker_entities mismatch",
            history.id
        );
    }
}
