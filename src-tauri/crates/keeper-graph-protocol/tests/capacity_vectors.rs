//! `syncular/limits.json` and `syncular/graph-capacity.vectors.json` hold for
//! this implementation: the three domain-capacity limits equal the worker's
//! constants, every pinned frame reproduces from `EventFrame::encode()`, the
//! closed-form frame-line length agrees with the encoder across every frame
//! and history event, and every history's digests, cumulative counts and
//! entity invariants match.

use std::collections::BTreeSet;
use std::fmt::Write as _;

use keeper_graph_protocol::delta;
use keeper_graph_protocol::vocabulary::{EntityType, Operation};
use keeper_graph_protocol::worker::{
    EventFrame, FrameOp, MAX_ENTITIES, MAX_EVENTS, MAX_EXPORT_BYTES,
};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};

const LIMITS: &str = include_str!("../../../../syncular/limits.json");
const VECTORS: &str = include_str!("../../../../syncular/graph-capacity.vectors.json");

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Limits {
    graph_max_events: u64,
    graph_max_entities: u64,
    graph_export_max_bytes: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct CapacityVectors {
    frames: Vec<FrameVector>,
    histories: Vec<HistoryVector>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrameVector {
    id: String,
    graph_seq: u64,
    event_id: String,
    delta_digest: String,
    ops: Vec<FrameOp>,
    line_bytes: u64,
    line_sha256: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryVector {
    id: String,
    events: Vec<HistoryEvent>,
    steps: Vec<HistoryStep>,
    worker_entities: u64,
    consumer_entities: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryEvent {
    event_id: String,
    delta_digest: String,
    descriptor: String,
    ops: Vec<delta::Op>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct HistoryStep {
    graph_seq: u64,
    creates: u64,
    review_creates: u64,
    removes: u64,
    export_bytes: u64,
}

fn vectors() -> CapacityVectors {
    serde_json::from_str(VECTORS).expect("capacity vector JSON")
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity(64);
    for b in digest {
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

fn digits(mut n: u64) -> usize {
    if n == 0 {
        return 1;
    }
    let mut count = 0;
    while n > 0 {
        count += 1;
        n /= 10;
    }
    count
}

fn closed_form_op_bytes(
    ordinal: u32,
    entity_type: EntityType,
    operation: Operation,
    after_revision: Option<&str>,
    after_value: Option<&Value>,
) -> usize {
    let entity_type_len = match entity_type {
        EntityType::Node => "node".len(),
        EntityType::Edge => "edge".len(),
        EntityType::Tag => "tag".len(),
        EntityType::NodeTag => "node_tag".len(),
        EntityType::Content => "content".len(),
        EntityType::Review => "review".len(),
    };
    let operation_len = match operation {
        Operation::Create => "create".len(),
        Operation::Update => "update".len(),
        Operation::Archive => "archive".len(),
        Operation::Unarchive => "unarchive".len(),
        Operation::Remove => "remove".len(),
    };
    let rev_len = match after_revision {
        None => 4,
        Some(rev) => rev.len() + 2,
    };
    let val_len = match after_value {
        None => 4,
        Some(val) => serde_jcs::to_vec(val).expect("jcs after_value").len(),
    };
    118 + digits(u64::from(ordinal)) + entity_type_len + operation_len + rev_len + val_len
}

fn closed_form_line_bytes_frame(graph_seq: u64, ops: &[FrameOp]) -> usize {
    let n = ops.len();
    assert!(n > 0, "frame must contain at least one operation");
    let ops_sum: usize = ops
        .iter()
        .map(|op| {
            closed_form_op_bytes(
                op.ordinal,
                op.entity_type,
                op.operation,
                op.after_revision.as_deref(),
                op.after_value.as_ref(),
            )
        })
        .sum();
    157 + digits(graph_seq) + digits(n as u64) + (n - 1) + ops_sum + 1
}

fn closed_form_line_bytes_delta(graph_seq: u64, ops: &[delta::Op]) -> usize {
    let n = ops.len();
    assert!(n > 0, "event must contain at least one operation");
    let ops_sum: usize = ops
        .iter()
        .map(|op| {
            closed_form_op_bytes(
                op.ordinal,
                op.entity_type,
                op.operation,
                op.after_revision.as_deref(),
                op.after_value.as_ref(),
            )
        })
        .sum();
    157 + digits(graph_seq) + digits(n as u64) + (n - 1) + ops_sum + 1
}

fn value_has_non_ascii(value: &Value) -> bool {
    match value {
        Value::String(s) => !s.is_ascii(),
        Value::Array(items) => items.iter().any(value_has_non_ascii),
        Value::Object(map) => map
            .iter()
            .any(|(k, v)| !k.is_ascii() || value_has_non_ascii(v)),
        _ => false,
    }
}

fn value_has_keys_10_and_2(value: &Value) -> bool {
    match value {
        Value::Object(map) => {
            (map.contains_key("10") && map.contains_key("2"))
                || map.values().any(value_has_keys_10_and_2)
        }
        Value::Array(items) => items.iter().any(value_has_keys_10_and_2),
        _ => false,
    }
}

fn value_has_number(value: &Value, target: f64) -> bool {
    match value {
        Value::Number(n) => n.as_f64() == Some(target),
        Value::Array(items) => items.iter().any(|v| value_has_number(v, target)),
        Value::Object(map) => map.values().any(|v| value_has_number(v, target)),
        _ => false,
    }
}

#[test]
fn limits_json_equals_the_worker_constants() {
    let limits: Limits = serde_json::from_str(LIMITS).expect("limits JSON");
    assert_eq!(limits.graph_max_events, MAX_EVENTS);
    assert_eq!(limits.graph_max_entities, MAX_ENTITIES);
    assert_eq!(limits.graph_export_max_bytes, MAX_EXPORT_BYTES);
}

#[test]
fn frames_reproduce_from_the_encoder() {
    let corpus = vectors();
    assert!(!corpus.frames.is_empty(), "frames must not be empty");
    let mut mismatches = Vec::new();
    for entry in &corpus.frames {
        let frame = EventFrame {
            graph_seq: entry.graph_seq,
            event_id: entry.event_id.clone(),
            op_count: u32::try_from(entry.ops.len()).expect("op_count fits u32"),
            delta_digest: entry.delta_digest.clone(),
            ops: entry.ops.clone(),
        };
        let encoded = frame.encode().expect("encode frame");
        let got_bytes = encoded.len() as u64;
        let got_sha256 = sha256_hex(&encoded);
        if got_bytes != entry.line_bytes || got_sha256 != entry.line_sha256 {
            mismatches.push(format!(
                "{}: bytes got={} want={}, sha256 got={} want={}",
                entry.id, got_bytes, entry.line_bytes, got_sha256, entry.line_sha256
            ));
        }
    }
    assert!(
        mismatches.is_empty(),
        "frame encoding mismatches:\n{}",
        mismatches.join("\n")
    );
}

#[test]
fn the_closed_form_equals_the_encoder() {
    let corpus = vectors();
    for entry in &corpus.frames {
        let frame = EventFrame {
            graph_seq: entry.graph_seq,
            event_id: entry.event_id.clone(),
            op_count: u32::try_from(entry.ops.len()).expect("op_count fits u32"),
            delta_digest: entry.delta_digest.clone(),
            ops: entry.ops.clone(),
        };
        let encoded = frame.encode().expect("encode frame");
        let closed = closed_form_line_bytes_frame(entry.graph_seq, &entry.ops);
        assert_eq!(closed, encoded.len(), "frame {}", entry.id);
    }
    for history in &corpus.histories {
        for (idx, event) in history.events.iter().enumerate() {
            let seq = (idx as u64) + 1;
            let encoded = EventFrame::new(seq, &event.event_id, &event.delta_digest, &event.ops)
                .encode()
                .expect("encode history frame");
            let closed = closed_form_line_bytes_delta(seq, &event.ops);
            assert_eq!(closed, encoded.len(), "history {} seq {seq}", history.id);
        }
    }
}

#[test]
fn histories_steps_and_invariants() {
    let corpus = vectors();
    assert!(!corpus.histories.is_empty(), "histories must not be empty");
    for history in &corpus.histories {
        assert!(
            !history.events.is_empty(),
            "history {} must have at least one event",
            history.id
        );
        assert_eq!(
            history.events.len(),
            history.steps.len(),
            "history {} events/steps length mismatch",
            history.id
        );
        let mut creates: u64 = 0;
        let mut review_creates: u64 = 0;
        let mut removes: u64 = 0;
        let mut export_bytes: u64 = 0;
        for (idx, (event, step)) in history.events.iter().zip(&history.steps).enumerate() {
            let expected_seq = (idx as u64) + 1;
            assert_eq!(
                step.graph_seq, expected_seq,
                "history {} step {idx} graph_seq",
                history.id
            );
            let got_digest = delta::digest(&event.ops).expect("delta digest");
            assert_eq!(
                got_digest, event.delta_digest,
                "history {} seq {expected_seq} delta_digest",
                history.id
            );
            for op in &event.ops {
                if op.operation == Operation::Create {
                    creates += 1;
                    if op.entity_type == EntityType::Review {
                        review_creates += 1;
                    }
                }
                if op.operation == Operation::Remove {
                    removes += 1;
                }
            }
            let encoded = EventFrame::new(
                expected_seq,
                &event.event_id,
                &event.delta_digest,
                &event.ops,
            )
            .encode()
            .expect("encode history frame");
            export_bytes += encoded.len() as u64;
            assert_eq!(
                step.creates, creates,
                "history {} seq {expected_seq} creates",
                history.id
            );
            assert_eq!(
                step.review_creates, review_creates,
                "history {} seq {expected_seq} review_creates",
                history.id
            );
            assert_eq!(
                step.removes, removes,
                "history {} seq {expected_seq} removes",
                history.id
            );
            assert_eq!(
                step.export_bytes, export_bytes,
                "history {} seq {expected_seq} export_bytes",
                history.id
            );
        }
        assert_eq!(
            history.worker_entities,
            creates - review_creates,
            "history {} worker_entities",
            history.id
        );
        assert_eq!(
            history.consumer_entities,
            creates - removes,
            "history {} consumer_entities",
            history.id
        );
    }
}

#[test]
fn the_vector_covers_its_cases() {
    let corpus = vectors();
    let seqs: BTreeSet<u64> = corpus.frames.iter().map(|f| f.graph_seq).collect();
    for required in [9_u64, 10, 99_999, 100_000] {
        assert!(
            seqs.contains(&required),
            "missing frame graph_seq {required}"
        );
    }
    assert!(
        corpus.frames.iter().any(|f| f.ops.len() == 128),
        "missing 128-op frame"
    );
    assert!(
        corpus.frames.iter().any(|f| f
            .ops
            .iter()
            .any(|op| op.after_revision.is_none() && op.after_value.is_none())),
        "missing frame op with null after_revision and null after_value"
    );
    assert!(
        corpus.frames.iter().any(|f| f
            .ops
            .iter()
            .any(|op| op.after_value.as_ref().is_some_and(value_has_non_ascii))),
        "missing frame op with non-ASCII string"
    );
    assert!(
        corpus.frames.iter().any(|f| f
            .ops
            .iter()
            .any(|op| op.after_value.as_ref().is_some_and(value_has_keys_10_and_2))),
        "missing frame op with object keys \"10\" and \"2\""
    );
    for target in [1e21_f64, 1e-7_f64, 0.1_f64] {
        assert!(
            corpus.frames.iter().any(|f| f.ops.iter().any(|op| op
                .after_value
                .as_ref()
                .is_some_and(|v| value_has_number(v, target)))),
            "missing frame op with number {target}"
        );
    }

    assert!(
        corpus.histories.iter().any(|h| h
            .events
            .iter()
            .any(|e| e.ops.iter().any(
                |op| op.entity_type == EntityType::Review && op.operation == Operation::Create
            ))),
        "missing history review create"
    );
    assert!(
        corpus.histories.iter().any(|h| h
            .events
            .iter()
            .any(|e| e.ops.iter().any(|op| op.operation == Operation::Remove))),
        "missing history remove"
    );
    assert!(
        corpus
            .histories
            .iter()
            .any(|h| h.events.iter().any(|e| e.descriptor == "graph.acknowledge")),
        "missing history descriptor graph.acknowledge"
    );
    assert!(
        corpus
            .histories
            .iter()
            .any(|h| h.events.iter().any(|e| e.descriptor == "graph.reverse")),
        "missing history descriptor graph.reverse"
    );
}
