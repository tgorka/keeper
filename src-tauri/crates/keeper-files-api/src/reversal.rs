//! `files.preview_reversal`: one event's server-planned inverse and whether it
//! applies now (`syncular/README.md`, *The drive*, *Reversals*; story 116.3).
//! The inverse itself is [`crate::rules::inverse`]; `files.reverse` takes only
//! the event's id, never operations.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::refusal::Code;
use crate::wire::WireOp;

/// `files.preview_reversal`'s params.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PreviewParams {
    pub version: u32,
    pub event_id: String,
}

/// Whether the planned inverse applies now.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PreviewStatus {
    /// `files.reverse` of the event by this caller would be accepted now.
    Ready,
    /// It would be refused `conflict` (`detail` names the entity and the
    /// precondition: `absent`, `revision`, `lifecycle`, `name` or
    /// `dependency`).
    Conflict,
    /// It would be refused with any other code.
    Rejected,
}

/// The answer to `files.preview_reversal`: the plan `files.reverse` would
/// re-derive (empty when the event has no inverse), the status, and for a
/// refusal its code and detail. Nothing is written.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Preview {
    pub target_event_id: String,
    pub status: PreviewStatus,
    pub plan: Vec<WireOp>,
    #[serde(default)]
    pub code: Option<Code>,
    #[serde(default)]
    pub detail: Option<Value>,
}
