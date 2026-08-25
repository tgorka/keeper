//! FR-332 and FR-333 against a real repository, a real index, a real open
//! descriptor and the real `git` binary (Story 56.4).
//!
//! This is the verb where a bug destroys the user's only copy (AD-125), so the
//! claims that live here are the ones no unit test can settle:
//!
//! 1. **The worktree gets the committed blob, byte for byte.** Asserted against
//!    the bytes read back out of the index rather than against a re-rendering,
//!    because a re-rendering is exactly the bug.
//! 2. **It was a `rename`, not a truncation.** The path's inode changes, and a
//!    [`std::fs::File`] held open across the call still reads the content it
//!    opened. A `set_len` would pass a bytes-on-disk assertion and fail this
//!    one.
//! 3. **The tree stays clean.** Replacing four mebibytes with a ~130-byte
//!    pointer leaves the index entry's cached stat describing something else, so
//!    `git status` calls the path modified; `refresh_index_stat` is the repair
//!    and only the real `git status --porcelain` can say whether it worked.
//! 4. **Every refusal writes nothing**, byte for byte, and is distinguishable
//!    *by type*.
//!
//! **No `filter=lfs` rule is registered in the fixture, on purpose** — the same
//! decision `tests/materialize_entry.rs` documents. `git status` must decide
//! from the refreshed index stat, so a broken refresh surfaces here as ` M`
//! rather than as a crash inside a clean filter that would have hidden it
//! (DW-121).
//!
//! **Loopback only, and no resolver anywhere.** Most of the file's remote is a
//! *filesystem* remote whose LFS store this fixture seeds directly, which is a
//! genuine per-object, size-verified proof (`lfs::local::remote_store`) rather
//! than a mocked one — so the happy path really does take the proof
//! `Engine::remote_serves` requires. Three tests need a real socket instead,
//! and every one of them is `127.0.0.1` on a port either fixed at `1` or chosen
//! by the kernel: an instant connection refusal for "the server cannot be
//! asked", a bound-but-never-accepted `TcpListener` for a connect that
//! completes and then hangs, and `serve_one_batch` for the one test that needs
//! the server to actually answer. No name is ever resolved, so no resolver —
//! hijacking, blackholing or otherwise — can change what these tests observe.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use keeper_sync::db;
use keeper_sync::engine::Engine;
use keeper_sync::error::SyncError;
use keeper_sync::git;
use keeper_sync::lfs::hydrate::ContentRefusal;
use keeper_sync::lfs::pointer::Pointer;
use keeper_sync::lfs::stage;
use keeper_sync::lfs::store::LfsStore;
use keeper_sync::platform::{OpenFileState, SyncPlatform, TestPlatform};
use keeper_sync::profile::{LfsMode, SyncProfile};
use keeper_sync::provenance::{Provenance, SyncSource};

/// What the pointer stands for. Four mebibytes, so a worktree still holding
/// content is unmistakable next to ~130 bytes of pointer text.
const CONTENT_BYTES: u64 = 4 * 1024 * 1024;

const PROFILE_ID: &str = "01JDEHYDRATE";

/// `false` when this machine has no usable `git`, in which case there is no
/// index to read and nothing here is falsifiable — the same skip
/// `tests/materialize_entry.rs` makes.
fn init_repo(root: &Path) -> bool {
    std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(root)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// The fixture profile, whose remote is a **filesystem** remote: that is what
/// lets the release take its per-object proof with no server anywhere.
///
/// `PointerOnly`, because that is the only mode a release is meaningful in. A
/// folder in the default mode re-materializes anything holding a pointer on the
/// next pass, so `dehydrate_entry` refuses it by name — see
/// `a_folder_that_keeps_everything_is_refused_by_name`, which is the one test
/// here that deliberately leaves the default in place.
fn profile(root: &Path, remote: &Path) -> SyncProfile {
    let mut p = SyncProfile::new(
        PROFILE_ID,
        "media",
        root,
        remote.to_string_lossy().into_owned(),
    );
    p.lfs_threshold_bytes = 1024;
    p.lfs_mode = LfsMode::PointerOnly;
    p
}

fn commit(repo: &gix::Repository, remote: &Path, changes: &git::commit::StagedChange) {
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
        &profile(root, remote),
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

/// The state a materialized LFS path is really in: the **pointer** committed as
/// the index's blob, the **content** in the worktree, and the entry's stat
/// describing the content — which is what a materialization leaves behind.
///
/// The object is seeded into the *remote's* store rather than this machine's,
/// deliberately: `dehydrate` must not require the local store, because
/// `lfs_prune_local` deletes the local copy precisely when the worktree holds
/// the content. A fixture that seeded it locally would let a store precondition
/// slip in unnoticed.
///
/// Returns the pointer and the content, so every assertion compares against
/// what the fixture actually wrote rather than a literal that could drift.
fn seed(root: &Path, remote: &Path) -> (Pointer, Vec<u8>) {
    let content = vec![9u8; CONTENT_BYTES as usize];
    let (oid, size) = LfsStore::in_git_dir(remote)
        .insert_streaming(std::io::Cursor::new(content.clone()))
        .expect("seed the remote's own LFS store");
    let pointer = Pointer::new(oid, size);

    // Committed while the worktree holds only the pointer, so the index's blob
    // IS the pointer — the invariant this whole epic rests on.
    std::fs::write(root.join("clip.mp4"), pointer.render()).expect("check out the pointer");
    {
        let repo = git::repo::open(root, false).expect("open the fixture repository");
        commit(
            &repo,
            remote,
            &git::commit::StagedChange {
                added: vec![PathBuf::from("clip.mp4")],
                ..Default::default()
            },
        );
    }

    // ...and then the content lands, exactly as `materialize_entry` leaves it:
    // real bytes on disk, the entry's stat refreshed to match.
    std::fs::write(root.join("clip.mp4"), &content).expect("materialize the content");
    {
        let repo = git::repo::open(root, false).expect("reopen");
        git::repo::refresh_index_stat(&repo, &[PathBuf::from("clip.mp4")]).expect("re-stat");
    }
    (pointer, content)
}

/// Stamp `.git/index` ten seconds into the future, with no keeper code
/// involved.
///
/// git's **racy-clean** rule: an index entry whose mtime is not strictly older
/// than the index file's own mtime is not trusted on its cached stat, so git
/// falls back to comparing content and reports ` M` for the second a release
/// lands in, however correct the refreshed stat is. `stage::dehydrate` writes
/// the file and `refresh_index_stat` writes the index microseconds later, so
/// that second is unavoidable.
///
/// Moving the index file's timestamp is what makes the assertion below decide
/// on the **stat** rather than on a content comparison — and it is deliberately
/// a filesystem call rather than `Engine::rescan`: pressing keeper's repair
/// button would re-stat every entry in the repository and the assertion would
/// then pass with `dehydrate_entry`'s own `refresh_index_stat` deleted.
fn stamp_index_forward(root: &Path) {
    let when = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
    std::fs::File::options()
        .write(true)
        .open(root.join(".git/index"))
        .and_then(|index| index.set_modified(when))
        .expect("stamp the index forward");
}

/// The engine, registered against the fixture, or `None` on a machine that
/// cannot host one (AD-41).
///
/// Returns the platform too: the open-file capability and the ledger this test
/// inspects both live behind it.
fn engine_for(root: &Path, remote: &Path, data: &Path) -> Option<(Engine, Arc<TestPlatform>)> {
    engine_with(profile(root, remote), data)
}

/// [`engine_for`], for a test that needs the profile altered first.
fn engine_with(profile: SyncProfile, data: &Path) -> Option<(Engine, Arc<TestPlatform>)> {
    let platform = Arc::new(TestPlatform::new(data));
    let engine = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>).ok()?;
    engine
        .upsert_profile(&profile)
        .expect("register the profile");
    Some((engine, platform))
}

/// A second connection to the very `sync.db` the engine wrote.
fn ledger_conn(platform: &TestPlatform) -> rusqlite::Connection {
    db::open(&platform.data_dir().expect("data dir")).expect("open sync.db")
}

/// Record that content for `path` is here — the fact a release must retract.
fn remember(platform: &TestPlatform, path: &str) {
    db::remember_materialized(&ledger_conn(platform), PROFILE_ID, path, 1_700_000_000_000)
        .expect("record the materialization");
}

fn ledger_paths(platform: &TestPlatform) -> Vec<String> {
    db::materialized_rows(&ledger_conn(platform), PROFILE_ID)
        .expect("read the ledger")
        .into_iter()
        .map(|row| row.path)
        .collect()
}

/// Pin `path` through the real writer (Story 56.5's `Engine::pin_entry`).
///
/// It was a raw `UPDATE pinned = 1` while FR-334 had no writer, which meant
/// this file asserted a refusal against a column nothing in production could
/// set. Going through the verb an operator actually has is what makes the
/// refusal below reachable rather than hypothetical — and `pin_entry` does not
/// require the content to be present, so it costs the fixture nothing.
fn pin(engine: &Engine, path: &str) {
    let done = engine
        .pin_entry(PROFILE_ID, path, true)
        .expect("the fixture path is tracked, so a pin is recorded");
    assert!(done.pinned, "the verb reports the state it wrote");
}

/// The committed blob's own bytes, read straight out of the index.
///
/// The comparison target for the byte-exactness claim. Read through
/// `stage::indexed_pointer_blob` because that is the accessor under test — and
/// captured *before* the release, so what it is compared against is what git
/// held all along.
fn committed_blob(root: &Path) -> Vec<u8> {
    let repo = git::repo::open(root, false).expect("open");
    stage::indexed_pointer_blob(&repo, Path::new("clip.mp4"))
        .expect("the index records a pointer for this path")
        .1
}

#[cfg(unix)]
fn inode(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::symlink_metadata(path).expect("stat").ino()
}

// ---------------------------------------------------------------------------
// The release
// ---------------------------------------------------------------------------

/// The happy release: the committed blob byte for byte, a new inode, the ledger
/// row gone, and a clean `git status`.
///
/// Four assertions, each of which fails for a different real bug:
///
/// * **the bytes** — a re-rendered pointer would differ for any non-canonical
///   committed blob, and equal it for a canonical one, so this is asserted
///   against what the index actually holds;
/// * **the inode** — a `set_len` would leave it unchanged;
/// * **the ledger** — a stale row makes the listing report content that is gone
///   and invites 56.5's sweep to release it again;
/// * **`git status`** — without the one-path `refresh_index_stat` the entry's
///   cached stat still describes four mebibytes and every status walk pays a
///   full content comparison for it, forever.
#[tokio::test]
#[cfg(unix)]
async fn a_release_publishes_the_committed_blob_and_the_tree_stays_clean() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let remote = tempfile::tempdir().expect("remote");
    let (pointer, content) = seed(root, remote.path());

    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, platform)) = engine_for(root, remote.path(), data.path()) else {
        return;
    };
    remember(&platform, "clip.mp4");

    let blob = committed_blob(root);
    let target = root.join("clip.mp4");
    assert_eq!(
        std::fs::read(&target).expect("read"),
        content,
        "the fixture starts materialized, or there is nothing to release"
    );
    let before = inode(&target);

    let done = engine
        .dehydrate_entry(PROFILE_ID, "clip.mp4")
        .await
        .expect("the remote provably holds the object, so this cannot refuse");

    assert_eq!(done.path, "clip.mp4");
    assert_eq!(done.oid, pointer.oid);
    assert_eq!(
        done.size_bytes, CONTENT_BYTES,
        "the honest number is the pointer's: the bytes this machine gave back"
    );

    assert_eq!(
        std::fs::read(&target).expect("read the released file"),
        blob,
        "byte for byte the blob git committed — not a re-rendering of it"
    );
    assert_ne!(
        inode(&target),
        before,
        "the inode changed, which is what a rename does and a truncation does not"
    );
    assert!(
        ledger_paths(&platform).is_empty(),
        "the ledger no longer claims this machine holds the content"
    );

    // And the real `git` binary agrees. The stamp is the fixture's, not
    // keeper's: what is under test is the one-path `refresh_index_stat`, and
    // with it deleted this reports ` M`.
    stamp_index_forward(root);
    assert_eq!(
        porcelain(root),
        "",
        "pointer blob, worktree stat, clean status — the invariant the design \
         rests on, in both directions"
    );
}

/// A reader that opened the file before the release finishes reading the file
/// it opened.
///
/// `rename(2)` leaves the old inode alone; `set_len` or `truncate(true)` would
/// pull the content out from under this descriptor, and on an `mmap`ed file
/// would deliver SIGBUS. This is the acceptance criterion, exercised with a real
/// descriptor across a real call.
#[tokio::test]
#[cfg(unix)]
async fn a_reader_holding_the_file_open_still_sees_the_content() {
    use std::io::Read as _;

    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let remote = tempfile::tempdir().expect("remote");
    let (_pointer, content) = seed(root, remote.path());

    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, _platform)) = engine_for(root, remote.path(), data.path()) else {
        return;
    };

    let target = root.join("clip.mp4");
    let mut reader = std::fs::File::open(&target).expect("a reader gets there first");

    engine
        .dehydrate_entry(PROFILE_ID, "clip.mp4")
        .await
        .expect("released");

    let mut held = Vec::new();
    reader
        .read_to_end(&mut held)
        .expect("the descriptor is still valid");
    assert_eq!(
        held, content,
        "the process reading it never noticed, which is the whole point of a rename"
    );
    assert_eq!(
        std::fs::read(&target).expect("read").len(),
        committed_blob(root).len(),
        "while the path itself now holds nothing but its pointer"
    );
}

// ---------------------------------------------------------------------------
// The refusals that need the worktree read
// ---------------------------------------------------------------------------

/// A same-length edit with the original mtime restored is refused.
///
/// The one `Modified` case a stat comparison cannot catch, and therefore the
/// only one worth writing: same length, same mtime, different bytes.
/// `is_false_modification`'s condition (a) — the tree's existing predicate —
/// would say this path agrees with the index, and the release would then destroy
/// somebody's edit with a green suite. Content identity is what refuses it.
#[tokio::test]
async fn a_same_length_edit_with_the_original_mtime_is_refused_as_modified() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let remote = tempfile::tempdir().expect("remote");
    seed(root, remote.path());

    let target = root.join("clip.mp4");
    let when = std::fs::symlink_metadata(&target)
        .expect("stat")
        .modified()
        .expect("mtime");
    // Different bytes, identical length — and the mtime put back, so that no
    // stat comparison anywhere in the chain can be the thing that refuses
    // this. That cuts both ways now: `is_false_modification`'s condition (a)
    // would have called the path clean, and `stage::dehydrate`'s own guard
    // would have caught a *moved* mtime before it published. Restoring it
    // leaves content identity as the only thing left that can say no, which is
    // the claim.
    let edited = vec![1u8; CONTENT_BYTES as usize];
    std::fs::write(&target, &edited).expect("edit in place");
    std::fs::File::options()
        .write(true)
        .open(&target)
        .and_then(|file| file.set_modified(when))
        .expect("put the mtime back");

    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, platform)) = engine_for(root, remote.path(), data.path()) else {
        return;
    };
    remember(&platform, "clip.mp4");

    let err = engine
        .dehydrate_entry(PROFILE_ID, "clip.mp4")
        .await
        .expect_err("keeper must not delete an edit");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::Modified { path }) if path == "clip.mp4"
        ),
        "distinguishable BY TYPE, not by message: {err}"
    );
    // The transport classification, pinned ONCE here rather than in every
    // refusal test: `code()` and `needs_user_action()` never read
    // `ContentRefusal`, so they hold for every variant and repeating them would
    // read as coverage of the release refusals while proving nothing about
    // them. What carries that load is the `matches!` above.
    assert_eq!(err.code(), "refused");
    assert!(!err.needs_user_action());

    assert_eq!(
        std::fs::read(&target).expect("read"),
        edited,
        "byte for byte what the user left there"
    );
    assert_eq!(
        ledger_paths(&platform),
        vec!["clip.mp4".to_owned()],
        "and the ledger row survives, because nothing was released"
    );
}

/// Something has it open, so the content stays.
#[tokio::test]
async fn a_file_the_machine_says_is_open_is_refused() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let remote = tempfile::tempdir().expect("remote");
    let (_pointer, content) = seed(root, remote.path());

    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, platform)) = engine_for(root, remote.path(), data.path()) else {
        return;
    };
    platform.set_open_file_state(OpenFileState::Open);

    let err = engine
        .dehydrate_entry(PROFILE_ID, "clip.mp4")
        .await
        .expect_err("something is reading it");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::Open { path }) if path == "clip.mp4"
        ),
        "got: {err}"
    );
    assert_eq!(
        std::fs::read(root.join("clip.mp4")).expect("read"),
        content,
        "nothing was written"
    );
}

/// The machine cannot tell whether it is open, so the content stays — the answer
/// every real host gives today, and the one that makes this verb fail-closed.
#[tokio::test]
async fn a_machine_that_cannot_tell_refuses_rather_than_guessing() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let remote = tempfile::tempdir().expect("remote");
    let (_pointer, content) = seed(root, remote.path());

    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, platform)) = engine_for(root, remote.path(), data.path()) else {
        return;
    };
    // What `SyncPlatform::open_file_state`'s own body answers, and therefore
    // what `LinuxPlatform` and the Tauri shell answer: there is no race-free
    // primitive available, so the trait says so instead of guessing (AD-125).
    platform.set_open_file_state(OpenFileState::Unknown);

    let err = engine
        .dehydrate_entry(PROFILE_ID, "clip.mp4")
        .await
        .expect_err("an unanswerable question is not permission");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::OpenUnknown { path }) if path == "clip.mp4"
        ),
        "its own variant, so an operator can tell \"close it\" from \"keeper \
         cannot tell yet\": {err}"
    );
    assert_eq!(
        std::fs::read(root.join("clip.mp4")).expect("read"),
        content,
        "nothing was written"
    );
}

/// The remote's store does not hold the object, so the local copy is the only
/// copy and it stays.
#[tokio::test]
async fn a_remote_that_does_not_hold_the_object_refuses() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let remote = tempfile::tempdir().expect("remote");
    let (pointer, content) = seed(root, remote.path());
    // Empty the remote's store, leaving the pointer committed: the exact state
    // the DW-140 audit found on two real repositories, where a folder reported
    // a clean sync while the server held nothing.
    std::fs::remove_file(LfsStore::in_git_dir(remote.path()).object_path(&pointer.oid))
        .expect("empty the remote store");

    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, _platform)) = engine_for(root, remote.path(), data.path()) else {
        return;
    };

    let err = engine
        .dehydrate_entry(PROFILE_ID, "clip.mp4")
        .await
        .expect_err("these bytes exist nowhere else");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::UnprovenOnRemote { path }) if path == "clip.mp4"
        ),
        "got: {err}"
    );
    assert_eq!(
        std::fs::read(root.join("clip.mp4")).expect("read"),
        content,
        "nothing was written"
    );
}

/// A remote that cannot be *asked* refuses too, and does **not** raise a
/// transient fault.
///
/// This is the load-bearing distinction: `BatchClient` maps an unreachable host
/// to `SyncError::Network`, which is `Retriability::Transient`, and 56.5 will
/// drive this verb from a sweep. A scheduler is entitled to re-drive a transient
/// fault — against a deletion. So every failure to ask collapses into the one
/// sentence that matters instead.
#[tokio::test]
async fn a_remote_that_cannot_be_asked_refuses_instead_of_raising_network() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let remote = tempfile::tempdir().expect("remote");
    let (_pointer, content) = seed(root, remote.path());

    let data = tempfile::tempdir().expect("data dir");
    // Loopback, port 1: nothing listens there, so the connect is refused by the
    // kernel immediately. No name is resolved, which is the point — a `.invalid`
    // host is only unreachable as long as no resolver on the machine hijacks
    // NXDOMAIN, and a blackholing one would make this test pay a connect
    // timeout instead of finishing.
    let mut unreachable = profile(root, remote.path());
    unreachable.remote_url = "http://127.0.0.1:1/r.git".to_owned();
    let Some((engine, _platform)) = engine_with(unreachable, data.path()) else {
        return;
    };

    let err = engine
        .dehydrate_entry(PROFILE_ID, "clip.mp4")
        .await
        .expect_err("nothing was established, so nothing may be deleted");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::UnprovenOnRemote { path }) if path == "clip.mp4"
        ),
        "a refusal, NOT a network fault a scheduler would retry: {err}"
    );
    assert_eq!(
        std::fs::read(root.join("clip.mp4")).expect("read"),
        content,
        "nothing was written"
    );
}

/// A pinned path keeps its content and its row.
#[tokio::test]
async fn a_pinned_path_is_refused_and_keeps_its_ledger_row() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let remote = tempfile::tempdir().expect("remote");
    let (_pointer, content) = seed(root, remote.path());

    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, platform)) = engine_for(root, remote.path(), data.path()) else {
        return;
    };
    pin(&engine, "clip.mp4");

    let err = engine
        .dehydrate_entry(PROFILE_ID, "clip.mp4")
        .await
        .expect_err("a pin is the one instruction a release may not weigh");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::Pinned { path }) if path == "clip.mp4"
        ),
        "got: {err}"
    );
    assert_eq!(
        std::fs::read(root.join("clip.mp4")).expect("read"),
        content,
        "nothing was written"
    );
    assert_eq!(
        ledger_paths(&platform),
        vec!["clip.mp4".to_owned()],
        "and the row that carries the pin is still there"
    );
}

/// A path already holding its pointer is refused by name, and nothing changes.
///
/// The door reports this one as a no-op at exit 0 — see `keeper-syncd`'s
/// `nothing_to_release` — but the engine still returns it as a typed refusal,
/// because `Ok` here would make "released something" and "found nothing to
/// release" indistinguishable in the return type.
#[tokio::test]
#[cfg(unix)]
async fn a_path_already_holding_its_pointer_is_refused_by_name() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let remote = tempfile::tempdir().expect("remote");
    let (pointer, _content) = seed(root, remote.path());

    let target = root.join("clip.mp4");
    std::fs::write(&target, pointer.render()).expect("put the pointer back");
    let before = inode(&target);

    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, _platform)) = engine_for(root, remote.path(), data.path()) else {
        return;
    };

    let err = engine
        .dehydrate_entry(PROFILE_ID, "clip.mp4")
        .await
        .expect_err("there is nothing here to release");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::AlreadyPointer { path }) if path == "clip.mp4"
        ),
        "got: {err}"
    );
    assert_eq!(
        std::fs::read(&target).expect("read"),
        pointer.render().into_bytes(),
        "the bytes are as they were"
    );
    assert_eq!(
        inode(&target),
        before,
        "and nothing was even renamed over it"
    );
}

/// A folder in the default mode is refused by name, and it keeps its content.
///
/// `Materialize` re-materializes any tracked path inside the cone whose
/// worktree holds a pointer — from this machine's object store, or by queueing
/// a download. Releasing one path there gives back nothing and costs a fetch,
/// and a sweep driving this verb would fight the scan loop forever. So it is
/// refused before anything is read, and the sentence names the folder's setting
/// rather than the file.
///
/// Deliberately the one test here that does not use the fixture's
/// `PointerOnly`: what is under test is the mode every profile starts in.
#[tokio::test]
async fn a_folder_that_keeps_everything_is_refused_by_name() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let remote = tempfile::tempdir().expect("remote");
    let (_pointer, content) = seed(root, remote.path());

    let data = tempfile::tempdir().expect("data dir");
    let mut keeps = profile(root, remote.path());
    keeps.lfs_mode = LfsMode::Materialize;
    assert_eq!(
        SyncProfile::new("01J", "x", root, "https://x/y.git").lfs_mode,
        LfsMode::Materialize,
        "and this is the mode every folder starts in, which is why the refusal \
         has to exist"
    );
    let Some((engine, platform)) = engine_with(keeps, data.path()) else {
        return;
    };
    remember(&platform, "clip.mp4");

    let err = engine
        .dehydrate_entry(PROFILE_ID, "clip.mp4")
        .await
        .expect_err("this folder would have the content back on the next pass");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::AlwaysMaterializes { profile })
                if profile == "media"
        ),
        "got: {err}"
    );
    assert_eq!(
        std::fs::read(root.join("clip.mp4")).expect("read"),
        content,
        "nothing was written"
    );
    assert_eq!(
        ledger_paths(&platform),
        vec!["clip.mp4".to_owned()],
        "and the ledger still says what is true"
    );
}

/// Asking again about an already-released path **repairs** its index entry.
///
/// The state this covers is real: `stage::dehydrate` renames the pointer into
/// place and `refresh_index_stat` runs microseconds later, so a crash, an
/// `index.lock` somebody else holds, a read-only `.git` or an ENOSPC between
/// the two leaves the worktree holding a pointer while the entry's cached stat
/// still describes four mebibytes — and `git status` calls the path modified
/// for as long as that lasts.
///
/// Nothing else can fix it. `git::repo::repair_index_stat` — keeper's
/// whole-repository repair button — only refreshes entries whose worktree file
/// is **not** a pointer, so it steps straight over this one. Asking to release
/// the path again is therefore the only repair there is, which is why the
/// `AlreadyPointer` arm re-stats before it refuses, exactly as
/// `materialize_entry`'s `AlreadyHeld` arm does.
#[tokio::test]
async fn asking_again_repairs_a_released_paths_index_entry() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let remote = tempfile::tempdir().expect("remote");
    let (pointer, _content) = seed(root, remote.path());

    // The post-crash state, planted with no keeper code involved: the pointer
    // is published and the index entry was never told.
    let target = root.join("clip.mp4");
    std::fs::write(&target, pointer.render()).expect("publish the pointer by hand");
    stamp_index_forward(root);
    assert!(
        porcelain(root).contains("clip.mp4"),
        "the fixture has to start broken, or this test proves nothing"
    );

    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, _platform)) = engine_for(root, remote.path(), data.path()) else {
        return;
    };

    let err = engine
        .dehydrate_entry(PROFILE_ID, "clip.mp4")
        .await
        .expect_err("there is no content here to release");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::AlreadyPointer { path }) if path == "clip.mp4"
        ),
        "got: {err}"
    );

    // Stamped after the call, because `refresh_index_stat` writes the index and
    // git's racily-clean rule would otherwise decide on a content comparison.
    stamp_index_forward(root);
    assert_eq!(
        porcelain(root),
        "",
        "the refusal repaired the entry on its way out; without that re-stat \
         this path reads modified forever and nothing can fix it"
    );
}

/// A release whose **bookkeeping** fails is still a release — and asking again
/// repairs what was missed.
///
/// The rename is the point of no return. The ledger row is retracted and the
/// index entry re-stated only afterwards, so a `sync.db` somebody else has
/// locked or an `index.lock` held at that instant used to make this exit 1 with
/// the content already deleted — contradicting the one thing this verb promises
/// about its exit codes, that a failure means nothing on disk changed.
///
/// Forced with a real obstruction rather than a stub: a **directory** at
/// `.git/index.lock`, which is a lock gitoxide cannot acquire and cannot
/// remove. Clearing it and asking again is the documented recovery, and it is
/// the `AlreadyPointer` arm's re-stat that performs it.
#[tokio::test]
async fn a_release_whose_bookkeeping_fails_is_still_a_release() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let remote = tempfile::tempdir().expect("remote");
    seed(root, remote.path());

    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, platform)) = engine_for(root, remote.path(), data.path()) else {
        return;
    };
    remember(&platform, "clip.mp4");
    let blob = committed_blob(root);

    let obstruction = root.join(".git/index.lock");
    std::fs::create_dir(&obstruction).expect("obstruct the index lock");

    let done = engine
        .dehydrate_entry(PROFILE_ID, "clip.mp4")
        .await
        .expect("the content is gone, so this must not be reported as a failure");
    assert_eq!(
        done.size_bytes, CONTENT_BYTES,
        "and it reports what it actually gave back"
    );
    assert_eq!(
        std::fs::read(root.join("clip.mp4")).expect("read"),
        blob,
        "the pointer really was published, which is why this cannot be a failure"
    );
    assert!(
        ledger_paths(&platform).is_empty(),
        "the half of the bookkeeping that could run, ran"
    );

    // The half that could not: the entry still describes four mebibytes, and
    // git says so.
    stamp_index_forward(root);
    assert!(
        porcelain(root).contains("clip.mp4"),
        "the re-stat was the step that failed, so this is the state it left"
    );

    std::fs::remove_dir(&obstruction).expect("clear the obstruction");
    let err = engine
        .dehydrate_entry(PROFILE_ID, "clip.mp4")
        .await
        .expect_err("there is nothing left to release");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::AlreadyPointer { path }) if path == "clip.mp4"
        ),
        "got: {err}"
    );
    stamp_index_forward(root);
    assert_eq!(
        porcelain(root),
        "",
        "and asking again is what repairs it — nothing else can"
    );
}

/// A subpath spelling a separator the wrong way is refused, and the file it
/// names is untouched.
///
/// On Unix `\` is an ordinary filename character, so `40-media\clip.mp4` is ONE
/// component — but the index is asked with `\` normalized to `/`, so the
/// committed pointer that comes back belongs to the real `40-media/clip.mp4`
/// while every step after it acts on the backslash-named file at the root. This
/// fixture makes that file a **copy of the content**, which is the case where
/// every remaining guard passes: the length matches, the digest matches, the
/// remote proves the object, and the rename would have destroyed an untracked
/// file while reporting a successful release.
#[tokio::test]
#[cfg(unix)]
async fn a_backslash_spelled_subpath_is_refused_and_its_file_is_untouched() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let remote = tempfile::tempdir().expect("remote");
    let (pointer, content) = seed(root, remote.path());

    // The real, tracked path the backslash spelling aliases onto.
    std::fs::create_dir_all(root.join("40-media")).expect("a second zone");
    std::fs::write(root.join("40-media/clip.mp4"), pointer.render()).expect("write");
    {
        let repo = git::repo::open(root, false).expect("open");
        commit(
            &repo,
            remote.path(),
            &git::commit::StagedChange {
                added: vec![PathBuf::from("40-media/clip.mp4")],
                ..Default::default()
            },
        );
    }
    // And the file the release would actually have acted on: one literal name
    // containing a backslash, holding a copy of the content.
    let aliased = root.join("40-media\\clip.mp4");
    std::fs::write(&aliased, &content).expect("plant the untracked copy");

    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, _platform)) = engine_for(root, remote.path(), data.path()) else {
        return;
    };

    let err = engine
        .dehydrate_entry(PROFILE_ID, "40-media\\clip.mp4")
        .await
        .expect_err("this folder tracks the slash-spelled path, not this one");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::NotTracked { path })
                if path == "40-media\\clip.mp4"
        ),
        "and the sentence names what was asked for, not the path it aliased \
         onto: {err}"
    );
    assert_eq!(
        std::fs::read(&aliased).expect("read"),
        content,
        "byte for byte the copy the owner left there"
    );
    assert_eq!(
        std::fs::read(root.join("40-media/clip.mp4")).expect("read"),
        pointer.render().into_bytes(),
        "and the real path was never touched either"
    );
}

/// A committed pointer spelling `size 0` has no content to release.
///
/// The empty object, which the LFS spec carves out of the format: the pointer
/// stands for nothing, so there is nothing on this machine to give back. Every
/// guard below is degenerate for it — a length comparison of `0 != 0` passes,
/// the digest of an empty file *is* that pointer's oid, and the publish would
/// accept any zero-byte file — so the remote's store is seeded with the empty
/// object here on purpose: with the carve-out gone, this release would run all
/// the way through and write ~90 bytes of pointer text over the owner's empty
/// file while reporting `sizeBytes: 0`.
#[tokio::test]
async fn a_size_zero_pointer_has_nothing_to_release() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let remote = tempfile::tempdir().expect("remote");
    seed(root, remote.path());
    LfsStore::in_git_dir(remote.path())
        .insert_streaming(std::io::Cursor::new(Vec::new()))
        .expect("seed the remote with the empty object, so only the carve-out refuses");

    // Hand-written, because `Pointer::render` encodes the empty pointer as
    // nothing at all — a legal blob no keeper code can produce, and exactly
    // what another client's `size 0` entry looks like in an index.
    let blob = format!(
        "version https://git-lfs.github.com/spec/v1\noid sha256:{}\nsize 0\n",
        keeper_sync::lfs::pointer::EMPTY_OID
    );
    let target = root.join("empty.mp4");
    std::fs::write(&target, &blob).expect("check the pointer out");
    {
        let repo = git::repo::open(root, false).expect("open");
        commit(
            &repo,
            remote.path(),
            &git::commit::StagedChange {
                added: vec![PathBuf::from("empty.mp4")],
                ..Default::default()
            },
        );
    }
    // An empty worktree file, which `worktree_pointer` does not call a pointer
    // — so the `AlreadyPointer` check above cannot be what refuses this.
    std::fs::write(&target, b"").expect("empty the worktree file");

    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, _platform)) = engine_for(root, remote.path(), data.path()) else {
        return;
    };

    let err = engine
        .dehydrate_entry(PROFILE_ID, "empty.mp4")
        .await
        .expect_err("the empty object is not content anyone can give back");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::AlreadyPointer { path }) if path == "empty.mp4"
        ),
        "got: {err}"
    );
    assert_eq!(
        std::fs::read(&target).expect("read").len(),
        0,
        "and the owner's empty file is still empty"
    );
}

// ---------------------------------------------------------------------------
// The door refusals, all of which happen before anything is touched
// ---------------------------------------------------------------------------

/// Every refusal that is decided before the worktree is read at all, over one
/// fixture, with the content asserted intact after each.
///
/// One test rather than one each, because the claim is the same claim every
/// time — this verb refuses without touching anything — and the guard order is
/// what makes each of them reachable.
#[tokio::test]
async fn every_door_refusal_is_decided_before_anything_is_touched() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let remote = tempfile::tempdir().expect("remote");
    let (pointer, content) = seed(root, remote.path());
    // A plain tracked file, and a SECOND committed pointer inside a zone the
    // cone can be narrowed away from. The zone matters: a file at the
    // repository root is inside every cone by construction (`sparse`'s own
    // rule, and git's), so asking about `clip.mp4` could never produce
    // `OutsideSubpaths` however the cone were set.
    std::fs::create_dir_all(root.join("40-media")).expect("a second zone");
    std::fs::write(root.join("notes.md"), "a plain tracked file\n").expect("write");
    std::fs::write(root.join("40-media/clip.mp4"), pointer.render()).expect("write");
    {
        let repo = git::repo::open(root, false).expect("open");
        commit(
            &repo,
            remote.path(),
            &git::commit::StagedChange {
                added: vec![
                    PathBuf::from("notes.md"),
                    PathBuf::from("40-media/clip.mp4"),
                ],
                ..Default::default()
            },
        );
    }

    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, _platform)) = engine_for(root, remote.path(), data.path()) else {
        return;
    };

    let intact = |what: &str| {
        assert_eq!(
            std::fs::read(root.join("clip.mp4")).expect("read"),
            content,
            "{what} must not have touched the content"
        );
    };

    // An unknown profile is configuration, not a refusal: nothing was asked
    // about a folder that exists.
    let err = engine
        .dehydrate_entry("01JNOPE", "clip.mp4")
        .await
        .expect_err("no such folder");
    assert!(matches!(err, SyncError::Config(_)), "got: {err}");

    // Containment, lexically. Both spellings, plus the profile root itself,
    // which `plain_segments` answers with no segments at all.
    for subpath in ["../etc/passwd", "/etc/passwd", ""] {
        let err = engine
            .dehydrate_entry(PROFILE_ID, subpath)
            .await
            .expect_err("a subpath that leaves the folder may not be asked for");
        assert!(
            matches!(&err, SyncError::Refused(ContentRefusal::Escapes(_))),
            "`{subpath}`: {err}"
        );
    }
    intact("an escaping subpath");

    // A plain tracked file has no committed pointer, so there is no content to
    // release — collapsed with "git does not track this" on purpose.
    for subpath in ["notes.md", "docs/never-existed.mp4"] {
        let err = engine
            .dehydrate_entry(PROFILE_ID, subpath)
            .await
            .expect_err("no committed pointer here");
        assert!(
            matches!(&err, SyncError::Refused(ContentRefusal::NotTracked { .. })),
            "{subpath}: {err}"
        );
    }
    intact("an untracked or plain path");

    // Outside the cone: content the profile exists to keep away.
    let mut coned = profile(root, remote.path());
    coned.subpaths = vec!["docs".to_owned()];
    engine.upsert_profile(&coned).expect("narrow the cone");
    let err = engine
        .dehydrate_entry(PROFILE_ID, "40-media/clip.mp4")
        .await
        .expect_err("outside the cone");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::OutsideSubpaths { path })
                if path == "40-media/clip.mp4"
        ),
        "got: {err}"
    );
    intact("a path outside the cone");

    // LFS turned off for the folder.
    let mut disabled = profile(root, remote.path());
    disabled.lfs_mode = LfsMode::Disabled;
    engine.upsert_profile(&disabled).expect("turn LFS off");
    let err = engine
        .dehydrate_entry(PROFILE_ID, "clip.mp4")
        .await
        .expect_err("LFS is off");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::LfsDisabled { profile }) if profile == "media"
        ),
        "got: {err}"
    );
    intact("a folder with LFS disabled");

    // Paused: keeper is not touching this working tree at all.
    let mut paused = profile(root, remote.path());
    paused.enabled = false;
    engine.upsert_profile(&paused).expect("pause");
    let err = engine
        .dehydrate_entry(PROFILE_ID, "clip.mp4")
        .await
        .expect_err("paused");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::Paused { profile }) if profile == "media"
        ),
        "got: {err}"
    );
    intact("a paused folder");

    // A removable volume that is not attached. Absence, never failure (AD-48),
    // and above all never a reason to delete anything: the content is not gone,
    // the drive is out. The fixture folder shares a volume with the data dir,
    // which is exactly the state `volume_ready` refuses to adopt.
    let mut removable = profile(root, remote.path());
    removable.removable = true;
    engine
        .upsert_profile(&removable)
        .expect("mark it removable");
    let err = engine
        .dehydrate_entry(PROFILE_ID, "clip.mp4")
        .await
        .expect_err("the drive is out");
    assert!(matches!(&err, SyncError::MediaAbsent), "got: {err}");
    intact("an unplugged volume");

    // A path the user deleted and has not committed: writing a pointer over it
    // would be this verb inventing a file.
    engine
        .upsert_profile(&profile(root, remote.path()))
        .expect("restore");
    std::fs::remove_file(root.join("clip.mp4")).expect("delete it");
    let err = engine
        .dehydrate_entry(PROFILE_ID, "clip.mp4")
        .await
        .expect_err("it is not there");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::Missing { path }) if path == "clip.mp4"
        ),
        "got: {err}"
    );
    assert!(
        !root.join("clip.mp4").exists(),
        "a refusal did not put the pointer there either"
    );

    // Something at the path that is not a regular file. A fifo has a length
    // that is not a number of bytes anyone can read out of it, and it is
    // something a person put there — so it is `Modified` and it is never
    // renamed over.
    #[cfg(unix)]
    {
        let fifo = root.join("clip.mp4");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if made {
            let err = engine
                .dehydrate_entry(PROFILE_ID, "clip.mp4")
                .await
                .expect_err("a fifo holds no committed content");
            assert!(
                matches!(
                    &err,
                    SyncError::Refused(ContentRefusal::Modified { path }) if path == "clip.mp4"
                ),
                "got: {err}"
            );
            assert!(
                !std::fs::symlink_metadata(&fifo)
                    .expect("stat")
                    .file_type()
                    .is_file(),
                "and it is still the fifo the user left there"
            );
        }
    }
}

/// A folder that is not a repository is refused — and **no `.git` is created**.
///
/// The reason this verb calls `git::repo::open` rather than `Engine::open_repo`:
/// a release must not be able to clone over the network or adopt a plain folder
/// on its way to saying no.
#[tokio::test]
async fn a_folder_with_no_repository_is_refused_and_none_is_created() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    std::fs::write(root.join("clip.mp4"), b"some bytes\n").expect("write");

    let remote = tempfile::tempdir().expect("remote");
    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, _platform)) = engine_for(root, remote.path(), data.path()) else {
        return;
    };

    let err = engine
        .dehydrate_entry(PROFILE_ID, "clip.mp4")
        .await
        .expect_err("there is no index here");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::NotTracked { path }) if path == "clip.mp4"
        ),
        "got: {err}"
    );
    assert!(
        !root.join(".git").exists(),
        "asking to release a file must never turn a plain folder into a repository"
    );
}

/// A parent component replaced by a symlink out of the folder is refused, so
/// the publish cannot land outside the profile root.
///
/// The lexical half of containment cannot see this — every segment of
/// `vault/clip.mp4` is a plain `Component::Normal` — and this is a write, so it
/// runs the canonicalizing half too (AD-59).
#[tokio::test]
#[cfg(unix)]
async fn a_parent_symlink_pointing_out_of_the_folder_is_refused() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let remote = tempfile::tempdir().expect("remote");
    let (_pointer, content) = seed(root, remote.path());

    let outside = tempfile::tempdir().expect("somewhere else");
    std::fs::write(outside.path().join("clip.mp4"), &content).expect("plant the content");
    std::os::unix::fs::symlink(outside.path(), root.join("vault")).expect("plant the symlink");

    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, _platform)) = engine_for(root, remote.path(), data.path()) else {
        return;
    };

    let err = engine
        .dehydrate_entry(PROFILE_ID, "vault/clip.mp4")
        .await
        .expect_err("a write may not leave the folder");
    assert!(
        matches!(&err, SyncError::Refused(ContentRefusal::Escapes(_))),
        "browse's own sentence, carried across: {err}"
    );
    assert_eq!(
        std::fs::read(outside.path().join("clip.mp4")).expect("read"),
        content,
        "and the file outside the folder is untouched"
    );
}

/// A second release while one is in flight is `Busy`, not a second write to the
/// same index.
///
/// Deterministic rather than timed, and it owes that to two things. The remote
/// is a **loopback socket nobody accepts on**: the kernel completes the connect
/// out of its backlog and the request goes out, so the first release is
/// guaranteed to be suspended *inside* the batch round trip, with the
/// reservation held — and no name is resolved, so no resolver can change the
/// outcome. And the first future is polled exactly **once**, by hand, rather
/// than joined: one poll runs every synchronous guard, takes the reservation
/// and reaches the round trip, which is the whole state this test is about.
/// Waiting for it to finish would mean waiting out `http::READ_TIMEOUT` for an
/// answer that is never coming.
#[tokio::test]
async fn a_second_release_while_one_is_running_is_busy() {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let remote = tempfile::tempdir().expect("remote");
    let (_pointer, content) = seed(root, remote.path());

    let listening = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listening
        .local_addr()
        .expect("the kernel's own port, so nothing is hard-coded")
        .port();

    let data = tempfile::tempdir().expect("data dir");
    let mut silent = profile(root, remote.path());
    silent.remote_url = format!("http://127.0.0.1:{port}/r.git");
    let Some((engine, _platform)) = engine_with(silent, data.path()) else {
        return;
    };

    let mut first = Box::pin(engine.dehydrate_entry(PROFILE_ID, "clip.mp4"));
    // Nothing ever wakes the suspended release: it is dropped, not awaited.
    let waker = Waker::noop();
    assert!(
        matches!(
            first.as_mut().poll(&mut Context::from_waker(waker)),
            Poll::Pending
        ),
        "the first release must be suspended in the round trip, or there is no \
         race for the second one to lose"
    );

    let second = engine
        .dehydrate_entry(PROFILE_ID, "clip.mp4")
        .await
        .expect_err("one operation per folder");
    assert!(
        matches!(&second, SyncError::Busy(name) if name == "media"),
        "the folder is reserved, and losing that race writes nothing: {second}"
    );

    // Abandoning the suspended release is what returns the reservation; there
    // is no answer coming from that socket to abandon it any other way.
    drop(first);
    assert_eq!(
        std::fs::read(root.join("clip.mp4")).expect("read"),
        content,
        "neither call touched the content"
    );
}

/// Answer exactly one LFS batch request, affirmatively, on its own thread.
///
/// Just enough of the protocol for `BatchClient::download`: read the head, read
/// the body it declared, and write a 200 whose one object carries a `download`
/// action — which is what `audit::serves` requires and therefore what makes the
/// release proceed. Loopback, on a port the kernel chose, so no name is
/// resolved.
///
/// A thread rather than a task, following `tests/durability_matrix_stages.rs`:
/// the caller drives the release future by hand and must not have to yield to a
/// runtime for the server to make progress.
fn serve_one_batch(listener: std::net::TcpListener, oid: String, size: u64) {
    use std::io::{Read as _, Write as _};

    std::thread::spawn(move || {
        let Ok((mut stream, _)) = listener.accept() else {
            return;
        };
        let mut head = Vec::new();
        let mut byte = [0u8; 1];
        while !head.ends_with(b"\r\n\r\n") {
            match stream.read(&mut byte) {
                Ok(1) => head.push(byte[0]),
                _ => return,
            }
        }
        let lowered = String::from_utf8_lossy(&head).to_ascii_lowercase();
        let length = lowered
            .lines()
            .find_map(|line| line.strip_prefix("content-length:"))
            .and_then(|value| value.trim().parse::<usize>().ok())
            .unwrap_or(0);
        let mut body = vec![0u8; length];
        if stream.read_exact(&mut body).is_err() {
            return;
        }
        let answer = format!(
            "{{\"transfer\":\"basic\",\"objects\":[{{\"oid\":\"{oid}\",\"size\":{size},\
             \"actions\":{{\"download\":{{\"href\":\"http://127.0.0.1:1/o\"}}}}}}]}}"
        );
        let _ = stream.write_all(
            format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/vnd.git-lfs+json\r\n\
                 Content-Length: {}\r\nConnection: close\r\n\r\n{answer}",
                answer.len()
            )
            .as_bytes(),
        );
        let _ = stream.flush();
    });
}

/// A pin that lands **while the proof is in flight** still stops the release.
///
/// NFR-40 puts the authorization at the moment of the deletion, and the cheap
/// pin check happens before the two slowest things this verb does — a whole-file
/// hash and a round trip. So the pin is read again with nothing between it and
/// the rename, and this is the window that proves it: the release is polled once
/// by hand, which runs every synchronous guard (including the first pin read)
/// and suspends it inside the round trip; the pin is written; and only then is
/// the future resumed. Nothing yields to the runtime in between, so the pin
/// really does land between the two reads rather than merely near them.
///
/// The server answers affirmatively on purpose. With the second read gone this
/// release runs all the way through and deletes content the owner asked keeper
/// to keep.
#[tokio::test]
async fn a_pin_that_lands_while_the_proof_is_in_flight_still_stops_the_release() {
    use std::future::Future;
    use std::task::{Context, Poll, Waker};

    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let remote = tempfile::tempdir().expect("remote");
    let (pointer, content) = seed(root, remote.path());

    let listening = std::net::TcpListener::bind("127.0.0.1:0").expect("bind loopback");
    let port = listening.local_addr().expect("addr").port();
    serve_one_batch(listening, pointer.oid.clone(), pointer.size);

    let data = tempfile::tempdir().expect("data dir");
    // An answering server rather than the filesystem remote, because the
    // filesystem proof takes no `await` at all and there would be no window.
    let mut asked = profile(root, remote.path());
    asked.remote_url = format!("http://127.0.0.1:{port}/r.git");
    let Some((engine, platform)) = engine_with(asked, data.path()) else {
        return;
    };
    remember(&platform, "clip.mp4");

    let mut release = Box::pin(engine.dehydrate_entry(PROFILE_ID, "clip.mp4"));
    // The hand poll needs a waker and has nothing to wake: the future is
    // resumed by the `await` below, on the runtime's own waker.
    let waker = Waker::noop();
    assert!(
        matches!(
            release.as_mut().poll(&mut Context::from_waker(waker)),
            Poll::Pending
        ),
        "the release must be suspended in the round trip, with every guard \
         before it — the first pin read included — already run"
    );

    pin(&engine, "clip.mp4");

    let err = release
        .await
        .expect_err("a pin is the one instruction a release may not weigh");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::Pinned { path }) if path == "clip.mp4"
        ),
        "asked again at the moment of the deletion, as NFR-40 requires: {err}"
    );
    assert_eq!(
        std::fs::read(root.join("clip.mp4")).expect("read"),
        content,
        "the content the owner asked to keep is still here"
    );
    assert_eq!(
        ledger_paths(&platform),
        vec!["clip.mp4".to_owned()],
        "and so is the row that carries the pin"
    );
}
