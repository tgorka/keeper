//! `files.decide` (story 116.6): one revision or a list, each judged on its
//! own; one event per approval.

use std::collections::BTreeSet;

use serde_json::Value;

use super::authorize::authorize;
use super::commit::commit;
use super::parse::{check_version, parse};
use super::review_common::{addressed_to, expired, holds_review_role, move_revision};
use super::stage::stage;
use super::{Body, EventMeta, Manifest, Request, Server};
use crate::caller::Caller;
use crate::refusal::{Code, Refusal};
use crate::review::{DecideItem, DecideReceipt};
use crate::rows::{ApprovalBasis, Outcome, ProposalStatus};
use crate::wire::{Decision, JobRef, ProposedChange, Verdict, LIST_MAX};
use crate::CHANGE_LIMIT;

/// Judges one `files.decide` list against `m` (now =
/// `server.recorded_at_ms`). A whole-list refusal (`Err`) changes nothing.
pub fn decide(
    m: &mut Manifest,
    caller: &Caller,
    params: &Value,
    server: &Server,
) -> Result<DecideReceipt, Refusal> {
    check_version(params)?;
    let request = parse(caller, "files.decide", params)?;
    let Body::Decide(payload) = &request.body else {
        return Err(Refusal::new(Code::InvalidChange));
    };
    if !matches!(caller, Caller::Person { .. }) || !holds_review_role(m, caller.actor_id()) {
        return Err(Refusal::new(Code::Forbidden));
    }

    let decisions = &payload.decisions;
    if decisions.is_empty() || decisions.len() > LIST_MAX {
        return Err(Refusal::new(Code::InvalidChange));
    }
    let mut seen = BTreeSet::new();
    for decision in decisions {
        if !seen.insert((
            decision.proposal_id.as_str(),
            decision.proposal_revision.as_str(),
        )) {
            return Err(Refusal::new(Code::InvalidChange));
        }
    }

    let total_ops = decisions
        .iter()
        .filter_map(|d| {
            m.proposals
                .get(&row_id(&d.proposal_id, &d.proposal_revision))
        })
        .fold(0usize, |acc, state| acc.saturating_add(state.ops.len()));
    if total_ops > CHANGE_LIMIT {
        return Err(Refusal::new(Code::ChangeLimit));
    }

    let mut approvals = 0usize;
    let mut items = Vec::with_capacity(decisions.len());
    for decision in decisions {
        items.push(judge_item(
            m,
            caller,
            &request,
            decision,
            server,
            &mut approvals,
        ));
    }
    Ok(build_receipt(items))
}

fn judge_item(
    m: &mut Manifest,
    caller: &Caller,
    request: &Request,
    decision: &Decision,
    server: &Server,
    approvals: &mut usize,
) -> DecideItem {
    let decider = caller.actor_id();
    let now = server.recorded_at_ms;
    let id = row_id(&decision.proposal_id, &decision.proposal_revision);

    let Some(state) = m.proposals.get(&id) else {
        return rejected_item(decision, Code::Forbidden, None);
    };
    if !addressed_to(m, &state.row.reviewers, decider) {
        return rejected_item(decision, Code::Forbidden, None);
    }
    if state.row.status != ProposalStatus::Open || state.row.digest != decision.proposal_digest {
        return rejected_item(decision, Code::ProposalStale, None);
    }
    if expired(state.row.expires_at_ms, now) {
        move_revision(
            m,
            &id,
            ProposalStatus::Expired,
            Some("proposal_stale"),
            Some((now, decider, None)),
        );
        return rejected_item(decision, Code::ProposalStale, Some(ProposalStatus::Expired));
    }

    match decision.decision {
        Verdict::Reject => {
            move_revision(
                m,
                &id,
                ProposalStatus::Rejected,
                None,
                Some((now, decider, None)),
            );
            DecideItem {
                proposal_id: decision.proposal_id.clone(),
                proposal_revision: decision.proposal_revision.clone(),
                outcome: Outcome::Proposed,
                event_id: None,
                seq: None,
                status: Some(ProposalStatus::Rejected),
                code: None,
            }
        }
        Verdict::Approve => approve_item(m, caller, request, decision, &id, server, approvals),
    }
}

fn approve_item(
    m: &mut Manifest,
    caller: &Caller,
    request: &Request,
    decision: &Decision,
    id: &str,
    server: &Server,
    approvals: &mut usize,
) -> DecideItem {
    let decider = caller.actor_id();
    let now = server.recorded_at_ms;
    let Some(state) = m.proposals.get(id).cloned() else {
        return rejected_item(decision, Code::Forbidden, None);
    };

    let row = state.row;
    let ops = state.ops;
    let proposer = state.proposer;
    let event_time_ms = serde_json::from_value::<ProposedChange>(row.change)
        .ok()
        .and_then(|change| change.event_time_ms);
    let job = match (row.job_id, row.job_ordinal, row.job_attempt_id) {
        (Some(job_id), Some(ordinal), Some(attempt_id)) => Some(JobRef {
            job_id,
            ordinal,
            attempt_id,
        }),
        _ => None,
    };

    let req = Request {
        descriptor: "files.decide".to_owned(),
        request_id: request.request_id.clone(),
        context_id: None,
        job: job.clone(),
        digest: request.digest.clone(),
        body: Body::Change {
            ops: ops.clone(),
            change_set_id: row.change_set_id,
            event_time_ms,
            import: false,
        },
    };

    let auth_ok = match authorize(m, &proposer, &req, &ops, None) {
        Ok(_) => true,
        Err(refusal) => refusal.code == Code::ProposeRequired,
    };
    let staged = if auth_ok {
        stage(m, &proposer, &ops, false, event_time_ms, false).ok()
    } else {
        None
    };
    let Some(staged) = staged else {
        move_revision(
            m,
            id,
            ProposalStatus::Rejected,
            Some("proposal_stale"),
            Some((now, decider, None)),
        );
        return rejected_item(
            decision,
            Code::ProposalStale,
            Some(ProposalStatus::Rejected),
        );
    };

    let basis = ApprovalBasis::Proposal {
        proposal_id: row.proposal_id.clone(),
        proposal_revision: row.revision,
        proposal_digest: row.digest,
        proposer_actor: row.proposer_actor,
        proposer_client: row.proposer_client,
        proposer_context: row.proposer_context,
    };
    let meta = EventMeta {
        origin_actor: row.origin_actor,
        origin_chain: row.origin_chain,
        proposal_id: Some(row.proposal_id),
        job,
    };

    let mut item_server = server.clone();
    item_server.event_id = approval_event_id(server, *approvals);
    *approvals += 1;

    let accepted = commit(m, caller, &req, basis, staged, None, &item_server, &meta);
    move_revision(
        m,
        id,
        ProposalStatus::Approved,
        None,
        Some((now, decider, Some(accepted.event.id.as_str()))),
    );
    DecideItem {
        proposal_id: decision.proposal_id.clone(),
        proposal_revision: decision.proposal_revision.clone(),
        outcome: Outcome::Accepted,
        event_id: Some(accepted.event.id),
        seq: Some(accepted.event.seq),
        status: Some(ProposalStatus::Approved),
        code: None,
    }
}

#[must_use]
fn rejected_item(decision: &Decision, code: Code, status: Option<ProposalStatus>) -> DecideItem {
    DecideItem {
        proposal_id: decision.proposal_id.clone(),
        proposal_revision: decision.proposal_revision.clone(),
        outcome: Outcome::Rejected,
        event_id: None,
        seq: None,
        status,
        code: Some(code),
    }
}

#[must_use]
fn build_receipt(items: Vec<DecideItem>) -> DecideReceipt {
    if items.len() == 1 {
        let only = &items[0];
        return DecideReceipt {
            outcome: only.outcome,
            code: only.code,
            event_id: only.event_id.clone(),
            seq: only.seq.clone(),
            detail: items,
        };
    }
    let (outcome, code) = if items.iter().any(|item| item.outcome == Outcome::Accepted) {
        (Outcome::Accepted, None)
    } else if items.iter().any(|item| item.outcome == Outcome::Proposed) {
        (Outcome::Proposed, None)
    } else {
        (Outcome::Rejected, items.first().and_then(|item| item.code))
    };
    DecideReceipt {
        outcome,
        code,
        event_id: None,
        seq: None,
        detail: items,
    }
}

#[must_use]
fn row_id(proposal_id: &str, revision: &str) -> String {
    format!("{proposal_id}/{revision}")
}

#[must_use]
fn approval_event_id(server: &Server, k: usize) -> String {
    if k == 0 {
        server.event_id.clone()
    } else {
        server
            .event_ids
            .get(k - 1)
            .cloned()
            .unwrap_or_else(|| format!("{}/{k}", server.event_id))
    }
}
