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

/// A second connection to the very `sync.db` the engine wrote — [`download_rows`]'
/// approach, for the other table this file now has claims about.
fn ledger_conn(platform: &TestPlatform) -> rusqlite::Connection {
    db::open(&platform.data_dir().expect("data dir")).expect("open sync.db")
}

/// Every `materialized` row this profile still holds, whole.
///
/// The rows rather than the paths, because the claims below are about the two
/// clocks — `at_ms` and `last_used_ms` — and which arm of
/// `db::observe_materialized` moved which.
fn ledger_rows(platform: &TestPlatform) -> Vec<db::MaterializedRow> {
    db::materialized_rows(&ledger_conn(platform), "01JMATERIALIZE").expect("read the ledger")
}

/// The one `materialized` row for `path`, or a failure naming what is there
/// instead.
fn ledger_row(platform: &TestPlatform, path: &str) -> db::MaterializedRow {
    let rows = ledger_rows(platform);
    rows.iter()
        .find(|row| row.path == path)
        .unwrap_or_else(|| panic!("the ledger must hold a row for {path}; it holds {rows:?}"))
        .clone()
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

/// The already-held answer records that **this machine holds the content**, and
/// records it as a *use* rather than as an arrival (Story 56.14, FR-334).
///
/// Without the fix this arm returned before any ledger write, so
/// `db::materialized_rows` — the release sweep's whole candidate list — reads
/// **no row at all** for the one path a human explicitly named: no candidate,
/// no recorded use, and therefore a path the release clocks cannot see. The
/// first `ledger_row` call below finds nothing and panics.
///
/// # Why the fixture publishes nothing first
///
/// [`an_object_already_in_the_store_is_published_and_the_tree_stays_clean`]
/// reaches this arm too, but only on its *second* call — after the publish arm
/// has already written a row through `db::remember_materialized`. An assertion
/// there cannot tell "this arm wrote the row" from "the previous call did", so
/// the row here has to be one nothing else could have written: the worktree
/// holds four mebibytes of real content that keeper never published, which is
/// `hydrate::plan`'s documented same-length tie and exactly the state a manual
/// `git lfs pull`, a pendrive copy or a second profile sharing the store leaves
/// behind.
///
/// # Why `observe_materialized` and not `remember_materialized`
///
/// This arm does not know *when* the content landed — it found it there — so it
/// must not claim it landed now. `at_ms` standing still across the second call
/// while `last_used_ms` moves is that distinction, asserted: a
/// `remember_materialized` here would push `at_ms` forward on every read and no
/// TTL measured from it could ever mature.
#[test]
fn the_already_held_answer_records_the_use_without_restarting_the_arrival_clock() {
    let work = tempfile::tempdir().expect("tempdir");
    let root = work.path();
    if !init_repo(root) {
        return;
    }
    let pointer = seed(root);
    // Real content of exactly the committed pointer's length, standing where the
    // pointer text was: `hydrate::plan`'s length tie, and the shape `git lfs
    // pull` leaves. Nothing keeper does wrote this, so nothing but the arm under
    // test can account for a ledger row afterwards.
    std::fs::write(root.join("clip.mp4"), vec![9u8; CONTENT_BYTES as usize])
        .expect("plant content keeper never published");

    let data = tempfile::tempdir().expect("data dir");
    let Some((engine, platform)) = engine_for(root, data.path()) else {
        return;
    };
    assert!(
        ledger_rows(&platform).is_empty(),
        "the fixture must start with an empty ledger, or the row below proves nothing"
    );
    let landed = platform.now_ms();

    let held = engine
        .materialize_entry("01JMATERIALIZE", "clip.mp4")
        .expect("the content is here, so this cannot refuse");
    assert_eq!(
        held.outcome,
        MaterializeOutcome::AlreadyMaterialized,
        "content of the committed pointer's own length is already here"
    );
    assert_eq!(held.oid, pointer.oid);
    assert_eq!(held.unit_id, None);

    let first = ledger_row(&platform, "clip.mp4");
    assert_eq!(
        first.at_ms, landed,
        "the arm that found the content writes the row the sweep needs"
    );
    assert_eq!(
        first.last_used_ms,
        Some(landed),
        "and stamps the use clock, because somebody just asked for this path"
    );

    // A minute later, and the same question again.
    platform.advance_ms(60_000);
    let again = engine
        .materialize_entry("01JMATERIALIZE", "clip.mp4")
        .expect("asking twice is not an error");
    assert_eq!(again.outcome, MaterializeOutcome::AlreadyMaterialized);

    let second = ledger_row(&platform, "clip.mp4");
    assert_eq!(
        second.at_ms, first.at_ms,
        "`at_ms` is when content for this path last LANDED, and nothing landed: \
         a writer that moved it would restart every TTL the sweep measures from it"
    );
    assert_eq!(
        second.last_used_ms,
        Some(landed + 60_000),
        "`last_used_ms` is when it was last read through keeper, and it just was"
    );
    assert_eq!(
        ledger_rows(&platform).len(),
        1,
        "one path, one row: the conflict arm updates rather than inserting"
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

/// The fixture profile with a **filesystem** remote, so an LFS download is
/// `lfs::local`'s real file copy and no server exists anywhere.
///
/// Derived from [`profile`] rather than respelled, so the threshold and every
/// other setting the tests above depend on stay one definition.
fn remote_profile(root: &Path, remote: &Path) -> SyncProfile {
    let mut p = profile(root);
    p.remote_url = remote.to_string_lossy().into_owned();
    p
}

/// Point the fixture's `origin` at the bare remote, as a real clone's would be.
///
/// The transfer leg reads `profile.remote_url` and not git's config, so this is
/// fidelity rather than a dependency — but `open_repo` opens the repository
/// production would, and a repository with no remote is not one.
fn add_origin(root: &Path, remote: &Path) -> bool {
    std::process::Command::new("git")
        .args(["remote", "add", "origin"])
        .arg(remote)
        .current_dir(root)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// The state a clone under a virtualization policy really arrives in: the
/// pointer committed as the index's blob and standing in the worktree, and the
/// object itself in the **remote's** store and nowhere else.
///
/// [`seed`]'s sibling, and the difference is the whole point: that one starts
/// with the object already local, so nothing has to be fetched to satisfy a
/// request. Only this one can say whether a fetch ever happens.
fn seed_remote_only(root: &Path, remote: &Path) -> Pointer {
    let store = LfsStore::in_git_dir(remote);
    store.ensure_layout().expect("the remote's store layout");
    let (oid, size) = store
        .insert_streaming(std::io::Cursor::new(vec![9u8; CONTENT_BYTES as usize]))
        .expect("seed the remote's own LFS store");
    let pointer = Pointer::new(oid, size);
    std::fs::write(root.join("clip.mp4"), pointer.render()).expect("check out the pointer");

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

/// Repository, bare filesystem remote, engine and the committed pointer — or
/// `None` on a machine with no usable `git` (AD-41).
///
/// `seed_remote` decides whether the remote's store actually holds the object,
/// which is the one difference between the two claims below.
fn remote_fixture(
    root: &Path,
    remote: &Path,
    data: &Path,
    seed_remote: bool,
) -> Option<(Engine, Arc<TestPlatform>, Pointer)> {
    if gix::init_bare(remote).is_err() {
        return None;
    }
    if !init_repo(root) || !add_origin(root, remote) {
        return None;
    }
    let pointer = seed_remote_only(root, remote);
    if !seed_remote {
        let store = LfsStore::in_git_dir(remote);
        std::fs::remove_file(store.object_path(&pointer.oid)).expect("empty the remote's store");
    }
    let platform = Arc::new(TestPlatform::new(data));
    let engine = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>).ok()?;
    engine
        .upsert_profile(&remote_profile(root, remote))
        .expect("register the profile");
    Some((engine, platform, pointer))
}

/// `Engine::CLAIM_LIMIT`, which is private. Restated here rather than exported:
/// what the test needs is "more background rows than one batch can hold", and a
/// value that drifts *upwards* in the engine only weakens the fixture into
/// asserting the same thing the count-based loop would already satisfy, which
/// is a false pass this comment is the guard against.
const CLAIM_LIMIT: u8 = 16;

/// One `lfsDownload` unit for `oid`, queued straight into the journal as a
/// background scan would have queued it: no label and no urgency.
///
/// Written through a second connection to the very file the engine reads, which
/// is `download_rows`' approach and for its reason — going through the engine
/// would mean going through the request verb, which promotes.
fn enqueue_download(platform: &TestPlatform, oid: &str, size: u64) -> i64 {
    let conn = db::open(&platform.data_dir().expect("data dir")).expect("open sync.db");
    db::enqueue(
        &conn,
        "01JMATERIALIZE",
        &db::WorkKind::LfsDownload {
            oid: oid.to_owned(),
            size,
        },
        0,
        0,
    )
    .expect("queue a background download")
}

/// Name a queued unit, the way `materialize_pending` names the rows it queues.
///
/// [`enqueue_download`]'s companion rather than an argument to it, for
/// `db::label_unit`'s own reason: the label says which path a transfer will
/// deliver, and `materialize_landed` defers to the next whole-tree sweep for an
/// unlabelled row. A claim about what a drained download PUBLISHES is therefore
/// a claim about a labelled row.
///
/// `Engine::backfill_labels` would name it too, and that is exactly why the
/// fixture does not lean on it: it runs at most **once per profile per
/// process**, so any earlier drain in the same test would leave the row
/// nameless and quietly turn the assertion below into one about nothing.
fn label_download(platform: &TestPlatform, unit: i64, path: &str) {
    db::label_unit(&ledger_conn(platform), unit, path).expect("name the queued unit");
}

/// One `Push` unit, queued the way a local edit queues one.
fn enqueue_push(platform: &TestPlatform) -> i64 {
    let conn = db::open(&platform.data_dir().expect("data dir")).expect("open sync.db");
    db::enqueue(&conn, "01JMATERIALIZE", &db::WorkKind::Push, 0, 0).expect("queue a push")
}

/// One unit's `state` and `not_before_ms`, or `None` when the row is gone.
///
/// The pair is what says whether a row was left alone: a state of `pending`
/// with `not_before_ms` in the future is a row waiting out a backoff, and the
/// same state with `not_before_ms` at or before now is a row somebody promoted.
///
/// `None` rather than a panic, because "the row is gone" is itself a claim a
/// test needs to be able to phrase: `db::complete` is the only thing that
/// deletes one, so a vanished row means somebody ran the work.
fn unit_schedule(platform: &TestPlatform, unit: i64) -> Option<(String, i64)> {
    let conn = db::open(&platform.data_dir().expect("data dir")).expect("open sync.db");
    conn.query_row(
        "SELECT state, not_before_ms FROM journal WHERE id = ?1",
        [unit],
        |r| Ok((r.get::<_, String>(0)?, r.get::<_, i64>(1)?)),
    )
    .ok()
}

/// A one-shot request with **no supervisor anywhere** leaves the real bytes on
/// disk, and the path reads clean afterwards.
///
/// This is the defect an end-to-end run of the shipped `keeper-syncd` binary
/// found and every test above missed. Each of those asserts what the *request*
/// left behind — one row, labelled, marked urgent — and none of them asks
/// whether a process with no event loop behind it ever delivers anything. It
/// does not: `keeper-syncd materialize` printed `queued`, exited `0`, and left
/// ~130 bytes of pointer text on disk. For the person or the cron entry this
/// verb exists for, that is a no-op wearing a success message.
///
/// So the claim is about the *content*, not about the journal: the assertion
/// reads the worktree file and compares every byte, and `git status` decides
/// the second half. Nothing here can pass on a queued row.
#[tokio::test]
async fn a_one_shot_request_drains_what_it_queued_and_lands_the_bytes() {
    let work = tempfile::tempdir().expect("tempdir");
    let remote = tempfile::tempdir().expect("remote");
    let data = tempfile::tempdir().expect("data dir");
    let root = work.path();
    let Some((engine, platform, pointer)) = remote_fixture(root, remote.path(), data.path(), true)
    else {
        return;
    };
    assert_eq!(
        std::fs::read(root.join("clip.mp4")).expect("read"),
        pointer.render().into_bytes(),
        "the fixture starts with the pointer on disk and the object only upstream"
    );

    let done = engine
        .materialize_entry_now("01JMATERIALIZE", "clip.mp4", SyncSource::Cli)
        .await
        .expect("the remote holds the object, so the request can be satisfied");

    assert_eq!(
        done.outcome,
        MaterializeOutcome::Materialized,
        "a one-shot invocation must report what it DID, and it did deliver"
    );
    assert_eq!(done.oid, pointer.oid);
    assert_eq!(done.size_bytes, CONTENT_BYTES);
    assert_eq!(
        done.unit_id, None,
        "`Materialization::unit_id` names the row that WILL deliver the content, \
         and after a successful drain there is no such row — `db::complete` \
         deleted it. A consumer that polls on the field's presence, which is what \
         the `--json` document's key set invites, would wait forever"
    );

    assert_eq!(
        std::fs::read(root.join("clip.mp4")).expect("read the published file"),
        vec![9u8; CONTENT_BYTES as usize],
        "the worktree holds the real content, not a promise about it"
    );
    assert!(
        download_rows(&platform).is_empty(),
        "and the unit is gone: `db::complete` deletes a drained row, so a later \
         pass has nothing left to owe"
    );

    // The real `git` binary agrees, deciding on the stat rather than on a
    // content comparison — see `stamp_index_forward`.
    stamp_index_forward(root);
    assert_eq!(
        porcelain(root),
        "",
        "and the path the drain published reads clean"
    );
}

/// A queued download the local store can already satisfy is published **without
/// a transfer** (Story 56.14).
///
/// Without the fix that row went on to read `.lfsconfig`, resolve an endpoint
/// and spend a batch round trip re-downloading content already on this disk. A
/// pending `LfsDownload` row outlives its reason routinely: the bytes arrive by
/// a pendrive copy, by a second profile sharing this store, by a manual `git lfs
/// pull`, or by `materialize_entry`'s own inline publish, which publishes
/// straight from the store and leaves the covering row standing. On a metered or
/// slow link that is exactly the transfer the request verb exists to avoid.
///
/// # What the assertion proves, and what it does not
///
/// `held.mp4`'s object is in **this machine's** store and in no remote store
/// anywhere, so a real download of it cannot succeed: `lfs::local::transfer`
/// from a filesystem remote that does not hold the object fails, and without the
/// gate the unit parks with the worktree still holding pointer text. So the
/// assertion that `held.mp4` holds its four mebibytes — and that its row is gone
/// rather than rescheduled — can only be satisfied by a publish that skipped the
/// download leg. That is the whole claim.
///
/// It does **not** prove anything about the HTTP path: this fixture's remote is a
/// filesystem store, so what a regression here would have spent is
/// `copy_lfs_object`'s file copy rather than an endpoint resolution and a batch
/// POST. The gate is common to both — it precedes the `.lfsconfig` read — and
/// this is the arrangement in which "a transfer would have failed" is a fact
/// rather than a count somebody has to trust.
///
/// # Why the drain is driven by a request for a *different* path
///
/// A request for `held.mp4` itself would never reach the transfer leg:
/// `materialize_held` finds the object in the store and publishes inline, which
/// is the arm [`an_object_already_in_the_store_is_published_and_the_tree_stays_clean`]
/// covers. So the fixture asks for `clip.mp4`, whose object really is only
/// upstream, and `held.mp4`'s background row is claimed in the same batch — which
/// is also how the defect was reached in production, one row among many.
#[tokio::test]
async fn a_queued_download_the_store_already_holds_costs_no_transfer() {
    let work = tempfile::tempdir().expect("tempdir");
    let remote = tempfile::tempdir().expect("remote");
    let data = tempfile::tempdir().expect("data dir");
    let root = work.path();
    let Some((engine, platform, requested)) =
        remote_fixture(root, remote.path(), data.path(), true)
    else {
        return;
    };

    // A second tracked path whose object is in THIS machine's store and nowhere
    // else. Distinct filler bytes, so the assertion below cannot be satisfied by
    // `clip.mp4`'s content arriving at the wrong path.
    let local = LfsStore::in_git_dir(root.join(".git"));
    let (oid, size) = local
        .insert_streaming(std::io::Cursor::new(vec![7u8; CONTENT_BYTES as usize]))
        .expect("insert the object into THIS machine's store");
    let held = Pointer::new(oid, size);
    std::fs::write(root.join("held.mp4"), held.render()).expect("check out the pointer");
    commit(
        &git::repo::open(root, false).expect("open the fixture repository"),
        &git::commit::StagedChange {
            added: vec![std::path::PathBuf::from("held.mp4")],
            ..Default::default()
        },
    );
    assert!(
        !LfsStore::in_git_dir(remote.path()).contains(&held.oid, held.size),
        "the remote must NOT hold this object: a transfer that could succeed \
         would make the assertion below pass either way"
    );

    // Queued as a background scan queues one — no urgency — and named, because
    // `materialize_landed` defers an unlabelled row to the next whole-tree sweep
    // and a claim about what a drain publishes has to be a claim about a row that
    // says which path it is for.
    let unit = enqueue_download(&platform, &held.oid, held.size);
    label_download(&platform, unit, "held.mp4");

    let done = engine
        .materialize_entry_now("01JMATERIALIZE", "clip.mp4", SyncSource::Cli)
        .await
        .expect("the remote holds the requested object");
    assert_eq!(
        done.outcome,
        MaterializeOutcome::Materialized,
        "the requested path is the drain's reason for running at all"
    );
    assert_eq!(done.oid, requested.oid);

    assert_eq!(
        std::fs::read(root.join("held.mp4")).expect("read the published file"),
        vec![7u8; CONTENT_BYTES as usize],
        "the background row was satisfied out of the store it was already in — \
         a real download of this object cannot succeed anywhere in this fixture"
    );
    assert!(
        download_rows(&platform).is_empty(),
        "and both rows are gone: `db::complete` deletes a drained unit, so the \
         work is retired rather than parked behind a backoff"
    );

    // The real `git` binary agrees about both published paths, deciding on the
    // stat rather than on a content comparison — see `stamp_index_forward`.
    stamp_index_forward(root);
    assert_eq!(
        porcelain(root),
        "",
        "and neither published path reads modified"
    );
}

/// A transfer that genuinely cannot land **terminates**, is reported as still
/// queued, and does not have its backoff cancelled on the way out.
///
/// Two claims, both about what the loop does when it cannot win.
///
/// *It stops.* The anti-spin guard is `sync_once`'s: drain until the profile's
/// outstanding-download count stops *decreasing*, never merely until it is
/// zero. A remote with no object makes no progress, so the loop ends. The await
/// is wrapped in a `timeout` deliberately: a regression here would otherwise
/// hang the whole suite and report as a runner timeout instead of as this claim
/// breaking.
///
/// *It writes nothing on the way to reporting.* The second `materialize_held`
/// pass runs in observing mode, so it does not re-enter `enqueue_unique` and
/// `promote_unit`. Asserted through `not_before_ms`, which `reschedule_after`
/// pushed into the future and `promote_unit` would pull back to now — with the
/// re-asking version of this code the row is immediately claimable again, so a
/// cron `materialize` resets the backoff on every invocation and hammers a dead
/// remote at the cron cadence with `Requested` urgency ahead of background work.
#[tokio::test]
async fn a_transfer_that_cannot_land_stops_without_cancelling_its_own_backoff() {
    let work = tempfile::tempdir().expect("tempdir");
    let remote = tempfile::tempdir().expect("remote");
    let data = tempfile::tempdir().expect("data dir");
    let root = work.path();
    let Some((engine, platform, pointer)) = remote_fixture(root, remote.path(), data.path(), false)
    else {
        return;
    };

    let done = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        engine.materialize_entry_now("01JMATERIALIZE", "clip.mp4", SyncSource::Cli),
    )
    .await
    .expect("the drain loop must terminate on a transfer that cannot land")
    .expect("a transfer that did not land is an outcome, not a raised error");

    assert_eq!(
        done.outcome,
        MaterializeOutcome::Queued,
        "nothing was delivered, so nothing may be reported as delivered"
    );
    let unit = done.unit_id.expect("and the unit that owes it is named");
    assert_eq!(
        std::fs::read(root.join("clip.mp4")).expect("read"),
        pointer.render().into_bytes(),
        "and the pointer is untouched"
    );

    let rows = download_rows(&platform);
    assert_eq!(
        rows.len(),
        1,
        "one row for one object, and the reporting pass added none: {rows:?}"
    );
    assert_eq!(
        rows[0].0, unit,
        "and it is the row the caller was told about"
    );
    let (state, not_before) =
        unit_schedule(&platform, unit).expect("the row that could not land is still there");
    assert_eq!(state, "pending", "a transient transfer failure retries");
    assert!(
        not_before > platform.now_ms(),
        "the backoff `reschedule_after` earned must survive the reporting pass; \
         `promote_unit` would have pulled it back to now (state={state}, \
         not_before={not_before})"
    );
}

/// The drain stops when **the requested unit** is settled, not when the folder
/// owes no more downloads.
///
/// The defect this was written against, found by review before it shipped: the
/// loop's only exit was the profile's whole outstanding-download count, and
/// `materialize_pending` queues one row per unfetched pointer after every pull.
/// With `CLAIM_LIMIT` at 16, the requested row completes on the first pass while
/// the count merely falls by sixteen — a strict decrease — so the loop kept
/// going until the entire backlog was fetched. `materialize photos one.jpg`
/// downloaded the folder, holding the reservation throughout.
///
/// So the fixture queues **more background downloads than one batch can hold**,
/// which is the only arrangement that can tell the two loops apart: with the
/// per-unit exit some background rows are still there afterwards, and with the
/// count-based exit none are.
#[tokio::test]
async fn one_request_does_not_drain_the_folders_whole_download_backlog() {
    let work = tempfile::tempdir().expect("tempdir");
    let remote = tempfile::tempdir().expect("remote");
    let data = tempfile::tempdir().expect("data dir");
    let root = work.path();
    let Some((engine, platform, pointer)) = remote_fixture(root, remote.path(), data.path(), true)
    else {
        return;
    };

    // Two batches' worth of background transfers, each a real object the remote
    // can serve, so every one of them WOULD succeed if the loop reached it.
    // Tiny, because their bytes are irrelevant — only their rows are.
    let store = LfsStore::in_git_dir(remote.path());
    let mut background = Vec::new();
    for byte in 0u8..(2 * CLAIM_LIMIT) {
        let (oid, size) = store
            .insert_streaming(std::io::Cursor::new(vec![byte; 64]))
            .expect("seed one background object");
        background.push(enqueue_download(&platform, &oid, size));
    }

    let done = tokio::time::timeout(
        std::time::Duration::from_secs(120),
        engine.materialize_entry_now("01JMATERIALIZE", "clip.mp4", SyncSource::Cli),
    )
    .await
    .expect("the drain must not run the folder's whole queue")
    .expect("the remote holds the requested object");

    assert_eq!(done.outcome, MaterializeOutcome::Materialized);
    assert_eq!(
        std::fs::read(root.join("clip.mp4")).expect("read"),
        vec![9u8; CONTENT_BYTES as usize],
        "the path that was asked for holds its content"
    );

    let left: Vec<i64> = download_rows(&platform)
        .into_iter()
        .map(|(id, _, _)| id)
        .filter(|id| background.contains(id))
        .collect();
    assert!(
        !left.is_empty(),
        "the request was satisfied on the first pass, so the loop had no reason \
         to claim a second batch; {} background rows were queued and none \
         survived, which is the whole-backlog drain",
        background.len()
    );
    assert_eq!(pointer.size, CONTENT_BYTES);
}

/// The drain runs **downloads only**, so this verb cannot commit, publish or
/// open a pull request on its way to fetching one file.
///
/// `Engine::execute` dispatches whatever kind it is handed, and the general
/// `drain_journal` claims by profile and state with no kind predicate. A `Push`
/// is queued on every local edit and a rejected push queues a `Pull`, so an
/// unnarrowed drain here would make a verb documented as "fetch one path's
/// content" merge the remote and publish the branch — and a `Pull` drained that
/// way can move the very pointer the request is about, under the observing pass
/// that follows.
#[tokio::test]
async fn the_request_drains_transfers_and_leaves_every_other_kind_alone() {
    let work = tempfile::tempdir().expect("tempdir");
    let remote = tempfile::tempdir().expect("remote");
    let data = tempfile::tempdir().expect("data dir");
    let root = work.path();
    let Some((engine, platform, _pointer)) = remote_fixture(root, remote.path(), data.path(), true)
    else {
        return;
    };
    let push = enqueue_push(&platform);

    let done = engine
        .materialize_entry_now("01JMATERIALIZE", "clip.mp4", SyncSource::Cli)
        .await
        .expect("the remote holds the requested object");
    assert_eq!(done.outcome, MaterializeOutcome::Materialized);

    assert_eq!(
        unit_schedule(&platform, push),
        Some(("pending".to_owned(), 0)),
        "the push is exactly as this verb found it: still pending, still at its \
         original schedule, and above all still THERE — `db::complete` is the \
         only thing that deletes a row, so a vanished push is a push this verb \
         performed"
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
