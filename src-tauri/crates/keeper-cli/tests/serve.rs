//! 123.3-AC4, AC6, AC7, AC13 (serve): process tests for `keeper-cli serve --state <dir>`.

#![allow(clippy::unwrap_used, clippy::expect_used)]
#![cfg(target_os = "linux")]

use std::collections::HashSet;
use std::fs;
use std::io::{self, BufRead as _, BufReader};
use std::os::unix::fs::{DirBuilderExt as _, PermissionsExt as _};
use std::path::Path;
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::mpsc;
use std::time::{Duration, Instant};

use keeper_wire_api::client::Client;
use keeper_wire_api::{Code, Describe};
use serde_json::{json, Value};

const URL: &str = "http://127.0.0.1:9/";
const WAIT_SERVING_TIMEOUT: Duration = Duration::from_secs(10);
const WAIT_EXIT_TIMEOUT: Duration = Duration::from_secs(5);

fn serve_cmd(state: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_keeper-cli"));
    cmd.args(["--json", "serve", "--state"])
        .arg(state)
        .args([
            "--url",
            URL,
            "--database",
            "d1",
            "--actor",
            "a1",
            "--client",
            "c1",
        ])
        .env("KEEPER_GRAPH_TOKEN", "t");
    cmd
}

fn spawn_serve(state: &Path) -> Child {
    serve_cmd(state)
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn keeper-cli serve")
}

fn read_first_line(child: &mut Child, timeout: Duration) -> String {
    let stdout = child.stdout.take().expect("child stdout piped");
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut reader = BufReader::new(stdout);
        let mut line = String::new();
        let res = reader.read_line(&mut line).map(|_| line);
        let _ = tx.send(res);
    });
    rx.recv_timeout(timeout)
        .expect("timed out waiting for stdout line from keeper-cli serve")
        .expect("read stdout line")
}

fn wait_serving(child: &mut Child) -> String {
    let line = read_first_line(child, WAIT_SERVING_TIMEOUT);
    let value: Value = serde_json::from_str(line.trim())
        .unwrap_or_else(|e| panic!("invalid json line {line:?}: {e}"));
    assert_eq!(
        value["ok"],
        Value::Bool(true),
        "expected serving ok=true, got: {value}"
    );
    value["serving"]
        .as_str()
        .expect("serving socket path string")
        .to_owned()
}

fn wait_exit(child: &mut Child, timeout: Duration) -> ExitStatus {
    let deadline = Instant::now() + timeout;
    loop {
        if let Some(status) = child.try_wait().expect("try_wait child") {
            return status;
        }
        assert!(
            Instant::now() < deadline,
            "timed out waiting for child pid {} to exit",
            child.id()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn send_signal(pid: u32, sig: &str) {
    let status = Command::new("kill")
        .args([sig, &pid.to_string()])
        .status()
        .expect("invoke kill");
    assert!(status.success(), "kill {sig} {pid} failed: {status}");
}

fn stop(child: &mut Child) -> ExitStatus {
    send_signal(child.id(), "-TERM");
    wait_exit(child, WAIT_EXIT_TIMEOUT)
}

fn run_to_exit(state: &Path, timeout: Duration) -> (ExitStatus, String) {
    let mut child = spawn_serve(state);
    let line = read_first_line(&mut child, timeout);
    let status = wait_exit(&mut child, timeout);
    (status, line)
}

fn fresh_state_dir() -> tempfile::TempDir {
    let dir = tempfile::tempdir().expect("tempdir");
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).expect("chmod 0700");
    dir
}

fn sorted_entries(dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = fs::read_dir(dir)
        .expect("read_dir")
        .map(|entry| {
            entry
                .expect("dir entry")
                .file_name()
                .to_string_lossy()
                .into_owned()
        })
        .collect();
    names.sort();
    names
}

fn non_app_catalogue_ops() -> Vec<String> {
    let snapshot_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../catalogue/snapshot.json");
    let raw = fs::read_to_string(&snapshot_path)
        .unwrap_or_else(|e| panic!("read {}: {e}", snapshot_path.display()));
    let doc: Value = serde_json::from_str(&raw).expect("parse catalogue/snapshot.json");
    let mut ids = Vec::new();
    fn collect_ids(val: &Value, out: &mut Vec<String>) {
        match val {
            Value::Array(items) => {
                for item in items {
                    collect_ids(item, out);
                }
            }
            Value::Object(map) => {
                if let Some(Value::String(id)) = map.get("id") {
                    if id.contains('.') {
                        out.push(id.clone());
                    }
                }
                for v in map.values() {
                    collect_ids(v, out);
                }
            }
            _ => {}
        }
    }
    collect_ids(&doc, &mut ids);
    ids.sort();
    ids.dedup();
    ids.retain(|id| !id.starts_with("app."));
    assert!(
        !ids.is_empty(),
        "expected non-app operations in catalogue/snapshot.json"
    );
    ids
}

fn process_socket_inodes(pid: u32) -> HashSet<String> {
    let fd_dir = format!("/proc/{pid}/fd");
    let mut inodes = HashSet::new();
    for entry in fs::read_dir(&fd_dir).unwrap_or_else(|e| panic!("read {fd_dir}: {e}")) {
        let Ok(entry) = entry else { continue };
        let Ok(target) = fs::read_link(entry.path()) else {
            continue;
        };
        let Some(target_str) = target.to_str() else {
            continue;
        };
        if let Some(inode) = target_str
            .strip_prefix("socket:[")
            .and_then(|s| s.strip_suffix(']'))
        {
            inodes.insert(inode.to_owned());
        }
    }
    inodes
}

fn read_proc_net(path: &str) -> Option<String> {
    match fs::read_to_string(path) {
        Ok(content) => Some(content),
        Err(err) if err.kind() == io::ErrorKind::NotFound => None,
        Err(err) => panic!("read {path}: {err}"),
    }
}

#[test]
fn ac4_one_listener_100_rounds() {
    for round in 0..100 {
        let dir = fresh_state_dir();
        let mut c1 = spawn_serve(dir.path());
        let mut c2 = spawn_serve(dir.path());

        let out1 = c1.stdout.take().expect("c1 stdout");
        let out2 = c2.stdout.take().expect("c2 stdout");
        let (tx, rx) = mpsc::channel();
        for (idx, stdout) in [(0usize, out1), (1usize, out2)] {
            let tx = tx.clone();
            std::thread::spawn(move || {
                let mut reader = BufReader::new(stdout);
                let mut line = String::new();
                let res = reader.read_line(&mut line).map(|_| line);
                let _ = tx.send((idx, res));
            });
        }
        drop(tx);

        let deadline = Instant::now() + WAIT_SERVING_TIMEOUT;
        let mut lines = [None, None];
        for _ in 0..2 {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let (idx, res) = rx
                .recv_timeout(remaining)
                .unwrap_or_else(|e| panic!("round {round}: timed out reading stdout: {e}"));
            lines[idx] = Some(res.expect("read line"));
        }

        let v0: Value = serde_json::from_str(lines[0].as_deref().unwrap().trim()).unwrap();
        let v1: Value = serde_json::from_str(lines[1].as_deref().unwrap().trim()).unwrap();

        let (winner, winner_val, loser, loser_val) = match (v0["ok"].as_bool(), v1["ok"].as_bool())
        {
            (Some(true), Some(false)) => (&mut c1, v0, &mut c2, v1),
            (Some(false), Some(true)) => (&mut c2, v1, &mut c1, v0),
            _ => panic!("round {round}: expected one winner and one loser, got v0={v0}, v1={v1}"),
        };

        assert_eq!(
            loser_val["error"], "data_dir_locked",
            "round {round}: unexpected loser error: {loser_val}"
        );
        let loser_status = wait_exit(loser, WAIT_SERVING_TIMEOUT);
        assert_eq!(
            loser_status.code(),
            Some(3),
            "round {round}: loser exit status: {loser_status}"
        );

        let serving_path = winner_val["serving"].as_str().expect("serving path");
        let winner_status = stop(winner);
        assert_eq!(
            winner_status.code(),
            Some(0),
            "round {round}: winner exit status: {winner_status}"
        );
        assert!(
            !Path::new(serving_path).exists(),
            "round {round}: socket path still exists after SIGTERM: {serving_path}"
        );
    }
}

#[tokio::test]
async fn ac4_stale_socket_100_rounds() {
    let dir = fresh_state_dir();
    let mut server = spawn_serve(dir.path());
    let mut sock_path = wait_serving(&mut server);

    for round in 0..100 {
        send_signal(server.id(), "-KILL");
        let _ = wait_exit(&mut server, WAIT_EXIT_TIMEOUT);
        assert!(
            Path::new(&sock_path).exists(),
            "round {round}: socket file should remain after SIGKILL: {sock_path}"
        );

        server = spawn_serve(dir.path());
        sock_path = wait_serving(&mut server);

        let client = Client::connect(dir.path(), "test")
            .await
            .unwrap_or_else(|e| panic!("round {round}: connect failed: {e:?}"));
        let version = client
            .call("app.version", json!({}), Default::default())
            .await
            .unwrap_or_else(|e| panic!("round {round}: app.version failed: {e:?}"));
        assert!(
            version.is_object(),
            "round {round}: expected object from app.version, got {version}"
        );
        drop(client);
    }

    let status = stop(&mut server);
    assert_eq!(status.code(), Some(0), "final SIGTERM status: {status}");
}

#[test]
fn ac4_wire_path_occupied() {
    let dir = fresh_state_dir();
    let wire_dir = dir.path().join("wire");
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&wire_dir)
        .expect("create wire dir");
    fs::set_permissions(&wire_dir, fs::Permissions::from_mode(0o700)).expect("chmod wire 0700");
    let sock = wire_dir.join("wire.sock");
    fs::write(&sock, b"keep").expect("write occupied file");

    let (status, line) = run_to_exit(dir.path(), WAIT_SERVING_TIMEOUT);
    assert_eq!(
        status.code(),
        Some(3),
        "expected exit 3, got {status}; line={line}"
    );
    let value: Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(value["ok"], Value::Bool(false));
    assert_eq!(value["error"], "wire_path_occupied");
    assert_eq!(fs::read(&sock).expect("read occupied file"), b"keep");
}

#[test]
fn ac4_held_lock_touches_nothing() {
    let dir = fresh_state_dir();
    fs::write(
        dir.path().join(".keeper-cli"),
        "keeper-cli state directory\n",
    )
    .expect("write marker");
    let lock_path = dir.path().join("keeper.lock");
    fs::write(&lock_path, "").expect("create lock file");
    fs::set_permissions(&lock_path, fs::Permissions::from_mode(0o600)).expect("chmod lock 0600");

    let mut holder = Command::new("flock")
        .arg("-x")
        .arg(&lock_path)
        .args(["-c", "echo locked; read _"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .expect("spawn flock holder");

    let locked_line = read_first_line(&mut holder, WAIT_EXIT_TIMEOUT);
    assert_eq!(locked_line.trim(), "locked", "flock did not acquire lock");

    let (status, line) = run_to_exit(dir.path(), WAIT_SERVING_TIMEOUT);

    drop(holder.stdin.take());
    let _ = wait_exit(&mut holder, WAIT_EXIT_TIMEOUT);

    assert_eq!(
        status.code(),
        Some(3),
        "expected exit 3, got {status}; line={line}"
    );
    let value: Value = serde_json::from_str(line.trim()).unwrap();
    assert_eq!(value["ok"], Value::Bool(false));
    assert_eq!(value["error"], "data_dir_locked");
    assert!(
        !dir.path().join("wire").exists(),
        "wire/ must not exist when lock is held"
    );
}

#[tokio::test]
async fn ac6_nothing_but_app_reads() {
    let dir = fresh_state_dir();
    let mut server = spawn_serve(dir.path());
    let _sock = wait_serving(&mut server);

    let client = Client::connect(dir.path(), "test")
        .await
        .expect("connect to serve");

    let unknown_err = client
        .call("nope.nothing", json!({}), Default::default())
        .await
        .expect_err("nope.nothing must fail");
    assert_eq!(unknown_err.code, Code::NotFound);

    for op in non_app_catalogue_ops() {
        let err = client
            .call(&op, json!({}), Default::default())
            .await
            .unwrap_err();
        assert_eq!(
            err.code,
            Code::NotFound,
            "op {op} should be NOT_FOUND, got {err:?}"
        );
        assert_eq!(
            err.message, unknown_err.message,
            "op {op} message differed from nope.nothing"
        );
    }

    let described = client
        .describe(Describe::default())
        .await
        .expect("describe");
    let mut described_ids: Vec<String> = described.operations.into_iter().map(|o| o.id).collect();
    described_ids.sort();
    assert_eq!(
        described_ids,
        vec![
            "app.capabilities",
            "app.domains",
            "app.health",
            "app.modules",
            "app.version",
        ]
    );

    let domains = client
        .call("app.domains", json!({}), Default::default())
        .await
        .expect("app.domains");
    assert_eq!(domains, json!({"domains": []}));

    let health = client
        .call("app.health", json!({}), Default::default())
        .await
        .expect("app.health");
    assert!(
        health.get("sentence").is_none_or(Value::is_null),
        "app.health must not expose a sentence, got: {health}"
    );

    drop(client);
    let status = stop(&mut server);
    assert_eq!(status.code(), Some(0), "SIGTERM status: {status}");
}

#[test]
fn ac7_no_network_listener() {
    let dir = fresh_state_dir();
    let mut server = spawn_serve(dir.path());
    let serving = wait_serving(&mut server);
    let pid = server.id();

    let inodes = process_socket_inodes(pid);
    assert!(
        !inodes.is_empty(),
        "expected at least one socket fd for serve pid {pid}"
    );

    for tcp_path in ["/proc/net/tcp", "/proc/net/tcp6"] {
        let Some(content) = read_proc_net(tcp_path) else {
            continue;
        };
        for line in content.lines().skip(1) {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() <= 9 {
                continue;
            }
            if inodes.contains(cols[9]) {
                assert_ne!(
                    cols[3], "0A",
                    "serve pid {pid} has a TCP LISTEN socket in {tcp_path}: {line}"
                );
            }
        }
    }

    for udp_path in ["/proc/net/udp", "/proc/net/udp6"] {
        let Some(content) = read_proc_net(udp_path) else {
            continue;
        };
        for line in content.lines().skip(1) {
            let cols: Vec<&str> = line.split_whitespace().collect();
            if cols.len() <= 9 {
                continue;
            }
            assert!(
                !inodes.contains(cols[9]),
                "serve pid {pid} has a UDP socket in {udp_path}: {line}"
            );
        }
    }

    let unix_content = fs::read_to_string("/proc/net/unix").expect("read /proc/net/unix");
    let mut listening_paths = Vec::new();
    for line in unix_content.lines().skip(1) {
        let cols: Vec<&str> = line.split_whitespace().collect();
        if cols.len() < 7 {
            continue;
        }
        let flags = cols[3];
        let inode = cols[6];
        if !inodes.contains(inode) {
            continue;
        }
        let path = if cols.len() >= 8 {
            cols[7..].join(" ")
        } else {
            String::new()
        };
        assert!(
            !path.starts_with('@'),
            "serve pid {pid} has an abstract unix socket: {line}"
        );
        if flags == "00010000" {
            listening_paths.push(path);
        }
    }

    let expected_sock = dir.path().join("wire").join("wire.sock");
    let expected_sock_str = expected_sock.to_string_lossy().into_owned();
    assert_eq!(serving, expected_sock_str);
    assert_eq!(
        listening_paths,
        vec![expected_sock_str],
        "expected exactly one listening unix socket for serve pid {pid}"
    );

    let status = stop(&mut server);
    assert_eq!(status.code(), Some(0), "SIGTERM status: {status}");
}

#[test]
fn ac13_state_marker() {
    // (a1) directory with keeper.db and no marker
    {
        let dir = fresh_state_dir();
        fs::write(dir.path().join("keeper.db"), b"").expect("create keeper.db");
        let before = sorted_entries(dir.path());

        let (status, line) = run_to_exit(dir.path(), WAIT_SERVING_TIMEOUT);
        assert_eq!(
            status.code(),
            Some(3),
            "expected exit 3 for keeper.db dir, got {status}; line={line}"
        );
        let value: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(value["ok"], Value::Bool(false));
        assert_eq!(value["error"], "state_not_ours");
        let detail = value["detail"].as_str().expect("detail string");
        assert!(
            detail.contains("This directory was not written by keeper-cli"),
            "unexpected detail: {detail}"
        );
        assert_eq!(sorted_entries(dir.path()), before);
    }

    // (a2) directory with hq/ and no marker
    {
        let dir = fresh_state_dir();
        fs::create_dir(dir.path().join("hq")).expect("create hq/");
        let before = sorted_entries(dir.path());

        let (status, line) = run_to_exit(dir.path(), WAIT_SERVING_TIMEOUT);
        assert_eq!(
            status.code(),
            Some(3),
            "expected exit 3 for hq/ dir, got {status}; line={line}"
        );
        let value: Value = serde_json::from_str(line.trim()).unwrap();
        assert_eq!(value["ok"], Value::Bool(false));
        assert_eq!(value["error"], "state_not_ours");
        let detail = value["detail"].as_str().expect("detail string");
        assert!(
            detail.contains("This directory was not written by keeper-cli"),
            "unexpected detail: {detail}"
        );
        assert_eq!(sorted_entries(dir.path()), before);
    }

    // (b) 113.5-style state directory without marker -> serves and writes .keeper-cli
    {
        let dir = fresh_state_dir();
        fs::write(dir.path().join("other.graph-queue"), b"").expect("create other.graph-queue");
        assert!(!dir.path().join(".keeper-cli").exists());

        let mut server = spawn_serve(dir.path());
        let _serving = wait_serving(&mut server);
        assert!(
            dir.path().join(".keeper-cli").exists(),
            ".keeper-cli marker must exist after serve claims 113.5 state dir"
        );

        let status = stop(&mut server);
        assert_eq!(status.code(), Some(0), "SIGTERM status: {status}");
    }
}
