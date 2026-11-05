//! The fixed reserved vocabulary. Reserved names have one meaning that no
//! custom vocabulary can define, alias or deprecate; a custom name is always
//! namespaced (`<namespace>:<name>`), so it can never spell a reserved one.

use crate::Refusal;
use serde::{Deserialize, Serialize};

/// The reserved node kinds, in the contract's order. `Tag` is a projected
/// kind only: a tag is a `kg_tags` row, never a `kg_nodes` row.
pub const RESERVED_KINDS: [&str; 5] = ["Concept", "Collection", "Resource", "Tag", "Guidance"];
/// The reserved relationship types. `HAS_TAG` is a projected type only: a
/// tag membership is a `kg_node_tags` row, never a `kg_edges` row.
pub const RESERVED_RELATIONS: [&str; 4] = ["HAS_TAG", "contains", "guidance_scope", "sources"];
/// A node's structural fields: no custom property may shadow one.
pub const NODE_FIELDS: [&str; 9] = [
    "id",
    "kind",
    "title",
    "description",
    "description_source_id",
    "lifecycle",
    "lifecycle_revision",
    "revision",
    "merged_into_id",
];
/// An edge's structural fields: no custom property may shadow one.
pub const EDGE_FIELDS: [&str; 6] = ["id", "from_id", "to_id", "type", "lifecycle", "revision"];
/// A projected Tag node's fields: its id and its canonical text.
pub const TAG_FIELDS: [&str; 2] = ["id", "text"];
/// The bytes a custom (namespaced) kind, type or property key may hold.
pub const MAX_NAME_BYTES: usize = 256;

/// A node's or edge's lifecycle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Active,
    Archived,
}

/// What an event operation changes. `content` and `review` are reserved for
/// the stories that add their tables; schema 6 holds no row of either.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Node,
    Edge,
    Tag,
    NodeTag,
    Content,
    Review,
}

/// What an event operation does to its entity.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Create,
    Update,
    Archive,
    Unarchive,
    /// Only a tag membership is ever removed.
    Remove,
}

/// Who an accepted event is attributed to, derived by the server from the
/// authenticated credential and context, never from a payload.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginClass {
    Human,
    Agent,
}

/// A receipt's terminal outcome. `pending` is never a server outcome.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Accepted,
    Proposed,
    Conflict,
    Rejected,
}

/// The contract's stable error codes (§7).
pub const ERRORS: [&str; 16] = [
    "unauthorised",
    "forbidden",
    "unsupported_version",
    "invalid_change",
    "change_limit",
    "revision_conflict",
    "idempotency_mismatch",
    "proposal_stale",
    "reversal_conflict",
    "query_unsupported",
    "query_limit",
    "query_cancelled",
    "projection_unavailable",
    "projection_stale",
    "sync_pending",
    "storage_unavailable",
];

/// A namespaced custom name: `<namespace>:<name>`, both non-empty, no
/// control character, at most [`MAX_NAME_BYTES`].
#[must_use]
pub fn is_custom_name(s: &str) -> bool {
    s.len() <= MAX_NAME_BYTES
        && !s.chars().any(char::is_control)
        && s.split_once(':')
            .is_some_and(|(namespace, name)| !namespace.is_empty() && !name.is_empty())
}

/// A node kind a `kg_nodes` row may hold: a reserved kind other than the
/// projected `Tag`, or a custom name.
pub fn stored_node_kind(kind: &str) -> Result<(), Refusal> {
    if (RESERVED_KINDS.contains(&kind) && kind != "Tag") || is_custom_name(kind) {
        Ok(())
    } else {
        Err(Refusal::InvalidChange)
    }
}

/// A relationship type a `kg_edges` row may hold: a reserved type other than
/// the projected `HAS_TAG`, or a custom name.
pub fn stored_edge_type(kind: &str) -> Result<(), Refusal> {
    if (RESERVED_RELATIONS.contains(&kind) && kind != "HAS_TAG") || is_custom_name(kind) {
        Ok(())
    } else {
        Err(Refusal::InvalidChange)
    }
}

/// Whether a custom property key would shadow a structural field of a node,
/// an edge or a projected tag.
#[must_use]
pub fn is_structural_field(key: &str) -> bool {
    NODE_FIELDS.contains(&key) || EDGE_FIELDS.contains(&key) || TAG_FIELDS.contains(&key)
}
