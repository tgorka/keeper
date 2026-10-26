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

use serde_json::Value;

use crate::hq::approvals::{self, Decisions, ShownApprovalVm};
use crate::hq::port::{ReplicaError, RowOp};
use crate::hq::sessions::{authored, now_ms, object, person_message, Source};
use crate::hq::timeline::session_row;

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
pub fn send(
    source: &Source,
    decisions: &Decisions,
    session_id: &str,
    body: &str,
    shown: &[ShownApprovalVm],
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
    let message = person_message(source, session_id, &ulid::Ulid::new().to_string(), body, at);
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
