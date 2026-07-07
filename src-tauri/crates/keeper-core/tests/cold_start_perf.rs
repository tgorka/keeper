//! Cold-start local-store-init budget gate (NFR-1, CI-measurable subset).
//!
//! **This gate measures only the deterministic, offline, Rust-measurable boot
//! slice** — opening a seeded ≥100k-event `archive.db` (WAL) plus the registry
//! reads that run at boot (`list_accounts` + a few `get_setting`s). It is
//! explicitly NOT the PRD's full "cold-start-to-interactive inbox < 2 s" figure:
//! that also spans webview render and lazy per-account SDK activation (which needs
//! the Keychain + network) and is therefore a release-time measurement on
//! reference hardware (see `docs/release.md` / `docs/performance.md`). What this
//! CI gate guards is the one boot cost that scales with archived data — opening a
//! large WAL archive + backfill/FTS-ensure no-op path + registry reads — so a
//! regression that makes local init slow at 100k+ events fails the build.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use keeper_core::archive::db::open_archive_db;
use keeper_core::registry::{get_setting, insert_account, list_accounts, set_setting};

/// Corpus size — comfortably over the epic's 100k-event threshold (matches the
/// FTS gate's shape).
const CORPUS: i64 = 100_000;

/// The local-init budget (NFR-1 subset). Opening a 100k-event WAL `archive.db`
/// (idempotent schema + migration + FTS-exists no-op) plus a handful of registry
/// reads measures in the low tens of milliseconds locally on `macos-latest`
/// (Apple Silicon) — a measured baseline of ~15-20 ms. The budget is set to a
/// defensible ceiling with a wide margin (~25x headroom over baseline) so only a
/// real regression (archive-open going slow at 100k+ scale) trips it, never normal
/// CI-runner variance or a cold page cache on first open.
const LOCAL_INIT_BUDGET: Duration = Duration::from_millis(500);

fn temp_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "keeper-cold-start-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    dir
}

/// Removes the seeded temp dir on scope exit — including on an assertion unwind —
/// so a failing gate never leaks its 100k-event archive in the system temp dir.
struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A deterministic, varied body for row `i` (mirrors the FTS perf gate so the
/// seeded archive is realistic).
fn body_for(i: i64) -> String {
    let base = match i % 5 {
        0 => "morning standup notes about the release schedule",
        1 => "please review the attached diagram before friday",
        2 => "the quick brown fox jumps over the lazy dog again",
        3 => "deployment finished without any reported errors today",
        _ => "reminder the meeting moved to the larger room upstairs",
    };
    base.to_owned()
}

/// Bulk-build the corpus in one transaction (fast), rebuild the FTS index once
/// (external content `'rebuild'`), then checkpoint the WAL — reusing the pattern
/// from `archive_search_perf.rs`. We are seeding, not measuring build throughput.
fn seed_archive(dir: &Path) {
    let conn = open_archive_db(dir).expect("open archive.db for seed");
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
                i,
                content_json,
                body,
            ])
            .expect("bulk insert row");
        }
    }
    conn.execute_batch("COMMIT").expect("commit");
    conn.execute("INSERT INTO events_fts(events_fts) VALUES('rebuild')", [])
        .expect("rebuild fts");
    conn.pragma_update(None, "wal_checkpoint", "TRUNCATE")
        .expect("checkpoint");
    drop(conn);
}

/// Seed the registry with a few accounts + settings so the boot-time reads
/// (`list_accounts`, `get_setting`) have real rows to return.
fn seed_registry(dir: &Path) {
    for acc in 0..5 {
        let account_id = format!("acct{acc}");
        insert_account(
            dir,
            &account_id,
            &format!("@user{acc}:example.org"),
            "https://matrix.example.org",
            &format!("DEVICE{acc}"),
            acc as i64,
            (acc % 8) as u8,
            "password",
        )
        .expect("seed account");
    }
    set_setting(dir, "honor_remote_deletions", "false").expect("seed setting");
    set_setting(dir, "theme", "system").expect("seed setting");
}

/// Cold-start local-init slice on a seeded ≥100k-event archive + a seeded
/// registry: `open_archive_db` + the boot registry reads must complete under
/// `LOCAL_INIT_BUDGET`. (I/O-Matrix row: "Cold-start local init at 100k+".)
#[test]
fn local_init_under_budget_at_100k_events() {
    let dir = temp_dir("gate");
    let _guard = TempDirGuard(dir.clone());
    seed_archive(&dir);
    seed_registry(&dir);

    // Warm-up: prime the OS page cache and any one-time open costs with a single
    // untimed open, so the timed run below measures steady-state archive-open (the
    // thing a regression would slow), not a single unlucky cold-cache first touch
    // on a noisy hosted CI runner. This keeps the budget assertion from flaking.
    {
        let warm = open_archive_db(&dir).expect("warm-up open archive.db");
        drop(warm);
    }

    // Time the boot slice: open the (large, WAL) archive + the registry reads a
    // cold boot performs. Everything here is offline and deterministic.
    let start = Instant::now();
    let conn = open_archive_db(&dir).expect("cold open archive.db");
    let accounts = list_accounts(&dir).expect("cold list_accounts");
    let honor = get_setting(&dir, "honor_remote_deletions").expect("cold get_setting");
    let theme = get_setting(&dir, "theme").expect("cold get_setting");
    let elapsed = start.elapsed();

    // Touch the results so nothing is optimized away.
    std::hint::black_box((&conn, &accounts, &honor, &theme));

    // Sanity: the corpus really is over the threshold and the registry seeded.
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .expect("count events");
    assert!(
        total >= 100_000,
        "seeded corpus must exceed 100k events (got {total})"
    );
    assert_eq!(accounts.len(), 5, "registry must have the seeded accounts");
    assert_eq!(honor.as_deref(), Some("false"));

    assert!(
        elapsed < LOCAL_INIT_BUDGET,
        "cold-start local init {elapsed:?} exceeded the {LOCAL_INIT_BUDGET:?} budget \
         (archive open + registry reads at {total} events)"
    );

    drop(conn);
    let _ = std::fs::remove_dir_all(&dir);
}
