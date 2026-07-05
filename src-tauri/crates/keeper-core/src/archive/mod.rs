//! Local archive ingestion (Story 5.1, epic 5 — the trust pillar).
//!
//! Persists message history to the user's own disk so it stops depending on any
//! platform's retention. This module owns one `archive.db` for *all* Accounts
//! (keyed by `account_id`, WAL mode) and a single serialized writer task that is
//! the only writer of that file.
//!
//! Coverage for 5.1 is `m.room.message` events delivered post-decryption through
//! the live sync flow (`account.rs` registers the per-account handler): text and
//! media messages, and edit events as their own rows. It is NOT a total capture
//! of everything a server holds — back-paginated history (Story 5.6), non-message
//! event types (reactions/state), and re-decryption of previously-UTD events are
//! not ingested here.
//!
//! The seam is tauri-free and matrix-free: [`ArchiveEvent`] is a plain
//! keeper-core struct, so the module is unit-testable without a live matrix
//! `Client`. `account.rs` performs the matrix-event → [`ArchiveEvent`] mapping and
//! calls [`ArchiveHandle::ingest`]; the producer side never blocks the
//! sync/messaging path (unbounded channel, non-blocking send).
//!
//! Scope for 5.1 is ingestion only: no FTS, no edit version chains, no export,
//! no archive-deletion / sign-out path — those are later epic-5 stories.

pub mod db;
mod ingest;

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use tokio::sync::mpsc::{self, UnboundedSender};

use crate::error::ArchiveError;

/// Media *metadata* for an archived media message (Story 5.1). Metadata only —
/// never the media bytes (those stay in the SDK media cache; the archive records
/// enough to identify and later re-fetch them). Every field is optional because a
/// sender may omit it. Serialized to the `media_json` column.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveMedia {
    /// The primary content `mxc://` URI, when the source is unencrypted (an
    /// encrypted source carries its URI inside the content JSON, not here).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mxc: Option<String>,
    /// MIME type from the message `info` (e.g. `image/png`), when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mimetype: Option<String>,
    /// Declared byte size from the message `info`, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub size: Option<u64>,
    /// Pixel width for image/video, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub width: Option<u64>,
    /// Pixel height for image/video, when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub height: Option<u64>,
    /// Original filename (for `m.file`), when present.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filename: Option<String>,
    /// Thumbnail `mxc://` URI, when an unencrypted dedicated thumbnail exists.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_mxc: Option<String>,
}

/// A normalized archive row awaiting the single writer (Story 5.1).
///
/// A plain keeper-core struct (not a `Vm`, never crosses IPC): `account.rs` maps
/// a post-decryption matrix event into one of these and hands it to
/// [`ArchiveHandle::ingest`]. `content_json` is the event content serialized as
/// JSON; `media` is `Some` only for a media message and holds metadata only.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ArchiveEvent {
    /// Opaque keeper account id (ULID) the event belongs to.
    pub account_id: String,
    /// Matrix event id — the per-account idempotency key.
    pub event_id: String,
    /// Matrix room id the event was sent to.
    pub room_id: String,
    /// Matrix user id of the sender.
    pub sender: String,
    /// Origin server timestamp in milliseconds since the Unix epoch.
    pub origin_ts: i64,
    /// Matrix event type (e.g. `m.room.message`).
    pub event_type: String,
    /// The event content, serialized as JSON.
    pub content_json: String,
    /// Media metadata (mxc/mimetype/size/dims/filename), or `None` for a
    /// non-media event. Never holds media bytes.
    pub media: Option<ArchiveMedia>,
}

/// The cloneable producer handle for archive ingestion (Story 5.1).
///
/// Created exactly once by [`ArchiveWriter::spawn`] and cloned into every
/// account's event handler, so all Accounts funnel through the one serialized
/// writer / one `archive.db`. [`ArchiveHandle::ingest`] is non-blocking (an
/// unbounded channel), so it never blocks the sync/messaging path.
#[derive(Clone)]
pub struct ArchiveHandle {
    tx: UnboundedSender<ArchiveEvent>,
}

impl ArchiveHandle {
    /// Hand a normalized event to the single writer. Non-blocking and infallible
    /// from the caller's view: an unbounded send never awaits or blocks the
    /// sync/messaging path, and a send onto a closed channel (the writer stopped)
    /// is logged with ids only and dropped — never propagated, never a panic.
    pub fn ingest(&self, ev: ArchiveEvent) {
        if let Err(e) = self.tx.send(ev) {
            // The event that failed to enqueue is `e.0`; log ids only, never
            // content. A closed channel means the writer task ended.
            let ev = e.0;
            tracing::warn!(
                account_id = %ev.account_id,
                event_id = %ev.event_id,
                "archive: writer channel closed; dropping event"
            );
        }
    }
}

/// Spawns and owns the single serialized archive writer task (Story 5.1).
pub struct ArchiveWriter;

impl ArchiveWriter {
    /// Open `archive.db` under `data_dir` and spawn the one writer task over an
    /// unbounded channel, returning the cloneable [`ArchiveHandle`] producers use.
    ///
    /// The writer task owns the [`rusqlite::Connection`] for the app's lifetime
    /// and is the sole writer of `archive.db`. It is spawned runtime-agnostically:
    /// onto the current tokio runtime when one is active (production under Tauri,
    /// and `#[tokio::test]`), otherwise onto a dedicated OS thread running a
    /// minimal current-thread runtime — so construction never depends on being
    /// inside an async context.
    pub fn spawn(data_dir: &Path) -> Result<ArchiveHandle, ArchiveError> {
        let conn = db::open_archive_db(data_dir)?;
        let (tx, rx) = mpsc::unbounded_channel::<ArchiveEvent>();
        spawn_writer(rx, conn);
        Ok(ArchiveHandle { tx })
    }
}

/// Spawn the writer future onto whatever runtime is available (see
/// [`ArchiveWriter::spawn`]). Kept separate so the runtime-selection logic has one
/// home.
fn spawn_writer(rx: mpsc::UnboundedReceiver<ArchiveEvent>, conn: rusqlite::Connection) {
    match tokio::runtime::Handle::try_current() {
        Ok(handle) => {
            handle.spawn(ingest::run(rx, conn));
        }
        Err(_) => {
            // No active runtime (e.g. `AccountManager::new` called synchronously
            // before Tauri enters its runtime): run the writer on its own thread
            // with a minimal current-thread runtime. The thread lives as long as
            // the channel stays open (until every handle is dropped).
            std::thread::Builder::new()
                .name("keeper-archive-writer".to_owned())
                .spawn(move || {
                    match tokio::runtime::Builder::new_current_thread()
                        .enable_all()
                        .build()
                    {
                        Ok(rt) => rt.block_on(ingest::run(rx, conn)),
                        Err(e) => {
                            tracing::error!(error = %e, "archive: could not build writer runtime")
                        }
                    }
                })
                .map(|_| ())
                .unwrap_or_else(
                    |e| tracing::error!(error = %e, "archive: could not spawn writer thread"),
                );
        }
    }
}

/// Resolve the `archive.db` path under a data directory. Exposed for tests and
/// downstream stories that need to locate the archive file; delegates to the
/// single canonical helper in [`db`] so the two never drift.
pub fn archive_db_path(data_dir: &Path) -> PathBuf {
    db::db_path(data_dir)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::db::{event_count, get_event, open_archive_db};
    use std::path::PathBuf;

    fn temp_dir() -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "keeper-archive-mod-test-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_nanos())
                .unwrap_or(0)
        ));
        dir
    }

    fn text_event(account_id: &str, event_id: &str) -> ArchiveEvent {
        ArchiveEvent {
            account_id: account_id.to_owned(),
            event_id: event_id.to_owned(),
            room_id: "!room:e.org".to_owned(),
            sender: "@u:e.org".to_owned(),
            origin_ts: 1_720_000_000_000,
            event_type: "m.room.message".to_owned(),
            content_json: r#"{"msgtype":"m.text","body":"hi"}"#.to_owned(),
            media: None,
        }
    }

    /// End-to-end through the spawned writer under a tokio runtime: ingest a few
    /// events (including a duplicate and a second account), let the writer drain,
    /// then reopen the file and assert every row landed exactly once.
    #[tokio::test]
    async fn spawn_ingests_dedupes_and_multi_accounts_end_to_end() {
        let dir = temp_dir();
        let handle = ArchiveWriter::spawn(&dir).expect("spawn writer");
        handle.ingest(text_event("acctA", "$e1"));
        handle.ingest(text_event("acctA", "$e1")); // duplicate → ignored
        handle.ingest(text_event("acctA", "$e2"));
        handle.ingest(text_event("acctB", "$e1")); // same id, other account
                                                   // Drop the handle so the channel closes and the writer drains and ends.
        drop(handle);
        // Give the writer task a moment to drain the queue and exit.
        for _ in 0..50 {
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            let conn = open_archive_db(&dir).expect("reopen");
            if event_count(&conn, "acctA").unwrap_or(0) == 2
                && event_count(&conn, "acctB").unwrap_or(0) == 1
            {
                break;
            }
        }
        // Reopen (as a restart would) and assert persistence + keying.
        let conn = open_archive_db(&dir).expect("reopen final");
        assert_eq!(event_count(&conn, "acctA").expect("count A"), 2);
        assert_eq!(event_count(&conn, "acctB").expect("count B"), 1);
        assert!(get_event(&conn, "acctA", "$e1").expect("A e1").is_some());
        assert!(get_event(&conn, "acctB", "$e1").expect("B e1").is_some());
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// `ingest` onto a closed channel is a swallowed no-op (never panics).
    #[test]
    fn ingest_after_writer_closed_is_swallowed() {
        let (tx, rx) = mpsc::unbounded_channel::<ArchiveEvent>();
        drop(rx); // writer gone
        let handle = ArchiveHandle { tx };
        handle.ingest(text_event("acctA", "$e1")); // must not panic
    }
}
