//! End-to-end LFS behaviour against a real repository (Stories 25.1–25.4).
//!
//! These exercise the whole clean/smudge path the way a user meets it, because
//! the interesting failures here are not in any single function: they are in
//! whether git, gitoxide and keeper all agree about what a tracked file
//! contains. Unit tests could not have caught the two defects that motivated
//! this file — a pointer blob reading as permanently modified under plain
//! `git`, and a materialized object leaving a stale index stat behind.
//!
//! No network: the transfer layer has its own tests, and what matters here is
//! the local object store plus the filter.

use std::path::{Path, PathBuf};

use keeper_sync::git;
use keeper_sync::lfs::pointer::Pointer;
use keeper_sync::lfs::stage;
use keeper_sync::lfs::store::LfsStore;
use keeper_sync::profile::SyncProfile;
use keeper_sync::provenance::{Provenance, SyncSource};

/// The daemon binary doubles as the clean/smudge filter, so the tests point git
/// at whatever test binary is running — it is not the daemon, so the filter
/// tests below drive `stage`/`store` directly and only the *registration* is
/// asserted against git.
fn init_repo(root: &Path) {
    let ok = std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(root)
        .status()
        .expect("git init")
        .success();
    assert!(ok, "git init must succeed");
}

fn profile(root: &Path) -> SyncProfile {
    let mut p = SyncProfile::new("01JTEST", "media", root, "https://git.invalid/r.git");
    p.lfs_threshold_bytes = 1024;
    p
}

fn commit(
    repo: &gix::Repository,
    changes: &git::commit::StagedChange,
    subs: &std::collections::BTreeMap<PathBuf, Vec<u8>>,
) {
    let prov = Provenance::new("media", "dev", "01JDEV", "host", SyncSource::Cli);
    let sig = gix::actor::Signature {
        name: "t".into(),
        email: "t@example.invalid".into(),
        time: gix::date::Time::new(1_700_000_000, 0),
    };
    let root = repo
        .workdir()
        .expect("the fixture repository has a work tree");
    git::commit::stage_and_commit(repo, changes, &prov, &profile(root), &sig, subs, None)
        .expect("commit");
}

#[test]
fn an_oversized_file_is_committed_as_a_pointer_while_the_worktree_keeps_it() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_repo(root);

    let payload = vec![42u8; 200_000];
    std::fs::write(root.join("clip.mp4"), &payload).expect("write");
    std::fs::write(root.join("note.txt"), b"small").expect("write");

    let p = profile(root);
    let store = LfsStore::in_git_dir(root.join(".git"));
    let candidates = vec![PathBuf::from("clip.mp4"), PathBuf::from("note.txt")];
    let staging = stage::prepare(&p, &store, &candidates).expect("prepare");

    // Only the oversized file is routed.
    assert_eq!(staging.substitutions.len(), 1);
    assert!(staging.attributes_changed);

    let repo = git::repo::open(root, false).expect("open");
    let mut changes = git::commit::StagedChange {
        added: candidates.clone(),
        ..Default::default()
    };
    changes.added.push(PathBuf::from(".gitattributes"));
    commit(&repo, &changes, &staging.substitutions);

    // git stored a pointer for the big file and the real bytes for the small one.
    let blob = std::process::Command::new("git")
        .args(["cat-file", "-s", "HEAD:clip.mp4"])
        .current_dir(root)
        .output()
        .expect("cat-file");
    let blob_size: u64 = String::from_utf8_lossy(&blob.stdout)
        .trim()
        .parse()
        .expect("size");
    assert!(
        blob_size < 1024,
        "the blob must be a pointer, got {blob_size} bytes"
    );

    // …and the user's file is untouched.
    assert_eq!(std::fs::read(root.join("clip.mp4")).expect("read"), payload);
    // …and the object is in the store, addressable by its digest.
    let oid = &staging.uploads[0].oid;
    assert!(store.contains(oid, payload.len() as u64));
}

#[test]
fn a_checked_out_pointer_is_materialized_and_leaves_the_index_consistent() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_repo(root);

    // Seed the store with an object, and put only its pointer in the worktree —
    // exactly the state a fresh checkout of an LFS path leaves behind.
    let payload = vec![5u8; 150_000];
    let store = LfsStore::in_git_dir(root.join(".git"));
    store.ensure_layout().expect("layout");
    let (oid, size) = store
        .insert_streaming(std::io::Cursor::new(payload.clone()))
        .expect("insert");
    let pointer = Pointer::new(oid.clone(), size);
    std::fs::write(root.join("clip.mp4"), pointer.render()).expect("write pointer");

    let repo = git::repo::open(root, false).expect("open");
    let changes = git::commit::StagedChange {
        added: vec![PathBuf::from("clip.mp4")],
        ..Default::default()
    };
    commit(&repo, &changes, &std::collections::BTreeMap::new());

    let tracked = git::repo::tracked_paths(&repo).expect("tracked");
    let pending = stage::pending_smudges(root, &tracked).expect("pending");
    assert_eq!(pending.len(), 1, "the pointer file must be recognised");
    assert_eq!(pending[0].pointer.oid, oid);

    stage::materialize(&store, root, &pending[0]).expect("materialize");
    assert_eq!(
        std::fs::read(root.join("clip.mp4")).expect("read"),
        payload,
        "the worktree must now hold the real content"
    );

    // The index still records the pointer's stat, which would make status call
    // the file modified. Refreshing it is what keeps the invariant.
    git::repo::refresh_index_stat(&repo, &[PathBuf::from("clip.mp4")]).expect("refresh");
    let debug = std::process::Command::new("git")
        .args(["ls-files", "--debug", "clip.mp4"])
        .current_dir(root)
        .output()
        .expect("ls-files");
    let text = String::from_utf8_lossy(&debug.stdout);
    assert!(
        text.contains(&format!("size: {}", payload.len())),
        "the index must record the materialized size, got:\n{text}"
    );
}

#[test]
fn materializing_is_atomic_so_an_absent_object_never_truncates_the_pointer() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    let store = LfsStore::in_git_dir(root.join(".git"));
    store.ensure_layout().expect("layout");

    let pointer = Pointer::new("b".repeat(64), 999);
    std::fs::write(root.join("missing.bin"), pointer.render()).expect("write pointer");
    let smudge = stage::PendingSmudge {
        path: PathBuf::from("missing.bin"),
        pointer: pointer.clone(),
    };

    let err = stage::materialize(&store, root, &smudge).expect_err("must refuse");
    assert_eq!(err.code(), "integrity");
    // The pointer survives intact, so the transfer can simply be retried.
    assert_eq!(
        std::fs::read_to_string(root.join("missing.bin")).expect("read"),
        pointer.render()
    );
}

#[test]
fn attributes_written_for_one_profile_are_readable_as_git_attributes() {
    // The generated file must be something git itself accepts, not merely
    // something we can parse back.
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_repo(root);
    stage::ensure_attributes(root, &["*.mp4".into(), "*.iso".into()]).expect("write");

    let out = std::process::Command::new("git")
        .args(["check-attr", "filter", "--", "a.mp4", "b.iso", "c.txt"])
        .current_dir(root)
        .output()
        .expect("check-attr");
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("a.mp4: filter: lfs"), "got:\n{text}");
    assert!(text.contains("b.iso: filter: lfs"), "got:\n{text}");
    assert!(text.contains("c.txt: filter: unspecified"), "got:\n{text}");
}
