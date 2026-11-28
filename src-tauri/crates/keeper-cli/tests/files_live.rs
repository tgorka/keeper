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
//! - Run only against a disposable, freshly provisioned domain (one test thread).
//!   `files_live/scenarios.rs` documents the extra principals and folders for those cases.

#![cfg(any(target_os = "linux", target_os = "macos"))]

#[path = "files_live/scenarios.rs"]
mod scenarios;

use std::fmt::Write as _;
use std::fs::File;
use std::io::{Read as _, Write as _};
use std::path::Path;
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
    run: String,
}

fn required(key: &str) -> String {
    std::env::var(key)
        .ok()
        .filter(|v| !v.trim().is_empty())
        .unwrap_or_else(|| panic!("files_live requires {key}"))
}

fn live() -> Live {
    let run = required("KEEPER_FILES_LIVE_RUN");
    assert!(
        run.len() == 32
            && run
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b)),
        "expected fresh 32-character lowercase hex run id"
    );
    let database = required("KEEPER_HQ_LIVE_DATABASE");
    assert!(
        database == format!("fileslive{run}"),
        "not this run's scratch database"
    );
    let cfg = Live {
        run,
        url: required("KEEPER_HQ_LIVE_URL"),
        database,
        actor: required("KEEPER_HQ_LIVE_ACTOR"),
        client: required("KEEPER_HQ_LIVE_CLIENT"),
        drive: required("KEEPER_FILES_LIVE_DRIVE"),
    };
    scenarios::verify_fixture(&cfg);
    cfg
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

fn check_ok_at(val: &Value, url: &str, db: &str) -> Result<(), &'static str> {
    if val["ok"].as_bool() != Some(true) {
        return Err("expected ok");
    }
    if val["at"]["server"].as_str() != Some(url.trim_end_matches('/')) {
        return Err("wrong server");
    }
    if val["at"]["domain"].as_str() != Some(db) {
        return Err("wrong domain");
    }
    Ok(())
}

fn check_drives(val: &Value, url: &str, db: &str, drive: &str) -> Result<(), &'static str> {
    check_ok_at(val, url, db)?;
    if !val["drives"]
        .as_array()
        .ok_or("missing drives")?
        .iter()
        .any(|d| d["name"].as_str() == Some(drive))
    {
        return Err("drive not listed");
    }
    Ok(())
}

fn check_put(
    val: &Value,
    url: &str,
    db: &str,
    expected_cid: &str,
    expected_size: u64,
) -> Result<(), &'static str> {
    check_ok_at(val, url, db)?;
    if val["outcome"].as_str() != Some("accepted") {
        return Err("put not accepted");
    }
    if val["content_id"].as_str() != Some(expected_cid)
        || val["size"].as_u64() != Some(expected_size)
    {
        return Err("wrong put content");
    }
    if val["entry_id"].as_str().is_none_or(str::is_empty) {
        return Err("missing entry id");
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
) -> Result<(), &'static str> {
    check_ok_at(val, url, db)?;
    let entry = val["entries"]
        .as_array()
        .ok_or("missing entries")?
        .iter()
        .find(|e| e["name"].as_str() == Some(name))
        .ok_or("entry not listed")?;
    if entry["content_id"].as_str() != Some(expected_cid)
        || entry["size"].as_u64() != Some(expected_size)
    {
        return Err("wrong listed content");
    }
    Ok(())
}

fn check_first_get(
    val: &Value,
    url: &str,
    db: &str,
    expected_cid: &str,
    actual_cid: &str,
) -> Result<(), &'static str> {
    check_ok_at(val, url, db)?;
    if val["fetched"].as_bool() != Some(true) || actual_cid != expected_cid {
        return Err("cold get did not fetch expected bytes");
    }
    Ok(())
}

fn check_cached_get(
    val: &Value,
    url: &str,
    db: &str,
    expected_cid: &str,
    actual_cid: &str,
) -> Result<(), &'static str> {
    check_ok_at(val, url, db)?;
    if val["fetched"].as_bool() != Some(false)
        || val["grants"].as_u64() != Some(0)
        || val["bytes_fetched"].as_u64() != Some(0)
        || actual_cid != expected_cid
    {
        return Err("cached get did not use expected local bytes");
    }
    Ok(())
}

fn must_ok(step: &str, result: Result<(), &'static str>) {
    if let Err(err) = result {
        panic!("{step} failed: {err}");
    }
}

#[test]
#[ignore = "requires a dedicated disposable files server"]
fn files_live_drives_put_ls_get() {
    let cfg = live();
    let dir = tempfile::tempdir().expect("tempdir");
    let editor = scenarios::Device::new(&cfg, "EDITOR");
    let peer = scenarios::Device::new(&cfg, "PEER");

    let drives_json = editor.ok(&["drives"]);
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

    let put_json = editor.ok(&["put", "--file", src_str, "--drive", &cfg.drive, &name]);
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

    let ls_json = editor.ok(&["ls", "--drive", &cfg.drive]);
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

    let out_1 = dir.path().join("out-1.bin");
    let out_1_str = out_1.to_str().expect("utf-8 out-1 path");
    let first_get_json = peer.ok(&["get", "--drive", &cfg.drive, &name, "--out", out_1_str]);
    let (first_size, first_cid) = hash_file(&out_1);
    assert!(first_size == expected_size, "first get byte count mismatch");
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
    let cached_get_json = peer.ok(&["get", "--drive", &cfg.drive, &name, "--out", out_2_str]);
    let (cached_size, cached_cid) = hash_file(&out_2);
    assert!(
        cached_size == expected_size,
        "cached get byte count mismatch"
    );
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
