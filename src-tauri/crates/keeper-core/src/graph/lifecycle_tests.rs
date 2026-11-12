//! The revocation fence over a real queue file and a real `GraphReads`
//! (a fake graph source and a stand-in worker file; no worker runs).

use std::path::Path;
use std::sync::Arc;

use serde_json::{json, Map};
use sha2::{Digest, Sha256};

use super::*;
use crate::graph::admission::{Admission, Process};
use crate::graph::client::{GraphClient, GraphClientError};
use crate::graph::outcome::{GraphOutcome, PendingCause, REQUEST_REFUSALS};
use crate::graph::protocol::change::ChangeOp;
use crate::graph::protocol::scope::{client_scope, receipt_id};
use crate::graph::protocol::wire::{self, Authority, ContextRow};
use crate::graph::query::GraphRemote;
use crate::graph::queue::{Binding, Restored, State};
use crate::graph::runtime::{Runtime, VerifiedWorker};
use crate::graph::source::{GraphSource, PrefixBatch, Revocation, SourceError};
use crate::graph::write::{Caller, GraphWrite, Writer};
use crate::hq::port::fake::FakeOpener;
use crate::hq::port::{CommandResult, LinkState, Opener, ReplicaError, ReplicaSpec};
use crate::hq::sessions::Source;

const DB: &str = "work";

struct Scope(Mutex<Checkpoint>, &'static str);

impl GraphSource for Scope {
    fn domain(&self) -> &str {
        self.1
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
    reads_of(dir, DB)
}

fn reads_of(dir: &Path, domain: &'static str) -> GraphReads {
    std::fs::create_dir_all(dir).expect("dir");
    let worker = dir.join("worker");
    std::fs::write(&worker, b"stand-in").expect("worker file");
    let digest: String = Sha256::digest(b"stand-in")
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    let worker = VerifiedWorker::open(&worker, Some(&digest)).expect("worker");
    GraphReads::open(
        Arc::new(Scope(Mutex::new(checkpoint(ScopeState::Granted)), domain)),
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
    lifecycle
        .revoke(Some(&reads), 5, FenceCause::Revocation)
        .await
        .expect("revoked");
    assert_eq!(lifecycle.gate(), Err(QueryError::Fenced));
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
    let error = lifecycle
        .revoke(None, 5, FenceCause::Revocation)
        .await
        .expect_err("refused");
    assert!(matches!(
        error,
        LifecycleError::Queue(QueueError::Storage(_))
    ));
    assert_eq!(lifecycle.gate(), Err(QueryError::Fenced));
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
            "accepted" => State::Accepted,
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

/// Owner decision 2026-11-12 (amending CC-7): a `forbidden` receipt on a
/// write, review, acknowledgement or decision ends that request alone,
/// whatever its code. No fence, the gate stays open, and every other entry
/// is untouched (CC-11 applies only to a real fence).
#[test]
fn a_forbidden_receipt_ends_that_request_alone() {
    for code in [
        None,
        Some("forbidden"),
        Some("context_revoked"),
        Some("source_audience"),
        Some("label_unknown"),
        Some("an_unknown_code"),
    ] {
        let dir = tempfile::tempdir().expect("dir");
        let lifecycle = queued(dir.path());
        let (id, _) = only_entry(dir.path());
        lifecycle
            .with_queue(|q| q.mark(&id, State::Queued, State::Submitting, None, 2))
            .expect("sent");
        let other = enqueue_another(&lifecycle, dir.path());
        assert_eq!(
            lifecycle.settle(&id, State::Submitting, &forbidden(code), 3),
            Ok(State::Rejected),
            "{code:?}"
        );
        lifecycle.gate().expect("no fence");
        assert_eq!(lifecycle.fence(), Ok(None), "{code:?}");
        assert_eq!(state_of(dir.path(), &id), State::Rejected, "{code:?}");
        assert_eq!(state_of(dir.path(), &other), State::Queued, "{code:?}");
    }
}

/// Queue a second change beside the fixture's one; its request id.
fn enqueue_another(lifecycle: &Lifecycle, dir: &Path) -> String {
    let client = GraphClient::new(Source {
        database: DB.to_owned(),
        replica: FakeOpener::default()
            .open(ReplicaSpec {
                database: DB.to_owned(),
                path: dir.join("ana-2.sqlite"),
                client_id: Some("dev1".to_owned()),
                link: None,
                subscriptions: Vec::new(),
                watch_presence: None,
            })
            .expect("opens"),
        actor: "ana".to_owned(),
        client: "dev1".to_owned(),
    });
    let change = client
        .change(
            vec![ChangeOp::CreateNode {
                id: "01JF0000000000000000000002".to_owned(),
                kind: "Concept".into(),
                title: "M".into(),
                description: None,
                properties: Map::new(),
            }],
            None,
        )
        .expect("change");
    let prepared = client.mint(&change).expect("minted");
    lifecycle
        .with_queue(|q| q.enqueue(&prepared, 2))
        .expect("queued");
    prepared.request().request_id.clone()
}

/// The state of entry `id` in the queue file.
fn state_of(dir: &Path, id: &str) -> State {
    let raw = rusqlite::Connection::open(queue_path(dir)).expect("raw");
    let state: String = raw
        .query_row(
            "SELECT state FROM graph_intents WHERE request_id = ?1",
            [id],
            |row| row.get(0),
        )
        .expect("entry");
    match state.as_str() {
        "queued" => State::Queued,
        "submitting" => State::Submitting,
        "disabled" => State::Disabled,
        "rejected" => State::Rejected,
        "unresolved" => State::Unresolved,
        "accepted" => State::Accepted,
        "refused_locally" => State::RefusedLocally,
        other => panic!("unexpected state {other}"),
    }
}

/// Review BLOCKING-1/B-A/B-B: a transient or lifecycle answer to a sent
/// request never fences: the server's storage outage, a failed handler, an
/// unknown code, the graph switched off, an unknown operation, an ended or
/// unbound context, an invalid request.
#[test]
fn a_transient_or_lifecycle_answer_never_fences() {
    let remote = |code: &str, message: &str, retryable: bool| ReplicaError::Remote {
        code: code.to_owned(),
        message: message.to_owned(),
        retryable,
    };
    for error in [
        remote("sync.internal_error", "storage_unavailable", true),
        remote("operation.execution_failed", "m", false),
        remote("operation.query_failed", "m", false),
        remote("some.new_code", "m", false),
        remote("operation.forbidden", "graph_unavailable", false),
        remote("operation.unknown", "m", false),
        remote("operation.forbidden", "context_expired", false),
        remote("operation.forbidden", "parent_unavailable", false),
        remote("operation.forbidden", "context_unbound", false),
        remote("operation.invalid_request", "m", false),
    ] {
        let dir = tempfile::tempdir().expect("dir");
        let lifecycle = queued(dir.path());
        let (id, _) = only_entry(dir.path());
        lifecycle
            .with_queue(|q| q.mark(&id, State::Queued, State::Submitting, None, 2))
            .expect("sent");
        let other = enqueue_another(&lifecycle, dir.path());
        let outcome = GraphOutcome::Pending {
            cause: PendingCause::of(&error),
        };
        lifecycle
            .settle(&id, State::Submitting, &outcome, 3)
            .expect("settled");
        lifecycle.gate().expect("no fence");
        assert_eq!(lifecycle.fence(), Ok(None), "{error:?}");
        assert_ne!(state_of(dir.path(), &id), State::Disabled, "{error:?}");
        assert_eq!(state_of(dir.path(), &other), State::Queued, "{error:?}");
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

/// A person's server after a reconnect, as a resume sees it: the receipt
/// this replica holds (once it replicated), a scripted answer to a same-id
/// send (none: nothing may be sent) and to `graph.read_status`, and every
/// remote operation asked, in order.
struct Reconnected {
    receipt: Mutex<Option<Map<String, serde_json::Value>>>,
    command: Option<Result<CommandResult, ReplicaError>>,
    status: Result<Vec<Map<String, serde_json::Value>>, ReplicaError>,
    asked: Mutex<Vec<String>>,
}

impl GraphWrite for Reconnected {
    fn command(
        &self,
        _: &str,
        request_id: &str,
        _: serde_json::Value,
    ) -> Result<CommandResult, ReplicaError> {
        let answer = self
            .command
            .clone()
            .expect("a cached receipt is never resent");
        answer.map(|mut result| {
            result.request_id = request_id.to_owned();
            result
        })
    }
    fn receipt(&self, _: &str) -> Result<Vec<Map<String, serde_json::Value>>, ReplicaError> {
        Ok(self
            .receipt
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect())
    }
    fn remote_query(
        &self,
        operation: &str,
        _: serde_json::Value,
    ) -> Result<Vec<Map<String, serde_json::Value>>, ReplicaError> {
        self.asked
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(operation.to_owned());
        if operation == wire::READ_STATUS {
            self.status.clone()
        } else {
            Ok(Vec::new())
        }
    }
    fn link(&self) -> LinkState {
        LinkState::Connected
    }
}

/// One sent change whose answer was lost and whose accepted receipt
/// replicates, resumed as the driver's `resume` does it (restore, then
/// `outcome` and `settle`) against a server answering `read_status` with
/// `status`. With no `replay` the receipt is already held and the lookup
/// has no attempt; with one, the same id is sent again, the server answers
/// `replay`, the receipt replicates meanwhile, and the lookup has that
/// attempt. The outcome looked up, the state the entry settled in (or the
/// lookup's error) and the remote operations asked.
fn resume_cached(
    dir: &Path,
    replay: Option<Result<CommandResult, ReplicaError>>,
    status: Result<Vec<Map<String, serde_json::Value>>, ReplicaError>,
) -> (
    Lifecycle,
    Result<(GraphOutcome, State), GraphClientError>,
    Vec<String>,
) {
    let writer = Writer {
        domain: DB.to_owned(),
        actor: "ana".to_owned(),
        client: "dev1".to_owned(),
    };
    let minting = GraphClient::over(
        Arc::new(Reconnected {
            receipt: Mutex::new(None),
            command: None,
            status: Ok(Vec::new()),
            asked: Mutex::new(Vec::new()),
        }),
        &writer,
        Caller::Person,
    );
    let change = minting
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
    let prepared = minting.mint(&change).expect("minted");
    let mut queue = GraphQueue::open(&queue_path(dir), binding()).expect("opens");
    queue.enqueue(&prepared, 1).expect("queued");
    let id = prepared.request().request_id.clone();
    queue
        .mark(&id, State::Queued, State::Submitting, Some("sent"), 2)
        .expect("sent");
    let lifecycle = Lifecycle::open(queue).expect("lifecycle");
    let key = prepared.key();
    let scope = client_scope("ana", "dev1");
    let receipt = json!({
        "id": receipt_id(key.descriptor(), key.request_id(), &scope),
        "client_scope": scope, "descriptor": key.descriptor(),
        "request_id": key.request_id(), "request_digest": key.request_digest(),
        "outcome": "accepted", "reason": null,
        "event_id": "01JF0000000000000000000900", "graph_seq": "7",
        "detail": "{\"affected\":[]}", "recorded_at_ms": 1,
        "author_actor": "server", "author_client": "server",
    });
    let receipt = receipt.as_object().expect("row").clone();
    let port = Arc::new(Reconnected {
        receipt: Mutex::new(replay.is_none().then(|| receipt.clone())),
        command: replay.clone(),
        status,
        asked: Mutex::new(Vec::new()),
    });
    let client = GraphClient::over(port.clone(), &writer, Caller::Person);
    let restored = lifecycle
        .with_queue(|q| q.restore(&client, 3))
        .expect("restores");
    let [Restored::Ready { prepared, state }] = restored.as_slice() else {
        panic!("one ready entry");
    };
    assert_eq!(*state, State::Submitting);
    let attempt = replay.map(|_| {
        let attempt = client.send(prepared).expect("sent");
        *port.receipt.lock().unwrap_or_else(PoisonError::into_inner) = Some(receipt);
        attempt
    });
    let settled = client.outcome(prepared, attempt.as_ref()).map(|looked| {
        let state = lifecycle
            .settle(&id, State::Submitting, &looked, 4)
            .expect("settled");
        (looked, state)
    });
    let asked = port
        .asked
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    (lifecycle, settled, asked)
}

/// 112.1 T2.3 (audit G1): on a person's resume after a reconnect, a
/// receipt already on the replica settles nothing until the server's
/// current `graph.read_status` answered. A refused status fences the domain
/// and locks the entry instead of recording the cached outcome; an
/// unreachable server leaves the entry where it was.
#[test]
fn a_cached_receipt_settles_only_after_a_fresh_read_status() {
    for refusal in [
        ReplicaError::Remote {
            code: "operation.forbidden".to_owned(),
            message: "forbidden".to_owned(),
            retryable: false,
        },
        ReplicaError::Rejected {
            code: "forbidden".to_owned(),
            message: "revoked".to_owned(),
        },
    ] {
        let dir = tempfile::tempdir().expect("dir");
        let (lifecycle, settled, asked) = resume_cached(dir.path(), None, Err(refusal.clone()));
        assert_eq!(asked, vec![wire::READ_STATUS.to_owned()], "{refusal:?}");
        assert_eq!(
            settled,
            Ok((
                GraphOutcome::Pending {
                    cause: PendingCause::Refused
                },
                State::Disabled
            )),
            "{refusal:?}"
        );
        assert_eq!(lifecycle.gate(), Err(QueryError::Fenced));
        assert_eq!(lifecycle.fence(), Ok(Some(FenceState::Revoked)));
        assert_eq!(only_entry(dir.path()).1, State::Disabled, "{refusal:?}");
    }

    let dir = tempfile::tempdir().expect("dir");
    let (lifecycle, settled, asked) = resume_cached(dir.path(), None, Err(ReplicaError::Closed));
    assert_eq!(asked, vec![wire::READ_STATUS.to_owned()]);
    assert_eq!(
        settled,
        Err(GraphClientError::Replica(ReplicaError::Closed))
    );
    lifecycle.gate().expect("no fence");
    assert_eq!(only_entry(dir.path()).1, State::Submitting);

    let dir = tempfile::tempdir().expect("dir");
    let (lifecycle, settled, asked) = resume_cached(dir.path(), None, Ok(vec![answered_status()]));
    assert_eq!(asked, vec![wire::READ_STATUS.to_owned()]);
    let (looked, state) = settled.expect("looked up");
    assert!(
        matches!(looked, GraphOutcome::Accepted { .. }),
        "{looked:?}"
    );
    assert_eq!(state, State::Accepted);
    lifecycle.gate().expect("no fence");
}

fn answered_status() -> Map<String, serde_json::Value> {
    json!({"domain": DB, "authority": {"epoch": "2", "digest": "b".repeat(64)}})
        .as_object()
        .expect("row")
        .clone()
}

fn forbidden_error() -> ReplicaError {
    ReplicaError::Remote {
        code: "operation.forbidden".to_owned(),
        message: "forbidden".to_owned(),
        retryable: false,
    }
}

fn unheard() -> ReplicaError {
    ReplicaError::Unheard {
        code: "transport".to_owned(),
        message: "response lost".to_owned(),
        retryable: true,
    }
}

/// 112.1 T2.3 (review B1): an attempt in hand is not current authority.
/// A same-id replay the server refused or never answered, with the
/// original's accepted receipt replicating meanwhile, settles that receipt
/// only after a fresh `graph.read_status`: a refused status fences and
/// locks the entry, and with no authority to be had the attempt speaks
/// alone (its refusal fences, an unanswered one stays pending). Only this
/// request's own fresh acknowledgement settles the receipt without asking;
/// a `cached` acknowledgement asks first.
#[test]
fn a_replay_settles_a_cached_receipt_only_under_fresh_authority() {
    // The reviewer's two probes: refused replay, refused status; unanswered
    // replay, unanswered status.
    let dir = tempfile::tempdir().expect("dir");
    let (lifecycle, settled, asked) = resume_cached(
        dir.path(),
        Some(Err(forbidden_error())),
        Err(forbidden_error()),
    );
    assert_eq!(asked, vec![wire::READ_STATUS.to_owned()]);
    assert_eq!(
        settled,
        Ok((
            GraphOutcome::Pending {
                cause: PendingCause::Refused
            },
            State::Disabled
        ))
    );
    assert_eq!(lifecycle.fence(), Ok(Some(FenceState::Revoked)));
    assert_eq!(only_entry(dir.path()).1, State::Disabled);

    let dir = tempfile::tempdir().expect("dir");
    let (lifecycle, settled, asked) =
        resume_cached(dir.path(), Some(Err(unheard())), Err(unheard()));
    assert_eq!(asked, vec![wire::READ_STATUS.to_owned()]);
    assert_eq!(
        settled,
        Ok((
            GraphOutcome::Pending {
                cause: PendingCause::Transport
            },
            State::Submitting
        ))
    );
    lifecycle.gate().expect("no fence");
    assert_eq!(only_entry(dir.path()).1, State::Submitting);

    // A replay refused at send with the server's authority unreachable: the
    // cached outcome stays unused, and the command's refusal ends that
    // request alone (owner 2026-11-12, amending CC-7): no fence.
    let dir = tempfile::tempdir().expect("dir");
    let (lifecycle, settled, _) =
        resume_cached(dir.path(), Some(Err(forbidden_error())), Err(unheard()));
    let (looked, state) = settled.expect("looked up");
    assert!(
        matches!(&looked, GraphOutcome::Rejected { reason, .. } if reason == "forbidden"),
        "{looked:?}"
    );
    assert_eq!(state, State::Rejected);
    assert_eq!(lifecycle.fence(), Ok(None));

    // A cached acknowledgement is a replay of an earlier answer: the
    // receipt waits for a fresh status, then settles.
    let replayed = CommandResult {
        request_id: String::new(),
        commit_seq: Some(7),
        cached: true,
    };
    let dir = tempfile::tempdir().expect("dir");
    let (_, settled, asked) = resume_cached(
        dir.path(),
        Some(Ok(replayed.clone())),
        Err(forbidden_error()),
    );
    assert_eq!(asked, vec![wire::READ_STATUS.to_owned()]);
    assert_eq!(
        settled.map(|(_, state)| state),
        Ok(State::Disabled),
        "a cached acknowledgement under a refused status"
    );
    let dir = tempfile::tempdir().expect("dir");
    let (_, settled, asked) =
        resume_cached(dir.path(), Some(Ok(replayed)), Ok(vec![answered_status()]));
    assert_eq!(asked, vec![wire::READ_STATUS.to_owned()]);
    assert_eq!(settled.map(|(_, state)| state), Ok(State::Accepted));

    // The control: this request's own fresh acknowledgement is the server's
    // authorisation at acceptance; its receipt settles without asking.
    let dir = tempfile::tempdir().expect("dir");
    let (lifecycle, settled, asked) = resume_cached(
        dir.path(),
        Some(Ok(CommandResult {
            request_id: String::new(),
            commit_seq: Some(7),
            cached: false,
        })),
        Err(forbidden_error()),
    );
    assert!(asked.is_empty(), "{asked:?}");
    let (looked, state) = settled.expect("looked up");
    assert!(
        matches!(looked, GraphOutcome::Accepted { .. }),
        "{looked:?}"
    );
    assert_eq!(state, State::Accepted);
    lifecycle.gate().expect("no fence");
}

/// A server whose grant is withdrawn: every command is refused
/// `operation.forbidden`; `graph.outcome` answers nothing until `withdrawn`
/// is learned at read level, then refuses too.
struct Withdrawn {
    reads_refused: std::sync::atomic::AtomicBool,
}

impl GraphWrite for Withdrawn {
    fn command(
        &self,
        _: &str,
        _: &str,
        _: serde_json::Value,
    ) -> Result<CommandResult, ReplicaError> {
        Err(forbidden_error())
    }
    fn receipt(&self, _: &str) -> Result<Vec<Map<String, serde_json::Value>>, ReplicaError> {
        Ok(Vec::new())
    }
    fn remote_query(
        &self,
        _: &str,
        _: serde_json::Value,
    ) -> Result<Vec<Map<String, serde_json::Value>>, ReplicaError> {
        if self.reads_refused.load(std::sync::atomic::Ordering::SeqCst) {
            Err(forbidden_error())
        } else {
            Ok(Vec::new())
        }
    }
    fn link(&self) -> LinkState {
        LinkState::Connected
    }
}

/// The inverse of the amended CC-7 (owner 2026-11-12): a command refused at
/// send ends that request alone, but when the grant really was withdrawn
/// the next read-level call (here the other entry's `graph.outcome` lookup)
/// still fences the domain and disables what is open.
#[test]
fn a_command_refusal_ends_its_request_and_a_read_level_refusal_still_fences() {
    let dir = tempfile::tempdir().expect("dir");
    let lifecycle = queued(dir.path());
    let (sent, _) = only_entry(dir.path());
    let other = enqueue_another(&lifecycle, dir.path());
    let port = Arc::new(Withdrawn {
        reads_refused: std::sync::atomic::AtomicBool::new(false),
    });
    let writer = Writer {
        domain: DB.to_owned(),
        actor: "ana".to_owned(),
        client: "dev1".to_owned(),
    };
    let client = GraphClient::over(port.clone(), &writer, Caller::Person);
    let restored = lifecycle
        .with_queue(|q| q.restore(&client, 3))
        .expect("restores");
    let prepared = |id: &str| {
        restored
            .iter()
            .find_map(|r| match r {
                Restored::Ready { prepared, .. } if prepared.request().request_id == id => {
                    Some(prepared)
                }
                _ => None,
            })
            .expect("restored")
    };

    // The command refused at send: that request ends, nothing is fenced.
    lifecycle
        .with_queue(|q| q.mark(&sent, State::Queued, State::Submitting, None, 4))
        .expect("sending");
    let attempt = client.send(prepared(&sent)).expect("sent");
    let outcome = client
        .outcome(prepared(&sent), Some(&attempt))
        .expect("outcome");
    assert_eq!(
        lifecycle.settle(&sent, State::Submitting, &outcome, 5),
        Ok(State::Rejected)
    );
    assert_eq!(lifecycle.fence(), Ok(None));
    assert_eq!(state_of(dir.path(), &other), State::Queued);

    // The grant was withdrawn: the next read-level call fences.
    port.reads_refused
        .store(true, std::sync::atomic::Ordering::SeqCst);
    let looked = client.outcome(prepared(&other), None).expect("lookup");
    assert!(looked.revokes(), "{looked:?}");
    assert_eq!(
        lifecycle.settle(&other, State::Queued, &looked, 6),
        Ok(State::Disabled)
    );
    assert_eq!(lifecycle.fence(), Ok(Some(FenceState::Revoked)));
    assert_eq!(lifecycle.gate(), Err(QueryError::Fenced));
    assert_eq!(state_of(dir.path(), &other), State::Disabled);
    assert_eq!(state_of(dir.path(), &sent), State::Rejected);
}

/// The server's `graph.admit_read` as an agent's regrant sees it: it admits
/// `domain` into context `C1` of session `S1` until `expires_at_ms`.
struct Admits {
    domain: &'static str,
    expires_at_ms: u64,
}

impl GraphRemote for Admits {
    fn admit_read(&self, context: &str, _: &str) -> Result<ContextRow, QueryError> {
        Ok(ContextRow {
            context: context.to_owned(),
            session: "01JF00000000000000000000S1".to_owned(),
            permitted: vec![self.domain.to_owned()],
            sources: vec![self.domain.to_owned()],
            expires_at_ms: self.expires_at_ms,
            authority: Authority {
                epoch: "2".to_owned(),
                digest: "b".repeat(64),
            },
        })
    }
    fn refresh_context(&self, context: &str) -> Result<ContextRow, QueryError> {
        self.admit_read(context, self.domain)
    }
    fn read_status(&self, _: &str) -> Result<ReadStatus, QueryError> {
        Err(QueryError::Forbidden)
    }
}

/// The far future: an admission that has not expired at any `now` here.
const LATER: u64 = 4_102_444_800_000;

fn admitted_in(domain: &'static str, expires_at_ms: u64) -> AdmittedRead {
    admitted_as("ana", domain, expires_at_ms)
}

/// An admission for `agent` (this queue's actor is `ana`).
fn admitted_as(agent: &str, domain: &'static str, expires_at_ms: u64) -> AdmittedRead {
    AdmittedRead::admit(
        &Admits {
            domain,
            expires_at_ms,
        },
        agent,
        "01JF00000000000000000000S1",
        "01JF00000000000000000000C1",
        domain,
    )
    .expect("admitted")
}

/// 112.1 CC-9/T3 (R-B6 K2), an agent's regrant: a lifecycle whose one
/// queued entry a revocation disabled, its revoked reads instance, a new
/// instance, and (`purged`) the replica's purge observed. Admitting and
/// opening the new instance happen while it is fenced.
async fn fenced(dir: &Path, purged: bool) -> (Lifecycle, GraphReads, GraphReads) {
    fenced_at(dir, purged, 5).await
}

/// [`fenced`], the fence set at `at` (the caller's clock).
async fn fenced_at(dir: &Path, purged: bool, at: i64) -> (Lifecycle, GraphReads, GraphReads) {
    let (lifecycle, revoked, fresh) = fenced_by(dir, at, FenceCause::Revocation).await;
    if purged {
        assert_eq!(
            lifecycle.observe_purge(Revocation::Purged, at + 1),
            Ok(true)
        );
    }
    (lifecycle, revoked, fresh)
}

/// A lifecycle fenced at `at` from `cause` (its one queued entry
/// disabled), its revoked reads instance and a new one; no purge observed.
async fn fenced_by(dir: &Path, at: i64, cause: FenceCause) -> (Lifecycle, GraphReads, GraphReads) {
    let lifecycle = queued(dir);
    let revoked = reads(dir);
    lifecycle
        .revoke(Some(&revoked), at, cause)
        .await
        .expect("revoked");
    let fresh = reads(&dir.join("fresh"));
    (lifecycle, revoked, fresh)
}

/// The lifecycle a restart opens over `dir`'s queue file.
fn restarted(lifecycle: Lifecycle, dir: &Path) -> Lifecycle {
    drop(lifecycle);
    let lifecycle =
        Lifecycle::open(GraphQueue::open(&queue_path(dir), binding()).expect("reopens"))
            .expect("lifecycle");
    assert!(lifecycle.is_locked());
    lifecycle
}

/// The fence still stands, unchanged, after a refused regrant.
fn still_fenced(lifecycle: &Lifecycle, fence: FenceState) {
    assert!(lifecycle.is_locked());
    assert_eq!(lifecycle.gate(), Err(QueryError::Fenced));
    assert_eq!(lifecycle.fence(), Ok(Some(fence)));
}

/// Everything fresh, from `purged`: the fence clears and the disabled
/// entry stays disabled (CC-11).
#[tokio::test(flavor = "current_thread")]
async fn an_agent_regrant_clears_a_purged_fence_under_a_fresh_admission() {
    let dir = tempfile::tempdir().expect("dir");
    let (lifecycle, _revoked, fresh) = fenced(dir.path(), true).await;
    let read = admitted_in(DB, LATER);
    lifecycle
        .regrant_admitted(&read, checkpoint(ScopeState::Granted), &fresh)
        .expect("regranted");
    lifecycle.gate().expect("unlocked");
    assert_eq!(lifecycle.fence(), Ok(None));
    assert_eq!(only_entry(dir.path()).1, State::Disabled);
}

/// Still `revoked` (the purge not observed): `PurgePending`.
#[tokio::test(flavor = "current_thread")]
async fn an_agent_regrant_waits_for_the_purge() {
    let dir = tempfile::tempdir().expect("dir");
    let (lifecycle, _revoked, fresh) = fenced(dir.path(), false).await;
    assert_eq!(
        lifecycle.regrant_admitted(
            &admitted_in(DB, LATER),
            checkpoint(ScopeState::Granted),
            &fresh
        ),
        Err(LifecycleError::PurgePending)
    );
    still_fenced(&lifecycle, FenceState::Revoked);
}

/// An admission that has expired since `admit_read` answered: `NotFresh`.
#[tokio::test(flavor = "current_thread")]
async fn an_agent_regrant_refuses_an_expired_admission() {
    let dir = tempfile::tempdir().expect("dir");
    let (lifecycle, _revoked, fresh) = fenced(dir.path(), true).await;
    let read = admitted_in(DB, now_plus(1_000));
    std::thread::sleep(std::time::Duration::from_millis(1_100));
    assert_eq!(
        lifecycle.regrant_admitted(&read, checkpoint(ScopeState::Granted), &fresh),
        Err(LifecycleError::NotFresh)
    );
    still_fenced(&lifecycle, FenceState::Purged);
}

/// An admission of another domain: `NotFresh`.
#[tokio::test(flavor = "current_thread")]
async fn an_agent_regrant_refuses_another_domains_admission() {
    let dir = tempfile::tempdir().expect("dir");
    let (lifecycle, _revoked, fresh) = fenced(dir.path(), true).await;
    assert_eq!(
        lifecycle.regrant_admitted(
            &admitted_in("other", LATER),
            checkpoint(ScopeState::Granted),
            &fresh
        ),
        Err(LifecycleError::NotFresh)
    );
    still_fenced(&lifecycle, FenceState::Purged);
}

/// The revoked instance is no evidence of fresh authority: `NotFresh`.
#[tokio::test(flavor = "current_thread")]
async fn an_agent_regrant_refuses_the_revoked_instance() {
    let dir = tempfile::tempdir().expect("dir");
    let (lifecycle, revoked, _fresh) = fenced(dir.path(), true).await;
    assert_eq!(
        lifecycle.regrant_admitted(
            &admitted_in(DB, LATER),
            checkpoint(ScopeState::Granted),
            &revoked
        ),
        Err(LifecycleError::NotFresh)
    );
    still_fenced(&lifecycle, FenceState::Purged);
}

/// Subscriptions still pending (or revoked again): `NotFresh`.
#[tokio::test(flavor = "current_thread")]
async fn an_agent_regrant_refuses_subscriptions_not_granted() {
    for scope in [ScopeState::Pending, ScopeState::Revoked] {
        let dir = tempfile::tempdir().expect("dir");
        let (lifecycle, _revoked, fresh) = fenced(dir.path(), true).await;
        assert_eq!(
            lifecycle.regrant_admitted(&admitted_in(DB, LATER), checkpoint(scope), &fresh),
            Err(LifecycleError::NotFresh),
            "{scope:?}"
        );
        still_fenced(&lifecycle, FenceState::Purged);
    }
}

/// A new instance of another domain: `NotFresh`.
#[tokio::test(flavor = "current_thread")]
async fn an_agent_regrant_refuses_another_domains_instance() {
    let dir = tempfile::tempdir().expect("dir");
    let (lifecycle, _revoked, _fresh) = fenced(dir.path(), true).await;
    let other = reads_of(&dir.path().join("other"), "other");
    assert_eq!(
        lifecycle.regrant_admitted(
            &admitted_in(DB, LATER),
            checkpoint(ScopeState::Granted),
            &other
        ),
        Err(LifecycleError::NotFresh)
    );
    still_fenced(&lifecycle, FenceState::Purged);
}

/// With no fence there is nothing to regrant: `NotFenced`.
#[test]
fn an_agent_regrant_needs_a_fence() {
    let dir = tempfile::tempdir().expect("dir");
    let lifecycle = queued(dir.path());
    let fresh = reads(&dir.path().join("fresh"));
    assert_eq!(
        lifecycle.regrant_admitted(
            &admitted_in(DB, LATER),
            checkpoint(ScopeState::Granted),
            &fresh
        ),
        Err(LifecycleError::NotFenced)
    );
}

/// An admission that expires `ms` from the wall clock now (a real
/// `admit_read` refuses an already-expired row).
fn now_plus(ms: u64) -> u64 {
    u64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock")
            .as_millis(),
    )
    .expect("ms")
        + ms
}

/// Review MEDIUM-1: an admission from before the fence — still unexpired,
/// as one an open reader holds (`AgentReader::admitted()`) — is not fresh
/// authority for a regrant: `NotFresh`. One from the fence's own
/// millisecond is not after it either; one a millisecond later is.
#[tokio::test(flavor = "current_thread")]
async fn an_agent_regrant_refuses_an_admission_from_before_the_fence() {
    let read = admitted_in(DB, LATER);
    let admitted = i64::try_from(read.admitted_at_ms().expect("clock")).expect("ms");
    for (fence, outcome) in [
        (admitted + 60_000, Err(LifecycleError::NotFresh)),
        (admitted, Err(LifecycleError::NotFresh)),
        (admitted - 1, Ok(())),
    ] {
        let dir = tempfile::tempdir().expect("dir");
        let (lifecycle, _revoked, fresh) = fenced_at(dir.path(), true, fence).await;
        assert_eq!(
            lifecycle.regrant_admitted(&read, checkpoint(ScopeState::Granted), &fresh),
            outcome,
            "fenced at admission{:+} ms",
            fence - admitted
        );
        if outcome.is_err() {
            still_fenced(&lifecycle, FenceState::Purged);
        }
    }
}

/// Review LOW-4: the expiry boundary, on an injected clock. An admission
/// expiring at `now` is expired; a millisecond before, it is not. A clock
/// that cannot be read makes nothing fresh.
#[tokio::test(flavor = "current_thread")]
async fn an_agent_regrant_expires_at_its_millisecond_and_without_a_clock() {
    let read = admitted_in(DB, LATER);
    for (now, outcome) in [
        (Some(LATER), Err(LifecycleError::NotFresh)),
        (Some(LATER + 1), Err(LifecycleError::NotFresh)),
        (None, Err(LifecycleError::NotFresh)),
        (Some(LATER - 1), Ok(())),
    ] {
        let dir = tempfile::tempdir().expect("dir");
        let (lifecycle, _revoked, fresh) = fenced(dir.path(), true).await;
        assert_eq!(
            lifecycle.regrant_admitted_at(&read, checkpoint(ScopeState::Granted), &fresh, now),
            outcome,
            "now {now:?}"
        );
    }
}

/// Owner decision 2026-11-12 (H3), a person: a fence learned from a refused
/// credential (a 401 on an attestation that lapsed while offline) is kept
/// with its cause across a restart and clears on fresh authority — a
/// successful `read_status`, granted subscriptions and a new instance —
/// without the replica's purge, which a refused credential never brings.
/// Stale evidence still refuses, and the disabled entry stays disabled
/// (CC-11).
#[tokio::test(flavor = "current_thread")]
async fn a_credential_fence_clears_on_fresh_authority_without_the_purge() {
    let dir = tempfile::tempdir().expect("dir");
    let (lifecycle, revoked, fresh) = fenced_by(dir.path(), 5, FenceCause::Credential).await;
    assert!(revoked.is_revoked());
    assert_eq!(only_entry(dir.path()).1, State::Disabled);
    let lifecycle = restarted(lifecycle, dir.path());
    assert_eq!(lifecycle.fence(), Ok(Some(FenceState::Revoked)));
    assert_eq!(lifecycle.fence_cause(), Ok(Some(FenceCause::Credential)));
    lifecycle.resume(None, 6).await.expect("resumed");
    // No revoke frame: the purge is never observed, and need not be.
    assert_eq!(lifecycle.observe_purge(Revocation::None, 7), Ok(false));
    let (answered, other_domain) = (
        status(),
        ReadStatus {
            domain: "other".to_owned(),
            ..status()
        },
    );
    for (answer, scope, instance) in [
        (&other_domain, ScopeState::Granted, &fresh),
        (&answered, ScopeState::Pending, &fresh),
        (&answered, ScopeState::Granted, &revoked),
    ] {
        assert_eq!(
            lifecycle.regrant(answer, checkpoint(scope), instance),
            Err(LifecycleError::NotFresh)
        );
        still_fenced(&lifecycle, FenceState::Revoked);
    }
    lifecycle
        .regrant(&status(), checkpoint(ScopeState::Granted), &fresh)
        .expect("regranted");
    lifecycle.gate().expect("unlocked");
    assert_eq!(lifecycle.fence(), Ok(None));
    assert_eq!(only_entry(dir.path()).1, State::Disabled);
}

/// H3, an agent: a credential fence clears on an `admit_read` from after
/// it, across a restart, without the purge; an admission from before the
/// fence is no fresh authority. The disabled entry stays disabled.
#[tokio::test(flavor = "current_thread")]
async fn an_agent_credential_fence_clears_on_a_fresh_admission_without_the_purge() {
    let read = admitted_in(DB, LATER);
    let admitted = i64::try_from(read.admitted_at_ms().expect("clock")).expect("ms");
    for (fence, outcome) in [
        (admitted + 60_000, Err(LifecycleError::NotFresh)),
        (admitted - 1, Ok(())),
    ] {
        let dir = tempfile::tempdir().expect("dir");
        let (lifecycle, _revoked, fresh) =
            fenced_by(dir.path(), fence, FenceCause::Credential).await;
        let lifecycle = restarted(lifecycle, dir.path());
        assert_eq!(lifecycle.fence_cause(), Ok(Some(FenceCause::Credential)));
        assert_eq!(
            lifecycle.regrant_admitted(&read, checkpoint(ScopeState::Granted), &fresh),
            outcome,
            "fenced at admission{:+} ms",
            fence - admitted
        );
        if outcome.is_ok() {
            lifecycle.gate().expect("unlocked");
            assert_eq!(lifecycle.fence(), Ok(None));
        } else {
            still_fenced(&lifecycle, FenceState::Revoked);
        }
        assert_eq!(only_entry(dir.path()).1, State::Disabled);
    }
}

/// H3, the inverse: a refused credential, then a real revocation (a
/// revoked subscription) escalates the fence to `revocation`, kept across
/// a restart; a later credential signal never downgrades it. A regrant is
/// then `PurgePending` until the replica's purge is observed, and clears
/// only with fresh authority after it; the disabled entry stays disabled.
#[tokio::test(flavor = "current_thread")]
async fn a_revocation_after_a_credential_refusal_waits_for_the_purge() {
    let dir = tempfile::tempdir().expect("dir");
    let (lifecycle, revoked, fresh) = fenced_by(dir.path(), 5, FenceCause::Credential).await;
    lifecycle
        .revoke(Some(&revoked), 6, FenceCause::Revocation)
        .await
        .expect("escalated");
    assert_eq!(lifecycle.fence_cause(), Ok(Some(FenceCause::Revocation)));
    let lifecycle = restarted(lifecycle, dir.path());
    lifecycle
        .lock(7, FenceCause::Credential)
        .expect("locked again");
    assert_eq!(lifecycle.fence_cause(), Ok(Some(FenceCause::Revocation)));
    assert_eq!(
        lifecycle.regrant(&status(), checkpoint(ScopeState::Granted), &fresh),
        Err(LifecycleError::PurgePending)
    );
    assert_eq!(
        lifecycle.regrant_admitted(
            &admitted_in(DB, LATER),
            checkpoint(ScopeState::Granted),
            &fresh
        ),
        Err(LifecycleError::PurgePending)
    );
    still_fenced(&lifecycle, FenceState::Revoked);
    assert_eq!(lifecycle.observe_purge(Revocation::Purged, 8), Ok(true));
    assert_eq!(
        lifecycle.regrant(&status(), checkpoint(ScopeState::Pending), &fresh),
        Err(LifecycleError::NotFresh)
    );
    lifecycle
        .regrant(&status(), checkpoint(ScopeState::Granted), &fresh)
        .expect("regranted");
    lifecycle.gate().expect("unlocked");
    assert_eq!(only_entry(dir.path()).1, State::Disabled);
}

/// H3: an escalation sets the fence anew, so an agent's admission from
/// between the credential refusal and the revocation is no fresh authority
/// for the revocation, even once purged.
#[tokio::test(flavor = "current_thread")]
async fn an_admission_from_before_the_escalation_is_not_fresh() {
    let read = admitted_in(DB, LATER);
    let admitted = i64::try_from(read.admitted_at_ms().expect("clock")).expect("ms");
    let dir = tempfile::tempdir().expect("dir");
    let (lifecycle, revoked, fresh) =
        fenced_by(dir.path(), admitted - 1, FenceCause::Credential).await;
    lifecycle
        .revoke(Some(&revoked), admitted + 60_000, FenceCause::Revocation)
        .await
        .expect("escalated");
    assert_eq!(
        lifecycle.observe_purge(Revocation::Purged, admitted + 60_001),
        Ok(true)
    );
    assert_eq!(
        lifecycle.regrant_admitted(&read, checkpoint(ScopeState::Granted), &fresh),
        Err(LifecycleError::NotFresh)
    );
    still_fenced(&lifecycle, FenceState::Purged);
}

/// H3 through the one queue policy: a lookup refused for the credential
/// fences from that cause; a further read-level refusal of the locked
/// entry escalates the fence to a revocation.
#[test]
fn a_refused_credential_settles_into_a_credential_fence() {
    let dir = tempfile::tempdir().expect("dir");
    let lifecycle = queued(dir.path());
    let (id, from) = only_entry(dir.path());
    let pending = |cause| GraphOutcome::Pending { cause };
    assert_eq!(
        lifecycle.settle(&id, from, &pending(PendingCause::CredentialRefused), 5),
        Ok(State::Disabled)
    );
    assert_eq!(lifecycle.fence_cause(), Ok(Some(FenceCause::Credential)));
    assert_eq!(only_entry(dir.path()).1, State::Disabled);
    assert_eq!(
        lifecycle.settle(&id, State::Disabled, &pending(PendingCause::Refused), 6),
        Ok(State::Disabled)
    );
    assert_eq!(lifecycle.fence_cause(), Ok(Some(FenceCause::Revocation)));
}

/// Review r1 LOW-1: an admission another agent holds is no fresh authority
/// for this queue's fence: `NotFresh`, and the fence stands.
#[tokio::test(flavor = "current_thread")]
async fn an_agent_regrant_refuses_another_agents_admission() {
    let dir = tempfile::tempdir().expect("dir");
    let (lifecycle, _revoked, fresh) = fenced(dir.path(), true).await;
    assert_eq!(
        lifecycle.regrant_admitted(
            &admitted_as("dixi", DB, LATER),
            checkpoint(ScopeState::Granted),
            &fresh
        ),
        Err(LifecycleError::NotFresh)
    );
    still_fenced(&lifecycle, FenceState::Purged);
    lifecycle
        .regrant_admitted(
            &admitted_in(DB, LATER),
            checkpoint(ScopeState::Granted),
            &fresh,
        )
        .expect("this queue's own agent regrants");
}
