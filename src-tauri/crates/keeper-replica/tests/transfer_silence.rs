use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use keeper_ports::transfer::FileTransfer;
use keeper_ports::{StoreOrigin, StoreSlot};
use keeper_replica::transfer::{HttpGrants, StoreTransfer, TransferConfig};
use keeper_testkit::store::StandIn;
use tempfile::TempDir;

const TIMEOUT: Duration = Duration::from_secs(1);
const PART_BYTES: u64 = 2 * 1024 * 1024;
const MULTI_PART_LEN: usize = 2 * (PART_BYTES as usize) + 1000;
const DOWNLOAD_SILENCE_LEN: usize = 1024 * 1024;
const PACE: (usize, Duration) = (64 * 1024, Duration::from_millis(62));
/// The slow upload's parts: the store paces the first half of each (about 2 s, twice the bound)
/// and reads the last half at full speed, so whatever the client's kernel still holds after its
/// last write (OS-sized: a loopback send buffer can outgrow the store's receive window) drains
/// before the answer wait, instead of at the paced rate inside it.
const SLOW_PART_BYTES: u64 = 4 * 1024 * 1024;
const SLOW_UPLOAD_LEN: usize = 2 * (SLOW_PART_BYTES as usize) + 1000;
const SLOW_UNPACED_TAIL: u64 = SLOW_PART_BYTES / 2;
const HOLD_FOR: Duration = Duration::from_secs(20);

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

fn make_transfer(
    s: &StandIn,
    slot: StoreSlot,
    plan_dir: PathBuf,
    part_bytes: u64,
    timeout: Duration,
) -> StoreTransfer {
    let grants = Arc::new(HttpGrants::new(&s.base_url(), s.token(), timeout).expect("http grants"));
    let config = TransferConfig {
        threshold: part_bytes,
        part_bytes,
        request_timeout: timeout,
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

#[test]
fn a_slow_but_steady_store_takes_an_upload_in_parts() {
    let s = StandIn::start("slow-upload");
    let dir = TempDir::new().expect("tempdir");
    let plan_dir = dir.path().join("plans");
    let src = dir.path().join("slow.bin");

    let bytes = xorshift_bytes(0x1153_1001, SLOW_UPLOAD_LEN);
    let size = bytes.len() as u64;
    fs::write(&src, &bytes).expect("write source");

    s.throttle(Some(PACE));
    s.unpaced_upload_tail(SLOW_UNPACED_TAIL);
    let t = make_transfer(&s, slot_for(&s), plan_dir.clone(), SLOW_PART_BYTES, TIMEOUT);
    let started = Instant::now();
    let uploaded = t.upload(&src, None, &mut |_| {}).expect("slow upload");
    let elapsed = started.elapsed();

    // Two full parts, each paced for ~2 s: the upload outlasts the 1 s bound several times over,
    // so it went through only because no single wait exceeded it.
    assert!(
        elapsed > 3 * TIMEOUT,
        "slow upload took {elapsed:?}, expected over {:?}",
        3 * TIMEOUT
    );

    assert_eq!(
        s.object(&uploaded.content_id).as_deref(),
        Some(bytes.as_slice())
    );
    assert_eq!(s.store_bytes_received(), size);
    assert!(plan_files(&plan_dir).is_empty());
}

#[test]
fn a_slow_but_steady_store_serves_a_download_in_one_request() {
    let s = StandIn::start("slow-download");
    let dir = TempDir::new().expect("tempdir");
    let plan_dir = dir.path().join("plans");
    let dest = dir.path().join("slow-out.bin");

    let bytes = xorshift_bytes(0x1153_1002, MULTI_PART_LEN);
    let size = bytes.len() as u64;
    let id = s.seed(&bytes);

    s.throttle(Some(PACE));
    let t = make_transfer(&s, slot_for(&s), plan_dir, PART_BYTES, TIMEOUT);
    t.download(&id, size, &dest, &mut |_| {})
        .expect("slow download");

    let got = fs::read(&dest).expect("read destination");
    assert_eq!(got, bytes);
    assert_eq!(s.store_gets(), 1);
    assert_eq!(s.store_bytes_served(), size);
}

#[test]
fn an_upload_whose_store_goes_silent_fails_within_the_bound_and_resumes() {
    let s = StandIn::start("silent-upload");
    let dir = TempDir::new().expect("tempdir");
    let plan_dir = dir.path().join("plans");
    let src = dir.path().join("silent.bin");

    let bytes = xorshift_bytes(0x1153_1003, MULTI_PART_LEN);
    let size = bytes.len() as u64;
    fs::write(&src, &bytes).expect("write source");

    let t1 = make_transfer(&s, slot_for(&s), plan_dir.clone(), PART_BYTES, TIMEOUT);
    let started = Instant::now();
    let mut silence_armed = false;
    let first = t1.upload(&src, None, &mut |progress| {
        if !silence_armed && progress.done >= PART_BYTES {
            silence_armed = true;
            s.silence_next(4096, HOLD_FOR);
        }
    });
    let elapsed = started.elapsed();

    assert!(
        first.is_err(),
        "expected silenced upload to fail, got {first:?}"
    );
    assert!(
        elapsed < 4 * TIMEOUT,
        "silenced upload took {elapsed:?}, expected under {:?}",
        4 * TIMEOUT
    );
    assert_eq!(
        plan_files(&plan_dir).len(),
        1,
        "expected plan file to persist after silence"
    );

    let t2 = make_transfer(&s, slot_for(&s), plan_dir.clone(), PART_BYTES, TIMEOUT);
    let uploaded = t2.upload(&src, None, &mut |_| {}).expect("resumed upload");

    assert!(
        uploaded.parts_skipped >= 1,
        "expected at least 1 skipped part, got {}",
        uploaded.parts_skipped
    );
    let overhead = s.store_bytes_received().saturating_sub(size);
    assert!(
        overhead <= PART_BYTES,
        "overhead {overhead} exceeded one part ({PART_BYTES})"
    );
    assert_eq!(
        s.object(&uploaded.content_id).as_deref(),
        Some(bytes.as_slice())
    );
    assert!(plan_files(&plan_dir).is_empty());
}

#[test]
fn a_download_whose_store_goes_silent_resumes_by_range_within_the_bound() {
    let s = StandIn::start("silent-download");
    let dir = TempDir::new().expect("tempdir");
    let plan_dir = dir.path().join("plans");
    let dest = dir.path().join("silent-out.bin");

    let bytes = xorshift_bytes(0x1153_1004, DOWNLOAD_SILENCE_LEN);
    let size = bytes.len() as u64;
    let id = s.seed(&bytes);

    s.silence_next(300_000, HOLD_FOR);
    let t = make_transfer(&s, slot_for(&s), plan_dir, PART_BYTES, TIMEOUT);
    let started = Instant::now();
    t.download(&id, size, &dest, &mut |_| {})
        .expect("resumed download");
    let elapsed = started.elapsed();

    assert!(
        elapsed < 4 * TIMEOUT,
        "resumed download took {elapsed:?}, expected under {:?}",
        4 * TIMEOUT
    );
    assert_eq!(s.store_gets(), 2);
    let got = fs::read(&dest).expect("read destination");
    assert_eq!(got, bytes);
    assert_eq!(s.store_bytes_served(), size);
}
