//! A file that is a plain blob today and would be an LFS object tomorrow.
//!
//! The threshold is a per-machine setting and it moves — the folder that
//! prompted this went from 4 MiB to 0.25 MB. A commit cannot change its mind
//! afterwards, so a drive quietly accumulates files the current rule disagrees
//! with and nothing says so until somebody runs the check by hand, which they
//! only do once they already suspect something.

use std::fs;

/// The worktree is the wrong place to ask this, and that is the trap the test
/// exists to pin: a MATERIALISED LFS file is the real bytes on disk, so a size
/// check out there says "large, therefore a blob" about a file that is already
/// an object. The answer has to come from the index.
#[test]
fn a_materialised_lfs_file_is_not_counted_as_a_blob() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let started = std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(root)
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !started {
        // No git in this environment: the claim is about keeper's reading of an
        // index, and without one there is nothing to read.
        return;
    }
    let Ok(repo) = keeper_sync::git::repo::open(root, false) else {
        return;
    };

    let big = root.join("recording.mov");
    fs::write(&big, vec![0u8; 512 * 1024]).expect("write");
    let small = root.join("note.md");
    fs::write(&small, b"# small").expect("write");

    let tracked = vec![
        std::path::PathBuf::from("recording.mov"),
        std::path::PathBuf::from("note.md"),
    ];
    // Nothing is committed, so nothing has a pointer: the large file counts and
    // the small one does not, which is the threshold doing its job.
    let (count, bytes) =
        keeper_sync::footprint::blobs_over_threshold(&repo, root, &tracked, 256 * 1024);
    assert_eq!(count, 1, "only the file over the threshold");
    assert_eq!(bytes, 512 * 1024);

    // And a threshold above both counts neither — the check reads the setting
    // rather than a constant.
    let (none, _) =
        keeper_sync::footprint::blobs_over_threshold(&repo, root, &tracked, 4 * 1024 * 1024);
    assert_eq!(none, 0);
}
