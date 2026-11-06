//! The graph contract as keeper compiles it: the shared tag vectors under
//! keeper's one tag policy, and the schema IR's graph scopes.

use serde_json::Value;

use super::protocol::scope::{CLIENT_VARIABLE, GRAPH_TABLES, GRAPH_VARIABLE, RECEIPT_TABLE};
use super::protocol::SCHEMA_VERSION;

const CORPUS: &str = include_str!("../../../../../syncular/knowledge-graph.vectors.json");

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
/// graph's rows and `kg_client` on receipts; no other table carries either,
/// and no subscription reaches a graph table through any other variable.
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
    for table in tables {
        let name = table["name"].as_str().expect("name");
        if name.starts_with("kg_") {
            assert!(
                GRAPH_TABLES.contains(&name) || name == RECEIPT_TABLE,
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
        let expected = if table == RECEIPT_TABLE {
            CLIENT_VARIABLE
        } else {
            GRAPH_VARIABLE
        };
        assert_eq!(requested, [expected], "{table}");
    }
}
