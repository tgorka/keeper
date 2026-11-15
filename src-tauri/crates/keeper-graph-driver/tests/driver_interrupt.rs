//! An interrupted driver (SIGINT, and separately SIGTERM) cancels its run
//! instead of dying by the signal: it answers `query_cancelled` and exits
//! with 128 + the signal's number, promptly. Dying by the signal is what
//! orphaned a running worker (its `kill_on_drop` never ran); the
//! cancellation's kill and reap of a live worker is keeper-core's
//! `a_cancellation_mid_query_kills_and_reaps_the_worker`.
//!
//! The run is `sync` against a server that cannot be reached, which waits
//! for the link for a minute: a long step needing no server and no worker.
//! Linux only: the handler's installation is read from `/proc` (`SigCgt`),
//! so the signal is never sent before it.

#![cfg(target_os = "linux")]

use std::io::Read;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use nix::sys::signal::{kill, Signal};
use nix::unistd::Pid;

/// Whether process `pid` catches `signal` (its `SigCgt` mask).
fn catches(pid: u32, signal: Signal) -> bool {
    std::fs::read_to_string(format!("/proc/{pid}/status"))
        .unwrap_or_default()
        .lines()
        .find_map(|line| line.strip_prefix("SigCgt:"))
        .and_then(|mask| u64::from_str_radix(mask.trim(), 16).ok())
        .is_some_and(|mask| mask & (1 << (signal as i32 - 1)) != 0)
}

#[test]
fn an_interrupt_cancels_the_run_and_exits_by_its_signal() {
    for (signal, code) in [(Signal::SIGINT, 130), (Signal::SIGTERM, 143)] {
        let state = std::env::temp_dir().join(format!(
            "kg1-driver-interrupt-{}-{signal}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&state);
        let mut driver = Command::new(env!("CARGO_BIN_EXE_keeper-graph-driver"))
            .arg("--state")
            .arg(&state)
            .args(["--url", "http://127.0.0.1:9", "--database", "work"])
            .args(["--actor", "ana", "--client", "dev1"])
            .args(["sync", "--wait-secs", "60"])
            .env("KEEPER_GRAPH_TOKEN", "interrupt-test")
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .expect("driver");
        let pid = driver.id();
        let started = Instant::now();
        while !(catches(pid, Signal::SIGINT) && catches(pid, Signal::SIGTERM)) {
            assert!(
                driver.try_wait().expect("polled").is_none(),
                "{signal}: the driver ended before it was interrupted"
            );
            assert!(
                started.elapsed() < Duration::from_secs(20),
                "{signal}: no handler"
            );
            std::thread::sleep(Duration::from_millis(5));
        }
        let interrupted = Instant::now();
        kill(Pid::from_raw(i32::try_from(pid).expect("pid")), signal).expect("signalled");
        let status = loop {
            if let Some(status) = driver.try_wait().expect("polled") {
                break status;
            }
            if interrupted.elapsed() > Duration::from_secs(5) {
                let _ = driver.kill();
                let _ = driver.wait();
                panic!("{signal}: the driver kept running");
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let elapsed = interrupted.elapsed();
        let mut out = String::new();
        driver
            .stdout
            .take()
            .expect("stdout")
            .read_to_string(&mut out)
            .expect("answer");
        let _ = std::fs::remove_dir_all(&state);
        assert_eq!(status.code(), Some(code), "{signal}: {status:?} {out}");
        let answer: serde_json::Value = serde_json::from_str(out.trim()).expect("one JSON line");
        assert_eq!(answer["ok"], false, "{signal}: {answer}");
        assert_eq!(answer["error"], "query_cancelled", "{signal}: {answer}");
        assert!(elapsed < Duration::from_secs(2), "{signal}: {elapsed:?}");
    }
}
