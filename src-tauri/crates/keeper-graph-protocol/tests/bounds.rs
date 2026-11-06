//! Bounds the vectors cannot hold: containment over a parent with up to a
//! million incoming `contains` edges (a lazily generated graph), and the
//! native decoders refusing images and history pages outside the contract.

use std::cell::Cell;

use keeper_graph_protocol::candidate::{apply, State, Verdict, MAX_CONTAINMENT_EDGES};
use keeper_graph_protocol::change::{Change, ChangeOp};
use keeper_graph_protocol::wire::{decode_edge, decode_history, decode_node};
use keeper_graph_protocol::Refusal;
use serde_json::{json, Map, Value};

const P: &str = "01JG0000000000000000000000";
const X: &str = "01JG0000000000000000000001";

fn id(prefix: &str, n: usize) -> String {
    format!("{prefix}{n:022}")
}

/// `P`, a Collection with `parents` parent collections, each containing it
/// and contained by nothing. Records the largest limit asked for and the
/// most edges returned at once.
struct Wide {
    parents: usize,
    asked: Cell<usize>,
    returned: Cell<usize>,
}

impl State for Wide {
    fn node(&self, node: &str) -> Option<Map<String, Value>> {
        (node == P).then(|| {
            json!({"id": P, "kind": "Collection", "title": "P", "description": null,
                   "description_source_id": null, "properties": {}, "lifecycle": "active",
                   "merged_into_id": null, "revision": "1", "lifecycle_revision": "1"})
            .as_object()
            .cloned()
            .unwrap_or_default()
        })
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
    fn contains_into(&self, to_ids: &[String], limit: usize) -> Vec<Map<String, Value>> {
        self.asked.set(self.asked.get().max(limit));
        if !to_ids.iter().any(|to| to == P) {
            return Vec::new();
        }
        let edges: Vec<_> = (0..self.parents.min(limit))
            .map(|n| {
                json!({"id": id("01JH", n), "from_id": id("01JK", n), "to_id": P,
                       "type": "contains", "properties": {}, "lifecycle": "active",
                       "revision": "1"})
                .as_object()
                .cloned()
                .unwrap_or_default()
            })
            .collect();
        self.returned.set(self.returned.get().max(edges.len()));
        edges
    }
}

fn contain_new_child(parents: usize) -> (Verdict, usize, usize) {
    let change = Change::new(
        vec![
            ChangeOp::CreateNode {
                id: X.into(),
                kind: "Concept".into(),
                title: "X".into(),
                description: None,
                properties: Map::new(),
            },
            ChangeOp::CreateEdge {
                id: "01JG0000000000000000000002".into(),
                from_id: P.into(),
                to_id: X.into(),
                edge_type: "contains".into(),
                properties: Map::new(),
                from_lifecycle_revision: Some("1".into()),
                to_lifecycle_revision: None,
            },
        ],
        None,
        None,
        |tag| Some(tag.to_owned()),
    )
    .expect("change");
    let state = Wide {
        parents,
        asked: Cell::new(0),
        returned: Cell::new(0),
    };
    let verdict = apply(&state, &change, &mut || unreachable!("nothing minted"));
    (verdict, state.asked.get(), state.returned.get())
}

#[test]
fn containment_reads_at_most_one_edge_past_its_budget() {
    let (verdict, asked, _) = contain_new_child(MAX_CONTAINMENT_EDGES);
    assert!(matches!(verdict, Verdict::Accepted(_)), "{verdict:?}");
    assert_eq!(asked, MAX_CONTAINMENT_EDGES + 1);
    for parents in [MAX_CONTAINMENT_EDGES + 1, 1_000_000] {
        let (verdict, asked, returned) = contain_new_child(parents);
        assert_eq!(verdict, Verdict::Refused(Refusal::ChangeLimit), "{parents}");
        assert_eq!(
            (asked, returned),
            (MAX_CONTAINMENT_EDGES + 1, MAX_CONTAINMENT_EDGES + 1)
        );
    }
}

fn node_image() -> Map<String, Value> {
    json!({"id": P, "kind": "Concept", "title": "T", "description": null,
           "description_source_id": null, "properties": {"a": 1}, "lifecycle": "active",
           "merged_into_id": null, "revision": "2", "lifecycle_revision": "1", "tags": ["a", "b"]})
    .as_object()
    .cloned()
    .unwrap_or_default()
}

#[test]
fn an_inspected_node_outside_the_node_rules_is_refused() {
    assert!(decode_node(P, vec![node_image()]).is_ok_and(|v| v.is_some()));
    let broken: [(&str, Value); 10] = [
        ("title", json!(42)),
        ("title", json!("")),
        ("lifecycle", json!("unknown")),
        ("revision", json!("0")),
        ("lifecycle_revision", json!("0")),
        ("kind", json!("Tag")),
        ("description", json!(1)),
        ("merged_into_id", json!("not-a-ulid")),
        ("properties", json!({"title": "shadow"})),
        ("tags", json!(["b", "a"])),
    ];
    for (field, value) in broken {
        let mut row = node_image();
        row.insert(field.into(), value);
        assert_eq!(
            decode_node(P, vec![row]),
            Err(Refusal::UnsupportedVersion),
            "{field}"
        );
    }
}

#[test]
fn an_inspected_edge_outside_the_edge_rules_is_refused() {
    let edge = || {
        json!({"id": X, "from_id": P, "to_id": P, "type": "test:rel", "properties": {},
               "lifecycle": "archived", "revision": "3"})
        .as_object()
        .cloned()
        .unwrap_or_default()
    };
    assert!(decode_edge(X, vec![edge()]).is_ok_and(|v| v.is_some()));
    for (field, value) in [
        ("from_id", json!("x")),
        ("type", json!("HAS_TAG")),
        ("revision", json!("0")),
        ("lifecycle", json!("gone")),
    ] {
        let mut row = edge();
        row.insert(field.into(), value);
        assert_eq!(
            decode_edge(X, vec![row]),
            Err(Refusal::UnsupportedVersion),
            "{field}"
        );
    }
}

fn page(scanned_through: &str, events: usize) -> Map<String, Value> {
    let event = json!({"event": {}, "ops": []});
    json!({"events": vec![event; events], "scanned_through": scanned_through})
        .as_object()
        .cloned()
        .unwrap_or_default()
}

#[test]
fn a_history_page_outside_its_bounds_is_refused_before_its_events_are_read() {
    // An empty page may scan up to 1,000 positions, not one more.
    assert!(decode_history("5", 10, vec![page("1005", 0)]).is_ok());
    assert_eq!(
        decode_history("5", 10, vec![page("1006", 0)]),
        Err(Refusal::UnsupportedVersion)
    );
    // More events than asked for refuse before any is decoded (these are
    // not even events).
    assert_eq!(
        decode_history("0", 2, vec![page("10", 3)]),
        Err(Refusal::UnsupportedVersion)
    );
    assert_eq!(
        decode_history("9", 2, vec![page("8", 0)]),
        Err(Refusal::UnsupportedVersion)
    );
}

/// One whole valid event at position 1 whose header's `origin_chain` holds
/// `pad` bytes of text, and its `ops` repeated `copies` times (renumbered).
fn event(pad: usize, copies: usize) -> Value {
    event_at(1, pad, copies)
}

/// [`event`] at graph position `seq`.
fn event_at(seq: u64, pad: usize, copies: usize) -> Value {
    use keeper_graph_protocol::delta::{digest, Op};
    use keeper_graph_protocol::vocabulary::{EntityType, Operation};
    let ops: Vec<Op> = (0..copies)
        .map(|n| Op {
            ordinal: u32::try_from(n).unwrap_or(u32::MAX),
            entity_type: EntityType::Node,
            entity_id: X.into(),
            operation: Operation::Create,
            before_revision: None,
            after_revision: Some("1".into()),
            before_value: None,
            after_value: Some(json!({"title": "x"})),
        })
        .collect();
    let delta = digest(&ops[..ops.len().min(128)]).unwrap_or_default();
    json!({
        "event": {
            "id": P, "graph_seq": seq.to_string(), "descriptor": "graph.apply", "request_id": X,
            "request_digest": "0".repeat(64), "actor_id": "ana", "client_id": "dev1",
            "origin_class": "agent", "origin_actor": "ana",
            "origin_chain": [{"session_id": "x".repeat(pad), "agent_actor": "nixi"}],
            "context_id": null, "approval_basis": {"basis": "direct"},
            "reverses_event_id": null, "event_time_ms": null, "recorded_at_ms": 1,
            "op_count": copies, "delta_digest": delta,
        },
        "ops": ops,
    })
}

fn header_bytes(event: &Value) -> usize {
    keeper_graph_protocol::value::canonical(&event["event"])
        .map(|b| b.len())
        .unwrap_or(0)
}

fn one_event_page(event: Value) -> Vec<Map<String, Value>> {
    vec![json!({"events": [event], "scanned_through": "1"})
        .as_object()
        .cloned()
        .unwrap_or_default()]
}

#[test]
fn the_first_event_of_a_page_is_held_to_its_row_and_group_bounds() {
    use keeper_graph_protocol::wire::MAX_EVENT_HEADER_BYTES;
    // A header of exactly 128 KiB is read; one byte more is refused, first
    // event or not.
    let base = header_bytes(&event(0, 1));
    let at = event(MAX_EVENT_HEADER_BYTES - base, 1);
    assert_eq!(header_bytes(&at), MAX_EVENT_HEADER_BYTES);
    assert!(decode_history("0", 1, one_event_page(at)).is_ok());
    let over = event(MAX_EVENT_HEADER_BYTES - base + 1, 1);
    assert_eq!(header_bytes(&over), MAX_EVENT_HEADER_BYTES + 1);
    assert_eq!(
        decode_history("0", 1, one_event_page(over)),
        Err(Refusal::UnsupportedVersion)
    );
    // The reviewer's 5 MB header.
    assert_eq!(
        decode_history("0", 1, one_event_page(event(5 * 1024 * 1024, 1))),
        Err(Refusal::UnsupportedVersion)
    );
    // 128 operations are a group; 129 refuse before any is decoded.
    assert!(decode_history("0", 1, one_event_page(event(0, 128))).is_ok());
    assert_eq!(
        decode_history("0", 1, one_event_page(event(0, 129))),
        Err(Refusal::UnsupportedVersion)
    );
}

fn entry_bytes(event: &Value) -> usize {
    keeper_graph_protocol::value::canonical(event)
        .map(|b| b.len())
        .unwrap_or(0)
}

/// A page whose events after the first total exactly 4 MiB plus `over`
/// bytes, each counted as the server counts it: the canonical bytes of the
/// whole `{"event":…,"ops":…}` entry.
fn page_over(over: usize) -> (Vec<Map<String, Value>>, usize) {
    use keeper_graph_protocol::wire::MAX_HISTORY_PAGE_BYTES;
    let mut events = vec![event_at(1, 0, 1)];
    let (mut total, mut seq) = (0usize, 2u64);
    loop {
        let full = event_at(seq, 120_000, 1);
        let size = entry_bytes(&full);
        // Leave room for a last event of at least its bare size.
        if total + size + entry_bytes(&event_at(seq + 1, 0, 1)) + over > MAX_HISTORY_PAGE_BYTES {
            break;
        }
        total += size;
        events.push(full);
        seq += 1;
    }
    let bare = entry_bytes(&event_at(seq, 0, 1));
    let last = event_at(seq, MAX_HISTORY_PAGE_BYTES - total - bare + over, 1);
    total += entry_bytes(&last);
    events.push(last);
    let row = json!({"events": events, "scanned_through": seq.to_string()});
    (vec![row.as_object().cloned().unwrap_or_default()], total)
}

#[test]
fn the_page_budget_counts_each_whole_entry_after_the_first() {
    use keeper_graph_protocol::wire::MAX_HISTORY_PAGE_BYTES;
    let (page, total) = page_over(0);
    assert_eq!(total, MAX_HISTORY_PAGE_BYTES);
    assert!(decode_history("0", 100, page).is_ok());
    // One byte over, and 34 over: the two 17-byte entry wrappers a count of
    // header and operations alone would miss.
    for over in [1, 34] {
        let (page, total) = page_over(over);
        assert_eq!(total, MAX_HISTORY_PAGE_BYTES + over);
        assert_eq!(
            decode_history("0", 100, page),
            Err(Refusal::UnsupportedVersion),
            "{over}"
        );
    }
}
