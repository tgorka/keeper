//! The graph contract as keeper compiles it: the shared tag vectors under
//! keeper's one tag policy, the schema IR's graph scopes, and the outcome
//! table.

use serde_json::{json, Map, Value};

use super::outcome::{resolve, GraphOutcome, Receipt, ReceiptOutcome, RequestKey};
use super::protocol::scope::{
    client_scope, receipt_id, CLIENT_VARIABLE, GRAPH_TABLES, GRAPH_VARIABLE, RECEIPT_TABLE,
};
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

const REQUEST: &str = "01JD0000000000000000000009";
const OTHER_REQUEST: &str = "01JD000000000000000000000A";
const EVENT: &str = "01JD00000000000000000000E1";

fn digest(c: char) -> String {
    c.to_string().repeat(64)
}

fn key(request_id: &str) -> RequestKey {
    RequestKey {
        descriptor: "graph.apply".into(),
        request_id: request_id.into(),
        client_scope: client_scope("tgorka", "device-1"),
        request_digest: digest('a'),
    }
}

/// A receipt row as a replica holds it, for `key`, with `extra` set over it.
fn row_for(key: &RequestKey, outcome: &str, extra: Value) -> Map<String, Value> {
    let mut row = json!({
        "id": receipt_id(&key.descriptor, &key.request_id, &key.client_scope),
        "descriptor": key.descriptor,
        "request_id": key.request_id,
        "client_scope": key.client_scope,
        "request_digest": key.request_digest,
        "outcome": outcome,
        "detail": "{\"ids\":[\"01JD0000000000000000000001\"]}",
    });
    for (column, value) in extra.as_object().expect("extra") {
        row[column] = value.clone();
    }
    row.as_object().expect("row").clone()
}

fn row(outcome: &str, extra: Value) -> Map<String, Value> {
    row_for(&key(REQUEST), outcome, extra)
}

fn accepted_extra() -> Value {
    json!({"event_id": EVENT, "graph_seq": "4"})
}

fn receipt_for(key: &RequestKey, outcome: &str, extra: Value) -> Receipt {
    Receipt::from_row(&row_for(key, outcome, extra)).expect(outcome)
}

fn acknowledged(request_id: &str) -> Result<CommandResult, ReplicaError> {
    Ok(CommandResult {
        request_id: request_id.into(),
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

fn rejected(code: &str, message: &str) -> Result<CommandResult, ReplicaError> {
    Err(ReplicaError::Rejected {
        code: code.into(),
        message: message.into(),
    })
}

#[test]
fn receipt_rows_decode_to_their_shape() {
    let accepted = Receipt::from_row(&row("accepted", accepted_extra())).expect("accepted");
    assert_eq!(accepted.key(), &key(REQUEST));
    assert_eq!(
        accepted.outcome(),
        &ReceiptOutcome::Accepted {
            event_id: EVENT.into(),
            graph_seq: "4".into()
        }
    );
    // Text, as a replica column holds it, and a decoded value read alike.
    let decoded = Receipt::from_row(&row(
        "accepted",
        json!({"event_id": EVENT, "graph_seq": "4", "detail": {"ids": ["01JD0000000000000000000001"]}}),
    ))
    .expect("decoded detail");
    assert_eq!(decoded, accepted);
    assert_eq!(
        Receipt::from_row(&row("rejected", json!({"reason": "invalid_change"})))
            .expect("rejected")
            .outcome(),
        &ReceiptOutcome::Rejected {
            reason: "invalid_change".into()
        }
    );
    for outcome in ["proposed", "conflict"] {
        assert!(
            Receipt::from_row(&row(outcome, json!({}))).is_ok(),
            "{outcome}"
        );
    }
}

#[test]
fn receipt_rows_outside_the_shape_refuse() {
    let cases = [
        ("accepted", json!({})),
        ("accepted", json!({"event_id": EVENT})),
        ("accepted", json!({"event_id": "", "graph_seq": "4"})),
        ("accepted", json!({"event_id": "e1", "graph_seq": "4"})),
        ("accepted", json!({"event_id": EVENT, "graph_seq": "04"})),
        ("accepted", json!({"event_id": EVENT, "graph_seq": "0"})),
        ("accepted", json!({"event_id": EVENT, "graph_seq": 4})),
        (
            "accepted",
            json!({"event_id": EVENT, "graph_seq": "4", "reason": "x"}),
        ),
        ("conflict", json!({"graph_seq": "4"})),
        ("conflict", json!({"event_id": EVENT})),
        ("conflict", json!({"reason": "revision_conflict"})),
        ("proposed", json!({"event_id": EVENT, "graph_seq": "4"})),
        ("rejected", json!({})),
        ("rejected", json!({"reason": ""})),
        ("rejected", json!({"reason": "x", "graph_seq": "4"})),
        ("pending", json!({})),
        ("conflict", json!({"detail": "null"})),
        ("conflict", json!({"detail": "[]"})),
        ("conflict", json!({"detail": "\"x\""})),
        ("conflict", json!({"detail": "{\"a\":1,\"a\":2}"})),
        ("conflict", json!({"detail": "{\"a\":9007199254740992}"})),
        ("conflict", json!({"detail": []})),
        ("conflict", json!({"detail": null})),
        ("conflict", json!({"id": "graph.apply/x"})),
        ("conflict", json!({"request_id": "r1"})),
        ("conflict", json!({"descriptor": "kg.apply"})),
        ("conflict", json!({"request_digest": "AA"})),
        ("conflict", json!({"client_scope": null})),
    ];
    for (outcome, extra) in cases {
        assert_eq!(
            Receipt::from_row(&row(outcome, extra.clone())),
            Err(Refusal::UnsupportedVersion),
            "{outcome} {extra}"
        );
    }
}

/// Same descriptor, context and payload under two request ids share a
/// digest; neither request may be resolved by the other's receipt or
/// borrow the other's commit.
#[test]
fn a_receipt_speaks_only_for_its_own_request() {
    let mine = key(REQUEST);
    let theirs = key(OTHER_REQUEST);
    assert_eq!(mine.request_digest, theirs.request_digest);
    let their_receipt = receipt_for(&theirs, "accepted", accepted_extra());
    assert_eq!(
        resolve(&mine, &acknowledged(REQUEST), Some(&their_receipt)),
        GraphOutcome::PendingVisibility {
            commit_seq: Some(9)
        }
    );
    let lost = Err(ReplicaError::Unheard {
        code: "transport.timeout".into(),
        message: "timed out".into(),
        retryable: true,
    });
    assert!(matches!(
        resolve(&mine, &lost, Some(&their_receipt)),
        GraphOutcome::Pending { .. }
    ));
    // Another client of the same actor, or another descriptor.
    for other in [
        RequestKey {
            client_scope: client_scope("tgorka", "device-2"),
            ..mine.clone()
        },
        RequestKey {
            descriptor: "graph.propose".into(),
            ..mine.clone()
        },
    ] {
        let receipt = receipt_for(&other, "accepted", accepted_extra());
        assert!(matches!(
            resolve(&mine, &lost, Some(&receipt)),
            GraphOutcome::Pending { .. }
        ));
    }
    // An acknowledgement naming another request lends no commit.
    let my_receipt = receipt_for(&mine, "accepted", accepted_extra());
    assert!(matches!(
        resolve(&mine, &acknowledged(OTHER_REQUEST), Some(&my_receipt)),
        GraphOutcome::Accepted {
            commit_seq: None,
            ..
        }
    ));
    assert!(matches!(
        resolve(&mine, &acknowledged(OTHER_REQUEST), None),
        GraphOutcome::Pending { .. }
    ));
    // The request's own receipt resolves its own lost answer.
    assert!(matches!(
        resolve(&mine, &lost, Some(&my_receipt)),
        GraphOutcome::Accepted { ref event_id, ref graph_seq, commit_seq: None, .. }
            if event_id == EVENT && graph_seq == "4"
    ));
    assert!(matches!(
        resolve(&mine, &acknowledged(REQUEST), Some(&my_receipt)),
        GraphOutcome::Accepted {
            commit_seq: Some(9),
            ..
        }
    ));
}

#[test]
fn outcome_table_separates_transport_from_acceptance() {
    let mine = key(REQUEST);
    // An acknowledgement is never acceptance on its own.
    assert_eq!(
        resolve(&mine, &acknowledged(REQUEST), None),
        GraphOutcome::PendingVisibility {
            commit_seq: Some(9)
        }
    );
    assert!(matches!(
        resolve(
            &mine,
            &acknowledged(REQUEST),
            Some(&receipt_for(&mine, "conflict", json!({})))
        ),
        GraphOutcome::Conflict { .. }
    ));
    assert!(matches!(
        resolve(
            &mine,
            &acknowledged(REQUEST),
            Some(&receipt_for(&mine, "proposed", json!({})))
        ),
        GraphOutcome::Proposed { .. }
    ));
    assert!(matches!(
        resolve(
            &mine,
            &acknowledged(REQUEST),
            Some(&receipt_for(&mine, "rejected", json!({"reason": "invalid_change"})))
        ),
        GraphOutcome::Rejected { ref reason, .. } if reason == "invalid_change"
    ));
    // The request's own receipt for another payload.
    let changed = RequestKey {
        request_digest: digest('b'),
        ..mine.clone()
    };
    assert_eq!(
        resolve(
            &mine,
            &acknowledged(REQUEST),
            Some(&receipt_for(&changed, "accepted", accepted_extra()))
        ),
        GraphOutcome::IdempotencyMismatch
    );
    // HQ's refusals: the stable code is the message of `operation.forbidden`.
    assert_eq!(
        resolve(
            &mine,
            &refused("operation.forbidden", "idempotency_mismatch"),
            None
        ),
        GraphOutcome::IdempotencyMismatch
    );
    assert_eq!(
        resolve(
            &mine,
            &refused("operation.forbidden", "unsupported_version"),
            None
        ),
        GraphOutcome::Unsupported
    );
    assert_eq!(
        resolve(&mine, &refused("kg.event_group", "kg.event_group"), None),
        GraphOutcome::TechnicalRefusal {
            code: "kg.event_group".into()
        }
    );
    for message in [
        "graph_unavailable",
        "forbidden",
        "context_invalid",
        "context_revoked",
        "context_closed",
        "context_expired",
        "source_audience",
        "label_unknown",
        "outcome_unresolved",
    ] {
        assert!(
            matches!(
                resolve(&mine, &refused("operation.forbidden", message), None),
                GraphOutcome::Pending { .. }
            ),
            "{message}"
        );
    }
}

/// A rejected command result is definite whatever its code: the commit was
/// refused, so the request is terminal, never left pending.
#[test]
fn a_rejected_command_result_is_terminal() {
    let mine = key(REQUEST);
    for code in [
        "kg.command_only",
        "kg.sink_audience",
        "hq.v19",
        "operation.rejected",
    ] {
        assert_eq!(
            resolve(&mine, &rejected(code, "[]"), None),
            GraphOutcome::TechnicalRefusal { code: code.into() },
            "{code}"
        );
    }
    assert_eq!(
        resolve(
            &mine,
            &rejected("operation.forbidden", "idempotency_mismatch"),
            None
        ),
        GraphOutcome::IdempotencyMismatch
    );
    // The same code refused before dispatch leaves the request pending.
    assert!(matches!(
        resolve(&mine, &refused("operation.rejected", "[]"), None),
        GraphOutcome::Pending { .. }
    ));
    // A receipt still decides over a rejected transport.
    assert!(matches!(
        resolve(
            &mine,
            &rejected("kg.command_only", "kg.command_only"),
            Some(&receipt_for(&mine, "accepted", accepted_extra()))
        ),
        GraphOutcome::Accepted {
            commit_seq: None,
            ..
        }
    ));
}
