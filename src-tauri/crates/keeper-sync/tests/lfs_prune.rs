//! `lfs::prune::plan` against a real index carrying pointer blobs.
//!
//! The planner's whole job is refusing to delete, so every case here is a
//! refusal except the first. The fixture reproduces the state the commit path
//! produces and the state prune must recognise: a POINTER blob in the index
//! paired with the WORKTREE file's stat, which is the arrangement
//! `lfs_pointer_stat.rs` proves reads as clean.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use gix::index::{entry::Flags, entry::Mode, entry::Stat, State};
use keeper_sync::git;
use keeper_sync::lfs::pointer::Pointer;
use keeper_sync::lfs::prune::{self, Releasable};
use keeper_sync::lfs::store::LfsStore;

struct Fixture {
    _dir: tempfile::TempDir,
    root: PathBuf,
    store: LfsStore,
    oid: String,
    size: u64,
    rela: PathBuf,
}

/// A committed LFS path: content in the worktree, pointer in the index, object
/// in the store — i.e. the duplication prune exists to remove.
fn fixture() -> Fixture {
    let dir = tempfile::tempdir().expect("tempdir");
    let root = dir.path().to_path_buf();
    std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(&root)
        .status()
        .expect("git init");

    let rela = PathBuf::from("video.bin");
    let payload = vec![7_u8; 1_048_576];
    std::fs::write(root.join(&rela), &payload).expect("write worktree");

    let store = LfsStore::in_git_dir(root.join(".git"));
    store.ensure_layout().expect("layout");
    let (oid, size) = store
        .insert_streaming(std::io::Cursor::new(payload))
        .expect("insert");

    let repo = git::repo::open(&root, false).expect("open");
    let blob = repo
        .write_blob(Pointer::new(oid.clone(), size).render().as_bytes())
        .expect("write blob")
        .detach();

    // Pointer OID, worktree stat — see the module docs.
    let metadata =
        gix::index::fs::Metadata::from_path_no_follow(&root.join(&rela)).expect("stat worktree");
    let stat = Stat::from_fs(&metadata).expect("stat convert");
    let mut index = State::new(repo.object_hash());
    index.dangerously_push_entry(stat, blob, Flags::empty(), Mode::FILE, "video.bin".into());
    index.sort_entries();
    gix::index::File::from_state(index, repo.index_path())
        .write(gix::index::write::Options::default())
        .expect("write index");

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
        .current_dir(&root)
        .status()
        .expect("git commit");

    Fixture {
        _dir: dir,
        root,
        store,
        oid,
        size,
        rela,
    }
}

fn plan_for(f: &Fixture, owed: &BTreeSet<String>) -> Vec<Releasable> {
    let repo = git::repo::open(&f.root, false).expect("reopen");
    let tracked = git::repo::tracked_paths(&repo).expect("tracked");
    prune::plan(&repo, &f.root, &f.store, &tracked, owed).expect("plan")
}

#[test]
fn an_uploaded_object_whose_content_is_still_in_the_worktree_is_releasable() {
    let f = fixture();
    let plan = plan_for(&f, &BTreeSet::new());
    assert_eq!(plan.len(), 1, "expected exactly one candidate: {plan:?}");
    assert_eq!(plan[0].oid, f.oid);
    assert_eq!(plan[0].size, f.size);
    assert_eq!(plan[0].rebuildable_from, f.rela);
}

#[test]
fn an_object_the_journal_still_owes_is_never_released() {
    // The engine re-cleans an object it still owes an upload for — verified
    // against a live sync, where a deleted object came back within a minute.
    // Pruning one is therefore futile as well as unsafe.
    let f = fixture();
    let owed: BTreeSet<String> = [f.oid.clone()].into_iter().collect();
    assert!(plan_for(&f, &owed).is_empty());
}

#[test]
fn a_pointer_only_worktree_is_never_released() {
    // The inverse case: here the store object is the ONLY local copy, and
    // releasing it would leave the remote as the sole holder of content this
    // machine's index claims to have.
    let f = fixture();
    std::fs::write(
        f.root.join(&f.rela),
        Pointer::new(f.oid.clone(), f.size).render(),
    )
    .expect("write pointer text");
    assert!(plan_for(&f, &BTreeSet::new()).is_empty());
}

#[test]
fn a_modified_worktree_file_is_never_released() {
    // Its bytes no longer rebuild the recorded object, so the store copy is
    // once again the only one that can satisfy the committed pointer.
    let f = fixture();
    std::fs::write(f.root.join(&f.rela), vec![9_u8; 2_097_152]).expect("modify");
    assert!(plan_for(&f, &BTreeSet::new()).is_empty());
}

#[test]
fn a_deleted_worktree_file_is_never_released() {
    let f = fixture();
    std::fs::remove_file(f.root.join(&f.rela)).expect("remove");
    assert!(plan_for(&f, &BTreeSet::new()).is_empty());
}

#[test]
fn a_worktree_file_of_the_right_length_but_different_bytes_is_still_refused_after_release() {
    // Guards the ordering the planner relies on: `store.contains` is checked
    // before the worktree, so an object already released is not re-proposed on
    // the next cycle just because the file is still there.
    let f = fixture();
    let plan = plan_for(&f, &BTreeSet::new());
    assert_eq!(prune::release(&f.store, &plan).expect("release"), f.size);
    assert!(plan_for(&f, &BTreeSet::new()).is_empty());
}

#[test]
fn an_ordinary_non_lfs_file_is_ignored_entirely() {
    let f = fixture();
    let plain = Path::new("readme.txt");
    std::fs::write(f.root.join(plain), b"not an LFS path").expect("write plain");
    std::process::Command::new("git")
        .args(["add", "readme.txt"])
        .current_dir(&f.root)
        .status()
        .expect("git add");

    let plan = plan_for(&f, &BTreeSet::new());
    assert!(
        plan.iter().all(|r| r.rebuildable_from != plain),
        "a plain file must never appear in a prune plan: {plan:?}",
    );
}
