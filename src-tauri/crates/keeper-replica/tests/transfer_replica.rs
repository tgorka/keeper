use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use keeper_ports::transfer::{FileTransfer, StoreOrigin, StoreSlot};
use keeper_ports::{Link, Replica, ReplicaSpec, RowOp};
use keeper_replica::transfer::{HttpGrants, StoreTransfer, TransferConfig};
use keeper_replica::SyncularReplica;
use keeper_testkit::store::StandIn;
use serde_json::{json, Value};

fn xorshift_bytes(len: usize) -> Vec<u8> {
    let mut state: u64 = 0x9e37_79b9_7f4a_7c15;
    let mut out = Vec::with_capacity(len);
    for _ in 0..len {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        out.push((state & 0xff) as u8);
    }
    out
}

fn make_transfer(s: &StandIn, plan_dir: PathBuf) -> StoreTransfer {
    let slot = StoreSlot::default();
    slot.set(StoreOrigin::parse(&s.store_origin()));
    let grants = Arc::new(
        HttpGrants::new(&s.base_url(), s.token(), Duration::from_secs(5)).expect("grants"),
    );
    let config = TransferConfig {
        threshold: 64 * 1024,
        part_bytes: 64 * 1024,
        request_timeout: Duration::from_secs(5),
        plan_dir,
    };
    StoreTransfer::new(grants, &s.base_url(), slot, config).expect("transfer")
}

fn replica_files_size(dir: &Path, prefix: &str) -> u64 {
    std::fs::read_dir(dir)
        .expect("read replica dir")
        .filter_map(|entry| {
            let entry = entry.expect("dir entry");
            let name = entry.file_name();
            let matches = name.to_str().is_some_and(|n| n.starts_with(prefix));
            matches.then(|| entry.metadata().expect("entry metadata").len())
        })
        .sum()
}

fn message(id: &str) -> RowOp {
    let Value::Object(values) = json!({
        "id": id,
        "session_id": "local.1",
        "page_key": "local.1/2026-10",
        "role": "person",
        "body": "hello",
        "created_at_ms": 1_792_000_000_000_i64,
        "author_actor": "",
        "author_client": "",
    }) else {
        unreachable!("a literal object");
    };
    RowOp::Upsert {
        table: "messages".to_owned(),
        values,
        base_version: None,
    }
}

#[test]
fn a_transfer_never_grows_the_replica_file() {
    let replica_dir = tempfile::tempdir().expect("replica tempdir");
    let replica = SyncularReplica::open(ReplicaSpec {
        database: "alpha".to_owned(),
        path: replica_dir.path().join("alpha.sqlite"),
        client_id: None,
        link: None,
        units: Vec::new(),
        watch_presence: None,
    })
    .expect("opens local replica");

    let before = replica_files_size(replica_dir.path(), "alpha.sqlite");

    let s = StandIn::start("alpha");
    let work_dir = tempfile::tempdir().expect("work tempdir");
    let plan_dir = work_dir.path().join("plans");
    std::fs::create_dir_all(&plan_dir).expect("create plan dir");
    let transfer = make_transfer(&s, plan_dir);

    let payload = xorshift_bytes(1024 * 1024);
    let src = work_dir.path().join("upload.bin");
    std::fs::write(&src, &payload).expect("write payload");

    let uploaded = transfer
        .upload(&src, Some("application/octet-stream"), &mut |_| {})
        .expect("parts upload succeeds");

    let dest = work_dir.path().join("download.bin");
    transfer
        .download(&uploaded.content_id, uploaded.size, &dest, &mut |_| {})
        .expect("download succeeds");
    assert_eq!(
        std::fs::read(&dest).expect("read downloaded"),
        payload,
        "downloaded bytes match uploaded payload"
    );

    let after = replica_files_size(replica_dir.path(), "alpha.sqlite");
    assert!(
        after.saturating_sub(before) < 1024 * 1024,
        "replica files grew from {before} to {after} bytes during transfer"
    );

    replica.close();
    replica.join();
}

#[test]
fn a_row_committed_while_a_part_upload_is_held_open_still_completes_its_sync_round() {
    let (port, seen) = keeper_testkit::blob_server(Vec::new());
    let replica_dir = tempfile::tempdir().expect("replica tempdir");
    let replica_path = replica_dir.path().join("alpha.sqlite");
    let replica = SyncularReplica::open(ReplicaSpec {
        database: "alpha".to_owned(),
        path: replica_path.clone(),
        client_id: Some("ana.dev".to_owned()),
        link: Some(Link {
            base_url: format!("http://127.0.0.1:{port}/alpha"),
            token: "0123456789abcdef".to_owned(),
            store: StoreSlot::default(),
        }),
        units: Vec::new(),
        watch_presence: None,
    })
    .expect("opens linked replica");
    keeper_testkit::bootstrapped(&replica_path);

    let s = StandIn::start("alpha");
    let work_dir = tempfile::tempdir().expect("work tempdir");
    let plan_dir = work_dir.path().join("plans");
    std::fs::create_dir_all(&plan_dir).expect("create plan dir");
    let transfer = make_transfer(&s, plan_dir);

    let payload = xorshift_bytes(10 * 64 * 1024 + 1000);
    let src = work_dir.path().join("upload.bin");
    std::fs::write(&src, &payload).expect("write payload");

    let hold = s.hold_next_part();
    let upload_path = src.clone();
    let uploader = std::thread::spawn(move || {
        transfer.upload(&upload_path, Some("application/octet-stream"), &mut |_| {})
    });

    assert!(
        hold.wait_held(Duration::from_secs(10)),
        "part upload should be held open"
    );

    let commit = replica.mutate(vec![message("m-1")]).expect("commits row");
    replica.sync_now();

    let deadline = Instant::now() + Duration::from_secs(20);
    while keeper_testkit::pushes_of(&seen, &commit).is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    assert!(
        !keeper_testkit::pushes_of(&seen, &commit).is_empty(),
        "sync round did not push commit {commit} while part upload was held"
    );

    hold.release();
    let uploaded = uploader
        .join()
        .expect("upload thread panicked")
        .expect("upload succeeds");
    assert_eq!(
        s.object(&uploaded.content_id).as_deref(),
        Some(payload.as_slice()),
        "uploaded object matches payload"
    );

    replica.close();
    replica.join();
}
