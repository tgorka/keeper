//! The outcome table through the domain-bound owner: receipts decoded from
//! a database's own replica, keys minted by that database, and requests of
//! equal ids in two databases kept apart.

use std::path::PathBuf;
use std::sync::Arc;

use serde_json::{json, Map, Value};

use super::outcome::{
    Attempt, GraphOutcome, GraphRequestError, GraphRequests, Receipt, ReceiptOutcome, RequestKey,
};
use super::protocol::request::{self, Request};
use super::protocol::scope::receipt_id;
use super::protocol::Refusal;
use crate::hq::port::fake::{FakeOpener, FakeReplica};
use crate::hq::port::{CommandResult, Opener, ReplicaError, ReplicaSpec};
use crate::hq::sessions::Source;

const REQUEST: &str = "01JD0000000000000000000009";
const OTHER_REQUEST: &str = "01JD000000000000000000000A";
const EVENT: &str = "01JD00000000000000000000E1";
const OTHER_EVENT: &str = "01JD00000000000000000000E2";

/// One database's owner over a fake replica, for this device's actor and
/// `client`.
fn owner_as(domain: &str, client: &str) -> (Arc<FakeReplica>, GraphRequests) {
    let opener = FakeOpener::default();
    let replica = opener
        .open(ReplicaSpec {
            database: domain.into(),
            path: PathBuf::from("/nowhere"),
            client_id: None,
            link: None,
            subscriptions: Vec::new(),
            watch_presence: None,
        })
        .expect("opens");
    let held = opener.all().remove(0);
    let source = Source {
        database: domain.into(),
        replica,
        actor: "tgorka".into(),
        client: client.into(),
    };
    (held, GraphRequests::new(source))
}

fn owner(domain: &str) -> (Arc<FakeReplica>, GraphRequests) {
    owner_as(domain, "device-1")
}

fn request(request_id: &str, n: u32) -> Request {
    request::parse(
        json!({"version": 1, "request_id": request_id, "context_id": null, "payload": {"n": n}})
            .to_string()
            .as_bytes(),
    )
    .expect("request")
}

fn key(owner: &GraphRequests, request_id: &str) -> RequestKey {
    owner
        .key("graph.apply", &request(request_id, 1))
        .expect("key")
}

/// A receipt row as a replica holds it, for `key`, with `extra` set over it.
fn row_for(key: &RequestKey, outcome: &str, extra: Value) -> Map<String, Value> {
    let mut row = json!({
        "id": receipt_id(key.descriptor(), key.request_id(), key.client_scope()),
        "descriptor": key.descriptor(),
        "request_id": key.request_id(),
        "client_scope": key.client_scope(),
        "request_digest": key.request_digest(),
        "outcome": outcome,
        "detail": "{\"ids\":[\"01JD0000000000000000000001\"]}",
    });
    for (column, value) in extra.as_object().expect("extra") {
        row[column] = value.clone();
    }
    row.as_object().expect("row").clone()
}

fn accepted(event: &str) -> Value {
    json!({"event_id": event, "graph_seq": "4"})
}

/// Replicate `rows` into `held` and read `key`'s receipt through `owner`.
fn held_receipt(
    held: &FakeReplica,
    owner: &GraphRequests,
    key: &RequestKey,
    rows: Vec<Map<String, Value>>,
) -> Receipt {
    held.replicate(rows);
    owner.receipt(key).expect("lookup").expect("receipt held")
}

fn acknowledged(key: &RequestKey, request_id: &str, commit: i64) -> Attempt {
    Attempt::new(
        key,
        Ok(CommandResult {
            request_id: request_id.into(),
            commit_seq: Some(commit),
            cached: false,
        }),
    )
}

fn failed(key: &RequestKey, error: ReplicaError) -> Attempt {
    Attempt::new(key, Err(error))
}

fn lost(key: &RequestKey) -> Attempt {
    failed(
        key,
        ReplicaError::Unheard {
            code: "transport.timeout".into(),
            message: "timed out".into(),
            retryable: true,
        },
    )
}

fn refused(key: &RequestKey, code: &str, message: &str) -> Attempt {
    failed(
        key,
        ReplicaError::Remote {
            code: code.into(),
            message: message.into(),
            retryable: false,
        },
    )
}

fn rejected(key: &RequestKey, code: &str, message: &str) -> Attempt {
    failed(
        key,
        ReplicaError::Rejected {
            code: code.into(),
            message: message.into(),
        },
    )
}

/// The same actor and client send the same descriptor, request id and
/// payload to both databases: one digest, two requests. Neither database's
/// receipt or acknowledgement may resolve the other's request.
#[test]
fn equal_requests_in_two_databases_never_resolve_each_other() {
    let (tg_held, tgdrive) = owner("tgdrive");
    let (neura_held, neuradrive) = owner("neuradrive");
    let tg = key(&tgdrive, REQUEST);
    let neura = key(&neuradrive, REQUEST);
    assert_eq!(tg.request_digest(), neura.request_digest());
    assert_ne!(tg, neura);

    let neura_receipt = held_receipt(
        &neura_held,
        &neuradrive,
        &neura,
        vec![row_for(&neura, "accepted", accepted(OTHER_EVENT))],
    );
    assert_eq!(neura_receipt.key().domain(), "neuradrive");
    // tgdrive's attempt, answered or lost, beside neuradrive's receipt.
    assert_eq!(
        tgdrive.resolve(&tg, &acknowledged(&tg, REQUEST, 99), Some(&neura_receipt)),
        GraphOutcome::PendingVisibility {
            commit_seq: Some(99)
        }
    );
    assert!(matches!(
        tgdrive.resolve(&tg, &lost(&tg), Some(&neura_receipt)),
        GraphOutcome::Pending { .. }
    ));
    // neuradrive's acknowledgement lends tgdrive no commit, even beside
    // tgdrive's own receipt.
    let tg_receipt = held_receipt(
        &tg_held,
        &tgdrive,
        &tg,
        vec![row_for(&tg, "accepted", accepted(EVENT))],
    );
    assert!(matches!(
        tgdrive.resolve(&tg, &acknowledged(&neura, REQUEST, 7), Some(&tg_receipt)),
        GraphOutcome::Accepted { ref event_id, commit_seq: None, .. } if event_id == EVENT
    ));
    assert!(matches!(
        tgdrive.resolve(&tg, &acknowledged(&neura, REQUEST, 7), None),
        GraphOutcome::Pending { .. }
    ));
    // Neither owner resolves, reads or sends the other's request.
    assert!(matches!(
        tgdrive.resolve(
            &neura,
            &acknowledged(&neura, REQUEST, 7),
            Some(&neura_receipt)
        ),
        GraphOutcome::Pending { .. }
    ));
    assert_eq!(
        tgdrive.receipt(&neura),
        Err(GraphRequestError::ForeignRequest)
    );
    assert_eq!(
        tgdrive.submit(&neura, &request(REQUEST, 1)),
        Err(GraphRequestError::ForeignRequest)
    );
    // Each database's own request still resolves from its own receipt,
    // answered or lost.
    assert!(matches!(
        tgdrive.resolve(&tg, &acknowledged(&tg, REQUEST, 99), Some(&tg_receipt)),
        GraphOutcome::Accepted { ref event_id, commit_seq: Some(99), .. } if event_id == EVENT
    ));
    assert!(matches!(
        tgdrive.resolve(&tg, &lost(&tg), Some(&tg_receipt)),
        GraphOutcome::Accepted { ref event_id, commit_seq: None, .. } if event_id == EVENT
    ));
    assert!(matches!(
        neuradrive.resolve(&neura, &lost(&neura), Some(&neura_receipt)),
        GraphOutcome::Accepted { ref event_id, commit_seq: None, .. } if event_id == OTHER_EVENT
    ));
}

/// Same database and payload, another request id, client or descriptor.
#[test]
fn a_receipt_speaks_only_for_its_own_request() {
    let (held, tgdrive) = owner("tgdrive");
    let mine = key(&tgdrive, REQUEST);
    let theirs = key(&tgdrive, OTHER_REQUEST);
    assert_eq!(mine.request_digest(), theirs.request_digest());
    let their_receipt = held_receipt(
        &held,
        &tgdrive,
        &theirs,
        vec![row_for(&theirs, "accepted", accepted(OTHER_EVENT))],
    );
    assert_eq!(
        tgdrive.resolve(
            &mine,
            &acknowledged(&mine, REQUEST, 9),
            Some(&their_receipt)
        ),
        GraphOutcome::PendingVisibility {
            commit_seq: Some(9)
        }
    );
    assert!(matches!(
        tgdrive.resolve(&mine, &lost(&mine), Some(&their_receipt)),
        GraphOutcome::Pending { .. }
    ));
    // Lookup reads only the request's own row, whatever else the replica holds.
    held.replicate(vec![row_for(&theirs, "accepted", accepted(OTHER_EVENT))]);
    assert_eq!(tgdrive.receipt(&mine), Ok(None));
    // Another client of the same actor, or another descriptor.
    let (other_held, other_device) = owner_as("tgdrive", "device-2");
    let other_client = key(&other_device, REQUEST);
    let other_client_receipt = held_receipt(
        &other_held,
        &other_device,
        &other_client,
        vec![row_for(&other_client, "accepted", accepted(OTHER_EVENT))],
    );
    let propose = tgdrive
        .key("graph.propose", &request(REQUEST, 1))
        .expect("key");
    let propose_receipt = held_receipt(
        &held,
        &tgdrive,
        &propose,
        vec![row_for(&propose, "accepted", accepted(OTHER_EVENT))],
    );
    for receipt in [&other_client_receipt, &propose_receipt] {
        assert!(matches!(
            tgdrive.resolve(&mine, &lost(&mine), Some(receipt)),
            GraphOutcome::Pending { .. }
        ));
    }
    assert_eq!(
        tgdrive.receipt(&other_client),
        Err(GraphRequestError::ForeignRequest)
    );
    // An acknowledgement naming another request lends no commit.
    let mine_receipt = held_receipt(
        &held,
        &tgdrive,
        &mine,
        vec![row_for(&mine, "accepted", accepted(EVENT))],
    );
    assert!(matches!(
        tgdrive.resolve(
            &mine,
            &acknowledged(&mine, OTHER_REQUEST, 9),
            Some(&mine_receipt)
        ),
        GraphOutcome::Accepted {
            commit_seq: None,
            ..
        }
    ));
    assert!(matches!(
        tgdrive.resolve(&mine, &acknowledged(&theirs, OTHER_REQUEST, 9), None),
        GraphOutcome::Pending { .. }
    ));
}

/// A key is this database's key of exactly the request sent under it.
#[test]
fn only_a_requests_own_key_sends_it() {
    let (_, tgdrive) = owner("tgdrive");
    let mine = key(&tgdrive, REQUEST);
    assert_eq!(
        tgdrive.submit(&mine, &request(REQUEST, 2)),
        Err(GraphRequestError::ForeignRequest),
        "another payload"
    );
    assert_eq!(
        tgdrive.submit(&mine, &request(OTHER_REQUEST, 1)),
        Err(GraphRequestError::ForeignRequest),
        "another request id"
    );
    // The fake replica does not sync: the attempt is the request's own,
    // unresolved, and its receipt still decides once held.
    let attempt = tgdrive.submit(&mine, &request(REQUEST, 1)).expect("sent");
    assert_eq!(attempt.result(), &Err(ReplicaError::NotSyncing));
    assert!(matches!(
        tgdrive.resolve(&mine, &attempt, None),
        GraphOutcome::Pending { .. }
    ));
}

#[test]
fn receipt_rows_decode_to_their_shape() {
    let (held, tgdrive) = owner("tgdrive");
    let mine = key(&tgdrive, REQUEST);
    let receipt = held_receipt(
        &held,
        &tgdrive,
        &mine,
        vec![row_for(&mine, "accepted", accepted(EVENT))],
    );
    assert_eq!(receipt.key(), &mine);
    assert_eq!(
        receipt.outcome(),
        &ReceiptOutcome::Accepted {
            event_id: EVENT.into(),
            graph_seq: "4".into()
        }
    );
    // Text, as a replica column holds it, and a decoded value read alike.
    let decoded = held_receipt(
        &held,
        &tgdrive,
        &mine,
        vec![row_for(
            &mine,
            "accepted",
            json!({"event_id": EVENT, "graph_seq": "4", "detail": {"ids": ["01JD0000000000000000000001"]}}),
        )],
    );
    assert_eq!(decoded, receipt);
    assert_eq!(
        held_receipt(
            &held,
            &tgdrive,
            &mine,
            vec![row_for(
                &mine,
                "rejected",
                json!({"reason": "invalid_change"})
            )]
        )
        .outcome(),
        &ReceiptOutcome::Rejected {
            reason: "invalid_change".into()
        }
    );
    for outcome in ["proposed", "conflict"] {
        assert!(Receipt::from_row("tgdrive", &row_for(&mine, outcome, json!({}))).is_ok());
    }
}

#[test]
fn receipt_rows_outside_the_shape_refuse() {
    let (held, tgdrive) = owner("tgdrive");
    let mine = key(&tgdrive, REQUEST);
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
            Receipt::from_row("tgdrive", &row_for(&mine, outcome, extra.clone())),
            Err(Refusal::UnsupportedVersion),
            "{outcome} {extra}"
        );
    }
    // The owner surfaces an unreadable own receipt rather than skipping it.
    held.replicate(vec![row_for(&mine, "pending", json!({}))]);
    assert_eq!(
        tgdrive.receipt(&mine),
        Err(GraphRequestError::Unreadable(Refusal::UnsupportedVersion))
    );
}

#[test]
fn outcome_table_separates_transport_from_acceptance() {
    let (held, tgdrive) = owner("tgdrive");
    let mine = key(&tgdrive, REQUEST);
    let receipt = |outcome: &str, extra: Value| {
        held_receipt(&held, &tgdrive, &mine, vec![row_for(&mine, outcome, extra)])
    };
    let answered = acknowledged(&mine, REQUEST, 9);
    // An acknowledgement is never acceptance on its own.
    assert_eq!(
        tgdrive.resolve(&mine, &answered, None),
        GraphOutcome::PendingVisibility {
            commit_seq: Some(9)
        }
    );
    assert!(matches!(
        tgdrive.resolve(&mine, &answered, Some(&receipt("conflict", json!({})))),
        GraphOutcome::Conflict { .. }
    ));
    assert!(matches!(
        tgdrive.resolve(&mine, &answered, Some(&receipt("proposed", json!({})))),
        GraphOutcome::Proposed { .. }
    ));
    assert!(matches!(
        tgdrive.resolve(&mine, &answered, Some(&receipt("rejected", json!({"reason": "invalid_change"})))),
        GraphOutcome::Rejected { ref reason, .. } if reason == "invalid_change"
    ));
    // The request's own receipt for another payload.
    let changed = tgdrive
        .key("graph.apply", &request(REQUEST, 2))
        .expect("key");
    let changed_receipt = held_receipt(
        &held,
        &tgdrive,
        &changed,
        vec![row_for(&changed, "accepted", accepted(EVENT))],
    );
    assert_eq!(
        tgdrive.resolve(&mine, &answered, Some(&changed_receipt)),
        GraphOutcome::IdempotencyMismatch
    );
    // HQ's refusals: the stable code is the message of `operation.forbidden`.
    assert_eq!(
        tgdrive.resolve(
            &mine,
            &refused(&mine, "operation.forbidden", "idempotency_mismatch"),
            None
        ),
        GraphOutcome::IdempotencyMismatch
    );
    assert_eq!(
        tgdrive.resolve(
            &mine,
            &refused(&mine, "operation.forbidden", "unsupported_version"),
            None
        ),
        GraphOutcome::Unsupported
    );
    assert_eq!(
        tgdrive.resolve(
            &mine,
            &refused(&mine, "kg.event_group", "kg.event_group"),
            None
        ),
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
                tgdrive.resolve(&mine, &refused(&mine, "operation.forbidden", message), None),
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
    let (held, tgdrive) = owner("tgdrive");
    let mine = key(&tgdrive, REQUEST);
    for code in [
        "kg.command_only",
        "kg.sink_audience",
        "hq.v19",
        "operation.rejected",
    ] {
        assert_eq!(
            tgdrive.resolve(&mine, &rejected(&mine, code, "[]"), None),
            GraphOutcome::TechnicalRefusal { code: code.into() },
            "{code}"
        );
    }
    assert_eq!(
        tgdrive.resolve(
            &mine,
            &rejected(&mine, "operation.forbidden", "idempotency_mismatch"),
            None
        ),
        GraphOutcome::IdempotencyMismatch
    );
    // The same code refused before dispatch leaves the request pending.
    assert!(matches!(
        tgdrive.resolve(&mine, &refused(&mine, "operation.rejected", "[]"), None),
        GraphOutcome::Pending { .. }
    ));
    // A receipt still decides over a rejected transport.
    let receipt = held_receipt(
        &held,
        &tgdrive,
        &mine,
        vec![row_for(&mine, "accepted", accepted(EVENT))],
    );
    assert!(matches!(
        tgdrive.resolve(
            &mine,
            &rejected(&mine, "kg.command_only", "kg.command_only"),
            Some(&receipt)
        ),
        GraphOutcome::Accepted {
            commit_seq: None,
            ..
        }
    ));
}

/// Each dimension of a request's key on its own: a receipt, an
/// acknowledgement or a rejection of a key differing in that one dimension
/// never speaks for the request. The unchanged key is the control.
#[test]
fn every_key_dimension_separates_requests() {
    let (held, tgdrive) = owner("tgdrive");
    let mine = key(&tgdrive, REQUEST);
    let (neura_held, neuradrive) = owner("neuradrive");
    let (other_held, other_device) = owner_as("tgdrive", "device-2");
    let variant = |owner: &GraphRequests, descriptor: &str, request_id: &str, n: u32| {
        owner.key(descriptor, &request(request_id, n)).expect("key")
    };
    // (dimension, key differing from `mine` in it alone, the replica its
    // receipt lives in, that replica's owner)
    let variants = [
        (
            "domain",
            variant(&neuradrive, "graph.apply", REQUEST, 1),
            &neura_held,
            &neuradrive,
        ),
        (
            "descriptor",
            variant(&tgdrive, "graph.propose", REQUEST, 1),
            &held,
            &tgdrive,
        ),
        (
            "request_id",
            variant(&tgdrive, "graph.apply", OTHER_REQUEST, 1),
            &held,
            &tgdrive,
        ),
        (
            "client",
            variant(&other_device, "graph.apply", REQUEST, 1),
            &other_held,
            &other_device,
        ),
        (
            "digest",
            variant(&tgdrive, "graph.apply", REQUEST, 2),
            &held,
            &tgdrive,
        ),
    ];
    // Each variant differs from `mine` in its named dimension and no other,
    // read part by part, so this precondition does not rest on the equality
    // the test exercises. The one exception is structural: the digest binds
    // the descriptor (`request::digest`), so no key a database mints differs
    // in descriptor alone.
    let parts = |key: &RequestKey| {
        [
            ("domain", key.domain().to_owned()),
            ("descriptor", key.descriptor().to_owned()),
            ("request_id", key.request_id().to_owned()),
            ("client", key.client_scope().to_owned()),
            ("digest", key.request_digest().to_owned()),
        ]
    };
    for (dimension, other, _, _) in &variants {
        let differing: Vec<&str> = parts(other)
            .iter()
            .zip(parts(&mine).iter())
            .filter(|(a, b)| a.1 != b.1)
            .map(|(a, _)| a.0)
            .collect();
        let expected = if *dimension == "descriptor" {
            vec!["descriptor", "digest"]
        } else {
            vec![*dimension]
        };
        assert_eq!(differing, expected, "{dimension}");
    }
    let ack = |key: &RequestKey| acknowledged(key, key.request_id(), 77);
    let reject = |key: &RequestKey| rejected(key, "kg.command_only", "kg.command_only");
    let receipts: Vec<(&str, RequestKey, Receipt)> = variants
        .iter()
        .map(|(dimension, other, replica, owner)| {
            let receipt = held_receipt(
                replica,
                owner,
                other,
                vec![row_for(other, "accepted", accepted(OTHER_EVENT))],
            );
            (*dimension, other.clone(), receipt)
        })
        .collect();
    let own_receipt = held_receipt(
        &held,
        &tgdrive,
        &mine,
        vec![row_for(&mine, "accepted", accepted(EVENT))],
    );

    // Control: the same key associates every way.
    assert!(matches!(
        tgdrive.resolve(&mine, &ack(&mine), Some(&own_receipt)),
        GraphOutcome::Accepted { ref event_id, commit_seq: Some(77), .. } if event_id == EVENT
    ));
    assert_eq!(
        tgdrive.resolve(&mine, &ack(&mine), None),
        GraphOutcome::PendingVisibility {
            commit_seq: Some(77)
        }
    );
    assert_eq!(
        tgdrive.resolve(&mine, &reject(&mine), None),
        GraphOutcome::TechnicalRefusal {
            code: "kg.command_only".into()
        }
    );

    for (dimension, other, receipt) in &receipts {
        // Its receipt, beside this request's lost attempt: never this
        // request's outcome. The same request under another digest is the
        // contract's idempotency mismatch.
        let by_receipt = tgdrive.resolve(&mine, &lost(&mine), Some(receipt));
        if *dimension == "digest" {
            assert_eq!(by_receipt, GraphOutcome::IdempotencyMismatch, "{dimension}");
        } else {
            assert!(
                matches!(by_receipt, GraphOutcome::Pending { .. }),
                "{dimension} receipt: {by_receipt:?}"
            );
        }
        // Its acknowledgement lends no commit and makes nothing visible.
        let acked = tgdrive.resolve(&mine, &ack(other), Some(&own_receipt));
        assert!(
            matches!(
                acked,
                GraphOutcome::Accepted {
                    commit_seq: None,
                    ..
                }
            ),
            "{dimension} acknowledgement with own receipt: {acked:?}"
        );
        let acked = tgdrive.resolve(&mine, &ack(other), None);
        assert!(
            matches!(acked, GraphOutcome::Pending { .. }),
            "{dimension} acknowledgement: {acked:?}"
        );
        // Its rejection does not end this request.
        let refused = tgdrive.resolve(&mine, &reject(other), None);
        assert!(
            matches!(refused, GraphOutcome::Pending { .. }),
            "{dimension} rejection: {refused:?}"
        );
        // And this owner resolves only its own keys.
        if matches!(*dimension, "domain" | "client") {
            assert!(matches!(
                tgdrive.resolve(other, &ack(other), Some(receipt)),
                GraphOutcome::Pending { .. }
            ));
        }
    }
}
