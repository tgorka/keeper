//! The reads `files.proposal`, `files.proposals` and `files.unreviewed`,
//! re-authorised per call.

use std::cmp::Ordering;

use serde_json::Value;

use super::parse::check_version;
use super::review_common::{addressed_to, expired, holds_review_role, reviewed};
use super::Manifest;
use crate::caller::Caller;
use crate::history::HistoryItem;
use crate::ids;
use crate::refusal::{Code, Refusal};
use crate::review::{
    ProposalParams, ProposalsPage, ProposalsParams, UnreviewedPage, UnreviewedParams, PAGE_MAX,
};
use crate::rows::{Event, Op, OriginClass, Proposal, ProposalStatus};

/// Evaluates a `files.proposal` read against `m`.
///
/// Returns the revisions of `proposal_id` ascending by numeric revision that
/// `caller` may read: all revisions for their proposer, or those addressed to
/// `caller` now when `caller` is a person.
pub fn proposal(m: &Manifest, caller: &Caller, params: &Value) -> Result<Vec<Proposal>, Refusal> {
    check_version(params)?;
    let params: ProposalParams =
        serde_json::from_value(params.clone()).map_err(|_| Refusal::new(Code::InvalidChange))?;
    if !ids::is_ulid(&params.proposal_id) {
        return Err(Refusal::new(Code::InvalidChange));
    }

    let actor = caller.actor_id();
    let is_person = matches!(caller, Caller::Person { .. });
    let mut rows: Vec<&Proposal> = m
        .proposals
        .values()
        .map(|state| &state.row)
        .filter(|row| {
            row.proposal_id == params.proposal_id
                && (row.proposer_actor == actor
                    || (is_person && addressed_to(m, &row.reviewers, actor)))
        })
        .collect();
    rows.sort_by(|a, b| cmp_decimal(&a.revision, &b.revision));
    Ok(rows.into_iter().cloned().collect())
}

/// Evaluates a `files.proposals` read against `m` at `now_ms`.
///
/// Returns open, unexpired revisions addressed to `caller`, ascending by row
/// id after `after`, up to `limit`.
pub fn proposals(
    m: &Manifest,
    caller: &Caller,
    params: &Value,
    now_ms: i64,
) -> Result<ProposalsPage, Refusal> {
    check_version(params)?;
    let params: ProposalsParams =
        serde_json::from_value(params.clone()).map_err(|_| Refusal::new(Code::InvalidChange))?;
    let actor = caller.actor_id();
    if !matches!(caller, Caller::Person { .. }) || !holds_review_role(m, actor) {
        return Err(Refusal::new(Code::Forbidden));
    }
    if !(1..=PAGE_MAX).contains(&params.limit)
        || params.after.as_deref().is_some_and(str::is_empty)
        || params
            .change_set_id
            .as_deref()
            .is_some_and(|id| !ids::is_ulid(id))
    {
        return Err(Refusal::new(Code::InvalidChange));
    }

    let limit = usize::try_from(params.limit).map_err(|_| Refusal::new(Code::InvalidChange))?;
    let after_filter = params.after.as_deref();
    let change_set_filter = params.change_set_id.as_deref();

    let proposals: Vec<Proposal> = m
        .proposals
        .values()
        .map(|state| &state.row)
        .filter(|row| {
            row.status == ProposalStatus::Open
                && !expired(row.expires_at_ms, now_ms)
                && addressed_to(m, &row.reviewers, actor)
                && change_set_filter.is_none_or(|cs| row.change_set_id.as_deref() == Some(cs))
                && after_filter.is_none_or(|after| row.id.as_str() > after)
        })
        .take(limit)
        .cloned()
        .collect();

    let next = if proposals.len() == limit {
        proposals.last().map(|row| row.id.clone())
    } else {
        None
    };

    Ok(ProposalsPage { proposals, next })
}

/// Evaluates a `files.unreviewed` read against `m`.
///
/// Returns unreviewed agent-, tool-, and job-origin events with `seq > after`,
/// ascending numerically by `seq`, up to `limit`.
pub fn unreviewed(
    m: &Manifest,
    caller: &Caller,
    params: &Value,
) -> Result<UnreviewedPage, Refusal> {
    check_version(params)?;
    let params: UnreviewedParams =
        serde_json::from_value(params.clone()).map_err(|_| Refusal::new(Code::InvalidChange))?;
    if !matches!(caller, Caller::Person { .. }) || !holds_review_role(m, caller.actor_id()) {
        return Err(Refusal::new(Code::Forbidden));
    }
    if !is_canonical_decimal(&params.after)
        || !(1..=PAGE_MAX).contains(&params.limit)
        || params
            .change_set_id
            .as_deref()
            .is_some_and(|id| !ids::is_ulid(id))
    {
        return Err(Refusal::new(Code::InvalidChange));
    }

    let limit = usize::try_from(params.limit).map_err(|_| Refusal::new(Code::InvalidChange))?;
    let change_set_filter = params.change_set_id.as_deref();

    let mut matched: Vec<(&Event, &Vec<Op>)> = m
        .events
        .values()
        .filter(|(event, _)| {
            matches!(
                event.origin_class,
                OriginClass::Agent | OriginClass::Tool | OriginClass::Job
            ) && !reviewed(m, &event.id)
                && cmp_decimal(&event.seq, &params.after).is_gt()
                && change_set_filter.is_none_or(|cs| event.change_set_id.as_deref() == Some(cs))
        })
        .map(|(event, ops)| (event, ops))
        .collect();

    matched.sort_by(|(a, _), (b, _)| cmp_decimal(&a.seq, &b.seq));
    matched.truncate(limit);

    let events: Vec<HistoryItem> = matched
        .into_iter()
        .map(|(event, ops)| {
            let mut item_ops = ops.clone();
            item_ops.sort_by_key(|op| op.ordinal);
            HistoryItem {
                event: event.clone(),
                ops: item_ops,
            }
        })
        .collect();

    let scanned_through = if events.len() == limit {
        events
            .last()
            .map_or_else(|| params.after.clone(), |item| item.event.seq.clone())
    } else {
        let head_seq = m.head.as_ref().map_or("0", |head| head.seq.as_str());
        if cmp_decimal(&params.after, head_seq).is_ge() {
            params.after
        } else {
            head_seq.to_owned()
        }
    };

    Ok(UnreviewedPage {
        events,
        scanned_through,
    })
}

#[must_use]
fn is_canonical_decimal(s: &str) -> bool {
    if s == "0" {
        return true;
    }
    let bytes = s.as_bytes();
    matches!(bytes.first(), Some(b'1'..=b'9')) && bytes.iter().all(u8::is_ascii_digit)
}

#[must_use]
fn cmp_decimal(a: &str, b: &str) -> Ordering {
    a.len().cmp(&b.len()).then_with(|| a.cmp(b))
}
