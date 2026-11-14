//! Property: every reversal the shared `inverses[]` vectors accept leaves a
//! graph that forward's own validation accepts. The after-state is built
//! from the accepted operations alone and judged as a forward candidate
//! over it — [`Candidate::whole`] (references, endpoints, containment,
//! [`Candidate::finish`]) and the merge rules for a re-merge — so a
//! reversal can never produce what no forward change could.

use std::collections::BTreeMap;

use serde_json::{Map, Value};

use super::{
    apply_inverse, carries_verification, mergeable, merged_into, text, Candidate, Entry, Image,
    Retained, Stamp, State, Verdict,
};
use crate::delta::Op;
use crate::inverse::{self, Plan};
use crate::value::typed_ref;
use crate::vocabulary::EntityType;
use crate::wire::Event;

const CORPUS: &str = include_str!("../../../../../syncular/knowledge-graph.vectors.json");

/// A graph as a vector's tables of images, keyed by id.
#[derive(Clone, Default)]
struct Graph {
    tables: BTreeMap<String, BTreeMap<String, Image>>,
    written: Map<String, Value>,
}

impl Graph {
    fn from_vector(state: &Value) -> Self {
        let mut graph = Self::default();
        for table in ["nodes", "edges", "contents", "tags", "node_tags"] {
            let rows = state[table].as_array().map_or(&[][..], Vec::as_slice);
            graph.tables.insert(
                table.to_owned(),
                rows.iter()
                    .filter_map(Value::as_object)
                    .map(|r| (text(r, "id"), r.clone()))
                    .collect(),
            );
        }
        graph.written = state["written"].as_object().cloned().unwrap_or_default();
        graph
    }

    fn rows(&self, table: &str) -> impl Iterator<Item = &Image> {
        self.tables
            .get(table)
            .into_iter()
            .flat_map(BTreeMap::values)
    }

    fn select(&self, table: &str, limit: usize, keep: impl Fn(&Image) -> bool) -> Vec<Image> {
        // BTreeMap order is ascending by id.
        self.rows(table)
            .filter(|r| keep(r))
            .take(limit)
            .cloned()
            .collect()
    }

    /// The graph after the accepted operations `ops`.
    fn after(&self, ops: &[Op]) -> Self {
        let mut graph = self.clone();
        for op in ops {
            let table = match op.entity_type {
                EntityType::Node => "nodes",
                EntityType::Edge => "edges",
                EntityType::Content => "contents",
                EntityType::Tag => "tags",
                EntityType::NodeTag => "node_tags",
                EntityType::Review => continue,
            };
            let rows = graph.tables.entry(table.to_owned()).or_default();
            match op.after_value.as_ref().and_then(Value::as_object) {
                Some(after) => {
                    rows.insert(text(after, "id"), after.clone());
                }
                None => {
                    rows.remove(&op.entity_id);
                }
            }
        }
        graph
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

impl State for Graph {
    fn node(&self, id: &str) -> Option<Image> {
        self.tables.get("nodes")?.get(id).cloned()
    }
    fn edge(&self, id: &str) -> Option<Image> {
        self.tables.get("edges")?.get(id).cloned()
    }
    fn tag(&self, tag: &str) -> Option<Image> {
        self.rows("tags").find(|r| text(r, "text") == tag).cloned()
    }
    fn membership(&self, node_id: &str, tag_id: &str) -> Option<Image> {
        self.rows("node_tags")
            .find(|r| text(r, "node_id") == node_id && text(r, "tag_id") == tag_id)
            .cloned()
    }
    fn contains_into(&self, to_ids: &[String], limit: usize) -> Vec<Image> {
        self.select("edges", limit, |r| {
            text(r, "type") == "contains"
                && text(r, "lifecycle") == "active"
                && to_ids.contains(&text(r, "to_id"))
        })
    }
    fn content(&self, id: &str) -> Option<Image> {
        self.tables.get("contents")?.get(id).cloned()
    }
    fn contents_of(&self, owner_id: &str, limit: usize) -> Vec<Image> {
        self.select("contents", limit, |r| text(r, "owner_node_id") == owner_id)
    }
    fn sources_from(&self, from_id: &str, limit: usize) -> Vec<Image> {
        self.select("edges", limit, |r| {
            text(r, "type") == "sources"
                && text(r, "lifecycle") == "active"
                && text(r, "from_id") == from_id
        })
    }
    fn incident_active(&self, node_ids: &[String], limit: usize) -> Vec<Image> {
        self.select("edges", limit, |r| {
            text(r, "lifecycle") == "active"
                && (node_ids.contains(&text(r, "from_id")) || node_ids.contains(&text(r, "to_id")))
        })
    }
    fn merged_into(&self, node_ids: &[String], limit: usize) -> Vec<Image> {
        self.select("nodes", limit, |r| {
            merged_into(r).is_some_and(|m| node_ids.iter().any(|i| i == m))
        })
    }
}

impl Retained for Graph {
    fn written_at(&self, id: &str) -> Option<u64> {
        self.written.get(id).and_then(Value::as_u64)
    }
    fn referrers(&self, node_id: &str, limit: usize) -> Vec<String> {
        let mut out: Vec<String> = self
            .rows("nodes")
            .chain(self.rows("edges"))
            .filter(|r| r.get("properties").is_some_and(|p| refers(p, node_id)))
            .chain(
                self.rows("contents")
                    .filter(|c| c.get("body").is_some_and(|b| refers(b, node_id))),
            )
            .map(|r| text(r, "id"))
            .collect();
        out.sort();
        out.truncate(limit);
        out
    }
}

fn image(value: Option<&Value>) -> Option<Image> {
    value.and_then(Value::as_object).cloned()
}

/// Forward's validation of the graph `after`, as written by `ops`: each
/// written node, edge and content a touched entry of a forward candidate
/// over it, judged by [`Candidate::whole`]; a re-merge by [`mergeable`].
fn forward_accepts(after: &Graph, ops: &[Op]) -> Result<(), String> {
    let mut candidate = Candidate::empty(after);
    for op in ops {
        let (Some(before), Some(now)) = (
            image(op.before_value.as_ref()),
            image(op.after_value.as_ref()),
        ) else {
            continue;
        };
        let id = op.entity_id.clone();
        let entry = |before: &Image, now: &Image| Entry {
            before: Some(before.clone()),
            after: now.clone(),
            touched: true,
        };
        match op.entity_type {
            EntityType::Node => {
                if carries_verification(&now) {
                    return Err(format!("node {id} verified"));
                }
                if merged_into(&before).is_none() {
                    if let Some(survivor) = merged_into(&now) {
                        let survivor = after.node(survivor).ok_or("no survivor")?;
                        if !mergeable(&survivor) || !mergeable(&before) {
                            return Err(format!("node {id} merged into an unmergeable node"));
                        }
                    }
                }
                let source = now.get("description_source_id").and_then(Value::as_str);
                if source.is_some()
                    && source != before.get("description_source_id").and_then(Value::as_str)
                {
                    candidate
                        .provenance
                        .push((id.clone(), source.unwrap_or_default().to_owned()));
                }
                candidate
                    .written
                    .push(now.get("properties").cloned().unwrap_or(Value::Null));
                candidate.nodes.insert(id, entry(&before, &now));
            }
            EntityType::Edge => {
                candidate
                    .written
                    .push(now.get("properties").cloned().unwrap_or(Value::Null));
                if text(&now, "lifecycle") == "active" {
                    candidate.endpoint_checks.push(id.clone());
                    if text(&now, "type") == "contains" {
                        candidate.containment.push(id.clone());
                    }
                }
                candidate.edges.insert(id, entry(&before, &now));
            }
            EntityType::Content => {
                candidate.contents.insert(id, entry(&before, &now));
            }
            _ => {}
        }
    }
    let written: Vec<(String, Image)> = candidate
        .nodes
        .iter()
        .map(|(id, e)| (id.clone(), e.after.clone()))
        .collect();
    match candidate.whole() {
        Err(super::Stop::Refused(refusal)) => return Err(format!("refused {}", refusal.code())),
        Err(super::Stop::Conflict(detail)) => return Err(format!("conflict {detail}")),
        Ok(()) => {}
    }
    // Forward's own X29 clear has nothing left to clear.
    for (id, now) in written {
        if candidate.nodes.get(&id).map(|e| &e.after) != Some(&now) {
            return Err(format!("node {id} cleared by finish"));
        }
    }
    Ok(())
}

#[test]
fn every_accepted_reversal_leaves_a_graph_forward_accepts() {
    let corpus: Value = serde_json::from_str(CORPUS).expect("vectors");
    let mut accepted = 0;
    for entry in corpus["inverses"].as_array().expect("inverses") {
        let name = entry["id"].as_str().expect("id");
        let state = Graph::from_vector(&entry["state"]);
        let plan: Plan = match entry.get("event") {
            Some(event) => {
                let event: Event = serde_json::from_value(event.clone()).expect("event");
                match inverse::plan(&event) {
                    Ok(plan) => plan,
                    Err(_) => continue,
                }
            }
            None => serde_json::from_value(entry["plan"].clone()).expect("plan"),
        };
        let mut mint = entry["mint"]
            .as_array()
            .map(|m| {
                m.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_else(Vec::new)
            .into_iter();
        let stamp = Stamp {
            time_ms: 7_000,
            actor_id: entry["actor"].as_str().expect("actor"),
        };
        let verdict = apply_inverse(&state, &plan, &stamp, &mut || {
            mint.next().expect("enough minted ids")
        });
        let Verdict::Accepted(effects) = verdict else {
            continue;
        };
        let after = state.after(&effects.ops);
        if let Err(why) = forward_accepts(&after, &effects.ops) {
            panic!("{name}: {why}");
        }
        accepted += 1;
    }
    assert!(accepted >= 25, "only {accepted} accepted inverse vectors");
}
