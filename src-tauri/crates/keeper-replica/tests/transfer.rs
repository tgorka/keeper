use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread;
use std::time::Duration;

use keeper_ports::transfer::{FileTransfer, ScopedFileTransfer, UploadTarget};
use keeper_ports::{ReplicaError, StoreOrigin, StoreSlot};
use keeper_replica::transfer::{HttpGrants, StoreTransfer, TransferConfig, FOREIGN};
use keeper_testkit::store::StandIn;
use tempfile::TempDir;

const PART_BYTES: u64 = 64 * 1024;
const LARGE_LEN: usize = 10 * 64 * 1024 + 1000;
const REQUEST_TIMEOUT: Duration = Duration::from_secs(5);

fn xorshift_bytes(mut state: u64, len: usize) -> Vec<u8> {
    assert_ne!(state, 0);
    let mut out = vec![0u8; len];
    for chunk in out.chunks_mut(8) {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        let bytes = state.to_le_bytes();
        chunk.copy_from_slice(&bytes[..chunk.len()]);
    }
    out
}

fn slot_for(s: &StandIn) -> StoreSlot {
    let slot = StoreSlot::default();
    slot.set(StoreOrigin::parse(&s.store_origin()));
    slot
}

fn make_transfer(s: &StandIn, slot: StoreSlot, plan_dir: PathBuf) -> StoreTransfer {
    let grants =
        Arc::new(HttpGrants::new(&s.base_url(), s.token(), REQUEST_TIMEOUT).expect("http grants"));
    let config = TransferConfig {
        threshold: PART_BYTES,
        part_bytes: PART_BYTES,
        request_timeout: REQUEST_TIMEOUT,
        plan_dir,
    };
    StoreTransfer::new(grants, &s.base_url(), slot, config).expect("store transfer")
}

fn plan_files(plan_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(plan_dir) else {
        return Vec::new();
    };
    entries
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.ends_with(".plan.json"))
        })
        .collect()
}

fn assert_foreign(err: ReplicaError) {
    match err {
        ReplicaError::Remote { code, .. } => assert_eq!(code, FOREIGN),
        other => panic!("expected {FOREIGN}, got {other:?}"),
    }
}

#[test]
fn a_small_file_goes_in_one_put_and_comes_back() {
    let s = StandIn::start("alpha");
    let dir = TempDir::new().expect("tempdir");
    let plan_dir = dir.path().join("plans");
    let src = dir.path().join("small.bin");
    let dest = dir.path().join("small-out.bin");

    let bytes = xorshift_bytes(0x1153_0001, 16 * 1024 + 333);
    let size = bytes.len() as u64;
    fs::write(&src, &bytes).expect("write source");

    let transfer = make_transfer(&s, slot_for(&s), plan_dir.clone());
    let uploaded = transfer
        .upload(&src, Some("application/octet-stream"), &mut |_| {})
        .expect("small upload");

    assert_eq!(
        s.last_grant_body(),
        Some(
            serde_json::json!({"contentId": uploaded.content_id, "size": size, "mediaType": "application/octet-stream", "partBytes": 0, "parts": []})
        )
    );
    assert_eq!(uploaded.size, size);
    assert_eq!(uploaded.parts_sent, 0);
    assert!(s.parts_accepted().is_empty());
    assert_eq!(
        s.object(&uploaded.content_id).as_deref(),
        Some(bytes.as_slice())
    );
    assert!(plan_files(&plan_dir).is_empty());

    let downloaded = transfer
        .download(&uploaded.content_id, size, &dest, &mut |_| {})
        .expect("small download");

    assert_eq!(
        s.last_grant_body(),
        Some(serde_json::json!({"contentId": uploaded.content_id}))
    );
    assert_eq!(downloaded.size, size);
    assert_eq!(fs::read(&dest).expect("read dest"), bytes);
    assert!(plan_files(&plan_dir).is_empty());
}

#[test]
fn a_large_file_goes_in_parts_and_comes_back_by_hash() {
    let s = StandIn::start("alpha");
    let dir = TempDir::new().expect("tempdir");
    let plan_dir = dir.path().join("plans");
    let src = dir.path().join("large.bin");
    let dest = dir.path().join("large-out.bin");

    let bytes = xorshift_bytes(0x1153_0002, LARGE_LEN);
    let size = bytes.len() as u64;
    fs::write(&src, &bytes).expect("write source");

    let transfer = make_transfer(&s, slot_for(&s), plan_dir.clone());
    let uploaded = transfer
        .upload(&src, Some("application/octet-stream"), &mut |_| {})
        .expect("large upload");

    assert_eq!(uploaded.size, size);
    assert_eq!(s.parts_accepted().len(), 11);
    assert_eq!(
        s.object(&uploaded.content_id).as_deref(),
        Some(bytes.as_slice())
    );
    assert!(plan_files(&plan_dir).is_empty());

    let downloaded = transfer
        .download(&uploaded.content_id, size, &dest, &mut |_| {})
        .expect("large download");

    assert_eq!(downloaded.size, size);
    assert_eq!(fs::read(&dest).expect("read dest"), bytes);
    assert!(plan_files(&plan_dir).is_empty());
}

#[test]
fn identical_bytes_for_another_target_need_their_own_grant() {
    let s = StandIn::start("alpha");
    let dir = TempDir::new().expect("tempdir");
    let src = dir.path().join("body");
    fs::write(&src, xorshift_bytes(42, LARGE_LEN)).expect("source");
    let transfer = make_transfer(&s, slot_for(&s), dir.path().join("plans"));
    let first = UploadTarget::Create {
        parent_id: "p",
        name: "first",
    };
    let cut = transfer.upload_file(&src, None, first, &mut |progress| {
        if progress.done == PART_BYTES {
            s.cut_next(100);
        }
    });
    assert!(cut.is_err(), "first target must have an unfinished plan");
    let second = UploadTarget::Create {
        parent_id: "p",
        name: "second",
    };
    let uploaded = transfer
        .upload_file(&src, None, second, &mut |_| {})
        .expect("new target");
    assert_eq!(uploaded.parts_skipped, 0);
    assert_eq!(
        s.last_grant_body().expect("new grant")["target"],
        serde_json::json!({"parent_id":"p","name":"second"})
    );
}

#[test]
fn an_upload_cut_on_the_route_resends_at_most_one_part() {
    let s = StandIn::start("alpha");
    let dir = TempDir::new().expect("tempdir");
    let plan_dir = dir.path().join("plans");
    let src = dir.path().join("cut.bin");

    let bytes = xorshift_bytes(0x1153_0003, LARGE_LEN);
    let size = bytes.len() as u64;
    fs::write(&src, &bytes).expect("write source");

    let t1 = make_transfer(&s, slot_for(&s), plan_dir.clone());
    let mut cut_armed = false;
    let first = t1.upload(&src, None, &mut |progress| {
        if !cut_armed && progress.done >= 3 * PART_BYTES {
            cut_armed = true;
            s.cut_next(100);
        }
    });
    assert!(first.is_err(), "expected cut upload to fail, got {first:?}");
    assert_eq!(
        plan_files(&plan_dir).len(),
        1,
        "expected plan file to persist after cut"
    );

    let t2 = make_transfer(&s, slot_for(&s), plan_dir.clone());
    let uploaded = t2.upload(&src, None, &mut |_| {}).expect("resumed upload");

    assert!(
        uploaded.parts_skipped >= 3,
        "expected at least 3 skipped parts, got {}",
        uploaded.parts_skipped
    );
    let overhead = s.store_bytes_received().saturating_sub(size);
    assert!(
        overhead <= PART_BYTES + 100,
        "overhead {overhead} exceeded one part + 100"
    );
    assert_eq!(
        s.object(&uploaded.content_id).as_deref(),
        Some(bytes.as_slice())
    );
    assert!(plan_files(&plan_dir).is_empty());
}

#[test]
fn a_killed_upload_restarts_from_its_plan_file() {
    let s = StandIn::start("alpha");
    let dir = TempDir::new().expect("tempdir");
    let plan_dir = dir.path().join("plans");
    let src = dir.path().join("killed.bin");

    let bytes = xorshift_bytes(0x1153_0004, LARGE_LEN);
    let size = bytes.len() as u64;
    fs::write(&src, &bytes).expect("write source");

    let hold = s.hold_next_part();
    let t1 = make_transfer(&s, slot_for(&s), plan_dir.clone());
    let src_thread = src.clone();
    let handle = thread::spawn(move || t1.upload(&src_thread, None, &mut |_| {}));

    assert!(
        hold.wait_held(Duration::from_secs(10)),
        "first part was not held within 10s"
    );
    s.cut_next(0);
    hold.release();

    let first = handle.join().expect("upload thread panicked");
    assert!(
        first.is_err(),
        "expected killed upload to fail, got {first:?}"
    );
    assert_eq!(
        plan_files(&plan_dir).len(),
        1,
        "expected plan file to remain after kill"
    );

    let t2 = make_transfer(&s, slot_for(&s), plan_dir.clone());
    let uploaded = t2
        .upload(&src, None, &mut |_| {})
        .expect("restarted upload");

    assert!(
        uploaded.parts_skipped >= 1,
        "expected at least 1 skipped part, got {}",
        uploaded.parts_skipped
    );
    let resent = s.store_bytes_received().saturating_sub(size);
    assert!(
        resent <= PART_BYTES,
        "re-sent bytes {resent} exceeded the one part in flight"
    );
    assert_eq!(
        s.object(&uploaded.content_id).as_deref(),
        Some(bytes.as_slice())
    );
    assert!(plan_files(&plan_dir).is_empty());
}

#[test]
fn a_download_cut_resumes_by_range_from_the_bytes_on_disk() {
    let s = StandIn::start("alpha");
    let dir = TempDir::new().expect("tempdir");
    let plan_dir = dir.path().join("plans");
    let dest = dir.path().join("resumed.bin");
    let part = dir.path().join("resumed.bin.part");

    let bytes = xorshift_bytes(0x1153_0005, LARGE_LEN);
    let size = bytes.len() as u64;
    let id = s.seed(&bytes);

    s.cut_next(200_000);
    let transfer = make_transfer(&s, slot_for(&s), plan_dir);
    let first = transfer.download(&id, size, &dest, &mut |_| {});
    if first.is_err() {
        assert!(part.exists(), "expected partial file on disk after cut");
        transfer
            .download(&id, size, &dest, &mut |_| {})
            .expect("resumed download");
    }

    assert_eq!(fs::read(&dest).expect("read dest"), bytes);
    assert!(!part.exists(), "partial file should be renamed into place");
    assert!(
        s.store_bytes_served() <= size + PART_BYTES,
        "served {} bytes for {} byte object",
        s.store_bytes_served(),
        size
    );
}

#[test]
fn an_expired_download_url_is_asked_again_and_succeeds() {
    let s = StandIn::start("alpha");
    let dir = TempDir::new().expect("tempdir");
    let plan_dir = dir.path().join("plans");
    let dest = dir.path().join("expired.bin");

    let bytes = xorshift_bytes(0x1153_0006, LARGE_LEN);
    let size = bytes.len() as u64;
    let id = s.seed(&bytes);

    s.cut_next(100_000);
    let transfer = make_transfer(&s, slot_for(&s), plan_dir);
    let mut expired = false;
    let downloaded = transfer
        .download(&id, size, &dest, &mut |progress| {
            if !expired && progress.done > 0 {
                expired = true;
                s.expire_urls();
            }
        })
        .expect("download with grant renewal");

    assert!(
        downloaded.grants >= 2,
        "expected at least 2 download grants after expiry, got {}",
        downloaded.grants
    );
    assert_eq!(downloaded.size, size);
    assert_eq!(fs::read(&dest).expect("read dest"), bytes);
}

#[test]
fn a_url_off_the_announced_store_is_refused_and_nothing_is_sent() {
    let s = StandIn::start("alpha");
    s.announce_foreign(true);

    let dir = TempDir::new().expect("tempdir");
    let plan_dir = dir.path().join("plans");
    let src = dir.path().join("foreign-src.bin");
    let dest = dir.path().join("foreign-dest.bin");

    let bytes = xorshift_bytes(0x1153_0007, 4096);
    let size = bytes.len() as u64;
    fs::write(&src, &bytes).expect("write source");

    let transfer = make_transfer(&s, slot_for(&s), plan_dir);
    let upload_err = transfer
        .upload(&src, None, &mut |_| {})
        .expect_err("foreign upload must be refused");
    assert_foreign(upload_err);
    assert_eq!(s.store_bytes_received(), 0);

    let id = s.seed(&bytes);
    let download_err = transfer
        .download(&id, size, &dest, &mut |_| {})
        .expect_err("foreign download must be refused");
    assert_foreign(download_err);
    assert_eq!(s.store_bytes_served(), 0);
}

#[test]
fn the_store_origin_last_announced_wins() {
    let s = StandIn::start("alpha");
    let dir = TempDir::new().expect("tempdir");
    let plan_dir = dir.path().join("plans");
    let src = dir.path().join("slot-src.bin");
    let dest = dir.path().join("slot-dest.bin");

    let bytes = xorshift_bytes(0x1153_0008, 4096);
    let size = bytes.len() as u64;
    fs::write(&src, &bytes).expect("write source");

    let slot = StoreSlot::default();
    let transfer = make_transfer(&s, slot.clone(), plan_dir);

    let err = transfer
        .upload(&src, None, &mut |_| {})
        .expect_err("unset slot must refuse store origin");
    assert_foreign(err);
    assert_eq!(s.store_bytes_received(), 0);

    slot.set(StoreOrigin::parse(&s.store_origin()));
    let uploaded = transfer
        .upload(&src, None, &mut |_| {})
        .expect("upload succeeds once slot is announced");
    assert_eq!(
        s.object(&uploaded.content_id).as_deref(),
        Some(bytes.as_slice())
    );

    slot.set(None);
    let download_err = transfer
        .download(&uploaded.content_id, size, &dest, &mut |_| {})
        .expect_err("cleared slot must refuse store origin");
    assert_foreign(download_err);
    assert_eq!(s.store_bytes_served(), 0);
}
