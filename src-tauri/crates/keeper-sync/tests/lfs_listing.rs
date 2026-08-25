//! FR-336 against a real repository, a real index and the real `git` binary.
//!
//! The claim this file settles cannot be settled by a unit test. Every unit test
//! of the listing hands the rule a file it wrote itself, which proves the rule
//! and says nothing about whether the state it describes is the state a real
//! checkout of an LFS path actually leaves behind. That state is a *committed*
//! blob whose content is the pointer, a worktree holding those same bytes, and a
//! `git status` that reports nothing — three facts only a real index and the
//! real `git` binary can produce together.
//!
//! So: a committed pointer naming four mebibytes, a twelve-byte Markdown file
//! beside it, and then the same numbers asked for three ways — through
//! [`keeper_sync::browse`], which has no repository, and through
//! `Engine::lfs_files`, which reads the index. If either reports about 130
//! bytes, this story did nothing.
//!
//! No network: the object store is local and nothing is fetched or pushed.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use keeper_sync::browse::{self, BrowseListing, EntrySyncStatus, PendingView};
use keeper_sync::engine::Engine;
use keeper_sync::exclude::ExcludeSet;
use keeper_sync::git;
use keeper_sync::lfs::listing::LfsFileState;
use keeper_sync::lfs::pointer::Pointer;
use keeper_sync::lfs::store::LfsStore;
use keeper_sync::platform::{SyncPlatform, TestPlatform};
use keeper_sync::profile::SyncProfile;
use keeper_sync::provenance::{Provenance, SyncSource};

/// What the pointer stands for. Four mebibytes, so the honest number is
/// unmistakable beside the ~130 bytes on disk.
const CONTENT_BYTES: u64 = 4 * 1024 * 1024;

/// The plain file beside it, whose answer must not move at all.
const PLAIN_BYTES: &[u8] = b"hello, disk\n";

/// `false` when this machine has no usable `git`, in which case there is no
/// index to read and nothing here is falsifiable — the same skip
/// `tests/blobs_over_threshold.rs` makes.
fn init_repo(root: &Path) -> bool {
    std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(root)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn profile(root: &Path) -> SyncProfile {
    let mut p = SyncProfile::new("01JLISTING", "media", root, "https://git.invalid/r.git");
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
        &BTreeMap::new(),
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

/// The state a checkout of an unmaterialized LFS path leaves behind: a real
/// object in the store, its pointer committed, and only the pointer on disk.
///
/// Returns the pointer so every assertion below compares against the same oid
/// and size the fixture actually wrote, rather than against a literal that could
/// drift from it.
fn seed(root: &Path) -> Pointer {
    let store = LfsStore::in_git_dir(root.join(".git"));
    store.ensure_layout().expect("store layout");
    let (oid, size) = store
        .insert_streaming(std::io::Cursor::new(vec![9u8; CONTENT_BYTES as usize]))
        .expect("insert the real object");
    let pointer = Pointer::new(oid, size);
    std::fs::write(root.join("clip.mp4"), pointer.render()).expect("write the pointer text");
    std::fs::write(root.join("a.md"), PLAIN_BYTES).expect("write the plain file");

    let repo = git::repo::open(root, false).expect("open the fixture repository");
    commit(
        &repo,
        &git::commit::StagedChange {
            added: vec![
                std::path::PathBuf::from("clip.mp4"),
                std::path::PathBuf::from("a.md"),
            ],
            ..Default::default()
        },
    );
    pointer
}

/// The one claim only a real index and a real `git` can settle: a path whose
/// worktree bytes are the committed pointer reports the pointer's size and oid,
/// the plain file beside it is untouched, and neither reading disturbs the tree.
#[test]
fn a_committed_pointer_reports_its_own_size_through_browse_and_through_the_engine() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let pointer = seed(root);

    // The fixture must start clean, or the assertion at the end proves nothing:
    // a tree that was already dirty cannot show that reading left it alone.
    assert_eq!(
        porcelain(root),
        "",
        "the committed pointer and the plain file are both clean to `git` itself"
    );
    let on_disk = std::fs::metadata(root.join("clip.mp4"))
        .expect("stat the pointer file")
        .len();
    assert!(
        on_disk < 200 && on_disk != CONTENT_BYTES,
        "the worktree really does hold only pointer text: {on_disk} bytes"
    );

    // --- Through `browse`, which never opens a repository -------------------
    let excludes = ExcludeSet::new(&[]).expect("the builtin corpus compiles");
    let pending = PendingView::Known(BTreeMap::new());
    let BrowseListing::Listed(dir) =
        browse::browse(&profile(root), "", &excludes, &pending).expect("no refusal")
    else {
        panic!("expected a listing");
    };
    let rows: Vec<(&str, Option<u64>, Option<&str>, &EntrySyncStatus)> = dir
        .entries
        .iter()
        .map(|entry| {
            (
                entry.name.as_str(),
                entry.size_bytes,
                entry.lfs_oid.as_deref(),
                &entry.sync,
            )
        })
        .collect();
    assert_eq!(
        rows,
        vec![
            (
                "a.md",
                Some(PLAIN_BYTES.len() as u64),
                None,
                &EntrySyncStatus::Synced
            ),
            (
                "clip.mp4",
                Some(CONTENT_BYTES),
                Some(pointer.oid.as_str()),
                &EntrySyncStatus::Virtual
            ),
        ],
        "the virtual path reports the four mebibytes the pointer names and the \
         oid that says why; the plain file answers exactly as it did before"
    );

    // --- Through the engine, which reads the index --------------------------
    let data = tempfile::tempdir().expect("data dir");
    let platform = Arc::new(TestPlatform::new(data.path()));
    // A machine with no usable git cannot host the engine at all (AD-41); skip
    // rather than fake it, exactly as this crate's other integration tests do.
    let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
        return;
    };
    let p = profile(root);
    engine.upsert_profile(&p).expect("register the profile");

    let files = engine.lfs_files(&p.id).expect("list the LFS paths");
    assert_eq!(
        files.len(),
        1,
        "only the pointer is an LFS path; the 12-byte Markdown file is a plain \
         blob and has no row: {files:?}"
    );
    assert_eq!(files[0].path, "clip.mp4");
    assert_eq!(
        files[0].size_bytes, CONTENT_BYTES,
        "the engine's number is the same number browse reported"
    );
    assert_eq!(files[0].oid, pointer.oid);
    assert_eq!(
        files[0].state,
        LfsFileState::Virtual,
        "the worktree holds the pointer, so this machine does not hold the content"
    );
    assert_eq!(
        files[0].materialized_at_ms, None,
        "nothing was ever materialized here, so the ledger has no row and says so"
    );
    assert!(!files[0].pinned);
    assert!(
        files[0].mtime_ms.is_some(),
        "the file exists, so it has an mtime"
    );

    // --- And nothing that just ran wrote anything ---------------------------
    assert_eq!(
        porcelain(root),
        "",
        "listing is looking: no stage, no modification, no untracked file"
    );
    assert_eq!(
        std::fs::read(root.join("clip.mp4")).expect("re-read the pointer"),
        pointer.render().as_bytes(),
        "and the worktree bytes are still the pointer, byte for byte"
    );
    assert_eq!(
        std::fs::read(root.join("a.md")).expect("re-read the plain file"),
        PLAIN_BYTES
    );
}

/// A listing must not be the thing that makes a repository.
///
/// `Engine::open_repo` — the obvious way to get a `gix::Repository` — is the
/// engine's *materialize* path: for a folder that is not a repository yet it
/// clones over the network, or, for a folder with content already in it, adopts
/// in place. Either would mean that an operator or an agent running
/// `keeper-syncd ls-files` to find out which paths are virtual pulled gigabytes
/// down, or silently turned a plain folder into a keeper repository — from a
/// verb whose whole promise is that it only looks.
///
/// The fixture is a non-empty directory with no `.git`, which is the adopt case:
/// under `open_repo` this call creates `.git`, attaches the remote and runs the
/// sparse-cone reconciliation over the working tree. The assertion is therefore
/// on the *absence* of that directory afterwards, not only on the answer.
#[test]
fn listing_a_folder_that_is_not_a_repository_yet_creates_nothing() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    std::fs::write(root.join("already-here.txt"), b"content").expect("seed a plain file");

    let data = tempfile::tempdir().expect("data dir");
    let platform = Arc::new(TestPlatform::new(data.path()));
    let Ok(engine) = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>) else {
        return;
    };
    let p = profile(root);
    engine.upsert_profile(&p).expect("register the profile");

    assert_eq!(
        engine.lfs_files(&p.id).expect("a listing, not a refusal"),
        Vec::new(),
        "a folder with no index has no LFS paths, and saying so is the answer"
    );
    assert!(
        !root.join(".git").exists(),
        "and the listing did not adopt the folder to find out"
    );
    assert_eq!(
        std::fs::read(root.join("already-here.txt")).expect("still there"),
        b"content",
        "nor touched what was in it"
    );
}
