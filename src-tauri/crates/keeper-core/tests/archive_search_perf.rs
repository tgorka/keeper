//! CI performance gate for Story 5.3 (NFR-2): offline full-text search must return
//! first results in under 200 ms p95 over a standard query set at 100k+ events.
//!
//! This is a *normal* test (not `#[ignore]`) so CI enforces the gate. It builds a
//! 100k+-event corpus by bulk-inserting directly in one transaction (we are
//! measuring *query* latency, not build throughput — a few seconds to build is
//! acceptable), then times a standard set of queries against a fresh read-only
//! connection and asserts the p95 latency stays under the budget.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use keeper_core::archive::db::{open_archive_db, open_readonly_archive_db};
use keeper_core::archive::{search, SearchFilter};
use rusqlite::Connection;

/// Corpus size — comfortably over the epic's 100k-event threshold.
const CORPUS: i64 = 120_000;

/// The p95 latency budget (NFR-2).
const BUDGET: Duration = Duration::from_millis(200);

fn temp_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "keeper-archive-perf-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    dir
}

/// A deterministic, varied body for row `i` so queries hit a realistic spread of
/// rows (not every row, not none). A handful of "needle" words are sprinkled at a
/// low frequency so the standard queries return bounded, index-served result sets.
fn body_for(i: i64) -> String {
    // A rotating vocabulary keeps trigrams varied; the needles appear ~1/1000.
    let base = match i % 7 {
        0 => "morning standup notes about the release schedule",
        1 => "please review the attached diagram before friday",
        2 => "the quick brown fox jumps over the lazy dog again",
        3 => "let us grab coffee and discuss the roadmap items",
        4 => "deployment finished without any reported errors today",
        5 => "こんにちは チーム 今日 の 進捗 を 共有 します",
        _ => "reminder the meeting moved to the larger room upstairs",
    };
    if i % 1000 == 0 {
        format!("{base} pineapple")
    } else if i % 997 == 0 {
        format!("{base} 日本語")
    } else {
        base.to_owned()
    }
}

/// Bulk-build the corpus in one transaction (fast), then rebuild the FTS index once
/// (external content `'rebuild'` is far faster than 120k incremental inserts and
/// yields the identical index). Returns after a WAL checkpoint so the reader sees
/// everything.
fn build_corpus(dir: &Path) {
    let conn = open_archive_db(dir).expect("open");
    conn.execute_batch("BEGIN").expect("begin");
    {
        let mut stmt = conn
            .prepare(
                "INSERT INTO events(account_id, event_id, room_id, sender, origin_ts, \
                 event_type, content_json, media_json, inserted_ts, relates_to_event_id, \
                 rel_type, body) VALUES (?1, ?2, ?3, ?4, ?5, 'm.room.message', ?6, NULL, 0, \
                 NULL, NULL, ?7)",
            )
            .expect("prepare bulk insert");
        for i in 0..CORPUS {
            let account_id = if i % 3 == 0 { "acctA" } else { "acctB" };
            let room_id = format!("!room{}:example.org", i % 50);
            let sender = format!("@user{}:example.org", i % 200);
            let event_id = format!("$e{i}");
            let body = body_for(i);
            let content_json = format!(r#"{{"msgtype":"m.text","body":"{body}"}}"#);
            stmt.execute(rusqlite::params![
                account_id,
                event_id,
                room_id,
                sender,
                i, // origin_ts
                content_json,
                body,
            ])
            .expect("bulk insert row");
        }
    }
    conn.execute_batch("COMMIT").expect("commit");
    // Populate the FTS index from the bulk-inserted bodies in one pass.
    conn.execute("INSERT INTO events_fts(events_fts) VALUES('rebuild')", [])
        .expect("rebuild fts");
    // Checkpoint the WAL so the fresh read-only connection sees all rows.
    conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")
        .expect("checkpoint");
    drop(conn);
}

/// Time one search, returning its wall-clock latency.
fn time_search(conn: &Connection, filter: &SearchFilter, honor: bool) -> Duration {
    let start = Instant::now();
    let hits = search(conn, filter, honor).expect("search");
    let elapsed = start.elapsed();
    // Touch the result so the query is not optimized away.
    std::hint::black_box(hits.len());
    elapsed
}

#[test]
fn search_p95_under_200ms_at_120k_events() {
    let dir = temp_dir("gate");
    build_corpus(&dir);
    let conn = open_readonly_archive_db(&dir).expect("readonly");

    // Sanity: the corpus really is over the threshold and searchable.
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .expect("count");
    assert!(
        total >= 100_000,
        "corpus must exceed 100k events (got {total})"
    );

    // A standard, varied query set: common trigram words, a rare needle, a CJK
    // query, a filtered query, and a short LIKE-fallback query.
    let queries: Vec<SearchFilter> = vec![
        filter("release"),
        filter("coffee"),
        filter("diagram"),
        filter("pineapple"), // rare needle
        filter("日本語"),    // CJK trigram
        filter("meeting"),
        SearchFilter {
            query: "roadmap".to_owned(),
            account_ids: vec!["acctA".to_owned()],
            ..Default::default()
        },
        SearchFilter {
            query: "errors".to_owned(),
            room_ids: vec!["!room3:example.org".to_owned()],
            ..Default::default()
        },
        filter("of"), // 2-char → LIKE fallback path
    ];

    // Warm up (open pages/caches) so we measure steady-state latency, then collect a
    // sample of repeated runs for a stable p95. Each query is measured on both the
    // honor-OFF and honor-ON paths: honoring adds a per-row root-redaction NOT EXISTS
    // subquery — the most expensive query shape and the one a privacy-conscious user
    // (honor-deletions enabled) actually runs — so the gate must hold for it too.
    for filter in &queries {
        let _ = time_search(&conn, filter, false);
        let _ = time_search(&conn, filter, true);
    }
    let mut samples: Vec<Duration> = Vec::new();
    for _ in 0..10 {
        for filter in &queries {
            samples.push(time_search(&conn, filter, false));
            samples.push(time_search(&conn, filter, true));
        }
    }

    samples.sort();
    let p95_index = ((samples.len() as f64) * 0.95).ceil() as usize - 1;
    let p95 = samples[p95_index.min(samples.len() - 1)];
    let max = *samples.last().expect("samples non-empty");
    assert!(
        p95 < BUDGET,
        "search p95 {p95:?} exceeded the {BUDGET:?} budget (max {max:?}, n={})",
        samples.len()
    );
    let _ = std::fs::remove_dir_all(&dir);
}

fn filter(query: &str) -> SearchFilter {
    SearchFilter {
        query: query.to_owned(),
        ..Default::default()
    }
}
