//! Integration tests for `keeper-cli files` covering 116.4-AC1, AC2, AC5,
//! and AC9 at the command line (the live server's half runs on delectra).

#![cfg(any(target_os = "linux", target_os = "macos"))]

use std::collections::HashSet;
use std::fmt::Write as _;
use std::path::Path;
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

use keeper_ports::{Link, Replica, ReplicaSpec, RowOp, StoreSlot};
use keeper_replica::SyncularReplica;
use keeper_testkit::store::StandIn;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

const BIN: &str = env!("CARGO_BIN_EXE_keeper-cli");
const DEAD_URL: &str = "http://127.0.0.1:9";
const EVENT_ID: &str = "01J00000000000000000000099";
const PART: usize = 65_536;
const FILE_SIZE: usize = 10 * PART + 7;

struct Ids {
    root: String,
    docs: String,
    files: Vec<String>,
}

fn ulid(n: u32) -> String {
    format!("01J{n:023}")
}

fn pattern_bytes(len: usize) -> Vec<u8> {
    (0..len)
        .map(|i| ((i.wrapping_mul(31).wrapping_add(17)) & 0xff) as u8)
        .collect()
}

fn sha256_content_id(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    let mut out = String::with_capacity("sha256:".len() + digest.len() * 2);
    out.push_str("sha256:");
    for byte in digest {
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}

/// One row written locally, authored by this device (the replica refuses a
/// local write without its author columns).
fn upsert(table: &str, value: Value) -> RowOp {
    let mut values: serde_json::Map<String, Value> =
        serde_json::from_value(value).expect("row object");
    values.insert("author_actor".into(), json!("ana"));
    values.insert("author_client".into(), json!("ana.1"));
    RowOp::Upsert {
        table: table.into(),
        values,
        base_version: None,
    }
}

fn seed(state: &Path, url: &str, files: &[(&str, &[u8])]) -> Ids {
    std::fs::create_dir_all(state).expect("create state dir");
    let root = ulid(1);
    let docs = ulid(2);
    let drives = serde_json::to_string(&json!([{"name": "notes", "root_id": &root}]))
        .expect("serialize drives");

    let mut ops = Vec::with_capacity(3 + files.len());
    ops.push(upsert(
        "files_head",
        json!({
            "id": "alpha",
            "domain": "alpha",
            "seq": "3",
            "event_id": EVENT_ID,
            "digest": "d",
            "drives": drives,
        }),
    ));
    ops.push(upsert(
        "files_entries",
        json!({
            "id": &root,
            "domain": "alpha",
            "drive_key": "alpha/notes",
            "parent_id": &root,
            "name": "notes",
            "name_fold": "notes",
            "kind": "dir",
            "content_id": Value::Null,
            "size": Value::Null,
            "media_type": Value::Null,
            "mtime_ms": Value::Null,
            "mode": "normal",
            "revision": "1",
            "lifecycle": "live",
            "created_event_id": EVENT_ID,
            "updated_event_id": EVENT_ID,
            "updated_seq": "1",
        }),
    ));
    ops.push(upsert(
        "files_entries",
        json!({
            "id": &docs,
            "domain": "alpha",
            "drive_key": "alpha/notes",
            "parent_id": &root,
            "name": "Docs",
            "name_fold": "docs",
            "kind": "dir",
            "content_id": Value::Null,
            "size": Value::Null,
            "media_type": Value::Null,
            "mtime_ms": Value::Null,
            "mode": "normal",
            "revision": "1",
            "lifecycle": "live",
            "created_event_id": EVENT_ID,
            "updated_event_id": EVENT_ID,
            "updated_seq": "1",
        }),
    ));

    let mut file_ids = Vec::with_capacity(files.len());
    for (idx, (name, bytes)) in files.iter().enumerate() {
        let id = ulid(10 + idx as u32);
        ops.push(upsert(
            "files_entries",
            json!({
                "id": &id,
                "domain": "alpha",
                "drive_key": "alpha/notes",
                "parent_id": &docs,
                "name": name,
                "name_fold": name.to_lowercase(),
                "kind": "file",
                "content_id": sha256_content_id(bytes),
                "size": bytes.len() as u64,
                "media_type": "application/octet-stream",
                "mtime_ms": Value::Null,
                "mode": "normal",
                "revision": "1",
                "lifecycle": "live",
                "created_event_id": EVENT_ID,
                "updated_event_id": EVENT_ID,
                "updated_seq": "1",
            }),
        ));
        file_ids.push(id);
    }

    let base_url = format!("{}/alpha", url.trim_end_matches('/'));
    let replica = SyncularReplica::open(ReplicaSpec {
        database: "alpha".into(),
        path: state.join("alpha.sqlite"),
        client_id: Some("ana.1".into()),
        link: Some(Link {
            base_url,
            token: "t".into(),
            store: StoreSlot::default(),
        }),
        units: keeper_files::units("alpha", "ana", "ana.1"),
        watch_presence: None,
    })
    .expect("open replica");
    replica.mutate(ops).expect("seed replica rows");
    Replica::close(&replica);
    Replica::join(&replica);

    Ids {
        root,
        docs,
        files: file_ids,
    }
}

fn parse_json(output: &Output) -> Value {
    let stdout = String::from_utf8(output.stdout.clone()).expect("utf-8 stdout");
    let stderr = String::from_utf8_lossy(&output.stderr);
    serde_json::from_str(stdout.trim())
        .unwrap_or_else(|e| panic!("invalid JSON ({e}): stdout={stdout:?} stderr={stderr:?}"))
}

fn files_cmd(state: &Path, url: &str, token: &str) -> Command {
    let mut cmd = Command::new(BIN);
    cmd.args(["files", "--json", "--state"])
        .arg(state)
        .args([
            "--url",
            url,
            "--database",
            "alpha",
            "--actor",
            "ana",
            "--client",
            "ana.1",
            "--token-env",
            "T",
            "--wait-ms",
            "300",
        ])
        .env("T", token);
    cmd
}

#[test]
fn ls_and_drives_answer_from_the_replica_offline() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let ids = seed(&state, DEAD_URL, &[("a.txt", b"offline payload")]);

    let drives_out = files_cmd(&state, DEAD_URL, "t")
        .arg("drives")
        .output()
        .expect("run drives");
    assert_eq!(
        drives_out.status.code(),
        Some(0),
        "drives failed: {drives_out:?}"
    );
    let drives_json = parse_json(&drives_out);
    assert_eq!(drives_json["ok"], true);
    assert_eq!(
        drives_json["at"],
        json!({"server": DEAD_URL, "domain": "alpha"})
    );
    assert_eq!(drives_json["drives"][0]["name"], "notes");
    assert_eq!(drives_json["drives"][0]["root_id"], ids.root);

    let ls_out = files_cmd(&state, DEAD_URL, "t")
        .args(["ls", "--drive", "notes", "Docs"])
        .output()
        .expect("run ls");
    assert_eq!(ls_out.status.code(), Some(0), "ls failed: {ls_out:?}");
    let ls_json = parse_json(&ls_out);
    assert_eq!(ls_json["ok"], true);
    assert_eq!(
        ls_json["at"],
        json!({"server": DEAD_URL, "domain": "alpha"})
    );

    let entries = ls_json["entries"].as_array().expect("entries array");
    assert_eq!(entries.len(), 1);
    let entry = entries[0].as_object().expect("entry object");
    let mut keys: Vec<&str> = entry.keys().map(String::as_str).collect();
    keys.sort_unstable();
    assert_eq!(
        keys,
        vec![
            "at",
            "content_id",
            "drive",
            "id",
            "kind",
            "media_type",
            "mode",
            "mtime_ms",
            "name",
            "parent_id",
            "path",
            "revision",
            "size",
        ]
    );
    assert_eq!(entry["id"], ids.files[0]);
    assert_eq!(entry["parent_id"], ids.docs);
    assert_eq!(entry["path"], "Docs/a.txt");
    assert_eq!(entry["at"], json!({"server": DEAD_URL, "domain": "alpha"}));
}

#[test]
fn ls_of_a_refused_path_is_refused_with_exit_2() {
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    seed(&state, DEAD_URL, &[("a.txt", b"hello")]);

    let ls_out = files_cmd(&state, DEAD_URL, "t")
        .args(["ls", "--drive", "notes", "../etc"])
        .output()
        .expect("run refused ls");
    assert_eq!(
        ls_out.status.code(),
        Some(2),
        "unexpected ls status: {ls_out:?}"
    );
    let ls_json = parse_json(&ls_out);
    assert_eq!(ls_json["ok"], false);
    assert_eq!(ls_json["code"], "path_refused");

    let get_out = files_cmd(&state, DEAD_URL, "t")
        .args(["get", "--drive", "notes", "Docs//a.txt"])
        .output()
        .expect("run refused get");
    assert_eq!(
        get_out.status.code(),
        Some(2),
        "unexpected get status: {get_out:?}"
    );
    let get_json = parse_json(&get_out);
    assert_eq!(get_json["ok"], false);
    assert_eq!(get_json["code"], "path_refused");
}

#[test]
fn get_fetches_once_then_answers_from_the_cache() {
    let store = StandIn::start("alpha");
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let out_path = dir.path().join("out.bin");
    let bytes = pattern_bytes(300_000);
    let seeded_cid = store.seed(&bytes);
    assert_eq!(seeded_cid, sha256_content_id(&bytes));
    seed(&state, &store.server_url(), &[("a.bin", &bytes)]);

    let first = files_cmd(&state, &store.server_url(), store.token())
        .args(["get", "--drive", "notes", "Docs/a.bin", "--out"])
        .arg(&out_path)
        .output()
        .expect("run first get");
    assert_eq!(first.status.code(), Some(0), "first get failed: {first:?}");
    let first_json = parse_json(&first);
    assert_eq!(first_json["ok"], true);
    assert_eq!(first_json["fetched"], true);
    assert_eq!(std::fs::read(&out_path).expect("read out.bin"), bytes);
    assert_eq!(store.store_gets(), 1);

    let second = files_cmd(&state, &store.server_url(), store.token())
        .args(["get", "--drive", "notes", "Docs/a.bin", "--out"])
        .arg(&out_path)
        .output()
        .expect("run second get");
    assert_eq!(
        second.status.code(),
        Some(0),
        "second get failed: {second:?}"
    );
    let second_json = parse_json(&second);
    assert_eq!(second_json["ok"], true);
    assert_eq!(second_json["fetched"], false);
    assert_eq!(
        std::fs::read(&out_path).expect("read cached out.bin"),
        bytes
    );
    assert_eq!(store.store_gets(), 1);
}

#[test]
fn a_killed_get_continues_from_the_bytes_on_disk() {
    let store = StandIn::start("alpha");
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let bytes = pattern_bytes(FILE_SIZE);
    let content_id = store.seed(&bytes);
    seed(&state, &store.server_url(), &[("big.bin", &bytes)]);

    store.throttle(Some((16_384, Duration::from_millis(20))));
    let mut child = files_cmd(&state, &store.server_url(), store.token())
        .args(["get", "--drive", "notes", "Docs/big.bin"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn first get");

    let deadline = Instant::now() + Duration::from_secs(30);
    while store.store_bytes_served() < (4 * PART) as u64 {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for get progress (served={})",
            store.store_bytes_served()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    child.kill().expect("kill first get");
    let _ = child.wait().expect("wait killed get");

    store.throttle(None);
    let resumed = files_cmd(&state, &store.server_url(), store.token())
        .args(["get", "--drive", "notes", "Docs/big.bin"])
        .output()
        .expect("run resumed get");
    assert_eq!(
        resumed.status.code(),
        Some(0),
        "resumed get failed: {resumed:?}"
    );
    let resumed_json = parse_json(&resumed);
    assert_eq!(resumed_json["ok"], true);
    assert_eq!(resumed_json["fetched"], true);
    let bytes_fetched = resumed_json["bytes_fetched"]
        .as_u64()
        .expect("bytes_fetched");
    assert!(
        bytes_fetched <= (FILE_SIZE - 3 * PART) as u64,
        "expected resumed get to skip cached prefix; bytes_fetched={bytes_fetched}"
    );

    let cache_file = resumed_json["file"].as_str().expect("cache file path");
    let cached_bytes = std::fs::read(cache_file).expect("read cache file");
    assert_eq!(sha256_content_id(&cached_bytes), content_id);

    // The body never enters the replica: no window of its bytes is in the
    // replica's file or its write-ahead log (which grows with every round,
    // so its size says nothing).
    let window = &bytes[300_000..300_256];
    for name in ["alpha.sqlite", "alpha.sqlite-wal"] {
        let held = std::fs::read(state.join(name)).unwrap_or_default();
        assert!(
            !held
                .windows(window.len())
                .any(|candidate| candidate == window),
            "{name} holds the file's bytes"
        );
    }
}

#[test]
fn a_killed_put_continues_from_its_plan_file() {
    let store = StandIn::start("alpha");
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let src = dir.path().join("big.bin");
    let bytes = pattern_bytes(FILE_SIZE);
    let expected_cid = sha256_content_id(&bytes);
    std::fs::write(&src, &bytes).expect("write local file");
    seed(&state, &store.server_url(), &[]);

    let build_put = || {
        let mut cmd = files_cmd(&state, &store.server_url(), store.token());
        cmd.args([
            "--threshold",
            &PART.to_string(),
            "--part-bytes",
            &PART.to_string(),
            "put",
            "--file",
        ])
        .arg(&src)
        .args(["--drive", "notes", "Docs/big.bin"]);
        cmd
    };

    store.throttle(Some((16_384, Duration::from_millis(20))));
    let mut child = build_put()
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn first put");

    let deadline = Instant::now() + Duration::from_secs(30);
    while store.store_bytes_received() < (4 * PART) as u64 {
        assert!(
            Instant::now() < deadline,
            "timed out waiting for put progress (received={})",
            store.store_bytes_received()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    child.kill().expect("kill first put");
    let _ = child.wait().expect("wait killed put");

    store.throttle(None);
    let resumed = build_put().output().expect("run resumed put");
    assert_eq!(
        resumed.status.code(),
        Some(4),
        "expected deferred exit 4: {resumed:?}"
    );
    let resumed_json = parse_json(&resumed);
    assert_eq!(resumed_json["ok"], true);
    let parts_skipped = resumed_json["parts_skipped"]
        .as_u64()
        .expect("parts_skipped");
    assert!(
        parts_skipped >= 3,
        "expected at least 3 skipped parts on resume, got {parts_skipped}"
    );
    let outcome = resumed_json["outcome"].as_str().expect("outcome");
    assert!(
        matches!(outcome, "queued" | "sent"),
        "expected queued or sent outcome, got {outcome:?}"
    );
    let content_id = resumed_json["content_id"].as_str().expect("content_id");
    assert_eq!(content_id, expected_cid);
    assert_eq!(store.object(content_id).as_deref(), Some(bytes.as_slice()));

    let accepted = store.parts_accepted();
    let unique: HashSet<u32> = accepted.iter().copied().collect();
    let repeated = accepted.len() - unique.len();
    assert!(
        repeated <= 1,
        "at most one part may repeat across kill+resume; repeated={repeated}, parts={accepted:?}"
    );

    let queue_path = state.join("files-queue.json");
    assert!(
        queue_path.exists(),
        "missing queue file at {}",
        queue_path.display()
    );
    let queue_text = std::fs::read_to_string(&queue_path).expect("read files-queue.json");
    assert!(
        queue_text.contains(content_id),
        "files-queue.json must name {content_id}; got {queue_text}"
    );
}

#[test]
fn a_wedged_queue_neither_fails_online_verbs_nor_survives_them() {
    let store = StandIn::start("alpha");
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let bytes = pattern_bytes(300_000);
    store.seed(&bytes);
    let ids = seed(&state, &store.server_url(), &[("a.bin", &bytes)]);

    let changed_path = dir.path().join("changed.bin");
    std::fs::write(&changed_path, b"after-edit").expect("write changed.bin");
    let changed_cid = sha256_content_id(b"before-edit");
    let gone_path = dir.path().join("gone.bin");
    let gone_cid = sha256_content_id(b"never-there");

    let queue = json!([
        {
            "request_id": ulid(600),
            "drive": "notes",
            "op": {
                "op": "create",
                "entry_id": ulid(500),
                "parent_id": ids.docs,
                "name": "changed.bin",
            },
            "content_id": changed_cid,
            "size": 11,
            "mtime_ms": 1_700_000_000_000_i64,
            "body": changed_path,
        },
        {
            "request_id": ulid(601),
            "drive": "notes",
            "op": {
                "op": "create",
                "entry_id": ulid(501),
                "parent_id": ids.docs,
                "name": "gone.bin",
            },
            "content_id": gone_cid,
            "size": 11,
            "mtime_ms": 1_700_000_000_000_i64,
            "body": gone_path,
        },
    ]);
    std::fs::write(
        state.join("files-queue.json"),
        serde_json::to_vec(&queue).expect("serialize queue"),
    )
    .expect("write files-queue.json");

    let drives = files_cmd(&state, &store.server_url(), store.token())
        .arg("drives")
        .output()
        .expect("run drives");
    assert_eq!(drives.status.code(), Some(0), "drives failed: {drives:?}");
    let drives_json = parse_json(&drives);
    assert_eq!(drives_json["ok"], true);
    assert!(
        drives_json.get("flush_error").is_none(),
        "unexpected flush_error: {drives_json}"
    );
    let flushed = drives_json["flushed"].as_array().expect("flushed array");
    assert_eq!(flushed.len(), 2, "flushed: {flushed:?}");
    assert_eq!(flushed[0]["outcome"], "dropped");
    assert_eq!(flushed[0]["dropped"], "source_changed");
    assert_eq!(flushed[1]["outcome"], "dropped");
    assert_eq!(flushed[1]["dropped"], "source_gone");
    // Nothing of the edited source reached the store (no orphan blob).
    assert!(store.object(&sha256_content_id(b"after-edit")).is_none());

    let remaining: Vec<Value> = serde_json::from_str(
        &std::fs::read_to_string(state.join("files-queue.json")).expect("read files-queue.json"),
    )
    .expect("parse files-queue.json");
    assert!(remaining.is_empty(), "queue not drained: {remaining:?}");

    let ls = files_cmd(&state, &store.server_url(), store.token())
        .args(["ls", "--drive", "notes", "Docs"])
        .output()
        .expect("run ls");
    assert_eq!(ls.status.code(), Some(0), "ls failed: {ls:?}");

    let get = files_cmd(&state, &store.server_url(), store.token())
        .args(["get", "--drive", "notes", "Docs/a.bin"])
        .output()
        .expect("run get");
    assert_eq!(get.status.code(), Some(0), "get failed: {get:?}");
    let get_json = parse_json(&get);
    assert_eq!(get_json["ok"], true);
    assert_eq!(get_json["fetched"], true);
}

#[test]
fn a_failing_flush_is_reported_and_every_verb_still_answers() {
    let store = StandIn::start("alpha");
    let dir = tempfile::tempdir().expect("tempdir");
    let state = dir.path().join("state");
    let bytes = pattern_bytes(300_000);
    store.seed(&bytes);
    let ids = seed(&state, &store.server_url(), &[("a.bin", &bytes)]);

    // A queued body that cannot be read (a folder): the flush fails with a
    // storage error that is neither "gone" nor "changed".
    let unreadable = dir.path().join("a-folder");
    std::fs::create_dir(&unreadable).expect("create folder");
    let queue = json!([{
        "request_id": ulid(610),
        "drive": "notes",
        "op": {"op": "create", "entry_id": ulid(510), "parent_id": ids.docs, "name": "x.bin"},
        "content_id": sha256_content_id(b"x"),
        "size": 1,
        "mtime_ms": null,
        "body": unreadable,
    }]);
    std::fs::write(
        state.join("files-queue.json"),
        serde_json::to_vec(&queue).expect("serialize queue"),
    )
    .expect("write files-queue.json");

    let src = dir.path().join("new.txt");
    std::fs::write(&src, b"new bytes").expect("write new.txt");
    let verbs: [Vec<std::ffi::OsString>; 4] = [
        vec!["drives".into()],
        vec!["ls".into(), "--drive".into(), "notes".into(), "Docs".into()],
        vec![
            "get".into(),
            "--drive".into(),
            "notes".into(),
            "Docs/a.bin".into(),
        ],
        vec![
            "put".into(),
            "--file".into(),
            src.clone().into(),
            "--drive".into(),
            "notes".into(),
            "Docs/new.txt".into(),
        ],
    ];
    for verb in verbs {
        let out = files_cmd(&state, &store.server_url(), store.token())
            .args(&verb)
            .output()
            .expect("run verb");
        let answer = parse_json(&out);
        assert_eq!(answer["ok"], true, "{verb:?}: {answer}");
        assert_eq!(
            answer["flush_error"]["code"], "storage",
            "{verb:?}: {answer}"
        );
        assert_eq!(answer["flushed"], json!([]), "{verb:?}: {answer}");
        // drives/ls/get answer in full; the put itself is sent (no
        // receipt from the stand-in) and waits, as any put would.
        let expected = if verb[0] == "put" { 4 } else { 0 };
        assert_eq!(out.status.code(), Some(expected), "{verb:?}: {answer}");
    }
}
