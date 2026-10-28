//! A person's message (R465): a `messages` row in the session's own
//! database, written into the replica's outbox in one commit with the
//! session's last activity, and shown at once — *waiting* until the server
//! has it ([`crate::hq::timeline`] reads the state from the row).
//!
//! A message declines the requests of its session its banner showed that
//! still wait for the person sending it (Q109-12 a): the denials are written
//! first, each its own commit ([`crate::hq::approvals::decline_pending`]), so
//! the agent's runtime reads them before the message that steers its turn. A
//! denial that cannot be written stops the message.
//!
//! A message sent from the Notes Assistant while a note is open carries what
//! the person is looking at (109.28, R-HQ-86): `metadata.focus`
//! ([`AgentFocus`]). It has no `kind`, so it is an ordinary message that
//! starts a turn; the runtime turns it into one line of that turn.
//!
//! keeper attaches it only where it judges the chat's readers all read the
//! note's drive ([`AgentFocus::for_destination`]), but the server is the
//! boundary (contract § 9.1a): it refuses a message whose focus drive the
//! chat's readers may not all read, `hq.focus.audience`. keeper then writes
//! the same message again once, without the focus, and says so under it
//! ([`resend_without_focus`], [`withheld_sentence`]). The message is never
//! dropped.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use ts_rs::TS;

use crate::hq::approvals::{self, Decisions, ShownApprovalVm};
use crate::hq::port::{ReplicaError, RowOp};
use crate::hq::sessions::{authored, now_ms, object, person_message, Source};
use crate::hq::timeline::session_row;
use crate::notes::outline::heading_in;

/// The longest `focus.path` the runtime reads, in bytes (contract §9.1).
pub const FOCUS_PATH_MAX_BYTES: usize = 1024;
/// The longest `focus.heading`, in characters; a longer trail is cut with `…`.
pub const FOCUS_HEADING_MAX_CHARS: usize = 200;

/// The note open in front of the person as the Notes Assistant sends:
/// which note of which vault, and the editor's caret (`note-caret.ts`) — the
/// 1-based body line and the buffer through that line, saved or not. The
/// shell names the note's drive and path from it; Rust names the heading.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct AgentNoteCaret {
    pub vault_id: String,
    pub note_id: String,
    pub line: u32,
    /// The editor's buffer from its start through the caret's line.
    pub text: String,
}

/// `metadata.focus` (contract §9.1, binding): what the person was looking at
/// when they sent the message — the note's drive and its path from the
/// drive's root, the caret's 1-based body line, and the trail of headings
/// that line is under (`Plans › Q3`), or `null` above the first.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentFocus {
    pub drive: String,
    pub path: String,
    pub heading: Option<String>,
    pub line: u32,
}

impl AgentFocus {
    /// Line `line` of the note at `path` on `drive`, under the heading the
    /// buffer `through` (the note's body through that line) names. `None`
    /// outside the bounds the runtime reads — an empty drive or path, a path
    /// over [`FOCUS_PATH_MAX_BYTES`], line 0 — so a message never carries a
    /// focus the runtime would drop. A trail over
    /// [`FOCUS_HEADING_MAX_CHARS`] is cut with `…`.
    pub fn new(drive: &str, path: &str, through: &str, line: u32) -> Option<AgentFocus> {
        if drive.is_empty() || path.is_empty() || path.len() > FOCUS_PATH_MAX_BYTES || line == 0 {
            return None;
        }
        let heading = heading_in(through, line).map(|trail| {
            if trail.chars().count() <= FOCUS_HEADING_MAX_CHARS {
                trail
            } else {
                let mut cut: String = trail.chars().take(FOCUS_HEADING_MAX_CHARS - 1).collect();
                cut.push('…');
                cut
            }
        });
        Some(AgentFocus {
            drive: drive.to_owned(),
            path: path.to_owned(),
            heading,
            line,
        })
    }

    /// The focus a message to `source`'s database may carry (D-34, R605-01):
    /// a note's path and heading are the drive's, so they go only where
    /// everyone who reads the conversation reads that drive. A wider or
    /// unknown audience gets no focus, and the message goes without it.
    pub(crate) fn for_destination<'a>(
        focus: Option<&'a AgentFocus>,
        source: &Source,
        sources: &[Source],
    ) -> Option<&'a AgentFocus> {
        focus.filter(|focus| crate::hq::drives::read_by_every_reader(source, &focus.drive, sources))
    }

    /// The message's `metadata`: `{"focus": {drive, path, heading, line}}`.
    pub fn metadata(&self) -> Value {
        serde_json::json!({ "focus": self })
    }
}

/// Why a message was not written, as the person reads it.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum SendError {
    #[error("Write something first.")]
    Empty,
    #[error("This conversation is not on this device.")]
    Unknown,
    #[error("This conversation is archived; unarchive it to write in it.")]
    Archived,
    /// A request the banner showed could not be declined on this device;
    /// the message was not written (R10911-06).
    #[error("{0}")]
    Declines(String),
    #[error("{0}")]
    Replica(#[from] ReplicaError),
    /// The message could not be written after the requests it declines were.
    #[error("Your message was not written ({0}); the requests it declines stay declined.")]
    AfterDeclines(ReplicaError),
}

/// Write `body` into `session_id` of `source`'s database, declining first
/// the requests the banner showed (`shown`) that still wait for this person.
/// `focus` is the note open in front of the person (109.28): the message's
/// `metadata.focus`.
pub fn send(
    source: &Source,
    decisions: &Decisions,
    session_id: &str,
    body: &str,
    shown: &[ShownApprovalVm],
    focus: Option<&AgentFocus>,
) -> Result<(), SendError> {
    if body.trim().is_empty() {
        return Err(SendError::Empty);
    }
    let session = session_row(source, session_id).ok_or(SendError::Unknown)?;
    if session.archived {
        return Err(SendError::Archived);
    }
    let at = now_ms();
    let declined = approvals::decline_pending(source, decisions, session_id, shown, at)
        .map_err(SendError::Declines)?;
    let message = person_message(
        source,
        session_id,
        &ulid::Ulid::new().to_string(),
        body,
        focus.map(AgentFocus::metadata),
        at,
    );
    source
        .replica
        .mutate(vec![message, activity(source, session_id, at)])
        .map_err(|error| match declined.is_empty() {
            true => SendError::Replica(error),
            false => SendError::AfterDeclines(error),
        })?;
    source.replica.sync_now();
    Ok(())
}

/// The session's last activity moved to `at`, in the commit that writes a
/// person's message.
pub(crate) fn activity(source: &Source, session_id: &str, at: i64) -> RowOp {
    RowOp::Patch {
        table: "sessions".to_owned(),
        values: authored(
            source,
            object(serde_json::json!({ "id": session_id, "last_activity_ms": Value::from(at) })),
        ),
        base_version: None,
    }
}

/// The server's code for a message whose focus drive the chat's readers may
/// not all read (contract § 9.1a).
pub const FOCUS_AUDIENCE: &str = "hq.focus.audience";

/// What keeper says under a message it sent again without the note in
/// front of the person (contract § 9.1a, binding).
pub fn withheld_sentence(drive: &str) -> String {
    format!("This chat's readers can't all read {drive}, so the note you're in wasn't sent.")
}

/// The messages written again without their focus, by database and message
/// id, each with the focus's drive — what [`withheld_sentence`] names. Kept
/// on this device beside the replicas: the drive's name is never written
/// into the chat, and the refusal itself is set aside by the resend.
fn withheld_path(root: &Path) -> PathBuf {
    root.join("focus-withheld.json")
}

fn withheld_key(database: &str, message: &str) -> String {
    format!("{database}\n{message}")
}

/// The drive a message of `database` was sent again without, for each such
/// message id.
pub(crate) fn withheld(root: &Path, database: &str) -> HashMap<String, String> {
    let all: HashMap<String, String> = std::fs::read_to_string(withheld_path(root))
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    let prefix = withheld_key(database, "");
    all.into_iter()
        .filter_map(|(key, drive)| Some((key.strip_prefix(&prefix)?.to_owned(), drive)))
        .collect()
}

fn record_withheld(root: &Path, database: &str, message: &str, drive: &str) {
    let path = withheld_path(root);
    let mut all: HashMap<String, String> = std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default();
    all.insert(withheld_key(database, message), drive.to_owned());
    let written = serde_json::to_string(&all)
        .map_err(std::io::Error::other)
        .and_then(|raw| std::fs::write(&path, raw));
    if let Err(error) = written {
        tracing::warn!(%error, "the note withheld from a message could not be kept");
    }
}

/// Held from reading the refusals to setting them aside, so two passes over
/// one refusal never write its message twice.
static RESENDING: Mutex<()> = Mutex::new(());

/// The focus drive of a message row's `metadata`, however the journal kept
/// it (JSON text or JSON).
fn focus_drive(values: &Map<String, Value>) -> Option<String> {
    let metadata = match values.get("metadata")? {
        Value::String(raw) => serde_json::from_str::<Value>(raw).ok()?,
        other => other.clone(),
    };
    metadata
        .get("focus")?
        .get("drive")?
        .as_str()
        .map(str::to_owned)
}

/// Every commit written here that the server refused for its focus's
/// audience (`hq.focus.audience`), written once more as the same rows — the
/// same message id, body and files — with the focus taken out, in a new
/// commit that sets the refusal aside, and the drive kept under `root` for
/// [`withheld_sentence`]. A row that carries no focus is never written again
/// for this code, so nothing loops. The messages written again.
pub fn resend_without_focus(source: &Source, root: &Path) -> Result<usize, ReplicaError> {
    let _held = RESENDING.lock().unwrap_or_else(PoisonError::into_inner);
    let mut resent = 0;
    for commit in source.replica.refused_commits("messages")? {
        if commit.rejection.code != FOCUS_AUDIENCE {
            continue;
        }
        let Some((message, drive)) = commit.rows.iter().find_map(|(table, values)| {
            (table == "messages")
                .then(|| focus_drive(values).map(|drive| (values, drive)))
                .flatten()
        }) else {
            continue;
        };
        let message_id = message
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned();
        let ops = commit
            .rows
            .iter()
            .map(|(table, values)| {
                let mut values = values.clone();
                if table == "messages" {
                    values.insert("metadata".to_owned(), Value::Null);
                }
                // A session row without its agent is the activity patch a
                // send writes; with it, the row a creation inserts.
                if table == "sessions" && !values.contains_key("agent_actor") {
                    RowOp::Patch {
                        table: table.clone(),
                        values,
                        base_version: None,
                    }
                } else {
                    RowOp::Upsert {
                        table: table.clone(),
                        values,
                        base_version: None,
                    }
                }
            })
            .collect();
        source
            .replica
            .mutate_superseding(ops, vec![commit.commit_id.clone()])?;
        record_withheld(root, &source.database, &message_id, &drive);
        resent += 1;
    }
    if resent > 0 {
        source.replica.sync_now();
    }
    Ok(resent)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The trail of the caret's line from the buffer through it, unsaved
    /// headings included; none above the first heading.
    #[test]
    fn a_focus_names_the_heading_its_line_is_under() {
        let through = "intro\n# Plans\n## Q3\nship it";
        let focus = AgentFocus::new("tgdrive", "10-notes/inbox.md", through, 4).expect("focus");
        assert_eq!(focus.heading.as_deref(), Some("Plans › Q3"));
        assert_eq!(
            focus.metadata(),
            serde_json::json!({"focus": {"drive": "tgdrive", "path": "10-notes/inbox.md",
                                         "heading": "Plans › Q3", "line": 4}})
        );
        let above = AgentFocus::new("tgdrive", "10-notes/inbox.md", "intro", 1).expect("focus");
        assert_eq!(
            above.metadata()["focus"]["heading"],
            Value::Null,
            "null, not absent"
        );
    }

    /// Contract §9.1's bounds: a heading over 200 characters is cut to 200
    /// with `…`; a path over 1,024 bytes, line 0, or no drive is no focus.
    #[test]
    fn a_focus_keeps_to_the_bounds_the_runtime_reads() {
        let long = "h".repeat(FOCUS_HEADING_MAX_CHARS + 1);
        let cut = AgentFocus::new("tgdrive", "n.md", &format!("# {long}"), 1)
            .expect("focus")
            .heading
            .expect("a heading");
        assert_eq!(cut.chars().count(), FOCUS_HEADING_MAX_CHARS);
        assert_eq!(cut, format!("{}…", "h".repeat(FOCUS_HEADING_MAX_CHARS - 1)));
        let exact = "h".repeat(FOCUS_HEADING_MAX_CHARS);
        assert_eq!(
            AgentFocus::new("tgdrive", "n.md", &format!("# {exact}"), 1)
                .expect("focus")
                .heading,
            Some(exact)
        );

        let at_most = "p".repeat(FOCUS_PATH_MAX_BYTES);
        assert!(AgentFocus::new("tgdrive", &at_most, "", 1).is_some());
        let over = "p".repeat(FOCUS_PATH_MAX_BYTES + 1);
        assert_eq!(AgentFocus::new("tgdrive", &over, "", 1), None);
        assert_eq!(AgentFocus::new("tgdrive", "n.md", "", 0), None);
        assert_eq!(AgentFocus::new("", "n.md", "", 1), None);
        assert_eq!(AgentFocus::new("tgdrive", "", "", 1), None);
    }
}
