//! The frames of `keeper.wire.v1` (MA/AD-3 as amended by O-123-WIRE-1): one
//! JSON object per line, ended by `\n`. These types are the envelope's one
//! definition; the committed JSON Schema (`schema/keeper.wire.v1.json`) is
//! generated from them.
//!
//! Within `keeper.wire.v1` the envelope only grows: each side ignores a field
//! it does not know (no type here denies unknown fields), and a field once
//! sent is never removed or retyped.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The one envelope version this build serves.
pub const WIRE_V1: &str = "keeper.wire.v1";

/// A line is at most this many bytes, its `\n` included, in both directions.
pub const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

/// What a frame from the client is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ClientFrame {
    Hello(ClientHello),
    Call(Call),
    Watch(Watch),
    Describe(Describe),
    Cancel(Cancel),
}

/// What a frame from the server is.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ServerFrame {
    Hello(ServerHello),
    Reply(Reply),
    Event(Event),
    End(End),
    Close(Close),
}

/// The client's first line: the versions it speaks, in its order of
/// preference, and its name (for the log; it decides nothing).
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ClientHello {
    pub wire: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub client: Option<String>,
}

/// The door a wire caller comes through. Absent reads as `cli`; a value the
/// envelope does not know is refused, never read as `cli`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Door {
    #[default]
    Cli,
    Mcp,
}

impl Door {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "cli" => Some(Self::Cli),
            "mcp" => Some(Self::Mcp),
            _ => None,
        }
    }
}

/// How much of an answer: absent reads as `full`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Detail {
    #[default]
    Full,
    Concise,
}

impl Detail {
    #[must_use]
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "full" => Some(Self::Full),
            "concise" => Some(Self::Concise),
            _ => None,
        }
    }
}

/// One query or command, answered by exactly one `reply`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Call {
    /// A ULID the client mints, in its canonical upper-case spelling.
    pub call_id: String,
    /// `<module>.<verb>`.
    pub operation: String,
    /// The operation's input, as a JSON value (never a string).
    #[serde(default)]
    pub input: Value,
    /// Milliseconds; absent or `0` is the kind's ceiling.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub door: Option<Door>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Detail>,
}

/// One stream, answered by `event` frames and exactly one `end`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Watch {
    pub call_id: String,
    pub operation: String,
    #[serde(default)]
    pub input: Value,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub door: Option<Door>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Detail>,
}

/// The catalogue this caller can see through its door, answered as a query.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Describe {
    pub call_id: String,
    /// Words an operation's id or summary must contain.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// One operation, by id.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub operation: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub detail: Option<Detail>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_size: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub page_token: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub deadline_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub door: Option<Door>,
}

/// Ends a call or a stream in flight on this connection. It has no answer.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Cancel {
    pub call_id: String,
}

/// The server's first line: the version chosen, and what a client needs to
/// know before it calls.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct ServerHello {
    pub wire: String,
    pub app_version: String,
    pub catalogue_digest: String,
    pub max_line_bytes: u64,
    pub max_in_flight: u32,
}

/// The one answer of a `call` or a `describe`: `output` or `error`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Reply {
    pub call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
}

/// One event of a stream; `seq` counts 1, 2, 3 … per stream.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct Event {
    pub call_id: String,
    pub seq: u64,
    pub output: Value,
}

/// A stream's last frame: no `error` when it simply finished.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct End {
    pub call_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<WireError>,
}

/// The server's last line when it closes the connection for a reason of its
/// own, with the versions it serves.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Close {
    pub error: WireError,
    pub wire: Vec<String>,
}

/// The codes every door maps to.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum Code {
    NotFound,
    InvalidArgument,
    Refused,
    Unavailable,
    Conflict,
    DeadlineExceeded,
    Cancelled,
    ResourceExhausted,
    Internal,
}

impl Code {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::NotFound => "NOT_FOUND",
            Self::InvalidArgument => "INVALID_ARGUMENT",
            Self::Refused => "REFUSED",
            Self::Unavailable => "UNAVAILABLE",
            Self::Conflict => "CONFLICT",
            Self::DeadlineExceeded => "DEADLINE_EXCEEDED",
            Self::Cancelled => "CANCELLED",
            Self::ResourceExhausted => "RESOURCE_EXHAUSTED",
            Self::Internal => "INTERNAL",
        }
    }
}

/// keeper's error as data: a code, keeper's fixed sentence for it, whether
/// waiting helps, the field an `INVALID_ARGUMENT` names and, for a command,
/// the id to ask again with.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct WireError {
    pub code: Code,
    pub message: String,
    pub retryable: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub field: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
}

/// `describe`'s output.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct DescribeOutput {
    pub envelope: String,
    pub app_version: String,
    pub catalogue_digest: String,
    pub operations: Vec<OperationEntry>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub next_page_token: Option<String>,
}

/// One operation as `describe` lists it to this caller and door.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, JsonSchema)]
pub struct OperationEntry {
    pub id: String,
    /// `query`, `command` or `stream`.
    pub kind: String,
    /// `read` or `write`.
    pub effect: String,
    pub summary: String,
    /// JSON Schema 2020-12; present with detail `full` only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub input_schema: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_schema: Option<Value>,
    /// `proposal`, `change` or `refused`, for a write.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub write_as: Option<String>,
}

/// One frame as one line: its JSON and the `\n`.
#[must_use]
pub fn line<T: Serialize>(frame: &T) -> Vec<u8> {
    // A frame of these types always serialises: every map key is a string.
    let mut bytes = serde_json::to_vec(frame).unwrap_or_default();
    bytes.push(b'\n');
    bytes
}
