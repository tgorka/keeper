//! Integration coverage for the Story 5.1 archive ingestion pipeline.
//!
//! Drives the public [`keeper_core::archive`] API end to end through the single
//! serialized writer, covering every row of the spec's I/O & Edge-Case Matrix:
//! insert, dedupe idempotency, media-metadata round-trip, reopen-after-close
//! persistence, and multi-account keying. Write-failure resilience and the pure
//! matrix-event mapping are covered by colocated unit tests.

use std::path::PathBuf;
use std::time::Duration;

use keeper_core::archive::db::{event_count, get_event, open_archive_db};
use keeper_core::archive::{ArchiveEvent, ArchiveHandle, ArchiveMedia, ArchiveWriter};

/// A unique temp data dir per test run (real SQLite files under the OS temp dir).
fn temp_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "keeper-archive-it-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    dir
}

fn text_event(account_id: &str, event_id: &str, body: &str) -> ArchiveEvent {
    ArchiveEvent {
        account_id: account_id.to_owned(),
        event_id: event_id.to_owned(),
        room_id: "!room:example.org".to_owned(),
        sender: "@bob:example.org".to_owned(),
        origin_ts: 1_720_000_000_000,
        event_type: "m.room.message".to_owned(),
        content_json: format!(r#"{{"msgtype":"m.text","body":"{body}"}}"#),
        media: None,
        relates_to_event_id: None,
        rel_type: None,
    }
}

/// Count archived rows for `account_id` by opening the archive file (a fresh
/// read connection; the writer owns its own). Used by the polling helper.
fn count_for(dir: &std::path::Path, account_id: &str) -> i64 {
    let conn = open_archive_db(dir).expect("open archive.db for count");
    event_count(&conn, account_id).unwrap_or(0)
}

/// Poll the archive file until `pred` holds or the deadline elapses. Necessary
/// because the writer drains its channel asynchronously.
fn wait_until(mut pred: impl FnMut() -> bool) {
    for _ in 0..100 {
        if pred() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(pred(), "condition not reached before deadline");
}

/// Insert, dedupe, media round-trip, and multi-account keying — all through the
/// single spawned writer on a shared `archive.db`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ingests_dedupes_media_and_multi_accounts() {
    let dir = temp_dir("core");
    let handle: ArchiveHandle = ArchiveWriter::spawn(&dir).expect("spawn writer");

    // New message events for account A.
    handle.ingest(text_event("acctA", "$e1", "hello"));
    handle.ingest(text_event("acctA", "$e2", "world"));
    // Duplicate (re-sync) of $e1 with different content: must be ignored.
    handle.ingest(text_event("acctA", "$e1", "CHANGED"));
    // A media message (metadata only, no bytes).
    let mut media_ev = text_event("acctA", "$img", "cat.png");
    media_ev.media = Some(ArchiveMedia {
        mxc: Some("mxc://example.org/abc".to_owned()),
        mimetype: Some("image/png".to_owned()),
        size: Some(2048),
        width: Some(640),
        height: Some(480),
        filename: Some("cat.png".to_owned()),
        thumbnail_mxc: Some("mxc://example.org/thumb".to_owned()),
    });
    handle.ingest(media_ev);
    // Account B with the SAME event id as A's $e1 — keyed by (account, event).
    handle.ingest(text_event("acctB", "$e1", "other account"));

    // Wait for the writer to drain all four distinct rows (3 for A, 1 for B).
    wait_until(|| count_for(&dir, "acctA") == 3 && count_for(&dir, "acctB") == 1);

    let conn = open_archive_db(&dir).expect("open for assertions");
    // Dedupe: exactly one $e1 for acctA, holding the ORIGINAL content.
    assert_eq!(event_count(&conn, "acctA").expect("count A"), 3);
    let e1 = get_event(&conn, "acctA", "$e1")
        .expect("get e1")
        .expect("e1 present");
    assert!(
        e1.content_json.contains("hello"),
        "duplicate must not overwrite the original row"
    );
    assert!(!e1.content_json.contains("CHANGED"));

    // Media metadata round-trips; no bytes present.
    let img = get_event(&conn, "acctA", "$img")
        .expect("get img")
        .expect("img present");
    let media_json = img.media_json.expect("media_json present");
    let media: ArchiveMedia = serde_json::from_str(&media_json).expect("deserialize media");
    assert_eq!(media.mxc.as_deref(), Some("mxc://example.org/abc"));
    assert_eq!(media.mimetype.as_deref(), Some("image/png"));
    assert_eq!(media.size, Some(2048));
    assert_eq!(media.filename.as_deref(), Some("cat.png"));

    // Multi-account keying: acctB's $e1 is a distinct row.
    assert_eq!(event_count(&conn, "acctB").expect("count B"), 1);
    assert!(get_event(&conn, "acctB", "$e1")
        .expect("get B e1")
        .is_some());

    drop(conn);
    let _ = std::fs::remove_dir_all(&dir);
}

/// Reopen-after-close persistence (restart, network off): rows committed by one
/// writer are present when a fresh writer reopens the same `archive.db`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn rows_survive_writer_close_and_reopen() {
    let dir = temp_dir("persist");
    {
        let handle = ArchiveWriter::spawn(&dir).expect("spawn writer 1");
        handle.ingest(text_event("acctA", "$p1", "durable"));
        handle.ingest(text_event("acctA", "$p2", "durable2"));
        wait_until(|| count_for(&dir, "acctA") == 2);
        // Drop the handle → channel closes → writer task drains and ends.
        drop(handle);
    }
    // Give the first writer a moment to fully finish before reopening.
    tokio::time::sleep(Duration::from_millis(50)).await;

    // A brand-new writer over the SAME file sees the previously committed rows —
    // this is the "app reopened, no network" case.
    let _handle2 = ArchiveWriter::spawn(&dir).expect("spawn writer 2");
    let conn = open_archive_db(&dir).expect("reopen");
    assert_eq!(event_count(&conn, "acctA").expect("count"), 2);
    assert!(get_event(&conn, "acctA", "$p1").expect("p1").is_some());
    assert!(get_event(&conn, "acctA", "$p2").expect("p2").is_some());

    drop(conn);
    let _ = std::fs::remove_dir_all(&dir);
}
