//! What a pull does with a path the virtualization policy authorizes to stay
//! away (Story 56.10, FR-328, FR-330, FR-331, AD-122, AD-123).
//!
//! `tests/virtual_policy.rs` settles what `VirtualPolicy` *answers*. This file
//! settles the thing that makes the answer matter: that `Engine`'s arrival path
//! **acts** on it. Before this story the policy had exactly one production
//! consumer — `Engine::verify`, which only excuses a pointer whose object is
//! missing — so a `virtualPatterns` list could not keep one single path virtual.
//! The only lever was `LfsMode::PointerOnly`, which is profile-wide, so the
//! choice on offer was "everything" or "nothing" and FR-328's per-path ask did
//! not exist.
//!
//! Three claims, and each is asserted in the only place it is falsifiable:
//!
//! 1. **The worktree keeps the committed pointer.** Not "the file is small" —
//!    byte-for-byte against the blob the index holds, and the path reads clean
//!    to the real `git` binary afterwards (FR-331). A test that only read the
//!    file back would pass over an index whose cached stat calls the path
//!    modified forever, which is DW-140's shape. The status question is asked
//!    **of that path** — see [`porcelain_of`] for the two fixture facts that
//!    make a whole-tree answer say nothing.
//! 2. **The object is not downloaded onto this clone.** This is the half of the
//!    ask a person can see, and it is asserted against **this machine's LFS
//!    store**, not against the journal: a queued unit is drained inside the same
//!    `sync_once`, so a journal that is empty afterwards is empty in both the
//!    fixed and the broken case. `lfs_prune_local` is switched **off** in the
//!    fixture for the same reason — otherwise "the store does not hold it" could
//!    be true because the object was fetched and then pruned.
//! 3. **The decision is per path.** Every test seeds two tracked paths and
//!    authorizes one, so a global off-switch fails as loudly as a missing check:
//!    the unauthorized path materializes in the very same pass.
//!
//! **A real repository, the real `git` binary, real bytes, and `Engine::sync_once`
//! as the invocation point.** The arrival path is not reachable any other way —
//! `materialize_pending` is private, and the epic's own standard is that a story
//! asserting its central claim through a pure function while the risk lives in
//! the impure shell comes back `incorrect`.
//!
//! **No `filter=lfs` rule is registered**, the decision `tests/dehydrate_entry.rs`
//! documents: `git status` has to decide from the index stat, so a broken
//! refresh surfaces here as ` M` rather than being hidden inside a clean filter.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use keeper_sync::db;
use keeper_sync::engine::Engine;
use keeper_sync::error::SyncError;
use keeper_sync::git;
use keeper_sync::lfs::hydrate::KeepFor;
use keeper_sync::lfs::pointer::Pointer;
use keeper_sync::lfs::store::LfsStore;
use keeper_sync::lfs::virtual_policy::VIRTUAL_PATTERN_FILE;
use keeper_sync::platform::{SyncPlatform, TestPlatform};
use keeper_sync::profile::{LfsMode, SyncProfile};
use keeper_sync::provenance::{Provenance, SyncSource};

/// What each pointer stands for. Deliberately larger than
/// [`FLOOR_BELOW_THE_SIZE`] and far larger than the ~130 bytes of pointer text
/// standing in the worktree, because the gap between those two numbers is what
/// the size-floor test is about.
const CONTENT_BYTES: u64 = 4096;

const PROFILE_ID: &str = "01JARRIVE";

/// The path the policy authorizes to stay away.
const VIRTUAL: &str = "40-media/clip.mp4";

/// A second path in the same authorized zone, which no test protects.
///
/// It exists so the protection test can fail. With only [`VIRTUAL`] and
/// [`KEPT`], a fixture whose `!` line protects `VIRTUAL` has nothing left
/// inside the zone: the assertion "the protected path materialized" is then
/// equally satisfied by a policy that was never consulted at all, so the guard
/// would not guard. This path keeps its pointer in that same pass, which is the
/// assertion that makes the zone demonstrably live.
const ALSO_VIRTUAL: &str = "40-media/take-2.mp4";

/// Its neighbour, tracked the same way and authorized by nothing. Every test
/// asserts on this one too: it is what tells "the policy decided this path"
/// apart from "the arrival path stopped working".
const KEPT: &str = "docs/manual.mp4";

/// The zone the committed pattern file names.
const POLICY: &str = "40-media/**\n";

/// A floor below the pointer's declared size and **above** the length of the
/// pointer text sitting in the worktree. Resolving at the stat instead of at the
/// pointer therefore flips the answer, which is the whole point of the test that
/// uses it.
const FLOOR_BELOW_THE_SIZE: u64 = 1024;

/// A floor above the pointer's declared size: the path materializes, because a
/// floor is a floor.
const FLOOR_ABOVE_THE_SIZE: u64 = 8192;

/// `false` when this machine has no usable `git`, in which case there is no
/// index to read and nothing here is falsifiable — the skip
/// `tests/release_sweep.rs` makes, for its reason.
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
    std::process::Command::new("git")
        .args(["remote", "add", "origin"])
        .arg(remote)
        .current_dir(root)
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

/// The fixture profile: a **filesystem** remote, so an LFS transfer is real with
/// no server anywhere, and the default [`LfsMode::Materialize`] — the mode every
/// folder starts in, and the one in which a per-path policy is the only thing
/// that can keep anything virtual.
///
/// Two settings are deliberate and load-bearing:
///
/// * `excludes` keeps keeper's **committer** off the seeded names.
///   [`seed`] commits pointer text from outside keeper, and once a pass
///   materializes one of these paths keeper's own scan would meet real content
///   where its `file_state` row remembers a stub and stage it as a local edit.
///   The exclusion stops the scan and nothing else: `materialize_pending` filters
///   on the sparse cone and the policy, never on `excludes`, so every decision
///   under test is the decision production makes.
/// * `lfs_prune_local` is **off**. It deletes the redundant store copy exactly
///   when the worktree holds the real content, so with it on, "this clone's store
///   does not hold the object" would be satisfiable by a fetch followed by a
///   prune — and the story's second claim would assert nothing.
fn profile(root: &Path, remote: &Path) -> SyncProfile {
    let mut p = SyncProfile::new(
        PROFILE_ID,
        "media",
        root,
        remote.to_string_lossy().into_owned(),
    );
    p.lfs_threshold_bytes = 1024;
    p.excludes = vec!["*.mp4".to_owned()];
    p.lfs_prune_local = false;
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

/// What the real `git` binary says about **one** pathspec, which is how every
/// claim here is made.
///
/// Two facts about this fixture, neither of them about the story, would
/// otherwise sit in a whole-tree answer forever:
///
/// * **`?? .keepervirtual`.** `commit_local` stages through
///   `collect_stable_changes`, whose stability gate defers a file written
///   immediately before the sync by one pass — so the policy file the fixture
///   writes is not in the index on the first sync, and nothing here needs it to
///   be: `VirtualPolicy::compile` reads the worktree and never `HEAD`.
/// * **` M docs/manual.mp4`.** The path this fixture lets materialize is written
///   and re-stat'd, and then the sync's final `drain_journal` re-scans inside
///   the same wall second; gix meets that entry as racily-clean, calls it
///   modified and hands back a **zeroed** stat size, which keeper persists.
///   Pre-existing and orthogonal — it is the cost of a materialization landing
///   in the same second as the walk that follows it, it self-corrects on the
///   next pass, and it is recorded as deferred work rather than papered over
///   here.
///
/// Neither touches the virtual path, whose entry stays at the pointer's 129
/// bytes from the checkout onwards — which is precisely the invariant FR-331
/// asks about and precisely what these tests assert.
fn porcelain_of(root: &Path, pathspec: &str) -> String {
    let out = std::process::Command::new("git")
        .args(["status", "--porcelain", "--", pathspec])
        .current_dir(root)
        .output()
        .expect("git status");
    String::from_utf8_lossy(&out.stdout).trim().to_owned()
}

/// Stamp `.git/index` ten seconds into the future, with no keeper code involved.
///
/// git's **racy-clean** rule: an index entry whose mtime is not strictly older
/// than the index file's own is not trusted on its cached stat, so git compares
/// content instead and reports ` M` for the second a write lands in, however
/// correct the refreshed stat is. Moving the index file's timestamp is what makes
/// the assertion decide on the **stat** — and it is a filesystem call rather than
/// `Engine::rescan`, because pressing keeper's repair button would re-stat every
/// entry and the assertion would then pass with the arrival path's own
/// `refresh_index_stat` deleted. `tests/release_sweep.rs` makes the same move for
/// the same reason.
fn stamp_index_forward(root: &Path) {
    let when = std::time::SystemTime::now() + std::time::Duration::from_secs(10);
    std::fs::File::options()
        .write(true)
        .open(root.join(".git/index"))
        .and_then(|index| index.set_modified(when))
        .expect("stamp the index forward");
}

/// Put `names` into the state a **checkout of a virtual repository** really
/// leaves: the pointer committed as the index's blob and standing in the
/// worktree, and the object itself in the *remote's* store and nowhere else.
///
/// The content is never written into the worktree here, which is what separates
/// this fixture from `tests/release_sweep.rs`'s: that one starts from a
/// materialized path and asks what lets it go, this one starts from a pointer and
/// asks what arrives.
fn seed(root: &Path, remote: &Path, names: &[&str]) -> Vec<(Pointer, Vec<u8>)> {
    let store = LfsStore::in_git_dir(remote);
    let mut seeded = Vec::with_capacity(names.len());
    for (index, name) in names.iter().enumerate() {
        // Distinct content per path, so no two share an oid and one path's
        // arrival can never be mistaken for another's.
        let content = vec![index as u8 ^ 0xa5; CONTENT_BYTES as usize];
        let (oid, size) = store
            .insert_streaming(std::io::Cursor::new(content.clone()))
            .expect("seed the remote's own LFS store");
        if let Some(parent) = root.join(name).parent() {
            std::fs::create_dir_all(parent).expect("make room for the pointer");
        }
        std::fs::write(root.join(name), Pointer::new(&oid, size).render())
            .expect("check out the pointer");
        seeded.push((Pointer::new(oid, size), content));
    }

    // Committed while the worktree holds only the pointers, so each index blob
    // IS the pointer — the invariant this whole epic rests on (AD-124).
    let repo = git::repo::open(root, false).expect("open the fixture repository");
    commit(
        &repo,
        remote,
        &git::commit::StagedChange {
            added: names.iter().map(PathBuf::from).collect(),
            ..Default::default()
        },
    );
    seeded
}

/// The committed blob's own bytes, read straight out of the index — never a
/// re-rendering of a parsed pointer, because a non-canonical committed pointer
/// renders to a different blob hash (AD-124).
fn committed_blob(root: &Path, name: &str) -> Vec<u8> {
    let repo = git::repo::open(root, false).expect("open");
    keeper_sync::lfs::stage::indexed_pointer_blob(&repo, Path::new(name))
        .expect("the index records a pointer for this path")
        .1
}

/// One complete fixture: repository, bare remote with a seeded LFS store,
/// engine, and the three seeded paths.
struct Fixture {
    _work: tempfile::TempDir,
    _remote: tempfile::TempDir,
    _data: tempfile::TempDir,
    root: PathBuf,
    /// The bare remote's own directory: both the git repository the push lands
    /// in and the LFS store a fetch reads from.
    remote: PathBuf,
    engine: Engine,
    platform: Arc<TestPlatform>,
    seeded: Vec<(Pointer, Vec<u8>)>,
    names: Vec<String>,
}

/// Build the whole fixture, or `None` on a machine with no usable `git`.
///
/// `policy` is written into the **worktree** copy of `.keepervirtual`, which is
/// the one `VirtualPolicy::compile` reads — it never looks at `HEAD`. It is left
/// untracked, and it stays untracked for at least the first pass: `commit_local`
/// stages through `collect_stable_changes`, whose stability gate defers a file
/// written immediately before the sync. Nothing here needs it committed —
/// `compile` reads the worktree — and the file is real, which is the only thing
/// the policy under test depends on.
fn fixture(policy: Option<&str>, adjust: impl FnOnce(&mut SyncProfile)) -> Option<Fixture> {
    let work = tempfile::tempdir().expect("tempdir");
    let remote = tempfile::tempdir().expect("remote");
    if gix::init_bare(remote.path()).is_err() {
        return None;
    }
    let root = work.path().to_path_buf();
    if !init_repo(&root, remote.path()) {
        return None;
    }
    let names = [VIRTUAL, ALSO_VIRTUAL, KEPT];
    let seeded = seed(&root, remote.path(), &names);
    if let Some(text) = policy {
        std::fs::write(root.join(VIRTUAL_PATTERN_FILE), text).expect("write the policy");
    }

    let data = tempfile::tempdir().expect("data dir");
    let mut p = profile(&root, remote.path());
    adjust(&mut p);
    let platform = Arc::new(TestPlatform::new(data.path()));
    let engine = Engine::open(Arc::clone(&platform) as Arc<dyn SyncPlatform>).ok()?;
    engine.upsert_profile(&p).expect("register the profile");
    Some(Fixture {
        _work: work,
        remote: remote.path().to_path_buf(),
        _remote: remote,
        root,
        _data: data,
        engine,
        platform,
        seeded,
        names: names.iter().map(|name| (*name).to_owned()).collect(),
    })
}

impl Fixture {
    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn index_of(&self, name: &str) -> usize {
        self.names
            .iter()
            .position(|seeded| seeded == name)
            .expect("this fixture seeded that path")
    }

    /// The pointer the index records for `name`: the identity, and the
    /// **declared size** every policy decision must be taken at.
    fn pointer(&self, name: &str) -> &Pointer {
        &self.seeded[self.index_of(name)].0
    }

    fn content(&self, name: &str) -> &[u8] {
        &self.seeded[self.index_of(name)].1
    }

    /// This machine's own object store — the thing "not downloaded on this
    /// clone" is a claim about.
    fn local_store(&self) -> LfsStore {
        LfsStore::in_git_dir(self.root.join(".git"))
    }

    /// Put `name`'s object into **this machine's** store, as a previous sync or a
    /// clean filter would have left it. The arrival path then has no reason to
    /// transfer anything, so what it does next is a pure publish decision.
    fn hold_locally(&self, name: &str) {
        self.local_store()
            .insert_streaming(std::io::Cursor::new(self.content(name).to_vec()))
            .expect("seed this machine's store");
    }

    fn holds_locally(&self, name: &str) -> bool {
        let pointer = self.pointer(name);
        self.local_store().contains(&pointer.oid, pointer.size)
    }

    /// One sync — the arrival edge.
    async fn sync(&self) {
        self.engine
            .sync_once(PROFILE_ID, SyncSource::Manual)
            .await
            .expect("the fixture's remote is a real bare repository, so a sync succeeds");
    }

    fn ledger_paths(&self) -> Vec<String> {
        let conn = db::open(&self.platform.data_dir().expect("data dir")).expect("open sync.db");
        db::materialized_rows(&conn, PROFILE_ID)
            .expect("read the ledger")
            .into_iter()
            .map(|row| row.path)
            .collect()
    }

    /// Every LFS download the journal is holding, as `(label, oid)`.
    fn queued_downloads(&self) -> Vec<(Option<String>, String)> {
        let conn = db::open(&self.platform.data_dir().expect("data dir")).expect("open sync.db");
        db::queued_downloads(&conn, PROFILE_ID)
            .expect("read the journal")
            .into_iter()
            .map(|(label, oid, _size)| (label, oid))
            .collect()
    }

    /// Whether `name`'s worktree file is still the committed pointer, byte for
    /// byte.
    fn still_a_pointer(&self, name: &str) -> bool {
        std::fs::read(self.path(name)).expect("read") == committed_blob(&self.root, name)
    }

    fn rewrite_policy(&self, text: Option<&str>) {
        let path = self.root.join(VIRTUAL_PATTERN_FILE);
        match text {
            Some(text) => std::fs::write(&path, text).expect("rewrite the policy"),
            None => std::fs::remove_file(&path).expect("remove the policy"),
        }
    }

    /// The commit the bare remote's branch points at — the fact a push moves,
    /// read with the real `git` binary out of the remote's own directory.
    fn remote_tip(&self) -> String {
        let out = std::process::Command::new("git")
            .arg("--git-dir")
            .arg(&self.remote)
            .args(["rev-parse", "--verify", "refs/heads/main"])
            .output()
            .expect("git rev-parse");
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }
}

/// A path the policy authorizes keeps its pointer **even though the object is
/// already here**, and its neighbour materializes in the same pass.
///
/// This is FR-328's arrival half, and it did not exist: `materialize_pending`
/// branched only on `profile.lfs_mode == LfsMode::PointerOnly`, so in the mode
/// every folder starts in the store's copy was published over the pointer
/// regardless of what any policy said.
///
/// The store is pre-loaded on purpose. It removes the transfer from the question
/// entirely: nothing has to be fetched, so the only thing that can leave pointer
/// text standing is a publish decision — and the neighbour publishing from the
/// same store in the same pass is what proves the decision is per path.
#[tokio::test]
async fn a_virtual_path_keeps_its_pointer_though_the_object_is_already_here() {
    let Some(f) = fixture(Some(POLICY), |_| {}) else {
        return;
    };
    f.hold_locally(VIRTUAL);
    f.hold_locally(ALSO_VIRTUAL);
    f.hold_locally(KEPT);
    let blob = committed_blob(&f.root, VIRTUAL);

    f.sync().await;

    assert_eq!(
        std::fs::read(f.path(VIRTUAL)).expect("read"),
        blob,
        "the authorized path must still hold the committed pointer, byte for byte"
    );
    assert!(
        f.still_a_pointer(ALSO_VIRTUAL),
        "and so must the second path in the same zone: the decision is per path, \
         not per fixture"
    );
    assert_eq!(
        std::fs::read(f.path(KEPT)).expect("read"),
        f.content(KEPT),
        "and the path nothing authorized must have materialized in the same pass, \
         or this test would pass with the whole arrival path broken"
    );
    assert_eq!(
        f.ledger_paths(),
        vec![KEPT.to_owned()],
        "the ledger must claim only the content this machine actually holds"
    );
    stamp_index_forward(&f.root);
    assert_eq!(
        porcelain_of(&f.root, VIRTUAL),
        "",
        "and a virtual path reads clean to the real git binary (FR-331)"
    );
}

/// A path the policy authorizes has its object **left on the server**.
///
/// The owner's first ask, and the half a person can see. Asserted against this
/// machine's own store rather than against the journal: a unit queued by the
/// arrival path is drained inside the same `sync_once`, so an empty journal
/// afterwards is equally true of the broken code. `lfs_prune_local` is off in the
/// fixture so an absent object cannot mean "fetched, then pruned".
///
/// The neighbour's object arriving in the same pass is what makes this
/// falsifiable: it proves the transfer leg ran, reached the remote store and
/// could have taken this object too.
#[tokio::test]
async fn a_virtual_paths_object_is_never_fetched_onto_this_clone() {
    let Some(f) = fixture(Some(POLICY), |_| {}) else {
        return;
    };
    assert!(
        !f.holds_locally(VIRTUAL) && !f.holds_locally(KEPT),
        "the fixture starts with both objects on the remote and neither here"
    );

    f.sync().await;

    assert!(
        !f.holds_locally(VIRTUAL),
        "the authorized path's object must not be downloaded onto this clone: \
         `virtualPatterns` is what keeps the bytes off this machine, not just off \
         the worktree"
    );
    assert!(
        f.holds_locally(KEPT),
        "and the unauthorized path's object must have been fetched in the same \
         pass — otherwise this test passes on a transfer leg that never ran"
    );
    assert!(
        f.still_a_pointer(VIRTUAL),
        "the worktree keeps the committed pointer too"
    );
    assert!(
        !f.holds_locally(ALSO_VIRTUAL),
        "and neither must the second path in the same zone"
    );
    let oid = &f.pointer(VIRTUAL).oid;
    assert!(
        f.queued_downloads()
            .iter()
            .all(|(_label, queued)| queued != oid),
        "and no transfer is left planned for it either"
    );
    stamp_index_forward(&f.root);
    assert_eq!(porcelain_of(&f.root, VIRTUAL), "");
}

/// Editing the policy to authorize materialization brings the content back on
/// the next pull, and neither edit deleted a byte (FR-330, AD-123).
///
/// The reversibility half of AD-123. A policy edit changes what future arrivals
/// materialize; it is never allowed to be a deletion, in either direction.
#[tokio::test]
async fn a_policy_edit_is_reversible_and_deletes_nothing() {
    let Some(f) = fixture(Some(POLICY), |_| {}) else {
        return;
    };
    f.hold_locally(VIRTUAL);

    f.sync().await;
    assert!(
        f.still_a_pointer(VIRTUAL),
        "virtual to begin with, which is the state the edit acts on"
    );
    assert!(
        f.holds_locally(VIRTUAL),
        "and the object is here, untouched: authorizing a path to stay away is \
         not a deletion"
    );

    f.rewrite_policy(None);
    f.sync().await;

    assert_eq!(
        std::fs::read(f.path(VIRTUAL)).expect("read"),
        f.content(VIRTUAL),
        "with nothing authorizing it, the next pull materializes it"
    );
    assert_eq!(
        f.ledger_paths(),
        vec![VIRTUAL.to_owned(), ALSO_VIRTUAL.to_owned(), KEPT.to_owned()],
        "and the ledger now says this machine holds that content too — the whole \
         zone materialized, and the neighbour's row has been there since the \
         first pass"
    );
    // No status assertion for the newly materialized path, and the reason is
    // recorded rather than left as a gap: the sync's final `drain_journal`
    // re-scans inside the same wall second, so the entry `materialize_pending`
    // just re-stat'd comes back racily-clean and keeper persists a zeroed stat
    // for it. Pre-existing, orthogonal to the policy, and deferred — see
    // `porcelain_of`. What this test is about is the two directions of the
    // policy edit, and both are asserted on the bytes.
}

/// The size floor is compared against the **pointer's declared size**, never
/// against the worktree stat.
///
/// A virtual path's worktree file is ~130 bytes of pointer text. Every floor a
/// person would ever configure is larger than that, so a decision taken on the
/// stat answers `Materialize` for every single path the floor exists to keep
/// away — the feature would look configured and do nothing. The pointer is the
/// only place the true size is known before the content exists, which is why
/// AD-122 requires every policy term to be answerable from it.
#[tokio::test]
async fn the_size_floor_is_the_pointers_own_size_not_the_worktree_stat() {
    // Below the declared size, above the pointer text: stays virtual.
    let Some(f) = fixture(Some(POLICY), |p| {
        p.virtual_over_bytes = FLOOR_BELOW_THE_SIZE;
    }) else {
        return;
    };
    f.hold_locally(VIRTUAL);
    let stat_before = std::fs::metadata(f.path(VIRTUAL)).expect("stat").len();
    assert!(
        stat_before < FLOOR_BELOW_THE_SIZE && FLOOR_BELOW_THE_SIZE <= f.pointer(VIRTUAL).size,
        "the fixture's whole claim: the floor sits between the {stat_before}-byte \
         pointer text and the {} bytes it stands for",
        f.pointer(VIRTUAL).size
    );

    f.sync().await;

    assert!(
        f.still_a_pointer(VIRTUAL),
        "the floor is met by the pointer's declared size, so the path stays \
         virtual; comparing the worktree stat would have materialized it"
    );

    // Above the declared size: a floor is a floor.
    let Some(g) = fixture(Some(POLICY), |p| {
        p.virtual_over_bytes = FLOOR_ABOVE_THE_SIZE;
    }) else {
        return;
    };
    g.hold_locally(VIRTUAL);

    g.sync().await;

    assert_eq!(
        std::fs::read(g.path(VIRTUAL)).expect("read"),
        g.content(VIRTUAL),
        "a path under the floor is not authorized to stay away at all"
    );
}

/// A `!` line keeps the content whatever the pattern says — and the zone it is
/// carved out of is still live.
///
/// `VirtualPolicy::resolve` already gives a protection unconditional precedence,
/// unlike gitignore's last-match-wins, because the decision is about somebody's
/// bytes. This asserts the arrival path inherits that rather than re-deciding it.
///
/// The second assertion is what makes the first one mean anything. "The
/// protected path holds its content" is equally true of a policy that was never
/// consulted at all, so with the whole arrival consult deleted this test would
/// still pass. [`ALSO_VIRTUAL`] sits in the same authorized zone with no `!`
/// line, and it keeps its pointer in the same pass — which is only possible if
/// the zone is in force and the protection is what carved this one path out of
/// it.
#[tokio::test]
async fn a_protected_path_materializes_though_the_zone_is_authorized() {
    let Some(f) = fixture(Some("40-media/**\n!40-media/clip.mp4\n"), |_| {}) else {
        return;
    };
    f.hold_locally(VIRTUAL);
    f.hold_locally(ALSO_VIRTUAL);

    f.sync().await;

    assert_eq!(
        std::fs::read(f.path(VIRTUAL)).expect("read"),
        f.content(VIRTUAL),
        "a protection wins over a matching pattern"
    );
    assert!(
        f.still_a_pointer(ALSO_VIRTUAL),
        "and the zone the protection was carved out of is live, which is what \
         makes the assertion above about the protection rather than about \
         nothing being consulted"
    );
}

/// A policy that does not compile refuses the arrival and changes nothing
/// (FR-329).
///
/// The one answer unavailable here is the permissive one. Reading an unparseable
/// policy as "nothing is virtual" would materialize exactly the gigabytes the
/// policy exists to keep away, on whatever link the machine is on, and there is
/// no undoing the bandwidth. So the arrival path propagates
/// `VirtualPolicy::compile`'s `SyncError::Config` with the line quoted as typed —
/// an explicit sync reports it, and the supervisor's own call already logs it as
/// one `warn!` and carries on. Nothing is fetched, nothing is published.
#[tokio::test]
async fn a_policy_that_does_not_compile_refuses_the_arrival() {
    let Some(f) = fixture(Some("40-media/**\n[unclosed\n"), |_| {}) else {
        return;
    };
    f.hold_locally(VIRTUAL);
    f.hold_locally(KEPT);

    let err = f
        .engine
        .sync_once(PROFILE_ID, SyncSource::Manual)
        .await
        .expect_err("a policy keeper cannot read is not a policy");
    let text = format!("{err}");
    assert!(
        matches!(err, SyncError::Config(_)),
        "it is a configuration fault, not a transfer one: {text}"
    );
    assert!(
        text.contains("[unclosed") && text.contains(VIRTUAL_PATTERN_FILE),
        "and it quotes the line as typed and names the file to edit, got: {text}"
    );
    assert!(
        f.still_a_pointer(VIRTUAL) && f.still_a_pointer(KEPT),
        "no path was materialized on the way to refusing"
    );
    assert!(f.ledger_paths().is_empty(), "and the ledger claims nothing");
}

/// `LfsMode` keeps its meaning: the policy is a lever beside it, never a
/// reinterpretation of it.
///
/// Under `PointerOnly` **everything** stays a pointer, including a path no
/// pattern names — so a policy cannot be read as "materialize whatever I did not
/// list". Under `Disabled` the arrival path returns before it looks at anything,
/// so an authorizing policy changes nothing there either.
#[tokio::test]
async fn the_profile_wide_modes_are_unchanged_by_a_policy() {
    let Some(f) = fixture(Some(POLICY), |p| p.lfs_mode = LfsMode::PointerOnly) else {
        return;
    };
    f.hold_locally(VIRTUAL);
    f.hold_locally(KEPT);

    f.sync().await;

    assert!(
        f.still_a_pointer(VIRTUAL) && f.still_a_pointer(KEPT),
        "`PointerOnly` is the whole-profile lever: an unlisted path stays a \
         pointer too"
    );
    assert!(
        f.ledger_paths().is_empty(),
        "and nothing claims to hold content"
    );

    let Some(g) = fixture(Some(POLICY), |p| p.lfs_mode = LfsMode::Disabled) else {
        return;
    };
    g.hold_locally(VIRTUAL);
    g.hold_locally(KEPT);

    g.sync().await;

    assert!(
        g.still_a_pointer(VIRTUAL) && g.still_a_pointer(KEPT),
        "a folder with large-file support off replaces no pointers at all"
    );
    assert!(g.ledger_paths().is_empty());
}

/// A transfer queued **before** the policy authorized the path does not publish
/// when it lands (Story 56.10).
///
/// `materialize_pending` skipping a virtual path is not on its own enough,
/// because it is not the only arm that publishes an object over a checked-out
/// pointer: `materialize_landed` is, and it is reached from the journal. A unit
/// queued on an earlier pass is still there when the owner adds the pattern that
/// was meant to stop exactly those bytes — `.keepervirtual` is committed, so a
/// *pull* can install it — and every unit queued before this story shipped
/// delivers under whatever policy is configured afterwards.
///
/// Nothing would reverse it reliably either: the release sweep needs a TTL, a
/// window, remote proof and a platform that can say the file is closed — and
/// on macOS and Windows that last one is still `Unknown`, which refuses
/// (AD-125). So the delivery arm has to ask.
///
/// The unit is written straight into the journal, which is exactly the state
/// under test — a row that outlived the policy that queued it — and it makes the
/// claim about `materialize_landed` rather than about how the row got there.
#[tokio::test]
async fn a_transfer_queued_before_the_policy_does_not_publish_when_it_lands() {
    let Some(f) = fixture(Some(POLICY), |_| {}) else {
        return;
    };
    let pointer = f.pointer(VIRTUAL).clone();
    {
        let conn = db::open(&f.platform.data_dir().expect("data dir")).expect("open sync.db");
        let unit = db::WorkKind::LfsDownload {
            oid: pointer.oid.clone(),
            size: pointer.size,
        };
        let id = db::enqueue_unique(&conn, PROFILE_ID, &unit, 0, 0).expect("queue the transfer");
        db::label_unit(&conn, id, VIRTUAL).expect("name the path it was queued for");
    }

    f.sync().await;

    assert!(
        f.still_a_pointer(VIRTUAL),
        "the delivery arm must consult the same policy the arrival sweep does, or \
         the pattern the owner just added is honoured by one arm and ignored by \
         the other in the same pass"
    );
    assert!(
        f.ledger_paths().iter().all(|path| path != VIRTUAL),
        "and nothing claims this machine holds that content"
    );
}

/// A path somebody **asked** for materializes, and its co-located siblings do
/// not (Story 56.10, AD-128, FR-330).
///
/// The request door outranks the policy: hydration is an explicit verb and a
/// policy edit has to be reversible, so a person who asks for a virtual file
/// gets it. But a request is for *content*, and `materialize_landed` publishes to
/// every indexed path carrying the delivered oid — ordinary for duplicated
/// content, by that function's own doc. Those siblings are not what anybody
/// asked for, and one of them being in the authorized zone is the case that
/// needs no policy edit at all to reach.
///
/// So the fixture commits a second path holding `KEPT`'s pointer **inside** the
/// authorized zone, asks for `KEPT`, and asserts both halves in one pass.
#[tokio::test]
async fn a_request_publishes_what_was_asked_for_and_not_its_virtual_siblings() {
    const ALIAS: &str = "40-media/manual-copy.mp4";

    let Some(f) = fixture(Some(POLICY), |_| {}) else {
        return;
    };
    // The same committed pointer at a second path, so one object belongs to two
    // paths — which is what makes the fan-out reachable.
    let pointer = f.pointer(KEPT).clone();
    std::fs::write(f.path(ALIAS), pointer.render()).expect("check out the alias pointer");
    {
        let repo = git::repo::open(&f.root, false).expect("open");
        commit(
            &repo,
            &f.remote,
            &git::commit::StagedChange {
                added: vec![PathBuf::from(ALIAS)],
                ..Default::default()
            },
        );
    }

    f.engine
        .materialize_entry(PROFILE_ID, KEPT, KeepFor::Unspecified)
        .expect("the path is tracked and the remote holds the object");
    f.sync().await;

    assert_eq!(
        std::fs::read(f.path(KEPT)).expect("read"),
        f.content(KEPT),
        "what was asked for arrives"
    );
    assert_eq!(
        std::fs::read(f.path(ALIAS)).expect("read"),
        pointer.render().into_bytes(),
        "and the sibling in the authorized zone keeps its pointer: a request is \
         authorization for the path somebody named, not for every path that \
         happens to share its bytes"
    );
}

/// Pointer text standing for the **empty object** materializes, whatever the
/// policy says (Story 56.10).
///
/// `virtualOverBytes` defaults to `0` and the floor is inclusive, so `0 < 0` is
/// false and a `size 0` pointer inside an authorized zone would otherwise be
/// answered `Virtual`. It would then be skipped by the arrival sweep on every
/// pass, forever, while the release side refuses it as `AlreadyPointer` — an
/// empty tracked file reading as ~90 bytes of pointer text to every application
/// that opens it, with no bytes anywhere to justify the substitution. There is
/// no content for a policy to keep away, so it is carved out on both sides.
///
/// **The fixture writes the text by hand, and that is the whole reachability
/// argument.** `Pointer::render` deliberately encodes the empty pointer as
/// nothing at all — upstream's `Pointer.Encoded()`, so an empty file stays an
/// empty file — so keeper's own checkout can never leave a `size 0` pointer
/// standing, and a fixture built through `render()` would assert that an
/// already-empty file is empty. What reaches this branch is text some other
/// tool or hand wrote, which `Pointer::parse` accepts: exactly the input
/// `release_resolved` carves out for, rather than leaving it to the accident
/// that `lfs::audit::tracked_objects` skips size-0 objects.
#[tokio::test]
async fn the_empty_object_is_not_something_a_policy_can_keep_away() {
    const EMPTY: &str = "40-media/silence.mp4";

    let Some(f) = fixture(Some(POLICY), |_| {}) else {
        return;
    };
    let store = f.local_store();
    let (oid, size) = store
        .insert_streaming(std::io::Cursor::new(Vec::new()))
        .expect("the empty object");
    assert_eq!(size, 0);
    let text = format!("version https://git-lfs.github.com/spec/v1\noid sha256:{oid}\nsize 0\n");
    std::fs::write(f.path(EMPTY), &text).expect("check out the pointer");
    assert_eq!(
        keeper_sync::lfs::stage::pending_smudges(&f.root, &[PathBuf::from(EMPTY)])
            .expect("read")
            .into_iter()
            .map(|smudge| smudge.pointer.size)
            .collect::<Vec<_>>(),
        vec![0],
        "the fixture's premise: this really is pointer text declaring size 0, \
         which is what `Pointer::render` will not produce and another tool can"
    );
    {
        let repo = git::repo::open(&f.root, false).expect("open");
        commit(
            &repo,
            &f.remote,
            &git::commit::StagedChange {
                added: vec![PathBuf::from(EMPTY)],
                ..Default::default()
            },
        );
    }

    f.sync().await;

    assert_eq!(
        std::fs::read(f.path(EMPTY)).expect("read"),
        Vec::<u8>::new(),
        "an empty tracked file is an empty file on disk, not a stub standing for \
         bytes that do not exist"
    );
}

/// A policy that does not compile must not cost the person their push (Story
/// 56.10, FR-329).
///
/// `.keepervirtual` is committed, so one typo'd glob is shared by every clone.
/// Raising it where the arrival leg runs made every explicit sync on every clone
/// commit locally, merge the remote in, and then stop — the uploads that commit
/// had just queued never drained and the branch was never pushed, until somebody
/// edited the file. FR-329 asks the arrival to refuse loudly; it does not ask the
/// user's own commits to be held hostage to it.
///
/// Both halves are asserted, because either alone is satisfiable by the wrong
/// implementation: the remote's branch moved (the push ran) **and** the call
/// still failed with the line quoted (the fault was not swallowed).
#[tokio::test]
async fn a_policy_that_does_not_compile_still_lets_the_commits_through() {
    let Some(f) = fixture(Some(POLICY), |_| {}) else {
        return;
    };
    // One clean pass first, so the branch exists on the remote.
    f.sync().await;

    std::fs::write(f.root.join("notes.txt"), b"work the owner did\n").expect("write");
    // Observed once, then settled. `commit_local` stages through
    // `collect_stable_changes`, which needs one walk to see the file and a
    // settle window to have elapsed since — and the window is measured on
    // `TestPlatform`'s injected clock, which nothing but `advance_ms` moves. A
    // test that only synced twice would be asserting that the machine is slow.
    f.sync().await;
    f.platform.advance_ms(300_000);

    // Captured after everything else that could move it, so the only commit
    // left to reach the remote is the one the erroring pass makes.
    let before = f.remote_tip();
    f.rewrite_policy(Some("40-media/**\n[unclosed\n"));

    let err = f
        .engine
        .sync_once(PROFILE_ID, SyncSource::Manual)
        .await
        .expect_err("the arrival leg must still refuse, and say which line");
    let text = format!("{err}");
    assert!(
        matches!(err, SyncError::Config(_)) && text.contains("[unclosed"),
        "the fault is reported with the line quoted: {text}"
    );
    assert_ne!(
        f.remote_tip(),
        before,
        "and the commits reached the remote anyway: a typo in a file one clone \
         committed must not strand every other clone's work"
    );
    assert!(
        f.still_a_pointer(VIRTUAL),
        "nothing was materialized on the way to refusing"
    );
}
