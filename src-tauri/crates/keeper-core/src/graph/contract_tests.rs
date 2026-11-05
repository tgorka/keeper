//! The graph contract as keeper compiles it: the shared tag vectors under
//! keeper's one tag policy, the schema IR's graph scopes, and the outcome
//! table.

use serde_json::{json, Map, Value};

use super::outcome::{resolve, GraphOutcome, Receipt};
use super::protocol::scope::{CLIENT_VARIABLE, GRAPH_TABLES, GRAPH_VARIABLE, RECEIPT_TABLE};
use super::protocol::{Refusal, SCHEMA_VERSION};
use crate::hq::port::{CommandResult, ReplicaError};

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

const DIGEST: &str = "aa";

fn row(outcome: &str, extra: Value) -> Map<String, Value> {
    let mut row = json!({
        "descriptor": "graph.apply",
        "request_id": "01JD0000000000000000000009",
        "request_digest": DIGEST,
        "outcome": outcome,
        "detail": "{\"ids\":[\"01JD0000000000000000000001\"]}",
    });
    for (key, value) in extra.as_object().expect("extra") {
        row[key] = value.clone();
    }
    row.as_object().expect("row").clone()
}

fn applied() -> Result<CommandResult, ReplicaError> {
    Ok(CommandResult {
        request_id: "01JD0000000000000000000009".into(),
        commit_seq: Some(9),
        cached: false,
    })
}

fn refused(code: &str, message: &str) -> Result<CommandResult, ReplicaError> {
    Err(ReplicaError::Remote {
        code: code.into(),
        message: message.into(),
        retryable: false,
    })
}

#[test]
fn receipt_rows_decode_or_refuse() {
    let accepted = Receipt::from_row(&row(
        "accepted",
        json!({"event_id": "01JD00000000000000000000E1", "graph_seq": "4"}),
    ))
    .expect("accepted");
    assert_eq!(
        accepted.detail,
        json!({"ids": ["01JD0000000000000000000001"]})
    );
    for (outcome, extra) in [
        ("accepted", json!({})),
        (
            "accepted",
            json!({"event_id": "01JD00000000000000000000E1", "graph_seq": "04"}),
        ),
        (
            "conflict",
            json!({"event_id": "01JD00000000000000000000E1", "graph_seq": "4"}),
        ),
        ("rejected", json!({})),
        ("pending", json!({})),
        ("applied", json!({})),
    ] {
        assert_eq!(
            Receipt::from_row(&row(outcome, extra.clone())),
            Err(Refusal::UnsupportedVersion),
            "{outcome} {extra}"
        );
    }
}

#[test]
fn outcome_table_separates_transport_from_acceptance() {
    let receipt =
        |outcome: &str, extra: Value| Receipt::from_row(&row(outcome, extra)).expect(outcome);
    let accepted = receipt(
        "accepted",
        json!({"event_id": "01JD00000000000000000000E1", "graph_seq": "4"}),
    );
    assert!(matches!(
        resolve(DIGEST, &applied(), Some(&accepted)),
        GraphOutcome::Accepted { ref graph_seq, commit_seq: Some(9), .. } if graph_seq == "4"
    ));
    // The durable receipt resolves an attempt whose answer was lost.
    let lost = Err(ReplicaError::Unheard {
        code: "x".into(),
        message: "y".into(),
        retryable: true,
    });
    assert!(matches!(
        resolve(DIGEST, &lost, Some(&accepted)),
        GraphOutcome::Accepted {
            commit_seq: None,
            ..
        }
    ));
    assert!(matches!(
        resolve(DIGEST, &lost, None),
        GraphOutcome::Pending { .. }
    ));
    // An acknowledgement is never acceptance on its own.
    assert_eq!(
        resolve(DIGEST, &applied(), None),
        GraphOutcome::PendingVisibility {
            commit_seq: Some(9)
        }
    );
    assert!(matches!(
        resolve(DIGEST, &applied(), Some(&receipt("conflict", json!({})))),
        GraphOutcome::Conflict { .. }
    ));
    assert!(matches!(
        resolve(DIGEST, &applied(), Some(&receipt("proposed", json!({})))),
        GraphOutcome::Proposed { .. }
    ));
    assert!(matches!(
        resolve(DIGEST, &applied(), Some(&receipt("rejected", json!({"reason": "invalid_change"})))),
        GraphOutcome::Rejected { ref reason, .. } if reason == "invalid_change"
    ));
    // A receipt for a different payload under the same id.
    assert_eq!(
        resolve("bb", &applied(), Some(&accepted)),
        GraphOutcome::IdempotencyMismatch
    );
    assert_eq!(
        resolve(
            DIGEST,
            &refused("operation.forbidden", "idempotency_mismatch"),
            None
        ),
        GraphOutcome::IdempotencyMismatch
    );
    assert_eq!(
        resolve(
            DIGEST,
            &refused("operation.invalid_request", "unsupported_version"),
            None
        ),
        GraphOutcome::Unsupported
    );
    assert_eq!(
        resolve(DIGEST, &refused("kg.event_group", "kg.event_group"), None),
        GraphOutcome::TechnicalRefusal {
            code: "kg.event_group".into()
        }
    );
    // Barrier, revocation and unresolved refusals never close the request.
    for message in ["graph_unavailable", "forbidden", "outcome_unresolved"] {
        assert!(matches!(
            resolve(DIGEST, &refused("operation.forbidden", message), None),
            GraphOutcome::Pending { .. }
        ));
    }
}
