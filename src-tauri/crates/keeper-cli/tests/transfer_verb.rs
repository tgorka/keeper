//! Integration tests for `keeper-cli debug transfer` against the loopback
//! stand-in store (story 115.3, AC17).

#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::path::Path;
use std::process::{Command, Output};
use std::time::{Duration, Instant};

use keeper_testkit::store::StandIn;
use serde_json::Value;

const BIN: &str = env!("CARGO_BIN_EXE_keeper-cli");
const PART: usize = 65_536;
const FILE_SIZE: usize = 10 * PART + 7;

fn pattern_bytes(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| ((i.wrapping_mul(31).wrapping_add(17)) & 0xff) as u8)
        .collect()
}

fn parse_json(output: &Output) -> Value {
    let stdout = String::from_utf8(output.stdout.clone()).expect("utf-8 stdout");
    let stderr = String::from_utf8_lossy(&output.stderr);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("invalid JSON ({e}): stdout={stdout:?} stderr={stderr:?}"))
}

fn put_cmd(server_url: &str, token: &str, plan_dir: &Path, file: &Path) -> Command {
    let mut cmd = Command::new(BIN);
    cmd.args([
        "debug",
        "transfer",
        "put",
        "--json",
        "--url",
        server_url,
        "--database",
        "alpha",
        "--token-env",
        "T",
        "--threshold",
        &PART.to_string(),
        "--part-bytes",
        &PART.to_string(),
        "--plan-dir",
    ])
    .arg(plan_dir)
    .arg(file)
    .env("T", token);
    cmd
}

fn get_cmd(server_url: &str, token: &str, content_id: &str, size: u64, out: &Path) -> Command {
    let mut cmd = Command::new(BIN);
    cmd.args([
        "debug",
        "transfer",
        "get",
        "--json",
        "--url",
        server_url,
        "--database",
        "alpha",
        "--token-env",
        "T",
        "--content-id",
        content_id,
        "--size",
        &size.to_string(),
        "--out",
    ])
    .arg(out)
    .env("T", token);
    cmd
}

#[test]
fn put_then_get_round_trips_bytes() {
    let stand_in = StandIn::start("alpha");
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("payload.bin");
    let plan_dir = dir.path().join("plans");
    let dst = dir.path().join("downloaded.bin");
    let data = pattern_bytes(FILE_SIZE);
    std::fs::write(&src, &data).expect("write fixture");

    let put_out = put_cmd(&stand_in.server_url(), stand_in.token(), &plan_dir, &src)
        .output()
        .expect("run put");
    assert!(put_out.status.success(), "put failed: {put_out:?}");
    let put_json = parse_json(&put_out);
    assert_eq!(put_json["ok"], true);
    let content_id = put_json["contentId"].as_str().expect("contentId");
    assert_eq!(put_json["size"], FILE_SIZE as u64);
    assert_eq!(
        stand_in.object(content_id).as_deref(),
        Some(data.as_slice())
    );

    let get_out = get_cmd(
        &stand_in.server_url(),
        stand_in.token(),
        content_id,
        FILE_SIZE as u64,
        &dst,
    )
    .output()
    .expect("run get");
    assert!(get_out.status.success(), "get failed: {get_out:?}");
    let get_json = parse_json(&get_out);
    assert_eq!(get_json["ok"], true);
    assert_eq!(get_json["size"], FILE_SIZE as u64);
    assert_eq!(std::fs::read(&dst).expect("read out"), data);
}

#[test]
fn killed_put_resumes_from_plan_without_resending_held_parts() {
    let stand_in = StandIn::start("alpha");
    let dir = tempfile::tempdir().expect("tempdir");
    let src = dir.path().join("payload.bin");
    let plan_dir = dir.path().join("plans");
    let data = pattern_bytes(FILE_SIZE);
    std::fs::write(&src, &data).expect("write fixture");

    let hold = stand_in.hold_next_part();
    let mut child = put_cmd(&stand_in.server_url(), stand_in.token(), &plan_dir, &src)
        .spawn()
        .expect("spawn put");
    assert!(
        hold.wait_held(Duration::from_secs(10)),
        "stand-in did not hold a part within 10s"
    );
    child.kill().expect("kill put child");
    hold.release();
    let _ = child.wait();

    let deadline = Instant::now() + Duration::from_secs(10);
    while stand_in.parts_accepted().is_empty() {
        assert!(
            Instant::now() < deadline,
            "stand-in did not accept held part within 10s"
        );
        std::thread::sleep(Duration::from_millis(10));
    }

    let resumed = put_cmd(&stand_in.server_url(), stand_in.token(), &plan_dir, &src)
        .output()
        .expect("run resumed put");
    assert!(resumed.status.success(), "resumed put failed: {resumed:?}");
    let json = parse_json(&resumed);
    assert_eq!(json["ok"], true);
    let parts_skipped = json["partsSkipped"].as_u64().expect("partsSkipped");
    assert!(
        parts_skipped >= 1,
        "expected at least one skipped part on resume, got {json}"
    );
    let content_id = json["contentId"].as_str().expect("contentId");
    assert_eq!(
        stand_in.object(content_id).as_deref(),
        Some(data.as_slice())
    );
    let extra = stand_in
        .store_bytes_received()
        .saturating_sub(FILE_SIZE as u64);
    assert!(
        extra <= PART as u64,
        "re-sent more than the one part in flight: received={}, size={}, extra={}",
        stand_in.store_bytes_received(),
        FILE_SIZE,
        extra
    );
}

#[test]
fn killed_get_resumes_from_its_part_file() {
    let stand_in = StandIn::start("alpha");
    let dir = tempfile::tempdir().expect("tempdir");
    let dst = dir.path().join("out.bin");
    let part = dir.path().join("out.bin.part");
    let data = pattern_bytes(FILE_SIZE);
    let content_id = stand_in.seed(&data);
    let half = FILE_SIZE as u64 / 2;

    stand_in.silence_next(half, Duration::from_secs(30));
    let mut child = get_cmd(
        &stand_in.server_url(),
        stand_in.token(),
        &content_id,
        FILE_SIZE as u64,
        &dst,
    )
    .spawn()
    .expect("spawn get");

    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if std::fs::metadata(&part).map(|m| m.len()).unwrap_or(0) == half {
            break;
        }
        assert!(
            Instant::now() < deadline,
            "part file did not reach {half} bytes within 10s (len={:?})",
            std::fs::metadata(&part).ok().map(|m| m.len())
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    child.kill().expect("kill get child");
    let _ = child.wait();

    assert!(!dst.exists(), "destination file exists before resume");
    assert_eq!(std::fs::metadata(&part).expect("part metadata").len(), half);

    let served_before = stand_in.store_bytes_served();
    let resumed = get_cmd(
        &stand_in.server_url(),
        stand_in.token(),
        &content_id,
        FILE_SIZE as u64,
        &dst,
    )
    .output()
    .expect("run resumed get");
    assert!(resumed.status.success(), "resumed get failed: {resumed:?}");
    let json = parse_json(&resumed);
    assert_eq!(json["ok"], true);
    assert_eq!(std::fs::read(&dst).expect("read out"), data);
    assert!(!part.exists(), "part file still exists after resume");

    let remaining = FILE_SIZE as u64 - half;
    let resumed_served = stand_in.store_bytes_served() - served_before;
    assert!(
        resumed_served <= remaining,
        "re-fetched too many bytes: resumed_served={resumed_served}, remaining={remaining}"
    );
    assert_eq!(stand_in.store_gets(), 2);
}
