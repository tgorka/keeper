//! Live round-trip through `keeper-cli files` (`drives`, `put`, `ls`, `get`)
//! against a real keeper-hq files server.
//!
//! Environment:
//! - `KEEPER_HQ_LIVE_URL`: server root URL.
//! - `KEEPER_HQ_LIVE_TOKEN_1`: bearer token (passed by env name via `--token-env`, never printed).
//! - `KEEPER_HQ_LIVE_DATABASE`: database name.
//! - `KEEPER_HQ_LIVE_ACTOR`: actor identifier.
//! - `KEEPER_HQ_LIVE_CLIENT`: base client identifier.
//! - `KEEPER_FILES_LIVE_DRIVE`: drive name the actor may write.

#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::fmt::Write as _;
use std::fs::File;
use std::io::{Read as _, Write as _};
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{json, Value};
use sha2::{Digest as _, Sha256};

const BIN: &str = env!("CARGO_BIN_EXE_keeper-cli");
const CHUNK: usize = 65_536;

struct Live {
    url: String,
    database: String,
    actor: String,
    client: String,
    drive: String,
}

fn read_env(key: &str) -> Option<String> {
    match std::env::var(key) {
        Ok(val) if !val.trim().is_empty() => Some(val),
        _ => {
            eprintln!("skipping files_live: {key} is unset or empty");
            None
        }
    }
}

fn live() -> Option<Live> {
    let url = read_env("KEEPER_HQ_LIVE_URL")?;
    let _token = read_env("KEEPER_HQ_LIVE_TOKEN_1")?;
    let database = read_env("KEEPER_HQ_LIVE_DATABASE")?;
    let actor = read_env("KEEPER_HQ_LIVE_ACTOR")?;
    let client = read_env("KEEPER_HQ_LIVE_CLIENT")?;
    let drive = read_env("KEEPER_FILES_LIVE_DRIVE")?;
    Some(Live {
        url,
        database,
        actor,
        client,
        drive,
    })
}

fn hex_digest(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(&mut out, "{b:02x}");
    }
    out
}

fn write_live_fixture(path: &Path, nanos: u128) -> (u64, String) {
    let mut bytes = Vec::with_capacity(4096);
    bytes.extend_from_slice(&nanos.to_le_bytes());
    for i in 0u32..4080 {
        let mix = (i.wrapping_mul(31) ^ (nanos as u32)).wrapping_add(0x5a);
        bytes.push((mix & 0xff) as u8);
    }
    let mut file = File::create(path).expect("create live fixture");
    file.write_all(&bytes).expect("write live fixture");
    file.flush().expect("flush live fixture");
    let digest = Sha256::digest(&bytes);
    (
        bytes.len() as u64,
        format!("sha256:{}", hex_digest(&digest)),
    )
}

fn hash_file(path: &Path) -> (u64, String) {
    let mut file = File::open(path).expect("open downloaded file");
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; CHUNK];
    let mut total: u64 = 0;
    loop {
        let n = file.read(&mut buf).expect("read downloaded file");
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
        total += n as u64;
    }
    (total, format!("sha256:{}", hex_digest(&hasher.finalize())))
}

fn run_files_cli(cfg: &Live, state_dir: &Path, client: &str, verb_args: &[&str]) -> Value {
    let out = Command::new(BIN)
        .args(["files", "--json", "--state"])
        .arg(state_dir)
        .args([
            "--url",
            &cfg.url,
            "--database",
            &cfg.database,
            "--actor",
            &cfg.actor,
            "--client",
            client,
            "--token-env",
            "KEEPER_HQ_LIVE_TOKEN_1",
        ])
        .args(verb_args)
        .output()
        .expect("run keeper-cli files");
    assert_eq!(
        out.status.code(),
        Some(0),
        "keeper-cli files {verb_args:?} exited with {:?}\nstdout: {}\nstderr: {}",
        out.status.code(),
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr),
    );
    serde_json::from_slice(&out.stdout).expect("parse keeper-cli files json")
}

fn check_ok_at(val: &Value, url: &str, db: &str) -> Result<(), String> {
    if val.get("ok").and_then(Value::as_bool) != Some(true) {
        return Err(format!("expected ok == true, got {val}"));
    }
    let expected_server = url.trim_end_matches('/');
    let server = val
        .get("at")
        .and_then(|at| at.get("server"))
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing at.server in {val}"))?;
    if server != expected_server {
        return Err(format!(
            "at.server mismatch: expected {expected_server:?}, got {server:?}"
        ));
    }
    let domain = val
        .get("at")
        .and_then(|at| at.get("domain"))
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing at.domain in {val}"))?;
    if domain != db {
        return Err(format!(
            "at.domain mismatch: expected {db:?}, got {domain:?}"
        ));
    }
    Ok(())
}

fn check_drives(val: &Value, url: &str, db: &str, drive: &str) -> Result<(), String> {
    check_ok_at(val, url, db)?;
    let drives = val
        .get("drives")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("missing drives array in {val}"))?;
    let found = drives
        .iter()
        .any(|item| item.get("name").and_then(Value::as_str) == Some(drive));
    if !found {
        return Err(format!("drive {drive:?} not listed in {drives:?}"));
    }
    Ok(())
}

fn check_put(
    val: &Value,
    url: &str,
    db: &str,
    expected_cid: &str,
    expected_size: u64,
) -> Result<(), String> {
    check_ok_at(val, url, db)?;
    let outcome = val
        .get("outcome")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing outcome in {val}"))?;
    if outcome != "accepted" {
        return Err(format!("expected outcome \"accepted\", got {outcome:?}"));
    }
    let cid = val
        .get("content_id")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing content_id in {val}"))?;
    if cid != expected_cid {
        return Err(format!(
            "content_id mismatch: expected {expected_cid:?}, got {cid:?}"
        ));
    }
    let size = val
        .get("size")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("missing size in {val}"))?;
    if size != expected_size {
        return Err(format!(
            "size mismatch: expected {expected_size}, got {size}"
        ));
    }
    let entry_id = val
        .get("entry_id")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing entry_id in {val}"))?;
    if entry_id.is_empty() {
        return Err("entry_id must not be empty".to_owned());
    }
    Ok(())
}

fn check_listed(
    val: &Value,
    url: &str,
    db: &str,
    name: &str,
    expected_cid: &str,
    expected_size: u64,
) -> Result<(), String> {
    check_ok_at(val, url, db)?;
    let entries = val
        .get("entries")
        .and_then(Value::as_array)
        .ok_or_else(|| format!("missing entries array in {val}"))?;
    let entry = entries
        .iter()
        .find(|item| item.get("name").and_then(Value::as_str) == Some(name))
        .ok_or_else(|| format!("entry {name:?} not found in {entries:?}"))?;
    let cid = entry
        .get("content_id")
        .and_then(Value::as_str)
        .ok_or_else(|| format!("missing content_id on entry {entry}"))?;
    if cid != expected_cid {
        return Err(format!(
            "listed content_id mismatch: expected {expected_cid:?}, got {cid:?}"
        ));
    }
    let size = entry
        .get("size")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("missing size on entry {entry}"))?;
    if size != expected_size {
        return Err(format!(
            "listed size mismatch: expected {expected_size}, got {size}"
        ));
    }
    Ok(())
}

fn check_first_get(
    val: &Value,
    url: &str,
    db: &str,
    expected_cid: &str,
    actual_cid: &str,
) -> Result<(), String> {
    check_ok_at(val, url, db)?;
    let fetched = val
        .get("fetched")
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("missing fetched in {val}"))?;
    if !fetched {
        return Err(format!("expected fetched == true on cold get, got {val}"));
    }
    if actual_cid != expected_cid {
        return Err(format!(
            "downloaded file sha256 mismatch: expected {expected_cid:?}, got {actual_cid:?}"
        ));
    }
    Ok(())
}

fn check_cached_get(
    val: &Value,
    url: &str,
    db: &str,
    expected_cid: &str,
    actual_cid: &str,
) -> Result<(), String> {
    check_ok_at(val, url, db)?;
    let fetched = val
        .get("fetched")
        .and_then(Value::as_bool)
        .ok_or_else(|| format!("missing fetched in {val}"))?;
    if fetched {
        return Err(format!(
            "expected fetched == false on cached get, got {val}"
        ));
    }
    let grants = val
        .get("grants")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("missing grants in {val}"))?;
    if grants != 0 {
        return Err(format!("expected grants == 0 on cached get, got {grants}"));
    }
    let bytes_fetched = val
        .get("bytes_fetched")
        .and_then(Value::as_u64)
        .ok_or_else(|| format!("missing bytes_fetched in {val}"))?;
    if bytes_fetched != 0 {
        return Err(format!(
            "expected bytes_fetched == 0 on cached get, got {bytes_fetched}"
        ));
    }
    if actual_cid != expected_cid {
        return Err(format!(
            "cached file sha256 mismatch: expected {expected_cid:?}, got {actual_cid:?}"
        ));
    }
    Ok(())
}

fn must_ok(step: &str, result: Result<(), String>) {
    if let Err(err) = result {
        panic!("{step} failed: {err}");
    }
}

#[test]
#[ignore = "needs a real keeper-hq files server (story 116.4, run on delectra at the epic end)"]
fn files_live_drives_put_ls_get() {
    let Some(cfg) = live() else {
        return;
    };
    let dir = tempfile::tempdir().expect("tempdir");
    let state_1 = dir.path().join("state-1");
    let state_2 = dir.path().join("state-2");

    let drives_json = run_files_cli(&cfg, &state_1, &cfg.client, &["drives"]);
    must_ok(
        "drives",
        check_drives(&drives_json, &cfg.url, &cfg.database, &cfg.drive),
    );

    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    let name = format!("live-{nanos}.bin");
    let src = dir.path().join(&name);
    let (expected_size, expected_cid) = write_live_fixture(&src, nanos);
    let src_str = src.to_str().expect("utf-8 src path");

    let put_json = run_files_cli(
        &cfg,
        &state_1,
        &cfg.client,
        &["put", "--file", src_str, "--drive", &cfg.drive, &name],
    );
    must_ok(
        "put",
        check_put(
            &put_json,
            &cfg.url,
            &cfg.database,
            &expected_cid,
            expected_size,
        ),
    );

    let ls_json = run_files_cli(&cfg, &state_1, &cfg.client, &["ls", "--drive", &cfg.drive]);
    must_ok(
        "ls",
        check_listed(
            &ls_json,
            &cfg.url,
            &cfg.database,
            &name,
            &expected_cid,
            expected_size,
        ),
    );

    let get_client = format!("{}.get", cfg.client);
    let out_1 = dir.path().join("out-1.bin");
    let out_1_str = out_1.to_str().expect("utf-8 out-1 path");
    let first_get_json = run_files_cli(
        &cfg,
        &state_2,
        &get_client,
        &["get", "--drive", &cfg.drive, &name, "--out", out_1_str],
    );
    let (first_size, first_cid) = hash_file(&out_1);
    assert_eq!(first_size, expected_size, "first get byte count mismatch");
    must_ok(
        "first get",
        check_first_get(
            &first_get_json,
            &cfg.url,
            &cfg.database,
            &expected_cid,
            &first_cid,
        ),
    );

    let out_2 = dir.path().join("out-2.bin");
    let out_2_str = out_2.to_str().expect("utf-8 out-2 path");
    let cached_get_json = run_files_cli(
        &cfg,
        &state_2,
        &get_client,
        &["get", "--drive", &cfg.drive, &name, "--out", out_2_str],
    );
    let (cached_size, cached_cid) = hash_file(&out_2);
    assert_eq!(cached_size, expected_size, "cached get byte count mismatch");
    must_ok(
        "cached get",
        check_cached_get(
            &cached_get_json,
            &cfg.url,
            &cfg.database,
            &expected_cid,
            &cached_cid,
        ),
    );
}

#[test]
fn files_live_checks_refuse_wrong_answers() {
    let url = "https://hq.example.test/";
    let server = "https://hq.example.test";
    let db = "personal";
    let drive = "docs";
    let name = "live-12345.bin";
    let cid = "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let other_cid = "sha256:fedcba9876543210fedcba9876543210fedcba9876543210fedcba9876543210";
    let size = 4096u64;

    let good_drives = json!({
        "ok": true,
        "at": { "server": server, "domain": db },
        "flushed": [],
        "drives": [{ "name": drive, "root_id": "root-1" }]
    });
    assert!(check_drives(&good_drives, url, db, drive).is_ok());
    assert!(check_drives(&good_drives, url, "other-db", drive).is_err());
    assert!(check_drives(&good_drives, url, db, "missing-drive").is_err());
    let bad_ok = json!({
        "ok": false,
        "at": { "server": server, "domain": db },
        "drives": [{ "name": drive, "root_id": "root-1" }]
    });
    assert!(check_drives(&bad_ok, url, db, drive).is_err());

    let good_put = json!({
        "ok": true,
        "at": { "server": server, "domain": db },
        "flushed": [],
        "outcome": "accepted",
        "content_id": cid,
        "size": size,
        "entry_id": "entry-1"
    });
    assert!(check_put(&good_put, url, db, cid, size).is_ok());
    let queued_put = json!({
        "ok": true,
        "at": { "server": server, "domain": db },
        "flushed": [],
        "outcome": "queued",
        "content_id": cid,
        "size": size,
        "entry_id": "entry-1"
    });
    assert!(check_put(&queued_put, url, db, cid, size).is_err());
    assert!(check_put(&good_put, url, db, other_cid, size).is_err());
    assert!(check_put(&good_put, url, db, cid, size + 1).is_err());

    let good_ls = json!({
        "ok": true,
        "at": { "server": server, "domain": db },
        "flushed": [],
        "entries": [{
            "id": "entry-1",
            "name": name,
            "kind": "file",
            "size": size,
            "content_id": cid,
            "path": name
        }]
    });
    assert!(check_listed(&good_ls, url, db, name, cid, size).is_ok());
    assert!(check_listed(&good_ls, url, db, "other.bin", cid, size).is_err());
    assert!(check_listed(&good_ls, url, db, name, other_cid, size).is_err());
    assert!(check_listed(&good_ls, url, db, name, cid, size - 1).is_err());

    let good_first_get = json!({
        "ok": true,
        "at": { "server": server, "domain": db },
        "flushed": [],
        "fetched": true,
        "bytes_fetched": size,
        "grants": 1,
        "file": "/tmp/out-1.bin"
    });
    assert!(check_first_get(&good_first_get, url, db, cid, cid).is_ok());
    assert!(check_first_get(&good_first_get, url, db, cid, other_cid).is_err());
    let cold_not_fetched = json!({
        "ok": true,
        "at": { "server": server, "domain": db },
        "flushed": [],
        "fetched": false,
        "bytes_fetched": 0,
        "grants": 0,
        "file": "/tmp/out-1.bin"
    });
    assert!(check_first_get(&cold_not_fetched, url, db, cid, cid).is_err());

    let good_cached_get = json!({
        "ok": true,
        "at": { "server": server, "domain": db },
        "flushed": [],
        "fetched": false,
        "bytes_fetched": 0,
        "grants": 0,
        "file": "/tmp/out-2.bin"
    });
    assert!(check_cached_get(&good_cached_get, url, db, cid, cid).is_ok());
    let cached_fetched_true = json!({
        "ok": true,
        "at": { "server": server, "domain": db },
        "flushed": [],
        "fetched": true,
        "bytes_fetched": 0,
        "grants": 0,
        "file": "/tmp/out-2.bin"
    });
    assert!(check_cached_get(&cached_fetched_true, url, db, cid, cid).is_err());
    let cached_with_grant = json!({
        "ok": true,
        "at": { "server": server, "domain": db },
        "flushed": [],
        "fetched": false,
        "bytes_fetched": 0,
        "grants": 1,
        "file": "/tmp/out-2.bin"
    });
    assert!(check_cached_get(&cached_with_grant, url, db, cid, cid).is_err());
    let cached_with_bytes = json!({
        "ok": true,
        "at": { "server": server, "domain": db },
        "flushed": [],
        "fetched": false,
        "bytes_fetched": 128,
        "grants": 0,
        "file": "/tmp/out-2.bin"
    });
    assert!(check_cached_get(&cached_with_bytes, url, db, cid, cid).is_err());
    assert!(check_cached_get(&good_cached_get, url, db, cid, other_cid).is_err());
}
