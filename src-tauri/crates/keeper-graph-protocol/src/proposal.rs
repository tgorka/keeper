//! Proposals and their decisions (contract §4 *Proposals and decisions*,
//! §9.5; schema 8).
//!
//! A proposal is private metadata: never replayed, never a graph head
//! increment, synced to its proposer only (`kg_proposal`). Each revision is
//! immutable; editing adds the next one and supersedes the one it names,
//! which invalidates any approval of it.
//!
//! [`PROPOSE`]'s payload is exactly
//! `{"proposal_id": <ULID>, "base_revision": <decimal> | null,
//! "change": <graph.apply payload>, "reviewers": [<actor id>…],
//! "expires_at_ms": <integer> | null}`:
//!
//! - `base_revision` null proposes a new proposal (its revision `1`);
//!   otherwise it is the open revision this one supersedes (its revision
//!   one more);
//! - `change` is a `graph.apply` payload ([`crate::change::parse`]) without
//!   `approval_id`: a proposal is never an agent's per-call write, and is
//!   validated exactly as a write is before it is stored;
//! - `reviewers` are 1 to [`MAX_REVIEWERS`] distinct actor ids in ascending
//!   byte order, the persons who may decide this revision;
//! - `expires_at_ms`, when not null, is the acceptance time from which the
//!   revision can no longer be approved.
//!
//! The proposal digest ([`digest`]) binds a revision to its domain: the
//! SHA-256 of the JCS bytes of `{"version", "graph", "proposal_id",
//! "revision", "change", "reviewers", "expires_at_ms"}`. A `proposed`
//! receipt's detail is `{proposal_id, proposal_revision, proposal_digest}`
//! ([`proposed_detail`]).
//!
//! [`DECIDE`]'s payload is exactly `{"proposal_id", "proposal_revision",
//! "proposal_digest", "decision": "approve" | "reject"}`. An approval that
//! finds the revision anything but open and current, past its expiry, its
//! digest different or its change's preconditions moved is refused
//! terminally `rejected proposal_stale`, never rebased. An approval's event
//! is attributed per [`approval_basis`]; a rejection's receipt is `proposed`
//! with [`decided_detail`], and advances no graph head.

use serde_json::{json, Map, Value};

use crate::assertion::actor_ok;
use crate::change::{self, Change, MAX_EVENT_TIME_MS};
use crate::worker::is_hex_digest;
use crate::{is_decimal, is_ulid, Refusal, CONTRACT_VERSION};

/// The descriptor of a proposal.
pub const PROPOSE: &str = "graph.propose";
/// The descriptor of a human reviewer's decision on one proposal revision.
pub const DECIDE: &str = "graph.decide";
/// The most reviewers one proposal revision may name.
pub const MAX_REVIEWERS: usize = 16;
/// A proposal revision's statuses: open until one persisted transition.
pub const STATUSES: [&str; 5] = ["open", "approved", "rejected", "expired", "superseded"];

/// A checked [`PROPOSE`] payload.
#[derive(Clone, Debug, PartialEq)]
pub struct Proposal {
    pub proposal_id: String,
    /// The open revision this one supersedes; `None` for a new proposal.
    pub base_revision: Option<String>,
    pub change: Change,
    pub reviewers: Vec<String>,
    pub expires_at_ms: Option<u64>,
}

impl Proposal {
    /// Check a proposal's parts by the rules of [`parse`].
    pub fn new(
        proposal_id: String,
        base_revision: Option<String>,
        change: Change,
        reviewers: Vec<String>,
        expires_at_ms: Option<u64>,
    ) -> Result<Self, Refusal> {
        let reviewers_ok = (1..=MAX_REVIEWERS).contains(&reviewers.len())
            && reviewers.iter().all(|r| actor_ok(r))
            && reviewers.windows(2).all(|pair| pair[0] < pair[1]);
        let base_ok = base_revision
            .as_deref()
            .is_none_or(|r| is_decimal(r) && r != "0");
        if !is_ulid(&proposal_id)
            || !base_ok
            || !reviewers_ok
            || change.approval_id().is_some()
            || expires_at_ms.is_some_and(|ms| ms > MAX_EVENT_TIME_MS)
        {
            return Err(Refusal::InvalidChange);
        }
        Ok(Self {
            proposal_id,
            base_revision,
            change,
            reviewers,
            expires_at_ms,
        })
    }

    /// The revision this payload creates: `1`, or one more than its base.
    pub fn revision(&self) -> Result<String, Refusal> {
        match &self.base_revision {
            None => Ok("1".to_owned()),
            Some(base) => base
                .parse::<u128>()
                .ok()
                .and_then(|n| n.checked_add(1))
                .map(|n| n.to_string())
                .filter(|n| n.len() <= 20)
                .ok_or(Refusal::InvalidChange),
        }
    }

    /// The payload [`parse`] reads back as this proposal.
    #[must_use]
    pub fn payload(&self) -> Value {
        json!({
            "proposal_id": self.proposal_id, "base_revision": self.base_revision,
            "change": self.change.payload(), "reviewers": self.reviewers,
            "expires_at_ms": self.expires_at_ms,
        })
    }
}

fn exactly<'a>(payload: &'a Value, keys: &[&str]) -> Result<&'a Map<String, Value>, Refusal> {
    let fields = payload.as_object().ok_or(Refusal::InvalidChange)?;
    if fields.len() != keys.len() || !keys.iter().all(|k| fields.contains_key(*k)) {
        return Err(Refusal::InvalidChange);
    }
    Ok(fields)
}

/// Read a [`PROPOSE`] payload, its change under `canonical_tag`.
pub fn parse(
    payload: &Value,
    canonical_tag: impl Fn(&str) -> Option<String>,
) -> Result<Proposal, Refusal> {
    let fields = exactly(
        payload,
        &[
            "proposal_id",
            "base_revision",
            "change",
            "reviewers",
            "expires_at_ms",
        ],
    )?;
    let text = |key: &str| match fields.get(key) {
        Some(Value::String(s)) => Ok(Some(s.clone())),
        Some(Value::Null) => Ok(None),
        _ => Err(Refusal::InvalidChange),
    };
    let proposal_id = text("proposal_id")?.ok_or(Refusal::InvalidChange)?;
    let base_revision = text("base_revision")?;
    let reviewers = match fields.get("reviewers") {
        Some(Value::Array(items)) if items.len() <= MAX_REVIEWERS => items
            .iter()
            .map(|r| r.as_str().map(str::to_owned).ok_or(Refusal::InvalidChange))
            .collect::<Result<Vec<_>, _>>()?,
        _ => return Err(Refusal::InvalidChange),
    };
    let expires_at_ms = match fields.get("expires_at_ms") {
        Some(Value::Null) => None,
        Some(value) => Some(value.as_u64().ok_or(Refusal::InvalidChange)?),
        None => return Err(Refusal::InvalidChange),
    };
    let change = change::parse(&fields["change"], canonical_tag)?;
    Proposal::new(proposal_id, base_revision, change, reviewers, expires_at_ms)
}

/// The proposal digest of `proposal`'s revision in `graph` (the module's
/// rule).
pub fn digest(graph: &str, proposal: &Proposal) -> Result<String, Refusal> {
    crate::value::digest(&json!({
        "version": CONTRACT_VERSION, "graph": graph,
        "proposal_id": proposal.proposal_id, "revision": proposal.revision()?,
        "change": proposal.change.payload(), "reviewers": proposal.reviewers,
        "expires_at_ms": proposal.expires_at_ms,
    }))
}

/// A `proposed` receipt's detail for a stored proposal revision.
#[must_use]
pub fn proposed_detail(proposal_id: &str, revision: &str, digest: &str) -> Value {
    json!({"proposal_id": proposal_id, "proposal_revision": revision, "proposal_digest": digest})
}

/// Whether a revision expiring at `expires_at_ms` is expired at acceptance
/// time `now_ms`: from that instant on.
#[must_use]
pub fn expired(expires_at_ms: Option<u64>, now_ms: u64) -> bool {
    expires_at_ms.is_some_and(|at| now_ms >= at)
}

/// What a reviewer decides.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Approve,
    Reject,
}

/// A checked [`DECIDE`] payload: the exact revision decided.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Decision {
    pub proposal_id: String,
    pub proposal_revision: String,
    pub proposal_digest: String,
    pub verdict: Verdict,
}

impl Decision {
    /// The payload [`parse_decide`] reads back as this decision.
    #[must_use]
    pub fn payload(&self) -> Value {
        json!({
            "proposal_id": self.proposal_id, "proposal_revision": self.proposal_revision,
            "proposal_digest": self.proposal_digest,
            "decision": match self.verdict { Verdict::Approve => "approve", Verdict::Reject => "reject" },
        })
    }
}

/// Read a [`DECIDE`] payload.
pub fn parse_decide(payload: &Value) -> Result<Decision, Refusal> {
    let fields = exactly(
        payload,
        &[
            "proposal_id",
            "proposal_revision",
            "proposal_digest",
            "decision",
        ],
    )?;
    let text = |key: &str| fields.get(key).and_then(Value::as_str);
    let (Some(id), Some(revision), Some(digest), Some(decision)) = (
        text("proposal_id"),
        text("proposal_revision"),
        text("proposal_digest"),
        text("decision"),
    ) else {
        return Err(Refusal::InvalidChange);
    };
    let verdict = match decision {
        "approve" => Verdict::Approve,
        "reject" => Verdict::Reject,
        _ => return Err(Refusal::InvalidChange),
    };
    if !is_ulid(id) || !is_decimal(revision) || revision == "0" || !is_hex_digest(digest) {
        return Err(Refusal::InvalidChange);
    }
    Ok(Decision {
        proposal_id: id.to_owned(),
        proposal_revision: revision.to_owned(),
        proposal_digest: digest.to_owned(),
        verdict,
    })
}

/// A rejection's `proposed` receipt detail: the revision and its new
/// status.
#[must_use]
pub fn decided_detail(decision: &Decision) -> Value {
    json!({
        "proposal_id": decision.proposal_id, "proposal_revision": decision.proposal_revision,
        "proposal_digest": decision.proposal_digest, "status": "rejected",
    })
}

/// Who proposed a revision, captured at [`PROPOSE`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Proposer {
    pub actor: String,
    pub client: String,
    pub context: Option<String>,
}

/// An approved proposal's event `approval_basis` (CC-X6): the event's own
/// request, actor, client and origin class are the decider's, its context
/// null; its `origin_actor`/`origin_chain` the proposer's captured origin.
#[must_use]
pub fn approval_basis(decision: &Decision, proposer: &Proposer) -> Value {
    json!({
        "basis": "proposal", "proposal_id": decision.proposal_id,
        "proposal_revision": decision.proposal_revision,
        "proposal_digest": decision.proposal_digest,
        "proposer_actor": proposer.actor, "proposer_client": proposer.client,
        "proposer_context": proposer.context,
    })
}
