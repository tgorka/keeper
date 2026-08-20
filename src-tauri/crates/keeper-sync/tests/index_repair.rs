//! The repair that puts a folder back where `status` is cheap again.
//!
//! # The failure it undoes
//!
//! `refresh_index_stat` runs in exactly one place: right after a
//! materialization, over the paths it touched. A run that dies between writing
//! the real files and refreshing their stat leaves the index describing
//! pointers and the worktree holding gigabytes — and nothing ever calls it for
//! those paths again, so the folder never heals.
//!
//! What that costs: every entry whose stat disagrees is one `status` must
//! convert through the LFS filter to compare. One vault reached 93 GB of
//! conversions per pass and stopped syncing entirely.
//!
//! The scenario is built the way `lfs_pointer_stat.rs` builds it — a pointer
//! blob paired with a stat — because that pairing IS the invariant, and a test
//! over an ordinary modified file would be testing something else: a file whose
//! content genuinely changed must read as modified, and re-stating it neither
//! can nor should hide that.

use gix::index::{entry::Flags, entry::Mode, entry::Stat, State};
use keeper_sync::git;
use keeper_sync::lfs::pointer::Pointer;

fn git_in(root: &std::path::Path, args: &[&str]) {
    let ok = std::process::Command::new("git")
        .args(args)
        .current_dir(root)
        .status()
        .expect("git")
        .success();
    assert!(ok, "git {args:?} failed");
}

fn porcelain(root: &std::path::Path) -> String {
    let out = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .expect("status");
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// A repository in the broken state: the worktree holds the real file, the
/// index holds the pointer blob AND the pointer's stat, because the run that
/// materialized it never got to refresh the stat.
fn a_folder_left_mid_materialization() -> (tempfile::TempDir, String) {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git_in(root, &["init", "-q", "-b", "main"]);

    let payload = vec![7u8; 1_048_576];
    let path = root.join("video.bin");

    // The pointer, and the stat OF THE POINTER — which is what an entry carries
    // before its content is materialized.
    std::fs::write(&path, b"placeholder").expect("write placeholder");
    let stale = gix::index::fs::Metadata::from_path_no_follow(&path).expect("stat");
    let stale = Stat::from_fs(&stale).expect("convert");

    let repo = git::repo::open(root, false).expect("open");
    let digest = {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(&payload);
        hex::encode(hasher.finalize())
    };
    let rendered = Pointer::new(digest.clone(), payload.len() as u64).render();
    let blob = repo
        .write_blob(rendered.as_bytes())
        .expect("write blob")
        .detach();

    let mut index = State::new(repo.object_hash());
    index.dangerously_push_entry(stale, blob, Flags::empty(), Mode::FILE, "video.bin".into());
    index.sort_entries();
    gix::index::File::from_state(index, repo.index_path())
        .write(gix::index::write::Options::default())
        .expect("write index");
    git_in(
        root,
        &[
            "-c",
            "user.email=t@e",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "pointer",
        ],
    );

    // Now materialize — and stop there, as a killed run does.
    std::fs::write(&path, &payload).expect("materialize");
    std::thread::sleep(std::time::Duration::from_millis(1_500));

    (dir, digest)
}

#[test]
fn a_folder_left_mid_materialization_is_repaired() {
    let (dir, _digest) = a_folder_left_mid_materialization();
    let root = dir.path();

    // The premise, asserted rather than assumed: without this the test could
    // pass over a folder that was never broken.
    assert!(
        !porcelain(root).is_empty(),
        "the entry must read as modified before the repair, or this test is not \
         looking at the failure it claims to"
    );

    let repo = git::repo::open(root, false).expect("open");
    let corrected = git::repo::repair_index_stat(&repo).expect("repair");

    assert_eq!(corrected, 1, "the one materialized entry needed correcting");
    let repo = git::repo::open(root, false).expect("reopen");
    let status = git::repo::status_paths(&repo).expect("status");
    assert!(
        status.is_empty(),
        "after the repair the folder reads clean, which is what stops every \
         status pass converting the worktree through the filter: {status:?}"
    );
}

#[test]
fn an_entry_still_holding_its_pointer_is_left_alone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    git_in(root, &["init", "-q", "-b", "main"]);
    // A worktree file that IS a pointer: consistent already, and re-stating it
    // would be a write for nothing.
    std::fs::write(
        root.join("pointer.bin"),
        Pointer::new("1".repeat(64), 12).render(),
    )
    .expect("write pointer");
    git_in(root, &["add", "-A"]);
    git_in(
        root,
        &[
            "-c",
            "user.email=t@e",
            "-c",
            "user.name=t",
            "commit",
            "-q",
            "-m",
            "x",
        ],
    );

    let repo = git::repo::open(root, false).expect("open");
    assert_eq!(
        git::repo::repair_index_stat(&repo).expect("repair"),
        0,
        "nothing to repair must be reported as nothing, or the button cannot \
         tell 'you were fine' from 'I fixed 500 files'"
    );
}
