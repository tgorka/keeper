//! The single serialized archive writer task (Story 5.1, AD-21, epic 5).
//!
//! Exactly one writer task owns the `archive.db` [`Connection`] app-wide and is
//! the *only* code that writes it. It awaits solely on the channel receive and
//! performs synchronous `INSERT OR IGNORE` between receives — the connection is
//! never shared and never held across any other `.await`.
//!
//! Ingestion is append-only and idempotent (`INSERT OR IGNORE` on the
//! `(account_id, event_id)` primary key): a re-synced event never duplicates or
//! mutates a row. A write failure (DB busy / IO error / media-JSON serialization
//! failure) is logged via `tracing` with ids only — never content — and the task
//! keeps running, so the sync/messaging path is never blocked or aborted.

use rusqlite::Connection;
use tokio::sync::mpsc::UnboundedReceiver;

use tokio::sync::oneshot;

use crate::error::ArchiveError;

use super::{db, fts, recordings, ArchiveEvent, ArchiveMsg};

/// Run the single archive writer loop until the channel closes.
///
/// Owns `conn` for the whole loop (a single owning task, so holding a rusqlite
/// [`Connection`] across the `recv().await` is sound — it is never shared). Each
/// received [`ArchiveMsg`] is applied with a synchronous rusqlite call:
/// `Insert` appends a row with `INSERT OR IGNORE`, `Redact` marks the target
/// row's `redacted_ts`. Any failure is logged with ids only and swallowed — the
/// task never dies, so the sync/messaging path is never blocked. Ends when every
/// [`super::ArchiveHandle`] sender is dropped.
pub(super) async fn run(mut rx: UnboundedReceiver<ArchiveMsg>, conn: Connection) {
    while let Some(msg) = rx.recv().await {
        match msg {
            ArchiveMsg::Insert(ev) => insert_event(&conn, &ev),
            ArchiveMsg::Redact {
                account_id,
                event_id,
                redacted_ts,
            } => mark_redacted(&conn, &account_id, &event_id, redacted_ts),
            ArchiveMsg::DeleteAccount { account_id, done } => {
                delete_account(&conn, &account_id, done)
            }
            ArchiveMsg::UpsertRecording(row) => upsert_recording(&conn, &row),
            ArchiveMsg::UpsertRecordingSegment(row) => upsert_recording_segment(&conn, &row),
            ArchiveMsg::SetRecordingDurability {
                session_id,
                durability,
            } => set_recording_durability(&conn, &session_id, &durability),
            ArchiveMsg::MoveRecording {
                session_id,
                relative_path,
            } => move_recording(&conn, &session_id, &relative_path),
            ArchiveMsg::RebuildRecordings {
                root,
                root_kind,
                profile_id,
            } => rebuild_recordings(&conn, &root, &root_kind, profile_id.as_deref()),
        }
    }
    tracing::info!("archive writer task ended (all senders dropped)");
}

/// Apply one redaction mark, swallowing (and logging with ids only) any failure.
/// A target not present in the archive is a zero-row `UPDATE`, not an error.
fn mark_redacted(conn: &Connection, account_id: &str, event_id: &str, redacted_ts: i64) {
    if let Err(e) = db::mark_redacted(conn, account_id, event_id, redacted_ts) {
        tracing::warn!(
            account_id = %account_id,
            event_id = %event_id,
            error = %e,
            "archive redaction mark failed"
        );
    }
}

/// Write one recording session row, swallowing (and logging with ids only) any
/// failure (Story 42.1). The recorder must never learn that the index write
/// failed: the session's own `manifest.json` is the truth, and a missing row is
/// something `recordings::rebuild_from_disk` fixes.
fn upsert_recording(conn: &Connection, row: &recordings::RecordingRow) {
    if let Err(e) = recordings::upsert_recording(conn, row) {
        tracing::warn!(
            session_id = %row.session_id,
            error = %e,
            "archive: could not write recording row"
        );
    }
}

/// Write one recording segment row, on the same best-effort terms.
fn upsert_recording_segment(conn: &Connection, row: &recordings::RecordingSegmentRow) {
    if let Err(e) = recordings::upsert_segment(conn, row) {
        tracing::warn!(
            session_id = %row.session_id,
            index = row.index,
            error = %e,
            "archive: could not write recording segment row"
        );
    }
}

/// Advance one recording's durability, on the same best-effort terms. A weaker
/// state than the row already holds is a no-op inside the write, not an error
/// here: epic 41's floor lives in one place.
fn set_recording_durability(conn: &Connection, session_id: &str, durability: &str) {
    if let Err(e) = recordings::set_durability(conn, session_id, durability) {
        tracing::warn!(
            session_id = %session_id,
            error = %e,
            "archive: could not update recording durability"
        );
    }
}

/// Repoint one session's row — and every one of its segment rows — at the
/// folder a retitle moved it to (Story 42.1, matrix row 11), on the same
/// best-effort terms as every other recording write.
///
/// A session with no row updates nothing and is not a failure: the index is a
/// cache of what the folders already say, and a retitle of a session that was
/// never indexed is a retitle, not an error.
fn move_recording(conn: &Connection, session_id: &str, relative_path: &str) {
    if let Err(e) = recordings::move_session(conn, session_id, relative_path) {
        tracing::warn!(
            session_id = %session_id,
            error = %e,
            "archive: could not repoint a retitled recording"
        );
    }
}

/// Re-derive every recording row under `root`, on the writer's own connection
/// (Story 42.1).
///
/// This is the production caller that makes the module's central claim true:
/// the manifests are the truth, the rows are a cache of them, and deleting
/// `archive.db` costs nothing that the folders do not already say. It runs here,
/// inside the writer task, because a rebuild reads and rewrites exactly the rows
/// the recorder may be appending to — a second connection doing that would be
/// the one race this whole module is arranged to make impossible.
///
/// Best-effort like every other recording write: a walk that cannot finish is
/// logged and the writer carries on. Nothing upstream is waiting on the count.
fn rebuild_recordings(
    conn: &Connection,
    root: &std::path::Path,
    root_kind: &str,
    profile_id: Option<&str>,
) {
    match recordings::rebuild_from_disk(conn, root, root_kind, profile_id) {
        Ok(0) => {}
        Ok(written) => tracing::info!(
            written,
            root = %root.display(),
            "archive: rebuilt the recordings index from the session folders"
        ),
        Err(e) => tracing::warn!(
            root = %root.display(),
            error = %e,
            "archive: could not rebuild the recordings index"
        ),
    }
}

/// Purge one account's archive through the writer connection (Story 5.7), then
/// forward the purge `Result` on the `done` channel. Logs a failure with ids only
/// (never content) and never panics — the writer task keeps running whatever the
/// outcome, and a closed completion receiver (the awaiting caller went away) is a
/// swallowed no-op.
fn delete_account(
    conn: &Connection,
    account_id: &str,
    done: oneshot::Sender<Result<(), ArchiveError>>,
) {
    let result = db::delete_account_archive(conn, account_id);
    if let Err(e) = &result {
        // Writer-context detail at debug; the single audit warn is emitted one layer
        // up in `AccountManager::delete_account_archive`. Kept here (at debug) so the
        // failure is still observable when the awaiting caller was dropped.
        tracing::debug!(
            account_id = %account_id,
            error = %e,
            "archive: account purge failed (writer)"
        );
    }
    // The awaiting caller may have been dropped; forwarding the outcome is
    // best-effort (never panic on a closed receiver).
    let _ = done.send(result);
}

/// Insert one normalized event, swallowing (and logging with ids only) any
/// failure. Split out so it is unit-testable without a live channel/runtime.
///
/// Serializes the optional [`super::ArchiveMedia`] to `media_json` first; a
/// serialization failure is logged and the row is dropped for this attempt (the
/// writer keeps running). The `INSERT OR IGNORE` makes a duplicate
/// `(account_id, event_id)` a silent no-op. When the base insert actually added a
/// row (rows-affected == 1) and the body is non-empty, the row is indexed into
/// `events_fts` on this *same* writer connection (Story 5.3) — re-synced duplicates
/// (rows-affected == 0) never reach the indexing step, so a row is never
/// double-indexed. An indexing failure is logged with ids only and swallowed; the
/// base row is already committed and the writer keeps running.
fn insert_event(conn: &Connection, ev: &ArchiveEvent) {
    let media_json = match ev.media.as_ref().map(serde_json::to_string).transpose() {
        Ok(json) => json,
        Err(e) => {
            tracing::warn!(
                account_id = %ev.account_id,
                event_id = %ev.event_id,
                error = %e,
                "archive: could not serialize media metadata; dropping row"
            );
            return;
        }
    };
    let inserted_ts = now_ms();
    match db::insert_event(conn, ev, media_json.as_deref(), inserted_ts) {
        Ok(Some(rowid)) => {
            // A row was actually inserted: index its body incrementally through the
            // same writer connection (empty bodies are skipped inside `index_body`).
            if let Err(e) = fts::index_body(conn, rowid, &ev.body) {
                tracing::warn!(
                    account_id = %ev.account_id,
                    event_id = %ev.event_id,
                    error = %e,
                    "archive: could not index body"
                );
            }
        }
        // Re-synced duplicate: no row added, so no indexing (never double-index).
        Ok(None) => {}
        Err(e) => {
            tracing::warn!(
                account_id = %ev.account_id,
                event_id = %ev.event_id,
                error = %e,
                "archive write failed"
            );
        }
    }
}

/// Current wall-clock time in milliseconds since the Unix epoch, or `0` if the
/// clock is before the epoch (never panics — the archive path must not).
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::archive::db::{event_count, get_event, open_archive_db};
    use crate::archive::recordings::{durability_label, RecordingRow, RecordingSegmentRow};
    use crate::archive::{ArchiveEvent, ArchiveMedia, ArchiveMsg};
    use crate::vm::RecordingDurabilityState;
    use std::path::PathBuf;

    /// A scratch directory no other test can land in.
    ///
    /// The pid plus a nanosecond stamp is NOT enough: two test threads that ask
    /// inside the same clock tick get the same name, open the same SQLite file,
    /// and then fail on whichever collision they reach first — a duplicate
    /// migration column or a UNIQUE violation on a fixture inserted twice. Both
    /// were observed on macOS under `cargo test --workspace`. The process-wide
    /// counter is what makes the name unique per CALL, the way
    /// `recording.rs`'s helper already does it.
    fn temp_dir() -> PathBuf {
        static COUNTER: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let n = COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "keeper-archive-ingest-test-{}-{}-{n}",
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
            body: "hi".to_owned(),
            media: None,
            relates_to_event_id: None,
            rel_type: None,
        }
    }

    /// A session row as the recorder hands one to the writer (Story 42.1), with
    /// every column set to something this fixture chose.
    ///
    /// Nothing here is left `None` to be interesting: a row whose optional
    /// columns are absent would assert against whatever the write path happens
    /// to do with a missing value, which is a rule that lives in
    /// `recordings::upsert_recording` and is tested there. These tests are about
    /// the seam — the channel, the arm and the table — so the row is fully
    /// populated and the assertion is simply that it survived the trip.
    fn recording_row(session_id: &str) -> RecordingRow {
        RecordingRow {
            session_id: session_id.to_owned(),
            device_id: Some("01DEVICE".to_owned()),
            relative_path: "2026/kickoff".to_owned(),
            root_kind: "profile".to_owned(),
            profile_id: Some("01PROFILE".to_owned()),
            started_ts: Some(1_754_600_000_000),
            ended_ts: Some(1_754_600_900_000),
            title: Some("Kickoff".to_owned()),
            // The manifest's one free-text participants line, JSON-string encoded
            // the way the column holds it.
            participants_json: Some(r#""Ada, Grace""#.to_owned()),
            note: Some("first pass".to_owned()),
            tags_json: Some(r#"["work","kickoff"]"#.to_owned()),
            custom_json: Some(r#"[{"name":"client","value":"Acme"}]"#.to_owned()),
            codec: Some("h264".to_owned()),
            width: Some(1920),
            height: Some(1080),
            fps: Some(30),
            durability: durability_label(RecordingDurabilityState::Local).to_owned(),
            manifest_version: 1,
        }
    }

    /// One closed segment of [`recording_row`]'s session, populated on the same
    /// terms and for the same reason.
    fn segment_row(session_id: &str) -> RecordingSegmentRow {
        RecordingSegmentRow {
            session_id: session_id.to_owned(),
            index: 0,
            track: "screen".to_owned(),
            relative_path: "2026/kickoff/screen-000.mov".to_owned(),
            bytes: 4_096,
            pts_start: Some(0.0),
            pts_end: Some(12.5),
            closed_ts: Some(1_754_600_120_000),
        }
    }

    /// Read one `recordings` row back into the struct it was written from, or
    /// `None` when the session has no row.
    ///
    /// Reconstructing the whole struct rather than picking columns is what lets a
    /// test say "the stored row IS the row that was sent" in one comparison, and
    /// what makes a column the writer silently failed to carry a failure here
    /// rather than a gap nobody looks at. There is no reader in the archive yet
    /// (Story 42.2 adds the queries), so the SQL is the test's own.
    fn read_recording(conn: &Connection, session_id: &str) -> Option<RecordingRow> {
        let read = conn.query_row(
            "SELECT session_id, device_id, relative_path, root_kind, profile_id, started_ts, \
             ended_ts, title, participants_json, note, tags_json, custom_json, codec, width, \
             height, fps, durability, manifest_version FROM recordings WHERE session_id = ?1",
            rusqlite::params![session_id],
            |r| {
                Ok(RecordingRow {
                    session_id: r.get(0)?,
                    device_id: r.get(1)?,
                    relative_path: r.get(2)?,
                    root_kind: r.get(3)?,
                    profile_id: r.get(4)?,
                    started_ts: r.get(5)?,
                    ended_ts: r.get(6)?,
                    title: r.get(7)?,
                    participants_json: r.get(8)?,
                    note: r.get(9)?,
                    tags_json: r.get(10)?,
                    custom_json: r.get(11)?,
                    codec: r.get(12)?,
                    width: r.get(13)?,
                    height: r.get(14)?,
                    fps: r.get(15)?,
                    durability: r.get(16)?,
                    manifest_version: r.get(17)?,
                })
            },
        );
        match read {
            Ok(row) => Some(row),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => panic!("could not read recording row: {e}"),
        }
    }

    /// Read one `recording_segments` row back, on the same terms as
    /// [`read_recording`]. The byte count comes back through `i64` because that
    /// is the width SQLite stores it at.
    fn read_segment(
        conn: &Connection,
        session_id: &str,
        index: u32,
        track: &str,
    ) -> Option<RecordingSegmentRow> {
        let read = conn.query_row(
            "SELECT session_id, \"index\", track, relative_path, bytes, pts_start, pts_end, \
             closed_ts FROM recording_segments WHERE session_id = ?1 AND \"index\" = ?2 \
             AND track = ?3",
            rusqlite::params![session_id, index, track],
            |r| {
                Ok(RecordingSegmentRow {
                    session_id: r.get(0)?,
                    index: r.get(1)?,
                    track: r.get(2)?,
                    relative_path: r.get(3)?,
                    bytes: r.get::<_, i64>(4)? as u64,
                    pts_start: r.get(5)?,
                    pts_end: r.get(6)?,
                    closed_ts: r.get(7)?,
                })
            },
        );
        match read {
            Ok(row) => Some(row),
            Err(rusqlite::Error::QueryReturnedNoRows) => None,
            Err(e) => panic!("could not read recording segment row: {e}"),
        }
    }

    #[test]
    fn insert_edit_persists_relation_columns() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        let mut edit = text_event("acctA", "$edit");
        edit.relates_to_event_id = Some("$orig".to_owned());
        edit.rel_type = Some("m.replace".to_owned());
        insert_event(&conn, &edit);
        let row = get_event(&conn, "acctA", "$edit")
            .expect("get")
            .expect("row");
        assert_eq!(row.relates_to_event_id.as_deref(), Some("$orig"));
        assert_eq!(row.rel_type.as_deref(), Some("m.replace"));
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn insert_then_read_back_a_text_event() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        insert_event(&conn, &text_event("acctA", "$e1"));
        let row = get_event(&conn, "acctA", "$e1")
            .expect("get")
            .expect("row present");
        assert_eq!(row.room_id, "!room:e.org");
        assert_eq!(row.sender, "@u:e.org");
        assert_eq!(row.origin_ts, 1_720_000_000_000);
        assert_eq!(row.event_type, "m.room.message");
        assert_eq!(row.content_json, r#"{"msgtype":"m.text","body":"hi"}"#);
        assert_eq!(row.media_json, None);
        assert!(row.inserted_ts >= 0);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn duplicate_event_is_idempotent() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        insert_event(&conn, &text_event("acctA", "$e1"));
        // Re-ingest the SAME (account_id, event_id) with different content: INSERT
        // OR IGNORE keeps exactly the first row, unchanged.
        let mut again = text_event("acctA", "$e1");
        again.content_json = r#"{"msgtype":"m.text","body":"changed"}"#.to_owned();
        insert_event(&conn, &again);
        assert_eq!(event_count(&conn, "acctA").expect("count"), 1);
        let row = get_event(&conn, "acctA", "$e1").expect("get").expect("row");
        assert_eq!(
            row.content_json, r#"{"msgtype":"m.text","body":"hi"}"#,
            "the original row must be unchanged"
        );
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn media_metadata_round_trips_as_json() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        let mut ev = text_event("acctA", "$img");
        ev.event_type = "m.room.message".to_owned();
        ev.media = Some(ArchiveMedia {
            mxc: Some("mxc://e.org/abc".to_owned()),
            mimetype: Some("image/png".to_owned()),
            size: Some(2048),
            width: Some(640),
            height: Some(480),
            filename: Some("cat.png".to_owned()),
            thumbnail_mxc: Some("mxc://e.org/thumb".to_owned()),
        });
        insert_event(&conn, &ev);
        let row = get_event(&conn, "acctA", "$img")
            .expect("get")
            .expect("row");
        let media_json = row.media_json.expect("media_json present");
        let media: ArchiveMedia = serde_json::from_str(&media_json).expect("deserialize media");
        assert_eq!(media.mxc.as_deref(), Some("mxc://e.org/abc"));
        assert_eq!(media.mimetype.as_deref(), Some("image/png"));
        assert_eq!(media.size, Some(2048));
        assert_eq!(media.width, Some(640));
        assert_eq!(media.height, Some(480));
        assert_eq!(media.filename.as_deref(), Some("cat.png"));
        assert_eq!(media.thumbnail_mxc.as_deref(), Some("mxc://e.org/thumb"));
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn multi_account_rows_are_keyed_by_account() {
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        // Same event_id under two accounts must NOT collide (PK is the pair).
        insert_event(&conn, &text_event("acctA", "$shared"));
        insert_event(&conn, &text_event("acctB", "$shared"));
        assert_eq!(event_count(&conn, "acctA").expect("count A"), 1);
        assert_eq!(event_count(&conn, "acctB").expect("count B"), 1);
        assert!(get_event(&conn, "acctA", "$shared").expect("A").is_some());
        assert!(get_event(&conn, "acctB", "$shared").expect("B").is_some());
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn write_failure_is_swallowed_and_writer_survives() {
        // Drop the `events` table out from under the writer so the next INSERT
        // fails ("no such table"). insert_event must log-and-swallow (never
        // panic), and once the table is restored, a subsequent insert succeeds —
        // proving the writer keeps running after a write failure.
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        conn.execute("DROP TABLE events", [])
            .expect("drop events table");
        // This insert MUST fail internally; the assertion is that we return here
        // without panicking (the failure is swallowed).
        insert_event(&conn, &text_event("acctA", "$e1"));
        // Restore the schema and prove the writer still works afterward.
        drop(conn);
        let conn = open_archive_db(&dir).expect("reopen restores schema");
        insert_event(&conn, &text_event("acctA", "$e2"));
        assert_eq!(event_count(&conn, "acctA").expect("count"), 1);
        assert!(get_event(&conn, "acctA", "$e1").expect("get e1").is_none());
        assert!(get_event(&conn, "acctA", "$e2").expect("get e2").is_some());
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// The single writer applies both `Insert` and `Redact` through one channel:
    /// insert a row, then a redaction marks it (content retained), and a redaction
    /// for an absent target is a swallowed no-op.
    #[tokio::test]
    async fn run_applies_insert_then_redact_through_one_writer() {
        use tokio::sync::mpsc;
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        let (tx, rx) = mpsc::unbounded_channel::<ArchiveMsg>();
        let task = tokio::spawn(run(rx, conn));
        tx.send(ArchiveMsg::Insert(Box::new(text_event("acctA", "$e1"))))
            .expect("send insert");
        tx.send(ArchiveMsg::Redact {
            account_id: "acctA".to_owned(),
            event_id: "$e1".to_owned(),
            redacted_ts: 555,
        })
        .expect("send redact");
        // A redaction for a target that was never ingested: a zero-row no-op.
        tx.send(ArchiveMsg::Redact {
            account_id: "acctA".to_owned(),
            event_id: "$ghost".to_owned(),
            redacted_ts: 777,
        })
        .expect("send redact ghost");
        drop(tx); // close the channel so the writer drains and ends
        task.await.expect("writer task joins");

        let conn = open_archive_db(&dir).expect("reopen");
        let row = get_event(&conn, "acctA", "$e1").expect("get").expect("row");
        assert_eq!(row.redacted_ts, Some(555));
        assert_eq!(
            row.content_json, r#"{"msgtype":"m.text","body":"hi"}"#,
            "content retained through redaction mark"
        );
        assert!(get_event(&conn, "acctA", "$ghost")
            .expect("get ghost")
            .is_none());
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A `DeleteAccount` message through the single writer resolves its oneshot with
    /// `Ok(())` and the account's rows are gone afterward.
    #[tokio::test]
    async fn run_delete_account_resolves_ok_and_removes_rows() {
        use tokio::sync::{mpsc, oneshot};
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        let (tx, rx) = mpsc::unbounded_channel::<ArchiveMsg>();
        let task = tokio::spawn(run(rx, conn));
        tx.send(ArchiveMsg::Insert(Box::new(text_event("acctA", "$e1"))))
            .expect("send insert");
        tx.send(ArchiveMsg::Insert(Box::new(text_event("acctB", "$e1"))))
            .expect("send insert B");
        let (done, ack) = oneshot::channel();
        tx.send(ArchiveMsg::DeleteAccount {
            account_id: "acctA".to_owned(),
            done,
        })
        .expect("send delete");
        let result = ack.await.expect("writer acknowledges");
        assert!(result.is_ok(), "purge resolves Ok");
        drop(tx);
        task.await.expect("writer task joins");

        let conn = open_archive_db(&dir).expect("reopen");
        assert_eq!(event_count(&conn, "acctA").expect("count A"), 0);
        assert_eq!(event_count(&conn, "acctB").expect("count B"), 1);
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A recording session reaches `archive.db` through the very same one writer
    /// the message path uses: an `UpsertRecording` sent on the real channel is a
    /// real `recordings` row afterwards, column for column.
    ///
    /// **This is the seam between Story 42.1's two halves** — the tables and the
    /// derivation on one side, the channel and the writer arm on the other — and
    /// it is the only test that observes them joined. Every other recording test
    /// calls `recordings::upsert_recording` directly and would stay green with
    /// the writer arm deleted.
    #[tokio::test]
    async fn run_writes_a_recording_row_sent_through_the_writer_channel() {
        use tokio::sync::mpsc;
        let session_id = "01DEVICE-01SESSION";
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        let (tx, rx) = mpsc::unbounded_channel::<ArchiveMsg>();
        let task = tokio::spawn(run(rx, conn));
        let row = recording_row(session_id);
        tx.send(ArchiveMsg::UpsertRecording(Box::new(row.clone())))
            .expect("send recording");
        drop(tx); // close the channel so the writer drains and ends
        task.await.expect("writer task joins");

        let conn = open_archive_db(&dir).expect("reopen");
        assert_eq!(
            read_recording(&conn, session_id),
            Some(row),
            "the stored row is the row that was sent, field for field"
        );
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A closed segment reported on the channel becomes a `recording_segments`
    /// row through the same writer, so the ledger the manifest carries is
    /// queryable without reopening the folder.
    #[tokio::test]
    async fn run_writes_a_recording_segment_row_sent_through_the_writer_channel() {
        use tokio::sync::mpsc;
        let session_id = "01DEVICE-01SESSION";
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        let (tx, rx) = mpsc::unbounded_channel::<ArchiveMsg>();
        let task = tokio::spawn(run(rx, conn));
        let segment = segment_row(session_id);
        tx.send(ArchiveMsg::UpsertRecordingSegment(Box::new(
            segment.clone(),
        )))
        .expect("send segment");
        drop(tx);
        task.await.expect("writer task joins");

        let conn = open_archive_db(&dir).expect("reopen");
        assert_eq!(
            read_segment(&conn, session_id, 0, "screen"),
            Some(segment),
            "the stored segment is the segment that was sent, field for field"
        );
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// Durability climbs through the writer and never falls: a session written as
    /// `local` and then advanced to `pushed` stays `pushed` when a later
    /// `committed` arrives.
    ///
    /// Epic 41 defines the state a session reports as a floor, and a durability
    /// poll can genuinely deliver a weaker reading after a stronger one (the two
    /// observations race). The rule itself lives in `recordings`; what this pins
    /// is that the writer arm actually reaches it — the floor is observed where
    /// the app observes it, on the channel, rather than by calling the write
    /// helper directly.
    #[tokio::test]
    async fn run_advances_recording_durability_and_a_weaker_state_never_walks_it_back() {
        use tokio::sync::mpsc;
        let session_id = "01DEVICE-01SESSION";
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        let (tx, rx) = mpsc::unbounded_channel::<ArchiveMsg>();
        let task = tokio::spawn(run(rx, conn));
        // The row starts at `local` (the fixture's state).
        tx.send(ArchiveMsg::UpsertRecording(Box::new(recording_row(
            session_id,
        ))))
        .expect("send recording");
        tx.send(ArchiveMsg::SetRecordingDurability {
            session_id: session_id.to_owned(),
            durability: durability_label(RecordingDurabilityState::Pushed).to_owned(),
        })
        .expect("send pushed");
        // Weaker than what the row now holds: the floor must refuse it.
        tx.send(ArchiveMsg::SetRecordingDurability {
            session_id: session_id.to_owned(),
            durability: durability_label(RecordingDurabilityState::Committed).to_owned(),
        })
        .expect("send committed");
        drop(tx);
        task.await.expect("writer task joins");

        let conn = open_archive_db(&dir).expect("reopen");
        let stored = read_recording(&conn, session_id).expect("row present");
        assert_eq!(
            stored.durability,
            durability_label(RecordingDurabilityState::Pushed),
            "the advance landed and the weaker state that followed did not undo it"
        );
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A recording write that fails is logged and swallowed exactly like an event
    /// write that fails: the writer task survives it, and everything sent
    /// afterwards still lands.
    ///
    /// This is the spec's "a failure to index is logged, never surfaced as a
    /// recording failure" seen from the writer's side — the recorder is not even
    /// on this path to be told. The failure is induced the way
    /// `write_failure_is_swallowed_and_writer_survives` induces its own: the
    /// table is dropped out from under the writer, so the next write fails with
    /// "no such table". It is then put back through a SECOND connection to the
    /// same file — the writer owns its own connection for the whole loop and
    /// cannot be reached, and SQLite makes the recreated table visible to it on
    /// its next statement — which is what lets the test prove the writer is
    /// still writing recordings rather than merely still alive.
    #[tokio::test]
    async fn run_swallows_a_failing_recording_write_and_keeps_writing_afterwards() {
        use tokio::sync::{mpsc, oneshot};
        let dir = temp_dir();
        let conn = open_archive_db(&dir).expect("open");
        conn.execute("DROP TABLE recordings", [])
            .expect("drop recordings table");
        let (tx, rx) = mpsc::unbounded_channel::<ArchiveMsg>();
        let task = tokio::spawn(run(rx, conn));
        // This one MUST fail inside the writer; the assertion is that the task
        // keeps running and the messages behind it are still applied.
        tx.send(ArchiveMsg::UpsertRecording(Box::new(recording_row(
            "01DEVICE-0DOOMED",
        ))))
        .expect("send doomed recording");
        tx.send(ArchiveMsg::Insert(Box::new(text_event("acctA", "$e1"))))
            .expect("send insert");
        // A purge acknowledgement is the barrier: the writer applies messages in
        // the order they were sent, so an ack for an account with no rows proves
        // the doomed write ahead of it has already been attempted. Only then is
        // it safe to put the table back, or the "failure" might never happen.
        let (done, ack) = oneshot::channel();
        tx.send(ArchiveMsg::DeleteAccount {
            account_id: "acctZ".to_owned(),
            done,
        })
        .expect("send barrier purge");
        let _ = ack.await.expect("writer acknowledges");
        drop(open_archive_db(&dir).expect("second open restores the recordings schema"));
        tx.send(ArchiveMsg::UpsertRecording(Box::new(recording_row(
            "01DEVICE-01SESSION",
        ))))
        .expect("send recording after restore");
        drop(tx);
        task.await
            .expect("writer task survives the failed recording write");

        let conn = open_archive_db(&dir).expect("reopen");
        assert!(
            get_event(&conn, "acctA", "$e1").expect("get e1").is_some(),
            "the ordinary write behind the failed one still landed"
        );
        assert!(
            read_recording(&conn, "01DEVICE-01SESSION").is_some(),
            "the writer is still writing recordings after swallowing a failure"
        );
        assert!(
            read_recording(&conn, "01DEVICE-0DOOMED").is_none(),
            "the write that failed left nothing behind"
        );
        drop(conn);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
