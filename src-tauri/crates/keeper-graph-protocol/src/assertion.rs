//! Assertion metadata (contract §3): the reserved node property member
//! `$assertion`, holding exactly the six [`ASSERTION_FIELDS`]:
//!
//! | field | value |
//! | --- | --- |
//! | `generated_at`, `observed_at`, `stale_after` | an integer ms in `0..=MAX_EVENT_TIME_MS`, or `null` (unknown) |
//! | `verified_at` | as above; non-null exactly when `status` is `verified` |
//! | `verified_by` | the verifying person's actor id; non-null exactly when `status` is `verified` |
//! | `status` | `unverified` or `verified` |
//!
//! The recorded time is the event's `recorded_at_ms`, never a property.
//!
//! Only `verify_assertion` (a person's, `crate::candidate`) makes an
//! assertion `verified`, stamping `verified_at`/`verified_by` from the
//! server's acceptance; a written `$assertion` ([`check_written`]) is
//! always `unverified` with both null. Verification binds the exact state
//! the person checked; whether it is still *current* is read, never
//! written: [`is_current`].

use serde_json::{Map, Value};

use crate::change::MAX_EVENT_TIME_MS;
use crate::value::{typed_ref, RefType, TypedRef};
use crate::vocabulary::ASSERTION_FIELDS;
use crate::Refusal;

/// The reserved property member that holds a node's assertion metadata.
pub const MEMBER: &str = "$assertion";
/// The most bytes a `verified_by` actor id may have.
pub const MAX_ACTOR_BYTES: usize = 256;

fn time(value: Option<&Value>) -> Result<Option<u64>, Refusal> {
    match value {
        Some(Value::Null) => Ok(None),
        Some(value) => match value.as_u64() {
            Some(ms) if ms <= MAX_EVENT_TIME_MS => Ok(Some(ms)),
            _ => Err(Refusal::InvalidChange),
        },
        None => Err(Refusal::InvalidChange),
    }
}

/// Whether `id` may stand as a `verified_by`: a non-empty actor id without
/// control characters, at most [`MAX_ACTOR_BYTES`].
#[must_use]
pub fn actor_ok(id: &str) -> bool {
    !id.is_empty() && id.len() <= MAX_ACTOR_BYTES && !id.chars().any(char::is_control)
}

/// Check a stored `$assertion` value: exactly the six fields, each of its
/// type, and `verified_at`/`verified_by` set exactly when `verified`.
pub fn check(value: &Value) -> Result<(), Refusal> {
    let fields = value.as_object().ok_or(Refusal::InvalidChange)?;
    if fields.len() != ASSERTION_FIELDS.len()
        || !ASSERTION_FIELDS.iter().all(|f| fields.contains_key(*f))
    {
        return Err(Refusal::InvalidChange);
    }
    for field in ["generated_at", "observed_at", "stale_after"] {
        time(fields.get(field))?;
    }
    let verified_at = time(fields.get("verified_at"))?;
    let verified_by = match fields.get("verified_by") {
        Some(Value::Null) => None,
        Some(Value::String(id)) if actor_ok(id) => Some(id),
        _ => return Err(Refusal::InvalidChange),
    };
    match fields.get("status").and_then(Value::as_str) {
        Some("unverified") if verified_at.is_none() && verified_by.is_none() => Ok(()),
        Some("verified") if verified_at.is_some() && verified_by.is_some() => Ok(()),
        _ => Err(Refusal::InvalidChange),
    }
}

/// Check an `$assertion` a `create_node` or `update_node` writes: a stored
/// shape that is `unverified`. Forged `verified_*` or `status: verified`
/// refuse, from anyone: only `verify_assertion` verifies.
pub fn check_written(value: &Value) -> Result<(), Refusal> {
    check(value)?;
    if value.get("status").and_then(Value::as_str) == Some("unverified") {
        Ok(())
    } else {
        Err(Refusal::InvalidChange)
    }
}

/// The `$assertion` of a node image, if it carries one.
#[must_use]
pub fn of(node: &Map<String, Value>) -> Option<&Map<String, Value>> {
    node.get("properties")?.get(MEMBER)?.as_object()
}

/// Whether a node image carries a `verified` assertion.
#[must_use]
pub fn is_verified(node: &Map<String, Value>) -> bool {
    of(node)
        .and_then(|a| a.get("status"))
        .and_then(Value::as_str)
        == Some("verified")
}

/// Clear a node image's verification in place: `status` `unverified`,
/// `verified_at`/`verified_by` null. Every write to a verified assertion
/// other than its verification does this in the same operation. Returns
/// whether anything changed.
pub fn clear(node: &mut Map<String, Value>) -> bool {
    if !is_verified(node) {
        return false;
    }
    let Some(Value::Object(assertion)) = node.get_mut("properties").and_then(|p| p.get_mut(MEMBER))
    else {
        return false;
    };
    assertion.insert("status".into(), Value::from("unverified"));
    assertion.insert("verified_at".into(), Value::Null);
    assertion.insert("verified_by".into(), Value::Null);
    true
}

/// Mark a node image verified at `time_ms` by `actor_id`, in place.
pub fn verify(node: &mut Map<String, Value>, time_ms: u64, actor_id: &str) -> Result<(), Refusal> {
    if time_ms > MAX_EVENT_TIME_MS || !actor_ok(actor_id) {
        return Err(Refusal::InvalidChange);
    }
    let Some(Value::Object(assertion)) = node.get_mut("properties").and_then(|p| p.get_mut(MEMBER))
    else {
        return Err(Refusal::InvalidChange);
    };
    assertion.insert("status".into(), Value::from("verified"));
    assertion.insert("verified_at".into(), Value::from(time_ms));
    assertion.insert("verified_by".into(), Value::from(actor_id));
    Ok(())
}

/// The typed reference a `sources` edge checks: the one top-level property
/// of the edge whose value is a typed reference. `None` when it has none,
/// or more than one (no single checked source), or a malformed one.
#[must_use]
pub fn source_ref(edge: &Map<String, Value>) -> Option<TypedRef> {
    let properties = edge.get("properties")?.as_object()?;
    let mut found = None;
    for value in properties.values() {
        match typed_ref(value) {
            Ok(Some(reference)) => {
                if found.replace(reference).is_some() {
                    return None;
                }
            }
            Ok(None) => {}
            Err(_) => return None,
        }
    }
    found
}

/// What a currency check reads of the graph as it is now.
pub trait Current {
    /// The node `id`, any lifecycle.
    fn node(&self, id: &str) -> Option<Map<String, Value>>;
    /// The content `id`.
    fn content(&self, id: &str) -> Option<Map<String, Value>>;
    /// The active outgoing `sources` edges of node `id`, ascending by id,
    /// at most `limit` of them.
    fn sources_from(&self, id: &str, limit: usize) -> Vec<Map<String, Value>>;
}

/// The most active `sources` edges a verification checks; a currency read
/// over more is not current.
pub const MAX_CHECKED_SOURCES: usize = 64;
/// The most contents of its assertion a verification checks.
pub const MAX_CHECKED_CONTENTS: usize = 64;

/// The current revision a `sources` edge's reference names, when it
/// resolves: a node reference must name the edge's `to_id`, a content
/// reference a content owned by `to_id`.
#[must_use]
pub fn resolve<C: Current + ?Sized>(
    current: &C,
    edge: &Map<String, Value>,
    reference: &TypedRef,
) -> Option<String> {
    let to = edge.get("to_id")?.as_str()?;
    let entity = match reference.entity_type {
        RefType::Node if reference.id == to => current.node(&reference.id)?,
        RefType::Content => {
            let content = current.content(&reference.id)?;
            if content.get("owner_node_id")?.as_str()? != to {
                return None;
            }
            content
        }
        RefType::Node => return None,
    };
    entity.get("revision")?.as_str().map(str::to_owned)
}

/// Whether node `id`'s verification is current: it is active and
/// `verified` with a `verified_at`, it has at least one active `sources`
/// edge (at most [`MAX_CHECKED_SOURCES`]), and every one's reference
/// resolves with a non-null revision equal to its entity's current
/// revision. A null, absent or unresolvable revision is not current, and a
/// changed source makes it not current with no write.
#[must_use]
pub fn is_current<C: Current + ?Sized>(current: &C, id: &str) -> bool {
    let Some(node) = current.node(id) else {
        return false;
    };
    let verified_at = of(&node)
        .and_then(|a| a.get("verified_at"))
        .is_some_and(|v| !v.is_null());
    if !is_verified(&node)
        || !verified_at
        || node.get("lifecycle").and_then(Value::as_str) != Some("active")
    {
        return false;
    }
    let edges = current.sources_from(id, MAX_CHECKED_SOURCES + 1);
    if edges.is_empty() || edges.len() > MAX_CHECKED_SOURCES {
        return false;
    }
    edges.iter().all(|edge| {
        source_ref(edge).is_some_and(|reference| {
            reference.revision.is_some() && resolve(current, edge, &reference) == reference.revision
        })
    })
}
