//! Turning a session's log back into the messages the model saw (FR-774).
//!
//! [`message_for`] is the one function that turns a line into a wire
//! message; the turn loop uses it as it writes and [`replay`] uses it as it
//! reads, so what replays is what was sent by construction. Replay is the
//! cold path — a session opened, taken over or restarted — and is pure over a
//! [`SessionLog`] already read: a host serving a session keeps the history in
//! memory from the writer's receipts and never re-reads the log on a turn.

use serde_json::Value;
use ulid::Ulid;

use super::reader::SessionLog;
use super::{LineBody, LogError, LogLine, OpenBody, PeerBody, UserBody};
use crate::bots::chat::{ChatMessage, ContentPart, Role, ToolCall};

/// The heading of the system message a `compact` line becomes.
pub const COMPACT_HEADING: &str = "Summary of the earlier part of this session";

/// The messages a log replays to.
#[derive(Debug, Clone, Default)]
pub struct Replay {
    /// The conversation, without the system prompt (which the host composes
    /// and the `open` line digests).
    pub messages: Vec<ChatMessage>,
    /// The last `open` line's body: who the session is, what it was told.
    pub last_open: Option<OpenBody>,
}

/// Why a log does not replay.
#[derive(Debug, thiserror::Error)]
pub enum ReplayRefusal {
    #[error("Two hosts both held this session at one epoch, so its log has two truths; keeper will not continue it until one host's lines are set aside.")]
    Conflicted,
    #[error("A line's body is stored as blob {sha256}, which could not be read: {source}")]
    Blob {
        sha256: String,
        #[source]
        source: LogError,
    },
    #[error("Blob {sha256} does not hold a {kind} body: {detail}")]
    BlobShape {
        sha256: String,
        kind: &'static str,
        detail: String,
    },
}

impl ReplayRefusal {
    /// The refusal as the sentence a person reads.
    pub fn sentence(&self) -> String {
        self.to_string()
    }
}

/// The wire message one line stands for, if it stands for one.
///
/// A `tool_call` line is an assistant message holding just that call:
/// [`replay`] folds it into the assistant message of its `parent` line. A
/// blob line has no message until it is hydrated. A person's attachments and
/// a peer's question are part of the message the model is sent, so they are
/// part of the message a replay rebuilds: the turn loop sends exactly this.
pub fn message_for(line: &LogLine) -> Option<ChatMessage> {
    match &line.body {
        LineBody::User(body) => Some(ChatMessage::text(Role::User, user_text(body))),
        LineBody::Peer(body) => Some(ChatMessage::text(Role::User, peer_text(body))),
        LineBody::Assistant(body) => Some(ChatMessage {
            role: Role::Assistant,
            content: if body.text.is_empty() {
                Vec::new()
            } else {
                vec![ContentPart::Text(body.text.clone())]
            },
            tool_call_id: None,
            tool_calls: Vec::new(),
        }),
        LineBody::ToolCall(body) => Some(ChatMessage {
            role: Role::Assistant,
            content: Vec::new(),
            tool_call_id: None,
            tool_calls: vec![ToolCall {
                id: body.call_id.clone(),
                name: body.tool.clone(),
                arguments_raw: body.args.clone(),
                arguments: serde_json::from_str(&body.args).ok(),
            }],
        }),
        LineBody::ToolResult(body) => Some(ChatMessage {
            role: Role::Tool,
            content: vec![ContentPart::Text(body.content.clone())],
            tool_call_id: Some(body.call_id.clone()),
            tool_calls: Vec::new(),
        }),
        LineBody::Compact(body) => Some(ChatMessage::text(
            Role::System,
            format!("{COMPACT_HEADING}\n\n{}", body.summary),
        )),
        _ => None,
    }
}

/// A person's text, then the drive files they attached, one per line.
fn user_text(body: &UserBody) -> String {
    if body.attachments.is_empty() {
        return body.text.clone();
    }
    let mut text = body.text.clone();
    text.push_str("\n\nAttached files:");
    for attachment in &body.attachments {
        text.push_str(&format!("\n- {}:{}", attachment.drive, attachment.path));
    }
    text
}

/// Another agent's message, as data: who sent it, its text, the files it
/// hands over one per line, then the question it asks under the ask's id.
fn peer_text(body: &PeerBody) -> String {
    let mut text = format!("From {}:\n{}", body.sender, body.text);
    if let Some(files) = body.artifacts.as_ref().filter(|files| !files.is_empty()) {
        text.push_str("\n\nFiles handed over:");
        for file in files {
            text.push_str(&format!("\n- {file}"));
        }
    }
    if let Some(ask) = &body.ask {
        text.push_str(&format!("\n\nQuestion {}: {}", ask.id, ask.question));
    }
    text
}

/// One replayed message and the line that made it.
struct Placed {
    line: Ulid,
    position: usize,
    message: ChatMessage,
}

/// Replay `log` into the messages the model saw, hydrating blobs through
/// `blobs` (given a blob's name, its stored JSON).
pub fn replay(
    log: &SessionLog,
    blobs: &dyn Fn(&str) -> Result<Value, LogError>,
) -> Result<Replay, ReplayRefusal> {
    if log.conflicted() {
        return Err(ReplayRefusal::Conflicted);
    }
    let mut placed: Vec<Placed> = Vec::new();
    let mut last_open = None;
    for (position, stored) in log.lines.iter().enumerate() {
        let hydrated;
        let line = match &stored.body {
            LineBody::Blob(blob) => {
                let value = blobs(&blob.sha256).map_err(|source| ReplayRefusal::Blob {
                    sha256: blob.sha256.clone(),
                    source,
                })?;
                let body =
                    LineBody::decode(blob.kind, value).map_err(|e| ReplayRefusal::BlobShape {
                        sha256: blob.sha256.clone(),
                        kind: blob.kind.as_str(),
                        detail: e.to_string(),
                    })?;
                hydrated = LogLine {
                    body,
                    ..stored.clone()
                };
                &hydrated
            }
            _ => stored,
        };

        match &line.body {
            LineBody::Open(open) => last_open = Some(open.clone()),
            LineBody::Compact(compact) => {
                let through = log
                    .lines
                    .iter()
                    .position(|l| l.id == compact.replaces_through)
                    .unwrap_or(position);
                placed.retain(|p| p.position > through);
                if let Some(message) = message_for(line) {
                    placed.insert(
                        0,
                        Placed {
                            line: line.id,
                            position: through,
                            message,
                        },
                    );
                }
                continue;
            }
            LineBody::ToolCall(_) => {
                if let Some(message) = message_for(line) {
                    let owner = line.parent.and_then(|parent| {
                        placed
                            .iter_mut()
                            .find(|p| p.line == parent && p.message.role == Role::Assistant)
                    });
                    match owner {
                        Some(owner) => owner.message.tool_calls.extend(message.tool_calls),
                        None => placed.push(Placed {
                            line: line.id,
                            position,
                            message,
                        }),
                    }
                }
                continue;
            }
            _ => {}
        }
        if let Some(message) = message_for(line) {
            placed.push(Placed {
                line: line.id,
                position,
                message,
            });
        }
    }
    Ok(Replay {
        messages: placed.into_iter().map(|p| p.message).collect(),
        last_open,
    })
}
