//! FR-338 against a real repository, a real index and the real `git` binary
//! (Story 56.3).
//!
//! Three claims live here rather than in a unit test because each of them is
//! about something only a real checkout can produce:
//!
//! 1. **The tree stays clean.** Publishing four mebibytes over a ~130-byte
//!    pointer changes the worktree file's length, so the index entry's cached
//!    stat stops describing what is on disk and `git status` calls the path
//!    modified. `refresh_index_stat` is the repair, and the only thing that can
//!    say whether it worked is `git status --porcelain` — a unit test over a
//!    fixture it wrote itself cannot.
//! 2. **A repeat request is one row and one download.** That is a property of
//!    `db::enqueue_unique` over a real journal file, driven through the engine's
//!    own public verb rather than by calling the statement directly.
//! 3. **A refusal writes nothing.** Byte-for-byte, on a file the fixture edited.
//!
//! **No `filter=lfs` rule is registered in the fixture, on purpose.** `git
//! status` must decide from the refreshed index stat, so a broken refresh
//! surfaces here as ` M` rather than as a crash inside a clean filter that
//! would have hidden it (DW-121).
//!
//! No network: the object store is local, and nothing is fetched or pushed.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::Arc;

use keeper_sync::db;
use keeper_sync::engine::Engine;
use keeper_sync::error::SyncError;
use keeper_sync::git;
use keeper_sync::lfs::hydrate::{ContentRefusal, MaterializeOutcome};
use keeper_sync::lfs::listing::LfsFileState;
use keeper_sync::lfs::pointer::Pointer;
use keeper_sync::lfs::store::LfsStore;
use keeper_sync::platform::{SyncPlatform, TestPlatform};
use keeper_sync::profile::SyncProfile;
use keeper_sync::provenance::{Provenance, SyncSource};

/// What the pointer stands for. Four mebibytes, so a worktree still holding
/// pointer text is unmistakable.
const CONTENT_BYTES: u64 = 4 * 1024 * 1024;

/// `false` when this machine has no usable `git`, in which case there is no
/// index to read and nothing here is falsifiable — the same skip
/// `tests/lfs_listing.rs` makes.
fn init_repo(root: &Path) -> bool {
    std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(root)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn profile(root: &Path) -> SyncProfile {
    let mut p = SyncProfile::new("01JMATERIALIZE", "media", root, "https://git.invalid/r.git");
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
/// Returns the pointer so every assertion compares against the oid and size the
/// fixture actually wrote rather than against a literal that could drift.
fn seed(root: &Path) -> Pointer {
    let store = LfsStore::in_git_dir(root.join(".git"));
    store.ensure_layout().expect("store layout");
    let (oid, size) = store
        .insert_streaming(std::io::Cursor::new(vec![9u8; CONTENT_BYTES as usize]))
        .expect("insert the real object");
    let pointer = Pointer::new(oid, size);
    std::fs::write(root.join("clip.mp4"), pointer.render()).expect("write the pointer text");

    let repo = git::repo::open(root, false).expect("open the fixture repository");
    commit(
        &repo,
        &git::commit::StagedChange {
            added: vec![std::path::PathBuf::from("clip.mp4")],
            ..Default::default()
        },
    );
    pointer
}

/// Stamp `.git/index` ten seconds into the future, with no keeper code
/// involved.
///
/// git's **racy-clean** rule: an index entry whose mtime is not strictly older
/// than the index file's own mtime is not trusted on its cached stat, so git
/// falls back to comparing content — here four mebibytes against a 130-byte
/// pointer blob — and reports ` M` for the second a materialization lands in,
/// however correct the refreshed stat is. `stage::materialize` writes the file
/// and `refresh_index_stat` writes the index microseconds later, so that second
/// is unavoidable and gitoxide's own walk applies the same rule
/// (`tests/index_repair.rs` sleeps 1.5 s ahead of its repair for it).
///
/// Moving the index file's timestamp is what makes the assertion below decide
/// on the **stat** rather than on a content comparison — and it is deliberately
/// a filesystem call rather than `Engine::rescan`: pressing keeper's repair
/// button would re-stat every entry in the repository and the assertion would
/// then pass with `materialize_entry`'s own `refresh_index_stat` deleted.
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
/// Returns the platform too, because the journal file this test inspects lives
/// under its data directory.
fn engine_for(root: &Path, data: &Path) -> Option<(Engine, Arc<TestPlatform>)> {
    let platform = Arc::new(TestPlatform::new(data));
    let engine = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>).ok()?;
    engine
        .upsert_profile(&profile(root))
        .expect("register the profile");
    Some((engine, platform))
}

/// Every `lfsDownload` row in the journal: its id, label and urgency, read
/// through a second connection to the very file the engine wrote.
///
/// Read rather than claimed: `claim_ready` would flip the row to `running`, and
/// what is being asserted is what the *request* left behind.
fn download_rows(platform: &TestPlatform) -> Vec<(i64, Option<String>, Option<i64>)> {
    let conn = db::open(&platform.data_dir().expect("data dir")).expect("open sync.db");
    let mut stmt = conn
        .prepare("SELECT id, label, urgency FROM journal WHERE kind = 'lfsDownload' ORDER BY id")
        .expect("prepare");
    let rows = stmt
        .query_map([], |r| {
            Ok((
                r.get::<_, i64>(0)?,
                r.get::<_, Option<String>>(1)?,
                r.get::<_, Option<i64>>(2)?,
            ))
        })
        .expect("query");
    rows.map(|row| row.expect("row")).collect()
}

/// The object is here, so the content is published inline — and the path reads
/// clean afterwards.
///
/// The cleanliness assertion is the load-bearing one. Without
/// `refresh_index_stat` the entry's cached stat still describes ~130 bytes of
/// pointer text while the file is four mebibytes, so every status walk pays a
/// full content comparison for it — the 343-second walk
/// `lfs::stage::materialize`'s own doc records.
///
/// # Why the assertion stamps the index first
///
/// git's racy-clean rule would otherwise decide this by comparing content, not
/// by the stat this story repairs — see [`stamp_index_forward`], which is a
/// filesystem call precisely so that the refresh remains the only thing this
/// assertion can be measuring.
#[test]
fn an_object_already_in_the_store_is_published_and_the_tree_stays_clean() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let pointer = seed(root);
    assert_eq!(
        porcelain(root),
        "",
        "the fixture must start clean, or the assertion at the end proves nothing"
    );

    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, platform)) = engine_for(root, data.path()) else {
        return;
    };

    let done = engine
        .materialize_entry("01JMATERIALIZE", "clip.mp4")
        .expect("the object is here, so this cannot refuse");

    assert_eq!(done.outcome, MaterializeOutcome::Materialized);
    assert_eq!(done.path, "clip.mp4");
    assert_eq!(done.oid, pointer.oid);
    assert_eq!(
        done.size_bytes, CONTENT_BYTES,
        "the honest number is the pointer's, never the pointer text's"
    );
    assert_eq!(
        done.unit_id, None,
        "nothing was queued: the bytes were already on this machine"
    );

    assert_eq!(
        std::fs::read(root.join("clip.mp4")).expect("read the published file"),
        vec![9u8; CONTENT_BYTES as usize],
        "the worktree holds the real bytes"
    );
    assert!(
        download_rows(&platform).is_empty(),
        "publishing inline queues no transfer"
    );

    let files = engine
        .lfs_files("01JMATERIALIZE")
        .expect("list the LFS paths");
    assert_eq!(files.len(), 1);
    assert_eq!(
        files[0].state,
        LfsFileState::Materialized,
        "the listing now reports content here"
    );
    assert!(
        files[0].materialized_at_ms.is_some(),
        "and the ledger has a row for it, which is the fact 56.5's sweep reads"
    );

    // Asking again is the already-held answer, and it writes nothing.
    let again = engine
        .materialize_entry("01JMATERIALIZE", "clip.mp4")
        .expect("asking twice is not an error");
    assert_eq!(again.outcome, MaterializeOutcome::AlreadyMaterialized);
    assert_eq!(again.unit_id, None);

    // And the real `git` binary agrees. The stamp is the fixture's, not
    // keeper's: what is under test is the one-path `refresh_index_stat` above,
    // and with it deleted this assertion reports ` M`.
    stamp_index_forward(root);
    assert_eq!(
        porcelain(root),
        "",
        "the real `git status` reports nothing about a materialized LFS path: \
         pointer blob, worktree stat, clean status"
    );
}

/// The object is absent, so one urgent labelled unit is queued — and asking
/// twice returns that same unit rather than a second download.
///
/// `enqueue_unique` deduplicates on the serialized payload, which is exactly
/// why the urgency is a separate narrow `UPDATE`: folded into the payload it
/// would have made the requested unit a *different* unit and fetched the same
/// immutable bytes twice.
#[test]
fn an_absent_object_is_queued_once_however_many_times_it_is_asked_for() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let pointer = seed(root);
    // Empty the store, leaving the committed pointer behind: the state a clone
    // of somebody else's folder arrives in.
    let store = LfsStore::in_git_dir(root.join(".git"));
    std::fs::remove_file(store.object_path(&pointer.oid)).expect("empty the store");

    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, platform)) = engine_for(root, data.path()) else {
        return;
    };

    let queued = engine
        .materialize_entry("01JMATERIALIZE", "clip.mp4")
        .expect("a queued transfer is an outcome, not a failure");
    assert_eq!(queued.outcome, MaterializeOutcome::Queued);
    assert_eq!(queued.oid, pointer.oid);
    assert_eq!(queued.size_bytes, CONTENT_BYTES);
    let unit = queued.unit_id.expect("a queued outcome names its unit");

    assert_eq!(
        download_rows(&platform),
        vec![(unit, Some("clip.mp4".to_owned()), Some(1))],
        "exactly one row, named with the path, marked as something somebody is \
         waiting for"
    );
    assert_eq!(
        std::fs::read(root.join("clip.mp4")).expect("read"),
        pointer.render().into_bytes(),
        "requesting a transfer writes no bytes; the daemon delivers them"
    );

    let repeat = engine
        .materialize_entry("01JMATERIALIZE", "clip.mp4")
        .expect("asking again is not an error");
    assert_eq!(repeat.outcome, MaterializeOutcome::Queued);
    assert_eq!(
        repeat.unit_id,
        Some(unit),
        "the second caller is told which unit already covers its work"
    );
    assert_eq!(
        download_rows(&platform).len(),
        1,
        "and no second transfer was queued for the same immutable bytes"
    );
}

/// A file the user has edited is refused by type, and its bytes are untouched.
#[test]
fn a_locally_modified_file_is_refused_and_nothing_is_written_or_queued() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    seed(root);
    // The object IS in the store, so nothing but the refusal stands between
    // this call and overwriting the edit.
    let edited = b"the user opened this in an editor and saved it\n";
    std::fs::write(root.join("clip.mp4"), edited).expect("edit the file");

    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, platform)) = engine_for(root, data.path()) else {
        return;
    };

    let err = engine
        .materialize_entry("01JMATERIALIZE", "clip.mp4")
        .expect_err("keeper must not overwrite an edit");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::LocallyModified { path }) if path == "clip.mp4"
        ),
        "the caller must be able to distinguish this BY TYPE, not by message: {err}"
    );
    assert_eq!(err.code(), "refused");

    assert_eq!(
        std::fs::read(root.join("clip.mp4")).expect("read"),
        edited,
        "byte for byte what the user left there"
    );
    assert!(
        download_rows(&platform).is_empty(),
        "a refusal queues nothing either"
    );
}

/// A parent component replaced by a symlink out of the folder is refused, so
/// the publish cannot land outside the profile root (Story 56.3 review).
///
/// The lexical half of containment cannot see this: every segment of
/// `vault/clip.mp4` is a plain `Component::Normal`, and the `symlink_metadata`
/// in `hydrate::plan` does not follow the FINAL component but does follow every
/// parent. Only the canonicalizing half — `browse::resolve`, which every read
/// verb in the crate already runs (AD-59) — catches it, which is why a write
/// verb must run it too. Without it `stage::materialize` writes its staging
/// file and renames a real object into a directory the profile does not own.
#[test]
#[cfg(unix)]
fn a_parent_symlink_pointing_out_of_the_folder_is_refused() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let pointer = seed(root);

    // Outside the profile entirely, and holding the committed pointer text — so
    // every check except containment would say "publish here".
    let outside = tempfile::tempdir().expect("somewhere else");
    std::fs::write(outside.path().join("clip.mp4"), pointer.render()).expect("plant the pointer");
    std::os::unix::fs::symlink(outside.path(), root.join("vault")).expect("plant the symlink");

    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, _platform)) = engine_for(root, data.path()) else {
        return;
    };

    let err = engine
        .materialize_entry("01JMATERIALIZE", "vault/clip.mp4")
        .expect_err("a write may not leave the folder");
    assert!(
        matches!(&err, SyncError::Refused(ContentRefusal::Escapes(_))),
        "the containment refusal is browse's own, carried across: {err}"
    );
    assert_eq!(
        std::fs::read(outside.path().join("clip.mp4")).expect("read"),
        pointer.render().as_bytes(),
        "and the file outside the folder is untouched"
    );
}
