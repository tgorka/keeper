//! The request envelope and the commands' payloads (decisions 11, 12, 17).
//! Every object is closed; [`crate::rules::evaluate`] parses them in the
//! contract's order, so a malformed request gets the code its first fault has.

use serde::{Deserialize, Serialize};

use crate::names::FileMode;
use crate::rows::Kind;

/// The registered command descriptors of this contract. `files.reverse` is
/// 116.3's; 116.6 adds `files.propose`, `files.decide`, `files.acknowledge`.
pub const DESCRIPTORS: [&str; 7] = [
    "files.change",
    "files.put",
    "files.move",
    "files.delete",
    "files.restore",
    "files.import",
    "files.reverse",
];

/// A job's effect identity: reserved, refused when set (epic 122).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobRef {
    pub job_id: String,
    pub ordinal: i64,
    pub attempt_id: String,
}

/// Every `files.*` command's params.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope {
    pub version: u32,
    pub request_id: String,
    pub context_id: Option<String>,
    #[serde(default)]
    pub approval_id: Option<String>,
    #[serde(default)]
    pub job: Option<JobRef>,
    pub payload: serde_json::Value,
}

/// Where an imported operation came from: kept in server-only records.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub drive: String,
    pub commit: String,
    pub path: String,
}

/// One operation of a change (`op` its kind). A `files.import` operation
/// carries `provenance` and, on a file, `git_mode` in place of `mode` and
/// `mtime_ms`; any other command's carries neither.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum WireOp {
    Create {
        id: String,
        kind: Kind,
        parent_id: Option<String>,
        name: String,
        #[serde(default)]
        content_id: Option<String>,
        #[serde(default)]
        size: Option<u64>,
        #[serde(default)]
        mtime_ms: Option<i64>,
        #[serde(default)]
        mode: Option<FileMode>,
        #[serde(default)]
        git_mode: Option<String>,
        #[serde(default)]
        provenance: Option<Provenance>,
    },
    Replace {
        id: String,
        expected_revision: String,
        content_id: String,
        size: u64,
        #[serde(default)]
        mtime_ms: Option<i64>,
        #[serde(default)]
        mode: Option<FileMode>,
        #[serde(default)]
        git_mode: Option<String>,
        #[serde(default)]
        provenance: Option<Provenance>,
    },
    Move {
        id: String,
        expected_revision: String,
        parent_id: String,
        name: String,
        #[serde(default)]
        provenance: Option<Provenance>,
    },
    Delete {
        id: String,
        expected_revision: String,
        #[serde(default)]
        provenance: Option<Provenance>,
    },
    Restore {
        id: String,
        expected_revision: String,
        #[serde(default)]
        provenance: Option<Provenance>,
    },
}

impl WireOp {
    /// The entity the operation names.
    #[must_use]
    pub fn id(&self) -> &str {
        match self {
            Self::Create { id, .. }
            | Self::Replace { id, .. }
            | Self::Move { id, .. }
            | Self::Delete { id, .. }
            | Self::Restore { id, .. } => id,
        }
    }

    #[must_use]
    pub fn provenance(&self) -> Option<&Provenance> {
        match self {
            Self::Create { provenance, .. }
            | Self::Replace { provenance, .. }
            | Self::Move { provenance, .. }
            | Self::Delete { provenance, .. }
            | Self::Restore { provenance, .. } => provenance.as_ref(),
        }
    }
}

/// `files.change`'s payload.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChangePayload {
    pub operations: Vec<WireOp>,
    #[serde(default)]
    pub change_set_id: Option<String>,
    #[serde(default)]
    pub event_time_ms: Option<i64>,
}

/// A shorthand's payload (`files.put`, `files.move`, `files.delete`,
/// `files.restore`): a one-operation change.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ShorthandPayload {
    pub operation: WireOp,
    #[serde(default)]
    pub change_set_id: Option<String>,
}

/// `files.import`'s payload: the import identity's alone.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ImportPayload {
    pub operations: Vec<WireOp>,
    pub change_set_id: String,
    pub event_time_ms: i64,
}

/// `files.reverse`'s payload (116.3 builds the command).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ReversePayload {
    pub event_id: String,
}
