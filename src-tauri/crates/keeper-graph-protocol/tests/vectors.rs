//! `syncular/knowledge-graph.vectors.json` holds for this implementation:
//! every accepted vector reproduces its canonical bytes and digest, every
//! refused one refuses with its stable code. keeper-hq's TypeScript
//! validator runs the same file; the tags are checked in keeper-core, where
//! the tag policy lives.

use keeper_graph_protocol::delta::{self, Op};
use keeper_graph_protocol::scope::{self, client_scope, receipt_id};
use keeper_graph_protocol::symbol::{decode, encode, SymbolKind};
use keeper_graph_protocol::value::{self, canonical, check_properties};
use keeper_graph_protocol::vocabulary::{
    self, stored_edge_type, stored_node_kind, EntityType, Lifecycle, Operation, OriginClass,
    Outcome,
};
use keeper_graph_protocol::{request, Refusal, CONTRACT_VERSION, PROFILE, SCHEMA_VERSION};
use serde_json::Value;

const CORPUS: &str = include_str!("../../../../syncular/knowledge-graph.vectors.json");

fn corpus() -> Value {
    serde_json::from_str(CORPUS).expect("vector JSON")
}

fn entries<'a>(corpus: &'a Value, key: &str) -> &'a [Value] {
    corpus[key].as_array().expect(key)
}

fn text<'a>(entry: &'a Value, key: &str) -> &'a str {
    entry[key]
        .as_str()
        .unwrap_or_else(|| panic!("{key} in {entry}"))
}

/// An entry's `json` with its `fill` applied.
fn expand(entry: &Value, json: &str) -> String {
    match entry.get("fill") {
        Some(fill) => json.replacen(
            "{FILL}",
            &text(fill, "text").repeat(fill["count"].as_u64().expect("count") as usize),
            1,
        ),
        None => json.to_owned(),
    }
}

fn refusal(entry: &Value) -> Option<&str> {
    entry.get("refusal").map(|r| r.as_str().expect("refusal"))
}

fn names<T: serde::Serialize>(items: &[T]) -> Value {
    serde_json::to_value(items).expect("names")
}

#[test]
fn versions_and_vocabulary_agree() {
    let v = corpus();
    assert_eq!(v["contract_version"], CONTRACT_VERSION);
    assert_eq!(v["schema_version"], SCHEMA_VERSION);
    assert_eq!(v["profile"], PROFILE);
    let w = &v["vocabulary"];
    assert_eq!(w["kinds"], names(&vocabulary::RESERVED_KINDS));
    assert_eq!(w["relations"], names(&vocabulary::RESERVED_RELATIONS));
    assert_eq!(w["node_fields"], names(&vocabulary::NODE_FIELDS));
    assert_eq!(w["edge_fields"], names(&vocabulary::EDGE_FIELDS));
    assert_eq!(w["tag_fields"], names(&vocabulary::TAG_FIELDS));
    assert_eq!(w["assertion_fields"], names(&vocabulary::ASSERTION_FIELDS));
    assert_eq!(
        w["lifecycles"],
        names(&[Lifecycle::Active, Lifecycle::Archived])
    );
    assert_eq!(
        w["entity_types"],
        names(&[
            EntityType::Node,
            EntityType::Edge,
            EntityType::Tag,
            EntityType::NodeTag,
            EntityType::Content,
            EntityType::Review,
        ])
    );
    assert_eq!(
        w["operations"],
        names(&[
            Operation::Create,
            Operation::Update,
            Operation::Archive,
            Operation::Unarchive,
            Operation::Remove,
        ])
    );
    assert_eq!(
        w["origin_classes"],
        names(&[OriginClass::Human, OriginClass::Agent])
    );
    assert_eq!(
        w["outcomes"],
        names(&[
            Outcome::Accepted,
            Outcome::Proposed,
            Outcome::Conflict,
            Outcome::Rejected,
        ])
    );
    assert_eq!(w["errors"], names(&vocabulary::ERRORS));
    for refusal in [
        Refusal::UnsupportedVersion,
        Refusal::InvalidChange,
        Refusal::ChangeLimit,
    ] {
        assert!(vocabulary::ERRORS.contains(&refusal.code()));
    }
    let s = &v["scopes"];
    assert_eq!(s["graph_variable"], scope::GRAPH_VARIABLE);
    assert_eq!(s["client_variable"], scope::CLIENT_VARIABLE);
    assert_eq!(s["graph_tables"], names(&scope::GRAPH_TABLES));
    assert_eq!(s["receipt_table"], scope::RECEIPT_TABLE);
}

#[test]
fn values_canonicalise_or_refuse() {
    let v = corpus();
    for entry in entries(&v, "values") {
        let json = expand(entry, text(entry, "json"));
        let id = text(entry, "id");
        let parsed = value::parse(json.as_bytes());
        match refusal(entry) {
            Some(code) => assert_eq!(parsed.err().map(Refusal::code), Some(code), "{id}"),
            None => {
                let parsed = parsed.unwrap_or_else(|e| panic!("{id}: {e}"));
                let bytes = canonical(&parsed).expect(id);
                assert_eq!(
                    std::str::from_utf8(&bytes).expect(id),
                    text(entry, "canonical"),
                    "{id}"
                );
                assert_eq!(
                    value::digest(&parsed).expect(id),
                    text(entry, "sha256"),
                    "{id}"
                );
            }
        }
    }
}

#[test]
fn properties_are_admitted_or_refused() {
    let v = corpus();
    for entry in entries(&v, "properties") {
        let json = expand(entry, text(entry, "json"));
        let id = text(entry, "id");
        let checked = value::parse(json.as_bytes()).and_then(|p| check_properties(&p));
        assert_eq!(checked.err().map(Refusal::code), refusal(entry), "{id}");
    }
}

#[test]
fn symbols_are_reversible_and_closed() {
    let v = corpus();
    let kind = |entry: &Value| match text(entry, "kind") {
        "node" => SymbolKind::Node,
        "edge" => SymbolKind::Edge,
        _ => SymbolKind::Property,
    };
    for entry in entries(&v, "symbols") {
        let (name, symbol) = (text(entry, "name"), text(entry, "symbol"));
        assert_eq!(encode(kind(entry), name).as_deref(), Ok(symbol), "{name}");
        assert_eq!(decode(kind(entry), symbol).as_deref(), Ok(name), "{symbol}");
    }
    for entry in entries(&v, "symbol_refusals") {
        let name = text(entry, "name");
        assert_eq!(
            encode(kind(entry), name),
            Err(Refusal::InvalidChange),
            "{name}"
        );
    }
    for entry in entries(&v, "decode_refusals") {
        let symbol = text(entry, "symbol");
        assert_eq!(
            decode(kind(entry), symbol),
            Err(Refusal::InvalidChange),
            "{symbol}"
        );
    }
    let stored = &v["stored_kinds"];
    let each = |key: &'static str| {
        stored[key]
            .as_array()
            .expect(key)
            .iter()
            .map(move |k| k.as_str().expect(key))
    };
    for kind in each("node_accepted") {
        assert_eq!(stored_node_kind(kind), Ok(()), "{kind}");
    }
    for kind in each("node_refused") {
        assert_eq!(
            stored_node_kind(kind),
            Err(Refusal::InvalidChange),
            "{kind}"
        );
    }
    for kind in each("edge_accepted") {
        assert_eq!(stored_edge_type(kind), Ok(()), "{kind}");
    }
    for kind in each("edge_refused") {
        assert_eq!(
            stored_edge_type(kind),
            Err(Refusal::InvalidChange),
            "{kind}"
        );
    }
}

#[test]
fn client_scopes_and_receipt_ids_reproduce() {
    let v = corpus();
    let mut seen = std::collections::BTreeSet::new();
    for entry in entries(&v, "client_scopes") {
        let scope = client_scope(text(entry, "actor"), text(entry, "client"));
        assert_eq!(scope, text(entry, "scope"));
        assert!(seen.insert(scope.clone()), "two pairs share {scope}");
        assert_eq!(
            receipt_id("graph.apply", "01JD0000000000000000000009", &scope),
            text(entry, "receipt_id")
        );
    }
}

#[test]
fn requests_digest_or_refuse() {
    let v = corpus();
    for entry in entries(&v, "requests") {
        let id = text(entry, "id");
        let parsed = request::parse(text(entry, "envelope").as_bytes())
            .and_then(|r| request::digest(text(entry, "descriptor"), &r));
        match refusal(entry) {
            Some(code) => assert_eq!(parsed.err().map(Refusal::code), Some(code), "{id}"),
            None => assert_eq!(parsed.as_deref(), Ok(text(entry, "sha256")), "{id}"),
        }
    }
}

#[test]
fn deltas_digest_or_refuse() {
    let v = corpus();
    for entry in entries(&v, "deltas") {
        let id = text(entry, "id");
        let mut ops = entry["ops"].clone();
        if let Some(fill) = entry.get("fill") {
            let json = expand(entry, &ops.to_string());
            ops = serde_json::from_str(&json).expect(id);
            assert!(fill.is_object());
        }
        if let Some(n) = entry.get("repeat").and_then(Value::as_u64) {
            let one = ops[0].clone();
            ops = Value::Array(
                (0..n)
                    .map(|ordinal| {
                        let mut op = one.clone();
                        op["ordinal"] = ordinal.into();
                        op
                    })
                    .collect(),
            );
        }
        let digest = serde_json::from_value::<Vec<Op>>(ops)
            .map_err(|_| Refusal::InvalidChange)
            .and_then(|ops| delta::digest(&ops));
        match refusal(entry) {
            Some(code) => assert_eq!(digest.err().map(Refusal::code), Some(code), "{id}"),
            None => assert_eq!(digest.as_deref(), Ok(text(entry, "sha256")), "{id}"),
        }
    }
}
