//! `syncular/knowledge-graph.vectors.json`'s schema-8 lists hold for this
//! implementation: reviews, proposals, inverses and reversals. keeper-hq's
//! TypeScript mirror runs the same entries.

use keeper_graph_protocol::candidate::{apply_inverse, Retained, Stamp, State, Verdict};
use keeper_graph_protocol::delta::Op;
use keeper_graph_protocol::inverse::{self, Plan};
use keeper_graph_protocol::review::{self, Target};
use keeper_graph_protocol::value::typed_ref;
use keeper_graph_protocol::wire::Event;
use keeper_graph_protocol::{proposal, Refusal};
use serde_json::{Map, Value};

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

/// A graph now, held as a vector's images.
struct Images(Value);

impl Images {
    fn rows(&self, table: &str) -> impl Iterator<Item = &Map<String, Value>> {
        self.0[table]
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
}

impl Retained for Images {
    fn written_at(&self, id: &str) -> Option<u64> {
        self.0["written"][id].as_u64()
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

fn minted(entry: &Value) -> impl FnMut() -> String {
    let mut ids = entry["mint"]
        .as_array()
        .map(|m| {
            m.iter()
                .map(|i| i.as_str().expect("id").to_owned())
                .collect::<Vec<_>>()
        })
        .unwrap_or_default()
        .into_iter();
    move || ids.next().expect("enough minted ids")
}

/// The verdict an entry pins: accepted ops, a conflict or a refusal.
fn assert_verdict(entry: &Value, verdict: Verdict) {
    let name = id(entry);
    if let Some(code) = entry.get("refusal").and_then(Value::as_str) {
        assert_eq!(
            match verdict {
                Verdict::Refused(r) => Some(r.code()),
                _ => None,
            },
            Some(code),
            "{name}"
        );
    } else if let Some(detail) = entry.get("conflict") {
        assert_eq!(verdict, Verdict::Conflict(detail.clone()), "{name}");
    } else {
        let Verdict::Accepted(effects) = verdict else {
            panic!("{name}: {verdict:?}");
        };
        let ops: Vec<Op> = serde_json::from_value(entry["ops"].clone()).expect("ops");
        assert_eq!(effects.ops, ops, "{name}");
        assert_eq!(effects.delta_digest, entry["delta_digest"], "{name}");
        assert_eq!(Value::Array(effects.affected), entry["affected"], "{name}");
    }
}

#[test]
fn inverses_plan_and_apply_or_refuse() {
    for entry in entries("inverses") {
        let name = id(&entry);
        let state = Images(entry["state"].clone());
        let stamp = Stamp {
            time_ms: 7_000,
            actor_id: entry["actor"].as_str().expect("actor"),
        };
        let plan: Plan = match entry.get("event") {
            Some(event) => {
                let event: Event = serde_json::from_value(event.clone()).expect("event");
                let preview = inverse::preview(&state, &event, &stamp);
                let want = &entry["preview"];
                assert_eq!(preview.outcome, want["outcome"], "{name}");
                assert_eq!(
                    serde_json::to_value(&preview.reason).expect("r"),
                    want["reason"],
                    "{name}"
                );
                assert_eq!(
                    serde_json::to_value(&preview.detail).expect("d"),
                    want["detail"],
                    "{name}"
                );
                match inverse::plan(&event) {
                    Err(refusal) => {
                        assert!(entry.get("plan").is_none(), "{name}");
                        assert_eq!(Some(refusal.code()), entry["refusal"].as_str(), "{name}");
                        continue;
                    }
                    Ok(plan) => {
                        assert_eq!(
                            serde_json::to_value(&plan).expect("plan"),
                            entry["plan"],
                            "{name}"
                        );
                        assert_eq!(
                            serde_json::to_value(inverse::expected(&plan)).expect("expected"),
                            entry["expected"],
                            "{name}"
                        );
                        plan
                    }
                }
            }
            None => serde_json::from_value(entry["plan"].clone()).expect("plan"),
        };
        assert_verdict(
            &entry,
            apply_inverse(&state, &plan, &stamp, &mut minted(&entry)),
        );
    }
}

#[test]
fn reversal_payloads_bind_the_previewed_inverse() {
    for entry in entries("reversals") {
        let name = id(&entry);
        let parsed = inverse::parse_reverse(&entry["payload"]);
        let Some(event) = entry.get("event") else {
            assert_eq!(
                parsed.map(|_| ()).map_err(Refusal::code),
                Err(entry["refusal"].as_str().expect("refusal")),
                "{name}"
            );
            continue;
        };
        let request = parsed.unwrap_or_else(|r| panic!("{name}: {r}"));
        assert_eq!(request.payload(), entry["payload"], "{name}");
        let event: Event = serde_json::from_value(event.clone()).expect("event");
        let stamp = Stamp {
            time_ms: 7_000,
            actor_id: entry["actor"].as_str().expect("actor"),
        };
        let state = Images(entry["state"].clone());
        assert_verdict(
            &entry,
            inverse::reverse(&state, &event, &request, &stamp, &mut minted(&entry)),
        );
    }
}

#[test]
fn reviews_are_one_shape() {
    for entry in entries("reviews") {
        let name = id(&entry);
        if let Some(payload) = entry.get("payload") {
            let parsed = review::parse_acknowledge(payload);
            match entry.get("refusal").and_then(Value::as_str) {
                Some(code) => assert_eq!(
                    parsed.map(|_| ()).map_err(Refusal::code),
                    Err(code),
                    "{name}"
                ),
                None => {
                    let target = parsed.unwrap_or_else(|r| panic!("{name}: {r}"));
                    assert_eq!(&review::acknowledge_payload(&target), payload, "{name}");
                }
            }
        } else if let Some(image) = entry.get("image") {
            let ok = review::image_ok(
                entry["entity_id"].as_str().expect("entity"),
                image.as_object().expect("image"),
            );
            assert_eq!(Some(ok), entry["valid"].as_bool(), "{name}");
        } else {
            let target = Target::new(
                entry["target_event_id"].as_str().expect("t"),
                entry["target_delta_digest"].as_str().expect("d"),
            )
            .expect("target");
            let effects = review::acknowledgement(
                entry["review_id"].as_str().expect("review"),
                &target,
                entry["reviewer_actor"].as_str().expect("reviewer"),
            )
            .expect("acknowledgement");
            let ops: Vec<Op> = serde_json::from_value(entry["ops"].clone()).expect("ops");
            assert_eq!(effects.ops, ops, "{name}");
            assert_eq!(effects.delta_digest, entry["delta_digest"], "{name}");
            assert_eq!(Value::Array(effects.affected), entry["affected"], "{name}");
        }
    }
}

#[test]
fn proposals_and_decisions_digest_or_refuse() {
    for entry in entries("proposals") {
        let name = id(&entry);
        if let Some(payload) = entry.get("decide") {
            let parsed = proposal::parse_decide(payload);
            if let Some(code) = entry.get("refusal").and_then(Value::as_str) {
                assert_eq!(
                    parsed.map(|_| ()).map_err(Refusal::code),
                    Err(code),
                    "{name}"
                );
                continue;
            }
            let decision = parsed.unwrap_or_else(|r| panic!("{name}: {r}"));
            assert_eq!(&decision.payload(), payload, "{name}");
            let proposer = proposal::Proposer {
                actor: "agent-1".into(),
                client: "runtime-1".into(),
                context: Some("01JN0000000000000000000030".into()),
            };
            assert_eq!(
                proposal::approval_basis(&decision, &proposer),
                entry["approval_basis"],
                "{name}"
            );
            if decision.verdict == proposal::Verdict::Reject {
                assert_eq!(
                    proposal::decided_detail(&decision),
                    entry["detail"],
                    "{name}"
                );
            }
            continue;
        }
        let parsed = proposal::parse(&entry["payload"], |t| Some(t.to_lowercase()));
        if let Some(code) = entry.get("refusal").and_then(Value::as_str) {
            assert_eq!(
                parsed.map(|_| ()).map_err(Refusal::code),
                Err(code),
                "{name}"
            );
            continue;
        }
        let proposal = parsed.unwrap_or_else(|r| panic!("{name}: {r}"));
        assert_eq!(proposal.payload(), entry["payload"], "{name}");
        let graph = entry["graph"].as_str().expect("graph");
        let digest = proposal::digest(graph, &proposal).expect("digest");
        let revision = proposal.revision().expect("revision");
        assert_eq!(revision, entry["revision"], "{name}");
        assert_eq!(digest, entry["digest"], "{name}");
        assert_eq!(
            proposal::proposed_detail(&proposal.proposal_id, &revision, &digest),
            entry["detail"],
            "{name}"
        );
    }
}

/// X7d: a reversal is its inverse and one review within one event's 128
/// operations — an event of `n` creates reverses only while `n + 1` fits.
#[test]
fn a_reversal_and_its_review_fit_one_event_or_refuse_change_limit() {
    use keeper_graph_protocol::candidate::apply;
    use keeper_graph_protocol::change;
    use serde_json::json;
    for (n, refused) in [(127u64, false), (128, true)] {
        let ids: Vec<String> = (0..n).map(|i| format!("01JQ{i:022}")).collect();
        let payload = json!({"ops": ids.iter().map(|i| json!({"op": "create_node", "id": i,
            "kind": "Concept", "title": "N", "description": null, "properties": {}})).collect::<Vec<_>>()});
        let change = change::parse(&payload, |t| Some(t.to_owned())).expect("change");
        let empty = Images(json!({}));
        let stamp = Stamp {
            time_ms: 1,
            actor_id: "person-editor",
        };
        let Verdict::Accepted(effects) = apply(&empty, &change, &stamp, &mut || unreachable!())
        else {
            panic!("forward {n}");
        };
        let nodes: Vec<Value> = effects
            .ops
            .iter()
            .filter_map(|op| op.after_value.clone())
            .collect();
        let written: Map<String, Value> = ids.iter().map(|i| (i.clone(), json!(1))).collect();
        let now = Images(json!({"nodes": nodes, "written": written}));
        let event: Event = serde_json::from_value(json!({
            "event": {"id": "01JQ9999999999999999999999", "graph_seq": "1", "descriptor": "graph.apply",
                "request_id": "01JQ9999999999999999999998", "request_digest": "ab".repeat(32),
                "actor_id": "person-editor", "client_id": "device", "origin_class": "human",
                "origin_actor": null, "origin_chain": null, "context_id": null,
                "approval_basis": {"basis": "direct"}, "reverses_event_id": null, "event_time_ms": null,
                "recorded_at_ms": 1, "op_count": n, "delta_digest": effects.delta_digest},
            "ops": effects.ops,
        }))
        .expect("event");
        let plan = inverse::plan(&event).expect("plan");
        let stamp = Stamp {
            time_ms: 2,
            actor_id: "person-reviewer",
        };
        let verdict = apply_inverse(&now, &plan, &stamp, &mut || {
            "01JQ9999999999999999999997".to_owned()
        });
        match verdict {
            Verdict::Refused(Refusal::ChangeLimit) => assert!(refused, "{n}"),
            Verdict::Accepted(effects) => {
                assert!(!refused, "{n}");
                assert_eq!(effects.ops.len(), 128);
            }
            other => panic!("{n}: {other:?}"),
        }
    }
}
