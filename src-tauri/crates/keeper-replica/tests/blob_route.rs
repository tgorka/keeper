use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use keeper_ports::transfer::{FileTransfer, TransferGrants, UploadAsk};
use keeper_ports::{ReplicaError, StoreOrigin, StoreSlot};
use keeper_replica::transfer::{BlobRouteGrants, HttpGrants, StoreTransfer, TransferConfig};
use keeper_testkit::store::StandIn;
use tempfile::TempDir;

const REQUEST_TIMEOUT: Duration = Duration::from_secs(10);
const THREE_MIB: usize = 3 * 1024 * 1024 + 137;

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
    slot.set(Some(
        StoreOrigin::parse(&s.store_origin()).expect("valid store origin"),
    ));
    slot
}

fn make_blob_transfer(s: &StandIn, plan_dir: PathBuf) -> StoreTransfer {
    let grants = Arc::new(
        BlobRouteGrants::new(&s.base_url(), s.token(), REQUEST_TIMEOUT).expect("blob route grants"),
    );
    StoreTransfer::new(
        grants,
        &s.base_url(),
        slot_for(s),
        TransferConfig::contract(plan_dir),
    )
    .expect("store transfer")
}

#[test]
fn seeded_blob_downloads_via_blob_route_only() {
    let s = StandIn::start("makidrive");
    let dir = TempDir::new().expect("tempdir");
    let dest = dir.path().join("model.bin");

    let bytes = xorshift_bytes(0x1213_0001, THREE_MIB);
    let size = bytes.len() as u64;
    let id = s.seed_blob(&bytes);

    let transfer = make_blob_transfer(&s, dir.path().join("plans"));
    let downloaded = transfer
        .download(&id, size, &dest, &mut |_| {})
        .expect("blob download");

    assert_eq!(downloaded.size, size);
    assert_eq!(fs::read(&dest).expect("read dest"), bytes);
    let requests = s.grant_requests();
    assert_eq!(requests, vec![format!("GET /makidrive/blobs/{id}")]);
    assert!(
        !requests.iter().any(|r| r.contains("/files/download-grant")),
        "unexpected files download grant in {requests:?}"
    );
}

#[test]
fn blob_key_space_is_not_visible_to_files_download_grant() {
    let s = StandIn::start("makidrive");
    let dir = TempDir::new().expect("tempdir");
    let dest = dir.path().join("missing.bin");

    let bytes = xorshift_bytes(0x1213_0002, 4096);
    let size = bytes.len() as u64;
    let id = s.seed_blob(&bytes);

    let grants =
        Arc::new(HttpGrants::new(&s.base_url(), s.token(), REQUEST_TIMEOUT).expect("http grants"));
    let transfer = StoreTransfer::new(
        grants,
        &s.base_url(),
        slot_for(&s),
        TransferConfig::contract(dir.path().join("plans")),
    )
    .expect("store transfer");

    let err = transfer
        .download(&id, size, &dest, &mut |_| {})
        .expect_err("files download grant must not serve blob key space");
    match err {
        ReplicaError::Remote { code, .. } => assert_eq!(code, "blob.not_found"),
        other => panic!("expected Remote blob.not_found, got {other:?}"),
    }
    assert!(!dest.exists());
    assert_eq!(
        s.grant_requests(),
        vec!["POST /makidrive/files/download-grant".to_owned()]
    );
}

#[test]
fn corrupted_blob_body_fails_integrity_and_leaves_no_dest_file() {
    let s = StandIn::start("makidrive");
    let dir = TempDir::new().expect("tempdir");
    let dest = dir.path().join("corrupt.bin");

    let good = xorshift_bytes(0x1213_0003, 64 * 1024);
    let mut wrong = good.clone();
    wrong[0] ^= 0xff;
    let size = good.len() as u64;
    let id = s.seed_blob(&good);
    s.seed_blob_as(&id, &wrong);

    let transfer = make_blob_transfer(&s, dir.path().join("plans"));
    let err = transfer
        .download(&id, size, &dest, &mut |_| {})
        .expect_err("wrong bytes must fail hash check");
    assert!(!dest.exists(), "dest file must not exist after {err:?}");
}

#[test]
fn refused_blobs_return_blob_not_found_and_write_nothing() {
    let s = StandIn::start("makidrive");
    let dir = TempDir::new().expect("tempdir");
    let dest = dir.path().join("refused.bin");

    let bytes = xorshift_bytes(0x1213_0004, 8192);
    let size = bytes.len() as u64;
    let id = s.seed_blob(&bytes);
    s.refuse_blobs(true);

    let transfer = make_blob_transfer(&s, dir.path().join("plans"));
    let err = transfer
        .download(&id, size, &dest, &mut |_| {})
        .expect_err("refused blob must fail");
    match err {
        ReplicaError::Remote { code, .. } => assert_eq!(code, "blob.not_found"),
        other => panic!("expected Remote blob.not_found, got {other:?}"),
    }
    assert!(!dest.exists());
}

#[test]
fn blob_route_grants_refuses_all_upload_methods_without_requests() {
    let s = StandIn::start("makidrive");
    let grants =
        BlobRouteGrants::new(&s.base_url(), s.token(), REQUEST_TIMEOUT).expect("blob route grants");

    let ask = UploadAsk {
        target: None,
        content_id: &format!("sha256:{}", "a".repeat(64)),
        size: 16,
        media_type: None,
        part_bytes: 0,
        parts: &[],
    };
    assert!(matches!(
        grants.grant_upload(&ask),
        Err(ReplicaError::Client(_))
    ));
    assert!(matches!(
        grants.part_urls("plan-1", &[1]),
        Err(ReplicaError::Client(_))
    ));
    assert!(matches!(
        grants.held_parts("plan-1"),
        Err(ReplicaError::Client(_))
    ));
    assert!(matches!(
        grants.complete("plan-1"),
        Err(ReplicaError::Client(_))
    ));
    assert!(s.grant_requests().is_empty());
}

#[test]
fn cut_blob_download_resumes_by_range() {
    let s = StandIn::start("makidrive");
    let dir = TempDir::new().expect("tempdir");
    let dest = dir.path().join("resumed.bin");
    let part = dir.path().join("resumed.bin.part");

    let bytes = xorshift_bytes(0x1213_0005, THREE_MIB);
    let size = bytes.len() as u64;
    let id = s.seed_blob(&bytes);

    s.cut_next(1024 * 1024);
    let transfer = make_blob_transfer(&s, dir.path().join("plans"));
    let first = transfer.download(&id, size, &dest, &mut |_| {});
    if first.is_err() {
        assert!(part.exists(), "expected partial file on disk after cut");
        transfer
            .download(&id, size, &dest, &mut |_| {})
            .expect("resumed blob download");
    }

    assert_eq!(fs::read(&dest).expect("read dest"), bytes);
    assert!(!part.exists(), "partial file should be renamed into place");
    assert!(
        s.store_bytes_served() < 2 * size,
        "served {} bytes for {} byte blob",
        s.store_bytes_served(),
        size
    );
}
