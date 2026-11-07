//! The revocation fence over a real queue file and a real `GraphReads`
//! (a fake graph source and a stand-in worker file; no worker runs).

use std::path::Path;
use std::sync::Arc;

use serde_json::Map;
use sha2::{Digest, Sha256};

use super::*;
use crate::graph::admission::{Admission, Process};
use crate::graph::client::GraphClient;
use crate::graph::protocol::change::ChangeOp;
use crate::graph::protocol::wire::Authority;
use crate::graph::queue::{Binding, State};
use crate::graph::runtime::{Runtime, VerifiedWorker};
use crate::graph::source::{GraphSource, PrefixBatch, SourceError};
use crate::hq::port::fake::FakeOpener;
use crate::hq::port::{Opener, ReplicaSpec};
use crate::hq::sessions::Source;

const DB: &str = "work";

struct Scope(Mutex<Checkpoint>);

impl GraphSource for Scope {
    fn domain(&self) -> &str {
        DB
    }
    fn prefix_batch(&self, _: u64, _: usize) -> Result<PrefixBatch, SourceError> {
        Err(SourceError::Unreadable(
            "not read in these tests".to_owned(),
        ))
    }
    fn checkpoint(&self) -> Result<Checkpoint, SourceError> {
        Ok(*self.0.lock().unwrap_or_else(PoisonError::into_inner))
    }
}

fn checkpoint(scope: ScopeState) -> Checkpoint {
    Checkpoint {
        scope,
        cursor: None,
    }
}

fn reads(dir: &Path) -> GraphReads {
    std::fs::create_dir_all(dir).expect("dir");
    let worker = dir.join("worker");
    std::fs::write(&worker, b"stand-in").expect("worker file");
    let digest: String = Sha256::digest(b"stand-in")
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let worker = VerifiedWorker::open(&worker, Some(&digest)).expect("worker");
    GraphReads::open(
        Arc::new(Scope(Mutex::new(checkpoint(ScopeState::Granted)))),
        &dir.join("projection"),
        Admission::new(Process::Agentd),
        Arc::new(Runtime::for_tests(worker)),
    )
    .expect("reads")
}

fn binding() -> Binding {
    Binding {
        server: "https://hq.example".to_owned(),
        domain: DB.to_owned(),
        actor: "ana".to_owned(),
        client: "dev1".to_owned(),
    }
}

fn queue_path(dir: &Path) -> std::path::PathBuf {
    dir.join("ana.graph-queue")
}

/// A lifecycle over a queue holding one queued change.
fn queued(dir: &Path) -> Lifecycle {
    let replica = FakeOpener::default()
        .open(ReplicaSpec {
            database: DB.to_owned(),
            path: dir.join("ana.sqlite"),
            client_id: Some("dev1".to_owned()),
            link: None,
            subscriptions: Vec::new(),
            watch_presence: None,
        })
        .expect("opens");
    let client = GraphClient::new(Source {
        database: DB.to_owned(),
        replica,
        actor: "ana".to_owned(),
        client: "dev1".to_owned(),
    });
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
    let prepared = client.mint(&change).expect("minted");
    let mut queue = GraphQueue::open(&queue_path(dir), binding()).expect("opens");
    queue.enqueue(&prepared, 1).expect("queued");
    Lifecycle::open(queue).expect("lifecycle")
}

fn status() -> ReadStatus {
    ReadStatus {
        domain: DB.to_owned(),
        authority: Authority {
            epoch: "2".to_owned(),
            digest: "b".repeat(64),
        },
    }
}

/// S08-AC4: a revocation locks first, locks the queue, purges the
/// projection; a restart before the purge is observed starts locked and
/// resumes; only a purged fence with fresh authority unlocks, and the
/// locked entry stays locked.
#[tokio::test(flavor = "current_thread")]
async fn a_revocation_locks_first_and_survives_a_restart() {
    let dir = tempfile::tempdir().expect("dir");
    let lifecycle = queued(dir.path());
    let reads = reads(dir.path());
    lifecycle.gate().expect("open before the signal");
    lifecycle.revoke(Some(&reads), 5).await.expect("revoked");
    assert_eq!(lifecycle.gate(), Err(QueryError::Forbidden));
    assert!(reads.is_revoked());
    assert_eq!(lifecycle.fence(), Ok(Some(FenceState::Revoked)));
    assert_eq!(
        lifecycle.with_queue(|queue| queue.entries()),
        Err(LifecycleError::Queue(QueueError::Fenced))
    );
    // The crash: a new process opens the same file.
    drop(lifecycle);
    let lifecycle =
        Lifecycle::open(GraphQueue::open(&queue_path(dir.path()), binding()).expect("reopens"))
            .expect("lifecycle");
    assert!(lifecycle.is_locked());
    lifecycle.resume(None, 6).await.expect("resumed");
    // A refusal learned before the server's revoke frame: no purge yet, so
    // no regrant.
    assert_eq!(
        lifecycle.observe_purge(checkpoint(ScopeState::Granted), 7),
        Ok(false)
    );
    let fresh = self::reads(&dir.path().join("fresh"));
    assert_eq!(
        lifecycle.regrant(&status(), checkpoint(ScopeState::Granted), &fresh),
        Err(LifecycleError::PurgePending)
    );
    assert_eq!(
        lifecycle.observe_purge(checkpoint(ScopeState::Revoked), 8),
        Ok(true)
    );
    // The old instance stays revoked and is no evidence of fresh authority.
    assert_eq!(
        lifecycle.regrant(&status(), checkpoint(ScopeState::Granted), &reads),
        Err(LifecycleError::NotFresh)
    );
    assert_eq!(
        lifecycle.regrant(&status(), checkpoint(ScopeState::Pending), &fresh),
        Err(LifecycleError::NotFresh)
    );
    assert!(lifecycle.is_locked());
    lifecycle
        .regrant(&status(), checkpoint(ScopeState::Granted), &fresh)
        .expect("regranted");
    lifecycle.gate().expect("unlocked");
    let entries = lifecycle
        .with_queue(|queue| queue.entries())
        .expect("listing");
    assert_eq!(entries[0].state, State::Disabled);
}

/// A failed fence write still leaves this process locked: no step of a
/// revocation ever unlocks.
#[tokio::test(flavor = "current_thread")]
async fn a_failed_fence_write_stays_locked() {
    let dir = tempfile::tempdir().expect("dir");
    let lifecycle = queued(dir.path());
    rusqlite::Connection::open(queue_path(dir.path()))
        .expect("raw")
        .execute_batch(
            "CREATE TRIGGER refuse BEFORE INSERT ON graph_fence \
             BEGIN SELECT RAISE(ABORT, 'disk full'); END;",
        )
        .expect("trigger");
    let error = lifecycle.revoke(None, 5).await.expect_err("refused");
    assert!(matches!(
        error,
        LifecycleError::Queue(QueueError::Storage(_))
    ));
    assert_eq!(lifecycle.gate(), Err(QueryError::Forbidden));
}
