//! What proposals, decisions and reviews share (story 116.6): the review
//! image and its row, who a revision is addressed to, expiry, and the
//! status move of a revision with its inbox rows.

use keeper_accounts_api::Role;
use serde_json::{json, Value};

use super::Manifest;
use crate::rows::{EntityType, Op, Operation, ProposalStatus, Review, ReviewKind};
use crate::wire::Reviewers;

/// A review's after-image: KG1's six fields, equal to its `files_reviews`
/// row less `domain`, `page_key` and `created_event_id`.
#[must_use]
pub(crate) fn review_image(
    id: &str,
    target_event_id: &str,
    target_delta_digest: &str,
    kind: ReviewKind,
    reviewer_actor: &str,
) -> Value {
    json!({
        "id": id,
        "target_event_id": target_event_id,
        "target_delta_digest": target_delta_digest,
        "kind": kind,
        "reviewer_actor": reviewer_actor,
        "revision": "1",
    })
}

/// The `review` operation at `ordinal` of event `event_id` creating review
/// `id`, and the `files_reviews` row equal to it.
#[allow(clippy::too_many_arguments)]
#[must_use]
pub(crate) fn review_op_and_row(
    domain: &str,
    event_id: &str,
    ordinal: u32,
    page_key: &str,
    id: &str,
    target_event_id: &str,
    target_delta_digest: &str,
    kind: ReviewKind,
    reviewer_actor: &str,
) -> (Op, Review) {
    let op = Op {
        id: format!("{event_id}/{ordinal}"),
        domain: domain.to_owned(),
        event_id: event_id.to_owned(),
        ordinal,
        page_key: page_key.to_owned(),
        entity_type: EntityType::Review,
        entity_id: id.to_owned(),
        operation: Operation::Create,
        before_revision: None,
        after_revision: Some("1".to_owned()),
        before_value: None,
        after_value: Some(review_image(
            id,
            target_event_id,
            target_delta_digest,
            kind,
            reviewer_actor,
        )),
        before_content_id: None,
        after_content_id: None,
    };
    let row = Review {
        id: id.to_owned(),
        domain: domain.to_owned(),
        page_key: page_key.to_owned(),
        target_event_id: target_event_id.to_owned(),
        target_delta_digest: target_delta_digest.to_owned(),
        kind,
        reviewer_actor: reviewer_actor.to_owned(),
        revision: "1".to_owned(),
        created_event_id: event_id.to_owned(),
    };
    (op, row)
}

/// Whether `actor` holds `reviewer` or `owner` in the domain now.
#[must_use]
pub(crate) fn holds_review_role(m: &Manifest, actor: &str) -> bool {
    matches!(m.roles.get(actor), Some(Role::Reviewer | Role::Owner))
}

/// Whether a revision with `reviewers` is addressed to `actor` now:
/// enumerated on it, or addressed to the role while `actor` holds it. An
/// enumerated reviewer must still hold the role as well.
#[must_use]
pub(crate) fn addressed_to(m: &Manifest, reviewers: &Reviewers, actor: &str) -> bool {
    holds_review_role(m, actor)
        && match reviewers {
            Reviewers::Actors(ids) => ids.iter().any(|id| id == actor),
            Reviewers::Role(_) => true,
        }
}

/// Whether a revision is past its expiry at `now_ms` (KG1's rule: expired
/// from the instant itself).
#[must_use]
pub(crate) fn expired(expires_at_ms: Option<i64>, now_ms: i64) -> bool {
    expires_at_ms.is_some_and(|at| now_ms >= at)
}

/// Whether event `event_id` is reviewed: a review targets it, or an event
/// reverses it (a 116.3-era reversal recorded no review row).
#[must_use]
pub(crate) fn reviewed(m: &Manifest, event_id: &str) -> bool {
    m.reviews.values().any(|r| r.target_event_id == event_id)
        || m.events
            .values()
            .any(|(event, _)| event.reverses_event_id.as_deref() == Some(event_id))
}

/// Moves revision `row_id` and every inbox row of it to `status`, setting
/// `reason` and, for a decision, `decided_*`. A no-op for an unknown id.
pub(crate) fn move_revision(
    m: &mut Manifest,
    row_id: &str,
    status: ProposalStatus,
    reason: Option<&str>,
    decided: Option<(i64, &str, Option<&str>)>,
) {
    let Some(state) = m.proposals.get_mut(row_id) else {
        return;
    };
    state.row.status = status;
    state.row.reason = reason.map(str::to_owned);
    if let Some((at_ms, by, event_id)) = decided {
        state.row.decided_at_ms = Some(at_ms);
        state.row.decided_by = Some(by.to_owned());
        state.row.decided_event_id = event_id.map(str::to_owned);
    }
    let prefix = format!("{row_id}/");
    for (id, inbox) in &mut m.inbox {
        if id.starts_with(&prefix) {
            inbox.status = status;
        }
    }
}
