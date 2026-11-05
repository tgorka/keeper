//! An accepted event's ordered operations and their digest (contract §9.5).
//!
//! An event header records `op_count` and `delta_digest`; its operations are
//! `kg_event_ops` rows with ordinals `0..op_count`. The delta digest is the
//! SHA-256 of the JCS bytes of the ordered array of operation objects, each
//! holding exactly the row's `ordinal`, `entity_type`, `entity_id`,
//! `operation`, `before_revision`, `after_revision`, `before_value` and
//! `after_value` (absent as `null`). A projection publishes a prefix only
//! when every event's group is complete and its digest matches.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::value::canonical;
use crate::vocabulary::{EntityType, Operation};
use crate::{is_decimal, is_ulid, Refusal};

/// The most operations one event may hold.
pub const MAX_OPERATIONS: usize = 128;
/// The most canonical bytes one operation may have.
pub const MAX_OPERATION_BYTES: usize = 128 * 1024;
/// The most canonical bytes one event's whole delta, before-images included,
/// may have.
pub const MAX_DELTA_BYTES: usize = 2 * 1024 * 1024;

/// One operation of an event, as its `kg_event_ops` row holds it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Op {
    pub ordinal: u32,
    pub entity_type: EntityType,
    pub entity_id: String,
    pub operation: Operation,
    pub before_revision: Option<String>,
    pub after_revision: Option<String>,
    pub before_value: Option<Value>,
    pub after_value: Option<Value>,
}

impl Op {
    /// The shape rules of one operation: a ULID entity, canonical decimal
    /// revisions, and the images its operation implies. A create has no
    /// before-image; a removal has no after-image and is only ever of a tag
    /// membership; every other operation has both. A tag membership carries
    /// no revision.
    fn check(&self) -> Result<(), Refusal> {
        let revisions = [&self.before_revision, &self.after_revision];
        if !is_ulid(&self.entity_id)
            || revisions
                .iter()
                .any(|revision| revision.as_deref().is_some_and(|r| !is_decimal(r)))
        {
            return Err(Refusal::InvalidChange);
        }
        let (before, after) = (self.before_value.is_some(), self.after_value.is_some());
        let images = match self.operation {
            Operation::Create => !before && after,
            Operation::Remove => self.entity_type == EntityType::NodeTag && before && !after,
            Operation::Update | Operation::Archive | Operation::Unarchive => before && after,
        };
        let revised = match self.entity_type {
            EntityType::NodeTag => revisions.iter().all(|r| r.is_none()),
            _ => {
                self.after_revision.is_some()
                    && (self.before_revision.is_some() == before)
                    && self.operation != Operation::Remove
            }
        };
        if !images || !revised {
            return Err(Refusal::InvalidChange);
        }
        Ok(())
    }
}

/// The delta digest of one event's operations, after checking that they
/// are one complete group: 1 to [`MAX_OPERATIONS`] operations, ordinals
/// `0..n` in order, each within [`MAX_OPERATION_BYTES`] and all within
/// [`MAX_DELTA_BYTES`].
pub fn digest(ops: &[Op]) -> Result<String, Refusal> {
    if ops.is_empty() {
        return Err(Refusal::InvalidChange);
    }
    if ops.len() > MAX_OPERATIONS {
        return Err(Refusal::ChangeLimit);
    }
    let mut total = 2 + ops.len() - 1;
    let mut array = Vec::with_capacity(ops.len());
    for (ordinal, op) in ops.iter().enumerate() {
        if usize::try_from(op.ordinal).ok() != Some(ordinal) {
            return Err(Refusal::InvalidChange);
        }
        op.check()?;
        let value = serde_json::to_value(op).map_err(|_| Refusal::InvalidChange)?;
        let bytes = canonical(&value)?;
        if bytes.len() > MAX_OPERATION_BYTES {
            return Err(Refusal::ChangeLimit);
        }
        total += bytes.len();
        if total > MAX_DELTA_BYTES {
            return Err(Refusal::ChangeLimit);
        }
        array.push(value);
    }
    crate::value::digest(&Value::Array(array))
}
