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
//!   in), `KEEPER_HQ_LIVE_FORBIDDEN_NODE` (a node retained there) and
//!   `KEEPER_HQ_LIVE_FORBIDDEN_REQUEST` (the request id of an accepted change
//!   another person made there).
//!
//! - 112 review cases (story 111.2): `KEEPER_HQ_LIVE_REVIEWER_ACTOR`,
//!   `KEEPER_HQ_LIVE_REVIEWER_CLIENT` and `KEEPER_HQ_LIVE_REVIEWER_TOKEN`, a
//!   second person holding the graph reviewer role, and
//!   `KEEPER_HQ_LIVE_AGENT_EVENT`, the event id of an agent-origin
//!   `graph.apply` (an approved create) the harness seeded and nobody has
//!   reviewed.
//!
//! Every claim is asserted here; each case also prints the `KG1_LIVE <case>
//! <json>` lines the harness checks against the server's authoritative rows.
//! Their names and shapes are the harness's interface: change neither
//! without it. Credentials are transient environment inputs and are never
//! printed.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use keeper_core::graph::client::{subscriptions, GraphClient, GraphClientError, Prepared};
use keeper_core::graph::outcome::GraphOutcome;
use keeper_core::graph::prefix::{self, Prefix};
use keeper_core::graph::protocol::change::{Change, ChangeOp, NodeFields, DESCRIPTOR};
use keeper_core::graph::protocol::inverse::{Preview, PreviewOutcome, Reverse};
use keeper_core::graph::protocol::proposal::{self, Decision, Proposal, Verdict};
use keeper_core::graph::protocol::request::Request;
use keeper_core::graph::protocol::review::Target;
use keeper_core::graph::protocol::wire::{self, HistoryFilter};
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
            description_source: None,
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
    let cached = replay.result().as_ref().map(|r| r.cached);
    assert_eq!(cached, Ok(true));
    let replayed = settled(&first, &one, &prepared);
    assert_eq!(replayed, outcome);
    report(
        "replay",
        &json!({"cached": cached.ok(), "outcome": outcome_json(&replayed)}),
    );

    // Both devices hold the event in a complete prefix, not merely equal rows.
    for (name, source) in [("one", &one), ("two", &two)] {
        let prefix = prefix_at(source, seq);
        assert_eq!(events_of(&prefix, &prepared.request().request_id), 1);
        let node = &prefix.state[&("node".to_owned(), a.clone())];
        assert_eq!(
            node["properties"],
            json!({"n": 1, "nothing": null, "big": "9007199254740993"})
        );
        let edges =
            [&e1, &e2].map(|id| prefix.state.contains_key(&("edge".to_owned(), id.clone())));
        assert_eq!(edges, [true, true]);
        report(
            &format!("prefix-{name}"),
            &json!({"n": prefix.n, "head": prefix.head, "edges": edges}),
        );
    }
    let view = second.get_node(&a, false).expect("read").expect("the node");
    assert_eq!(view.tags, ["my-tag"]);
    report("get_node", &json!({"image": view.image, "tags": view.tags}));
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
                    description_source: None,
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
    let shown = first.get_node(&b, false).expect("read").is_some();
    let hidden = first
        .get_node(&b, true)
        .expect("read")
        .expect("archived node");
    assert!(!shown);
    assert_eq!(hidden.image["lifecycle"], "archived");
    let edge_shown = first.get_edge(&e1, false).expect("read").is_some();
    let edge_hidden = first.get_edge(&e1, true).expect("read");
    assert!(!edge_shown);
    assert!(edge_hidden.is_some());
    report(
        "archived-view",
        &json!({"node": shown, "node_archived": hidden.image, "edge": edge_shown,
                "edge_archived": edge_hidden}),
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
    report(
        "history",
        &json!({"scanned_through": page.scanned_through,
                "events": page.events.iter().map(|e| json!({"seq": e.event.graph_seq,
                    "ops": e.ops.len(), "origin_class": e.event.origin_class,
                    "approval_basis": e.event.approval_basis})).collect::<Vec<_>>()}),
    );

    // A replica opened only now bootstraps every graph table from scratch and
    // reaches the same complete prefix. This is a fresh late bootstrap, not a
    // controlled skewed-delivery proof: nothing here withholds a table or an
    // operation (the local missing/partial-group tests are that evidence).
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
        .prepared(DESCRIPTOR, prepared.request().clone())
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

/// Phase `send` mints and persists a request, sends it and waits until it
/// is accepted (receipt and event), recording that, then the process ends.
/// Phase `resume` reopens the same replica file and first resolves the
/// persisted request with no attempt in hand and **no resend** — the
/// original acceptance, recovered — then proves the cached replay
/// separately, then pages history from the persisted cursor.
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
            let page = client
                .history("0", 100, &HistoryFilter::default())
                .expect("history");
            std::fs::write(&cursor, &page.scanned_through).expect("cursor");
            // Persisted before the first send.
            let mut saved = json!({"request_id": prepared.request().request_id,
                                   "payload": prepared.request().payload});
            std::fs::write(&pending, saved.to_string()).expect("persisted");
            let attempt = client.send(&prepared).expect("sent");
            assert!(attempt.result().is_ok(), "{:?}", attempt.result());
            // The original acceptance, established before this process ends.
            let outcome = settled(&client, &source, &prepared);
            let (event_id, seq) = accepted(&outcome);
            assert_eq!(
                events_of(&prefix_at(&source, seq), &prepared.request().request_id),
                1
            );
            saved["event_id"] = json!(event_id);
            std::fs::write(&pending, saved.to_string()).expect("recorded");
            report(
                "sent",
                &json!({"request_id": prepared.request().request_id, "event_id": event_id,
                        "graph_seq": seq.to_string(), "cursor": page.scanned_through}),
            );
        }
        "resume" => {
            let saved: Value =
                serde_json::from_str(&std::fs::read_to_string(&pending).expect("persisted"))
                    .expect("json");
            let prepared = client
                .prepared(
                    DESCRIPTOR,
                    Request {
                        request_id: saved["request_id"].as_str().expect("id").to_owned(),
                        context_id: None,
                        payload: saved["payload"].clone(),
                    },
                )
                .expect("restored");
            // Resolved before anything is sent again: the persisted identity
            // finds the original receipt.
            let recovered = settled(&client, &source, &prepared);
            let (event_id, seq) = accepted(&recovered);
            assert_eq!(saved["event_id"], event_id.as_str());
            // Then the same id sent again: the server's record answers, and
            // nothing new is accepted.
            let replay = client.send(&prepared).expect("resent");
            let cached = replay.result().as_ref().map(|r| r.cached);
            assert_eq!(cached, Ok(true));
            assert_eq!(settled(&client, &source, &prepared), recovered);
            assert_eq!(
                events_of(&prefix_at(&source, seq), &prepared.request().request_id),
                1
            );
            let after = std::fs::read_to_string(&cursor).expect("cursor");
            let page = client
                .history(after.trim(), 100, &HistoryFilter::default())
                .expect("history");
            let events: Vec<String> = page
                .events
                .iter()
                .map(|e| e.event.request_id.clone())
                .collect();
            assert!(events.contains(&prepared.request().request_id));
            report(
                "resumed",
                &json!({"outcome": outcome_json(&recovered), "cached": cached.ok(),
                        "cursor": after.trim(), "scanned_through": page.scanned_through,
                        "events": events}),
            );
        }
        phase => panic!("unknown phase {phase}"),
    }
    close(source);
}

/// The refusal a caller without graph read authority gets from every graph
/// read: `operation.forbidden` with the stable code `forbidden`, as the
/// native client surfaces it.
const FORBIDDEN: &str = "operation.forbidden: forbidden";

/// A database the person holds no graph role in: nothing is accepted, and
/// the native client sees the same forbidden refusal for what is retained
/// there — a node, another person's accepted request — as for what does not
/// exist, through inspection, outcome and history. Any other answer (a
/// transport error, `NotSyncing`, rows) fails the case. This is what the
/// native client receives; equality of the complete server responses
/// (status and body) is keeper-hq's own control (`graph-apply.test.ts`).
/// An unlinked replica refuses before sending.
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
    let retained_request = env("KEEPER_HQ_LIVE_FORBIDDEN_REQUEST");
    let (absent, absent_request) = (ulid(), ulid());
    // The server's answer (CC-6): `Remote`, never a client failure.
    let refused = |what: &str, operation: &str, params: Value| match forbidden
        .replica
        .remote_query(operation, params)
    {
        Err(error @ ReplicaError::Remote { .. }) if error.to_string() == FORBIDDEN => {}
        other => panic!("{what}: not the forbidden refusal: {other:?}"),
    };
    for (what, request) in [
        ("retained outcome", &retained_request),
        ("absent outcome", &absent_request),
    ] {
        refused(what, wire::OUTCOME, wire::outcome_params(None, request));
    }
    let filter = |entity: &str| HistoryFilter {
        entity_id: Some(entity.to_owned()),
        ..HistoryFilter::default()
    };
    for (what, id) in [("retained", &retained), ("absent", &absent)] {
        for archived in [false, true] {
            refused(what, wire::GET_NODE, wire::get_params(None, id, archived));
            assert!(
                matches!(
                    &client.get_node(id, archived),
                    Err(GraphClientError::Replica(e @ ReplicaError::Remote { .. })) if e.to_string() == FORBIDDEN
                ),
                "{what} node through the client"
            );
        }
        refused(
            what,
            wire::HISTORY,
            wire::history_params(None, "0", 10, &filter(id)).expect("params"),
        );
    }
    assert!(
        matches!(
            &client.history("0", 10, &HistoryFilter::default()),
            Err(GraphClientError::Replica(e @ ReplicaError::Remote { .. })) if e.to_string() == FORBIDDEN
        ),
        "history through the client"
    );
    report("forbidden-reads", &json!({"refusal": FORBIDDEN}));

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

fn rejected_reason(outcome: &GraphOutcome) -> Option<&str> {
    match outcome {
        GraphOutcome::Rejected { reason, .. } => Some(reason.as_str()),
        _ => None,
    }
}

/// Story 111.1 on two devices of an editor: a collection with two parents
/// (add, reorder, remove), persisted content across a restart and its
/// stale-revision conflict, a merge whose redirect is reported and whose
/// content stays with the absorbed identity, a person's verification bound
/// to the checked state (and its stale-state conflict), and refusals with
/// no effect: a credential-bearing locator (refused by the client before it
/// is sent) and a
/// `Vocabulary` node, which an editor may not write (owner only).
#[test]
#[ignore = "driven by keeper-hq's native graph harness (111.1 server)"]
#[allow(clippy::too_many_lines)]
fn graph_live_111_collections_content_merge_and_verification() {
    let (one, two) = (device(1), device(2));
    let (first, second) = (GraphClient::new(one.clone()), GraphClient::new(two.clone()));
    let [p1, p2, k, c, x, t, claim, res, e1, e2, e3, e4, src] = std::array::from_fn(|_| ulid());
    let contains = |id: &str, from: &str, key: &str| ChangeOp::CreateEdge {
        id: id.to_owned(),
        from_id: from.to_owned(),
        to_id: k.clone(),
        edge_type: "contains".to_owned(),
        properties: json!({"order_key": key}).as_object().expect("p").clone(),
        from_lifecycle_revision: None,
        to_lifecycle_revision: None,
    };
    let collection = |id: &str| ChangeOp::CreateNode {
        id: id.to_owned(),
        kind: "Collection".to_owned(),
        title: "C".to_owned(),
        description: None,
        properties: Map::new(),
    };
    let assertion = json!({"generated_at": null, "observed_at": 1, "verified_at": null,
                           "verified_by": null, "stale_after": null, "status": "unverified"});
    let setup = first
        .change(
            vec![
                collection(&p1),
                collection(&p2),
                create_node(&k, "Member", json!({})),
                contains(&e1, &p1, "b"),
                contains(&e2, &p2, "a"),
                create_node(&x, "Absorbed", json!({})),
                create_node(&t, "Survivor", json!({})),
                ChangeOp::CreateContent {
                    id: c.clone(),
                    owner_node_id: x.clone(),
                    body: json!({"inline_text": "v1"}),
                },
                ChangeOp::CreateNode {
                    id: res.clone(),
                    kind: "Resource".to_owned(),
                    title: "Source".to_owned(),
                    description: None,
                    properties: Map::new(),
                },
                create_node(&claim, "Claim", json!({"$assertion": assertion})),
                ChangeOp::CreateEdge {
                    id: src.clone(),
                    from_id: claim.clone(),
                    to_id: res.clone(),
                    edge_type: "sources".to_owned(),
                    properties: json!({"source": {"$ref": {"entity_type": "node", "id": res, "revision": "1"}}})
                        .as_object()
                        .expect("p")
                        .clone(),
                    from_lifecycle_revision: None,
                    to_lifecycle_revision: None,
                },
            ],
            None,
        )
        .expect("setup");
    let (_, outcome) = applied(&first, &one, &setup);
    let (_, seq) = accepted(&outcome);
    report("111-setup", &outcome_json(&outcome));
    for source in [&one, &two] {
        let prefix = prefix_at(source, seq);
        let content = &prefix.state[&("content".to_owned(), c.clone())];
        assert_eq!(
            (
                content["owner_node_id"].as_str(),
                content["revision"].as_str()
            ),
            (Some(x.as_str()), Some("1"))
        );
    }

    // Reorder is a property edit of one membership; remove archives it.
    let reorder = second
        .change(
            vec![ChangeOp::UpdateEdge {
                id: e1.clone(),
                expected_revision: "1".into(),
                set_properties: json!({"order_key": "0"}).as_object().expect("p").clone(),
                remove_properties: Vec::new(),
                from_lifecycle_revision: "1".into(),
                to_lifecycle_revision: "1".into(),
            }],
            None,
        )
        .expect("reorder");
    accepted(&applied(&second, &two, &reorder).1);
    let remove = first
        .change(
            vec![ChangeOp::ArchiveEdge {
                id: e2.clone(),
                expected_revision: "1".into(),
                from_lifecycle_revision: "1".into(),
                to_lifecycle_revision: "1".into(),
            }],
            None,
        )
        .expect("remove");
    let (_, removed) = applied(&first, &one, &remove);
    let (_, seq) = accepted(&removed);
    let prefix = prefix_at(&two, seq);
    let edge = |id: &String| prefix.state[&("edge".to_owned(), id.clone())].clone();
    assert_eq!(
        (
            edge(&e1)["properties"]["order_key"].clone(),
            edge(&e2)["lifecycle"].clone()
        ),
        (json!("0"), json!("archived"))
    );
    report("111-collection", &json!({"e1": edge(&e1), "e2": edge(&e2)}));

    // Content survives a restart; its update moves only its own revision; a
    // stale update conflicts and writes nothing.
    close(one);
    let one = device(1);
    let first = GraphClient::new(one.clone());
    let before = first.get_node(&x, false).expect("read").expect("owner");
    let update = |expected: &str, text: &str| {
        first
            .change(
                vec![ChangeOp::UpdateContent {
                    id: c.clone(),
                    owner_node_id: x.clone(),
                    expected_revision: expected.to_owned(),
                    body: json!({"inline_text": text}),
                }],
                None,
            )
            .expect("content update")
    };
    accepted(&applied(&first, &one, &update("1", "v2")).1);
    let (_, stale) = applied(&first, &one, &update("1", "v3"));
    assert!(matches!(stale, GraphOutcome::Conflict { .. }), "{stale:?}");
    let content = first
        .get_content(&c, false)
        .expect("read")
        .expect("content");
    let after = first.get_node(&x, false).expect("read").expect("owner");
    assert_eq!(
        (
            content["revision"].clone(),
            content["body"]["inline_text"].clone()
        ),
        (json!("2"), json!("v2"))
    );
    assert_eq!(
        (
            after.image["revision"].clone(),
            after.image["lifecycle_revision"].clone()
        ),
        (
            before.image["revision"].clone(),
            before.image["lifecycle_revision"].clone()
        )
    );
    report(
        "111-content",
        &json!({"content": content, "stale": outcome_json(&stale)}),
    );

    // Merge: the redirect is reported, never substituted; the content stays.
    let merge = first
        .change(
            vec![ChangeOp::MergeNodes {
                target_id: t.clone(),
                target_revision: "1".into(),
                absorbed: vec![keeper_core::graph::protocol::change::Absorbed {
                    id: x.clone(),
                    expected_revision: revision(&after.image, "revision"),
                }],
                edges: Vec::new(),
            }],
            None,
        )
        .expect("merge");
    accepted(&applied(&first, &one, &merge).1);
    assert_eq!(second.redirects(&x).expect("read"), Some(vec![t.clone()]));
    let absorbed = second.get_node(&x, true).expect("read").expect("absorbed");
    assert_eq!(absorbed.image["id"], x.as_str());
    let kept = second
        .get_content(&c, true)
        .expect("read")
        .expect("content");
    assert_eq!(
        (kept["owner_node_id"].clone(), kept["revision"].clone()),
        (json!(x), json!("2"))
    );
    report(
        "111-merge",
        &json!({"absorbed": absorbed.image, "content": kept}),
    );

    // Verification by the person, bound to the checked state.
    let verify = |expected: &str| {
        second
            .change(
                vec![ChangeOp::VerifyAssertion {
                    id: claim.clone(),
                    expected_revision: expected.to_owned(),
                    sources: vec![keeper_core::graph::protocol::change::CheckedSource {
                        edge_id: src.clone(),
                        edge_revision: "1".into(),
                        ref_id: res.clone(),
                        ref_revision: "1".into(),
                    }],
                    contents: Vec::new(),
                }],
                None,
            )
            .expect("verify")
    };
    let (_, stale) = applied(&second, &two, &verify("2"));
    assert!(matches!(stale, GraphOutcome::Conflict { .. }), "{stale:?}");
    accepted(&applied(&second, &two, &verify("1")).1);
    let verified = second
        .get_node(&claim, false)
        .expect("read")
        .expect("claim");
    let stamped = &verified.image["properties"]["$assertion"];
    assert_eq!(
        (stamped["status"].clone(), stamped["verified_by"].clone()),
        (json!("verified"), json!(env("KEEPER_HQ_LIVE_ACTOR")))
    );
    assert!(stamped["verified_at"].is_u64());
    report(
        "111-verify",
        &json!({"assertion": stamped, "stale": outcome_json(&stale)}),
    );

    // Refusals with no effect, each at its exact point and code: the
    // locator never leaves the client (`invalid_change`), the Vocabulary
    // node reaches the server and is rejected `forbidden` (owner only).
    let head = prefix::complete(&one).expect("prefix").n;
    let refusals = [
        ("locator-userinfo", "client", "invalid_change", ChangeOp::CreateNode {
            id: e3.clone(),
            kind: "Resource".into(),
            title: "R".into(),
            description: None,
            properties: json!({"$locator": {"kind": "https", "management": "external",
                "locator": "https://user:pw@example.com/a", "selector": null, "version": null, "format": null}})
                .as_object()
                .expect("p")
                .clone(),
        }),
        ("vocabulary-by-editor", "server", "forbidden", ChangeOp::CreateNode {
            id: e4.clone(),
            kind: "Vocabulary".into(),
            title: "V".into(),
            description: None,
            properties: json!({"$definition": {"defines": "ex:Thing", "of": "kind", "status": "active"}})
                .as_object()
                .expect("p")
                .clone(),
        }),
    ];
    for (name, at, code, op) in refusals {
        match (at, first.change(vec![op], None)) {
            ("client", Err(refusal)) => {
                assert_eq!(refusal.code(), code, "{name}");
                report(
                    &format!("111-refused-{name}"),
                    &json!({"refused": "client", "code": refusal.code()}),
                );
            }
            ("server", Ok(change)) => {
                let (_, outcome) = applied(&first, &one, &change);
                assert_eq!(rejected_reason(&outcome), Some(code), "{name}: {outcome:?}");
                report(&format!("111-refused-{name}"), &outcome_json(&outcome));
            }
            (_, other) => panic!("{name}: expected a {at} refusal {code}, got {other:?}"),
        }
    }
    assert_eq!(
        prefix::complete(&one).expect("prefix").n,
        head,
        "no refused event"
    );
    close(one);
    close(two);
}

/// The reviewer's device: the second person's replica of the database.
fn reviewer() -> Source {
    let database = env("KEEPER_HQ_LIVE_DATABASE");
    let actor = env("KEEPER_HQ_LIVE_REVIEWER_ACTOR");
    let client = env("KEEPER_HQ_LIVE_REVIEWER_CLIENT");
    let replica = SyncularReplica::open(ReplicaSpec {
        database: database.clone(),
        path: state_dir().join(format!("{database}-reviewer.sqlite")),
        client_id: Some(client.clone()),
        link: Some(Link {
            base_url: format!("{}/{database}", env("KEEPER_HQ_LIVE_URL")),
            token: env("KEEPER_HQ_LIVE_REVIEWER_TOKEN"),
        }),
        subscriptions: subscriptions(&database, &actor, &client),
        watch_presence: None,
    })
    .expect("opens a real replica");
    Source {
        database,
        replica: Arc::new(replica),
        actor,
        client,
    }
}

/// Send one command and wait for its terminal outcome.
fn commanded(
    client: &GraphClient,
    source: &Source,
    applied: Result<keeper_core::graph::client::Applied, GraphClientError>,
) -> (Prepared, GraphOutcome) {
    let applied = applied.expect("a request id");
    let outcome = settled(client, source, &applied.prepared);
    (applied.prepared, outcome)
}

/// Preview `event_id`'s reversal (it must be ready) and reverse it under
/// exactly the previewed revisions.
fn reversed(client: &GraphClient, source: &Source, event_id: &str) -> (Preview, GraphOutcome) {
    let preview = client.preview_reversal(event_id).expect("preview");
    assert_eq!(preview.outcome, PreviewOutcome::Ready, "{preview:?}");
    let reverse = Reverse::of(preview.plan.as_ref().expect("plan"), None, None).expect("reverse");
    let (_, outcome) = commanded(client, source, client.reverse(&reverse));
    (preview, outcome)
}

fn node_at(prefix: &Prefix, id: &str) -> Value {
    Value::Object(prefix.state[&("node".to_owned(), id.to_owned())].clone())
}

fn digest_of(prefix: &Prefix, event_id: &str) -> String {
    prefix
        .events
        .iter()
        .find(|e| e.event.id == event_id)
        .map(|e| e.event.delta_digest.clone())
        .expect("event retained")
}

/// Story 111.2 on two devices of an editor reversing their own effects: a
/// merge (two parallel edges) reversed — restore-merged and
/// restore-endpoints — then the reversal reversed (a re-merge) and that one
/// reversed (depth 3), each a new event with the original delta unchanged
/// across a restart; an edit of a verified assertion reversed, restoring
/// its fields but never its verification; and a reversal raced by a later
/// edit, which conflicts (`reversal_conflict`) with no event.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (111.2 server)"]
#[allow(clippy::too_many_lines)]
fn graph_live_112_reversal_of_merge_to_depth_3_and_verification() {
    let (one, two) = (device(1), device(2));
    let (first, second) = (GraphClient::new(one.clone()), GraphClient::new(two.clone()));
    let [t, b, o, e1, e2, claim, res, src] = std::array::from_fn(|_| ulid());
    let edge = |id: &str| ChangeOp::CreateEdge {
        id: id.to_owned(),
        from_id: o.clone(),
        to_id: b.clone(),
        edge_type: "ex:rel".to_owned(),
        properties: Map::new(),
        from_lifecycle_revision: None,
        to_lifecycle_revision: None,
    };
    let assertion = json!({"generated_at": null, "observed_at": 1, "verified_at": null,
                           "verified_by": null, "stale_after": null, "status": "unverified"});
    let setup = first
        .change(
            vec![
                create_node(&t, "Survivor", json!({})),
                create_node(&b, "Absorbed", json!({})),
                create_node(&o, "Other", json!({})),
                edge(&e1),
                edge(&e2),
                ChangeOp::CreateNode {
                    id: res.clone(),
                    kind: "Resource".to_owned(),
                    title: "Source".to_owned(),
                    description: None,
                    properties: Map::new(),
                },
                create_node(&claim, "Claim", json!({"$assertion": assertion})),
                ChangeOp::CreateEdge {
                    id: src.clone(),
                    from_id: claim.clone(),
                    to_id: res.clone(),
                    edge_type: "sources".to_owned(),
                    properties: json!({"source": {"$ref": {"entity_type": "node", "id": res, "revision": "1"}}})
                        .as_object()
                        .expect("p")
                        .clone(),
                    from_lifecycle_revision: None,
                    to_lifecycle_revision: None,
                },
            ],
            None,
        )
        .expect("setup");
    accepted(&applied(&first, &one, &setup).1);

    // Merge B into T, rewriting both parallel edges.
    let merge_edge = |id: &str| keeper_core::graph::protocol::change::MergeEdge {
        id: id.to_owned(),
        expected_revision: "1".into(),
        from_lifecycle_revision: "1".into(),
        to_lifecycle_revision: "1".into(),
    };
    let merge = first
        .change(
            vec![ChangeOp::MergeNodes {
                target_id: t.clone(),
                target_revision: "1".into(),
                absorbed: vec![keeper_core::graph::protocol::change::Absorbed {
                    id: b.clone(),
                    expected_revision: "1".into(),
                }],
                edges: vec![merge_edge(&e1), merge_edge(&e2)],
            }],
            None,
        )
        .expect("merge");
    let (m, merged_at) = accepted(&applied(&first, &one, &merge).1);
    let merged_digest = digest_of(&prefix_at(&two, merged_at), &m);

    // Reversal (device two, the same person): B active again, both edges
    // back on B, no redirect; the original merge event unchanged.
    let (preview, outcome) = reversed(&second, &two, &m);
    let (r1, seq) = accepted(&outcome);
    let prefix = prefix_at(&one, seq);
    let restored = node_at(&prefix, &b);
    assert_eq!(
        (
            restored["lifecycle"].clone(),
            restored["merged_into_id"].clone()
        ),
        (json!("active"), Value::Null)
    );
    for id in [&e1, &e2] {
        assert_eq!(
            prefix.state[&("edge".to_owned(), id.clone())]["to_id"],
            b.as_str()
        );
    }
    assert_eq!(first.redirects(&b).expect("read"), Some(Vec::new()));
    assert_eq!(digest_of(&prefix, &m), merged_digest);
    let event = prefix
        .events
        .iter()
        .find(|e| e.event.id == r1)
        .expect("reversal");
    assert_eq!(event.event.reverses_event_id.as_deref(), Some(m.as_str()));
    report(
        "112-merge-reversed",
        &json!({"preview": preview, "outcome": outcome_json(&outcome), "node": restored}),
    );

    // Reversal of the reversal re-merges; its reversal restores (depth 3).
    let (_, outcome) = reversed(&first, &one, &r1);
    let (r2, seq) = accepted(&outcome);
    assert_eq!(
        node_at(&prefix_at(&two, seq), &b)["merged_into_id"],
        t.as_str()
    );
    assert_eq!(second.redirects(&b).expect("read"), Some(vec![t.clone()]));
    let (_, outcome) = reversed(&second, &two, &r2);
    let (_, seq) = accepted(&outcome);
    let prefix = prefix_at(&one, seq);
    assert_eq!(node_at(&prefix, &b)["lifecycle"], "active");
    report("112-depth-3", &json!({"node": node_at(&prefix, &b)}));

    // A verified assertion's edit reversed: fields restored, never the
    // verification.
    let verify = second
        .change(
            vec![ChangeOp::VerifyAssertion {
                id: claim.clone(),
                expected_revision: "1".into(),
                sources: vec![keeper_core::graph::protocol::change::CheckedSource {
                    edge_id: src.clone(),
                    edge_revision: "1".into(),
                    ref_id: res.clone(),
                    ref_revision: "1".into(),
                }],
                contents: Vec::new(),
            }],
            None,
        )
        .expect("verify");
    accepted(&applied(&second, &two, &verify).1);
    let edit = first
        .change(vec![retitle(&claim, "2", "Edited claim")], None)
        .expect("edit");
    let (edited, _) = accepted(&applied(&first, &one, &edit).1);
    let (_, outcome) = reversed(&first, &one, &edited);
    let (r_edit, seq) = accepted(&outcome);
    let prefix = prefix_at(&two, seq);
    let node = node_at(&prefix, &claim);
    let stamped = &node["properties"]["$assertion"];
    assert_eq!(node["title"], "Claim");
    assert_eq!(
        (
            stamped["status"].clone(),
            stamped["verified_at"].clone(),
            stamped["verified_by"].clone()
        ),
        (json!("unverified"), Value::Null, Value::Null)
    );
    report("112-verified-edit-reversed", &json!({"node": node}));

    // A reversal raced by a later edit: conflict, no event, no change.
    let preview = second.preview_reversal(&r_edit).expect("preview");
    assert_eq!(preview.outcome, PreviewOutcome::Ready);
    let reverse = Reverse::of(preview.plan.as_ref().expect("plan"), None, None).expect("reverse");
    let revision = node["revision"].as_str().expect("revision").to_owned();
    accepted(
        &applied(
            &first,
            &one,
            &first
                .change(vec![retitle(&claim, &revision, "Later")], None)
                .expect("later"),
        )
        .1,
    );
    let head = prefix::complete(&one).expect("prefix").n;
    let (prepared, outcome) = commanded(&second, &two, second.reverse(&reverse));
    assert!(
        matches!(outcome, GraphOutcome::Conflict { .. }),
        "{outcome:?}"
    );
    assert_eq!(outcome.code(prepared.key()), Some("reversal_conflict"));
    assert_eq!(
        prefix::complete(&one).expect("prefix").n,
        head,
        "no reversal event"
    );
    report("112-reversal-raced", &outcome_json(&outcome));

    // The retained original deltas survive a restart unchanged.
    close(one);
    let one = device(1);
    let prefix = prefix_at(&one, head);
    assert_eq!(digest_of(&prefix, &m), merged_digest);
    close(one);
    close(two);
}

/// Story 111.2 with a reviewer: a proposal synced to its proposer only,
/// read by its enumerated reviewer and approved into an accepted event; a
/// superseded revision's approval refused `proposal_stale`; the seeded
/// agent event in the unreviewed feed, acknowledged once (a second
/// acknowledgement and a reversal of the acknowledgement refused), still
/// reversible after it, and reversed by the reviewer (another's effect).
#[test]
#[ignore = "driven by keeper-hq's native graph harness (111.2 server)"]
#[allow(clippy::too_many_lines)]
fn graph_live_112_proposal_acknowledgement_and_unreviewed() {
    let one = device(1);
    let first = GraphClient::new(one.clone());
    let review = reviewer();
    let checker = GraphClient::new(review.clone());
    let reviewer_actor = env("KEEPER_HQ_LIVE_REVIEWER_ACTOR");
    let graph = env("KEEPER_HQ_LIVE_DATABASE");
    let [p1, p2, n1, n2] = std::array::from_fn(|_| ulid());
    let propose = |id: &str, node: &str, base: Option<&str>, title: &str| {
        Proposal::new(
            id.to_owned(),
            base.map(str::to_owned),
            first
                .change(vec![create_node(node, title, json!({}))], None)
                .expect("change"),
            vec![reviewer_actor.clone()],
            None,
        )
        .expect("proposal")
    };
    let proposal_one = propose(&p1, &n1, None, "Proposed");
    let (_, outcome) = commanded(&first, &one, first.propose(&proposal_one));
    let digest = proposal::digest(&graph, &proposal_one).expect("digest");
    match &outcome {
        GraphOutcome::Proposed { detail } => assert_eq!(
            Value::Object(detail.clone()),
            proposal::proposed_detail(&p1, "1", &digest)
        ),
        other => panic!("not proposed: {other:?}"),
    }
    // The proposer's own replica holds it; the reviewer reads it per call.
    until(&one, "the proposal row", || {
        let rows = one
            .replica
            .query(
                "SELECT * FROM kg_proposals WHERE proposal_id = ?",
                &[Value::from(p1.as_str())],
            )
            .expect("query");
        (!rows.is_empty()).then_some(())
    });
    let held = review
        .replica
        .query("SELECT * FROM kg_proposals", &[])
        .expect("query");
    assert!(
        held.is_empty(),
        "a reviewer never syncs another's proposals"
    );
    let revisions = checker.proposal(&p1).expect("read");
    assert_eq!(revisions.len(), 1);
    assert_eq!(
        (revisions[0].status.as_str(), revisions[0].digest.as_str()),
        ("open", digest.as_str())
    );
    let decision = |id: &str, revision: &str, digest: &str| Decision {
        proposal_id: id.to_owned(),
        proposal_revision: revision.to_owned(),
        proposal_digest: digest.to_owned(),
        verdict: Verdict::Approve,
    };
    let (_, outcome) = commanded(
        &checker,
        &review,
        checker.decide(&decision(&p1, "1", &digest)),
    );
    let (approved, seq) = accepted(&outcome);
    let prefix = prefix_at(&review, seq);
    let event = prefix
        .events
        .iter()
        .find(|e| e.event.id == approved)
        .expect("event");
    assert_eq!(
        event.event.approval_basis.as_ref().expect("basis")["basis"],
        "proposal"
    );
    report(
        "112-proposal-approved",
        &json!({"outcome": outcome_json(&outcome), "header": event.event}),
    );

    // A superseded revision cannot be approved.
    let first_revision = propose(&p2, &n2, None, "Draft");
    commanded(&first, &one, first.propose(&first_revision));
    let stale_digest = proposal::digest(&graph, &first_revision).expect("digest");
    commanded(
        &first,
        &one,
        first.propose(&propose(&p2, &n2, Some("1"), "Edited draft")),
    );
    let head = prefix::complete(&review).expect("prefix").n;
    let (prepared, outcome) = commanded(
        &checker,
        &review,
        checker.decide(&decision(&p2, "1", &stale_digest)),
    );
    assert_eq!(
        rejected_reason(&outcome),
        Some("proposal_stale"),
        "{outcome:?}"
    );
    assert_eq!(outcome.code(prepared.key()), Some("proposal_stale"));
    assert_eq!(
        prefix::complete(&review).expect("prefix").n,
        head,
        "no event"
    );
    report("112-proposal-stale", &outcome_json(&outcome));

    // The seeded agent event is unreviewed until a reviewer acknowledges it.
    let agent_event = env("KEEPER_HQ_LIVE_AGENT_EVENT");
    let in_feed = |client: &GraphClient| {
        let mut after = "0".to_owned();
        loop {
            let page = client.unreviewed(&after, 100).expect("feed");
            if page.events.iter().any(|e| e.event.id == agent_event) {
                return true;
            }
            if page.scanned_through == after {
                return false;
            }
            after = page.scanned_through;
        }
    };
    assert!(in_feed(&checker));
    let prefix = prefix_at(&review, prefix::complete(&review).expect("prefix").n);
    let original = digest_of(&prefix, &agent_event);
    let target = Target::new(&agent_event, &original).expect("target");
    let (_, outcome) = commanded(&checker, &review, checker.acknowledge(&target));
    let (ack, seq) = accepted(&outcome);
    assert!(!in_feed(&checker), "acknowledged events leave the feed");
    let (_, again) = commanded(&checker, &review, checker.acknowledge(&target));
    assert_eq!(rejected_reason(&again), Some("invalid_change"), "{again:?}");
    let ack_preview = checker.preview_reversal(&ack).expect("preview");
    assert_eq!(
        (ack_preview.outcome, ack_preview.reason.as_deref()),
        (PreviewOutcome::Rejected, Some("invalid_change"))
    );
    assert_eq!(digest_of(&prefix_at(&review, seq), &agent_event), original);
    report(
        "112-acknowledged",
        &json!({"outcome": outcome_json(&outcome), "again": outcome_json(&again), "preview": ack_preview}),
    );

    // Acknowledgement leaves it reversible: the reviewer reverses another's
    // effect.
    let (_, outcome) = reversed(&checker, &review, &agent_event);
    accepted(&outcome);
    report("112-agent-event-reversed", &outcome_json(&outcome));
    close(one);
    close(review);
}
