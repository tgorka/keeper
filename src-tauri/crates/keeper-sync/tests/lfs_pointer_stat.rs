//! Regression: `gix::status` MUST accept a POINTER blob paired with the
//! worktree file's stat data as "unchanged".
//!
//! This is the hinge the whole LFS design turns on, and it is why keeper needs
//! no `filter.lfs.process` subprocess. Real git+LFS works exactly
//! this way: the blob is a ~130-byte pointer, the worktree holds gigabytes, and
//! `git status` is fast and clean because the index caches the WORKTREE's stat
//! and the stat matches. If gix honours the same shortcut we need no filter
//! subprocess; if it re-hashes, every LFS file reads as permanently modified.
use gix::index::{entry::Flags, entry::Mode, entry::Stat, State};
use keeper_sync::git;
use keeper_sync::lfs::pointer::Pointer;

#[test]
fn pointer_blob_with_worktree_stat_reads_clean() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(root)
        .status()
        .expect("git init");

    let big = root.join("video.bin");
    let payload = vec![7u8; 1_048_576];
    std::fs::write(&big, &payload).expect("write big");

    // Let the file's mtime fall safely behind the index we are about to write.
    // git calls an entry whose mtime is not older than the index "racily clean"
    // and re-reads its content regardless of stat — which would mask the very
    // thing this probe is measuring.
    std::thread::sleep(std::time::Duration::from_millis(1_500));

    let repo = git::repo::open(root, false).expect("open");

    // The pointer that stands in for the payload.
    let digest = {
        use sha2::{Digest, Sha256};
        let mut h = Sha256::new();
        h.update(&payload);
        hex::encode(h.finalize())
    };
    let pointer = Pointer::new(digest, payload.len() as u64);
    let rendered = pointer.render();
    let blob = repo
        .write_blob(rendered.as_bytes())
        .expect("write blob")
        .detach();
    println!(
        "pointer blob = {blob} ({} bytes) vs payload {} bytes",
        rendered.len(),
        payload.len()
    );

    // Index entry: pointer OID, but the WORKTREE file's stat.
    let metadata = gix::index::fs::Metadata::from_path_no_follow(&big).expect("stat");
    let stat = Stat::from_fs(&metadata).expect("stat convert");
    let mut index = State::new(repo.object_hash());
    index.dangerously_push_entry(stat, blob, Flags::empty(), Mode::FILE, "video.bin".into());
    index.sort_entries();
    let mut file = gix::index::File::from_state(index, repo.index_path());
    file.write(gix::index::write::Options::default())
        .expect("write index");

    // Commit the pointer so HEAD exists and "added" is not reported for every
    // path simply because the branch is unborn.
    std::process::Command::new("git")
        .args([
            "-c",
            "user.email=t@e",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "pointer",
        ])
        .current_dir(root)
        .status()
        .expect("git commit");

    let repo = git::repo::open(root, false).expect("reopen");
    let status = git::repo::status_paths(&repo).expect("status");
    println!("POINTER+WORKTREE-STAT -> {status:?}");
    println!("git's own view:");
    let out = std::process::Command::new("git")
        .args(["status", "--short"])
        .current_dir(root)
        .output()
        .expect("git status");
    println!("  [{}]", String::from_utf8_lossy(&out.stdout).trim());
    assert!(
        status.is_empty(),
        "gix must honour the stat shortcut, or LFS files read as permanently modified"
    );
}
