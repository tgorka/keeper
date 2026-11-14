//! The graph contract as keeper compiles it: the shared tag vectors under
//! keeper's one tag policy, the `graph.apply` payload rules and acceptance
//! algebra (whose tags are canonical under that policy), and the schema
//! IR's graph scopes.

use serde_json::{Map, Value};

use super::protocol::candidate::{self, State, Verdict};
use super::protocol::change;
use super::protocol::delta::Op;
use super::protocol::scope::{
    CLIENT_VARIABLE, GRAPH_TABLES, GRAPH_VARIABLE, PROPOSAL_TABLE, PROPOSAL_VARIABLE, RECEIPT_TABLE,
};
use super::protocol::SCHEMA_VERSION;
use crate::notes::tags::normalise;

const CORPUS: &str = include_str!("../../../../../syncular/knowledge-graph.vectors.json");

/// `value` with every `{NAME}` of the entry's `fills` expanded.
fn expand(value: &Value, fills: &Map<String, Value>) -> Value {
    match value {
        Value::String(text) => {
            let mut text = text.clone();
            for (name, fill) in fills {
                let piece = fill["text"].as_str().expect("fill text").repeat(
                    usize::try_from(fill["count"].as_u64().expect("count")).expect("count"),
                );
                text = text.replace(&format!("{{{name}}}"), &piece);
            }
            Value::String(text)
        }
        Value::Array(items) => Value::Array(items.iter().map(|i| expand(i, fills)).collect()),
        Value::Object(members) => Value::Object(
            members
                .iter()
                .map(|(k, v)| (k.clone(), expand(v, fills)))
                .collect(),
        ),
        other => other.clone(),
    }
}

fn entries(key: &str) -> Vec<Value> {
    let corpus: Value = serde_json::from_str(CORPUS).expect("vectors");
    let entries = corpus[key].as_array().expect(key).clone();
    assert!(!entries.is_empty(), "{key}");
    entries
        .iter()
        .map(|entry| match entry.get("fills") {
            Some(Value::Object(fills)) => expand(entry, fills),
            _ => entry.clone(),
        })
        .collect()
}

/// An accepted graph held as the vector's images (`contents` optional).
struct Images(Value);

impl Images {
    fn rows(&self, table: &str) -> impl Iterator<Item = &Map<String, Value>> {
        self.0
            .get(table)
            .and_then(Value::as_array)
            .map_or(&[][..], Vec::as_slice)
            .iter()
            .map(|row| row.as_object().expect("image"))
    }
    fn find(&self, table: &str, key: &str, value: &str) -> Option<Map<String, Value>> {
        self.rows(table).find(|row| row[key] == value).cloned()
    }
    /// Rows of `table` matching `keep`, ascending by id, at most `limit`.
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
            .find(|row| row["node_id"] == node_id && row["tag_id"] == tag_id)
            .cloned()
    }
    fn contains_into(&self, to_ids: &[String], limit: usize) -> Vec<Map<String, Value>> {
        self.select("edges", limit, |row| {
            row["type"] == "contains"
                && row["lifecycle"] == "active"
                && to_ids.iter().any(|id| row["to_id"] == id.as_str())
        })
    }
    fn content(&self, id: &str) -> Option<Map<String, Value>> {
        self.find("contents", "id", id)
    }
    fn contents_of(&self, owner_id: &str, limit: usize) -> Vec<Map<String, Value>> {
        self.select("contents", limit, |row| row["owner_node_id"] == owner_id)
    }
    fn sources_from(&self, from_id: &str, limit: usize) -> Vec<Map<String, Value>> {
        self.select("edges", limit, |row| {
            row["type"] == "sources" && row["lifecycle"] == "active" && row["from_id"] == from_id
        })
    }
    fn incident_active(&self, node_ids: &[String], limit: usize) -> Vec<Map<String, Value>> {
        self.select("edges", limit, |row| {
            row["lifecycle"] == "active"
                && node_ids
                    .iter()
                    .any(|id| row["from_id"] == id.as_str() || row["to_id"] == id.as_str())
        })
    }
    fn merged_into(&self, node_ids: &[String], limit: usize) -> Vec<Map<String, Value>> {
        self.select("nodes", limit, |row| {
            node_ids
                .iter()
                .any(|id| row["merged_into_id"] == id.as_str())
        })
    }
}

#[test]
fn change_vectors_parse_or_refuse() {
    for entry in entries("changes") {
        let id = entry["id"].as_str().expect("id");
        let payload =
            super::protocol::value::parse(entry["json"].as_str().expect("json").as_bytes())
                .expect("payload JSON");
        let parsed = change::parse(&payload, normalise);
        match entry.get("refusal").and_then(Value::as_str) {
            Some(code) => assert_eq!(parsed.map(|_| ()).map_err(|r| r.code()), Err(code), "{id}"),
            None => {
                let change = parsed.unwrap_or_else(|r| panic!("{id}: {r}"));
                // What a client builds is what the server reads back.
                assert_eq!(
                    change::parse(&change.payload(), normalise),
                    Ok(change),
                    "{id}"
                );
            }
        }
    }
}

#[test]
fn content_vectors_reproduce() {
    use super::protocol::content::{revise, Revision};
    for entry in entries("contents") {
        let id = entry["id"].as_str().expect("id");
        let before = entry["before"].as_object().expect("before");
        let revision = revise(before, entry["update"].as_object().expect("update"));
        if let Some(code) = entry.get("refusal").and_then(Value::as_str) {
            assert_eq!(
                revision,
                Revision::Refused(match code {
                    "change_limit" => super::protocol::Refusal::ChangeLimit,
                    _ => super::protocol::Refusal::InvalidChange,
                }),
                "{id}"
            );
        } else if let Some(detail) = entry.get("conflict") {
            assert_eq!(revision, Revision::Conflict(detail.clone()), "{id}");
        } else {
            let op: Op = serde_json::from_value(entry["op"].clone()).expect("op");
            assert_eq!(revision, Revision::Accepted(op.clone()), "{id}");
            assert_eq!(
                super::protocol::delta::digest(std::slice::from_ref(&op)).expect("digest"),
                entry["delta_digest"],
                "{id}"
            );
        }
    }
}

#[test]
fn algebra_vectors_reproduce() {
    for entry in entries("algebra") {
        let id = entry["id"].as_str().expect("id");
        let change = change::parse(&entry["payload"], normalise)
            .unwrap_or_else(|r| panic!("{id}: payload {r}"));
        let mut minted = entry["mint"]
            .as_array()
            .expect("mint")
            .iter()
            .map(|m| m.as_str().expect("minted id").to_owned());
        let mut mint = || minted.next().expect("enough minted ids");
        // A vector's `stamp` is the acceptance a verification is stamped
        // with; without one, a fixed stamp no other vector names.
        let stamp = entry.get("stamp");
        let actor = stamp
            .and_then(|s| s["actor_id"].as_str())
            .unwrap_or("vector-actor");
        let stamp = candidate::Stamp {
            time_ms: stamp.and_then(|s| s["time_ms"].as_u64()).unwrap_or(1),
            actor_id: actor,
        };
        let verdict = candidate::apply(&Images(entry["state"].clone()), &change, &stamp, &mut mint);
        if let Some(code) = entry.get("refusal").and_then(Value::as_str) {
            assert_eq!(
                match verdict {
                    Verdict::Refused(r) => Some(r.code()),
                    _ => None,
                },
                Some(code),
                "{id}"
            );
        } else if let Some(detail) = entry.get("conflict") {
            assert_eq!(verdict, Verdict::Conflict(detail.clone()), "{id}");
        } else {
            let Verdict::Accepted(effects) = verdict else {
                panic!("{id}: {verdict:?}");
            };
            let ops: Vec<Op> = serde_json::from_value(entry["ops"].clone()).expect("ops");
            assert_eq!(effects.ops, ops, "{id}");
            assert_eq!(effects.delta_digest, entry["delta_digest"], "{id}");
            assert_eq!(Value::Array(effects.affected), entry["affected"], "{id}");
        }
    }
}

#[test]
fn shared_tag_vectors_hold_under_the_notes_policy() {
    let corpus: Value = serde_json::from_str(CORPUS).expect("vectors");
    let tags = corpus["tags"].as_array().expect("tags");
    assert!(!tags.is_empty());
    for tag in tags {
        let input = tag["input"].as_str().expect("input");
        assert_eq!(
            crate::notes::tags::normalise(input).as_deref(),
            tag["canonical"].as_str(),
            "{input:?}"
        );
    }
}

/// Every graph table carries exactly one scope variable, `kg_graph` on the
/// graph's rows, `kg_client` on receipts and `kg_proposal` on proposals;
/// no other table carries any of them, and no subscription reaches a graph
/// table through any other variable.
#[test]
fn compiled_schema_scopes_graph_tables_by_one_variable() {
    let ir = crate::hq::syncular::schema().expect("schema");
    assert_eq!(ir["schemaVersion"], SCHEMA_VERSION);
    let tables = ir["tables"].as_array().expect("tables");
    let scopes = |name: &str| -> Vec<(String, String)> {
        let table = tables
            .iter()
            .find(|t| t["name"] == name)
            .unwrap_or_else(|| panic!("{name} missing"));
        table["scopes"]
            .as_array()
            .expect("scopes")
            .iter()
            .map(|s| {
                (
                    s["variable"].as_str().expect("variable").to_owned(),
                    s["column"].as_str().expect("column").to_owned(),
                )
            })
            .collect()
    };
    for table in GRAPH_TABLES {
        assert_eq!(
            scopes(table),
            [(GRAPH_VARIABLE.to_owned(), "graph".to_owned())],
            "{table}"
        );
    }
    assert_eq!(
        scopes(RECEIPT_TABLE),
        [(CLIENT_VARIABLE.to_owned(), "client_scope".to_owned())]
    );
    assert_eq!(
        scopes(PROPOSAL_TABLE),
        [(PROPOSAL_VARIABLE.to_owned(), "proposer_actor".to_owned())]
    );
    for table in tables {
        let name = table["name"].as_str().expect("name");
        if name.starts_with("kg_") {
            assert!(
                GRAPH_TABLES.contains(&name) || name == RECEIPT_TABLE || name == PROPOSAL_TABLE,
                "{name}"
            );
            continue;
        }
        for (variable, _) in scopes(name) {
            assert!(!variable.starts_with("kg_"), "{name} carries {variable}");
        }
    }
    for subscription in ir["subscriptions"].as_array().expect("subscriptions") {
        let table = subscription["table"].as_str().expect("table");
        if !table.starts_with("kg_") {
            continue;
        }
        let requested: Vec<&str> = subscription["scopes"]
            .as_array()
            .expect("scopes")
            .iter()
            .map(|scope| scope["variable"].as_str().expect("variable"))
            .collect();
        let expected = match table {
            RECEIPT_TABLE => CLIENT_VARIABLE,
            PROPOSAL_TABLE => PROPOSAL_VARIABLE,
            _ => GRAPH_VARIABLE,
        };
        assert_eq!(requested, [expected], "{table}");
    }
}
