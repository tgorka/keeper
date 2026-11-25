//! `syncular/knowledge-graph.vectors.json`'s `file_refs` and
//! `file_ref_admissions` lists hold for this implementation.

use std::collections::HashMap;

use keeper_graph_protocol::candidate::{
    apply, apply_inverse, Effects, Retained, Stamp, State, Verdict,
};
use keeper_graph_protocol::change;
use keeper_graph_protocol::file_ref::{self, EntryKind, Row, Write};
use keeper_graph_protocol::inverse;
use keeper_graph_protocol::tag;
use keeper_graph_protocol::value::typed_ref;
use keeper_graph_protocol::vocabulary::{EntityType, Operation, OriginClass};
use keeper_graph_protocol::wire::{Event, EventHeader};
use keeper_graph_protocol::Refusal;
use serde_json::{json, Map, Value};

const CORPUS: &str = include_str!("../../../../syncular/knowledge-graph.vectors.json");

fn entries(key: &str) -> Vec<Value> {
    let corpus: Value = serde_json::from_str(CORPUS).expect("vector JSON");
    let entries = corpus[key].as_array().expect(key).clone();
    assert!(!entries.is_empty(), "{key}");
    entries
}

fn id(entry: &Value) -> &str {
    entry["id"].as_str().expect("id")
}

#[derive(Clone, PartialEq)]
struct Images {
    tables: Value,
    written_at: HashMap<String, u64>,
}

impl Images {
    fn new(state: Value) -> Self {
        Self {
            tables: state,
            written_at: HashMap::new(),
        }
    }

    fn rows(&self, table: &str) -> impl Iterator<Item = &Map<String, Value>> {
        self.tables[table]
            .as_array()
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .map(|r| r.as_object().expect("image"))
    }

    fn select(
        &self,
        table: &str,
        limit: usize,
        keep: impl Fn(&Map<String, Value>) -> bool,
    ) -> Vec<Map<String, Value>> {
        let mut rows: Vec<_> = self.rows(table).filter(|r| keep(r)).cloned().collect();
        rows.sort_by(|a, b| a["id"].as_str().cmp(&b["id"].as_str()));
        rows.truncate(limit);
        rows
    }

    fn find(&self, table: &str, key: &str, value: &str) -> Option<Map<String, Value>> {
        self.rows(table).find(|r| r[key] == value).cloned()
    }

    fn upsert(&mut self, table: &str, key: &str, image: Value) {
        let rows = self.tables[table].as_array_mut().expect(table);
        let target = image[key].as_str().expect(key).to_owned();
        if let Some(slot) = rows.iter_mut().find(|r| r[key] == target) {
            *slot = image;
        } else {
            rows.push(image);
        }
    }

    fn remove(&mut self, table: &str, key: &str, value: &str) {
        let rows = self.tables[table].as_array_mut().expect(table);
        rows.retain(|r| r[key] != value);
    }

    fn apply_effects(&mut self, effects: &Effects, seq: u64) {
        for op in &effects.ops {
            match op.entity_type {
                EntityType::Node => {
                    self.upsert("nodes", "id", op.after_value.clone().expect("node after"));
                }
                EntityType::Edge => {
                    self.upsert("edges", "id", op.after_value.clone().expect("edge after"));
                }
                EntityType::Content => {
                    self.upsert(
                        "contents",
                        "id",
                        op.after_value.clone().expect("content after"),
                    );
                }
                EntityType::Tag => {
                    self.upsert("tags", "id", op.after_value.clone().expect("tag after"));
                }
                EntityType::NodeTag => match op.operation {
                    Operation::Create => {
                        self.tables["node_tags"]
                            .as_array_mut()
                            .expect("node_tags")
                            .push(op.after_value.clone().expect("node_tag after"));
                    }
                    Operation::Remove => {
                        self.remove("node_tags", "id", &op.entity_id);
                    }
                    other => panic!("unexpected node_tag op {other:?}"),
                },
                EntityType::Review => {}
            }
            self.written_at.insert(op.entity_id.clone(), seq);
        }
        for write in &effects.file_refs {
            match write {
                Write::Upsert(row) => {
                    self.upsert(
                        "file_refs",
                        "entry_id",
                        json!({
                            "entry_id": row.entry_id,
                            "graph": "tg",
                            "node_id": row.node_id,
                            "node_kind": row.node_kind,
                        }),
                    );
                }
                Write::Delete { entry_id } => {
                    self.remove("file_refs", "entry_id", entry_id);
                }
            }
        }
    }

    fn projected_file_refs(&self) -> Vec<Row> {
        let mut rows: Vec<Row> = self
            .rows("file_refs")
            .map(|r| Row::of(r).expect("file_ref row"))
            .collect();
        rows.sort_by(|a, b| a.entry_id.cmp(&b.entry_id));
        rows
    }
}

fn refers(value: &Value, id: &str) -> bool {
    if let Ok(Some(r)) = typed_ref(value) {
        return r.id == id;
    }
    match value {
        Value::Array(items) => items.iter().any(|v| refers(v, id)),
        Value::Object(members) => members.values().any(|v| refers(v, id)),
        _ => false,
    }
}

impl State for Images {
    fn node(&self, id: &str) -> Option<Map<String, Value>> {
        self.find("nodes", "id", id)
    }
    fn edge(&self, id: &str) -> Option<Map<String, Value>> {
        self.find("edges", "id", id)
    }
    fn tag(&self, text: &str) -> Option<Map<String, Value>> {
        self.find("tags", "text", text)
    }
    fn membership(&self, node_id: &str, tag_id: &str) -> Option<Map<String, Value>> {
        self.rows("node_tags")
            .find(|r| r["node_id"] == node_id && r["tag_id"] == tag_id)
            .cloned()
    }
    fn contains_into(&self, to_ids: &[String], limit: usize) -> Vec<Map<String, Value>> {
        self.select("edges", limit, |r| {
            r["type"] == "contains"
                && r["lifecycle"] == "active"
                && to_ids.iter().any(|t| r["to_id"] == t.as_str())
        })
    }
    fn content(&self, id: &str) -> Option<Map<String, Value>> {
        self.find("contents", "id", id)
    }
    fn contents_of(&self, owner_id: &str, limit: usize) -> Vec<Map<String, Value>> {
        self.select("contents", limit, |r| r["owner_node_id"] == owner_id)
    }
    fn sources_from(&self, from_id: &str, limit: usize) -> Vec<Map<String, Value>> {
        self.select("edges", limit, |r| {
            r["type"] == "sources" && r["lifecycle"] == "active" && r["from_id"] == from_id
        })
    }
    fn incident_active(&self, node_ids: &[String], limit: usize) -> Vec<Map<String, Value>> {
        self.select("edges", limit, |r| {
            r["lifecycle"] == "active"
                && node_ids
                    .iter()
                    .any(|i| r["from_id"] == i.as_str() || r["to_id"] == i.as_str())
        })
    }
    fn merged_into(&self, node_ids: &[String], limit: usize) -> Vec<Map<String, Value>> {
        self.select("nodes", limit, |r| {
            node_ids.iter().any(|i| r["merged_into_id"] == i.as_str())
        })
    }
    fn file_ref(&self, entry_id: &str) -> Option<Map<String, Value>> {
        self.find("file_refs", "entry_id", entry_id)
    }
}

impl Retained for Images {
    fn written_at(&self, id: &str) -> Option<u64> {
        self.written_at.get(id).copied()
    }
    fn referrers(&self, node_id: &str, limit: usize) -> Vec<String> {
        let mut out: Vec<String> = self
            .rows("nodes")
            .chain(self.rows("edges"))
            .filter(|r| refers(&r["properties"], node_id))
            .chain(
                self.rows("contents")
                    .filter(|c| refers(&c["body"], node_id)),
            )
            .map(|r| r["id"].as_str().expect("id").to_owned())
            .collect();
        out.sort();
        out.truncate(limit);
        out
    }
}

fn run_scenario(entry: &Value) -> Images {
    let name = id(entry);
    let mut images = Images::new(entry["state"].clone());
    let stamp = Stamp {
        time_ms: 1,
        actor_id: "person-editor",
    };
    let mut mint_counter: u64 = 0;
    let mut mint = || {
        mint_counter += 1;
        format!("01JY{mint_counter:022}")
    };
    let mut accepted: HashMap<usize, Effects> = HashMap::new();

    for (step_idx, step) in entry["steps"].as_array().expect("steps").iter().enumerate() {
        let before = images.clone();
        let verdict = if let Some(payload) = step.get("payload") {
            match change::parse(payload, tag::normalise) {
                Ok(parsed) => apply(&images, &parsed, &stamp, &mut mint),
                Err(refusal) => Verdict::Refused(refusal),
            }
        } else if let Some(target_idx) = step.get("reverse").and_then(Value::as_u64) {
            let target_idx = usize::try_from(target_idx).expect("reverse index");
            let target = accepted.get(&target_idx).unwrap_or_else(|| {
                panic!("{name} step {step_idx}: step {target_idx} not accepted")
            });
            let event = Event {
                event: EventHeader {
                    id: format!("01JZ{:022}", target_idx + 1),
                    graph_seq: (target_idx + 1).to_string(),
                    descriptor: change::DESCRIPTOR.to_owned(),
                    request_id: format!("req-{target_idx}"),
                    request_digest: "0".repeat(64),
                    actor_id: stamp.actor_id.to_owned(),
                    client_id: "client-1".to_owned(),
                    origin_class: OriginClass::Human,
                    origin_actor: None,
                    origin_chain: None,
                    context_id: None,
                    approval_basis: None,
                    reverses_event_id: None,
                    event_time_ms: None,
                    recorded_at_ms: 1,
                    op_count: u32::try_from(target.ops.len()).expect("op_count"),
                    delta_digest: target.delta_digest.clone(),
                },
                ops: target.ops.clone(),
            };
            match inverse::plan(&event) {
                Ok(plan) => apply_inverse(&images, &plan, &stamp, &mut mint),
                Err(refusal) => Verdict::Refused(refusal),
            }
        } else {
            panic!("{name} step {step_idx}: expected payload or reverse");
        };

        if let Some(code) = step.get("refusal").and_then(Value::as_str) {
            assert_eq!(
                match verdict {
                    Verdict::Refused(r) => Some(r.code()),
                    _ => None,
                },
                Some(code),
                "{name} step {step_idx}"
            );
            assert!(
                images == before,
                "{name} step {step_idx}: state mutated on refusal"
            );
        } else if let Some(detail) = step.get("conflict") {
            assert_eq!(
                verdict,
                Verdict::Conflict(detail.clone()),
                "{name} step {step_idx}"
            );
            assert!(
                images == before,
                "{name} step {step_idx}: state mutated on conflict"
            );
        } else if let Some(expected) = step.get("file_refs") {
            let Verdict::Accepted(effects) = verdict else {
                panic!("{name} step {step_idx}: expected Accepted, got {verdict:?}");
            };
            images.apply_effects(&effects, (step_idx + 1) as u64);
            let want: Vec<Row> = serde_json::from_value(expected.clone()).expect("expected rows");
            assert_eq!(images.projected_file_refs(), want, "{name} step {step_idx}");
            accepted.insert(step_idx, effects);
        } else {
            panic!("{name} step {step_idx}: missing expectation");
        }
    }

    images
}

#[test]
fn file_ref_scenarios_hold() {
    for entry in entries("file_refs") {
        run_scenario(&entry);
    }
}

#[test]
fn file_ref_admissions_hold() {
    let mut results: HashMap<String, Result<(), Refusal>> = HashMap::new();
    for entry in entries("file_ref_admissions") {
        let name = id(&entry);
        let domain = entry["domain"].as_str().expect("domain");
        let manifest: HashMap<String, EntryKind> = entry["entries"]
            .as_array()
            .expect("entries")
            .iter()
            .map(|e| {
                let entry_id = e["id"].as_str().expect("entry id").to_owned();
                let kind: EntryKind = serde_json::from_value(e["kind"].clone()).expect("kind");
                (entry_id, kind)
            })
            .collect();
        let node = entry["node"].as_object().expect("node");
        let outcome = file_ref::admit(node, domain, &|entry_id| manifest.get(entry_id).copied());
        let want = entry.get("refusal").and_then(Value::as_str);
        assert_eq!(outcome.err().map(Refusal::code), want, "{name}");
        results.insert(name.to_owned(), outcome);
    }
    assert_eq!(
        results.get("resource-other-domain"),
        results.get("resource-nowhere"),
        "resource-other-domain and resource-nowhere must give the same result"
    );
}

fn assert_no_summary_property(value: &Value, path: &str) {
    match value {
        Value::Object(map) => {
            if let Some(Value::Object(props)) = map.get("properties") {
                assert!(
                    !props.contains_key(file_ref::SUMMARY),
                    "reserved property {} present at {path}",
                    file_ref::SUMMARY
                );
            }
            for (k, v) in map {
                assert_no_summary_property(v, &format!("{path}.{k}"));
            }
        }
        Value::Array(items) => {
            for (idx, item) in items.iter().enumerate() {
                assert_no_summary_property(item, &format!("{path}[{idx}]"));
            }
        }
        _ => {}
    }
}

#[test]
fn no_generated_text_on_nodes() {
    let corpus: Value = serde_json::from_str(CORPUS).expect("vector JSON");
    assert_no_summary_property(&corpus, "$");

    for entry in entries("file_refs") {
        let name = id(&entry);
        let final_images = run_scenario(&entry);
        for node in final_images.rows("nodes") {
            let node_id = node["id"].as_str().expect("node id");
            assert!(
                node["description_source_id"].is_null(),
                "{name} node {node_id}: non-null description_source_id"
            );
            assert!(
                node["description"].is_null(),
                "{name} node {node_id}: non-null description"
            );
        }
        if name == "note-creates-node" {
            let note = final_images
                .content("01JH0000000000000000000001")
                .expect("note content present");
            assert_eq!(
                note["owner_node_id"].as_str(),
                Some("01JG0000000000000000000001"),
                "note-creates-node: note must be owned by Rx"
            );
        }
    }
}
