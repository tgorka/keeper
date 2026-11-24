//! Live file-transfer round-trip through `keeper-cli debug transfer` against
//! a real agents server with the scratch instance (story 115.3).
//!
//! Environment:
//! - `KEEPER_HQ_LIVE_URL`: server root URL.
//! - `KEEPER_HQ_LIVE_TOKEN_1`: bearer token (passed by env name, never printed).
//! - `KEEPER_HQ_LIVE_DATABASE`: database name.
//! - `KEEPER_TRANSFER_LIVE_BYTES` (optional): comma-separated byte sizes;
//!   defaults to 1 MiB, `threshold - 1`, and `threshold + 1`.

#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::fmt::Write as _;
use std::fs::File;
use std::io::{Read as _, Write as _};
use std::path::Path;
use std::process::Command;

use keeper_replica::limits::FILE_PART_THRESHOLD_BYTES;
use serde_json::Value;
use sha2::{Digest as _, Sha256};

const BIN: &str = env!("CARGO_BIN_EXE_keeper-cli");
const CHUNK: usize = 65_536;

fn env(key: &str) -> String {
    std::env::var(key).unwrap_or_else(|_| panic!("{key} is required"))
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

fn write_fixture(path: &Path, size: u64, seed: u8) -> String {
    let mut file = File::create(path).expect("create fixture");
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    let mut written: u64 = 0;
    while written < size {
        let take = ((size - written).min(CHUNK as u64)) as usize;
        for (i, slot) in buf[..take].iter_mut().enumerate() {
            let pos = written.wrapping_add(i as u64);
            *slot = ((pos.wrapping_mul(31).wrapping_add(u64::from(seed))) & 0xff) as u8;
        }
        file.write_all(&buf[..take]).expect("write fixture");
        hasher.update(&buf[..take]);
        written += take as u64;
    }
    file.flush().expect("flush fixture");
    hex_digest(&hasher.finalize())
}

fn hash_file(path: &Path) -> (u64, String) {
    let mut file = File::open(path).expect("open downloaded");
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    let mut total: u64 = 0;
    loop {
        let n = file.read(&mut buf).expect("read downloaded");
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    (total, hex_digest(&hasher.finalize()))
}

fn sizes() -> Vec<u64> {
    match std::env::var("KEEPER_TRANSFER_LIVE_BYTES") {
        Ok(raw) if !raw.trim().is_empty() => raw
            .split(',')
            .map(|part| {
                part.trim().parse::<u64>().unwrap_or_else(|e| {
                    panic!("invalid KEEPER_TRANSFER_LIVE_BYTES entry {part:?}: {e}")
                })
            })
            .collect(),
        _ => vec![
            1 << 20,
            FILE_PART_THRESHOLD_BYTES - 1,
            FILE_PART_THRESHOLD_BYTES + 1,
        ],
    }
}

#[test]
#[ignore = "needs a real agents server with the scratch instance (story 115.3)"]
fn transfer_live_put_then_get_matches_hashes() {
    let url = env("KEEPER_HQ_LIVE_URL");
    let _token = env("KEEPER_HQ_LIVE_TOKEN_1");
    let database = env("KEEPER_HQ_LIVE_DATABASE");
    let dir = tempfile::tempdir().expect("tempdir");

    for (idx, size) in sizes().into_iter().enumerate() {
        let src = dir.path().join(format!("src-{idx}-{size}.bin"));
        let plan_dir = dir.path().join(format!("plans-{idx}"));
        let dst = dir.path().join(format!("dst-{idx}-{size}.bin"));
        let expected_hex = write_fixture(&src, size, (idx as u8).wrapping_add(1));
        let expected_id = format!("sha256:{expected_hex}");

        let put_out = Command::new(BIN)
            .args([
                "debug",
                "transfer",
                "put",
                "--json",
                "--url",
                &url,
                "--database",
                &database,
                "--token-env",
                "KEEPER_HQ_LIVE_TOKEN_1",
                "--plan-dir",
            ])
            .arg(&plan_dir)
            .arg(&src)
            .output()
            .expect("run put");
        assert!(
            put_out.status.success(),
            "put failed for size {size}: {put_out:?}"
        );
        let put_json: Value = serde_json::from_slice(&put_out.stdout).expect("put json");
        assert_eq!(put_json["ok"], true);
        assert_eq!(put_json["contentId"], expected_id);
        assert_eq!(put_json["size"], size);

        let get_out = Command::new(BIN)
            .args([
                "debug",
                "transfer",
                "get",
                "--json",
                "--url",
                &url,
                "--database",
                &database,
                "--token-env",
                "KEEPER_HQ_LIVE_TOKEN_1",
                "--content-id",
                &expected_id,
                "--size",
                &size.to_string(),
                "--out",
            ])
            .arg(&dst)
            .output()
            .expect("run get");
        assert!(
            get_out.status.success(),
            "get failed for size {size}: {get_out:?}"
        );
        let get_json: Value = serde_json::from_slice(&get_out.stdout).expect("get json");
        assert_eq!(get_json["ok"], true);
        assert_eq!(get_json["size"], size);

        let (actual_size, actual_hex) = hash_file(&dst);
        assert_eq!(actual_size, size, "size mismatch for {size}");
        assert_eq!(actual_hex, expected_hex, "hash mismatch for {size}");

        let _ = std::fs::remove_file(&src);
        let _ = std::fs::remove_file(&dst);
    }
}
