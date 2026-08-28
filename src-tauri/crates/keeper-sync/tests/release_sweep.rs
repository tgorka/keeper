//! The automatic release sweep against a real repository, a real index, real
//! bytes and an injected clock (Story 56.5, FR-341, FR-342, AD-126, AD-131).
//!
//! `tests/dehydrate_entry.rs` settles what one *requested* release does. This
//! file settles the three things that only exist once nobody asks:
//!
//! 1. **Which paths are candidates at all.** The two clocks are selected by
//!    provenance, and the claim about a locally authored path the remote has
//!    never confirmed is that it is **not considered** — not that it was
//!    considered and refused. So the candidate set is asserted as a set, by
//!    asking [`Engine::release_due_at`] over the rows the real ledger holds.
//! 2. **When the sweep runs.** On the first successful sync after the TTL
//!    expires, and on nothing else. No timer, no thread, no second scheduler
//!    (AD-62) — so the test that proves it advances the clock a hundred times
//!    past the window with no sync happening and asserts the content is still
//!    there.
//! 3. **What one pass costs, and where the next one starts.** Bounded on both
//!    dimensions — [`RELEASE_BUDGET_OBJECTS`] and [`RELEASE_BUDGET_BYTES`] —
//!    with the remainder released by a later pass, which resumes where the
//!    last one stopped rather than at the top of the folder. Both fixtures are
//!    sized *from* those constants, so raising one cannot quietly turn "more
//!    candidates than a pass allows" into "fewer".
//! 4. **When the FIRST pass runs.** Never on the sync that first shows keeper
//!    this folder: the gate arms its window and declines, so a whole
//!    [`RELEASE_LOOK_EVERY_MS`] always separates keeper meeting a folder from
//!    deleting anything in it. Every test here that wants a release spends
//!    that look through [`Fixture::open_the_release_window`] and says so.
//!
//! **No test sleeps.** The clock is [`TestPlatform`]'s and `advance_ms` is the
//! only thing that moves it: a TTL proved by waiting would be asserting that
//! the machine is slow.
//!
//! **No network, and no resolver.** The remote is a bare repository in a temp
//! directory whose LFS store this fixture seeds directly, which is what makes
//! `Engine::remote_serves`' per-object, size-verified proof genuine rather than
//! mocked — the release really does take the proof NFR-40 requires.
//!
//! **No `filter=lfs` rule is registered**, the decision `tests/dehydrate_entry.rs`
//! documents: `git status` has to decide from the refreshed index stat, so a
//! broken refresh surfaces here as ` M` rather than being hidden inside a clean
//! filter.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use keeper_sync::db;
use keeper_sync::engine::{
    release_due_at, Engine, ReleaseSchedule, RELEASE_BUDGET_BYTES, RELEASE_BUDGET_OBJECTS,
    RELEASE_LOOK_EVERY_MS,
};
use keeper_sync::git;
use keeper_sync::lfs::pointer::Pointer;
use keeper_sync::lfs::store::LfsStore;
use keeper_sync::platform::{SyncPlatform, TestPlatform};
use keeper_sync::profile::{LfsMode, SyncProfile, DEFAULT_RELEASE_TTL_MS};
use keeper_sync::provenance::{Provenance, SyncSource};

/// What each pointer stands for. Small on purpose: the budget test seeds
/// `RELEASE_BUDGET_OBJECTS + 1` of these and every one of them is hashed twice
/// on its way through the content-identity proof.
const CONTENT_BYTES: u64 = 4096;

const PROFILE_ID: &str = "01JRELEASE";

/// The retention window every test here reasons about. Short and explicit, so
/// an assertion says "a minute past the window" rather than "a day and a bit".
const TTL_MS: u64 = 60 * 60 * 1000;

/// `false` when this machine has no usable `git`, in which case there is no
/// index to read and nothing here is falsifiable — the skip
/// `tests/dehydrate_entry.rs` makes, for its reason.
fn init_repo(root: &Path, remote: &Path) -> bool {
    let ok = std::process::Command::new("git")
        .args(["init", "-q", "-b", "main"])
        .current_dir(root)
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !ok {
        return false;
    }
    // Named explicitly rather than left to the engine: `open_repo` adopts a
    // folder that has no `.git` and opens one that has, so a fixture that
    // pre-initializes its repository has to attach its own origin or the sync
    // leg has nowhere to push.
    std::process::Command::new("git")
        .args(["remote", "add", "origin"])
        .arg(remote)
        .current_dir(root)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// The fixture profile: a **filesystem** remote, so the per-object proof is
/// real with no server anywhere, and [`LfsMode::PointerOnly`], because a folder
/// in the default mode re-materializes anything holding a pointer on the next
/// pass and the sweep's own mode gate declines it by name.
fn profile(root: &Path, remote: &Path) -> SyncProfile {
    let mut p = SyncProfile::new(
        PROFILE_ID,
        "media",
        root,
        remote.to_string_lossy().into_owned(),
    );
    p.lfs_threshold_bytes = 1024;
    p.lfs_mode = LfsMode::PointerOnly;
    p.release_ttl_ms = TTL_MS;
    // Kept out of keeper's *committer*, and only out of that.
    //
    // `seed` puts content into the worktree behind keeper's back, which no real
    // arrival ever does, so keeper's scan would meet every seeded path as new
    // local content and record it — correctly, this being Story 56.5's own
    // writer — as locally authored, overwriting the provenance each test is
    // about. The exclusion stops the scan and nothing else: the release sweep
    // reads the ledger and the index, neither of which consults this list, so
    // every guard the sweep runs is the guard it runs in production.
    p.excludes = vec!["*.mp4".to_owned()];
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

/// [`porcelain`] narrowed to one pathspec.
///
/// Needed by exactly one test, and for a reason worth writing down rather than
/// hiding behind a wider assertion. [`seed`] writes a path's content and its
/// index stat inside the same wall second, which no real arrival does, so
/// keeper's own status walk meets those entries as racily-clean, classifies
/// them modified and persists a **zeroed** stat size for them
/// (`git::repo::persist_observed_stats`). A path the test then releases is
/// re-stat'd on its way through the release and reads clean; a path the test
/// deliberately leaves materialized keeps the zeroed stat, so `git status`
/// distrusts it, hashes the content and reports ` M`. In production keeper's
/// committer repairs that on the next pass — this fixture excludes `*.mp4`
/// from the committer on purpose, which is what leaves it standing.
///
/// So the claim is made about the path the release touched, which is the path
/// the claim is about: pointer blob, worktree stat, clean status. Widening it
/// would be asserting something about the fixture instead.
fn porcelain_of(root: &Path, pathspec: &str) -> String {
    let out = std::process::Command::new("git")
        .args(["status", "--porcelain", "--", pathspec])
        .current_dir(root)
        .output()
        .expect("git status");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Put `names` into the state a materialized LFS path is really in: the
/// **pointer** committed as the index's blob, the **content** in the worktree,
/// and the entry's stat describing the content.
///
/// Seeded into the *remote's* store rather than this machine's, deliberately —
/// `tests/dehydrate_entry.rs`'s reason: `lfs_prune_local` deletes the local
/// copy precisely when the worktree holds the content, so a fixture that seeded
/// it locally would let a store precondition slip in unnoticed.
///
/// One commit for the whole set: the budget test seeds thirty-three paths, and
/// thirty-three commits would be thirty-three tree writes for no extra claim.
fn seed(root: &Path, remote: &Path, names: &[&str]) -> Vec<(Pointer, Vec<u8>)> {
    let store = LfsStore::in_git_dir(remote);
    let mut seeded = Vec::with_capacity(names.len());
    for (index, name) in names.iter().enumerate() {
        // Distinct content per path, so no two share an oid and a release of
        // one cannot be mistaken for a release of another.
        let content = vec![index as u8 ^ 0x5a; CONTENT_BYTES as usize];
        let (oid, size) = store
            .insert_streaming(std::io::Cursor::new(content.clone()))
            .expect("seed the remote's own LFS store");
        // A real checkout makes the directories on its way down, and one test
        // needs a path in a subdirectory to be outside a sparse cone at all —
        // without this it would fail to write rather than fail to pin.
        if let Some(parent) = root.join(name).parent() {
            std::fs::create_dir_all(parent).expect("make room for the pointer");
        }
        std::fs::write(root.join(name), Pointer::new(&oid, size).render())
            .expect("check out the pointer");
        seeded.push((Pointer::new(oid, size), content));
    }

    // Committed while the worktree holds only the pointers, so each index blob
    // IS the pointer — the invariant this whole epic rests on.
    {
        let repo = git::repo::open(root, false).expect("open the fixture repository");
        commit(
            &repo,
            remote,
            &git::commit::StagedChange {
                added: names.iter().map(PathBuf::from).collect(),
                ..Default::default()
            },
        );
    }

    // ...and then the content lands, exactly as an arrival leaves it: real
    // bytes on disk, each entry's stat refreshed to match.
    for (name, (_, content)) in names.iter().zip(&seeded) {
        std::fs::write(root.join(name), content).expect("materialize the content");
    }
    {
        let repo = git::repo::open(root, false).expect("reopen");
        let paths: Vec<PathBuf> = names.iter().map(PathBuf::from).collect();
        git::repo::refresh_index_stat(&repo, &paths).expect("re-stat");
    }
    seeded
}

/// Stamp `.git/index` ten seconds into the future, with no keeper code
/// involved.
///
/// git's **racy-clean** rule: an index entry whose mtime is not strictly older
/// than the index file's own is not trusted on its cached stat, so git falls
/// back to comparing content and reports ` M` for the second a release lands
/// in, however correct the refreshed stat is. Moving the index file's timestamp
/// is what makes the assertion decide on the **stat**, and it is deliberately a
/// filesystem call rather than `Engine::rescan`: pressing keeper's repair
/// button would re-stat every entry and the assertion would then pass with the
/// sweep's own `refresh_index_stat` deleted.
fn stamp_index_forward(root: &Path) {
    let when = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
    std::fs::File::options()
        .write(true)
        .open(root.join(".git/index"))
        .and_then(|index| index.set_modified(when))
        .expect("stamp the index forward");
}

/// The engine, registered against a profile the caller has already shaped.
///
/// Every test here reaches it through [`fixture`], which is the only thing
/// that knows how to build a repository and its remote together.
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

fn ledger_rows(platform: &TestPlatform) -> Vec<db::MaterializedRow> {
    db::materialized_rows(&ledger_conn(platform), PROFILE_ID).expect("read the ledger")
}

fn ledger_paths(platform: &TestPlatform) -> Vec<String> {
    ledger_rows(platform)
        .into_iter()
        .map(|row| row.path)
        .collect()
}

/// The paths the sweep would consider right now, derived exactly as the sweep
/// derives them: the pure predicate, over the rows the real ledger holds.
///
/// This is what AC 2 asks for. "The file was not released" would also be true
/// of a path that was considered and refused, and the whole point of AD-131 is
/// that an unconfirmed locally authored path must not reach 56.4's guards at
/// all.
fn candidates(platform: &TestPlatform, now_ms: i64) -> Vec<String> {
    ledger_rows(platform)
        .into_iter()
        .filter(|row| release_due_at(row, TTL_MS).is_some_and(|due| now_ms >= due))
        .map(|row| row.path)
        .collect()
}

/// The committed blob's own bytes, read straight out of the index.
fn committed_blob(root: &Path, name: &str) -> Vec<u8> {
    let repo = git::repo::open(root, false).expect("open");
    keeper_sync::lfs::stage::indexed_pointer_blob(&repo, Path::new(name))
        .expect("the index records a pointer for this path")
        .1
}

#[cfg(unix)]
fn inode(path: &Path) -> u64 {
    use std::os::unix::fs::MetadataExt as _;
    std::fs::symlink_metadata(path).expect("stat").ino()
}

/// One complete fixture: repository, bare remote, engine, and the seeded paths.
struct Fixture {
    _work: tempfile::TempDir,
    _remote: tempfile::TempDir,
    _data: tempfile::TempDir,
    root: PathBuf,
    engine: Engine,
    platform: Arc<TestPlatform>,
    seeded: Vec<(Pointer, Vec<u8>)>,
    /// The seeded names in the order [`seed`] took them, so a test asks for a
    /// path's pointer by name instead of counting.
    names: Vec<String>,
}

/// Build the whole fixture, or `None` on a machine with no usable `git`.
///
/// The remote is a **bare repository** as well as an LFS store, because the
/// invocation point under test is a *successful sync* — so the sync leg has to
/// really run, really push, and really reach `mark_synced`.
///
/// # Why the profile excludes the seeded names
///
/// [`seed`] puts content into the worktree behind keeper's back, which no real
/// arrival ever does. Keeper's scan would therefore meet every seeded path as
/// *new local content*, stage it, and — correctly, this being Story 56.5's own
/// writer — record it as **locally authored**, overwriting the provenance each
/// test is about. So [`profile`] excludes those names from keeper's *committer*
/// and from nothing else: the sweep reads the ledger and the index, neither of
/// which consults `excludes`, and each test writes the exact provenance it
/// asserts on through the same `db` writers production uses.
async fn fixture(names: &[&str], adjust: impl FnOnce(&mut SyncProfile)) -> Option<Fixture> {
    let work = tempfile::tempdir().expect("tempdir");
    let remote = tempfile::tempdir().expect("remote");
    if gix::init_bare(remote.path()).is_err() {
        return None;
    }
    let root = work.path().to_path_buf();
    if !init_repo(&root, remote.path()) {
        return None;
    }
    let seeded = seed(&root, remote.path(), names);

    let data = tempfile::tempdir().expect("data dir");
    let mut p = profile(&root, remote.path());
    adjust(&mut p);
    let (engine, platform) = engine_with(p, data.path())?;
    let f = Fixture {
        _work: work,
        _remote: remote,
        _data: data,
        root,
        engine,
        platform,
        seeded,
        names: names.iter().map(|name| (*name).to_owned()).collect(),
    };
    Some(f)
}

impl Fixture {
    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    /// One successful sync — the edge the sweep rides.
    async fn sync(&self) {
        self.engine
            .sync_once(PROFILE_ID, SyncSource::Manual)
            .await
            .expect("the fixture's remote is a real bare repository, so a sync succeeds");
    }

    fn last_sync_ms(&self) -> Option<i64> {
        self.engine
            .status(PROFILE_ID)
            .expect("the profile is registered")
            .last_sync_ms
    }

    /// Move past the release look-gate's window, so a later sync looks again.
    fn skip_the_look_window(&self) {
        self.platform.advance_ms(RELEASE_LOOK_EVERY_MS);
    }

    /// Spend the sweep's **first look** at this folder, so that a later sync
    /// really sweeps.
    ///
    /// The gate arms a window and declines the first time it is asked about a
    /// folder (Story 56.5): a keeper that has never swept this one — freshly
    /// added, just resumed, re-enabled, or a daemon that has only just
    /// restarted — gets one whole [`RELEASE_LOOK_EVERY_MS`] of grace before
    /// its first deletion. `scan_is_due` answers yes on first sight and this
    /// gate does not, deliberately: a scan is a read, and this pass deletes.
    /// So the first successful sync of a profile's life releases nothing
    /// whatever the clocks say, and a test that wants a release has to spend
    /// that look first.
    ///
    /// Called **before** a test writes the provenance it asserts on rather
    /// than after, because this window and [`TTL_MS`] are the same hour:
    /// skipping it afterwards would move the retention clock too, and a test
    /// about the edge of the retention window would stop being about it. The
    /// sync spent here runs while the ledger is still empty, so it is a look
    /// that could not have released anything even had the gate allowed it.
    async fn open_the_release_window(&self) {
        self.sync().await;
        self.skip_the_look_window();
    }

    /// Where `name` sits in the seeded set, so a test names a path instead of
    /// counting to it.
    fn index_of(&self, name: &str) -> usize {
        self.names
            .iter()
            .position(|seeded| seeded == name)
            .expect("this fixture seeded that path")
    }

    /// The pointer the index records for `name`: the object identity and the
    /// size an arrival has in hand at the moment it writes its ledger row.
    fn pointer(&self, name: &str) -> &Pointer {
        &self.seeded[self.index_of(name)].0
    }

    /// The bytes `name`'s object actually is.
    fn content(&self, name: &str) -> &[u8] {
        &self.seeded[self.index_of(name)].1
    }

    /// Record `path` as content that ARRIVED here, through the writers the
    /// arrival paths themselves use.
    ///
    /// The oid and the size are the seeded pointer's own, because the arrival
    /// is the one moment the pointer is in hand — and the size is the very
    /// column the sweep's byte budget adds up, so a fixture that left it
    /// `NULL` would make that budget untestable.
    fn arrived(&self, path: &str, at_ms: i64) {
        self.arrived_declaring(path, at_ms, self.pointer(path).size);
    }

    /// [`Self::arrived`] with the size the ledger records overridden.
    ///
    /// For the byte budget alone. `size_bytes` is the **ledger's own number**
    /// and the sweep's accumulator reads that column rather than measuring the
    /// file, so a declared gigabyte costs this suite nothing on disk: the real
    /// bytes stay [`CONTENT_BYTES`] small on purpose. Every guard past the
    /// accumulator works off the committed pointer, which is the honest size,
    /// so the release itself is as real as everywhere else here.
    fn arrived_declaring(&self, path: &str, at_ms: i64, size_bytes: u64) {
        let conn = ledger_conn(&self.platform);
        db::remember_materialized(&conn, PROFILE_ID, path, at_ms)
            .expect("record the materialization");
        db::note_arrival(
            &conn,
            PROFILE_ID,
            path,
            at_ms,
            &self.pointer(path).oid,
            size_bytes,
        )
        .expect("record where it came from");
    }

    /// Record `path` as content THIS CLONE wrote, through the writer
    /// `commit_local`'s upload loop uses.
    fn authored(&self, path: &str, at_ms: i64) {
        let conn = ledger_conn(&self.platform);
        let pointer = self.pointer(path);
        db::remember_materialized(&conn, PROFILE_ID, path, at_ms)
            .expect("record the materialization");
        db::note_local_authorship(&conn, PROFILE_ID, path, at_ms, &pointer.oid, pointer.size)
            .expect("record the authorship");
    }
}

// ---------------------------------------------------------------------------
// The two clocks
// ---------------------------------------------------------------------------

/// Content that arrived from the remote goes a TTL after its last use.
///
/// The whole happy path, and every assertion fails for a different real bug:
/// the bytes (a re-rendered pointer rather than the committed blob), the inode
/// (a truncation rather than a rename), the ledger (a row still claiming this
/// machine holds content it does not), and `git status` (the one-path re-stat
/// not happening).
#[tokio::test]
#[cfg(unix)]
async fn content_that_arrived_from_the_remote_goes_a_ttl_after_its_last_use() {
    let Some(f) = fixture(&["clip.mp4"], |_| {}).await else {
        return;
    };
    // The sweep's first look at a folder arms its window and releases nothing,
    // so that look is spent here — before the arrival is recorded, because the
    // look window and the retention window are the same hour.
    f.open_the_release_window().await;

    let used_at = f.platform.now_ms();
    f.arrived("clip.mp4", used_at);

    let blob = committed_blob(&f.root, "clip.mp4");
    let target = f.path("clip.mp4");
    let before = inode(&target);
    assert_eq!(
        std::fs::read(&target).expect("read"),
        f.content("clip.mp4"),
        "the fixture starts materialized, or there is nothing to release"
    );

    f.platform.advance_ms(TTL_MS as i64);
    assert_eq!(
        candidates(&f.platform, f.platform.now_ms()),
        vec!["clip.mp4".to_owned()],
        "exactly at the deadline the path is a candidate"
    );

    f.sync().await;

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
        ledger_paths(&f.platform).is_empty(),
        "the ledger no longer claims this machine holds the content"
    );

    stamp_index_forward(&f.root);
    assert_eq!(
        porcelain(&f.root),
        "",
        "and the real git binary calls the released path clean"
    );
}

/// One millisecond inside the window is inside the window.
#[tokio::test]
async fn content_still_inside_its_window_is_not_a_candidate_and_is_not_touched() {
    let Some(f) = fixture(&["clip.mp4"], |_| {}).await else {
        return;
    };
    // Spent first, so the sweep below really looks. Without it this test would
    // pass with the whole retention comparison deleted: the first-look grace
    // alone would keep the file, and the claim being made is that ONE
    // MILLISECOND keeps it.
    f.open_the_release_window().await;

    f.arrived("clip.mp4", f.platform.now_ms());

    f.platform.advance_ms(TTL_MS as i64 - 1);
    assert!(
        candidates(&f.platform, f.platform.now_ms()).is_empty(),
        "one millisecond short is short"
    );

    f.sync().await;

    assert_eq!(
        std::fs::read(f.path("clip.mp4")).expect("read"),
        f.content("clip.mp4"),
        "nothing was read and nothing was written"
    );
    assert_eq!(ledger_paths(&f.platform), vec!["clip.mp4".to_owned()]);
}

/// A row written before this story has no `last_used_ms`, and must not be
/// inert forever.
///
/// `at_ms` is `NOT NULL` and is a real instant the content was here, so it is
/// the fallback. Without it every folder that upgraded into this release would
/// hold every byte it has for good.
#[tokio::test]
async fn a_row_with_no_recorded_use_falls_back_to_when_the_content_landed() {
    let Some(f) = fixture(&["clip.mp4"], |_| {}).await else {
        return;
    };
    // Spent first: the release below is the assertion, so the sweep has to be
    // past its first look by the time it happens.
    f.open_the_release_window().await;

    // `remember_materialized` alone: the shape of every row written before
    // Story 56.5 existed.
    db::remember_materialized(
        &ledger_conn(&f.platform),
        PROFILE_ID,
        "clip.mp4",
        f.platform.now_ms(),
    )
    .expect("a pre-56.5 row");
    assert_eq!(
        ledger_rows(&f.platform)[0].last_used_ms,
        None,
        "the fixture really is a row from before the clocks existed"
    );

    f.platform.advance_ms(TTL_MS as i64);
    assert_eq!(
        candidates(&f.platform, f.platform.now_ms()),
        vec!["clip.mp4".to_owned()],
        "measured from `at_ms`, so an upgraded folder is not inert"
    );

    f.sync().await;
    assert!(ledger_paths(&f.platform).is_empty(), "released");
}

/// **The data-loss case.** Content this clone wrote and the remote has never
/// confirmed is not a candidate at any age, and a use does not rescue it.
///
/// Asserted on the candidate set rather than on the absence of a release: "it
/// was not released" would also be true of a path that reached 56.4's guards
/// and was refused there, and AD-131 exists precisely so that the barrier is
/// not one guard deep.
#[tokio::test]
async fn an_unconfirmed_locally_authored_path_is_never_a_candidate() {
    let Some(f) = fixture(&["clip.mp4"], |_| {}).await else {
        return;
    };
    // Spent first, so the sweep below is a real one. This test's claim is that
    // the guard keeps the file, and a pass that was never allowed to run would
    // keep it too.
    f.open_the_release_window().await;

    let wrote_at = f.platform.now_ms();
    f.authored("clip.mp4", wrote_at);

    // A hundred times the window, and a use inside it — the two things that
    // would each be enough to make an arrived path eligible.
    f.platform.advance_ms(TTL_MS as i64 * 100);
    db::note_use(
        &ledger_conn(&f.platform),
        PROFILE_ID,
        "clip.mp4",
        f.platform.now_ms(),
    )
    .expect("the owner opened their own file");
    f.platform.advance_ms(TTL_MS as i64 * 100);

    let row = ledger_rows(&f.platform).remove(0);
    assert!(row.local_origin, "the fixture is a locally authored row");
    assert_eq!(
        row.synced_at_ms, None,
        "and the remote has never been observed holding these bytes"
    );
    assert!(
        candidates(&f.platform, f.platform.now_ms()).is_empty(),
        "so it is absent from the candidate set — not refused later, never considered"
    );

    f.sync().await;

    assert_eq!(
        std::fs::read(f.path("clip.mp4")).expect("read"),
        f.content("clip.mp4"),
        "and a real sync leaves the only copy of these bytes exactly where it is"
    );
    assert_eq!(ledger_paths(&f.platform), vec!["clip.mp4".to_owned()]);
}

/// Once the remote has confirmed it, the same path goes a TTL after that
/// instant — not a TTL after it was written.
#[tokio::test]
async fn a_locally_authored_path_goes_a_ttl_after_the_remote_confirmed_it() {
    let Some(f) = fixture(&["clip.mp4"], |_| {}).await else {
        return;
    };
    // Spent first: the release at the end is the assertion.
    f.open_the_release_window().await;

    f.authored("clip.mp4", f.platform.now_ms());

    // Time passes with no confirmation. Still not a candidate.
    f.platform.advance_ms(TTL_MS as i64 * 5);
    assert!(candidates(&f.platform, f.platform.now_ms()).is_empty());

    // The upload completes, which is the one moment the per-path proof exists.
    let confirmed_at = f.platform.now_ms();
    db::note_synced(
        &ledger_conn(&f.platform),
        PROFILE_ID,
        "clip.mp4",
        confirmed_at,
    )
    .expect("the unit that carried these bytes completed");
    assert!(
        candidates(&f.platform, f.platform.now_ms()).is_empty(),
        "the clock starts at the confirmation, so it is not immediately due"
    );

    f.platform.advance_ms(TTL_MS as i64);
    assert_eq!(
        candidates(&f.platform, f.platform.now_ms()),
        vec!["clip.mp4".to_owned()],
        "a TTL after the confirmation, and measured from it"
    );

    f.sync().await;
    assert!(ledger_paths(&f.platform).is_empty(), "released");
}

// ---------------------------------------------------------------------------
// The floors
// ---------------------------------------------------------------------------

/// A pin is absolute, and it is recorded through the verb an operator has.
///
/// Through `Engine::pin_entry` rather than a planted column, because a floor
/// nothing in production can raise is not a floor.
#[tokio::test]
async fn a_pin_is_an_absolute_floor_at_any_age() {
    let Some(f) = fixture(&["clip.mp4"], |_| {}).await else {
        return;
    };
    // Spent first, so the sync in the middle of this test is a real sweep.
    // Without it the pin would appear to hold a file that nothing had yet been
    // allowed to consider, and the floor would go untested.
    f.open_the_release_window().await;

    f.arrived("clip.mp4", f.platform.now_ms());
    let done = f
        .engine
        .pin_entry(PROFILE_ID, "clip.mp4", true)
        .expect("the path is tracked, so the pin is recorded");
    assert!(done.pinned);
    assert_eq!(done.path, "clip.mp4");

    f.platform.advance_ms(TTL_MS as i64 * 1_000);
    assert!(
        candidates(&f.platform, f.platform.now_ms()).is_empty(),
        "a pinned path is never a candidate, whatever the clock says"
    );

    f.sync().await;
    assert_eq!(
        std::fs::read(f.path("clip.mp4")).expect("read"),
        f.content("clip.mp4"),
        "the content is untouched"
    );
    assert_eq!(
        ledger_paths(&f.platform),
        vec!["clip.mp4".to_owned()],
        "and so is the row that carries the pin"
    );

    // Withdrawn, the path is an ordinary candidate again — the clocks were
    // never disturbed, so it is due the instant the pin lifts.
    f.engine
        .pin_entry(PROFILE_ID, "clip.mp4", false)
        .expect("unpin");
    assert_eq!(
        candidates(&f.platform, f.platform.now_ms()),
        vec!["clip.mp4".to_owned()]
    );
    f.skip_the_look_window();
    f.sync().await;
    assert!(
        ledger_paths(&f.platform).is_empty(),
        "and the next successful sync releases it"
    );
}

/// A pin against a path this folder does not track is refused, so a pin is
/// never recorded where no sweep will ever look.
#[tokio::test]
async fn pinning_an_untracked_path_is_refused_and_writes_nothing() {
    use keeper_sync::error::SyncError;
    use keeper_sync::lfs::hydrate::ContentRefusal;

    let Some(f) = fixture(&["clip.mp4"], |_| {}).await else {
        return;
    };
    std::fs::write(f.path("notes.md"), b"an ordinary file").expect("write");

    let err = f
        .engine
        .pin_entry(PROFILE_ID, "notes.md", true)
        .expect_err("this folder tracks no pointer for that path");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::NotTracked { path }) if path == "notes.md"
        ),
        "got: {err}"
    );
    assert!(
        ledger_rows(&f.platform).is_empty(),
        "and nothing was written, so no row exists for a sweep to consult"
    );
}

/// Withdrawing a pin from a path that has no ledger row writes nothing.
///
/// `set_pinned(false)` must not be the writer that CREATES a row. A row is a
/// claim that this machine holds the content, so an unpin that inserted one
/// would invent a candidate out of an instruction to stop protecting
/// something: the sweep would then consider a path whose content it never
/// recorded landing, and `release_due_at`'s fallback to `at_ms` would make
/// that phantom eligible a TTL later.
#[tokio::test]
async fn unpinning_a_path_with_no_row_writes_nothing() {
    let Some(f) = fixture(&["clip.mp4"], |_| {}).await else {
        return;
    };
    assert!(
        ledger_rows(&f.platform).is_empty(),
        "the fixture holds no materialization row for this path yet"
    );

    let done = f
        .engine
        .pin_entry(PROFILE_ID, "clip.mp4", false)
        .expect("the path is tracked, so the instruction itself is accepted");
    assert!(!done.pinned);

    assert!(
        ledger_rows(&f.platform).is_empty(),
        "and the unpin wrote nothing, so there is no phantom candidate for the \
         sweep to find a TTL from now"
    );
}

/// A pin against a path outside this folder's sparse cone is refused.
///
/// Content outside the cone is deliberately not kept on this machine (Story
/// 27.2) and `release_resolved` refuses that path by name, so accepting the
/// pin would write a row no verb could ever act on — the same "never recorded
/// where nothing will consult it" reason the untracked refusal exists for.
#[tokio::test]
async fn pinning_a_path_outside_the_sparse_cone_is_refused_and_writes_nothing() {
    use keeper_sync::error::SyncError;
    use keeper_sync::lfs::hydrate::ContentRefusal;

    // In a subdirectory, because a file at the repository root is inside every
    // cone: git's own cone matching keeps the root files and the files along
    // the way to a cone root, so a path at the top could not be outside one.
    let Some(f) = fixture(&["outside/clip.mp4"], |p| {
        p.subpaths = vec!["kept".to_owned()];
    })
    .await
    else {
        return;
    };

    let err = f
        .engine
        .pin_entry(PROFILE_ID, "outside/clip.mp4", true)
        .expect_err("this folder keeps no content for that path");
    assert!(
        matches!(
            &err,
            SyncError::Refused(ContentRefusal::OutsideSubpaths { path })
                if path == "outside/clip.mp4"
        ),
        "got: {err}"
    );
    assert!(
        ledger_rows(&f.platform).is_empty(),
        "and nothing was written, so no row exists for a sweep to consult"
    );
}

/// `releaseTtlMs = 0` switches the sweep off, and switches it off **before the
/// clock is read**.
///
/// The second half is the load-bearing one: a gate that armed a window would
/// mean an operator turning the sweep back on an hour later finds it already
/// open and loses content on the very next sync.
///
/// Both halves run **two** passes with the look window skipped between them,
/// which is what makes them falsifiable. A zero that reached the gate would
/// arm a window on the first pass and delete on the second, so a test with one
/// pass in it would pass with the zero check deleted — the first-look grace
/// would be doing the work the zero check is supposed to do.
#[tokio::test]
async fn a_zero_ttl_releases_nothing_and_arms_no_window() {
    let Some(f) = fixture(&["clip.mp4"], |p| p.release_ttl_ms = 0).await else {
        return;
    };
    f.arrived("clip.mp4", f.platform.now_ms());
    f.platform.advance_ms(DEFAULT_RELEASE_TTL_MS as i64 * 10);

    f.sync().await;
    f.skip_the_look_window();
    f.sync().await;

    assert_eq!(
        std::fs::read(f.path("clip.mp4")).expect("read"),
        f.content("clip.mp4"),
        "nothing was released"
    );
    assert_eq!(ledger_paths(&f.platform), vec!["clip.mp4".to_owned()]);

    // And turning it on does not find an open window: the folder waits out a
    // fresh interval rather than going on the very next sync, which is the
    // whole reason the zero must not arm one.
    let mut on = profile(&f.root, Path::new("unused"));
    on.remote_url = f
        .engine
        .list_profiles()
        .expect("profiles")
        .remove(0)
        .remote_url;
    f.engine.upsert_profile(&on).expect("turn the sweep on");
    f.sync().await;
    assert_eq!(
        ledger_paths(&f.platform),
        vec!["clip.mp4".to_owned()],
        "the first sync after the knob is turned on is the sweep's first look \
         at this folder: it arms a fresh window and deletes nothing, however \
         overdue the path is by then"
    );

    f.skip_the_look_window();
    f.sync().await;
    assert!(
        ledger_paths(&f.platform).is_empty(),
        "and the pass after that window releases it — so the knob works, it \
         just does not work retroactively"
    );
}

// ---------------------------------------------------------------------------
// The budget
// ---------------------------------------------------------------------------

/// One pass releases exactly the object budget; the next releases the rest.
///
/// The fixture is sized from [`RELEASE_BUDGET_OBJECTS`] rather than from a
/// literal, so raising the constant cannot quietly turn "more candidates than
/// one pass allows" into "fewer".
#[tokio::test]
async fn the_object_budget_caps_one_pass_and_the_next_releases_the_remainder() {
    let names: Vec<String> = (0..=RELEASE_BUDGET_OBJECTS)
        .map(|n| format!("clip-{n:03}.mp4"))
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let Some(f) = fixture(&refs, |_| {}).await else {
        return;
    };
    // Spent first: this test is about what one real pass takes.
    f.open_the_release_window().await;

    let landed = f.platform.now_ms();
    for name in &names {
        f.arrived(name, landed);
    }

    f.platform.advance_ms(TTL_MS as i64);
    assert_eq!(
        candidates(&f.platform, f.platform.now_ms()).len(),
        RELEASE_BUDGET_OBJECTS + 1,
        "one more than a pass may take"
    );

    f.sync().await;
    assert_eq!(
        ledger_paths(&f.platform).len(),
        1,
        "exactly the budget went; the reservation is not held for the rest"
    );

    // The next successful sync, once the look window has reopened, takes it.
    f.skip_the_look_window();
    f.sync().await;
    assert!(
        ledger_paths(&f.platform).is_empty(),
        "and nothing is left permanently behind the budget"
    );
}

/// The byte ceiling stops a pass, and stops it **at** the candidate that
/// crosses rather than before it.
///
/// The spec's I/O matrix promises this dimension and only the object count was
/// ever asserted. It is the ceiling that bounds the real cost: the
/// content-identity proof reads every byte of every candidate, so a policy
/// edit making forty thousand paths eligible would otherwise turn one sync
/// into an hour of hashing.
///
/// The sizes here are **declared, not written**. `size_bytes` is the ledger's
/// own number and the accumulator adds up that column rather than measuring
/// the file, so this test plants more than half the whole ceiling in each row
/// and leaves the real bytes [`CONTENT_BYTES`] small on purpose — nothing in
/// this suite writes a gigabyte, and every guard past the accumulator works
/// off the committed pointer, which carries the honest size, so the releases
/// themselves are as real as everywhere else here. The declared size is
/// derived from [`RELEASE_BUDGET_BYTES`] rather than from a literal, so
/// raising the ceiling cannot quietly turn "past the budget" into "inside it".
///
/// The crossing candidate is released rather than skipped: an object larger
/// than the whole budget must still get its one attempt per pass, or it would
/// be unreleasable forever — the failure mode a `continue` here would create.
#[tokio::test]
async fn the_byte_budget_stops_the_pass_at_the_candidate_that_crosses_it() {
    let names = ["a-big.mp4", "b-big.mp4", "c-big.mp4"];
    let Some(f) = fixture(&names, |_| {}).await else {
        return;
    };
    // Spent first: this test is about what one real pass takes.
    f.open_the_release_window().await;

    // More than half the ceiling each, so one candidate is inside it and any
    // two together are past it: a correct pass stops after the second.
    let declared = RELEASE_BUDGET_BYTES / 2 + 1;
    let landed = f.platform.now_ms();
    for name in names {
        f.arrived_declaring(name, landed, declared);
    }

    f.platform.advance_ms(TTL_MS as i64);
    assert_eq!(
        candidates(&f.platform, f.platform.now_ms()).len(),
        names.len(),
        "all three are due, so it is the ceiling that stops the pass and not \
         the clock"
    );

    f.sync().await;
    assert_eq!(
        ledger_paths(&f.platform),
        vec!["c-big.mp4".to_owned()],
        "the crossing candidate got its one attempt and the pass stopped there"
    );

    f.skip_the_look_window();
    f.sync().await;
    assert!(
        ledger_paths(&f.platform).is_empty(),
        "and a later pass takes the rest, so nothing is stranded behind the \
         byte ceiling"
    );
}

/// A whole budget of paths that refuse every time cannot hide the paths after
/// them.
///
/// One pass resumes where the last one stopped rather than at the top of the
/// folder, and this is the fence for the starvation a plain "first N by path"
/// shape has: a folder whose first [`RELEASE_BUDGET_OBJECTS`] paths are
/// locally modified would otherwise shield every path behind them from every
/// pass forever, and the budget's own promise that the remainder waits for the
/// next pass would be false exactly when the front of the folder refuses.
///
/// The refusal is the cheapest deterministic one available — a local in-place
/// edit of the same length, so only the content-identity digest can tell —
/// and it refuses again on every pass, which is precisely the condition a
/// rotating cursor exists for.
#[tokio::test]
async fn a_later_pass_reaches_past_a_whole_budget_of_paths_that_always_refuse() {
    let mut names: Vec<String> = (0..RELEASE_BUDGET_OBJECTS)
        .map(|n| format!("a-refuses-{n:03}.mp4"))
        .collect();
    // Sorts after every one of them, and the sweep reads its candidates in
    // path order, so nothing short of a cursor can ever reach it.
    names.push("z-clean.mp4".to_owned());
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let Some(f) = fixture(&refs, |_| {}).await else {
        return;
    };
    // Spent first: this test needs two real passes.
    f.open_the_release_window().await;

    let landed = f.platform.now_ms();
    for name in &names {
        f.arrived(name, landed);
    }
    let edited = vec![0xAAu8; CONTENT_BYTES as usize];
    assert!(
        names
            .iter()
            .take(RELEASE_BUDGET_OBJECTS)
            .all(|name| f.content(name) != edited.as_slice()),
        "every fixture edit has to be an edit"
    );
    for name in names.iter().take(RELEASE_BUDGET_OBJECTS) {
        std::fs::write(f.path(name), &edited).expect("edit in place");
    }

    f.platform.advance_ms(TTL_MS as i64);
    f.sync().await;
    assert_eq!(
        ledger_paths(&f.platform).len(),
        RELEASE_BUDGET_OBJECTS + 1,
        "the whole first window refused, so that pass released nothing at all"
    );

    f.skip_the_look_window();
    f.sync().await;
    assert_eq!(
        std::fs::read(f.path("z-clean.mp4")).expect("read"),
        committed_blob(&f.root, "z-clean.mp4"),
        "the pass after the refusing window reached past the whole of it and \
         released the path behind it"
    );
    assert_eq!(
        ledger_paths(&f.platform).len(),
        RELEASE_BUDGET_OBJECTS,
        "and only the paths that refused are left"
    );
}

// ---------------------------------------------------------------------------
// Failures and refusals never fail the sync
// ---------------------------------------------------------------------------

/// A candidate whose content cannot be read raises a real error, and the sync
/// still succeeds.
///
/// `prune_lfs_store`'s contract, verbatim: housekeeping that could stop a
/// folder syncing would have traded a disk-space problem for a sync problem.
/// `last_sync_ms` moving is the assertion that the pass really did report
/// success rather than being swallowed somewhere earlier.
#[tokio::test]
#[cfg(unix)]
async fn a_candidate_that_cannot_be_read_is_logged_and_the_sync_still_succeeds() {
    use std::os::unix::fs::PermissionsExt as _;

    let Some(f) = fixture(&["clip.mp4"], |_| {}).await else {
        return;
    };
    // Spent first: an error swallowed by a pass that was never allowed to run
    // is not the contract being asserted.
    f.open_the_release_window().await;

    f.arrived("clip.mp4", f.platform.now_ms());
    f.platform.advance_ms(TTL_MS as i64);
    // What the window-opening sync recorded, so the assertion below is that
    // THIS pass reported success rather than that some earlier one did.
    let before = f.last_sync_ms().expect("the window-opening sync succeeded");

    let target = f.path("clip.mp4");
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o000)).expect("chmod 000");

    f.sync().await;

    // Restored before the assertions, so a failing one still leaves a
    // directory the harness can delete.
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o644)).expect("restore");
    assert!(
        f.last_sync_ms().is_some_and(|at| at > before),
        "the pass reported success: a release that could not be read is \
         housekeeping that did not happen, never a sync that failed"
    );
    assert_eq!(
        std::fs::read(&target).expect("read"),
        f.content("clip.mp4"),
        "and the unreadable file is exactly as it was"
    );
    assert_eq!(ledger_paths(&f.platform), vec!["clip.mp4".to_owned()]);
}

/// A hard error on one candidate does not starve the candidates after it.
///
/// The single-candidate test above asserts that the pass reports success; with
/// one candidate it cannot see whether the pass CONTINUED. This is that half.
/// A real `SyncError::Io` out of the content-identity hash — not a refusal,
/// which the loop is already documented to swallow — must not abandon
/// everything sorted after it, or one unreadable file at the front of a folder
/// would silently stop the sweep from ever reaching anything again.
#[tokio::test]
#[cfg(unix)]
async fn a_hard_error_on_one_candidate_does_not_stop_the_pass() {
    use std::os::unix::fs::PermissionsExt as _;

    let Some(f) = fixture(&["a-unreadable.mp4", "b-clean.mp4"], |_| {}).await else {
        return;
    };
    // Spent first: a pass that was never allowed to run cannot be starved.
    f.open_the_release_window().await;

    let landed = f.platform.now_ms();
    f.arrived("a-unreadable.mp4", landed);
    f.arrived("b-clean.mp4", landed);
    f.platform.advance_ms(TTL_MS as i64);
    let before = f.last_sync_ms().expect("the window-opening sync succeeded");

    // Mode 000, so the hash of the actual bytes fails with an I/O error rather
    // than deciding anything — and on the alphabetically FIRST candidate,
    // which is the one the pass meets before the one it can release.
    let unreadable = f.path("a-unreadable.mp4");
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o000))
        .expect("chmod 000");

    f.sync().await;

    // Restored before the assertions, so a failing one still leaves a
    // directory the harness can delete.
    std::fs::set_permissions(&unreadable, std::fs::Permissions::from_mode(0o644))
        .expect("restore the mode");
    assert_eq!(
        ledger_paths(&f.platform),
        vec!["a-unreadable.mp4".to_owned()],
        "the pass carried on past the error and released the candidate after it"
    );
    assert!(
        f.last_sync_ms().is_some_and(|at| at > before),
        "and the sync still reported success"
    );
    assert_eq!(
        std::fs::read(&unreadable).expect("read"),
        f.content("a-unreadable.mp4"),
        "while the file it could not read is byte for byte as it was"
    );
}

/// A 56.4 refusal leaves its file intact and the pass moves to the next
/// candidate.
///
/// A refusal is the *expected* answer for most candidates, so it must not stop
/// the pass — otherwise one locally modified path would shield every path
/// after it forever.
#[tokio::test]
async fn a_refusal_leaves_its_file_intact_and_the_pass_continues() {
    let Some(f) = fixture(&["a-edited.mp4", "b-clean.mp4"], |_| {}).await else {
        return;
    };
    // Spent first: both the refusal and the release after it need a real pass.
    f.open_the_release_window().await;

    let landed = f.platform.now_ms();
    f.arrived("a-edited.mp4", landed);
    f.arrived("b-clean.mp4", landed);

    // The first candidate is no longer the content this folder committed: the
    // same length, so only the content-identity digest can tell.
    let edited = vec![0xAAu8; CONTENT_BYTES as usize];
    assert_ne!(
        edited,
        f.content("a-edited.mp4"),
        "the fixture edit has to be an edit"
    );
    std::fs::write(f.path("a-edited.mp4"), &edited).expect("edit in place");

    f.platform.advance_ms(TTL_MS as i64);
    assert_eq!(
        candidates(&f.platform, f.platform.now_ms()).len(),
        2,
        "both are candidates; the refusal happens later, at the guards"
    );

    f.sync().await;

    assert_eq!(
        std::fs::read(f.path("a-edited.mp4")).expect("read"),
        edited,
        "the modified file is untouched — keeper does not delete an edit"
    );
    assert_eq!(
        ledger_paths(&f.platform),
        vec!["a-edited.mp4".to_owned()],
        "and the clean candidate after it was still attempted and released"
    );
}

/// A row whose worktree file is already the pointer is RETRACTED, not refused
/// every hour forever.
///
/// The row is a claim that this machine holds the content. Once the sweep has
/// proved the claim false — the worktree holds the committed pointer, which is
/// 56.4's `AlreadyPointer` — the honest answer is to forget the row, rather
/// than to spend one of a few dozen budget slots on it on every pass for the
/// rest of the folder's life and starve the paths behind it.
#[tokio::test]
async fn the_sweep_retracts_a_row_whose_content_is_no_longer_here() {
    let Some(f) = fixture(&["clip.mp4"], |_| {}).await else {
        return;
    };
    // Spent first: this test needs two real passes.
    f.open_the_release_window().await;

    f.arrived("clip.mp4", f.platform.now_ms());
    f.platform.advance_ms(TTL_MS as i64);
    f.sync().await;
    assert!(
        ledger_paths(&f.platform).is_empty(),
        "released, so the worktree holds the pointer and not the content"
    );

    // The state a crash between the rename and the row delete leaves, and the
    // state `materialize_entry`'s already-held arm leaves: a row claiming
    // content this machine does not have.
    f.arrived("clip.mp4", f.platform.now_ms());
    assert_eq!(
        ledger_paths(&f.platform),
        vec!["clip.mp4".to_owned()],
        "the fixture really did put a false claim back into the ledger, so the \
         assertion below is not vacuous"
    );
    f.platform.advance_ms(TTL_MS as i64);
    f.skip_the_look_window();
    f.sync().await;

    assert!(
        ledger_paths(&f.platform).is_empty(),
        "the sweep retracted the row rather than refusing `AlreadyPointer` for \
         it and burning a budget slot every hour for good"
    );
    assert_eq!(
        std::fs::read(f.path("clip.mp4")).expect("read"),
        committed_blob(&f.root, "clip.mp4"),
        "and nothing was written over the pointer on the way"
    );
}

// ---------------------------------------------------------------------------
// The invocation point
// ---------------------------------------------------------------------------

/// Nothing is on a timer. The clock alone releases nothing.
///
/// AD-62 forbids a second scheduler over one git repository absolutely, so the
/// sweep rides an edge that was going to happen anyway. This is that claim
/// asserted behaviourally: a thousand windows go by, nothing happens, and then
/// one successful sync does the whole thing.
///
/// The one sync that opens the look window runs **before** the ledger holds
/// any row at all, so it cannot be the pass that released anything; after it,
/// a thousand retention windows go by with no sync whatsoever.
#[tokio::test]
async fn the_clock_alone_releases_nothing_until_a_sync_succeeds() {
    let Some(f) = fixture(&["clip.mp4"], |_| {}).await else {
        return;
    };
    f.open_the_release_window().await;

    f.arrived("clip.mp4", f.platform.now_ms());

    f.platform.advance_ms(TTL_MS as i64 * 1_000);
    assert_eq!(
        candidates(&f.platform, f.platform.now_ms()),
        vec!["clip.mp4".to_owned()],
        "long overdue by every measure the sweep has"
    );
    assert_eq!(
        std::fs::read(f.path("clip.mp4")).expect("read"),
        f.content("clip.mp4"),
        "and still here, because no pass has succeeded: no timer, no thread, \
         no second scheduler"
    );

    f.sync().await;

    assert!(
        ledger_paths(&f.platform).is_empty(),
        "the first successful sync after the window is what releases it"
    );
}

/// A paused folder releases nothing, however far past the window the clock is.
///
/// The correct direction, and the one this story must not get wrong: "paused"
/// means keeper is not touching this folder, and a deletion is the most
/// touching thing it does. Asserted here through the real repository and the
/// real bytes rather than only through the gate's own bookkeeping, because
/// this is the story whose bug deletes the only copy of a file.
///
/// Two passes with the look window skipped between them: one pass alone would
/// be declined by the first-look grace, so a single-pass version of this test
/// would still pass with the pause check deleted.
#[tokio::test]
async fn a_paused_folder_releases_nothing_and_keeps_its_bytes() {
    let Some(f) = fixture(&["clip.mp4"], |p| p.enabled = false).await else {
        return;
    };
    f.arrived("clip.mp4", f.platform.now_ms());
    f.platform.advance_ms(TTL_MS as i64 * 100);

    f.sync().await;
    f.skip_the_look_window();
    f.sync().await;

    assert_eq!(
        std::fs::read(f.path("clip.mp4")).expect("read"),
        f.content("clip.mp4"),
        "the bytes are exactly where they were"
    );
    assert_eq!(
        ledger_paths(&f.platform),
        vec!["clip.mp4".to_owned()],
        "and so is the row that would have gone with them"
    );
}

// ---------------------------------------------------------------------------
// What a surface is told
// ---------------------------------------------------------------------------

/// A surface is handed exactly the instant the sweep would use, and a pin
/// replaces it with words (Story 56.9, FR-343).
///
/// `Engine::release_schedules` is the Files pane's only source of a deadline,
/// so the claim worth asserting through a real engine, a real repository and a
/// real ledger is that it agrees with `release_due_at` — the predicate the
/// sweep filters candidates with — on the very row the sweep would read. A
/// classifier that re-derived 56.5's clock selection would pass a unit test
/// over hand-built rows and still show a countdown to an instant nothing
/// releases anything at.
///
/// The pin half is taken through `Engine::pin_entry`, the verb an operator
/// actually has, rather than a planted column: the whole point of the words is
/// that they appear on the row a person just pinned.
///
/// No sync and no sweep here, deliberately. This is a read, the look-gate has
/// nothing to do with it, and spending the first look would move the clock the
/// deadline is measured from.
#[tokio::test]
async fn a_surface_is_told_the_instant_the_sweep_would_use_and_a_pin_replaces_it() {
    let Some(f) = fixture(&["clip.mp4"], |_| {}).await else {
        return;
    };
    let landed = f.platform.now_ms();
    f.arrived("clip.mp4", landed);

    let schedules = f
        .engine
        .release_schedules(PROFILE_ID)
        .expect("the profile is registered");
    let schedule = schedules
        .get("clip.mp4")
        .expect("the ledger's own path spelling, the same key `materialized_paths` hands back");
    assert_eq!(
        *schedule,
        ReleaseSchedule::Due {
            at_ms: landed + TTL_MS as i64
        },
        "a TTL after the instant the ledger row names, and not a millisecond else"
    );

    // The stronger claim: the same instant the sweep itself would compare
    // against, taken from the same row through the sweep's own predicate.
    let row = ledger_rows(&f.platform)
        .into_iter()
        .find(|row| row.path == "clip.mp4")
        .expect("the ledger holds the row a surface was just told about");
    assert_eq!(
        schedule.releases_after_ms(),
        release_due_at(&row, TTL_MS),
        "the pane counts down to the instant the sweep acts on"
    );
    assert!(
        schedule.hold().is_none(),
        "a row on a clock draws a figure, so it carries no word instead"
    );

    // Pinned through the verb, and the countdown is gone rather than frozen:
    // there is no instant left to count down to, and the words say which of the
    // four reasons this is.
    f.engine
        .pin_entry(PROFILE_ID, "clip.mp4", true)
        .expect("the path is tracked, so the pin is recorded");
    let pinned = f
        .engine
        .release_schedules(PROFILE_ID)
        .expect("the profile is registered");
    let pinned = pinned.get("clip.mp4").expect("the row is still there");
    assert_eq!(*pinned, ReleaseSchedule::Pinned);
    assert_eq!(
        pinned.releases_after_ms(),
        None,
        "a pinned row has no deadline to draw, at any age"
    );
    assert_eq!(pinned.hold(), Some("Pinned"));
}

// ---------------------------------------------------------------------------
// The mode gate, and the policy that lifts it per path (Story 56.10)
// ---------------------------------------------------------------------------

/// The sweep releases a path the folder's **committed** policy authorizes, in
/// the mode every folder starts in (Story 56.10, FR-328, FR-335, AD-122,
/// AD-123).
///
/// Everything else in this file runs on [`LfsMode::PointerOnly`], because that
/// was the only mode `release_mode_gate` returned `Ok` for — so 56.5's sweep,
/// on a default folder, declined before it read a clock and released nothing,
/// ever. A user who set `releaseTtlMs` and never touched `lfsMode` had a TTL
/// that did nothing at all.
///
/// Four claims in one pass, and each fails for a different bug:
///
/// * the **authorized** path is released — the gate lifts;
/// * the **unauthorized** path beside it is untouched — the gate lifted for a
///   path, not for a folder;
/// * `git status` still reads clean past [`stamp_index_forward`] — the released
///   entry's stat was repaired, in a mode this had never run in;
/// * a **second** sync leaves the released path a pointer — which is the whole
///   reason the refusal existed. Before this story `materialize_pending` would
///   have copied the object straight back out of the store on the next pass, so
///   the release and the arrival would have fought forever. Now they read the
///   same policy.
///
/// The policy is the committed `.keepervirtual` here rather than the profile
/// tier, which `tests/dehydrate_entry.rs` uses: both tiers reach the same
/// compiled answer and both are worth having under a real sync. It is written
/// before the window-opening sync, so keeper's own committer picks it up and
/// the tree is clean before anything is asserted.
#[tokio::test]
#[cfg(unix)]
async fn a_default_mode_folder_releases_only_what_its_policy_authorizes() {
    let Some(f) = fixture(&["zone/clip.mp4", "keep/manual.mp4"], |p| {
        p.lfs_mode = LfsMode::Materialize;
    })
    .await
    else {
        return;
    };
    std::fs::write(
        f.root
            .join(keeper_sync::lfs::virtual_policy::VIRTUAL_PATTERN_FILE),
        "zone/**\n",
    )
    .expect("commit the repository's own answer");

    f.open_the_release_window().await;

    let used_at = f.platform.now_ms();
    f.arrived("zone/clip.mp4", used_at);
    f.arrived("keep/manual.mp4", used_at);

    let blob = committed_blob(&f.root, "zone/clip.mp4");
    let authorized = f.path("zone/clip.mp4");
    let before = inode(&authorized);

    f.platform.advance_ms(TTL_MS as i64);
    assert_eq!(
        candidates(&f.platform, f.platform.now_ms()),
        vec!["keep/manual.mp4".to_owned(), "zone/clip.mp4".to_owned()],
        "the clock makes both rows candidates: the policy is what decides \
         between them, and it decides inside the chain that deletes"
    );

    f.sync().await;

    assert_eq!(
        std::fs::read(&authorized).expect("read the released file"),
        blob,
        "byte for byte the blob git committed"
    );
    assert_ne!(inode(&authorized), before, "a rename, not a truncation");
    assert_eq!(
        std::fs::read(f.path("keep/manual.mp4")).expect("read"),
        f.content("keep/manual.mp4"),
        "and the path nothing authorized keeps its content: the mode gate lifted \
         for a path, not for the folder"
    );
    assert_eq!(
        ledger_paths(&f.platform),
        vec!["keep/manual.mp4".to_owned()],
        "only the released row is retracted"
    );

    stamp_index_forward(&f.root);
    assert_eq!(
        porcelain_of(&f.root, "zone/clip.mp4"),
        "",
        "and the real git binary calls the released path clean — the one-path \
         re-stat happened, in a mode this had never run in. Scoped to that path: \
         see `porcelain_of` for what the fixture does to the other one's stat"
    );

    // The reason the refusal existed, now settled: the arrival path reads the
    // same policy, so the next pass leaves the pointer where the sweep put it
    // instead of publishing the object back over it.
    f.skip_the_look_window();
    f.sync().await;
    assert_eq!(
        std::fs::read(&authorized).expect("read"),
        blob,
        "the arrival sweep and the release sweep agree about this path"
    );
}

/// ...and with no policy at all, the same folder releases nothing.
///
/// The other half of the gate, and the one that keeps every folder that never
/// heard of `virtualPatterns` behaving exactly as it did: the mode gate refuses
/// the folder before a clock is read, so a default folder with a TTL and no
/// policy is not silently put on one by this story.
#[tokio::test]
async fn a_default_mode_folder_with_no_policy_releases_nothing() {
    let Some(f) = fixture(&["zone/clip.mp4"], |p| {
        p.lfs_mode = LfsMode::Materialize;
    })
    .await
    else {
        return;
    };
    f.open_the_release_window().await;
    f.arrived("zone/clip.mp4", f.platform.now_ms());

    f.platform.advance_ms(TTL_MS as i64);
    assert_eq!(
        candidates(&f.platform, f.platform.now_ms()),
        vec!["zone/clip.mp4".to_owned()],
        "the clock says yes, so what keeps the content is the gate and nothing else"
    );

    f.sync().await;

    assert_eq!(
        std::fs::read(f.path("zone/clip.mp4")).expect("read"),
        f.content("zone/clip.mp4"),
        "nothing authorized this content to stay away, so it stays here"
    );
    assert_eq!(ledger_paths(&f.platform), vec!["zone/clip.mp4".to_owned()]);
}

/// One pass releases the authorized path even when a whole budget of
/// unauthorized candidates sorts ahead of it (Story 56.10).
///
/// This is the shape a default-mode folder actually has: the `materialized`
/// ledger holds a row for every path this clone ever hydrated, while the policy
/// names one narrow zone. The clock makes all of them candidates, the window
/// takes [`RELEASE_BUDGET_OBJECTS`] attempts in `ORDER BY path` cursor order, and
/// every attempt costs a repository open and an index read before the per-path
/// gate can refuse it — so with the policy asked only inside `release_resolved`
/// the pass spends its whole budget in `aaa/` and never reaches `zone/`.
/// `RELEASE_BUDGET_BYTES` makes it worse rather than better: the accumulator
/// counts refused attempts too, so a few large unauthorized rows can end a pass
/// outright.
///
/// The fixture is sized **from** the constant, so raising the budget cannot
/// quietly turn "more unauthorized candidates than a pass allows" into "fewer".
/// The claim is a release in ONE pass, which is what makes it about the
/// pre-filter rather than about the cursor eventually rotating around.
#[tokio::test]
async fn one_pass_releases_the_authorized_path_past_a_budget_of_refusals() {
    // Every one of these sorts before `zone/`, so they are what the window
    // takes first.
    let crowd: Vec<String> = (0..=RELEASE_BUDGET_OBJECTS)
        .map(|n| format!("aaa/{n:03}.mp4"))
        .collect();
    let mut names: Vec<&str> = crowd.iter().map(String::as_str).collect();
    names.push("zone/clip.mp4");

    let Some(f) = fixture(&names, |p| {
        p.lfs_mode = LfsMode::Materialize;
    })
    .await
    else {
        return;
    };
    std::fs::write(
        f.root
            .join(keeper_sync::lfs::virtual_policy::VIRTUAL_PATTERN_FILE),
        "zone/**\n",
    )
    .expect("commit the repository's own answer");

    f.open_the_release_window().await;

    let landed = f.platform.now_ms();
    for name in &names {
        f.arrived(name, landed);
    }

    let blob = committed_blob(&f.root, "zone/clip.mp4");
    f.platform.advance_ms(TTL_MS as i64);
    assert!(
        candidates(&f.platform, f.platform.now_ms()).len() > RELEASE_BUDGET_OBJECTS,
        "the clock alone makes more candidates than one pass may attempt, which \
         is the whole premise"
    );

    f.sync().await;

    assert_eq!(
        std::fs::read(f.path("zone/clip.mp4")).expect("read"),
        blob,
        "the one authorized path is released in the first pass: the budget is \
         spent on rows the folder can actually release, not on being refused"
    );
    for name in &crowd {
        assert_eq!(
            std::fs::read(f.path(name)).expect("read"),
            f.content(name),
            "{name}: and nothing unauthorized was touched"
        );
    }
}

/// A ledger row that recorded no size draws the clock the sweep will honour, not
/// the folder's "kept" (Story 56.10).
///
/// Every `materialized` row written before Story 56.5 has `size_bytes` `NULL`,
/// because `db::note_arrival` is the first writer of that column. Read as zero,
/// such a row falls under any `virtualOverBytes` floor and the surface says *this
/// folder is set to keep large-file content* — while the sweep, which asks the
/// committed pointer for the honest size, releases that very path. Of the two
/// ways to be wrong here only one is safe: promising a release that is refused
/// disappoints, promising the content is kept and then losing it from the
/// worktree surprises. So an absent size means unknown, and clears the floor.
#[tokio::test]
async fn a_row_with_no_recorded_size_is_shown_the_clock_the_sweep_will_use() {
    let Some(f) = fixture(&["zone/clip.mp4"], |p| {
        p.lfs_mode = LfsMode::Materialize;
        // A floor above nothing and below the fixture's own content, so a row
        // read at zero would be under it and the committed pointer is over it.
        p.virtual_over_bytes = CONTENT_BYTES / 2;
    })
    .await
    else {
        return;
    };
    std::fs::write(
        f.root
            .join(keeper_sync::lfs::virtual_policy::VIRTUAL_PATTERN_FILE),
        "zone/**\n",
    )
    .expect("commit the repository's own answer");
    f.open_the_release_window().await;

    // A row exactly as a keeper that had never heard of Story 56.5 wrote it:
    // the materialization clock and nothing else.
    let landed = f.platform.now_ms();
    db::remember_materialized(
        &ledger_conn(&f.platform),
        PROFILE_ID,
        "zone/clip.mp4",
        landed,
    )
    .expect("record the materialization");
    assert!(
        ledger_rows(&f.platform)
            .iter()
            .all(|row| row.size_bytes.is_none()),
        "the fixture's whole premise: no size was recorded"
    );

    let schedules = f
        .engine
        .release_schedules(PROFILE_ID)
        .expect("the profile is registered");
    assert_eq!(
        schedules.get("zone/clip.mp4"),
        Some(&ReleaseSchedule::Due {
            at_ms: landed + TTL_MS as i64
        }),
        "an absent size is unknown, not zero: the row is shown the clock, because \
         that is the clock the sweep is about to honour"
    );

    // And it really is: the same folder, one TTL later, releases it.
    f.platform.advance_ms(TTL_MS as i64);
    f.sync().await;
    assert_eq!(
        std::fs::read(f.path("zone/clip.mp4")).expect("read"),
        committed_blob(&f.root, "zone/clip.mp4"),
        "which is the half that makes the surface's answer true rather than \
         merely optimistic"
    );
}
