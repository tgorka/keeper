//! An agent session's log: append-only JSONL in dated, per-host,
//! size-bounded chunks (AD-365, AD-366, ruling R3; story 89.5).
//!
//! The log is the truth of an agent's session: any host with the folder can
//! rebuild the model's context from it, tool calls and results included. One
//! JSON object per line, keys in the documented order
//! (`v, id, parent, ts, host, epoch, claim, kind, matrix_event, body`) so a
//! log reads in a text editor. [`writer::ChunkWriter`] writes it,
//! [`reader::read_session`] merges it, [`replay::replay`] turns it back into
//! the messages the model saw.

pub mod reader;
pub mod replay;
pub mod writer;

use std::fmt;
use std::str::FromStr;

use chrono::{DateTime, NaiveDate, SecondsFormat, Utc};
use matrix_sdk::ruma::{OwnedEventId, OwnedRoomId, OwnedUserId};
use serde::ser::{SerializeMap, SerializeStruct};
use serde::{Deserialize, Serialize, Serializer};
use serde_json::Value;
use ulid::Ulid;

use crate::agents::label::{Label, LabelBody};
use crate::agents::session::SessionKind;

/// The line schema this build writes and reads.
pub const LINE_VERSION: u32 = 1;

/// No line is longer than this, newline included, after blobbing (NFR-116).
pub const MAX_LINE_BYTES: usize = 64 * 1024;

/// A body that serialises to more than this becomes a blob.
pub const BLOB_OVER_BYTES: usize = 16 * 1024;

/// The log's folder inside a session.
pub const LOG_DIR: &str = "log";

/// The blobs' folder inside the log folder.
pub const BLOBS_DIR: &str = "blobs";

/// Why a log could not be written or a blob read.
#[derive(Debug, thiserror::Error)]
pub enum LogError {
    #[error("{path}: {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{path} is a symbolic link; keeper writes a session's log only into a real folder.")]
    Symlink { path: String },
    #[error("{path} is not a folder.")]
    NotADirectory { path: String },
    #[error("\"{slug}\" is not a host slug: lowercase letters, digits and dashes, 1 to 32.")]
    BadHost { slug: String },
    #[error("A line from host {line} cannot be written by host {writer}: each host writes only its own chunks.")]
    ForeignHost { line: String, writer: String },
    #[error("A line of {bytes} bytes is over the {limit}-byte limit even after its body became a blob; it was not written.")]
    LineTooLong { bytes: usize, limit: usize },
    #[error("A chunk bound of {rotate_at} bytes is under {least}: a body small enough to stay in its line could not fit in a chunk. Raise the folder's LFS threshold.")]
    RotateTooSmall { rotate_at: u64, least: u64 },
    #[error("A log line is one line; this one holds a newline.")]
    Newline,
    #[error("\"{name}\" is not a blob name (64 lowercase hex digits).")]
    BadBlobName { name: String },
    #[error("Blob {name} does not hash to its name; it was changed or damaged.")]
    BlobMismatch { name: String },
    #[error("Blob {name} is not JSON: {detail}")]
    BlobNotJson { name: String, detail: String },
    #[error("A log line could not be serialised: {0}")]
    Json(#[from] serde_json::Error),
}

impl LogError {
    pub(crate) fn io(path: &std::path::Path, source: std::io::Error) -> Self {
        Self::Io {
            path: path.display().to_string(),
            source,
        }
    }
}

/// A host's slug: `[a-z0-9-]{1,32}`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct HostSlug(String);

impl HostSlug {
    /// A slug, if `slug` fits the grammar.
    pub fn new(slug: &str) -> Result<HostSlug, LogError> {
        let fits = (1..=32).contains(&slug.len())
            && slug
                .bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
        if fits {
            Ok(HostSlug(slug.to_owned()))
        } else {
            Err(LogError::BadHost {
                slug: slug.to_owned(),
            })
        }
    }

    /// The slug.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl fmt::Display for HostSlug {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// A chunk's name: `YYYY-MM-DD.<host>.<n>.jsonl`, `n` from 1, unpadded.
/// Ordered by date, host, then `n` as a number.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ChunkName {
    /// The UTC date of the chunk's first line.
    pub date: NaiveDate,
    /// The writing host.
    pub host: HostSlug,
    /// Which chunk of that (date, host).
    pub n: u32,
}

impl fmt::Display for ChunkName {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}.{}.{}.jsonl",
            self.date.format("%Y-%m-%d"),
            self.host,
            self.n
        )
    }
}

impl FromStr for ChunkName {
    type Err = ();

    fn from_str(name: &str) -> Result<Self, ()> {
        let stem = name.strip_suffix(".jsonl").ok_or(())?;
        let mut parts = stem.split('.');
        let (Some(date), Some(host), Some(n), None) =
            (parts.next(), parts.next(), parts.next(), parts.next())
        else {
            return Err(());
        };
        if date.len() != 10 {
            return Err(());
        }
        let date = NaiveDate::parse_from_str(date, "%Y-%m-%d").map_err(|_| ())?;
        let host = HostSlug::new(host).map_err(|_| ())?;
        if n.is_empty() || n.starts_with('0') || !n.bytes().all(|b| b.is_ascii_digit()) {
            return Err(());
        }
        let n = n.parse().map_err(|_| ())?;
        Ok(ChunkName { date, host, n })
    }
}

/// Every kind of line.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LineKind {
    Open,
    Claim,
    User,
    Peer,
    Assistant,
    ToolCall,
    ToolResult,
    Approval,
    Delegate,
    Label,
    Scope,
    Run,
    Surface,
    Heard,
    Memory,
    Compact,
    Error,
    Close,
}

impl LineKind {
    /// Every kind, in the documented order.
    pub const ALL: [LineKind; 18] = [
        Self::Open,
        Self::Claim,
        Self::User,
        Self::Peer,
        Self::Assistant,
        Self::ToolCall,
        Self::ToolResult,
        Self::Approval,
        Self::Delegate,
        Self::Label,
        Self::Scope,
        Self::Run,
        Self::Surface,
        Self::Heard,
        Self::Memory,
        Self::Compact,
        Self::Error,
        Self::Close,
    ];

    /// The `kind` word.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Open => "open",
            Self::Claim => "claim",
            Self::User => "user",
            Self::Peer => "peer",
            Self::Assistant => "assistant",
            Self::ToolCall => "tool_call",
            Self::ToolResult => "tool_result",
            Self::Approval => "approval",
            Self::Delegate => "delegate",
            Self::Label => "label",
            Self::Scope => "scope",
            Self::Run => "run",
            Self::Surface => "surface",
            Self::Heard => "heard",
            Self::Memory => "memory",
            Self::Compact => "compact",
            Self::Error => "error",
            Self::Close => "close",
        }
    }

    /// The kind a word names.
    pub fn from_word(word: &str) -> Option<LineKind> {
        Self::ALL.into_iter().find(|kind| kind.as_str() == word)
    }
}

/// `open`: the session's opening, with the digests of what the model was told.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct OpenBody {
    pub agent: String,
    pub drive: String,
    pub kind: SessionKind,
    pub title: String,
    pub requested_by: OwnedUserId,
    pub label: Label,
    pub drives: Vec<String>,
    /// `bot:{kind}:{base}#{target}`.
    pub model: String,
    pub prompt_sha256: String,
    pub memory_sha256: String,
}

/// A claim transition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ClaimAction {
    Acquired,
    Renewed,
    Released,
    Lost,
}

/// `claim`: a transition of the session's claim (renewals are not logged).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ClaimBody {
    pub epoch: u64,
    pub action: ClaimAction,
    /// The host the claim was taken from, on a takeover.
    pub from_host: Option<String>,
    /// The claim state event's id.
    pub claim_event: String,
    /// The homeserver's time of the claim event, RFC 3339.
    pub server_ts: String,
}

/// A file a person attached.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Attachment {
    pub drive: String,
    pub path: String,
}

/// `user`: a person's message (proxy sessions only).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserBody {
    pub sender: OwnedUserId,
    pub text: String,
    pub attachments: Vec<Attachment>,
}

/// A question another agent asks through a `peer` line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerAsk {
    pub id: String,
    pub question: String,
}

/// `peer`: a message from another agent.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PeerBody {
    pub sender: OwnedUserId,
    pub text: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub ask: Option<PeerAsk>,
    /// Session-relative paths of the artifacts it hands over.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub artifacts: Option<Vec<String>>,
}

/// Token counts the endpoint reported; absent stays absent.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Usage {
    pub prompt: Option<u32>,
    pub completion: Option<u32>,
}

/// `assistant`: one model step's final text.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AssistantBody {
    pub text: String,
    pub model: String,
    pub finish: String,
    pub usage: Usage,
    pub ttft_ms: Option<u64>,
    pub duration_ms: u64,
    pub anchor_event: Option<String>,
}

/// `tool_call`: one call the model made. `args` is the string it streamed,
/// verbatim — replay re-parses it, and only the raw string replays exactly.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolCallBody {
    pub call_id: String,
    pub tool: String,
    pub args: String,
    pub tier: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub grant_id: Option<String>,
}

/// How a tool call ended.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ToolOutcomeWord {
    Ok,
    Refused,
    Failed,
}

/// How much of a result the model was shown.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Truncated {
    pub shown: u64,
    pub total: u64,
}

/// `tool_result`: the result as the model received it, with its label.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ToolResultBody {
    pub call_id: String,
    pub outcome: ToolOutcomeWord,
    pub content: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub truncated: Option<Truncated>,
    pub label: Label,
}

/// Where an approval stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ApprovalState {
    Requested,
    Decided,
    Consumed,
    Expired,
}

/// `approval`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovalBody {
    pub id: String,
    pub state: ApprovalState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub decision: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub result: Option<String>,
}

/// The session a delegation opened.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ChildSession {
    pub drive: String,
    pub session: String,
}

/// Where a delegation stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum DelegateState {
    Opened,
    Accepted,
    Replied,
    Refused,
}

/// `delegate`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DelegateBody {
    pub id: String,
    pub to: String,
    pub room: OwnedRoomId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child: Option<ChildSession>,
    pub state: DelegateState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// `scope`: the drives in scope, changed by the person.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeBody {
    pub drives: Vec<String>,
    pub set_by: OwnedUserId,
}

/// A run's state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum RunState {
    Queued,
    Running,
    Blocked,
    Review,
    Failed,
    Idle,
}

impl RunState {
    /// The word the log and a card use.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Queued => "queued",
            Self::Running => "running",
            Self::Blocked => "blocked",
            Self::Review => "review",
            Self::Failed => "failed",
            Self::Idle => "idle",
        }
    }
}

/// `run`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RunBody {
    pub state: RunState,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

/// `surface`: a surface tool's call on a person's device.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SurfaceBody {
    pub id: String,
    pub tool: String,
    pub device: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub outcome: Option<String>,
}

/// Why speech stopped.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HeardReason {
    BargeIn,
    Stop,
}

/// `heard`: how much of a spoken answer the person heard (AD-411).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HeardBody {
    #[serde(with = "ulid_text")]
    pub assistant: Ulid,
    pub heard_until: u64,
    pub sentence: u32,
    pub reason: HeardReason,
}

/// What an agent wrote to its memory.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum MemoryOp {
    Journal,
    Proposal,
}

/// `memory`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryBody {
    pub op: MemoryOp,
    #[serde(rename = "ref")]
    pub reference: String,
}

/// `compact`: a summary that replaces every line through `replaces_through`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactBody {
    pub summary: String,
    #[serde(with = "ulid_text")]
    pub replaces_through: Ulid,
}

/// `error`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ErrorBody {
    pub sentence: String,
    pub code: String,
}

/// Why a session closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CloseReason {
    Done,
    Archived,
    Failed,
}

/// `close`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CloseBody {
    pub reason: CloseReason,
    pub by: String,
}

/// A body stored as `log/blobs/<sha256>.json`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlobRef {
    /// The kind of the body it stands for.
    pub kind: LineKind,
    /// The hex SHA-256 of the stored bytes, and the blob's name.
    pub sha256: String,
    /// How many bytes the blob holds.
    pub bytes: u64,
}

/// A line's body: one variant per kind, with exactly its fields, or a blob.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LineBody {
    Open(OpenBody),
    Claim(ClaimBody),
    User(UserBody),
    Peer(PeerBody),
    Assistant(AssistantBody),
    ToolCall(ToolCallBody),
    ToolResult(ToolResultBody),
    Approval(ApprovalBody),
    Delegate(DelegateBody),
    Label(LabelBody),
    Scope(ScopeBody),
    Run(RunBody),
    Surface(SurfaceBody),
    Heard(HeardBody),
    Memory(MemoryBody),
    Compact(CompactBody),
    Error(ErrorBody),
    Close(CloseBody),
    /// A body over [`BLOB_OVER_BYTES`], stored beside the log.
    Blob(BlobRef),
}

impl LineBody {
    /// The line's `kind` (a blob's is the kind of the body it holds).
    pub fn kind(&self) -> LineKind {
        match self {
            Self::Open(_) => LineKind::Open,
            Self::Claim(_) => LineKind::Claim,
            Self::User(_) => LineKind::User,
            Self::Peer(_) => LineKind::Peer,
            Self::Assistant(_) => LineKind::Assistant,
            Self::ToolCall(_) => LineKind::ToolCall,
            Self::ToolResult(_) => LineKind::ToolResult,
            Self::Approval(_) => LineKind::Approval,
            Self::Delegate(_) => LineKind::Delegate,
            Self::Label(_) => LineKind::Label,
            Self::Scope(_) => LineKind::Scope,
            Self::Run(_) => LineKind::Run,
            Self::Surface(_) => LineKind::Surface,
            Self::Heard(_) => LineKind::Heard,
            Self::Memory(_) => LineKind::Memory,
            Self::Compact(_) => LineKind::Compact,
            Self::Error(_) => LineKind::Error,
            Self::Close(_) => LineKind::Close,
            Self::Blob(blob) => blob.kind,
        }
    }

    /// The body of `kind` held in `value`.
    pub fn decode(kind: LineKind, value: Value) -> Result<LineBody, serde_json::Error> {
        use serde_json::from_value as from;
        Ok(match kind {
            LineKind::Open => Self::Open(from(value)?),
            LineKind::Claim => Self::Claim(from(value)?),
            LineKind::User => Self::User(from(value)?),
            LineKind::Peer => Self::Peer(from(value)?),
            LineKind::Assistant => Self::Assistant(from(value)?),
            LineKind::ToolCall => Self::ToolCall(from(value)?),
            LineKind::ToolResult => Self::ToolResult(from(value)?),
            LineKind::Approval => Self::Approval(from(value)?),
            LineKind::Delegate => Self::Delegate(from(value)?),
            LineKind::Label => Self::Label(from(value)?),
            LineKind::Scope => Self::Scope(from(value)?),
            LineKind::Run => Self::Run(from(value)?),
            LineKind::Surface => Self::Surface(from(value)?),
            LineKind::Heard => Self::Heard(from(value)?),
            LineKind::Memory => Self::Memory(from(value)?),
            LineKind::Compact => Self::Compact(from(value)?),
            LineKind::Error => Self::Error(from(value)?),
            LineKind::Close => Self::Close(from(value)?),
        })
    }
}

impl Serialize for LineBody {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self {
            Self::Open(b) => b.serialize(s),
            Self::Claim(b) => b.serialize(s),
            Self::User(b) => b.serialize(s),
            Self::Peer(b) => b.serialize(s),
            Self::Assistant(b) => b.serialize(s),
            Self::ToolCall(b) => b.serialize(s),
            Self::ToolResult(b) => b.serialize(s),
            Self::Approval(b) => b.serialize(s),
            Self::Delegate(b) => b.serialize(s),
            Self::Label(b) => b.serialize(s),
            Self::Scope(b) => b.serialize(s),
            Self::Run(b) => b.serialize(s),
            Self::Surface(b) => b.serialize(s),
            Self::Heard(b) => b.serialize(s),
            Self::Memory(b) => b.serialize(s),
            Self::Compact(b) => b.serialize(s),
            Self::Error(b) => b.serialize(s),
            Self::Close(b) => b.serialize(s),
            Self::Blob(blob) => {
                let mut map = s.serialize_map(Some(2))?;
                map.serialize_entry("blob", &blob.sha256)?;
                map.serialize_entry("bytes", &blob.bytes)?;
                map.end()
            }
        }
    }
}

/// One line of a session's log.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogLine {
    /// Always [`LINE_VERSION`] when written.
    pub v: u32,
    /// Unique in the session.
    pub id: Ulid,
    /// The line this one answers or continues: a `tool_call`'s parent is the
    /// `assistant` line that made it, a `tool_result`'s its `tool_call`.
    pub parent: Option<Ulid>,
    /// The writing host's clock, written RFC 3339 UTC with milliseconds.
    pub ts: DateTime<Utc>,
    /// The writing host.
    pub host: HostSlug,
    /// The claim epoch the host held when writing.
    pub epoch: u64,
    /// The claim state event id the host held, which with `epoch` fences a
    /// stale writer.
    pub claim: Option<String>,
    /// The Matrix event this line received or sent.
    pub matrix_event: Option<OwnedEventId>,
    /// The kind's body, or a blob reference.
    pub body: LineBody,
}

impl LogLine {
    /// The line's kind.
    pub fn kind(&self) -> LineKind {
        self.body.kind()
    }

    /// `ts` as written: `2026-10-02T08:15:03.120Z`.
    pub fn ts_text(&self) -> String {
        self.ts.to_rfc3339_opts(SecondsFormat::Millis, true)
    }

    /// The line as JSON, keys in the documented order, no newline.
    pub fn to_json(&self) -> Result<String, LogError> {
        Ok(serde_json::to_string(self)?)
    }

    /// Read one line. A line of another version or an unknown kind is a
    /// [`LineProblem`], never a panic.
    pub fn parse(text: &str) -> Result<LogLine, LineProblem> {
        let value: Value =
            serde_json::from_str(text).map_err(|e| LineProblem::NotJson(e.to_string()))?;
        match value.get("v").and_then(Value::as_u64) {
            Some(v) if v == u64::from(LINE_VERSION) => {}
            Some(v) => return Err(LineProblem::Version(v)),
            None => return Err(LineProblem::Shape("`v` is missing".to_owned())),
        }
        let raw: RawLine =
            serde_json::from_value(value).map_err(|e| LineProblem::Shape(e.to_string()))?;
        let kind = LineKind::from_word(&raw.kind).ok_or(LineProblem::UnknownKind(raw.kind))?;
        let id = Ulid::from_string(&raw.id)
            .map_err(|_| LineProblem::Shape(format!("`id` {} is not a ULID", raw.id)))?;
        let parent = match raw.parent {
            None => None,
            Some(parent) => Some(
                Ulid::from_string(&parent)
                    .map_err(|_| LineProblem::Shape(format!("`parent` {parent} is not a ULID")))?,
            ),
        };
        let ts = DateTime::parse_from_rfc3339(&raw.ts)
            .map_err(|_| LineProblem::Shape(format!("`ts` {} is not RFC 3339", raw.ts)))?
            .with_timezone(&Utc);
        let host = HostSlug::new(&raw.host).map_err(|e| LineProblem::Shape(e.to_string()))?;
        let body = match blob_ref(&raw.body) {
            Some((sha256, bytes)) => LineBody::Blob(BlobRef {
                kind,
                sha256,
                bytes,
            }),
            None => LineBody::decode(kind, raw.body)
                .map_err(|e| LineProblem::Shape(format!("its {} body: {e}", kind.as_str())))?,
        };
        Ok(LogLine {
            v: raw.v,
            id,
            parent,
            ts,
            host,
            epoch: raw.epoch,
            claim: raw.claim,
            matrix_event: raw.matrix_event,
            body,
        })
    }
}

/// `{"blob": "<sha256>", "bytes": n}`, and nothing else.
fn blob_ref(body: &Value) -> Option<(String, u64)> {
    let object = body.as_object()?;
    if object.len() != 2 {
        return None;
    }
    let sha = object.get("blob")?.as_str()?;
    let bytes = object.get("bytes")?.as_u64()?;
    Some((sha.to_owned(), bytes))
}

impl Serialize for LogLine {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        let mut line = s.serialize_struct("LogLine", 10)?;
        line.serialize_field("v", &self.v)?;
        line.serialize_field("id", &self.id.to_string())?;
        line.serialize_field("parent", &self.parent.map(|p| p.to_string()))?;
        line.serialize_field("ts", &self.ts_text())?;
        line.serialize_field("host", self.host.as_str())?;
        line.serialize_field("epoch", &self.epoch)?;
        line.serialize_field("claim", &self.claim)?;
        line.serialize_field("kind", self.kind().as_str())?;
        line.serialize_field("matrix_event", &self.matrix_event)?;
        line.serialize_field("body", &self.body)?;
        line.end()
    }
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawLine {
    v: u32,
    id: String,
    parent: Option<String>,
    ts: String,
    host: String,
    epoch: u64,
    claim: Option<String>,
    kind: String,
    matrix_event: Option<OwnedEventId>,
    body: Value,
}

/// Why one line was skipped.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum LineProblem {
    #[error("it is not JSON: {0}")]
    NotJson(String),
    #[error("it is version {0}; this keeper reads version 1")]
    Version(u64),
    #[error("its kind \"{0}\" is not one this keeper knows")]
    UnknownKind(String),
    #[error("{0}")]
    Shape(String),
}

mod ulid_text {
    use serde::{Deserialize, Deserializer, Serializer};
    use ulid::Ulid;

    pub fn serialize<S: Serializer>(id: &Ulid, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(&id.to_string())
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(d: D) -> Result<Ulid, D::Error> {
        let text = String::deserialize(d)?;
        Ulid::from_string(&text).map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use chrono::TimeZone;
    use matrix_sdk::ruma::{EventId, RoomId, UserId};

    use super::*;
    use crate::agents::label::{Integrity, LabelCause, LabelCauseKind, Readers};

    fn user(id: &str) -> OwnedUserId {
        UserId::parse(id).expect("user")
    }

    fn label() -> Label {
        Label {
            readers: Readers::Only([user("@tgorka:h")].into_iter().collect::<BTreeSet<_>>()),
            integrity: Integrity::Owner,
            local_only: false,
        }
    }

    pub(crate) fn one_of_each() -> Vec<LineBody> {
        let assistant_id = Ulid::from_parts(1_759_392_903_120, 7);
        vec![
            LineBody::Open(OpenBody {
                agent: "amelia".into(),
                drive: "tgdrive".into(),
                kind: SessionKind::Main,
                title: "Release".into(),
                requested_by: user("@tgorka:h"),
                label: label(),
                drives: vec!["tgdrive".into()],
                model: "bot:openai:http://127.0.0.1:8317#gpt".into(),
                prompt_sha256: "a".repeat(64),
                memory_sha256: "b".repeat(64),
            }),
            LineBody::Claim(ClaimBody {
                epoch: 2,
                action: ClaimAction::Acquired,
                from_host: Some("electra".into()),
                claim_event: "$claim2".into(),
                server_ts: "2026-10-02T08:15:03.120Z".into(),
            }),
            LineBody::User(UserBody {
                sender: user("@tgorka:h"),
                text: "hi".into(),
                attachments: vec![Attachment {
                    drive: "tgdrive".into(),
                    path: "notes/a.md".into(),
                }],
            }),
            LineBody::Peer(PeerBody {
                sender: user("@tola-grey:h"),
                text: "brief".into(),
                ask: Some(PeerAsk {
                    id: "q1".into(),
                    question: "which?".into(),
                }),
                artifacts: Some(vec!["artifacts/x.md".into()]),
            }),
            LineBody::Assistant(AssistantBody {
                text: "done".into(),
                model: "gpt".into(),
                finish: "stop".into(),
                usage: Usage {
                    prompt: Some(10),
                    completion: None,
                },
                ttft_ms: Some(120),
                duration_ms: 900,
                anchor_event: Some("$anchor".into()),
            }),
            LineBody::ToolCall(ToolCallBody {
                call_id: "call_1".into(),
                tool: "drive_read".into(),
                args: "{\"path\":\"a.md\"}".into(),
                tier: 0,
                grant_id: None,
            }),
            LineBody::ToolResult(ToolResultBody {
                call_id: "call_1".into(),
                outcome: ToolOutcomeWord::Refused,
                content: "no".into(),
                truncated: Some(Truncated {
                    shown: 10,
                    total: 20,
                }),
                label: label(),
            }),
            LineBody::Approval(ApprovalBody {
                id: "01J".into(),
                state: ApprovalState::Consumed,
                decision: Some("approve".into()),
                by: Some("@tgorka:h".into()),
                result: None,
            }),
            LineBody::Delegate(DelegateBody {
                id: "d1".into(),
                to: "winston".into(),
                room: RoomId::parse("!child:h").expect("room"),
                child: Some(ChildSession {
                    drive: "tgdrive".into(),
                    session: "active/2026-10-02-x".into(),
                }),
                state: DelegateState::Opened,
                reason: None,
            }),
            LineBody::Label(LabelBody::new(
                &label(),
                LabelCause {
                    kind: LabelCauseKind::ToolResult,
                    reference: "call_1".into(),
                },
            )),
            LineBody::Scope(ScopeBody {
                drives: vec!["tgdrive".into(), "neuradrive".into()],
                set_by: user("@tgorka:h"),
            }),
            LineBody::Run(RunBody {
                state: RunState::Blocked,
                detail: Some("waiting".into()),
            }),
            LineBody::Surface(SurfaceBody {
                id: "s1".into(),
                tool: "surface_open".into(),
                device: "KALYPSO".into(),
                outcome: None,
            }),
            LineBody::Heard(HeardBody {
                assistant: assistant_id,
                heard_until: 42,
                sentence: 2,
                reason: HeardReason::BargeIn,
            }),
            LineBody::Memory(MemoryBody {
                op: MemoryOp::Journal,
                reference: "journal/2026-10-02.electra.md".into(),
            }),
            LineBody::Compact(CompactBody {
                summary: "earlier".into(),
                replaces_through: assistant_id,
            }),
            LineBody::Error(ErrorBody {
                sentence: "The provider did not answer.".into(),
                code: "provider_down".into(),
            }),
            LineBody::Close(CloseBody {
                reason: CloseReason::Done,
                by: "@tgorka:h".into(),
            }),
        ]
    }

    fn line(body: LineBody) -> LogLine {
        LogLine {
            v: LINE_VERSION,
            id: Ulid::from_parts(1_759_392_903_120, 1),
            parent: Some(Ulid::from_parts(1_759_392_903_000, 9)),
            ts: Utc
                .with_ymd_and_hms(2026, 10, 2, 8, 15, 3)
                .single()
                .expect("ts")
                + chrono::Duration::milliseconds(120),
            host: HostSlug::new("electra").expect("host"),
            epoch: 2,
            claim: Some("$claim2".into()),
            matrix_event: Some(EventId::parse("$ev:h").expect("event")),
            body,
        }
    }

    #[test]
    fn every_kind_round_trips_in_the_documented_key_order() {
        let bodies = one_of_each();
        let kinds: Vec<LineKind> = bodies.iter().map(LineBody::kind).collect();
        assert_eq!(kinds, LineKind::ALL.to_vec(), "one line of every kind");
        for body in bodies {
            let line = line(body);
            let json = line.to_json().expect("json");
            let keys = [
                "\"v\":",
                "\"id\":",
                "\"parent\":",
                "\"ts\":",
                "\"host\":",
                "\"epoch\":",
                "\"claim\":",
                "\"kind\":",
                "\"matrix_event\":",
                "\"body\":",
            ];
            let positions: Vec<usize> = keys
                .iter()
                .map(|key| json.find(key).unwrap_or_else(|| panic!("{key} in {json}")))
                .collect();
            assert!(positions.windows(2).all(|w| w[0] < w[1]), "{json}");
            assert!(json.contains("\"ts\":\"2026-10-02T08:15:03.120Z\""));
            assert_eq!(LogLine::parse(&json).expect("parse"), line, "{json}");
        }
    }

    #[test]
    fn an_unknown_kind_and_a_newer_version_are_problems_not_panics() {
        let json = line(one_of_each().remove(2)).to_json().expect("json");
        let unknown = json.replace("\"kind\":\"user\"", "\"kind\":\"dream\"");
        assert_eq!(
            LogLine::parse(&unknown),
            Err(LineProblem::UnknownKind("dream".into()))
        );
        let newer = json.replacen("\"v\":1", "\"v\":2", 1);
        assert_eq!(LogLine::parse(&newer), Err(LineProblem::Version(2)));
        assert!(matches!(
            LogLine::parse("{\"v\":1"),
            Err(LineProblem::NotJson(_))
        ));
        let extra = json.replacen("\"text\":\"hi\"", "\"text\":\"hi\",\"mood\":1", 1);
        assert!(matches!(LogLine::parse(&extra), Err(LineProblem::Shape(_))));
    }

    #[test]
    fn chunk_names_parse_print_and_order_numerically() {
        let name: ChunkName = "2026-10-02.electra.10.jsonl".parse().expect("name");
        assert_eq!(name.n, 10);
        assert_eq!(name.to_string(), "2026-10-02.electra.10.jsonl");
        let two: ChunkName = "2026-10-02.electra.2.jsonl".parse().expect("name");
        assert!(two < name, "n compares as a number");
        for bad in [
            "2026-10-02.electra.02.jsonl",
            "2026-10-02.electra.0.jsonl",
            "2026-10-02.Electra.1.jsonl",
            "2026-10-2.electra.1.jsonl",
            "2026-10-02.electra.1.json",
            "2026-10-02.elec.tra.1.jsonl",
        ] {
            assert!(bad.parse::<ChunkName>().is_err(), "{bad}");
        }
        assert!(HostSlug::new(&"a".repeat(32)).is_ok());
        assert!(HostSlug::new(&"a".repeat(33)).is_err());
        assert!(HostSlug::new("").is_err());
    }
}
