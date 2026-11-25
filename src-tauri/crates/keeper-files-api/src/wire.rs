//! The request envelope and the commands' payloads (decisions 11, 12, 17).
//! Every object is closed; [`crate::rules::evaluate`] parses them in the
//! contract's order, so a malformed request gets the code its first fault has.

use serde::{Deserialize, Serialize};

use crate::names::FileMode;
use crate::rows::Kind;

/// The registered command descriptors of this contract. `files.reverse` is
/// 116.3's; `files.propose`, `files.decide` and `files.acknowledge` 116.6's.
pub const DESCRIPTORS: [&str; 10] = [
    "files.change",
    "files.put",
    "files.move",
    "files.delete",
    "files.restore",
    "files.import",
    "files.reverse",
    "files.propose",
    "files.decide",
    "files.acknowledge",
];

/// A job's effect identity (epic 122): accepted only from a job caller, whose
/// attempt the server's registration point judged current (story 116.6);
/// from anyone else `reserved_field`.
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

/// The most reviewers a revision enumerates.
pub const REVIEWERS_MAX: usize = 16;

/// The most items one `files.decide` or `files.acknowledge` list holds (the
/// change bound, [`crate::CHANGE_LIMIT`]).
pub const LIST_MAX: usize = crate::CHANGE_LIMIT;

/// Who may decide a revision: 1 to [`REVIEWERS_MAX`] ascending distinct
/// person actor ids, or the role — any person holding `reviewer` or `owner`
/// in the domain when the decision is made (`{"role": "reviewer"}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Reviewers {
    Actors(Vec<String>),
    Role(RoleReviewers),
}

/// The role form of [`Reviewers`]: `{"role": "reviewer"}`, nothing else.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RoleReviewers {
    pub role: ReviewerRole,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewerRole {
    Reviewer,
}

/// A proposal's `change`: one `files.change` payload without its own
/// `change_set_id` (the proposal's is the one).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposedChange {
    pub operations: Vec<WireOp>,
    #[serde(default)]
    pub event_time_ms: Option<i64>,
}

/// `files.propose`'s payload: a new proposal (`base_revision` null,
/// revision 1) or the next revision of the proposer's own open one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProposePayload {
    pub proposal_id: String,
    pub base_revision: Option<String>,
    pub change: ProposedChange,
    pub reviewers: Reviewers,
    pub expires_at_ms: Option<i64>,
    #[serde(default)]
    pub change_set_id: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Approve,
    Reject,
}

/// One decision of a `files.decide` list.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Decision {
    pub proposal_id: String,
    pub proposal_revision: String,
    pub proposal_digest: String,
    pub decision: Verdict,
}

/// `files.decide`'s payload: 1 to [`LIST_MAX`] decisions, distinct
/// revisions, whose revisions' `op_count` sum to at most the change bound.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DecidePayload {
    pub decisions: Vec<Decision>,
}

/// The event a review names: its id and its `delta_digest`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub target_event_id: String,
    pub target_delta_digest: String,
}

/// `files.acknowledge`'s payload: 1 to [`LIST_MAX`] distinct targets,
/// acknowledged as one event or refused whole.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AcknowledgePayload {
    pub targets: Vec<Target>,
}
