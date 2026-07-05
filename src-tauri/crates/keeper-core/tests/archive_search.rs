//! Integration coverage for Story 5.3 (offline full-text search engine).
//!
//! Drives the public [`keeper_core::archive`] search API end to end through the
//! single serialized writer and a fresh read connection, covering every row of the
//! spec's I/O & Edge-Case Matrix: ingest indexing, the re-synced-duplicate
//! no-double-index guarantee, trigram MATCH (≥3), the `LIKE` fallback (<3), a CJK
//! query, each filter (account / room / sender / date-range), honor-deletions
//! on/off over a redacted match, an edit-version match deduped to one chain-root
//! hit, an empty-body event stored-but-not-indexed, a pre-5.1/5.2 archive migrating
//! + backfilling + becoming searchable (re-open a no-op), and a no-match empty vec.
//!
//! Colocated unit tests in `fts.rs`/`db.rs` cover the dispatch boundary, chain-root
//! dedup, and empty-body skip at the unit level; this file exercises the whole
//! path through the writer and the honor-deletions setting.

use std::path::{Path, PathBuf};
use std::time::Duration;

use keeper_core::archive::db::{mark_redacted, open_archive_db, open_readonly_archive_db};
use keeper_core::archive::{
    search, set_honor_remote_deletions, ArchiveEvent, ArchiveHandle, ArchiveWriter, SearchFilter,
};
use rusqlite::Connection;

/// A unique temp data dir per test run.
fn temp_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "keeper-archive-search-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    dir
}

/// A plain text message row in room `!r1` from `@alice` unless overridden.
fn text_event(account_id: &str, event_id: &str, origin_ts: i64, body: &str) -> ArchiveEvent {
    ArchiveEvent {
        account_id: account_id.to_owned(),
        event_id: event_id.to_owned(),
        room_id: "!r1:example.org".to_owned(),
        sender: "@alice:example.org".to_owned(),
        origin_ts,
        event_type: "m.room.message".to_owned(),
        content_json: format!(r#"{{"msgtype":"m.text","body":"{body}"}}"#),
        body: body.to_owned(),
        media: None,
        relates_to_event_id: None,
        rel_type: None,
    }
}

/// Poll a read connection until `pred` holds or the deadline elapses (the writer
/// drains its channel asynchronously).
fn wait_until(dir: &Path, mut pred: impl FnMut(&Connection) -> bool) {
    for _ in 0..200 {
        let conn = open_archive_db(dir).expect("open for poll");
        if pred(&conn) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    let conn = open_archive_db(dir).expect("open final poll");
    assert!(pred(&conn), "condition not reached before deadline");
}

/// Count *indexed documents* in `events_fts`. `events_fts_docsize` holds one row
/// per indexed document (keyed by content rowid); a plain `COUNT(*) FROM events_fts`
/// would instead count the external content table (`events`), so it is not a signal
/// for how many rows were actually indexed.
fn fts_count(conn: &Connection) -> i64 {
    conn.query_row("SELECT COUNT(*) FROM events_fts_docsize", [], |r| r.get(0))
        .expect("count events_fts_docsize")
}

/// A query-only filter (no restrictions).
fn q(query: &str) -> SearchFilter {
    SearchFilter {
        query: query.to_owned(),
        ..Default::default()
    }
}

/// Ingest through the writer, a re-synced duplicate must NOT double-index, and the
/// indexed row is findable via trigram MATCH (≥3 chars).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn ingest_indexes_and_duplicate_does_not_double_index() {
    let dir = temp_dir("ingest");
    let handle: ArchiveHandle = ArchiveWriter::spawn(&dir).expect("spawn");
    handle.ingest(text_event("acctA", "$e1", 100, "hello there"));
    handle.ingest(text_event("acctA", "$e1", 100, "hello there")); // duplicate
    handle.ingest(text_event("acctA", "$e2", 200, "another message"));
    drop(handle);
    // Wait until both distinct rows are indexed exactly once (2 fts rows, not 3).
    wait_until(&dir, |conn| fts_count(conn) == 2);

    let conn = open_readonly_archive_db(&dir).expect("readonly");
    assert_eq!(fts_count(&conn), 2, "duplicate must not double-index");
    let hits = search(&conn, &q("hello"), false).expect("search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].event_id, "$e1");
    assert_eq!(hits[0].body, "hello there");
    assert_eq!(hits[0].account_id, "acctA");
    assert_eq!(hits[0].room_id, "!r1:example.org");
    assert_eq!(hits[0].sender, "@alice:example.org");
    assert!(!hits[0].redacted);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A short query (<3 Unicode scalars) uses the `LIKE` fallback and still matches;
/// results honor `origin_ts DESC` ordering.
#[test]
fn short_query_uses_like_fallback_and_orders_desc() {
    let dir = temp_dir("like");
    let conn = open_archive_db(&dir).expect("open");
    // Direct ingest via the sync insert path (writer-equivalent) for determinism.
    ingest_sync(&conn, &text_event("acctA", "$e1", 100, "hi world"));
    ingest_sync(&conn, &text_event("acctA", "$e2", 300, "hi again"));
    ingest_sync(&conn, &text_event("acctA", "$e3", 200, "nope"));
    let hits = search(&conn, &q("hi"), false).expect("search");
    let ids: Vec<&str> = hits.iter().map(|h| h.event_id.as_str()).collect();
    // "hi" is a 2-char query → LIKE; both "hi ..." rows match, newest first.
    assert_eq!(ids, vec!["$e2", "$e1"]);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A CJK query (≥3 scalars) matches CJK substrings via trigram.
#[test]
fn cjk_query_matches_via_trigram() {
    let dir = temp_dir("cjk");
    let conn = open_archive_db(&dir).expect("open");
    ingest_sync(
        &conn,
        &text_event("acctA", "$e1", 100, "こんにちは日本語のテスト"),
    );
    let hits = search(&conn, &q("日本語"), false).expect("search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].event_id, "$e1");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Operator-like query text (`AND`, `*`) is matched literally, not parsed as an FTS
/// operator.
#[test]
fn operator_like_query_is_literal() {
    let dir = temp_dir("operator");
    let conn = open_archive_db(&dir).expect("open");
    ingest_sync(&conn, &text_event("acctA", "$e1", 100, "cats AND dogs"));
    ingest_sync(&conn, &text_event("acctA", "$e2", 200, "cats only"));
    // Literal "AND dogs" appears only in $e1; if AND were an operator this would
    // behave very differently.
    let hits = search(&conn, &q("AND dogs"), false).expect("search");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].event_id, "$e1");
    let _ = std::fs::remove_dir_all(&dir);
}

/// Each filter narrows the result set; empty filter lists are unrestricted; an
/// inverted date range yields an empty result (no error).
#[test]
fn filters_narrow_results() {
    let dir = temp_dir("filters");
    let conn = open_archive_db(&dir).expect("open");
    // Two accounts, two rooms, two senders, spread across timestamps.
    let mut a1 = text_event("acctA", "$a1", 100, "shared keyword one");
    a1.room_id = "!rA:example.org".to_owned();
    a1.sender = "@alice:example.org".to_owned();
    let mut a2 = text_event("acctA", "$a2", 200, "shared keyword two");
    a2.room_id = "!rB:example.org".to_owned();
    a2.sender = "@bob:example.org".to_owned();
    let mut b1 = text_event("acctB", "$b1", 300, "shared keyword three");
    b1.room_id = "!rA:example.org".to_owned();
    b1.sender = "@alice:example.org".to_owned();
    ingest_sync(&conn, &a1);
    ingest_sync(&conn, &a2);
    ingest_sync(&conn, &b1);

    // Unrestricted: all three.
    assert_eq!(search(&conn, &q("keyword"), false).expect("all").len(), 3);

    // Account filter.
    let by_acct = SearchFilter {
        query: "keyword".to_owned(),
        account_ids: vec!["acctA".to_owned()],
        ..Default::default()
    };
    let hits = search(&conn, &by_acct, false).expect("acct");
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|h| h.account_id == "acctA"));

    // Room filter.
    let by_room = SearchFilter {
        query: "keyword".to_owned(),
        room_ids: vec!["!rA:example.org".to_owned()],
        ..Default::default()
    };
    let hits = search(&conn, &by_room, false).expect("room");
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().all(|h| h.room_id == "!rA:example.org"));

    // Sender filter.
    let by_sender = SearchFilter {
        query: "keyword".to_owned(),
        sender: Some("@bob:example.org".to_owned()),
        ..Default::default()
    };
    let hits = search(&conn, &by_sender, false).expect("sender");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].event_id, "$a2");

    // Date range [150, 250] → only $a2 (ts 200).
    let by_range = SearchFilter {
        query: "keyword".to_owned(),
        start_ts: Some(150),
        end_ts: Some(250),
        ..Default::default()
    };
    let hits = search(&conn, &by_range, false).expect("range");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].event_id, "$a2");

    // Inverted range (start > end) → empty, not an error.
    let inverted = SearchFilter {
        query: "keyword".to_owned(),
        start_ts: Some(500),
        end_ts: Some(100),
        ..Default::default()
    };
    assert!(search(&conn, &inverted, false)
        .expect("inverted")
        .is_empty());

    let _ = std::fs::remove_dir_all(&dir);
}

/// Honor-deletions ON excludes a redacted match; OFF includes it. Content stays on
/// disk either way.
#[test]
fn honor_deletions_gates_redacted_match() {
    let dir = temp_dir("honor");
    let conn = open_archive_db(&dir).expect("open");
    ingest_sync(
        &conn,
        &text_event("acctA", "$e1", 100, "secret payload text"),
    );
    mark_redacted(&conn, "acctA", "$e1", 999).expect("mark redacted");

    // OFF → the redacted row is returned (flagged redacted).
    let off = search(&conn, &q("secret"), false).expect("off");
    assert_eq!(off.len(), 1);
    assert!(off[0].redacted);

    // ON → excluded entirely.
    let on = search(&conn, &q("secret"), true).expect("on");
    assert!(on.is_empty());

    // Content still physically on disk regardless of the gate.
    let still: String = conn
        .query_row(
            "SELECT content_json FROM events WHERE account_id='acctA' AND event_id='$e1'",
            [],
            |r| r.get(0),
        )
        .expect("content still present");
    assert!(still.contains("secret payload text"));
    let _ = std::fs::remove_dir_all(&dir);
}

/// A query matching only a prior edit version returns one hit rooted at the chain
/// root (no duplicate for the current version).
#[test]
fn edit_version_match_dedups_to_chain_root() {
    let dir = temp_dir("edit");
    let conn = open_archive_db(&dir).expect("open");
    ingest_sync(
        &conn,
        &text_event("acctA", "$orig", 100, "original wombat text"),
    );
    // The edit removes "wombat" (edited-away), so only the prior version matches it.
    let mut edit = text_event("acctA", "$edit", 200, "edited kangaroo text");
    edit.relates_to_event_id = Some("$orig".to_owned());
    edit.rel_type = Some("m.replace".to_owned());
    ingest_sync(&conn, &edit);

    // "wombat" matches ONLY the prior version — one hit, rooted at $orig.
    let prior = search(&conn, &q("wombat"), false).expect("prior");
    assert_eq!(prior.len(), 1);
    assert_eq!(prior[0].event_id, "$orig");

    // "text" matches BOTH versions but dedups to a single chain-root hit.
    let both = search(&conn, &q("text"), false).expect("both");
    assert_eq!(both.len(), 1);
    assert_eq!(both[0].event_id, "$orig");
    let _ = std::fs::remove_dir_all(&dir);
}

/// An empty-body event is stored but not indexed (nothing to match).
#[test]
fn empty_body_event_is_stored_but_not_indexed() {
    let dir = temp_dir("empty");
    let conn = open_archive_db(&dir).expect("open");
    let mut empty = text_event("acctA", "$img", 100, "");
    empty.content_json = r#"{"msgtype":"m.image","url":"mxc://e.org/x"}"#.to_owned();
    ingest_sync(&conn, &empty);
    // The row is stored...
    let count: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .expect("count events");
    assert_eq!(count, 1);
    // ...but nothing was indexed.
    assert_eq!(fts_count(&conn), 0);
    let _ = std::fs::remove_dir_all(&dir);
}

/// A query with zero hits returns an empty vec, not an error.
#[test]
fn no_match_returns_empty_vec() {
    let dir = temp_dir("nomatch");
    let conn = open_archive_db(&dir).expect("open");
    ingest_sync(&conn, &text_event("acctA", "$e1", 100, "hello world"));
    assert!(search(&conn, &q("zzznotfound"), false)
        .expect("search")
        .is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// A pre-5.1/5.2 `archive.db` with no `body` column or `events_fts` table migrates:
/// the column is added, existing rows are backfilled, the FTS index is built once,
/// old rows become searchable, and re-opening is a no-op.
#[test]
fn pre_5_1_archive_migrates_backfills_and_becomes_searchable() {
    let dir = temp_dir("migrate");
    std::fs::create_dir_all(&dir).expect("mkdir");
    {
        // Hand-build the pre-5.1 (Story 5.1) 9-column schema, no body / no FTS.
        let raw = Connection::open(keeper_core::archive::archive_db_path(&dir)).expect("raw open");
        raw.execute(
            "CREATE TABLE events(\
                account_id TEXT NOT NULL, event_id TEXT NOT NULL, room_id TEXT NOT NULL, \
                sender TEXT NOT NULL, origin_ts INTEGER NOT NULL, event_type TEXT NOT NULL, \
                content_json TEXT NOT NULL, media_json TEXT, inserted_ts INTEGER NOT NULL, \
                PRIMARY KEY(account_id, event_id))",
            [],
        )
        .expect("create old schema");
        raw.execute(
            "INSERT INTO events(account_id, event_id, room_id, sender, origin_ts, event_type, \
             content_json, media_json, inserted_ts) \
             VALUES ('acctA', '$old', '!r1:example.org', '@alice:example.org', 42, \
             'm.room.message', '{\"msgtype\":\"m.text\",\"body\":\"vintage archived note\"}', NULL, 7)",
            [],
        )
        .expect("insert old row");
    }
    // Reopen via the migration path: adds body, backfills, builds FTS.
    let conn = open_archive_db(&dir).expect("open migrates");
    let body: Option<String> = conn
        .query_row(
            "SELECT body FROM events WHERE account_id='acctA' AND event_id='$old'",
            [],
            |r| r.get(0),
        )
        .expect("read backfilled body");
    assert_eq!(body.as_deref(), Some("vintage archived note"));
    assert_eq!(fts_count(&conn), 1, "old row indexed once on rebuild");

    // The old row is now searchable.
    let hits = search(&conn, &q("vintage"), false).expect("search migrated");
    assert_eq!(hits.len(), 1);
    assert_eq!(hits[0].event_id, "$old");
    drop(conn);

    // Re-open: a no-op. FTS is not rebuilt (still exactly one row), still searchable.
    let conn = open_archive_db(&dir).expect("reopen no-op");
    assert_eq!(fts_count(&conn), 1);
    assert_eq!(
        search(&conn, &q("vintage"), false)
            .expect("search after reopen")
            .len(),
        1
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// The command-level honor setting flows into search: enabling the app-wide setting
/// then searching a redacted match excludes it, mirroring the IPC command's read.
#[test]
fn honor_setting_round_trip_affects_search() {
    let dir = temp_dir("setting");
    let conn = open_archive_db(&dir).expect("open");
    ingest_sync(
        &conn,
        &text_event("acctA", "$e1", 100, "confidential note here"),
    );
    mark_redacted(&conn, "acctA", "$e1", 999).expect("mark");
    drop(conn);

    set_honor_remote_deletions(&dir, true).expect("set on");
    let honor = keeper_core::archive::get_honor_remote_deletions(&dir).expect("get");
    assert!(honor);
    let conn = open_readonly_archive_db(&dir).expect("readonly");
    assert!(search(&conn, &q("confidential"), honor)
        .expect("search")
        .is_empty());
    let _ = std::fs::remove_dir_all(&dir);
}

/// Insert + index one event synchronously, exactly as the writer's `insert_event`
/// does (base insert, then index the non-empty body by the returned rowid). Used by
/// the deterministic (non-writer) tests above.
fn ingest_sync(conn: &Connection, ev: &ArchiveEvent) {
    let rowid = keeper_core::archive::db::insert_event(conn, ev, None, 0)
        .expect("insert")
        .expect("row inserted");
    keeper_core::archive::fts::index_body(conn, rowid, &ev.body).expect("index");
}
