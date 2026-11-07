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
