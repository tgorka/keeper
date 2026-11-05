//! What a graph request's caller may conclude (contract §9.1).
//!
//! | transport | the client's receipt | outcome |
//! | --- | --- | --- |
//! | any | a receipt whose digest is not the request's | [`GraphOutcome::IdempotencyMismatch`] |
//! | any | `accepted` | [`GraphOutcome::Accepted`], with the event and graph sequence the receipt names |
//! | any | `proposed`, `conflict`, `rejected` | exactly that, never accepted |
//! | applied or cached | none visible yet | [`GraphOutcome::PendingVisibility`]: the commit is a visibility bound; pull the receipt or ask `graph.outcome` |
//! | refused `idempotency_mismatch` | none | [`GraphOutcome::IdempotencyMismatch`] |
//! | refused `unsupported_version` | none | [`GraphOutcome::Unsupported`] |
//! | refused with a `kg.` technical code | none | [`GraphOutcome::TechnicalRefusal`] |
//! | any other refusal, a lost or unreadable answer | none | [`GraphOutcome::Pending`]: keep the same request id and resolve it later |
//!
//! A receipt is the durable record, so it decides whatever the transport
//! said: an unheard answer whose receipt has since arrived is resolved.

use serde_json::{Map, Value};

use super::protocol::vocabulary::Outcome;
use super::protocol::{is_decimal, Refusal};
use crate::hq::port::{CommandResult, ReplicaError};

/// One `kg_receipts` row, decoded.
#[derive(Clone, Debug, PartialEq)]
pub struct Receipt {
    pub descriptor: String,
    pub request_id: String,
    pub request_digest: String,
    pub outcome: Outcome,
    /// The stable reason code of a `rejected` receipt.
    pub reason: Option<String>,
    /// The accepted event, on an `accepted` receipt.
    pub event_id: Option<String>,
    /// The accepted graph sequence, on an `accepted` receipt.
    pub graph_seq: Option<String>,
    /// Bounded safe detail: affected ids and revisions, a conflict's current
    /// revisions, a proposal's id.
    pub detail: Value,
}

impl Receipt {
    /// Decode a replica row of `kg_receipts`. A row this build cannot read —
    /// an outcome it does not know, an accepted receipt without its event —
    /// refuses `unsupported_version` rather than guessing.
    pub fn from_row(row: &Map<String, Value>) -> Result<Self, Refusal> {
        let text = |key: &str| row.get(key).and_then(Value::as_str).map(str::to_owned);
        let required = |key: &str| text(key).ok_or(Refusal::UnsupportedVersion);
        let outcome = serde_json::from_value(row.get("outcome").cloned().unwrap_or(Value::Null))
            .map_err(|_| Refusal::UnsupportedVersion)?;
        // A JSON column reaches a replica row as its text.
        let detail = match row.get("detail") {
            Some(Value::String(json)) => {
                serde_json::from_str(json).map_err(|_| Refusal::UnsupportedVersion)?
            }
            Some(value @ Value::Object(_)) => value.clone(),
            _ => return Err(Refusal::UnsupportedVersion),
        };
        let receipt = Self {
            descriptor: required("descriptor")?,
            request_id: required("request_id")?,
            request_digest: required("request_digest")?,
            outcome,
            reason: text("reason"),
            event_id: text("event_id"),
            graph_seq: text("graph_seq"),
            detail,
        };
        let accepted = receipt.outcome == Outcome::Accepted;
        let sequenced =
            receipt.event_id.is_some() && receipt.graph_seq.as_deref().is_some_and(is_decimal);
        let rejected_has_reason = receipt.outcome != Outcome::Rejected || receipt.reason.is_some();
        if accepted != sequenced || !rejected_has_reason {
            return Err(Refusal::UnsupportedVersion);
        }
        Ok(receipt)
    }
}

/// What a caller may conclude about one graph request.
#[derive(Clone, Debug, PartialEq)]
pub enum GraphOutcome {
    Accepted {
        event_id: String,
        graph_seq: String,
        /// The command's commit when this attempt's answer named it.
        commit_seq: Option<i64>,
        detail: Value,
    },
    /// A private proposal: nothing accepted, no graph sequence.
    Proposed {
        detail: Value,
    },
    Conflict {
        detail: Value,
    },
    Rejected {
        reason: String,
        detail: Value,
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
    /// The server's invariant backstop refused the request: a server fault,
    /// terminal for this request id.
    TechnicalRefusal {
        code: String,
    },
}

/// Resolve one attempt at a request whose digest is `request_digest`.
#[must_use]
pub fn resolve(
    request_digest: &str,
    attempt: &Result<CommandResult, ReplicaError>,
    receipt: Option<&Receipt>,
) -> GraphOutcome {
    if let Some(receipt) = receipt {
        if receipt.request_digest != request_digest {
            return GraphOutcome::IdempotencyMismatch;
        }
        let detail = receipt.detail.clone();
        return match receipt.outcome {
            Outcome::Accepted => GraphOutcome::Accepted {
                event_id: receipt.event_id.clone().unwrap_or_default(),
                graph_seq: receipt.graph_seq.clone().unwrap_or_default(),
                commit_seq: attempt.as_ref().ok().and_then(|result| result.commit_seq),
                detail,
            },
            Outcome::Proposed => GraphOutcome::Proposed { detail },
            Outcome::Conflict => GraphOutcome::Conflict { detail },
            Outcome::Rejected => GraphOutcome::Rejected {
                reason: receipt.reason.clone().unwrap_or_default(),
                detail,
            },
        };
    }
    match attempt {
        Ok(result) => GraphOutcome::PendingVisibility {
            commit_seq: result.commit_seq,
        },
        Err(ReplicaError::Remote { code, message, .. }) => {
            if code == "idempotency_mismatch" || message == "idempotency_mismatch" {
                GraphOutcome::IdempotencyMismatch
            } else if code == "unsupported_version" || message == "unsupported_version" {
                GraphOutcome::Unsupported
            } else if code.starts_with("kg.") {
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
        }
        Err(error) => GraphOutcome::Pending {
            reason: error.to_string(),
        },
    }
}
