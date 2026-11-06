//! What a graph request's caller may conclude (contract §9.1).
//!
//! A request's identity is its database (the domain), descriptor, request
//! id and client scope; its payload is bound by its digest. Equal ids in two
//! databases are two requests. [`GraphRequests`] is one database's owner of
//! this device's graph requests, built from the [`Source`] that opened the
//! database's replica: the domain comes from that boundary, never from a
//! row or a caller. Only it mints a [`RequestKey`], sends a request (an
//! [`Attempt`]), reads a [`Receipt`] from its own replica and resolves them,
//! so a key, attempt or receipt of another database never speaks for one of
//! its requests.
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
//! | any other refusal, a lost or unread answer | none | [`GraphOutcome::Pending`]: keep the same request id and resolve it later |
//!
//! A receipt is the durable record, so it decides whatever the transport
//! said: an unheard answer whose receipt has since arrived is resolved. An
//! attempt at another request, or in another database, tells nothing.

use serde_json::{json, Map, Value};

use super::protocol::request::{self, is_descriptor, Request};
use super::protocol::scope::{client_scope, receipt_id};
use super::protocol::{is_decimal, is_ulid, value, Refusal, CONTRACT_VERSION};
use crate::hq::port::{CommandResult, ReplicaError};
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
    /// Unresolved: the same request id is retried or looked up later.
    Pending {
        reason: String,
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
    source: Source,
    client_scope: String,
}

impl GraphRequests {
    /// The owner for the database `source` opened; its domain is
    /// `source.database`, its client scope this device's actor and client.
    #[must_use]
    pub fn new(source: Source) -> Self {
        let client_scope = client_scope(&source.actor, &source.client);
        Self {
            source,
            client_scope,
        }
    }

    /// The database these requests belong to.
    #[must_use]
    pub fn domain(&self) -> &str {
        &self.source.database
    }

    /// The replica boundary these requests go through.
    pub(crate) fn source(&self) -> &Source {
        &self.source
    }

    /// The key of `request` sent to `descriptor` from this device, its
    /// digest computed here.
    pub fn key(&self, descriptor: &str, request: &Request) -> Result<RequestKey, Refusal> {
        Ok(RequestKey {
            identity: RequestIdentity {
                domain: self.source.database.clone(),
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
        key.identity.domain == self.source.database
            && key.identity.client_scope == self.client_scope
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
        let result =
            self.source
                .replica
                .remote_command(key.descriptor(), key.request_id(), envelope);
        Ok(Attempt::new(key, result))
    }

    /// The receipt this database's replica holds for `key`, if any.
    pub fn receipt(&self, key: &RequestKey) -> Result<Option<Receipt>, GraphRequestError> {
        if !self.owns(key) {
            return Err(GraphRequestError::ForeignRequest);
        }
        let id = receipt_id(key.descriptor(), key.request_id(), key.client_scope());
        let rows = self.source.replica.query(
            "SELECT * FROM kg_receipts WHERE id = ?",
            &[Value::from(id.as_str())],
        )?;
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
        let rows = self.source.replica.remote_query(
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
            return Receipt::from_row(&self.source.database, &row)
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
                reason: "another database's or client's request".to_owned(),
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
                reason: "no attempt at this request".to_owned(),
            },
            Some(Ok(_)) => match own_commit {
                Some(commit_seq) => GraphOutcome::PendingVisibility { commit_seq },
                None => GraphOutcome::Pending {
                    reason: "acknowledgement of another request".to_owned(),
                },
            },
            Some(Err(ReplicaError::Rejected { code, message })) => terminal(code, message)
                .unwrap_or_else(|| GraphOutcome::TechnicalRefusal { code: code.clone() }),
            Some(Err(ReplicaError::Remote { code, message, .. })) => terminal(code, message)
                .unwrap_or_else(|| {
                    if code.starts_with("kg.") {
                        GraphOutcome::TechnicalRefusal { code: code.clone() }
                    } else if message.starts_with("kg.") {
                        GraphOutcome::TechnicalRefusal {
                            code: message.clone(),
                        }
                    } else {
                        GraphOutcome::Pending {
                            reason: message.clone(),
                        }
                    }
                }),
            Some(Err(error)) => GraphOutcome::Pending {
                reason: error.to_string(),
            },
        }
    }
}
