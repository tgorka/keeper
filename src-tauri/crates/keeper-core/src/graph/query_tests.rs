//! The query path end to end on this host: real on-disk replicas seeded with
//! accepted events (the shared algebra's), the real export and the real
//! worker (`--profile graph-worker`, built once by cargo) as builder, query
//! and inspect processes, real publication and recovery. The runtime class
//! is the test class: a dev host is never the measured Linux class (Q6),
//! which `runtime`'s and `ledger`'s guards cover.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, LazyLock, Mutex};

use serde_json::{json, Map, Value};

use super::*;
use crate::graph::admission::Process;
use crate::graph::protocol::candidate::{self, State, Verdict};
use crate::graph::protocol::change::{Change, ChangeOp, NodeFields};
use crate::graph::protocol::delta::Op;
use crate::graph::protocol::scope::event_op_id;
use crate::graph::protocol::worker::InspectEntity;
use crate::graph::runtime::VerifiedWorker;
use crate::graph::source::{Checkpoint, PrefixBatch, SourceError};
use crate::hq::port::{ReplicaSpec, RowOp};
use crate::hq::sessions::Source;
use crate::hq::syncular::SyncularReplica;
use crate::notes::tags::normalise;

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

/// The worker as shipped: `cargo build --profile graph-worker -p
/// keeper-graph-query --bins`, once per test process (cargo serialises
/// concurrent builds itself).
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
    // The worker corroborates `no_new_privs` itself (its execution-class
    // check), so the spawning test thread carries it, as agentd's do.
    #[cfg(target_os = "linux")]
    rustix::thread::set_no_new_privs(true).expect("no_new_privs");
    Arc::new(Runtime::for_tests(
        VerifiedWorker::open(path, Some(&digest)).expect("verified worker"),
    ))
}

fn ulid(n: u32) -> String {
    format!("01JG{n:022}")
}

/// The accepted graph's images, as the server's algebra sees them.
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
}

/// A reversal's extra reads: every row was last written before any event
/// a test reverses, and nothing references another node.
impl candidate::Retained for Graph {
    fn written_at(&self, _: &str) -> Option<u64> {
        Some(0)
    }
    fn referrers(&self, _: &str, _: usize) -> Vec<String> {
        Vec::new()
    }
}

/// A domain's server: the algebra over a graph, writing accepted events
/// into a real replica as the server's sync would.
struct World {
    graph: Graph,
    events: Vec<Vec<Op>>,
    minted: u32,
    source: Source,
}

fn upsert(table: &str, values: Value) -> RowOp {
    RowOp::Upsert {
        table: table.to_owned(),
        values: values.as_object().expect("row").clone(),
        base_version: None,
    }
}

impl World {
    fn new(dir: &Path) -> Self {
        let replica = SyncularReplica::open(ReplicaSpec {
            database: DB.to_owned(),
            path: dir.join("replica.sqlite"),
            client_id: Some("dev1".to_owned()),
            link: None,
            subscriptions: Vec::new(),
            watch_presence: None,
        })
        .expect("replica");
        Self {
            graph: Graph::default(),
            events: Vec::new(),
            minted: 5_000,
            source: Source {
                database: DB.to_owned(),
                replica: Arc::new(replica),
                actor: "ana".to_owned(),
                client: "dev1".to_owned(),
            },
        }
    }

    /// Accept `ops` as the next event; its operations.
    fn accept(&mut self, ops: Vec<ChangeOp>) -> Vec<Op> {
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
            panic!("{verdict:?}");
        };
        for op in &effects.ops {
            let map = match op.entity_type {
                crate::graph::protocol::vocabulary::EntityType::Node => &mut self.graph.nodes,
                crate::graph::protocol::vocabulary::EntityType::Edge => &mut self.graph.edges,
                crate::graph::protocol::vocabulary::EntityType::Tag => &mut self.graph.tags,
                crate::graph::protocol::vocabulary::EntityType::Content => &mut self.graph.contents,
                _ => &mut self.graph.memberships,
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
        self.events.push(effects.ops.clone());
        effects.ops
    }

    /// Accept already-judged event operations (a review, a reversal) as
    /// the next event.
    fn push(&mut self, ops: Vec<Op>) {
        for op in &ops {
            let map = match op.entity_type {
                crate::graph::protocol::vocabulary::EntityType::Node => &mut self.graph.nodes,
                crate::graph::protocol::vocabulary::EntityType::Edge => &mut self.graph.edges,
                crate::graph::protocol::vocabulary::EntityType::Tag => &mut self.graph.tags,
                crate::graph::protocol::vocabulary::EntityType::Content => &mut self.graph.contents,
                crate::graph::protocol::vocabulary::EntityType::Review => continue,
                crate::graph::protocol::vocabulary::EntityType::NodeTag => {
                    &mut self.graph.memberships
                }
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
        self.events.push(ops);
    }

    /// Event `seq` as the server retains it (its header as [`Self::replicate`] writes it).
    fn retained(&self, seq: u64) -> crate::graph::protocol::wire::Event {
        let ops = self.events[usize::try_from(seq - 1).expect("seq")].clone();
        serde_json::from_value(json!({
            "event": {"id": Self::event_id(seq), "graph_seq": seq.to_string(), "descriptor": "graph.apply",
                "request_id": ulid(2_000_000 + u32::try_from(seq).expect("seq")), "request_digest": "0".repeat(64),
                "actor_id": "ana", "client_id": "dev1", "origin_class": "human", "origin_actor": null,
                "origin_chain": null, "context_id": null, "approval_basis": {"basis": "direct"},
                "reverses_event_id": null, "event_time_ms": null, "recorded_at_ms": 1,
                "op_count": ops.len(), "delta_digest": crate::graph::protocol::delta::digest(&ops).expect("digest")},
            "ops": ops,
        }))
        .expect("event")
    }

    fn event_id(seq: u64) -> String {
        ulid(1_000_000 + u32::try_from(seq).expect("seq"))
    }

    /// Replicate events `from..=to` (all their operations unless `drop`
    /// names an ordinal to withhold from the last one) and the head `head`.
    fn replicate(&self, from: u64, to: u64, head: u64, drop: Option<u32>) {
        let text = |v: &Option<Value>| v.as_ref().map(|v| Value::from(v.to_string()));
        let mut rows = Vec::new();
        for seq in from..=to {
            let ops = &self.events[usize::try_from(seq - 1).expect("seq")];
            let event = Self::event_id(seq);
            rows.push(upsert(
                "kg_events",
                json!({
                    "id": event, "graph": DB, "graph_seq": seq.to_string(), "descriptor": "graph.apply",
                    "request_id": ulid(2_000_000 + u32::try_from(seq).expect("seq")), "request_digest": "0".repeat(64),
                    "actor_id": "ana", "client_id": "dev1", "origin_class": "human", "origin_actor": null,
                    "origin_chain": null, "context_id": null, "approval_basis": "{\"basis\":\"direct\"}",
                    "reverses_event_id": null, "event_time_ms": null, "recorded_at_ms": 1,
                    "op_count": ops.len(), "delta_digest": crate::graph::protocol::delta::digest(ops).expect("digest"),
                    "author_actor": "server", "author_client": "server",
                }),
            ));
            for op in ops {
                if seq == to && drop == Some(op.ordinal) {
                    continue;
                }
                rows.push(upsert(
                    "kg_event_ops",
                    json!({
                        "id": event_op_id(&event, op.ordinal), "graph": DB, "event_id": event,
                        "ordinal": op.ordinal, "entity_type": op.entity_type, "entity_id": op.entity_id,
                        "operation": op.operation, "before_revision": op.before_revision,
                        "after_revision": op.after_revision, "before_value": text(&op.before_value),
                        "after_value": text(&op.after_value),
                        "author_actor": "server", "author_client": "server",
                    }),
                ));
            }
        }
        rows.push(upsert(
            "kg_head",
            json!({"id": DB, "graph": DB, "graph_seq": head.to_string(), "event_id": Self::event_id(head),
                   "author_actor": "server", "author_client": "server"}),
        ));
        self.source.replica.mutate(rows).expect("replicated");
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

fn node(id: &str, kind: &str, title: &str, properties: Value) -> ChangeOp {
    ChangeOp::CreateNode {
        id: id.to_owned(),
        kind: kind.to_owned(),
        title: title.to_owned(),
        description: None,
        properties: properties.as_object().expect("props").clone(),
    }
}

fn edge(w: &World, id: &str, from: &str, to: &str, kind: &str, properties: Value) -> ChangeOp {
    let life = |n: &str| w.graph.nodes.get(n).and_then(|_| w.life(n));
    ChangeOp::CreateEdge {
        id: id.to_owned(),
        from_id: from.to_owned(),
        to_id: to.to_owned(),
        edge_type: kind.to_owned(),
        properties: properties.as_object().expect("props").clone(),
        from_lifecycle_revision: life(from),
        to_lifecycle_revision: life(to),
    }
}

/// The source the reads see: the real replica's history, its scope as the
/// test sets it.
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

/// The server's context operations for one agent session, as the test
/// scripts them: each `refresh_context` answers the next scripted step.
struct Server {
    session: String,
    sources: Vec<String>,
    epoch: Mutex<String>,
    refusals: Mutex<Vec<Option<QueryError>>>,
    refreshed: AtomicU32,
    /// When set, the next `refresh_context` answers only once released.
    hold: Mutex<Option<std::sync::mpsc::Receiver<()>>>,
}

impl Server {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            session: "s1".into(),
            sources: vec![DB.to_owned()],
            epoch: Mutex::new("1".into()),
            refusals: Mutex::new(Vec::new()),
            refreshed: AtomicU32::new(0),
            hold: Mutex::new(None),
        })
    }
    fn row(&self, context: &str) -> ContextRow {
        ContextRow {
            context: context.to_owned(),
            session: self.session.clone(),
            permitted: self.sources.clone(),
            sources: self.sources.clone(),
            expires_at_ms: u64::MAX / 2,
            authority: authority(&self.epoch.lock().expect("epoch")),
        }
    }
}

impl GraphRemote for Server {
    fn admit_read(&self, context: &str, domain: &str) -> Result<ContextRow, QueryError> {
        if !self.sources.iter().any(|d| d == domain) {
            return Err(QueryError::Forbidden);
        }
        Ok(self.row(context))
    }
    fn refresh_context(&self, context: &str) -> Result<ContextRow, QueryError> {
        self.refreshed.fetch_add(1, Ordering::SeqCst);
        let next = {
            let mut steps = self.refusals.lock().expect("steps");
            if steps.is_empty() {
                None
            } else {
                steps.remove(0)
            }
        };
        let row = match next {
            Some(error) => Err(error),
            None => Ok(self.row(context)),
        };
        let held = self.hold.lock().expect("hold").take();
        if let Some(release) = held {
            let _ = release.recv();
        }
        row
    }
    fn read_status(&self, _: &str) -> Result<ReadStatus, QueryError> {
        Err(QueryError::Forbidden)
    }
}

const CONTEXT: &str = "01JG0000000000000000000099";

struct Rig {
    _dir: tempfile::TempDir,
    world: World,
    source: Arc<Replicated>,
    reads: GraphReads,
    server: Arc<Server>,
    root: PathBuf,
    admission: Arc<Admission>,
}

impl Rig {
    fn new() -> Self {
        let dir = tempfile::tempdir().expect("dir");
        let world = World::new(dir.path());
        let source = Arc::new(Replicated {
            source: world.source.clone(),
            scope: Mutex::new(granted()),
        });
        let root = dir.path().join("projection");
        let admission = Admission::new(Process::Agentd);
        let reads =
            GraphReads::open(source.clone(), &root, admission.clone(), worker()).expect("reads");
        Self {
            _dir: dir,
            world,
            source,
            reads,
            server: Server::new(),
            root,
            admission,
        }
    }

    fn reader(&self) -> AgentReader {
        let read = AdmittedRead::admit(self.server.as_ref(), "agent-1", "s1", CONTEXT, DB)
            .expect("admitted");
        self.reads.agent(self.server.clone(), read).expect("reader")
    }

    async fn rows(
        &self,
        reader: &AgentReader,
        source: &str,
        params: &str,
    ) -> Vec<Map<String, Value>> {
        reader
            .query(
                source,
                params.as_bytes(),
                Freshness::Current,
                &Cancellation::default(),
            )
            .await
            .unwrap_or_else(|e| panic!("{source}: {e}"))
            .rows
    }
}

fn is_ulid_text(v: &Value) -> bool {
    v.as_str().is_some_and(crate::graph::protocol::is_ulid)
}

/// AC1/AC3: an accepted graph with custom kinds, properties, tags and
/// parallel edges answers exactly, names its prefix, and every id is a
/// canonical ULID, never an engine integer.
#[tokio::test(flavor = "current_thread")]
async fn a_replayed_prefix_answers_the_profile_exactly() {
    let mut rig = Rig::new();
    let (a, b, c) = (ulid(1), ulid(2), ulid(3));
    let w = &mut rig.world;
    w.accept(vec![
        node(&a, "Concept", "Alpha", json!({"score": 1, "note": "x"})),
        node(&b, "work:Task", "Beta", json!({"score": 2.5})),
        node(&c, "Concept", "Gamma", json!({})),
        ChangeOp::AddTag {
            node_id: a.clone(),
            tag: "#Project".into(),
            expected_revision: None,
        },
        ChangeOp::AddTag {
            node_id: b.clone(),
            tag: "#Project".into(),
            expected_revision: None,
        },
    ]);
    let ops = vec![
        edge(
            w,
            &ulid(10),
            &a,
            &b,
            "work:depends_on",
            json!({"weight": 1}),
        ),
        edge(
            w,
            &ulid(11),
            &a,
            &b,
            "work:depends_on",
            json!({"weight": 2}),
        ),
        edge(w, &ulid(12), &a, &c, "contains", json!({})),
    ];
    w.accept(ops);
    w.replicate(1, 2, 2, None);
    let reader = rig.reader();
    let refreshed = reader
        .refresh(&Cancellation::default())
        .await
        .expect("refresh");
    assert_eq!(
        (refreshed.graph_seq, refreshed.built, refreshed.completeness),
        (2, true, Completeness::Current)
    );

    let task = crate::graph::protocol::symbol::encode(
        crate::graph::protocol::symbol::SymbolKind::Node,
        "work:Task",
    )
    .expect("sym");
    let dep = crate::graph::protocol::symbol::encode(
        crate::graph::protocol::symbol::SymbolKind::Edge,
        "work:depends_on",
    )
    .expect("sym");
    let weight = crate::graph::protocol::symbol::encode(
        crate::graph::protocol::symbol::SymbolKind::Property,
        "weight",
    )
    .expect("sym");
    let result = reader
        .query(
            &format!("MATCH (a:Concept)-[d:{dep}]->(b:{task}) RETURN a.title AS from, d.{weight} AS w, d.id AS id ORDER BY w DESC LIMIT 10"),
            b"{}",
            Freshness::Current,
            &Cancellation::default(),
        )
        .await
        .expect("parallel edges");
    assert_eq!(
        (
            result.domain.as_str(),
            result.graph_seq,
            result.checkpoint.as_str(),
            result.completeness
        ),
        (DB, 2, "42", Completeness::Current)
    );
    assert_eq!(result.rows.len(), 2, "parallel edges stay independent");
    assert_eq!(result.rows[0]["w"], json!(2.0));
    assert_eq!(result.rows[0]["id"], json!(ulid(11)));
    assert!(result.rows.iter().all(|r| is_ulid_text(&r["id"])));

    let tagged = rig
        .rows(&reader, "MATCH (n)-[:HAS_TAG]->(t:Tag) WHERE t.text = $t RETURN n.id AS id, t.text AS tag ORDER BY id LIMIT 10", r#"{"t":"project"}"#)
        .await;
    assert_eq!(
        tagged,
        vec![
            json!({"id": a, "tag": "project"})
                .as_object()
                .expect("row")
                .clone(),
            json!({"id": b, "tag": "project"})
                .as_object()
                .expect("row")
                .clone(),
        ]
    );
    let count = rig
        .rows(
            &reader,
            "MATCH (n) WHERE n.kind = $k RETURN count(n) AS n LIMIT 1",
            r#"{"k":"Concept"}"#,
        )
        .await;
    assert_eq!(count[0]["n"], json!(2.0));
    let null = rig
        .rows(
            &reader,
            "MATCH (n:Concept) WHERE n.description IS NULL RETURN n.id AS id ORDER BY id LIMIT 5",
            "{}",
        )
        .await;
    assert_eq!(null.len(), 2);

    let inspected = reader
        .inspect(
            InspectEntity::Node,
            &a,
            false,
            Freshness::Current,
            &Cancellation::default(),
        )
        .await
        .expect("inspect");
    let Some(Inspected::Node(view)) = inspected.rows else {
        panic!("node")
    };
    assert_eq!(view.tags, vec!["project".to_owned()]);
    assert_eq!(view.image["title"], "Alpha");
    assert_eq!(
        rig.admission.stats().live_workers,
        0,
        "every worker was reaped"
    );
}

/// AC3: a new generation is the accepted state exactly — a removed
/// property is gone, a changed value's type changes, an archived endpoint
/// hides its retained edge and its unarchive brings it back.
#[tokio::test(flavor = "current_thread")]
async fn a_new_generation_leaves_no_stale_value() {
    let mut rig = Rig::new();
    let (a, b) = (ulid(1), ulid(2));
    rig.world.accept(vec![
        node(&a, "Concept", "A", json!({"gone": 1, "kind_of": 5})),
        node(&b, "Concept", "B", json!({})),
    ]);
    let ops = vec![edge(&rig.world, &ulid(10), &a, &b, "contains", json!({}))];
    rig.world.accept(ops);
    rig.world.replicate(1, 2, 2, None);
    let reader = rig.reader();
    reader.refresh(&Cancellation::default()).await.expect("g1");
    let gone = crate::graph::protocol::symbol::encode(
        crate::graph::protocol::symbol::SymbolKind::Property,
        "gone",
    )
    .expect("sym");
    let kind_of = crate::graph::protocol::symbol::encode(
        crate::graph::protocol::symbol::SymbolKind::Property,
        "kind_of",
    )
    .expect("sym");
    let probe =
        format!("MATCH (n) WHERE n.id = $id RETURN n.{gone} AS gone, n.{kind_of} AS k LIMIT 1");
    let params = format!(r#"{{"id":"{a}"}}"#);
    assert_eq!(
        rig.rows(&reader, &probe, &params).await[0]["gone"],
        json!(1.0)
    );

    let rev = rig.world.rev(&a);
    rig.world.accept(vec![ChangeOp::UpdateNode {
        id: a.clone(),
        expected_revision: rev,
        set: NodeFields::default(),
        set_properties: json!({"kind_of": "five"}).as_object().expect("p").clone(),
        remove_properties: vec!["gone".into()],
    }]);
    let rev = rig.world.rev(&b);
    rig.world.accept(vec![ChangeOp::ArchiveNode {
        id: b.clone(),
        expected_revision: rev,
    }]);
    rig.world.replicate(3, 4, 4, None);
    assert!(
        reader
            .refresh(&Cancellation::default())
            .await
            .expect("g2")
            .built
    );
    let row = &rig.rows(&reader, &probe, &params).await[0];
    assert_eq!(
        (row["gone"].clone(), row["k"].clone()),
        (Value::Null, json!("five"))
    );
    let hop = "MATCH (a)-[e:contains]->(b) RETURN e.id AS id LIMIT 5";
    assert!(
        rig.rows(&reader, hop, "{}").await.is_empty(),
        "an archived endpoint hides the edge"
    );
    let archived = reader
        .inspect(
            InspectEntity::Node,
            &b,
            false,
            Freshness::Current,
            &Cancellation::default(),
        )
        .await
        .expect("inspect");
    assert_eq!(archived.rows, None);
    let shown = reader
        .inspect(
            InspectEntity::Node,
            &b,
            true,
            Freshness::Current,
            &Cancellation::default(),
        )
        .await
        .expect("inspect");
    assert!(matches!(&shown.rows, Some(Inspected::Node(v)) if v.image["lifecycle"] == "archived"));

    let rev = rig.world.rev(&b);
    rig.world.accept(vec![ChangeOp::UnarchiveNode {
        id: b.clone(),
        expected_revision: rev,
    }]);
    rig.world.replicate(5, 5, 5, None);
    reader.refresh(&Cancellation::default()).await.expect("g3");
    assert_eq!(
        rig.rows(&reader, hop, "{}").await[0]["id"],
        json!(ulid(10)),
        "unarchive re-exposes the retained edge"
    );
}

/// AC2: a missing operation row ends the prefix early — the older complete
/// generation is served only as explicitly stale — and a present event that
/// does not verify is refused, never served shortened.
#[tokio::test(flavor = "current_thread")]
async fn a_partial_or_corrupt_history_is_stale_or_refused() {
    let mut rig = Rig::new();
    let a = ulid(1);
    rig.world.accept(vec![node(&a, "Concept", "A", json!({}))]);
    rig.world.replicate(1, 1, 1, None);
    let reader = rig.reader();
    reader.refresh(&Cancellation::default()).await.expect("g1");
    let rev = rig.world.rev(&a);
    rig.world.accept(vec![
        ChangeOp::UpdateNode {
            id: a.clone(),
            expected_revision: rev,
            set: NodeFields {
                title: Some("B".into()),
                description: None,
                description_source: None,
            },
            set_properties: Map::new(),
            remove_properties: Vec::new(),
        },
        ChangeOp::AddTag {
            node_id: a.clone(),
            tag: "#x".into(),
            expected_revision: Some("1".into()),
        },
    ]);
    // Event 2 arrives without its operation 1.
    rig.world.replicate(2, 2, 2, Some(1));
    let refreshed = reader
        .refresh(&Cancellation::default())
        .await
        .expect("stays at 1");
    assert_eq!(
        (refreshed.graph_seq, refreshed.built, refreshed.completeness),
        (1, false, Completeness::Stale)
    );
    let q = "MATCH (n) RETURN n.title AS t LIMIT 1";
    assert_eq!(
        reader
            .query(q, b"{}", Freshness::Current, &Cancellation::default())
            .await
            .err(),
        Some(QueryError::Stale)
    );
    let stale = reader
        .query(q, b"{}", Freshness::AllowStale, &Cancellation::default())
        .await
        .expect("stale");
    assert_eq!(
        (
            stale.completeness,
            stale.graph_seq,
            stale.rows[0]["t"].clone()
        ),
        (Completeness::Stale, 1, json!("A"))
    );
    // The missing row arrives; the prefix completes.
    rig.world.replicate(2, 2, 2, None);
    assert_eq!(
        reader
            .refresh(&Cancellation::default())
            .await
            .expect("g2")
            .graph_seq,
        2
    );
    assert_eq!(rig.rows(&reader, q, "{}").await[0]["t"], json!("B"));

    // Event 3 is present with a wrong before-image: refused, and the
    // complete generation 2 is never replaced by a shorter one.
    let rev = rig.world.rev(&a);
    rig.world.accept(vec![ChangeOp::ArchiveNode {
        id: a.clone(),
        expected_revision: rev,
    }]);
    rig.world.events[2][0]
        .before_value
        .as_mut()
        .and_then(Value::as_object_mut)
        .expect("image")
        .insert("title".into(), json!("forged"));
    rig.world.replicate(3, 3, 3, None);
    assert_eq!(
        reader.refresh(&Cancellation::default()).await.err(),
        Some(QueryError::Unavailable("prefix.corrupt"))
    );
    let still = reader
        .query(q, b"{}", Freshness::AllowStale, &Cancellation::default())
        .await
        .expect("g2 stale");
    assert_eq!(
        (still.graph_seq, still.completeness),
        (2, Completeness::Stale)
    );
}

/// AC2: every crash window recovers to the last selected complete
/// generation (or none), discarding only disposable output.
#[tokio::test(flavor = "current_thread")]
async fn recovery_keeps_only_the_selected_generation() {
    let mut rig = Rig::new();
    rig.world
        .accept(vec![node(&ulid(1), "Concept", "A", json!({}))]);
    rig.world.replicate(1, 1, 1, None);
    let reader = rig.reader();
    reader.refresh(&Cancellation::default()).await.expect("g1");
    let selected: Vec<_> = std::fs::read_dir(&rig.root)
        .expect("dir")
        .map(|e| e.expect("e").file_name())
        .collect();
    // Leftovers of each window: a builder that never committed, a renamed
    // but unselected generation, a half-written selection.
    for name in [
        "01JG000000000000000000000S.staging",
        "01JG000000000000000000000V.db",
        "current.tmp",
        "01JG000000000000000000000V.db-journal",
    ] {
        std::fs::write(rig.root.join(name), b"partial").expect("leftover");
    }
    let source = rig.source.clone();
    drop(reader);
    // Release this root: the instance holding it is replaced by one elsewhere.
    let elsewhere = GraphReads::open(
        source.clone(),
        &rig.dir_root(),
        rig.admission.clone(),
        worker(),
    )
    .expect("other root");
    drop(std::mem::replace(&mut rig.reads, elsewhere));
    let reads = GraphReads::open(source.clone(), &rig.root, rig.admission.clone(), worker())
        .expect("reopened");
    let mut after: Vec<_> = std::fs::read_dir(&rig.root)
        .expect("dir")
        .map(|e| e.expect("e").file_name())
        .collect();
    // The export is disposable too: a reopened instance replays from genesis.
    let mut before: Vec<_> = selected
        .into_iter()
        .filter(|n| n != "prefix.export")
        .collect();
    before.sort();
    after.sort();
    assert_eq!(
        after, before,
        "only the selected generation, its selection and the lock remain"
    );
    let read =
        AdmittedRead::admit(rig.server.as_ref(), "agent-1", "s1", CONTEXT, DB).expect("admitted");
    let reader = reads.agent(rig.server.clone(), read).expect("reader");
    let rows = reader
        .query(
            "MATCH (n) RETURN n.title AS t LIMIT 1",
            b"{}",
            Freshness::Current,
            &Cancellation::default(),
        )
        .await
        .expect("served after recovery");
    assert_eq!(rows.rows[0]["t"], json!("A"));

    // A selection naming a generation that is gone selects nothing.
    drop(reader);
    drop(reads);
    for entry in std::fs::read_dir(&rig.root).expect("dir") {
        let path = entry.expect("e").path();
        if path.extension().is_some_and(|e| e == "db") {
            std::fs::remove_file(path).expect("rm");
        }
    }
    let reads =
        GraphReads::open(source, &rig.root, rig.admission.clone(), worker()).expect("reopened");
    let read =
        AdmittedRead::admit(rig.server.as_ref(), "agent-1", "s1", CONTEXT, DB).expect("admitted");
    let reader = reads.agent(rig.server.clone(), read).expect("reader");
    assert_eq!(
        reader
            .query(
                "MATCH (n) RETURN n.id AS i LIMIT 1",
                b"{}",
                Freshness::AllowStale,
                &Cancellation::default()
            )
            .await
            .err(),
        Some(QueryError::Unavailable("projection.none"))
    );
    assert!(!rig.root.join("current").exists());
}

impl Rig {
    fn dir_root(&self) -> PathBuf {
        self.root.with_file_name("elsewhere")
    }
}

/// AC4: unsupported grammar and bounds refuse before any worker; a
/// high-cardinality small-LIMIT query is `query_limit`, never a truncated
/// success; another domain or a non-admitted context gets nothing.
#[tokio::test(flavor = "current_thread")]
async fn grammar_limits_and_domains_are_enforced_through_the_reader() {
    let mut rig = Rig::new();
    let ops: Vec<ChangeOp> = (1..=20)
        .map(|i| node(&ulid(i), "Concept", "N", json!({})))
        .collect();
    rig.world.accept(ops);
    rig.world.replicate(1, 1, 1, None);
    let reader = rig.reader();
    reader.refresh(&Cancellation::default()).await.expect("g1");
    for (source, error) in [
        (
            "MATCH (n) DETACH DELETE n RETURN n.id AS i LIMIT 1",
            QueryError::Unsupported,
        ),
        ("MATCH (n) RETURN n.id AS i LIMIT 1001", QueryError::Limit),
        (
            "CALL db.labels() YIELD label RETURN label LIMIT 1",
            QueryError::Unsupported,
        ),
    ] {
        assert_eq!(
            reader
                .query(source, b"{}", Freshness::Current, &Cancellation::default())
                .await
                .err(),
            Some(error)
        );
    }
    // A caller path, URI or domain in parameters is data, never a selector.
    let rows = rig
        .rows(
            &reader,
            "MATCH (n) WHERE n.title = $p RETURN n.id AS i LIMIT 5",
            r#"{"p":"file:/etc/passwd?mode=ro&domain=other"}"#,
        )
        .await;
    assert!(rows.is_empty());
    let all = rig
        .rows(&reader, "MATCH (n) RETURN n.id AS i LIMIT 20", "{}")
        .await;
    assert_eq!(all.len(), 20);
    // A LIMIT is the query's own bound: exactly the first rows by id.
    let first = rig
        .rows(&reader, "MATCH (n) RETURN n.id AS i LIMIT 19", "{}")
        .await;
    assert_eq!(first.len(), 19);
    assert_eq!(first[..], all[..19]);
    // A reader admitted for another domain is refused by these reads.
    let other = Arc::new(Server {
        sources: vec!["other".into()],
        ..Arc::try_unwrap(Server::new()).ok().expect("fresh")
    });
    let read = AdmittedRead::admit(other.as_ref(), "agent-1", "s1", CONTEXT, "other")
        .expect("other domain");
    assert!(matches!(
        rig.reads.agent(other.clone(), read),
        Err(QueryError::Forbidden)
    ));
    // A context the server did not admit for this domain gets nothing.
    assert!(matches!(
        AdmittedRead::admit(other.as_ref(), "agent-1", "s1", CONTEXT, DB),
        Err(QueryError::Forbidden)
    ));
    // A session other than the context's is refused at admission.
    assert!(matches!(
        AdmittedRead::admit(rig.server.as_ref(), "agent-1", "s2", CONTEXT, DB),
        Err(QueryError::Forbidden)
    ));
}

/// AC4 / I-3: a result whose authority is revoked or changed between the
/// native completion and delivery is suppressed.
#[tokio::test(flavor = "current_thread")]
async fn revocation_before_delivery_suppresses_the_result() {
    let mut rig = Rig::new();
    rig.world
        .accept(vec![node(&ulid(1), "Concept", "secret-title", json!({}))]);
    rig.world.replicate(1, 1, 1, None);
    let reader = rig.reader();
    reader.refresh(&Cancellation::default()).await.expect("g1");
    let q = "MATCH (n) RETURN n.title AS t LIMIT 1";
    // The worker answers; the delivery gate's refresh is refused.
    rig.server
        .refusals
        .lock()
        .expect("steps")
        .push(Some(QueryError::Forbidden));
    let before = rig.server.refreshed.load(Ordering::SeqCst);
    let refused = reader
        .query(q, b"{}", Freshness::Current, &Cancellation::default())
        .await;
    assert_eq!(refused.err(), Some(QueryError::Forbidden));
    assert_eq!(
        rig.server.refreshed.load(Ordering::SeqCst),
        before + 1,
        "the gate asked after the worker answered"
    );
    // MED-1: the refusal invalidated the reader: its next call is refused
    // before any work, and no worker is spawned for it.
    let spawned = rig.admission.stats().spawn_attempts;
    // Every attempt so far passed step 5 and was a real spawn call.
    assert!(spawned > 0);
    assert_eq!(rig.admission.stats().spawn_calls, spawned);
    let asked = rig.server.refreshed.load(Ordering::SeqCst);
    for _ in 0..3 {
        assert_eq!(
            reader
                .query(q, b"{}", Freshness::Current, &Cancellation::default())
                .await
                .err(),
            Some(QueryError::Forbidden)
        );
    }
    assert_eq!(
        reader.refresh(&Cancellation::default()).await.err(),
        Some(QueryError::Forbidden)
    );
    assert_eq!(rig.admission.stats().spawn_attempts, spawned);
    assert_eq!(rig.admission.stats().spawn_calls, spawned);
    assert_eq!(
        rig.server.refreshed.load(Ordering::SeqCst),
        asked,
        "nothing asked the server either"
    );
    // LOW-6: a new admission is a new reader; an authority other than its
    // admitted one at delivery is `forbidden` and invalidates it, so the
    // next call spawns nothing.
    let reader = rig.reader();
    *rig.server.epoch.lock().expect("epoch") = "2".into();
    assert_eq!(
        reader
            .query(q, b"{}", Freshness::Current, &Cancellation::default())
            .await
            .err(),
        Some(QueryError::Forbidden)
    );
    let spawned = rig.admission.stats().spawn_attempts;
    assert_eq!(
        reader
            .query(q, b"{}", Freshness::Current, &Cancellation::default())
            .await
            .err(),
        Some(QueryError::Forbidden)
    );
    assert_eq!(rig.admission.stats().spawn_attempts, spawned);
    assert_eq!(rig.admission.stats().live_workers, 0);
}

/// D12: the kill switch kills and reaps a running worker, delivers
/// nothing, purges the projection and leaves the instance refused.
#[tokio::test(flavor = "current_thread")]
async fn revoking_the_domain_kills_running_work_and_purges() {
    let mut rig = Rig::new();
    let n = 120u32;
    let ops: Vec<ChangeOp> = (1..=n)
        .map(|i| node(&ulid(i), "Concept", "N", json!({})))
        .collect();
    rig.world.accept(ops);
    let mut edges = Vec::new();
    let mut next = 10_000;
    for i in 1..=n {
        for j in (i + 1)..=n.min(i + 40) {
            next += 1;
            edges.push(edge(
                &rig.world,
                &ulid(next),
                &ulid(i),
                &ulid(j),
                "sources",
                json!({}),
            ));
        }
    }
    for chunk in edges.chunks(128) {
        rig.world.accept(chunk.to_vec());
    }
    let head = rig.world.events.len() as u64;
    rig.world.replicate(1, head, head, None);
    let reader = Arc::new(rig.reader());
    reader
        .refresh(&Cancellation::default())
        .await
        .expect("built");
    let running = {
        let reader = reader.clone();
        tokio::spawn(async move {
            reader
                .query(
                    "MATCH (a)-[:sources]->(b)-[:sources]->(c)-[:sources]->(d)-[:sources]->(e) RETURN count(*) AS n LIMIT 1",
                    b"{}",
                    Freshness::Current,
                    &Cancellation::default(),
                )
                .await
        })
    };
    let started = std::time::Instant::now();
    while rig.admission.stats().live_workers == 0
        && started.elapsed() < std::time::Duration::from_secs(5)
    {
        tokio::task::yield_now().await;
        tokio::time::sleep(std::time::Duration::from_millis(1)).await;
    }
    assert_eq!(
        rig.admission.stats().live_workers,
        1,
        "the query worker is running"
    );
    rig.reads.revoke().await.expect("revoked");
    let outcome = running.await.expect("joined");
    assert_eq!(outcome.err(), Some(QueryError::Forbidden));
    assert_eq!(
        rig.admission.stats().live_workers,
        0,
        "the worker was killed and reaped"
    );
    let left: Vec<_> = std::fs::read_dir(&rig.root)
        .expect("dir")
        .map(|e| e.expect("e").file_name())
        .collect();
    assert_eq!(
        left,
        vec![std::ffi::OsString::from(".projection.lock")],
        "the projection is purged"
    );
    assert_eq!(
        reader
            .query(
                "MATCH (n) RETURN n.id AS i LIMIT 1",
                b"{}",
                Freshness::AllowStale,
                &Cancellation::default()
            )
            .await
            .err(),
        Some(QueryError::Forbidden)
    );
}

/// A replica whose scope the server revoked kills the domain on the next
/// read, and a cancelled read is reaped as cancelled.
#[tokio::test(flavor = "current_thread")]
async fn a_revoked_scope_or_a_cancellation_stops_reads() {
    let mut rig = Rig::new();
    rig.world
        .accept(vec![node(&ulid(1), "Concept", "A", json!({}))]);
    rig.world.replicate(1, 1, 1, None);
    let reader = rig.reader();
    let cancel = Cancellation::default();
    cancel.cancel();
    assert_eq!(
        reader.refresh(&cancel).await.err(),
        Some(QueryError::Cancelled)
    );
    reader.refresh(&Cancellation::default()).await.expect("g1");
    *rig.source.scope.lock().expect("scope") = Checkpoint {
        scope: ScopeState::Revoked,
        cursor: None,
    };
    assert_eq!(
        reader
            .query(
                "MATCH (n) RETURN n.id AS i LIMIT 1",
                b"{}",
                Freshness::AllowStale,
                &Cancellation::default()
            )
            .await
            .err(),
        Some(QueryError::Forbidden)
    );
    assert!(rig.reads.is_revoked());
    assert!(!rig.root.join("current").exists());
}

#[test]
fn checkpoint_rows_decide_the_scope() {
    use crate::graph::source::checkpoint_of;
    let row = |tbl: &str, status: &str, cursor: i64, holds: bool| {
        json!({"tbl": tbl, "status": status, "cursor": cursor, "holds": holds})
            .as_object()
            .expect("row")
            .clone()
    };
    let all = |status: &str, holds: bool| {
        vec![
            row("kg_events", status, 7, holds),
            row("kg_event_ops", status, 5, holds),
            row("kg_head", status, 9, holds),
        ]
    };
    assert_eq!(
        checkpoint_of(&all("active", true)),
        Checkpoint {
            scope: ScopeState::Granted,
            cursor: Some(5)
        }
    );
    assert_eq!(
        checkpoint_of(&all("active", false)).scope,
        ScopeState::Revoked
    );
    assert_eq!(
        checkpoint_of(&all("revoked", true)).scope,
        ScopeState::Revoked
    );
    assert_eq!(
        checkpoint_of(&all("active", true)[..2]).scope,
        ScopeState::Pending
    );
    let mut first = all("active", true);
    first[0] = row("kg_events", "active", -1, true);
    assert_eq!(checkpoint_of(&first).scope, ScopeState::Pending);
}

/// CC-6 (review BLOCKING-1): only an unheard authority read (it left and its
/// transport failed) is `Unreachable`; only the server refusing the
/// caller's authority is `Forbidden`; every other answered error (retryable
/// or not: internal, storage, unknown, invalid, a technical `kg.*`, the
/// graph switched off) and every local, closed or unreadable failure is
/// `SyncPending`, never `Unreachable`.
#[test]
fn a_native_authority_failure_is_typed_by_its_cause() {
    use crate::hq::port::{ReplicaError, CREDENTIAL_REFUSED};
    let remote = |code: &str, message: &str, retryable: bool| ReplicaError::Remote {
        code: code.to_owned(),
        message: message.to_owned(),
        retryable,
    };
    let unheard = ReplicaError::Unheard {
        code: "transport.failed".to_owned(),
        message: "m".to_owned(),
        retryable: true,
    };
    assert_eq!(remote_error(unheard), QueryError::Unreachable);
    for refusal in [
        remote("operation.forbidden", "forbidden", false),
        remote("operation.forbidden", "context_revoked", false),
        // A message keeper does not name, on a read: fail closed (S0).
        remote("operation.forbidden", "some_new_refusal", false),
        remote("sync.forbidden", "m", false),
        remote("sync.scope_revoked", "m", false),
        remote(CREDENTIAL_REFUSED, "m", false),
        remote("sync.auth_required", "m", false),
        remote("source_audience", "m", false),
        ReplicaError::Rejected {
            code: "operation.forbidden".to_owned(),
            message: "label_unknown".to_owned(),
        },
    ] {
        assert_eq!(
            remote_error(refusal.clone()),
            QueryError::Forbidden,
            "{refusal:?}"
        );
    }
    for answered in [
        remote("sync.internal_error", "storage_unavailable", true),
        remote("operation.execution_failed", "m", false),
        remote("operation.unknown", "m", false),
        remote("operation.invalid_request", "m", false),
        remote("operation.query_failed", "m", false),
        remote("operation.result_too_large", "m", false),
        remote("sync.invalid_client_id", "m", false),
        remote("kg.forbidden", "m", false),
        // An unknown code, even non-retryable, is transient (S0).
        remote("some.new_code", "m", false),
        remote("operation.forbidden", "graph_unavailable", false),
        remote("operation.forbidden", "unsupported_version", false),
        remote("operation.forbidden", "change_limit", false),
        remote("operation.forbidden", "context_expired", false),
        remote("operation.forbidden", "context_closed", false),
        remote("operation.forbidden", "context_invalid", false),
        remote("operation.forbidden", "parent_unavailable", false),
        remote("operation.forbidden", "context_unbound", false),
        ReplicaError::Rejected {
            code: "operation.rejected".to_owned(),
            message: "m".to_owned(),
        },
    ] {
        assert_eq!(
            remote_error(answered.clone()),
            QueryError::SyncPending,
            "{answered:?}"
        );
    }
    for local in [
        ReplicaError::Client("client.invalid_host_response".to_owned()),
        ReplicaError::NotSyncing,
        ReplicaError::Closed,
    ] {
        assert_eq!(remote_error(local), QueryError::SyncPending);
    }
    assert_eq!(QueryError::Unreachable.code(), "sync_pending");
}

/// B2: a selected generation a worker refuses as unreadable (here its
/// `nodes` table is gone) is unselected, and the next refresh rebuilds it
/// from the accepted history with exact rows.
#[tokio::test(flavor = "current_thread")]
async fn a_corrupt_selected_generation_is_rebuilt() {
    let mut rig = Rig::new();
    rig.world
        .accept(vec![node(&ulid(1), "Concept", "kept", json!({}))]);
    rig.world.replicate(1, 1, 1, None);
    let reader = rig.reader();
    reader.refresh(&Cancellation::default()).await.expect("g1");
    let q = "MATCH (n) RETURN n.title AS t LIMIT 1";
    assert_eq!(rig.rows(&reader, q, "{}").await[0]["t"], json!("kept"));
    let generation = std::fs::read_dir(&rig.root)
        .expect("dir")
        .map(|e| e.expect("e").path())
        .find(|p| p.extension().is_some_and(|e| e == "db"))
        .expect("generation");
    let db = rusqlite::Connection::open(&generation).expect("open");
    db.execute_batch("PRAGMA foreign_keys=OFF; DROP TABLE nodes;")
        .expect("corrupt");
    drop(db);
    assert_eq!(
        reader
            .query(q, b"{}", Freshness::Current, &Cancellation::default())
            .await
            .err(),
        Some(QueryError::Unavailable("worker.refused"))
    );
    let rebuilt = reader
        .refresh(&Cancellation::default())
        .await
        .expect("rebuilt");
    assert!(
        rebuilt.built,
        "the refused generation is not kept by the no-change shortcut"
    );
    assert_eq!(rig.rows(&reader, q, "{}").await[0]["t"], json!("kept"));
}

/// LOW-5: a call whose delivery check the server answered just before a
/// sibling call invalidated the reader releases nothing.
#[tokio::test(flavor = "current_thread")]
async fn an_invalidation_during_the_delivery_check_suppresses_the_result() {
    let mut rig = Rig::new();
    rig.world
        .accept(vec![node(&ulid(1), "Concept", "secret", json!({}))]);
    rig.world.replicate(1, 1, 1, None);
    let reader = Arc::new(rig.reader());
    reader.refresh(&Cancellation::default()).await.expect("g1");
    let (release, held) = std::sync::mpsc::channel();
    *rig.server.hold.lock().expect("hold") = Some(held);
    let before = rig.server.refreshed.load(Ordering::SeqCst);
    let pending = {
        let reader = reader.clone();
        tokio::spawn(async move {
            reader
                .query(
                    "MATCH (n) RETURN n.title AS t LIMIT 1",
                    b"{}",
                    Freshness::Current,
                    &Cancellation::default(),
                )
                .await
        })
    };
    // The query is at its delivery check; the server has answered it OK
    // and holds the answer.
    while rig.server.refreshed.load(Ordering::SeqCst) == before {
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;
    }
    rig.server
        .refusals
        .lock()
        .expect("steps")
        .push(Some(QueryError::Forbidden));
    assert_eq!(
        reader.refresh(&Cancellation::default()).await.err(),
        Some(QueryError::Forbidden)
    );
    release.send(()).expect("release");
    assert_eq!(
        pending.await.expect("joined").err(),
        Some(QueryError::Forbidden)
    );
}

/// Projection version 2 (111.1): the assertion fields are structural keys
/// (`stale_after` selected at its boundary with no expiry event), reserved
/// members never become symbols, a content replays for typed inspection
/// only while its owner is active, and a merge's endpoint rewrites are
/// re-projected with the absorbed node out of the view.
#[tokio::test(flavor = "current_thread")]
async fn projection_two_exposes_assertions_contents_and_merges() {
    let mut rig = Rig::new();
    let (claim, target, x, y, content) = (ulid(1), ulid(2), ulid(3), ulid(4), ulid(5));
    let t: u64 = 1_800_000_000_000;
    let assertion = json!({"generated_at": null, "observed_at": 1, "verified_at": null,
                           "verified_by": null, "stale_after": t, "status": "unverified"});
    rig.world.accept(vec![
        node(&claim, "Concept", "Claim", json!({"$assertion": assertion})),
        node(&target, "Concept", "Target", json!({})),
        node(&x, "Concept", "X", json!({})),
        node(&y, "Concept", "Y", json!({})),
        ChangeOp::CreateContent {
            id: content.clone(),
            owner_node_id: x.clone(),
            body: json!({"inline_text": "body"}),
        },
    ]);
    let ops = vec![edge(&rig.world, &ulid(10), &x, &y, "ex:rel", json!({}))];
    rig.world.accept(ops);
    rig.world.replicate(1, 2, 2, None);
    let reader = rig.reader();
    reader.refresh(&Cancellation::default()).await.expect("g1");

    let stale = "MATCH (n) WHERE n.stale_after <= $t RETURN n.id AS id, n.status AS status ORDER BY id LIMIT 5";
    for (at, expected) in [(t - 1, 0), (t, 1), (t + 1, 1)] {
        let rows = rig.rows(&reader, stale, &format!(r#"{{"t":{at}}}"#)).await;
        assert_eq!(rows.len(), expected, "stale_after <= {at}");
        if let Some(row) = rows.first() {
            assert_eq!(
                (row["id"].clone(), row["status"].clone()),
                (json!(claim), json!("unverified"))
            );
        }
    }
    let member = crate::graph::protocol::symbol::encode(
        crate::graph::protocol::symbol::SymbolKind::Property,
        "$assertion",
    );
    assert!(member.is_err(), "a reserved member is never a symbol");

    let inspect = |entity, id: String, archived| {
        let reader = &reader;
        async move {
            reader
                .inspect(
                    entity,
                    &id,
                    archived,
                    Freshness::Current,
                    &Cancellation::default(),
                )
                .await
                .expect("inspect")
                .rows
        }
    };
    let Some(Inspected::Content(image)) =
        inspect(InspectEntity::Content, content.clone(), false).await
    else {
        panic!("content")
    };
    assert_eq!(
        (image["owner_node_id"].clone(), image["revision"].clone()),
        (json!(x), json!("1"))
    );

    let (x_rev, t_rev) = (rig.world.rev(&x), rig.world.rev(&target));
    rig.world.accept(vec![ChangeOp::MergeNodes {
        target_id: target.clone(),
        target_revision: t_rev,
        absorbed: vec![crate::graph::protocol::change::Absorbed {
            id: x.clone(),
            expected_revision: x_rev,
        }],
        edges: vec![crate::graph::protocol::change::MergeEdge {
            id: ulid(10),
            expected_revision: "1".into(),
            from_lifecycle_revision: "1".into(),
            to_lifecycle_revision: "1".into(),
        }],
    }]);
    rig.world.replicate(3, 3, 3, None);
    assert!(
        reader
            .refresh(&Cancellation::default())
            .await
            .expect("g2")
            .built
    );
    let rel = crate::graph::protocol::symbol::encode(
        crate::graph::protocol::symbol::SymbolKind::Edge,
        "ex:rel",
    )
    .expect("sym");
    let hop =
        format!("MATCH (a)-[e:{rel}]->(b) WHERE e.id = $e RETURN a.id AS from, b.id AS to LIMIT 1");
    let rows = rig
        .rows(&reader, &hop, &format!(r#"{{"e":"{}"}}"#, ulid(10)))
        .await;
    assert_eq!(
        (rows[0]["from"].clone(), rows[0]["to"].clone()),
        (json!(target), json!(y)),
        "the rewritten endpoint is re-projected"
    );
    let absorbed = "MATCH (n) WHERE n.id = $id RETURN n.id AS id LIMIT 1";
    assert!(rig
        .rows(&reader, absorbed, &format!(r#"{{"id":"{x}"}}"#))
        .await
        .is_empty());
    // The content stays with the absorbed identity: hidden with its owner,
    // there with archived ones, owner and revision unchanged.
    assert_eq!(
        inspect(InspectEntity::Content, content.clone(), false).await,
        None
    );
    let Some(Inspected::Content(image)) = inspect(InspectEntity::Content, content, true).await
    else {
        panic!("archived content")
    };
    assert_eq!(
        (image["owner_node_id"].clone(), image["revision"].clone()),
        (json!(x), json!("1"))
    );
    let Some(Inspected::Node(view)) = inspect(InspectEntity::Node, x.clone(), true).await else {
        panic!("absorbed node")
    };
    assert_eq!(
        view.image["merged_into_id"],
        json!(target),
        "reported, not substituted"
    );
}

/// Projection version 3: an acknowledgement's `review` operation is
/// accepted and skipped (no entity, no symbol), and a merge's reversal
/// re-projects the cleared redirect and the restored endpoint.
#[tokio::test]
async fn projection_three_skips_reviews_and_reprojects_a_reversed_merge() {
    use crate::graph::protocol::{inverse, review};
    let mut rig = Rig::new();
    let (target, x, y) = (ulid(1), ulid(2), ulid(3));
    rig.world.accept(vec![
        node(&target, "Concept", "Target", json!({})),
        node(&x, "Concept", "X", json!({})),
        node(&y, "Concept", "Y", json!({})),
    ]);
    let ops = vec![edge(&rig.world, &ulid(10), &x, &y, "ex:rel", json!({}))];
    rig.world.accept(ops);
    let (x_rev, t_rev) = (rig.world.rev(&x), rig.world.rev(&target));
    rig.world.accept(vec![ChangeOp::MergeNodes {
        target_id: target.clone(),
        target_revision: t_rev,
        absorbed: vec![crate::graph::protocol::change::Absorbed {
            id: x.clone(),
            expected_revision: x_rev,
        }],
        edges: vec![crate::graph::protocol::change::MergeEdge {
            id: ulid(10),
            expected_revision: "1".into(),
            from_lifecycle_revision: "1".into(),
            to_lifecycle_revision: "1".into(),
        }],
    }]);
    let merge = rig.world.retained(3);
    let target_of =
        review::Target::new(&merge.event.id, &merge.event.delta_digest).expect("target");
    let ack = review::acknowledgement(&ulid(20), &target_of, "reviewer").expect("ack");
    rig.world.push(ack.ops);
    let plan = inverse::plan(&merge).expect("plan");
    let stamp = candidate::Stamp {
        time_ms: 2,
        actor_id: "ana",
    };
    let Verdict::Accepted(reversal) =
        candidate::apply_inverse(&rig.world.graph, &plan, &stamp, &mut || ulid(21))
    else {
        panic!("reversal");
    };
    rig.world.push(reversal.ops);
    rig.world.replicate(1, 5, 5, None);
    let reader = rig.reader();
    let built = reader
        .refresh(&Cancellation::default())
        .await
        .expect("generation");
    assert_eq!(built.graph_seq, 5);
    let rel = crate::graph::protocol::symbol::encode(
        crate::graph::protocol::symbol::SymbolKind::Edge,
        "ex:rel",
    )
    .expect("sym");
    let hop =
        format!("MATCH (a)-[e:{rel}]->(b) WHERE e.id = $e RETURN a.id AS from, b.id AS to LIMIT 1");
    let rows = rig
        .rows(&reader, &hop, &format!(r#"{{"e":"{}"}}"#, ulid(10)))
        .await;
    assert_eq!(
        (rows[0]["from"].clone(), rows[0]["to"].clone()),
        (json!(x), json!(y)),
        "the restored endpoint is re-projected"
    );
    let visible = "MATCH (n) WHERE n.id = $id RETURN n.id AS id, n.merged_into_id AS m LIMIT 1";
    let rows = rig
        .rows(&reader, visible, &format!(r#"{{"id":"{x}"}}"#))
        .await;
    assert_eq!(
        (rows[0]["id"].clone(), rows[0]["m"].clone()),
        (json!(x), Value::Null)
    );
    let review_node = "MATCH (n) WHERE n.id = $id RETURN n.id AS id LIMIT 1";
    assert!(rig
        .rows(&reader, review_node, &format!(r#"{{"id":"{}"}}"#, ulid(20)))
        .await
        .is_empty());
}

/// CC-22: a person's last-authorised offline reads, over the real worker
/// and projection, with a scripted `read_status` and an injected clock.
#[cfg(feature = "human-reads")]
mod offline {
    use std::collections::VecDeque;
    use std::sync::atomic::AtomicI64;

    use super::*;
    use crate::graph::lifecycle::Lifecycle;
    use crate::graph::queue::{Binding, FenceState, GraphQueue};

    const T0: i64 = 1_000_000_000_000;
    const DAY: i64 = 24 * 60 * 60 * 1000;
    const Q: &str = "MATCH (n) RETURN n.title AS t LIMIT 1";

    #[derive(Clone)]
    enum Step {
        Answer,
        Fail(QueryError),
        Panic,
    }

    /// The person's server as the test scripts it: each `read_status`
    /// first moves the clock by `tick`, then answers the next scripted
    /// step (`then` once the script is spent).
    struct Person {
        script: Mutex<VecDeque<Step>>,
        then: Mutex<Step>,
        clock: Arc<AtomicI64>,
        tick: AtomicI64,
        asked: AtomicU32,
    }

    impl GraphRemote for Person {
        fn admit_read(&self, _: &str, _: &str) -> Result<ContextRow, QueryError> {
            Err(QueryError::Forbidden)
        }
        fn refresh_context(&self, _: &str) -> Result<ContextRow, QueryError> {
            Err(QueryError::Forbidden)
        }
        fn read_status(&self, domain: &str) -> Result<ReadStatus, QueryError> {
            self.asked.fetch_add(1, Ordering::SeqCst);
            self.clock
                .fetch_add(self.tick.load(Ordering::SeqCst), Ordering::SeqCst);
            let step = self
                .script
                .lock()
                .expect("script")
                .pop_front()
                .unwrap_or_else(|| self.then.lock().expect("then").clone());
            match step {
                Step::Answer => Ok(ReadStatus {
                    domain: domain.to_owned(),
                    authority: authority("1"),
                }),
                Step::Fail(error) => Err(error),
                Step::Panic => panic!("read_status failed inside the blocking task"),
            }
        }
    }

    struct Offline {
        _rig: Rig,
        _dir: tempfile::TempDir,
        reader: HumanReader,
        person: Arc<Person>,
        clock: Arc<AtomicI64>,
        lifecycle: Arc<Lifecycle>,
    }

    impl Offline {
        /// Built online at `T0` (the authority kept then), then the server
        /// becomes unreachable.
        async fn new() -> Self {
            let mut rig = Rig::new();
            rig.world
                .accept(vec![node(&ulid(1), "Concept", "kept", json!({}))]);
            rig.world.replicate(1, 1, 1, None);
            let clock = Arc::new(AtomicI64::new(T0));
            let person = Arc::new(Person {
                script: Mutex::new(VecDeque::new()),
                then: Mutex::new(Step::Answer),
                clock: clock.clone(),
                tick: AtomicI64::new(0),
                asked: AtomicU32::new(0),
            });
            let dir = tempfile::tempdir().expect("dir");
            let queue = GraphQueue::open(
                &dir.path().join("ana.graph-queue"),
                Binding {
                    server: "https://hq.example".to_owned(),
                    domain: DB.to_owned(),
                    actor: "ana".to_owned(),
                    client: "dev1".to_owned(),
                },
            )
            .expect("queue");
            let lifecycle = Arc::new(Lifecycle::open(queue).expect("lifecycle"));
            let now = clock.clone();
            let reader = rig
                .reads
                .human_with(&rig.world.source, person.clone())
                .with_lifecycle(lifecycle.clone())
                .expect("same domain")
                .with_clock(Arc::new(move || now.load(Ordering::SeqCst)));
            reader
                .refresh(&Cancellation::default())
                .await
                .expect("built online");
            assert_eq!(
                lifecycle
                    .authority()
                    .expect("row")
                    .map(|r| r.observed_at_ms),
                Some(T0)
            );
            *person.then.lock().expect("then") = Step::Fail(QueryError::Unreachable);
            Self {
                _rig: rig,
                _dir: dir,
                reader,
                person,
                clock,
                lifecycle,
            }
        }

        fn at(&self, now: i64, tick: i64) {
            self.clock.store(now, Ordering::SeqCst);
            self.person.tick.store(tick, Ordering::SeqCst);
        }

        fn script(&self, steps: Vec<Step>) {
            *self.person.script.lock().expect("script") = steps.into();
        }

        async fn query(&self, freshness: Freshness) -> Result<Completeness, QueryError> {
            self.reader
                .query(Q, b"{}", freshness, &Cancellation::default())
                .await
                .map(|r| {
                    assert_eq!(r.rows[0]["t"], json!("kept"));
                    r.completeness
                })
        }
    }

    /// An unreachable server serves the kept authority's generation, only
    /// with `AllowStale`, always `Stale`; and an inspect alike.
    #[tokio::test(flavor = "current_thread")]
    async fn an_unreachable_server_serves_the_last_authorised_generation_stale() {
        let o = Offline::new().await;
        o.at(T0 + 60_000, 0);
        assert_eq!(
            o.query(Freshness::AllowStale).await,
            Ok(Completeness::Stale)
        );
        assert_eq!(
            o.query(Freshness::Current).await.map_err(|e| e.code()),
            Err("sync_pending")
        );
        let inspected = o
            .reader
            .inspect(
                InspectEntity::Node,
                &ulid(1),
                false,
                Freshness::AllowStale,
                &Cancellation::default(),
            )
            .await
            .expect("inspected");
        assert_eq!(inspected.completeness, Completeness::Stale);
        // Online again: the same generation is current.
        *o.person.then.lock().expect("then") = Step::Answer;
        assert_eq!(o.query(Freshness::Current).await, Ok(Completeness::Current));
    }

    /// T5 3a: only the typed transport cause serves. A failed join, a local
    /// or closed failure (`SyncPending`), an unreadable answer and storage
    /// refuse, at admission and at delivery; nothing of them is offline.
    #[tokio::test(flavor = "current_thread")]
    async fn only_an_unreachable_server_enters_offline_mode() {
        let o = Offline::new().await;
        o.at(T0 + 60_000, 0);
        let failures = [
            Step::Panic,
            Step::Fail(QueryError::SyncPending),
            Step::Fail(QueryError::Unavailable("authority.answer")),
            Step::Fail(QueryError::Storage("replica")),
        ];
        for failure in failures {
            o.script(vec![failure.clone()]);
            assert!(
                o.query(Freshness::AllowStale).await.is_err(),
                "refused at admission"
            );
            o.script(vec![Step::Fail(QueryError::Unreachable), failure]);
            assert!(
                o.query(Freshness::AllowStale).await.is_err(),
                "refused at delivery"
            );
        }
        // None of them fenced: the next unreachable read still serves.
        assert_eq!(
            o.query(Freshness::AllowStale).await,
            Ok(Completeness::Stale)
        );
    }

    /// The 24 h bound is inclusive and checked at admission and again at
    /// delivery against a fresh clock; a row from the future refuses.
    #[tokio::test(flavor = "current_thread")]
    async fn the_offline_window_is_exactly_one_day_at_admission_and_delivery() {
        let o = Offline::new().await;
        // Exactly 24 h at both checks: served.
        o.at(T0 + DAY, 0);
        assert_eq!(
            o.query(Freshness::AllowStale).await,
            Ok(Completeness::Stale)
        );
        // 24 h + 1 ms at admission: refused.
        o.at(T0 + DAY + 1, 0);
        assert!(o.query(Freshness::AllowStale).await.is_err());
        // Admitted at 24 h − 1 ms, delivered at 24 h + 1 ms: refused.
        o.at(T0 + DAY - 3, 2);
        assert!(o.query(Freshness::AllowStale).await.is_err());
        // Admitted at 24 h − 1 ms, delivered at exactly 24 h: served.
        o.at(T0 + DAY - 2, 1);
        assert_eq!(
            o.query(Freshness::AllowStale).await,
            Ok(Completeness::Stale)
        );
        // The row is 1 ms in the future at admission: refused.
        o.at(T0 - 1, 0);
        assert!(o.query(Freshness::AllowStale).await.is_err());
        // Admitted at the row's own time, the clock set back 1 ms before
        // delivery: refused.
        o.at(T0 + 1, -1);
        assert!(o.query(Freshness::AllowStale).await.is_err());
    }

    /// A server refusal is a revocation: the domain is fenced first, and a
    /// fenced domain serves nothing offline either.
    #[tokio::test(flavor = "current_thread")]
    async fn a_refusal_fences_and_nothing_serves_after_it() {
        let o = Offline::new().await;
        o.at(T0 + 60_000, 0);
        o.script(vec![Step::Fail(QueryError::Forbidden)]);
        assert_eq!(
            o.query(Freshness::AllowStale).await,
            Err(QueryError::Forbidden)
        );
        assert!(o.lifecycle.is_locked());
        assert_eq!(o.lifecycle.fence(), Ok(Some(FenceState::Revoked)));
        let asked = o.person.asked.load(Ordering::SeqCst);
        assert_eq!(
            o.query(Freshness::AllowStale).await,
            Err(QueryError::Forbidden)
        );
        // Refused before the server is even asked.
        assert_eq!(o.person.asked.load(Ordering::SeqCst), asked);
    }

    /// A kept authority found to be from the future is dropped on sight:
    /// the clock catching up does not make it trusted again, a restart does
    /// not bring it back, and only a successful online `read_status` keeps
    /// one again (112.1 r1 BLOCKING-5).
    #[tokio::test(flavor = "current_thread")]
    async fn a_future_authority_stays_untrusted_until_the_server_answers() {
        let o = Offline::new().await;
        o.at(T0 - 1, 0);
        assert!(o.query(Freshness::AllowStale).await.is_err());
        assert_eq!(o.lifecycle.authority(), Ok(None), "dropped, durably");
        o.at(T0 + 1, 0);
        assert!(
            o.query(Freshness::AllowStale).await.is_err(),
            "the clock caught up; the server was not asked successfully"
        );
        // Online again: kept anew, and offline reads serve from it.
        o.script(vec![Step::Answer, Step::Answer]);
        assert_eq!(o.query(Freshness::Current).await, Ok(Completeness::Current));
        assert_eq!(
            o.lifecycle
                .authority()
                .map(|row| row.map(|r| r.observed_at_ms)),
            Ok(Some(T0 + 1))
        );
        o.at(T0 + 2, 0);
        assert_eq!(
            o.query(Freshness::AllowStale).await,
            Ok(Completeness::Stale)
        );
    }

    /// A clock that locks the domain on its `nth` reading, as a revocation
    /// learned elsewhere (a send, a receipt) would while a read is running.
    fn locking_at(o: &mut Offline, nth: u32) {
        let lifecycle = o.lifecycle.clone();
        let clock = o.clock.clone();
        let calls = AtomicU32::new(0);
        let placeholder = o
            ._rig
            .reads
            .human_with(&o._rig.world.source, o.person.clone());
        let reader = std::mem::replace(&mut o.reader, placeholder);
        o.reader = reader.with_clock(Arc::new(move || {
            if calls.fetch_add(1, Ordering::SeqCst) == nth {
                lifecycle.lock(T0 + 1).expect("locked");
            }
            clock.load(Ordering::SeqCst)
        }));
    }

    /// A fence that stands up after admission, while the delivery authority
    /// is asked, refuses the result — online or offline at admission, online
    /// or offline at delivery, query and inspect — though the fence's
    /// cleanup has not run (112.1 r1 BLOCKING-2).
    #[tokio::test(flavor = "current_thread")]
    async fn a_lock_during_delivery_refuses_the_result() {
        let online = Step::Answer;
        let offline = Step::Fail(QueryError::Unreachable);
        for (case, steps, freshness) in [
            (
                "online, online",
                [online.clone(), online.clone()],
                Freshness::Current,
            ),
            (
                "offline, online",
                [offline.clone(), online.clone()],
                Freshness::AllowStale,
            ),
            (
                "offline, offline",
                [offline.clone(), offline],
                Freshness::AllowStale,
            ),
        ] {
            for inspect in [false, true] {
                let mut o = Offline::new().await;
                o.at(T0 + 60_000, 0);
                // The clock is read once at admission (keeping an online
                // authority, or checking the kept one) and once at delivery.
                locking_at(&mut o, 1);
                o.script(steps.to_vec());
                let result = if inspect {
                    o.reader
                        .inspect(
                            InspectEntity::Node,
                            &ulid(1),
                            false,
                            freshness,
                            &Cancellation::default(),
                        )
                        .await
                        .map(|r| r.completeness)
                } else {
                    o.query(freshness).await
                };
                assert!(o.lifecycle.is_locked(), "{case}: the lock landed");
                assert_eq!(
                    result,
                    Err(QueryError::Forbidden),
                    "{case}, inspect {inspect}"
                );
            }
        }
    }

    /// A refresh whose authority answer lands as the domain locks reports
    /// a refusal, never a built projection.
    #[tokio::test(flavor = "current_thread")]
    async fn a_lock_during_refresh_refuses_it() {
        let mut o = Offline::new().await;
        *o.person.then.lock().expect("then") = Step::Answer;
        locking_at(&mut o, 0);
        let refreshed = o.reader.refresh(&Cancellation::default()).await;
        assert!(o.lifecycle.is_locked());
        assert_eq!(refreshed.err(), Some(QueryError::Forbidden));
    }

    /// A fence another holder of the queue file persisted refuses this
    /// reader, though its own lifecycle never saw the lock.
    #[tokio::test(flavor = "current_thread")]
    async fn a_fence_persisted_by_another_holder_refuses() {
        let o = Offline::new().await;
        let other = GraphQueue::open(
            &o._dir.path().join("ana.graph-queue"),
            Binding {
                server: "https://hq.example".to_owned(),
                domain: DB.to_owned(),
                actor: "ana".to_owned(),
                client: "dev1".to_owned(),
            },
        )
        .expect("queue");
        other.persist_fence(T0 + 1).expect("fenced");
        assert!(!o.lifecycle.is_locked());
        assert_eq!(
            o.query(Freshness::Current).await,
            Err(QueryError::Forbidden)
        );
    }

    /// The authority-level fence check on its own (112.1 r2 R2-L4, M-auth):
    /// a lock that lands while the admission authority is taken stops the
    /// read at admission, `forbidden`, and the server is not asked again
    /// for delivery (online); `forbidden`, not `sync_pending` (offline).
    #[tokio::test(flavor = "current_thread")]
    async fn a_lock_at_admission_stops_the_read_there() {
        let mut o = Offline::new().await;
        o.at(T0 + 60_000, 0);
        locking_at(&mut o, 0);
        o.script(vec![Step::Answer, Step::Answer]);
        let asked = o.person.asked.load(Ordering::SeqCst);
        let result = o.query(Freshness::Current).await;
        assert!(o.lifecycle.is_locked());
        assert_eq!(result, Err(QueryError::Forbidden), "online admission");
        assert_eq!(
            o.person.asked.load(Ordering::SeqCst) - asked,
            1,
            "no delivery read_status after the fence"
        );
        let mut o = Offline::new().await;
        o.at(T0 + 60_000, 0);
        locking_at(&mut o, 0);
        let result = o.query(Freshness::AllowStale).await;
        assert!(o.lifecycle.is_locked());
        assert_eq!(result, Err(QueryError::Forbidden), "offline admission");
    }

    /// Locks the domain when woken.
    struct LockOnWake(Arc<Lifecycle>);

    impl std::task::Wake for LockOnWake {
        fn wake(self: Arc<Self>) {
            self.0.lock(T0 + 1).expect("locked");
        }
    }

    /// The final-result fence check on its own (112.1 r2 R2-L4, M-final):
    /// the lock lands synchronously when the worker call's in-flight guard
    /// drops — after the delivery authority (and its check) returned,
    /// before the result is handed out — and the result is refused.
    #[tokio::test(flavor = "current_thread")]
    async fn a_lock_after_the_delivery_authority_refuses_the_result() {
        for inspect in [false, true] {
            let o = Offline::new().await;
            o.at(T0 + 60_000, 0);
            *o.person.then.lock().expect("then") = Step::Answer;
            let reads = o._rig.reads.clone();
            let idle = reads.0.idle.notified();
            let mut idle = std::pin::pin!(idle);
            let waker = std::task::Waker::from(Arc::new(LockOnWake(o.lifecycle.clone())));
            let mut cx = std::task::Context::from_waker(&waker);
            assert!(std::future::Future::poll(idle.as_mut(), &mut cx).is_pending());
            let result = if inspect {
                o.reader
                    .inspect(
                        InspectEntity::Node,
                        &ulid(1),
                        false,
                        Freshness::Current,
                        &Cancellation::default(),
                    )
                    .await
                    .map(|r| r.completeness)
            } else {
                o.query(Freshness::Current).await
            };
            assert!(
                o.lifecycle.is_locked(),
                "the lock landed (inspect {inspect})"
            );
            assert_eq!(result, Err(QueryError::Forbidden), "inspect {inspect}");
        }
    }

    /// A future row whose durable drop failed (another holder write-locks
    /// the queue file) stays distrusted: the clock catching up does not
    /// make it trusted, until the server answers online (112.1 r2 R2-L3).
    #[tokio::test(flavor = "current_thread")]
    async fn a_future_row_that_could_not_be_dropped_stays_untrusted() {
        let o = Offline::new().await;
        let blocker =
            rusqlite::Connection::open(o._dir.path().join("ana.graph-queue")).expect("raw");
        blocker
            .execute_batch("BEGIN IMMEDIATE")
            .expect("write lock");
        o.at(T0 - 1, 0);
        assert!(o.query(Freshness::AllowStale).await.is_err());
        assert!(
            o.lifecycle.authority().expect("row").is_some(),
            "the drop failed: the future row survived"
        );
        o.at(T0 + 1, 0);
        let result = o.query(Freshness::AllowStale).await;
        blocker.execute_batch("ROLLBACK").expect("released");
        assert!(
            result.is_err(),
            "a detected future row regained trust offline: {result:?}"
        );
        // Once the file can be written the drop commits; online again, a
        // fresh answer is kept and trusted.
        o.at(T0 + 2, 0);
        assert!(o.query(Freshness::AllowStale).await.is_err());
        assert_eq!(o.lifecycle.authority(), Ok(None));
        o.script(vec![Step::Answer, Step::Answer]);
        assert_eq!(o.query(Freshness::Current).await, Ok(Completeness::Current));
        o.at(T0 + 3, 0);
        assert_eq!(
            o.query(Freshness::AllowStale).await,
            Ok(Completeness::Stale)
        );
    }
}
