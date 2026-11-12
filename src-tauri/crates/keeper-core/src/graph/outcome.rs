//! What a graph request's caller may conclude (contract §9.1).
//!
//! A request's identity is its database (the domain), descriptor, request
//! id and client scope; its payload is bound by its digest. Equal ids in two
//! databases are two requests. [`GraphRequests`] is one database's owner of
//! this device's graph requests, built over the database's narrow write
//! port ([`super::write::GraphWrite`]) and its writer: the domain comes from
//! that boundary, never from a row or a caller. Only it mints a
//! [`RequestKey`], sends a request (an [`Attempt`]), reads a [`Receipt`]
//! and resolves them, so a key, attempt or receipt of another database
//! never speaks for one of its requests.
//!
//! | transport | the request's receipt | outcome |
//! | --- | --- | --- |
//! | any | same identity, another digest | [`GraphOutcome::IdempotencyMismatch`] |
//! | any | `accepted` | [`GraphOutcome::Accepted`], with the event and graph sequence the receipt names |
//! | any | `proposed`, `conflict`, `rejected` | exactly that, never accepted |
//! | applied or cached | none visible yet | [`GraphOutcome::PendingVisibility`]: the commit is a visibility bound; pull the receipt or ask `graph.outcome` |
//! | a rejected result ([`ReplicaError::Rejected`]) | none | [`GraphOutcome::TechnicalRefusal`] with its code (or the terminal codes below) |
//! | refused `idempotency_mismatch` | none | [`GraphOutcome::IdempotencyMismatch`] |
//! | refused `unsupported_version` | none | [`GraphOutcome::Unsupported`] |
//! | refused with a `kg.` code | none | [`GraphOutcome::TechnicalRefusal`] |
//! | any other refusal, a lost or unread answer | none | [`GraphOutcome::Pending`] with its typed [`PendingCause`]: keep the same request id and resolve it later |
//!
//! A receipt is the durable record, so it decides whatever the transport
//! said: an unheard answer whose receipt has since arrived is resolved. An
//! attempt at another request, or in another database, tells nothing.
//! [`GraphOutcome::revokes`] says which outcomes are revocation signals.

use std::sync::Arc;

use serde_json::{json, Map, Value};

use super::protocol::request::{self, is_descriptor, Request};
use super::protocol::scope::{client_scope, receipt_id};
use super::protocol::{is_decimal, is_ulid, value, Refusal, CONTRACT_VERSION};
use super::write::{GraphWrite, Writer};
use crate::hq::port::{CommandResult, ErrorClass, ReplicaError};
use crate::hq::sessions::Source;

/// Which request this is: its database, descriptor, request id and client
/// scope, compared only as a whole. Two requests with equal identities are
/// the same request, whatever their payloads.
#[derive(Clone, Debug, PartialEq, Eq)]
struct RequestIdentity {
    domain: String,
    descriptor: String,
    request_id: String,
    client_scope: String,
}

/// One graph request in one database: its identity and the digest that
/// binds its payload. Minted only by that database's [`GraphRequests::key`].
/// Requests are associated only by whole-key equality: the key's parts can
/// be read, never compared one by one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestKey {
    identity: RequestIdentity,
    request_digest: String,
}

impl RequestKey {
    /// The database the request belongs to.
    #[must_use]
    pub fn domain(&self) -> &str {
        &self.identity.domain
    }
    #[must_use]
    pub fn descriptor(&self) -> &str {
        &self.identity.descriptor
    }
    #[must_use]
    pub fn request_id(&self) -> &str {
        &self.identity.request_id
    }
    #[must_use]
    pub fn client_scope(&self) -> &str {
        &self.identity.client_scope
    }
    /// [`request::digest`] of the request.
    #[must_use]
    pub fn request_digest(&self) -> &str {
        &self.request_digest
    }
}

/// One send of a request and what the transport answered, bound to the
/// whole key it was sent under. Made only by [`GraphRequests::submit`].
#[derive(Clone, Debug, PartialEq)]
pub struct Attempt {
    key: RequestKey,
    result: Result<CommandResult, ReplicaError>,
}

impl Attempt {
    pub(crate) fn new(key: &RequestKey, result: Result<CommandResult, ReplicaError>) -> Self {
        Self {
            key: key.clone(),
            result,
        }
    }

    /// The request this attempt sent.
    #[must_use]
    pub fn key(&self) -> &RequestKey {
        &self.key
    }

    /// What the transport answered.
    #[must_use]
    pub fn result(&self) -> &Result<CommandResult, ReplicaError> {
        &self.result
    }
}

/// A receipt's terminal outcome, with what that outcome carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReceiptOutcome {
    Accepted { event_id: String, graph_seq: String },
    Proposed,
    Conflict,
    Rejected { reason: String },
}

/// One `kg_receipts` row of one database, decoded and checked: every value
/// it holds obeys the contract's receipt shape, and its database is the one
/// whose replica held it.
#[derive(Clone, Debug, PartialEq)]
pub struct Receipt {
    key: RequestKey,
    outcome: ReceiptOutcome,
    detail: Map<String, Value>,
}

impl Receipt {
    /// Decode a `kg_receipts` row read from `domain`'s replica. A row that
    /// breaks the receipt shape refuses `unsupported_version` rather than
    /// being guessed at: an id that is not
    /// `<descriptor>/<request_id>/<client_scope>`, an outcome this build does
    /// not know, an accepted receipt without a ULID event and a positive
    /// graph sequence, any other receipt naming either, a rejection without
    /// a reason or a reason on anything else, a `detail` that is not a
    /// contract JSON object (as text, as a replica column holds it, or
    /// decoded).
    pub(crate) fn from_row(domain: &str, row: &Map<String, Value>) -> Result<Self, Refusal> {
        let text = |key: &str| -> Result<Option<String>, Refusal> {
            match row.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(text)) => Ok(Some(text.clone())),
                Some(_) => Err(Refusal::UnsupportedVersion),
            }
        };
        let required = |key: &str| text(key)?.ok_or(Refusal::UnsupportedVersion);
        let key = RequestKey {
            identity: RequestIdentity {
                domain: domain.to_owned(),
                descriptor: required("descriptor")?,
                request_id: required("request_id")?,
                client_scope: required("client_scope")?,
            },
            request_digest: required("request_digest")?,
        };
        if !is_descriptor(key.descriptor())
            || !is_ulid(key.request_id())
            || key.client_scope().is_empty()
            || !is_digest(key.request_digest())
            || required("id")? != receipt_id(key.descriptor(), key.request_id(), key.client_scope())
        {
            return Err(Refusal::UnsupportedVersion);
        }
        let (event_id, graph_seq, reason) =
            (text("event_id")?, text("graph_seq")?, text("reason")?);
        let outcome = match (required("outcome")?.as_str(), event_id, graph_seq, reason) {
            ("accepted", Some(event_id), Some(graph_seq), None)
                if is_ulid(&event_id) && is_decimal(&graph_seq) && graph_seq != "0" =>
            {
                ReceiptOutcome::Accepted {
                    event_id,
                    graph_seq,
                }
            }
            ("proposed", None, None, None) => ReceiptOutcome::Proposed,
            ("conflict", None, None, None) => ReceiptOutcome::Conflict,
            ("rejected", None, None, Some(reason)) if !reason.is_empty() => {
                ReceiptOutcome::Rejected { reason }
            }
            _ => return Err(Refusal::UnsupportedVersion),
        };
        // One path for both representations: text is parsed under the
        // contract's value rules, a decoded value is checked under the same.
        let detail = match row.get("detail") {
            Some(Value::String(json)) => value::parse(json.as_bytes()),
            Some(decoded) => value::normalize(decoded),
            None => Err(Refusal::UnsupportedVersion),
        };
        let Ok(Value::Object(detail)) = detail else {
            return Err(Refusal::UnsupportedVersion);
        };
        Ok(Self {
            key,
            outcome,
            detail,
        })
    }

    /// The request this receipt answers, in the database that holds it.
    #[must_use]
    pub fn key(&self) -> &RequestKey {
        &self.key
    }

    #[must_use]
    pub fn outcome(&self) -> &ReceiptOutcome {
        &self.outcome
    }

    /// Bounded safe detail: affected ids and revisions, a conflict's current
    /// revisions, a proposal's id.
    #[must_use]
    pub fn detail(&self) -> &Map<String, Value> {
        &self.detail
    }
}

fn is_digest(digest: &str) -> bool {
    digest.len() == 64
        && digest
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

/// What a caller may conclude about one graph request.
#[derive(Clone, Debug, PartialEq)]
pub enum GraphOutcome {
    Accepted {
        event_id: String,
        graph_seq: String,
        /// The command's commit when this request's own attempt named it.
        commit_seq: Option<i64>,
        detail: Map<String, Value>,
    },
    /// A private proposal: nothing accepted, no graph sequence.
    Proposed {
        detail: Map<String, Value>,
    },
    Conflict {
        detail: Map<String, Value>,
    },
    Rejected {
        reason: String,
        detail: Map<String, Value>,
    },
    /// The server answered, and the receipt has not reached this replica.
    PendingVisibility {
        commit_seq: Option<i64>,
    },
    /// Unresolved: the same request id is retried or looked up later, as
    /// its [`PendingCause`] says.
    Pending {
        cause: PendingCause,
    },
    /// The request id is bound to a different request.
    IdempotencyMismatch,
    /// The server does not implement this request's contract version.
    Unsupported,
    /// The command ran and its commit was refused, or the server's invariant
    /// backstop refused it: terminal for this request id.
    TechnicalRefusal {
        code: String,
    },
}

impl GraphOutcome {
    /// The contract's stable error code (§7) a caller reports for this
    /// outcome of `key`'s request, when it has one: a conflict of a
    /// `graph.reverse` request is `reversal_conflict` and any other
    /// conflict `revision_conflict` (a conflict receipt carries no reason;
    /// its descriptor tells them apart, CC-X5); a rejection its reason
    /// (`proposal_stale` for a stale approval); a mismatch, an unsupported
    /// version and a technical refusal their codes. Accepted, proposed and
    /// unresolved outcomes have none.
    #[must_use]
    pub fn code(&self, key: &RequestKey) -> Option<&str> {
        match self {
            Self::Conflict { .. } if key.descriptor() == super::protocol::inverse::REVERSE => {
                Some("reversal_conflict")
            }
            Self::Conflict { .. } => Some("revision_conflict"),
            Self::Rejected { reason, .. } => Some(reason),
            Self::IdempotencyMismatch => Some("idempotency_mismatch"),
            Self::Unsupported => Some("unsupported_version"),
            Self::TechnicalRefusal { code } => Some(code),
            Self::Accepted { .. }
            | Self::Proposed { .. }
            | Self::PendingVisibility { .. }
            | Self::Pending { .. } => None,
        }
    }

    /// Whether this outcome is a revocation signal for the domain: only a
    /// refused read-level call ([`PendingCause::Refused`]: a refused
    /// `graph.read_status` or `graph.outcome`, or a refused credential or
    /// scope). A `forbidden` receipt or a command refused at send ends that
    /// one request, whatever its code, and fences nothing (owner decision
    /// 2026-11-12, amending CC-7: per-request refusals are CC-14's class).
    #[must_use]
    pub fn revokes(&self) -> bool {
        matches!(
            self,
            Self::Pending {
                cause: PendingCause::Refused
            }
        )
    }
}

/// The `forbidden` codes that end one request because of the request
/// itself or its context: an agent's approval that does not bind (the
/// server's `approvalProblem`) or was already used, a person's change that
/// names an approval, and an agent context that ended or was unbound. Like
/// every `forbidden` receipt, never a revocation signal ([`GraphOutcome::revokes`]).
pub const REQUEST_REFUSALS: [&str; 11] = [
    "approval_required",
    "approval_binding",
    "approval_unavailable",
    "approval_used",
    "tool_off",
    "approval_not_applicable",
    "context_expired",
    "context_closed",
    "context_invalid",
    "parent_unavailable",
    "context_unbound",
];

/// Why a request is still [`GraphOutcome::Pending`] (112.1 CC-5). The queue
/// policy keys on this, never on text.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PendingCause {
    /// No answer this device can trust, or one that says nothing of the
    /// caller (a failed handler, an internal or storage error, an unknown
    /// code): the identical request is retried under its own id after a
    /// back-off.
    Transport,
    /// The graph is switched off on the server (`graph_unavailable`), or
    /// the server lacks the operation (`operation.unknown`): back off, then
    /// retry the identical request.
    Unavailable,
    /// The server cannot say what became of it (`outcome_unresolved`):
    /// only a same-id replay or `graph.outcome`, never a new id.
    Unresolved,
    /// The agent context the request names ended or was unbound
    /// ([`crate::hq::port::CONTEXT_ENDED`]).
    ContextEnded,
    /// The server refused a lookup as it is (an invalid request, an answer
    /// over its limit): the entry stays as it is, nothing is resent.
    Invalid,
    /// The server refused the caller at read level (a refused credential
    /// or scope, or `graph.outcome`/`graph.read_status` refused): a
    /// revocation signal.
    Refused,
    /// Nothing was sent and no receipt is known: look the receipt up.
    NoAttempt,
    /// The key is not this owner's (another database or client): never
    /// sent under this identity.
    Foreign,
}

impl PendingCause {
    /// The stable, domain-safe name.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Transport => "transport",
            Self::Unavailable => "graph_unavailable",
            Self::Unresolved => "outcome_unresolved",
            Self::ContextEnded => "context_ended",
            Self::Invalid => "invalid_request",
            Self::Refused => "refused",
            Self::NoAttempt => "no_attempt",
            Self::Foreign => "foreign_request",
        }
    }

    /// The cause a failed read-level call (a lookup, an authority read)
    /// gives, through the one classifier ([`ReplicaError::class`]) the read
    /// path uses too. Only a refusal of the caller is
    /// [`PendingCause::Refused`]; whatever says nothing of the caller,
    /// retryable or not, is never one.
    #[must_use]
    pub fn of(error: &ReplicaError) -> Self {
        match error.class() {
            ErrorClass::Revoked | ErrorClass::Forbidden => Self::Refused,
            ErrorClass::Unavailable => Self::Unavailable,
            ErrorClass::Unresolved => Self::Unresolved,
            ErrorClass::ContextEnded => Self::ContextEnded,
            ErrorClass::Invalid => Self::Invalid,
            ErrorClass::Transient | ErrorClass::Unanswered => Self::Transport,
        }
    }
}

impl GraphOutcome {
    /// This outcome's pending cause, when it is pending.
    #[must_use]
    pub fn pending_cause(&self) -> Option<PendingCause> {
        match self {
            Self::Pending { cause } => Some(*cause),
            _ => None,
        }
    }
}

/// Why a send or a receipt lookup did not happen.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum GraphRequestError {
    /// The key is another database's, or does not describe this request.
    #[error("not this database's request")]
    ForeignRequest,
    /// The replica could not be read.
    #[error(transparent)]
    Replica(#[from] ReplicaError),
    /// The receipt this database holds breaks the receipt shape.
    #[error("unreadable receipt: {0}")]
    Unreadable(Refusal),
}

/// This device's graph requests in one database.
pub struct GraphRequests {
    port: Arc<dyn GraphWrite>,
    domain: String,
    client_scope: String,
}

impl GraphRequests {
    /// The owner for the database `source` opened; its domain is
    /// `source.database`, its client scope this device's actor and client.
    #[must_use]
    pub fn new(source: Source) -> Self {
        let writer = Writer::of(&source);
        Self::over(Arc::new(source), &writer)
    }

    /// The owner for `writer`'s database behind `port`; its client scope
    /// is `writer`'s actor and client.
    #[must_use]
    pub fn over(port: Arc<dyn GraphWrite>, writer: &Writer) -> Self {
        Self {
            port,
            domain: writer.domain.clone(),
            client_scope: client_scope(&writer.actor, &writer.client),
        }
    }

    /// The database these requests belong to.
    #[must_use]
    pub fn domain(&self) -> &str {
        &self.domain
    }

    /// The write boundary these requests go through.
    pub(crate) fn port(&self) -> &dyn GraphWrite {
        self.port.as_ref()
    }

    /// The key of `request` sent to `descriptor` from this device, its
    /// digest computed here.
    pub fn key(&self, descriptor: &str, request: &Request) -> Result<RequestKey, Refusal> {
        Ok(RequestKey {
            identity: RequestIdentity {
                domain: self.domain.clone(),
                descriptor: descriptor.to_owned(),
                request_id: request.request_id.clone(),
                client_scope: self.client_scope.clone(),
            },
            request_digest: request::digest(descriptor, request)?,
        })
    }

    /// Whether `key` is one this owner mints: its database and this
    /// device's client scope. Ownership, not association: two requests are
    /// associated only by whole-key equality.
    fn owns(&self, key: &RequestKey) -> bool {
        key.identity.domain == self.domain && key.identity.client_scope == self.client_scope
    }

    /// Send `request` under `key` through this database's replica. The key
    /// must be this database's key of exactly this request.
    pub fn submit(
        &self,
        key: &RequestKey,
        request: &Request,
    ) -> Result<Attempt, GraphRequestError> {
        if self.key(key.descriptor(), request).ok().as_ref() != Some(key) {
            return Err(GraphRequestError::ForeignRequest);
        }
        let envelope = json!({
            "version": CONTRACT_VERSION,
            "request_id": request.request_id,
            "context_id": request.context_id,
            "payload": request.payload,
        });
        let result = self
            .port
            .command(key.descriptor(), key.request_id(), envelope);
        Ok(Attempt::new(key, result))
    }

    /// The receipt this database's replica holds for `key`, if any.
    pub fn receipt(&self, key: &RequestKey) -> Result<Option<Receipt>, GraphRequestError> {
        if !self.owns(key) {
            return Err(GraphRequestError::ForeignRequest);
        }
        let id = receipt_id(key.descriptor(), key.request_id(), key.client_scope());
        let rows = self.port.receipt(&id)?;
        self.own_receipt(&id, rows)
    }

    /// The receipt the server holds for `key`, asked through `graph.outcome`
    /// (fresh authorisation first, on the server): for a request whose
    /// receipt has not replicated, or after a restart. `context_id` is the
    /// request's (null for a person's device).
    pub fn remote_receipt(
        &self,
        key: &RequestKey,
        context_id: Option<&str>,
    ) -> Result<Option<Receipt>, GraphRequestError> {
        if !self.owns(key) {
            return Err(GraphRequestError::ForeignRequest);
        }
        let id = receipt_id(key.descriptor(), key.request_id(), key.client_scope());
        let rows = self.port.remote_query(
            super::protocol::wire::OUTCOME,
            super::protocol::wire::outcome_params(context_id, key.request_id()),
        )?;
        self.own_receipt(&id, rows)
    }

    /// The row whose id is `id` among `rows`, decoded under this database;
    /// every other row is ignored.
    fn own_receipt(
        &self,
        id: &str,
        rows: Vec<Map<String, Value>>,
    ) -> Result<Option<Receipt>, GraphRequestError> {
        for row in rows {
            if row.get("id").and_then(Value::as_str) != Some(id) {
                continue;
            }
            return Receipt::from_row(&self.domain, &row)
                .map(Some)
                .map_err(GraphRequestError::Unreadable);
        }
        Ok(None)
    }

    /// Resolve `key` from one `attempt` and the `receipt` this database
    /// holds, if any. Only a key this owner mints resolves. An attempt
    /// speaks for the request only when it was sent under the whole same
    /// key, and a receipt only when its identity is the request's (with
    /// another digest it is `idempotency_mismatch`, never an outcome).
    #[must_use]
    pub fn resolve(
        &self,
        key: &RequestKey,
        attempt: &Attempt,
        receipt: Option<&Receipt>,
    ) -> GraphOutcome {
        self.decide(key, Some(attempt), receipt)
    }

    /// Resolve `key` from its `receipt` alone, with no attempt in hand (a
    /// request sent before a restart): the receipt decides, and without one
    /// the request stays pending under its id.
    #[must_use]
    pub fn settle(&self, key: &RequestKey, receipt: Option<&Receipt>) -> GraphOutcome {
        self.decide(key, None, receipt)
    }

    fn decide(
        &self,
        key: &RequestKey,
        attempt: Option<&Attempt>,
        receipt: Option<&Receipt>,
    ) -> GraphOutcome {
        if !self.owns(key) {
            return GraphOutcome::Pending {
                cause: PendingCause::Foreign,
            };
        }
        let attempt = attempt
            .filter(|attempt| attempt.key == *key)
            .map(|attempt| &attempt.result);
        // The acknowledgement must name the request it was sent as.
        let own_commit = match attempt {
            Some(Ok(result)) if result.request_id == key.request_id() => Some(result.commit_seq),
            _ => None,
        };
        if let Some(receipt) = receipt.filter(|receipt| receipt.key.identity == key.identity) {
            if receipt.key != *key {
                return GraphOutcome::IdempotencyMismatch;
            }
            let detail = receipt.detail.clone();
            return match &receipt.outcome {
                ReceiptOutcome::Accepted {
                    event_id,
                    graph_seq,
                } => GraphOutcome::Accepted {
                    event_id: event_id.clone(),
                    graph_seq: graph_seq.clone(),
                    commit_seq: own_commit.flatten(),
                    detail,
                },
                ReceiptOutcome::Proposed => GraphOutcome::Proposed { detail },
                ReceiptOutcome::Conflict => GraphOutcome::Conflict { detail },
                ReceiptOutcome::Rejected { reason } => GraphOutcome::Rejected {
                    reason: reason.clone(),
                    detail,
                },
            };
        }
        let terminal = |code: &str, message: &str| {
            if code == "idempotency_mismatch" || message == "idempotency_mismatch" {
                Some(GraphOutcome::IdempotencyMismatch)
            } else if code == "unsupported_version" || message == "unsupported_version" {
                Some(GraphOutcome::Unsupported)
            } else {
                None
            }
        };
        match attempt {
            None => GraphOutcome::Pending {
                cause: PendingCause::NoAttempt,
            },
            Some(Ok(_)) => match own_commit {
                Some(commit_seq) => GraphOutcome::PendingVisibility { commit_seq },
                // An acknowledgement of another id says nothing of this one:
                // the same request is asked again.
                None => GraphOutcome::Pending {
                    cause: PendingCause::Transport,
                },
            },
            Some(Err(ReplicaError::Rejected { code, message })) => terminal(code, message)
                .unwrap_or_else(|| GraphOutcome::TechnicalRefusal { code: code.clone() }),
            Some(Err(error @ ReplicaError::Remote { code, message, .. })) => {
                terminal(code, message).unwrap_or_else(|| {
                    // A named code is the code; under `operation.forbidden`
                    // the server names it in the message.
                    let named = if code == "operation.forbidden" {
                        message
                    } else {
                        code
                    };
                    if code.starts_with("kg.") {
                        GraphOutcome::TechnicalRefusal { code: code.clone() }
                    } else if message.starts_with("kg.") {
                        GraphOutcome::TechnicalRefusal {
                            code: message.clone(),
                        }
                    } else {
                        match error.class() {
                            // The command refused at send: a terminal refusal
                            // of this request, never the domain's (owner
                            // decision 2026-11-12, amending CC-7).
                            ErrorClass::Forbidden => GraphOutcome::Rejected {
                                reason: "forbidden".to_owned(),
                                detail: [("code".to_owned(), Value::from(named.clone()))]
                                    .into_iter()
                                    .collect(),
                            },
                            ErrorClass::Invalid => GraphOutcome::TechnicalRefusal {
                                code: named.clone(),
                            },
                            _ => GraphOutcome::Pending {
                                cause: PendingCause::of(error),
                            },
                        }
                    }
                })
            }
            Some(Err(error)) => GraphOutcome::Pending {
                cause: PendingCause::of(error),
            },
        }
    }
}
