//! 123.3-AC1, AC11, AC12: process tests for `keeper-cli debug app-start`.

#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(target_os = "linux")]

use std::fs;
use std::io::{BufRead as _, BufReader, Read as _};
use std::os::unix::fs::{FileTypeExt as _, PermissionsExt as _};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

use keeper_wire_api::DataLock;
use nix::errno::Errno;
use nix::sys::inotify::{AddWatchFlags, InitFlags, Inotify, WatchDescriptor};
use rusqlite::{Connection, OpenFlags};
use serde_json::{json, Value};

const DEFAULT_EXIT_TIMEOUT: Duration = Duration::from_secs(15);

fn fresh_data_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).expect("chmod 0700");
    dir
}

fn app_start_cmd(
    data: &Path,
    dbs: &[&str],
    extra_env: &[(&str, &str)],
    wait_ms: Option<u64>,
    for_ms: u64,
) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_keeper-cli"));
    cmd.args(["--json", "debug", "app-start", "--data"])
        .arg(data);
    for db in dbs {
        cmd.args(["--database", db]);
    }
    if let Some(wait) = wait_ms {
        cmd.args(["--wait-ms", &wait.to_string()]);
    }
    cmd.args(["--for-ms", &for_ms.to_string()]);
    cmd.env_remove("KEEPER_WIRE_DEBUG_ON")
        .env_remove("KEEPER_TEST_LOCK_UNSUPPORTED");
    for (k, v) in extra_env {
        cmd.env(k, v);
    }
    cmd
}

fn spawn_output_readers(child: &mut Child) -> (JoinHandle<Vec<String>>, JoinHandle<String>) {
    let stdout = child.stdout.take().expect("child stdout piped");
    let stderr = child.stderr.take().expect("child stderr piped");
    let out_handle = std::thread::spawn(move || {
        BufReader::new(stdout)
            .lines()
            .map(|l| l.expect("read stdout line"))
            .collect()
    });
    let err_handle = std::thread::spawn(move || {
        let mut buf = String::new();
        let mut reader = BufReader::new(stderr);
        reader.read_to_string(&mut buf).expect("read stderr");
        buf
    });
    (out_handle, err_handle)
}

fn wait_exit(child: &mut Child, timeout: Duration) -> ExitStatus {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().expect("try_wait child") {
            return status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("timed out waiting for child pid {} to exit", child.id());
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn start_with_wait(
    data: &Path,
    dbs: &[&str],
    extra_env: &[(&str, &str)],
    wait_ms: Option<u64>,
    for_ms: u64,
    timeout: Duration,
) -> (ExitStatus, Vec<String>, String) {
    let mut child = app_start_cmd(data, dbs, extra_env, wait_ms, for_ms)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn keeper-cli debug app-start");
    let (out_handle, err_handle) = spawn_output_readers(&mut child);
    let status = wait_exit(&mut child, timeout);
    let stdout = out_handle.join().expect("join stdout reader");
    let stderr = err_handle.join().expect("join stderr reader");
    (status, stdout, stderr)
}

fn start(
    data: &Path,
    dbs: &[&str],
    extra_env: &[(&str, &str)],
    for_ms: u64,
) -> (ExitStatus, Vec<String>, String) {
    start_with_wait(data, dbs, extra_env, None, for_ms, DEFAULT_EXIT_TIMEOUT)
}

fn write_setting(data: &Path, key: &str, value: &str) {
    let conn = Connection::open(data.join("keeper.db")).expect("open keeper.db");
    conn.execute(
        "CREATE TABLE IF NOT EXISTS settings(key TEXT PRIMARY KEY, value TEXT NOT NULL)",
        [],
    )
    .expect("create settings table");
    conn.execute(
        "INSERT INTO settings(key, value) VALUES (?1, ?2) \
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        [key, value],
    )
    .expect("upsert setting");
}

fn all_unit_scopes(data: &Path, db: &str) -> Vec<String> {
    let path = data.join("hq").join(format!("{db}.sqlite"));
    if !path.exists() {
        return Vec::new();
    }
    let conn = Connection::open_with_flags(&path, OpenFlags::SQLITE_OPEN_READ_ONLY)
        .expect("open replica sqlite read-only");
    let has_table: bool = conn
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM sqlite_master WHERE type = 'table' AND name = '_syncular_units')",
            [],
            |row| row.get(0),
        )
        .expect("check _syncular_units existence");
    if !has_table {
        return Vec::new();
    }
    let mut stmt = conn
        .prepare("SELECT scopes FROM _syncular_units")
        .expect("prepare _syncular_units query");
    stmt.query_map([], |row| row.get::<_, String>(0))
        .expect("query _syncular_units")
        .map(|r| r.expect("read scopes column"))
        .collect()
}

fn kg_rows(data: &Path, db: &str) -> Vec<String> {
    all_unit_scopes(data, db)
        .into_iter()
        .filter(|s| s.contains("\"kg_"))
        .collect()
}

fn has_any_socket(root: &Path) -> bool {
    let Ok(entries) = fs::read_dir(root) else {
        return false;
    };
    for entry in entries {
        let entry = entry.expect("dir entry");
        let path = entry.path();
        let meta = fs::symlink_metadata(&path).expect("symlink_metadata");
        let ft = meta.file_type();
        if ft.is_socket() {
            return true;
        }
        if ft.is_dir() && has_any_socket(&path) {
            return true;
        }
    }
    false
}

fn drain_inotify(inotify: &Inotify) -> Vec<(WatchDescriptor, Option<String>)> {
    let mut out = Vec::new();
    loop {
        match inotify.read_events() {
            Ok(events) => {
                if events.is_empty() {
                    break;
                }
                for ev in events {
                    let name = ev.name.map(|n| n.to_string_lossy().into_owned());
                    out.push((ev.wd, name));
                }
            }
            Err(Errno::EAGAIN) => break,
            Err(e) => panic!("inotify read_events failed: {e}"),
        }
    }
    out
}

#[test]
fn ac1_fresh_directory_has_no_socket() {
    let dir = fresh_data_dir();
    let wire_dir = dir.path().join("wire");
    assert!(!wire_dir.exists(), "wire/ must not exist before start");

    let (status, stdout, stderr) = start(dir.path(), &["a", "b"], &[], 800);
    assert!(
        status.success(),
        "expected exit 0, got {status}; stdout={stdout:?}; stderr={stderr:?}"
    );
    assert!(!wire_dir.exists(), "wire/ must not exist after start");
    assert!(
        !has_any_socket(dir.path()),
        "found unexpected socket under {}",
        dir.path().display()
    );
}

#[test]
fn ac11_off_or_unlisted_subscribes_nothing() {
    // (a) wire.enabled=0, wire.domains=a -> subscribes nothing.
    let dir_off = fresh_data_dir();
    write_setting(dir_off.path(), "wire.enabled", "0");
    write_setting(dir_off.path(), "wire.domains", "a");

    let (status, stdout, stderr) = start(dir_off.path(), &["a", "b"], &[], 800);
    assert!(
        status.success(),
        "expected exit 0 (off), got {status}; stdout={stdout:?}; stderr={stderr:?}"
    );
    assert!(kg_rows(dir_off.path(), "a").is_empty());
    assert!(kg_rows(dir_off.path(), "b").is_empty());

    // (b) wire.enabled=1, no wire.domains -> serves socket, subscribes nothing.
    let dir_on = fresh_data_dir();
    write_setting(dir_on.path(), "wire.enabled", "1");
    let sock = dir_on.path().join("wire").join("wire.sock");

    let mut child = app_start_cmd(dir_on.path(), &["a", "b"], &[], None, 2500)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn keeper-cli debug app-start");
    let (out_handle, err_handle) = spawn_output_readers(&mut child);

    let poll_deadline = Instant::now() + Duration::from_secs(3);
    let mut saw_socket = false;
    while Instant::now() < poll_deadline {
        if fs::symlink_metadata(&sock)
            .map(|m| m.file_type().is_socket())
            .unwrap_or(false)
        {
            saw_socket = true;
            break;
        }
        if child.try_wait().expect("try_wait child").is_some() {
            break;
        }
        std::thread::sleep(Duration::from_millis(25));
    }

    let status = wait_exit(&mut child, DEFAULT_EXIT_TIMEOUT);
    let stdout = out_handle.join().expect("join stdout reader");
    let stderr = err_handle.join().expect("join stderr reader");
    assert!(
        status.success(),
        "expected exit 0 (on, unlisted), got {status}; stdout={stdout:?}; stderr={stderr:?}"
    );
    assert!(
        saw_socket,
        "expected {} to exist while running; stdout={stdout:?}; stderr={stderr:?}",
        sock.display()
    );
    assert!(kg_rows(dir_on.path(), "a").is_empty());
    assert!(kg_rows(dir_on.path(), "b").is_empty());
}

#[test]
fn ac11_listing_one_subscribes_it_alone_once() {
    let dir = fresh_data_dir();
    write_setting(dir.path(), "wire.enabled", "1");
    write_setting(dir.path(), "wire.domains", "a");

    let (status, stdout, stderr) = start(dir.path(), &["a", "b"], &[], 2500);
    assert!(
        status.success(),
        "expected exit 0 on first start, got {status}; stdout={stdout:?}; stderr={stderr:?}"
    );

    let a_kg = kg_rows(dir.path(), "a");
    assert_eq!(a_kg.len(), 4, "expected 4 kg_ units in a, got: {a_kg:?}");
    for kind in ["kg_graph", "kg_history", "kg_client", "kg_proposal"] {
        assert!(
            a_kg.iter().any(|s| s.contains(kind)),
            "expected {kind} in a's _syncular_units, got: {a_kg:?}"
        );
    }
    assert!(
        kg_rows(dir.path(), "b").is_empty(),
        "expected no kg_ units in b"
    );
    let saw_followed = stdout.iter().any(|line| {
        line.contains(r#""followed":["a"]"#)
            || serde_json::from_str::<Value>(line)
                .ok()
                .is_some_and(|v| v.get("followed") == Some(&json!(["a"])))
    });
    assert!(
        saw_followed,
        "expected stdout line with followed:[\"a\"], got: {stdout:?}"
    );

    let total_units_before = all_unit_scopes(dir.path(), "a").len();

    // Second start must be idempotent.
    let (status2, stdout2, stderr2) = start(dir.path(), &["a", "b"], &[], 2500);
    assert!(
        status2.success(),
        "expected exit 0 on second start, got {status2}; stdout={stdout2:?}; stderr={stderr2:?}"
    );
    assert_eq!(
        kg_rows(dir.path(), "a").len(),
        4,
        "second start must not duplicate kg_ units"
    );
    assert_eq!(
        all_unit_scopes(dir.path(), "a").len(),
        total_units_before,
        "total _syncular_units rows in a must stay unchanged"
    );
    assert!(kg_rows(dir.path(), "b").is_empty());
}

#[test]
fn ac12_second_process_opens_nothing_and_names_the_holder() {
    let dir = fresh_data_dir();
    write_setting(dir.path(), "wire.enabled", "0");

    // First normal start populates keeper.db and hq/a.sqlite.
    let (init_status, init_out, init_err) = start(dir.path(), &["a", "b"], &[], 500);
    assert!(
        init_status.success(),
        "initial start failed: {init_status}; stdout={init_out:?}; stderr={init_err:?}"
    );
    assert!(dir.path().join("keeper.db").exists());
    assert!(dir.path().join("hq").join("a.sqlite").exists());

    let holder_text = "the first keeper, pid 1";
    let lock = DataLock::acquire(dir.path(), holder_text).expect("acquire DataLock");

    let hq_dir = dir.path().join("hq");
    let inotify = Inotify::init(InitFlags::IN_NONBLOCK).expect("inotify init");
    let wd_data = inotify
        .add_watch(dir.path(), AddWatchFlags::IN_OPEN)
        .expect("watch data dir");
    let wd_hq = inotify
        .add_watch(&hq_dir, AddWatchFlags::IN_OPEN)
        .expect("watch hq dir");
    let _ = drain_inotify(&inotify);

    let started = Instant::now();
    let (status, stdout, stderr) = start_with_wait(
        dir.path(),
        &["a", "b"],
        &[],
        Some(600),
        500,
        Duration::from_secs(5),
    );
    let elapsed = started.elapsed();

    assert_eq!(
        status.code(),
        Some(3),
        "expected exit 3 when locked, got {status}; stdout={stdout:?}; stderr={stderr:?}"
    );
    assert!(
        elapsed >= Duration::from_millis(500),
        "expected retry wait >= 500ms, elapsed={elapsed:?}"
    );
    let joined_stdout = stdout.join("\n");
    assert!(
        joined_stdout.contains("data_dir_locked"),
        "expected data_dir_locked in stdout, got: {stdout:?}"
    );
    assert!(
        joined_stdout.contains(holder_text),
        "expected holder text in stdout, got: {stdout:?}"
    );

    let locked_events = drain_inotify(&inotify);
    assert!(
        !locked_events
            .iter()
            .any(|(_, name)| name.as_deref() == Some("keeper.db")),
        "locked start must not open keeper.db; events={locked_events:?}"
    );
    assert!(
        !locked_events.iter().any(|(wd, _)| *wd == wd_hq),
        "locked start must not open anything under hq/; events={locked_events:?}"
    );

    // Positive control: once unlocked, a normal start triggers IN_OPEN on keeper.db.
    drop(lock);
    let _ = drain_inotify(&inotify);

    let (ok_status, ok_out, ok_err) = start(dir.path(), &["a", "b"], &[], 500);
    assert!(
        ok_status.success(),
        "unlocked start failed: {ok_status}; stdout={ok_out:?}; stderr={ok_err:?}"
    );
    let unlocked_events = drain_inotify(&inotify);
    assert!(
        unlocked_events
            .iter()
            .any(|(wd, name)| *wd == wd_data && name.as_deref() == Some("keeper.db")),
        "positive control expected IN_OPEN on keeper.db; events={unlocked_events:?}"
    );
}

#[test]
fn ac12_unsupported_lock_opens_data_serves_nothing_one_warn() {
    let dir = fresh_data_dir();
    write_setting(dir.path(), "wire.enabled", "1");
    let sock = dir.path().join("wire").join("wire.sock");

    let mut child = app_start_cmd(
        dir.path(),
        &["a", "b"],
        &[("KEEPER_TEST_LOCK_UNSUPPORTED", "1")],
        None,
        1500,
    )
    .stdout(Stdio::piped())
    .stderr(Stdio::piped())
    .spawn()
    .expect("spawn keeper-cli debug app-start");
    let (out_handle, err_handle) = spawn_output_readers(&mut child);

    let deadline = Instant::now() + DEFAULT_EXIT_TIMEOUT;
    let status = loop {
        assert!(
            !sock.exists(),
            "wire.sock must never exist when lock is unsupported"
        );
        if let Some(status) = child.try_wait().expect("try_wait child") {
            break status;
        }
        if Instant::now() >= deadline {
            let _ = child.kill();
            let _ = child.wait();
            panic!("timed out waiting for child pid {} to exit", child.id());
        }
        std::thread::sleep(Duration::from_millis(25));
    };

    let stdout = out_handle.join().expect("join stdout reader");
    let stderr = err_handle.join().expect("join stderr reader");
    assert!(
        status.success(),
        "expected exit 0, got {status}; stdout={stdout:?}; stderr={stderr:?}"
    );
    let joined_stdout = stdout.join("\n");
    assert!(
        joined_stdout.contains(r#""lock":"unsupported""#),
        "expected \"lock\":\"unsupported\" in stdout, got: {stdout:?}"
    );
    assert!(
        dir.path().join("keeper.db").exists(),
        "keeper.db must exist"
    );
    assert!(
        dir.path().join("hq").join("a.sqlite").exists(),
        "hq/a.sqlite must exist"
    );
    assert!(!sock.exists(), "wire.sock must not exist after exit");

    let warn_lines: Vec<&str> = stderr
        .lines()
        .filter(|line| line.contains("lock_unsupported"))
        .collect();
    assert_eq!(
        warn_lines.len(),
        1,
        "expected exactly one lock_unsupported line in stderr, got: {stderr:?}"
    );
    assert!(
        warn_lines[0].contains("WARN"),
        "expected WARN level on lock_unsupported line, got: {:?}",
        warn_lines[0]
    );
}

#[test]
fn ac12_unsafe_directory_is_refused_before_the_lock() {
    let dir = fresh_data_dir();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o777)).expect("chmod 0777");

    let (status, stdout, stderr) = start_with_wait(
        dir.path(),
        &["a", "b"],
        &[],
        Some(1000),
        500,
        Duration::from_secs(5),
    );
    assert_eq!(
        status.code(),
        Some(3),
        "expected exit 3 for unsafe dir, got {status}; stdout={stdout:?}; stderr={stderr:?}"
    );
    let joined_stdout = stdout.join("\n");
    assert!(
        joined_stdout.contains(r#""ok":false"#),
        "expected \"ok\":false in stdout, got: {stdout:?}"
    );
    assert!(
        !joined_stdout.contains("data_dir_locked"),
        "unsafe dir must not report data_dir_locked, got: {stdout:?}"
    );
    assert!(
        !dir.path().join("keeper.lock").exists(),
        "keeper.lock must not be created for unsafe dir"
    );
    assert!(
        !dir.path().join("keeper.db").exists(),
        "keeper.db must not be created for unsafe dir"
    );
    assert!(
        !dir.path().join("hq").exists(),
        "hq/ must not be created for unsafe dir"
    );
}
