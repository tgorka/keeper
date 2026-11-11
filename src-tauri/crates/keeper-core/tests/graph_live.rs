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
//! - 111.2 native matrix (audit G2), in addition:
//!   - `KEEPER_HQ_LIVE_EDITOR_ACTOR`, `KEEPER_HQ_LIVE_EDITOR_CLIENT` and
//!     `KEEPER_HQ_LIVE_EDITOR_TOKEN`: a third person holding only the graph
//!     editor role;
//!   - `KEEPER_HQ_LIVE_REVOKED_ACTOR`, `KEEPER_HQ_LIVE_REVOKED_CLIENT` and
//!     `KEEPER_HQ_LIVE_REVOKED_TOKEN`: a fourth person holding the graph
//!     reviewer role, which the harness revokes between
//!     `graph_live_112_native_transport_refusals`' phases `before` and
//!     `after`;
//!   - `KEEPER_HQ_LIVE_FOREIGN_EVENT` and `KEEPER_HQ_LIVE_FOREIGN_EVENT_DIGEST`:
//!     an accepted event of `KEEPER_HQ_LIVE_FORBIDDEN_DATABASE` (one the
//!     reviewer holds no role in) and its `delta_digest`;
//!     `KEEPER_HQ_LIVE_FOREIGN_PROPOSAL` and
//!     `KEEPER_HQ_LIVE_FOREIGN_PROPOSAL_DIGEST`: an open proposal there
//!     (revision 1) and its digest;
//!   - `KEEPER_HQ_LIVE_AGENT_ROOT`: the collection node below which the
//!     harness's agent writes; `graph_live_112_x29_reversal_by_every_role`
//!     places its assertion there in phase `seed`;
//!   - `KEEPER_HQ_LIVE_AGENT_EDIT_EVENT` and
//!     `KEEPER_HQ_LIVE_AGENT_REVERSAL_EVENT`: between that case's phases
//!     `seed` and `roles` the harness's agent (holding `read`, `content`
//!     and `reverse_own`, with per-call approvals) edits the seeded
//!     assertion's description under the revision `seed` reports, then
//!     reverses its own edit; these are the two accepted events;
//!   - `KEEPER_HQ_LIVE_AGENT_EVENT` for `graph_live_112_preview_races`: the
//!     agent's own accepted event; between that case's phases `races` and
//!     `revoked` the harness has the agent preview its reversal, revokes
//!     the agent's capability and has the agent send the reversal, which
//!     is refused; `KEEPER_HQ_LIVE_AGENT_REFUSED_REQUEST` is that refused
//!     request's id;
//!   - `KEEPER_HQ_LIVE_PHASE` per case, as each case names.
//!
//! Every claim is asserted here; each case also prints the `KG1_LIVE <case>
//! <json>` lines the harness checks against the server's authoritative rows.
//! Their names and shapes are the harness's interface: change neither
//! without it. Credentials are transient environment inputs and are never
//! printed.

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, Instant};

use keeper_core::graph::client::{
    subscribed_tables, subscriptions, Applied, GraphClient, GraphClientError, Prepared,
};
use keeper_core::graph::lifecycle::{Lifecycle, LifecycleError};
use keeper_core::graph::outcome::{GraphOutcome, GraphRequests, PendingCause, ReceiptOutcome};
use keeper_core::graph::prefix::{self, Prefix};
use keeper_core::graph::protocol::change::{
    Absorbed, Change, ChangeOp, CheckedContent, CheckedSource, MergeEdge, NodeFields, DESCRIPTOR,
};
use keeper_core::graph::protocol::inverse::{Preview, PreviewOutcome, Reverse, REVERSE};
use keeper_core::graph::protocol::proposal::{self, Decision, Proposal, Verdict, DECIDE};
use keeper_core::graph::protocol::request::Request;
use keeper_core::graph::protocol::review::{self, Target, ACKNOWLEDGE};
use keeper_core::graph::protocol::vocabulary::OriginClass;
use keeper_core::graph::protocol::wire::{self, HistoryFilter};
use keeper_core::graph::protocol::{assertion, Refusal, CONTRACT_VERSION};
use keeper_core::graph::queue::{
    resume_sends, Binding, FenceCause, FenceState, GraphQueue, QueueError, Restored, State,
};
use keeper_core::graph::source::{revocation_with, Revocation};
use keeper_core::graph::write::{Caller, Writer};
use keeper_core::hq::port::{Link, ReplicaError, ReplicaSpec};
use keeper_core::hq::server::ServerUrl;
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

/// The complete prefix once it holds `event_id` (pulling, at most `until`'s
/// deadline). A replica that has not synced yet is complete at `n = 0`, so
/// a wait on its own head returns at once: wait for the event itself.
fn prefix_with(source: &Source, event_id: &str) -> Prefix {
    until(
        source,
        &format!("event {event_id} in the complete prefix"),
        || {
            let prefix = prefix::complete(source).expect("no corruption");
            (prefix.is_complete() && prefix.events.iter().any(|e| e.event.id == event_id))
                .then_some(prefix)
        },
    )
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
    // 112.1 CC-6/CC-7: the refusal is typed and is a revocation signal,
    // never a quiet pending.
    assert_eq!(
        outcome,
        &GraphOutcome::Pending {
            cause: PendingCause::Refused
        }
    );
    assert!(outcome.revokes());
    report(
        "forbidden",
        &json!({"transport": format!("{:?}", attempt.result()),
                "outcome": outcome_json(outcome), "revokes": outcome.revokes()}),
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

/// Another person's device: their replica of the database in
/// `{database}-{name}.sqlite`, under `{prefix}_ACTOR`, `{prefix}_CLIENT` and
/// `{prefix}_TOKEN`.
fn person(prefix: &str, name: &str) -> Source {
    let database = env("KEEPER_HQ_LIVE_DATABASE");
    let actor = env(&format!("{prefix}_ACTOR"));
    let client = env(&format!("{prefix}_CLIENT"));
    let replica = SyncularReplica::open(ReplicaSpec {
        database: database.clone(),
        path: state_dir().join(format!("{database}-{name}.sqlite")),
        client_id: Some(client.clone()),
        link: Some(Link {
            base_url: format!("{}/{database}", env("KEEPER_HQ_LIVE_URL")),
            token: env(&format!("{prefix}_TOKEN")),
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

/// The reviewer's device: the second person's replica of the database.
fn reviewer() -> Source {
    person("KEEPER_HQ_LIVE_REVIEWER", "reviewer")
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
    let prefix = prefix_with(&review, &agent_event);
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

// ---------------------------------------------------------------------------
// Story 111.2, the native matrix (audit G2). Every case runs on the person's
// two persistent replicas (devices 1 and 2). After each refusal both still
// hold exactly the complete prefix (events, head, every entity's state) and
// the review rows they held before it, and report them with the native reads
// the harness compares with the server's rows. Extra harness inputs are in
// the module header.
// ---------------------------------------------------------------------------

fn text(image: &Map<String, Value>, key: &str) -> String {
    image
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_owned()
}

fn number(revision: &str) -> u64 {
    revision.parse().expect("a decimal revision")
}

fn node_image<'a>(prefix: &'a Prefix, id: &str) -> &'a Map<String, Value> {
    &prefix.state[&("node".to_owned(), id.to_owned())]
}

fn edge_image<'a>(prefix: &'a Prefix, id: &str) -> &'a Map<String, Value> {
    &prefix.state[&("edge".to_owned(), id.to_owned())]
}

fn node_revision(prefix: &Prefix, id: &str) -> String {
    text(node_image(prefix, id), "revision")
}

fn header<'a>(prefix: &'a Prefix, event_id: &str) -> &'a wire::EventHeader {
    &prefix
        .events
        .iter()
        .find(|e| e.event.id == event_id)
        .expect("event retained")
        .event
}

/// The revision event `event_id` left entity `id` at.
fn after_revision(prefix: &Prefix, event_id: &str, id: &str) -> String {
    prefix
        .events
        .iter()
        .find(|e| e.event.id == event_id)
        .and_then(|e| e.ops.iter().find(|op| op.entity_id == id))
        .and_then(|op| op.after_revision.clone())
        .expect("the event's operation on the entity")
}

/// Apply `ops` from `client` and wait for the accepted event.
fn landed(client: &GraphClient, source: &Source, ops: Vec<ChangeOp>) -> (String, u64) {
    let change = client.change(ops, None).expect("a change");
    accepted(&applied(client, source, &change).1)
}

/// An assertion node: a `Concept` carrying an unverified `$assertion`.
fn assertion_node(id: &str, title: &str, description: Option<&str>) -> ChangeOp {
    ChangeOp::CreateNode {
        id: id.to_owned(),
        kind: "Concept".to_owned(),
        title: title.to_owned(),
        description: description.map(str::to_owned),
        properties: json!({"$assertion": {"generated_at": null, "observed_at": 1,
            "verified_at": null, "verified_by": null, "stale_after": null,
            "status": "unverified"}})
        .as_object()
        .expect("p")
        .clone(),
    }
}

fn resource(id: &str) -> ChangeOp {
    ChangeOp::CreateNode {
        id: id.to_owned(),
        kind: "Resource".to_owned(),
        title: "Source".to_owned(),
        description: None,
        properties: Map::new(),
    }
}

/// A `sources` edge from `from` to `to` whose typed reference names the
/// `entity_type` `reference` at revision 1.
fn sources(id: &str, from: &str, to: &str, entity_type: &str, reference: &str) -> ChangeOp {
    ChangeOp::CreateEdge {
        id: id.to_owned(),
        from_id: from.to_owned(),
        to_id: to.to_owned(),
        edge_type: "sources".to_owned(),
        properties: json!({"source": {"$ref": {"entity_type": entity_type, "id": reference,
            "revision": "1"}}})
        .as_object()
        .expect("p")
        .clone(),
        from_lifecycle_revision: None,
        to_lifecycle_revision: None,
    }
}

fn edge_between(id: &str, from: &str, to: &str) -> ChangeOp {
    ChangeOp::CreateEdge {
        id: id.to_owned(),
        from_id: from.to_owned(),
        to_id: to.to_owned(),
        edge_type: "ex:rel".to_owned(),
        properties: Map::new(),
        from_lifecycle_revision: None,
        to_lifecycle_revision: None,
    }
}

/// `op`, an edge creation, with each endpoint's lifecycle revision as
/// `prefix` holds it: the contract lets it be null only for a node the
/// same change creates (one `prefix` does not hold).
fn on_existing(prefix: &Prefix, op: ChangeOp) -> ChangeOp {
    let lifecycle_of = |id: &str| {
        prefix
            .state
            .get(&("node".to_owned(), id.to_owned()))
            .map(|node| text(node, "lifecycle_revision"))
    };
    match op {
        ChangeOp::CreateEdge {
            id,
            from_id,
            to_id,
            edge_type,
            properties,
            from_lifecycle_revision,
            to_lifecycle_revision,
        } => ChangeOp::CreateEdge {
            from_lifecycle_revision: from_lifecycle_revision.or_else(|| lifecycle_of(&from_id)),
            to_lifecycle_revision: to_lifecycle_revision.or_else(|| lifecycle_of(&to_id)),
            id,
            from_id,
            to_id,
            edge_type,
            properties,
        },
        other => other,
    }
}

fn describe(id: &str, expected: &str, description: &str) -> ChangeOp {
    ChangeOp::UpdateNode {
        id: id.to_owned(),
        expected_revision: expected.to_owned(),
        set: NodeFields {
            title: None,
            description: Some(Some(description.to_owned())),
            description_source: None,
        },
        set_properties: Map::new(),
        remove_properties: Vec::new(),
    }
}

fn lifecycle_op(id: &str, expected: &str, archive: bool) -> ChangeOp {
    let (id, expected_revision) = (id.to_owned(), expected.to_owned());
    if archive {
        ChangeOp::ArchiveNode {
            id,
            expected_revision,
        }
    } else {
        ChangeOp::UnarchiveNode {
            id,
            expected_revision,
        }
    }
}

/// Merge `absorbed` into `target`, rewriting `edges`, under the
/// preconditions `prefix` holds.
fn merge_of(prefix: &Prefix, target: &str, absorbed: &[&str], edges: &[&str]) -> ChangeOp {
    let lifecycle_of = |id: &str| text(node_image(prefix, id), "lifecycle_revision");
    ChangeOp::MergeNodes {
        target_id: target.to_owned(),
        target_revision: node_revision(prefix, target),
        absorbed: absorbed
            .iter()
            .map(|id| Absorbed {
                id: (*id).to_owned(),
                expected_revision: node_revision(prefix, id),
            })
            .collect(),
        edges: edges
            .iter()
            .map(|id| {
                let edge = edge_image(prefix, id);
                MergeEdge {
                    id: (*id).to_owned(),
                    expected_revision: text(edge, "revision"),
                    from_lifecycle_revision: lifecycle_of(&text(edge, "from_id")),
                    to_lifecycle_revision: lifecycle_of(&text(edge, "to_id")),
                }
            })
            .collect(),
    }
}

/// The graph `prefix` holds, as 111.1's currency read sees it.
struct Now<'a>(&'a Prefix);

impl assertion::Current for Now<'_> {
    fn node(&self, id: &str) -> Option<Map<String, Value>> {
        self.0
            .state
            .get(&("node".to_owned(), id.to_owned()))
            .cloned()
    }
    fn content(&self, id: &str) -> Option<Map<String, Value>> {
        self.0
            .state
            .get(&("content".to_owned(), id.to_owned()))
            .cloned()
    }
    fn sources_from(&self, id: &str, limit: usize) -> Vec<Map<String, Value>> {
        self.0
            .state
            .iter()
            .filter(|(key, edge)| {
                key.0 == "edge"
                    && text(edge, "type") == "sources"
                    && text(edge, "lifecycle") == "active"
                    && text(edge, "from_id") == id
            })
            .map(|(_, edge)| edge.clone())
            .take(limit)
            .collect()
    }
}

/// A person's verification of assertion `id` exactly as `prefix` holds it:
/// its revision, every active `sources` edge from it (each reference at the
/// revision it names) and every content it owns.
fn verify_as_held(prefix: &Prefix, id: &str) -> ChangeOp {
    let sources =
        assertion::Current::sources_from(&Now(prefix), id, assertion::MAX_CHECKED_SOURCES)
            .iter()
            .map(|edge| {
                let reference = assertion::source_ref(edge).expect("a checked reference");
                CheckedSource {
                    edge_id: text(edge, "id"),
                    edge_revision: text(edge, "revision"),
                    ref_id: reference.id,
                    ref_revision: reference.revision.expect("a referenced revision"),
                }
            })
            .collect();
    let contents = prefix
        .state
        .iter()
        .filter(|(key, content)| key.0 == "content" && text(content, "owner_node_id") == id)
        .map(|(key, content)| CheckedContent {
            id: key.1.clone(),
            revision: text(content, "revision"),
        })
        .collect();
    ChangeOp::VerifyAssertion {
        id: id.to_owned(),
        expected_revision: node_revision(prefix, id),
        sources,
        contents,
    }
}

/// The person's verification of assertion `id` from `client`, as
/// `source`'s replica holds it once it holds `after`: accepted, and read
/// back verified and current. Its event.
fn verified(client: &GraphClient, source: &Source, id: &str, after: &str) -> String {
    let check = verify_as_held(&prefix_with(source, after), id);
    let (event, _) = landed(client, source, vec![check]);
    let prefix = prefix_with(source, &event);
    assert!(assertion::is_verified(node_image(&prefix, id)), "{id}");
    assert!(
        assertion::is_current(&Now(&prefix), id),
        "a fresh verification is current"
    );
    event
}

/// CC-X29 on every replica's native read once it holds `event_id`: node
/// `id` `unverified` with null `verified_at`/`verified_by`, its revision
/// past `was`, 111.1's currency read false, and the same on each. The node.
fn unverified_on(sources: &[&Source], event_id: &str, id: &str, was: &str) -> Value {
    let mut read: Option<Value> = None;
    for source in sources {
        let prefix = prefix_with(source, event_id);
        let node = node_at(&prefix, id);
        let stamp = &node["properties"]["$assertion"];
        assert_eq!(
            (
                stamp["status"].clone(),
                stamp["verified_at"].clone(),
                stamp["verified_by"].clone()
            ),
            (json!("unverified"), Value::Null, Value::Null),
            "{id} after {event_id}"
        );
        let revision = node["revision"].as_str().expect("revision");
        assert!(
            number(revision) > number(was),
            "{id}: revision {revision} is not past {was}"
        );
        assert!(
            !assertion::is_current(&Now(&prefix), id),
            "{id} reads current"
        );
        if let Some(other) = &read {
            assert_eq!(other, &node, "both replicas read the same node");
        }
        read = Some(node);
    }
    read.expect("a replica")
}

/// What a refusal must leave as it was on one replica: its complete prefix
/// (events, head, every entity's state) and its review rows.
#[derive(Debug, PartialEq)]
struct Held {
    prefix: Prefix,
    reviews: u64,
}

fn held(source: &Source) -> Held {
    let prefix = until(source, "a complete prefix", || {
        let prefix = prefix::complete(source).expect("no corruption");
        prefix.is_complete().then_some(prefix)
    });
    Held {
        prefix,
        reviews: rows_in(source, "kg_reviews"),
    }
}

/// What each of `sources` holds once it holds `event_id`.
fn held_with(sources: &[&Source], event_id: &str) -> Vec<Held> {
    sources
        .iter()
        .map(|source| {
            prefix_with(source, event_id);
            held(source)
        })
        .collect()
}

/// A pull barrier after a command: a new event written now (a plain
/// `Barrier` node, by `sources[0]`'s person) and each of `sources` waited
/// on until its complete prefix holds it. The server orders its commits, so
/// a replica whose complete prefix holds the barrier has completed a pull
/// that started after the command and covers anything the command
/// committed. The barrier's node id, event id and position.
fn barrier(sources: &[&Source]) -> (String, String, u64) {
    let writer = GraphClient::new(sources[0].clone());
    let node = ulid();
    let (event, seq) = landed(
        &writer,
        sources[0],
        vec![create_node(&node, "Barrier", json!({}))],
    );
    for source in sources {
        prefix_with(source, &event);
    }
    (node, event, seq)
}

/// After a refusal: once a [`barrier`] written after it reached each of
/// `sources`, each holds exactly what it held `before` plus that barrier —
/// the barrier is the very next event (nothing landed between), every
/// earlier event and every other entity's state is as it was, and so are
/// the review rows. Reported as `case`, with `reads`, the native reads the
/// harness compares with the server's rows. `before` then holds what each
/// replica holds now, barrier included, for the next refusal.
fn unchanged(case: &str, sources: &[&Source], before: &mut [Held], reads: &Value) {
    let (node, event, seq) = barrier(sources);
    let replicas: Vec<Value> = sources
        .iter()
        .zip(before.iter_mut())
        .map(|(source, before)| {
            let now = held(source);
            assert_eq!(
                seq,
                before.prefix.n + 1,
                "{case}: an event landed between the refusal and the barrier"
            );
            assert!(
                now.prefix.events.get(..before.prefix.events.len())
                    == Some(before.prefix.events.as_slice()),
                "{case}: a refusal changed the retained events"
            );
            let mut state = now.prefix.state.clone();
            state.remove(&("node".to_owned(), node.clone()));
            assert!(
                state == before.prefix.state,
                "{case}: a refusal changed an entity"
            );
            assert_eq!(
                now.reviews, before.reviews,
                "{case}: a refusal changed a review"
            );
            let reported = json!({"n": now.prefix.n, "head": now.prefix.head,
                   "reviews": now.reviews, "before_n": before.prefix.n,
                   "barrier_event": event, "barrier_seq": seq.to_string()});
            *before = now;
            reported
        })
        .collect();
    report(case, &json!({"replicas": replicas, "reads": reads}));
}

/// Send one command that must be refused and wait for the answer: a
/// terminal outcome, or the server's refusal of the lookup itself
/// (`pending`, cause `refused`), never accepted.
fn refused(
    client: &GraphClient,
    source: &Source,
    applied: Result<Applied, GraphClientError>,
) -> (Prepared, GraphOutcome) {
    let applied = applied.expect("a request id");
    let attempt = applied.attempt.expect("sent");
    let outcome = until(source, "the refusal", || {
        match client.outcome(&applied.prepared, Some(&attempt)) {
            Ok(
                refusal @ GraphOutcome::Pending {
                    cause: PendingCause::Refused,
                },
            ) => Some(refusal),
            Ok(GraphOutcome::Pending { .. } | GraphOutcome::PendingVisibility { .. }) => None,
            Ok(outcome) => Some(outcome),
            Err(error) => panic!("outcome: {error}"),
        }
    });
    assert!(
        !matches!(outcome, GraphOutcome::Accepted { .. }),
        "{outcome:?}"
    );
    (applied.prepared, outcome)
}

/// A reversal of `event_id` that must conflict now: its preview says so,
/// and the reversal sent under the previewed plan is `reversal_conflict`
/// and changes none of `sources` (`case`'s report). The conflict's detail.
fn conflicted(
    client: &GraphClient,
    source: &Source,
    event_id: &str,
    sources: &[&Source],
    before: &mut [Held],
    case: &str,
) -> Value {
    let preview = client.preview_reversal(event_id).expect("preview");
    assert_eq!(
        preview.outcome,
        PreviewOutcome::Conflict,
        "{case}: {preview:?}"
    );
    let reverse = Reverse::of(preview.plan.as_ref().expect("plan"), None, None).expect("reverse");
    let (prepared, outcome) = commanded(client, source, client.reverse(&reverse));
    assert_eq!(
        outcome.code(prepared.key()),
        Some("reversal_conflict"),
        "{case}: {outcome:?}"
    );
    let GraphOutcome::Conflict { detail } = &outcome else {
        panic!("{case}: {outcome:?}");
    };
    let detail = Value::Object(detail.clone());
    unchanged(
        case,
        sources,
        before,
        &json!({"preview": preview, "outcome": outcome_json(&outcome)}),
    );
    detail
}

/// Story 111.2 T2 (S:76): the reviewer-negative transports, through the
/// native client and raw to the server.
///
/// Phase `before`: forged human authority — `graph.decide` and
/// `graph.acknowledge` from an agent caller are refused by the client
/// (`invalid_change`, nothing sent) and, sent raw from the reviewer's
/// device naming an agent context, by the server (`forbidden`, no request
/// recorded); a missing reviewer role — the editor decides and
/// acknowledges, `rejected forbidden`; cross-domain target ids — the
/// reviewer names another database's event and proposal and is refused
/// exactly as for absent ids, through acknowledgement, decision, preview
/// and proposal read. It leaves a proposal naming the reviewer the harness
/// revokes next, who reads it now.
///
/// Phase `after` (that reviewer's role revoked): their decision and
/// acknowledgement are refused as revocation signals and their reviewer
/// reads are refused. No refusal changes either replica.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (111.2 server)"]
#[allow(clippy::too_many_lines)]
fn graph_live_112_native_transport_refusals() {
    let saved = state_dir().join("transport-112.json");
    let (one, two) = (device(1), device(2));
    let both = [&one, &two];
    let decision = |id: &str, digest: &str| Decision {
        proposal_id: id.to_owned(),
        proposal_revision: "1".to_owned(),
        proposal_digest: digest.to_owned(),
        verdict: Verdict::Approve,
    };
    match env("KEEPER_HQ_LIVE_PHASE").as_str() {
        "before" => {
            let first = GraphClient::new(one.clone());
            let graph = env("KEEPER_HQ_LIVE_DATABASE");
            let (event, _) = landed(
                &first,
                &one,
                vec![create_node(&ulid(), "Reviewed", json!({}))],
            );
            let mut before = held_with(&both, &event);
            let target =
                Target::new(&event, &digest_of(&before[0].prefix, &event)).expect("target");
            let propose = |reviewer: String| {
                let id = ulid();
                let proposal = Proposal::new(
                    id.clone(),
                    None,
                    first
                        .change(vec![create_node(&ulid(), "Proposed", json!({}))], None)
                        .expect("change"),
                    vec![reviewer],
                    None,
                )
                .expect("proposal");
                let (_, outcome) = commanded(&first, &one, first.propose(&proposal));
                assert!(
                    matches!(outcome, GraphOutcome::Proposed { .. }),
                    "{outcome:?}"
                );
                (id, proposal::digest(&graph, &proposal).expect("digest"))
            };
            let (open, open_digest) = propose(env("KEEPER_HQ_LIVE_REVIEWER_ACTOR"));
            let (doomed, doomed_digest) = propose(env("KEEPER_HQ_LIVE_REVOKED_ACTOR"));
            let review = reviewer();
            let checker = GraphClient::new(review.clone());

            // Forged human authority: the client refuses an agent caller's
            // decision and acknowledgement before anything is sent...
            let as_agent = GraphClient::over(
                Arc::new(review.clone()),
                &Writer::of(&review),
                Caller::Agent { context_id: ulid() },
            );
            let invalid = Some(GraphClientError::Refused(Refusal::InvalidChange));
            assert_eq!(
                as_agent.decide(&decision(&open, &open_digest)).err(),
                invalid
            );
            assert_eq!(as_agent.acknowledge(&target).err(), invalid);
            // ...and the server refuses them sent raw, with a reviewer's
            // credential naming an agent context: nothing is recorded under
            // the request.
            let requests = GraphRequests::new(review.clone());
            let raw = |descriptor: &str, payload: Value| -> String {
                let request = Request {
                    request_id: ulid(),
                    context_id: Some(ulid()),
                    payload,
                };
                let envelope = json!({"version": CONTRACT_VERSION,
                    "request_id": request.request_id, "context_id": request.context_id,
                    "payload": request.payload});
                let answer =
                    review
                        .replica
                        .remote_command(descriptor, &request.request_id, envelope);
                match &answer {
                    Err(error)
                        if error.to_string().contains("forbidden")
                            && PendingCause::of(error) == PendingCause::Refused => {}
                    other => panic!("{descriptor}: not refused forbidden: {other:?}"),
                }
                let key = requests.key(descriptor, &request).expect("key");
                assert!(
                    requests
                        .remote_receipt(&key, None)
                        .expect("graph.outcome")
                        .is_none(),
                    "{descriptor}: a refused raw request left a receipt"
                );
                format!("{:?}", answer.err())
            };
            let forged = [
                raw(DECIDE, decision(&open, &open_digest).payload()),
                raw(ACKNOWLEDGE, review::acknowledge_payload(&target)),
            ];
            unchanged(
                "112-forged-human-authority",
                &both,
                &mut before,
                &json!({"client": "invalid_change", "server": forged}),
            );

            // A missing reviewer role: an editor may not decide or
            // acknowledge.
            let editing = person("KEEPER_HQ_LIVE_EDITOR", "editor");
            let editor = GraphClient::new(editing.clone());
            let (_, decided) = refused(
                &editor,
                &editing,
                editor.decide(&decision(&open, &open_digest)),
            );
            let (_, acknowledged) = refused(&editor, &editing, editor.acknowledge(&target));
            for outcome in [&decided, &acknowledged] {
                assert_eq!(rejected_reason(outcome), Some("forbidden"), "{outcome:?}");
            }
            unchanged(
                "112-editor-reviews",
                &both,
                &mut before,
                &json!({"decide": outcome_json(&decided),
                        "acknowledge": outcome_json(&acknowledged)}),
            );
            close(editing);

            // Cross-domain ids: another database's event and proposal are
            // refused exactly as absent ones are.
            let (event_digest, proposal_digest) = (
                env("KEEPER_HQ_LIVE_FOREIGN_EVENT_DIGEST"),
                env("KEEPER_HQ_LIVE_FOREIGN_PROPOSAL_DIGEST"),
            );
            let ask = |event: &str, proposal_id: &str| {
                let named = Target::new(event, &event_digest).expect("target");
                let (_, acknowledged) = refused(&checker, &review, checker.acknowledge(&named));
                let (_, decided) = refused(
                    &checker,
                    &review,
                    checker.decide(&decision(proposal_id, &proposal_digest)),
                );
                let preview = checker
                    .preview_reversal(event)
                    .map(|preview| preview.outcome)
                    .map_err(|error| error.to_string());
                let rows = checker
                    .proposal(proposal_id)
                    .map(|rows| rows.len())
                    .map_err(|error| error.to_string());
                (acknowledged, decided, preview, rows)
            };
            let inaccessible = ask(
                &env("KEEPER_HQ_LIVE_FOREIGN_EVENT"),
                &env("KEEPER_HQ_LIVE_FOREIGN_PROPOSAL"),
            );
            let absent = ask(&ulid(), &ulid());
            assert_eq!(rejected_reason(&inaccessible.0), Some("invalid_change"));
            assert_eq!(rejected_reason(&inaccessible.1), Some("forbidden"));
            assert_eq!(inaccessible.2, Err(FORBIDDEN.to_owned()));
            assert_eq!(inaccessible.3, Ok(0));
            assert_eq!(
                inaccessible, absent,
                "an inaccessible id is refused as an absent one"
            );
            unchanged(
                "112-cross-domain-ids",
                &both,
                &mut before,
                &json!({"acknowledge": outcome_json(&inaccessible.0),
                        "decide": outcome_json(&inaccessible.1),
                        "preview": format!("{:?}", inaccessible.2),
                        "proposal_rows": format!("{:?}", inaccessible.3)}),
            );

            // The reviewer the harness revokes next reads the proposal that
            // names them.
            let revoking = person("KEEPER_HQ_LIVE_REVOKED", "revoked");
            let rows = GraphClient::new(revoking.clone())
                .proposal(&doomed)
                .expect("read");
            assert_eq!(rows.len(), 1, "the reviewer reads the proposal naming them");
            close(revoking);
            std::fs::write(
                &saved,
                json!({"event": event, "digest": target.delta_digest, "proposal": doomed,
                       "proposal_digest": doomed_digest})
                .to_string(),
            )
            .expect("saved");
            report(
                "112-transport-before",
                &json!({"event": event, "proposal": doomed, "open_proposal": open}),
            );
            close(review);
        }
        "after" => {
            let kept: Value =
                serde_json::from_str(&std::fs::read_to_string(&saved).expect("saved"))
                    .expect("json");
            let field = |key: &str| kept[key].as_str().expect("saved field").to_owned();
            let event = field("event");
            let target = Target::new(&event, &field("digest")).expect("target");
            let mut before = held_with(&both, &event);
            let revoking = person("KEEPER_HQ_LIVE_REVOKED", "revoked");
            let revoked = GraphClient::new(revoking.clone());
            let (_, decided) = refused(
                &revoked,
                &revoking,
                revoked.decide(&decision(&field("proposal"), &field("proposal_digest"))),
            );
            let (_, acknowledged) = refused(&revoked, &revoking, revoked.acknowledge(&target));
            for outcome in [&decided, &acknowledged] {
                assert!(
                    outcome.revokes(),
                    "not refused as a revocation: {outcome:?}"
                );
            }
            let feed = revoked
                .unreviewed("0", 10)
                .map(|page| page.events.len())
                .map_err(|error| error.to_string());
            assert_eq!(feed, Err(FORBIDDEN.to_owned()));
            let rows = revoked
                .proposal(&field("proposal"))
                .map(|rows| rows.len())
                .map_err(|error| error.to_string());
            assert!(
                rows == Ok(0) || rows == Err(FORBIDDEN.to_owned()),
                "the revoked reviewer read the proposal: {rows:?}"
            );
            unchanged(
                "112-revoked-reviewer",
                &both,
                &mut before,
                &json!({"decide": outcome_json(&decided),
                        "acknowledge": outcome_json(&acknowledged),
                        "feed": format!("{feed:?}"), "proposal_rows": format!("{rows:?}")}),
            );
            close(revoking);
        }
        phase => panic!("unknown phase {phase}"),
    }
    close(one);
    close(two);
}

/// Story 111.2 CC-X29 (S:106): a verified assertion's edit, reversed by
/// each role, comes back with its description and never its verification.
///
/// Phase `seed`: the person verifies assertion A (contained below the
/// agent's root) and reports its revision. Between the phases the
/// harness's agent edits A's description and reverses its own edit.
///
/// Phase `roles`: the agent's reversal, read natively; then, A verified
/// again before each edit, an editor's edit reversed by the editor and
/// another reversed by the reviewer (another's effect). After every
/// reversal, on both replicas: the description restored, `unverified`
/// with null `verified_at`/`verified_by`, the revision forward and the
/// currency read false.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (111.2 server)"]
#[allow(clippy::too_many_lines)]
fn graph_live_112_x29_reversal_by_every_role() {
    let saved = state_dir().join("x29-roles-112.json");
    let (one, two) = (device(1), device(2));
    let both = [&one, &two];
    let (first, second) = (GraphClient::new(one.clone()), GraphClient::new(two.clone()));
    let original = "Before every edit";
    match env("KEEPER_HQ_LIVE_PHASE").as_str() {
        "seed" => {
            let [a, r, s, contained] = std::array::from_fn(|_| ulid());
            let root = env("KEEPER_HQ_LIVE_AGENT_ROOT");
            // The root exists already: its lifecycle revision as this
            // replica holds it.
            let rooted = until(&one, "the agent root", || {
                let prefix = prefix::complete(&one).expect("no corruption");
                (prefix.is_complete()
                    && prefix
                        .state
                        .contains_key(&("node".to_owned(), root.clone())))
                .then_some(prefix)
            });
            let (created, _) = landed(
                &first,
                &one,
                vec![
                    assertion_node(&a, "Claim", Some(original)),
                    resource(&r),
                    sources(&s, &a, &r, "node", &r),
                    on_existing(
                        &rooted,
                        ChangeOp::CreateEdge {
                            id: contained,
                            from_id: root,
                            to_id: a.clone(),
                            edge_type: "contains".to_owned(),
                            properties: Map::new(),
                            from_lifecycle_revision: None,
                            to_lifecycle_revision: None,
                        },
                    ),
                ],
            );
            let verify = verified(&second, &two, &a, &created);
            let node = node_at(&prefix_with(&one, &verify), &a);
            std::fs::write(&saved, json!({"node": a}).to_string()).expect("saved");
            report(
                "112-x29-roles-seed",
                &json!({"node": a, "revision": node["revision"],
                        "description": node["description"], "verify_event": verify}),
            );
        }
        "roles" => {
            let kept: Value =
                serde_json::from_str(&std::fs::read_to_string(&saved).expect("saved"))
                    .expect("json");
            let a = kept["node"].as_str().expect("node").to_owned();
            let (edit, reversal) = (
                env("KEEPER_HQ_LIVE_AGENT_EDIT_EVENT"),
                env("KEEPER_HQ_LIVE_AGENT_REVERSAL_EVENT"),
            );
            let prefix = prefix_with(&one, &reversal);
            let (edited, reversing) = (header(&prefix, &edit), header(&prefix, &reversal));
            assert_eq!(
                (edited.origin_class, reversing.origin_class),
                (OriginClass::Agent, OriginClass::Agent)
            );
            assert_eq!(edited.actor_id, reversing.actor_id, "the agent's own edit");
            assert_eq!(reversing.reverses_event_id.as_deref(), Some(edit.as_str()));
            let was = after_revision(&prefix, &edit, &a);
            let node = unverified_on(&both, &reversal, &a, &was);
            assert_eq!(node["description"], original);
            report(
                "112-x29-agent-reversed",
                &json!({"edit": edit, "event": reversal, "node": node}),
            );

            let editing = person("KEEPER_HQ_LIVE_EDITOR", "editor");
            let editor = GraphClient::new(editing.clone());
            let review = reviewer();
            let checker = GraphClient::new(review.clone());
            let mut latest = reversal;
            for (role, reverser, source) in [
                ("editor", &editor, &editing),
                ("reviewer", &checker, &review),
            ] {
                let verify = verified(&second, &two, &a, &latest);
                let expected = node_revision(&prefix_with(&one, &verify), &a);
                let (edit, _) = landed(
                    &editor,
                    &editing,
                    vec![describe(
                        &a,
                        &expected,
                        &format!("Edited for the {role}'s reversal"),
                    )],
                );
                let prefix = prefix_with(&one, &edit);
                assert_eq!(header(&prefix, &edit).actor_id, editing.actor);
                let was = after_revision(&prefix, &edit, &a);
                let (_, outcome) = reversed(reverser, source, &edit);
                let (reversal, _) = accepted(&outcome);
                let node = unverified_on(&both, &reversal, &a, &was);
                assert_eq!(node["description"], original);
                let reversing = header(&prefix_with(&one, &reversal), &reversal).clone();
                assert_eq!(reversing.actor_id, source.actor);
                assert_eq!(reversing.reverses_event_id.as_deref(), Some(edit.as_str()));
                report(
                    &format!("112-x29-{role}-reversed"),
                    &json!({"edit": edit, "event": reversal, "node": node}),
                );
                latest = reversal;
            }
            close(editing);
            close(review);
        }
        phase => panic!("unknown phase {phase}"),
    }
    close(one);
    close(two);
}

/// Story 111.2 CC-X29 (S:107): lifecycle reversals never verify. A
/// verified assertion archived (cleared in its after-image), the archive
/// reversed (active, unverified) and that reversal reversed (archived,
/// unverified); likewise from an unarchive of a verified-then-archived
/// assertion: reversed (archived) and re-reversed (active), unverified.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (111.2 server)"]
#[allow(clippy::too_many_lines)]
fn graph_live_112_x29_lifecycle_reversal() {
    let (one, two) = (device(1), device(2));
    let both = [&one, &two];
    let (first, second) = (GraphClient::new(one.clone()), GraphClient::new(two.clone()));
    let [a, b, r, sa, sb] = std::array::from_fn(|_| ulid());
    let (created, _) = landed(
        &first,
        &one,
        vec![
            assertion_node(&a, "Archived first", None),
            assertion_node(&b, "Unarchived first", None),
            resource(&r),
            sources(&sa, &a, &r, "node", &r),
            sources(&sb, &b, &r, "node", &r),
        ],
    );
    // `start` left `id` `unverified` in `lifecycles[0]`'s opposite; its
    // reversal and that reversal's reversal, each read on both replicas.
    let reverse_twice = |start: String, id: &str, lifecycles: [&str; 2]| -> Vec<Value> {
        let mut target = start;
        let mut was = after_revision(&prefix_with(&one, &target), &target, id);
        let mut steps = Vec::new();
        for (lifecycle, (client, source)) in lifecycles
            .into_iter()
            .zip([(&second, &two), (&first, &one)])
        {
            let (_, outcome) = reversed(client, source, &target);
            let (event, _) = accepted(&outcome);
            let node = unverified_on(&both, &event, id, &was);
            assert_eq!(node["lifecycle"], lifecycle, "{id} after {event}");
            let header = header(&prefix_with(&one, &event), &event).clone();
            assert_eq!(header.reverses_event_id.as_deref(), Some(target.as_str()));
            was = node["revision"].as_str().expect("revision").to_owned();
            steps.push(json!({"event": event, "node": node}));
            target = event;
        }
        steps
    };

    // From an archive.
    let verify = verified(&second, &two, &a, &created);
    let expected = node_revision(&prefix_with(&one, &verify), &a);
    let (archived, _) = landed(&first, &one, vec![lifecycle_op(&a, &expected, true)]);
    let node = unverified_on(&both, &archived, &a, &expected);
    assert_eq!(node["lifecycle"], "archived");
    let steps = reverse_twice(archived.clone(), &a, ["active", "archived"]);
    report(
        "112-x29-lifecycle-from-archive",
        &json!({"archived": {"event": archived, "node": node}, "reversals": steps}),
    );

    // From an unarchive.
    let verify = verified(&second, &two, &b, &created);
    let expected = node_revision(&prefix_with(&one, &verify), &b);
    let (archived, _) = landed(&first, &one, vec![lifecycle_op(&b, &expected, true)]);
    let expected = after_revision(&prefix_with(&one, &archived), &archived, &b);
    let (unarchived, _) = landed(&first, &one, vec![lifecycle_op(&b, &expected, false)]);
    let node = unverified_on(&both, &unarchived, &b, &expected);
    assert_eq!(node["lifecycle"], "active");
    let steps = reverse_twice(unarchived.clone(), &b, ["archived", "active"]);
    report(
        "112-x29-lifecycle-from-unarchive",
        &json!({"unarchived": {"event": unarchived, "node": node}, "reversals": steps}),
    );
    close(one);
    close(two);
}

/// Story 111.2 CC-X29 (S:108): a person's `verify_assertion` reversed, that
/// reversal reversed, and that one reversed (depth 3): never re-stamped —
/// `unverified`, null `verified_*`, revisions forward and the currency
/// read false at every depth, on both replicas.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (111.2 server)"]
fn graph_live_112_x29_verify_reversed_to_depth_3() {
    let (one, two) = (device(1), device(2));
    let both = [&one, &two];
    let (first, second) = (GraphClient::new(one.clone()), GraphClient::new(two.clone()));
    let [a, r, s] = std::array::from_fn(|_| ulid());
    let (created, _) = landed(
        &first,
        &one,
        vec![
            assertion_node(&a, "Claim", None),
            resource(&r),
            sources(&s, &a, &r, "node", &r),
        ],
    );
    let mut target = verified(&second, &two, &a, &created);
    let mut was = after_revision(&prefix_with(&one, &target), &target, &a);
    let mut depths = Vec::new();
    for (depth, (client, source)) in [(&first, &one), (&second, &two), (&first, &one)]
        .into_iter()
        .enumerate()
    {
        let (_, outcome) = reversed(client, source, &target);
        let (event, _) = accepted(&outcome);
        let node = unverified_on(&both, &event, &a, &was);
        let header = header(&prefix_with(&two, &event), &event).clone();
        assert_eq!(header.reverses_event_id.as_deref(), Some(target.as_str()));
        was = node["revision"].as_str().expect("revision").to_owned();
        depths.push(json!({"depth": depth + 1, "event": event, "node": node}));
        target = event;
    }
    report("112-x29-verify-depth-3", &json!(depths));
    close(one);
    close(two);
}

/// Story 111.2 CC-X29 (S:109): a merge that rewrites one of verified A's
/// `sources` edges. Without an operation on A the change is refused
/// `invalid_change` (a verified state's source is never written beside
/// it), pinned with no effect. With A's edit in the same change (which
/// clears its verification) it is accepted; its reversal restores the
/// edge's endpoints and A's prior fields with A unverified, and the
/// re-merge (the reversal reversed) is unverified too.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (111.2 server)"]
#[allow(clippy::too_many_lines)]
fn graph_live_112_x29_merge_restore_with_sources() {
    let (one, two) = (device(1), device(2));
    let both = [&one, &two];
    let (first, second) = (GraphClient::new(one.clone()), GraphClient::new(two.clone()));
    let [a, r1, r2, s] = std::array::from_fn(|_| ulid());
    let (before_merge, after_merge) = ("Before the merge", "Edited with the merge");
    let (created, _) = landed(
        &first,
        &one,
        vec![
            assertion_node(&a, "Claim", Some(before_merge)),
            resource(&r1),
            resource(&r2),
            sources(&s, &a, &r1, "node", &r1),
        ],
    );
    let verify = verified(&second, &two, &a, &created);
    let mut before = held_with(&both, &verify);
    let base = before[0].prefix.clone();
    let prefix = &base;
    let merge = merge_of(prefix, &r2, &[&r1], &[&s]);
    let verified_revision = node_revision(prefix, &a);

    // The merge alone writes the verified assertion's source: refused.
    let (_, unguarded) = applied(
        &first,
        &one,
        &first.change(vec![merge.clone()], None).expect("merge"),
    );
    assert_eq!(
        rejected_reason(&unguarded),
        Some("invalid_change"),
        "{unguarded:?}"
    );
    unchanged(
        "112-x29-merge-without-a-refused",
        &both,
        &mut before,
        &json!({"outcome": outcome_json(&unguarded), "node": node_at(prefix, &a)}),
    );

    // With A's edit: accepted, A unverified, the edge on the survivor.
    let (merged, _) = landed(
        &first,
        &one,
        vec![describe(&a, &verified_revision, after_merge), merge],
    );
    let node = unverified_on(&both, &merged, &a, &verified_revision);
    assert_eq!(node["description"], after_merge);
    let rewritten = |event: &str, to: &str, absorbed: Value| {
        for source in both {
            let prefix = prefix_with(source, event);
            assert_eq!(text(edge_image(&prefix, &s), "to_id"), to);
            assert_eq!(node_at(&prefix, &r1)["merged_into_id"], absorbed);
        }
    };
    rewritten(&merged, &r2, json!(r2));
    let mut steps = vec![json!({"event": merged, "node": node})];

    // Its reversal: endpoints and A's fields restored, A unverified.
    let was = after_revision(&prefix_with(&one, &merged), &merged, &a);
    let (_, outcome) = reversed(&second, &two, &merged);
    let (restored, _) = accepted(&outcome);
    let node = unverified_on(&both, &restored, &a, &was);
    assert_eq!(node["description"], before_merge);
    rewritten(&restored, &r1, Value::Null);
    assert_eq!(
        node_at(&prefix_with(&one, &restored), &r1)["lifecycle"],
        "active"
    );
    steps.push(json!({"event": restored, "node": node}));

    // The re-merge: unverified as well.
    let was = node["revision"].as_str().expect("revision").to_owned();
    let (_, outcome) = reversed(&first, &one, &restored);
    let (remerged, _) = accepted(&outcome);
    let node = unverified_on(&both, &remerged, &a, &was);
    assert_eq!(node["description"], after_merge);
    rewritten(&remerged, &r2, json!(r2));
    steps.push(json!({"event": remerged, "node": node}));
    report("112-x29-merge-restore", &json!(steps));
    close(one);
    close(two);
}

/// Story 111.2 CC-X29 (S:110): A is verified now, and the reversed event
/// created, archived or endpoint-rewrote (a merge) a `sources` edge from A,
/// or updated a content A owns, with no operation on A: the reversal is
/// refused `invalid_change`, its preview says the same, and nothing
/// changes. After the person's ordinary edit of A clears its verification,
/// each of those reversals is accepted.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (111.2 server)"]
#[allow(clippy::too_many_lines)]
fn graph_live_112_x29_invalidation_then_clear() {
    let (one, two) = (device(1), device(2));
    let both = [&one, &two];
    let (first, second) = (GraphClient::new(one.clone()), GraphClient::new(two.clone()));
    let [a, r0, r1, r3, r4, s0, s1, s2, s3, c, c4] = std::array::from_fn(|_| ulid());
    // `s3` names a content of `r4`: once a merge moves its end from `r3`
    // to `r4` it resolves, so A can be verified with it.
    let (setup, _) = landed(
        &first,
        &one,
        vec![
            assertion_node(&a, "Claim", None),
            resource(&r0),
            resource(&r1),
            resource(&r3),
            resource(&r4),
            sources(&s0, &a, &r0, "node", &r0),
            sources(&s2, &a, &r0, "node", &r0),
            ChangeOp::CreateContent {
                id: c.clone(),
                owner_node_id: a.clone(),
                body: json!({"inline_text": "v1"}),
            },
            ChangeOp::CreateContent {
                id: c4.clone(),
                owner_node_id: r4.clone(),
                body: json!({"inline_text": "held"}),
            },
            sources(&s3, &a, &r3, "content", &c4),
        ],
    );
    let (created_edge, _) = landed(
        &first,
        &one,
        vec![on_existing(
            &prefix_with(&one, &setup),
            sources(&s1, &a, &r1, "node", &r1),
        )],
    );
    let prefix = prefix_with(&one, &created_edge);
    let edge = edge_image(&prefix, &s2);
    let (archived_edge, _) = landed(
        &first,
        &one,
        vec![ChangeOp::ArchiveEdge {
            id: s2.clone(),
            expected_revision: text(edge, "revision"),
            from_lifecycle_revision: text(node_image(&prefix, &a), "lifecycle_revision"),
            to_lifecycle_revision: text(node_image(&prefix, &r0), "lifecycle_revision"),
        }],
    );
    let (rewritten_edge, _) = landed(
        &first,
        &one,
        vec![merge_of(
            &prefix_with(&one, &archived_edge),
            &r4,
            &[&r3],
            &[&s3],
        )],
    );
    let (updated_content, _) = landed(
        &first,
        &one,
        vec![ChangeOp::UpdateContent {
            id: c.clone(),
            owner_node_id: a.clone(),
            expected_revision: "1".to_owned(),
            body: json!({"inline_text": "v2"}),
        }],
    );
    let verify = verified(&second, &two, &a, &updated_content);
    let targets = [
        ("create-sources", &created_edge),
        ("archive-sources", &archived_edge),
        ("rewrite-sources", &rewritten_edge),
        ("update-content", &updated_content),
    ];

    let mut before = held_with(&both, &verify);
    for (name, event) in targets {
        let preview = second.preview_reversal(event).expect("preview");
        assert_eq!(
            (preview.outcome, preview.reason.as_deref()),
            (PreviewOutcome::Rejected, Some("invalid_change")),
            "{name}: {preview:?}"
        );
        let reverse =
            Reverse::of(preview.plan.as_ref().expect("plan"), None, None).expect("reverse");
        let (prepared, outcome) = commanded(&second, &two, second.reverse(&reverse));
        assert_eq!(
            outcome.code(prepared.key()),
            Some("invalid_change"),
            "{name}: {outcome:?}"
        );
        let node = node_at(&before[0].prefix, &a);
        unchanged(
            &format!("112-x29-invalidation-{name}"),
            &both,
            &mut before,
            &json!({"preview": preview, "outcome": outcome_json(&outcome),
                    "node": node}),
        );
    }

    // An ordinary edit clears the verification; each reversal then lands.
    let expected = node_revision(&before[0].prefix, &a);
    let (cleared, _) = landed(&first, &one, vec![retitle(&a, &expected, "Edited claim")]);
    let node = unverified_on(&both, &cleared, &a, &expected);
    let mut reversals = Map::new();
    for (name, event) in targets {
        let (preview, outcome) = reversed(&second, &two, event);
        let (reversal, _) = accepted(&outcome);
        prefix_with(&one, &reversal);
        reversals.insert(
            name.to_owned(),
            json!({"preview": preview.outcome, "outcome": outcome_json(&outcome)}),
        );
    }
    report(
        "112-x29-invalidation-cleared",
        &json!({"cleared": {"event": cleared, "node": node}, "reversals": reversals}),
    );
    close(one);
    close(two);
}

/// Story 111.2 T4 (S:118): a preview is no reservation. Previewed, then
/// raced by an edit, a new incident edge, a new content, a new typed
/// reference and a merge: each reversal is `reversal_conflict`, a fresh
/// preview reports the conflict, and both replicas keep the original
/// state and history.
///
/// Phase `races` does those and records the agent's own event as both
/// replicas hold it. Between the phases the harness has the agent preview
/// that event's reversal, revokes the agent's capability and has the agent
/// send the reversal, which is refused. Phase `revoked`: the agent's event,
/// its entities and the history are as recorded on both replicas, with no
/// event of the refused request and no reversal of the agent's event.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (111.2 server)"]
#[allow(clippy::too_many_lines)]
fn graph_live_112_preview_races() {
    let saved = state_dir().join("races-112.json");
    let (one, two) = (device(1), device(2));
    let both = [&one, &two];
    let agent = env("KEEPER_HQ_LIVE_AGENT_EVENT");
    // The agent's event as `source` holds it: its digest, the images of
    // the entities it wrote, and how many events reverse it.
    let retained = |source: &Source| -> Value {
        let prefix = prefix_with(source, &agent);
        let event = prefix
            .events
            .iter()
            .find(|e| e.event.id == agent)
            .expect("the agent's event");
        let images: Map<String, Value> = event
            .ops
            .iter()
            .filter_map(|op| {
                let kind = serde_json::to_value(op.entity_type).ok()?;
                let kind = kind.as_str()?.to_owned();
                let image = prefix.state.get(&(kind.clone(), op.entity_id.clone()))?;
                Some((
                    format!("{kind}/{}", op.entity_id),
                    Value::Object(image.clone()),
                ))
            })
            .collect();
        let reversals = prefix
            .events
            .iter()
            .filter(|e| e.event.reverses_event_id.as_deref() == Some(agent.as_str()))
            .count();
        json!({"digest": event.event.delta_digest, "images": images, "reversals": reversals})
    };
    match env("KEEPER_HQ_LIVE_PHASE").as_str() {
        "races" => {
            let (first, second) = (GraphClient::new(one.clone()), GraphClient::new(two.clone()));
            let [x, p, t, y, w, u, m, edge, content, referrer] = std::array::from_fn(|_| ulid());
            landed(
                &first,
                &one,
                vec![
                    create_node(&x, "Edited", json!({})),
                    create_node(&p, "Other end", json!({})),
                    create_node(&t, "Survivor", json!({})),
                ],
            );
            let create =
                |id: &str| landed(&first, &one, vec![create_node(id, "Target", json!({}))]).0;
            let (edited, _) = landed(&first, &one, vec![retitle(&x, "1", "Edited once")]);
            let (made_y, made_w, made_u, made_m) = (create(&y), create(&w), create(&u), create(&m));
            // Each race's own entities: no race moves another's.
            let races = [
                ("edit", &edited, vec![retitle(&x, "2", "Edited again")]),
                (
                    "edge",
                    &made_y,
                    vec![on_existing(
                        &prefix_with(&one, &made_y),
                        edge_between(&edge, &p, &y),
                    )],
                ),
                (
                    "content",
                    &made_w,
                    vec![ChangeOp::CreateContent {
                        id: content.clone(),
                        owner_node_id: w.clone(),
                        body: json!({"inline_text": "raced"}),
                    }],
                ),
                (
                    "reference",
                    &made_u,
                    vec![create_node(
                        &referrer,
                        "Referrer",
                        json!({"see": {"$ref": {"entity_type": "node", "id": u,
                                                "revision": "1"}}}),
                    )],
                ),
                (
                    "merge",
                    &made_m,
                    vec![merge_of(&prefix_with(&one, &made_m), &t, &[&m], &[])],
                ),
            ];
            for (name, target, racing) in races {
                let preview = second.preview_reversal(target).expect("preview");
                assert_eq!(
                    preview.outcome,
                    PreviewOutcome::Ready,
                    "{name}: {preview:?}"
                );
                let reverse =
                    Reverse::of(preview.plan.as_ref().expect("plan"), None, None).expect("reverse");
                let (raced, _) = landed(&first, &one, racing);
                let mut before = held_with(&both, &raced);
                let (prepared, outcome) = commanded(&second, &two, second.reverse(&reverse));
                assert_eq!(
                    outcome.code(prepared.key()),
                    Some("reversal_conflict"),
                    "{name}: {outcome:?}"
                );
                let again = second.preview_reversal(target).expect("preview");
                assert_eq!(again.outcome, PreviewOutcome::Conflict, "{name}: {again:?}");
                unchanged(
                    &format!("112-race-{name}"),
                    &both,
                    &mut before,
                    &json!({"raced": raced, "outcome": outcome_json(&outcome),
                            "preview_after": again}),
                );
            }
            let recorded = retained(&one);
            assert_eq!(retained(&two), recorded);
            std::fs::write(&saved, recorded.to_string()).expect("saved");
            report("112-race-agent-before", &recorded);
        }
        "revoked" => {
            let recorded: Value =
                serde_json::from_str(&std::fs::read_to_string(&saved).expect("saved"))
                    .expect("json");
            let refused_request = env("KEEPER_HQ_LIVE_AGENT_REFUSED_REQUEST");
            // A pull completed after the refused send on both replicas
            // before anything is compared.
            let (_, barrier_event, barrier_seq) = barrier(&both);
            for source in both {
                assert_eq!(
                    retained(source),
                    recorded,
                    "the agent's event and state retained"
                );
                let now = held(source);
                assert!(now.prefix.n >= barrier_seq);
                assert_eq!(events_of(&now.prefix, &refused_request), 0);
            }
            report(
                "112-race-agent-revoked",
                &json!({"retained": recorded, "refused_request": refused_request,
                        "events": 0, "barrier_event": barrier_event,
                        "barrier_seq": barrier_seq.to_string()}),
            );
        }
        phase => panic!("unknown phase {phase}"),
    }
    close(one);
    close(two);
}

/// Story 111.2 T3 (S:104): independent create, update, archive and merge
/// reversals on two replicas — the create and the merge to depth 3, with
/// archived inspection, parallel edges keeping their ids and immutable
/// original deltas; a redirect chain at 8 hops whose last merge is
/// reversed and re-merged; a re-merge refused by a new incident edge
/// (`dependency`) and by an archived target (`lifecycle_revision`). The
/// edge with both endpoints absorbed is pinned as what it is: a merge
/// that would make it a self-loop, refused `invalid_change`, not
/// substituted by another fixture.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (111.2 server)"]
#[allow(clippy::too_many_lines)]
fn graph_live_112_reversal_matrix_create_update_archive_merge() {
    let (one, two) = (device(1), device(2));
    let both = [&one, &two];
    let (first, second) = (GraphClient::new(one.clone()), GraphClient::new(two.clone()));

    // Create (to depth 3), update and archive, each reversed on its own.
    let [n1, n2, n3] = std::array::from_fn(|_| ulid());
    let (created, _) = landed(&first, &one, vec![create_node(&n1, "Created", json!({}))]);
    landed(
        &first,
        &one,
        vec![
            create_node(&n2, "Before the update", json!({})),
            create_node(&n3, "Before the archive", json!({})),
        ],
    );
    let (updated, _) = landed(&first, &one, vec![retitle(&n2, "1", "Updated")]);
    let (archived, _) = landed(&first, &one, vec![lifecycle_op(&n3, "1", true)]);
    let mut target = created;
    let mut was = "1".to_owned();
    let mut depths = Vec::new();
    for (lifecycle, (client, source)) in ["archived", "active", "archived"].into_iter().zip([
        (&second, &two),
        (&first, &one),
        (&second, &two),
    ]) {
        let (_, outcome) = reversed(client, source, &target);
        let (event, _) = accepted(&outcome);
        let mut read = Value::Null;
        for replica in both {
            let prefix = prefix_with(replica, &event);
            let node = node_at(&prefix, &n1);
            assert_eq!(node["lifecycle"], lifecycle);
            assert!(number(node["revision"].as_str().expect("revision")) > number(&was));
            assert_eq!(
                header(&prefix, &event).reverses_event_id.as_deref(),
                Some(target.as_str())
            );
            read = node;
        }
        let shown = second.get_node(&n1, false).expect("read").is_some();
        let inspected = second.get_node(&n1, true).expect("read").expect("retained");
        assert_eq!(shown, lifecycle == "active");
        assert_eq!(inspected.image["lifecycle"], lifecycle);
        was = read["revision"].as_str().expect("revision").to_owned();
        depths.push(json!({"event": event, "node": read, "shown": shown,
                           "inspected": inspected.image}));
        target = event;
    }
    report("112-matrix-create-depth-3", &json!(depths));
    let (_, outcome) = reversed(&second, &two, &updated);
    let (unupdated, _) = accepted(&outcome);
    let (_, outcome) = reversed(&second, &two, &archived);
    let (unarchived, _) = accepted(&outcome);
    let mut reads = Vec::new();
    for replica in both {
        // `unarchived` landed after `unupdated`: its complete prefix holds both.
        let prefix = prefix_with(replica, &unarchived);
        assert!(prefix.events.iter().any(|e| e.event.id == unupdated));
        let (n2_read, n3_read) = (node_at(&prefix, &n2), node_at(&prefix, &n3));
        assert_eq!(
            (n2_read["title"].clone(), n2_read["revision"].clone()),
            (json!("Before the update"), json!("3"))
        );
        assert_eq!(n3_read["lifecycle"], "active");
        reads.push(json!({"updated": n2_read, "archived": n3_read}));
    }
    assert!(first.get_node(&n3, false).expect("read").is_some());
    report("112-matrix-update-archive", &json!(reads));

    // A merge of two absorbed nodes rewriting three edges (two parallel),
    // reversed to depth 3.
    let [t, b1, b2, o, e1, e2, e3] = std::array::from_fn(|_| ulid());
    let (setup, _) = landed(
        &first,
        &one,
        vec![
            create_node(&t, "Survivor", json!({})),
            create_node(&b1, "Absorbed one", json!({})),
            create_node(&b2, "Absorbed two", json!({})),
            create_node(&o, "Other", json!({})),
            edge_between(&e1, &o, &b1),
            edge_between(&e2, &o, &b1),
            edge_between(&e3, &o, &b2),
        ],
    );
    let merge = merge_of(
        &prefix_with(&one, &setup),
        &t,
        &[&b1, &b2],
        &[&e1, &e2, &e3],
    );
    let (merged, _) = landed(&first, &one, vec![merge]);
    let ends = |event: &str, merged_into: &Value, to: [&String; 3]| {
        for replica in both {
            let prefix = prefix_with(replica, event);
            let now = [&e1, &e2, &e3].map(|id| text(edge_image(&prefix, id), "to_id"));
            assert_eq!(now, to.map(String::clone), "parallel edges keep their ids");
            for id in [&b1, &b2] {
                assert_eq!(&node_at(&prefix, id)["merged_into_id"], merged_into);
            }
        }
    };
    ends(&merged, &json!(t), [&t, &t, &t]);
    assert_eq!(second.redirects(&b1).expect("read"), Some(vec![t.clone()]));
    assert!(second.get_node(&b1, false).expect("read").is_none());
    let inspected = second.get_node(&b1, true).expect("read").expect("retained");
    assert_eq!(
        (
            inspected.image["lifecycle"].clone(),
            inspected.image["merged_into_id"].clone()
        ),
        (json!("archived"), json!(t))
    );
    let digest = digest_of(&prefix_with(&one, &merged), &merged);
    let (_, outcome) = reversed(&second, &two, &merged);
    let (restored, _) = accepted(&outcome);
    ends(&restored, &Value::Null, [&b1, &b1, &b2]);
    assert_eq!(first.redirects(&b1).expect("read"), Some(Vec::new()));
    let (_, outcome) = reversed(&first, &one, &restored);
    let (remerged, _) = accepted(&outcome);
    ends(&remerged, &json!(t), [&t, &t, &t]);
    assert_eq!(second.redirects(&b2).expect("read"), Some(vec![t.clone()]));
    let (_, outcome) = reversed(&second, &two, &remerged);
    let (depth_3, _) = accepted(&outcome);
    ends(&depth_3, &Value::Null, [&b1, &b1, &b2]);
    for replica in both {
        assert_eq!(
            digest_of(&prefix_with(replica, &depth_3), &merged),
            digest,
            "the original delta is immutable"
        );
    }
    report(
        "112-matrix-merge-depth-3",
        &json!({"merged": merged, "reversals": [restored, remerged, depth_3],
                "inspected": inspected.image, "digest": digest}),
    );

    // A redirect chain at 8 hops: the last merge reversed, then re-merged.
    let chain: [String; 9] = std::array::from_fn(|_| ulid());
    let (mut last, _) = landed(
        &first,
        &one,
        chain
            .iter()
            .map(|id| create_node(id, "Hop", json!({})))
            .collect(),
    );
    for pair in chain.windows(2) {
        let merge = merge_of(&prefix_with(&one, &last), &pair[1], &[&pair[0]], &[]);
        last = landed(&first, &one, vec![merge]).0;
    }
    let hops = |client: &GraphClient| {
        client
            .redirects(&chain[0])
            .expect("read")
            .expect("the node")
    };
    // The chain as each persisted replica stores it after `event`: hop i
    // merged into hop i+1 and archived, for every hop below `merged`.
    let stored = |event: &str, merged: usize| {
        for replica in both {
            let prefix = prefix_with(replica, event);
            for (i, id) in chain.iter().enumerate() {
                let node = node_at(&prefix, id);
                if i < merged {
                    assert_eq!(
                        (node["merged_into_id"].clone(), node["lifecycle"].clone()),
                        (json!(chain[i + 1]), json!("archived")),
                        "hop {i}"
                    );
                } else {
                    assert_eq!(
                        (node["merged_into_id"].clone(), node["lifecycle"].clone()),
                        (Value::Null, json!("active")),
                        "hop {i}"
                    );
                }
            }
        }
    };
    stored(&last, 8);
    assert_eq!(hops(&second), chain[1..].to_vec());
    let (_, outcome) = reversed(&second, &two, &last);
    let (unmerged, _) = accepted(&outcome);
    stored(&unmerged, 7);
    assert_eq!(hops(&first), chain[1..8].to_vec());
    let (_, outcome) = reversed(&first, &one, &unmerged);
    let (remerged, _) = accepted(&outcome);
    stored(&remerged, 8);
    assert_eq!(hops(&second), chain[1..].to_vec());
    report(
        "112-matrix-redirect-8-hops",
        &json!({"chain": hops(&first), "reversed": unmerged, "remerged": remerged}),
    );

    // A re-merge refused by a new incident edge: `dependency` on the node.
    let [t4, b4, o4, e4, e5] = std::array::from_fn(|_| ulid());
    let (setup, _) = landed(
        &first,
        &one,
        vec![
            create_node(&t4, "Survivor", json!({})),
            create_node(&b4, "Absorbed", json!({})),
            create_node(&o4, "Other", json!({})),
            edge_between(&e4, &o4, &b4),
        ],
    );
    let merge = merge_of(&prefix_with(&one, &setup), &t4, &[&b4], &[&e4]);
    let (merged, _) = landed(&first, &one, vec![merge]);
    let (_, outcome) = reversed(&second, &two, &merged);
    let (restored, _) = accepted(&outcome);
    let (incident, _) = landed(
        &first,
        &one,
        vec![on_existing(
            &prefix_with(&one, &restored),
            edge_between(&e5, &o4, &b4),
        )],
    );
    let mut before = held_with(&both, &incident);
    let detail = conflicted(
        &second,
        &two,
        &restored,
        &both,
        &mut before,
        "112-matrix-remerge-dependency",
    );
    assert_eq!(
        (
            detail["precondition"].clone(),
            detail["entity"]["id"].clone()
        ),
        (json!("dependency"), json!(b4))
    );

    // A re-merge into an archived target: `lifecycle_revision` on it.
    let [t5, b5] = std::array::from_fn(|_| ulid());
    let (setup, _) = landed(
        &first,
        &one,
        vec![
            create_node(&t5, "Survivor", json!({})),
            create_node(&b5, "Absorbed", json!({})),
        ],
    );
    let merge = merge_of(&prefix_with(&one, &setup), &t5, &[&b5], &[]);
    let (merged, _) = landed(&first, &one, vec![merge]);
    let (_, outcome) = reversed(&second, &two, &merged);
    let (restored, _) = accepted(&outcome);
    let expected = node_revision(&prefix_with(&one, &restored), &t5);
    let (gone, _) = landed(&first, &one, vec![lifecycle_op(&t5, &expected, true)]);
    let mut before = held_with(&both, &gone);
    let detail = conflicted(
        &second,
        &two,
        &restored,
        &both,
        &mut before,
        "112-matrix-remerge-archived-target",
    );
    assert_eq!(
        (
            detail["precondition"].clone(),
            detail["entity"]["id"].clone()
        ),
        (json!("lifecycle_revision"), json!(t5))
    );

    // Both endpoints of an edge absorbed into one survivor: the self-loop
    // the merge rules forbid, refused, never substituted.
    let [x1, x2, y, loop_edge] = std::array::from_fn(|_| ulid());
    let (setup, _) = landed(
        &first,
        &one,
        vec![
            create_node(&x1, "Absorbed one", json!({})),
            create_node(&x2, "Absorbed two", json!({})),
            create_node(&y, "Survivor", json!({})),
            edge_between(&loop_edge, &x1, &x2),
        ],
    );
    let mut before = held_with(&both, &setup);
    let merge = merge_of(&before[0].prefix, &y, &[&x1, &x2], &[&loop_edge]);
    let (_, outcome) = applied(
        &first,
        &one,
        &first.change(vec![merge], None).expect("merge"),
    );
    assert_eq!(
        rejected_reason(&outcome),
        Some("invalid_change"),
        "{outcome:?}"
    );
    let reads = json!({"outcome": outcome_json(&outcome), "fixture": "self-loop",
                       "edge": edge_image(&before[0].prefix, &loop_edge)});
    unchanged(
        "112-matrix-both-ends-absorbed-refused",
        &both,
        &mut before,
        &reads,
    );
    close(one);
    close(two);
}

/// The replica's current rows (they bootstrap apart from the events) of
/// these nodes, edges and contents, in id order: the columns the contract
/// replicates, not the authorship bookkeeping.
fn rows_of(source: &Source, nodes: &[&str], edges: &[&str], contents: &[&str]) -> Value {
    let select = |columns: &str, table: &str, ids: &[&str]| -> Value {
        let marks = vec!["?"; ids.len()].join(", ");
        let params: Vec<Value> = ids.iter().map(|id| Value::from(*id)).collect();
        let rows = source
            .replica
            .query(
                &format!("SELECT {columns} FROM {table} WHERE id IN ({marks}) ORDER BY id"),
                &params,
            )
            .expect("rows");
        Value::Array(rows.into_iter().map(Value::Object).collect())
    };
    json!({
        "nodes": select("id, kind, title, description, properties, lifecycle, merged_into_id, \
                         revision, lifecycle_revision, updated_event_id", "kg_nodes", nodes),
        "edges": select("id, from_id, to_id, type, properties, lifecycle, revision, \
                         updated_event_id", "kg_edges", edges),
        "contents": select("id, owner_node_id, revision, body, updated_event_id", "kg_contents",
                           contents),
    })
}

/// Story 111.1 T10 (F1(e)): after content created and updated, a merge with
/// its redirect and a server-stamped verification on the two persistent
/// replicas, a fresh third replica bootstrapped late holds the same: the
/// head and prefix, the node, content and edge rows with their revisions,
/// the redirect chain and the `$assertion` with `verified_at` and
/// `verified_by`; archived inspection through it reads the same.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (111.1 server)"]
#[allow(clippy::too_many_lines)]
fn graph_live_111_late_bootstrap_after_content_merge_verify() {
    let (one, two) = (device(1), device(2));
    let (first, second) = (GraphClient::new(one.clone()), GraphClient::new(two.clone()));
    let [x, t, c, claim, res, src] = std::array::from_fn(|_| ulid());
    let (created, _) = landed(
        &first,
        &one,
        vec![
            create_node(&x, "Absorbed", json!({})),
            create_node(&t, "Survivor", json!({})),
            ChangeOp::CreateContent {
                id: c.clone(),
                owner_node_id: x.clone(),
                body: json!({"inline_text": "v1"}),
            },
            resource(&res),
            assertion_node(&claim, "Claim", None),
            sources(&src, &claim, &res, "node", &res),
        ],
    );
    prefix_with(&two, &created);
    let (updated, _) = landed(
        &second,
        &two,
        vec![ChangeOp::UpdateContent {
            id: c.clone(),
            owner_node_id: x.clone(),
            expected_revision: "1".to_owned(),
            body: json!({"inline_text": "v2"}),
        }],
    );
    let (merged, _) = landed(
        &first,
        &one,
        vec![merge_of(&prefix_with(&one, &updated), &t, &[&x], &[])],
    );
    let verify = verified(&second, &two, &claim, &merged);
    let (nodes, edges, contents) = (
        [x.as_str(), t.as_str(), claim.as_str(), res.as_str()],
        [src.as_str()],
        [c.as_str()],
    );
    let images = |prefix: &Prefix| -> Vec<((String, String), Map<String, Value>)> {
        prefix
            .state
            .iter()
            .filter(|(key, _)| {
                nodes
                    .iter()
                    .chain(&edges)
                    .chain(&contents)
                    .any(|id| *id == key.1)
            })
            .map(|(key, image)| (key.clone(), image.clone()))
            .collect()
    };
    let reference = prefix_with(&one, &verify);
    assert_eq!(images(&prefix_with(&two, &verify)), images(&reference));
    let rows = |source: &Source| rows_of(source, &nodes, &edges, &contents);
    // Rows at the reference: every one present at the revision the prefix
    // holds for it.
    let at_reference = |rows: &Value| {
        [
            ("nodes", "node", nodes.len()),
            ("edges", "edge", edges.len()),
            ("contents", "content", contents.len()),
        ]
        .into_iter()
        .all(|(table, kind, count)| {
            rows[table].as_array().is_some_and(|found| {
                found.len() == count
                    && found.iter().all(|row| {
                        let id = row["id"].as_str().unwrap_or_default().to_owned();
                        reference
                            .state
                            .get(&(kind.to_owned(), id))
                            .is_some_and(|image| row["revision"] == image["revision"])
                    })
            })
        })
    };
    let held_rows = until(&one, "device one's rows at the reference", || {
        let held = rows(&one);
        at_reference(&held).then_some(held)
    });
    until(&two, "device two's rows at the reference", || {
        (rows(&two) == held_rows).then_some(())
    });
    let chain = second.redirects(&x).expect("read");
    assert_eq!(chain, Some(vec![t.clone()]));
    let inspect = |client: &GraphClient| {
        json!({
            "active": client.get_node(&x, false).expect("read").is_some(),
            "archived": client.get_node(&x, true).expect("read").map(|view| view.image),
            "content": client.get_content(&c, true).expect("read"),
        })
    };
    let inspected = inspect(&first);
    assert_eq!(inspected["active"], false);

    // The third replica: device 2's credential, a file never synced.
    close(two);
    let late = open_at(
        2,
        &env("KEEPER_HQ_LIVE_DATABASE"),
        &env("KEEPER_HQ_LIVE_URL"),
        &format!("late-111-{}.sqlite", ulid()),
    );
    // The fixture's last event is the verification: the late replica's
    // complete prefix is the reference's exactly — same height, same head,
    // same events, same state. (Nothing else writes this database during
    // the case; a later event would fail this, not be skipped.)
    assert!(
        reference.is_complete(),
        "the reference is a complete prefix"
    );
    let fresh = prefix_at(&late, reference.n);
    assert_eq!(
        (fresh.n, fresh.head),
        (reference.n, reference.head),
        "the late replica bootstraps to the same height and head"
    );
    assert_eq!(fresh.events, reference.events);
    assert_eq!(fresh.state, reference.state);
    assert_eq!(images(&fresh), images(&reference));
    let (late_claim, held_claim) = (node_at(&fresh, &claim), node_at(&reference, &claim));
    let stamp = &late_claim["properties"]["$assertion"];
    assert_eq!(stamp, &held_claim["properties"]["$assertion"]);
    assert_eq!(stamp["status"], "verified");
    assert!(assertion::is_current(&Now(&fresh), &claim));
    until(&late, "the late replica's rows", || {
        (rows(&late) == held_rows).then_some(())
    });
    let later = GraphClient::new(late.clone());
    assert_eq!(later.redirects(&x).expect("read"), chain);
    assert_eq!(inspect(&later), inspected);
    report(
        "111-late-bootstrap",
        &json!({"n": fresh.n, "head": fresh.head, "reference_n": reference.n,
                "rows": held_rows, "redirects": chain, "assertion": stamp,
                "inspected": inspected}),
    );
    close(one);
    close(late);
}

// ---------------------------------------------------------------------------
// Story 112.1 (T5): durable intents and revocation, over the queue file and
// the fence. Extra harness inputs:
// - `KEEPER_HQ_LIVE_OFFLINE_URL`: an address nothing answers (the network is
//   cut; a send leaves the gate and fails);
// - `KEEPER_HQ_LIVE_SERVER_2_URL`: a second keeper-hq holding a database of
//   the same name (queues never cross servers);
// - `KEEPER_HQ_LIVE_DROP_URL` (as above) for
//   `graph_live_1121_a_result_cache_miss_resolves_from_the_receipt`;
// - `KEEPER_HQ_LIVE_PHASE` per case, as each case names
//   (`graph_live_1121_a_crash_before_payload_cleanup_settles_once`:
//   `commit`, then `restart` in a new process).
// ---------------------------------------------------------------------------

/// The person's queue file and fence for `source`'s database on `url`.
fn lifecycle_at(url: &str, source: &Source) -> Lifecycle {
    let server = ServerUrl::parse(url)
        .expect("a server address")
        .expect("not local");
    let file = keeper_core::hq::databases::graph_queue(
        &state_dir(),
        &server,
        &source.database,
        &source.actor,
    );
    Lifecycle::open(GraphQueue::open(&file, Binding::of(&server, source)).expect("queue"))
        .expect("lifecycle")
}

/// What the driver's `resume` does: restore every entry, look its receipt
/// up first, and send the identical request under its own id only when the
/// one policy says nothing is known of it ([`resume_sends`]) and the domain
/// is not locked; every answer goes through the one policy. The states each
/// entry ended in.
fn resume_all(lifecycle: &Lifecycle, client: &GraphClient) -> Vec<(String, State)> {
    let restored = lifecycle
        .with_queue(|q| q.restore(client, now_ms()))
        .expect("restores");
    let mut states = Vec::new();
    for restored in restored {
        let (prepared, state) = match restored {
            Restored::Ready { prepared, state } => (prepared, state),
            Restored::Suspended { request_id } => {
                states.push((request_id, State::Suspended));
                continue;
            }
            Restored::Corrupt { request_id } => panic!("corrupt entry {request_id}"),
        };
        let id = prepared.request().request_id.clone();
        let looked = client.outcome(&prepared, None).expect("lookup");
        let from = state;
        let state = lifecycle
            .settle(&id, from, &looked, now_ms())
            .expect("settled");
        let agent = prepared.request().context_id.is_some();
        let sendable = resume_sends(from, &looked, state, agent)
            && lifecycle.gate().is_ok()
            && client.online().is_ok();
        if !sendable {
            states.push((id, state));
            continue;
        }
        lifecycle
            .with_queue(|q| match state {
                State::Submitting => q.retry(&id, "resent_same_id", now_ms()),
                from => q.mark(&id, from, State::Submitting, Some("sent"), now_ms()),
            })
            .expect("moved");
        let attempt = client.send(&prepared).expect("sent");
        let outcome = client.outcome(&prepared, Some(&attempt)).expect("outcome");
        let state = lifecycle
            .settle(&id, State::Submitting, &outcome, now_ms())
            .expect("settled");
        states.push((id, state));
    }
    states
}

fn now_ms() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_millis(),
    )
    .expect("ms")
}

/// The request phase `offline` kept: its id and payload (a person's: no
/// context).
fn saved_request(path: &std::path::Path) -> Request {
    let kept: Value =
        serde_json::from_str(&std::fs::read_to_string(path).expect("saved")).expect("json");
    Request {
        request_id: kept["request_id"].as_str().expect("id").to_owned(),
        context_id: None,
        payload: kept["payload"].clone(),
    }
}

fn entry_of(lifecycle: &Lifecycle, id: &str) -> Option<keeper_core::graph::queue::Entry> {
    lifecycle
        .with_queue(|q| q.entries())
        .expect("listing")
        .into_iter()
        .find(|e| e.request_id == id)
}

/// S08-AC1 (T5 steps 1, 2, 4): phase `offline` enqueues a change with the
/// network cut and sends nothing; phase `restart` (a new process, still
/// offline) finds the same identity and bytes, still queued, and the
/// accepted prefix holds no event of it; phase `online` resumes it — its
/// receipt first, then the one identical send — and exactly one event is
/// accepted. A second resume sends nothing.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (112.1)"]
fn graph_live_1121_an_offline_intent_survives_a_restart_and_lands_once() {
    let saved = state_dir().join("intent-1121.json");
    let url = env("KEEPER_HQ_LIVE_URL");
    match env("KEEPER_HQ_LIVE_PHASE").as_str() {
        "offline" => {
            let source = open(
                1,
                &env("KEEPER_HQ_LIVE_DATABASE"),
                &env("KEEPER_HQ_LIVE_OFFLINE_URL"),
            );
            let lifecycle = lifecycle_at(&url, &source);
            let client = GraphClient::new(source.clone());
            let change = client
                .change(
                    vec![create_node(&ulid(), "Queued offline", json!({}))],
                    None,
                )
                .expect("change");
            let prepared = client.mint(&change).expect("minted offline");
            lifecycle
                .with_queue(|q| q.enqueue(&prepared, now_ms()))
                .expect("queued");
            let request = prepared.request();
            let kept = json!({"request_id": request.request_id, "payload": request.payload});
            std::fs::write(&saved, kept.to_string()).expect("saved");
            report(
                "1121-offline",
                &json!({"request_id": prepared.request().request_id,
                                          "state": "queued"}),
            );
            close(source);
        }
        "restart" => {
            let source = open(
                1,
                &env("KEEPER_HQ_LIVE_DATABASE"),
                &env("KEEPER_HQ_LIVE_OFFLINE_URL"),
            );
            let lifecycle = lifecycle_at(&url, &source);
            let client = GraphClient::new(source.clone());
            let request = saved_request(&saved);
            let restored = lifecycle
                .with_queue(|q| q.restore(&client, now_ms()))
                .expect("restores");
            assert_eq!(restored.len(), 1);
            let Restored::Ready { prepared, state } = &restored[0] else {
                panic!("not restored: {restored:?}");
            };
            assert_eq!(*state, State::Queued);
            assert_eq!(
                prepared.request(),
                &request,
                "same identity, payload, preconditions"
            );
            // Offline, no wait: the prefix as the file holds it. Complete,
            // and its height is reported, so an empty one is visible.
            let prefix = prefix::complete(&source).expect("no corruption");
            assert!(prefix.is_complete(), "a complete prefix");
            assert_eq!(
                events_of(&prefix, &request.request_id),
                0,
                "accepted state only"
            );
            report(
                "1121-restart",
                &json!({"request_id": request.request_id, "state": "queued",
                                          "events": 0, "prefix_n": prefix.n}),
            );
            close(source);
        }
        "online" => {
            let source = device(1);
            let lifecycle = lifecycle_at(&url, &source);
            let client = GraphClient::new(source.clone());
            let request = saved_request(&saved);
            until(&source, "the first round", || {
                (source.replica.state().link == keeper_core::hq::port::LinkState::Connected)
                    .then_some(())
            });
            let states = resume_all(&lifecycle, &client);
            assert_eq!(states, vec![(request.request_id.clone(), State::Accepted)]);
            let entry = entry_of(&lifecycle, &request.request_id).expect("record kept");
            assert_eq!(entry.attempts, 1, "sent once");
            // Nothing left to send.
            assert!(resume_all(&lifecycle, &client).is_empty());
            let prepared = client
                .prepared(DESCRIPTOR, request.clone())
                .expect("same request");
            let (_, seq) = accepted(&settled(&client, &source, &prepared));
            assert_eq!(events_of(&prefix_at(&source, seq), &request.request_id), 1);
            report(
                "1121-online",
                &json!({"request_id": request.request_id,
                                         "state": "accepted", "attempts": entry.attempts,
                                         "events": 1}),
            );
            close(source);
        }
        phase => panic!("unknown phase {phase}"),
    }
}

/// S08-AC2 (T5 crash windows): a queued change is sent through a relay that
/// loses the answer, then the process dies with the entry `submitting`.
/// The next process looks the receipt up first (`graph.outcome`), resolves
/// it accepted and sends nothing again: one attempt, one event.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (112.1)"]
fn graph_live_1121_a_lost_answer_resolves_from_the_queue_without_a_resend() {
    let url = env("KEEPER_HQ_LIVE_URL");
    let lossy = open(
        1,
        &env("KEEPER_HQ_LIVE_DATABASE"),
        &env("KEEPER_HQ_LIVE_DROP_URL"),
    );
    let request_id = {
        let lifecycle = lifecycle_at(&url, &lossy);
        let client = GraphClient::new(lossy.clone());
        let change = client
            .change(
                vec![create_node(&ulid(), "Lost from the queue", json!({}))],
                None,
            )
            .expect("change");
        let prepared = client.mint(&change).expect("minted");
        let id = prepared.request().request_id.clone();
        lifecycle
            .with_queue(|q| q.enqueue(&prepared, now_ms()))
            .expect("queued");
        lifecycle
            .with_queue(|q| {
                q.mark(
                    &id,
                    State::Queued,
                    State::Submitting,
                    Some("sent"),
                    now_ms(),
                )
            })
            .expect("submitting");
        let attempt = client.send(&prepared).expect("sent");
        assert!(
            matches!(attempt.result(), Err(ReplicaError::Unheard { .. })),
            "the answer was not lost: {:?}",
            attempt.result()
        );
        // The crash: nothing after the send is persisted.
        id
    };
    close(lossy);
    let direct = device(1);
    let lifecycle = lifecycle_at(&url, &direct);
    let client = GraphClient::new(direct.clone());
    let states = until(&direct, "the receipt", || {
        let states = resume_all(&lifecycle, &client);
        states
            .iter()
            .all(|(_, s)| *s == State::Accepted)
            .then_some(states)
    });
    assert_eq!(states, vec![(request_id.clone(), State::Accepted)]);
    let entry = entry_of(&lifecycle, &request_id).expect("record");
    assert_eq!(entry.attempts, 1, "resolved by its receipt, never resent");
    report(
        "1121-lost-resolved",
        &json!({"request_id": request_id, "attempts": entry.attempts}),
    );
    close(direct);
}

/// S08-AC3: a conflicting remote edit before replay ends the queued change
/// `conflict`; a domain the person holds no graph role in refuses it, which
/// locks that domain (`disabled`, fenced), and nothing is accepted.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (112.1)"]
fn graph_live_1121_conflict_and_denial_are_never_accepted() {
    let url = env("KEEPER_HQ_LIVE_URL");
    let (one, two) = (device(1), device(2));
    let (client, other) = (GraphClient::new(one.clone()), GraphClient::new(two.clone()));
    let id = ulid();
    let created = client
        .change(vec![create_node(&id, "Raced", json!({}))], None)
        .expect("change");
    let (_, outcome) = applied(&client, &one, &created);
    let (_, seq) = accepted(&outcome);
    prefix_at(&two, seq);
    let node = other.get_node(&id, false).expect("read").expect("node");
    let revision = revision(&node.image, "revision");
    // Queued on device 1 against the revision it read; device 2 edits first.
    let lifecycle = lifecycle_at(&url, &one);
    let mine = client
        .mint(
            &client
                .change(vec![retitle(&id, &revision, "Mine")], None)
                .expect("change"),
        )
        .expect("minted");
    lifecycle
        .with_queue(|q| q.enqueue(&mine, now_ms()))
        .expect("queued");
    let theirs = other
        .change(vec![retitle(&id, &revision, "Theirs")], None)
        .expect("change");
    accepted(&applied(&other, &two, &theirs).1);
    let states = resume_all(&lifecycle, &client);
    assert_eq!(
        states,
        vec![(mine.request().request_id.clone(), State::Conflict)]
    );
    report("1121-conflict", &json!({"state": "conflict"}));
    close(one);
    close(two);

    let denied = open(1, &env("KEEPER_HQ_LIVE_FORBIDDEN_DATABASE"), &url);
    let lifecycle = lifecycle_at(&url, &denied);
    let client = GraphClient::new(denied.clone());
    let probe = client
        .mint(
            &client
                .change(vec![create_node(&ulid(), "Denied", json!({}))], None)
                .expect("change"),
        )
        .expect("minted");
    lifecycle
        .with_queue(|q| q.enqueue(&probe, now_ms()))
        .expect("queued");
    let states = resume_all(&lifecycle, &client);
    assert_eq!(
        states,
        vec![(probe.request().request_id.clone(), State::Disabled)]
    );
    assert!(lifecycle.is_locked(), "a refusal fences the domain");
    assert_eq!(lifecycle.fence(), Ok(Some(FenceState::Revoked)));
    report(
        "1121-denied",
        &json!({"state": "disabled", "fence": "revoked"}),
    );
    close(denied);
}

/// S08-AC3: an entry is never rebound. Restored under another client of
/// the same person it is `suspended` and nothing is sent; the same file
/// opened for another server refuses, and that server's own queue holds
/// nothing of it.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (112.1)"]
fn graph_live_1121_an_identity_change_suspends_and_servers_never_cross() {
    let url = env("KEEPER_HQ_LIVE_URL");
    let one = device(1);
    let client = GraphClient::new(one.clone());
    let prepared = client
        .mint(
            &client
                .change(vec![create_node(&ulid(), "Bound", json!({}))], None)
                .expect("change"),
        )
        .expect("minted");
    let id = prepared.request().request_id.clone();
    lifecycle_at(&url, &one)
        .with_queue(|q| q.enqueue(&prepared, now_ms()))
        .expect("queued");
    close(one);
    // Same person, another device: the stored identity is not this one's.
    let two = device(2);
    let switched = Source {
        client: env("KEEPER_HQ_LIVE_CLIENT_2"),
        ..two.clone()
    };
    let server = ServerUrl::parse(&url).expect("url").expect("server");
    let file = keeper_core::hq::databases::graph_queue(
        &state_dir(),
        &server,
        &switched.database,
        &switched.actor,
    );
    let lifecycle = Lifecycle::open(
        GraphQueue::open(&file, Binding::of(&server, &switched)).expect("same person's file"),
    )
    .expect("lifecycle");
    let states = resume_all(&lifecycle, &GraphClient::new(switched.clone()));
    assert_eq!(states, vec![(id.clone(), State::Suspended)]);
    // The second server: the first server's file is not its file.
    let second = ServerUrl::parse(&env("KEEPER_HQ_LIVE_SERVER_2_URL"))
        .expect("url")
        .expect("server");
    assert!(matches!(
        GraphQueue::open(&file, Binding::of(&second, &switched)),
        Err(keeper_core::graph::queue::QueueError::ForeignFile)
    ));
    let elsewhere = keeper_core::hq::databases::graph_queue(
        &state_dir(),
        &second,
        &switched.database,
        &switched.actor,
    );
    let queue = GraphQueue::open(&elsewhere, Binding::of(&second, &switched)).expect("its own");
    assert!(queue.entries().expect("listing").is_empty());
    report(
        "1121-suspended",
        &json!({"request_id": id, "state": "suspended",
                                    "second_server_entries": 0}),
    );
    close(two);
}

/// The rows the replica holds in `table`.
fn rows_in(source: &Source, table: &str) -> u64 {
    source
        .replica
        .query(&format!("SELECT COUNT(*) AS n FROM {table}"), &[])
        .expect("count")
        .first()
        .and_then(|row| row.get("n"))
        .and_then(Value::as_u64)
        .unwrap_or(0)
}

/// Device `index`'s replica of `database` in file `name`, opened without a
/// link: it reads the file as the last sync left it and sends nothing, so
/// no scope revocation can reach it while it is open.
fn unlinked_at(index: u8, database: &str, name: &str) -> Source {
    let actor = env("KEEPER_HQ_LIVE_ACTOR");
    let client = env(&format!("KEEPER_HQ_LIVE_CLIENT_{index}"));
    let replica = SyncularReplica::open(ReplicaSpec {
        database: database.to_owned(),
        path: state_dir().join(name),
        client_id: Some(client.clone()),
        link: None,
        subscriptions: subscriptions(database, &actor, &client),
        watch_presence: None,
    })
    .expect("opens the replica unlinked");
    Source {
        database: database.to_owned(),
        replica: Arc::new(replica),
        actor,
        client,
    }
}

/// S08-AC4 (T5 revocation), lock first and purge after, every table.
///
/// Phase `before` seeds a row in every one of the eleven subscribed
/// tables (the nine graph tables, the receipts and the proposals) and
/// asserts each holds one; it leaves one entry queued and one submitting.
/// The harness then revokes the person's graph role, counting the person's
/// conversation rows and attachments around it (the sentinels that must not
/// change).
///
/// Phase `after`, the barrier: the device's file is opened unlinked, so the
/// server's scope cleanup cannot reach it. The revocation is learned from
/// the server (`read_status` refused, on device 2), the domain is fenced,
/// and the fence is observed while every one of the eleven tables still
/// holds its rows: the lock came before the purge. Both open entries are
/// locked, the queue discloses nothing, and a restart while cleanup is
/// incomplete starts locked and resumes. Only then is the link opened:
/// the scope cleanup lands, every one of the eleven tables reads empty and
/// the fence records `purged`.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (112.1)"]
fn graph_live_1121_a_revocation_locks_first_and_purges_every_graph_table() {
    let url = env("KEEPER_HQ_LIVE_URL");
    let database = env("KEEPER_HQ_LIVE_DATABASE");
    let file = format!("{database}-1.sqlite");
    let tables = subscribed_tables();
    assert_eq!(tables.len(), 11, "nine graph tables, receipts, proposals");
    let counts = |source: &Source| -> Map<String, Value> {
        tables
            .iter()
            .map(|t| ((*t).to_owned(), json!(rows_in(source, t))))
            .collect()
    };
    let every_table_holds = |source: &Source| tables.iter().all(|t| rows_in(source, t) > 0);
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("runtime");
    match env("KEEPER_HQ_LIVE_PHASE").as_str() {
        "before" => {
            let source = device(1);
            let lifecycle = lifecycle_at(&url, &source);
            let client = GraphClient::new(source.clone());
            // kg_nodes, kg_edges, kg_tags, kg_node_tags, kg_contents and,
            // through the accepted event, kg_events, kg_event_ops, kg_head
            // and kg_receipts.
            let [a, b, edge, content] = std::array::from_fn(|_| ulid());
            let seed = client
                .change(
                    vec![
                        create_node(&a, "Held", json!({})),
                        create_node(&b, "Held too", json!({})),
                        ChangeOp::CreateEdge {
                            id: edge,
                            from_id: a.clone(),
                            to_id: b,
                            edge_type: "test:rel".to_owned(),
                            properties: Map::new(),
                            from_lifecycle_revision: None,
                            to_lifecycle_revision: None,
                        },
                        ChangeOp::AddTag {
                            node_id: a.clone(),
                            tag: "Held".to_owned(),
                            expected_revision: None,
                        },
                        ChangeOp::CreateContent {
                            id: content,
                            owner_node_id: a,
                            body: json!({"inline_text": "held"}),
                        },
                    ],
                    None,
                )
                .expect("change");
            let (_, seq) = accepted(&applied(&client, &source, &seed).1);
            prefix_at(&source, seq);
            // kg_proposals: the proposer's own replica holds its proposal.
            let reviewer_actor = env("KEEPER_HQ_LIVE_REVIEWER_ACTOR");
            let proposal = Proposal::new(
                ulid(),
                None,
                client
                    .change(vec![create_node(&ulid(), "Proposed", json!({}))], None)
                    .expect("change"),
                vec![reviewer_actor],
                None,
            )
            .expect("proposal");
            let (_, proposed) = commanded(&client, &source, client.propose(&proposal));
            assert!(
                matches!(proposed, GraphOutcome::Proposed { .. }),
                "{proposed:?}"
            );
            // kg_reviews: the reviewer acknowledges the seeded agent event
            // (a second acknowledgement is refused; the first one's row
            // stands either way).
            let review = reviewer();
            let checker = GraphClient::new(review.clone());
            let agent_event = env("KEEPER_HQ_LIVE_AGENT_EVENT");
            let prefix = prefix_with(&review, &agent_event);
            let target =
                Target::new(&agent_event, &digest_of(&prefix, &agent_event)).expect("target");
            let (_, acknowledged) = commanded(&checker, &review, checker.acknowledge(&target));
            assert!(
                matches!(acknowledged, GraphOutcome::Accepted { .. })
                    || rejected_reason(&acknowledged) == Some("invalid_change"),
                "{acknowledged:?}"
            );
            close(review);
            until(&source, "a row in every subscribed table", || {
                every_table_holds(&source).then_some(())
            });
            // One entry queued, one submitting: both open when the
            // revocation lands.
            let mint = |title: &str| {
                client
                    .mint(
                        &client
                            .change(vec![create_node(&ulid(), title, json!({}))], None)
                            .expect("change"),
                    )
                    .expect("minted")
            };
            let (queued, submitting) = (mint("Pending"), mint("Submitting"));
            lifecycle
                .with_queue(|q| {
                    q.enqueue(&queued, now_ms())?;
                    q.enqueue(&submitting, now_ms())?;
                    q.mark(
                        &submitting.request().request_id,
                        State::Queued,
                        State::Submitting,
                        Some("sent"),
                        now_ms(),
                    )
                })
                .expect("queued");
            report("1121-before-revocation", &json!({"rows": counts(&source)}));
            close(source);
        }
        "after" => {
            // The barrier: the scope cleanup cannot reach this file.
            let held = unlinked_at(1, &database, &file);
            assert!(
                every_table_holds(&held),
                "the cleanup reached the file before the barrier: {:?}",
                counts(&held)
            );
            let lifecycle = lifecycle_at(&url, &held);
            assert!(!lifecycle.is_locked());
            // The revocation, learned from the server.
            let probe = open_at(2, &database, &url, &format!("{database}-1121-probe.sqlite"));
            until(&probe, "the server's refusal", || {
                match keeper_core::graph::query::GraphRemote::read_status(&probe, &database) {
                    Err(keeper_core::graph::query::QueryError::Forbidden) => Some(()),
                    _ => None,
                }
            });
            close(probe);
            runtime
                .block_on(lifecycle.revoke(None, now_ms(), FenceCause::Revocation))
                .expect("fenced");
            // Locked, with every table's rows still on this device: the
            // fence came first.
            assert!(lifecycle.is_locked());
            assert_eq!(lifecycle.fence(), Ok(Some(FenceState::Revoked)));
            let still_held = counts(&held);
            assert!(
                every_table_holds(&held),
                "locked before the purge: {still_held:?}"
            );
            assert!(lifecycle.gate().is_err());
            assert!(
                lifecycle.with_queue(|q| q.entries()).is_err(),
                "no queue detail"
            );
            assert_eq!(
                lifecycle.counts().expect("counts").get(&State::Disabled),
                Some(&2),
                "the queued and the submitting entry are both locked"
            );
            // A restart while the cleanup is incomplete (the queue file
            // reopened) starts locked and resumes it.
            drop(lifecycle);
            let lifecycle = lifecycle_at(&url, &held);
            assert!(lifecycle.is_locked());
            assert_eq!(lifecycle.fence(), Ok(Some(FenceState::Revoked)));
            runtime
                .block_on(lifecycle.resume(None, now_ms()))
                .expect("resumed");
            assert!(every_table_holds(&held), "still before the purge");
            close(held);
            // Now the link opens and the server's scope cleanup lands.
            let source = device(1);
            let query = |sql: &str, params: &[Value]| Ok(source.replica.query(sql, params)?);
            let revocation =
                until(
                    &source,
                    "the server's scope revocation",
                    || match revocation_with(&tables, query).expect("subscriptions") {
                        Revocation::Purged => Some(Revocation::Purged),
                        _ => None,
                    },
                );
            assert!(lifecycle
                .observe_purge(revocation, now_ms())
                .expect("observed"));
            assert_eq!(lifecycle.fence(), Ok(Some(FenceState::Purged)));
            for table in &tables {
                assert_eq!(rows_in(&source, table), 0, "{table} purged");
            }
            assert!(lifecycle.is_locked(), "purged stays locked");
            report(
                "1121-after-revocation",
                &json!({"locked_with_rows": still_held, "rows": counts(&source),
                        "fence": "purged", "disabled": 2}),
            );
            close(source);
        }
        phase => panic!("unknown phase {phase}"),
    }
}

/// The person's queue file for `source`'s database on `url`, the one
/// [`lifecycle_at`] opens.
fn queue_path(url: &str, source: &Source) -> PathBuf {
    let server = ServerUrl::parse(url)
        .expect("a server address")
        .expect("not local");
    keeper_core::hq::databases::graph_queue(&state_dir(), &server, &source.database, &source.actor)
}

/// A crash between the queue's terminal commit and its payload cleanup.
/// [`GraphQueue::resolve`] commits the terminal state in one transaction
/// and then deletes the payload in a separate statement; this trigger
/// fails exactly that second statement, so the real terminal commit lands
/// and the real cleanup does not, as when the process dies between them.
const STOP_BEFORE_CLEANUP: &str = "CREATE TRIGGER kg1_live_stop_before_cleanup \
     BEFORE UPDATE OF request_jcs ON graph_intents WHEN NEW.request_jcs IS NULL \
     BEGIN SELECT RAISE(ABORT, 'stopped before payload cleanup'); END;";

/// An entry's stored state, payload and event, read raw from the file.
fn raw_intent(
    path: &std::path::Path,
    request_id: &str,
) -> (String, Option<String>, Option<String>) {
    rusqlite::Connection::open(path)
        .expect("raw")
        .query_row(
            "SELECT state, request_jcs, event_id FROM graph_intents WHERE request_id = ?1",
            [request_id],
            |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)),
        )
        .expect("the entry")
}

/// S08-AC2 (T5 crash windows, F1(f)): an accepted entry whose terminal
/// state is committed and whose payload cleanup never ran.
///
/// Phase `commit`: a queued change is sent once and accepted (receipt and
/// event); the lifecycle settles it, and its terminal commit lands while
/// the cleanup after it is stopped ([`STOP_BEFORE_CLEANUP`]). The process
/// ends with the row `accepted` and its payload still on disk.
///
/// Phase `restart` (a new process; the stop is gone): opening the queue
/// deletes the payload before anything else. The entry stays `accepted`
/// with the original event and one attempt, a resume neither restores nor
/// sends it, the original request resolves to the original receipt, and
/// both replicas hold exactly one event of it.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (112.1)"]
#[allow(clippy::too_many_lines)]
fn graph_live_1121_a_crash_before_payload_cleanup_settles_once() {
    let saved = state_dir().join("cleanup-1121.json");
    let url = env("KEEPER_HQ_LIVE_URL");
    let source = device(1);
    let file = queue_path(&url, &source);
    let client = GraphClient::new(source.clone());
    match env("KEEPER_HQ_LIVE_PHASE").as_str() {
        "commit" => {
            let lifecycle = lifecycle_at(&url, &source);
            let prepared = client
                .mint(
                    &client
                        .change(vec![create_node(&ulid(), "Settled once", json!({}))], None)
                        .expect("change"),
                )
                .expect("minted");
            let id = prepared.request().request_id.clone();
            lifecycle
                .with_queue(|q| {
                    q.enqueue(&prepared, now_ms())?;
                    q.mark(
                        &id,
                        State::Queued,
                        State::Submitting,
                        Some("sent"),
                        now_ms(),
                    )
                })
                .expect("submitting");
            let request = prepared.request();
            let kept = json!({"request_id": request.request_id, "payload": request.payload});
            std::fs::write(&saved, kept.to_string()).expect("saved");
            rusqlite::Connection::open(&file)
                .expect("raw")
                .execute_batch(STOP_BEFORE_CLEANUP)
                .expect("the stop");
            let attempt = client.send(&prepared).expect("sent");
            assert!(attempt.result().is_ok(), "{:?}", attempt.result());
            let outcome = until(&source, "the receipt", || {
                match client.outcome(&prepared, Some(&attempt)) {
                    Ok(outcome @ GraphOutcome::Accepted { .. }) => Some(outcome),
                    Ok(GraphOutcome::Pending { .. } | GraphOutcome::PendingVisibility { .. }) => {
                        None
                    }
                    other => panic!("not accepted: {other:?}"),
                }
            });
            let (event_id, seq) = accepted(&outcome);
            // The terminal commit lands; the cleanup after it does not.
            let stopped = lifecycle.settle(&id, State::Submitting, &outcome, now_ms());
            assert!(
                matches!(
                    &stopped,
                    Err(LifecycleError::Queue(QueueError::Storage(message)))
                        if message.contains("stopped before payload cleanup")
                ),
                "{stopped:?}"
            );
            let (state, payload, event) = raw_intent(&file, &id);
            assert_eq!(
                (state.as_str(), event.as_deref()),
                (State::Accepted.as_str(), Some(event_id.as_str()))
            );
            assert!(
                payload.is_some(),
                "the payload outlives the terminal commit"
            );
            report(
                "1121-stopped-before-cleanup",
                &json!({"request_id": id, "event_id": event_id, "graph_seq": seq.to_string(),
                        "state": state, "payload_kept": true}),
            );
            // The crash: this process ends here.
        }
        "restart" => {
            let request = saved_request(&saved);
            let id = request.request_id.clone();
            rusqlite::Connection::open(&file)
                .expect("raw")
                .execute_batch("DROP TRIGGER kg1_live_stop_before_cleanup;")
                .expect("the stop removed");
            let (state, payload, event) = raw_intent(&file, &id);
            assert_eq!(state, State::Accepted.as_str());
            assert!(payload.is_some(), "kept until the queue opens");
            let event_id = event.expect("the receipt's event");
            let lifecycle = lifecycle_at(&url, &source);
            assert_eq!(
                raw_intent(&file, &id),
                (
                    State::Accepted.as_str().to_owned(),
                    None,
                    Some(event_id.clone())
                ),
                "the payload deleted on open, the outcome kept"
            );
            let entry = entry_of(&lifecycle, &id).expect("record kept");
            assert_eq!((entry.state, entry.attempts), (State::Accepted, 1));
            let states = resume_all(&lifecycle, &client);
            assert!(
                states.iter().all(|(request_id, _)| *request_id != id),
                "restored again: {states:?}"
            );
            assert_eq!(entry_of(&lifecycle, &id).expect("record").attempts, 1);
            let prepared = client.prepared(DESCRIPTOR, request).expect("same request");
            let recovered = settled(&client, &source, &prepared);
            let (event, seq) = accepted(&recovered);
            assert_eq!(event, event_id, "the original receipt");
            assert_eq!(events_of(&prefix_at(&source, seq), &id), 1);
            let two = device(2);
            assert_eq!(events_of(&prefix_with(&two, &event_id), &id), 1);
            close(two);
            report(
                "1121-cleaned-on-restart",
                &json!({"request_id": id, "event_id": event_id, "state": "accepted",
                        "attempts": entry.attempts, "payload_kept": false, "events": 1,
                        "outcome": outcome_json(&recovered)}),
            );
        }
        phase => panic!("unknown phase {phase}"),
    }
    close(source);
}

/// S08-AC2 (T5 cached-outcome loss, F1(f)): a queued change is accepted on
/// its first send, and every cached answer is then lost: that send's
/// answer is not kept, the same id resent through a relay that drops the
/// server's cached replay, and no replica holds a cached receipt (a fresh
/// file). `graph.outcome` under the same id finds the receipt, the queue
/// resolves from it with no further send and no new request id, and one
/// event is accepted.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (112.1)"]
fn graph_live_1121_a_result_cache_miss_resolves_from_the_receipt() {
    let url = env("KEEPER_HQ_LIVE_URL");
    let database = env("KEEPER_HQ_LIVE_DATABASE");
    let (direct, lossy) = (
        device(1),
        open(1, &database, &env("KEEPER_HQ_LIVE_DROP_URL")),
    );
    let request = {
        let lifecycle = lifecycle_at(&url, &direct);
        let (client, relay) = (
            GraphClient::new(direct.clone()),
            GraphClient::new(lossy.clone()),
        );
        let prepared = client
            .mint(
                &client
                    .change(vec![create_node(&ulid(), "Cache missed", json!({}))], None)
                    .expect("change"),
            )
            .expect("minted");
        let id = prepared.request().request_id.clone();
        lifecycle
            .with_queue(|q| {
                q.enqueue(&prepared, now_ms())?;
                q.mark(
                    &id,
                    State::Queued,
                    State::Submitting,
                    Some("sent"),
                    now_ms(),
                )
            })
            .expect("submitting");
        let attempt = client.send(&prepared).expect("sent");
        assert_eq!(attempt.result().as_ref().map(|r| r.cached), Ok(false));
        // That answer is not kept. The same id again: the server answers
        // from its cached result, and the relay drops that answer.
        lifecycle
            .with_queue(|q| q.retry(&id, "resent_same_id", now_ms()))
            .expect("resent");
        let replay = relay.send(&prepared).expect("sent again");
        assert!(
            matches!(replay.result(), Err(ReplicaError::Unheard { .. })),
            "the cached answer was not lost: {:?}",
            replay.result()
        );
        prepared.request().clone()
    };
    close(lossy);
    close(direct);
    // Nor does any replica hold the receipt: a file that never synced.
    let fresh = open_at(1, &database, &url, &format!("cache-miss-{}.sqlite", ulid()));
    let client = GraphClient::new(fresh.clone());
    let prepared = client
        .prepared(DESCRIPTOR, request.clone())
        .expect("same request");
    let receipt = GraphRequests::new(fresh.clone())
        .remote_receipt(prepared.key(), None)
        .expect("graph.outcome")
        .expect("the server's receipt");
    let ReceiptOutcome::Accepted {
        event_id,
        graph_seq,
    } = receipt.outcome().clone()
    else {
        panic!("not accepted: {receipt:?}");
    };
    let lifecycle = lifecycle_at(&url, &fresh);
    let states = resume_all(&lifecycle, &client);
    assert!(
        states.contains(&(request.request_id.clone(), State::Accepted)),
        "{states:?}"
    );
    let entry = entry_of(&lifecycle, &request.request_id).expect("record");
    assert_eq!(
        entry.attempts, 2,
        "the first send and the same-id resend, never a third"
    );
    let (event, seq) = accepted(&settled(&client, &fresh, &prepared));
    assert_eq!(
        (event.as_str(), seq.to_string()),
        (event_id.as_str(), graph_seq.clone())
    );
    assert_eq!(events_of(&prefix_at(&fresh, seq), &request.request_id), 1);
    let two = device(2);
    assert_eq!(
        events_of(&prefix_with(&two, &event_id), &request.request_id),
        1
    );
    close(two);
    report(
        "1121-cache-miss-resolved",
        &json!({"request_id": request.request_id, "event_id": event_id, "graph_seq": graph_seq,
                "attempts": entry.attempts, "state": "accepted", "events": 1}),
    );
    close(fresh);
}

// ---------------------------------------------------------------------------
// Story 111.2 G1/T5: process death after a reviewer command is sent. Each
// case runs twice, as two processes, under `KEEPER_HQ_LIVE_PHASE`:
// - `send`: a person's `mint_command`, the request persisted to the state
//   dir, one send, then the marker line `KG1_LIVE <case>-sent {...}` and a
//   process abort (SIGABRT) before anything records an outcome;
// - `restart`: a new process restores the persisted request through
//   `GraphClient::prepared` and resolves it under the same id: one terminal
//   outcome, exactly one event for that request, and the same id sent with a
//   changed payload is `idempotency_mismatch` with no further event.
// Extra input: `KEEPER_HQ_LIVE_CRASH_ACK_EVENT`, an agent-origin accepted
// event nobody has reviewed (the acknowledge case's target).
// ---------------------------------------------------------------------------

/// Phase `send`: mint `descriptor` with `payload`, persist the request at
/// `saved`, send it once, print the marker and abort.
fn sent_then_aborted(
    case: &str,
    client: &GraphClient,
    descriptor: &str,
    payload: Value,
    saved: &std::path::Path,
) -> ! {
    let prepared = client.mint_command(descriptor, payload).expect("minted");
    let request = prepared.request();
    std::fs::write(
        saved,
        json!({"descriptor": descriptor, "request_id": request.request_id,
               "payload": request.payload})
        .to_string(),
    )
    .expect("persisted before the send");
    let attempt = client.send(&prepared).expect("sent");
    report(
        &format!("{case}-sent"),
        &json!({"request_id": request.request_id, "descriptor": descriptor,
                "answered": attempt.result().is_ok()}),
    );
    std::io::Write::flush(&mut std::io::stdout()).expect("marker flushed");
    // The crash: nothing after the send is recorded.
    std::process::abort();
}

/// Phase `restart`: restore the request `saved` holds, resolve it under its
/// own id (terminal, accepted, one event), then send the same id with
/// `changed`'s payload: `idempotency_mismatch`, still one event. The
/// accepted event's id, for the case's own checks.
fn restored_once(
    case: &str,
    client: &GraphClient,
    source: &Source,
    saved: &std::path::Path,
    changed: impl FnOnce(&Value) -> Value,
) -> String {
    let kept: Value =
        serde_json::from_str(&std::fs::read_to_string(saved).expect("persisted")).expect("json");
    let descriptor = kept["descriptor"].as_str().expect("descriptor").to_owned();
    let request = Request {
        request_id: kept["request_id"].as_str().expect("id").to_owned(),
        context_id: None,
        payload: kept["payload"].clone(),
    };
    let prepared = client
        .prepared(&descriptor, request.clone())
        .expect("the same request");
    let outcome = settled(client, source, &prepared);
    let (event_id, seq) = accepted(&outcome);
    let prefix = prefix_at(source, seq);
    assert_eq!(
        events_of(&prefix, &request.request_id),
        1,
        "{case}: one event"
    );

    let other = client
        .prepared(
            &descriptor,
            Request {
                payload: changed(&request.payload),
                ..request.clone()
            },
        )
        .expect("a changed payload under the same id");
    assert_ne!(other.key(), prepared.key());
    let attempt = client.send(&other).expect("sent");
    let mismatch = until(source, "the mismatch", || {
        match client.outcome(&other, Some(&attempt)) {
            Ok(GraphOutcome::Pending { .. } | GraphOutcome::PendingVisibility { .. }) => None,
            Ok(outcome) => Some(outcome),
            Err(error) => panic!("{case}: {error}"),
        }
    });
    assert_eq!(
        mismatch.code(other.key()),
        Some("idempotency_mismatch"),
        "{case}: {mismatch:?}"
    );
    let (_, barrier_event, _) = barrier(&[source]);
    let after = prefix_with(source, &barrier_event);
    assert_eq!(
        events_of(&after, &request.request_id),
        1,
        "{case}: the changed payload added no event"
    );
    report(
        &format!("{case}-restarted"),
        &json!({"request_id": request.request_id, "descriptor": descriptor,
                "outcome": outcome_json(&outcome), "events": 1,
                "changed_payload": format!("{:?}", mismatch.code(other.key())),
                "barrier_event": barrier_event}),
    );
    event_id
}

/// 111.2 G1/T5 (S:45): an approval (`graph.decide`) whose process dies
/// after the send settles once, under its id, after a restart.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (111.2 server)"]
fn graph_live_112_decide_survives_process_death() {
    let case = "112-crash-decide";
    let saved = state_dir().join("crash-112-decide.json");
    let review = reviewer();
    let checker = GraphClient::new(review.clone());
    match env("KEEPER_HQ_LIVE_PHASE").as_str() {
        "send" => {
            let one = device(1);
            let first = GraphClient::new(one.clone());
            let graph = env("KEEPER_HQ_LIVE_DATABASE");
            let (p, n) = (ulid(), ulid());
            let proposal = Proposal::new(
                p.clone(),
                None,
                first
                    .change(vec![create_node(&n, "Crash-approved", json!({}))], None)
                    .expect("change"),
                vec![env("KEEPER_HQ_LIVE_REVIEWER_ACTOR")],
                None,
            )
            .expect("proposal");
            let (_, proposed) = commanded(&first, &one, first.propose(&proposal));
            assert!(
                matches!(proposed, GraphOutcome::Proposed { .. }),
                "{proposed:?}"
            );
            let digest = proposal::digest(&graph, &proposal).expect("digest");
            let decision = Decision {
                proposal_id: p,
                proposal_revision: "1".to_owned(),
                proposal_digest: digest,
                verdict: Verdict::Approve,
            };
            close(one);
            sent_then_aborted(case, &checker, DECIDE, decision.payload(), &saved);
        }
        "restart" => {
            restored_once(case, &checker, &review, &saved, |payload| {
                let mut payload = payload.clone();
                payload["decision"] = json!("reject");
                payload
            });
        }
        phase => panic!("unknown phase {phase}"),
    }
    close(review);
}

/// 111.2 G1/T5 (S:45): an acknowledgement whose process dies after the send
/// settles once, under its id, after a restart: one review of the target.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (111.2 server)"]
fn graph_live_112_acknowledge_survives_process_death() {
    let case = "112-crash-acknowledge";
    let saved = state_dir().join("crash-112-acknowledge.json");
    let review = reviewer();
    let checker = GraphClient::new(review.clone());
    let target_event = env("KEEPER_HQ_LIVE_CRASH_ACK_EVENT");
    match env("KEEPER_HQ_LIVE_PHASE").as_str() {
        "send" => {
            let prefix = prefix_with(&review, &target_event);
            let target =
                Target::new(&target_event, &digest_of(&prefix, &target_event)).expect("target");
            sent_then_aborted(
                case,
                &checker,
                ACKNOWLEDGE,
                review::acknowledge_payload(&target),
                &saved,
            );
        }
        "restart" => {
            let ack = restored_once(case, &checker, &review, &saved, |_| {
                let other = Target::new(&target_event, &"0".repeat(64)).expect("target");
                review::acknowledge_payload(&other)
            });
            let reviews = review
                .replica
                .query(
                    "SELECT COUNT(*) AS n FROM kg_reviews WHERE target_event_id = ?",
                    &[Value::from(target_event.as_str())],
                )
                .expect("count")
                .first()
                .and_then(|row| row.get("n"))
                .and_then(Value::as_u64)
                .unwrap_or(0);
            assert_eq!(reviews, 1, "exactly one review of the target");
            report(
                "112-crash-acknowledge-review",
                &json!({"target": target_event, "ack_event": ack, "reviews": reviews}),
            );
        }
        phase => panic!("unknown phase {phase}"),
    }
    close(review);
}

/// 111.2 G1/T5 (S:45): a reversal whose process dies after the send
/// settles once, under its id, after a restart.
#[test]
#[ignore = "driven by keeper-hq's native graph harness (111.2 server)"]
fn graph_live_112_reverse_survives_process_death() {
    let case = "112-crash-reverse";
    let saved = state_dir().join("crash-112-reverse.json");
    let one = device(1);
    let first = GraphClient::new(one.clone());
    match env("KEEPER_HQ_LIVE_PHASE").as_str() {
        "send" => {
            let (created, _) = landed(
                &first,
                &one,
                vec![create_node(&ulid(), "Crash-reversed", json!({}))],
            );
            let preview = first.preview_reversal(&created).expect("preview");
            assert_eq!(preview.outcome, PreviewOutcome::Ready, "{preview:?}");
            let reverse =
                Reverse::of(preview.plan.as_ref().expect("plan"), None, None).expect("reverse");
            sent_then_aborted(case, &first, REVERSE, reverse.payload(), &saved);
        }
        "restart" => {
            restored_once(case, &first, &one, &saved, |payload| {
                let mut payload = payload.clone();
                payload["event_time_ms"] = json!(1);
                payload
            });
        }
        phase => panic!("unknown phase {phase}"),
    }
    close(one);
}
