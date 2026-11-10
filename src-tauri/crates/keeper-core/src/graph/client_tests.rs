//! The complete-prefix reader and the online-only client over real on-disk
//! replicas seeded through `mutate` (no server): whole events replay, a
//! missing or partial event ends the prefix as stale, a present wrong one is
//! corruption; a change is never sent without a link, its request id
//! survives a restart, and a receipt alone resolves it afterwards.

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Map, Value};

use super::client::{GraphClient, GraphClientError};
use super::outcome::GraphOutcome;
use super::prefix::{self, PrefixError};
use super::protocol::candidate::{self, State, Verdict};
use super::protocol::change::{Change, ChangeOp, DESCRIPTOR};
use super::protocol::delta::Op;
use super::protocol::scope::{client_scope, event_op_id, receipt_id};
use crate::hq::port::{ReplicaSpec, RowOp};
use crate::hq::sessions::Source;
use crate::hq::syncular::SyncularReplica;
use crate::notes::tags::normalise;

const DB: &str = "tgdrive";

fn open(dir: &Path) -> Source {
    let replica = SyncularReplica::open(ReplicaSpec {
        database: DB.to_owned(),
        path: dir.join("graph.sqlite"),
        client_id: Some("dev1".to_owned()),
        link: None,
        subscriptions: Vec::new(),
        watch_presence: None,
    })
    .expect("opens");
    Source {
        database: DB.to_owned(),
        replica: Arc::new(replica),
        actor: "ana".to_owned(),
        client: "dev1".to_owned(),
    }
}

fn ulid(n: u32) -> String {
    format!("01JF{n:022}")
}

fn upsert(table: &str, values: Value) -> RowOp {
    RowOp::Upsert {
        table: table.to_owned(),
        values: values.as_object().expect("row").clone(),
        base_version: None,
    }
}

/// An empty accepted graph, or one holding the given node images.
struct Nodes(Vec<Map<String, Value>>);

impl State for Nodes {
    fn node(&self, id: &str) -> Option<Map<String, Value>> {
        self.0.iter().find(|n| n["id"] == id).cloned()
    }
    fn edge(&self, _: &str) -> Option<Map<String, Value>> {
        None
    }
    fn tag(&self, _: &str) -> Option<Map<String, Value>> {
        None
    }
    fn membership(&self, _: &str, _: &str) -> Option<Map<String, Value>> {
        None
    }
    fn contains_into(&self, _: &[String], _: usize) -> Vec<Map<String, Value>> {
        Vec::new()
    }
    fn content(&self, _: &str) -> Option<Map<String, Value>> {
        None
    }
    fn contents_of(&self, _: &str, _: usize) -> Vec<Map<String, Value>> {
        Vec::new()
    }
    fn sources_from(&self, _: &str, _: usize) -> Vec<Map<String, Value>> {
        Vec::new()
    }
    fn incident_active(&self, _: &[String], _: usize) -> Vec<Map<String, Value>> {
        Vec::new()
    }
    fn merged_into(&self, _: &[String], _: usize) -> Vec<Map<String, Value>> {
        Vec::new()
    }
}

/// The operations the shared algebra gives `ops` over `state`.
fn accepted(state: &Nodes, ops: Vec<ChangeOp>, minted: u32) -> Vec<Op> {
    let change = Change::new(ops, None, None, normalise).expect("change");
    let mut next = minted;
    let mut mint = || {
        next += 1;
        ulid(next)
    };
    let stamp = candidate::Stamp {
        time_ms: 1,
        actor_id: "ana",
    };
    match candidate::apply(state, &change, &stamp, &mut mint) {
        Verdict::Accepted(effects) => effects.ops,
        other => panic!("{other:?}"),
    }
}

/// The replica rows of event `seq` holding `ops`, as the server writes them
/// (JSON columns as text), with its digest unless `digest` overrides it.
fn event_rows(seq: u64, ops: &[Op], digest: Option<&str>) -> Vec<RowOp> {
    let event = ulid(1_000 + u32::try_from(seq).expect("seq"));
    let digest = digest.map_or_else(
        || super::protocol::delta::digest(ops).expect("digest"),
        str::to_owned,
    );
    let text = |v: &Option<Value>| v.as_ref().map(|v| Value::from(v.to_string()));
    let mut rows = vec![upsert(
        "kg_events",
        json!({
            "id": event, "graph": DB, "graph_seq": seq.to_string(), "descriptor": "graph.apply",
            "request_id": ulid(2_000 + u32::try_from(seq).expect("seq")), "request_digest": "0".repeat(64),
            "actor_id": "ana", "client_id": "dev1", "origin_class": "human", "origin_actor": null,
            "origin_chain": null, "context_id": null, "approval_basis": "{\"basis\":\"direct\"}",
            "reverses_event_id": null, "event_time_ms": null, "recorded_at_ms": 1,
            "op_count": ops.len(), "delta_digest": digest,
            "author_actor": "server", "author_client": "server",
        }),
    )];
    for op in ops {
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
    rows
}

fn head(seq: u64) -> RowOp {
    upsert(
        "kg_head",
        json!({"id": DB, "graph": DB, "graph_seq": seq.to_string(),
               "event_id": ulid(1_000 + u32::try_from(seq).expect("seq")),
               "author_actor": "server", "author_client": "server"}),
    )
}

fn create(id: &str) -> ChangeOp {
    ChangeOp::CreateNode {
        id: id.to_owned(),
        kind: "Concept".into(),
        title: "N".into(),
        description: None,
        properties: Map::new(),
    }
}

/// Event 1 creates a node and tags it; event 2 renames it.
fn two_events() -> (Vec<Op>, Vec<Op>) {
    let node = ulid(1);
    let first = accepted(
        &Nodes(Vec::new()),
        vec![
            create(&node),
            ChangeOp::AddTag {
                node_id: node.clone(),
                tag: "#Project".into(),
                expected_revision: None,
            },
        ],
        100,
    );
    let created = first[0].after_value.clone().expect("image");
    let second = accepted(
        &Nodes(vec![created.as_object().expect("image").clone()]),
        vec![ChangeOp::UpdateNode {
            id: node,
            expected_revision: "1".into(),
            set: super::protocol::change::NodeFields {
                title: Some("Renamed".into()),
                description: None,
                description_source: None,
            },
            set_properties: Map::new(),
            remove_properties: Vec::new(),
        }],
        200,
    );
    (first, second)
}

#[test]
fn a_prefix_replays_whole_events_from_the_replica() {
    let dir = tempfile::tempdir().expect("dir");
    let source = open(dir.path());
    let (first, second) = two_events();
    let mut rows = event_rows(1, &first, None);
    rows.extend(event_rows(2, &second, None));
    rows.push(head(2));
    source.replica.mutate(rows).expect("seeded");

    let prefix = prefix::complete(&source).expect("prefix");
    assert_eq!((prefix.head, prefix.n), (2, 2));
    assert!(prefix.is_complete());
    assert_eq!(prefix.events[1].ops, second);
    let node = &prefix.state[&("node".to_owned(), ulid(1))];
    assert_eq!(
        (node["title"].clone(), node["revision"].clone()),
        (json!("Renamed"), json!("2"))
    );
    assert_eq!(
        prefix.state[&("tag".to_owned(), ulid(101))]["text"],
        "project"
    );
}

#[test]
fn a_missing_or_partial_event_ends_the_prefix_stale() {
    let (first, second) = two_events();
    // Head 3 with event 3 absent; then event 2 holding one of its operations.
    for (rows_of_second, expected) in [(second.len(), 2), (0, 1)] {
        let dir = tempfile::tempdir().expect("dir");
        let source = open(dir.path());
        let mut rows = event_rows(1, &first, None);
        let mut second_rows = event_rows(2, &second, None);
        if rows_of_second == 0 {
            // The header arrived, its only operation did not.
            second_rows.truncate(1);
        }
        rows.extend(second_rows);
        rows.push(head(3));
        source.replica.mutate(rows).expect("seeded");
        let prefix = prefix::complete(&source).expect("prefix");
        assert_eq!((prefix.head, prefix.n), (3, expected));
        assert!(!prefix.is_complete());
    }
}

#[test]
fn a_present_wrong_event_is_corruption_not_a_shorter_prefix() {
    let (first, second) = two_events();
    // A digest that does not match, and a before-image the replay disagrees with.
    let mut wrong_image = second.clone();
    if let Some(Value::Object(before)) = wrong_image[0].before_value.as_mut() {
        before.insert("title".into(), json!("Not what event 1 wrote"));
    }
    for (ops, digest) in [(second.clone(), Some("f".repeat(64))), (wrong_image, None)] {
        let dir = tempfile::tempdir().expect("dir");
        let source = open(dir.path());
        let mut rows = event_rows(1, &first, None);
        rows.extend(event_rows(2, &ops, digest.as_deref()));
        rows.push(head(2));
        source.replica.mutate(rows).expect("seeded");
        assert!(matches!(
            prefix::complete(&source),
            Err(PrefixError::Corrupt { at: 2, .. })
        ));
    }
}

#[test]
fn a_prefix_spans_snapshot_batches() {
    let dir = tempfile::tempdir().expect("dir");
    let source = open(dir.path());
    let count = u32::try_from(prefix::BATCH_EVENTS).expect("batch") + 44;
    let mut rows = Vec::new();
    for n in 1..=count {
        let ops = accepted(&Nodes(Vec::new()), vec![create(&ulid(5_000 + n))], 0);
        rows.extend(event_rows(u64::from(n), &ops, None));
    }
    rows.push(head(u64::from(count)));
    source.replica.mutate(rows).expect("seeded");
    let prefix = prefix::complete(&source).expect("prefix");
    assert_eq!(prefix.n, u64::from(count));
    assert_eq!(prefix.state.len(), usize::try_from(count).expect("count"));
}

#[test]
fn a_change_is_never_sent_without_a_link_and_its_id_survives_a_restart() {
    let dir = tempfile::tempdir().expect("dir");
    let source = open(dir.path());
    let client = GraphClient::new(source.clone());
    let change = client.change(vec![create(&ulid(1))], None).expect("change");
    assert_eq!(client.apply(&change), Err(GraphClientError::Offline));

    // Minted once and persisted; restored after a restart under the same key.
    let prepared = client.mint(&change).expect("minted");
    assert_eq!(client.send(&prepared), Err(GraphClientError::Offline));
    let persisted = serde_json::to_string(&json!({
        "request_id": prepared.request().request_id,
        "payload": prepared.request().payload,
    }))
    .expect("persisted");
    drop(client);
    let client = GraphClient::new(source.clone());
    let saved: Value = serde_json::from_str(&persisted).expect("read back");
    let restored = client
        .prepared(
            super::protocol::change::DESCRIPTOR,
            super::protocol::request::Request {
                request_id: saved["request_id"].as_str().expect("id").to_owned(),
                context_id: None,
                payload: saved["payload"].clone(),
            },
        )
        .expect("restored");
    assert_eq!(restored.key(), prepared.key());

    // Without an attempt in hand, only the request's own receipt settles it.
    assert!(matches!(
        client.outcome(&restored, None),
        Ok(GraphOutcome::Pending { .. })
    ));
    let scope = client_scope("ana", "dev1");
    let key = restored.key();
    source
        .replica
        .mutate(vec![upsert(
            "kg_receipts",
            json!({
                "id": receipt_id(key.descriptor(), key.request_id(), &scope),
                "client_scope": scope, "descriptor": key.descriptor(),
                "request_id": key.request_id(), "request_digest": key.request_digest(),
                "outcome": "accepted", "reason": null, "event_id": ulid(900), "graph_seq": "7",
                "detail": "{\"affected\":[]}", "recorded_at_ms": 1,
                "author_actor": "server", "author_client": "server",
            }),
        )])
        .expect("receipt");
    assert_eq!(
        client.outcome(&restored, None),
        Ok(GraphOutcome::Accepted {
            event_id: ulid(900),
            graph_seq: "7".into(),
            commit_seq: None,
            detail: json!({"affected": []}).as_object().expect("detail").clone(),
        })
    );
}

#[test]
fn a_restored_request_must_still_be_a_persons_change() {
    let dir = tempfile::tempdir().expect("dir");
    let client = GraphClient::new(open(dir.path()));
    let request = |context: Option<String>, payload: Value| super::protocol::request::Request {
        request_id: ulid(3),
        context_id: context,
        payload,
    };
    let change = json!({"ops": [{"op": "archive_node", "id": ulid(1), "expected_revision": "1"}]});
    assert!(client
        .prepared(DESCRIPTOR, request(None, change.clone()))
        .is_ok());
    assert!(client
        .prepared(DESCRIPTOR, request(Some(ulid(4)), change.clone()))
        .is_err());
    assert!(client
        .prepared(DESCRIPTOR, request(None, json!({"ops": []})))
        .is_err());
    // A payload is restored only as the command it is: a change is not a
    // reversal, and no descriptor outside the client's commands restores.
    assert!(client
        .prepared("graph.reverse", request(None, change.clone()))
        .is_err());
    assert!(client
        .prepared("graph.outcome", request(None, change.clone()))
        .is_err());
    let reverse = json!({"target_event_id": ulid(5), "target_delta_digest": "ab".repeat(32),
        "expected": [{"entity_type": "node", "id": ulid(1), "revision": "2", "lifecycle_revision": "1"}]});
    assert!(client
        .prepared("graph.reverse", request(None, reverse.clone()))
        .is_ok());
    assert!(client.prepared(DESCRIPTOR, request(None, reverse)).is_err());
}

/// Event 1, then event 2's header with its operation rows replaced by
/// `ops` and its `op_count` set to `count`.
fn second_with(ops: &[Op], count: usize) -> Vec<RowOp> {
    let mut rows = event_rows(2, ops, Some(&"0".repeat(64)));
    if let Some(RowOp::Upsert { values, .. }) = rows.first_mut() {
        values.insert("op_count".into(), json!(count));
    }
    rows
}

#[test]
fn an_impossible_partial_group_is_corruption_not_lag() {
    let (first, second) = two_events();
    let mut past = second[0].clone();
    past.ordinal = 2;
    // Count 2 holding ordinal 2, which no arriving row can fix; a count past
    // the 128-operation cap; and, as the control, count 2 holding ordinal 0
    // only, which a later row can still complete.
    for (ops, count, corrupt) in [
        (vec![past], 2, true),
        (second.clone(), 129, true),
        (second.clone(), 2, false),
    ] {
        let dir = tempfile::tempdir().expect("dir");
        let source = open(dir.path());
        let mut rows = event_rows(1, &first, None);
        rows.extend(second_with(&ops, count));
        rows.push(head(2));
        source.replica.mutate(rows).expect("seeded");
        let read = prefix::complete(&source);
        if corrupt {
            assert!(
                matches!(read, Err(PrefixError::Corrupt { at: 2, .. })),
                "{count}: {read:?}"
            );
        } else {
            let prefix = read.expect("stale prefix");
            assert_eq!((prefix.n, prefix.is_complete()), (1, false));
        }
    }
}

/// A batch seeks every position on the `(graph, graph_seq)` index and the
/// operations on `(event_id, ordinal)`: it never scans the graph's history.
#[test]
fn a_prefix_batch_seeks_exact_positions() {
    let dir = tempfile::tempdir().expect("dir");
    let source = open(dir.path());
    let plan = source
        .replica
        .query(
            &format!("EXPLAIN QUERY PLAN {}", super::source::BATCH),
            &[Value::from(DB), Value::from("[\"1\",\"2\"]")],
        )
        .expect("plan");
    let steps: Vec<String> = plan
        .iter()
        .filter_map(|row| row.get("detail").and_then(Value::as_str).map(str::to_owned))
        .collect();
    assert!(
        steps
            .iter()
            .any(|s| s.contains("kg_events_graph_seq (graph=? AND graph_seq=?)")),
        "{steps:?}"
    );
    assert!(
        steps
            .iter()
            .any(|s| s.contains("kg_event_ops_event_ordinal (event_id=?)")),
        "{steps:?}"
    );
    assert!(
        !steps
            .iter()
            .any(|s| s.starts_with("SCAN kg_events") || s.starts_with("SCAN p")),
        "{steps:?}"
    );
}

/// A replica that accepts every command and cannot read its own rows: what
/// a send followed by a failing receipt read looks like.
struct Answering {
    inner: Arc<dyn crate::hq::port::Replica>,
}

mod answering {
    use super::Answering;
    use crate::hq::port::{
        BlobBody, CommandResult, Covered, Delivery, LinkState, Peer, RefusedCommit, Replica,
        ReplicaError, ReplicaState, RowOp, RowWrites, Subscription, UploadRefusal, WindowBase,
        WindowState,
    };
    use serde_json::{Map, Value};

    impl Replica for Answering {
        fn mutate(&self, ops: Vec<RowOp>) -> Result<String, ReplicaError> {
            self.inner.mutate(ops)
        }
        fn query(&self, _: &str, _: &[Value]) -> Result<Vec<Map<String, Value>>, ReplicaError> {
            Err(ReplicaError::Closed)
        }
        fn query_covered(
            &self,
            sql: &str,
            params: &[Value],
            base: &WindowBase,
            units: &[String],
        ) -> Result<Covered, ReplicaError> {
            self.inner.query_covered(sql, params, base, units)
        }
        fn subscribe(&self, subscription: Subscription) -> Result<(), ReplicaError> {
            self.inner.subscribe(subscription)
        }
        fn set_window(&self, base: &WindowBase, units: &[String]) -> Result<(), ReplicaError> {
            self.inner.set_window(base, units)
        }
        fn window_state(&self, base: &WindowBase) -> Result<WindowState, ReplicaError> {
            self.inner.window_state(base)
        }
        /// Presence stays withheld outside `hq` (`signals` guard): never
        /// forwarded.
        fn set_presence(&self, _: &str, _: Option<Value>) -> Result<(), ReplicaError> {
            Err(ReplicaError::NotSyncing)
        }
        fn presence(&self, scope_key: &str) -> Result<Vec<Peer>, ReplicaError> {
            self.inner.presence(scope_key)
        }
        fn commit_with_blobs(
            &self,
            blobs: Vec<BlobBody>,
            ops: Vec<RowOp>,
        ) -> Result<String, ReplicaError> {
            self.inner.commit_with_blobs(blobs, ops)
        }
        fn fetch_blob(&self, blob: &str) -> Result<Vec<u8>, ReplicaError> {
            self.inner.fetch_blob(blob)
        }
        fn cached_blob(&self, blob: &str) -> Result<Vec<u8>, ReplicaError> {
            self.inner.cached_blob(blob)
        }
        fn delivery(&self, commit_id: &str) -> Result<Delivery, ReplicaError> {
            self.inner.delivery(commit_id)
        }
        fn row_writes(
            &self,
            table: &str,
            row_id: &str,
            field: Option<&str>,
        ) -> Result<RowWrites, ReplicaError> {
            self.inner.row_writes(table, row_id, field)
        }
        fn refused_commits(&self, table: &str) -> Result<Vec<RefusedCommit>, ReplicaError> {
            self.inner.refused_commits(table)
        }
        fn granted(&self, table: &str, variable: &str) -> Result<Vec<String>, ReplicaError> {
            self.inner.granted(table, variable)
        }
        fn revoked_units(&self, table: &str) -> Result<Vec<String>, ReplicaError> {
            self.inner.revoked_units(table)
        }
        fn upload_refusals(&self) -> Vec<UploadRefusal> {
            self.inner.upload_refusals()
        }
        fn blob_cache_bytes(&self) -> Result<u64, ReplicaError> {
            self.inner.blob_cache_bytes()
        }
        fn mutate_superseding(
            &self,
            ops: Vec<RowOp>,
            earlier: Vec<String>,
        ) -> Result<String, ReplicaError> {
            self.inner.mutate_superseding(ops, earlier)
        }
        fn remote_query(&self, _: &str, _: Value) -> Result<Vec<Map<String, Value>>, ReplicaError> {
            Err(ReplicaError::Closed)
        }
        fn remote_command(
            &self,
            _: &str,
            request_id: &str,
            _: Value,
        ) -> Result<CommandResult, ReplicaError> {
            Ok(CommandResult {
                request_id: request_id.to_owned(),
                commit_seq: Some(41),
                cached: false,
            })
        }
        fn sync_now(&self) {}
        fn state(&self) -> ReplicaState {
            ReplicaState {
                link: LinkState::Connected,
                ..self.inner.state()
            }
        }
        fn watch(&self) -> tokio::sync::watch::Receiver<ReplicaState> {
            self.inner.watch()
        }
        fn close(&self) {
            self.inner.close();
        }
        fn join(&self) {
            self.inner.join();
        }
    }
}

/// The command was sent and answered; reading its outcome then fails. The
/// caller still holds the request, and the attempt, to ask again.
#[test]
fn a_sent_change_keeps_its_request_when_its_outcome_cannot_be_read() {
    let dir = tempfile::tempdir().expect("dir");
    let local = open(dir.path());
    let source = Source {
        replica: Arc::new(Answering {
            inner: Arc::clone(&local.replica),
        }),
        ..local.clone()
    };
    let client = GraphClient::new(source);
    let change = client.change(vec![create(&ulid(1))], None).expect("change");
    let applied = client.apply(&change).expect("a request id was minted");
    assert!(applied.outcome.is_err(), "{:?}", applied.outcome);
    let attempt = applied.attempt.as_ref().expect("the attempt");
    assert_eq!(
        attempt.result().as_ref().map(|r| r.commit_seq),
        Ok(Some(41))
    );
    assert_eq!(attempt.key(), applied.prepared.key());
    assert_eq!(
        attempt.key().request_id(),
        applied.prepared.request().request_id
    );
}

/// The query-agnostic helpers keeper-hq's replica uses are the one
/// implementation: over the same replica they answer exactly what the
/// `Source` port does, a failing query fails them, and an answer that is
/// not a batch is unreadable, never an empty prefix.
#[test]
fn the_source_helpers_answer_over_any_query_channel() {
    use super::source::{checkpoint_with, prefix_batch_with, GraphSource, ScopeState, SourceError};
    let dir = tempfile::tempdir().expect("dir");
    let source = open(dir.path());
    let (first, second) = two_events();
    let mut rows = event_rows(1, &first, None);
    rows.extend(event_rows(2, &second, None));
    rows.push(head(2));
    source.replica.mutate(rows).expect("seeded");
    let replica = source.replica.clone();
    let query = |sql: &str, params: &[Value]| Ok(replica.query(sql, params)?);
    for (after, limit) in [(0, 1), (0, 2), (1, 5), (2, 1)] {
        let via = prefix_batch_with(DB, after, limit, query).expect("batch");
        assert_eq!(via, source.prefix_batch(after, limit).expect("port"));
    }
    let both = prefix_batch_with(DB, 0, 10, query).expect("batch");
    assert_eq!(
        (both.head, both.events.len(), both.ops.len()),
        (Some(2), 2, first.len() + second.len())
    );
    assert_eq!(
        checkpoint_with(DB, query).expect("checkpoint"),
        source.checkpoint().expect("port")
    );
    // No graph subscription has pulled on this local replica.
    assert_eq!(
        checkpoint_with(DB, query).expect("checkpoint").scope,
        ScopeState::Pending
    );

    let failing = |_: &str, _: &[Value]| -> Result<Vec<Map<String, Value>>, SourceError> {
        Err(SourceError::Unreadable("channel".into()))
    };
    assert!(prefix_batch_with(DB, 0, 1, failing).is_err());
    assert!(checkpoint_with(DB, failing).is_err());
    let nothing =
        |_: &str, _: &[Value]| -> Result<Vec<Map<String, Value>>, SourceError> { Ok(Vec::new()) };
    assert_eq!(
        prefix_batch_with(DB, 0, 1, nothing),
        Err(SourceError::Unreadable("no batch".into()))
    );
    let bad_head = |_: &str, _: &[Value]| -> Result<Vec<Map<String, Value>>, SourceError> {
        Ok(vec![json!({"batch": {"head": 7, "events": [], "ops": []}})
            .as_object()
            .expect("row")
            .clone()])
    };
    assert_eq!(
        prefix_batch_with(DB, 0, 1, bad_head),
        Err(SourceError::Unreadable("head".into()))
    );
}

/// CC-8 (112.1): a server's scope revocation is purged by the Syncular
/// client itself (`purge_scope_rows`, in the transaction that records the
/// subscription `revoked`), and that purge fails closed — the revocation is
/// not applied — for a table whose schema maps no column to the revoked
/// scope variable. Every subscription the graph takes (each graph table
/// under `kg_graph`, 111.x's `kg_contents`/`kg_reviews` among them, the
/// receipts under `kg_client` and the proposals under `kg_proposal`)
/// therefore needs that mapping in the contract's schema, or a revocation
/// would leave its rows; and the purge check reads exactly those tables.
#[test]
fn every_graph_subscription_has_a_purgeable_scope_column() {
    let schema = syncular_client::parse_schema_json(
        crate::hq::syncular::schema().expect("the contract's schema"),
    )
    .expect("compiles");
    let taken = super::client::subscriptions(DB, "ana", "dev1");
    let tables: Vec<&str> = taken.iter().map(|s| s.table.as_str()).collect();
    assert_eq!(tables, super::client::subscribed_tables());
    assert_eq!(
        taken.len(),
        11,
        "nine graph tables, the receipts, the proposals"
    );
    for subscription in taken {
        let table = schema
            .table(&subscription.table)
            .unwrap_or_else(|| panic!("{} is in the schema", subscription.table));
        for (variable, _) in &subscription.scopes {
            assert!(
                table.scope_column(variable).is_some(),
                "{} maps no column to {variable}: a revocation could not purge it",
                subscription.table
            );
        }
    }
}
