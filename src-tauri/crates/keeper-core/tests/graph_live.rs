//! The graph client against a real keeper-hq: two persistent native
//! replicas, no Tauri. keeper-hq's test harness starts the server, issues
//! the credentials and runs each ignored case with:
//!
//! - `KEEPER_HQ_LIVE_URL`, `KEEPER_HQ_LIVE_DATABASE`, `KEEPER_HQ_LIVE_ACTOR`;
//! - `KEEPER_HQ_LIVE_CLIENT_{1,2}` and `KEEPER_HQ_LIVE_TOKEN_{1,2}`, two
//!   devices of that person holding a graph editor role;
//! - `KEEPER_HQ_LIVE_STATE_DIR`, where the replicas persist (a restart is a
//!   second process over the same files);
//! - per case: `KEEPER_HQ_LIVE_DROP_URL` (a relay that loses command
//!   answers), `KEEPER_HQ_LIVE_PHASE` (`send` | `resume`),
//!   `KEEPER_HQ_LIVE_FORBIDDEN_DATABASE` (one the person holds no graph role
//!   in).
//!
//! Each case prints `KG1_LIVE <case> <json>` lines the harness checks
//! against the server's authoritative rows. Credentials are transient
//! environment inputs and are never printed.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use keeper_core::graph::client::{subscriptions, GraphClient, GraphClientError, Prepared};
use keeper_core::graph::outcome::GraphOutcome;
use keeper_core::graph::prefix::{self, Prefix};
use keeper_core::graph::protocol::change::{ChangeOp, NodeFields};
use keeper_core::graph::protocol::request::Request;
use keeper_core::graph::protocol::wire::HistoryFilter;
use keeper_core::hq::port::{Link, ReplicaSpec};
use keeper_core::hq::sessions::Source;
use keeper_core::hq::syncular::SyncularReplica;
use serde_json::{json, Map, Value};

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("{key} is required"))
}

fn state_dir() -> PathBuf {
    PathBuf::from(env("KEEPER_HQ_LIVE_STATE_DIR"))
}

/// Device `index`'s persistent replica of `database`, through `url`.
fn open(index: u8, database: &str, url: &str) -> Source {
    let actor = env("KEEPER_HQ_LIVE_ACTOR");
    let client = env(&format!("KEEPER_HQ_LIVE_CLIENT_{index}"));
    let replica = SyncularReplica::open(ReplicaSpec {
        database: database.to_owned(),
        path: state_dir().join(format!("{database}-{index}.sqlite")),
        client_id: Some(client.clone()),
        link: Some(Link {
            base_url: format!("{url}/{database}"),
            token: env(&format!("KEEPER_HQ_LIVE_TOKEN_{index}")),
        }),
        subscriptions: subscriptions(database, &actor, &client),
        watch_presence: None,
    })
    .expect("opens a real replica");
    Source {
        database: database.to_owned(),
        replica: Arc::new(replica),
        actor,
        client,
    }
}

fn device(index: u8) -> Source {
    open(
        index,
        &env("KEEPER_HQ_LIVE_DATABASE"),
        &env("KEEPER_HQ_LIVE_URL"),
    )
}

fn report(case: &str, value: &Value) {
    println!("KG1_LIVE {case} {value}");
}

fn ulid() -> String {
    ulid::Ulid::new().to_string()
}

/// Wait (pulling) until `done` holds on `source`'s replica.
fn until<T>(source: &Source, what: &str, mut done: impl FnMut() -> Option<T>) -> T {
    let deadline = Instant::now() + Duration::from_secs(60);
    loop {
        if let Some(value) = done() {
            return value;
        }
        assert!(Instant::now() < deadline, "{what} did not arrive");
        source.replica.sync_now();
        std::thread::sleep(Duration::from_millis(50));
    }
}

/// Resolve `prepared` until it is no longer pending.
fn settled(client: &GraphClient, source: &Source, prepared: &Prepared) -> GraphOutcome {
    until(source, "the outcome", || {
        match client.outcome(prepared, None) {
            Ok(GraphOutcome::Pending { .. } | GraphOutcome::PendingVisibility { .. }) => None,
            Ok(outcome) => Some(outcome),
            Err(error) => panic!("outcome: {error}"),
        }
    })
}

/// The complete prefix once it reaches `seq`.
fn prefix_at(source: &Source, seq: u64) -> Prefix {
    until(source, "the complete prefix", || {
        let prefix = prefix::complete(source).expect("no corruption");
        (prefix.is_complete() && prefix.n >= seq).then_some(prefix)
    })
}

fn create_node(id: &str, title: &str, properties: Value) -> ChangeOp {
    ChangeOp::CreateNode {
        id: id.to_owned(),
        kind: "Concept".to_owned(),
        title: title.to_owned(),
        description: None,
        properties: properties.as_object().expect("properties").clone(),
    }
}

fn accepted(outcome: &GraphOutcome) -> (String, u64) {
    match outcome {
        GraphOutcome::Accepted {
            event_id,
            graph_seq,
            ..
        } => (event_id.clone(), graph_seq.parse().expect("sequence")),
        other => panic!("not accepted: {other:?}"),
    }
}

fn revision(view: &Map<String, Value>, field: &str) -> String {
    view[field].as_str().expect("revision").to_owned()
}

fn outcome_json(outcome: &GraphOutcome) -> Value {
    match outcome {
        GraphOutcome::Accepted {
            event_id,
            graph_seq,
            commit_seq,
            detail,
        } => json!({"outcome": "accepted", "event_id": event_id, "graph_seq": graph_seq,
                    "commit_seq": commit_seq, "detail": detail}),
        GraphOutcome::Conflict { detail } => json!({"outcome": "conflict", "detail": detail}),
        GraphOutcome::Rejected { reason, .. } => json!({"outcome": "rejected", "reason": reason}),
        other => json!({"outcome": format!("{other:?}")}),
    }
}

/// create → receipt → replicate → typed inspect → history, on two devices;
/// replay; rename; a same-revision race; archive and the active view.
#[test]
#[ignore = "driven by keeper-hq's native graph harness"]
fn graph_live_two_devices_accept_replicate_inspect_and_race() {
    let (one, two) = (device(1), device(2));
    let (first, second) = (GraphClient::new(one.clone()), GraphClient::new(two.clone()));
    let (a, b, e1, e2) = (ulid(), ulid(), ulid(), ulid());
    let edge = |id: &str| ChangeOp::CreateEdge {
        id: id.to_owned(),
        from_id: a.clone(),
        to_id: b.clone(),
        edge_type: "test:rel".to_owned(),
        properties: Map::new(),
        from_lifecycle_revision: None,
        to_lifecycle_revision: None,
    };
    let change = first
        .change(
            vec![
                create_node(
                    &a,
                    "Alpha",
                    json!({"n": 1.0, "nothing": null, "big": "9007199254740993"}),
                ),
                create_node(&b, "Beta", json!({})),
                edge(&e1),
                edge(&e2),
                ChangeOp::AddTag {
                    node_id: a.clone(),
                    tag: "My Tag".to_owned(),
                    expected_revision: None,
                },
            ],
            None,
        )
        .expect("a change");
    let prepared = first.mint(&change).expect("minted");
    let attempt = first.send(&prepared).expect("sent");
    let outcome = first.outcome(&prepared, Some(&attempt)).expect("outcome");
    let outcome = match outcome {
        GraphOutcome::PendingVisibility { .. } => settled(&first, &one, &prepared),
        other => other,
    };
    let (_, seq) = accepted(&outcome);
    report("create", &outcome_json(&outcome));

    // The same request again: the original receipt, no second event.
    let replay = first.send(&prepared).expect("replayed");
    report(
        "replay",
        &json!({"cached": replay.result().as_ref().map(|r| r.cached).ok(),
                "outcome": outcome_json(&settled(&first, &one, &prepared))}),
    );

    // Both devices hold the event in a complete prefix, not merely equal rows.
    for (name, source) in [("one", &one), ("two", &two)] {
        let prefix = prefix_at(source, seq);
        let node = &prefix.state[&("node".to_owned(), a.clone())];
        report(
            &format!("prefix-{name}"),
            &json!({"n": prefix.n, "head": prefix.head, "node": node,
                    "edges": [prefix.state.contains_key(&("edge".to_owned(), e1.clone())),
                              prefix.state.contains_key(&("edge".to_owned(), e2.clone()))]}),
        );
    }
    let view = second.get_node(&a, false).expect("read").expect("the node");
    report("get_node", &json!({"image": view.image, "tags": view.tags}));
    let parallel = [&e1, &e2].map(|id| second.get_edge(id, false).expect("read"));
    report("get_edges", &json!(parallel));

    // Rename keeps the id; the revision moves once.
    let rename = first
        .change(
            vec![ChangeOp::UpdateNode {
                id: a.clone(),
                expected_revision: revision(&view.image, "revision"),
                set: NodeFields {
                    title: Some("Alpha renamed".to_owned()),
                    description: None,
                },
                set_properties: Map::new(),
                remove_properties: vec!["nothing".to_owned()],
            }],
            None,
        )
        .expect("rename");
    let (prepared, _) = first.apply(&rename).expect("applied");
    let renamed = settled(&first, &one, &prepared);
    report("rename", &outcome_json(&renamed));
    let current = first.get_node(&a, false).expect("read").expect("the node");

    // Two devices, one expected revision: exactly one is accepted, the other
    // conflicts with the current revision.
    let racer = |client: &GraphClient, title: &str| {
        client
            .change(
                vec![ChangeOp::UpdateNode {
                    id: a.clone(),
                    expected_revision: revision(&current.image, "revision"),
                    set: NodeFields {
                        title: Some(title.to_owned()),
                        description: None,
                    },
                    set_properties: Map::new(),
                    remove_properties: Vec::new(),
                }],
                None,
            )
            .expect("race change")
    };
    let (left, right) = (racer(&first, "left"), racer(&second, "right"));
    let (left, right) = std::thread::scope(|scope| {
        let l = scope.spawn(|| first.apply(&left).expect("left"));
        let r = scope.spawn(|| second.apply(&right).expect("right"));
        (
            l.join().expect("left thread"),
            r.join().expect("right thread"),
        )
    });
    report(
        "race",
        &json!([
            outcome_json(&settled(&first, &one, &left.0)),
            outcome_json(&settled(&second, &two, &right.0))
        ]),
    );

    // Archive: the default view hides the node and the edges into it.
    let now = first.get_node(&b, false).expect("read").expect("beta");
    let archive = first
        .change(
            vec![ChangeOp::ArchiveNode {
                id: b.clone(),
                expected_revision: revision(&now.image, "revision"),
            }],
            None,
        )
        .expect("archive");
    let (prepared, _) = first.apply(&archive).expect("applied");
    report("archive", &outcome_json(&settled(&first, &one, &prepared)));
    report(
        "archived-view",
        &json!({"node": first.get_node(&b, false).expect("read").is_some(),
                "node_archived": first.get_node(&b, true).expect("read").map(|v| v.image),
                "edge": first.get_edge(&e1, false).expect("read").is_some(),
                "edge_archived": first.get_edge(&e1, true).expect("read")}),
    );

    // History filtered to the node: whole groups, a cursor that moves.
    let page = first
        .history(
            "0",
            100,
            &HistoryFilter {
                entity_id: Some(a.clone()),
                ..HistoryFilter::default()
            },
        )
        .expect("history");
    report(
        "history",
        &json!({"scanned_through": page.scanned_through,
                "events": page.events.iter().map(|e| json!({"seq": e.event.graph_seq,
                    "ops": e.ops.len(), "origin_class": e.event.origin_class,
                    "approval_basis": e.event.approval_basis})).collect::<Vec<_>>()}),
    );
    for source in [one, two] {
        source.replica.close();
        source.replica.join();
    }
}

/// A command whose answer is lost resolves under the same id through the
/// receipt or `graph.outcome`; never a second request.
#[test]
#[ignore = "driven by keeper-hq's native graph harness"]
fn graph_live_a_lost_answer_resolves_by_outcome() {
    let lossy = open(
        1,
        &env("KEEPER_HQ_LIVE_DATABASE"),
        &env("KEEPER_HQ_LIVE_DROP_URL"),
    );
    let direct = device(1);
    let (through_relay, client) = (
        GraphClient::new(lossy.clone()),
        GraphClient::new(direct.clone()),
    );
    let change = client
        .change(vec![create_node(&ulid(), "Lost", json!({}))], None)
        .expect("change");
    let prepared = through_relay.mint(&change).expect("minted");
    let attempt = through_relay.send(&prepared).expect("sent");
    let first = through_relay
        .outcome(&prepared, Some(&attempt))
        .expect("outcome");
    report(
        "lost-attempt",
        &json!({"transport": format!("{:?}", attempt.result()), "first": outcome_json(&first)}),
    );
    let restored = client
        .prepared(prepared.request().clone())
        .expect("same request");
    report(
        "lost-resolved",
        &outcome_json(&settled(&client, &direct, &restored)),
    );
    for source in [lossy, direct] {
        source.replica.close();
        source.replica.join();
    }
}

/// Phase `send` mints, persists and sends, then the harness kills or ends
/// the process; phase `resume` reopens the same replica file and resolves
/// the persisted request with no attempt in hand, then pages history from a
/// persisted cursor.
#[test]
#[ignore = "driven by keeper-hq's native graph harness"]
fn graph_live_a_restart_resumes_the_same_request_and_cursor() {
    let pending = state_dir().join("pending-request.json");
    let cursor = state_dir().join("history-cursor");
    let source = device(1);
    let client = GraphClient::new(source.clone());
    match env("KEEPER_HQ_LIVE_PHASE").as_str() {
        "send" => {
            let change = client
                .change(
                    vec![create_node(&ulid(), "Before restart", json!({}))],
                    None,
                )
                .expect("change");
            let prepared = client.mint(&change).expect("minted");
            // Persisted before the first send.
            std::fs::write(
                &pending,
                json!({"request_id": prepared.request().request_id,
                       "payload": prepared.request().payload})
                .to_string(),
            )
            .expect("persisted");
            let page = client
                .history("0", 100, &HistoryFilter::default())
                .expect("history");
            std::fs::write(&cursor, &page.scanned_through).expect("cursor");
            report(
                "sent",
                &json!({"request_id": prepared.request().request_id,
                                    "cursor": page.scanned_through,
                                    "transport": format!("{:?}", client.send(&prepared).map(|a| a.result().is_ok()))}),
            );
        }
        "resume" => {
            let saved: Value =
                serde_json::from_str(&std::fs::read_to_string(&pending).expect("persisted"))
                    .expect("json");
            let prepared = client
                .prepared(Request {
                    request_id: saved["request_id"].as_str().expect("id").to_owned(),
                    context_id: None,
                    payload: saved["payload"].clone(),
                })
                .expect("restored");
            // The same id is sent again: the server answers from its record.
            let _ = client.send(&prepared);
            let outcome = settled(&client, &source, &prepared);
            let after = std::fs::read_to_string(&cursor).expect("cursor");
            let page = client
                .history(after.trim(), 100, &HistoryFilter::default())
                .expect("history");
            report(
                "resumed",
                &json!({"outcome": outcome_json(&outcome),
                    "cursor": after.trim(), "scanned_through": page.scanned_through,
                    "events": page.events.iter().map(|e| e.event.request_id.clone()).collect::<Vec<_>>()}),
            );
        }
        phase => panic!("unknown phase {phase}"),
    }
    source.replica.close();
    source.replica.join();
}

/// A database the person holds no graph role in: nothing is accepted and
/// nothing is told about what exists; an unlinked replica refuses before
/// sending.
#[test]
#[ignore = "driven by keeper-hq's native graph harness"]
fn graph_live_a_forbidden_domain_tells_nothing_and_offline_sends_nothing() {
    let forbidden = open(
        1,
        &env("KEEPER_HQ_LIVE_FORBIDDEN_DATABASE"),
        &env("KEEPER_HQ_LIVE_URL"),
    );
    let client = GraphClient::new(forbidden.clone());
    let change = client
        .change(vec![create_node(&ulid(), "Probe", json!({}))], None)
        .expect("change");
    let prepared = client.mint(&change).expect("minted");
    let attempt = client.send(&prepared).expect("sent");
    report(
        "forbidden",
        &json!({"transport": format!("{:?}", attempt.result()),
                "outcome": outcome_json(&client.outcome(&prepared, Some(&attempt)).expect("outcome")),
                "get_node": format!("{:?}", client.get_node(&ulid(), true)),
                "history": format!("{:?}", client.history("0", 10, &HistoryFilter::default()).map(|p| p.events.len()))}),
    );
    forbidden.replica.close();
    forbidden.replica.join();

    let local = SyncularReplica::open(ReplicaSpec {
        database: env("KEEPER_HQ_LIVE_DATABASE"),
        path: state_dir().join("offline.sqlite"),
        client_id: Some(env("KEEPER_HQ_LIVE_CLIENT_1")),
        link: None,
        subscriptions: Vec::new(),
        watch_presence: None,
    })
    .expect("opens");
    let offline = GraphClient::new(Source {
        database: env("KEEPER_HQ_LIVE_DATABASE"),
        replica: Arc::new(local),
        actor: env("KEEPER_HQ_LIVE_ACTOR"),
        client: env("KEEPER_HQ_LIVE_CLIENT_1"),
    });
    let refused = offline.apply(&change);
    assert_eq!(refused.as_ref().err(), Some(&GraphClientError::Offline));
    report(
        "offline",
        &json!({"refused": format!("{:?}", refused.err())}),
    );
}
