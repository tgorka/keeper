//! What a graph request's caller may conclude (contract §9.1).
//!
//! A request is identified by its [`RequestKey`]: descriptor, request id
//! and client scope, plus the digest of its payload. Only a receipt with
//! that exact identity speaks for it, and only an acknowledgement naming
//! its request id lends it a commit.
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
//! | any other refusal, a lost or unread answer, an acknowledgement of another request | none | [`GraphOutcome::Pending`]: keep the same request id and resolve it later |
//!
//! A receipt is the durable record, so it decides whatever the transport
//! said: an unheard answer whose receipt has since arrived is resolved.

use serde_json::{Map, Value};

use super::protocol::request::is_descriptor;
use super::protocol::scope::receipt_id;
use super::protocol::{is_decimal, is_ulid, value, Refusal};
use crate::hq::port::{CommandResult, ReplicaError};

/// The identity of one graph request.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RequestKey {
    pub descriptor: String,
    pub request_id: String,
    /// [`super::protocol::scope::client_scope`] of the sending actor and client.
    pub client_scope: String,
    /// [`super::protocol::request::digest`] of the request.
    pub request_digest: String,
}

/// A receipt's terminal outcome, with what that outcome carries.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ReceiptOutcome {
    Accepted { event_id: String, graph_seq: String },
    Proposed,
    Conflict,
    Rejected { reason: String },
}

/// One `kg_receipts` row, decoded and checked: every value it holds obeys
/// the contract's receipt shape.
#[derive(Clone, Debug, PartialEq)]
pub struct Receipt {
    key: RequestKey,
    outcome: ReceiptOutcome,
    detail: Map<String, Value>,
}

impl Receipt {
    /// Decode a replica row of `kg_receipts`. A row that breaks the receipt
    /// shape refuses `unsupported_version` rather than being guessed at: an
    /// id that is not `<descriptor>/<request_id>/<client_scope>`, an outcome
    /// this build does not know, an accepted receipt without a ULID event
    /// and a positive graph sequence, any other receipt naming either, a
    /// rejection without a reason or a reason on anything else, a `detail`
    /// that is not a contract JSON object (as text, as a replica column
    /// holds it, or decoded).
    pub fn from_row(row: &Map<String, Value>) -> Result<Self, Refusal> {
        let text = |key: &str| -> Result<Option<String>, Refusal> {
            match row.get(key) {
                None | Some(Value::Null) => Ok(None),
                Some(Value::String(text)) => Ok(Some(text.clone())),
                Some(_) => Err(Refusal::UnsupportedVersion),
            }
        };
        let required = |key: &str| text(key)?.ok_or(Refusal::UnsupportedVersion);
        let key = RequestKey {
            descriptor: required("descriptor")?,
            request_id: required("request_id")?,
            client_scope: required("client_scope")?,
            request_digest: required("request_digest")?,
        };
        let digest_shape = key.request_digest.len() == 64
            && key
                .request_digest
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b));
        if !is_descriptor(&key.descriptor)
            || !is_ulid(&key.request_id)
            || key.client_scope.is_empty()
            || !digest_shape
            || required("id")? != receipt_id(&key.descriptor, &key.request_id, &key.client_scope)
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

    /// The request this receipt answers.
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

/// What a caller may conclude about one graph request.
#[derive(Clone, Debug, PartialEq)]
pub enum GraphOutcome {
    Accepted {
        event_id: String,
        graph_seq: String,
        /// The command's commit when this attempt's own answer named it.
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

/// Resolve one attempt at the request `key`, given the receipt its client
/// holds for it, if any.
#[must_use]
pub fn resolve(
    key: &RequestKey,
    attempt: &Result<CommandResult, ReplicaError>,
    receipt: Option<&Receipt>,
) -> GraphOutcome {
    // An acknowledgement of another request id says nothing about this one.
    let own_commit = match attempt {
        Ok(result) if result.request_id == key.request_id => Some(result.commit_seq),
        _ => None,
    };
    let receipt = receipt.filter(|receipt| {
        receipt.key.descriptor == key.descriptor
            && receipt.key.request_id == key.request_id
            && receipt.key.client_scope == key.client_scope
    });
    if let Some(receipt) = receipt {
        if receipt.key.request_digest != key.request_digest {
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
        Ok(_) => match own_commit {
            Some(commit_seq) => GraphOutcome::PendingVisibility { commit_seq },
            None => GraphOutcome::Pending {
                reason: "acknowledgement of another request".to_owned(),
            },
        },
        Err(ReplicaError::Rejected { code, message }) => terminal(code, message)
            .unwrap_or_else(|| GraphOutcome::TechnicalRefusal { code: code.clone() }),
        Err(ReplicaError::Remote { code, message, .. }) => {
            terminal(code, message).unwrap_or_else(|| {
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
            })
        }
        Err(error) => GraphOutcome::Pending {
            reason: error.to_string(),
        },
    }
}
