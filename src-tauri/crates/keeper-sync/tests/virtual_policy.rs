//! FR-331 against a real repository, a real index and the real `git` binary.
//!
//! The claim this file settles cannot be settled by a unit test: compiling a
//! virtualization policy over a worktree that holds a committed LFS pointer must
//! disturb nothing at all. In this story no materialization behaviour changes —
//! `VirtualPolicy` only answers a question, and nothing consults it yet — so a
//! `git status` that came back dirty, or a worktree byte that moved, would mean
//! the policy had reached for content it has no authority over (AD-123, FR-330).
//!
//! No network: the policy makes no request, and the object store here is local.

use std::path::Path;

use keeper_sync::git;
use keeper_sync::lfs::pointer::Pointer;
use keeper_sync::lfs::store::LfsStore;
use keeper_sync::lfs::virtual_policy::{VirtualPolicy, Virtualization, VIRTUAL_PATTERN_FILE};
use keeper_sync::profile::SyncProfile;
use keeper_sync::provenance::{Provenance, SyncSource};

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
    let mut p = SyncProfile::new("01JVIRT", "media", root, "https://git.invalid/r.git");
    p.lfs_threshold_bytes = 1024;
    p
}

fn commit(repo: &gix::Repository, changes: &git::commit::StagedChange) {
    let prov = Provenance::new("media", "dev", "01JDEV", "host", SyncSource::Cli);
    let sig = gix::actor::Signature {
        name: "t".into(),
        email: "t@example.invalid".into(),
        time: gix::date::Time::new(1_700_000_000, 0),
    };
    let root = repo
        .workdir()
        .expect("the fixture repository has a work tree");
    git::commit::stage_and_commit(
        repo,
        changes,
        &prov,
        &profile(root),
        &sig,
        &std::collections::BTreeMap::new(),
        None,
    )
    .expect("commit");
}

fn porcelain(root: &Path) -> String {
    let out = std::process::Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(root)
        .output()
        .expect("git status");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// A committed pointer standing alone in the worktree is the exact state a
/// virtual file will be in, and it is already a clean tree. Compiling a policy
/// that covers it must leave both facts standing: the index untouched and the
/// bytes on disk still the pointer, because authorizing virtualization is not
/// materializing, dehydrating or rewriting anything (FR-330, FR-331).
#[test]
fn compiling_a_policy_over_a_committed_pointer_leaves_the_tree_clean_and_the_bytes_alone() {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path();
    init_repo(root);

    // Seed the store with a real object and put only its pointer in the
    // worktree — the state a checkout of an LFS path leaves behind, and the one
    // a virtual file will live in.
    let payload = vec![9u8; 150_000];
    let store = LfsStore::in_git_dir(root.join(".git"));
    store.ensure_layout().expect("layout");
    let (oid, size) = store
        .insert_streaming(std::io::Cursor::new(payload.clone()))
        .expect("insert");
    let pointer = Pointer::new(oid, size);
    std::fs::create_dir_all(root.join("40-media")).expect("create the zone");
    std::fs::write(root.join("40-media/clip.mp4"), pointer.render()).expect("write pointer");

    // The policy is committed too: it is the repository's own answer, so it has
    // to survive the round trip like any other tracked file.
    std::fs::write(root.join(VIRTUAL_PATTERN_FILE), "40-media/**\n").expect("write policy");

    let repo = git::repo::open(root, false).expect("open");
    let changes = git::commit::StagedChange {
        added: vec![
            std::path::PathBuf::from("40-media/clip.mp4"),
            std::path::PathBuf::from(VIRTUAL_PATTERN_FILE),
        ],
        ..Default::default()
    };
    commit(&repo, &changes);

    assert_eq!(
        porcelain(root),
        "",
        "the fixture itself must start from a clean tree, or the assertion below proves nothing"
    );
    let before = std::fs::read(root.join("40-media/clip.mp4")).expect("read pointer");
    assert_eq!(
        before,
        pointer.render().as_bytes(),
        "the worktree holds the pointer text, not the content"
    );

    let policy = VirtualPolicy::compile(&profile(root)).expect("the committed policy compiles");
    assert_eq!(
        policy.resolve(Path::new("40-media/clip.mp4"), size),
        Virtualization::Virtual,
        "the committed policy covers the pointer's path"
    );

    // The two facts that matter, re-asserted after the policy has been compiled
    // and consulted: `git` itself sees nothing to report, and the file on disk
    // is byte-for-byte what it was.
    assert_eq!(
        porcelain(root),
        "",
        "compiling and resolving a policy must leave the tree clean: nothing is staged, \
         nothing is modified, nothing is untracked"
    );
    assert_eq!(
        std::fs::read(root.join("40-media/clip.mp4")).expect("read pointer"),
        pointer.render().as_bytes(),
        "and the worktree bytes are still the pointer — the policy deletes nothing"
    );
}
