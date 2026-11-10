//! The narrow write port (112.1 CC-1), the typed pending causes (CC-5) and
//! the refusal tail of `GraphClient::outcome` (CC-6), over a scripted port:
//! nothing is sent anywhere.

use std::sync::{Arc, Mutex, PoisonError};

use serde_json::{json, Map, Value};

use super::client::{GraphClient, GraphClientError};
use super::outcome::{GraphOutcome, PendingCause, REQUEST_REFUSALS};
use super::protocol::change::{Change, ChangeOp, DESCRIPTOR};
use super::protocol::request::Request;
use super::protocol::Refusal;
use super::queue::{next, Next, State};
use super::write::{Caller, GraphWrite, Writer};
use crate::hq::port::{CommandResult, LinkState, ReplicaError};
use crate::notes::tags::normalise;

/// A port whose every remote answer is scripted; it records what it sent.
struct Scripted {
    link: LinkState,
    command: Result<CommandResult, ReplicaError>,
    outcome: Result<Vec<Map<String, Value>>, ReplicaError>,
    sent: Mutex<Vec<(String, Value)>>,
    queried: Mutex<Vec<String>>,
}

impl Scripted {
    fn new(
        command: Result<CommandResult, ReplicaError>,
        outcome: Result<Vec<Map<String, Value>>, ReplicaError>,
    ) -> Arc<Self> {
        Arc::new(Self {
            link: LinkState::Connected,
            command,
            outcome,
            sent: Mutex::new(Vec::new()),
            queried: Mutex::new(Vec::new()),
        })
    }
}

impl GraphWrite for Scripted {
    fn command(
        &self,
        descriptor: &str,
        request_id: &str,
        envelope: Value,
    ) -> Result<CommandResult, ReplicaError> {
        self.sent
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push((descriptor.to_owned(), envelope));
        self.command.clone().map(|mut result| {
            result.request_id = request_id.to_owned();
            result
        })
    }
    fn receipt(&self, _: &str) -> Result<Vec<Map<String, Value>>, ReplicaError> {
        Ok(Vec::new())
    }
    fn remote_query(
        &self,
        operation: &str,
        _: Value,
    ) -> Result<Vec<Map<String, Value>>, ReplicaError> {
        self.queried
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(operation.to_owned());
        self.outcome.clone()
    }
    fn link(&self) -> LinkState {
        self.link
    }
}

fn writer() -> Writer {
    Writer {
        domain: "work".to_owned(),
        actor: "agent-1".to_owned(),
        client: "inst-1".to_owned(),
    }
}

fn ops() -> Vec<ChangeOp> {
    vec![ChangeOp::CreateNode {
        id: "01JF0000000000000000000001".to_owned(),
        kind: "Concept".into(),
        title: "N".into(),
        description: None,
        properties: Map::new(),
    }]
}

fn approved_change(approval: Option<&str>) -> Change {
    Change::new(ops(), approval.map(str::to_owned), None, normalise).expect("change")
}

fn agent(port: Arc<Scripted>) -> GraphClient {
    GraphClient::over(
        port,
        &writer(),
        Caller::Agent {
            context_id: "01JF00000000000000000000C1".to_owned(),
        },
    )
}

fn person(port: Arc<Scripted>) -> GraphClient {
    GraphClient::over(port, &writer(), Caller::Person)
}

fn remote(code: &str, message: &str, retryable: bool) -> ReplicaError {
    ReplicaError::Remote {
        code: code.to_owned(),
        message: message.to_owned(),
        retryable,
    }
}

fn applied() -> Result<CommandResult, ReplicaError> {
    Ok(CommandResult {
        request_id: String::new(),
        commit_seq: Some(4),
        cached: false,
    })
}

/// CC-5: every server refusal code maps to its cause, a refusal the server
/// does not call retryable is `Refused` (fail closed), and every failure
/// that is not the server's answer is `Transport`.
#[test]
fn every_failure_has_its_typed_cause() {
    let refusal = |message: &str| remote("operation.forbidden", message, false);
    let cases = [
        (refusal("graph_unavailable"), PendingCause::Unavailable),
        (refusal("outcome_unresolved"), PendingCause::Unresolved),
        (refusal("context_expired"), PendingCause::ContextEnded),
        (refusal("context_closed"), PendingCause::ContextEnded),
        (refusal("context_invalid"), PendingCause::ContextEnded),
        (refusal("forbidden"), PendingCause::Refused),
        (refusal("context_revoked"), PendingCause::Refused),
        (refusal("source_audience"), PendingCause::Refused),
        (refusal("label_unknown"), PendingCause::Refused),
        (refusal("something_new"), PendingCause::Refused),
        (remote("forbidden", "", true), PendingCause::Refused),
        (remote("sync.busy", "later", true), PendingCause::Transport),
        (
            ReplicaError::Unheard {
                code: "transport".into(),
                message: "reset".into(),
                retryable: true,
            },
            PendingCause::Transport,
        ),
        (ReplicaError::Client("x".into()), PendingCause::Transport),
        (ReplicaError::NotSyncing, PendingCause::Transport),
        (ReplicaError::Closed, PendingCause::Transport),
    ];
    for (error, cause) in cases {
        assert_eq!(PendingCause::of(&error), cause, "{error:?}");
    }
}

/// CC-7: only a refusal or a forbidden receipt outside the request's own
/// terminal refusals revokes; nothing else does.
#[test]
fn only_a_refusal_revokes() {
    let forbidden = |code: Option<&str>| GraphOutcome::Rejected {
        reason: "forbidden".into(),
        detail: code
            .map(|c| json!({"code": c}).as_object().cloned().unwrap_or_default())
            .unwrap_or_default(),
    };
    assert!(GraphOutcome::Pending {
        cause: PendingCause::Refused
    }
    .revokes());
    assert!(forbidden(None).revokes());
    assert!(forbidden(Some("forbidden")).revokes());
    assert!(forbidden(Some("an_unknown_code")).revokes());
    for code in REQUEST_REFUSALS {
        assert!(!forbidden(Some(code)).revokes(), "{code}");
    }
    for cause in [
        PendingCause::Transport,
        PendingCause::Unavailable,
        PendingCause::Unresolved,
        PendingCause::ContextEnded,
        PendingCause::NoAttempt,
        PendingCause::Foreign,
    ] {
        assert!(!GraphOutcome::Pending { cause }.revokes(), "{cause:?}");
    }
    let other = GraphOutcome::Rejected {
        reason: "invalid_change".into(),
        detail: Map::new(),
    };
    assert!(!other.revokes());
    assert!(!GraphOutcome::Conflict { detail: Map::new() }.revokes());
}

/// CC-5: the queue policy, keyed on the outcome and its cause.
#[test]
fn the_queue_policy_follows_the_cause() {
    let pending = |cause| GraphOutcome::Pending { cause };
    assert_eq!(
        next(State::Submitting, &pending(PendingCause::Transport)),
        Next::Retry(PendingCause::Transport)
    );
    assert_eq!(
        next(State::Submitting, &pending(PendingCause::Unavailable)),
        Next::Retry(PendingCause::Unavailable)
    );
    assert_eq!(
        next(State::Submitting, &pending(PendingCause::Unresolved)),
        Next::Mark(State::Unresolved)
    );
    for never_sent in [State::Queued, State::AwaitingApproval] {
        assert_eq!(
            next(never_sent, &pending(PendingCause::ContextEnded)),
            Next::RefuseLocally
        );
    }
    for maybe_sent in [
        State::Submitting,
        State::PendingVisibility,
        State::Unresolved,
    ] {
        assert_eq!(
            next(maybe_sent, &pending(PendingCause::ContextEnded)),
            Next::Mark(State::Unresolved),
            "{maybe_sent:?}"
        );
    }
    assert_eq!(
        next(State::Submitting, &pending(PendingCause::Refused)),
        Next::Revoke
    );
    assert_eq!(
        next(State::Unresolved, &pending(PendingCause::NoAttempt)),
        Next::Lookup
    );
    assert_eq!(
        next(State::Queued, &pending(PendingCause::Foreign)),
        Next::Mark(State::Suspended)
    );
    assert_eq!(
        next(
            State::Submitting,
            &GraphOutcome::PendingVisibility {
                commit_seq: Some(3)
            }
        ),
        Next::Mark(State::PendingVisibility)
    );
    let Next::Resolve { to, revokes, .. } = next(
        State::Submitting,
        &GraphOutcome::Rejected {
            reason: "forbidden".into(),
            detail: Map::new(),
        },
    ) else {
        panic!("a receipt resolves");
    };
    assert_eq!((to, revokes), (State::Rejected, true));
    let Next::Resolve {
        to,
        resolution,
        revokes,
    } = next(
        State::Submitting,
        &GraphOutcome::Rejected {
            reason: "forbidden".into(),
            detail: json!({"code": "approval_required"})
                .as_object()
                .cloned()
                .unwrap_or_default(),
        },
    )
    else {
        panic!("a receipt resolves");
    };
    assert_eq!(
        (to, resolution.code.as_deref(), revokes),
        (State::Rejected, Some("approval_required"), false)
    );
}

/// CC-6: a `graph.outcome` the server refuses is that refusal, never a
/// quiet pending, whatever the attempt said; one that could not be asked
/// leaves the attempt to speak.
#[test]
fn a_refused_outcome_lookup_is_never_swallowed() {
    let refused = Scripted::new(
        applied(),
        Err(remote("operation.forbidden", "forbidden", false)),
    );
    let client = person(Arc::clone(&refused));
    let prepared = client.mint(&approved_change(None)).expect("minted");
    let attempt = client.send(&prepared).expect("sent");
    assert_eq!(
        client.outcome(&prepared, Some(&attempt)),
        Ok(GraphOutcome::Pending {
            cause: PendingCause::Refused
        })
    );
    assert_eq!(
        client.outcome(&prepared, None),
        Ok(GraphOutcome::Pending {
            cause: PendingCause::Refused
        })
    );
    // Both answers asked `graph.outcome`; its refusal won over the attempt
    // the transport applied.
    assert_eq!(
        refused
            .queried
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .as_slice(),
        [
            super::protocol::wire::OUTCOME,
            super::protocol::wire::OUTCOME
        ]
    );
    let ended = Scripted::new(
        applied(),
        Err(remote("operation.forbidden", "context_expired", false)),
    );
    let client = person(ended);
    assert_eq!(
        client.outcome(&prepared, None),
        Ok(GraphOutcome::Pending {
            cause: PendingCause::ContextEnded
        })
    );
    // Unreachable: the attempt's own answer stands.
    let unheard = Scripted::new(
        applied(),
        Err(ReplicaError::Unheard {
            code: "transport".into(),
            message: "reset".into(),
            retryable: true,
        }),
    );
    let client = person(unheard);
    let prepared = client.mint(&approved_change(None)).expect("minted");
    let attempt = client.send(&prepared).expect("sent");
    assert_eq!(
        client.outcome(&prepared, Some(&attempt)),
        Ok(GraphOutcome::PendingVisibility {
            commit_seq: Some(4)
        })
    );
    assert_eq!(
        client.outcome(&prepared, None),
        Ok(GraphOutcome::Pending {
            cause: PendingCause::NoAttempt
        })
    );
}

/// CC-1/CC-14: an agent client sends only a `graph.apply` change that names
/// its approval, always under its own context; a person never names a
/// context. A request bound to another context, or none, is refused at
/// restore and never sent.
#[test]
fn a_caller_sends_only_under_its_own_authority() {
    let port = Scripted::new(applied(), Ok(Vec::new()));
    let agent = agent(Arc::clone(&port));
    assert_eq!(
        agent.mint(&approved_change(None)),
        Err(Refusal::InvalidChange),
        "an agent change without its approval"
    );
    let prepared = agent
        .mint(&approved_change(Some("01JF00000000000000000000A1")))
        .expect("minted");
    assert_eq!(
        prepared.request().context_id.as_deref(),
        Some("01JF00000000000000000000C1")
    );
    let as_other = Request {
        context_id: Some("01JF00000000000000000000C2".to_owned()),
        ..prepared.request().clone()
    };
    assert_eq!(
        agent.prepared(DESCRIPTOR, as_other.clone()),
        Err(Refusal::InvalidChange)
    );
    let without = Request {
        context_id: None,
        ..prepared.request().clone()
    };
    assert_eq!(
        agent.prepared(DESCRIPTOR, without.clone()),
        Err(Refusal::InvalidChange)
    );
    for descriptor in [
        super::protocol::proposal::PROPOSE,
        super::protocol::proposal::DECIDE,
        super::protocol::review::ACKNOWLEDGE,
        super::protocol::inverse::REVERSE,
    ] {
        assert_eq!(
            agent.prepared(descriptor, prepared.request().clone()),
            Err(Refusal::InvalidChange),
            "{descriptor}"
        );
    }
    let person = person(Arc::clone(&port));
    assert_eq!(
        person.prepared(DESCRIPTOR, prepared.request().clone()),
        Err(Refusal::InvalidChange),
        "a person never sends an agent's context"
    );
    assert_eq!(
        agent.get_node("x", false),
        Err(GraphClientError::PersonOnly)
    );
    // The agent's request goes out under its own context, once.
    agent.send(&prepared).expect("sent");
    let sent = port
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .clone();
    assert_eq!(sent.len(), 1);
    assert_eq!(sent[0].0, DESCRIPTOR);
    assert_eq!(sent[0].1["context_id"], json!("01JF00000000000000000000C1"));
    // Offline, nothing is sent.
    let offline = Arc::new(Scripted {
        link: LinkState::Offline,
        ..Arc::into_inner(Scripted::new(applied(), Ok(Vec::new()))).expect("one owner")
    });
    let agent = self::agent(Arc::clone(&offline));
    assert_eq!(agent.send(&prepared), Err(GraphClientError::Offline));
    assert!(offline
        .sent
        .lock()
        .unwrap_or_else(PoisonError::into_inner)
        .is_empty());
}

/// CC-7/CC-8: a revocation is a signal as soon as one graph, receipt or
/// proposal subscription reads revoked, and purged only once every one of
/// the subscribed tables has a subscription and all of them read revoked.
#[test]
fn a_purge_needs_every_subscribed_table_revoked() {
    use super::client::subscribed_tables;
    use super::source::{revocation_of, Revocation};
    let tables = subscribed_tables();
    assert!(tables.contains(&"kg_receipts"));
    assert!(tables.contains(&"kg_contents"));
    assert!(tables.contains(&"kg_reviews"));
    assert!(tables.contains(&"kg_proposals"));
    let row = |table: &str, status: &str| {
        json!({"tbl": table, "status": status})
            .as_object()
            .cloned()
            .unwrap_or_default()
    };
    let all = |status: &str| -> Vec<Map<String, Value>> {
        tables.iter().map(|t| row(t, status)).collect()
    };
    assert_eq!(revocation_of(&tables, &all("active")), Revocation::None);
    assert_eq!(revocation_of(&tables, &all("revoked")), Revocation::Purged);
    for (n, table) in tables.iter().enumerate() {
        // One table still active: its rows may be on disk.
        let mut rows = all("revoked");
        rows[n] = row(table, "active");
        assert_eq!(
            revocation_of(&tables, &rows),
            Revocation::Partial,
            "{table}"
        );
        // One table with no subscription at all: not purged either.
        let mut rows = all("revoked");
        rows.remove(n);
        assert_eq!(
            revocation_of(&tables, &rows),
            Revocation::Partial,
            "{table}"
        );
        // Only that table revoked: already a signal.
        let mut rows = all("active");
        rows[n] = row(table, "revoked");
        assert_eq!(
            revocation_of(&tables, &rows),
            Revocation::Partial,
            "{table}"
        );
    }
    assert_eq!(revocation_of(&tables, &[]), Revocation::None);
}
