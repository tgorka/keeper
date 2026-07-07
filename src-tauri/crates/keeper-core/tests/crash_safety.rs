//! Crash-safety durability gate (NFR-8): a **real OS process kill** (SIGKILL)
//! of a child that is *actively writing* must never lose a previously committed
//! row, and every DB must reopen with `PRAGMA integrity_check` == `ok`.
//!
//! This covers the three local write paths through their public API:
//! - archive ingest (`insert_event` + `fts::index_body`) into `archive.db`,
//! - outbox insert (`insert_outbox`) into `keeper.db`,
//! - settings write (`set_setting`) into `keeper.db`.
//!
//! **Why a subprocess, not an in-process `drop`.** A real crash is an OS kill,
//! not a graceful teardown: a `drop` runs SQLite's normal shutdown (a clean
//! checkpoint), so it never exercises recovery from an unclean `-wal`. A SIGKILL
//! leaves the `-wal` unclean and forces the re-opener to recover it. **Scope
//! (NFR-8): this proves a killed *process* loses no previously-committed row — not
//! power-loss/fsync durability.** A process kill leaves already-written WAL frames
//! and the OS page cache intact, so a re-opener on the same machine sees every
//! committed frame regardless of the `synchronous` fsync level; simulating real
//! power loss would need a barrier-dropping/OS-crash harness, which is out of
//! scope here. Mechanism: the parent re-invokes its own test binary
//! (`std::env::current_exe()`) to run a dedicated *child* test that writes in a
//! tight loop, printing + flushing `committed <id>` after each committed row. The
//! parent reads those ids from the child's piped stdout, then `child.kill()`s it
//! (SIGKILL on Unix, no `unsafe`, no new dependency) while writes are still in
//! flight, reopens each DB, and asserts every reported id survived. The "torn
//! final write" invariant — a not-yet-committed row may be absent, but no
//! previously-committed row is lost — is exercised by killing mid loop and
//! asserting only on the ids the child actually reported as committed.
//!
//! Each child entry-point test returns IMMEDIATELY when `KEEPER_CRASH_CHILD` is
//! unset, so a normal `cargo nextest run` runs them as trivial no-ops and never
//! recurses / never hangs. Only when the parent sets `KEEPER_CRASH_CHILD=<dir>`
//! does a child open the real DB in `<dir>` and write until it is killed.

use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::mpsc;
use std::time::Duration;

use keeper_core::archive::db::{insert_event, open_archive_db};
use keeper_core::archive::{fts, ArchiveEvent};
use keeper_core::registry::{get_setting, insert_outbox, list_outbox_rows, set_setting};
use rusqlite::Connection;

/// The env var that switches a child entry-point test from "no-op" to "write
/// until killed". Its value is the data dir holding the DBs under test.
const CRASH_CHILD_ENV: &str = "KEEPER_CRASH_CHILD";

/// How many committed ids the parent collects from the child before SIGKILLing
/// it. Kept modest so the kill lands promptly while writes are still in flight.
const KILL_AFTER: usize = 200;

/// A unique temp data dir per test run (no `tempfile` dev-dep — mirrors the
/// existing perf/durability tests' `std::env::temp_dir()` + unique subdir).
fn temp_dir(tag: &str) -> PathBuf {
    let mut dir = std::env::temp_dir();
    dir.push(format!(
        "keeper-crash-safety-{tag}-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0)
    ));
    dir
}

/// Removes a temp data dir on scope exit — including on an assertion unwind — so a
/// failing gate never leaks its seeded DBs in the system temp dir.
struct TempDirGuard(PathBuf);

impl Drop for TempDirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A plain text event for row `i` (mirrors the `text_event` helper used by the
/// other archive integration tests).
fn text_event(i: i64) -> ArchiveEvent {
    let body = format!("crash safety durability row {i}");
    ArchiveEvent {
        account_id: "acctA".to_owned(),
        event_id: format!("$crash{i}"),
        room_id: "!room:example.org".to_owned(),
        sender: "@bob:example.org".to_owned(),
        origin_ts: i,
        event_type: "m.room.message".to_owned(),
        content_json: format!(r#"{{"msgtype":"m.text","body":"{body}"}}"#),
        body,
        media: None,
        relates_to_event_id: None,
        rel_type: None,
    }
}

/// Spawn this test binary to run one child entry-point test as a writing child,
/// read its committed ids from stdout until `KILL_AFTER` are seen, SIGKILL it,
/// and return the collected ids. The child keeps writing until the kill lands.
fn run_child_until_killed(child_test: &str, dir: &Path) -> Vec<String> {
    let exe = std::env::current_exe().expect("locate current test exe");
    let mut child = Command::new(&exe)
        // libtest accepts `--exact <name> --nocapture` when the binary is run
        // directly; `--exact` pins the single child test so nothing else runs.
        .args(["--exact", child_test, "--nocapture"])
        .env(CRASH_CHILD_ENV, dir)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("spawn writing child");

    // Drain the child's stdout on a helper thread, forwarding each committed id
    // over a channel. Reading on a thread lets the parent apply a wall-clock
    // deadline (`recv_timeout`) so a wedged child can never hang the gate
    // indefinitely — it turns a hypothetical infinite hang into a bounded,
    // diagnosable failure (this is a *reliability* gate; it must not itself hang).
    let stdout = child.stdout.take().expect("child stdout piped");
    let (tx, rx) = mpsc::channel::<String>();
    let reader = std::thread::spawn(move || {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            // The pipe closes when we kill the child — stop reading.
            let Ok(line) = line else { break };
            if let Some(id) = line.strip_prefix("committed ") {
                if tx.send(id.trim().to_owned()).is_err() {
                    break; // parent stopped listening
                }
            }
        }
    });

    // Collect until we have enough committed ids or the child stalls/exits. A
    // healthy child streams to `KILL_AFTER` in milliseconds (each write resets the
    // per-message deadline), so only a genuinely stuck child hits the timeout.
    let deadline = Duration::from_secs(30);
    let mut committed: Vec<String> = Vec::new();
    while committed.len() < KILL_AFTER {
        match rx.recv_timeout(deadline) {
            Ok(id) => committed.push(id),
            Err(_) => break, // timeout, or the reader finished (pipe closed)
        }
    }

    // SIGKILL while the child is still writing: `std::process::Child::kill` sends
    // SIGKILL on Unix (safe, no `unsafe`, no new dependency). Be lenient — if the
    // child already exited on its own (e.g. it panicked) `kill()` can error; the
    // child's exit status is surfaced in the assertion below instead.
    let _ = child.kill();
    let status = child.wait();
    let _ = reader.join();

    assert!(
        committed.len() >= KILL_AFTER,
        "child `{child_test}` produced only {} committed ids before stalling or exiting \
         (child exit status: {status:?}); expected >= {KILL_AFTER}. A short count means the \
         writer failed to spawn/write — investigate the child, not durability.",
        committed.len()
    );
    committed
}

/// Whether this process is running as a writing child (the env var is set to the
/// data dir); `None` for a normal parent-side run.
fn crash_child_dir() -> Option<PathBuf> {
    std::env::var_os(CRASH_CHILD_ENV).map(PathBuf::from)
}

/// Assert `PRAGMA integrity_check` returns exactly `ok` on a reopened DB.
fn assert_integrity_ok(conn: &Connection, what: &str) {
    let result: String = conn
        .query_row("PRAGMA integrity_check", [], |r| r.get(0))
        .unwrap_or_else(|e| panic!("integrity_check failed to run for {what}: {e}"));
    assert_eq!(result, "ok", "{what} integrity_check must be ok");
}

// ---------------------------------------------------------------------------
// Child entry-points. Each is a no-op unless KEEPER_CRASH_CHILD is set. When set,
// it opens the real DB and writes committed rows in an infinite loop, printing +
// flushing `committed <id>` after each commit, until the parent SIGKILLs it.
// ---------------------------------------------------------------------------

/// Archive-ingest writing child: inserts events into `archive.db` and indexes
/// each body into the FTS index exactly as the writer does, printing the
/// committed `event_id` after each insert.
#[test]
fn crash_child_archive() {
    let Some(dir) = crash_child_dir() else {
        return; // normal run: trivial no-op, never recurses.
    };
    let conn = open_archive_db(&dir).expect("child: open archive.db");
    let stdout = std::io::stdout();
    let mut i: i64 = 0;
    loop {
        let ev = text_event(i);
        if let Some(rowid) = insert_event(&conn, &ev, None, i).expect("child: insert event") {
            // Index the body so FTS consistency holds (mirrors the writer).
            fts::index_body(&conn, rowid, &ev.body).expect("child: index body");
            let mut lock = stdout.lock();
            let _ = writeln!(lock, "committed {}", ev.event_id);
            let _ = lock.flush();
        }
        i += 1;
    }
}

/// Outbox-insert writing child: inserts held-send rows into `keeper.db`, printing
/// each committed row `id`.
#[test]
fn crash_child_outbox() {
    let Some(dir) = crash_child_dir() else {
        return;
    };
    let stdout = std::io::stdout();
    let mut i: i64 = 0;
    loop {
        let id = format!("outbox-{i}");
        insert_outbox(&dir, &id, "acctA", "!room:example.org", "body", i, i + 1000)
            .expect("child: insert outbox row");
        let mut lock = stdout.lock();
        let _ = writeln!(lock, "committed {id}");
        let _ = lock.flush();
        i += 1;
    }
}

/// Settings-write writing child: upserts settings keys into `keeper.db`, printing
/// each committed key.
#[test]
fn crash_child_settings() {
    let Some(dir) = crash_child_dir() else {
        return;
    };
    let stdout = std::io::stdout();
    let mut i: i64 = 0;
    loop {
        let key = format!("crash-key-{i}");
        set_setting(&dir, &key, &format!("value-{i}")).expect("child: set setting");
        let mut lock = stdout.lock();
        let _ = writeln!(lock, "committed {key}");
        let _ = lock.flush();
        i += 1;
    }
}

// ---------------------------------------------------------------------------
// Parent tests. Each spawns its child, kills it mid-write, reopens the DB, and
// asserts zero loss of previously-committed rows + integrity_check == ok.
// ---------------------------------------------------------------------------

/// Archive ingest killed mid-batch: every reported `event_id` survives, the DB
/// passes `integrity_check`, and the FTS index stays consistent with the indexed
/// bodies (no orphaned/missing index rows). (I/O-Matrix rows 1 + 4.)
#[test]
fn archive_ingest_survives_kill() {
    // A parent-side guard: if this test ever runs with the child env set (it must
    // not — the child tests own that mode), do nothing rather than write real data.
    if crash_child_dir().is_some() {
        return;
    }
    let dir = temp_dir("archive");
    let _guard = TempDirGuard(dir.clone());
    let committed = run_child_until_killed("crash_child_archive", &dir);

    // Reopen in the parent and assert every committed event survived.
    let conn = open_archive_db(&dir).expect("reopen archive.db after kill");
    for event_id in &committed {
        let present: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM events WHERE event_id = ?1",
                rusqlite::params![event_id],
                |r| r.get(0),
            )
            .expect("count event");
        assert_eq!(present, 1, "committed event {event_id} lost after SIGKILL");
    }

    // `integrity_check` == ok on reopen.
    assert_integrity_ok(&conn, "archive.db");

    // FTS consistency: every non-empty indexed body maps to exactly one docsize
    // row, and no docsize id is orphaned. Each committed event has a non-empty
    // body, so the count of indexed docs must equal the count of committed rows
    // that carry a non-empty body (all of them), with zero orphans.
    let orphans: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM events_fts_docsize d \
             WHERE NOT EXISTS (SELECT 1 FROM events e WHERE e.rowid = d.id)",
            [],
            |r| r.get(0),
        )
        .expect("count fts orphans");
    assert_eq!(orphans, 0, "no orphaned FTS docsize rows after kill");
    // The FTS index passes its own internal integrity check.
    conn.execute(
        "INSERT INTO events_fts(events_fts) VALUES('integrity-check')",
        [],
    )
    .expect("fts integrity-check passes");
    // Every committed body is searchable (index not missing rows). The child
    // prints `committed <id>` only after BOTH the base insert AND its paired
    // `index_body` have committed, so every reported id must be indexed: no
    // committed row lost its FTS entry.
    let indexed_docs: i64 = conn
        .query_row("SELECT COUNT(*) FROM events_fts_docsize", [], |r| r.get(0))
        .expect("count indexed docs");
    let stored_rows: i64 = conn
        .query_row("SELECT COUNT(*) FROM events", [], |r| r.get(0))
        .expect("count events");
    assert!(
        indexed_docs >= committed.len() as i64,
        "FTS index missing committed rows: indexed {indexed_docs} < committed {}",
        committed.len()
    );
    // The torn-final-write window: the base `INSERT OR IGNORE` and the paired
    // `index_body` commit separately, so a SIGKILL between them can leave exactly
    // ONE stored row whose FTS entry never committed (its `committed` line was
    // never printed, so it is not in the committed set). That single un-indexed
    // torn row is the only allowed divergence — the index is never AHEAD of stored
    // rows, and never behind by more than that one in-flight row. The FTS
    // `integrity-check` above already proved no orphaned/corrupt index entries.
    assert!(
        stored_rows >= indexed_docs && stored_rows - indexed_docs <= 1,
        "FTS index/doc count desynced beyond one torn final write after kill \
         (stored {stored_rows}, indexed {indexed_docs})"
    );

    let _ = std::fs::remove_dir_all(&dir);
}

/// Outbox insert killed mid-loop: every reported id is present via
/// `list_outbox_rows`, and `keeper.db` passes `integrity_check`. (I/O-Matrix rows
/// 2 + 4.)
#[test]
fn outbox_survives_kill() {
    if crash_child_dir().is_some() {
        return;
    }
    let dir = temp_dir("outbox");
    let _guard = TempDirGuard(dir.clone());
    let committed = run_child_until_killed("crash_child_outbox", &dir);

    let rows = list_outbox_rows(&dir).expect("list outbox rows after kill");
    let present: std::collections::HashSet<&str> = rows.iter().map(|r| r.id.as_str()).collect();
    for id in &committed {
        assert!(
            present.contains(id.as_str()),
            "committed outbox row {id} lost after SIGKILL"
        );
    }

    let conn = Connection::open(dir.join("keeper.db")).expect("reopen keeper.db");
    assert_integrity_ok(&conn, "keeper.db (outbox)");

    let _ = std::fs::remove_dir_all(&dir);
}

/// Settings write killed mid-loop: every reported key is readable via
/// `get_setting`, and `keeper.db` passes `integrity_check`. (I/O-Matrix rows 3 +
/// 4.)
#[test]
fn settings_survive_kill() {
    if crash_child_dir().is_some() {
        return;
    }
    let dir = temp_dir("settings");
    let _guard = TempDirGuard(dir.clone());
    let committed = run_child_until_killed("crash_child_settings", &dir);

    for key in &committed {
        let value = get_setting(&dir, key).expect("read setting after kill");
        assert!(
            value.is_some(),
            "committed setting {key} lost after SIGKILL"
        );
    }

    let conn = Connection::open(dir.join("keeper.db")).expect("reopen keeper.db");
    assert_integrity_ok(&conn, "keeper.db (settings)");

    let _ = std::fs::remove_dir_all(&dir);
}
