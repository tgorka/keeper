//! The synced rows of schema 10 (`syncular/migrations/0010_files/up.sql`),
//! field for column, and the values their JSON columns hold.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::names::FileMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    File,
    Dir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Live,
    Deleted,
}

/// One `files_entries` row. A drive's root is a `dir` whose `parent_id` is
/// its own `id`; a `dir` has null `content_id`, `size` and `media_type`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    pub id: String,
    pub domain: String,
    pub drive_key: String,
    pub parent_id: String,
    pub name: String,
    pub name_fold: String,
    pub kind: Kind,
    pub content_id: Option<String>,
    pub size: Option<u64>,
    pub media_type: Option<String>,
    pub mtime_ms: Option<i64>,
    pub mode: FileMode,
    pub revision: String,
    pub lifecycle: Lifecycle,
    pub created_event_id: String,
    pub updated_event_id: String,
    pub updated_seq: String,
}

impl Entry {
    /// Whether the entry is a drive's root.
    #[must_use]
    pub fn is_root(&self) -> bool {
        self.parent_id == self.id
    }
}

/// `origin_class` (decision 13): KG1's `human` for a person.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OriginClass {
    Human,
    Agent,
    Tool,
    Import,
    Job,
}

/// `files_events.approval_basis`: what the event was accepted on
/// (decision 13). 116.1 writes `direct`, `grant` and `import`; the others
/// are frozen here for 116.6 and 120.2.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "basis", rename_all = "snake_case", deny_unknown_fields)]
pub enum ApprovalBasis {
    Direct,
    Approval {
        approval_id: String,
    },
    Proposal {
        proposal_id: String,
        proposal_revision: String,
        proposal_digest: String,
        proposer_actor: String,
        proposer_client: String,
        proposer_context: Option<String>,
    },
    Grant {
        grant_id: String,
        grant_revision: String,
        mode: keeper_accounts_api::grant::GrantMode,
    },
    Release {
        release_id: String,
        released_by_actor: String,
        released_by_client: String,
    },
    Import,
}

/// One `files_events` row.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub id: String,
    pub domain: String,
    pub seq: String,
    pub page_key: String,
    pub descriptor: String,
    pub request_id: String,
    pub request_digest: String,
    pub actor_id: String,
    pub client_id: String,
    pub origin_class: OriginClass,
    pub origin_actor: Option<String>,
    pub origin_chain: Option<Value>,
    pub context_id: Option<String>,
    pub approval_id: Option<String>,
    pub approval_basis: ApprovalBasis,
    pub proposal_id: Option<String>,
    pub change_set_id: Option<String>,
    pub job_id: Option<String>,
    pub job_ordinal: Option<i64>,
    pub job_attempt_id: Option<String>,
    pub reverses_event_id: Option<String>,
    pub event_time_ms: Option<i64>,
    pub recorded_at_ms: i64,
    pub op_count: u32,
    pub delta_digest: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EntityType {
    Entry,
    Review,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation {
    Create,
    Replace,
    Move,
    Delete,
    Restore,
}

/// One `files_event_ops` row: `id` is `<event_id>/<ordinal>`; the values
/// hold only the fields the operation changed (decision 15).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Op {
    pub id: String,
    pub domain: String,
    pub event_id: String,
    pub ordinal: u32,
    pub page_key: String,
    pub entity_type: EntityType,
    pub entity_id: String,
    pub operation: Operation,
    pub before_revision: Option<String>,
    pub after_revision: Option<String>,
    pub before_value: Option<Value>,
    pub after_value: Option<Value>,
    pub before_content_id: Option<String>,
    pub after_content_id: Option<String>,
}

/// One drive of `files_head.drives`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Drive {
    pub name: String,
    pub root_id: String,
}

/// The `files_head` row: `id` and `domain` the domain.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Head {
    pub id: String,
    pub domain: String,
    pub seq: String,
    pub event_id: String,
    pub digest: String,
    pub drives: Vec<Drive>,
}

/// A `files_receipts.outcome`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Accepted,
    Proposed,
    Conflict,
    Rejected,
}
