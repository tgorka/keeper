//! The graph client against a real keeper-hq: persistent native replicas,
//! no Tauri. keeper-hq's harness (`server/test/graph-native.test.ts`) starts
//! the server, issues the credentials and runs each ignored case with:
//!
//! - `KEEPER_HQ_LIVE_URL`, `KEEPER_HQ_LIVE_DATABASE`, `KEEPER_HQ_LIVE_ACTOR`;
//! - `KEEPER_HQ_LIVE_CLIENT_{1,2}` and `KEEPER_HQ_LIVE_TOKEN_{1,2}`, two
//!   devices of that person holding a graph editor role;
//! - `KEEPER_HQ_LIVE_STATE_DIR`, where the replicas persist (a restart is a
//!   second process over the same files);
//! - per case: `KEEPER_HQ_LIVE_DROP_URL` (a relay that answers no command:
//!   the command reaches the server and its answer is lost),
//!   `KEEPER_HQ_LIVE_PHASE` (`send` | `resume`),
//!   `KEEPER_HQ_LIVE_FORBIDDEN_DATABASE` (one the person holds no graph role
//!   in) and `KEEPER_HQ_LIVE_FORBIDDEN_NODE` (a node retained there).
//!
//! Every claim is asserted here; each case also prints `KG1_LIVE <case>
//! <json>` lines the harness checks against the server's authoritative
//! rows. Credentials are transient environment inputs and are never
//! printed.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use keeper_core::graph::client::{subscriptions, GraphClient, GraphClientError, Prepared};
use keeper_core::graph::outcome::GraphOutcome;
use keeper_core::graph::prefix::{self, Prefix};
use keeper_core::graph::protocol::change::{Change, ChangeOp, NodeFields};
use keeper_core::graph::protocol::request::Request;
use keeper_core::graph::protocol::wire::HistoryFilter;
use keeper_core::hq::port::{Link, ReplicaError, ReplicaSpec};
use keeper_core::hq::sessions::Source;
use keeper_core::hq::syncular::SyncularReplica;
use serde_json::{json, Map, Value};

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("{key} is required"))
}

fn state_dir() -> PathBuf {
    PathBuf::from(env("KEEPER_HQ_LIVE_STATE_DIR"))
}

/// Device `index`'s replica of `database` in file `name`, through `url`.
fn open_at(index: u8, database: &str, url: &str, name: &str) -> Source {
    let actor = env("KEEPER_HQ_LIVE_ACTOR");
    let client = env(&format!("KEEPER_HQ_LIVE_CLIENT_{index}"));
    let replica = SyncularReplica::open(ReplicaSpec {
        database: database.to_owned(),
        path: state_dir().join(name),
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

fn open(index: u8, database: &str, url: &str) -> Source {
    open_at(index, database, url, &format!("{database}-{index}.sqlite"))
}

fn device(index: u8) -> Source {
    open(
        index,
        &env("KEEPER_HQ_LIVE_DATABASE"),
        &env("KEEPER_HQ_LIVE_URL"),
    )
}

fn close(source: Source) {
    source.replica.close();
    source.replica.join();
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

/// Apply `change` and wait for its terminal outcome.
fn applied(client: &GraphClient, source: &Source, change: &Change) -> (Prepared, GraphOutcome) {
    let applied = client.apply(change).expect("a request id");
    let outcome = settled(client, source, &applied.prepared);
    (applied.prepared, outcome)
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

fn retitle(id: &str, expected: &str, title: &str) -> ChangeOp {
    ChangeOp::UpdateNode {
        id: id.to_owned(),
        expected_revision: expected.to_owned(),
        set: NodeFields {
            title: Some(title.to_owned()),
            description: None,
        },
        set_properties: Map::new(),
        remove_properties: Vec::new(),
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

/// The events of a prefix that answer `request_id`.
fn events_of(prefix: &Prefix, request_id: &str) -> usize {
    prefix
        .events
        .iter()
        .filter(|event| event.event.request_id == request_id)
        .count()
}

/// create → receipt → replicate → typed inspect → history, on two devices;
/// replay; rename; a same-revision race; archive and the active view; and a
/// fresh replica bootstrapping the whole history late.
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
    assert!(attempt.result().is_ok(), "{:?}", attempt.result());
    let outcome = settled(&first, &one, &prepared);
    let (_, seq) = accepted(&outcome);
    report("create", &outcome_json(&outcome));

    // The same request again: the server's record answers, no second event.
    let replay = first.send(&prepared).expect("replayed");
    assert_eq!(replay.result().as_ref().map(|r| r.cached), Ok(true));
    assert_eq!(settled(&first, &one, &prepared), outcome);

    // Both devices hold the event in a complete prefix, not merely equal rows.
    for (name, source) in [("one", &one), ("two", &two)] {
        let prefix = prefix_at(source, seq);
        assert_eq!(events_of(&prefix, &prepared.request().request_id), 1);
        let node = &prefix.state[&("node".to_owned(), a.clone())];
        assert_eq!(
            node["properties"],
            json!({"n": 1, "nothing": null, "big": "9007199254740993"})
        );
        for id in [&e1, &e2] {
            assert!(prefix.state.contains_key(&("edge".to_owned(), id.clone())));
        }
        report(
            &format!("prefix-{name}"),
            &json!({"n": prefix.n, "head": prefix.head}),
        );
    }
    let view = second.get_node(&a, false).expect("read").expect("the node");
    assert_eq!(view.tags, ["my-tag"]);
    for id in [&e1, &e2] {
        assert!(second.get_edge(id, false).expect("read").is_some());
    }

    // Rename keeps the id; the revision moves once; the removed property
    // survives in the event's before-image.
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
    let (renamed_request, renamed) = applied(&first, &one, &rename);
    accepted(&renamed);
    let current = first.get_node(&a, false).expect("read").expect("the node");
    assert_eq!(current.image["id"], a.as_str());
    assert_eq!(current.image["title"], "Alpha renamed");
    assert!(current.image["properties"].get("nothing").is_none());

    // Two devices, one expected revision: exactly one is accepted, the other
    // conflicts naming the winner's revision.
    let expected = revision(&current.image, "revision");
    let left = first
        .change(vec![retitle(&a, &expected, "left")], None)
        .expect("left");
    let right = second
        .change(vec![retitle(&a, &expected, "right")], None)
        .expect("right");
    let (left, right) = std::thread::scope(|scope| {
        let l = scope.spawn(|| applied(&first, &one, &left).1);
        let r = scope.spawn(|| applied(&second, &two, &right).1);
        (l.join().expect("left"), r.join().expect("right"))
    });
    report("race", &json!([outcome_json(&left), outcome_json(&right)]));
    let (winner, loser) = match (&left, &right) {
        (GraphOutcome::Accepted { .. }, GraphOutcome::Conflict { detail })
        | (GraphOutcome::Conflict { detail }, GraphOutcome::Accepted { .. }) => (
            first.get_node(&a, false).expect("read").expect("node"),
            detail.clone(),
        ),
        other => panic!("not one accepted and one conflict: {other:?}"),
    };
    assert_eq!(loser["current_revision"], winner.image["revision"]);
    assert_eq!(loser["precondition"], "revision");

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
    let (_, archived) = applied(&first, &one, &archive);
    let (_, last) = accepted(&archived);
    assert!(first.get_node(&b, false).expect("read").is_none());
    let hidden = first
        .get_node(&b, true)
        .expect("read")
        .expect("archived node");
    assert_eq!(hidden.image["lifecycle"], "archived");
    assert!(first.get_edge(&e1, false).expect("read").is_none());
    assert!(first.get_edge(&e1, true).expect("read").is_some());

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
    let requests: Vec<&str> = page
        .events
        .iter()
        .map(|e| e.event.request_id.as_str())
        .collect();
    assert!(requests.contains(&prepared.request().request_id.as_str()));
    assert!(requests.contains(&renamed_request.request().request_id.as_str()));
    assert!(page.scanned_through.parse::<u64>().expect("cursor") >= last);
    assert!(page
        .events
        .iter()
        .all(|e| e.ops.iter().any(|op| op.entity_id == a)));

    // A replica opened only now bootstraps every graph table at once (skewed,
    // foreign keys and all) and still reaches the same complete prefix.
    let reference = prefix_at(&one, last);
    close(two);
    let late = open_at(
        2,
        &env("KEEPER_HQ_LIVE_DATABASE"),
        &env("KEEPER_HQ_LIVE_URL"),
        &format!("late-{}.sqlite", ulid()),
    );
    let fresh = prefix_at(&late, reference.n);
    // Equal history up to the reference position (later events, if any
    // writer added some, may follow), and equal state at equal positions.
    assert_eq!(
        &fresh.events[..reference.events.len()],
        &reference.events[..]
    );
    if fresh.n == reference.n {
        assert_eq!(fresh.state, reference.state);
    }
    report(
        "late-bootstrap",
        &json!({"n": fresh.n, "entities": fresh.state.len()}),
    );
    close(one);
    close(late);
}

/// A command whose answer is lost resolves under the same id through the
/// receipt or `graph.outcome`, into exactly one event; never a second
/// request.
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
    assert!(
        matches!(attempt.result(), Err(ReplicaError::Unheard { .. })),
        "the answer was not lost: {:?}",
        attempt.result()
    );
    // Only a receipt can settle it now: the lost answer alone is pending,
    // or the receipt has already replicated.
    let first = through_relay
        .outcome(&prepared, Some(&attempt))
        .expect("outcome");
    assert!(
        matches!(
            first,
            GraphOutcome::Pending { .. } | GraphOutcome::Accepted { .. }
        ),
        "{first:?}"
    );
    let restored = client
        .prepared(prepared.request().clone())
        .expect("same request");
    assert_eq!(restored.key(), prepared.key());
    let outcome = settled(&client, &direct, &restored);
    let (_, seq) = accepted(&outcome);
    let prefix = prefix_at(&direct, seq);
    assert_eq!(events_of(&prefix, &prepared.request().request_id), 1);
    report("lost-resolved", &outcome_json(&outcome));
    close(lossy);
    close(direct);
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
            let sent = client.send(&prepared).map(|a| a.result().is_ok());
            report(
                "sent",
                &json!({"request_id": prepared.request().request_id,
                        "cursor": page.scanned_through, "transport": format!("{sent:?}")}),
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
            let (_, seq) = accepted(&outcome);
            assert_eq!(
                events_of(&prefix_at(&source, seq), &prepared.request().request_id),
                1
            );
            let after = std::fs::read_to_string(&cursor).expect("cursor");
            let page = client
                .history(after.trim(), 100, &HistoryFilter::default())
                .expect("history");
            assert!(page
                .events
                .iter()
                .any(|e| e.event.request_id == prepared.request().request_id));
            report(
                "resumed",
                &json!({"outcome": outcome_json(&outcome), "cursor": after.trim(),
                        "scanned_through": page.scanned_through}),
            );
        }
        phase => panic!("unknown phase {phase}"),
    }
    close(source);
}

/// A database the person holds no graph role in: nothing is accepted, and a
/// node retained there reads exactly like one that does not exist — through
/// inspection, history and outcome. An unlinked replica refuses before
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
    let retained = env("KEEPER_HQ_LIVE_FORBIDDEN_NODE");
    let absent = ulid();
    let probe = |id: &str, archived: bool| format!("{:?}", client.get_node(id, archived));
    for archived in [false, true] {
        let (known, unknown) = (probe(&retained, archived), probe(&absent, archived));
        assert!(known.starts_with("Err("), "{known}");
        assert_eq!(
            known, unknown,
            "a retained node reads differently from an absent one"
        );
    }
    let history = client.history("0", 10, &HistoryFilter::default());
    assert!(history.is_err(), "{history:?}");

    let change = client
        .change(vec![create_node(&ulid(), "Probe", json!({}))], None)
        .expect("change");
    let applied = client.apply(&change).expect("a request id");
    let attempt = applied.attempt.as_ref().expect("sent");
    assert!(attempt.result().is_err(), "{:?}", attempt.result());
    let outcome = applied.outcome.as_ref().expect("readable");
    assert!(
        !matches!(outcome, GraphOutcome::Accepted { .. }),
        "{outcome:?}"
    );
    report(
        "forbidden",
        &json!({"transport": format!("{:?}", attempt.result()),
                "outcome": outcome_json(outcome)}),
    );
    close(forbidden);

    let local = SyncularReplica::open(ReplicaSpec {
        database: env("KEEPER_HQ_LIVE_DATABASE"),
        path: state_dir().join(format!("offline-{}.sqlite", ulid())),
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
