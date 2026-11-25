//! Story 123.3 switch tests: debug-only override (AC13), stop latency on
//! switch-off, and compose refusal.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use keeper_runtime::{app_module, AppInfo, Runtime};
use keeper_wire::{follow_switch, Dispatch, Limits, DEBUG_ON_VAR};
use keeper_wire_api::{socket_path, DataLock};
use tokio::io::{AsyncBufReadExt as _, BufReader};
use tokio::net::UnixStream;
use tokio::sync::watch;

fn test_info() -> AppInfo {
    AppInfo {
        version: "0.12.0".to_owned(),
        build: "switch-test".to_owned(),
        snapshot_digest: "deadbeef".to_owned(),
        domains: Vec::new(),
    }
}

fn workspace_target_dir(cargo: &str, manifest_path: &Path) -> PathBuf {
    let output = Command::new(cargo)
        .args([
            "metadata",
            "--format-version",
            "1",
            "--no-deps",
            "--manifest-path",
        ])
        .arg(manifest_path)
        .output()
        .expect("run cargo metadata");
    assert!(
        output.status.success(),
        "cargo metadata failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    let metadata: serde_json::Value =
        serde_json::from_slice(&output.stdout).expect("parse cargo metadata JSON");
    PathBuf::from(
        metadata["target_directory"]
            .as_str()
            .expect("target_directory in cargo metadata"),
    )
}

#[test]
fn ac13_only_a_debug_build_serves_with_the_override() {
    let cargo = env!("CARGO");
    let manifest_path = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../Cargo.toml");
    let target_dir = workspace_target_dir(cargo, &manifest_path);

    for (profile, release, expect_socket) in [("debug", false, true), ("release", true, false)] {
        let mut build = Command::new(cargo);
        build
            .args([
                "build",
                "--example",
                "switch_probe",
                "-p",
                "keeper-wire",
                "--manifest-path",
            ])
            .arg(&manifest_path);
        if release {
            build.arg("--release");
        }
        let status = build.status().expect("run cargo build for switch_probe");
        assert!(
            status.success(),
            "cargo build --example switch_probe ({profile}) failed: {status}"
        );

        let bin = target_dir
            .join(profile)
            .join("examples")
            .join("switch_probe");
        assert!(bin.is_file(), "expected built binary at {}", bin.display());

        let dir = common::data_dir();
        let sock = socket_path(dir.path());

        let mut child = Command::new(&bin)
            .arg(dir.path())
            .arg("2000")
            .env(DEBUG_ON_VAR, "1")
            .spawn()
            .expect("spawn switch_probe");

        let deadline = Instant::now() + Duration::from_secs(2);
        let mut appeared = false;
        while Instant::now() < deadline {
            if sock.exists() {
                appeared = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(20));
        }

        let exit = child.wait().expect("wait for switch_probe");
        assert!(
            exit.success(),
            "switch_probe ({profile}) exited with {exit}"
        );
        assert_eq!(
            appeared, expect_socket,
            "profile={profile}: expected socket appeared={expect_socket}, got {appeared}"
        );
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn switch_off_stops_within_a_second() {
    let dir = common::data_dir();
    let sock = socket_path(dir.path());
    let lock = Arc::new(DataLock::acquire(dir.path(), "switch-test").expect("acquire lock"));

    let (runtime, _local) = Runtime::build(test_info(), vec![app_module()]).expect("build runtime");
    let dispatcher: Arc<dyn Dispatch> = Arc::new(runtime.wire_dispatcher());

    let enabled = Arc::new(AtomicBool::new(false));
    let (quit_tx, quit_rx) = watch::channel(false);

    let enabled_for_task = Arc::clone(&enabled);
    let task = tokio::spawn(async move {
        follow_switch(
            lock,
            move || enabled_for_task.load(Ordering::SeqCst),
            move || Ok(Arc::clone(&dispatcher)),
            Limits::default(),
            quit_rx,
        )
        .await;
    });

    enabled.store(true, Ordering::SeqCst);
    assert!(
        common::eventually(Duration::from_secs(1), || sock.exists()).await,
        "socket did not appear within 1s of switch on"
    );

    let stream = UnixStream::connect(&sock)
        .await
        .expect("connect client to wire.sock");

    let off_at = Instant::now();
    enabled.store(false, Ordering::SeqCst);

    assert!(
        common::eventually(Duration::from_millis(1200), || !sock.exists()).await,
        "socket path still existed {:?} after switch off",
        off_at.elapsed()
    );
    assert!(
        off_at.elapsed() <= Duration::from_millis(1200),
        "socket path disappeared after {:?} (> 1.2s)",
        off_at.elapsed()
    );

    let remaining = Duration::from_millis(1200).saturating_sub(off_at.elapsed());
    let mut reader = BufReader::new(stream);
    let mut line = Vec::new();
    let read = tokio::time::timeout(remaining, reader.read_until(b'\n', &mut line))
        .await
        .expect("connected stream did not read EOF or close frame within 1.2s of switch off");
    match read {
        Ok(0) | Err(_) => {}
        Ok(_) => {
            let frame: serde_json::Value =
                serde_json::from_slice(&line).expect("close frame must be valid JSON");
            assert_eq!(frame["type"], "close", "expected close frame, got {frame}");
        }
    }

    let _ = quit_tx.send(true);
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("follow_switch did not stop on quit")
        .expect("follow_switch task panicked");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn switch_compose_refusal_warns_and_serves_nothing() {
    let dir = common::data_dir();
    let sock = socket_path(dir.path());
    let lock = Arc::new(DataLock::acquire(dir.path(), "switch-refuse-test").expect("acquire lock"));
    let (quit_tx, quit_rx) = watch::channel(false);

    let task = tokio::spawn(async move {
        follow_switch(
            lock,
            || true,
            || Err("no".to_owned()),
            Limits::default(),
            quit_rx,
        )
        .await;
    });

    tokio::time::sleep(Duration::from_millis(600)).await;
    assert!(
        !sock.exists(),
        "socket should not exist after compose refusal"
    );

    let _ = quit_tx.send(true);
    tokio::time::timeout(Duration::from_secs(2), task)
        .await
        .expect("follow_switch did not stop on quit")
        .expect("follow_switch task panicked");
}
