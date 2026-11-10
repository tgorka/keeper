//! The revocation fence over a real queue file and a real `GraphReads`
//! (a fake graph source and a stand-in worker file; no worker runs).

use std::path::Path;
use std::sync::Arc;

use serde_json::Map;
use sha2::{Digest, Sha256};

use super::*;
use crate::graph::admission::{Admission, Process};
use crate::graph::client::GraphClient;
use crate::graph::outcome::{GraphOutcome, PendingCause, REQUEST_REFUSALS};
use crate::graph::protocol::change::ChangeOp;
use crate::graph::protocol::wire::Authority;
use crate::graph::queue::{Binding, State};
use crate::graph::runtime::{Runtime, VerifiedWorker};
use crate::graph::source::{GraphSource, PrefixBatch, Revocation, SourceError};
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
    assert_eq!(lifecycle.observe_purge(Revocation::None, 7), Ok(false));
    let fresh = self::reads(&dir.path().join("fresh"));
    assert_eq!(
        lifecycle.regrant(&status(), checkpoint(ScopeState::Granted), &fresh),
        Err(LifecycleError::PurgePending)
    );
    // The history tables revoked, the receipts or proposals not yet: their
    // rows may still be on disk, so the fence stays `revoked`.
    assert_eq!(lifecycle.observe_purge(Revocation::Partial, 8), Ok(false));
    assert_eq!(lifecycle.fence(), Ok(Some(FenceState::Revoked)));
    assert_eq!(lifecycle.observe_purge(Revocation::Purged, 8), Ok(true));
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

/// The one entry the queued fixture holds.
fn only_entry(dir: &Path) -> (String, State) {
    let raw = rusqlite::Connection::open(queue_path(dir)).expect("raw");
    raw.query_row("SELECT request_id, state FROM graph_intents", [], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
    })
    .map(|(id, state)| {
        let state = match state.as_str() {
            "queued" => State::Queued,
            "submitting" => State::Submitting,
            "disabled" => State::Disabled,
            "rejected" => State::Rejected,
            "unresolved" => State::Unresolved,
            other => panic!("unexpected state {other}"),
        };
        (id, state)
    })
    .expect("one entry")
}

fn forbidden(code: Option<&str>) -> GraphOutcome {
    let mut detail = Map::new();
    if let Some(code) = code {
        detail.insert("code".into(), code.into());
    }
    GraphOutcome::Rejected {
        reason: "forbidden".to_owned(),
        detail,
    }
}

/// CC-7: a forbidden receipt (the caller lost the domain) is a revocation
/// signal. The fence is persisted and every open entry locked before the
/// receipt's own outcome is recorded, and the domain stays locked.
#[test]
fn a_forbidden_receipt_locks_the_domain_before_it_is_recorded() {
    for code in [
        None,
        Some("forbidden"),
        Some("source_audience"),
        Some("label_unknown"),
    ] {
        let dir = tempfile::tempdir().expect("dir");
        let lifecycle = queued(dir.path());
        let (id, _) = only_entry(dir.path());
        lifecycle
            .with_queue(|q| q.mark(&id, State::Queued, State::Submitting, None, 2))
            .expect("sent");
        assert_eq!(
            lifecycle.settle(&id, State::Submitting, &forbidden(code), 3),
            Ok(State::Rejected),
            "{code:?}"
        );
        assert_eq!(lifecycle.gate(), Err(QueryError::Forbidden), "{code:?}");
        assert_eq!(lifecycle.fence(), Ok(Some(FenceState::Revoked)), "{code:?}");
        assert_eq!(only_entry(dir.path()).1, State::Rejected);
    }
}

/// CC-7/CC-14: an approval-class or ended-context refusal ends that one
/// request and is never a revocation signal.
#[test]
fn a_request_refusal_ends_the_request_and_never_fences() {
    for code in REQUEST_REFUSALS {
        let dir = tempfile::tempdir().expect("dir");
        let lifecycle = queued(dir.path());
        let (id, _) = only_entry(dir.path());
        lifecycle
            .with_queue(|q| q.mark(&id, State::Queued, State::Submitting, None, 2))
            .expect("sent");
        assert_eq!(
            lifecycle.settle(&id, State::Submitting, &forbidden(Some(code)), 3),
            Ok(State::Rejected),
            "{code}"
        );
        lifecycle.gate().expect(code);
        assert_eq!(lifecycle.fence(), Ok(None), "{code}");
    }
}

/// CC-5: a refused send or `graph.outcome` locks the domain and disables
/// the entry; a locked entry is moved by nothing but its own outcome — a
/// later transport failure, unresolved answer or ended context leaves it
/// disabled.
#[test]
fn a_refusal_disables_and_a_disabled_entry_stays_locked() {
    let dir = tempfile::tempdir().expect("dir");
    let lifecycle = queued(dir.path());
    let (id, _) = only_entry(dir.path());
    let refused = GraphOutcome::Pending {
        cause: PendingCause::Refused,
    };
    assert_eq!(
        lifecycle.settle(&id, State::Queued, &refused, 2),
        Ok(State::Disabled)
    );
    assert_eq!(lifecycle.fence(), Ok(Some(FenceState::Revoked)));
    assert_eq!(only_entry(dir.path()).1, State::Disabled);
    for cause in [
        PendingCause::Transport,
        PendingCause::Unresolved,
        PendingCause::ContextEnded,
        PendingCause::Foreign,
    ] {
        assert_eq!(
            lifecycle.settle(&id, State::Disabled, &GraphOutcome::Pending { cause }, 3),
            Ok(State::Disabled),
            "{cause:?}"
        );
        assert_eq!(only_entry(dir.path()).1, State::Disabled, "{cause:?}");
    }
}

/// CC-5: a transport failure keeps the entry under its own id (retried
/// identically), and `outcome_unresolved` makes it `unresolved`, never
/// refused and never re-minted.
#[test]
fn transport_retries_and_unresolved_is_kept() {
    let dir = tempfile::tempdir().expect("dir");
    let lifecycle = queued(dir.path());
    let (id, _) = only_entry(dir.path());
    lifecycle
        .with_queue(|q| q.mark(&id, State::Queued, State::Submitting, None, 2))
        .expect("sent");
    for cause in [PendingCause::Transport, PendingCause::Unavailable] {
        assert_eq!(
            lifecycle.settle(&id, State::Submitting, &GraphOutcome::Pending { cause }, 3),
            Ok(State::Submitting)
        );
    }
    assert_eq!(
        lifecycle.settle(
            &id,
            State::Submitting,
            &GraphOutcome::Pending {
                cause: PendingCause::ContextEnded
            },
            4
        ),
        Ok(State::Unresolved),
        "an entry that may have been sent is never refused locally"
    );
    assert_eq!(only_entry(dir.path()), (id, State::Unresolved));
    lifecycle.gate().expect("no fence");
}
