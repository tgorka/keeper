//! The sweep that reclaims what a killed run left behind.
//!
//! Measured on one machine after four days of a crash loop: 100 GB in 863
//! files under `lfs/tmp`, beside 4 GB of abandoned resumable downloads, on a
//! volume with 153 GB free. `tempfile` deletes its file when the handle drops,
//! and a killed process drops nothing — so every crash leaked one file per
//! transfer in flight and nothing had ever removed them.

use keeper_sync::lfs::store::LfsStore;

/// Backdate a file with `std::fs::FileTimes` rather than a crate: the sweep
/// decides by age, so a test of it has to be able to make a file old, and that
/// is not worth a dependency.
fn aged(path: &std::path::Path, seconds: u64) {
    let when = std::time::SystemTime::now() - std::time::Duration::from_secs(seconds);
    let file = std::fs::File::options()
        .write(true)
        .open(path)
        .expect("open to age");
    file.set_times(std::fs::FileTimes::new().set_modified(when))
        .expect("age the file");
}

#[test]
fn debris_goes_and_a_live_transfer_stays() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = LfsStore::in_git_dir(dir.path().join(".git"));
    store.ensure_layout().expect("layout");

    let stale = store.tmp_dir().join(".tmpAAAAAA");
    std::fs::write(&stale, vec![1u8; 4096]).expect("write stale");
    aged(&stale, 7 * 24 * 3600);

    let part = store.incomplete_path("beef");
    std::fs::write(&part, vec![2u8; 2048]).expect("write part");
    aged(&part, 7 * 24 * 3600);

    // Seconds old: exactly the shape of a transfer running right now, which a
    // sweep that went by name rather than by age would have deleted.
    let live = store.tmp_dir().join(".tmpZZZZZZ");
    std::fs::write(&live, vec![3u8; 1024]).expect("write live");

    let swept = store.sweep_scratch("test");

    assert_eq!(
        swept.found, 3,
        "everything present is counted, taken or not"
    );
    assert_eq!(
        swept.removed, 2,
        "only the two that no transfer can still be using"
    );
    assert_eq!(swept.removed_bytes, 4096 + 2048);
    assert!(!stale.exists(), "week-old scratch is debris");
    assert!(!part.exists(), "so is an abandoned resumable download");
    assert!(
        live.exists(),
        "a file seconds old belongs to a transfer in flight; taking it would trade \
         a disk problem for a corrupted download"
    );
}

#[test]
fn a_clean_store_is_swept_without_complaint() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = LfsStore::in_git_dir(dir.path().join(".git"));
    store.ensure_layout().expect("layout");

    let swept = store.sweep_scratch("test");

    assert_eq!((swept.found, swept.removed), (0, 0));
}

/// A store whose directories are not there at all must not fail the caller: a
/// sweep is housekeeping, and a folder that refused to sync because it could
/// not tidy a temp directory would have traded a small problem for a large one.
#[test]
fn a_store_with_no_scratch_directories_is_not_an_error() {
    let dir = tempfile::tempdir().expect("tempdir");
    let store = LfsStore::in_git_dir(dir.path().join(".git"));

    let swept = store.sweep_scratch("test");

    assert_eq!(swept.found, 0);
}
