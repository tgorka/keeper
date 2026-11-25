//! The closed set of refusal codes a `files.*` command answers (decision 14:
//! every refusal the README describes has one, and the vectors pin each).

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A stable refusal code, on the wire in snake case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Code {
    UnsupportedVersion,
    IdempotencyMismatch,
    InvalidChange,
    InvalidName,
    ReservedName,
    ReservedField,
    Forbidden,
    OutsideRoots,
    ProposeRequired,
    UnknownMode,
    Conflict,
    ParentInvalid,
    MoveIntoSelf,
    NotEmpty,
    ContentAbsent,
    ChangeLimit,
    /// A proposal revision that is no longer open, current, unexpired or
    /// applicable (story 116.6): terminal, never rebased.
    ProposalStale,
}

impl Code {
    /// Every code, in the README's order.
    pub const ALL: [Self; 17] = [
        Self::UnsupportedVersion,
        Self::IdempotencyMismatch,
        Self::InvalidChange,
        Self::InvalidName,
        Self::ReservedName,
        Self::ReservedField,
        Self::Forbidden,
        Self::OutsideRoots,
        Self::ProposeRequired,
        Self::UnknownMode,
        Self::Conflict,
        Self::ParentInvalid,
        Self::MoveIntoSelf,
        Self::NotEmpty,
        Self::ContentAbsent,
        Self::ChangeLimit,
        Self::ProposalStale,
    ];
}

/// One refusal: its code and, for a `conflict`, the precondition that
/// failed (`{entity_id, precondition, current_revision?}`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Refusal {
    pub code: Code,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Value>,
}

impl Refusal {
    #[must_use]
    pub const fn new(code: Code) -> Self {
        Self { code, detail: None }
    }

    #[must_use]
    pub fn with_detail(code: Code, detail: Value) -> Self {
        Self {
            code,
            detail: Some(detail),
        }
    }
}
