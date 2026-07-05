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

use super::{db, fts, ArchiveEvent, ArchiveMsg};

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
    use crate::archive::{ArchiveEvent, ArchiveMedia, ArchiveMsg};
    use std::path::PathBuf;

    fn temp_dir() -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "keeper-archive-ingest-test-{}-{}",
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
}
