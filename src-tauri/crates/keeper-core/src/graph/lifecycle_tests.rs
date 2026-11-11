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
use crate::graph::protocol::wire::{self, Authority};
use crate::graph::queue::{Binding, Restored, State};
use crate::graph::runtime::{Runtime, VerifiedWorker};
use crate::graph::source::{GraphSource, PrefixBatch, Revocation, SourceError};
use crate::graph::write::{Caller, GraphWrite, Writer};
use crate::hq::port::fake::FakeOpener;
use crate::hq::port::{CommandResult, LinkState, Opener, ReplicaError, ReplicaSpec};
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
        assert_eq!(lifecycle.gate(), Err(QueryError::Forbidden));
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

    // A refused replay with the server's authority unreachable: the
    // observed refusal still fences, the cached outcome stays unused.
    let dir = tempfile::tempdir().expect("dir");
    let (lifecycle, settled, _) =
        resume_cached(dir.path(), Some(Err(forbidden_error())), Err(unheard()));
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
