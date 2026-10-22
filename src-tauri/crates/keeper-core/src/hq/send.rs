//! A person's message (R465): a `messages` row in the session's own
//! database, written into the replica's outbox in one commit with the
//! session's last activity, and shown at once — *waiting* until the server
//! has it ([`crate::hq::timeline`] reads the state from the row).

use serde_json::Value;

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
    #[error("{0}")]
    Replica(#[from] ReplicaError),
}

/// Write `body` into `session_id` of `source`'s database.
pub fn send(source: &Source, session_id: &str, body: &str) -> Result<(), SendError> {
    if body.trim().is_empty() {
        return Err(SendError::Empty);
    }
    let session = session_row(source, session_id).ok_or(SendError::Unknown)?;
    if session.archived {
        return Err(SendError::Archived);
    }
    let at = now_ms();
    let activity = RowOp::Patch {
        table: "sessions".to_owned(),
        values: authored(
            source,
            object(serde_json::json!({ "id": session_id, "last_activity_ms": Value::from(at) })),
        ),
        base_version: None,
    };
    source
        .replica
        .mutate(vec![person_message(source, session_id, body, at), activity])?;
    source.replica.sync_now();
    Ok(())
}
