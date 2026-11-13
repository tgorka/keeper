//! A review of one accepted event (contract §9.5, schema 8): an
//! acknowledgement by a human reviewer, or the linkage of a reversal.
//!
//! A review is an accepted `review` event operation and the `kg_reviews`
//! row equal to its after-image, inserted together and never changed:
//! `entity_type review`, `create`, a new ULID, no before-image, after-image
//! `{id, target_event_id, target_delta_digest, kind, reviewer_actor,
//! revision: "1"}`, `after_revision "1"`. It holds identities and a digest
//! only, never a value of the reviewed event, so it carries no typed
//! reference.
//!
//! [`ACKNOWLEDGE`]'s payload is `{"target_event_id": <ULID>,
//! "target_delta_digest": <hex>}` and nothing else; its accepted event holds
//! exactly one review operation of kind [`ACKNOWLEDGEMENT`]
//! ([`acknowledgement`]). A reversal's event holds its inverse operations
//! and then one review operation of kind [`REVERSAL`]
//! ([`crate::candidate::apply_inverse`]). The reviewed event's bytes and
//! digest never change.

use serde_json::{json, Map, Value};

use crate::assertion::actor_ok;
use crate::candidate::Effects;
use crate::delta::{self, Op};
use crate::vocabulary::{EntityType, Operation};
use crate::worker::is_hex_digest;
use crate::{is_ulid, Refusal};

/// The descriptor of a human reviewer's acknowledgement.
pub const ACKNOWLEDGE: &str = "graph.acknowledge";
/// A review's kind: a human reviewer saw the event.
pub const ACKNOWLEDGEMENT: &str = "acknowledgement";
/// A review's kind: the event a reversal reverses.
pub const REVERSAL: &str = "reversal";
/// The review kinds.
pub const KINDS: [&str; 2] = [ACKNOWLEDGEMENT, REVERSAL];
/// A review image's fields.
pub const FIELDS: [&str; 6] = [
    "id",
    "target_event_id",
    "target_delta_digest",
    "kind",
    "reviewer_actor",
    "revision",
];

/// The event a review names: its id and its delta digest.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub event_id: String,
    pub delta_digest: String,
}

impl Target {
    /// A ULID event and a lowercase SHA-256 hex digest.
    pub fn new(event_id: &str, delta_digest: &str) -> Result<Self, Refusal> {
        if is_ulid(event_id) && is_hex_digest(delta_digest) {
            Ok(Self {
                event_id: event_id.to_owned(),
                delta_digest: delta_digest.to_owned(),
            })
        } else {
            Err(Refusal::InvalidChange)
        }
    }
}

/// The review image `id` of `kind` over `target` by `reviewer`.
pub fn image(
    id: &str,
    target: &Target,
    kind: &str,
    reviewer: &str,
) -> Result<Map<String, Value>, Refusal> {
    let image = json!({
        "id": id, "target_event_id": target.event_id,
        "target_delta_digest": target.delta_digest, "kind": kind,
        "reviewer_actor": reviewer, "revision": "1",
    });
    match image {
        Value::Object(image) if image_ok(id, &image) => Ok(image),
        _ => Err(Refusal::InvalidChange),
    }
}

/// Whether `image` is review `id` as the contract writes one: exactly the
/// [`FIELDS`], a ULID id and target, a hex digest, a kind of [`KINDS`], an
/// actor id and revision `"1"`.
#[must_use]
pub fn image_ok(id: &str, image: &Map<String, Value>) -> bool {
    let text = |key: &str| image.get(key).and_then(Value::as_str);
    image.len() == FIELDS.len()
        && FIELDS.iter().all(|field| image.contains_key(*field))
        && is_ulid(id)
        && text("id") == Some(id)
        && text("target_event_id").is_some_and(is_ulid)
        && text("target_delta_digest").is_some_and(is_hex_digest)
        && text("kind").is_some_and(|kind| KINDS.contains(&kind))
        && text("reviewer_actor").is_some_and(actor_ok)
        && text("revision") == Some("1")
}

/// The review operation at `ordinal` creating `image`.
#[must_use]
pub fn op(ordinal: u32, image: Map<String, Value>) -> Op {
    Op {
        ordinal,
        entity_type: EntityType::Review,
        entity_id: image
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
        operation: Operation::Create,
        before_revision: None,
        after_revision: Some("1".to_owned()),
        before_value: None,
        after_value: Some(Value::Object(image)),
    }
}

/// [`ACKNOWLEDGE`]'s payload: the event acknowledged.
pub fn parse_acknowledge(payload: &Value) -> Result<Target, Refusal> {
    let fields = payload.as_object().ok_or(Refusal::InvalidChange)?;
    let text = |key: &str| fields.get(key).and_then(Value::as_str);
    match (
        fields.len(),
        text("target_event_id"),
        text("target_delta_digest"),
    ) {
        (2, Some(event_id), Some(digest)) => Target::new(event_id, digest),
        _ => Err(Refusal::InvalidChange),
    }
}

/// The payload [`parse_acknowledge`] reads back as `target`.
#[must_use]
pub fn acknowledge_payload(target: &Target) -> Value {
    json!({"target_event_id": target.event_id, "target_delta_digest": target.delta_digest})
}

/// The accepted event of an acknowledgement of `target` by `reviewer`: its
/// one review operation, its digest, and `affected` naming the review.
pub fn acknowledgement(id: &str, target: &Target, reviewer: &str) -> Result<Effects, Refusal> {
    let ops = vec![op(0, image(id, target, ACKNOWLEDGEMENT, reviewer)?)];
    let delta_digest = delta::digest(&ops)?;
    Ok(Effects {
        ops,
        delta_digest,
        affected: vec![json!({"entity_type": "review", "id": id, "revision": "1"})],
    })
}
