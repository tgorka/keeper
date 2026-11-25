//! `files.acknowledge`: one event of one `review` operation of kind
//! `acknowledgement` per target, or the whole list refused.

use std::collections::BTreeSet;

use super::commit::{compute_delta_digest, next_ordinal};
use super::helpers::digest_hex;
use super::review_common::{holds_review_role, review_op_and_row, reviewed};
use super::{Accepted, Manifest, Request, Server};
use crate::caller::Caller;
use crate::ids;
use crate::refusal::{Code, Refusal};
use crate::rows::{ApprovalBasis, Event, Head, OriginClass, ReviewKind};
use crate::wire::{Target, LIST_MAX};

/// Judges and commits a `files.acknowledge` request against `m`.
pub(crate) fn acknowledge(
    m: &mut Manifest,
    caller: &Caller,
    request: &Request,
    targets: &[Target],
    server: &Server,
) -> Result<Accepted, Refusal> {
    if !matches!(caller, Caller::Person { .. }) || !holds_review_role(m, caller.actor_id()) {
        return Err(Refusal::new(Code::Forbidden));
    }
    if targets.is_empty() || targets.len() > LIST_MAX {
        return Err(Refusal::new(Code::InvalidChange));
    }

    let mut seen = BTreeSet::new();
    for target in targets {
        if !ids::is_ulid(&target.target_event_id) || !seen.insert(target.target_event_id.as_str()) {
            return Err(Refusal::new(Code::InvalidChange));
        }
    }
    for target in targets {
        let Some((target_event, _)) = m.events.get(&target.target_event_id) else {
            return Err(Refusal::new(Code::InvalidChange));
        };
        if target_event.delta_digest != target.target_delta_digest
            || target_event.descriptor == "files.acknowledge"
            || reviewed(m, &target.target_event_id)
        {
            return Err(Refusal::new(Code::InvalidChange));
        }
    }

    let seq = ids::next_decimal(m.head.as_ref().map_or("0", |h| h.seq.as_str()));
    let page_key = ids::page_key(&m.domain, server.recorded_at_ms, false);

    let mut ops = Vec::with_capacity(targets.len());
    let mut reviews = Vec::with_capacity(targets.len());
    for (i, target) in targets.iter().enumerate() {
        let ordinal = next_ordinal(&ops);
        let review_id = if i == 0 {
            server.review_id.clone()
        } else {
            server.review_ids.get(i - 1).cloned()
        }
        .unwrap_or_else(|| format!("{}/review/{i}", server.event_id));
        let (op, row) = review_op_and_row(
            &m.domain,
            &server.event_id,
            ordinal,
            &page_key,
            &review_id,
            &target.target_event_id,
            &target.target_delta_digest,
            ReviewKind::Acknowledgement,
            caller.actor_id(),
        );
        ops.push(op);
        reviews.push(row);
    }

    let delta_digest = compute_delta_digest(&ops);
    let request_digest = digest_hex(&request.digest);
    let event = Event {
        id: server.event_id.clone(),
        domain: m.domain.clone(),
        seq: seq.clone(),
        page_key,
        descriptor: request.descriptor.clone(),
        request_id: request.request_id.clone(),
        request_digest,
        actor_id: caller.actor_id().to_owned(),
        client_id: server.client_id.clone(),
        origin_class: OriginClass::Human,
        origin_actor: None,
        origin_chain: None,
        context_id: None,
        approval_id: None,
        approval_basis: ApprovalBasis::Direct,
        proposal_id: None,
        change_set_id: None,
        job_id: None,
        job_ordinal: None,
        job_attempt_id: None,
        reverses_event_id: None,
        event_time_ms: None,
        recorded_at_ms: server.recorded_at_ms,
        op_count: next_ordinal(&ops),
        delta_digest: delta_digest.clone(),
    };

    let drives = m.head.as_ref().map_or_else(Vec::new, |h| h.drives.clone());
    m.head = Some(Head {
        id: m.domain.clone(),
        domain: m.domain.clone(),
        seq,
        event_id: server.event_id.clone(),
        digest: delta_digest,
        drives,
    });
    for row in reviews {
        m.reviews.insert(row.id.clone(), row);
    }
    m.events
        .insert(server.event_id.clone(), (event.clone(), ops.clone()));

    Ok(Accepted { event, ops })
}
