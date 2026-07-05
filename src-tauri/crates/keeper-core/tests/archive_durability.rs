//! Integration coverage for Story 5.2 (durability against remote rewrites +
//! edit history).
//!
//! Drives the public [`keeper_core::archive`] API end to end through the single
//! serialized writer, covering the spec's I/O & Edge-Case Matrix rows that span
//! the writer + read helpers: edit-chain extraction & ordering, plain-message
//! fallback, redaction marking (incl. target-absent no-op), `retrievable_content`
//! honor on/off, the honor-deletions setting round-trip, and migration
//! idempotency over a pre-5.1 schema. The pure chain→VM mapping and the matrix
//! relation extraction are covered by colocated unit tests.

use std::path::{Path, PathBuf};
use std::time::Duration;

use keeper_core::archive::db::{edit_chain, get_event, open_archive_db, retrievable_content};
use keeper_core::archive::{
    get_honor_remote_deletions, set_honor_remote_deletions, ArchiveEvent, ArchiveHandle,
    ArchiveWriter,
};
use rusqlite::Connection;

/// A unique temp data dir per test run.
fn temp_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "keeper-archive-dur-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    dir
}

/// A plain message row (no relation).
fn text_event(account_id: &str, event_id: &str, origin_ts: i64, body: &str) -> ArchiveEvent {
    ArchiveEvent {
        account_id: account_id.to_owned(),
        event_id: event_id.to_owned(),
        room_id: "!room:example.org".to_owned(),
        sender: "@bob:example.org".to_owned(),
        origin_ts,
        event_type: "m.room.message".to_owned(),
        content_json: format!(r#"{{"msgtype":"m.text","body":"{body}"}}"#),
        body: body.to_owned(),
        media: None,
        relates_to_event_id: None,
        rel_type: None,
    }
}

/// An edit (`m.replace`) row targeting `target`.
fn edit_event(
    account_id: &str,
    event_id: &str,
    origin_ts: i64,
    target: &str,
    new_body: &str,
) -> ArchiveEvent {
    ArchiveEvent {
        account_id: account_id.to_owned(),
        event_id: event_id.to_owned(),
        room_id: "!room:example.org".to_owned(),
        sender: "@bob:example.org".to_owned(),
        origin_ts,
        event_type: "m.room.message".to_owned(),
        content_json: format!(
            r#"{{"msgtype":"m.text","body":"* {new_body}","m.new_content":{{"msgtype":"m.text","body":"{new_body}"}}}}"#
        ),
        body: new_body.to_owned(),
        media: None,
        relates_to_event_id: Some(target.to_owned()),
        rel_type: Some("m.replace".to_owned()),
    }
}

/// Poll a read connection until `pred` holds or the deadline elapses (the writer
/// drains its channel asynchronously).
fn wait_until(dir: &Path, mut pred: impl FnMut(&Connection) -> bool) {
    for _ in 0..100 {
        let conn = open_archive_db(dir).expect("open for poll");
        if pred(&conn) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let conn = open_archive_db(dir).expect("open final poll");
    assert!(pred(&conn), "condition not reached before deadline");
}

/// A remote edit synced through the writer stores its own row with the relation
/// columns, and the version chain reads back original-then-edits by `origin_ts`.
#[test]
fn edit_chain_built_through_the_writer() {
    let dir = temp_dir("chain");
    let handle: ArchiveHandle = ArchiveWriter::spawn(&dir).expect("spawn writer");
    handle.ingest(text_event("acctA", "$orig", 100, "v1"));
    handle.ingest(edit_event("acctA", "$edit1", 200, "$orig", "v2"));
    handle.ingest(edit_event("acctA", "$edit2", 300, "$orig", "v3"));
    drop(handle);

    wait_until(&dir, |conn| {
        edit_chain(conn, "acctA", "$orig")
            .map(|c| c.len())
            .unwrap_or(0)
            == 3
    });
    let conn = open_archive_db(&dir).expect("reopen");
    let chain = edit_chain(&conn, "acctA", "$orig").expect("chain");
    let ids: Vec<&str> = chain.iter().map(|r| r.event_id.as_str()).collect();
    assert_eq!(ids, vec!["$orig", "$edit1", "$edit2"]);
    // The original row is never mutated by the edits.
    let orig = get_event(&conn, "acctA", "$orig")
        .expect("get")
        .expect("row");
    assert_eq!(orig.content_json, r#"{"msgtype":"m.text","body":"v1"}"#);
    assert_eq!(orig.relates_to_event_id, None);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A message with no relation is stored plainly (no relation cols) and forms a
/// single-version chain.
#[test]
fn plain_message_has_no_relation_and_single_version_chain() {
    let dir = temp_dir("plain");
    let handle = ArchiveWriter::spawn(&dir).expect("spawn writer");
    handle.ingest(text_event("acctA", "$solo", 100, "hi"));
    drop(handle);

    wait_until(&dir, |conn| {
        get_event(conn, "acctA", "$solo").ok().flatten().is_some()
    });
    let conn = open_archive_db(&dir).expect("reopen");
    let row = get_event(&conn, "acctA", "$solo")
        .expect("get")
        .expect("row");
    assert_eq!(row.relates_to_event_id, None);
    assert_eq!(row.rel_type, None);
    let chain = edit_chain(&conn, "acctA", "$solo").expect("chain");
    assert_eq!(chain.len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A remote redaction synced through the writer marks the target row's
/// `redacted_ts` while retaining content; `retrievable_content` honors the policy
/// on read; a redaction for an absent target is a no-op.
#[test]
fn redaction_marks_and_retrievable_content_honors_policy() {
    let dir = temp_dir("redact");
    let handle = ArchiveWriter::spawn(&dir).expect("spawn writer");
    handle.ingest(text_event("acctA", "$e1", 100, "kept"));
    // Wait for the insert before the redact so ordering is observable.
    wait_until(&dir, |conn| {
        get_event(conn, "acctA", "$e1").ok().flatten().is_some()
    });
    handle.redact("acctA", "$e1", 999);
    // A redaction for a target never ingested must be a swallowed no-op.
    handle.redact("acctA", "$ghost", 555);
    drop(handle);

    wait_until(&dir, |conn| {
        get_event(conn, "acctA", "$e1")
            .ok()
            .flatten()
            .and_then(|r| r.redacted_ts)
            .is_some()
    });
    let conn = open_archive_db(&dir).expect("reopen");
    let row = get_event(&conn, "acctA", "$e1").expect("get").expect("row");
    assert_eq!(row.redacted_ts, Some(999));
    assert_eq!(
        row.content_json, r#"{"msgtype":"m.text","body":"kept"}"#,
        "content retained on redaction mark"
    );
    // honor OFF → returns pre-redaction content; honor ON → None (row still on disk).
    assert!(retrievable_content(&conn, "acctA", "$e1", false)
        .expect("off")
        .is_some());
    assert!(retrievable_content(&conn, "acctA", "$e1", true)
        .expect("on")
        .is_none());
    assert!(get_event(&conn, "acctA", "$ghost")
        .expect("ghost")
        .is_none());
    let _ = std::fs::remove_dir_all(&dir);
}

/// The app-wide honor-deletions setting round-trips through `keeper.db`: absent ⇒
/// false, then on/off persist.
#[test]
fn honor_remote_deletions_setting_round_trips() {
    let dir = temp_dir("setting");
    assert!(!get_honor_remote_deletions(&dir).expect("absent ⇒ false"));
    set_honor_remote_deletions(&dir, true).expect("set on");
    assert!(get_honor_remote_deletions(&dir).expect("on"));
    set_honor_remote_deletions(&dir, false).expect("set off");
    assert!(!get_honor_remote_deletions(&dir).expect("off"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// Reopening a pre-5.1 `archive.db` (9-column schema) adds the durability
/// columns/index idempotently, keeps old rows queryable, and a re-run is a no-op.
#[test]
fn reopen_pre_5_1_archive_migrates_idempotently() {
    let dir = temp_dir("migrate");
    std::fs::create_dir_all(&dir).expect("mkdir");
    let db_file = dir.join("archive.db");
    {
        let conn = Connection::open(&db_file).expect("open raw");
        conn.execute(
            "CREATE TABLE events(\
                account_id TEXT NOT NULL, event_id TEXT NOT NULL, room_id TEXT NOT NULL, \
                sender TEXT NOT NULL, origin_ts INTEGER NOT NULL, event_type TEXT NOT NULL, \
                content_json TEXT NOT NULL, media_json TEXT, inserted_ts INTEGER NOT NULL, \
                PRIMARY KEY(account_id, event_id))",
            [],
        )
        .expect("old schema");
        conn.execute(
            "INSERT INTO events(account_id, event_id, room_id, sender, origin_ts, event_type, \
             content_json, media_json, inserted_ts) \
             VALUES ('acctA', '$old', '!r', '@u:e.org', 42, 'm.room.message', \
             '{\"msgtype\":\"m.text\",\"body\":\"old\"}', NULL, 7)",
            [],
        )
        .expect("old row");
    }
    // Reopen via the migration path.
    let conn = open_archive_db(&dir).expect("open migrates");
    let row = get_event(&conn, "acctA", "$old")
        .expect("get")
        .expect("row");
    assert_eq!(row.content_json, r#"{"msgtype":"m.text","body":"old"}"#);
    assert_eq!(row.redacted_ts, None);
    drop(conn);
    // Re-run: a second open is a no-op; the row survives.
    let conn = open_archive_db(&dir).expect("reopen no-op");
    assert!(get_event(&conn, "acctA", "$old").expect("get").is_some());
    let _ = std::fs::remove_dir_all(&dir);
}
