//! The intent queue on disk, through restarts and every crash window it
//! owns, with a fake replica port: nothing here is sent anywhere.

use std::path::Path;

use serde_json::Map;

use super::*;
use crate::graph::protocol::change::ChangeOp;
use crate::hq::port::fake::FakeOpener;
use crate::hq::port::{Opener, ReplicaSpec};
use crate::hq::sessions::Source;

const SERVER: &str = "https://hq.example";
const DB: &str = "work";

fn source(dir: &Path, actor: &str, client: &str) -> Source {
    let replica = FakeOpener::default()
        .open(ReplicaSpec {
            database: DB.to_owned(),
            path: dir.join(format!("{actor}.sqlite")),
            client_id: Some(client.to_owned()),
            link: None,
            subscriptions: Vec::new(),
            watch_presence: None,
        })
        .expect("opens");
    Source {
        database: DB.to_owned(),
        replica,
        actor: actor.to_owned(),
        client: client.to_owned(),
    }
}

fn binding(server: &str, actor: &str, client: &str) -> Binding {
    Binding {
        server: server.to_owned(),
        domain: DB.to_owned(),
        actor: actor.to_owned(),
        client: client.to_owned(),
    }
}

fn path(dir: &Path) -> std::path::PathBuf {
    dir.join("ana.graph-queue")
}

fn minted(client: &GraphClient) -> Prepared {
    let change = client
        .change(
            vec![ChangeOp::CreateNode {
                id: "01JF0000000000000000000001".to_owned(),
                kind: "Concept".into(),
                title: "N".into(),
                description: None,
                properties: Map::new(),
            }],
            None,
        )
        .expect("change");
    client.mint(&change).expect("minted")
}

/// One queued change, in a fresh queue under `dir`.
fn queued(dir: &Path) -> (GraphClient, Prepared) {
    let client = GraphClient::new(source(dir, "ana", "dev1"));
    let prepared = minted(&client);
    let mut queue = GraphQueue::open(&path(dir), binding(SERVER, "ana", "dev1")).expect("opens");
    queue.enqueue(&prepared, 10).expect("queued");
    (client, prepared)
}

fn reopen(dir: &Path) -> GraphQueue {
    GraphQueue::open(&path(dir), binding(SERVER, "ana", "dev1")).expect("reopens")
}

fn payload(dir: &Path, request_id: &str) -> Option<String> {
    rusqlite::Connection::open(path(dir))
        .expect("raw")
        .query_row(
            "SELECT request_jcs FROM graph_intents WHERE request_id = ?1",
            [request_id],
            |row| row.get(0),
        )
        .expect("row")
}

fn raw(dir: &Path, sql: &str) {
    rusqlite::Connection::open(path(dir))
        .expect("raw")
        .execute_batch(sql)
        .expect("raw sql");
}

/// S08-AC1: a queued change survives a restart with the same identity,
/// payload and digest, still queued, and is rebuilt from its stored bytes.
#[test]
fn a_queued_change_survives_a_restart_unchanged() {
    let dir = tempfile::tempdir().expect("dir");
    let (_, prepared) = queued(dir.path());
    // Restart: a new client over the same identity, the queue reopened.
    let client = GraphClient::new(source(dir.path(), "ana", "dev1"));
    let mut queue = reopen(dir.path());
    let restored = queue.restore(&client, 20).expect("restores");
    assert_eq!(
        restored,
        vec![Restored::Ready {
            prepared: prepared.clone(),
            state: State::Queued
        }]
    );
    let entries = queue.entries().expect("listing");
    assert_eq!(entries.len(), 1);
    assert_eq!(entries[0].request_id, prepared.request().request_id);
    assert_eq!(entries[0].state, State::Queued);
    assert_eq!(entries[0].enqueued_at_ms, 10);
}

/// A stored payload whose bytes no longer carry the stored digest is
/// corruption: refused, the row kept as it was.
#[test]
fn a_payload_that_does_not_match_its_digest_is_refused_and_kept() {
    let dir = tempfile::tempdir().expect("dir");
    let (client, prepared) = queued(dir.path());
    let id = prepared.request().request_id.clone();
    let tampered = payload(dir.path(), &id)
        .expect("payload")
        .replace("\"N\"", "\"M\"");
    rusqlite::Connection::open(path(dir.path()))
        .expect("raw")
        .execute(
            "UPDATE graph_intents SET request_jcs = ?1 WHERE request_id = ?2",
            [&tampered, &id],
        )
        .expect("tamper");
    let mut queue = reopen(dir.path());
    assert_eq!(
        queue.restore(&client, 20).expect("restores"),
        vec![Restored::Corrupt {
            request_id: id.clone()
        }]
    );
    assert_eq!(queue.entries().expect("listing")[0].state, State::Queued);
    assert_eq!(payload(dir.path(), &id), Some(tampered));
}

/// S08-AC3: an entry restored under another client is suspended — never
/// sent and never rebound, even back under its own client — and a file of
/// another server or actor refuses to open at all.
#[test]
fn another_identity_suspends_and_never_rebinds() {
    let dir = tempfile::tempdir().expect("dir");
    let (_, prepared) = queued(dir.path());
    let id = prepared.request().request_id.clone();
    let other = GraphClient::new(source(dir.path(), "ana", "dev2"));
    let mut queue =
        GraphQueue::open(&path(dir.path()), binding(SERVER, "ana", "dev2")).expect("reopens");
    assert_eq!(
        queue.restore(&other, 20).expect("restores"),
        vec![Restored::Suspended {
            request_id: id.clone()
        }]
    );
    assert_eq!(queue.entries().expect("listing")[0].state, State::Suspended);
    let original = GraphClient::new(source(dir.path(), "ana", "dev1"));
    let mut queue = reopen(dir.path());
    assert_eq!(queue.restore(&original, 30).expect("restores"), Vec::new());
    assert_eq!(
        queue.mark(&id, State::Suspended, State::Submitting, None, 31),
        Err(QueueError::Transition {
            from: State::Suspended,
            to: State::Submitting
        })
    );
    // The same domain on another server, or another actor: another file.
    assert!(matches!(
        GraphQueue::open(
            &path(dir.path()),
            binding("https://other.example", "ana", "dev1")
        ),
        Err(QueueError::ForeignFile)
    ));
    assert!(matches!(
        GraphQueue::open(&path(dir.path()), binding(SERVER, "bo", "dev1")),
        Err(QueueError::ForeignFile)
    ));
}

/// A change minted for another client is not enqueued here.
#[test]
fn a_foreign_request_is_not_enqueued() {
    let dir = tempfile::tempdir().expect("dir");
    let foreign = minted(&GraphClient::new(source(dir.path(), "ana", "dev2")));
    let mut queue =
        GraphQueue::open(&path(dir.path()), binding(SERVER, "ana", "dev1")).expect("opens");
    assert_eq!(queue.enqueue(&foreign, 1), Err(QueueError::ForeignRequest));
    assert_eq!(queue.entries().expect("listing"), Vec::new());
}

/// A failed enqueue commit is never `queued`: nothing is persisted.
#[test]
fn a_failed_enqueue_is_storage_unavailable_and_leaves_nothing() {
    let dir = tempfile::tempdir().expect("dir");
    let client = GraphClient::new(source(dir.path(), "ana", "dev1"));
    let mut queue =
        GraphQueue::open(&path(dir.path()), binding(SERVER, "ana", "dev1")).expect("opens");
    raw(
        dir.path(),
        "CREATE TRIGGER refuse BEFORE INSERT ON graph_intents \
         BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
    );
    let error = queue.enqueue(&minted(&client), 1).expect_err("refused");
    assert_eq!(error.code(), "storage_unavailable");
    assert_eq!(queue.entries().expect("listing"), Vec::new());
}

/// Terminal outcome first, payload cleanup second: a crash between the two
/// leaves a terminal row with its payload, and the next open deletes it
/// before anything else; the receipt summary and binding stay.
#[test]
fn a_crash_after_the_terminal_persist_cleans_the_payload_on_the_next_open() {
    let dir = tempfile::tempdir().expect("dir");
    let (_, prepared) = queued(dir.path());
    let id = prepared.request().request_id.clone();
    {
        let mut queue = reopen(dir.path());
        queue
            .mark(&id, State::Queued, State::Submitting, None, 11)
            .expect("submitting");
        queue
            .persist_terminal(
                &id,
                State::Submitting,
                State::Accepted,
                &Resolution {
                    event_id: Some("01JF0000000000000000000009".to_owned()),
                    graph_seq: Some("7".to_owned()),
                    code: None,
                },
                12,
            )
            .expect("persisted");
        // The crash: the cleanup step never ran.
    }
    assert!(payload(dir.path(), &id).is_some());
    let queue = reopen(dir.path());
    assert_eq!(payload(dir.path(), &id), None);
    let entry = &queue.entries().expect("listing")[0];
    assert_eq!((entry.state, entry.attempts), (State::Accepted, 1));
    let (event, digest): (String, String) = rusqlite::Connection::open(path(dir.path()))
        .expect("raw")
        .query_row(
            "SELECT event_id, request_digest FROM graph_intents WHERE request_id = ?1",
            [&id],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .expect("row");
    assert_eq!(event, "01JF0000000000000000000009");
    assert_eq!(digest, prepared.key().request_digest());
}

/// A failed terminal persist leaves the entry where it was, payload and
/// all, so it is resolved again after the restart under the same id.
#[test]
fn a_failed_terminal_persist_leaves_replayable_work() {
    let dir = tempfile::tempdir().expect("dir");
    let (client, prepared) = queued(dir.path());
    let id = prepared.request().request_id.clone();
    let mut queue = reopen(dir.path());
    queue
        .mark(&id, State::Queued, State::Submitting, None, 11)
        .expect("submitting");
    raw(
        dir.path(),
        "CREATE TRIGGER refuse BEFORE UPDATE ON graph_intents WHEN NEW.state = 'accepted' \
         BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
    );
    let error = queue
        .resolve(
            &id,
            State::Submitting,
            State::Accepted,
            &Resolution::default(),
            12,
        )
        .expect_err("refused");
    assert_eq!(error.code(), "storage_unavailable");
    drop(queue);
    let mut queue = reopen(dir.path());
    assert_eq!(
        queue.restore(&client, 20).expect("restores"),
        vec![Restored::Ready {
            prepared,
            state: State::Submitting
        }]
    );
}

/// Only the table's transitions happen, and only from the state the entry
/// is in: no outcome without a send, no withdrawal of a request that may
/// have been sent, no acknowledgement before an outcome.
#[test]
fn only_allowed_transitions_from_the_current_state_happen() {
    let dir = tempfile::tempdir().expect("dir");
    let (_, prepared) = queued(dir.path());
    let id = prepared.request().request_id.clone();
    let mut queue = reopen(dir.path());
    let summary = Resolution::default();
    assert!(matches!(
        queue.resolve(&id, State::Queued, State::Accepted, &summary, 1),
        Err(QueueError::Transition { .. })
    ));
    assert_eq!(queue.acknowledge(&id), Err(QueueError::Open(id.clone())));
    assert!(matches!(
        queue.mark(&id, State::Unresolved, State::Submitting, None, 1),
        Err(QueueError::Moved {
            actual: State::Queued,
            ..
        })
    ));
    queue
        .mark(&id, State::Queued, State::Submitting, None, 2)
        .expect("submitting");
    queue.retry(&id, "transport", 3).expect("retried");
    assert!(matches!(
        queue.discard(&id, 4),
        Err(QueueError::Transition { .. })
    ));
    queue
        .mark(
            &id,
            State::Submitting,
            State::Unresolved,
            Some("outcome_unresolved"),
            5,
        )
        .expect("unresolved");
    queue
        .resolve(&id, State::Unresolved, State::Conflict, &summary, 6)
        .expect("conflict");
    let entry = &queue.entries().expect("listing")[0];
    assert_eq!((entry.state, entry.attempts), (State::Conflict, 2));
    queue.acknowledge(&id).expect("acknowledged");
    assert_eq!(queue.entries().expect("listing"), Vec::new());
}

/// The fence locks every open entry (identity and payload kept), and while
/// it stands nothing is enqueued, restored or listed. A locked entry is
/// never sent again, even after the fence clears; it may be discarded.
#[test]
fn a_fence_locks_open_entries_and_disclosure() {
    let dir = tempfile::tempdir().expect("dir");
    let (client, prepared) = queued(dir.path());
    let id = prepared.request().request_id.clone();
    let mut queue = reopen(dir.path());
    queue.persist_fence(5).expect("fenced");
    assert_eq!(queue.disable_open(5).expect("disabled"), 1);
    assert_eq!(queue.fence().expect("fence"), Some(FenceState::Revoked));
    assert_eq!(queue.entries(), Err(QueueError::Fenced));
    assert_eq!(queue.restore(&client, 6), Err(QueueError::Fenced));
    assert_eq!(queue.enqueue(&minted(&client), 6), Err(QueueError::Fenced));
    assert_eq!(
        queue.counts().expect("counts"),
        BTreeMap::from([(State::Disabled, 1)])
    );
    assert!(payload(dir.path(), &id).is_some());
    queue.mark_purged(7).expect("purged");
    assert_eq!(queue.fence().expect("fence"), Some(FenceState::Purged));
    queue.clear_fence().expect("cleared");
    assert!(matches!(
        queue.mark(&id, State::Disabled, State::Submitting, None, 8),
        Err(QueueError::Transition { .. })
    ));
    queue.discard(&id, 9).expect("withdrawn");
    assert_eq!(payload(dir.path(), &id), None);
}

/// The last read authority is one row, replaced by each successful answer.
#[test]
fn the_read_authority_row_is_replaced_whole() {
    let dir = tempfile::tempdir().expect("dir");
    let queue = reopen(dir.path());
    assert_eq!(queue.authority().expect("row"), None);
    let authority = |epoch: &str| Authority {
        epoch: epoch.to_owned(),
        digest: "a".repeat(64),
    };
    queue
        .record_authority(&authority("1"), 100)
        .expect("recorded");
    queue
        .record_authority(&authority("2"), 200)
        .expect("recorded");
    assert_eq!(
        queue.authority().expect("row"),
        Some(AuthorityRow {
            authority: authority("2"),
            observed_at_ms: 200
        })
    );
}

/// `forget`'s log: counts by state across a server's queue files.
#[test]
fn counts_cover_every_queue_file_of_a_server() {
    let dir = tempfile::tempdir().expect("dir");
    let server = crate::hq::server::ServerUrl::parse(SERVER)
        .expect("url")
        .expect("server");
    for actor in ["ana", "bo"] {
        let file = crate::hq::databases::graph_queue(dir.path(), &server, DB, actor);
        let graph = GraphClient::new(source(dir.path(), actor, "dev1"));
        let mut queue = GraphQueue::open(&file, binding(SERVER, actor, "dev1")).expect("opens");
        queue.enqueue(&minted(&graph), 1).expect("queued");
    }
    let (counts, unreadable) = counts_under(&crate::hq::databases::server_dir(dir.path(), &server));
    assert_eq!(counts, BTreeMap::from([(State::Queued, 2)]));
    assert_eq!(unreadable, 0);
}

const APPROVAL: &str = "01JF00000000000000000000A1";
const CONTEXT: &str = "01JF00000000000000000000C1";

/// An agent instance's client over `source`'s port, under its context.
fn agent_client(dir: &Path) -> GraphClient {
    let source = source(dir, "agent-1", "inst-1");
    let writer = crate::graph::write::Writer::of(&source);
    GraphClient::over(
        std::sync::Arc::new(source),
        &writer,
        crate::graph::write::Caller::Agent {
            context_id: CONTEXT.to_owned(),
        },
    )
}

fn agent_minted(client: &GraphClient, approval: &str) -> Prepared {
    let change = crate::graph::protocol::change::Change::new(
        vec![ChangeOp::CreateNode {
            id: "01JF0000000000000000000002".to_owned(),
            kind: "Concept".into(),
            title: "A".into(),
            description: None,
            properties: Map::new(),
        }],
        Some(approval.to_owned()),
        None,
        crate::notes::tags::normalise,
    )
    .expect("change");
    client.mint(&change).expect("minted")
}

fn agent_queue(dir: &Path) -> GraphQueue {
    GraphQueue::open(&path(dir), binding(SERVER, "agent-1", "inst-1")).expect("opens")
}

fn state_of(dir: &Path, request_id: &str) -> String {
    rusqlite::Connection::open(path(dir))
        .expect("raw")
        .query_row(
            "SELECT state FROM graph_intents WHERE request_id = ?1",
            [request_id],
            |row| row.get(0),
        )
        .expect("row")
}

/// CC-14: an agent request is only ever persisted bound to the approval it
/// names, with its context; anything else refuses and persists nothing.
#[test]
fn an_agent_request_is_persisted_only_bound_to_its_approval() {
    let dir = tempfile::tempdir().expect("dir");
    let client = agent_client(dir.path());
    let mut queue = agent_queue(dir.path());
    let prepared = agent_minted(&client, APPROVAL);
    assert_eq!(
        queue.enqueue(&prepared, 1),
        Err(QueueError::Unbound),
        "a plain enqueue never takes an agent's request"
    );
    assert_eq!(
        queue.enqueue_for_approval(&prepared, "01JF00000000000000000000A9", "row-1", 1),
        Err(QueueError::Unbound),
        "another approval than the payload names"
    );
    assert_eq!(
        queue.enqueue_for_approval(&prepared, APPROVAL, "", 1),
        Err(QueueError::Unbound),
        "no tool-call row"
    );
    let person = GraphClient::new(source(dir.path(), "agent-1", "inst-1"));
    assert_eq!(
        queue.enqueue_for_approval(&minted(&person), APPROVAL, "row-1", 1),
        Err(QueueError::Unbound),
        "a person's request has no approval to bind"
    );
    assert!(queue.entries().expect("listing").is_empty());
    let args = queue
        .enqueue_for_approval(&prepared, APPROVAL, "row-1", 1)
        .expect("bound");
    assert_eq!(args, bound_args(DB, &prepared).expect("args"));
    let parsed: serde_json::Value = serde_json::from_str(&args).expect("json");
    assert_eq!(parsed["domain"], DB);
    assert_eq!(parsed["request"]["context_id"], CONTEXT);
    assert_eq!(parsed["request"]["payload"]["approval_id"], APPROVAL);
    assert_eq!(
        queue.bound(&prepared.request().request_id).expect("bound"),
        Some(Bound {
            approval_id: APPROVAL.to_owned(),
            tool_call_id: "row-1".to_owned(),
            bound_args: Some(args),
        })
    );
    assert_eq!(
        state_of(dir.path(), &prepared.request().request_id),
        "awaiting_approval"
    );
}

/// CC-14 step 5: an approved entry is sent exactly as stored only when the
/// call's frozen arguments are the bound ones; modified arguments suspend
/// it, never sent.
#[test]
fn an_approval_sends_only_the_frozen_request() {
    let dir = tempfile::tempdir().expect("dir");
    let client = agent_client(dir.path());
    let mut queue = agent_queue(dir.path());
    let prepared = agent_minted(&client, APPROVAL);
    let id = prepared.request().request_id.clone();
    let args = queue
        .enqueue_for_approval(&prepared, APPROVAL, "row-1", 1)
        .expect("bound");
    // The same bytes, reformatted, are the same arguments.
    let spaced = serde_json::to_string_pretty(
        &serde_json::from_str::<serde_json::Value>(&args).expect("json"),
    )
    .expect("pretty");
    assert_eq!(
        queue.approve(&client, &id, &spaced, 2),
        Ok(Approved::Send(prepared))
    );
    assert_eq!(state_of(dir.path(), &id), "submitting");
    // Approved once: a second approval of it is refused.
    assert!(matches!(
        queue.approve(&client, &id, &args, 3),
        Err(QueueError::Moved { .. })
    ));
}

/// Modified frozen arguments (another title, unreadable, another context
/// or another approval in the row): suspended, never sent, and a suspended
/// entry is never approved again.
#[test]
fn modified_arguments_suspend_the_entry() {
    let dir = tempfile::tempdir().expect("dir");
    let client = agent_client(dir.path());
    let mut queue = agent_queue(dir.path());
    let mutations: [fn(&str) -> String; 4] = [
        |args| args.replace("\"title\":\"A\"", "\"title\":\"B\""),
        |_| "not json".to_owned(),
        |args| args.replace(CONTEXT, "01JF00000000000000000000C2"),
        |args| args.replace(APPROVAL, "01JF00000000000000000000A9"),
    ];
    for (n, mutate) in mutations.into_iter().enumerate() {
        let prepared = agent_minted(&client, APPROVAL);
        let id = prepared.request().request_id.clone();
        let args = queue
            .enqueue_for_approval(&prepared, APPROVAL, &format!("row-{n}"), 1)
            .expect("bound");
        let frozen = mutate(&args);
        assert_ne!(frozen, args, "mutation {n}");
        assert_eq!(
            queue.approve(&client, &id, &frozen, 2),
            Ok(Approved::Suspended),
            "mutation {n}"
        );
        assert_eq!(state_of(dir.path(), &id), "suspended");
        assert!(queue.approve(&client, &id, &args, 3).is_err());
    }
}

/// T0-HQ restart: an agent entry never sent ends `refused_locally` (payload
/// deleted); one that may have been sent becomes `unresolved`, never
/// refused; a person's entries are untouched.
#[test]
fn an_agent_restart_refuses_only_what_was_never_sent() {
    let dir = tempfile::tempdir().expect("dir");
    let client = agent_client(dir.path());
    let mut queue = agent_queue(dir.path());
    let waiting = agent_minted(&client, APPROVAL);
    let sent = agent_minted(&client, "01JF00000000000000000000A2");
    let visible = agent_minted(&client, "01JF00000000000000000000A3");
    for (prepared, row) in [(&waiting, "r1"), (&sent, "r2"), (&visible, "r3")] {
        queue
            .enqueue_for_approval(
                prepared,
                prepared.request().payload["approval_id"]
                    .as_str()
                    .unwrap_or_default(),
                row,
                1,
            )
            .expect("bound");
    }
    for prepared in [&sent, &visible] {
        let id = &prepared.request().request_id;
        let args = queue
            .bound(id)
            .expect("bound")
            .and_then(|b| b.bound_args)
            .unwrap_or_default();
        queue.approve(&client, id, &args, 2).expect("approved");
    }
    queue
        .mark(
            &visible.request().request_id,
            State::Submitting,
            State::PendingVisibility,
            None,
            3,
        )
        .expect("applied");
    assert_eq!(queue.incarnation_ended(4), Ok((1, 2)));
    assert_eq!(
        state_of(dir.path(), &waiting.request().request_id),
        "refused_locally"
    );
    assert_eq!(payload(dir.path(), &waiting.request().request_id), None);
    for prepared in [&sent, &visible] {
        assert_eq!(
            state_of(dir.path(), &prepared.request().request_id),
            "unresolved"
        );
        assert!(payload(dir.path(), &prepared.request().request_id).is_some());
    }
    // An entry that may have been sent is never refused on this device.
    assert!(matches!(
        queue.refuse_locally(&sent.request().request_id, "context_ended", 5),
        Err(QueueError::Transition { .. })
    ));
}

/// A person's queue is never touched by an agent restart, and a person's
/// queued entry may be refused locally only while never sent.
#[test]
fn a_persons_entries_survive_an_agent_restart() {
    let dir = tempfile::tempdir().expect("dir");
    let (_, prepared) = queued(dir.path());
    let mut queue = reopen(dir.path());
    assert_eq!(queue.incarnation_ended(5), Ok((0, 0)));
    assert_eq!(
        state_of(dir.path(), &prepared.request().request_id),
        "queued"
    );
    queue
        .refuse_locally(&prepared.request().request_id, "context_ended", 6)
        .expect("never sent");
    assert_eq!(payload(dir.path(), &prepared.request().request_id), None);
}

/// An entry that may have been sent (`submitting`, `pending_visibility`,
/// `unresolved`) is never refused on this device, whatever ended its
/// context; it stays where it is.
#[test]
fn an_entry_that_may_have_been_sent_is_never_refused_locally() {
    let dir = tempfile::tempdir().expect("dir");
    let (_, prepared) = queued(dir.path());
    let id = prepared.request().request_id.clone();
    let mut queue = reopen(dir.path());
    for (from, to) in [
        (State::Queued, State::Submitting),
        (State::Submitting, State::PendingVisibility),
        (State::PendingVisibility, State::Unresolved),
    ] {
        queue.mark(&id, from, to, None, 2).expect("moved");
        assert!(
            matches!(
                queue.refuse_locally(&id, "context_ended", 3),
                Err(QueueError::Transition { .. })
            ),
            "{to:?}"
        );
        assert_eq!(state_of(dir.path(), &id), to.as_str());
    }
}

/// A version-1 file (before the approval binding) opens as version 2 with
/// its entries kept.
#[test]
fn a_version_one_file_is_migrated_in_place() {
    let dir = tempfile::tempdir().expect("dir");
    let (_, prepared) = queued(dir.path());
    raw(
        dir.path(),
        "CREATE TABLE v1 AS SELECT request_id, server, domain, actor, client, descriptor, \
         version, context_id, request_jcs, request_digest, state, attempts, last_cause, \
         event_id, graph_seq, outcome_code, enqueued_at, updated_at FROM graph_intents; \
         DROP TABLE graph_intents; ALTER TABLE v1 RENAME TO graph_intents; \
         PRAGMA user_version = 1;",
    );
    let client = GraphClient::new(source(dir.path(), "ana", "dev1"));
    let mut queue = reopen(dir.path());
    assert_eq!(
        queue.restore(&client, 7).expect("restores"),
        vec![Restored::Ready {
            prepared: prepared.clone(),
            state: State::Queued
        }]
    );
    assert_eq!(queue.bound(&prepared.request().request_id), Ok(None));
}
