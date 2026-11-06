//! Stable content's revision rule (contract §2, §3, §9.5), at the type level
//! only: schema 6 has no `kg_contents` table and `graph.apply` has no content
//! operation, so nothing here is stored or accepted. The rule is pinned now
//! (the vectors' `contents`) so the story that adds content persistence
//! implements exactly this one.
//!
//! A content image is `{id, owner_node_id, revision, body}`, `body` either
//! `{"inline_text": <string of at most 32 KiB UTF-8>}` or
//! `{"reference": {"resource_node_id": <ULID>, "observed_revision": <decimal> | null}}`.
//! An update `{id, owner_node_id, expected_revision, body}` replaces the body
//! whole: it names the content's own id and owner (both fixed for good — a
//! change of either refuses), its revision moves by exactly one, and nothing
//! else moves: not the owner node's revision, which content has its own of.

use serde_json::{json, Map, Value};

use crate::delta::Op;
use crate::vocabulary::{EntityType, Operation};
use crate::{is_decimal, is_ulid, Refusal};

/// The most UTF-8 bytes an inline text body may have.
pub const MAX_INLINE_TEXT_BYTES: usize = 32 * 1024;

/// What an update does to a content.
#[derive(Clone, Debug, PartialEq)]
pub enum Revision {
    /// The content's event operation: its `update`, ordinal 0.
    Accepted(Op),
    /// `{entity: {entity_type: "content", id}, precondition: "revision",
    /// current_revision}`, the shape of every graph conflict.
    Conflict(Value),
    Refused(Refusal),
}

/// A content body's shape.
pub fn check_body(body: &Value) -> Result<(), Refusal> {
    let fields = body.as_object().ok_or(Refusal::InvalidChange)?;
    match (
        fields.len(),
        fields.get("inline_text"),
        fields.get("reference"),
    ) {
        (1, Some(Value::String(text)), None) => {
            if text.len() > MAX_INLINE_TEXT_BYTES {
                Err(Refusal::ChangeLimit)
            } else {
                Ok(())
            }
        }
        (1, None, Some(Value::Object(reference))) => {
            let resource = reference.get("resource_node_id").and_then(Value::as_str);
            let observed = match reference.get("observed_revision") {
                Some(Value::Null) => true,
                Some(Value::String(revision)) => is_decimal(revision) && revision != "0",
                _ => false,
            };
            if reference.len() == 2 && resource.is_some_and(is_ulid) && observed {
                Ok(())
            } else {
                Err(Refusal::InvalidChange)
            }
        }
        _ => Err(Refusal::InvalidChange),
    }
}

fn text<'a>(map: &'a Map<String, Value>, key: &str) -> Option<&'a str> {
    map.get(key).and_then(Value::as_str)
}

/// Judge `update` against the accepted content image `before`.
#[must_use]
pub fn revise(before: &Map<String, Value>, update: &Map<String, Value>) -> Revision {
    let closed = update.len() == 4
        && ["id", "owner_node_id", "expected_revision", "body"]
            .iter()
            .all(|key| update.contains_key(*key));
    let (Some(id), Some(owner), Some(expected)) = (
        text(update, "id"),
        text(update, "owner_node_id"),
        text(update, "expected_revision"),
    ) else {
        return Revision::Refused(Refusal::InvalidChange);
    };
    if !closed || !is_ulid(id) || !is_ulid(owner) || !is_decimal(expected) || expected == "0" {
        return Revision::Refused(Refusal::InvalidChange);
    }
    // Identity is fixed: the update names this content and its one owner.
    if text(before, "id") != Some(id) || text(before, "owner_node_id") != Some(owner) {
        return Revision::Refused(Refusal::InvalidChange);
    }
    let current = text(before, "revision").unwrap_or_default();
    if current != expected {
        return Revision::Conflict(json!({
            "entity": {"entity_type": "content", "id": id},
            "precondition": "revision",
            "current_revision": current,
        }));
    }
    let body = &update["body"];
    if let Err(refusal) = check_body(body) {
        return Revision::Refused(refusal);
    }
    let Some(next) = current
        .parse::<u128>()
        .ok()
        .and_then(|n| n.checked_add(1))
        .map(|n| n.to_string())
        .filter(|n| n.len() <= 20)
    else {
        return Revision::Refused(Refusal::InvalidChange);
    };
    let mut after = before.clone();
    after.insert("revision".into(), json!(next));
    after.insert("body".into(), body.clone());
    Revision::Accepted(Op {
        ordinal: 0,
        entity_type: EntityType::Content,
        entity_id: id.to_owned(),
        operation: Operation::Update,
        before_revision: Some(current.to_owned()),
        after_revision: Some(next),
        before_value: Some(Value::Object(before.clone())),
        after_value: Some(Value::Object(after)),
    })
}
