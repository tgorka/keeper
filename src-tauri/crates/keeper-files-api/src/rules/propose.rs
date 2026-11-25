//! `files.propose` (story 116.6): a revision stored, no event, no head move.

use serde_json::{json, Value};

use super::authorize::authorize;
use super::helpers::digest_hex;
use super::parse::{check_version, parse};
use super::review_common::{expired, holds_review_role, move_revision};
use super::stage::stage;
use super::{Body, Manifest, ProposalState, Server};
use crate::caller::Caller;
use crate::ids;
use crate::refusal::{Code, Refusal};
use crate::review::ProposedDetail;
use crate::rows::{Proposal, ProposalReviewer, ProposalStatus};
use crate::wire::{Reviewers, REVIEWERS_MAX};

/// Judges one `files.propose` against `m` (now = `server.recorded_at_ms`).
/// On success stores the revision and its inbox rows (and marks the one it
/// replaces `superseded`) and answers the `proposed` receipt's detail;
/// refused, `m` is unchanged.
pub fn propose(
    m: &mut Manifest,
    caller: &Caller,
    params: &Value,
    server: &Server,
) -> Result<ProposedDetail, Refusal> {
    // 1. Parse (steps 1, 3–6).
    check_version(params)?;
    let request = parse(caller, "files.propose", params)?;
    let Body::Propose(payload) = &request.body else {
        return Err(Refusal::new(Code::InvalidChange));
    };

    // 2. Who: import is forbidden; authorize person/rooted caller (`propose_required` is allowed).
    if matches!(caller, Caller::Import { .. }) {
        return Err(Refusal::new(Code::Forbidden));
    }
    match authorize(m, caller, &request, &payload.change.operations, None) {
        Ok(_) => {}
        Err(refusal) if refusal.code == Code::ProposeRequired => {}
        Err(refusal) => return Err(refusal),
    }

    // 3. Shape.
    if !ids::is_ulid(&payload.proposal_id)
        || payload
            .change_set_id
            .as_deref()
            .is_some_and(|id| !ids::is_ulid(id))
        || payload
            .base_revision
            .as_deref()
            .is_some_and(|rev| !is_canonical_revision(rev))
        || payload.expires_at_ms.is_some_and(|at| at < 0)
    {
        return Err(Refusal::new(Code::InvalidChange));
    }
    if let Reviewers::Actors(ids) = &payload.reviewers {
        if ids.is_empty()
            || ids.len() > REVIEWERS_MAX
            || ids.iter().any(String::is_empty)
            || ids.windows(2).any(|pair| pair[0] >= pair[1])
        {
            return Err(Refusal::new(Code::InvalidChange));
        }
    }

    // 4. Reviewers.
    if let Reviewers::Actors(ids) = &payload.reviewers {
        if ids.iter().any(|id| !holds_review_role(m, id)) {
            return Err(Refusal::new(Code::InvalidChange));
        }
    }

    // 5. Revision.
    let now = server.recorded_at_ms;
    let latest = m
        .proposals
        .values()
        .filter(|state| state.row.proposal_id == payload.proposal_id)
        .max_by_key(|state| revision_num(&state.row.revision));
    let (revision, base_row_id) = match payload.base_revision.as_deref() {
        None => {
            if let Some(latest) = latest {
                let code = if latest.row.proposer_actor == caller.actor_id() {
                    Code::InvalidChange
                } else {
                    Code::Forbidden
                };
                return Err(Refusal::new(code));
            }
            ("1".to_owned(), None)
        }
        Some(base_rev) => {
            let base_id = format!("{}/{base_rev}", payload.proposal_id);
            let Some(base) = m.proposals.get(&base_id) else {
                return Err(Refusal::new(Code::Forbidden));
            };
            if base.row.proposer_actor != caller.actor_id() {
                return Err(Refusal::new(Code::Forbidden));
            }
            let is_latest = latest.is_some_and(|l| l.row.id == base.row.id);
            if !is_latest
                || base.row.status != ProposalStatus::Open
                || expired(base.row.expires_at_ms, now)
            {
                return Err(Refusal::new(Code::ProposalStale));
            }
            (ids::next_decimal(&base.row.revision), Some(base_id))
        }
    };

    // 6. Judge the change.
    let _ = stage(
        m,
        caller,
        &payload.change.operations,
        false,
        payload.change.event_time_ms,
        false,
    )?;

    // Prepare values before any mutation of `m`.
    // The stored change and the digest are the payload as sent (its JSON),
    // so every implementation digests the same bytes.
    let raw = params.get("payload").cloned().unwrap_or(Value::Null);
    let change = raw.get("change").cloned().unwrap_or(Value::Null);
    let op_count = u32::try_from(payload.change.operations.len())
        .map_err(|_| Refusal::new(Code::ChangeLimit))?;
    let digest = proposal_digest(&m.domain, &revision, &raw);
    let row_id = format!("{}/{revision}", payload.proposal_id);
    let inbox_reviewers: Vec<String> = match &payload.reviewers {
        Reviewers::Actors(ids) => ids.clone(),
        Reviewers::Role(_) => m
            .roles
            .keys()
            .filter(|actor| holds_review_role(m, actor))
            .cloned()
            .collect(),
    };

    // 7. Store.
    if let Some(base_id) = base_row_id {
        move_revision(m, &base_id, ProposalStatus::Superseded, None, None);
    }
    let row = Proposal {
        id: row_id.clone(),
        domain: m.domain.clone(),
        proposal_id: payload.proposal_id.clone(),
        revision: revision.clone(),
        status: ProposalStatus::Open,
        reason: None,
        digest: digest.clone(),
        change,
        reviewers: payload.reviewers.clone(),
        expires_at_ms: payload.expires_at_ms,
        proposer_actor: caller.actor_id().to_owned(),
        proposer_client: server.client_id.clone(),
        proposer_context: request.context_id.clone(),
        origin_actor: None,
        origin_chain: None,
        source_labels: Vec::new(),
        created_at_ms: now,
        decided_at_ms: None,
        decided_by: None,
        decided_event_id: None,
        change_set_id: payload.change_set_id.clone(),
        job_id: request.job.as_ref().map(|job| job.job_id.clone()),
        job_ordinal: request.job.as_ref().map(|job| job.ordinal),
        job_attempt_id: request.job.as_ref().map(|job| job.attempt_id.clone()),
    };
    m.proposals.insert(
        row_id.clone(),
        ProposalState {
            row,
            proposer: caller.clone(),
            ops: payload.change.operations.clone(),
        },
    );
    for reviewer_actor in inbox_reviewers {
        let id = format!("{row_id}/{reviewer_actor}");
        m.inbox.insert(
            id.clone(),
            ProposalReviewer {
                id,
                domain: m.domain.clone(),
                reviewer_actor,
                proposal_id: payload.proposal_id.clone(),
                revision: revision.clone(),
                status: ProposalStatus::Open,
                proposer_actor: caller.actor_id().to_owned(),
                change_set_id: payload.change_set_id.clone(),
                expires_at_ms: payload.expires_at_ms,
                op_count,
            },
        );
    }

    // 8. Answer.
    Ok(ProposedDetail {
        proposal_id: payload.proposal_id.clone(),
        proposal_revision: revision,
        proposal_digest: digest,
    })
}

/// The proposal digest of revision `revision` of the `files.propose`
/// payload `payload` (as sent): lowercase hex SHA-256 of the canonical JSON
/// of `{version: 1, domain, proposal_id, revision, change, reviewers,
/// expires_at_ms, change_set_id}`, an absent member as `null`.
#[must_use]
pub fn proposal_digest(domain: &str, revision: &str, payload: &Value) -> String {
    let field = |key: &str| payload.get(key).cloned().unwrap_or(Value::Null);
    let value = json!({
        "version": 1,
        "domain": domain,
        "proposal_id": field("proposal_id"),
        "revision": revision,
        "change": field("change"),
        "reviewers": field("reviewers"),
        "expires_at_ms": field("expires_at_ms"),
        "change_set_id": field("change_set_id"),
    });
    digest_hex(&value)
}

#[must_use]
fn is_canonical_revision(text: &str) -> bool {
    let bytes = text.as_bytes();
    !bytes.is_empty()
        && matches!(bytes[0], b'1'..=b'9')
        && bytes.iter().all(u8::is_ascii_digit)
        && text.parse::<u64>().is_ok()
}

#[must_use]
fn revision_num(rev: &str) -> u64 {
    rev.parse::<u64>().unwrap_or(0)
}
