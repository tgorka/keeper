//! The rows of `jobs`, `job_claims` and `job_capabilities` (migration
//! `0012_jobs`), the values they hold, and the view model a client lists.

use std::fmt;

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use crate::{is_capability, is_ulid};

/// A job's state. The last four are terminal: a job in one never changes again.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Queued,
    Claimed,
    Succeeded,
    Failed,
    Cancelled,
    Dead,
}

impl JobState {
    /// Every state, in the contract's order.
    pub const ALL: [Self; 6] = [
        Self::Queued,
        Self::Claimed,
        Self::Succeeded,
        Self::Failed,
        Self::Cancelled,
        Self::Dead,
    ];

    /// Whether the job has ended.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Succeeded | Self::Failed | Self::Cancelled | Self::Dead
        )
    }
}

/// Where a job may run: `server`, `any`, or `node:<capability>` (a client
/// that declared the capability, story 122.5).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum RunsOn {
    Server,
    Any,
    Node(String),
}

impl RunsOn {
    /// Parses the column's text; `None` for anything else.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        match text {
            "server" => Some(Self::Server),
            "any" => Some(Self::Any),
            _ => text
                .strip_prefix("node:")
                .filter(|capability| is_capability(capability))
                .map(|capability| Self::Node(capability.to_owned())),
        }
    }
}

impl fmt::Display for RunsOn {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Server => f.write_str("server"),
            Self::Any => f.write_str("any"),
            Self::Node(capability) => write!(f, "node:{capability}"),
        }
    }
}

impl Serialize for RunsOn {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for RunsOn {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom("not a runs_on value"))
    }
}

/// The id of one acquisition of a job: a ULID the server mints on every
/// `jobs.claim` it accepts. Every write an attempt makes names it.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize)]
#[serde(transparent)]
pub struct AttemptId(String);

impl AttemptId {
    /// `None` unless `id` is a ULID.
    #[must_use]
    pub fn parse(id: &str) -> Option<Self> {
        is_ulid(id).then(|| Self(id.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl<'de> Deserialize<'de> for AttemptId {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        Self::parse(&text).ok_or_else(|| serde::de::Error::custom("not a ULID"))
    }
}

/// A Syncular blob reference, the value of a `BLOB_REF` column: a body in
/// the conversation key space (DF/AD-8's third class), never the files one.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct BlobRef {
    /// `sha256:<64 lowercase hex>`.
    pub blob_id: String,
    pub byte_length: u64,
}

impl BlobRef {
    /// Whether `blob_id` is a content address.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.blob_id.strip_prefix("sha256:").is_some_and(|hex| {
            hex.len() == 64
                && hex
                    .bytes()
                    .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
        })
    }
}

/// Which of a job's three blob columns a [`JobRef::JobBlob`] names.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum JobBlobColumn {
    InputBlob,
    PlanBlob,
    ResultBlob,
}

/// What a job's input may name. Never bytes: a reference only.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum JobRef {
    /// A row of this domain's contract.
    Row { table: String, id: String },
    /// A blob of a job (its own or another's), by column.
    JobBlob {
        job_id: String,
        column: JobBlobColumn,
    },
    /// A file, by its entry id and content id as plain strings (116.1).
    File {
        entry_id: String,
        content_id: String,
    },
    /// A graph node at a revision.
    GraphNode { id: String, revision: String },
    /// A graph event.
    GraphEvent { id: String },
    /// A proposal.
    Proposal { id: String },
}

/// An effect's identity, `(job_id, ordinal)`, carried in the effect itself
/// where the domain replicates it: a graph change's property
/// (`<job_id>/<ordinal>`), a proposal's id fixed by the plan, or a file
/// event's `job_effect` `{job_id, ordinal, attempt_id}`.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct EffectId {
    pub job_id: String,
    pub ordinal: u32,
}

impl fmt::Display for EffectId {
    /// The text a graph change's property carries: `<job_id>/<ordinal>`.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}/{}", self.job_id, self.ordinal)
    }
}

impl EffectId {
    /// Parses `<job_id>/<ordinal>`; `None` unless the job id is a ULID and
    /// the ordinal a canonical decimal.
    #[must_use]
    pub fn parse(text: &str) -> Option<Self> {
        let (job_id, ordinal) = text.split_once('/')?;
        if !is_ulid(job_id) || ordinal.is_empty() || (ordinal.len() > 1 && ordinal.starts_with('0'))
        {
            return None;
        }
        if !ordinal.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        Some(Self {
            job_id: job_id.to_owned(),
            ordinal: ordinal.parse().ok()?,
        })
    }
}

/// What one effect produced.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub enum EffectTarget {
    /// A graph event, which wrote the effect's id as a property of `node_id`.
    GraphChange { event_id: String, node_id: String },
    /// A proposal, whose id the plan fixed in advance.
    Proposal { proposal_id: String },
    /// A files event carrying `job_effect` in its envelope's `job` field.
    FileEvent { event_id: String },
}

/// One effect a completion or failure reports.
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Effect {
    pub id: EffectId,
    pub produced: EffectTarget,
}

/// A `jobs` row.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Job {
    pub id: String,
    pub domain: String,
    pub page_key: String,
    pub kind: String,
    pub input: Option<Value>,
    pub input_blob: Option<BlobRef>,
    pub input_digest: String,
    pub runs_on: RunsOn,
    pub state: JobState,
    pub attempt: u32,
    pub max_attempts: u32,
    pub not_before_ms: u64,
    pub plan_blob: Option<BlobRef>,
    pub plan_attempt_id: Option<AttemptId>,
    pub note: Option<String>,
    pub result: Option<Value>,
    pub result_blob: Option<BlobRef>,
    pub effects: Option<Vec<Effect>>,
    pub error_code: Option<String>,
    pub error_sentence: Option<String>,
    pub submitter_actor: String,
    pub submitter_client: String,
    pub submitter_context: Option<String>,
    pub submitted_at_ms: u64,
    pub updated_at_ms: u64,
    pub ended_at_ms: Option<u64>,
    pub revision: u64,
    pub author_actor: String,
    pub author_client: String,
}

/// A `job_claims` row, keyed by its job.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobClaim {
    pub job_id: String,
    pub domain: String,
    pub page_key: String,
    pub holder_actor: String,
    pub holder_client: String,
    pub attempt_id: AttemptId,
    /// The acquisition count: rises on every acquisition.
    pub epoch: u64,
    pub lease_ms: u64,
    /// The server's clock when it accepted the acquisition.
    pub acquired_at_ms: u64,
    /// The server's clock when it accepted the last claim or renewal.
    pub renewed_at_ms: u64,
    pub released: bool,
    pub revision: u64,
    pub author_actor: String,
    pub author_client: String,
}

/// A `job_capabilities` row: what one client says it can run.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobCapabilities {
    /// The client scope `<a>:<actor_id><c>:<client_id>`.
    pub id: String,
    pub domain: String,
    pub actor_id: String,
    pub client_id: String,
    /// Ascending and distinct.
    pub capabilities: Vec<String>,
    pub declared_at_ms: u64,
    pub revision: u64,
    pub author_actor: String,
    pub author_client: String,
}

/// The client scope of an (actor, client) pair, `<a>:<actor_id><c>:<client_id>`
/// with `a` and `c` the UTF-8 byte lengths (the README's *Ids*).
#[must_use]
pub fn client_scope(actor_id: &str, client_id: &str) -> String {
    format!(
        "{}:{actor_id}{}:{client_id}",
        actor_id.len(),
        client_id.len()
    )
}

/// What a client lists of one job (122.4 renders it). It carries no input,
/// plan or result: those are read on demand.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JobView {
    pub id: String,
    pub kind: String,
    pub state: JobState,
    pub runs_on: RunsOn,
    pub attempt: u32,
    pub max_attempts: u32,
    pub submitted_at_ms: u64,
    pub ended_at_ms: Option<u64>,
    pub error_code: Option<String>,
    pub error_sentence: Option<String>,
    pub page_key: String,
}

impl From<&Job> for JobView {
    fn from(job: &Job) -> Self {
        Self {
            id: job.id.clone(),
            kind: job.kind.clone(),
            state: job.state,
            runs_on: job.runs_on.clone(),
            attempt: job.attempt,
            max_attempts: job.max_attempts,
            submitted_at_ms: job.submitted_at_ms,
            ended_at_ms: job.ended_at_ms,
            error_code: job.error_code.clone(),
            error_sentence: job.error_sentence.clone(),
            page_key: job.page_key.clone(),
        }
    }
}
